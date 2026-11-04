//! axum HTTP control plane.
//!
//! The scaffold serves the endpoints later milestones build on: `/health`,
//! `/session`, and a read-only `/audit`. `PUT` / `PATCH` / `DELETE` on `/audit`
//! answer `405` because the audit log is append-only, and so does every
//! method on `/audit/{seq}`.

use std::sync::Arc;

use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::routing::{any, get, post};
use axum::{Json, Router};
use ocs_collab::protocol::codes;
use ocs_collab::{DrawingState, Operation};
use serde::{Deserialize, Serialize};
use serde_json::json;
use uuid::Uuid;

use crate::perm;
use crate::store::{AuditQuery, SessionRow};
use crate::ws;
use crate::AppState;

/// Builds the router. Integration tests use this same function.
pub fn router(state: Arc<AppState>) -> Router {
    Router::new()
        .route("/health", get(health))
        .route("/session", post(create_session))
        .route("/ws", get(ws::upgrade))
        .route(
            "/audit",
            get(audit_list)
                .put(audit_immutable)
                .patch(audit_immutable)
                .delete(audit_immutable),
        )
        .route("/audit/{seq}", any(audit_immutable))
        .route("/permission/check", post(permission_check))
        .route("/drawing/{id}/state", post(drawing_state))
        .with_state(state)
}

/// `GET /health` → 200 `{"ok":true,"db":"ok","version":"..."}`, or 503 when the
/// database is unusable (appendix A.1).
async fn health(State(state): State<Arc<AppState>>) -> Response {
    if state.db_healthy() {
        (
            StatusCode::OK,
            Json(json!({ "ok": true, "db": "ok", "version": state.version })),
        )
            .into_response()
    } else {
        (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(json!({ "ok": false, "db": "error", "version": state.version })),
        )
            .into_response()
    }
}

#[derive(Debug, Deserialize)]
struct CreateSessionBody {
    user_id: String,
    drawing_id: String,
    #[serde(default = "default_mode")]
    mode: String,
    #[serde(default)]
    workspace: String,
}

fn default_mode() -> String {
    "协同编辑".to_owned()
}

#[derive(Debug, Serialize)]
struct CreateSessionResponse {
    session_id: String,
    base_revision: i64,
}

/// `POST /session` → 201 `{session_id, base_revision}`.
///
/// Two rules beyond persistence: on a `单人编辑` drawing a non-`只读` request
/// is refused with 403 + `PERM_DENIED_STATE` + `holder` while somebody else's
/// editing session is still live, and the endpoint is idempotent for a
/// `(user_id, drawing_id)` pair — the session id is deterministic, so a repost
/// upserts the same row (`ON CONFLICT` refreshes `last_heartbeat`) instead of
/// stacking a duplicate session.
async fn create_session(
    State(state): State<Arc<AppState>>,
    Json(body): Json<CreateSessionBody>,
) -> Response {
    let Some(store) = state.store.clone() else {
        return error_response(StatusCode::SERVICE_UNAVAILABLE, codes::SESSION_UNKNOWN, "数据库不可用");
    };

    if let Err(err) = store.ensure_drawing(&body.drawing_id) {
        return error_response(StatusCode::INTERNAL_SERVER_ERROR, codes::UNKNOWN_DRAWING, &err);
    }

    let (base_revision, drawing_state) = match store.drawing(&body.drawing_id) {
        Ok(Some((_, state, revision))) => (revision, DrawingState::parse(&state)),
        Ok(None) => (0, None),
        Err(err) => {
            return error_response(StatusCode::INTERNAL_SERVER_ERROR, codes::UNKNOWN_DRAWING, &err)
        }
    };

    let now = chrono::Utc::now().timestamp();

    // The single-editor cap: somebody else's live editing session holds a
    // 单人编辑 drawing. A failed query is only logged — never a panic and
    // never a verdict flip — the same fail-open stance `active_editors_on`
    // takes.
    if drawing_state == Some(DrawingState::SingleEditor) && body.mode != "只读" {
        match store.active_editing_sessions(
            &body.drawing_id,
            now,
            state.session_config.heartbeat_timeout,
        ) {
            Ok(rows) => {
                if let Some((session_id, _)) = rows
                    .iter()
                    .find(|(_, holder_user)| holder_user != &body.user_id)
                {
                    return (
                        StatusCode::FORBIDDEN,
                        Json(json!({
                            "code": codes::PERM_DENIED_STATE,
                            "message": "该图面已有活动的编辑会话",
                            "holder": session_id,
                        })),
                    )
                        .into_response();
                }
            }
            Err(err) => {
                eprintln!(
                    "[ocs_collab_server] active_editing_sessions({}) 失败：{err}",
                    body.drawing_id
                );
            }
        }
    }

    let session_id = session_id_for(&body.user_id, &body.drawing_id);
    let row = SessionRow {
        session_id: session_id.clone(),
        drawing_id: body.drawing_id.clone(),
        user_id: body.user_id.clone(),
        mode: body.mode.clone(),
        status: "活跃".to_owned(),
        base_revision,
        last_heartbeat: now,
        created_at: now,
        workspace: body.workspace.clone(),
    };
    if let Err(err) = store.upsert_session(&row) {
        return error_response(StatusCode::INTERNAL_SERVER_ERROR, codes::INVALID_MESSAGE, &err);
    }

    (
        StatusCode::CREATED,
        Json(CreateSessionResponse {
            session_id,
            base_revision,
        }),
    )
        .into_response()
}

/// The id `POST /session` mints for a `(user_id, drawing_id)` pair.
///
/// Unlike `new_session_id()` this is deterministic: a repost of the same pair
/// lands on the same `sessions` row, so `upsert_session`'s `ON CONFLICT` is
/// what refreshes the session — no dedicated lookup is needed, and a stale
/// row whose heartbeat already expired is still reused rather than replaced.
fn session_id_for(user_id: &str, drawing_id: &str) -> String {
    // FNV-1a 64 over the pair under two seeds → 128 bits rendered as an
    // RFC-4122 v4 UUID, keeping the `s-<uuid>` silhouette `new_session_id()`
    // produces for WebSocket sessions.
    let fnv = |seed: u64| {
        user_id
            .bytes()
            .chain([0x1f])
            .chain(drawing_id.bytes())
            .fold(0xcbf2_9ce4_8422_2325_u64 ^ seed, |hash, byte| {
                (hash ^ u64::from(byte)).wrapping_mul(0x0000_0100_0000_01b3)
            })
    };
    let mut bytes =
        ((u128::from(fnv(0x9e37_79b9_7f4a_7c15)) << 64) | u128::from(fnv(0))).to_be_bytes();
    bytes[6] = (bytes[6] & 0x0f) | 0x40;
    bytes[8] = (bytes[8] & 0x3f) | 0x80;
    format!("s-{}", Uuid::from_bytes(bytes))
}

#[derive(Debug, Deserialize)]
struct PermissionCheckBody {
    user_id: Option<String>,
    drawing_id: Option<String>,
    operation: Option<String>,
}

/// `POST /permission/check` → the chain's decision. The body is the six
/// `PermissionDecision` fields plus `holder` when a 单人编辑 drawing is held
/// by somebody else's live editing session.
async fn permission_check(
    State(state): State<Arc<AppState>>,
    Json(body): Json<PermissionCheckBody>,
) -> Response {
    let Some(store) = state.store.clone() else {
        return error_response(
            StatusCode::SERVICE_UNAVAILABLE,
            codes::INVALID_MESSAGE,
            "数据库不可用",
        );
    };
    let (Some(user_id), Some(drawing_id), Some(operation)) =
        (body.user_id, body.drawing_id, body.operation)
    else {
        return error_response(
            StatusCode::BAD_REQUEST,
            codes::INVALID_MESSAGE,
            "缺少必填字段",
        );
    };

    let active_editors = active_editors_on(&state, &drawing_id).await;
    match perm::evaluate(&store, &user_id, &drawing_id, &operation, &active_editors) {
        Ok(outcome) => (outcome.status, Json(decision_json(&outcome))).into_response(),
        Err(err) => perm_error_response(&err),
    }
}

#[derive(Debug, Deserialize)]
struct DrawingStateBody {
    user_id: Option<String>,
    state: Option<String>,
}

/// `POST /drawing/{id}/state` — gated on 授权配置. The requested state value
/// is validated before anything is written; an illegal value is a 400 and
/// nothing is persisted.
async fn drawing_state(
    State(state): State<Arc<AppState>>,
    Path(drawing_id): Path<String>,
    Json(body): Json<DrawingStateBody>,
) -> Response {
    let Some(store) = state.store.clone() else {
        return error_response(
            StatusCode::SERVICE_UNAVAILABLE,
            codes::INVALID_MESSAGE,
            "数据库不可用",
        );
    };
    let Some(user_id) = body.user_id else {
        return error_response(
            StatusCode::BAD_REQUEST,
            codes::INVALID_MESSAGE,
            "缺少 user_id",
        );
    };

    // Switching state is 授权配置 in the role matrix.
    let active_editors = active_editors_on(&state, &drawing_id).await;
    match perm::evaluate(
        &store,
        &user_id,
        &drawing_id,
        Operation::ConfigureAuthorization.as_str(),
        &active_editors,
    ) {
        Ok(outcome) if outcome.decision.allowed => {}
        Ok(outcome) => return (outcome.status, Json(decision_json(&outcome))).into_response(),
        Err(err) => return perm_error_response(&err),
    }

    let Some(state_text) = body.state else {
        return error_response(
            StatusCode::BAD_REQUEST,
            codes::INVALID_MESSAGE,
            "缺少 state",
        );
    };
    let Some(next_state) = DrawingState::parse(&state_text) else {
        return error_response(
            StatusCode::BAD_REQUEST,
            codes::INVALID_STATE_TRANSITION,
            "状态值非法",
        );
    };

    // `evaluate` already 404s an unknown drawing; this re-read is for the
    // transition check on the current state.
    let row = match store.drawing(&drawing_id) {
        Ok(row) => row,
        Err(err) => {
            return error_response(StatusCode::INTERNAL_SERVER_ERROR, codes::INVALID_MESSAGE, &err)
        }
    };
    let Some((_node, current_text, _revision)) = row else {
        return error_response(
            StatusCode::NOT_FOUND,
            codes::PERM_UNKNOWN_SUBJECT,
            "图纸不存在",
        );
    };

    // All three states currently interconvert, so this never fires today; the
    // 409 stays for the day a real transition rule exists.
    if DrawingState::parse(&current_text)
        .is_some_and(|current| !current.can_transition_to(next_state))
    {
        return error_response(
            StatusCode::CONFLICT,
            codes::INVALID_STATE_TRANSITION,
            "状态迁移不允许",
        );
    }

    if let Err(err) = store.set_drawing_state(&drawing_id, &state_text) {
        return error_response(StatusCode::INTERNAL_SERVER_ERROR, codes::INVALID_MESSAGE, &err);
    }

    (
        StatusCode::OK,
        Json(json!({ "drawing_id": drawing_id, "state": state_text })),
    )
        .into_response()
}

/// `(session_id, user_id)` of every live editing session on the drawing —
/// the input the single-editor half of the state tier needs.
///
/// The `sessions` table is authoritative (appendix A.3): a session created by
/// `POST /session` lives there and never touches the WebSocket registry, so
/// reading the registry alone made `VAL-PERM-017` unreachable. The in-memory
/// registry is unioned in as well, because a socket that has just opened may
/// not have been mirrored to the table yet; a session present in both is
/// deduplicated by id.
async fn active_editors_on(state: &AppState, drawing_id: &str) -> Vec<(String, String)> {
    let mut live: Vec<(String, String)> = Vec::new();
    let mut seen: std::collections::BTreeSet<String> = std::collections::BTreeSet::new();

    if let Some(store) = state.store.as_ref() {
        let now = chrono::Utc::now().timestamp();
        match store.active_editing_sessions(
            drawing_id,
            now,
            state.session_config.heartbeat_timeout,
        ) {
            Ok(rows) => {
                for (session_id, user_id) in rows {
                    if seen.insert(session_id.clone()) {
                        live.push((session_id, user_id));
                    }
                }
            }
            Err(err) => {
                eprintln!(
                    "[ocs_collab_server] active_editing_sessions({drawing_id}) 失败：{err}"
                );
            }
        }
    }

    let registry = state.sessions.lock().await;
    for session in registry.all() {
        if session.drawing_id != drawing_id
            || !session.is_active()
            || !session.is_editing()
        {
            continue;
        }
        if seen.insert(session.session_id.clone()) {
            live.push((session.session_id.clone(), session.user_id.clone()));
        }
    }
    live
}

/// The decision's six serialized fields plus `holder` when the single-editor
/// override produced one.
fn decision_json(outcome: &perm::EvalOutcome) -> serde_json::Value {
    let mut value = serde_json::to_value(&outcome.decision)
        .unwrap_or_else(|_| json!({ "allowed": false }));
    if let Some(holder) = &outcome.holder {
        value["holder"] = json!(holder);
    }
    value
}

fn perm_error_response(err: &perm::PermError) -> Response {
    error_response(err.status_code(), err.code(), &err.message())
}

#[derive(Debug, Deserialize)]
struct AuditParams {
    object_ref: Option<String>,
    tag: Option<String>,
    user_id: Option<String>,
    from: Option<i64>,
    to: Option<i64>,
    limit: Option<i64>,
    offset: Option<i64>,
}

/// `GET /audit` → 200 `{"items":[...],"total":N}` ordered by `at DESC`.
async fn audit_list(
    State(state): State<Arc<AppState>>,
    Query(params): Query<AuditParams>,
) -> Response {
    let Some(store) = state.store.clone() else {
        return error_response(StatusCode::SERVICE_UNAVAILABLE, codes::INVALID_MESSAGE, "数据库不可用");
    };

    let query = AuditQuery {
        object_ref: params.object_ref.or(params.tag),
        user_id: params.user_id,
        from: params.from,
        to: params.to,
        limit: params.limit,
        offset: params.offset,
    };
    match store.audit_query(&query) {
        Ok(page) => (StatusCode::OK, Json(page)).into_response(),
        Err(err) => error_response(StatusCode::INTERNAL_SERVER_ERROR, codes::INVALID_MESSAGE, &err),
    }
}

/// The audit log has no mutation path at all (invariant 7).
async fn audit_immutable() -> Response {
    (
        StatusCode::METHOD_NOT_ALLOWED,
        Json(json!({
            "code": "AUDIT_APPEND_ONLY",
            "message": "审计日志只追加，不支持修改或删除",
        })),
    )
        .into_response()
}

fn error_response(status: StatusCode, code: &str, message: &str) -> Response {
    (
        status,
        Json(json!({ "code": code, "message": message })),
    )
        .into_response()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::{AuditPage, NewAuditRow, Store};
    use ocs_collab::SessionConfig;

    fn state_with_memory_store() -> Arc<AppState> {
        let store = Store::open_in_memory().expect("memory store");
        Arc::new(AppState::new(
            Some(store),
            None,
            ":memory:",
            SessionConfig::default(),
        ))
    }

    fn state_without_store() -> Arc<AppState> {
        Arc::new(AppState::new(
            None,
            Some("open failed".into()),
            "bad.db",
            SessionConfig::default(),
        ))
    }

    #[tokio::test]
    async fn health_reports_ok_with_a_working_database() {
        let state = state_with_memory_store();
        assert!(state.db_healthy());
    }

    #[tokio::test]
    async fn health_reports_failure_without_a_database() {
        let state = state_without_store();
        assert!(!state.db_healthy());
    }

    #[test]
    fn the_router_builds_with_every_documented_route() {
        let state = state_with_memory_store();
        let _router = router(state);
    }

    #[tokio::test]
    async fn audit_mutation_returns_405() {
        let response = audit_immutable().await;
        assert_eq!(response.status(), StatusCode::METHOD_NOT_ALLOWED);
    }

    /// `?tag=X` must filter exactly like `?object_ref=X` instead of being
    /// silently dropped into an unfiltered dump.
    #[tokio::test]
    async fn audit_tag_filters_the_same_as_object_ref() {
        let state = state_with_memory_store();
        let store = state.store.clone().expect("store");
        for (object_ref, at) in [("AAA-1", 1), ("BBB-2", 2)] {
            store
                .audit_append(&NewAuditRow {
                    module: "P&ID".into(),
                    action: "set_property".into(),
                    at,
                    object_ref: object_ref.into(),
                    user_id: "u1".into(),
                    company: "甲设计院".into(),
                    ip: "127.0.0.1".into(),
                    workspace: "plant-a".into(),
                    change_text: format!("set_property {object_ref}.field: (未知) -> v"),
                })
                .expect("append audit");
        }

        let page = |response: Response| async move {
            let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
                .await
                .expect("body bytes");
            serde_json::from_slice::<AuditPage>(&bytes).expect("audit page json")
        };
        let params = |tag: Option<&str>, object_ref: Option<&str>| AuditParams {
            tag: tag.map(str::to_owned),
            object_ref: object_ref.map(str::to_owned),
            user_id: None,
            from: None,
            to: None,
            limit: None,
            offset: None,
        };

        let by_object_ref = page(
            audit_list(State(state.clone()), Query(params(None, Some("AAA-1")))).await,
        )
        .await;
        let by_tag =
            page(audit_list(State(state), Query(params(Some("AAA-1"), None))).await).await;

        assert_eq!(by_object_ref.total, 1);
        assert_eq!(by_tag.total, 1, "?tag must filter like ?object_ref");
        assert_eq!(by_tag.items, by_object_ref.items);
        assert_eq!(by_tag.items[0].object_ref, "AAA-1");
    }

    /// `/audit/<seq>` answers 405 with the append-only body for every
    /// method, and the rejections leave the log's row count and max `seq`
    /// untouched (VAL-CONF-018). Driven over real TCP because `tower` is
    /// not a dependency and `oneshot` is out of reach.
    #[tokio::test]
    async fn audit_seq_requests_return_405_and_leave_the_log_untouched() {
        let state = state_with_memory_store();
        let store = state.store.clone().expect("store");
        let max_seq = store
            .audit_append(&NewAuditRow {
                module: "P&ID".into(),
                action: "set_property".into(),
                at: 1,
                object_ref: "AAA-1".into(),
                user_id: "u1".into(),
                company: "甲设计院".into(),
                ip: "127.0.0.1".into(),
                workspace: "plant-a".into(),
                change_text: "set_property AAA-1.field: (未知) -> v".into(),
            })
            .expect("append audit");
        let count_before = store.audit_count().expect("count");

        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind");
        let port = listener.local_addr().expect("local addr").port();
        tokio::spawn(async move {
            let _ = axum::serve(listener, router(state)).await;
        });

        for method in ["GET", "PUT", "PATCH", "DELETE"] {
            let (status, body) = http_exchange(port, method, "/audit/1").await;
            assert_eq!(status, 405, "{method} /audit/1");
            assert_eq!(body["code"], "AUDIT_APPEND_ONLY", "{method} /audit/1");
        }

        assert_eq!(store.audit_count().expect("count"), count_before);
        let page = store
            .audit_query(&AuditQuery::default())
            .expect("audit query");
        assert_eq!(page.items.iter().map(|row| row.seq).max(), Some(max_seq));
    }

    /// One raw HTTP/1.1 exchange with `Connection: close`, read to EOF.
    /// `io-util` is not enabled, so the stream uses `try_read`/`try_write`.
    async fn http_exchange(port: u16, method: &str, path: &str) -> (u16, serde_json::Value) {
        let stream = tokio::net::TcpStream::connect(("127.0.0.1", port))
            .await
            .expect("connect");
        let request = format!(
            "{method} {path} HTTP/1.1\r\nHost: 127.0.0.1\r\nConnection: close\r\n\r\n"
        );
        let mut written = 0;
        while written < request.len() {
            stream.writable().await.expect("writable");
            match stream.try_write(&request.as_bytes()[written..]) {
                Ok(n) => written += n,
                Err(err) if err.kind() == std::io::ErrorKind::WouldBlock => {}
                Err(err) => panic!("write: {err}"),
            }
        }
        let mut buf = vec![0u8; 4096];
        let mut raw = Vec::new();
        loop {
            stream.readable().await.expect("readable");
            match stream.try_read(&mut buf) {
                Ok(0) => break,
                Ok(n) => raw.extend_from_slice(&buf[..n]),
                Err(err) if err.kind() == std::io::ErrorKind::WouldBlock => {}
                Err(err) => panic!("read: {err}"),
            }
        }
        let text = String::from_utf8(raw).expect("utf8 response");
        let (head, body) = text.split_once("\r\n\r\n").expect("header/body split");
        let status = head
            .split_whitespace()
            .nth(1)
            .and_then(|code| code.parse::<u16>().ok())
            .expect("status code");
        (status, serde_json::from_str(body).expect("json body"))
    }

    /// A store seeded with three users, one root ACL row each, and the two
    /// drawings the permission tests need (协同编辑 + 单人编辑).
    fn seeded_state() -> Arc<AppState> {
        let store = Store::open_in_memory().expect("memory store");
        let doc: crate::store::SeedDoc = serde_json::from_str(
            r#"{
              "users":[
                {"user_id":"u-admin","display_name":"管理员","role":"系统管理员"},
                {"user_id":"u-proc","display_name":"工艺甲","role":"工艺设计人员"},
                {"user_id":"u-view","display_name":"查看者","role":"只读查看者"}
              ],
              "pbs_nodes":[
                {"node_id":"n-plant","name":"工厂","node_type":"工厂"},
                {"node_id":"n-cell-a1","name":"单元A1","node_type":"单元","parent_id":"n-plant"},
                {"node_id":"n-cell-a2","name":"单元A2","node_type":"单元","parent_id":"n-plant"}
              ],
              "pbs_acl":[
                {"node_id":"n-plant","subject":"u-admin","permission":"编辑","allowed":true},
                {"node_id":"n-plant","subject":"u-proc","permission":"编辑","allowed":true},
                {"node_id":"n-plant","subject":"u-view","permission":"查看","allowed":true}
              ],
              "drawings":[
                {"drawing_id":"d-a1-coop","file_name":"a1.dwg","pbs_node_id":"n-cell-a1","state":"协同编辑"},
                {"drawing_id":"d-a2-solo","file_name":"a2.dwg","pbs_node_id":"n-cell-a2","state":"单人编辑"}
              ]
            }"#,
        )
        .expect("seed json");
        store.seed(&doc).expect("seed applies");
        Arc::new(AppState::new(
            Some(store),
            None,
            ":memory:",
            SessionConfig::default(),
        ))
    }

    #[tokio::test]
    async fn permission_check_allows_a_permitted_write() {
        let state = seeded_state();
        let response = permission_check(
            State(state),
            Json(PermissionCheckBody {
                user_id: Some("u-proc".into()),
                drawing_id: Some("d-a2-solo".into()),
                operation: Some("编辑图纸".into()),
            }),
        )
        .await;
        assert_eq!(response.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn permission_check_maps_errors_to_their_statuses() {
        let state = seeded_state();
        let unknown_drawing = permission_check(
            State(state.clone()),
            Json(PermissionCheckBody {
                user_id: Some("u-admin".into()),
                drawing_id: Some("d-missing".into()),
                operation: Some("查看图纸".into()),
            }),
        )
        .await;
        assert_eq!(unknown_drawing.status(), StatusCode::NOT_FOUND);

        let unknown_user = permission_check(
            State(state.clone()),
            Json(PermissionCheckBody {
                user_id: Some("u-ghost".into()),
                drawing_id: Some("d-a1-coop".into()),
                operation: Some("查看图纸".into()),
            }),
        )
        .await;
        assert_eq!(unknown_user.status(), StatusCode::BAD_REQUEST);

        let bad_operation = permission_check(
            State(state),
            Json(PermissionCheckBody {
                user_id: Some("u-admin".into()),
                drawing_id: Some("d-a1-coop".into()),
                operation: Some("打印图纸".into()),
            }),
        )
        .await;
        assert_eq!(bad_operation.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn permission_check_without_required_fields_is_a_bad_request() {
        let state = seeded_state();
        let response = permission_check(
            State(state),
            Json(PermissionCheckBody {
                user_id: None,
                drawing_id: Some("d-a1-coop".into()),
                operation: Some("查看图纸".into()),
            }),
        )
        .await;
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn permission_check_denies_a_second_editor_on_a_solo_drawing() {
        let state = seeded_state();
        {
            let mut sessions = state.sessions.lock().await;
            sessions.open(ocs_collab::OpenRequest {
                session_id: "s-holder",
                drawing_id: "d-a2-solo",
                user_id: "u-admin",
                display_name: "管理员",
                mode: "单人编辑",
                workspace: "",
                base_revision: 0,
                now: 1_000,
            });
        }
        let response = permission_check(
            State(state),
            Json(PermissionCheckBody {
                user_id: Some("u-proc".into()),
                drawing_id: Some("d-a2-solo".into()),
                operation: Some("编辑图纸".into()),
            }),
        )
        .await;
        assert_eq!(response.status(), StatusCode::FORBIDDEN);
    }

    /// `VAL-PERM-017` 的真实路径：会话由 `POST /session` 建立，**从不经过**
    /// WebSocket 注册表。`/permission/check` 必须仍能看见它并返回 403 +
    /// `denied_level="state"` + `holder` 等于该 `session_id`。
    ///
    /// 回归背景：`active_editors_on` 曾只读内存注册表，而 `create_session`
    /// 只写库不注册，于是这条断言永远走不到 403。
    #[tokio::test]
    async fn a_session_created_over_http_blocks_a_second_editor() {
        let state = seeded_state();

        let created = create_session(
            State(state.clone()),
            Json(CreateSessionBody {
                user_id: "u-proc".into(),
                drawing_id: "d-a2-solo".into(),
                mode: "单人编辑".into(),
                workspace: String::new(),
            }),
        )
        .await;
        assert_eq!(created.status(), StatusCode::CREATED, "POST /session 应当建成");
        let body = axum::body::to_bytes(created.into_body(), 64 * 1024)
            .await
            .expect("body readable");
        let payload: serde_json::Value =
            serde_json::from_slice(&body).expect("session response is json");
        let holder = payload["session_id"]
            .as_str()
            .expect("session_id present")
            .to_owned();

        // 内存注册表必须为空 —— 证明下面的 403 来自数据库，不是注册表。
        assert_eq!(
            state.sessions.lock().await.len(),
            0,
            "POST /session 不应写内存注册表，否则这条测试证明不了任何事"
        );

        let response = permission_check(
            State(state),
            Json(PermissionCheckBody {
                user_id: Some("u-admin".into()),
                drawing_id: Some("d-a2-solo".into()),
                operation: Some("编辑图纸".into()),
            }),
        )
        .await;
        assert_eq!(
            response.status(),
            StatusCode::FORBIDDEN,
            "单人编辑图纸上的第 2 个编辑会话必须被拒（VAL-PERM-017）"
        );
        let body = axum::body::to_bytes(response.into_body(), 64 * 1024)
            .await
            .expect("body readable");
        let decision: serde_json::Value =
            serde_json::from_slice(&body).expect("decision is json");
        assert_eq!(
            decision["denied_level"].as_str(),
            Some("state"),
            "拒绝层级应当是 state，实际 {decision}"
        );
        assert_eq!(
            decision["holder"].as_str(),
            Some(holder.as_str()),
            "holder 应当指向 POST /session 建的会话，实际 {decision}"
        );
    }

    /// The HTTP entry enforces the same 单人编辑 cap the permission chain
    /// does: a second live editing session from another user is refused at
    /// `POST /session` with 403 + `PERM_DENIED_STATE` + `holder` naming the
    /// occupying `session_id` — not a fresh row silently coexisting.
    #[tokio::test]
    async fn a_second_editor_is_refused_at_the_http_session_entry() {
        let state = seeded_state();

        let created = create_session(
            State(state.clone()),
            Json(CreateSessionBody {
                user_id: "u-proc".into(),
                drawing_id: "d-a2-solo".into(),
                mode: "单人编辑".into(),
                workspace: String::new(),
            }),
        )
        .await;
        assert_eq!(created.status(), StatusCode::CREATED);
        let body = axum::body::to_bytes(created.into_body(), 64 * 1024)
            .await
            .expect("body readable");
        let payload: serde_json::Value =
            serde_json::from_slice(&body).expect("session response is json");
        let holder = payload["session_id"]
            .as_str()
            .expect("session_id present")
            .to_owned();

        // Somebody else's editing request on the held 单人编辑 drawing.
        let refused = create_session(
            State(state.clone()),
            Json(CreateSessionBody {
                user_id: "u-admin".into(),
                drawing_id: "d-a2-solo".into(),
                mode: "单人编辑".into(),
                workspace: String::new(),
            }),
        )
        .await;
        assert_eq!(
            refused.status(),
            StatusCode::FORBIDDEN,
            "单人编辑图纸上的第 2 个编辑会话必须被 HTTP 入口拒绝"
        );
        let body = axum::body::to_bytes(refused.into_body(), 64 * 1024)
            .await
            .expect("body readable");
        let refusal: serde_json::Value =
            serde_json::from_slice(&body).expect("refusal is json");
        assert_eq!(
            refusal["code"].as_str(),
            Some(codes::PERM_DENIED_STATE),
            "拒绝 code 应当是 PERM_DENIED_STATE，实际 {refusal}"
        );
        assert_eq!(
            refusal["holder"].as_str(),
            Some(holder.as_str()),
            "holder 应当指向占用图纸的会话，实际 {refusal}"
        );

        // The cap is per drawing: the same user opens a different drawing.
        let other = create_session(
            State(state),
            Json(CreateSessionBody {
                user_id: "u-admin".into(),
                drawing_id: "d-a1-coop".into(),
                mode: "单人编辑".into(),
                workspace: String::new(),
            }),
        )
        .await;
        assert_eq!(other.status(), StatusCode::CREATED);
    }

    /// `POST /session` is idempotent for a `(user_id, drawing_id)` pair: a
    /// repost refreshes `last_heartbeat` on the existing row instead of
    /// stacking a duplicate behind a new id. The row is staled to
    /// `last_heartbeat = 0` first so `active_editing_sessions` no longer
    /// returns it — the repost must still find the same session.
    #[tokio::test]
    async fn reposting_the_same_session_refreshes_instead_of_duplicating() {
        let state = seeded_state();
        let store = state.store.clone().expect("store");

        let body = || CreateSessionBody {
            user_id: "u-proc".into(),
            drawing_id: "d-a2-solo".into(),
            mode: "单人编辑".into(),
            workspace: String::new(),
        };

        let first = create_session(State(state.clone()), Json(body())).await;
        assert_eq!(first.status(), StatusCode::CREATED);
        let bytes = axum::body::to_bytes(first.into_body(), 64 * 1024)
            .await
            .expect("body readable");
        let payload: serde_json::Value =
            serde_json::from_slice(&bytes).expect("session response is json");
        let session_id = payload["session_id"]
            .as_str()
            .expect("session_id present")
            .to_owned();

        // Age the row out of the heartbeat window so the repost cannot be
        // resolved through the active-session query.
        let mut stale = store
            .session_row(&session_id)
            .expect("lookup")
            .expect("session row exists");
        stale.last_heartbeat = 0;
        store.upsert_session(&stale).expect("stale the heartbeat");

        let before = chrono::Utc::now().timestamp() - 1;
        let second = create_session(State(state), Json(body())).await;
        assert_eq!(
            second.status(),
            StatusCode::CREATED,
            "重复 POST 同一 (user_id, drawing_id) 仍然返回 201"
        );
        let bytes = axum::body::to_bytes(second.into_body(), 64 * 1024)
            .await
            .expect("body readable");
        let repost: serde_json::Value =
            serde_json::from_slice(&bytes).expect("session response is json");
        assert_eq!(
            repost["session_id"].as_str(),
            Some(session_id.as_str()),
            "重复 POST 必须复用已有 session_id，实际 {repost}"
        );

        let row = store
            .session_row(&session_id)
            .expect("lookup")
            .expect("session row exists");
        assert!(
            row.last_heartbeat >= before,
            "last_heartbeat 应被刷新到 repost 时刻，实际 {}",
            row.last_heartbeat
        );
    }

    #[test]
    fn decision_json_merges_the_holder_into_the_six_decision_fields() {
        let outcome = perm::EvalOutcome {
            decision: ocs_collab::PermissionDecision {
                allowed: false,
                denied_level: Some(ocs_collab::DenialLevel::State),
                code: Some("PERM_DENIED_STATE".into()),
                effective_node_id: None,
                effective_node_path: Vec::new(),
                inherited: false,
            },
            status: StatusCode::FORBIDDEN,
            holder: Some("s-9".into()),
        };
        let body = decision_json(&outcome);
        assert_eq!(body["allowed"], false);
        assert_eq!(body["denied_level"], "state");
        assert_eq!(body["code"], "PERM_DENIED_STATE");
        assert!(body.get("effective_node_id").is_some());
        assert!(body.get("effective_node_path").is_some());
        assert!(body.get("inherited").is_some());
        assert_eq!(body["holder"], "s-9");
    }

    #[tokio::test]
    async fn state_change_persists_a_legal_value() {
        let state = seeded_state();
        let store = state.store.clone().expect("store");
        let response = drawing_state(
            State(state.clone()),
            Path("d-a1-coop".into()),
            Json(DrawingStateBody {
                user_id: Some("u-admin".into()),
                state: Some("只读".into()),
            }),
        )
        .await;
        assert_eq!(response.status(), StatusCode::OK);
        let (_, stored, _) = store
            .drawing("d-a1-coop")
            .expect("lookup")
            .expect("exists");
        assert_eq!(stored, "只读");
    }

    #[tokio::test]
    async fn state_change_with_an_illegal_value_is_rejected_without_persisting() {
        let state = seeded_state();
        let store = state.store.clone().expect("store");
        let response = drawing_state(
            State(state.clone()),
            Path("d-a1-coop".into()),
            Json(DrawingStateBody {
                user_id: Some("u-admin".into()),
                state: Some("四态".into()),
            }),
        )
        .await;
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
        let (_, stored, _) = store
            .drawing("d-a1-coop")
            .expect("lookup")
            .expect("exists");
        assert_eq!(
            stored, "协同编辑",
            "an illegal value must not be persisted"
        );
    }

    #[tokio::test]
    async fn state_change_without_user_id_is_a_bad_request() {
        let state = seeded_state();
        let response = drawing_state(
            State(state),
            Path("d-a1-coop".into()),
            Json(DrawingStateBody {
                user_id: None,
                state: Some("只读".into()),
            }),
        )
        .await;
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn state_change_requires_configure_authorization_permission() {
        let state = seeded_state();
        // VAL-PERM-020(a): u-view's role denies 授权配置 before anything else.
        let response = drawing_state(
            State(state),
            Path("d-a1-coop".into()),
            Json(DrawingStateBody {
                user_id: Some("u-view".into()),
                state: Some("只读".into()),
            }),
        )
        .await;
        assert_eq!(response.status(), StatusCode::FORBIDDEN);
    }

    #[tokio::test]
    async fn state_change_on_an_unknown_drawing_is_not_found() {
        let state = seeded_state();
        let response = drawing_state(
            State(state),
            Path("d-missing".into()),
            Json(DrawingStateBody {
                user_id: Some("u-admin".into()),
                state: Some("只读".into()),
            }),
        )
        .await;
        assert_eq!(response.status(), StatusCode::NOT_FOUND);
    }
}
