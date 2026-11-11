//! App-side glue for change broadcast: the outbound hooks every edit passes
//! through, the inbound fold that writes a peer's changes onto the open
//! drawing, and the read-only gate that keeps a downgraded session from
//! writing at all.
//!
//! The pure halves live in [`super::broadcast`]; this file only wires them to
//! `OpenCADStudio` and to the scene's redraw bookkeeping.

use crate::app::OpenCADStudio;

use super::broadcast;

/// What an undo or redo on the session's drawing keeps from before its steps
/// are applied, for `replay_remote_after`.
pub(in crate::app) struct HistoryGuard {
    /// The lowest `remote_mark` among the steps: the peers' changes later
    /// than this are the ones the steps' images do not hold.
    mark: u64,
    /// Every entity the steps or those later changes touch, as it was.
    before: Vec<(
        document::Handle,
        Option<std::sync::Arc<document::EntityType>>,
    )>,
}

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
    pub(in crate::app) fn is_collab_tab(&mut self, i: usize) -> bool {
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
    /// frame the roster and baseline do (`VAL-CLI-009`).
    ///
    /// A peer's change is not a step on this person's undo stack: undo takes
    /// back what they did themselves, and never a colleague's edit (which it
    /// used to, the newest step being whatever arrived last — and it went
    /// out as this person's op). Nor does it clear their redo stack. What
    /// keeps a local undo from wiping a peer's change off an entity both
    /// touched is `replay_remote_after`.
    ///
    /// A local step still open when a change arrives is closed first, as
    /// the user's own: it is broadcast like any other step.
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
        // The user's own step, if one is open, ends here and goes out as
        // theirs — before the flag below is raised, which would keep it in.
        // Left open it would record the peer's writes as part of itself.
        self.finish_pending_history(i);
        // Raised while the peers' changes are written: nothing done here is
        // this client's to send.
        self.applying_remote = true;
        for remote in pending {
            let read = broadcast::changes_from_payload(&remote.changes);
            if !read.unread.is_empty() {
                // Said, whether or not the rest of the op can be applied:
                // the room has these changes and this drawing does not.
                failure.get_or_insert(format!(
                    "操作 {} 里有 {} 项变更本客户端不认识（{}），没有应用",
                    remote.op_id,
                    read.unread.len(),
                    read.unread.join("、")
                ));
            }
            let parsed = read.changes;
            if parsed.is_empty() {
                continue;
            }
            let applied = {
                let document = &mut self.tabs[i].scene.document;
                broadcast::apply_changes(document, &parsed)
            };
            if !applied.unresolved.is_empty() {
                failure.get_or_insert(format!(
                    "操作 {}：位号 {} 在本图里对不上一个对象（没有，或者有好几个），这部分变更未应用",
                    remote.op_id,
                    applied.unresolved.join("、")
                ));
            }
            // Kept, with the entities each was written to, for the undo and
            // redo that come later (`replay_remote_after`).
            for (change, handles) in &applied.resolved {
                self.collab.remote_seq += 1;
                self.collab.remote_log.push(super::AppliedRemote {
                    seq: self.collab.remote_seq,
                    handles: handles.clone(),
                    change: change.clone(),
                });
            }
            if applied.is_empty() {
                continue;
            }
            total += applied.handles.len();
            self.settle_remote_writes(i, &applied.handles, &applied.retagged);
        }
        self.applying_remote = false;
        self.prune_remote_log(i);
        if total > 0 {
            self.refresh_properties();
            self.command_line
                .push_info(&format!("协同：已应用远端变更（{total} 个实体）"));
        }
        match failure {
            Some(reason) => {
                // The room has a change this drawing does not. Its baseline
                // moved all the same — that is the server's count — so the
                // session is marked, and writes nothing more on top of the
                // difference (`State::request_op`).
                let first = self.collab.diverged.is_none();
                self.collab.mark_diverged(reason.clone());
                self.report_apply_error(reason);
                if first {
                    self.command_line.push_warning(
                        "协同：本图与房间已不一致，此后的本地修改不再提交；\
                         请取得最新图纸后重新打开",
                    );
                }
            }
            None if total > 0 => self.collab.last_apply_error = None,
            None => {}
        }
    }

    /// What follows a peer's change having been written to `handles` on tab
    /// `i`, outside any undo step.
    ///
    /// What hangs off the changed objects is this drawing's own to work out,
    /// as after a local edit: a pipe line's field goes down to its runs and
    /// their segments (the sender states only the tagged objects it changed
    /// — a segment has no tag to name it by), and a tag that moved takes its
    /// label and its uniqueness status with it.
    fn settle_remote_writes(
        &mut self,
        i: usize,
        handles: &[document::Handle],
        retagged: &[document::Handle],
    ) {
        let carried_down = self.propagate_pid_chain(i, handles);
        let changes: Vec<(document::Handle, crate::scene::ChangeKind)> = handles
            .iter()
            .chain(&carried_down)
            .map(|handle| (*handle, crate::scene::ChangeKind::Modified))
            .collect();
        self.tabs[i].scene.bump_entities(&changes);
        self.tabs[i].dirty = true;
        if !retagged.is_empty() {
            self.refresh_pid_tags(i, retagged);
        }
        // The drawing changed without a step being pushed, which is what
        // normally moves this on.
        self.tabs[i].edit_revision = self.tabs[i].edit_revision.wrapping_add(1);
    }

    /// The `remote_mark` of a step being pushed on tab `i`: how many of the
    /// peers' changes the session's drawing has taken so far. A step on any
    /// other drawing has none to count.
    pub(in crate::app) fn collab_remote_mark(&self, i: usize) -> u64 {
        match self.collab.document {
            Some(id) if self.tabs.get(i).is_some_and(|tab| tab.id == id) => self.collab.remote_seq,
            _ => 0,
        }
    }

    /// Before the top `steps` steps of tab `i`'s undo stack (or redo stack)
    /// are applied: what [`Self::replay_remote_after`] will need afterwards.
    /// `None` when no peer's change is later than those steps — an undo then
    /// is what it always was.
    pub(in crate::app) fn history_guard(
        &self,
        i: usize,
        undo: bool,
        steps: usize,
    ) -> Option<HistoryGuard> {
        let tab = self.tabs.get(i)?;
        if self.collab.remote_log.is_empty() || self.collab.document != Some(tab.id) {
            return None;
        }
        let stack = if undo {
            &tab.history.undo_stack
        } else {
            &tab.history.redo_stack
        };
        let mut mark = None;
        let mut handles: Vec<document::Handle> = Vec::new();
        for snapshot in stack.iter().rev().take(steps) {
            if let crate::app::tab_document::HistorySnapshot::Delta(delta) = snapshot {
                mark = Some(mark.map_or(delta.remote_mark, |lowest: u64| lowest.min(delta.remote_mark)));
                handles.extend(delta.entities.iter().map(|(handle, _, _)| *handle));
            }
        }
        let mark = mark?;
        let later = self.collab.remote_log.iter().filter(|entry| entry.seq > mark);
        let before_later = handles.len();
        handles.extend(later.flat_map(|entry| entry.handles.iter().copied()));
        if handles.len() == before_later {
            return None;
        }
        handles.sort();
        handles.dedup();
        let document = &tab.scene.document;
        Some(HistoryGuard {
            mark,
            before: handles
                .into_iter()
                .map(|handle| (handle, document.get_entity_arc(handle)))
                .collect(),
        })
    }

    /// After local steps were taken back or put on again on tab `i`: write
    /// the peers' changes that came later than those steps once more, and
    /// say what the whole thing amounts to for the room.
    ///
    /// A step restores the entities it touched to images from when it was
    /// made. A peer's change made since to one of those entities is in
    /// neither image, so it is written again here — to the entities it was
    /// written to when it arrived, in the order the changes came. What is
    /// then sent to the room is this drawing before the undo against this
    /// drawing now: only what really changed. A field a peer has since set
    /// again stays as the peer set it, and nothing is said of it.
    pub(in crate::app) fn replay_remote_after(
        &mut self,
        i: usize,
        guard: HistoryGuard,
    ) -> Vec<broadcast::Change> {
        let later: Vec<super::AppliedRemote> = self
            .collab
            .remote_log
            .iter()
            .filter(|entry| entry.seq > guard.mark)
            .cloned()
            .collect();
        let mut applied = broadcast::Applied::default();
        let mut targets: Vec<document::Handle> = Vec::new();
        {
            let document = &mut self.tabs[i].scene.document;
            for entry in &later {
                broadcast::write_to(document, &entry.handles, &entry.change, &mut applied);
                targets.extend(
                    entry
                        .handles
                        .iter()
                        .copied()
                        .filter(|handle| document.get_entity(*handle).is_some()),
                );
            }
        }
        targets.sort();
        targets.dedup();
        // Carried down again whether or not anything had to be rewritten:
        // the step may have put back an entity that only *derives* from one
        // a peer changed — a segment under a line whose field moved — and
        // that is in no entry of the log.
        self.settle_remote_writes(i, &targets, &applied.retagged);
        self.refresh_properties();
        let document = &self.tabs[i].scene.document;
        let images: Vec<_> = guard
            .before
            .into_iter()
            .map(|(handle, before)| (handle, before, document.get_entity_arc(handle)))
            .collect();
        broadcast::collect_changes(&images)
    }

    /// Drop the part of `remote_log` no step can ask for any more: changes
    /// no later than the oldest step still on tab `i`'s undo or redo stack.
    fn prune_remote_log(&mut self, i: usize) {
        const KEEP_WITHOUT_ASKING: usize = 1024;
        if self.collab.remote_log.len() <= KEEP_WITHOUT_ASKING {
            return;
        }
        let history = &self.tabs[i].history;
        let oldest = history
            .undo_stack
            .iter()
            .chain(&history.redo_stack)
            .filter_map(|snapshot| match snapshot {
                crate::app::tab_document::HistorySnapshot::Delta(delta) => Some(delta.remote_mark),
                _ => None,
            })
            .min()
            .unwrap_or(self.collab.remote_seq);
        self.collab.remote_log.retain(|entry| entry.seq > oldest);
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
    /// Two verbs read or write by what follows them, and are let through
    /// only in the forms that read: `PIDTAG` with one word jumps to a tag
    /// (two rename it), and `PIDLEGEND` reports, lists and exports (`ON`
    /// draws the legend, `OFF` and `PURGE` erase it — and the bare verb
    /// defaults to `ON`).
    ///
    /// This list is a courtesy, not the lock: a command that does write is
    /// better turned away before it prompts for anything than after. What
    /// keeps a read-only session from changing its drawing is
    /// [`Self::refuses_step`], at the one place every edit passes through;
    /// a verb wrongly let through here still changes nothing.
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
        let option = words.get(1).copied().unwrap_or("").to_ascii_uppercase();
        match verb.as_str() {
            "PIDTAG" => words.len() > 2,
            "PIDLEGEND" => !matches!(option.as_str(), "REPORT" | "LIST" | "EXPORT"),
            _ => !READONLY_ALLOWED.contains(&verb.as_str()),
        }
    }

    /// Turn `cmd` away if this session may not write and `cmd` would.
    ///
    /// Says so on the command line under the code the refusing tier calls
    /// for (`State::refusal`) — which is what a caller over REST is answered
    /// with — and tells the server, so the attempt is on record there with
    /// the object it was aimed at (`VAL-CROSS-004`, `-011`).
    pub(in crate::app) fn refuse_if_readonly(&mut self, cmd: &str) -> bool {
        if !self.readonly_refuses_command(cmd) {
            return false;
        }
        let (code, _level) = self.collab.refusal();
        self.command_line.push_error_coded(
            &code,
            &format!("协同：当前会话只读，拒绝执行 {cmd}"),
        );
        let object_ref = self.attempted_object(cmd);
        self.collab.report_attempt(&object_ref, cmd.trim());
        true
    }

    /// The tag of the object a refused command was aimed at, for the record:
    /// the tag `PIDTAG` names, else that of the first selected object that
    /// has one. Empty when the command names nothing and nothing is selected.
    fn attempted_object(&mut self, cmd: &str) -> String {
        let words: Vec<&str> = cmd.split_whitespace().collect();
        if words.first().is_some_and(|verb| verb.eq_ignore_ascii_case("PIDTAG")) {
            if let Some(tag) = words.get(1) {
                return self.canonical_tag(tag);
            }
        }
        let Some(i) = self.collab_tab() else {
            return String::new();
        };
        let tab = &self.tabs[i];
        tab.scene
            .selected_handles_in_order()
            .into_iter()
            .filter_map(|handle| crate::pid::persist::PidAuthor::load(&tab.scene.document, handle))
            .map(|author| author.tag)
            .find(|tag| !tag.is_empty())
            .unwrap_or_default()
    }

    /// Whether a step that has just been made on tab `i` is one this session
    /// may not make: it is read-only, the tab holds its drawing, and the step
    /// is not a peer's change being written.
    ///
    /// This is the lock. Every edit — a command, a row of the properties
    /// panel, a grip, a drop from the palette, a write over REST — ends as
    /// one step handed to `push_undo_entry`, and that is where this is
    /// asked. A step that may not be made is taken back there
    /// ([`Self::take_back_refused_step`]) instead of being kept.
    pub(in crate::app) fn refuses_step(&mut self, i: usize) -> bool {
        self.collab.readonly && !self.applying_remote && self.is_collab_tab(i)
    }

    /// The tier (`role` / `pbs` / `state`) behind `code`, when `code` is what
    /// the read-only gate has just answered a write with. `None` for any
    /// other failure code.
    pub(in crate::app) fn collab_refusal_level(&self, code: &str) -> Option<String> {
        if !self.collab.readonly {
            return None;
        }
        let (refusal, level) = self.collab.refusal();
        (refusal == code).then_some(level)
    }

    /// Whether an undo or a redo on tab `i` is refused because the session
    /// may not write — said and put on record when it is.
    pub(in crate::app) fn refuses_history_replay(&mut self, i: usize, what: &str) -> bool {
        if !self.refuses_step(i) {
            return false;
        }
        let (code, _level) = self.collab.refusal();
        self.command_line.push_error_coded(
            &code,
            &format!("协同：当前会话只读，拒绝执行 {what}"),
        );
        self.collab.report_attempt("", what);
        true
    }

    /// Say that a step was taken back, and put the attempt on record.
    /// `changes` is what the step would have told the room; `label` is the
    /// step's own name, for a step that says nothing in P&ID terms.
    pub(in crate::app) fn report_refused_step(
        &mut self,
        changes: &[broadcast::Change],
        label: &str,
    ) {
        let (code, _level) = self.collab.refusal();
        self.command_line.push_error_coded(
            &code,
            &format!("协同：当前会话只读，这一步修改（{label}）已撤回"),
        );
        let (object_ref, text) = match changes.first() {
            Some(first) => (
                first.object_ref.clone(),
                changes
                    .iter()
                    .map(|change| {
                        format!(
                            "{} {}.{} = {}",
                            change.kind, change.object_ref, change.field, change.value
                        )
                    })
                    .collect::<Vec<_>>()
                    .join("；"),
            ),
            None => (String::new(), label.to_owned()),
        };
        self.collab.report_attempt(&object_ref, &text);
    }
}

/// Commands a read-only session may still run: looking around, inspecting, and
/// the collaboration panel itself. Everything else is a write.
///
/// `PIDLEGEND` is here for its reading forms only (see
/// `readonly_refuses_command`). Not here, though they once were: `MEASURE`
/// (places points along an object), `FIND` (opens find-and-replace),
/// `PIDGROUP` (every one of its forms regroups or retags) and `AUDIT`
/// (applies its fixes when answered yes).
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
    "MEASUREGEOM",
    "PROPERTIES",
    "PROPS",
    "PR",
    "QSELECT",
    "QS",
    "SELECTALL",
    "HELP",
    "PIDLEGEND",
    "PIDLINE",
    "PIDTAG",
    "PIDTOPOLOGY",
    "PIDPALETTE",
    "PIDCOLLAB",
    "PIDCLAIM",
    "PIDRELEASE",
    "PIDPLOT",
    "PLOT",
    "PRINT",
    "PREVIEW",
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
        // The session is at revision 1; the room's next op is 2.
        app.collab.base_revision = 1;
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
        assert_eq!(app.collab.diverged, None, "the next revision is no gap");
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

    // ── Undo takes back this person's steps, not a peer's ────────────────

    /// A second vessel on the drawing `app_with_vessel` made.
    fn add_vessel(app: &mut OpenCADStudio, tag: &str, props: &str) -> document::Handle {
        let i = app.active_tab;
        let document = &mut app.tabs[i].scene.document;
        let handle = document
            .add_entity(document::EntityType::Insert(Insert::new(
                "PID_SYM_equipment_vessel",
                Vector3::new(40.0, 0.0, 0.0),
            )))
            .expect("entity");
        PidAuthor {
            type_code: "EQUIPMENT".to_owned(),
            tag: tag.to_owned(),
            tag_source: "auto".to_owned(),
            tag_status: "ok".to_owned(),
            props: props.to_owned(),
            ..PidAuthor::default()
        }
        .store(document, handle);
        handle
    }

    fn connected(app: &mut OpenCADStudio) {
        app.collab.connected = true;
        app.collab.session_id = Some("s-1".to_owned());
    }

    /// A local property edit, through the contract's own write channel
    /// (`xdata_set` carrying the whole `props`).
    fn put_props(app: &mut OpenCADStudio, handle: document::Handle, props: &str) {
        let state = app.control_state();
        let (response, task) = app.control_request(serde_json::json!({
            "protocol": 1,
            "op": "xdata_set",
            "request_id": format!("collab-props-{}", app.collab.sent.len()),
            "client_id": "test",
            "document_id": state["document_id"],
            "revision": state["revision"],
            "handles": [format!("{:X}", handle.value())],
            "app": "PID_AUTHOR",
            "data": [{"code": 1000, "value": format!("props={props}")}],
        }));
        app.drive_headless_task(task).unwrap();
        assert_eq!(response["ok"], true, "{response}");
        app.finish_all_pending_history();
    }

    fn remote(app: &mut OpenCADStudio, revision: i64, changes: Vec<serde_json::Value>) {
        app.collab.apply(CollabEvent::OpApplied {
            op_id: format!("op-peer-{revision}"),
            revision,
            changes,
        });
        app.apply_remote_changes();
    }

    fn author(app: &OpenCADStudio, handle: document::Handle) -> PidAuthor {
        PidAuthor::load(&app.tabs[app.active_tab].scene.document, handle).expect("author")
    }

    fn prop(app: &OpenCADStudio, handle: document::Handle, key: &str) -> String {
        crate::pid::propagate::split_props(&author(app, handle).props)
            .0
            .get(key)
            .cloned()
            .unwrap_or_default()
    }

    /// Every change the last op this session queued states, flattened.
    fn last_sent(app: &OpenCADStudio) -> Vec<broadcast::Change> {
        let payload = app.collab.sent.last().expect("an op was queued").clone();
        broadcast::changes_from_payload(&[payload]).changes
    }

    fn undo(app: &mut OpenCADStudio) {
        let _ = app.automation_op(r#"{"op":"undo"}"#);
    }

    fn redo(app: &mut OpenCADStudio) {
        let _ = app.automation_op(r#"{"op":"redo"}"#);
    }

    /// A peer's change is not a step on this person's undo stack. Undo takes
    /// back what they did themselves — it used to take back whatever arrived
    /// last, a colleague's edit, and send that out as this person's op.
    #[test]
    fn undo_takes_back_this_persons_step_and_leaves_a_peers_change_alone() {
        let (mut app, pump) = app_with_vessel("P-1001");
        let vessel = add_vessel(&mut app, "V-1001", "{}");
        connected(&mut app);
        let _ = app.run_command_line("PIDTAG P-1001 P-1005");
        app.finish_all_pending_history();
        assert_eq!(app.collab.sent.len(), 1);
        let steps = app.tabs[app.active_tab].history.undo_stack.len();

        remote(
            &mut app,
            1,
            vec![serde_json::json!({
                "kind": "set_property", "object_ref": "V-1001", "field": "设计压力", "value": "2.5MPa",
            })],
        );
        assert_eq!(prop(&app, vessel, "设计压力"), "2.5MPa");
        assert_eq!(
            app.tabs[app.active_tab].history.undo_stack.len(),
            steps,
            "a peer's change must not become a step of this person's"
        );

        undo(&mut app);
        assert_eq!(author(&app, pump).tag, "P-1001", "this person's retag is taken back");
        assert_eq!(prop(&app, vessel, "设计压力"), "2.5MPa", "the peer's change stays");
        assert_eq!(app.collab.sent.len(), 2, "the undo goes out");
        let said = last_sent(&app);
        assert_eq!(said.len(), 1, "{said:?}");
        assert_eq!(
            (said[0].kind.as_str(), said[0].object_ref.as_str(), said[0].value.as_str()),
            ("set_tag", "P-1005", "P-1001")
        );

        // Nothing of this person's is left to take back, and the peer's
        // change is not there to be taken.
        undo(&mut app);
        assert_eq!(prop(&app, vessel, "设计压力"), "2.5MPa");
        assert_eq!(app.collab.sent.len(), 2);
    }

    /// One object, two people, two fields. Taking back one's own field puts
    /// back the entity as it was before one's step — an image the peer's
    /// field is not in — so the peer's change is written again afterwards,
    /// and only one's own field is said to have moved. Redo likewise.
    #[test]
    fn undo_puts_back_ones_own_field_and_keeps_the_field_a_peer_changed() {
        let (mut app, _) = app_with_vessel("X-1");
        let pump = add_vessel(&mut app, "P-1001", r#"{"设计压力":"1.0MPa","设计温度":"80℃"}"#);
        connected(&mut app);
        put_props(&mut app, pump, r#"{"设计压力":"1.6MPa","设计温度":"80℃"}"#);
        remote(
            &mut app,
            1,
            vec![serde_json::json!({
                "kind": "set_property", "object_ref": "P-1001", "field": "设计温度", "value": "120℃",
            })],
        );

        undo(&mut app);
        assert_eq!(prop(&app, pump, "设计压力"), "1.0MPa", "one's own field goes back");
        assert_eq!(prop(&app, pump, "设计温度"), "120℃", "the peer's field stays");
        let said = last_sent(&app);
        assert_eq!(said.len(), 1, "{said:?}");
        assert_eq!(
            (said[0].field.as_str(), said[0].value.as_str()),
            ("设计压力", "1.0MPa")
        );

        redo(&mut app);
        assert_eq!(prop(&app, pump, "设计压力"), "1.6MPa");
        assert_eq!(prop(&app, pump, "设计温度"), "120℃", "and stays through the redo");
        let said = last_sent(&app);
        assert_eq!(said.len(), 1, "{said:?}");
        assert_eq!(
            (said[0].field.as_str(), said[0].value.as_str()),
            ("设计压力", "1.6MPa")
        );
    }

    /// The same field. The peer wrote it after this person did, so the
    /// peer's value is what the room has; taking one's own write back
    /// changes nothing here and says nothing there. Said, it would put the
    /// old value over the peer's for everyone.
    #[test]
    fn undoing_a_value_a_peer_has_since_overwritten_changes_nothing_and_says_nothing() {
        let (mut app, _) = app_with_vessel("X-1");
        let pump = add_vessel(&mut app, "P-1001", r#"{"设计压力":"1.0MPa"}"#);
        connected(&mut app);
        put_props(&mut app, pump, r#"{"设计压力":"1.6MPa"}"#);
        remote(
            &mut app,
            1,
            vec![serde_json::json!({
                "kind": "set_property", "object_ref": "P-1001", "field": "设计压力", "value": "2.5MPa",
            })],
        );
        let sent = app.collab.sent.len();

        undo(&mut app);
        assert_eq!(prop(&app, pump, "设计压力"), "2.5MPa");
        assert_eq!(app.collab.sent.len(), sent, "{:?}", app.collab.sent.last());
    }

    /// A peer's change used to clear this person's redo stack, being pushed
    /// as a step of theirs. It is not a step, and the redo is still there.
    #[test]
    fn a_peers_change_leaves_this_persons_redo_in_place() {
        let (mut app, pump) = app_with_vessel("P-1001");
        let vessel = add_vessel(&mut app, "V-1001", "{}");
        connected(&mut app);
        let _ = app.run_command_line("PIDTAG P-1001 P-1005");
        app.finish_all_pending_history();
        undo(&mut app);
        assert_eq!(author(&app, pump).tag, "P-1001");

        remote(
            &mut app,
            1,
            vec![serde_json::json!({
                "kind": "set_property", "object_ref": "V-1001", "field": "设计压力", "value": "2.5MPa",
            })],
        );
        assert_eq!(
            app.tabs[app.active_tab].history.redo_stack.len(),
            1,
            "the redo was thrown away"
        );
        redo(&mut app);
        assert_eq!(author(&app, pump).tag, "P-1005");
        assert_eq!(prop(&app, vessel, "设计压力"), "2.5MPa");
    }

    /// A peer renamed the object after this person edited it. The undo finds
    /// it all the same — by the entity, not the name it had — keeps the new
    /// name, and tells the room about the object under the name it has now.
    #[test]
    fn an_undo_after_a_peers_retag_names_the_object_by_the_tag_it_has_now() {
        let (mut app, _) = app_with_vessel("X-1");
        let pump = add_vessel(&mut app, "P-1001", r#"{"设计压力":"1.0MPa"}"#);
        connected(&mut app);
        put_props(&mut app, pump, r#"{"设计压力":"1.6MPa"}"#);
        remote(
            &mut app,
            1,
            vec![serde_json::json!({
                "kind": "set_tag", "object_ref": "P-1001", "field": "tag", "value": "P-1009",
            })],
        );

        undo(&mut app);
        assert_eq!(author(&app, pump).tag, "P-1009", "the peer's name for it stays");
        assert_eq!(prop(&app, pump, "设计压力"), "1.0MPa");
        let said = last_sent(&app);
        assert_eq!(said.len(), 1, "{said:?}");
        assert_eq!(
            (said[0].object_ref.as_str(), said[0].field.as_str(), said[0].value.as_str()),
            ("P-1009", "设计压力", "1.0MPa")
        );
    }

    /// A step of this person's still open when a peer's change arrives ends
    /// there and goes out as theirs. It used to be closed inside the fold,
    /// where nothing is sent: the edit stayed in this drawing only.
    #[test]
    fn a_step_still_open_when_a_peers_change_arrives_goes_out_as_this_persons() {
        let (mut app, pump) = app_with_vessel("P-1001");
        let vessel = add_vessel(&mut app, "V-1001", "{}");
        connected(&mut app);
        let i = app.active_tab;
        app.push_undo_snapshot(i, "TEST");
        {
            let document = &mut app.tabs[i].scene.document;
            let mut record = PidAuthor::load(document, pump).expect("author");
            record.tag = "P-1005".to_owned();
            record.store(document, pump);
        }
        assert!(app.collab.sent.is_empty(), "the step is still open");

        remote(
            &mut app,
            1,
            vec![serde_json::json!({
                "kind": "set_property", "object_ref": "V-1001", "field": "设计压力", "value": "2.5MPa",
            })],
        );
        assert_eq!(app.collab.sent.len(), 1, "this person's step went out");
        let said = last_sent(&app);
        assert_eq!(
            (said[0].kind.as_str(), said[0].object_ref.as_str(), said[0].value.as_str()),
            ("set_tag", "P-1001", "P-1005")
        );
        assert_eq!(prop(&app, vessel, "设计压力"), "2.5MPa");
        // Their step alone is on the stack; undo takes it back and leaves
        // the peer's change.
        undo(&mut app);
        assert_eq!(author(&app, pump).tag, "P-1001");
        assert_eq!(prop(&app, vessel, "设计压力"), "2.5MPa");
    }

    // ── A session that may not write ─────────────────────────────────────

    /// Put the session in the state a `readonly_downgrade` from the server
    /// leaves it in, refused at `level`.
    fn told_it_may_not_write(app: &mut OpenCADStudio, level: &str, code: &str) {
        app.collab.connected = true;
        app.collab.session_id = Some("s-2".to_owned());
        app.collab.apply(CollabEvent::ReadonlyDowngrade {
            reason: "测试".to_owned(),
            holder: None,
            level: Some(level.to_owned()),
            code: Some(code.to_owned()),
        });
        assert!(app.collab.readonly);
    }

    /// The commands that write and were once let through are refused; the
    /// forms that only read are not.
    #[test]
    fn commands_that_write_are_refused_and_the_forms_that_read_are_not() {
        let (mut app, _) = app_with_vessel("P-1001");
        app.collab.readonly = true;
        for cmd in [
            "PIDLEGEND ON",
            "PIDLEGEND OFF",
            "PIDLEGEND PURGE",
            // Bare, it prompts with ON as the default.
            "PIDLEGEND",
            "PIDGROUP",
            "PIDGROUP GROUP",
            "PIDGROUP TAG P-1",
            "PIDGROUP AUTO",
            "PIDGROUP OFF",
            "AUDIT",
            "AUDIT Yes",
            "MEASURE",
            "FIND",
        ] {
            assert!(app.readonly_refuses_command(cmd), "{cmd} must be refused");
        }
        for cmd in [
            "PIDLEGEND REPORT",
            "pidlegend list",
            "PIDLEGEND EXPORT out.json",
            "PIDLINE",
            "PIDTAG P-1001",
            "MEASUREGEOM",
            "ZOOM EXTENTS",
        ] {
            assert!(!app.readonly_refuses_command(cmd), "{cmd} only reads");
        }
    }

    /// What the list lets through, run as it would be typed, leaves the
    /// drawing as it was: no step on the undo stack, the same entities, the
    /// same record. (`QUIT`, `EXIT` and `SAVEAS` are not run here: the first
    /// two leave the program and the third asks for a file name.)
    #[test]
    fn every_command_a_readonly_session_may_run_leaves_the_drawing_as_it_was() {
        for verb in READONLY_ALLOWED {
            if matches!(*verb, "QUIT" | "EXIT" | "SAVEAS") {
                continue;
            }
            let (mut app, vessel) = app_with_vessel("P-1001");
            let i = app.active_tab;
            app.collab.document = Some(app.tabs[i].id);
            app.collab.readonly = true;
            let steps = app.tabs[i].history.undo_stack.len();
            let entities = app.tabs[i].scene.document.entities().count();
            let record = author(&app, vessel);

            let cmd = match *verb {
                "PIDLEGEND" => "PIDLEGEND REPORT".to_owned(),
                "PIDTAG" => "PIDTAG P-1001".to_owned(),
                other => other.to_owned(),
            };
            assert!(!app.readonly_refuses_command(&cmd), "{cmd} is on the list");
            let _ = app.run_command_line(&cmd);
            app.finish_all_pending_history();

            let i = app.active_tab;
            assert_eq!(
                app.tabs[i].history.undo_stack.len(),
                steps,
                "{cmd} made a step in a session that may not write"
            );
            assert_eq!(
                app.tabs[i].scene.document.entities().count(),
                entities,
                "{cmd} added or removed an entity"
            );
            assert_eq!(author(&app, vessel), record, "{cmd} rewrote the record");
        }
    }

    /// The lock itself. An edit that reaches the drawing by a door the
    /// command gate does not watch is taken back as its step closes: the
    /// drawing and both history stacks are as they were, nothing goes to the
    /// room, the caller is told it was refused and by which tier, and the
    /// attempt is on record with the object it was aimed at.
    #[test]
    fn a_write_that_gets_past_the_command_gate_is_taken_back() {
        let (mut app, _) = app_with_vessel("X-1");
        let pump = add_vessel(&mut app, "P-1001", r#"{"设计压力":"1.0MPa"}"#);
        connected(&mut app);
        // A step of one's own from before, and one taken back, so there is
        // an undo stack and a redo stack to be left alone.
        put_props(&mut app, pump, r#"{"设计压力":"1.6MPa"}"#);
        put_props(&mut app, pump, r#"{"设计压力":"2.0MPa"}"#);
        undo(&mut app);
        let i = app.active_tab;
        let undo_steps = app.tabs[i].history.undo_stack.len();
        let redo_steps = app.tabs[i].history.redo_stack.len();
        assert_eq!((undo_steps, redo_steps), (1, 1));
        let sent = app.collab.sent.len();

        told_it_may_not_write(&mut app, "role", "PERM_DENIED_ROLE");

        // Over REST: the contract's own write channel for properties.
        let state = app.control_state();
        let (response, task) = app.control_request(serde_json::json!({
            "protocol": 1,
            "op": "xdata_set",
            "request_id": "refused-1",
            "client_id": "test",
            "document_id": state["document_id"],
            "revision": state["revision"],
            "handles": [format!("{:X}", pump.value())],
            "app": "PID_AUTHOR",
            "data": [{"code": 1000, "value": r#"props={"设计压力":"9.9MPa"}"#}],
        }));
        app.drive_headless_task(task).unwrap();
        assert_eq!(response["ok"], false, "{response}");
        assert_eq!(response["code"], "PERM_DENIED_ROLE", "{response}");
        assert_eq!(response["level"], "role", "{response}");
        assert_eq!(prop(&app, pump, "设计压力"), "1.6MPa", "the write was taken back");

        // By a door of its own: a step opened, the record rewritten, closed.
        app.push_undo_snapshot(i, "PANEL");
        {
            let document = &mut app.tabs[i].scene.document;
            let mut record = PidAuthor::load(document, pump).expect("author");
            record.tag = "P-1009".to_owned();
            record.store(document, pump);
        }
        app.finish_all_pending_history();
        assert_eq!(author(&app, pump).tag, "P-1001", "the retag was taken back");

        assert_eq!(app.tabs[i].history.undo_stack.len(), undo_steps);
        assert_eq!(app.tabs[i].history.redo_stack.len(), redo_steps);
        assert_eq!(app.collab.sent.len(), sent, "nothing goes to the room");
        let attempts = &app.collab.attempts;
        assert_eq!(attempts.len(), 2, "{attempts:?}");
        assert_eq!(attempts[0].0, "P-1001");
        assert!(attempts[0].1.contains("设计压力"), "{attempts:?}");
        assert_eq!(attempts[1].0, "P-1001");
        assert!(attempts[1].1.contains("P-1009"), "{attempts:?}");

        // Nor does it take its own earlier steps back, or put them on again.
        undo(&mut app);
        assert_eq!(prop(&app, pump, "设计压力"), "1.6MPa");
        redo(&mut app);
        assert_eq!(prop(&app, pump, "设计压力"), "1.6MPa");
        assert_eq!(app.tabs[i].history.undo_stack.len(), undo_steps);
        assert_eq!(app.tabs[i].history.redo_stack.len(), redo_steps);
    }

    /// A command the gate turns away answers its caller with the code and
    /// the tier of the refusal, and reports the attempt with the object the
    /// command names — spelled as the drawing spells it.
    #[test]
    fn a_refused_command_says_which_tier_refused_it_and_is_reported() {
        let (mut app, pump) = app_with_vessel("P-1001");
        told_it_may_not_write(&mut app, "role", "PERM_DENIED_ROLE");
        let refused = app.automation_op(r#"{"op":"run","cmd":"PIDTAG p-1001 P-1009"}"#);
        assert_eq!(refused["ok"], false, "{refused}");
        assert_eq!(refused["code"], "PERM_DENIED_ROLE", "{refused}");
        assert_eq!(refused["level"], "role", "{refused}");
        assert_eq!(author(&app, pump).tag, "P-1001");
        assert_eq!(
            app.collab.attempts,
            [("P-1001".to_owned(), "PIDTAG p-1001 P-1009".to_owned())]
        );

        // At the state tier the code is the one the gate has always given.
        let (mut app, _) = app_with_vessel("P-1001");
        told_it_may_not_write(&mut app, "state", "PERM_DENIED_STATE");
        let refused = app.automation_op(r#"{"op":"run","cmd":"PIDTAG P-1001 P-1009"}"#);
        assert_eq!(refused["code"], "COLLAB_READONLY", "{refused}");
        assert_eq!(refused["level"], "state", "{refused}");
    }

    /// A session that may not write does not come to write by losing its
    /// socket. It used to: `readonly` was cleared on every disconnect.
    #[test]
    fn a_session_that_may_not_write_still_may_not_while_disconnected() {
        let (mut app, pump) = app_with_vessel("P-1001");
        let i = app.active_tab;
        app.collab.document = Some(app.tabs[i].id);
        told_it_may_not_write(&mut app, "role", "PERM_DENIED_ROLE");
        app.collab.apply(CollabEvent::Disconnected {
            reason: "read error: ConnectionReset".to_owned(),
            will_retry: true,
        });
        assert!(app.collab.readonly, "the downgrade was dropped with the socket");
        let refused = app.automation_op(r#"{"op":"run","cmd":"PIDTAG P-1001 P-1009"}"#);
        assert_eq!(refused["ok"], false, "{refused}");
        assert_eq!(author(&app, pump).tag, "P-1001");

        // A new session decides afresh: the server says so right behind the
        // welcome if it still may not, and says nothing if it may.
        app.collab.apply(CollabEvent::Connected {
            session_id: "s-3".to_owned(),
            base_revision: 0,
            peers: Vec::new(),
            claims: Vec::new(),
        });
        assert!(!app.collab.readonly);
    }

    fn select(app: &mut OpenCADStudio, handle: document::Handle) {
        let _ = app.automation_op(r#"{"op":"select","clear":true}"#);
        let state = app.control_state();
        let (response, task) = app.control_request(serde_json::json!({
            "protocol": 1,
            "op": "select",
            "request_id": format!("collab-select-{:X}", handle.value()),
            "client_id": "test",
            "document_id": state["document_id"],
            "revision": state["revision"],
            "handles": [format!("{:X}", handle.value())],
        }));
        app.drive_headless_task(task).unwrap();
        assert_eq!(response["ok"], true, "{response}");
    }

    /// The Properties panel's rows for the drawing in front, as they stand.
    fn panel_rows(app: &OpenCADStudio) -> Vec<crate::scene::model::object::Property> {
        app.tabs[app.active_tab]
            .properties
            .sections
            .iter()
            .flat_map(|section| section.props.iter().cloned())
            .collect()
    }

    fn panel_row(app: &OpenCADStudio, label: &str) -> crate::scene::model::object::PropValue {
        panel_rows(app)
            .into_iter()
            .find(|row| row.label == label)
            .unwrap_or_else(|| panic!("no row labelled {label}"))
            .value
    }

    /// What the Properties panel shows of an object in a session that may
    /// not write: its values, as values. An input that takes what is typed
    /// and then gives it back — the step is undone as it closes — is worse
    /// than no input. The panel changes over when the session is told, with
    /// nobody touching the selection; a peer's change still shows in it; and
    /// a drawing the session is not on keeps its inputs.
    #[test]
    fn a_session_that_may_not_write_shows_an_objects_properties_as_values() {
        use crate::scene::model::object::PropValue;
        let is_value = |value: &PropValue| {
            matches!(value, PropValue::ReadOnly(_) | PropValue::ReadOnlyWithTooltip { .. })
        };
        let (mut app, _) = app_with_vessel("X-1");
        let pump = add_vessel(&mut app, "P-1001", r#"{"设计压力":"1.0MPa"}"#);
        connected(&mut app);
        let session_tab = app.active_tab;
        app.collab.document = Some(app.tabs[session_tab].id);
        select(&mut app, pump);
        app.refresh_properties();
        assert_eq!(panel_row(&app, "设计压力"), PropValue::PlainText("1.0MPa".to_owned()));
        assert!(
            panel_rows(&app).iter().any(|row| !is_value(&row.value)),
            "a session that writes has inputs"
        );

        let _ = app.update(crate::app::Message::CollabEvent(CollabEvent::ReadonlyDowngrade {
            reason: "角色无编辑权限".to_owned(),
            holder: None,
            level: Some("role".to_owned()),
            code: Some("PERM_DENIED_ROLE".to_owned()),
        }));
        let rows = panel_rows(&app);
        assert!(!rows.is_empty());
        for row in &rows {
            assert!(is_value(&row.value), "{} is still an input: {:?}", row.label, row.value);
        }
        assert_eq!(panel_row(&app, "设计压力"), PropValue::ReadOnly("1.0MPa".to_owned()));

        // A peer's change shows, and is still not an input.
        let _ = app.update(crate::app::Message::CollabEvent(CollabEvent::OpApplied {
            op_id: "op-peer-1".to_owned(),
            revision: 1,
            changes: vec![serde_json::json!({
                "kind": "set_property", "object_ref": "P-1001", "field": "设计压力", "value": "2.5MPa",
            })],
        }));
        assert_eq!(prop(&app, pump, "设计压力"), "2.5MPa");
        assert_eq!(panel_row(&app, "设计压力"), PropValue::ReadOnly("2.5MPa".to_owned()));

        // Another drawing is not the session's, and is edited as ever.
        let (_, other) = another_drawing_in_front(&mut app, "P-2001");
        select(&mut app, other);
        app.refresh_properties();
        assert!(
            panel_rows(&app).iter().any(|row| !is_value(&row.value)),
            "a drawing the session is not on lost its inputs"
        );

        // Told it may write again — a new session the server does not
        // downgrade — the inputs come back, again without being asked.
        app.active_tab = session_tab;
        app.refresh_properties();
        assert_eq!(panel_row(&app, "设计压力"), PropValue::ReadOnly("2.5MPa".to_owned()));
        let _ = app.update(crate::app::Message::CollabEvent(CollabEvent::Connected {
            session_id: "s-9".to_owned(),
            base_revision: 1,
            peers: Vec::new(),
            claims: Vec::new(),
        }));
        assert_eq!(panel_row(&app, "设计压力"), PropValue::PlainText("2.5MPa".to_owned()));
    }

    /// `VAL-CROSS-007`, this client's part. A write that tied with a peer's
    /// stays in this drawing while the entry waits — it is what this person
    /// wrote — and the panel lists the entry. The ruling arrives as an op:
    /// the field takes the ruled value here too, the Properties panel shows
    /// it, and the entry is off the panel.
    #[test]
    fn a_ruling_reaches_the_drawing_and_takes_the_entry_off_the_panel() {
        use crate::app::collab::PendingConflict;
        use crate::scene::model::object::PropValue;
        let (mut app, _) = app_with_vessel("X-1");
        let pump = add_vessel(&mut app, "P-1001", r#"{"设计压力":"1.0MPa"}"#);
        connected(&mut app);
        put_props(&mut app, pump, r#"{"设计压力":"1.6MPa"}"#);
        assert_eq!(app.collab.sent.len(), 1, "this person's write went out");

        let _ = app.update(crate::app::Message::CollabEvent(CollabEvent::Conflict(
            PendingConflict {
                conflict_id: "c-1".to_owned(),
                object_ref: "P-1001".to_owned(),
                field: "设计压力".to_owned(),
                value_a: "1.6MPa".to_owned(),
                value_b: "2.5MPa".to_owned(),
            },
        )));
        assert_eq!(app.pid_collab_view.conflicts.len(), 1, "the panel was not told");
        assert_eq!(app.pid_collab_view.conflicts[0].subject(), "P-1001 · 设计压力");
        assert_eq!(app.pid_collab_view.conflicts[0].values(), "1.6MPa ↔ 2.5MPa");
        assert_eq!(prop(&app, pump, "设计压力"), "1.6MPa");
        assert_eq!(app.collab.base_revision, 0, "a conflict is not a revision");

        let _ = app.update(crate::app::Message::CollabEvent(CollabEvent::OpApplied {
            op_id: "resolve-c-1".to_owned(),
            revision: 1,
            changes: vec![serde_json::json!({
                "kind": "set_property", "object_ref": "P-1001", "field": "设计压力",
                "value": "2.5MPa", "old_value": "1.0MPa",
            })],
        }));
        assert_eq!(prop(&app, pump, "设计压力"), "2.5MPa");
        assert!(app.pid_collab_view.conflicts.is_empty(), "the entry is still listed");
        assert_eq!(app.collab.last_apply_error, None);
        assert_eq!(app.collab.diverged, None);
        select(&mut app, pump);
        app.refresh_properties();
        assert_eq!(panel_row(&app, "设计压力"), PropValue::PlainText("2.5MPa".to_owned()));
    }

    /// A peer's change this drawing cannot follow — here, to an object it
    /// does not have — leaves it short of the room. The baseline moved all
    /// the same, so nothing else would stop the next local edit from going
    /// out on top of the difference: the session is marked and sends no more.
    #[test]
    fn a_change_this_drawing_cannot_follow_stops_the_sessions_writes() {
        let (mut app, vessel) = app_with_vessel("P-1001");
        app.collab.connected = true;
        app.collab.session_id = Some("s-1".to_owned());
        app.collab.apply(CollabEvent::OpApplied {
            op_id: "op-peer".to_owned(),
            revision: 1,
            changes: vec![serde_json::json!({
                "kind": "set_property", "object_ref": "P-9001", "field": "设计压力", "value": "2.5MPa",
            })],
        });
        app.apply_remote_changes();
        assert_eq!(app.collab.base_revision, 1, "the baseline is the server's count");
        let reason = app.collab.diverged.clone().expect("the session is marked");
        assert!(reason.contains("P-9001"), "{reason}");
        assert_eq!(app.collab.status_json()["diverged"], reason.as_str());

        // The local edit still happens — it is this person's drawing — but
        // it does not go out.
        let _ = app.run_command_line("PIDTAG P-1001 P-1005");
        app.finish_all_pending_history();
        assert_eq!(
            PidAuthor::load(&app.tabs[app.active_tab].scene.document, vessel)
                .expect("author")
                .tag,
            "P-1005"
        );
        assert!(
            app.collab.own_ops.is_empty(),
            "an op went out from a drawing known to differ: {:?}",
            app.collab.own_ops
        );
    }

    /// An op this client can read only in part is applied as far as it can
    /// be, and said to have left the drawing short: the change it could not
    /// read took effect in the room all the same. It used to be passed over
    /// in silence as long as anything else in the op could be applied.
    #[test]
    fn an_op_read_only_in_part_is_applied_as_far_as_it_goes_and_said_to_be_short() {
        let (mut app, vessel) = app_with_vessel("P-1001");
        app.collab.apply(CollabEvent::OpApplied {
            op_id: "op-peer".to_owned(),
            revision: 1,
            changes: vec![
                serde_json::json!({
                    "kind": "set_tag", "object_ref": "P-1001", "field": "tag", "value": "P-1009",
                }),
                serde_json::json!({"kind": "insert_symbol", "object_ref": "V-1001"}),
            ],
        });
        app.apply_remote_changes();
        assert_eq!(
            PidAuthor::load(&app.tabs[app.active_tab].scene.document, vessel)
                .expect("author")
                .tag,
            "P-1009",
            "what could be read is applied"
        );
        let said = app.collab.last_apply_error.clone().expect("it is reported");
        assert!(said.contains("insert_symbol"), "{said}");
        assert!(app.collab.diverged.is_some(), "and the session is marked");
    }

    /// An op none of whose changes this client can read is the same thing: the
    /// room changed in a way this drawing did not.
    #[test]
    fn an_op_this_client_cannot_read_marks_the_session_too() {
        let (mut app, _) = app_with_vessel("P-1001");
        app.collab.apply(CollabEvent::OpApplied {
            op_id: "op-peer".to_owned(),
            revision: 1,
            changes: vec![serde_json::json!({"kind": "something_new", "object_ref": "P-1001"})],
        });
        app.apply_remote_changes();
        assert!(app.collab.diverged.is_some());
        assert!(app.collab.last_apply_error.is_some());
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
