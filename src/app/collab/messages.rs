//! Server→client frame decoding.
//!
//! The wire shapes live in the shared `ocs_collab` protocol crate, so the
//! client parses the exact same enum the server serialises instead of
//! re-deriving field names here. Only the messages that change visible state
//! become a [`CollabEvent`]; the rest are dropped deliberately.

use super::{CollabEvent, PeerInfo, PendingConflict};
use ocs_collab::protocol::Peer;
use ocs_collab::ServerMessage;

/// What one inbound frame means to the GUI.
pub(super) enum Frame {
    /// A message the UI reacts to.
    Event(CollabEvent),
    /// The server asked for a graceful close, or the session is over.
    Close,
    /// Well-formed JSON that carries nothing this layer acts on.
    Ignored,
    /// The frame could not be understood at all.
    Unreadable(String),
}

/// Classify one inbound text frame.
pub(super) fn classify(text: &str) -> Frame {
    let message: ServerMessage = match serde_json::from_str(text) {
        Ok(message) => message,
        Err(error) => return Frame::Unreadable(error.to_string()),
    };
    match message {
        ServerMessage::Welcome {
            session_id,
            base_revision,
            peers,
            claims,
        } => Frame::Event(CollabEvent::Connected {
            session_id,
            base_revision,
            peers: peers.iter().map(peer_info).collect(),
            claims: claims
                .into_iter()
                .map(|claim| (claim.tag, claim.user_id))
                .collect(),
        }),
        ServerMessage::PeerJoined(peer) => Frame::Event(CollabEvent::PeerJoined(peer_info(&peer))),
        ServerMessage::PeerLeft(peer) => Frame::Event(CollabEvent::PeerLeft {
            session_id: peer.session_id,
        }),
        ServerMessage::ClaimGranted { tag, .. } => Frame::Event(CollabEvent::ClaimGranted { tag }),
        ServerMessage::ClaimDenied {
            tag,
            holder,
            expires_at,
        } => Frame::Event(CollabEvent::ClaimDenied {
            tag,
            holder,
            expires_at,
        }),
        ServerMessage::ClaimReleased { tag, reason } => {
            Frame::Event(CollabEvent::ClaimReleased { tag, reason })
        }
        ServerMessage::OpApplied {
            op_id,
            revision,
            changes,
        } => Frame::Event(CollabEvent::OpApplied {
            op_id,
            revision,
            changes,
        }),
        ServerMessage::ReadonlyDowngrade {
            reason,
            holder,
            level,
            code,
        } => Frame::Event(CollabEvent::ReadonlyDowngrade {
            reason,
            holder,
            level,
            code,
        }),
        ServerMessage::Error {
            code,
            message,
            holder,
            ..
        } => {
            // A taken-over session is disconnected, not merely reported: the
            // server stops answering heartbeats for it, so the read loop would
            // otherwise spin until the socket dies on its own.
            if code == ocs_collab::protocol::codes::SESSION_TAKEN_OVER {
                return Frame::Close;
            }
            if let Some(holder) = holder {
                // `PERM_DENIED_STATE` names the session holding the drawing;
                // that is a read-only downgrade in everything but the name.
                if code == ocs_collab::protocol::codes::PERM_DENIED_STATE {
                    return Frame::Event(CollabEvent::ReadonlyDowngrade {
                        reason: message,
                        holder: Some(holder),
                        level: Some("state".to_owned()),
                        code: Some(code),
                    });
                }
            }
            Frame::Event(CollabEvent::ServerError { code, message })
        }
        // Used to be dropped here: the room was told and this client was not
        // listening, so the panel never showed an entry and the person whose
        // write had not taken effect was never told why.
        ServerMessage::Conflict {
            conflict_id,
            object_ref,
            field,
            value_a,
            value_b,
        } => Frame::Event(CollabEvent::Conflict(PendingConflict {
            conflict_id,
            object_ref,
            field,
            value_a,
            value_b,
        })),
    }
}

/// One participant on the drawing, as the server reports it.
fn peer_info(peer: &Peer) -> PeerInfo {
    PeerInfo {
        session_id: peer.session_id.clone(),
        user_id: peer.user_id.clone(),
        display_name: if peer.display_name.is_empty() {
            peer.user_id.clone()
        } else {
            peer.display_name.clone()
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn welcome_becomes_a_connected_event_with_peers_and_claims() {
        let frame = r#"{"type":"welcome","session_id":"s-1","base_revision":0,
            "peers":[{"session_id":"s-2","user_id":"u2","display_name":"工艺乙"}],
            "claims":[{"tag":"P-1001","session_id":"s-2","user_id":"u2","expires_at":1700000300}]}"#;
        match classify(frame) {
            Frame::Event(CollabEvent::Connected {
                session_id,
                base_revision,
                peers,
                claims,
            }) => {
                assert_eq!(session_id, "s-1");
                assert_eq!(base_revision, 0);
                assert_eq!(peers.len(), 1);
                assert_eq!(peers[0].display_name, "工艺乙");
                assert_eq!(claims, vec![("P-1001".to_owned(), "u2".to_owned())]);
            }
            _ => panic!("welcome must map to Connected"),
        }
    }

    /// `VAL-CROSS-007`: a `conflict` frame reaches the UI with everything it
    /// says — which entry, which field of which object, and the two values.
    #[test]
    fn a_conflict_frame_becomes_a_conflict_event() {
        let frame = r#"{"type":"conflict","conflict_id":"c-7f3a","object_ref":"P-1001",
            "field":"设计压力","value_a":"1.6MPa","value_b":"2.5MPa"}"#;
        match classify(frame) {
            Frame::Event(CollabEvent::Conflict(entry)) => {
                assert_eq!(
                    entry,
                    PendingConflict {
                        conflict_id: "c-7f3a".to_owned(),
                        object_ref: "P-1001".to_owned(),
                        field: "设计压力".to_owned(),
                        value_a: "1.6MPa".to_owned(),
                        value_b: "2.5MPa".to_owned(),
                    }
                );
            }
            _ => panic!("conflict must map to Conflict"),
        }
    }

    #[test]
    fn peer_joined_is_flat_on_the_wire() {
        let frame =
            r#"{"type":"peer_joined","session_id":"s-2","user_id":"u2","display_name":"工艺乙"}"#;
        match classify(frame) {
            Frame::Event(CollabEvent::PeerJoined(peer)) => {
                assert_eq!(peer.session_id, "s-2");
                assert_eq!(peer.user_id, "u2");
            }
            _ => panic!("peer_joined must map to PeerJoined"),
        }
    }

    #[test]
    fn a_blank_display_name_falls_back_to_the_user_id() {
        let frame = r#"{"type":"peer_joined","session_id":"s-2","user_id":"u2"}"#;
        match classify(frame) {
            Frame::Event(CollabEvent::PeerJoined(peer)) => {
                assert_eq!(peer.display_name, "u2");
            }
            _ => panic!("peer_joined must map to PeerJoined"),
        }
    }

    #[test]
    fn op_applied_moves_the_baseline() {
        let frame = r#"{"type":"op_applied","op_id":"op-1","revision":4,"changes":[]}"#;
        match classify(frame) {
            Frame::Event(CollabEvent::OpApplied {
                op_id,
                revision,
                changes,
            }) => {
                assert_eq!(op_id, "op-1");
                assert_eq!(revision, 4);
                assert!(changes.is_empty());
            }
            _ => panic!("op_applied must move the baseline"),
        }
    }

    /// The change list is what a peer applies to its own document
    /// (`VAL-CLI-009`), so it must survive the trip verbatim.
    #[test]
    fn op_applied_carries_the_change_list() {
        let frame = r#"{"type":"op_applied","op_id":"op-7","revision":1,
            "changes":[{"kind":"set_tag","object_ref":"P-1001","field":"tag",
            "value":"P-1009","old_value":"P-1001","handle":"1A"}]}"#;
        match classify(frame) {
            Frame::Event(CollabEvent::OpApplied { changes, .. }) => {
                assert_eq!(changes.len(), 1);
                assert_eq!(changes[0]["object_ref"], "P-1001");
                assert_eq!(changes[0]["value"], "P-1009");
                assert_eq!(changes[0]["kind"], "set_tag");
                assert_eq!(changes[0]["handle"], "1A");
            }
            _ => panic!("op_applied must map to OpApplied"),
        }
    }

    #[test]
    fn takeover_closes_the_connection_instead_of_reporting_an_error() {
        let frame = r#"{"type":"error","code":"SESSION_TAKEN_OVER","message":"会话已被新连接接管"}"#;
        assert!(matches!(classify(frame), Frame::Close));
    }

    #[test]
    fn a_state_denial_is_a_readonly_downgrade() {
        let frame = r#"{"type":"error","code":"PERM_DENIED_STATE","message":"已有活动会话",
            "level":"state","holder":"s-9"}"#;
        match classify(frame) {
            Frame::Event(CollabEvent::ReadonlyDowngrade { holder, .. }) => {
                assert_eq!(holder.as_deref(), Some("s-9"));
            }
            _ => panic!("a state denial must downgrade to read-only"),
        }
    }

    #[test]
    fn an_unknown_frame_is_unreadable_rather_than_fatal() {
        assert!(matches!(
            classify(r#"{"type":"something_new"}"#),
            Frame::Unreadable(_)
        ));
        assert!(matches!(classify("not json"), Frame::Unreadable(_)));
    }
}
