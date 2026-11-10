//! The collaboration panel's app-side glue: the `PIDCOLLAB` / `PIDCLAIM` /
//! `PIDRELEASE` commands, the panel's own messages, and the two synchronisations
//! the panel needs — the cached view it draws, and the scene colouring of tags
//! another session holds.
//!
//! The panel is a projection of [`crate::app::collab::State`]: it holds no
//! session state of its own, so a server push, a reconnect or a release needs no
//! extra bookkeeping beyond refreshing the cached view.

use crate::app::{Message, OpenCADStudio};
use crate::ui::dock::PanelId;
use crate::ui::window::pid_collab::{ClaimRow, CollabView, PeerRow, PidCollabMsg};
use iced::Task;

impl OpenCADStudio {
    /// Rebuild the cached panel view from the live session state.
    ///
    /// Called after every collaboration event and on every panel interaction,
    /// so the panel never draws a stale roster or a released claim.
    pub(crate) fn refresh_collab_view(&mut self) {
        let state = &self.collab;
        let peers: Vec<PeerRow> = state
            .peers
            .iter()
            .map(|peer| PeerRow {
                user_id: peer.user_id.clone(),
                display_name: peer.display_name.clone(),
                is_self: state
                    .session_id
                    .as_deref()
                    .is_some_and(|id| id == peer.session_id),
            })
            .collect();
        let claims: Vec<ClaimRow> = state
            .claims
            .iter()
            .map(|(tag, holder)| ClaimRow {
                tag: tag.clone(),
                holder: holder.clone(),
                is_self: *holder == state.user_id,
            })
            .collect();
        // A row that is no longer drawn must stop reporting its old rectangle,
        // or a bounds reader sees a claim the panel has already dropped. The
        // claim rows are rebuilt below on the next frame, so forgetting the
        // whole prefix here is exact rather than lossy.
        if self.pid_collab_view.claims != claims {
            crate::ui::wrap_bar::forget_bounds(
                crate::ui::window::pid_collab::ROW_BOUNDS_PREFIX,
            );
        }
        self.pid_collab_view = CollabView {
            connected: state.connected,
            server_url: state.server_url.clone(),
            session_id: state.session_id.clone(),
            readonly: state.readonly,
            peers,
            claims,
            last_message: state.last_claim_message.clone(),
            claims_open: self.pid_collab_claims_open,
        };
    }

    /// Colour the entities whose tag a session holds, so the drawing and the
    /// panel agree about what is occupied.
    ///
    /// Every held tag is coloured, including this session's own: the colour
    /// marks the entity as *occupied* — what any peer sees — and
    /// `VAL-CLI-008` reads the tint of the symbol `u1` itself claimed. The
    /// tag→handles resolution reads each entity's `PID_AUTHOR` record, the same
    /// source the tag engine and the properties panel use.
    pub(crate) fn sync_claim_colors(&mut self) {
        let tags: Vec<String> = self.collab.claims.keys().cloned().collect();
        if tags.is_empty() {
            for tab in &mut self.tabs {
                tab.scene.set_claim_display(None);
            }
            return;
        }
        let wanted: std::collections::HashSet<&str> = tags.iter().map(String::as_str).collect();
        for tab in &mut self.tabs {
            let document = &tab.scene.document;
            let handles: rustc_hash::FxHashSet<codec::Handle> = document
                .entities()
                .filter_map(|entity| {
                    let handle = entity.common().handle;
                    let author = crate::pid::persist::PidAuthor::load(document, handle)?;
                    wanted.contains(author.tag.as_str()).then_some(handle)
                })
                .collect();
            let display = (!handles.is_empty()).then_some(crate::scene::ClaimDisplay {
                handles,
                color: crate::scene::CLAIM_COLOR,
            });
            tab.scene.set_claim_display(display);
        }
    }

    /// Show the collaboration panel where the layout has it, docking it on the
    /// right the first time. `PIDCOLLAB` uses this to show; the panel's own
    /// close button hides it.
    pub(in crate::app) fn show_pid_collab_panel(&mut self) -> Task<Message> {
        if self.dock.location(PanelId::PidCollab).is_none() {
            self.dock
                .dock(PanelId::PidCollab, crate::app::config::DockSide::Right, 0);
        }
        self.show_pid_collab = true;
        self.refresh_collab_view();
        self.dock_reveal(PanelId::PidCollab);
        Task::none()
    }

    /// `PIDCOLLAB [ON|OFF]` — bare or `ON` shows the panel, `OFF` hides it, so
    /// the command has a deterministic form for scripted runs.
    pub(in crate::app) fn dispatch_pidcollab(&mut self, cmd: &str, _i: usize) -> Option<Task<Message>> {
        let verb_end = cmd
            .trim_start()
            .find(char::is_whitespace)
            .unwrap_or(cmd.trim_start().len());
        let verb = &cmd.trim_start()[..verb_end];
        if !verb.eq_ignore_ascii_case("PIDCOLLAB") {
            return None;
        }
        let arg = cmd.trim_start()[verb_end..].trim();
        match arg.to_ascii_uppercase().as_str() {
            "" | "ON" => Some(self.show_pid_collab_panel()),
            "OFF" => Some(self.on_dock(crate::ui::dock::DockMsg::Close(PanelId::PidCollab))),
            other => {
                self.command_line.push_error(&format!(
                    "PIDCOLLAB: 只接受 ON 或 OFF，实得 \"{other}\"。"
                ));
                Some(Task::none())
            }
        }
    }

    /// A panel interaction: claim, release, or expand the claims list.
    pub(in crate::app) fn on_pid_collab(&mut self, msg: PidCollabMsg) -> Task<Message> {
        match msg {
            PidCollabMsg::ToggleClaims => {
                self.pid_collab_claims_open = !self.pid_collab_claims_open;
                self.refresh_collab_view();
                Task::none()
            }
            PidCollabMsg::Claim(tag) => self.on_collab_claim(&tag, true),
            PidCollabMsg::Release(tag) => self.on_collab_claim(&tag, false),
        }
    }

    /// Queue one `claim` / `release` frame for `tag`, or say why it cannot go.
    ///
    /// The server's answer arrives later as a `CollabEvent`; this only puts the
    /// request on the transport's outbound queue, so the panel's click never
    /// blocks the UI thread on a socket write.
    pub(in crate::app) fn on_collab_claim(&mut self, tag: &str, claim: bool) -> Task<Message> {
        let tag = tag.trim();
        if tag.is_empty() {
            self.command_line
                .push_error("协同：位号为空，未发起占用请求。");
            return Task::none();
        }
        let outcome = if claim {
            self.collab.request_claim(tag)
        } else {
            self.collab.request_release(tag)
        };
        match outcome {
            Ok(()) => {
                let verb = if claim { "占用" } else { "释放" };
                self.command_line
                    .push_info(&format!("协同：已请求{verb}位号 {tag} …"));
            }
            Err(reason) => {
                self.command_line
                    .push_error(&format!("协同：位号 {tag} 请求未发送（{reason}）。"));
            }
        }
        Task::none()
    }

    /// `PIDCLAIM <tag>` / `PIDRELEASE <tag>`.
    ///
    /// Both are one-shot: they queue the frame and report, so there is nothing
    /// to install as an active command.
    pub(in crate::app) fn dispatch_pidclaim(&mut self, cmd: &str, _i: usize) -> Option<Task<Message>> {
        let (verb, rest) = split_verb(cmd)?;
        let claim = match verb {
            "PIDCLAIM" => true,
            "PIDRELEASE" => false,
            _ => return None,
        };
        let tag = rest.trim();
        if tag.is_empty() {
            let name = if claim { "PIDCLAIM" } else { "PIDRELEASE" };
            self.command_line
                .push_info(&format!("{name}  位号 <P-1001>:"));
            return Some(Task::none());
        }
        // A tag with a space would be two words; P&ID tags never are, so the
        // whole remainder is the tag and a second word is a typo worth naming.
        if tag.split_whitespace().count() > 1 {
            self.command_line.push_error(&format!(
                "{}: 只接受一个位号，实得 \"{tag}\"。",
                if claim { "PIDCLAIM" } else { "PIDRELEASE" }
            ));
            return Some(Task::none());
        }
        Some(self.on_collab_claim(tag, claim))
    }
}

/// Split a command line into its verb and the remainder, or `None` when the
/// verb does not match a claim command at all.
fn split_verb(cmd: &str) -> Option<(&str, &str)> {
    let trimmed = cmd.trim_start();
    let end = trimmed
        .find(char::is_whitespace)
        .unwrap_or(trimmed.len());
    let verb = &trimmed[..end];
    let verb = if verb.eq_ignore_ascii_case("PIDCLAIM") {
        "PIDCLAIM"
    } else if verb.eq_ignore_ascii_case("PIDRELEASE") {
        "PIDRELEASE"
    } else {
        return None;
    };
    Some((verb, &trimmed[end..]))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_claim_command_splits_into_verb_and_tag() {
        assert_eq!(
            split_verb("PIDCLAIM P-1001"),
            Some(("PIDCLAIM", " P-1001"))
        );
        assert_eq!(
            split_verb("pidrelease  P-1001"),
            Some(("PIDRELEASE", "  P-1001"))
        );
        assert_eq!(split_verb("PIDCLAIM"), Some(("PIDCLAIM", "")));
    }

    #[test]
    fn another_command_is_not_a_claim_command() {
        assert_eq!(split_verb("PIDTAG P-1001 P-1009"), None);
        assert_eq!(split_verb("PIDCLAIMX P-1001"), None);
        assert_eq!(split_verb(""), None);
    }

    /// The panel is a projection of the session state: a `welcome` that already
    /// lists a claim shows up in the panel with its holder, and the row offers
    /// "release" only for this session's own tags.
    #[test]
    fn the_panel_view_projects_the_session_state() {
        use crate::app::collab::{CollabEvent, PeerInfo};
        use crate::app::OpenCADStudio;

        let mut app = OpenCADStudio::new_for_test();
        app.automation_op(r#"{"op":"new"}"#);
        app.collab.server_url = "ws://127.0.0.1:3240/ws".to_owned();
        app.collab.user_id = "u1".to_owned();
        app.collab.apply(CollabEvent::Connected {
            session_id: "s-1".to_owned(),
            base_revision: 0,
            peers: vec![
                PeerInfo {
                    session_id: "s-1".to_owned(),
                    user_id: "u1".to_owned(),
                    display_name: "u1".to_owned(),
                },
                PeerInfo {
                    session_id: "s-2".to_owned(),
                    user_id: "u2".to_owned(),
                    display_name: "工艺乙".to_owned(),
                },
            ],
            claims: vec![("P-1001".to_owned(), "u2".to_owned())],
        });
        app.refresh_collab_view();

        let view = &app.pid_collab_view;
        assert!(view.connected);
        assert_eq!(view.peers.len(), 2);
        assert!(view.peers[0].is_self, "本机会话必须标记");
        assert!(!view.peers[1].is_self);
        let row = view
            .claims
            .iter()
            .find(|claim| claim.tag == "P-1001")
            .expect("占用行必须出现在面板里");
        assert_eq!(row.holder, "u2");
        assert!(!row.is_self, "别人的占用行不能提供「释放」");
        assert_eq!(view.remote_tags(), vec!["P-1001".to_string()]);
    }

    /// VAL-CLI-008: the claimed tag draws in the claim colour on the entities
    /// that carry it, and an unclaimed tag is left alone.
    #[test]
    fn a_claimed_tag_colours_the_entities_that_carry_it() {
        use crate::app::collab::CollabEvent;
        use crate::app::OpenCADStudio;
        use crate::pid::persist::PidAuthor;
        use crate::scene::CLAIM_COLOR;
        use codec::entities::Insert;
        use codec::types::Vector3;

        let mut app = OpenCADStudio::new_for_test();
        app.automation_op(r#"{"op":"new"}"#);
        app.collab.user_id = "u1".to_owned();

        let i = app.active_tab;
        let (claimed, other) = {
            let document = &mut app.tabs[i].scene.document;
            let claimed = document
                .add_entity(codec::EntityType::Insert(Insert::new(
                    "PID_SYM_equipment_vessel",
                    Vector3::ZERO,
                )))
                .unwrap();
            let other = document
                .add_entity(codec::EntityType::Insert(Insert::new(
                    "PID_SYM_equipment_vessel",
                    Vector3::new(40.0, 0.0, 0.0),
                )))
                .unwrap();
            PidAuthor {
                type_code: "EQUIPMENT".to_owned(),
                tag: "P-1001".to_owned(),
                tag_source: "auto".to_owned(),
                tag_status: "ok".to_owned(),
                ..PidAuthor::default()
            }
            .store(document, claimed);
            PidAuthor {
                type_code: "EQUIPMENT".to_owned(),
                tag: "P-1002".to_owned(),
                tag_source: "auto".to_owned(),
                tag_status: "ok".to_owned(),
                ..PidAuthor::default()
            }
            .store(document, other);
            (claimed, other)
        };

        // u2 holds P-1001: that symbol draws in the claim colour.
        app.collab.apply(CollabEvent::ClaimDenied {
            tag: "P-1001".to_owned(),
            holder: "u2".to_owned(),
            expires_at: 1_700_000_300,
        });
        app.sync_claim_colors();
        let display = app.tabs[i]
            .scene
            .claim_display
            .as_ref()
            .expect("占用必须着色");
        assert_eq!(display.color, CLAIM_COLOR);
        assert!(display.handles.contains(&claimed));
        assert!(!display.handles.contains(&other));

        // A release drops the colouring entirely.
        app.collab.apply(CollabEvent::ClaimReleased {
            tag: "P-1001".to_owned(),
            reason: "expired".to_owned(),
        });
        app.sync_claim_colors();
        assert!(app.tabs[i].scene.claim_display.is_none());
    }

    /// This session's own claim colours the drawing too: the tint marks the
    /// entity as occupied, which is what `VAL-CLI-008` reads off `u1`'s symbol.
    #[test]
    fn this_sessions_own_claim_is_coloured_as_occupied() {
        use crate::app::collab::CollabEvent;
        use crate::app::OpenCADStudio;
        use crate::pid::persist::PidAuthor;
        use codec::entities::Insert;
        use codec::types::Vector3;

        let mut app = OpenCADStudio::new_for_test();
        app.automation_op(r#"{"op":"new"}"#);
        app.collab.user_id = "u1".to_owned();

        let i = app.active_tab;
        let claimed = {
            let document = &mut app.tabs[i].scene.document;
            let claimed = document
                .add_entity(codec::EntityType::Insert(Insert::new(
                    "PID_SYM_equipment_vessel",
                    Vector3::ZERO,
                )))
                .unwrap();
            PidAuthor {
                type_code: "EQUIPMENT".to_owned(),
                tag: "P-1001".to_owned(),
                tag_source: "auto".to_owned(),
                tag_status: "ok".to_owned(),
                ..PidAuthor::default()
            }
            .store(document, claimed);
            claimed
        };

        app.collab.apply(CollabEvent::ClaimGranted {
            tag: "P-1001".to_owned(),
        });
        app.sync_claim_colors();
        let display = app.tabs[i]
            .scene
            .claim_display
            .as_ref()
            .expect("自己占用也要显示占用色");
        assert!(display.handles.contains(&claimed));
    }

    /// A claim row that stops being drawn must stop reporting its old
    /// rectangle: a bounds reader (the REST `ui_bounds` op) would otherwise
    /// find a row for a claim the panel already dropped, and `VAL-CLI-008`
    /// reads exactly that rectangle to locate the row it screenshots.
    #[test]
    fn a_released_claim_row_stops_reporting_its_rectangle() {
        use crate::app::collab::CollabEvent;
        use crate::app::OpenCADStudio;
        use crate::ui::window::pid_collab::ROW_BOUNDS_PREFIX;

        crate::ui::wrap_bar::forget_bounds(ROW_BOUNDS_PREFIX);
        let mut app = OpenCADStudio::new_for_test();
        app.automation_op(r#"{"op":"new"}"#);
        app.collab.server_url = "ws://127.0.0.1:3240/ws".to_owned();
        app.collab.user_id = "u1".to_owned();
        app.collab.apply(CollabEvent::ClaimGranted {
            tag: "P-1001".to_owned(),
        });
        app.refresh_collab_view();
        crate::ui::wrap_bar::set_drawn_bounds(
            &crate::ui::window::pid_collab::row_bounds_id("P-1001"),
            iced::Rectangle {
                x: 0.0,
                y: 0.0,
                width: 10.0,
                height: 10.0,
            },
        );
        assert!(crate::ui::wrap_bar::recorded_bounds()
            .iter()
            .any(|(id, _)| id == "pid-collab-row-P-1001"));

        app.collab.apply(CollabEvent::ClaimReleased {
            tag: "P-1001".to_owned(),
            reason: "released".to_owned(),
        });
        app.refresh_collab_view();
        assert!(
            !crate::ui::wrap_bar::recorded_bounds()
                .iter()
                .any(|(id, _)| id.starts_with(ROW_BOUNDS_PREFIX)),
            "释放后占用行不应再报告矩形"
        );
    }

    /// The first live session brings the panel out once, so a claim is visible
    /// without typing `PIDCOLLAB`; closing it by hand stays closed, because a
    /// reconnect must not reopen a panel the person dismissed.
    #[test]
    fn the_first_session_shows_the_panel_only_once() {
        use crate::app::collab::CollabEvent;
        use crate::app::OpenCADStudio;
        use crate::ui::dock::PanelId;

        let mut app = OpenCADStudio::new_for_test();
        app.automation_op(r#"{"op":"new"}"#);
        app.collab.server_url = "ws://127.0.0.1:3240/ws".to_owned();
        assert!(!app.show_pid_collab);

        let _ = app.update(crate::app::Message::CollabEvent(CollabEvent::Connected {
            session_id: "s-1".to_owned(),
            base_revision: 0,
            peers: Vec::new(),
            claims: Vec::new(),
        }));
        assert!(app.show_pid_collab, "首次连上应把协同面板显示出来");
        assert!(app.dock.location(PanelId::PidCollab).is_some());

        // The person closes it; a later reconnect leaves it closed.
        app.show_pid_collab = false;
        let _ = app.update(crate::app::Message::CollabEvent(CollabEvent::Connected {
            session_id: "s-2".to_owned(),
            base_revision: 0,
            peers: Vec::new(),
            claims: Vec::new(),
        }));
        assert!(!app.show_pid_collab, "重连不应重新打开已关闭的面板");
    }

    /// A tag with no entity in this drawing colours nothing: the panel still
    /// lists the claim, but there is no symbol to tint.
    #[test]
    fn a_claim_on_a_tag_this_drawing_lacks_colours_nothing() {
        use crate::app::collab::CollabEvent;
        use crate::app::OpenCADStudio;

        let mut app = OpenCADStudio::new_for_test();
        app.automation_op(r#"{"op":"new"}"#);
        app.collab.apply(CollabEvent::ClaimGranted {
            tag: "P-9999".to_owned(),
        });
        app.sync_claim_colors();
        assert!(app.tabs[app.active_tab].scene.claim_display.is_none());
    }
}
