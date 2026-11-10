//! Op intake: the three-step judgment (`ocs_collab::conflict`) wired to the
//! store, the room and the tie window.
//!
//! An op used to be taken only when its `base_revision` was the drawing's
//! current revision, and refused otherwise. Field writes are now judged
//! instead (`architecture.md` §3.3):
//!
//! 1. made against an older baseline, on a field nobody else wrote since: it
//!    merges, and takes the next revision;
//! 2. made against an older baseline, on a field another session wrote since:
//!    it yields — `error` `OP_SUPERSEDED` to its sender, the value it would
//!    have overwritten in the audit log, no conflict entry;
//! 3. two of them made against the current revision, same field, different
//!    values, arriving inside the tie window: neither takes effect, a
//!    conflict entry is made and the room is told (`conflict`).
//!
//! Step 3 needs the first of the two not to have taken effect when the second
//! arrives, so with a tie window configured (`--tie-window-ms`) an op made
//! against the current revision is *held* for that long before it commits.
//! With no window (the default) nothing is held, every op commits on arrival,
//! and two writes to one field are always decided by arrival: the second is
//! behind the first.
//!
//! Ops of any other kind are opaque here and keep the old rule: current
//! revision or `STALE_BASE_REVISION`.

use std::time::Duration;

use ocs_collab::conflict::{collides, Change, DrawingModel, Verdict, KIND_SET_TAG};
use ocs_collab::protocol::codes;
use ocs_collab::{new_conflict_id, ServerMessage, Session};
use serde_json::Value;
use tokio::time::Instant;

use crate::store::{ConflictRow, NewAuditRow, OpCommit, OpRecord};
use crate::ws::{self, Reaction};
use crate::AppState;

/// `conflicts.status` of an entry nobody has ruled on.
pub const STATUS_PENDING: &str = "待裁决";

/// What the server knows of one drawing, and the ops it is holding.
#[derive(Debug, Default)]
pub struct DrawingOps {
    pub model: DrawingModel,
    /// Ops made against the current revision, inside the tie window, in the
    /// order they arrived.
    pub held: Vec<HeldOp>,
}

/// An op waiting out the tie window.
#[derive(Debug, Clone)]
pub struct HeldOp {
    pub op_id: String,
    pub session: Session,
    pub peer_addr: String,
    pub base_revision: i64,
    pub changes: Vec<Change>,
    /// Unix seconds it arrived at; the audit rows it writes carry this.
    pub received_at: i64,
    pub due: Instant,
}

/// What handling one op came to.
#[derive(Debug, Default)]
pub struct Outcome {
    pub reactions: Vec<Reaction>,
    /// The revision the drawing moved to, when it moved.
    pub revision: Option<i64>,
}

impl DrawingOps {
    /// The drawing as the op log has it: every applied change replayed in
    /// order, then the revision the `drawings` row says.
    fn load(state: &AppState, drawing_id: &str) -> Self {
        let mut model = DrawingModel::default();
        if let Some(store) = &state.store {
            match store.load_ops(drawing_id, 0) {
                Ok(ops) => {
                    for op in ops {
                        for entry in &op.changes {
                            for change in Change::list(entry) {
                                let name = change.object_ref.clone();
                                model.commit(&change, &name, op.revision, &op.session_id, &op.user_id);
                            }
                        }
                    }
                }
                Err(err) => eprintln!("[ocs_collab_server] 读取 {drawing_id} 的操作日志失败：{err}"),
            }
        }
        model.set_revision(ws::current_revision(state, drawing_id));
        Self {
            model,
            held: Vec::new(),
        }
    }

    /// The value a held op of another session is about to give the field
    /// `change` writes, when it differs.
    fn held_against(&self, change: &Change, session_id: &str) -> Option<(&HeldOp, &Change)> {
        self.held
            .iter()
            .filter(|held| held.session.session_id != session_id)
            .find_map(|held| {
                held.changes
                    .iter()
                    .find(|theirs| collides(theirs, change))
                    .map(|theirs| (held, theirs))
            })
    }
}

/// An op as the judgment sees it, whether it just arrived or was held.
struct Incoming<'a> {
    session: &'a Session,
    peer_addr: &'a str,
    op_id: &'a str,
    base_revision: i64,
    now: i64,
}

/// Take one op from `session`.
///
/// Synchronous on purpose: the judgment, the commit and the model update run
/// under one lock with no await in between, so two ops on one drawing cannot
/// interleave. The caller moves the session registry afterwards.
pub fn receive(
    state: &AppState,
    session: &Session,
    peer_addr: &str,
    base_revision: i64,
    op_id: String,
    payload: Value,
    now: i64,
) -> Outcome {
    let mut drawings = state.drawings.lock().expect("drawings mutex poisoned");
    let drawing = drawings
        .entry(session.drawing_id.clone())
        .or_insert_with(|| DrawingOps::load(state, &session.drawing_id));
    let current = drawing.model.revision();
    let incoming = Incoming {
        session,
        peer_addr,
        op_id: &op_id,
        base_revision,
        now,
    };

    if base_revision > current {
        return stale(&incoming, current);
    }
    let changes = Change::list(&payload);
    if changes.is_empty() || !changes.iter().all(Change::writes_a_field) {
        // Not something the three steps are defined for.
        if base_revision != current {
            return stale(&incoming, current);
        }
        return commit_opaque(state, drawing, &incoming, &payload);
    }

    if base_revision == current {
        // Step 3: another session's op made against this same revision is
        // still inside the window and writes one of these fields otherwise.
        let tied = drawing.held.iter().position(|held| {
            held.session.session_id != session.session_id
                && held.base_revision == base_revision
                && held
                    .changes
                    .iter()
                    .any(|theirs| changes.iter().any(|ours| collides(theirs, ours)))
        });
        if let Some(index) = tied {
            let first = drawing.held.remove(index);
            return tie(state, drawing, first, &incoming, changes);
        }
        if !state.tie_window.is_zero() {
            println!(
                "[ocs_collab_server] 入站 op session={} user={} op_id={op_id} base_revision={base_revision} -> 并列窗口内暂存 {} 毫秒 at={now}",
                session.session_id,
                session.user_id,
                state.tie_window.as_millis()
            );
            drawing.held.push(HeldOp {
                op_id,
                session: session.clone(),
                peer_addr: peer_addr.to_owned(),
                base_revision,
                changes,
                received_at: now,
                due: Instant::now() + state.tie_window,
            });
            return Outcome::default();
        }
    }
    judge_and_commit(state, drawing, &incoming, changes)
}

/// Commit every held op whose window has passed. Returns, per drawing that
/// moved, what to publish and the revision it reached.
pub fn flush_due(state: &AppState, now: Instant) -> Vec<(String, Outcome)> {
    let mut drawings = state.drawings.lock().expect("drawings mutex poisoned");
    let mut outcomes = Vec::new();
    for (drawing_id, drawing) in drawings.iter_mut() {
        while let Some(index) = drawing.held.iter().position(|held| held.due <= now) {
            let held = drawing.held.remove(index);
            let incoming = Incoming {
                session: &held.session,
                peer_addr: &held.peer_addr,
                op_id: &held.op_id,
                base_revision: held.base_revision,
                now: held.received_at,
            };
            let mut outcome = judge_and_commit(state, drawing, &incoming, held.changes.clone());
            // Nobody is on the other end of a `Send` here; address the
            // session instead.
            for reaction in &mut outcome.reactions {
                if let Reaction::Send(message) = reaction {
                    *reaction = Reaction::SendTo {
                        session_id: held.session.session_id.clone(),
                        message: message.clone(),
                    };
                }
            }
            outcomes.push((drawing_id.clone(), outcome));
        }
    }
    outcomes
}

/// [`flush_due`], with its results delivered: publishes go to the room,
/// errors to the session they are for, and the registry follows the revision.
pub async fn flush_and_deliver(state: &AppState) {
    for (drawing_id, outcome) in flush_due(state, Instant::now()) {
        if let Some(revision) = outcome.revision {
            state
                .sessions
                .lock()
                .await
                .advance_base_revision(&drawing_id, revision);
        }
        for reaction in outcome.reactions {
            match reaction {
                Reaction::Publish {
                    drawing_id,
                    message,
                } => {
                    state.bus.publish(&drawing_id, &message);
                }
                Reaction::SendTo {
                    session_id,
                    message,
                } => {
                    let sender = state
                        .direct
                        .lock()
                        .expect("direct mutex poisoned")
                        .get(&session_id)
                        .cloned();
                    if let Some(sender) = sender {
                        let _ = sender.send(message);
                    }
                }
                _ => {}
            }
        }
    }
}

/// The drawing's objects as the server knows them, for the semantic read-back
/// (`GET /drawing/{id}/objects`): `(revision, items)`, one item per object —
/// or only `object_ref`'s, when one is named.
pub fn objects(
    state: &AppState,
    drawing_id: &str,
    object_ref: Option<&str>,
) -> (i64, Vec<Value>) {
    let mut drawings = state.drawings.lock().expect("drawings mutex poisoned");
    let drawing = drawings
        .entry(drawing_id.to_owned())
        .or_insert_with(|| DrawingOps::load(state, drawing_id));
    let items = drawing
        .model
        .objects()
        .filter(|(tag, _)| object_ref.is_none_or(|wanted| wanted == tag.as_str()))
        .map(|(tag, object)| {
            let fields: serde_json::Map<String, Value> = object
                .fields
                .iter()
                .map(|(field, state)| {
                    (
                        field.clone(),
                        serde_json::json!({
                            "value": state.value,
                            "revision": state.revision,
                            "user_id": state.user_id,
                        }),
                    )
                })
                .collect();
            serde_json::json!({ "object_ref": tag, "fields": fields })
        })
        .collect();
    (drawing.model.revision(), items)
}

/// How often the flush loop looks, for a given window: often enough that an
/// op is not held noticeably longer than asked.
pub fn flush_interval(window: Duration) -> Duration {
    (window / 4).clamp(Duration::from_millis(5), Duration::from_millis(50))
}

fn stale(incoming: &Incoming<'_>, current: i64) -> Outcome {
    println!(
        "[ocs_collab_server] 入站 op session={} user={} op_id={} base_revision={} -> error {} current={current} at={}",
        incoming.session.session_id,
        incoming.session.user_id,
        incoming.op_id,
        incoming.base_revision,
        codes::STALE_BASE_REVISION,
        incoming.now
    );
    Outcome {
        reactions: vec![Reaction::Send(error(
            codes::STALE_BASE_REVISION,
            format!(
                "图面当前修订为 {current}，操作基于 {}",
                incoming.base_revision
            ),
        ))],
        revision: None,
    }
}

fn error(code: &str, message: String) -> ServerMessage {
    ServerMessage::Error {
        code: code.to_owned(),
        message,
        level: None,
        holder: None,
    }
}

/// An op the judgment does not read: taken whole at the current revision, as
/// before, and logged for replay.
fn commit_opaque(
    state: &AppState,
    drawing: &mut DrawingOps,
    incoming: &Incoming<'_>,
    payload: &Value,
) -> Outcome {
    let changes = ws::op_changes(payload);
    let rows = match &state.store {
        Some(store) => vec![ws::op_audit_row(
            store,
            incoming.session,
            payload,
            incoming.op_id,
            incoming.peer_addr,
            incoming.now,
        )],
        None => Vec::new(),
    };
    match commit(state, drawing, incoming, rows, changes) {
        Ok((revision, message)) => {
            log_applied(incoming, payload, revision);
            Outcome {
                reactions: vec![Reaction::Publish {
                    drawing_id: incoming.session.drawing_id.clone(),
                    message,
                }],
                revision: Some(revision),
            }
        }
        Err(outcome) => outcome,
    }
}

/// Steps 1 and 2 for every change of a field-writing op, then the commit of
/// the ones that take effect.
fn judge_and_commit(
    state: &AppState,
    drawing: &mut DrawingOps,
    incoming: &Incoming<'_>,
    changes: Vec<Change>,
) -> Outcome {
    let session = incoming.session;
    let mut reactions = Vec::new();
    let mut applied: Vec<(Change, String, Option<String>)> = Vec::new();
    for change in changes {
        // A held op was made against a newer revision than this one: its
        // value is the one this change is behind.
        if incoming.base_revision < drawing.model.revision() {
            if let Some((held, theirs)) = drawing.held_against(&change, &session.session_id) {
                let object_ref = drawing
                    .model
                    .current_name(&change.object_ref, incoming.base_revision);
                let winner = (
                    theirs.value.clone(),
                    drawing.model.revision() + 1,
                    held.session.user_id.clone(),
                );
                reactions.push(yielded(state, incoming, &change, &object_ref, winner));
                continue;
            }
        }
        match drawing
            .model
            .judge(&change, incoming.base_revision, &session.session_id)
        {
            Verdict::Apply {
                object_ref,
                replaces,
            } => applied.push((change, object_ref, replaces)),
            Verdict::Yield {
                object_ref,
                winner,
                winner_revision,
                winner_user,
            } => reactions.push(yielded(
                state,
                incoming,
                &change,
                &object_ref,
                (winner, winner_revision, winner_user),
            )),
        }
    }
    if applied.is_empty() {
        return Outcome {
            reactions,
            revision: None,
        };
    }

    let rows = match &state.store {
        Some(store) => applied
            .iter()
            .map(|(change, object_ref, replaces)| {
                let mut described = change.raw.clone();
                if described.get("old_value").is_none() {
                    if let Some(replaces) = replaces {
                        described["old_value"] = Value::String(replaces.clone());
                    }
                }
                ws::audit_row(
                    Some(store),
                    session,
                    &change.kind,
                    object_ref,
                    ws::describe_change(&change.kind, object_ref, &described),
                    incoming.peer_addr,
                    incoming.now,
                )
            })
            .collect(),
        None => Vec::new(),
    };
    let broadcast: Vec<Value> = applied
        .iter()
        .map(|(change, object_ref, _)| change.broadcast_form(object_ref))
        .collect();
    match commit(state, drawing, incoming, rows, broadcast) {
        Ok((revision, message)) => {
            for (change, object_ref, _) in &applied {
                drawing.model.commit(
                    change,
                    object_ref,
                    revision,
                    &session.session_id,
                    &session.user_id,
                );
            }
            log_applied(incoming, &applied[0].0.raw, revision);
            reactions.push(Reaction::Publish {
                drawing_id: session.drawing_id.clone(),
                message,
            });
            Outcome {
                reactions,
                revision: Some(revision),
            }
        }
        Err(mut outcome) => {
            reactions.append(&mut outcome.reactions);
            Outcome {
                reactions,
                revision: None,
            }
        }
    }
}

/// The revision bump, the audit rows and the op-log row as one unit, and the
/// `op_applied` to publish for it.
fn commit(
    state: &AppState,
    drawing: &mut DrawingOps,
    incoming: &Incoming<'_>,
    rows: Vec<NewAuditRow>,
    changes: Vec<Value>,
) -> Result<(i64, ServerMessage), Outcome> {
    let session = incoming.session;
    let expected = drawing.model.revision();
    let revision = match &state.store {
        Some(store) => {
            let record = OpRecord {
                op_id: incoming.op_id.to_owned(),
                session_id: session.session_id.clone(),
                user_id: session.user_id.clone(),
                base_revision: incoming.base_revision,
                at: incoming.now,
                changes: Value::Array(changes.clone()).to_string(),
            };
            match store.apply_op(&session.drawing_id, expected, &rows, Some(&record)) {
                Ok(OpCommit::Applied { revision, .. }) => revision,
                Ok(OpCommit::Stale { current }) => {
                    // The row moved without this model (another process on
                    // the same file). Take its word and have the client retry.
                    drawing.model.set_revision(current);
                    return Err(stale(incoming, current));
                }
                Err(err) => {
                    eprintln!("[ocs_collab_server] 提交操作失败：{err}");
                    return Err(Outcome {
                        reactions: vec![Reaction::Send(error(
                            codes::INVALID_MESSAGE,
                            "提交操作失败".to_owned(),
                        ))],
                        revision: None,
                    });
                }
            }
        }
        None => {
            let next = expected + 1;
            state
                .revisions
                .lock()
                .expect("revisions mutex poisoned")
                .insert(session.drawing_id.clone(), next);
            next
        }
    };
    drawing.model.set_revision(revision);
    Ok((
        revision,
        ServerMessage::OpApplied {
            op_id: incoming.op_id.to_owned(),
            revision,
            changes,
        },
    ))
}

fn log_applied(incoming: &Incoming<'_>, first: &Value, revision: i64) {
    println!(
        "[ocs_collab_server] 入站 op session={} user={} op_id={} base_revision={} kind={} object_ref={} -> 广播 op_applied revision={revision} at={}",
        incoming.session.session_id,
        incoming.session.user_id,
        incoming.op_id,
        incoming.base_revision,
        first.get("kind").and_then(Value::as_str).unwrap_or("op"),
        first.get("object_ref").and_then(Value::as_str).unwrap_or(""),
        incoming.now
    );
}

/// Step 2's bookkeeping: the audit row saying what was not overwritten, and
/// the `error` telling the sender.
fn yielded(
    state: &AppState,
    incoming: &Incoming<'_>,
    change: &Change,
    object_ref: &str,
    (winner, winner_revision, winner_user): (String, i64, String),
) -> Reaction {
    let session = incoming.session;
    let what = format!("{}.{}", object_ref, change.field);
    let text = format!(
        "{} {what}: {} 让位于 {winner}（基线 {} 落后于修订 {winner_revision}，{winner_user} 先写入）",
        change.kind, change.value, incoming.base_revision
    );
    if let Some(store) = &state.store {
        let action = if change.kind == KIND_SET_TAG {
            "tag_superseded"
        } else {
            "property_superseded"
        };
        let row = ws::audit_row(
            Some(store),
            session,
            action,
            object_ref,
            text,
            incoming.peer_addr,
            incoming.now,
        );
        if let Err(err) = store.audit_append(&row) {
            eprintln!("[ocs_collab_server] 写入让位审计失败：{err}");
        }
    }
    println!(
        "[ocs_collab_server] 入站 op session={} user={} op_id={} base_revision={} kind={} object_ref={object_ref} field={} -> error {} winner_revision={winner_revision} at={}",
        session.session_id,
        session.user_id,
        incoming.op_id,
        incoming.base_revision,
        change.kind,
        change.field,
        codes::OP_SUPERSEDED,
        incoming.now
    );
    Reaction::Send(error(
        codes::OP_SUPERSEDED,
        format!(
            "操作 {} 的基线 {} 落后：{what} 已在修订 {winner_revision} 被 {winner_user} 改为 {winner}，本次提交的 {} 未生效",
            incoming.op_id, incoming.base_revision, change.value
        ),
    ))
}

/// Step 3: `first` was held, `second` just arrived, both against one
/// revision, and they write some field differently. Neither write takes
/// effect; one conflict entry per such field, and the room is told. What else
/// the two ops carry is unaffected and commits now.
fn tie(
    state: &AppState,
    drawing: &mut DrawingOps,
    first: HeldOp,
    second: &Incoming<'_>,
    second_changes: Vec<Change>,
) -> Outcome {
    let drawing_id = second.session.drawing_id.clone();
    let first_incoming = Incoming {
        session: &first.session,
        peer_addr: &first.peer_addr,
        op_id: &first.op_id,
        base_revision: first.base_revision,
        now: first.received_at,
    };
    let mut reactions = Vec::new();
    let (mut first_rest, mut second_rest) = (Vec::new(), second_changes.clone());
    for ours in &first.changes {
        let Some(theirs) = second_changes.iter().find(|theirs| collides(ours, theirs)) else {
            first_rest.push(ours.clone());
            continue;
        };
        second_rest.retain(|change| change != theirs);
        let conflict_id = new_conflict_id();
        if let Some(store) = &state.store {
            let row = ConflictRow {
                conflict_id: conflict_id.clone(),
                drawing_id: drawing_id.clone(),
                object_ref: ours.object_ref.clone(),
                field: ours.field.clone(),
                value_a: ours.value.clone(),
                value_b: theirs.value.clone(),
                base_revision: first.base_revision,
                status: STATUS_PENDING.to_owned(),
                resolution: None,
                resolved_value: None,
                resolved_by: None,
            };
            if let Err(err) = store.insert_conflict(&row) {
                eprintln!("[ocs_collab_server] 写入冲突条目失败：{err}");
            }
            // Both attempts go on record, each under the person who made it.
            for (who, value, other) in [
                (&first_incoming, &ours.value, &theirs.value),
                (second, &theirs.value, &ours.value),
            ] {
                let text = format!(
                    "{} {}.{}: 提交 {value}，与 {other} 并列（基线 {}），待裁决 {conflict_id}",
                    ours.kind, ours.object_ref, ours.field, first.base_revision
                );
                let row = ws::audit_row(
                    Some(store),
                    who.session,
                    "conflict",
                    &ours.object_ref,
                    text,
                    who.peer_addr,
                    who.now,
                );
                if let Err(err) = store.audit_append(&row) {
                    eprintln!("[ocs_collab_server] 写入冲突审计失败：{err}");
                }
            }
        }
        println!(
            "[ocs_collab_server] 入站 op session={} user={} op_id={} base_revision={} kind={} object_ref={} field={} -> conflict {conflict_id} 与 session={} op_id={} 并列 at={}",
            second.session.session_id,
            second.session.user_id,
            second.op_id,
            second.base_revision,
            ours.kind,
            ours.object_ref,
            ours.field,
            first.session.session_id,
            first.op_id,
            second.now
        );
        reactions.push(Reaction::Publish {
            drawing_id: drawing_id.clone(),
            message: ServerMessage::Conflict {
                conflict_id,
                object_ref: ours.object_ref.clone(),
                field: ours.field.clone(),
                value_a: ours.value.clone(),
                value_b: theirs.value.clone(),
            },
        });
    }

    let mut revision = None;
    for (who, rest) in [(&first_incoming, first_rest), (second, second_rest)] {
        if rest.is_empty() {
            continue;
        }
        let mut outcome = judge_and_commit(state, drawing, who, rest);
        // `first` has no socket in this call; address its session.
        if std::ptr::eq(who, &first_incoming) {
            for reaction in &mut outcome.reactions {
                if let Reaction::Send(message) = reaction {
                    *reaction = Reaction::SendTo {
                        session_id: first.session.session_id.clone(),
                        message: message.clone(),
                    };
                }
            }
        }
        revision = outcome.revision.or(revision);
        reactions.append(&mut outcome.reactions);
    }
    Outcome {
        reactions,
        revision,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::{AuditQuery, Store};
    use crate::ws::{handle_message, Connection};
    use ocs_collab::{ClientMessage, SessionConfig};
    use serde_json::json;

    const T0: i64 = 1_700_000_000;
    const DRAWING: &str = "PID-1101";

    fn state() -> AppState {
        AppState::new(
            Some(Store::open_in_memory().expect("memory store")),
            None,
            ":memory:",
            SessionConfig::default(),
        )
    }

    /// A server started with `--tie-window-ms`.
    fn windowed() -> AppState {
        let mut state = state();
        state.tie_window = Duration::from_millis(500);
        state
    }

    async fn join(state: &AppState, user_id: &str) -> Connection {
        let mut conn = Connection::new("10.0.0.7");
        let hello = ClientMessage::Hello {
            user_id: user_id.into(),
            drawing_id: DRAWING.into(),
            mode: "协同编辑".into(),
            client_version: "test".into(),
            workspace: "plant-a".into(),
        };
        handle_message(state, &mut conn, hello, T0).await;
        assert!(conn.session.is_some(), "hello must open a session");
        conn
    }

    fn set(field: &str, value: &str) -> Value {
        json!({"kind": "set_property", "object_ref": "P-1001", "field": field, "value": value})
    }

    async fn submit(
        state: &AppState,
        conn: &mut Connection,
        base_revision: i64,
        op_id: &str,
        payload: Value,
    ) -> Vec<Reaction> {
        let session_id = conn.session.as_ref().expect("session").session_id.clone();
        let message = ClientMessage::Op {
            session_id,
            base_revision,
            op_id: op_id.into(),
            payload,
        };
        handle_message(state, conn, message, T0 + 1).await
    }

    /// `(op_id, revision, changes)` of the `op_applied` among `reactions`.
    fn applied(reactions: &[Reaction]) -> Option<(String, i64, Vec<Value>)> {
        reactions.iter().find_map(|reaction| match reaction {
            Reaction::Publish {
                message:
                    ServerMessage::OpApplied {
                        op_id,
                        revision,
                        changes,
                    },
                ..
            } => Some((op_id.clone(), *revision, changes.clone())),
            _ => None,
        })
    }

    /// `(code, message)` of every `error` sent back.
    fn errors(reactions: &[Reaction]) -> Vec<(String, String)> {
        reactions
            .iter()
            .filter_map(|reaction| match reaction {
                Reaction::Send(ServerMessage::Error { code, message, .. })
                | Reaction::SendTo {
                    message: ServerMessage::Error { code, message, .. },
                    ..
                } => Some((code.clone(), message.clone())),
                _ => None,
            })
            .collect()
    }

    fn conflicts_published(reactions: &[Reaction]) -> Vec<ServerMessage> {
        reactions
            .iter()
            .filter_map(|reaction| match reaction {
                Reaction::Publish { message, .. }
                    if matches!(message, ServerMessage::Conflict { .. }) =>
                {
                    Some(message.clone())
                }
                _ => None,
            })
            .collect()
    }

    fn conflict_rows(state: &AppState) -> Vec<ConflictRow> {
        state
            .store
            .as_ref()
            .expect("store")
            .list_conflicts(Some(DRAWING), None)
            .expect("conflicts")
    }

    /// The value the server holds for `field` of P-1001.
    fn value_of(state: &AppState, field: &str) -> Option<String> {
        let (_, items) = objects(state, DRAWING, Some("P-1001"));
        items
            .first()
            .and_then(|item| item["fields"][field]["value"].as_str().map(str::to_owned))
    }

    // `VAL-CONF-001`: s1 writes one field and gets revision 1; s2, still at
    // baseline 0, writes another field of the same object. Both stand.
    #[tokio::test]
    async fn a_write_behind_the_revision_to_a_field_nobody_else_wrote_merges() {
        let state = state();
        let mut s1 = join(&state, "u1").await;
        let mut s2 = join(&state, "u2").await;

        let first = submit(&state, &mut s1, 0, "op-1", set("设计压力", "1.6MPa")).await;
        assert_eq!(applied(&first).map(|(_, revision, _)| revision), Some(1));

        let second = submit(&state, &mut s2, 0, "op-2", set("公称直径", "DN50")).await;
        let (op_id, revision, changes) = applied(&second).expect("merged, so applied");
        assert_eq!((op_id.as_str(), revision), ("op-2", 2));
        assert_eq!(changes[0]["field"], "公称直径");
        assert!(errors(&second).is_empty(), "{second:?}");
        assert!(conflicts_published(&second).is_empty());
        assert!(conflict_rows(&state).is_empty());

        assert_eq!(value_of(&state, "设计压力").as_deref(), Some("1.6MPa"));
        assert_eq!(value_of(&state, "公称直径").as_deref(), Some("DN50"));
    }

    // `VAL-CONF-002` and `-003`: the same field. s2 is behind and yields: an
    // `error` saying so, no conflict entry, the first value stays, and the
    // audit log keeps the value that did not take effect beside the one that
    // did.
    #[tokio::test]
    async fn a_write_behind_the_revision_to_a_field_someone_else_wrote_yields() {
        let state = state();
        let mut s1 = join(&state, "u1").await;
        let mut s2 = join(&state, "u2").await;
        submit(&state, &mut s1, 0, "op-1", set("设计压力", "1.6MPa")).await;

        let second = submit(&state, &mut s2, 0, "op-2", set("设计压力", "2.5MPa")).await;
        assert!(applied(&second).is_none(), "{second:?}");
        let refused = errors(&second);
        assert_eq!(refused.len(), 1, "{second:?}");
        assert_eq!(refused[0].0, codes::OP_SUPERSEDED);
        assert!(refused[0].1.contains("1.6MPa") && refused[0].1.contains("2.5MPa"));
        assert!(conflict_rows(&state).is_empty());
        assert_eq!(value_of(&state, "设计压力").as_deref(), Some("1.6MPa"));

        let store = state.store.as_ref().expect("store");
        let page = store
            .audit_query(&AuditQuery {
                object_ref: Some("P-1001".into()),
                ..AuditQuery::default()
            })
            .expect("audit");
        let row = page
            .items
            .iter()
            .find(|row| row.action == "property_superseded")
            .expect("the yield is on record");
        assert_eq!(row.user_id, "u2");
        assert!(row.change_text.contains("2.5MPa"), "{}", row.change_text);
        assert!(row.change_text.contains("1.6MPa"), "{}", row.change_text);
        for column in [
            &row.module,
            &row.action,
            &row.object_ref,
            &row.user_id,
            &row.company,
            &row.ip,
            &row.workspace,
            &row.change_text,
        ] {
            assert!(!column.is_empty(), "{row:?}");
        }
        assert!(row.at > 0);
        // The revision did not move for a write that did not happen.
        assert_eq!(store.drawing(DRAWING).expect("drawing").expect("row").2, 1);
    }

    // `VAL-CONF-005`: the field was written at revision 3; a write carrying
    // baseline 1 yields, whatever happened in between.
    #[tokio::test]
    async fn a_write_two_revisions_behind_yields() {
        let state = state();
        let mut s1 = join(&state, "u1").await;
        let mut s2 = join(&state, "u2").await;
        submit(&state, &mut s1, 0, "op-1", set("设计压力", "1.6MPa")).await;
        submit(&state, &mut s1, 1, "op-2", set("绝热等级", "H")).await;
        let third = submit(&state, &mut s1, 2, "op-3", set("公称直径", "DN50")).await;
        assert_eq!(applied(&third).map(|(_, revision, _)| revision), Some(3));

        let behind = submit(&state, &mut s2, 1, "op-4", set("公称直径", "DN80")).await;
        assert!(applied(&behind).is_none());
        assert_eq!(errors(&behind)[0].0, codes::OP_SUPERSEDED);
        assert!(conflict_rows(&state).is_empty());
        assert_eq!(value_of(&state, "公称直径").as_deref(), Some("DN50"));
    }

    // `VAL-CONF-004`: with a tie window, two sessions write one field
    // differently against the same revision. Neither takes effect; there is
    // one entry, `value_a` the first to arrive, and the room is told.
    #[tokio::test]
    async fn two_writes_against_one_revision_inside_the_window_make_a_conflict_entry() {
        let state = windowed();
        let mut s1 = join(&state, "u1").await;
        let mut s2 = join(&state, "u2").await;

        let first = submit(&state, &mut s1, 0, "op-1", set("设计压力", "1.6MPa")).await;
        assert!(first.is_empty(), "held, so nothing yet: {first:?}");

        let second = submit(&state, &mut s2, 0, "op-2", set("设计压力", "2.5MPa")).await;
        assert!(applied(&second).is_none(), "{second:?}");
        let told = conflicts_published(&second);
        assert_eq!(told.len(), 1, "{second:?}");

        let rows = conflict_rows(&state);
        assert_eq!(rows.len(), 1);
        let row = &rows[0];
        assert_eq!(row.drawing_id, DRAWING);
        assert_eq!(row.object_ref, "P-1001");
        assert_eq!(row.field, "设计压力");
        assert_eq!(row.value_a, "1.6MPa");
        assert_eq!(row.value_b, "2.5MPa");
        assert_eq!(row.base_revision, 0);
        assert_eq!(row.status, STATUS_PENDING);
        assert_eq!(row.resolution, None);
        assert_eq!(row.resolved_by, None);
        match &told[0] {
            ServerMessage::Conflict {
                conflict_id,
                object_ref,
                field,
                value_a,
                value_b,
            } => {
                assert_eq!(conflict_id, &row.conflict_id);
                assert_eq!(
                    (object_ref.as_str(), field.as_str(), value_a.as_str(), value_b.as_str()),
                    ("P-1001", "设计压力", "1.6MPa", "2.5MPa")
                );
            }
            other => panic!("expected a conflict, got {other:?}"),
        }

        // Neither side's value is the drawing's, and the revision is where
        // it was. Letting the window pass changes nothing: the held op is
        // gone.
        assert_eq!(value_of(&state, "设计压力"), None);
        let later = flush_due(&state, Instant::now() + Duration::from_secs(5));
        assert!(later.is_empty(), "{later:?}");
        assert_eq!(objects(&state, DRAWING, None).0, 0);

        // Both attempts are on record under the people who made them.
        let store = state.store.as_ref().expect("store");
        let page = store
            .audit_query(&AuditQuery {
                object_ref: Some("P-1001".into()),
                ..AuditQuery::default()
            })
            .expect("audit");
        let mut users: Vec<&str> = page
            .items
            .iter()
            .filter(|row| row.action == "conflict")
            .map(|row| row.user_id.as_str())
            .collect();
        users.sort();
        assert_eq!(users, ["u1", "u2"]);
    }

    // The window is a wait, not a refusal: an op nobody ties with commits
    // when it passes, and its sender is told then.
    #[tokio::test]
    async fn a_held_op_commits_once_its_window_has_passed() {
        let state = windowed();
        let mut s1 = join(&state, "u1").await;
        let held = submit(&state, &mut s1, 0, "op-1", set("设计压力", "1.6MPa")).await;
        assert!(held.is_empty());
        assert!(flush_due(&state, Instant::now()).is_empty(), "not yet due");

        let due = flush_due(&state, Instant::now() + Duration::from_secs(1));
        assert_eq!(due.len(), 1);
        assert_eq!(due[0].0, DRAWING);
        assert_eq!(due[0].1.revision, Some(1));
        let (op_id, revision, _) = applied(&due[0].1.reactions).expect("applied");
        assert_eq!((op_id.as_str(), revision), ("op-1", 1));
        assert_eq!(value_of(&state, "设计压力").as_deref(), Some("1.6MPa"));
    }

    // Inside the window two sessions writing different fields tie on
    // nothing; both commit, in the order they arrived.
    #[tokio::test]
    async fn inside_the_window_writes_to_different_fields_both_commit() {
        let state = windowed();
        let mut s1 = join(&state, "u1").await;
        let mut s2 = join(&state, "u2").await;
        submit(&state, &mut s1, 0, "op-1", set("设计压力", "1.6MPa")).await;
        let second = submit(&state, &mut s2, 0, "op-2", set("公称直径", "DN50")).await;
        assert!(second.is_empty(), "held as well: {second:?}");

        let due = flush_due(&state, Instant::now() + Duration::from_secs(1));
        let revisions: Vec<(String, i64)> = due
            .iter()
            .filter_map(|(_, outcome)| applied(&outcome.reactions))
            .map(|(op_id, revision, _)| (op_id, revision))
            .collect();
        assert_eq!(revisions, [("op-1".to_owned(), 1), ("op-2".to_owned(), 2)]);
        assert!(conflict_rows(&state).is_empty());
    }

    // A write made against an older revision does not tie with a held one —
    // the held one was made against a newer revision — it yields to it.
    #[tokio::test]
    async fn a_write_behind_the_revision_yields_to_a_held_one() {
        let state = windowed();
        let mut s1 = join(&state, "u1").await;
        let mut s2 = join(&state, "u2").await;
        submit(&state, &mut s1, 0, "op-0", set("绝热等级", "H")).await;
        flush_due(&state, Instant::now() + Duration::from_secs(1));
        // s1 is at revision 1 and writes; s2 is still at 0.
        let held = submit(&state, &mut s1, 1, "op-1", set("设计压力", "1.6MPa")).await;
        assert!(held.is_empty());
        let behind = submit(&state, &mut s2, 0, "op-2", set("设计压力", "2.5MPa")).await;
        assert_eq!(errors(&behind)[0].0, codes::OP_SUPERSEDED);
        assert!(conflict_rows(&state).is_empty());
    }

    // A session's second edit, sent before its first came back, carries the
    // older baseline. It is the newer intent, not a lagging one.
    #[tokio::test]
    async fn a_sessions_second_edit_sent_before_the_first_came_back_is_taken() {
        let state = state();
        let mut s1 = join(&state, "u1").await;
        submit(&state, &mut s1, 0, "op-1", set("设计压力", "1.6MPa")).await;
        let second = submit(&state, &mut s1, 0, "op-2", set("设计压力", "2.5MPa")).await;
        assert_eq!(applied(&second).map(|(_, revision, _)| revision), Some(2));
        assert!(errors(&second).is_empty());
        assert_eq!(value_of(&state, "设计压力").as_deref(), Some("2.5MPa"));
    }

    // An op the three steps are not defined for keeps the old rule.
    #[tokio::test]
    async fn an_op_of_another_kind_behind_the_revision_is_still_refused_as_stale() {
        let state = state();
        let mut s1 = join(&state, "u1").await;
        let mut s2 = join(&state, "u2").await;
        submit(&state, &mut s1, 0, "op-1", set("设计压力", "1.6MPa")).await;
        let insert = json!({"kind": "insert_symbol", "type": "VALVE", "tag": "V-1001"});
        let behind = submit(&state, &mut s2, 0, "op-2", insert.clone()).await;
        assert_eq!(errors(&behind)[0].0, codes::STALE_BASE_REVISION);
        let current = submit(&state, &mut s2, 1, "op-3", insert).await;
        assert_eq!(applied(&current).map(|(_, revision, _)| revision), Some(2));
    }

    // One undo step is one op. What merges of it is applied and what is
    // behind yields, change by change; the revision moves once.
    #[tokio::test]
    async fn a_batched_op_applies_what_merges_and_yields_the_rest() {
        let state = state();
        let mut s1 = join(&state, "u1").await;
        let mut s2 = join(&state, "u2").await;
        submit(&state, &mut s1, 0, "op-1", set("设计压力", "1.6MPa")).await;

        let mut batch = set("设计压力", "2.5MPa");
        batch["changes"] = json!([set("公称直径", "DN50"), set("绝热等级", "H")]);
        let outcome = submit(&state, &mut s2, 0, "op-2", batch).await;
        let (_, revision, changes) = applied(&outcome).expect("two of three apply");
        assert_eq!(revision, 2);
        let fields: Vec<&str> = changes
            .iter()
            .map(|change| change["field"].as_str().unwrap_or(""))
            .collect();
        assert_eq!(fields, ["公称直径", "绝热等级"]);
        assert_eq!(errors(&outcome).len(), 1);
        assert_eq!(value_of(&state, "设计压力").as_deref(), Some("1.6MPa"));
        assert_eq!(value_of(&state, "绝热等级").as_deref(), Some("H"));
    }

    // What the server knows of a drawing is the op log. Forget the model —
    // as a restart does — and a write behind the revision is judged the same.
    #[tokio::test]
    async fn the_drawing_is_rebuilt_from_the_op_log() {
        let state = state();
        let mut s1 = join(&state, "u1").await;
        let mut s2 = join(&state, "u2").await;
        submit(&state, &mut s1, 0, "op-1", set("设计压力", "1.6MPa")).await;
        submit(
            &state,
            &mut s1,
            1,
            "op-2",
            json!({"kind": "set_tag", "object_ref": "P-1001", "value": "P-1009"}),
        )
        .await;

        state.drawings.lock().expect("drawings").clear();

        let (revision, items) = objects(&state, DRAWING, None);
        assert_eq!(revision, 2);
        assert_eq!(items.len(), 1);
        assert_eq!(items[0]["object_ref"], "P-1009");
        assert_eq!(items[0]["fields"]["设计压力"]["value"], "1.6MPa");

        // s2 never saw the retag: it still says P-1001, and is still behind
        // on the field s1 wrote.
        let behind = submit(&state, &mut s2, 0, "op-3", set("设计压力", "2.5MPa")).await;
        assert_eq!(errors(&behind)[0].0, codes::OP_SUPERSEDED);
        // A field s1 did not write merges — and goes out under the tag the
        // object has now, so that everyone can find it.
        let merged = submit(&state, &mut s2, 0, "op-4", set("公称直径", "DN50")).await;
        let (_, revision, changes) = applied(&merged).expect("merged");
        assert_eq!(revision, 3);
        assert_eq!(changes[0]["object_ref"], "P-1009");
    }

    // Without a database the judgment still runs on what this process saw.
    #[tokio::test]
    async fn without_a_database_writes_are_still_merged_and_yielded() {
        let state = AppState::new(
            None,
            Some("no database".into()),
            "missing.db",
            SessionConfig::default(),
        );
        let mut s1 = join(&state, "u1").await;
        let mut s2 = join(&state, "u2").await;
        submit(&state, &mut s1, 0, "op-1", set("设计压力", "1.6MPa")).await;
        let merged = submit(&state, &mut s2, 0, "op-2", set("公称直径", "DN50")).await;
        assert_eq!(applied(&merged).map(|(_, revision, _)| revision), Some(2));
        let behind = submit(&state, &mut s2, 0, "op-3", set("设计压力", "2.5MPa")).await;
        assert_eq!(errors(&behind)[0].0, codes::OP_SUPERSEDED);
    }

    #[test]
    fn the_flush_loop_looks_often_enough_for_the_window() {
        assert_eq!(flush_interval(Duration::from_millis(8)), Duration::from_millis(5));
        assert_eq!(flush_interval(Duration::from_millis(100)), Duration::from_millis(25));
        assert_eq!(flush_interval(Duration::from_secs(3)), Duration::from_millis(50));
    }
}
