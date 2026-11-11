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
use std::sync::{Arc, Mutex, OnceLock};
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

/// Reconnect backoff: 200 ms doubling to a 5 s ceiling, matching the
/// documented retry ladder.
const BACKOFF_START: Duration = Duration::from_millis(200);
const BACKOFF_MAX: Duration = Duration::from_secs(5);

/// How long one wait-for-retry slice sleeps before re-checking whether the
/// subscription is still alive.
const BACKOFF_SLICE: Duration = Duration::from_millis(50);

/// How long [`emit`] waits before offering the UI an event again when its
/// channel was full.
const EMIT_RETRY: Duration = Duration::from_millis(2);

/// The outbound queue: requests the UI wants on the wire, oldest first.
type Outbox = Mutex<std::collections::VecDeque<Outbound>>;

/// What one transport thread works with: where its session parameters come
/// from, the requests waiting to go out, the signal that tells it to quit, and
/// its heartbeat cadence.
///
/// The app's client is wired to the two process-wide slots ([`Link::app`]):
/// iced starts the subscription worker as a bare `fn`, so a slot is the only
/// way its parameters can reach it. Those slots are the app client's and
/// nobody else's. In a test binary every app that gets built republishes its
/// (empty) config to them and any test may queue a claim on them, so a client
/// is always handed its link, and one that is not the app's gets a link that
/// shares nothing ([`Link::detached`]).
#[derive(Clone)]
pub(super) struct Link {
    config: Arc<Mutex<ClientConfig>>,
    outbox: Arc<Outbox>,
    /// Raised by [`shutdown`]. Unlike a dropped subscription it asks the
    /// thread to say goodbye on the wire. It belongs to this link alone: it
    /// used to be one process-wide flag that `shutdown` set and nothing
    /// cleared, which read as "the app is quitting" to every client started
    /// after the first quit.
    stop: Arc<AtomicBool>,
    heartbeat: Duration,
}

impl Link {
    /// The link of the app's own client: the published config, the queue the
    /// UI fills, and a quit signal of its own.
    pub(super) fn app() -> Self {
        Self {
            config: Arc::clone(app_config()),
            outbox: Arc::clone(app_outbox()),
            stop: Arc::new(AtomicBool::new(false)),
            heartbeat: HEARTBEAT_INTERVAL,
        }
    }

    /// A link that shares nothing with the app's client.
    #[cfg(test)]
    fn detached(config: ClientConfig) -> Self {
        Self {
            config: Arc::new(Mutex::new(config)),
            outbox: Arc::new(Mutex::new(std::collections::VecDeque::new())),
            stop: Arc::new(AtomicBool::new(false)),
            heartbeat: HEARTBEAT_INTERVAL,
        }
    }

    /// The session parameters in effect right now.
    fn config(&self) -> ClientConfig {
        self.config
            .lock()
            .map(|slot| slot.clone())
            .unwrap_or_default()
    }

    /// Whether this client was asked to quit.
    fn stop_requested(&self) -> bool {
        self.stop.load(Ordering::Relaxed)
    }
}

/// What [`shutdown`] needs to end the app's transport thread politely: the
/// thread, and the link whose quit signal that thread watches.
struct Worker {
    handle: std::thread::JoinHandle<()>,
    link: Link,
}

/// The app's transport thread, kept so [`shutdown`] can ask it to say goodbye
/// and wait until it has.
static WORKER: Mutex<Option<Worker>> = Mutex::new(None);

/// Records the thread [`shutdown`] stops and waits on, with the link it was
/// started on. Called from the subscription worker, which is the only place
/// that spawns it.
pub(super) fn register_worker(handle: std::thread::JoinHandle<()>, link: Link) {
    if let Ok(mut slot) = WORKER.lock() {
        *slot = Some(Worker { handle, link });
    }
}

/// Asks the transport to close its socket and waits for the thread to finish.
///
/// Unlike a dropped subscription, this asks the thread to say goodbye on the
/// wire: a process that just dies leaves the server waiting out the heartbeat
/// timeout, while a `Close` frame retires the session at once (`VAL-CLI-016`).
///
/// With no transport thread running there is nobody to say goodbye for, and
/// nothing is left behind for a later client to trip over.
///
/// Returns once the worker is gone or the budget elapses. The budget exists so
/// a wedged socket cannot hang the quit path; the thread's 100 ms read timeout
/// means the normal case returns almost immediately.
pub(super) fn shutdown() {
    // Taken out of the slot, so the join does not leak on a later restart of
    // the subscription (iced may rebuild it if the window reopens).
    let Some(worker) = WORKER.lock().ok().and_then(|mut slot| slot.take()) else {
        return;
    };
    log("退出：请求关闭协同连接");
    stop_worker(worker, SHUTDOWN_BUDGET);
}

/// How long [`shutdown`] waits for the transport thread to say goodbye.
const SHUTDOWN_BUDGET: Duration = Duration::from_secs(3);

/// Ask `worker` to say goodbye and wait up to `budget` for it to finish.
/// Returns whether it did.
///
/// A thread that has not finished by then is left behind, not joined: it is
/// in a name lookup or a connect with a longer timeout of its own, and a join
/// would wait that out with the quit behind it. The process is exiting and
/// the thread goes with it.
fn stop_worker(worker: Worker, budget: Duration) -> bool {
    worker.link.stop.store(true, Ordering::Relaxed);
    let deadline = Instant::now() + budget;
    while !worker.handle.is_finished() && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(10));
    }
    if !worker.handle.is_finished() {
        log("退出：协同线程没有在预算内结束，不再等它");
        return false;
    }
    let _ = worker.handle.join();
    true
}

/// Session config published for the app's client. The worker is spawned by
/// iced with no arguments, so the value travels through here.
fn app_config() -> &'static Arc<Mutex<ClientConfig>> {
    static SLOT: OnceLock<Arc<Mutex<ClientConfig>>> = OnceLock::new();
    SLOT.get_or_init(|| Arc::new(Mutex::new(ClientConfig::default())))
}

/// Publish the session parameters the app's transport thread should use.
pub(super) fn set_client_config(config: ClientConfig) {
    publish_config(app_config(), config);
}

/// Replace what a config slot holds; its client reads it on the next attempt.
fn publish_config(slot: &Mutex<ClientConfig>, config: ClientConfig) {
    if let Ok(mut slot) = slot.lock() {
        *slot = config;
    }
}

/// One outbound request the UI wants on the wire. The transport thread owns the
/// socket, so the UI never writes to it directly: it enqueues here and the read
/// loop drains the queue between reads (the 100 ms read timeout is the tick).
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) enum Outbound {
    /// `claim`: ask the server for exclusive ownership of `tag`.
    Claim { tag: String },
    /// `release`: give `tag` back.
    Release { tag: String },
    /// `op`: submit one P&ID semantic change. `base_revision` is captured when
    /// the request is queued, not when it is sent, so a request that waits
    /// behind a broadcast still states what the client believed.
    Op {
        base_revision: i64,
        op_id: String,
        payload: serde_json::Value,
    },
    /// `attempt`: a write this session refused itself, for the server's
    /// record.
    Attempt { object_ref: String, text: String },
}

impl Outbound {
    /// How the request reads in a message ("claim P-1001").
    pub(super) fn describe(&self) -> String {
        match self {
            Outbound::Claim { tag } => format!("claim {tag}"),
            Outbound::Release { tag } => format!("release {tag}"),
            Outbound::Attempt { object_ref, .. } => format!("attempt object_ref={object_ref}"),
            Outbound::Op {
                op_id,
                base_revision,
                ..
            } => format!("op {op_id} base_revision={base_revision}"),
        }
    }
}

/// One line on stderr, stamped with the wall clock in milliseconds.
///
/// What the client did on the wire is otherwise only visible from the far end
/// — and not at all while the far end is down, which is exactly when the
/// retry ladder's cadence has to be read off something (`VAL-CLI-014`).
fn log(line: &str) {
    let at = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|elapsed| elapsed.as_millis())
        .unwrap_or_default();
    eprintln!("[collab] t={at} {line}");
}

/// How much of the traffic the client writes to stderr.
///
/// The lifecycle — connecting, dropped, retrying, quitting — is always
/// written: it is what tells a person why nothing is arriving. The frames
/// are another matter, and `OCS_COLLAB_LOG` decides: `quiet` writes nothing
/// per frame; `frames` writes every frame as it was on the wire, next to
/// the one-line reading of it; anything else, or nothing, writes the
/// readings alone (what it has always done).
///
/// The wire text is for runs that keep evidence. A reading is this client's
/// account of a frame, and an account can be wrong where the frame is not.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum LogLevel {
    Quiet,
    Events,
    Frames,
}

/// The level `OCS_COLLAB_LOG` names, read as leniently as an environment
/// variable set by hand deserves.
fn level_named(value: Option<&str>) -> LogLevel {
    match value.map(|value| value.trim().to_ascii_lowercase()).as_deref() {
        Some("quiet" | "off" | "0") => LogLevel::Quiet,
        Some("frames" | "frame" | "2") => LogLevel::Frames,
        _ => LogLevel::Events,
    }
}

/// The level this process logs at, read once.
fn log_level() -> LogLevel {
    static LEVEL: OnceLock<LogLevel> = OnceLock::new();
    *LEVEL.get_or_init(|| level_named(std::env::var("OCS_COLLAB_LOG").ok().as_deref()))
}

/// What is written for one frame at `level`: the frame itself, in the
/// direction it travelled, when frames are kept; then the reading of it,
/// when it has one and readings are written.
fn frame_lines(level: LogLevel, inbound: bool, wire: &str, reading: Option<&str>) -> Vec<String> {
    let mut lines = Vec::new();
    if level >= LogLevel::Frames {
        lines.push(format!("帧 {} {wire}", if inbound { "<-" } else { "->" }));
    }
    if level >= LogLevel::Events {
        lines.extend(reading.map(str::to_owned));
    }
    lines
}

/// Write what [`frame_lines`] gives for one frame at this process's level.
fn log_frame(inbound: bool, wire: &str, reading: Option<&str>) {
    for line in frame_lines(log_level(), inbound, wire, reading) {
        log(&line);
    }
}

/// Outbound queue of the app's client, shared between the UI thread
/// (producer) and the socket thread (consumer). A `Mutex<VecDeque>` is enough:
/// claim traffic is human-paced, never a hot path.
fn app_outbox() -> &'static Arc<Outbox> {
    static OUTBOX: OnceLock<Arc<Outbox>> = OnceLock::new();
    OUTBOX.get_or_init(|| Arc::new(Mutex::new(std::collections::VecDeque::new())))
}

fn push_outbound(outbox: &Outbox, request: Outbound) {
    if let Ok(mut queue) = outbox.lock() {
        queue.push_back(request);
    }
}

/// Take everything queued since the last drain.
fn drain_outbound(outbox: &Outbox) -> Vec<Outbound> {
    outbox
        .lock()
        .map(|mut queue| queue.drain(..).collect())
        .unwrap_or_default()
}

/// Forget queued requests. Done when a session starts, so a reconnect never
/// replays a claim the user made against the old session.
fn clear_outbound(outbox: &Outbox) {
    if let Ok(mut queue) = outbox.lock() {
        queue.clear();
    }
}

/// Put a `claim` for `tag` on `outbox`. An empty tag never gets there.
fn queue_claim(outbox: &Outbox, tag: &str) -> Result<(), String> {
    if tag.trim().is_empty() {
        return Err("位号为空".to_owned());
    }
    push_outbound(
        outbox,
        Outbound::Claim {
            tag: tag.trim().to_owned(),
        },
    );
    Ok(())
}

/// Put a `release` for `tag` on `outbox`. An empty tag never gets there.
fn queue_release(outbox: &Outbox, tag: &str) -> Result<(), String> {
    if tag.trim().is_empty() {
        return Err("位号为空".to_owned());
    }
    push_outbound(
        outbox,
        Outbound::Release {
            tag: tag.trim().to_owned(),
        },
    );
    Ok(())
}

/// Queue a `claim` for `tag` for the app's client. The `session_id` is checked
/// against the live session by the caller (`State::request_claim`), so the
/// queue itself only carries the intent.
pub(super) fn request_claim(session_id: &str, tag: &str) -> Result<(), String> {
    let _ = session_id;
    queue_claim(app_outbox(), tag)
}

/// Queue a `release` for `tag` for the app's client.
pub(super) fn request_release(session_id: &str, tag: &str) -> Result<(), String> {
    let _ = session_id;
    queue_release(app_outbox(), tag)
}

/// Monotonic counter behind [`next_op_id`]. The id only has to be unique
/// within this process's session; the server echoes it back verbatim.
static OP_SEQUENCE: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);

/// A fresh `op_id` for an op of `session_id`. The room broadcast of our own
/// change comes back with this id, which is how the app tells its own op from
/// a peer's — so it has to be the room's only one. The session id is the
/// server's, unique among everyone on it; a process id and a counter are
/// only unique on this machine, and two machines count from 1 alike.
pub(super) fn next_op_id(session_id: &str) -> String {
    let n = OP_SEQUENCE.fetch_add(1, Ordering::Relaxed);
    format!("op-{session_id}-{n}")
}

/// Put an `op` carrying one semantic change on `outbox`. `base_revision` is
/// the revision the change was made against (`VAL-CLI-010`). An op with no id
/// never gets there: the id is how its echo is told from a peer's change.
fn queue_op(
    outbox: &Outbox,
    base_revision: i64,
    op_id: &str,
    payload: serde_json::Value,
) -> Result<(), String> {
    if op_id.trim().is_empty() {
        return Err("操作 id 为空".to_owned());
    }
    push_outbound(
        outbox,
        Outbound::Op {
            base_revision,
            op_id: op_id.to_owned(),
            payload,
        },
    );
    Ok(())
}

/// Queue an `op` for the app's client.
pub(super) fn request_op(
    session_id: &str,
    base_revision: i64,
    op_id: &str,
    payload: serde_json::Value,
) -> Result<(), String> {
    let _ = session_id;
    queue_op(app_outbox(), base_revision, op_id, payload)
}

/// Queue an `attempt` for the app's client: a write the session refused
/// itself, reported so the server can put it on record.
pub(super) fn request_attempt(session_id: &str, object_ref: &str, text: &str) {
    let _ = session_id;
    push_outbound(
        app_outbox(),
        Outbound::Attempt {
            object_ref: object_ref.to_owned(),
            text: text.to_owned(),
        },
    );
}

/// Turn one queued request into the wire message for `session_id`.
fn outbound_message(request: &Outbound, session_id: &str) -> ClientMessage {
    match request {
        Outbound::Attempt { object_ref, text } => ClientMessage::Attempt {
            session_id: session_id.to_owned(),
            object_ref: object_ref.clone(),
            text: text.clone(),
        },
        Outbound::Claim { tag } => ClientMessage::Claim {
            session_id: session_id.to_owned(),
            tag: tag.clone(),
        },
        Outbound::Release { tag } => ClientMessage::Release {
            session_id: session_id.to_owned(),
            tag: tag.clone(),
        },
        Outbound::Op {
            base_revision,
            op_id,
            payload,
        } => ClientMessage::Op {
            session_id: session_id.to_owned(),
            base_revision: *base_revision,
            op_id: op_id.clone(),
            payload: payload.clone(),
        },
    }
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

/// How one server frame reads in the log: its wire name and what it is about.
fn describe_event(event: &CollabEvent) -> String {
    match event {
        CollabEvent::Connected {
            session_id,
            base_revision,
            peers,
            claims,
        } => format!(
            "welcome session={session_id} base_revision={base_revision} peers={} claims={}",
            peers.len(),
            claims.len()
        ),
        CollabEvent::PeerJoined(peer) => format!(
            "peer_joined session={} user={}",
            peer.session_id, peer.user_id
        ),
        CollabEvent::PeerLeft { session_id } => format!("peer_left session={session_id}"),
        CollabEvent::ClaimGranted { tag } => format!("claim_granted tag={tag}"),
        CollabEvent::ClaimDenied {
            tag,
            holder,
            expires_at,
        } => format!("claim_denied tag={tag} holder={holder} expires_at={expires_at}"),
        CollabEvent::ClaimReleased { tag, reason } => {
            format!("claim_released tag={tag} reason={reason}")
        }
        CollabEvent::ReadonlyDowngrade { reason, holder, .. } => format!(
            "readonly_downgrade holder={} reason={reason}",
            holder.as_deref().unwrap_or("(无)")
        ),
        CollabEvent::OpApplied {
            op_id,
            revision,
            changes,
        } => format!(
            "op_applied op_id={op_id} revision={revision} changes={}",
            changes.len()
        ),
        CollabEvent::ServerError { code, message } => format!("error code={code} {message}"),
        // Not frames: the transport makes these up itself.
        CollabEvent::Connecting { .. } | CollabEvent::Disconnected { .. } => {
            "(not a server frame)".to_owned()
        }
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
/// `TrySendError::Full` means the UI thread is behind. For a status line
/// (`Connecting`) dropping it is strictly better than blocking the socket
/// thread. For anything that changes what the session is — a peer's change,
/// the welcome, a claim, a downgrade — it is not: the UI's picture, and for
/// `op_applied` the drawing itself, would be short of something the server
/// counts as delivered. Those wait for room — for as long as there is
/// somebody to deliver to: a quit ends the wait (the UI thread is then
/// waiting on this one and will not read the channel again), and so does a
/// cancelled subscription.
/// `TrySendError::Disconnected` means iced cancelled the subscription, so the
/// thread must exit or it leaks.
fn emit(
    sender: &mut mpsc::Sender<CollabEvent>,
    link: &Link,
    event: CollabEvent,
) -> Result<(), ()> {
    let droppable = matches!(event, CollabEvent::Connecting { .. });
    let mut event = event;
    loop {
        match sender.try_send(event) {
            Ok(()) => return Ok(()),
            Err(error) if error.is_disconnected() => return Err(()),
            Err(_full) if droppable => return Ok(()),
            Err(full) => {
                if link.stop_requested() {
                    return Err(());
                }
                event = full.into_inner();
                std::thread::sleep(EMIT_RETRY);
            }
        }
    }
}

/// Sleep out a retry delay, aborting early when the subscription disappears or
/// the app starts quitting.
fn wait_before_retry(
    sender: &mpsc::Sender<CollabEvent>,
    delay: Duration,
    link: &Link,
) -> Result<(), ()> {
    let deadline = Instant::now() + delay;
    while Instant::now() < deadline {
        if sender.is_closed() || link.stop_requested() {
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
///
/// Everything the client reads or drains comes through `link`: its session
/// parameters, its outbound queue, and its quit signal ([`shutdown`] raises the
/// one of the link it was registered with). Once that is set the session says
/// goodbye on the wire and the client does not reconnect.
pub(super) fn run_client(
    sender: &mut mpsc::Sender<CollabEvent>,
    link: &Link,
) -> std::io::Result<()> {
    let mut attempt = 0u32;
    loop {
        if link.stop_requested() || sender.is_closed() {
            return Ok(());
        }
        let config = link.config();
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
        if emit(sender, link, CollabEvent::Connecting { attempt }).is_err() {
            return Ok(());
        }
        log(&format!(
            "连接 {}（第 {} 次尝试）",
            super::shown_address(&config.server_url),
            attempt.saturating_add(1)
        ));
        let mut came_up = false;
        let reason = match run_session(sender, &config, link, &mut came_up) {
            Ok(Ended::SubscriptionGone) => return Ok(()),
            Ok(Ended::Quit) => return Ok(()),
            Ok(Ended::Closed(reason)) => reason,
            Err(error) => error,
        };
        // The ladder counts failures in a row. A session that came up ends
        // the row: the next drop starts from the first rung again, instead
        // of every drop after the fifth waiting out the full five seconds.
        if came_up {
            attempt = 0;
        }
        let delay = backoff_delay(attempt);
        log(&format!("断开：{reason}；{} 毫秒后重连", delay.as_millis()));
        if emit(
            sender,
            link,
            CollabEvent::Disconnected {
                reason,
                will_retry: true,
            },
        )
        .is_err()
        {
            return Ok(());
        }
        if wait_before_retry(sender, delay, link).is_err() {
            return Ok(());
        }
        attempt = attempt.saturating_add(1);
    }
}

/// One connection: handshake, `hello`, then the read loop until it ends.
/// `came_up` is set once the server has answered with `welcome`.
fn run_session(
    sender: &mut mpsc::Sender<CollabEvent>,
    config: &ClientConfig,
    link: &Link,
    came_up: &mut bool,
) -> Result<Ended, String> {
    // A request queued against a session that has since ended must not be
    // replayed into this one: the tag may belong to somebody else by now.
    clear_outbound(&link.outbox);
    let mut ws = connect(&config.server_url)?;
    let hello = ClientMessage::Hello {
        user_id: config.user_id.clone(),
        drawing_id: config.drawing_id.clone(),
        mode: config.mode.clone(),
        client_version: config.client_version.clone(),
        workspace: String::new(),
    };
    let wire = send_json(&mut ws, &hello)?;
    log_frame(false, &wire, None);

    let mut last_heartbeat = Instant::now();
    let mut session_id: Option<String> = None;

    loop {
        if link.stop_requested() {
            // The app is quitting: say goodbye on the wire so the server
            // retires the session now instead of after the 90 s timeout.
            close(&mut ws);
            log("退出：已发送 close 帧");
            return Ok(Ended::Quit);
        }
        if sender.is_closed() {
            close(&mut ws);
            return Ok(Ended::SubscriptionGone);
        }
        match ws.read() {
            Ok(Message::Text(text)) => {
                let frame = classify(text.as_str());
                let reading = match &frame {
                    Frame::Event(event) => Some(format!("收到 {}", describe_event(event))),
                    _ => None,
                };
                log_frame(true, text.as_str(), reading.as_deref());
                match frame {
                    Frame::Event(event) => {
                        // `welcome` carries the session id every later frame
                        // references, so remember it before forwarding.
                        if let CollabEvent::Connected {
                            session_id: id, ..
                        } = &event
                        {
                            session_id = Some(id.clone());
                            *came_up = true;
                        }
                        if emit(sender, link, event).is_err() {
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
                        log(&format!("无法解析服务器帧：{error}"));
                    }
                }
            }
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

        // Drain the outbound queue: claim / release requests the UI queued
        // since the last tick. Sending here (rather than from the UI thread)
        // keeps every write to the socket on the thread that owns it.
        if let Some(session_id) = session_id.clone() {
            for request in drain_outbound(&link.outbox) {
                let message = outbound_message(&request, &session_id);
                match send_json(&mut ws, &message) {
                    Ok(wire) => {
                        let reading = format!("发送 {}", request.describe());
                        log_frame(false, &wire, Some(&reading));
                    }
                    Err(error) => {
                        // A failed write means the socket is gone; the retry
                        // ladder takes over and the queued request is dropped
                        // with it.
                        let reason = format!("{} 发送失败：{error}", request.describe());
                        close(&mut ws);
                        return Ok(Ended::Closed(reason));
                    }
                }
            }
        }

        if last_heartbeat.elapsed() >= link.heartbeat {
            let Some(session_id) = session_id.clone() else {
                continue;
            };
            let beat = ClientMessage::Heartbeat {
                session_id: session_id.clone(),
            };
            match send_json(&mut ws, &beat) {
                Ok(wire) => log_frame(false, &wire, None),
                Err(error) => {
                    let reason = format!("heartbeat failed: {error}");
                    close(&mut ws);
                    return Ok(Ended::Closed(reason));
                }
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

/// Serialise one client message and flush it. Returns the text that went
/// out, for the frame log.
fn send_json<S>(ws: &mut WebSocket<S>, message: &ClientMessage) -> Result<String, String>
where
    S: std::io::Read + Write,
{
    let text = serde_json::to_string(message).map_err(|error| format!("序列化失败：{error}"))?;
    ws.send(Message::text(text.clone()))
        .map_err(|error| format!("发送失败：{error}"))?;
    Ok(text)
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
    use std::net::TcpListener;

    /// The parameters every test client joins with, pointed at `server_url`.
    fn config_for(server_url: String) -> ClientConfig {
        ClientConfig {
            server_url,
            user_id: "u1".to_owned(),
            drawing_id: "PID-1101".to_owned(),
            mode: "协同编辑".to_owned(),
            client_version: "test".to_owned(),
        }
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

    /// A server that takes `sessions` connections one after the other, answers
    /// each `hello` with `welcome` and then drops the socket; after the last
    /// one nobody listens any more.
    fn dropping_server(sessions: usize) -> (String, std::thread::JoinHandle<()>) {
        let listener = TcpListener::bind(("127.0.0.1", 0)).expect("bind loopback");
        let addr = listener.local_addr().expect("local addr");
        let url = format!("ws://{addr}/ws");
        let handle = std::thread::spawn(move || {
            for _ in 0..sessions {
                let Ok((stream, _)) = listener.accept() else {
                    return;
                };
                let Ok(mut ws) = tungstenite::accept(stream) else {
                    continue;
                };
                let _ = ws.read();
                let welcome = r#"{"type":"welcome","session_id":"s-test","base_revision":0,"peers":[],"claims":[]}"#;
                let _ = ws.send(Message::text(welcome));
                // Dropped here, with no close frame: a server that died.
            }
        });
        (url, handle)
    }

    /// The retry ladder counts failures in a row, and a session that came up
    /// ends the row. It used to count every drop of the run: after a handful
    /// of them each new one waited out the full five seconds, however long
    /// the sessions in between had lasted.
    #[test]
    fn a_session_that_came_up_starts_the_retry_ladder_again() {
        let (url, server) = dropping_server(2);
        let link = Link::detached(config_for(url));
        let (mut receiver, worker) = start_client(&link);

        // Which retry the first try after each session that was up is. The
        // very first connection is try 0; a retry after one failure is 1.
        let mut after_a_session = Vec::new();
        let mut was_up = false;
        let deadline = Instant::now() + Duration::from_secs(20);
        while after_a_session.len() < 2 && Instant::now() < deadline {
            match receiver.try_recv() {
                Ok(CollabEvent::Connected { .. }) => was_up = true,
                Ok(CollabEvent::Connecting { attempt }) if was_up => {
                    after_a_session.push(attempt);
                    was_up = false;
                }
                Ok(_) => {}
                Err(mpsc::TryRecvError::Closed) => break,
                Err(mpsc::TryRecvError::Empty) => std::thread::sleep(Duration::from_millis(5)),
            }
        }
        drop(receiver);
        let _ = worker.join();
        let _ = server.join();
        assert_eq!(
            after_a_session,
            [1, 1],
            "each drop of a session that was up is followed by the first retry \
             again (it read [1, 2]: the second drop went on up the ladder)"
        );
    }

    /// How long a test waits for the client's session to come up. A loopback
    /// handshake takes a few milliseconds and the wait ends the moment
    /// `Connected` arrives, so the bound is only ever spent on a failure — and
    /// it is wide enough that a busy machine cannot turn a slow start into one.
    const SESSION_WAIT: Duration = Duration::from_secs(10);

    /// Start a client on `link`, on a thread of its own.
    fn start_client(link: &Link) -> (mpsc::Receiver<CollabEvent>, std::thread::JoinHandle<()>) {
        let (mut sender, receiver) = mpsc::channel::<CollabEvent>(64);
        let link = link.clone();
        let worker = std::thread::spawn(move || {
            let _ = run_client(&mut sender, &link);
        });
        (receiver, worker)
    }

    /// Wait until the client reports its session. `false` once
    /// [`SESSION_WAIT`] has run out, or as soon as the client thread is gone
    /// (its end of the channel dropped) without ever having connected.
    fn session_came_up(receiver: &mut mpsc::Receiver<CollabEvent>) -> bool {
        let started = Instant::now();
        loop {
            match receiver.try_recv() {
                Ok(CollabEvent::Connected { .. }) => {
                    eprintln!("[transport-test] session after {:?}", started.elapsed());
                    return true;
                }
                Ok(_) => {}
                Err(mpsc::TryRecvError::Closed) => return false,
                Err(mpsc::TryRecvError::Empty) => {
                    if started.elapsed() >= SESSION_WAIT {
                        return false;
                    }
                    std::thread::sleep(Duration::from_millis(10));
                }
            }
        }
    }

    /// VAL-CLI-002: the read loop really emits `heartbeat` frames on its
    /// cadence, and every one carries the session id from `welcome`.
    #[test]
    fn the_session_emits_heartbeats_carrying_its_session_id() {
        let (url, seen, _closed, server) = recording_server();
        // A cadence short enough to watch several beats go by without sitting
        // through a real minute; the app's client always runs on
        // `HEARTBEAT_INTERVAL`.
        let mut link = Link::detached(config_for(url));
        link.heartbeat = Duration::from_millis(120);

        let (mut receiver, worker) = start_client(&link);
        assert!(
            session_came_up(&mut receiver),
            "the client never reported a session"
        );

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
        let _ = server.join();
    }

    /// VAL-CLI-016: quitting closes the socket instead of vanishing, and the
    /// thread is joined rather than leaked.
    #[test]
    fn quitting_sends_a_close_frame_and_joins_the_thread() {
        let (url, _seen, closed, server) = recording_server();
        let link = Link::detached(config_for(url));

        let (mut receiver, worker) = start_client(&link);
        assert!(
            session_came_up(&mut receiver),
            "the client never reported a session"
        );
        // Registered once the session is up: the registration slot is the one
        // the whole process shares, so a test elsewhere that quits an app takes
        // whatever is in it. From here on that would only say this client's
        // goodbye early, which is the very thing asserted below.
        register_worker(worker, link);

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
    }

    /// A quit earlier in the process must not stop a client that starts later.
    ///
    /// Tests elsewhere in this binary walk the quit path (`EXIT` at the end of
    /// a batch run lands in `exit_app()`), and `exit_app()` says goodbye to
    /// the collaboration server on its way out. With no transport thread
    /// running there is nobody to say goodbye for, so nothing may be left
    /// behind that a later client would read as "the app is quitting".
    #[test]
    fn a_quit_with_no_transport_thread_leaves_a_later_client_alone() {
        let mut app = crate::app::OpenCADStudio::new_for_test();
        let _ = app.exit_app();

        let (url, server) = quiet_server();
        let link = Link::detached(config_for(url));
        let (mut receiver, worker) = start_client(&link);
        assert!(
            session_came_up(&mut receiver),
            "the client never reported a session"
        );

        drop(receiver);
        let _ = worker.join();
        let _ = server.join();
    }

    /// What an app publishes for its own client must not reach anybody
    /// else's.
    ///
    /// Every app that applies a config republishes its collaboration settings
    /// to the app client's slot, and with collaboration unconfigured that is
    /// an empty address — which a client reads as "wait until one is
    /// configured". A client on a link of its own keeps the address it was
    /// started with, however many apps come and go meanwhile.
    #[test]
    fn what_an_app_publishes_does_not_reach_a_client_on_its_own_link() {
        let (url, server) = quiet_server();
        let link = Link::detached(config_for(url));

        // What `apply_config` does in every app this binary builds.
        crate::app::OpenCADStudio::new_for_test()
            .collab
            .sync_client_config();
        set_client_config(ClientConfig::default());

        let (mut receiver, worker) = start_client(&link);
        assert!(
            session_came_up(&mut receiver),
            "the client never reported a session"
        );

        drop(receiver);
        let _ = worker.join();
        let _ = server.join();
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
        // And that cadence is the one the app's client is started on.
        assert_eq!(Link::app().heartbeat, HEARTBEAT_INTERVAL);
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
    fn a_full_channel_drops_a_status_event_rather_than_blocking() {
        // futures' bounded channel holds `size` messages plus the slot the
        // sender owns, so fill it by sending until it refuses rather than
        // assuming an exact capacity.
        let link = Link::detached(ClientConfig::default());
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
        assert!(emit(&mut sender, &link, CollabEvent::Connecting { attempt }).is_ok());
    }

    /// The three levels, and what each writes for a frame: nothing; the
    /// one-line reading; the frame as it was on the wire and then the
    /// reading. A frame with no reading — a heartbeat, a `hello` — is
    /// written only where frames are.
    #[test]
    fn a_frame_is_written_by_the_level_the_log_is_at() {
        assert_eq!(level_named(None), LogLevel::Events, "what it has always done");
        assert_eq!(level_named(Some("")), LogLevel::Events);
        assert_eq!(level_named(Some("events")), LogLevel::Events);
        assert_eq!(level_named(Some(" Quiet ")), LogLevel::Quiet);
        assert_eq!(level_named(Some("off")), LogLevel::Quiet);
        assert_eq!(level_named(Some("FRAMES")), LogLevel::Frames);

        let wire = r#"{"type":"op_applied","op_id":"op-1","revision":2,"changes":[{"value":"2.5MPa"}]}"#;
        let reading = "收到 op_applied op_id=op-1 revision=2 changes=1";
        assert!(frame_lines(LogLevel::Quiet, true, wire, Some(reading)).is_empty());
        assert_eq!(frame_lines(LogLevel::Events, true, wire, Some(reading)), [reading]);
        assert_eq!(
            frame_lines(LogLevel::Frames, true, wire, Some(reading)),
            [format!("帧 <- {wire}"), reading.to_owned()],
            "the frame goes in as it was, not as it was read"
        );

        let beat = r#"{"type":"heartbeat","session_id":"s-1"}"#;
        assert!(frame_lines(LogLevel::Events, false, beat, None).is_empty());
        assert_eq!(frame_lines(LogLevel::Frames, false, beat, None), [format!("帧 -> {beat}")]);
    }

    /// What is published to a link's config slot is what its client reads on
    /// its next attempt.
    #[test]
    fn the_published_config_round_trips_through_the_slot() {
        let link = Link::detached(ClientConfig::default());
        assert!(link.config().server_url.is_empty());
        publish_config(
            &link.config,
            ClientConfig {
                server_url: "ws://127.0.0.1:3240/ws".to_owned(),
                user_id: "u1".to_owned(),
                drawing_id: "PID-1101".to_owned(),
                mode: "协同编辑".to_owned(),
                client_version: "test".to_owned(),
            },
        );
        let config = link.config();
        assert_eq!(config.server_url, "ws://127.0.0.1:3240/ws");
        assert_eq!(config.user_id, "u1");
        assert_eq!(config.drawing_id, "PID-1101");
    }

    /// End-to-end: a real socket, a real `welcome`, then the subscription is
    /// cancelled and the thread must notice within its 100 ms read timeout.
    /// This is the "no leak, no hang" contract of the transport.
    #[test]
    fn cancelling_the_subscription_ends_the_thread_promptly() {
        let (url, server) = quiet_server();
        let link = Link::detached(config_for(url));

        // Wait for the session to come up, so the read loop is definitely
        // spinning on its timeout when the receiver goes away.
        let (mut receiver, worker) = start_client(&link);
        assert!(
            session_came_up(&mut receiver),
            "the client never reported a session"
        );

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

    /// VAL-CLI-004 / 007: the queued `claim` and `release` reach the wire as
    /// the protocol's frames, carrying the session id from `welcome`.
    #[test]
    fn queued_claims_and_releases_reach_the_wire_with_the_session_id() {
        let (url, seen, _closed, server) = recording_server();
        let link = Link::detached(config_for(url));

        let (mut receiver, worker) = start_client(&link);
        assert!(
            session_came_up(&mut receiver),
            "the client never reported a session"
        );

        queue_claim(&link.outbox, "P-1001").expect("claim queues");
        queue_release(&link.outbox, "P-1001").expect("release queues");

        let deadline = Instant::now() + Duration::from_secs(2);
        let frames = loop {
            let frames: Vec<String> = seen.lock().expect("frame log").clone();
            let claim = frames
                .iter()
                .any(|frame| frame.contains("\"type\":\"claim\""));
            let release = frames
                .iter()
                .any(|frame| frame.contains("\"type\":\"release\""));
            if (claim && release) || Instant::now() >= deadline {
                break frames;
            }
            std::thread::sleep(Duration::from_millis(20));
        };
        let claim = frames
            .iter()
            .find(|frame| frame.contains("\"type\":\"claim\""))
            .unwrap_or_else(|| panic!("no claim frame on the wire: {frames:?}"));
        assert!(claim.contains("P-1001"), "{claim}");
        assert!(claim.contains("s-test"), "{claim}");
        let release = frames
            .iter()
            .find(|frame| frame.contains("\"type\":\"release\""))
            .unwrap_or_else(|| panic!("no release frame on the wire: {frames:?}"));
        assert!(release.contains("P-1001"), "{release}");
        assert!(release.contains("s-test"), "{release}");

        drop(receiver);
        let _ = worker.join();
        let _ = server.join();
    }

    /// A session that ends before its queued request is sent must not carry the
    /// request into the next session: the tag may belong to somebody else by
    /// then.
    #[test]
    fn a_new_session_starts_with_an_empty_outbox() {
        let outbox = Outbox::default();
        queue_claim(&outbox, "P-1001").expect("claim queues");
        clear_outbound(&outbox);
        assert!(drain_outbound(&outbox).is_empty());
    }

    /// A request queued before the session exists never reaches the wire: the
    /// session clears the queue as it starts.
    #[test]
    fn a_request_queued_before_the_session_is_not_replayed_into_it() {
        let (url, seen, _closed, server) = recording_server();
        let link = Link::detached(config_for(url));
        queue_claim(&link.outbox, "P-0001").expect("claim queues");

        let (mut receiver, worker) = start_client(&link);
        assert!(
            session_came_up(&mut receiver),
            "the client never reported a session"
        );
        // One queued after the session is up does go out; once it is on the
        // wire, anything the session was going to send from before is too.
        queue_claim(&link.outbox, "P-1001").expect("claim queues");
        let deadline = Instant::now() + Duration::from_secs(2);
        let frames = loop {
            let frames: Vec<String> = seen.lock().expect("frame log").clone();
            if frames.iter().any(|frame| frame.contains("P-1001")) || Instant::now() >= deadline {
                break frames;
            }
            std::thread::sleep(Duration::from_millis(20));
        };
        assert!(
            frames.iter().any(|frame| frame.contains("P-1001")),
            "the claim made in the session never went out: {frames:?}"
        );
        assert!(
            !frames.iter().any(|frame| frame.contains("P-0001")),
            "a claim from before the session was replayed into it: {frames:?}"
        );

        drop(receiver);
        let _ = worker.join();
        let _ = server.join();
    }

    #[test]
    fn an_empty_tag_is_refused_before_it_reaches_the_queue() {
        let outbox = Outbox::default();
        assert!(queue_claim(&outbox, "   ").is_err());
        assert!(queue_release(&outbox, "").is_err());
        assert!(drain_outbound(&outbox).is_empty());
        // The app's own entry points refuse it the same way.
        assert!(request_claim("s-1", "   ").is_err());
        assert!(request_release("s-1", "").is_err());
    }

    /// VAL-CLI-009 / 010: a queued `op` reaches the wire as the protocol's
    /// frame — the session id from `welcome`, the baseline it was queued
    /// against, its id, and the payload untouched.
    #[test]
    fn a_queued_op_reaches_the_wire_with_the_baseline_it_was_queued_against() {
        let (url, seen, _closed, server) = recording_server();
        let link = Link::detached(config_for(url));

        let (mut receiver, worker) = start_client(&link);
        assert!(
            session_came_up(&mut receiver),
            "the client never reported a session"
        );

        let payload = serde_json::json!({
            "kind": "set_tag",
            "object_ref": "P-1001",
            "field": "tag",
            "value": "P-1009",
        });
        queue_op(&link.outbox, 7, "op-test-1", payload.clone()).expect("op queues");

        let deadline = Instant::now() + Duration::from_secs(2);
        let frame = loop {
            let found = seen
                .lock()
                .expect("frame log")
                .iter()
                .find(|frame| frame.contains("\"type\":\"op\""))
                .cloned();
            if found.is_some() || Instant::now() >= deadline {
                break found;
            }
            std::thread::sleep(Duration::from_millis(20));
        };
        let frame = frame.expect("no op frame on the wire");
        let sent: serde_json::Value = serde_json::from_str(&frame).expect("the frame is JSON");
        assert_eq!(sent["session_id"], "s-test", "{frame}");
        assert_eq!(sent["base_revision"], 7, "{frame}");
        assert_eq!(sent["op_id"], "op-test-1", "{frame}");
        assert_eq!(sent["payload"], payload, "{frame}");

        drop(receiver);
        let _ = worker.join();
        let _ = server.join();
    }

    /// An op with no id could not be told from a peer's when it comes back, so
    /// it never gets as far as the queue; and every id handed out is new.
    #[test]
    fn an_op_needs_an_id_and_every_id_is_new() {
        let outbox = Outbox::default();
        assert!(queue_op(&outbox, 0, "  ", serde_json::json!({})).is_err());
        assert!(drain_outbound(&outbox).is_empty());
        assert!(request_op("s-1", 0, "", serde_json::json!({})).is_err());

        let first = next_op_id("s-1");
        let second = next_op_id("s-1");
        assert!(!first.is_empty());
        assert_ne!(first, second);
    }

    /// An id is the room's only one, not this machine's: it carries the
    /// session, which the server hands out to one connection. Two editors
    /// with the same process id counting from the same number must not make
    /// the same id — one would take the other's op for its own echo.
    #[test]
    fn two_sessions_never_make_the_same_op_id() {
        let mine = next_op_id("s-aaaa");
        let theirs = next_op_id("s-bbbb");
        assert!(mine.contains("s-aaaa"), "{mine}");
        // Whatever their counter says, the session part differs.
        let n = mine.rsplit('-').next().expect("counter");
        assert_ne!(mine, format!("op-s-bbbb-{n}"));
        assert!(theirs.starts_with("op-s-bbbb-"), "{theirs}");
    }

    /// A status line may be dropped when the UI is behind. A peer's change
    /// may not: the baseline moves with it, so dropping it leaves the drawing
    /// short a change it claims to have. It waits for room instead.
    #[test]
    fn a_peers_change_waits_for_room_in_the_channel_instead_of_being_dropped() {
        let link = Link::detached(ClientConfig::default());
        let (mut sender, mut receiver) = mpsc::channel::<CollabEvent>(1);
        while sender
            .try_send(CollabEvent::Connecting { attempt: 0 })
            .is_ok()
        {}
        // Full: a status event is given up without blocking.
        assert!(emit(&mut sender, &link, CollabEvent::Connecting { attempt: 1 }).is_ok());

        let change = CollabEvent::OpApplied {
            op_id: "op-peer".to_owned(),
            revision: 1,
            changes: Vec::new(),
        };
        let waiting = std::thread::spawn(move || emit(&mut sender, &link, change));
        std::thread::sleep(Duration::from_millis(60));
        assert!(!waiting.is_finished(), "it must wait, not drop");

        let deadline = Instant::now() + Duration::from_secs(5);
        let mut arrived = false;
        while Instant::now() < deadline && !arrived {
            match receiver.try_next() {
                Ok(Some(CollabEvent::OpApplied { op_id, .. })) => arrived = op_id == "op-peer",
                Ok(Some(_)) => {}
                Ok(None) => break,
                Err(_) => std::thread::sleep(Duration::from_millis(5)),
            }
        }
        assert!(arrived, "the change never reached the UI");
        assert_eq!(waiting.join().expect("emit thread"), Ok(()));
    }

    /// With nobody left to listen the wait ends: the thread must not hang on
    /// a subscription that is gone.
    #[test]
    fn a_waiting_change_gives_up_when_the_subscription_is_gone() {
        let link = Link::detached(ClientConfig::default());
        let (mut sender, receiver) = mpsc::channel::<CollabEvent>(1);
        while sender
            .try_send(CollabEvent::Connecting { attempt: 0 })
            .is_ok()
        {}
        let change = CollabEvent::OpApplied {
            op_id: "op-peer".to_owned(),
            revision: 1,
            changes: Vec::new(),
        };
        let waiting = std::thread::spawn(move || emit(&mut sender, &link, change));
        std::thread::sleep(Duration::from_millis(30));
        drop(receiver);
        assert_eq!(waiting.join().expect("emit thread"), Err(()));
    }

    /// The quit waits for the transport thread only as long as its budget. A
    /// thread that is not done by then — here one that never looks at the
    /// quit signal — is left behind; joining it would hold the quit for as
    /// long as whatever it is stuck in takes.
    #[test]
    fn a_wedged_worker_does_not_hold_the_quit_past_its_budget() {
        let link = Link::detached(ClientConfig::default());
        let (release, wedged) = std::sync::mpsc::channel::<()>();
        let handle = std::thread::spawn(move || {
            // Stuck until the test lets go, whatever the quit signal says.
            let _ = wedged.recv();
        });
        let worker = Worker {
            handle,
            link: link.clone(),
        };
        // Returning at all is the assertion: a join here would never come back.
        assert!(!stop_worker(worker, Duration::from_millis(50)));
        assert!(link.stop_requested(), "it was asked to stop");
        drop(release);
    }

    /// One that does stop is waited for and joined.
    #[test]
    fn a_worker_that_stops_is_joined_within_the_budget() {
        let link = Link::detached(ClientConfig::default());
        let handle = {
            let link = link.clone();
            std::thread::spawn(move || {
                while !link.stop_requested() {
                    std::thread::sleep(Duration::from_millis(5));
                }
            })
        };
        assert!(stop_worker(Worker { handle, link }, Duration::from_secs(30)));
    }

    /// Quitting ends the wait as well. The UI thread asks the transport to
    /// stop and then waits for it; it will not read the channel again, so a
    /// thread still offering it an event would hold the quit up for good.
    #[test]
    fn a_waiting_change_gives_up_when_the_app_is_quitting() {
        let link = Link::detached(ClientConfig::default());
        let (mut sender, _receiver) = mpsc::channel::<CollabEvent>(1);
        while sender
            .try_send(CollabEvent::Connecting { attempt: 0 })
            .is_ok()
        {}
        let change = CollabEvent::OpApplied {
            op_id: "op-peer".to_owned(),
            revision: 1,
            changes: Vec::new(),
        };
        let waiting = {
            let link = link.clone();
            std::thread::spawn(move || emit(&mut sender, &link, change))
        };
        std::thread::sleep(Duration::from_millis(30));
        assert!(!waiting.is_finished(), "nobody has read the channel");
        link.stop.store(true, Ordering::Relaxed);
        assert_eq!(waiting.join().expect("emit thread"), Err(()));
    }

    /// What the log says of an op is the two things `VAL-CLI-010` reads off
    /// it: which op, and the baseline it claims.
    #[test]
    fn an_op_describes_itself_by_id_and_baseline() {
        let op = Outbound::Op {
            base_revision: 1,
            op_id: "op-9-3".to_owned(),
            payload: serde_json::json!({}),
        };
        assert_eq!(op.describe(), "op op-9-3 base_revision=1");
    }
}
