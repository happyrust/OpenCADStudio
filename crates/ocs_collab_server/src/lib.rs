//! Collaboration server library surface.
//!
//! `main.rs` only parses the CLI and starts the runtime; everything testable
//! lives here so integration tests can build the same router the binary serves.

pub mod bus;
pub mod http;
pub mod store;

use std::sync::Arc;

use ocs_collab::SessionConfig;

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
