//! Collaboration server library surface.
//!
//! `main.rs` only parses the CLI and starts the runtime; everything testable
//! lives here so integration tests can build the same router the binary serves.

pub mod bus;
pub mod http;
pub mod perm;
pub mod store;
pub mod ws;

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use ocs_collab::{OccupancyTable, ServerMessage, SessionConfig, SessionRegistry};

pub use bus::Bus;
pub use store::Store;

/// The server's shared state.
pub struct AppState {
    /// `None` when the database could not be opened; `GET /health` then
    /// reports 503 instead of pretending everything is fine.
    pub store: Option<Arc<Store>>,
    pub db_error: Option<String>,
    pub db_path: String,
    pub bus: Arc<Bus>,
    pub session_config: SessionConfig,
    /// Live session registry — the in-memory authority mirrored into the
    /// `sessions` table. Behind a tokio mutex because handlers await while
    /// deciding.
    pub sessions: Arc<tokio::sync::Mutex<SessionRegistry>>,
    /// Live tag-occupancy table — the in-memory authority mirrored into the
    /// `claims` table.
    pub occupancy: Arc<tokio::sync::Mutex<OccupancyTable>>,
    /// Per-drawing revision counter used only when `store` is `None`. With a
    /// database, `drawings.revision` is authoritative; without one, ops still
    /// need a monotonically increasing revision for `STALE_BASE_REVISION`
    /// checking to keep working.
    pub revisions: Arc<Mutex<HashMap<String, i64>>>,
    /// Per-session unicast channel, used for messages that must not reach the
    /// rest of the drawing room (targeted errors).
    pub direct: Arc<Mutex<HashMap<String, tokio::sync::mpsc::UnboundedSender<ServerMessage>>>>,
    pub version: &'static str,
}

impl AppState {
    pub fn new(
        store: Option<Store>,
        db_error: Option<String>,
        db_path: impl Into<String>,
        session_config: SessionConfig,
    ) -> AppState {
        AppState {
            store: store.map(Arc::new),
            db_error,
            db_path: db_path.into(),
            bus: Arc::new(Bus::default()),
            session_config,
            sessions: Arc::new(tokio::sync::Mutex::new(SessionRegistry::new(session_config))),
            occupancy: Arc::new(tokio::sync::Mutex::new(OccupancyTable::new())),
            revisions: Arc::new(Mutex::new(HashMap::new())),
            direct: Arc::new(Mutex::new(HashMap::new())),
            version: env!("CARGO_PKG_VERSION"),
        }
    }

    /// Whether the database answered a trivial query just now.
    pub fn db_healthy(&self) -> bool {
        self.store.as_ref().is_some_and(|store| store.health_check())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_storeless_state_reports_an_unhealthy_database() {
        let state = AppState::new(None, Some("open failed".into()), "bad.db", SessionConfig::default());
        assert!(!state.db_healthy());
        assert_eq!(state.db_error.as_deref(), Some("open failed"));
    }

    #[test]
    fn a_memory_store_reports_a_healthy_database() {
        let store = Store::open_in_memory().expect("memory store");
        let state = AppState::new(Some(store), None, ":memory:", SessionConfig::default());
        assert!(state.db_healthy());
        assert_eq!(state.session_config.heartbeat_timeout, 90);
        assert_eq!(state.session_config.claim_ttl, 300);
    }
}
