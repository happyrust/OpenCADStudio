//! WebSocket data plane (`GET /ws`).
//!
//! A connection runs three phases: a `hello` handshake under a 10 s deadline,
//! a `select!` loop over client frames, room broadcasts and a 1 s housekeeping
//! tick, and a disconnect path that leaves the session and its claims for the
//! heartbeat timeout to reap — per appendix A.2 a dropped socket must not lose
//! the drawing's occupancy (VAL-SRV-027).
//!
//! The protocol logic lives in [`handle_message`] and [`sweep_expired`], which
//! return [`Reaction`]s instead of touching the socket; the thin
//! [`apply_reactions`] executor is the only code that needs a live `WebSocket`,
//! so the whole protocol is unit-testable without one.
//!
//! Room fan-out note: every room message — `peer_joined` included — is
//! published to the whole room, including its originator. [`should_forward`]
//! stops a client from seeing its own `peer_joined` / `peer_left`, while
//! `op_applied` and `claim_released` deliberately still reach the sender
//! (VAL-SRV-016, VAL-SRV-012). Clients therefore must ignore self-addressed
//! peer events on their side too.

use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::extract::{ConnectInfo, State};
use axum::response::Response;
use ocs_collab::protocol::{codes, release_reasons, ClaimInfo, Peer};
use ocs_collab::{
    new_session_id, now_unix, ClaimOutcome, ClientMessage, Occupancy, OpenOutcome, OpenRequest,
    ReleaseOutcome, ServerMessage, Session, SessionStatus,
};
use serde_json::Value;
use tokio::time::{interval, timeout, MissedTickBehavior};

use crate::bus::Subscription;
use crate::store::{ClaimRow, NewAuditRow, OpCommit, SessionRow, Store};
use crate::AppState;

/// How long a socket may sit before sending `hello`. Keeps dead TCP peers
/// from occupying the map of open connections forever.
const HELLO_TIMEOUT: Duration = Duration::from_secs(10);

/// Housekeeping cadence for stale-session sweeps and claim expiry.
const SWEEP_INTERVAL: Duration = Duration::from_secs(1);

/// `GET /ws` — upgrades the request and hands the socket to [`handle_socket`].
///
/// The peer address feeds the `ip` audit column; the router is served through
/// `into_make_service_with_connect_info` so `ConnectInfo` is always present.
pub async fn upgrade(
    State(state): State<Arc<AppState>>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    ws: WebSocketUpgrade,
) -> Response {
    let peer_addr = addr.ip().to_string();
    ws.on_upgrade(move |socket| handle_socket(socket, state, peer_addr))
}

/// Per-socket bookkeeping: which session the connection speaks for and the
/// peer address stamped on its audit rows.
#[derive(Debug)]
pub struct Connection {
    /// The handshaken session; `None` until `hello` succeeds. Only the
    /// immutable identity fields are read from this copy — liveness checks
    /// always go through the registry, which owns the current status.
    pub session: Option<Session>,
    /// Peer IP for audit rows (loopback when the socket had no address).
    pub peer_addr: String,
}

impl Connection {
    pub fn new(peer_addr: impl Into<String>) -> Self {
        Connection {
            session: None,
            peer_addr: peer_addr.into(),
        }
    }
}

/// One side effect the socket layer performs for a handled message. Keeping
/// the effects as data is what makes the protocol unit-testable.
#[derive(Debug, Clone, PartialEq)]
pub enum Reaction {
    /// Reply to this connection only.
    Send(ServerMessage),
    /// Deliver to one session only, wherever its socket lives.
    ///
    /// The room bus is per-drawing, so it cannot express "this peer and nobody
    /// else"; targeted messages (the takeover error) need the per-session
    /// unicast channel on `AppState::direct`.
    SendTo {
        session_id: String,
        message: ServerMessage,
    },
    /// Fan out to every subscriber of `drawing_id`, the originator included.
    Publish { drawing_id: String, message: ServerMessage },
    /// Join the drawing's broadcast room. Emitted by a successful `hello`
    /// *before* the `peer_joined` publish, so no room traffic is missed.
    Subscribe { drawing_id: String },
    /// Shut the connection down (after pending `Send`s are flushed).
    Close,
    /// Message consumed; nothing further to do.
    Nothing,
}

/// One socket's full lifecycle.
async fn handle_socket(mut socket: WebSocket, state: Arc<AppState>, peer_addr: String) {
    let mut conn = Connection::new(peer_addr);
    let mut subscription: Option<Subscription> = None;

    let handshaken = timeout(
        HELLO_TIMEOUT,
        wait_for_hello(&mut socket, &state, &mut conn, &mut subscription),
    )
    .await
    .unwrap_or(false);
    if !handshaken {
        return;
    }

    // The unicast sender is registered only now: `hello` is what produces the
    // session id. Every early return below must go through the deregistration
    // at the tail of this function, otherwise `state.direct` leaks one entry
    // per closed socket.
    let direct_session_id = conn
        .session
        .as_ref()
        .map(|session| session.session_id.clone());
    let mut direct_rx = direct_session_id.as_ref().map(|session_id| {
        let (sender, receiver) = tokio::sync::mpsc::unbounded_channel();
        state
            .direct
            .lock()
            .expect("direct mutex poisoned")
            .insert(session_id.clone(), sender);
        receiver
    });

    let mut ticker = interval(SWEEP_INTERVAL);
    ticker.set_missed_tick_behavior(MissedTickBehavior::Skip);
    loop {
        tokio::select! {
            frame = socket.recv() => {
                match frame {
                    Some(Ok(message)) => {
                        if let Message::Ping(payload) = &message {
                            if socket.send(Message::Pong(payload.clone())).await.is_err() {
                                break;
                            }
                            continue;
                        }
                        match classify(&message) {
                            Incoming::Message(client_message) => {
                                let reactions =
                                    handle_message(&state, &mut conn, client_message, now_unix())
                                        .await;
                                if !apply_reactions(
                                    &mut socket,
                                    &state,
                                    &mut subscription,
                                    reactions,
                                )
                                .await
                                {
                                    break;
                                }
                            }
                            Incoming::Bad => {
                                if !send_message(&mut socket, &invalid_message_error()).await {
                                    break;
                                }
                            }
                            Incoming::Close => break,
                            Incoming::Ignore => {}
                        }
                    }
                    Some(Err(_)) | None => break,
                }
            }
            room = async {
                match subscription.as_mut() {
                    Some(subscription) => subscription.recv().await,
                    None => std::future::pending().await,
                }
            } => {
                if let Some(message) = room {
                    let own = conn
                        .session
                        .as_ref()
                        .map(|session| session.session_id.as_str())
                        .unwrap_or_default();
                    if should_forward(own, &message) && !send_message(&mut socket, &message).await {
                        break;
                    }
                }
            }
            direct = async {
                match direct_rx.as_mut() {
                    Some(receiver) => receiver.recv().await,
                    None => std::future::pending().await,
                }
            } => {
                match direct {
                    Some(message) => {
                        if !send_message(&mut socket, &message).await {
                            break;
                        }
                    }
                    // Every sender for this session is gone (the registry entry
                    // was reaped), so nothing can be addressed here any more.
                    // Ending the loop beats spinning on a permanently ready
                    // `None`.
                    None => break,
                }
            }
            _ = ticker.tick() => {
                let reactions = sweep_expired(&state, now_unix()).await;
                if !apply_reactions(&mut socket, &state, &mut subscription, reactions).await {
                    break;
                }
            }
        }
    }

    if let Some(session_id) = &direct_session_id {
        state
            .direct
            .lock()
            .expect("direct mutex poisoned")
            .remove(session_id);
    }

    // The session and its claims survive disconnect: the heartbeat timeout,
    // not the socket close, owns teardown (VAL-SRV-027). The room still learns
    // that this peer went away (VAL-SRV-018).
    if let Some(session) = &conn.session {
        state
            .bus
            .publish(&session.drawing_id, &ServerMessage::PeerLeft(peer_of(session)));
    }
}

/// Reads frames until `hello` establishes a session. Returns `false` when the
/// peer goes away or the socket errors before that happens.
async fn wait_for_hello(
    socket: &mut WebSocket,
    state: &AppState,
    conn: &mut Connection,
    subscription: &mut Option<Subscription>,
) -> bool {
    while let Some(frame) = socket.recv().await {
        let Ok(message) = frame else { return false };
        if let Message::Ping(payload) = &message {
            if socket.send(Message::Pong(payload.clone())).await.is_err() {
                return false;
            }
            continue;
        }
        match classify(&message) {
            Incoming::Message(client_message) => {
                let reactions = handle_message(state, conn, client_message, now_unix()).await;
                let opened = conn.session.is_some();
                if !apply_reactions(socket, state, subscription, reactions).await {
                    return false;
                }
                if opened {
                    return true;
                }
            }
            Incoming::Bad => {
                if !send_message(socket, &invalid_message_error()).await {
                    return false;
                }
            }
            Incoming::Close => return false,
            Incoming::Ignore => {}
        }
    }
    false
}

/// One inbound frame, classified for the two read loops.
enum Incoming {
    Message(ClientMessage),
    /// A text/binary frame that is not a `ClientMessage`.
    Bad,
    Close,
    /// Pongs and anything else the protocol does not consume.
    Ignore,
}

fn classify(message: &Message) -> Incoming {
    match message {
        Message::Text(text) => match serde_json::from_str::<ClientMessage>(text) {
            Ok(client_message) => Incoming::Message(client_message),
            Err(_) => Incoming::Bad,
        },
        Message::Binary(bytes) => match serde_json::from_slice::<ClientMessage>(bytes) {
            Ok(client_message) => Incoming::Message(client_message),
            Err(_) => Incoming::Bad,
        },
        Message::Close(_) => Incoming::Close,
        _ => Incoming::Ignore,
    }
}

/// Executes reactions in order; returns `false` once the socket must close.
async fn apply_reactions(
    socket: &mut WebSocket,
    state: &AppState,
    subscription: &mut Option<Subscription>,
    reactions: Vec<Reaction>,
) -> bool {
    for reaction in reactions {
        match reaction {
            Reaction::Send(message) => {
                if !send_message(socket, &message).await {
                    return false;
                }
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
                match sender {
                    Some(tx) => {
                        if tx.send(message).is_err() {
                            // The receiving socket is gone; drop the stale key so
                            // the map does not accumulate dead sessions.
                            state
                                .direct
                                .lock()
                                .expect("direct mutex poisoned")
                                .remove(&session_id);
                        }
                    }
                    None => {
                        // Session already disconnected: nobody to deliver to.
                    }
                }
            }
            Reaction::Publish {
                drawing_id,
                message,
            } => {
                state.bus.publish(&drawing_id, &message);
            }
            Reaction::Subscribe { drawing_id } => {
                *subscription = Some(state.bus.subscribe(&drawing_id));
            }
            Reaction::Close => {
                let _ = socket.send(Message::Close(None)).await;
                return false;
            }
            Reaction::Nothing => {}
        }
    }
    true
}

/// Serializes and sends one server message. Returns `false` when the write
/// failed and the loop should end the connection.
async fn send_message(socket: &mut WebSocket, message: &ServerMessage) -> bool {
    match serde_json::to_string(message) {
        Ok(text) => socket.send(Message::Text(text.into())).await.is_ok(),
        Err(err) => {
            eprintln!("[ocs_collab_server] 序列化出站消息失败：{err}");
            true
        }
    }
}

/// Whether a room message should reach `own_session_id`'s socket. A client
/// never sees its own join/leave — it already knows — while everything else
/// (`op_applied`, `claim_released`, errors) reaches the originator too.
pub fn should_forward(own_session_id: &str, message: &ServerMessage) -> bool {
    match message {
        ServerMessage::PeerJoined(peer) | ServerMessage::PeerLeft(peer) => {
            peer.session_id != own_session_id
        }
        _ => true,
    }
}

/// Applies one client message to the shared state and returns what the socket
/// layer must do next. `now` is unix seconds; callers pass [`now_unix`] so the
/// function itself stays deterministic.
pub async fn handle_message(
    state: &AppState,
    conn: &mut Connection,
    message: ClientMessage,
    now: i64,
) -> Vec<Reaction> {
    if let ClientMessage::Hello {
        user_id,
        drawing_id,
        mode,
        workspace,
        ..
    } = &message
    {
        if conn.session.is_some() {
            return vec![Reaction::Send(error_message(
                codes::INVALID_MESSAGE,
                "会话已完成 hello，重复的 hello 被拒绝",
            ))];
        }
        return hello(state, conn, user_id, drawing_id, mode, workspace, now).await;
    }

    let Some(session) = conn.session.clone() else {
        return vec![Reaction::Send(error_message(
            codes::HELLO_REQUIRED,
            "请先发送 hello 建立会话",
        ))];
    };

    // The wire's session_id is advisory only: the authoritative binding is the
    // handshaken session on this connection, so one client cannot act as
    // another by guessing its session id.
    if message_session_id(&message) != session.session_id {
        return vec![Reaction::Send(error_message(
            codes::SESSION_UNKNOWN,
            "session_id 与本连接的会话不一致",
        ))];
    }

    match message {
        ClientMessage::Heartbeat { .. } => heartbeat(state, &session, now).await,
        ClientMessage::Claim { tag, .. } => claim(state, &session, tag, now).await,
        ClientMessage::Release { tag, .. } => release(state, &session, tag).await,
        ClientMessage::Op {
            base_revision,
            op_id,
            payload,
            ..
        } => op(state, &session, &conn.peer_addr, base_revision, op_id, payload, now).await,
        ClientMessage::Hello { .. } => unreachable!("hello is handled before the session check"),
    }
}

/// Opens a session for a `hello`: ensures the drawing row, applies the
/// takeover rules and answers `welcome` + `peer_joined`.
async fn hello(
    state: &AppState,
    conn: &mut Connection,
    user_id: &str,
    drawing_id: &str,
    mode: &str,
    workspace: &str,
    now: i64,
) -> Vec<Reaction> {
    if let Some(store) = &state.store {
        // A failed ensure must not block the handshake (appendix A.1); only
        // the audit writer truly needs the row.
        if let Err(err) = store.ensure_drawing(drawing_id) {
            eprintln!("[ocs_collab_server] ensure_drawing({drawing_id}) 失败：{err}");
        }
    }
    let base_revision = current_revision(state, drawing_id);
    let session_id = new_session_id();
    let outcome = {
        let mut sessions = state.sessions.lock().await;
        sessions.open(OpenRequest {
            session_id: &session_id,
            drawing_id,
            user_id,
            display_name: user_id,
            mode,
            workspace,
            base_revision,
            now,
        })
    };

    match outcome {
        OpenOutcome::Rejected { code, holder } => vec![
            Reaction::Send(ServerMessage::Error {
                code,
                message: "该图面已有活动的编辑会话".to_owned(),
                level: Some("state".to_owned()),
                holder: Some(holder),
            }),
            Reaction::Close,
        ],
        OpenOutcome::Opened {
            session,
            taken_over,
        } => {
            let mut reactions = Vec::new();
            for old_session_id in &taken_over {
                let released = state
                    .occupancy
                    .lock()
                    .await
                    .release_session(old_session_id);
                let old_session = state.sessions.lock().await.get(old_session_id).cloned();
                for occupancy in released {
                    if let Some(store) = &state.store {
                        if let Err(err) = store.delete_claim(&occupancy.tag) {
                            eprintln!("[ocs_collab_server] 清理被接管会话的占用失败：{err}");
                        }
                    }
                    reactions.push(Reaction::Publish {
                        drawing_id: occupancy.drawing_id.clone(),
                        message: ServerMessage::ClaimReleased {
                            tag: occupancy.tag,
                            reason: release_reasons::EXPIRED.to_owned(),
                        },
                    });
                }
                if let (Some(store), Some(old)) = (&state.store, &old_session) {
                    if let Err(err) = store.upsert_session(&session_row(old)) {
                        eprintln!("[ocs_collab_server] 更新被接管会话失败：{err}");
                    }
                }
                // The displaced connection learns it was taken over; actually
                // dropping that socket is driven by its own heartbeat failure.
                // Unicast on purpose: a third peer on the same drawing is not
                // part of the takeover and must not see this error.
                reactions.push(Reaction::SendTo {
                    session_id: old_session_id.clone(),
                    message: ServerMessage::Error {
                        code: codes::SESSION_TAKEN_OVER.to_owned(),
                        message: "会话已被新连接接管".to_owned(),
                        level: None,
                        holder: None,
                    },
                });
            }

            if let Some(store) = &state.store {
                if let Err(err) = store.upsert_session(&session_row(&session)) {
                    eprintln!("[ocs_collab_server] 写入会话失败：{err}");
                }
            }

            let peers = {
                let sessions = state.sessions.lock().await;
                sessions
                    .active_peers(drawing_id)
                    .iter()
                    .map(peer_of)
                    .collect()
            };
            let (claims, expired) = {
                let mut occupancy = state.occupancy.lock().await;
                occupancy.for_drawing(drawing_id, now)
            };
            // A listing request may be the first thing to notice an expired
            // claim; the room still owes a `claim_released` for it, exactly as
            // `sweep_expired` would have sent.
            reactions.extend(expired_release_reactions(state, expired));
            let claims: Vec<ClaimInfo> = claims.iter().map(claim_info_of).collect();

            conn.session = Some(session.clone());
            reactions.extend([
                Reaction::Subscribe {
                    drawing_id: drawing_id.to_owned(),
                },
                Reaction::Send(ServerMessage::Welcome {
                    session_id,
                    base_revision,
                    peers,
                    claims,
                }),
                Reaction::Publish {
                    drawing_id: drawing_id.to_owned(),
                    message: ServerMessage::PeerJoined(peer_of(&session)),
                },
            ]);
            reactions
        }
    }
}

/// Keeps the session alive and mirrors the fresh heartbeat into SQLite.
async fn heartbeat(state: &AppState, session: &Session, now: i64) -> Vec<Reaction> {
    let beat = {
        let mut sessions = state.sessions.lock().await;
        sessions.heartbeat(&session.session_id, now).cloned()
    };
    match beat {
        Ok(fresh) => {
            if let Some(store) = &state.store {
                if let Err(err) = store.upsert_session(&session_row(&fresh)) {
                    eprintln!("[ocs_collab_server] 心跳写库失败：{err}");
                }
            }
            vec![Reaction::Nothing]
        }
        // The connection stays open: the client decides whether to re-hello.
        Err(code) => vec![Reaction::Send(error_message(&code, "心跳被会话状态拒绝"))],
    }
}

/// One `claim` request against the occupancy table.
async fn claim(state: &AppState, session: &Session, tag: String, now: i64) -> Vec<Reaction> {
    if let Some(code) = session_unavailable(state, &session.session_id).await {
        return vec![Reaction::Send(error_message(&code, "会话已失效，无法占用位号"))];
    }
    let ttl = state.session_config.claim_ttl;
    let (outcome, expired) = state.occupancy.lock().await.claim(
        &tag,
        &session.drawing_id,
        &session.session_id,
        &session.user_id,
        now,
        ttl,
    );
    // Serving this request reclaimed whatever had already timed out; announce
    // each of them so a claim request cannot silently swallow an expiry that
    // the housekeeping sweep would have broadcast.
    let mut reactions = expired_release_reactions(state, expired);
    match outcome {
        ClaimOutcome::Granted(occupancy) => {
            if let Some(store) = &state.store {
                if let Err(err) = store.insert_claim(&ClaimRow {
                    tag: occupancy.tag.clone(),
                    drawing_id: occupancy.drawing_id.clone(),
                    session_id: occupancy.session_id.clone(),
                    user_id: occupancy.user_id.clone(),
                    expires_at: occupancy.expires_at,
                }) {
                    eprintln!("[ocs_collab_server] 写入占用失败：{err}");
                }
            }
            reactions.push(Reaction::Send(ServerMessage::ClaimGranted {
                tag,
                session_id: session.session_id.clone(),
            }));
            reactions
        }
        ClaimOutcome::Denied { holder, expires_at } => {
            reactions.push(Reaction::Send(ServerMessage::ClaimDenied {
                tag,
                holder,
                expires_at,
            }));
            reactions
        }
    }
}

/// Turns expired occupancy rows into the `claim_released` publishes the room
/// owes them, mirroring the store first. Shared by every path that can notice
/// an expiry (`claim`, `welcome`'s listing) so the reason string and the mirror
/// write cannot drift apart.
fn expired_release_reactions(state: &AppState, expired: Vec<Occupancy>) -> Vec<Reaction> {
    let mut reactions = Vec::with_capacity(expired.len());
    for occupancy in expired {
        if let Some(store) = &state.store {
            if let Err(err) = store.delete_claim(&occupancy.tag) {
                eprintln!("[ocs_collab_server] 清理过期占用失败：{err}");
            }
        }
        reactions.push(Reaction::Publish {
            drawing_id: occupancy.drawing_id.clone(),
            message: ServerMessage::ClaimReleased {
                tag: occupancy.tag,
                reason: release_reasons::EXPIRED.to_owned(),
            },
        });
    }
    reactions
}

/// One voluntary `release`; broadcast so the originator sees it too
/// (VAL-SRV-012).
async fn release(state: &AppState, session: &Session, tag: String) -> Vec<Reaction> {
    if let Some(code) = session_unavailable(state, &session.session_id).await {
        return vec![Reaction::Send(error_message(&code, "会话已失效，无法释放位号"))];
    }
    match state.occupancy.lock().await.release(&tag, &session.session_id) {
        ReleaseOutcome::Released => {
            if let Some(store) = &state.store {
                if let Err(err) = store.delete_claim(&tag) {
                    eprintln!("[ocs_collab_server] 删除占用失败：{err}");
                }
            }
            vec![Reaction::Publish {
                drawing_id: session.drawing_id.clone(),
                message: ServerMessage::ClaimReleased {
                    tag,
                    reason: release_reasons::RELEASED.to_owned(),
                },
            }]
        }
        ReleaseOutcome::NotHeld => vec![Reaction::Send(error_message(
            codes::INVALID_MESSAGE,
            "该会话未持有此位号",
        ))],
    }
}

/// One `op`: revision-checked, persisted, audited, then broadcast to the whole
/// room including the sender (VAL-SRV-016). Conflict detection arrives in M7;
/// this version applies every well-formed op.
///
/// The check → increment → audit insert is a single critical section inside
/// [`Store::commit_op`] (or, without a database, inside the `revisions` mutex).
/// Doing it step by step let two concurrent ops both pass the check at the same
/// revision and both increment, so the room saw two `op_applied` frames sharing
/// one revision while `drawings.revision` advanced once.
async fn op(
    state: &AppState,
    session: &Session,
    peer_addr: &str,
    base_revision: i64,
    op_id: String,
    payload: Value,
    now: i64,
) -> Vec<Reaction> {
    if let Some(code) = session_unavailable(state, &session.session_id).await {
        return vec![Reaction::Send(error_message(&code, "会话已失效，无法提交操作"))];
    }

    let revision = match &state.store {
        Some(store) => {
            let row = op_audit_row(store, session, &payload, &op_id, peer_addr, now);
            match store.commit_op(&session.drawing_id, base_revision, &row) {
                Ok(OpCommit::Applied { revision, .. }) => revision,
                Ok(OpCommit::Stale { current }) => {
                    return vec![Reaction::Send(error_message(
                        codes::STALE_BASE_REVISION,
                        &format!("图面当前修订为 {current}，操作基于 {base_revision}"),
                    ))];
                }
                Err(err) => {
                    // `commit_op` rolls the revision back when the audit insert
                    // fails, so a failed commit leaves nothing to undo here.
                    eprintln!("[ocs_collab_server] 提交操作失败：{err}");
                    return vec![Reaction::Send(error_message(
                        codes::INVALID_MESSAGE,
                        "提交操作失败",
                    ))];
                }
            }
        }
        None => {
            // Same critical section shape as `commit_op`, just in memory: read,
            // compare and insert under one lock acquisition, then drop the guard
            // before building the reaction.
            let mut revisions = state.revisions.lock().expect("revisions mutex poisoned");
            let current = revisions.get(&session.drawing_id).copied().unwrap_or(0);
            if base_revision != current {
                drop(revisions);
                return vec![Reaction::Send(error_message(
                    codes::STALE_BASE_REVISION,
                    &format!("图面当前修订为 {current}，操作基于 {base_revision}"),
                ))];
            }
            let next = current + 1;
            revisions.insert(session.drawing_id.clone(), next);
            drop(revisions);
            next
        }
    };

    vec![Reaction::Publish {
        drawing_id: session.drawing_id.clone(),
        message: ServerMessage::OpApplied {
            op_id,
            revision,
            changes: op_changes(&payload),
        },
    }]
}

/// One housekeeping pass: mark timed-out sessions stale and expire claims.
/// Returns the publishes the caller must fan out; mutations of the shared
/// state and the store mirror happen inside.
pub async fn sweep_expired(state: &AppState, now: i64) -> Vec<Reaction> {
    let stale = state.sessions.lock().await.sweep(now);
    let mut reactions = Vec::new();

    for session_id in &stale {
        let Some(session) = ({
            let sessions = state.sessions.lock().await;
            sessions.get(session_id).cloned()
        }) else {
            continue;
        };
        if let Some(store) = &state.store {
            if let Err(err) = store.upsert_session(&session_row(&session)) {
                eprintln!("[ocs_collab_server] 写入失效会话失败：{err}");
            }
        }
        let released = state.occupancy.lock().await.release_session(session_id);
        for occupancy in released {
            if let Some(store) = &state.store {
                if let Err(err) = store.delete_claim(&occupancy.tag) {
                    eprintln!("[ocs_collab_server] 清理失效会话的占用失败：{err}");
                }
            }
            reactions.push(Reaction::Publish {
                drawing_id: occupancy.drawing_id.clone(),
                message: ServerMessage::ClaimReleased {
                    tag: occupancy.tag,
                    reason: release_reasons::EXPIRED.to_owned(),
                },
            });
        }
        reactions.push(Reaction::Publish {
            drawing_id: session.drawing_id.clone(),
            message: ServerMessage::PeerLeft(peer_of(&session)),
        });
    }

    let expired = state.occupancy.lock().await.expire(now);
    for occupancy in expired {
        if let Some(store) = &state.store {
            if let Err(err) = store.delete_claim(&occupancy.tag) {
                eprintln!("[ocs_collab_server] 清理过期占用失败：{err}");
            }
        }
        reactions.push(Reaction::Publish {
            drawing_id: occupancy.drawing_id.clone(),
            message: ServerMessage::ClaimReleased {
                tag: occupancy.tag,
                reason: release_reasons::EXPIRED.to_owned(),
            },
        });
    }

    reactions
}

/// `Some(code)` when the session may no longer read or write.
async fn session_unavailable(state: &AppState, session_id: &str) -> Option<String> {
    let sessions = state.sessions.lock().await;
    match sessions.get(session_id).map(|session| session.status) {
        None => Some(codes::SESSION_UNKNOWN.to_owned()),
        Some(SessionStatus::Active) => None,
        Some(SessionStatus::Stale) => Some(codes::SESSION_EXPIRED.to_owned()),
        Some(SessionStatus::TakenOver) => Some(codes::SESSION_TAKEN_OVER.to_owned()),
    }
}

/// The drawing's committed revision: `drawings.revision` when a database is
/// attached, otherwise the in-memory counter so revision checks still work.
///
/// Only used to stamp `welcome.base_revision`; commits go through
/// [`Store::commit_op`] (or the `revisions` mutex) so they never split the
/// read from the write.
fn current_revision(state: &AppState, drawing_id: &str) -> i64 {
    match &state.store {
        Some(store) => match store.drawing(drawing_id) {
            Ok(Some((_, _, revision))) => revision,
            _ => 0,
        },
        None => state
            .revisions
            .lock()
            .expect("revisions mutex poisoned")
            .get(drawing_id)
            .copied()
            .unwrap_or(0),
    }
}

/// Normalizes an op payload into the `changes` array on `op_applied`: object
/// payloads keep every key and get the canonical fields backfilled with
/// `null` where absent; anything else is wrapped verbatim.
fn op_changes(payload: &Value) -> Vec<Value> {
    match payload {
        Value::Object(map) => {
            let mut change = map.clone();
            for key in ["kind", "object_ref", "field", "value"] {
                change.entry(key.to_owned()).or_insert(Value::Null);
            }
            vec![Value::Object(change)]
        }
        _ => vec![payload.clone()],
    }
}

fn error_message(code: &str, message: &str) -> ServerMessage {
    ServerMessage::Error {
        code: code.to_owned(),
        message: message.to_owned(),
        level: None,
        holder: None,
    }
}

fn invalid_message_error() -> ServerMessage {
    error_message(codes::INVALID_MESSAGE, "消息无法解析或不是合法的客户端消息")
}

/// The `session_id` a client message claims to speak for.
fn message_session_id(message: &ClientMessage) -> &str {
    match message {
        ClientMessage::Heartbeat { session_id }
        | ClientMessage::Claim { session_id, .. }
        | ClientMessage::Release { session_id, .. }
        | ClientMessage::Op { session_id, .. } => session_id,
        ClientMessage::Hello { .. } => "",
    }
}

fn peer_of(session: &Session) -> Peer {
    Peer {
        session_id: session.session_id.clone(),
        user_id: session.user_id.clone(),
        display_name: session.display_name.clone(),
    }
}

fn claim_info_of(occupancy: &Occupancy) -> ClaimInfo {
    ClaimInfo {
        tag: occupancy.tag.clone(),
        session_id: occupancy.session_id.clone(),
        user_id: occupancy.user_id.clone(),
        expires_at: occupancy.expires_at,
    }
}

fn session_row(session: &Session) -> SessionRow {
    SessionRow {
        session_id: session.session_id.clone(),
        drawing_id: session.drawing_id.clone(),
        user_id: session.user_id.clone(),
        mode: session.mode.clone(),
        status: session.status.as_str().to_owned(),
        base_revision: session.base_revision,
        last_heartbeat: session.last_heartbeat,
        created_at: session.created_at,
        workspace: session.workspace.clone(),
    }
}

fn value_text(value: &Value) -> String {
    match value {
        Value::String(text) => text.clone(),
        other => other.to_string(),
    }
}

/// A readable one-liner for the audit `change_text` column. Always non-empty:
/// `kind` and `object_ref` are already defaulted by the caller.
fn describe_change(kind: &str, object_ref: &str, payload: &Value) -> String {
    let field = payload
        .get("field")
        .and_then(Value::as_str)
        .filter(|field| !field.is_empty());
    let old = payload
        .get("old_value")
        .map(value_text)
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| "(未知)".to_owned());
    let new = payload
        .get("value")
        .map(value_text)
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| "(未知)".to_owned());
    match field {
        Some(field) => format!("{kind} {object_ref}.{field}: {old} -> {new}"),
        None => format!("{kind} {object_ref}"),
    }
}

/// Builds the audit row for one applied op. VAL-SRV-020 requires all nine
/// fields non-empty, so every source has a fallback.
fn op_audit_row(
    store: &Store,
    session: &Session,
    payload: &Value,
    op_id: &str,
    peer_addr: &str,
    now: i64,
) -> NewAuditRow {
    let kind = payload
        .get("kind")
        .and_then(Value::as_str)
        .filter(|kind| !kind.is_empty())
        .unwrap_or("op");
    let object_ref = payload
        .get("object_ref")
        .and_then(Value::as_str)
        .filter(|object_ref| !object_ref.is_empty())
        .or(if op_id.is_empty() {
            None
        } else {
            Some(op_id)
        })
        .unwrap_or("未知对象");
    let company = store
        .user_role(&session.user_id)
        .ok()
        .flatten()
        .map(|(_, company)| company)
        .filter(|company| !company.is_empty())
        .unwrap_or_else(|| "未知".to_owned());
    let user_id = if session.user_id.is_empty() {
        "未知用户".to_owned()
    } else {
        session.user_id.clone()
    };
    let ip = if peer_addr.is_empty() {
        "127.0.0.1".to_owned()
    } else {
        peer_addr.to_owned()
    };
    let workspace = if session.workspace.is_empty() {
        "默认工作区".to_owned()
    } else {
        session.workspace.clone()
    };
    NewAuditRow {
        module: "协同编辑".to_owned(),
        action: kind.to_owned(),
        at: now,
        object_ref: object_ref.to_owned(),
        user_id,
        company,
        ip,
        workspace,
        change_text: describe_change(kind, object_ref, payload),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::AuditQuery;
    use ocs_collab::SessionConfig;
    use serde_json::json;

    const T0: i64 = 1_700_000_000;

    fn state() -> AppState {
        AppState::new(
            Some(Store::open_in_memory().expect("memory store")),
            None,
            ":memory:",
            SessionConfig::default(),
        )
    }

    fn storeless_state() -> AppState {
        AppState::new(
            None,
            Some("no database".into()),
            "missing.db",
            SessionConfig::default(),
        )
    }

    fn store_of(state: &AppState) -> &Store {
        state.store.as_ref().expect("test state has a store")
    }

    fn conn() -> Connection {
        Connection::new("10.0.0.7")
    }

    fn hello(user_id: &str, mode: &str, workspace: &str) -> ClientMessage {
        ClientMessage::Hello {
            user_id: user_id.into(),
            drawing_id: "PID-1101".into(),
            mode: mode.into(),
            client_version: "test".into(),
            workspace: workspace.into(),
        }
    }

    fn session_id(conn: &Connection) -> String {
        conn.session
            .as_ref()
            .expect("handshaken")
            .session_id
            .clone()
    }

    fn sends(reactions: &[Reaction]) -> Vec<ServerMessage> {
        reactions
            .iter()
            .filter_map(|reaction| match reaction {
                Reaction::Send(message) => Some(message.clone()),
                _ => None,
            })
            .collect()
    }

    fn publishes(reactions: &[Reaction]) -> Vec<ServerMessage> {
        reactions
            .iter()
            .filter_map(|reaction| match reaction {
                Reaction::Publish { message, .. } => Some(message.clone()),
                _ => None,
            })
            .collect()
    }

    async fn opened_conn(state: &AppState, user_id: &str, now: i64) -> Connection {
        let mut conn = conn();
        let reactions =
            handle_message(state, &mut conn, hello(user_id, "协同编辑", "plant-a"), now).await;
        assert!(
            sends(&reactions)
                .iter()
                .any(|message| matches!(message, ServerMessage::Welcome { .. })),
            "hello must yield a welcome: {reactions:?}"
        );
        conn
    }

    #[tokio::test]
    async fn hello_opens_a_session_and_returns_welcome() {
        let state = state();
        let mut conn = conn();
        let reactions =
            handle_message(&state, &mut conn, hello("u1", "协同编辑", "plant-a"), T0).await;
        let session_id = session_id(&conn);
        assert!(
            reactions
                .iter()
                .any(|reaction| matches!(reaction, Reaction::Subscribe { drawing_id } if drawing_id == "PID-1101")),
            "hello must subscribe the room before peer_joined: {reactions:?}"
        );
        match &sends(&reactions)[0] {
            ServerMessage::Welcome {
                session_id: welcomed,
                base_revision,
                peers,
                claims,
            } => {
                assert_eq!(welcomed, &session_id);
                assert_eq!(*base_revision, 0);
                assert!(peers
                    .iter()
                    .any(|peer| peer.session_id == session_id && peer.user_id == "u1"));
                assert!(claims.is_empty());
            }
            other => panic!("expected welcome, got {other:?}"),
        }
        let published = publishes(&reactions);
        assert!(
            matches!(published.last(), Some(ServerMessage::PeerJoined(peer)) if peer.session_id == session_id),
            "the room must hear about the join: {published:?}"
        );
    }

    #[tokio::test]
    async fn welcome_lists_active_peers_and_outstanding_claims() {
        let state = state();
        let mut a = opened_conn(&state, "u1", T0).await;
        let a_id = session_id(&a);
        handle_message(
            &state,
            &mut a,
            ClientMessage::Claim {
                session_id: a_id.clone(),
                tag: "V-1001".into(),
            },
            T0,
        )
        .await;

        let mut b = opened_conn(&state, "u2", T0 + 1).await;
        let b_id = session_id(&b);
        let reactions = handle_message(
            &state,
            &mut b,
            ClientMessage::Heartbeat {
                session_id: b_id.clone(),
            },
            T0 + 2,
        )
        .await;
        assert_eq!(reactions, vec![Reaction::Nothing]);

        // Re-issue hello assertions against B's stored welcome indirectly:
        // the welcome B already received must have listed both peers and A's
        // claim. The registry + table state is the observable truth here.
        let peers = state.sessions.lock().await.active_peers("PID-1101");
        assert_eq!(peers.len(), 2);
        let (claims, _) = state
            .occupancy
            .lock()
            .await
            .for_drawing("PID-1101", T0 + 2);
        assert_eq!(claims.len(), 1);
        assert_eq!(claims[0].tag, "V-1001");
        assert_eq!(claims[0].session_id, a_id);
        assert_eq!(claims[0].user_id, "u1");
    }

    #[tokio::test]
    async fn welcome_payload_contains_peers_and_claims() {
        let state = state();
        let mut a = opened_conn(&state, "u1", T0).await;
        let a_id = session_id(&a);
        handle_message(
            &state,
            &mut a,
            ClientMessage::Claim {
                session_id: a_id.clone(),
                tag: "V-1001".into(),
            },
            T0,
        )
        .await;

        let mut b = conn();
        let reactions =
            handle_message(&state, &mut b, hello("u2", "协同编辑", "plant-b"), T0 + 1).await;
        match &sends(&reactions)[0] {
            ServerMessage::Welcome { peers, claims, .. } => {
                assert_eq!(peers.len(), 2, "both active sessions are peers");
                assert!(peers.iter().any(|peer| peer.session_id == a_id));
                assert_eq!(claims.len(), 1);
                assert_eq!(claims[0].tag, "V-1001");
                assert_eq!(claims[0].session_id, a_id);
            }
            other => panic!("expected welcome, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn a_message_before_hello_is_refused() {
        let state = state();
        let mut conn = conn();
        let reactions = handle_message(
            &state,
            &mut conn,
            ClientMessage::Claim {
                session_id: "s-whatever".into(),
                tag: "V-1001".into(),
            },
            T0,
        )
        .await;
        assert_eq!(
            sends(&reactions),
            vec![error_message(codes::HELLO_REQUIRED, "请先发送 hello 建立会话")],
            "a claim before hello must be refused, not crash"
        );
    }

    #[tokio::test]
    async fn a_repeated_hello_is_an_invalid_message() {
        let state = state();
        let mut conn = opened_conn(&state, "u1", T0).await;
        let first_session_id = session_id(&conn);
        let reactions =
            handle_message(&state, &mut conn, hello("u1", "协同编辑", "plant-a"), T0 + 1).await;
        assert_eq!(
            sends(&reactions),
            vec![error_message(
                codes::INVALID_MESSAGE,
                "会话已完成 hello，重复的 hello 被拒绝"
            )]
        );
        assert_eq!(session_id(&conn), first_session_id, "the session survives");
    }

    #[tokio::test]
    async fn single_editor_mode_rejects_the_second_session() {
        let state = state();
        let mut a = conn();
        handle_message(&state, &mut a, hello("u1", "单人编辑", ""), T0).await;
        let a_id = session_id(&a);

        let mut b = conn();
        let reactions =
            handle_message(&state, &mut b, hello("u2", "单人编辑", ""), T0 + 1).await;
        assert_eq!(
            reactions,
            vec![
                Reaction::Send(ServerMessage::Error {
                    code: codes::PERM_DENIED_STATE.to_owned(),
                    message: "该图面已有活动的编辑会话".to_owned(),
                    level: Some("state".to_owned()),
                    holder: Some(a_id),
                }),
                Reaction::Close,
            ],
            "a rejected hello must close the socket"
        );
        assert!(b.session.is_none());
    }

    #[tokio::test]
    async fn a_storeless_state_still_handshakes() {
        let state = storeless_state();
        let mut conn = conn();
        let reactions =
            handle_message(&state, &mut conn, hello("u1", "协同编辑", ""), T0).await;
        match &sends(&reactions)[0] {
            ServerMessage::Welcome { base_revision, .. } => assert_eq!(*base_revision, 0),
            other => panic!("expected welcome, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn claim_after_hello_is_granted_and_mirrored() {
        let state = state();
        let mut conn = opened_conn(&state, "u1", T0).await;
        let session_id = session_id(&conn);
        let reactions = handle_message(
            &state,
            &mut conn,
            ClientMessage::Claim {
                session_id: session_id.clone(),
                tag: "V-1001".into(),
            },
            T0,
        )
        .await;
        assert_eq!(
            sends(&reactions),
            vec![ServerMessage::ClaimGranted {
                tag: "V-1001".into(),
                session_id: session_id.clone(),
            }]
        );
        let rows = store_of(&state)
            .claims_on_drawing("PID-1101")
            .expect("claims query");
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].session_id, session_id);
        assert_eq!(rows[0].expires_at, T0 + 300);
    }

    #[tokio::test]
    async fn a_second_session_claiming_the_tag_is_denied() {
        let state = state();
        let mut a = opened_conn(&state, "u1", T0).await;
        let a_id = session_id(&a);
        handle_message(
            &state,
            &mut a,
            ClientMessage::Claim {
                session_id: a_id.clone(),
                tag: "V-1001".into(),
            },
            T0,
        )
        .await;

        let mut b = opened_conn(&state, "u2", T0 + 1).await;
        let b_id = session_id(&b);
        let reactions = handle_message(
            &state,
            &mut b,
            ClientMessage::Claim {
                session_id: b_id,
                tag: "V-1001".into(),
            },
            T0 + 1,
        )
        .await;
        assert_eq!(
            sends(&reactions),
            vec![ServerMessage::ClaimDenied {
                tag: "V-1001".into(),
                holder: a_id,
                expires_at: T0 + 300,
            }]
        );
    }

    #[tokio::test]
    async fn release_broadcasts_claim_released_to_the_room() {
        let state = state();
        let mut conn = opened_conn(&state, "u1", T0).await;
        let session_id = session_id(&conn);
        handle_message(
            &state,
            &mut conn,
            ClientMessage::Claim {
                session_id: session_id.clone(),
                tag: "V-1001".into(),
            },
            T0,
        )
        .await;
        let reactions = handle_message(
            &state,
            &mut conn,
            ClientMessage::Release {
                session_id,
                tag: "V-1001".into(),
            },
            T0 + 1,
        )
        .await;
        // VAL-SRV-012: the publisher sees its own claim_released too.
        assert_eq!(
            publishes(&reactions),
            vec![ServerMessage::ClaimReleased {
                tag: "V-1001".into(),
                reason: "released".into(),
            }]
        );
        assert!(store_of(&state)
            .claims_on_drawing("PID-1101")
            .expect("claims query")
            .is_empty());
    }

    #[tokio::test]
    async fn releasing_an_unheld_tag_is_an_error() {
        let state = state();
        let mut conn = opened_conn(&state, "u1", T0).await;
        let sid = session_id(&conn);
        let reactions = handle_message(
            &state,
            &mut conn,
            ClientMessage::Release {
                session_id: sid,
                tag: "V-9999".into(),
            },
            T0,
        )
        .await;
        assert_eq!(
            sends(&reactions),
            vec![error_message(codes::INVALID_MESSAGE, "该会话未持有此位号")]
        );
    }

    #[tokio::test]
    async fn a_foreign_session_id_is_rejected() {
        let state = state();
        let mut conn = opened_conn(&state, "u1", T0).await;
        let reactions = handle_message(
            &state,
            &mut conn,
            ClientMessage::Claim {
                session_id: "s-not-mine".into(),
                tag: "V-1001".into(),
            },
            T0,
        )
        .await;
        assert_eq!(
            sends(&reactions),
            vec![error_message(
                codes::SESSION_UNKNOWN,
                "session_id 与本连接的会话不一致"
            )]
        );
    }

    #[tokio::test]
    async fn ops_advance_the_revision_monotonically() {
        let state = state();
        let mut conn = opened_conn(&state, "u1", T0).await;
        let session_id = session_id(&conn);
        let payload = json!({
            "kind": "set_property",
            "object_ref": "V-1001",
            "field": "设计压力",
            "value": "1.6MPa",
        });
        for (index, expected) in [(0, 1_i64), (1, 2_i64)] {
            let reactions = handle_message(
                &state,
                &mut conn,
                ClientMessage::Op {
                    session_id: session_id.clone(),
                    base_revision: index,
                    op_id: format!("op-{index}"),
                    payload: payload.clone(),
                },
                T0 + index,
            )
            .await;
            let published = publishes(&reactions);
            match &published[0] {
                ServerMessage::OpApplied {
                    op_id,
                    revision,
                    changes,
                } => {
                    assert_eq!(op_id, &format!("op-{index}"));
                    assert_eq!(*revision, expected);
                    assert_eq!(changes[0]["object_ref"], "V-1001");
                    assert_eq!(changes[0]["field"], "设计压力");
                    assert_eq!(changes[0]["value"], "1.6MPa");
                    assert_eq!(changes[0]["kind"], "set_property");
                }
                other => panic!("expected op_applied, got {other:?}"),
            }
        }
        let (_, _, revision) = store_of(&state)
            .drawing("PID-1101")
            .expect("lookup")
            .expect("drawing exists");
        assert_eq!(revision, 2);
    }

    #[tokio::test]
    async fn a_stale_base_revision_is_rejected() {
        let state = state();
        let mut conn = opened_conn(&state, "u1", T0).await;
        let session_id = session_id(&conn);
        let payload = json!({"kind": "set_property", "object_ref": "V-1001"});
        handle_message(
            &state,
            &mut conn,
            ClientMessage::Op {
                session_id: session_id.clone(),
                base_revision: 0,
                op_id: "op-0".into(),
                payload: payload.clone(),
            },
            T0,
        )
        .await;
        let reactions = handle_message(
            &state,
            &mut conn,
            ClientMessage::Op {
                session_id,
                base_revision: 0,
                op_id: "op-1".into(),
                payload,
            },
            T0 + 1,
        )
        .await;
        match &sends(&reactions)[0] {
            ServerMessage::Error { code, .. } => {
                assert_eq!(code, codes::STALE_BASE_REVISION)
            }
            other => panic!("expected stale error, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn ops_work_without_a_store_via_the_in_memory_revisions() {
        let state = storeless_state();
        let mut conn = opened_conn(&state, "u1", T0).await;
        let session_id = session_id(&conn);
        let payload = json!({"kind": "set_property", "object_ref": "V-1001"});
        handle_message(
            &state,
            &mut conn,
            ClientMessage::Op {
                session_id: session_id.clone(),
                base_revision: 0,
                op_id: "op-0".into(),
                payload: payload.clone(),
            },
            T0,
        )
        .await;
        let reactions = handle_message(
            &state,
            &mut conn,
            ClientMessage::Op {
                session_id,
                base_revision: 1,
                op_id: "op-1".into(),
                payload,
            },
            T0 + 1,
        )
        .await;
        match &publishes(&reactions)[0] {
            ServerMessage::OpApplied { revision, .. } => assert_eq!(*revision, 2),
            other => panic!("expected op_applied, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn an_op_writes_a_complete_audit_row() {
        let state = state();
        let mut conn = opened_conn(&state, "u1", T0).await;
        let session_id = session_id(&conn);
        handle_message(
            &state,
            &mut conn,
            ClientMessage::Op {
                session_id,
                base_revision: 0,
                op_id: "op-1".into(),
                payload: json!({
                    "kind": "set_property",
                    "object_ref": "V-1001",
                    "field": "设计压力",
                    "value": "1.6MPa",
                }),
            },
            T0,
        )
        .await;

        let page = store_of(&state)
            .audit_query(&AuditQuery::default())
            .expect("audit query");
        assert_eq!(page.total, 1);
        let row = &page.items[0];
        // VAL-SRV-020: all nine fields non-empty.
        assert!(!row.module.is_empty());
        assert_eq!(row.action, "set_property");
        assert_eq!(row.at, T0);
        assert_eq!(row.object_ref, "V-1001");
        assert_eq!(row.user_id, "u1");
        assert_eq!(row.company, "未知", "no users table seeded");
        assert_eq!(row.ip, "10.0.0.7");
        assert_eq!(row.workspace, "plant-a");
        assert_eq!(
            row.change_text,
            "set_property V-1001.设计压力: (未知) -> 1.6MPa"
        );
    }

    #[tokio::test]
    async fn a_heartbeat_on_a_stale_session_reports_expired() {
        let state = state();
        let mut conn = opened_conn(&state, "u1", T0).await;
        let session_id = session_id(&conn);
        sweep_expired(&state, T0 + 91).await;
        let reactions = handle_message(
            &state,
            &mut conn,
            ClientMessage::Heartbeat { session_id },
            T0 + 92,
        )
        .await;
        match &sends(&reactions)[0] {
            ServerMessage::Error { code, .. } => assert_eq!(code, codes::SESSION_EXPIRED),
            other => panic!("expected expired error, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn sweeping_a_stale_session_releases_claims_and_broadcasts_peer_left() {
        let state = state();
        let mut conn = opened_conn(&state, "u1", T0).await;
        let session_id = session_id(&conn);
        for tag in ["V-1001", "V-1002"] {
            handle_message(
                &state,
                &mut conn,
                ClientMessage::Claim {
                    session_id: session_id.clone(),
                    tag: tag.into(),
                },
                T0,
            )
            .await;
        }

        let reactions = sweep_expired(&state, T0 + 91).await;
        let published = publishes(&reactions);
        assert_eq!(published.len(), 3);
        assert!(matches!(&published[0], ServerMessage::ClaimReleased { tag, reason } if tag == "V-1001" && reason == "expired"));
        assert!(matches!(&published[1], ServerMessage::ClaimReleased { tag, reason } if tag == "V-1002" && reason == "expired"));
        assert!(matches!(&published[2], ServerMessage::PeerLeft(peer) if peer.session_id == session_id));

        assert!(store_of(&state)
            .claims_on_drawing("PID-1101")
            .expect("claims query")
            .is_empty());
        let row = store_of(&state)
            .session_row(&session_id)
            .expect("lookup")
            .expect("session row");
        assert_eq!(row.status, "失效");
    }

    #[tokio::test]
    async fn an_expired_claim_broadcasts_claim_released() {
        let state = state();
        let mut conn = opened_conn(&state, "u1", T0).await;
        let session_id = session_id(&conn);
        handle_message(
            &state,
            &mut conn,
            ClientMessage::Claim {
                session_id: session_id.clone(),
                tag: "V-1001".into(),
            },
            T0,
        )
        .await;
        // Keep the session alive past the claim TTL (300 s).
        handle_message(
            &state,
            &mut conn,
            ClientMessage::Heartbeat {
                session_id: session_id.clone(),
            },
            T0 + 250,
        )
        .await;
        let reactions = sweep_expired(&state, T0 + 301).await;
        assert_eq!(
            publishes(&reactions),
            vec![ServerMessage::ClaimReleased {
                tag: "V-1001".into(),
                reason: "expired".into(),
            }]
        );
        assert!(store_of(&state)
            .claims_on_drawing("PID-1101")
            .expect("claims query")
            .is_empty());
    }

    #[tokio::test]
    async fn takeover_releases_the_old_sessions_claims_and_notifies_the_room() {
        let state = state();
        let mut a = opened_conn(&state, "u1", T0).await;
        let a_id = session_id(&a);
        handle_message(
            &state,
            &mut a,
            ClientMessage::Claim {
                session_id: a_id.clone(),
                tag: "V-1001".into(),
            },
            T0,
        )
        .await;
        // A goes stale; B's hello on the same drawing+mode takes it over.
        let reactions = sweep_expired(&state, T0 + 91).await;
        assert_eq!(publishes(&reactions).len(), 2, "claim release + peer left");

        let mut b = conn();
        let reactions =
            handle_message(&state, &mut b, hello("u2", "协同编辑", ""), T0 + 92).await;
        // The takeover error is targeted: it names A and nobody else. It must
        // not ride the room bus, or every peer on the drawing would see an
        // error that has nothing to do with them.
        assert!(
            reactions.iter().any(|reaction| matches!(reaction,
                Reaction::SendTo { session_id, message }
                    if session_id == &a_id
                        && matches!(message, ServerMessage::Error { code, .. }
                            if code == codes::SESSION_TAKEN_OVER))),
            "the displaced session must be told directly: {reactions:?}"
        );
        assert!(
            !reactions.iter().any(|reaction| matches!(reaction,
                Reaction::Publish { message: ServerMessage::Error { code, .. }, .. }
                    if code == codes::SESSION_TAKEN_OVER)),
            "the takeover error must not be published to the room: {reactions:?}"
        );
        assert!(state
            .occupancy
            .lock()
            .await
            .for_drawing("PID-1101", T0 + 93)
            .0
            .is_empty());
    }

    #[tokio::test]
    async fn takeover_error_does_not_reach_a_third_peer() {
        let state = state();
        let a = opened_conn(&state, "u1", T0).await;
        let a_id = session_id(&a);
        // D is an unrelated active peer on the same drawing throughout; it must
        // never be handed the takeover error meant for A.
        let mut d = opened_conn(&state, "u4", T0).await;
        let d_id = session_id(&d);
        assert_ne!(a_id, d_id);

        // Only A goes stale (D keeps heartbeating).
        let _ = handle_message(
            &state,
            &mut d,
            ClientMessage::Heartbeat {
                session_id: d_id.clone(),
            },
            T0 + 90,
        )
        .await;
        sweep_expired(&state, T0 + 91).await;

        let mut b = conn();
        let reactions =
            handle_message(&state, &mut b, hello("u2", "协同编辑", ""), T0 + 92).await;

        let targeted: Vec<&String> = reactions
            .iter()
            .filter_map(|reaction| match reaction {
                Reaction::SendTo { session_id, message } => match message {
                    ServerMessage::Error { code, .. } if code == codes::SESSION_TAKEN_OVER => {
                        Some(session_id)
                    }
                    _ => None,
                },
                _ => None,
            })
            .collect();
        assert_eq!(
            targeted,
            vec![&a_id],
            "exactly one takeover error, addressed to A: {reactions:?}"
        );
        assert!(
            !reactions.iter().any(|reaction| matches!(reaction,
                Reaction::Publish { message: ServerMessage::Error { code, .. }, .. }
                    if code == codes::SESSION_TAKEN_OVER)),
            "no publish may carry the takeover error: {reactions:?}"
        );
    }

    #[test]
    fn unparseable_frames_map_to_invalid_message() {
        assert!(serde_json::from_str::<ClientMessage>("{not json").is_err());
        assert!(
            serde_json::from_str::<ClientMessage>(r#"{"type":"totally_unknown"}"#).is_err()
        );
        match invalid_message_error() {
            ServerMessage::Error { code, .. } => assert_eq!(code, codes::INVALID_MESSAGE),
            other => panic!("expected error, got {other:?}"),
        }
    }

    #[test]
    fn own_peer_events_are_not_forwarded() {
        let own = Peer {
            session_id: "s-1".into(),
            user_id: "u1".into(),
            display_name: "u1".into(),
        };
        let other = Peer {
            session_id: "s-2".into(),
            user_id: "u2".into(),
            display_name: "u2".into(),
        };
        assert!(!should_forward("s-1", &ServerMessage::PeerJoined(own.clone())));
        assert!(!should_forward("s-1", &ServerMessage::PeerLeft(own)));
        assert!(should_forward("s-1", &ServerMessage::PeerJoined(other.clone())));
        assert!(should_forward("s-1", &ServerMessage::PeerLeft(other)));
        assert!(should_forward(
            "s-1",
            &ServerMessage::OpApplied {
                op_id: "op-1".into(),
                revision: 1,
                changes: vec![],
            }
        ));
        assert!(should_forward(
            "s-1",
            &ServerMessage::ClaimReleased {
                tag: "V-1001".into(),
                reason: "released".into(),
            }
        ));
    }
}
