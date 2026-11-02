//! axum HTTP control plane.
//!
//! The scaffold serves the endpoints later milestones build on: `/health`,
//! `/session`, and a read-only `/audit`. `PUT` / `PATCH` / `DELETE` on `/audit`
//! answer `405` because the audit log is append-only.

use std::sync::Arc;

use axum::extract::{Query, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use ocs_collab::protocol::codes;
use serde::{Deserialize, Serialize};
use serde_json::json;
use uuid::Uuid;

use crate::store::{AuditQuery, SessionRow};
use crate::AppState;

/// Builds the router. Integration tests use this same function.
pub fn router(state: Arc<AppState>) -> Router {
    Router::new()
        .route("/health", get(health))
        .route("/session", post(create_session))
        .route(
            "/audit",
            get(audit_list)
                .put(audit_immutable)
                .patch(audit_immutable)
                .delete(audit_immutable),
        )
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

    let base_revision = match store.drawing(&body.drawing_id) {
        Ok(Some((_, _, revision))) => revision,
        Ok(None) => 0,
        Err(err) => {
            return error_response(StatusCode::INTERNAL_SERVER_ERROR, codes::UNKNOWN_DRAWING, &err)
        }
    };

    let session_id = format!("s-{}", Uuid::new_v4());
    let now = chrono::Utc::now().timestamp();
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

#[derive(Debug, Deserialize)]
struct AuditParams {
    object_ref: Option<String>,
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
        object_ref: params.object_ref,
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
    use crate::store::Store;
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
}
