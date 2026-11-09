//! Desktop collaboration client: the state the GUI shows, the message stream
//! that feeds it, and the synchronous WebSocket transport underneath.
//!
//! The transport is deliberately synchronous (`tungstenite` + `std::thread` +
//! an iced futures channel): the desktop build must stay free of an async
//! runtime (see `architecture.md` §5.2 and `VAL-CLI-019`). Everything the GUI
//! needs to know arrives as a [`CollabEvent`] on the subscription channel, so
//! no collab code ever touches the socket from the UI thread.
//!
//! This module owns *state* only. Claim/broadcast behaviour lands in later
//! features; the transport here already carries the whole server→client
//! message set so those features only have to react to the events.

use serde_json::{json, Value};
use std::collections::BTreeMap;

#[cfg(not(target_arch = "wasm32"))]
mod messages;
#[cfg(not(target_arch = "wasm32"))]
mod transport;

/// One participant on the drawing, as the server reports it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PeerInfo {
    pub session_id: String,
    pub user_id: String,
    pub display_name: String,
}

/// What the transport thread reports to the UI thread.
///
/// One variant per server message that changes visible state, plus the three
/// connection-lifecycle variants the UI needs to draw "connecting", "offline"
/// and "connected" honestly.
#[derive(Clone, Debug, PartialEq)]
pub enum CollabEvent {
    /// A connection attempt is about to start; `attempt` counts from zero for
    /// the first try after a successful session.
    Connecting { attempt: u32 },
    /// `welcome` arrived: the session exists and this is its baseline.
    Connected {
        session_id: String,
        base_revision: i64,
        peers: Vec<PeerInfo>,
        claims: Vec<(String, String)>,
    },
    /// The socket ended. `reason` names the branch that ended it (`read error:
    /// ConnectionReset` vs `close frame`), which is what `VAL-CLI-020` reads.
    Disconnected { reason: String, will_retry: bool },
    PeerJoined(PeerInfo),
    PeerLeft { session_id: String },
    ClaimGranted { tag: String },
    ClaimDenied {
        tag: String,
        holder: String,
        expires_at: i64,
    },
    ClaimReleased { tag: String, reason: String },
    ReadonlyDowngrade {
        reason: String,
        holder: Option<String>,
    },
    /// `op_applied`: the baseline moved forward.
    BaseRevision { revision: i64 },
    ServerError { code: String, message: String },
}

/// Everything the transport thread needs to open a session. Copied out of the
/// app state before the thread starts, so the thread never reads app memory.
#[cfg(not(target_arch = "wasm32"))]
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(super) struct ClientConfig {
    pub server_url: String,
    pub user_id: String,
    pub drawing_id: String,
    pub mode: String,
    pub client_version: String,
}

/// The GUI's view of the collaboration session.
#[derive(Clone, Debug, Default)]
pub(super) struct State {
    /// Server URL in effect (CLI wins over the saved config). Empty means the
    /// user never configured collaboration, and nothing connects at all.
    pub server_url: String,
    /// Server URL as persisted in `settings.json`. `--collab` overrides
    /// `server_url` for this run only; persisting the override would make a
    /// later launch without the flag connect to whatever the last scripted
    /// run used.
    pub configured_server_url: String,
    pub user_id: String,
    /// User id as persisted in `settings.json` (same reasoning as above).
    pub configured_user_id: String,
    pub drawing_id: String,
    /// Drawing id as persisted in `settings.json`.
    pub configured_drawing_id: String,
    pub mode: String,
    /// Mode as persisted in `settings.json`.
    pub configured_mode: String,
    pub connected: bool,
    pub session_id: Option<String>,
    pub base_revision: i64,
    pub peers: Vec<PeerInfo>,
    /// tag → holder user id, exactly the shape `GET /api/v1/collab/status`
    /// publishes (see `architecture.md` A.5).
    pub claims: BTreeMap<String, String>,
    pub readonly: bool,
    pub readonly_holder: Option<String>,
    /// Last transport-level failure, for the command line and diagnostics.
    pub last_error: Option<String>,
    /// How many reconnect attempts have failed since the last live session.
    pub reconnect_attempts: u32,
}

impl State {
    pub(super) fn new() -> Self {
        Self {
            mode: DEFAULT_MODE.to_owned(),
            ..Self::default()
        }
    }

    /// Whether a session should be (or is being) maintained. Without a server
    /// URL nothing connects — the offline-first invariant of `VAL-CLI-017`.
    pub(super) fn wanted(&self) -> bool {
        !self.server_url.trim().is_empty()
    }

    /// Take the config the transport thread will use.
    #[cfg(not(target_arch = "wasm32"))]
    pub(super) fn client_config(&self) -> ClientConfig {
        ClientConfig {
            server_url: self.server_url.clone(),
            user_id: self.user_id.clone(),
            drawing_id: self.drawing_id.clone(),
            mode: if self.mode.is_empty() {
                DEFAULT_MODE.to_owned()
            } else {
                self.mode.clone()
            },
            client_version: env!("OCS_APP_VERSION").to_owned(),
        }
    }

    /// Publish the current config to the process-global the subscription
    /// worker reads when it starts its thread.
    #[cfg(not(target_arch = "wasm32"))]
    pub(super) fn sync_client_config(&self) {
        transport::set_client_config(self.client_config());
    }

    /// Fold one transport event into the visible state.
    pub(super) fn apply(&mut self, event: CollabEvent) {
        match event {
            CollabEvent::Connecting { attempt } => {
                self.connected = false;
                self.reconnect_attempts = attempt;
            }
            CollabEvent::Connected {
                session_id,
                base_revision,
                peers,
                claims,
            } => {
                self.connected = true;
                self.session_id = Some(session_id);
                self.base_revision = base_revision;
                self.peers = peers;
                self.claims = claims.into_iter().collect();
                // A fresh session re-derives write access from the server; a
                // stale downgrade must not survive a reconnect.
                self.readonly = false;
                self.readonly_holder = None;
                self.reconnect_attempts = 0;
                self.last_error = None;
            }
            CollabEvent::Disconnected { reason, will_retry } => {
                self.connected = false;
                self.session_id = None;
                self.peers.clear();
                self.claims.clear();
                self.readonly = false;
                self.readonly_holder = None;
                self.last_error = Some(if will_retry {
                    format!("{reason}（将自动重连）")
                } else {
                    reason
                });
            }
            CollabEvent::PeerJoined(peer) => {
                if let Some(existing) = self
                    .peers
                    .iter_mut()
                    .find(|p| p.session_id == peer.session_id)
                {
                    *existing = peer;
                } else {
                    self.peers.push(peer);
                }
            }
            CollabEvent::PeerLeft { session_id } => {
                self.peers.retain(|p| p.session_id != session_id);
            }
            CollabEvent::ClaimGranted { tag } => {
                let holder = self.user_id.clone();
                self.claims.insert(tag, holder);
            }
            CollabEvent::ClaimDenied {
                tag,
                holder,
                expires_at,
            } => {
                // The claim stays with its holder; record who it is so the
                // panel can grey the row out and say until when.
                self.last_error = Some(format!("位号 {tag} 已被占用，到期时间 {expires_at}"));
                self.claims.insert(tag, holder);
            }
            CollabEvent::ClaimReleased { tag, .. } => {
                self.claims.remove(&tag);
            }
            CollabEvent::ReadonlyDowngrade { reason, holder } => {
                self.readonly = true;
                self.readonly_holder = holder;
                self.last_error = Some(format!("只读降级：{reason}"));
            }
            CollabEvent::BaseRevision { revision } => {
                self.base_revision = revision;
            }
            CollabEvent::ServerError { code, message } => {
                self.last_error = Some(format!("{code}: {message}"));
            }
        }
    }

    /// The body of `GET /api/v1/collab/status` (`architecture.md` A.5).
    pub(super) fn status_json(&self) -> Value {
        json!({
            "ok": true,
            "configured": self.wanted(),
            "connected": self.connected,
            "server_url": self.server_url,
            "user_id": self.user_id,
            "drawing_id": self.drawing_id,
            "session_id": self.session_id,
            "base_revision": self.base_revision,
            "peers": self
                .peers
                .iter()
                .map(|p| json!({
                    "session_id": p.session_id,
                    "user_id": p.user_id,
                    "display_name": p.display_name,
                }))
                .collect::<Vec<_>>(),
            "claims": self
                .claims
                .iter()
                .map(|(tag, holder)| (tag.clone(), json!(holder)))
                .collect::<serde_json::Map<String, Value>>(),
            "readonly": self.readonly,
            "readonly_holder": self.readonly_holder,
            "reconnect_attempts": self.reconnect_attempts,
            "last_error": self.last_error,
        })
    }
}

/// Session mode used when neither the CLI nor the config names one. The server
/// treats any mode other than `只读` as an editing session.
pub(super) const DEFAULT_MODE: &str = "协同编辑";

/// Subscribe the app to the collaboration stream.
///
/// `iced::Subscription::run` identifies a subscription by the worker's function
/// pointer, so the worker must be a bare `fn` — a closure would re-subscribe on
/// every frame.
#[cfg(not(target_arch = "wasm32"))]
pub(super) fn subscribe() -> iced::Subscription<CollabEvent> {
    iced::Subscription::run(worker)
}

#[cfg(not(target_arch = "wasm32"))]
fn worker() -> impl iced::futures::Stream<Item = CollabEvent> {
    iced::stream::channel(64, |mut sender| async move {
        eprintln!("[collab] 订阅已启动，准备连接");
        let handle = std::thread::spawn(move || {
            if let Err(error) = transport::run_client(&mut sender) {
                eprintln!("[collab] 传输层退出：{error}");
            }
        });
        // The quit path needs this handle to wait for the goodbye frame, so
        // the thread that owns the socket is the one the app joins.
        transport::register_worker(handle);
        iced::futures::future::pending::<()>().await;
    })
}

/// Close the collaboration socket before the process exits.
///
/// A process that just dies leaves the server counting down the 90 s heartbeat
/// timeout; saying goodbye retires the session and tells the other peers at
/// once (`VAL-CLI-016`). Blocking here is bounded by the transport's own
/// budget, so a wedged socket cannot stall the quit.
#[cfg(not(target_arch = "wasm32"))]
pub(super) fn shutdown() {
    transport::shutdown();
}

#[cfg(test)]
mod tests {
    use super::*;

    fn peer(id: &str, user: &str) -> PeerInfo {
        PeerInfo {
            session_id: id.to_owned(),
            user_id: user.to_owned(),
            display_name: user.to_owned(),
        }
    }

    #[test]
    fn no_server_url_means_no_connection_is_wanted() {
        let mut state = State::new();
        assert!(!state.wanted(), "an unconfigured client must stay offline");
        assert_eq!(state.status_json()["connected"], false);
        assert_eq!(state.status_json()["server_url"], "");

        state.server_url = "ws://127.0.0.1:3240/ws".to_owned();
        assert!(state.wanted());
    }

    #[test]
    fn welcome_fills_the_published_status_shape() {
        let mut state = State::new();
        state.server_url = "ws://127.0.0.1:3240/ws".to_owned();
        state.user_id = "u1".to_owned();
        state.apply(CollabEvent::Connected {
            session_id: "s-1".to_owned(),
            base_revision: 0,
            peers: vec![peer("s-2", "u2")],
            claims: vec![("P-1001".to_owned(), "u1".to_owned())],
        });

        let status = state.status_json();
        assert_eq!(status["connected"], true);
        assert_eq!(status["server_url"], "ws://127.0.0.1:3240/ws");
        assert_eq!(status["session_id"], "s-1");
        assert_eq!(status["base_revision"], 0);
        assert_eq!(status["peers"][0]["user_id"], "u2");
        assert_eq!(status["claims"]["P-1001"], "u1");
        assert_eq!(status["readonly"], false);
        assert!(status["readonly_holder"].is_null());
    }

    #[test]
    fn disconnect_clears_the_session_but_keeps_the_configured_url() {
        let mut state = State::new();
        state.server_url = "ws://127.0.0.1:3240/ws".to_owned();
        state.apply(CollabEvent::Connected {
            session_id: "s-1".to_owned(),
            base_revision: 3,
            peers: vec![peer("s-2", "u2")],
            claims: vec![("P-1001".to_owned(), "u1".to_owned())],
        });
        state.apply(CollabEvent::Disconnected {
            reason: "read error: ConnectionReset".to_owned(),
            will_retry: true,
        });

        let status = state.status_json();
        assert_eq!(status["connected"], false);
        assert!(status["session_id"].is_null());
        assert_eq!(status["peers"].as_array().unwrap().len(), 0);
        assert_eq!(status["claims"].as_object().unwrap().len(), 0);
        assert_eq!(status["server_url"], "ws://127.0.0.1:3240/ws");
        assert!(status["last_error"]
            .as_str()
            .unwrap()
            .contains("ConnectionReset"));
    }

    #[test]
    fn peer_join_and_leave_track_the_roster_without_duplicates() {
        let mut state = State::new();
        state.apply(CollabEvent::PeerJoined(peer("s-2", "u2")));
        state.apply(CollabEvent::PeerJoined(peer("s-2", "u2")));
        assert_eq!(state.peers.len(), 1);

        state.apply(CollabEvent::PeerLeft {
            session_id: "s-2".to_owned(),
        });
        assert!(state.peers.is_empty());
    }

    #[test]
    fn readonly_downgrade_records_the_holder() {
        let mut state = State::new();
        state.apply(CollabEvent::ReadonlyDowngrade {
            reason: "另一会话进入单人编辑".to_owned(),
            holder: Some("s-9".to_owned()),
        });
        assert!(state.readonly);
        assert_eq!(state.readonly_holder.as_deref(), Some("s-9"));
    }

    #[test]
    fn claim_events_keep_the_holder_map_in_step() {
        let mut state = State::new();
        state.user_id = "u1".to_owned();
        state.apply(CollabEvent::ClaimGranted {
            tag: "P-1001".to_owned(),
        });
        assert_eq!(state.claims.get("P-1001").map(String::as_str), Some("u1"));

        state.apply(CollabEvent::ClaimDenied {
            tag: "P-1002".to_owned(),
            holder: "u2".to_owned(),
            expires_at: 1_700_000_300,
        });
        assert_eq!(state.claims.get("P-1002").map(String::as_str), Some("u2"));

        state.apply(CollabEvent::ClaimReleased {
            tag: "P-1001".to_owned(),
            reason: "released".to_owned(),
        });
        assert!(!state.claims.contains_key("P-1001"));
    }
}
