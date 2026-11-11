//! App-side glue for change broadcast: the outbound hooks every edit passes
//! through, the inbound fold that writes a peer's changes onto the open
//! drawing, and the read-only gate that keeps a downgraded session from
//! writing at all.
//!
//! The pure halves live in [`super::broadcast`]; this file only wires them to
//! `OpenCADStudio` and to the scene's redraw bookkeeping.

use crate::app::OpenCADStudio;

use super::broadcast;

impl OpenCADStudio {
    /// Broadcast the semantic content of one undo step to the room.
    ///
    /// Called from `push_undo_entry`, the single convergence point of every
    /// edit (`architecture.md` §5.3), so no per-command hook can be missed.
    pub(in crate::app) fn broadcast_history_delta(
        &mut self,
        i: usize,
        delta: &crate::app::tab_document::DeltaSnapshot,
    ) {
        self.broadcast_changes(i, broadcast::collect_changes(&delta.entities));
    }

    /// The tab holding the drawing the session speaks for, if it is open.
    ///
    /// Which tab that is gets settled the first time the session needs a
    /// document — to send an edit, or to take a peer's. An editor started on
    /// a file has a session for that file: it is the tab showing it, and
    /// until the file has loaded there is none — not the drawing that
    /// happens to be in front meanwhile. Started on no file, it is the
    /// drawing in front at that moment. From then on it is that tab whatever
    /// is in front, until the tab is closed; a session whose drawing was
    /// closed has no document, and says so rather than move on to the next
    /// one.
    pub(in crate::app) fn collab_tab(&mut self) -> Option<usize> {
        if let Some(id) = self.collab.document {
            return self.tabs.iter().position(|tab| tab.id == id);
        }
        let i = match self.collab.document_path.clone() {
            Some(path) => self.tab_showing(&path)?,
            None => self.active_tab,
        };
        if self.tabs.get(i).is_some_and(|tab| !tab.is_start) {
            self.collab.document = Some(self.tabs[i].id);
            return Some(i);
        }
        None
    }

    /// Whether tab `i` holds the session's drawing, settling it on `i` when
    /// the session has no document yet and `i` is the drawing in front.
    fn is_collab_tab(&mut self, i: usize) -> bool {
        self.collab_tab() == Some(i)
    }

    /// Queue one `op` carrying the `changes` made on tab `i`, or stay quiet.
    /// Four things silence it:
    ///
    /// * no live session — the offline-first invariant (`VAL-CLI-017`);
    /// * the changes come from a remote op being folded in
    ///   (`applying_remote`), which must not echo back to the peer that sent
    ///   it;
    /// * tab `i` is not the session's drawing: an edit to another sheet is
    ///   that sheet's own, and would reach the room as a change to this one;
    /// * the session is read-only, which may not write. The command gate
    ///   refuses a write before it happens, so a change that got here anyway
    ///   came in by a door the gate does not watch; it stays in this drawing
    ///   only, and the command line says so.
    ///
    /// Everything handed over in one call goes out as one op, which the server
    /// takes or refuses as a whole.
    pub(in crate::app) fn broadcast_changes(&mut self, i: usize, changes: Vec<broadcast::Change>) {
        if self.applying_remote || !self.collab.connected {
            return;
        }
        let Some(payload) = broadcast::payload_for(&changes) else {
            return;
        };
        if !self.is_collab_tab(i) {
            return;
        }
        if self.collab.readonly {
            self.command_line.push_warning(&format!(
                "协同：当前会话只读，本地的 {} 项变更没有广播，本图与其他人的图已不一致",
                changes.len()
            ));
            return;
        }
        match self.collab.request_op(payload) {
            Ok(()) => {
                self.command_line
                    .push_info(&format!("协同：已广播 {} 项变更", changes.len()));
            }
            Err(reason) => {
                self.command_line
                    .push_warning(&format!("协同：变更未广播（{reason}）"));
            }
        }
    }

    /// Fold every remote `op_applied` the state is holding onto the open
    /// drawing.
    ///
    /// Runs right after `collab.apply` so the drawing catches up on the same
    /// frame the roster and baseline do (`VAL-CLI-009`). Each op is one undo
    /// step, so a person can undo a change a peer made — and that undo is
    /// broadcast like any other (`undo_steps_reported`).
    ///
    /// While the session's drawing is not among the tabs — not loaded yet, or
    /// closed — the ops stay in `pending_remote`, in order, and `on_tick`
    /// comes back here every frame until there is a drawing to put them on.
    ///
    /// An op this drawing cannot follow — a change shape it does not know, or
    /// an object it does not have — is reported (`last_apply_error`, and on
    /// the command line) rather than passed over: from there on the two
    /// drawings differ, and nobody should have to find that out later.
    pub(in crate::app) fn apply_remote_changes(&mut self) {
        if self.collab.pending_remote.is_empty() {
            return;
        }
        // The session's drawing, wherever it is among the tabs — not the tab
        // in front, which may be another sheet carrying the same tags.
        let Some(i) = self.collab_tab() else {
            // Nowhere to put them yet. They stay queued: a session comes up
            // before the drawing it was started on has finished loading, and
            // what arrives in between must reach the drawing when it is there
            // (`on_tick` comes back here). Said once per change in the count,
            // not once per frame.
            let waiting = self.collab.pending_remote.len();
            let reason = if self.collab.document.is_some() {
                format!("协同的图纸已经关闭，{waiting} 个远端操作留在队列里，未应用")
            } else {
                format!("协同的图纸还没有打开，{waiting} 个远端操作在等它")
            };
            if self.collab.last_apply_error.as_deref() != Some(reason.as_str()) {
                self.report_apply_error(reason);
            }
            return;
        };
        let pending = std::mem::take(&mut self.collab.pending_remote);
        let mut total = 0usize;
        let mut failure: Option<String> = None;
        // Raised for as long as a step opened here can still reach
        // `push_undo_entry`: the fold must not go back out as this client's op.
        self.applying_remote = true;
        for remote in pending {
            let parsed = broadcast::changes_from_payload(&remote.changes);
            if parsed.is_empty() {
                // Reported, not guessed at: half-applying an op diverges the
                // two documents.
                failure = Some(format!("操作 {} 的变更无法识别，已跳过", remote.op_id));
                continue;
            }
            // Opening a step closes the one before it, so each op is its own.
            self.push_undo_snapshot(i, "COLLAB");
            let applied = {
                let document = &mut self.tabs[i].scene.document;
                broadcast::apply_changes(document, &parsed)
            };
            if !applied.unresolved.is_empty() {
                failure = Some(format!(
                    "操作 {}：本图找不到位号 {}，这部分变更未应用",
                    remote.op_id,
                    applied.unresolved.join("、")
                ));
            }
            if applied.is_empty() {
                self.discard_last_undo_entry(i);
                continue;
            }
            total += applied.handles.len();
            // What hangs off the changed objects is this drawing's own to
            // work out, as after a local edit: a pipe line's field goes down
            // to its runs and their segments. The sender states only the
            // tagged objects it changed — a segment has no tag to name it by
            // — so without this the segments here keep the old values.
            let carried_down = self.propagate_pid_chain(i, &applied.handles);
            let changes: Vec<(document::Handle, crate::scene::ChangeKind)> = applied
                .handles
                .iter()
                .chain(&carried_down)
                .map(|handle| (*handle, crate::scene::ChangeKind::Modified))
                .collect();
            self.tabs[i].scene.bump_entities(&changes);
            self.tabs[i].dirty = true;
            // Inside the step, as `refresh_pid_tags` asks of its caller: the
            // label and the uniqueness status then undo and redo with the tag.
            if !applied.retagged.is_empty() {
                self.refresh_pid_tags(i, &applied.retagged);
            }
        }
        self.finish_pending_history(i);
        self.applying_remote = false;
        if total > 0 {
            self.command_line
                .push_info(&format!("协同：已应用远端变更（{total} 个实体）"));
        }
        match failure {
            Some(reason) => self.report_apply_error(reason),
            None if total > 0 => self.collab.last_apply_error = None,
            None => {}
        }
    }

    /// Record why a remote change did not reach this drawing, where the status
    /// endpoint and the person at the command line both see it.
    fn report_apply_error(&mut self, reason: String) {
        self.command_line.push_warning(&format!("协同：{reason}"));
        self.collab.last_apply_error = Some(reason);
    }

    /// Whether a command may not run because the session is read-only.
    ///
    /// Navigation and inspection keep working — a read-only participant still
    /// has to look around the drawing — while anything that would change the
    /// document is refused (`VAL-CLI-011`). The allowlist is deliberately
    /// explicit: a new drawing command defaults to refused, which is the safe
    /// direction for a write gate.
    ///
    /// `PIDTAG` is the one verb whose argument count changes its meaning: one
    /// word jumps to a tag (a read), two rename it (a write).
    ///
    /// The gate is on the session's drawing. Another drawing open beside it
    /// is not in the session, and its commands are its own; while the session
    /// has no document yet, the drawing in front is the one it would take.
    pub(in crate::app) fn readonly_refuses_command(&self, cmd: &str) -> bool {
        if !self.collab.readonly {
            return false;
        }
        let in_front = self.tabs.get(self.active_tab).map(|tab| tab.id);
        if self.collab.document.is_some() && self.collab.document != in_front {
            return false;
        }
        let words: Vec<&str> = cmd.split_whitespace().collect();
        let verb = words.first().copied().unwrap_or("").to_ascii_uppercase();
        if verb == "PIDTAG" && words.len() > 2 {
            return true;
        }
        !READONLY_ALLOWED.contains(&verb.as_str())
    }
}

/// Commands a read-only session may still run: looking around, inspecting, and
/// the collaboration panel itself. Everything else is a write.
pub(super) const READONLY_ALLOWED: &[&str] = &[
    "ZOOM",
    "ZO",
    "PAN",
    "P",
    "REGEN",
    "RE",
    "REGENALL",
    "UCS",
    "UCSICON",
    "ORTHO",
    "OSNAP",
    "GRID",
    "SNAP",
    "LIST",
    "LI",
    "DIST",
    "DI",
    "AREA",
    "AA",
    "ID",
    "MEASURE",
    "MEASUREGEOM",
    "PROPERTIES",
    "PROPS",
    "PR",
    "QSELECT",
    "QS",
    "SELECTALL",
    "FIND",
    "HELP",
    "PIDLEGEND",
    "PIDLINE",
    "PIDTAG",
    "PIDGROUP",
    "PIDTOPOLOGY",
    "PIDPALETTE",
    "PIDCOLLAB",
    "PIDCLAIM",
    "PIDRELEASE",
    "PIDPLOT",
    "PLOT",
    "PRINT",
    "PREVIEW",
    "AUDIT",
    "PERF",
    "LAYERS",
    "LA",
    "LAYER",
    "LINETYPE",
    "LT",
    "SAVEAS",
    "QUIT",
    "EXIT",
];

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::collab::CollabEvent;
    use crate::pid::persist::PidAuthor;
    use document::entities::Insert;
    use document::types::Vector3;

    fn app_with_vessel(tag: &str) -> (OpenCADStudio, document::Handle) {
        let mut app = OpenCADStudio::new_for_test();
        let _ = app.automation_op(r#"{"op":"new"}"#);
        let i = app.active_tab;
        let handle = {
            let document = &mut app.tabs[i].scene.document;
            let handle = document
                .add_entity(document::EntityType::Insert(Insert::new(
                    "PID_SYM_equipment_vessel",
                    Vector3::ZERO,
                )))
                .expect("entity");
            PidAuthor {
                type_code: "EQUIPMENT".to_owned(),
                tag: tag.to_owned(),
                tag_source: "auto".to_owned(),
                tag_status: "ok".to_owned(),
                props: r#"{"设计压力":"1.6MPa"}"#.to_owned(),
                ..PidAuthor::default()
            }
            .store(document, handle);
            handle
        };
        (app, handle)
    }

    /// VAL-CLI-009: a remote `op_applied` naming a tag rewrites the local
    /// document, and the label follows.
    #[test]
    fn a_remote_tag_change_reaches_the_local_document() {
        let (mut app, handle) = app_with_vessel("P-1001");
        app.collab.user_id = "u2".to_owned();
        app.collab.apply(CollabEvent::OpApplied {
            op_id: "op-peer-1".to_owned(),
            revision: 1,
            changes: vec![serde_json::json!({
                "kind": "set_tag",
                "object_ref": "P-1001",
                "handle": format!("{:X}", handle.value()),
                "field": "tag",
                "value": "P-1009",
                "old_value": "P-1001",
            })],
        });
        app.apply_remote_changes();

        let author =
            PidAuthor::load(&app.tabs[app.active_tab].scene.document, handle).expect("author");
        assert_eq!(author.tag, "P-1009");
        assert_eq!(app.collab.base_revision, 1);
        assert_eq!(app.collab.applied_ops, 1);
        assert!(app.collab.pending_remote.is_empty());
    }

    /// VAL-CLI-010: the baseline moves with the broadcast even for a remote op.
    #[test]
    fn a_remote_op_moves_the_baseline_to_the_server_revision() {
        let (mut app, handle) = app_with_vessel("P-1001");
        app.collab.apply(CollabEvent::OpApplied {
            op_id: "op-peer-2".to_owned(),
            revision: 2,
            changes: vec![serde_json::json!({
                "kind": "set_property",
                "object_ref": "P-1001",
                "handle": format!("{:X}", handle.value()),
                "field": "设计压力",
                "value": "2.5MPa",
            })],
        });
        assert_eq!(app.collab.base_revision, 2);
        app.apply_remote_changes();
        let author =
            PidAuthor::load(&app.tabs[app.active_tab].scene.document, handle).expect("author");
        assert!(author.props.contains("2.5MPa"), "{}", author.props);
    }

    /// Our own op comes back from the room broadcast; applying it again would
    /// double-count and, worse, re-open an undo step for a change already in
    /// the document.
    #[test]
    fn our_own_op_is_not_applied_twice() {
        let (mut app, handle) = app_with_vessel("P-1001");
        app.collab.user_id = "u1".to_owned();
        app.collab.own_ops.push_back("op-mine".to_owned());
        app.collab.apply(CollabEvent::OpApplied {
            op_id: "op-mine".to_owned(),
            revision: 1,
            changes: vec![serde_json::json!({
                "kind": "set_tag",
                "object_ref": "P-1001",
                "handle": format!("{:X}", handle.value()),
                "field": "tag",
                "value": "P-1009",
            })],
        });
        assert!(app.collab.pending_remote.is_empty(), "自己的 op 不该重放");
        assert_eq!(app.collab.base_revision, 1, "基线仍要跟着走");
        assert!(app.collab.own_ops.is_empty(), "已回来的 op id 要出队");
    }

    /// Applying a remote change must not echo back to the room.
    ///
    /// The assertion is on `own_ops` rather than the outbound queue: the queue
    /// is process-global and other tests in this binary write to it, while
    /// `own_ops` only grows when *this* app queues an op.
    #[test]
    fn folding_a_remote_change_does_not_broadcast_it_back() {
        let (mut app, handle) = app_with_vessel("P-1001");
        app.collab.connected = true;
        app.collab.session_id = Some("s-2".to_owned());
        app.collab.apply(CollabEvent::OpApplied {
            op_id: "op-peer-3".to_owned(),
            revision: 1,
            changes: vec![serde_json::json!({
                "kind": "set_tag",
                "object_ref": "P-1001",
                "handle": format!("{:X}", handle.value()),
                "field": "tag",
                "value": "P-1009",
            })],
        });
        app.apply_remote_changes();
        assert!(
            app.collab.own_ops.is_empty(),
            "远端变更不应回灌成自己的 op：{:?}",
            app.collab.own_ops
        );
        // And the fold really did land, so the assertion above is not vacuous.
        assert_eq!(
            PidAuthor::load(&app.tabs[app.active_tab].scene.document, handle)
                .expect("author")
                .tag,
            "P-1009"
        );
    }

    /// VAL-CLI-011: a downgraded session refuses a write command and leaves
    /// the record alone.
    #[test]
    fn a_readonly_session_refuses_writes_and_keeps_inspection() {
        let (mut app, _) = app_with_vessel("P-1001");
        // A two-word PIDTAG is a retag, which is a write.
        assert!(!app.readonly_refuses_command("PIDTAG P-1001 P-1009"));

        app.collab.readonly = true;
        assert!(app.readonly_refuses_command("PIDTAG P-1001 P-1009"));
        assert!(app.readonly_refuses_command("LINE 0,0 10,0"));
        assert!(app.readonly_refuses_command("ERASE"));
        // Looking around and reading stay available.
        assert!(!app.readonly_refuses_command("ZOOM EXTENTS"));
        assert!(!app.readonly_refuses_command("PIDCOLLAB ON"));
        assert!(!app.readonly_refuses_command("LIST"));
        // Jumping to a tag by name is navigation, not a write.
        assert!(!app.readonly_refuses_command("PIDTAG P-1001"));
    }

    /// What the last undo step of the active drawing would put on the wire.
    fn last_step_payload(app: &OpenCADStudio) -> serde_json::Value {
        let i = app.active_tab;
        let Some(crate::app::tab_document::HistorySnapshot::Delta(delta)) =
            app.tabs[i].history.undo_stack.last()
        else {
            panic!("the last undo step is not a delta");
        };
        broadcast::payload_for(&broadcast::collect_changes(&delta.entities))
            .expect("the step carries a semantic change")
    }

    /// The server and every other client know an object by its tag
    /// (`architecture.md` §3.2), so that is what an op names it by; the entity
    /// handle rides along only as a hint for peers holding the same file.
    #[test]
    fn a_local_retag_names_its_object_by_the_tag_it_had() {
        let (mut app, handle) = app_with_vessel("P-1001");
        let _ = app.run_command_line("PIDTAG P-1001 P-1009");
        app.finish_all_pending_history();

        let payload = last_step_payload(&app);
        assert_eq!(payload["kind"], "set_tag");
        assert_eq!(payload["object_ref"], "P-1001", "{payload}");
        assert_eq!(payload["field"], "tag");
        assert_eq!(payload["value"], "P-1009");
        assert_eq!(payload["old_value"], "P-1001", "{payload}");
        assert_eq!(
            payload["handle"],
            format!("{:X}", handle.value()),
            "{payload}"
        );
    }

    /// A step that changes several properties of one symbol broadcasts all of
    /// them, or the peers keep whatever the others were.
    #[test]
    fn every_property_a_step_changes_is_broadcast() {
        let (mut app, handle) = app_with_vessel("P-1001");
        // The contract's own write channel for properties:
        // `PUT /api/v1/entities/{h}/xdata/PID_AUTHOR` carrying `props`.
        let state = app.control_state();
        let (response, task) = app.control_request(serde_json::json!({
            "protocol": 1,
            "op": "xdata_set",
            "request_id": "collab-props-1",
            "client_id": "test",
            "document_id": state["document_id"],
            "revision": state["revision"],
            "handles": [format!("{:X}", handle.value())],
            "app": "PID_AUTHOR",
            "data": [{
                "code": 1000,
                "value": r#"props={"设计压力":"2.5MPa","设计温度":"120℃"}"#,
            }],
        }));
        app.drive_headless_task(task).unwrap();
        assert_eq!(response["ok"], true, "{response}");
        app.finish_all_pending_history();

        let payload = last_step_payload(&app);
        let mut fields = vec![payload["field"].as_str().unwrap_or("").to_owned()];
        fields.extend(
            payload["changes"]
                .as_array()
                .into_iter()
                .flatten()
                .map(|change| change["field"].as_str().unwrap_or("").to_owned()),
        );
        fields.sort();
        assert_eq!(fields, ["设计压力", "设计温度"], "{payload}");
        assert_eq!(payload["object_ref"], "P-1001", "{payload}");
    }

    /// A change that names its object by tag alone — what a client with no
    /// drawing of its own sends — is applied to the symbol carrying that tag.
    #[test]
    fn a_remote_change_naming_only_a_tag_reaches_the_symbol_carrying_it() {
        let (mut app, handle) = app_with_vessel("P-1001");
        app.collab.apply(CollabEvent::OpApplied {
            op_id: "op-peer-9".to_owned(),
            revision: 1,
            changes: vec![serde_json::json!({
                "kind": "set_property",
                "object_ref": "P-1001",
                "field": "设计压力",
                "value": "2.5MPa",
            })],
        });
        app.apply_remote_changes();

        let author =
            PidAuthor::load(&app.tabs[app.active_tab].scene.document, handle).expect("author");
        assert!(author.props.contains("2.5MPa"), "{}", author.props);
        assert_eq!(app.collab.last_apply_error, None);
    }

    /// Undoing a change puts the old value back in this drawing, so the room
    /// has to hear about it as well; redoing it likewise.
    #[test]
    fn undo_and_redo_are_broadcast_like_the_edit_they_replay() {
        let (mut app, _) = app_with_vessel("P-1001");
        app.collab.connected = true;
        app.collab.session_id = Some("s-1".to_owned());

        let _ = app.run_command_line("PIDTAG P-1001 P-1009");
        app.finish_all_pending_history();
        assert_eq!(app.collab.own_ops.len(), 1, "the edit itself");

        let _ = app.automation_op(r#"{"op":"undo"}"#);
        assert_eq!(app.collab.own_ops.len(), 2, "the undo");

        let _ = app.automation_op(r#"{"op":"redo"}"#);
        assert_eq!(app.collab.own_ops.len(), 3, "the redo");
    }

    /// The gate runs before the command body, so a refused write leaves the
    /// stored tag untouched.
    #[test]
    fn a_refused_write_leaves_the_tag_unchanged() {
        let (mut app, handle) = app_with_vessel("P-1001");
        app.collab.readonly = true;
        let refused = app.automation_op(r#"{"op":"run","cmd":"PIDTAG P-1001 P-1009"}"#);
        assert_eq!(refused["ok"], false, "{refused}");
        let author =
            PidAuthor::load(&app.tabs[app.active_tab].scene.document, handle).expect("author");
        assert_eq!(author.tag, "P-1001", "只读拒绝后位号不得改写");
    }

    /// Open another drawing in a tab of its own, put a vessel carrying `tag`
    /// on it and bring it to the front. Returns the tab and the vessel.
    fn another_drawing_in_front(app: &mut OpenCADStudio, tag: &str) -> (usize, document::Handle) {
        let id = app.push_test_document();
        let tab = app
            .tabs
            .iter()
            .position(|tab| tab.id == id)
            .expect("the new tab");
        let document = &mut app.tabs[tab].scene.document;
        let handle = document
            .add_entity(document::EntityType::Insert(Insert::new(
                "PID_SYM_equipment_vessel",
                Vector3::ZERO,
            )))
            .expect("entity");
        PidAuthor {
            type_code: "EQUIPMENT".to_owned(),
            tag: tag.to_owned(),
            tag_source: "auto".to_owned(),
            tag_status: "ok".to_owned(),
            ..PidAuthor::default()
        }
        .store(document, handle);
        app.active_tab = tab;
        (tab, handle)
    }

    fn remote_retag(app: &mut OpenCADStudio, op_id: &str, revision: i64, old: &str, new: &str) {
        app.collab.apply(CollabEvent::OpApplied {
            op_id: op_id.to_owned(),
            revision,
            changes: vec![serde_json::json!({
                "kind": "set_tag",
                "object_ref": old,
                "field": "tag",
                "value": new,
            })],
        });
        app.apply_remote_changes();
    }

    /// The session speaks for one drawing. A peer's change belongs on the tab
    /// holding that drawing, not on whichever tab is in front when it
    /// arrives — which may well carry the same tag: it can be a copy of the
    /// same sheet.
    #[test]
    fn a_remote_change_lands_on_the_sessions_drawing_not_on_the_tab_in_front() {
        let (mut app, vessel) = app_with_vessel("P-1001");
        let session_tab = app.active_tab;
        // The first change arrives with the session's drawing in front.
        remote_retag(&mut app, "op-peer-a", 1, "P-1001", "P-1005");
        assert_eq!(
            PidAuthor::load(&app.tabs[session_tab].scene.document, vessel)
                .expect("author")
                .tag,
            "P-1005"
        );

        // Then the person opens a copy of the sheet and works in that.
        let (copy_tab, copy_vessel) = another_drawing_in_front(&mut app, "P-1005");
        remote_retag(&mut app, "op-peer-b", 2, "P-1005", "P-1009");

        assert_eq!(
            PidAuthor::load(&app.tabs[copy_tab].scene.document, copy_vessel)
                .expect("author")
                .tag,
            "P-1005",
            "the tab in front is not the session's drawing and must be left alone"
        );
        assert_eq!(
            PidAuthor::load(&app.tabs[session_tab].scene.document, vessel)
                .expect("author")
                .tag,
            "P-1009",
            "the session's drawing takes the change while it is in the background"
        );
        assert_eq!(app.collab.last_apply_error, None);
    }

    /// And the other way: an edit made on another drawing is that drawing's
    /// business. Broadcast, it would tell the room the session's drawing
    /// changed.
    #[test]
    fn an_edit_on_another_drawing_is_not_broadcast_as_the_sessions() {
        let (mut app, _) = app_with_vessel("P-1001");
        app.collab.connected = true;
        app.collab.session_id = Some("s-1".to_owned());
        let _ = app.run_command_line("PIDTAG P-1001 P-1005");
        app.finish_all_pending_history();
        assert_eq!(app.collab.own_ops.len(), 1, "the session's drawing");

        let (copy_tab, copy_vessel) = another_drawing_in_front(&mut app, "P-2001");
        let _ = app.run_command_line("PIDTAG P-2001 P-2009");
        app.finish_all_pending_history();
        assert_eq!(
            PidAuthor::load(&app.tabs[copy_tab].scene.document, copy_vessel)
                .expect("author")
                .tag,
            "P-2009",
            "the edit itself goes through"
        );
        assert_eq!(
            app.collab.own_ops.len(),
            1,
            "an edit to another drawing went out as this session's: {:?}",
            app.collab.own_ops
        );

        // Taking that edit back and putting it on again is the other
        // drawing's business too.
        let _ = app.automation_op(r#"{"op":"undo"}"#);
        assert_eq!(
            PidAuthor::load(&app.tabs[copy_tab].scene.document, copy_vessel)
                .expect("author")
                .tag,
            "P-2001",
            "the undo itself goes through"
        );
        assert_eq!(app.collab.own_ops.len(), 1, "its undo went out");
        let _ = app.automation_op(r#"{"op":"redo"}"#);
        assert_eq!(
            PidAuthor::load(&app.tabs[copy_tab].scene.document, copy_vessel)
                .expect("author")
                .tag,
            "P-2009"
        );
        assert_eq!(app.collab.own_ops.len(), 1, "its redo went out");
    }

    /// The session comes up before the drawing it was started on has finished
    /// loading. What a peer does in between waits for the drawing; it is not
    /// thrown away, and the next frame after the drawing is there applies it.
    #[test]
    fn a_change_that_arrives_before_the_drawing_is_open_waits_for_it() {
        let mut app = OpenCADStudio::new_for_test();
        assert!(app.tabs[app.active_tab].is_start, "only the start page so far");
        app.collab.apply(CollabEvent::OpApplied {
            op_id: "op-early".to_owned(),
            revision: 1,
            changes: vec![serde_json::json!({
                "kind": "set_tag", "object_ref": "P-1001", "field": "tag", "value": "P-1009",
            })],
        });
        app.apply_remote_changes();
        assert_eq!(app.collab.pending_remote.len(), 1, "kept, not dropped");
        assert_eq!(app.collab.document, None, "the start page is not a drawing");
        let said = app.collab.last_apply_error.clone().expect("it says it is waiting");
        // Asked again on the next frame, it has nothing new to say.
        app.apply_remote_changes();
        assert_eq!(app.collab.last_apply_error.as_deref(), Some(said.as_str()));
        assert_eq!(app.collab.pending_remote.len(), 1);

        // The drawing opens.
        let _ = app.automation_op(r#"{"op":"new"}"#);
        let tab = app.active_tab;
        let vessel = {
            let document = &mut app.tabs[tab].scene.document;
            let handle = document
                .add_entity(document::EntityType::Insert(Insert::new(
                    "PID_SYM_equipment_vessel",
                    Vector3::ZERO,
                )))
                .expect("entity");
            PidAuthor {
                type_code: "EQUIPMENT".to_owned(),
                tag: "P-1001".to_owned(),
                tag_source: "auto".to_owned(),
                tag_status: "ok".to_owned(),
                ..PidAuthor::default()
            }
            .store(document, handle);
            handle
        };
        let _ = app.update(crate::app::Message::Tick(iced::time::Instant::now()));

        assert_eq!(
            PidAuthor::load(&app.tabs[tab].scene.document, vessel)
                .expect("author")
                .tag,
            "P-1009",
            "the frame after the drawing opened applies what was waiting"
        );
        assert!(app.collab.pending_remote.is_empty());
        assert_eq!(app.collab.last_apply_error, None);
        assert_eq!(app.collab.document, Some(app.tabs[tab].id));
    }

    /// An editor started on a file has a session for that file. While it is
    /// still loading, a drawing that happens to be in front — another file of
    /// the same launch, a new one — is not it: the change waits, and goes to
    /// the tab showing the file once there is one.
    #[test]
    fn a_session_started_on_a_file_waits_for_that_file_not_the_tab_in_front() {
        let dir = std::env::temp_dir().join(format!("ocs-collab-bind-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("temp dir");
        let file = dir.join("PID-1101.dwg");
        std::fs::write(&file, b"").expect("the file the launch named");

        let mut app = OpenCADStudio::new_for_test();
        app.collab.document_path = Some(file.clone());
        // Something else is open and in front before the file has loaded.
        let (other_tab, other_vessel) = another_drawing_in_front(&mut app, "P-1001");
        remote_retag(&mut app, "op-early", 1, "P-1001", "P-1009");
        assert_eq!(app.collab.pending_remote.len(), 1, "it waits for the file");
        assert_eq!(app.collab.document, None, "the tab in front is not the session's");
        assert_eq!(
            PidAuthor::load(&app.tabs[other_tab].scene.document, other_vessel)
                .expect("author")
                .tag,
            "P-1001"
        );

        // The file finishes loading into a tab of its own, behind.
        let (file_tab, file_vessel) = another_drawing_in_front(&mut app, "P-1001");
        app.tabs[file_tab].current_path = Some(file);
        app.active_tab = other_tab;
        let _ = app.update(crate::app::Message::Tick(iced::time::Instant::now()));
        let _ = std::fs::remove_dir_all(&dir);

        assert_eq!(app.collab.document, Some(app.tabs[file_tab].id));
        assert!(app.collab.pending_remote.is_empty());
        assert_eq!(
            PidAuthor::load(&app.tabs[file_tab].scene.document, file_vessel)
                .expect("author")
                .tag,
            "P-1009",
            "the change reaches the file the session was started on"
        );
        assert_eq!(
            PidAuthor::load(&app.tabs[other_tab].scene.document, other_vessel)
                .expect("author")
                .tag,
            "P-1001",
            "and no other drawing"
        );
    }

    /// Once the session's drawing is closed there is nowhere to put a peer's
    /// change. It stays queued and is said to be; it does not move on to
    /// whatever drawing is open instead.
    #[test]
    fn a_change_for_a_closed_drawing_stays_queued_and_touches_no_other() {
        let (mut app, _) = app_with_vessel("P-1001");
        remote_retag(&mut app, "op-peer-a", 1, "P-1001", "P-1005");
        let session_id = app.tabs[app.active_tab].id;
        let (_, copy_vessel) = another_drawing_in_front(&mut app, "P-1005");
        let copy_id = app.tabs[app.active_tab].id;
        // The session's drawing is closed; the copy is what is left. (Saved
        // first as far as the tab is concerned, so the close is not held up
        // by the question about unsaved changes.)
        let session_tab = app
            .tabs
            .iter()
            .position(|tab| tab.id == session_id)
            .expect("the session's tab");
        app.tabs[session_tab].dirty = false;
        let _ = app.update(crate::app::Message::TabClose(session_id));
        assert!(
            app.tabs.iter().all(|tab| tab.id != session_id),
            "the session's drawing is closed"
        );
        let copy_tab = app
            .tabs
            .iter()
            .position(|tab| tab.id == copy_id)
            .expect("the copy is still open");
        remote_retag(&mut app, "op-peer-b", 2, "P-1005", "P-1009");
        assert_eq!(app.collab.pending_remote.len(), 1);
        assert!(app
            .collab
            .last_apply_error
            .as_deref()
            .is_some_and(|said| said.contains("已经关闭")));
        assert_eq!(
            PidAuthor::load(&app.tabs[copy_tab].scene.document, copy_vessel)
                .expect("author")
                .tag,
            "P-1005"
        );
    }

    /// A claim is on a tag of the session's drawing. Another sheet open
    /// beside it that carries the same tag is not occupied by anyone.
    #[test]
    fn a_claim_colours_the_sessions_drawing_only() {
        let (mut app, vessel) = app_with_vessel("P-1001");
        let session_tab = app.active_tab;
        app.collab.document = Some(app.tabs[session_tab].id);
        let (copy_tab, _) = another_drawing_in_front(&mut app, "P-1001");
        app.collab.claims.insert("P-1001".to_owned(), "u1".to_owned());
        app.sync_claim_colors();
        let coloured = app.tabs[session_tab]
            .scene
            .claim_display
            .as_ref()
            .expect("the session's drawing shows the claim");
        assert!(coloured.handles.contains(&vessel));
        assert!(
            app.tabs[copy_tab].scene.claim_display.is_none(),
            "a sheet outside the session was coloured as occupied"
        );
    }

    /// A read-only session may not write to its drawing. Another drawing
    /// open beside it is not in the session and stays writable.
    #[test]
    fn a_readonly_session_leaves_other_drawings_writable() {
        let (mut app, _) = app_with_vessel("P-1001");
        // Settle which drawing the session is about, then lose write access.
        remote_retag(&mut app, "op-peer-a", 1, "P-1001", "P-1005");
        app.collab.readonly = true;
        assert!(app.readonly_refuses_command("PIDTAG P-1005 P-1009"));

        let _ = another_drawing_in_front(&mut app, "P-2001");
        assert!(
            !app.readonly_refuses_command("PIDTAG P-2001 P-2009"),
            "a drawing outside the session was refused a write"
        );
    }
}
