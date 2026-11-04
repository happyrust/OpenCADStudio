//! CLI and startup for the collaboration server.
//!
//! Flags follow `architecture.md` appendix A.2: `--port` (3240), `--db`,
//! `--heartbeat-timeout` (90), `--claim-ttl` (300) and an optional `--seed`.
//!
//! Two startup behaviours matter beyond "it binds a port":
//! * a port already in use must fail with a non-zero exit code and say so on
//!   stderr, never silently pick another port (VAL-SRV-026);
//! * a database that cannot be opened does not abort the process — the server
//!   still listens so `GET /health` can report 503 (VAL-SRV-002).
//!
//! The order of those two steps is load-bearing: the port is bound *before* the
//! database is opened. `build_state` clears the ephemeral session and claim
//! rows, so opening first would let a start that is about to fail on a taken
//! port erase the state of the instance already serving on it.

use std::net::SocketAddr;
use std::process::ExitCode;
use std::sync::Arc;

use ocs_collab::SessionConfig;
use ocs_collab_server::http;
use ocs_collab_server::store::Store;
use ocs_collab_server::AppState;

/// Default listen port, matching `services.yaml`.
const DEFAULT_PORT: u16 = 3240;
const DEFAULT_HEARTBEAT_TIMEOUT: i64 = 90;
const DEFAULT_CLAIM_TTL: i64 = 300;

#[derive(Debug, Clone, PartialEq, Eq)]
struct Cli {
    port: u16,
    db: String,
    heartbeat_timeout: i64,
    claim_ttl: i64,
    seed: Option<String>,
}

fn default_db_path() -> String {
    // %APPDATA%\OpenCADStudio\collab.db on Windows; a plain fallback elsewhere
    // keeps the binary usable in tests without an environment lookup.
    match std::env::var("APPDATA") {
        Ok(appdata) if !appdata.is_empty() => {
            format!("{appdata}\\OpenCADStudio\\collab.db")
        }
        _ => "collab.db".to_owned(),
    }
}

fn usage() -> &'static str {
    "ocs_collab_server --port <port> --db <path> \
     [--heartbeat-timeout <seconds>] [--claim-ttl <seconds>] [--seed <json>]"
}

fn parse_args<I: IntoIterator<Item = String>>(args: I) -> Result<Cli, String> {
    let mut cli = Cli {
        port: DEFAULT_PORT,
        db: default_db_path(),
        heartbeat_timeout: DEFAULT_HEARTBEAT_TIMEOUT,
        claim_ttl: DEFAULT_CLAIM_TTL,
        seed: None,
    };

    let mut iter = args.into_iter();
    while let Some(flag) = iter.next() {
        let mut value = |name: &str| -> Result<String, String> {
            iter.next()
                .ok_or_else(|| format!("{name} 需要一个值；用法：{}", usage()))
        };
        match flag.as_str() {
            "--port" => {
                let raw = value("--port")?;
                cli.port = raw
                    .parse()
                    .map_err(|_| format!("--port 需要 1-65535 的整数，收到 {raw}"))?;
            }
            "--db" => cli.db = value("--db")?,
            "--heartbeat-timeout" => {
                let raw = value("--heartbeat-timeout")?;
                cli.heartbeat_timeout = raw.parse().map_err(|_| {
                    format!("--heartbeat-timeout 需要正整数秒数，收到 {raw}")
                })?;
                if cli.heartbeat_timeout <= 0 {
                    return Err("--heartbeat-timeout 必须大于 0".to_owned());
                }
            }
            "--claim-ttl" => {
                let raw = value("--claim-ttl")?;
                cli.claim_ttl = raw
                    .parse()
                    .map_err(|_| format!("--claim-ttl 需要正整数秒数，收到 {raw}"))?;
                if cli.claim_ttl <= 0 {
                    return Err("--claim-ttl 必须大于 0".to_owned());
                }
            }
            "--seed" => cli.seed = Some(value("--seed")?),
            "--help" | "-h" => return Err(usage().to_owned()),
            other => return Err(format!("未知参数 {other}；用法：{}", usage())),
        }
    }

    Ok(cli)
}

fn build_state(cli: &Cli) -> AppState {
    let session_config = SessionConfig {
        heartbeat_timeout: cli.heartbeat_timeout,
        claim_ttl: cli.claim_ttl,
    };

    match Store::open(&cli.db) {
        Ok(store) => {
            // Sessions and claims are ephemeral across restarts (appendix A.2).
            if let Err(err) = store.clear_ephemeral() {
                eprintln!("[ocs_collab_server] 清理旧会话失败：{err}");
            }
            if let Some(seed) = &cli.seed {
                match store.seed_from_file(seed) {
                    Ok(()) => println!("[ocs_collab_server] 已从 {seed} 播种"),
                    Err(err) => eprintln!("[ocs_collab_server] 播种失败：{err}"),
                }
            }
            match store.journal_mode() {
                Ok(mode) => println!("[ocs_collab_server] journal_mode = {mode}"),
                Err(err) => eprintln!("[ocs_collab_server] 读取 journal_mode 失败：{err}"),
            }
            println!("[ocs_collab_server] 数据库 {}", cli.db);
            AppState::new(Some(store), None, cli.db.clone(), session_config)
        }
        Err(err) => {
            eprintln!("[ocs_collab_server] 数据库初始化失败：{err}");
            AppState::new(None, Some(err), cli.db.clone(), session_config)
        }
    }
}

#[tokio::main]
async fn main() -> ExitCode {
    let cli = match parse_args(std::env::args().skip(1)) {
        Ok(cli) => cli,
        Err(message) => {
            eprintln!("{message}");
            return ExitCode::from(2);
        }
    };

    // Bind **before** the database is opened. `build_state` calls
    // `clear_ephemeral`, which drops every session and claim row in the shared
    // database; opening the store first would therefore let a start that is
    // about to fail on a taken port wipe the sessions of the instance that is
    // already serving on it. Ordering is the fix — the cleanup itself stays,
    // because a restarted server must not honour pre-restart `session_id`s.
    let addr = SocketAddr::from(([127, 0, 0, 1], cli.port));
    let listener = match tokio::net::TcpListener::bind(addr).await {
        Ok(listener) => listener,
        Err(err) => {
            // Never fall back to another port: a silent rebind would make the
            // whole verification surface lie about where the server is.
            eprintln!("[ocs_collab_server] 无法监听 {addr}：{err}");
            return ExitCode::FAILURE;
        }
    };

    let state = Arc::new(build_state(&cli));
    let app = http::router(state);

    println!(
        "[ocs_collab_server] v{} 监听 http://{addr}（heartbeat_timeout={}s, claim_ttl={}s）",
        env!("CARGO_PKG_VERSION"),
        cli.heartbeat_timeout,
        cli.claim_ttl
    );

    let shutdown = async {
        let _ = tokio::signal::ctrl_c().await;
        println!("[ocs_collab_server] 收到中断，正在退出");
    };

    if let Err(err) = axum::serve(
        listener,
        app.into_make_service_with_connect_info::<SocketAddr>(),
    )
    .with_graceful_shutdown(shutdown)
    .await
    {
        eprintln!("[ocs_collab_server] 服务异常退出：{err}");
        return ExitCode::FAILURE;
    }

    ExitCode::SUCCESS
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(items: &[&str]) -> Vec<String> {
        items.iter().map(|item| (*item).to_owned()).collect()
    }

    #[test]
    fn defaults_match_the_architecture_numbers() {
        let cli = parse_args(args(&[])).expect("no args is legal");
        assert_eq!(cli.port, 3240);
        assert_eq!(cli.heartbeat_timeout, 90);
        assert_eq!(cli.claim_ttl, 300);
        assert!(cli.seed.is_none());
        assert!(!cli.db.is_empty(), "the default db path is never empty");
    }

    #[test]
    fn port_and_db_are_taken_from_the_command_line() {
        let cli = parse_args(args(&["--port", "3241", "--db", "D:\\scratch\\collab\\dev.db"]))
            .expect("flags parse");
        assert_eq!(cli.port, 3241);
        assert_eq!(cli.db, "D:\\scratch\\collab\\dev.db");
    }

    #[test]
    fn heartbeat_timeout_and_claim_ttl_override_the_defaults() {
        let cli = parse_args(args(&[
            "--heartbeat-timeout",
            "5",
            "--claim-ttl",
            "5",
        ]))
        .expect("flags parse");
        assert_eq!(cli.heartbeat_timeout, 5);
        assert_eq!(cli.claim_ttl, 5);
    }

    #[test]
    fn a_zero_or_negative_timeout_is_rejected() {
        assert!(parse_args(args(&["--heartbeat-timeout", "0"])).is_err());
        assert!(parse_args(args(&["--claim-ttl", "-1"])).is_err());
    }

    #[test]
    fn a_missing_flag_value_is_rejected() {
        let err = parse_args(args(&["--port"])).expect_err("missing value must fail");
        assert!(err.contains("--port"), "{err}");
    }

    #[test]
    fn an_unknown_flag_is_rejected_with_the_usage_line() {
        let err = parse_args(args(&["--bogus"])).expect_err("unknown flag must fail");
        assert!(err.contains("--bogus"), "{err}");
        assert!(err.contains("--heartbeat-timeout"), "{err}");
    }

    #[test]
    fn a_non_numeric_port_is_rejected() {
        assert!(parse_args(args(&["--port", "abc"])).is_err());
    }

    #[test]
    fn seed_is_optional_and_captured_when_present() {
        let cli = parse_args(args(&["--seed", "D:\\scratch\\seed.json"])).expect("flags parse");
        assert_eq!(cli.seed.as_deref(), Some("D:\\scratch\\seed.json"));
    }

    #[test]
    fn the_session_config_reflects_the_flags() {
        let cli = parse_args(args(&["--heartbeat-timeout", "7", "--claim-ttl", "9"])).expect("parse");
        let config = SessionConfig {
            heartbeat_timeout: cli.heartbeat_timeout,
            claim_ttl: cli.claim_ttl,
        };
        assert_eq!(config.heartbeat_timeout, 7);
        assert_eq!(config.claim_ttl, 9);
    }

    #[test]
    fn a_database_path_that_is_a_directory_yields_an_unhealthy_state() {
        let dir = std::env::temp_dir().join("ocs_collab_cli_dir");
        std::fs::create_dir_all(&dir).expect("create dir");
        let cli = Cli {
            port: 3240,
            db: dir.display().to_string(),
            heartbeat_timeout: 90,
            claim_ttl: 300,
            seed: None,
        };
        let state = build_state(&cli);
        assert!(state.store.is_none());
        assert!(state.db_error.is_some());
    }

    #[test]
    fn a_writable_path_yields_a_wal_backed_store() {
        let path = std::env::temp_dir()
            .join("ocs_collab_cli_ok")
            .join(format!("{}.db", uuid::Uuid::new_v4()));
        let cli = Cli {
            port: 3240,
            db: path.display().to_string(),
            heartbeat_timeout: 5,
            claim_ttl: 5,
            seed: None,
        };
        let state = build_state(&cli);
        assert!(state.db_healthy());
        assert_eq!(state.session_config.claim_ttl, 5);
        let store = state.store.clone().expect("store");
        assert_eq!(store.journal_mode().expect("mode"), "wal");
        drop(store);
        let _ = std::fs::remove_file(&path);
        let _ = std::fs::remove_file(format!("{}-wal", path.display()));
        let _ = std::fs::remove_file(format!("{}-shm", path.display()));
    }
}
