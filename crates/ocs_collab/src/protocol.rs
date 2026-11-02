//! Wire protocol shared by the collaboration server and the desktop client.
//!
//! Every frame is a JSON object with a `type` discriminator (see
//! `architecture.md` §3.1). The server only understands P&ID *semantics*; it
//! never looks inside DWG bytes, so `Op` payloads are carried as opaque JSON.

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// A message sent by a client to the server.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ClientMessage {
    /// Opens a session. Answered with [`ServerMessage::Welcome`].
    Hello {
        user_id: String,
        drawing_id: String,
        mode: String,
        #[serde(default)]
        client_version: String,
        /// Source workspace, copied into every audit row this session writes.
        #[serde(default)]
        workspace: String,
    },
    /// Keep-alive, expected every 15 s.
    Heartbeat { session_id: String },
    /// Requests exclusive ownership of one tag.
    Claim { session_id: String, tag: String },
    /// Gives up an owned tag voluntarily.
    Release { session_id: String, tag: String },
    /// Submits one P&ID semantic operation.
    Op {
        session_id: String,
        base_revision: i64,
        op_id: String,
        payload: Value,
    },
}

/// A message sent by the server to a client.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ServerMessage {
    /// Session established.
    Welcome {
        session_id: String,
        base_revision: i64,
        peers: Vec<Peer>,
        claims: Vec<ClaimInfo>,
    },
    /// Structured failure. `level` names the first denying tier when the
    /// failure came out of the permission chain.
    Error {
        code: String,
        message: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        level: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        holder: Option<String>,
    },
    /// Tag ownership granted.
    ClaimGranted { tag: String, session_id: String },
    /// Tag ownership refused; `holder` still owns it until `expires_at`.
    ClaimDenied {
        tag: String,
        holder: String,
        expires_at: i64,
    },
    /// Tag ownership ended. `reason` is `released` or `expired`.
    ClaimReleased { tag: String, reason: String },
    /// Operation applied; broadcast to every session on the drawing.
    OpApplied {
        op_id: String,
        revision: i64,
        changes: Vec<Value>,
    },
    /// A peer joined this drawing.
    PeerJoined(Peer),
    /// A peer left this drawing.
    PeerLeft(Peer),
    /// The session lost write access.
    ReadonlyDowngrade { reason: String, holder: Option<String> },
    /// A conflict entry was created and awaits adjudication.
    Conflict {
        conflict_id: String,
        object_ref: String,
        field: String,
        value_a: String,
        value_b: String,
    },
}

/// One connected participant.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Peer {
    pub session_id: String,
    pub user_id: String,
    #[serde(default)]
    pub display_name: String,
}

/// One outstanding tag claim, as advertised in `welcome`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ClaimInfo {
    pub tag: String,
    pub session_id: String,
    pub user_id: String,
    pub expires_at: i64,
}

/// Error codes the protocol may return. Kept as constants so the client and
/// the server can never drift on spelling.
pub mod codes {
    pub const SESSION_UNKNOWN: &str = "SESSION_UNKNOWN";
    pub const SESSION_EXPIRED: &str = "SESSION_EXPIRED";
    pub const SESSION_TAKEN_OVER: &str = "SESSION_TAKEN_OVER";
    pub const HELLO_REQUIRED: &str = "HELLO_REQUIRED";
    pub const UNKNOWN_MESSAGE_TYPE: &str = "UNKNOWN_MESSAGE_TYPE";
    pub const INVALID_MESSAGE: &str = "INVALID_MESSAGE";
    pub const UNKNOWN_DRAWING: &str = "UNKNOWN_DRAWING";
    pub const UNKNOWN_USER: &str = "UNKNOWN_USER";
    pub const PERM_DENIED_ROLE: &str = "PERM_DENIED_ROLE";
    pub const PERM_DENIED_PBS: &str = "PERM_DENIED_PBS";
    pub const PERM_DENIED_STATE: &str = "PERM_DENIED_STATE";
    pub const PERM_UNKNOWN_SUBJECT: &str = "PERM_UNKNOWN_SUBJECT";
    pub const PERM_UNKNOWN_OPERATION: &str = "PERM_UNKNOWN_OPERATION";
    pub const PERM_INVALID_STATE: &str = "PERM_INVALID_STATE";
    pub const OBJECT_REFERENCED: &str = "OBJECT_REFERENCED";
    pub const INVALID_STATE_TRANSITION: &str = "INVALID_STATE_TRANSITION";
    pub const STALE_BASE_REVISION: &str = "STALE_BASE_REVISION";
}

/// Reasons carried by [`ServerMessage::ClaimReleased`].
pub mod release_reasons {
    /// The owner sent `release`.
    pub const RELEASED: &str = "released";
    /// The claim's TTL elapsed.
    pub const EXPIRED: &str = "expired";
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hello_round_trips_through_json_with_workspace() {
        let raw = r#"{"type":"hello","user_id":"u1","drawing_id":"PID-1101",
                      "mode":"协同编辑","client_version":"1","workspace":"plant-a"}"#;
        let msg: ClientMessage = serde_json::from_str(raw).expect("hello must parse");
        match &msg {
            ClientMessage::Hello {
                user_id,
                drawing_id,
                workspace,
                ..
            } => {
                assert_eq!(user_id, "u1");
                assert_eq!(drawing_id, "PID-1101");
                assert_eq!(workspace, "plant-a");
            }
            other => panic!("expected hello, got {other:?}"),
        }
        let back = serde_json::to_string(&msg).expect("hello must serialize");
        assert!(back.contains("\"type\":\"hello\""));
    }

    #[test]
    fn hello_tolerates_missing_optional_fields() {
        let raw = r#"{"type":"hello","user_id":"u1","drawing_id":"PID-1101","mode":"协同编辑"}"#;
        let msg: ClientMessage = serde_json::from_str(raw).expect("optional fields default");
        match msg {
            ClientMessage::Hello {
                client_version,
                workspace,
                ..
            } => {
                assert_eq!(client_version, "");
                assert_eq!(workspace, "");
            }
            other => panic!("expected hello, got {other:?}"),
        }
    }

    #[test]
    fn op_payload_is_opaque_to_the_protocol_layer() {
        let raw = r#"{"type":"op","session_id":"s-1","base_revision":0,"op_id":"op-001",
                      "payload":{"kind":"set_property","object_ref":"V-1001",
                                 "field":"设计压力","value":"1.6MPa"}}"#;
        let msg: ClientMessage = serde_json::from_str(raw).expect("op must parse");
        match msg {
            ClientMessage::Op { payload, op_id, .. } => {
                assert_eq!(op_id, "op-001");
                assert_eq!(payload["kind"], "set_property");
                assert_eq!(payload["value"], "1.6MPa");
            }
            other => panic!("expected op, got {other:?}"),
        }
    }

    #[test]
    fn unknown_message_type_fails_to_deserialize() {
        let raw = r#"{"type":"totally_unknown_type","session_id":"s-1"}"#;
        assert!(serde_json::from_str::<ClientMessage>(raw).is_err());
    }

    #[test]
    fn welcome_serializes_peers_and_claims_as_arrays() {
        let msg = ServerMessage::Welcome {
            session_id: "s-1".into(),
            base_revision: 0,
            peers: vec![Peer {
                session_id: "s-1".into(),
                user_id: "u1".into(),
                display_name: "工艺甲".into(),
            }],
            claims: vec![ClaimInfo {
                tag: "V-1001".into(),
                session_id: "s-1".into(),
                user_id: "u1".into(),
                expires_at: 1_700_000_000,
            }],
        };
        let value: Value = serde_json::to_value(&msg).expect("welcome must serialize");
        assert_eq!(value["type"], "welcome");
        assert!(value["peers"].is_array());
        assert!(value["claims"].is_array());
        assert_eq!(value["claims"][0]["tag"], "V-1001");
    }

    #[test]
    fn error_omits_absent_optional_fields() {
        let msg = ServerMessage::Error {
            code: codes::HELLO_REQUIRED.into(),
            message: "send hello first".into(),
            level: None,
            holder: None,
        };
        let text = serde_json::to_string(&msg).expect("error must serialize");
        assert!(!text.contains("level"));
        assert!(!text.contains("holder"));
    }

    #[test]
    fn peer_joined_carries_the_peer_inline() {
        let msg = ServerMessage::PeerJoined(Peer {
            session_id: "s-2".into(),
            user_id: "u2".into(),
            display_name: "工艺乙".into(),
        });
        let value: Value = serde_json::to_value(&msg).expect("peer_joined must serialize");
        assert_eq!(value["type"], "peer_joined");
        assert_eq!(value["session_id"], "s-2");
        assert_eq!(value["display_name"], "工艺乙");
    }

    #[test]
    fn claim_denied_reports_holder_and_expiry() {
        let msg = ServerMessage::ClaimDenied {
            tag: "V-1001".into(),
            holder: "s-1".into(),
            expires_at: 1_700_000_300,
        };
        let value: Value = serde_json::to_value(&msg).expect("claim_denied must serialize");
        assert_eq!(value["type"], "claim_denied");
        assert_eq!(value["holder"], "s-1");
        assert_eq!(value["expires_at"], 1_700_000_300_i64);
    }
}
