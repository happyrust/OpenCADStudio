//! The HTTP assembly layer over `ocs_collab`'s pure permission chain.
//!
//! [`evaluate`] gathers the chain's inputs out of SQLite — the caller's role,
//! the drawing's state, the whole PBS ACL tree — runs [`decide`], and adds
//! the two things the pure chain cannot know about: live editing sessions
//! (the second half of the 单人编辑 rule) and the contract's root-first
//! `effective_node_path`.

use axum::http::StatusCode;
use ocs_collab::protocol::codes;
use ocs_collab::{
    decide, AclEntry, AclNode, AclPermission, DenialLevel, DrawingState, Operation,
    PermissionDecision, PermissionRequest, Role,
};

use crate::store::Store;

/// A failure detected while gathering the chain's inputs — before the chain
/// itself ever runs. Each variant maps to one HTTP status and one wire code.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PermError {
    /// `user_id` is absent from `users`, or its stored role is not one of the
    /// five (dirty data). The contract maps both to 400.
    UnknownSubject,
    /// `operation` is not one of the eight.
    UnknownOperation,
    /// `drawing_id` is absent from `drawings`.
    UnknownDrawing,
    /// `drawings.state` holds a value outside the three legal states.
    InvalidState(String),
    /// Any other store failure.
    Store(String),
}

impl PermError {
    /// The HTTP status the variant maps to.
    pub fn status_code(&self) -> StatusCode {
        match self {
            PermError::UnknownSubject | PermError::UnknownOperation => StatusCode::BAD_REQUEST,
            PermError::UnknownDrawing => StatusCode::NOT_FOUND,
            PermError::InvalidState(_) | PermError::Store(_) => {
                StatusCode::INTERNAL_SERVER_ERROR
            }
        }
    }

    /// The `code` written into the `{"code","message"}` error body.
    ///
    /// An unknown *drawing* still reports `PERM_UNKNOWN_SUBJECT`: the contract
    /// deliberately reuses the subject code rather than `UNKNOWN_DRAWING`.
    pub fn code(&self) -> &'static str {
        match self {
            PermError::UnknownSubject | PermError::UnknownDrawing => codes::PERM_UNKNOWN_SUBJECT,
            PermError::UnknownOperation => codes::PERM_UNKNOWN_OPERATION,
            PermError::InvalidState(_) => codes::PERM_INVALID_STATE,
            PermError::Store(_) => codes::INVALID_MESSAGE,
        }
    }

    /// The `message` written into the error body.
    pub fn message(&self) -> String {
        match self {
            PermError::UnknownSubject => "用户不存在".to_owned(),
            PermError::UnknownOperation => "operation 非法".to_owned(),
            PermError::UnknownDrawing => "图纸不存在".to_owned(),
            PermError::InvalidState(state) => format!("图纸状态非法: {state}"),
            PermError::Store(detail) => detail.clone(),
        }
    }
}

/// The chain's answer plus the HTTP dressing around it.
#[derive(Debug, Clone)]
pub struct EvalOutcome {
    /// The six-field decision, serialized directly into the response body.
    pub decision: PermissionDecision,
    /// 200 when allowed, 403 for any of the three denial levels.
    pub status: StatusCode,
    /// Session id holding the drawing under 单人编辑, present only when the
    /// session check produced the denial. Serialized as `holder`.
    pub holder: Option<String>,
}

/// Reads the whole PBS tree out of the store and shapes it as the flat
/// `AclNode` slice [`decide`] walks.
///
/// ACL rows whose `permission` string is not one of the four legal values are
/// dropped: a dirty literal can never match an operation's permission anyway.
pub fn load_acl_tree(store: &Store) -> Result<Vec<AclNode>, String> {
    let mut nodes: Vec<AclNode> = store
        .list_pbs_nodes()?
        .into_iter()
        .map(|(node_id, parent_id)| AclNode {
            node_id,
            parent_id,
            entries: Vec::new(),
        })
        .collect();

    for (node_id, subject, permission, allowed) in store.list_pbs_acl()? {
        let Some(permission) = acl_permission(&permission) else {
            continue;
        };
        if let Some(node) = nodes.iter_mut().find(|node| node.node_id == node_id) {
            node.entries.push(AclEntry {
                subject,
                permission,
                allowed,
            });
        }
    }
    Ok(nodes)
}

/// The database spelling → [`AclPermission`]. The pure crate deliberately has
/// no `parse`, so the mapping lives next to the database read.
fn acl_permission(text: &str) -> Option<AclPermission> {
    [
        AclPermission::View,
        AclPermission::Edit,
        AclPermission::Export,
        AclPermission::Delete,
    ]
    .into_iter()
    .find(|permission| permission.as_str() == text)
}

/// Runs the full check for `(user_id, drawing_id, operation)`.
///
/// `active_editors` is the list of `(session_id, user_id)` of every live
/// editing session already on this drawing — the input the single-editor
/// rule needs. The caller collects it from `AppState.sessions`.
///
/// Ordering: the operation string is validated first, then the user, then
/// the drawing, then the chain runs. A 单人编辑 drawing additionally refuses
/// a write when somebody *else* holds a live editing session; that override
/// only applies when the chain itself allowed, because the first denying
/// tier must keep its verdict.
pub fn evaluate(
    store: &Store,
    user_id: &str,
    drawing_id: &str,
    operation: &str,
    active_editors: &[(String, String)],
) -> Result<EvalOutcome, PermError> {
    let operation = Operation::parse(operation).ok_or(PermError::UnknownOperation)?;

    let (role_text, _company) = store
        .user_role(user_id)
        .map_err(PermError::Store)?
        .ok_or(PermError::UnknownSubject)?;
    let role = Role::parse(&role_text).ok_or(PermError::UnknownSubject)?;

    let (node_id, state_text, _revision) = store
        .drawing(drawing_id)
        .map_err(PermError::Store)?
        .ok_or(PermError::UnknownDrawing)?;
    let drawing_state =
        DrawingState::parse(&state_text).ok_or_else(|| PermError::InvalidState(state_text.clone()))?;

    let nodes = load_acl_tree(store).map_err(PermError::Store)?;
    let request = PermissionRequest {
        user_id,
        role,
        operation,
        drawing_state,
        node_id: &node_id,
        nodes: &nodes,
    };
    let mut decision = decide(&request);

    // The second half of the state tier: 单人编辑 admits exactly one live
    // editing session. The chain knows nothing about sessions, so the check
    // happens here — but only when every real tier already passed, since the
    // first denying tier must keep its verdict.
    let mut holder = None;
    if decision.allowed && drawing_state == DrawingState::SingleEditor && operation.is_write() {
        if let Some((session_id, _)) = active_editors
            .iter()
            .find(|(_, editor)| editor != user_id)
        {
            holder = Some(session_id.clone());
            decision = PermissionDecision {
                allowed: false,
                denied_level: Some(DenialLevel::State),
                code: Some(codes::PERM_DENIED_STATE.to_owned()),
                effective_node_id: None,
                effective_node_path: Vec::new(),
                inherited: false,
            };
        }
    }

    // `ocs_collab::permission::lookup_acl` already reports the full chain
    // root → target (the deciding node is carried separately in
    // `effective_node_id`), so the path is used as-is here.

    let status = if decision.allowed {
        StatusCode::OK
    } else {
        StatusCode::FORBIDDEN
    };
    Ok(EvalOutcome {
        decision,
        status,
        holder,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::{SeedAcl, SeedDoc, SeedDrawing, SeedPbsNode, SeedUser};
    use ocs_collab::permission::role_allows;

    fn user(user_id: &str, role: &str) -> SeedUser {
        SeedUser {
            user_id: user_id.to_owned(),
            display_name: user_id.to_owned(),
            role: role.to_owned(),
            company: "甲设计院".to_owned(),
        }
    }

    fn pbs(node_id: &str, parent_id: Option<&str>) -> SeedPbsNode {
        SeedPbsNode {
            node_id: node_id.to_owned(),
            name: node_id.to_owned(),
            node_type: "单元".to_owned(),
            parent_id: parent_id.map(str::to_owned),
        }
    }

    fn drawing(drawing_id: &str, node: &str, state: &str) -> SeedDrawing {
        SeedDrawing {
            drawing_id: drawing_id.to_owned(),
            file_name: format!("{drawing_id}.dwg"),
            pbs_node_id: node.to_owned(),
            state: state.to_owned(),
            lifecycle: "设计中".to_owned(),
        }
    }

    fn acl(node_id: &str, subject: &str, permission: &str, allowed: bool) -> SeedAcl {
        SeedAcl {
            node_id: node_id.to_owned(),
            subject: subject.to_owned(),
            permission: permission.to_owned(),
            allowed,
        }
    }

    /// The FIXTURE-PERM layout: six users, the plant → units → cells tree,
    /// four drawings. Every user except `u-nobody` holds all four ACL
    /// permissions on the root, so single-tier tests stay isolated.
    fn seed_doc() -> SeedDoc {
        let users = vec![
            user("u-admin", "系统管理员"),
            user("u-pm", "项目管理员"),
            user("u-proc", "工艺设计人员"),
            user("u-check", "校对人员"),
            user("u-view", "只读查看者"),
            user("u-nobody", "工艺设计人员"),
        ];
        let pbs_nodes = vec![
            pbs("n-plant", None),
            pbs("n-unit-a", Some("n-plant")),
            pbs("n-cell-a1", Some("n-unit-a")),
            pbs("n-cell-a2", Some("n-unit-a")),
            pbs("n-unit-b", Some("n-plant")),
            pbs("n-cell-b1", Some("n-unit-b")),
            pbs("n-cell-orphan", Some("n-plant")),
        ];
        let drawings = vec![
            drawing("d-a1-coop", "n-cell-a1", "协同编辑"),
            drawing("d-a2-solo", "n-cell-a2", "单人编辑"),
            drawing("d-b1-ro", "n-cell-b1", "只读"),
            drawing("d-orphan-ro", "n-cell-orphan", "协同编辑"),
        ];
        let mut pbs_acl = Vec::new();
        for user_id in ["u-admin", "u-pm", "u-proc", "u-check", "u-view"] {
            for permission in ["查看", "编辑", "导出", "删除"] {
                pbs_acl.push(acl("n-plant", user_id, permission, true));
            }
        }
        SeedDoc {
            users,
            pbs_nodes,
            pbs_acl,
            drawings,
        }
    }

    fn store_with(doc: SeedDoc) -> Store {
        let store = Store::open_in_memory().expect("memory store");
        store.seed(&doc).expect("seed applies");
        store
    }

    fn seeded_store() -> Store {
        store_with(seed_doc())
    }

    /// `evaluate` with no live editors — most tests never touch sessions.
    fn check(store: &Store, user_id: &str, drawing_id: &str, operation: &str) -> EvalOutcome {
        evaluate(store, user_id, drawing_id, operation, &[]).expect("evaluate")
    }

    #[test]
    fn the_role_matrix_is_evaluated_for_all_40_cells() {
        let store = seeded_store();
        let users = [
            ("u-admin", Role::SystemAdmin),
            ("u-pm", Role::ProjectAdmin),
            ("u-proc", Role::ProcessDesigner),
            ("u-check", Role::Proofreader),
            ("u-view", Role::ReadOnlyViewer),
        ];
        let mut cells = 0;
        for (user_id, role) in users {
            for operation in Operation::ALL {
                cells += 1;
                let outcome = check(&store, user_id, "d-a1-coop", operation.as_str());
                let expected = role_allows(role, operation);
                assert_eq!(
                    outcome.decision.allowed, expected,
                    "{role:?} × {operation:?}"
                );
                assert_eq!(
                    outcome.status,
                    if expected {
                        StatusCode::OK
                    } else {
                        StatusCode::FORBIDDEN
                    }
                );
                // The root allows every ACL permission, so any denial must
                // come from the role tier.
                if expected {
                    assert!(outcome.decision.denied_level.is_none());
                } else {
                    assert_eq!(outcome.decision.denied_level, Some(DenialLevel::Role));
                }
            }
        }
        assert_eq!(cells, 40);
    }

    #[test]
    fn an_unknown_user_is_an_unknown_subject() {
        let store = seeded_store();
        assert_eq!(
            evaluate(&store, "u-ghost", "d-a1-coop", "编辑图纸", &[]).unwrap_err(),
            PermError::UnknownSubject
        );
    }

    #[test]
    fn an_unparseable_stored_role_is_also_an_unknown_subject() {
        let mut doc = seed_doc();
        doc.users.push(user("u-dirty", "不存在的角色"));
        let store = store_with(doc);
        assert_eq!(
            evaluate(&store, "u-dirty", "d-a1-coop", "查看图纸", &[]).unwrap_err(),
            PermError::UnknownSubject
        );
    }

    #[test]
    fn an_unknown_operation_is_rejected_before_the_chain() {
        let store = seeded_store();
        for operation in ["打印图纸", ""] {
            assert_eq!(
                evaluate(&store, "u-admin", "d-a1-coop", operation, &[]).unwrap_err(),
                PermError::UnknownOperation,
                "{operation:?}"
            );
        }
    }

    #[test]
    fn an_unknown_drawing_is_not_found() {
        let store = seeded_store();
        assert_eq!(
            evaluate(&store, "u-admin", "不存在", "查看图纸", &[]).unwrap_err(),
            PermError::UnknownDrawing
        );
    }

    #[test]
    fn each_perm_error_carries_its_contract_status_and_code() {
        assert_eq!(
            PermError::UnknownSubject.status_code(),
            StatusCode::BAD_REQUEST
        );
        assert_eq!(
            PermError::UnknownSubject.code(),
            codes::PERM_UNKNOWN_SUBJECT
        );
        assert_eq!(
            PermError::UnknownOperation.status_code(),
            StatusCode::BAD_REQUEST
        );
        assert_eq!(
            PermError::UnknownOperation.code(),
            codes::PERM_UNKNOWN_OPERATION
        );
        assert_eq!(
            PermError::UnknownDrawing.status_code(),
            StatusCode::NOT_FOUND
        );
        assert_eq!(
            PermError::UnknownDrawing.code(),
            codes::PERM_UNKNOWN_SUBJECT
        );
        assert_eq!(
            PermError::InvalidState("四态".into()).status_code(),
            StatusCode::INTERNAL_SERVER_ERROR
        );
        assert_eq!(
            PermError::InvalidState("四态".into()).code(),
            codes::PERM_INVALID_STATE
        );
        assert_eq!(
            PermError::Store("db".into()).status_code(),
            StatusCode::INTERNAL_SERVER_ERROR
        );
    }

    #[test]
    fn an_acl_on_the_target_node_is_the_effective_node() {
        let mut doc = seed_doc();
        doc.pbs_acl.push(acl("n-cell-a1", "u-proc", "查看", true));
        let store = store_with(doc);
        let outcome = check(&store, "u-proc", "d-a1-coop", "查看图纸");
        assert!(outcome.decision.allowed);
        assert_eq!(
            outcome.decision.effective_node_id.as_deref(),
            Some("n-cell-a1")
        );
        assert!(!outcome.decision.inherited);
    }

    #[test]
    fn an_inherited_acl_reports_the_ancestor_and_a_root_first_path() {
        let store = seeded_store();
        // Neither n-cell-a1 nor n-unit-a carries a u-proc row; the root does.
        let outcome = check(&store, "u-proc", "d-a1-coop", "编辑图纸");
        assert!(outcome.decision.allowed);
        assert_eq!(
            outcome.decision.effective_node_id.as_deref(),
            Some("n-plant")
        );
        assert!(outcome.decision.inherited);
        assert_eq!(
            outcome.decision.effective_node_path,
            vec!["n-plant", "n-unit-a", "n-cell-a1"],
            "the contract reports the effective path root-first"
        );
    }

    #[test]
    fn a_path_without_any_acl_row_denies_at_pbs() {
        let store = seeded_store();
        // u-nobody exists but holds no rows anywhere on the path.
        let outcome = check(&store, "u-nobody", "d-a1-coop", "编辑图纸");
        assert!(!outcome.decision.allowed);
        assert_eq!(outcome.decision.denied_level, Some(DenialLevel::Pbs));
        assert_eq!(
            outcome.decision.code.as_deref(),
            Some(codes::PERM_DENIED_PBS)
        );
        assert_eq!(outcome.status, StatusCode::FORBIDDEN);
    }

    #[test]
    fn an_explicit_deny_row_denies_at_pbs_despite_a_root_allow() {
        let mut doc = seed_doc();
        doc.pbs_acl.push(acl("n-unit-b", "u-check", "导出", false));
        let store = store_with(doc);
        // u-check may export by role and the root allows it, but the nearer
        // unit-level deny row decides.
        let outcome = check(&store, "u-check", "d-b1-ro", "导出图纸");
        assert!(!outcome.decision.allowed);
        assert_eq!(outcome.decision.denied_level, Some(DenialLevel::Pbs));
    }

    #[test]
    fn a_user_allow_row_wins_over_a_role_deny_row_on_the_same_node() {
        let mut doc = seed_doc();
        doc.pbs_acl.push(acl("n-cell-a1", "u-proc", "编辑", true));
        doc.pbs_acl
            .push(acl("n-cell-a1", "工艺设计人员", "编辑", false));
        let store = store_with(doc);
        let outcome = check(&store, "u-proc", "d-a1-coop", "编辑图纸");
        assert!(outcome.decision.allowed, "the user row beats the role row");
        assert_eq!(
            outcome.decision.effective_node_id.as_deref(),
            Some("n-cell-a1")
        );
    }

    #[test]
    fn a_readonly_drawing_denies_the_five_write_operations_at_state() {
        let store = seeded_store();
        for operation in ["编辑图纸", "删除图纸", "发布签核", "授权配置", "主数据维护"] {
            let outcome = check(&store, "u-admin", "d-b1-ro", operation);
            assert!(!outcome.decision.allowed, "{operation}");
            assert_eq!(outcome.decision.denied_level, Some(DenialLevel::State));
            assert_eq!(
                outcome.decision.code.as_deref(),
                Some(codes::PERM_DENIED_STATE)
            );
        }
    }

    #[test]
    fn role_and_acl_allow_then_the_state_tier_denies() {
        let store = seeded_store();
        // u-proc passes role and ACL for editing; 只读 is the only tier left.
        let outcome = check(&store, "u-proc", "d-b1-ro", "编辑图纸");
        assert!(!outcome.decision.allowed);
        assert_eq!(outcome.decision.denied_level, Some(DenialLevel::State));
    }

    #[test]
    fn a_role_denial_short_circuits_even_when_state_would_deny_too() {
        let store = seeded_store();
        // u-view cannot edit at the role tier; that verdict is reported even
        // though 只读 would deny as well.
        let outcome = check(&store, "u-view", "d-b1-ro", "编辑图纸");
        assert!(!outcome.decision.allowed);
        assert_eq!(outcome.decision.denied_level, Some(DenialLevel::Role));
        assert_eq!(
            outcome.decision.code.as_deref(),
            Some(codes::PERM_DENIED_ROLE)
        );
    }

    #[test]
    fn single_editor_denies_a_second_editors_write_with_the_holder() {
        let store = seeded_store();
        let editors = vec![("s-other".to_owned(), "u-admin".to_owned())];
        let outcome = evaluate(&store, "u-proc", "d-a2-solo", "编辑图纸", &editors)
            .expect("evaluate");
        assert!(!outcome.decision.allowed);
        assert_eq!(outcome.decision.denied_level, Some(DenialLevel::State));
        assert_eq!(
            outcome.decision.code.as_deref(),
            Some(codes::PERM_DENIED_STATE)
        );
        assert_eq!(outcome.holder.as_deref(), Some("s-other"));
        assert_eq!(outcome.status, StatusCode::FORBIDDEN);
    }

    #[test]
    fn single_editor_allows_the_write_with_no_other_editor() {
        let store = seeded_store();
        let outcome = check(&store, "u-proc", "d-a2-solo", "编辑图纸");
        assert!(outcome.decision.allowed);
        assert!(outcome.holder.is_none());
        assert_eq!(outcome.status, StatusCode::OK);
    }

    #[test]
    fn single_editor_ignores_sessions_of_the_same_user() {
        let store = seeded_store();
        // A session the requester already owns is not "somebody else".
        let editors = vec![("s-own".to_owned(), "u-proc".to_owned())];
        let outcome = evaluate(&store, "u-proc", "d-a2-solo", "编辑图纸", &editors)
            .expect("evaluate");
        assert!(outcome.decision.allowed);
        assert!(outcome.holder.is_none());
    }

    #[test]
    fn the_single_editor_override_never_masks_an_earlier_tier_denial() {
        let store = seeded_store();
        // u-view fails the role tier; a live session must not turn that into
        // a state-level denial — the first denying tier keeps its verdict.
        let editors = vec![("s-other".to_owned(), "u-admin".to_owned())];
        let outcome = evaluate(&store, "u-view", "d-a2-solo", "编辑图纸", &editors)
            .expect("evaluate");
        assert_eq!(outcome.decision.denied_level, Some(DenialLevel::Role));
        assert!(outcome.holder.is_none());
    }

    #[test]
    fn flipping_a_drawing_to_readonly_flips_the_same_request_to_a_state_denial() {
        let store = seeded_store();
        let before = check(&store, "u-proc", "d-a1-coop", "编辑图纸");
        assert!(before.decision.allowed);
        assert_eq!(before.status, StatusCode::OK);

        store
            .set_drawing_state("d-a1-coop", "只读")
            .expect("state write");

        let after = check(&store, "u-proc", "d-a1-coop", "编辑图纸");
        assert!(!after.decision.allowed);
        assert_eq!(after.decision.denied_level, Some(DenialLevel::State));
        assert_eq!(after.status, StatusCode::FORBIDDEN);
    }

    #[test]
    fn a_dirty_drawing_state_is_an_invalid_state_error() {
        let store = seeded_store();
        store
            .set_drawing_state("d-a1-coop", "四态")
            .expect("dirty write");
        let err = evaluate(&store, "u-admin", "d-a1-coop", "查看图纸", &[]).unwrap_err();
        assert_eq!(err, PermError::InvalidState("四态".to_owned()));
        assert_eq!(err.status_code(), StatusCode::INTERNAL_SERVER_ERROR);
        assert_eq!(err.code(), codes::PERM_INVALID_STATE);
    }

    #[test]
    fn load_acl_tree_shapes_the_store_into_acl_nodes() {
        let mut doc = seed_doc();
        doc.pbs_acl.push(acl("n-cell-a1", "u-proc", "盖章", true));
        let store = store_with(doc);
        let nodes = load_acl_tree(&store).expect("tree");
        assert_eq!(nodes.len(), 7);

        let root = nodes
            .iter()
            .find(|node| node.node_id == "n-plant")
            .expect("root");
        assert!(root.parent_id.is_none());
        assert_eq!(root.entries.len(), 20, "five users × four permissions");

        let cell = nodes
            .iter()
            .find(|node| node.node_id == "n-cell-a1")
            .expect("cell");
        assert_eq!(cell.parent_id.as_deref(), Some("n-unit-a"));
        assert!(
            cell.entries.is_empty(),
            "the dirty permission literal is dropped"
        );
    }
}
