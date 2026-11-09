//! Synchronous WebSocket transport for the desktop client.
//!
//! `tungstenite` in blocking mode over a `TcpStream` with an explicit connect
//! timeout, driven from a `std::thread` and reported through an iced futures
//! channel. No async runtime is involved (`VAL-CLI-019`).
//!
//! Five constraints are load-bearing, each of them learned from a hang:
//!
//! 1. `set_read_timeout(100ms)` after connecting — without it `read()` blocks
//!    forever, the stop flag is never observed and `join()` never returns.
//! 2. A read timeout (`WouldBlock` / `TimedOut`) is a normal branch: `continue`.
//! 3. `Message::Close` must `break` immediately — `Close` is delivered once and
//!    the next `read()` reports `ConnectionAborted` instead.
//! 4. A peer killed with no close frame surfaces as
//!    `Io(ConnectionReset)`, not `ConnectionClosed`; every `Err` means the
//!    connection is gone and must go through the retry path (`VAL-CLI-020`).
//! 5. `TcpStream::connect_timeout` + `tungstenite::client` — never
//!    `tungstenite::connect`, which has no timeout (21 s against a black hole).

use super::messages::{classify, Frame};
use super::{ClientConfig, CollabEvent};
use iced::futures::channel::mpsc;
use ocs_collab::ClientMessage;
use std::io::{ErrorKind, Write};
use std::net::{TcpStream, ToSocketAddrs};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};
use tungstenite::{Message, WebSocket};

/// Read timeout once connected. Short enough that a cancelled subscription is
/// noticed promptly, long enough that an idle session costs no CPU.
const READ_TIMEOUT: Duration = Duration::from_millis(100);

/// Connect timeout. A refused connection returns at once on Windows (~2 s);
/// this bounds the black-hole case.
const CONNECT_TIMEOUT: Duration = Duration::from_secs(3);

/// Heartbeat cadence the server expects (see `architecture.md` §3.1).
const HEARTBEAT_INTERVAL: Duration = Duration::from_secs(15);

/// Test-only override for the cadence, so the emission path can be exercised
/// without sitting through a real minute of wall clock. Production always uses
/// [`HEARTBEAT_INTERVAL`].
#[cfg(test)]
static HEARTBEAT_OVERRIDE: Mutex<Option<Duration>> = Mutex::new(None);

/// Cadence actually used by the read loop.
fn heartbeat_interval() -> Duration {
    #[cfg(test)]
    if let Some(interval) = *HEARTBEAT_OVERRIDE
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
    {
        return interval;
    }
    HEARTBEAT_INTERVAL
}

/// Reconnect backoff: 200 ms doubling to a 5 s ceiling, matching the
/// documented retry ladder.
const BACKOFF_START: Duration = Duration::from_millis(200);
const BACKOFF_MAX: Duration = Duration::from_secs(5);

/// How long one wait-for-retry slice sleeps before re-checking whether the
/// subscription is still alive.
const BACKOFF_SLICE: Duration = Duration::from_millis(50);

/// Set by [`shutdown`] when the app is quitting. Unlike a dropped
/// subscription, this asks the thread to say goodbye on the wire: a process
/// that just dies leaves the server waiting out the heartbeat timeout, while a
/// `Close` frame retires the session at once (`VAL-CLI-016`).
static STOP: AtomicBool = AtomicBool::new(false);

/// The transport thread, kept so [`shutdown`] can wait for its goodbye.
static WORKER: Mutex<Option<std::thread::JoinHandle<()>>> = Mutex::new(None);

/// Whether the app is quitting and the transport must close its socket.
pub(super) fn stop_requested() -> bool {
    STOP.load(Ordering::Relaxed)
}

/// Records the thread [`shutdown`] waits on. Called from the subscription
/// worker, which is the only place that spawns it.
pub(super) fn register_worker(handle: std::thread::JoinHandle<()>) {
    if let Ok(mut slot) = WORKER.lock() {
        *slot = Some(handle);
    }
}

/// Asks the transport to close its socket and waits for the thread to finish.
///
/// Returns once the worker is gone or the budget elapses. The budget exists so
/// a wedged socket cannot hang the quit path; the thread's 100 ms read timeout
/// means the normal case returns almost immediately.
pub(super) fn shutdown() {
    eprintln!("[collab] 退出：请求关闭协同连接");
    STOP.store(true, Ordering::Relaxed);
    let deadline = Instant::now() + Duration::from_secs(3);
    loop {
        let finished = WORKER
            .lock()
            .map(|slot| slot.as_ref().is_none_or(|handle| handle.is_finished()))
            .unwrap_or(true);
        if finished || Instant::now() >= deadline {
            break;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    // Reap the handle so the join does not leak on a later restart of the
    // subscription (iced may rebuild it if the window reopens).
    let handle = WORKER.lock().ok().and_then(|mut slot| slot.take());
    if let Some(handle) = handle {
        let _ = handle.join();
    }
}

/// Session config handed to the transport thread before it starts. The worker
/// is spawned by iced with no arguments, so the value travels through here.
fn config_slot() -> &'static Mutex<ClientConfig> {
    static SLOT: OnceLock<Mutex<ClientConfig>> = OnceLock::new();
    SLOT.get_or_init(|| Mutex::new(ClientConfig::default()))
}

/// Publish the session parameters the transport thread should use.
pub(super) fn set_client_config(config: ClientConfig) {
    if let Ok(mut slot) = config_slot().lock() {
        *slot = config;
    }
}

fn current_config() -> ClientConfig {
    config_slot()
        .lock()
        .map(|slot| slot.clone())
        .unwrap_or_default()
}

/// Why one session ended.
enum Ended {
    /// The server closed the conversation cleanly.
    Closed(String),
    /// The app is quitting: the socket was closed on purpose.
    Quit,
    /// The subscription is gone; nothing may reconnect.
    SubscriptionGone,
}

/// Whether a read failure is just the 100 ms timeout elapsing.
pub(super) fn is_read_timeout(error: &tungstenite::Error) -> bool {
    matches!(
        error,
        tungstenite::Error::Io(io) if matches!(io.kind(), ErrorKind::WouldBlock | ErrorKind::TimedOut)
    )
}

/// One-line description of a read failure, including the OS error kind the
/// validation contract looks for (`ConnectionReset`).
pub(super) fn describe_read_error(error: &tungstenite::Error) -> String {
    match error {
        tungstenite::Error::Io(io) => format!("read error: {:?} ({io})", io.kind()),
        tungstenite::Error::ConnectionClosed => "read error: ConnectionClosed".to_owned(),
        other => format!("read error: {other}"),
    }
}

/// Retry delay for the given zero-based attempt.
pub(super) fn backoff_delay(attempt: u32) -> Duration {
    let factor = 1u32.checked_shl(attempt.min(16)).unwrap_or(u32::MAX);
    BACKOFF_START
        .saturating_mul(factor)
        .min(BACKOFF_MAX)
}

/// Keep the GUI informed, and report whether the subscription still exists.
///
/// `TrySendError::Full` means the UI thread is behind: dropping one status
/// event is strictly better than blocking the socket thread.
/// `TrySendError::Disconnected` means iced cancelled the subscription, so the
/// thread must exit or it leaks.
fn emit(sender: &mut mpsc::Sender<CollabEvent>, event: CollabEvent) -> Result<(), ()> {
    match sender.try_send(event) {
        Ok(()) => Ok(()),
        Err(error) if error.is_disconnected() => Err(()),
        Err(_full) => Ok(()),
    }
}

/// Sleep out a retry delay, aborting early when the subscription disappears or
/// the app starts quitting.
fn wait_before_retry(sender: &mpsc::Sender<CollabEvent>, delay: Duration) -> Result<(), ()> {
    let deadline = Instant::now() + delay;
    while Instant::now() < deadline {
        if sender.is_closed() || stop_requested() {
            return Err(());
        }
        std::thread::sleep(BACKOFF_SLICE.min(deadline.saturating_duration_since(Instant::now())));
    }
    Ok(())
}

/// The transport thread body: connect, hold the session, reconnect on failure.
///
/// Returns `Ok(())` once the subscription is cancelled (or the client was never
/// configured); errors are reported through the channel rather than returned,
/// because there is no one left to return them to.
pub(super) fn run_client(sender: &mut mpsc::Sender<CollabEvent>) -> std::io::Result<()> {
    let mut attempt = 0u32;
    loop {
        if stop_requested() || sender.is_closed() {
            return Ok(());
        }
        let config = current_config();
        if config.server_url.trim().is_empty() {
            // The subscription is gated on a configured address, so this is
            // only reachable in the window between the worker starting and the
            // boot path publishing the CLI address. Wait for it instead of
            // exiting: returning here would leave the subscription alive but
            // permanently silent, with no way to ever reconnect.
            if sender.is_closed() {
                return Ok(());
            }
            std::thread::sleep(BACKOFF_SLICE);
            continue;
        }
        if emit(sender, CollabEvent::Connecting { attempt }).is_err() {
            return Ok(());
        }
        match run_session(sender, &config) {
            Ok(Ended::SubscriptionGone) => return Ok(()),
            Ok(Ended::Quit) => return Ok(()),
            Ok(Ended::Closed(reason)) => {
                if emit(
                    sender,
                    CollabEvent::Disconnected {
                        reason,
                        will_retry: true,
                    },
                )
                .is_err()
                {
                    return Ok(());
                }
            }
            Err(error) => {
                if emit(
                    sender,
                    CollabEvent::Disconnected {
                        reason: error,
                        will_retry: true,
                    },
                )
                .is_err()
                {
                    return Ok(());
                }
            }
        }
        if wait_before_retry(sender, backoff_delay(attempt)).is_err() {
            return Ok(());
        }
        attempt = attempt.saturating_add(1);
    }
}

/// One connection: handshake, `hello`, then the read loop until it ends.
fn run_session(
    sender: &mut mpsc::Sender<CollabEvent>,
    config: &ClientConfig,
) -> Result<Ended, String> {
    let mut ws = connect(&config.server_url)?;
    let hello = ClientMessage::Hello {
        user_id: config.user_id.clone(),
        drawing_id: config.drawing_id.clone(),
        mode: config.mode.clone(),
        client_version: config.client_version.clone(),
        workspace: String::new(),
    };
    send_json(&mut ws, &hello)?;

    let mut last_heartbeat = Instant::now();
    let mut session_id: Option<String> = None;

    loop {
        if stop_requested() {
            // The app is quitting: say goodbye on the wire so the server
            // retires the session now instead of after the 90 s timeout.
            close(&mut ws);
            return Ok(Ended::Quit);
        }
        if sender.is_closed() {
            close(&mut ws);
            return Ok(Ended::SubscriptionGone);
        }
        match ws.read() {
            Ok(Message::Text(text)) => match classify(text.as_str()) {
                Frame::Event(event) => {
                    // `welcome` carries the session id every later frame
                    // references, so remember it before forwarding.
                    if let CollabEvent::Connected {
                        session_id: id, ..
                    } = &event
                    {
                        session_id = Some(id.clone());
                    }
                    if emit(sender, event).is_err() {
                        close(&mut ws);
                        return Ok(Ended::SubscriptionGone);
                    }
                }
                Frame::Close => {
                    close(&mut ws);
                    return Ok(Ended::Closed("server closed the session".to_owned()));
                }
                Frame::Ignored => {}
                Frame::Unreadable(error) => {
                    eprintln!("[collab] 无法解析服务器帧：{error}");
                }
            },
            Ok(Message::Binary(_)) => {}
            Ok(Message::Ping(payload)) => {
                // tungstenite queues the pong; flushing pushes it out now
                // rather than on the next write.
                let _ = ws.send(Message::Pong(payload));
            }
            Ok(Message::Pong(_)) => {}
            Ok(Message::Close(_)) => {
                // Constraint 3: `Close` arrives once. Anything after it is
                // `ConnectionAborted`, so leave the loop here.
                close(&mut ws);
                return Ok(Ended::Closed("close frame received".to_owned()));
            }
            Ok(Message::Frame(_)) => {}
            Err(error) if is_read_timeout(&error) => {
                // Constraint 2: the 100 ms timeout is the loop's clock.
            }
            Err(error) => {
                // Constraint 4: everything else — including
                // `Io(ConnectionReset)` from a killed server — means the
                // connection is gone and the retry ladder takes over.
                let reason = describe_read_error(&error);
                close(&mut ws);
                return Ok(Ended::Closed(reason));
            }
        }

        if last_heartbeat.elapsed() >= heartbeat_interval() {
            let Some(session_id) = session_id.clone() else {
                continue;
            };
            let beat = ClientMessage::Heartbeat {
                session_id: session_id.clone(),
            };
            if let Err(error) = send_json(&mut ws, &beat) {
                let reason = format!("heartbeat failed: {error}");
                close(&mut ws);
                return Ok(Ended::Closed(reason));
            }
            last_heartbeat = Instant::now();
        }
    }
}

/// Open the socket with a bounded connect and complete the WS handshake.
fn connect(url: &str) -> Result<WebSocket<TcpStream>, String> {
    let parsed = url::Url::parse(url).map_err(|error| format!("服务器地址无效：{error}"))?;
    let host = parsed
        .host_str()
        .ok_or_else(|| "服务器地址缺少主机名".to_owned())?;
    let port = parsed.port_or_known_default().unwrap_or(80);

    let mut last_error = None;
    let mut stream = None;
    for addr in (host, port)
        .to_socket_addrs()
        .map_err(|error| format!("无法解析 {host}:{port}：{error}"))?
    {
        match TcpStream::connect_timeout(&addr, CONNECT_TIMEOUT) {
            Ok(socket) => {
                stream = Some(socket);
                break;
            }
            Err(error) => last_error = Some(error),
        }
    }
    let stream = stream.ok_or_else(|| {
        format!(
            "连接 {host}:{port} 失败：{}",
            last_error
                .map(|error| error.to_string())
                .unwrap_or_else(|| "没有可用地址".to_owned())
        )
    })?;
    // The read timeout is what makes the loop stoppable at all (constraint 1).
    stream
        .set_read_timeout(Some(READ_TIMEOUT))
        .map_err(|error| format!("设置读超时失败：{error}"))?;
    stream
        .set_write_timeout(Some(Duration::from_secs(5)))
        .map_err(|error| format!("设置写超时失败：{error}"))?;
    let _ = stream.set_nodelay(true);

    // `client`, not `connect`: the socket already has a deadline and a
    // configured timeout, so the handshake inherits both.
    let (ws, _response) =
        tungstenite::client(url, stream).map_err(|error| format!("WebSocket 握手失败：{error}"))?;
    Ok(ws)
}

/// Serialise one client message and flush it.
fn send_json<S>(ws: &mut WebSocket<S>, message: &ClientMessage) -> Result<(), String>
where
    S: std::io::Read + Write,
{
    let text = serde_json::to_string(message).map_err(|error| format!("序列化失败：{error}"))?;
    ws.send(Message::text(text))
        .map_err(|error| format!("发送失败：{error}"))
}

/// Say goodbye politely. `close` only queues the frame; without the flush the
/// peer waits out the TCP timeout instead of seeing the close.
fn close<S>(ws: &mut WebSocket<S>)
where
    S: std::io::Read + Write,
{
    let _ = ws.close(None);
    let _ = ws.flush();
}

#[cfg(test)]
mod tests {
    use super::*;
    use iced::futures::StreamExt;
    use std::net::TcpListener;
    use std::sync::MutexGuard;

    /// The session config lives in a process-global slot, so tests that touch
    /// it must not overlap: two threads would fight over one URL.
    fn config_lock() -> MutexGuard<'static, ()> {
        static LOCK: Mutex<()> = Mutex::new(());
        LOCK.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    /// A one-connection WebSocket server that answers `hello` with `welcome`
    /// and then goes quiet, so the client's read loop idles on its timeout.
    fn quiet_server() -> (String, std::thread::JoinHandle<()>) {
        let listener = TcpListener::bind(("127.0.0.1", 0)).expect("bind loopback");
        let addr = listener.local_addr().expect("local addr");
        let url = format!("ws://{addr}/ws");
        let handle = std::thread::spawn(move || {
            let Ok((stream, _)) = listener.accept() else {
                return;
            };
            let mut ws = match tungstenite::accept(stream) {
                Ok(ws) => ws,
                Err(_) => return,
            };
            let _ = ws.read();
            let welcome = r#"{"type":"welcome","session_id":"s-test","base_revision":0,"peers":[],"claims":[]}"#;
            if ws.send(Message::text(welcome)).is_err() {
                return;
            }
            // Hold the socket open and ignore whatever arrives until the peer
            // hangs up, so the client keeps waiting on its read timeout.
            while ws.read().is_ok() {}
        });
        (url, handle)
    }

    /// A one-connection server that records every client frame it reads, so a
    /// test can assert what the client actually put on the wire.
    fn recording_server() -> (
        String,
        std::sync::Arc<Mutex<Vec<String>>>,
        std::sync::Arc<AtomicBool>,
        std::thread::JoinHandle<()>,
    ) {
        let listener = TcpListener::bind(("127.0.0.1", 0)).expect("bind loopback");
        let addr = listener.local_addr().expect("local addr");
        let url = format!("ws://{addr}/ws");
        let seen = std::sync::Arc::new(Mutex::new(Vec::new()));
        let closed = std::sync::Arc::new(AtomicBool::new(false));
        let seen_writer = std::sync::Arc::clone(&seen);
        let closed_writer = std::sync::Arc::clone(&closed);
        let handle = std::thread::spawn(move || {
            let Ok((stream, _)) = listener.accept() else {
                return;
            };
            let mut ws = match tungstenite::accept(stream) {
                Ok(ws) => ws,
                Err(_) => return,
            };
            let _ = ws.read();
            let welcome = r#"{"type":"welcome","session_id":"s-test","base_revision":0,"peers":[],"claims":[]}"#;
            if ws.send(Message::text(welcome)).is_err() {
                return;
            }
            loop {
                match ws.read() {
                    Ok(Message::Text(text)) => {
                        if let Ok(mut log) = seen_writer.lock() {
                            log.push(text.to_string());
                        }
                    }
                    Ok(Message::Close(_)) => {
                        closed_writer.store(true, Ordering::Relaxed);
                        let _ = ws.flush();
                        return;
                    }
                    Ok(_) => {}
                    Err(_) => return,
                }
            }
        });
        (url, seen, closed, handle)
    }

    /// VAL-CLI-002: the read loop really emits `heartbeat` frames on its
    /// cadence, and every one carries the session id from `welcome`.
    #[test]
    fn the_session_emits_heartbeats_carrying_its_session_id() {
        let _guard = config_lock();
        let (url, seen, _closed, server) = recording_server();
        set_client_config(ClientConfig {
            server_url: url,
            user_id: "u1".to_owned(),
            drawing_id: "PID-1101".to_owned(),
            mode: "协同编辑".to_owned(),
            client_version: "test".to_owned(),
        });
        *HEARTBEAT_OVERRIDE
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner()) = Some(Duration::from_millis(120));

        let (mut sender, receiver) = mpsc::channel::<CollabEvent>(64);
        let worker = std::thread::spawn(move || {
            let _ = run_client(&mut sender);
        });

        let mut receiver = receiver;
        let mut connected = false;
        for _ in 0..60 {
            match receiver.try_next() {
                Ok(Some(CollabEvent::Connected { .. })) => {
                    connected = true;
                    break;
                }
                Ok(Some(_)) => {}
                Ok(None) => break,
                Err(_) => std::thread::sleep(Duration::from_millis(20)),
            }
        }
        assert!(connected, "the client never reported a session");

        // Four cadence periods is enough for at least three beats.
        let deadline = Instant::now() + Duration::from_secs(2);
        let beats = loop {
            let beats: Vec<String> = seen
                .lock()
                .expect("frame log")
                .iter()
                .filter(|frame| frame.contains("\"type\":\"heartbeat\""))
                .cloned()
                .collect();
            if beats.len() >= 3 || Instant::now() >= deadline {
                break beats;
            }
            std::thread::sleep(Duration::from_millis(20));
        };
        assert!(
            beats.len() >= 3,
            "expected at least 3 heartbeats, saw {}: {beats:?}",
            beats.len()
        );
        for beat in &beats {
            assert!(
                beat.contains("s-test"),
                "a heartbeat must carry the session id from welcome: {beat}"
            );
        }

        drop(receiver);
        let mut exited = false;
        for _ in 0..60 {
            if worker.is_finished() {
                exited = true;
                break;
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        assert!(exited, "the transport thread leaked after cancellation");
        let _ = worker.join();
        *HEARTBEAT_OVERRIDE
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner()) = None;
        let _ = server.join();
    }

    /// VAL-CLI-016: quitting closes the socket instead of vanishing, and the
    /// thread is joined rather than leaked.
    #[test]
    fn quitting_sends_a_close_frame_and_joins_the_thread() {
        let _guard = config_lock();
        let (url, _seen, closed, server) = recording_server();
        set_client_config(ClientConfig {
            server_url: url,
            user_id: "u1".to_owned(),
            drawing_id: "PID-1101".to_owned(),
            mode: "协同编辑".to_owned(),
            client_version: "test".to_owned(),
        });

        let (mut sender, receiver) = mpsc::channel::<CollabEvent>(64);
        let worker = std::thread::spawn(move || {
            let _ = run_client(&mut sender);
        });
        register_worker(worker);

        let mut receiver = receiver;
        let mut connected = false;
        for _ in 0..60 {
            match receiver.try_next() {
                Ok(Some(CollabEvent::Connected { .. })) => {
                    connected = true;
                    break;
                }
                Ok(Some(_)) => {}
                Ok(None) => break,
                Err(_) => std::thread::sleep(Duration::from_millis(20)),
            }
        }
        assert!(connected, "the client never reported a session");

        let started = Instant::now();
        shutdown();
        let elapsed = started.elapsed();
        assert!(
            elapsed <= Duration::from_secs(3),
            "shutdown took {elapsed:?}, over its budget"
        );

        // The server saw the close frame, not a dead socket.
        let mut saw_close = false;
        for _ in 0..40 {
            if closed.load(Ordering::Relaxed) {
                saw_close = true;
                break;
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        assert!(saw_close, "the client never sent a close frame on quit");
        let _ = server.join();
        drop(receiver);
        // Reset the process-global stop flag so later tests are unaffected.
        STOP.store(false, Ordering::Relaxed);
    }

    #[test]
    fn a_read_timeout_is_not_an_error_branch() {
        let would_block =
            tungstenite::Error::Io(std::io::Error::from(ErrorKind::WouldBlock));
        let timed_out = tungstenite::Error::Io(std::io::Error::from(ErrorKind::TimedOut));
        assert!(is_read_timeout(&would_block));
        assert!(is_read_timeout(&timed_out));

        let reset = tungstenite::Error::Io(std::io::Error::from(ErrorKind::ConnectionReset));
        assert!(!is_read_timeout(&reset));
    }

    #[test]
    fn a_connection_reset_is_reported_as_a_read_error() {
        let reset = tungstenite::Error::Io(std::io::Error::from(ErrorKind::ConnectionReset));
        let described = describe_read_error(&reset);
        assert!(described.starts_with("read error:"));
        assert!(described.contains("ConnectionReset"), "{described}");
    }

    #[test]
    fn a_clean_close_is_not_confused_with_a_reset() {
        let described = describe_read_error(&tungstenite::Error::ConnectionClosed);
        assert!(described.contains("ConnectionClosed"));
        assert!(!described.contains("ConnectionReset"));
    }

    #[test]
    fn the_retry_ladder_doubles_then_holds_at_the_ceiling() {
        assert_eq!(backoff_delay(0), Duration::from_millis(200));
        assert_eq!(backoff_delay(1), Duration::from_millis(400));
        assert_eq!(backoff_delay(2), Duration::from_millis(800));
        assert_eq!(backoff_delay(3), Duration::from_millis(1600));
        assert_eq!(backoff_delay(4), Duration::from_millis(3200));
        assert_eq!(backoff_delay(5), BACKOFF_MAX);
        assert_eq!(backoff_delay(64), BACKOFF_MAX);
    }

    #[test]
    fn the_heartbeat_cadence_lands_inside_the_contract_window() {
        // VAL-CLI-002: 60 s must carry 3–5 beats, 13–17 s apart. The cadence
        // itself is the only input, so assert the arithmetic the contract
        // measures rather than waiting a minute for it.
        assert_eq!(HEARTBEAT_INTERVAL, Duration::from_secs(15));
        let window = Duration::from_secs(60);
        let beats = window.as_secs() / HEARTBEAT_INTERVAL.as_secs();
        assert!(
            (3..=5).contains(&beats),
            "60 s at 15 s per beat yields {beats} beats, outside 3..=5"
        );
        let gap = HEARTBEAT_INTERVAL.as_secs();
        assert!((13..=17).contains(&gap), "beat gap {gap}s outside 13..=17");
        // The server's timeout must be several beats wide, or one dropped beat
        // would retire a live session.
        assert!(
            HEARTBEAT_INTERVAL.as_secs() * 3 <= 90,
            "90 s timeout must tolerate several missed beats"
        );
    }

    #[test]
    fn a_cancelled_subscription_stops_the_thread_instead_of_leaking() {
        // `try_send` on a channel whose receiver is gone is the signal the read
        // thread exits on; assert the distinction the loop relies on.
        let (mut sender, receiver) = mpsc::channel::<CollabEvent>(4);
        drop(receiver);
        let error = sender
            .try_send(CollabEvent::Connecting { attempt: 0 })
            .expect_err("a dropped receiver must refuse the send");
        assert!(error.is_disconnected());
    }

    #[test]
    fn a_full_channel_drops_the_event_rather_than_blocking() {
        // futures' bounded channel holds `size` messages plus the slot the
        // sender owns, so fill it by sending until it refuses rather than
        // assuming an exact capacity.
        let (mut sender, _receiver) = mpsc::channel::<CollabEvent>(1);
        let mut attempt = 0u32;
        let full = loop {
            match sender.try_send(CollabEvent::Connecting { attempt }) {
                Ok(()) => attempt += 1,
                Err(error) => break error,
            }
            assert!(attempt < 16, "the channel never filled");
        };
        assert!(!full.is_disconnected(), "full is not disconnected");
        // And the loop's `emit` treats that as "keep going".
        assert!(emit(&mut sender, CollabEvent::Connecting { attempt }).is_ok());
    }

    #[test]
    fn the_published_config_round_trips_through_the_slot() {
        let _guard = config_lock();
        set_client_config(ClientConfig {
            server_url: "ws://127.0.0.1:3240/ws".to_owned(),
            user_id: "u1".to_owned(),
            drawing_id: "PID-1101".to_owned(),
            mode: "协同编辑".to_owned(),
            client_version: "test".to_owned(),
        });
        let config = current_config();
        assert_eq!(config.server_url, "ws://127.0.0.1:3240/ws");
        assert_eq!(config.user_id, "u1");
        assert_eq!(config.drawing_id, "PID-1101");
    }

    /// End-to-end: a real socket, a real `welcome`, then the subscription is
    /// cancelled and the thread must notice within its 100 ms read timeout.
    /// This is the "no leak, no hang" contract of the transport.
    #[test]
    fn cancelling_the_subscription_ends_the_thread_promptly() {
        let _guard = config_lock();
        let (url, server) = quiet_server();
        set_client_config(ClientConfig {
            server_url: url,
            user_id: "u1".to_owned(),
            drawing_id: "PID-1101".to_owned(),
            mode: "协同编辑".to_owned(),
            client_version: "test".to_owned(),
        });

        let (mut sender, receiver) = mpsc::channel::<CollabEvent>(64);
        let worker = std::thread::spawn(move || {
            let _ = run_client(&mut sender);
        });

        // Wait for the session to come up, so the read loop is definitely
        // spinning on its timeout when the receiver goes away.
        let mut receiver = receiver;
        let mut connected = false;
        for _ in 0..60 {
            match receiver.try_next() {
                Ok(Some(CollabEvent::Connected { .. })) => {
                    connected = true;
                    break;
                }
                Ok(Some(_)) => {}
                Ok(None) => break,
                Err(_) => std::thread::sleep(Duration::from_millis(50)),
            }
        }
        assert!(connected, "the client never reported a session");

        drop(receiver);
        let started = Instant::now();
        // The thread must observe the closed channel through its 100 ms read
        // timeout and exit; 200 ms is the contract, 2 s is the harness limit.
        let mut joined = false;
        for _ in 0..40 {
            if worker.is_finished() {
                joined = true;
                break;
            }
            std::thread::sleep(Duration::from_millis(50));
        }
        let elapsed = started.elapsed();
        assert!(joined, "the transport thread leaked after cancellation");
        assert!(
            elapsed <= Duration::from_millis(200),
            "cancellation took {elapsed:?}, over the 200 ms budget"
        );
        let _ = server.join();
    }
}
