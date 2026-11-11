//! Collaboration panel — docked (`PIDCOLLAB`).
//!
//! Two lists the user needs while a session is live:
//!
//! * **在线用户** — every peer on this drawing, this session marked, each row
//!   showing the user id and, when the server sent one, the display name. On
//!   a drawing this session was downgraded on, the row of whoever holds the
//!   edit right says so, and the banner above says this session is read-only.
//! * **位号占用** — one row per held tag: the tag, who holds it, and a button
//!   that claims it (free / held by somebody else) or releases it (held by
//!   this session). The button is the mouse front of `PIDCLAIM` / `PIDRELEASE`,
//!   so the panel and the command line share one implementation.
//!
//! The panel is the visible half of the claim state the scene also colours
//! (`Scene::set_claim_display`): a tag somebody else holds draws in
//! [`crate::scene::CLAIM_COLOR`], and the same tag appears here.
//!
//! Everything shown comes from [`crate::app::collab::State`]; the panel holds no
//! state of its own beyond which section is expanded, so a reconnect or a
//! server push is reflected on the next frame without any sync code.

use crate::app::Message;
use crate::ui::dock::{Chrome, PanelId};
use iced::widget::{button, column, container, row, scrollable, text, Space};
use iced::{Background, Border, Element, Fill, Length, Theme};

/// What the user did in the panel.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PidCollabMsg {
    /// Claim `tag` for this session.
    Claim(String),
    /// Release `tag` from this session.
    Release(String),
    /// Show or hide the claims list.
    ToggleClaims,
}

impl PidCollabMsg {
    pub(crate) fn message(self) -> Message {
        Message::PidCollab(self)
    }
}

/// `PosReport` id of the panel body, so a client can locate it on screen.
pub const PANEL_BOUNDS_ID: &str = "pid-collab-panel";

/// `PosReport` id prefix of the claim rows.
pub const ROW_BOUNDS_PREFIX: &str = "pid-collab-row-";

/// `PosReport` id of the row for `tag`.
pub fn row_bounds_id(tag: &str) -> String {
    format!("{ROW_BOUNDS_PREFIX}{tag}")
}

/// `PosReport` id of the read-only badge in the banner. Reported only while
/// the session is read-only, so its presence is the state.
pub const READONLY_BOUNDS_ID: &str = "pid-collab-readonly";

/// `PosReport` id prefix of the edit-right marker on a peer row.
pub const HOLDER_BOUNDS_PREFIX: &str = "pid-collab-holder-";

/// `PosReport` id of the edit-right marker on `user_id`'s row, so a client
/// can read off who the panel shows as the holder.
pub fn holder_bounds_id(user_id: &str) -> String {
    format!("{HOLDER_BOUNDS_PREFIX}{user_id}")
}

/// What the banner says of a read-only session.
pub const READONLY_LABEL: &str = "只读";

/// `PosReport` id of the badge a session wears once its drawing is known to
/// be short of the room's changes. Reported only then, so its presence is
/// the state.
pub const DIVERGED_BOUNDS_ID: &str = "pid-collab-diverged";

/// What the banner says of such a session.
pub const DIVERGED_LABEL: &str = "不一致";

/// What a peer row says of the session holding the edit right.
pub const HOLDER_LABEL: &str = "编辑权";

/// What the roster says under this session's own row when nobody else is
/// on the drawing.
pub const NO_OTHERS_LABEL: &str = "暂无其他在线用户";

/// What the panel says when no server address is set (`VAL-CLI-017`).
pub const NOT_CONFIGURED_LABEL: &str = "未配置协同服务器";

/// And how to set one, under it.
pub const NOT_CONFIGURED_HINT: &str = "在命令行输入 PIDCOLLAB SERVER ws://主机:端口/ws";

/// One peer row.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PeerRow {
    pub user_id: String,
    pub display_name: String,
    pub is_self: bool,
    /// This session holds the edit right of a drawing that went read-only for
    /// the others.
    pub is_holder: bool,
}

/// One claim row.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClaimRow {
    pub tag: String,
    pub holder: String,
    /// This session owns the tag, so the row offers "release".
    pub is_self: bool,
}

/// One conflict entry awaiting a ruling.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConflictRow {
    pub conflict_id: String,
    pub object_ref: String,
    pub field: String,
    pub value_a: String,
    pub value_b: String,
}

impl ConflictRow {
    /// What is contended: the object and the field.
    pub fn subject(&self) -> String {
        format!("{} · {}", self.object_ref, self.field)
    }

    /// The two values that were written, the first to arrive first.
    pub fn values(&self) -> String {
        format!("{} ↔ {}", self.value_a, self.value_b)
    }
}

/// `PosReport` id prefix of the conflict rows.
pub const CONFLICT_BOUNDS_PREFIX: &str = "pid-collab-conflict-";

/// `PosReport` id of the row for conflict entry `conflict_id`, so a client
/// can tell that the panel shows it (`VAL-CROSS-007`).
pub fn conflict_bounds_id(conflict_id: &str) -> String {
    format!("{CONFLICT_BOUNDS_PREFIX}{conflict_id}")
}

/// The header of the conflicts section. Drawn only while there is an entry.
pub const CONFLICTS_LABEL: &str = "待裁决冲突";

/// Everything the panel draws, taken from the collaboration state so the view
/// function stays free of app types (and therefore testable).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CollabView {
    pub connected: bool,
    pub server_url: String,
    pub session_id: Option<String>,
    pub readonly: bool,
    /// Why this drawing is no longer what the room has, once it is not: the
    /// session then sends nothing more, and the panel says so.
    pub diverged: Option<String>,
    pub peers: Vec<PeerRow>,
    /// The conflict entries nobody has ruled on yet, oldest first.
    pub conflicts: Vec<ConflictRow>,
    pub claims: Vec<ClaimRow>,
    pub last_message: Option<String>,
    /// The claims section is expanded.
    pub claims_open: bool,
}

impl CollabView {
    /// Whether the panel has anything to say at all (a configured server).
    pub fn configured(&self) -> bool {
        !self.server_url.trim().is_empty()
    }

    /// Claims held by somebody else, which is what the panel marks as
    /// other-owned (the drawing colours every held tag, its own included).
    pub fn remote_tags(&self) -> Vec<String> {
        self.claims
            .iter()
            .filter(|claim| !claim.is_self)
            .map(|claim| claim.tag.clone())
            .collect()
    }
}

fn muted(theme: &Theme) -> iced::widget::text::Style {
    iced::widget::text::Style {
        color: Some(theme.palette().background.base.text.scale_alpha(0.65)),
    }
}

fn section_header<'a>(label: &'a str, count: usize) -> Element<'a, Message> {
    container(
        row![
            text(label).size(12),
            Space::new().width(Fill),
            text(format!("{count}")).size(10).style(muted),
        ]
        .align_y(iced::Center),
    )
    .width(Fill)
    .into()
}

fn status_dot<'a>(connected: bool) -> Element<'a, Message> {
    let color = if connected {
        iced::Color::from_rgb8(51, 199, 89)
    } else {
        iced::Color::from_rgb8(140, 140, 140)
    };
    container(Space::new().width(Length::Fixed(8.0)).height(Length::Fixed(8.0)))
        .style(move |_: &Theme| container::Style {
            background: Some(Background::Color(color)),
            border: Border {
                radius: 4.0.into(),
                ..Default::default()
            },
            ..Default::default()
        })
        .into()
}

/// The colour of what the panel wants noticed: the read-only badge and the
/// edit-right marker. The amber the drawing tints an occupied tag with, so
/// "somebody else has this" reads the same in both places.
fn notice(_: &Theme) -> iced::widget::text::Style {
    let [r, g, b, a] = crate::scene::CLAIM_COLOR;
    iced::widget::text::Style {
        color: Some(iced::Color::from_rgba(r, g, b, a)),
    }
}

/// Everybody on the drawing: this session first, marked 本机, then the
/// others in the order the server reported them, with whoever holds the
/// edit right marked 编辑权. With nobody else there, it says so.
fn peer_rows<'a>(view: &'a CollabView) -> Element<'a, Message> {
    let mut rows = column![].spacing(2);
    for peer in &view.peers {
        let name = if peer.display_name.is_empty() || peer.display_name == peer.user_id {
            peer.user_id.clone()
        } else {
            format!("{}（{}）", peer.display_name, peer.user_id)
        };
        let mut line = row![text(name).size(11), Space::new().width(Fill)]
            .spacing(6)
            .align_y(iced::Center);
        if peer.is_holder {
            line = line.push(crate::ui::wrap_bar::PosReport::owned(
                holder_bounds_id(&peer.user_id),
                text(HOLDER_LABEL).size(10).style(notice),
            ));
        }
        if peer.is_self {
            line = line.push(text("本机").size(10).style(muted));
        }
        rows = rows.push(container(line).width(Fill).padding([2, 2]));
    }
    if view.peers.iter().all(|peer| peer.is_self) {
        rows = rows.push(text(NO_OTHERS_LABEL).size(11).style(muted));
    }
    rows.into()
}

/// One conflict entry: what is contended on the first line, in the colour
/// of things to notice, and the two values under it.
fn conflict_row<'a>(entry: &'a ConflictRow) -> Element<'a, Message> {
    let lines = column![
        text(entry.subject()).size(11).style(notice),
        text(entry.values()).size(10).style(muted),
    ]
    .spacing(1);
    let reported =
        crate::ui::wrap_bar::PosReport::owned(conflict_bounds_id(&entry.conflict_id), lines);
    container(reported).width(Fill).padding([2, 2]).into()
}

/// One claim row: the tag, its holder, and the claim/release button.
fn claim_row<'a>(claim: &'a ClaimRow) -> Element<'a, Message> {
    let action = if claim.is_self {
        button(text("释放").size(10))
            .on_press(PidCollabMsg::Release(claim.tag.clone()).message())
            .style(button::secondary)
            .padding([2, 6])
    } else {
        button(text("占用").size(10))
            .on_press(PidCollabMsg::Claim(claim.tag.clone()).message())
            .style(button::secondary)
            .padding([2, 6])
    };
    let holder = if claim.is_self {
        format!("{}（本机）", claim.holder)
    } else {
        claim.holder.clone()
    };
    let row = row![
        text(claim.tag.clone()).size(11),
        Space::new().width(Fill),
        text(holder).size(10).style(muted),
        action,
    ]
    .spacing(4)
    .align_y(iced::Center);
    let reported = crate::ui::wrap_bar::PosReport::owned(row_bounds_id(&claim.tag), row);
    container(reported).width(Fill).padding([2, 2]).into()
}

/// Build the docked panel.
pub fn view<'a>(view: &'a CollabView, width: f32, chrome: Chrome) -> Element<'a, Message> {
    let title_bar = crate::ui::dock::title_bar(PanelId::PidCollab, "协同".to_string(), chrome);

    if !view.configured() {
        // Says what to do about it as well: the address is given on the
        // command line (`PIDCOLLAB SERVER`), which is not somewhere a person
        // would look unprompted.
        let hint = container(
            column![
                text(NOT_CONFIGURED_LABEL).size(12).style(muted),
                text(NOT_CONFIGURED_HINT).size(10).style(muted),
            ]
            .spacing(4)
            .align_x(iced::Center),
        )
        .center_x(Fill)
        .center_y(Fill)
        .width(Fill)
        .height(Fill);
        return crate::ui::dock::frame(
            title_bar,
            column![hint].spacing(6).height(Fill),
            width,
        );
    }

    // ── Session banner ───────────────────────────────────────────────────
    let status = if view.connected {
        "已连接"
    } else {
        "未连接"
    };
    let mut banner_items = row![
        status_dot(view.connected),
        text(status).size(11),
        Space::new().width(Fill),
    ]
    .spacing(5)
    .align_y(iced::Center);
    if view.diverged.is_some() {
        banner_items = banner_items.push(crate::ui::wrap_bar::PosReport::new(
            DIVERGED_BOUNDS_ID,
            text(DIVERGED_LABEL).size(11).style(notice),
        ));
    }
    if view.readonly {
        banner_items = banner_items.push(crate::ui::wrap_bar::PosReport::new(
            READONLY_BOUNDS_ID,
            text(READONLY_LABEL).size(11).style(notice),
        ));
    }
    let banner = container(banner_items).width(Fill).padding([2, 2]);

    let mut body = column![banner].spacing(6);
    if let Some(reason) = &view.diverged {
        // Under the banner rather than in the last-outcome line, which the
        // next claim would overwrite: this stays true until the drawing is
        // replaced.
        body = body.push(
            container(
                text(format!(
                    "本图与房间已不一致，本地修改不再提交。{reason}"
                ))
                .size(10)
                .style(notice),
            )
            .width(Fill)
            .padding([0, 2]),
        );
    }

    // ── Peers ────────────────────────────────────────────────────────────
    body = body.push(section_header("在线用户", view.peers.len()));
    body = body.push(peer_rows(view));

    // ── Conflicts awaiting a ruling ──────────────────────────────────────
    // Above the claims, and not behind a toggle: an entry here means a
    // write — possibly this person's — has not taken effect, and stays so
    // until somebody rules on it.
    if !view.conflicts.is_empty() {
        body = body.push(section_header(CONFLICTS_LABEL, view.conflicts.len()));
        let mut rows = column![].spacing(2);
        for entry in &view.conflicts {
            rows = rows.push(conflict_row(entry));
        }
        body = body.push(rows);
    }

    // ── Claims ───────────────────────────────────────────────────────────
    let toggle_label = if view.claims_open {
        "位号占用 ▾"
    } else {
        "位号占用 ▸"
    };
    body = body.push(
        button(text(toggle_label).size(12))
            .on_press(PidCollabMsg::ToggleClaims.message())
            .style(button::text)
            .padding([2, 0])
            .width(Fill),
    );
    if view.claims_open {
        if view.claims.is_empty() {
            body = body.push(text("暂无占用").size(11).style(muted));
        } else {
            let mut claims = column![].spacing(1);
            for claim in &view.claims {
                claims = claims.push(claim_row(claim));
            }
            body = body.push(claims);
        }
    }

    // ── Last outcome ─────────────────────────────────────────────────────
    if let Some(message) = &view.last_message {
        body = body.push(
            container(text(message.clone()).size(10).style(muted))
                .width(Fill)
                .padding([2, 0]),
        );
    }

    let body = scrollable(body)
        .direction(scrollable::Direction::Vertical(
            scrollable::Scrollbar::default(),
        ))
        .width(Fill)
        .height(Fill);

    let reported = crate::ui::wrap_bar::PosReport::new(PANEL_BOUNDS_ID, body);
    crate::ui::dock::frame(title_bar, reported, width)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn view_with(claims: Vec<ClaimRow>) -> CollabView {
        CollabView {
            connected: true,
            server_url: "ws://127.0.0.1:3240/ws".to_owned(),
            session_id: Some("s-1".to_owned()),
            claims,
            claims_open: true,
            ..CollabView::default()
        }
    }

    /// A tag this session does not hold is what the drawing colours.
    #[test]
    fn remote_tags_are_the_ones_this_session_does_not_hold() {
        let view = view_with(vec![
            ClaimRow {
                tag: "P-1001".to_owned(),
                holder: "u2".to_owned(),
                is_self: false,
            },
            ClaimRow {
                tag: "P-1002".to_owned(),
                holder: "u1".to_owned(),
                is_self: true,
            },
        ]);
        assert_eq!(view.remote_tags(), vec!["P-1001".to_string()]);
    }

    #[test]
    fn a_panel_without_a_server_url_is_not_configured() {
        let mut view = view_with(Vec::new());
        assert!(view.configured());
        view.server_url.clear();
        assert!(!view.configured());
    }

    /// Row ids carry the tag, so a client can find the row it means.
    #[test]
    fn row_bounds_ids_name_their_tag() {
        assert_eq!(row_bounds_id("P-1001"), "pid-collab-row-P-1001");
        assert!(row_bounds_id("P-1001").starts_with(ROW_BOUNDS_PREFIX));
    }

    /// The marker id carries the user, so a client can read off who the panel
    /// shows as holding the edit right.
    #[test]
    fn holder_bounds_ids_name_their_user() {
        assert_eq!(holder_bounds_id("u1"), "pid-collab-holder-u1");
        assert!(holder_bounds_id("u1").starts_with(HOLDER_BOUNDS_PREFIX));
    }

    // ── The panel as it is drawn ─────────────────────────────────────────

    /// The panel's docked width, and a height that fits what the tests show.
    const WIDTH: f32 = 248.0;
    const HEIGHT: f32 = 420.0;

    type Ui<'a> = iced_test::simulator::Simulator<'a, Message>;

    fn draw(view_model: &CollabView) -> Ui<'_> {
        iced_test::simulator::Simulator::with_size(
            iced::Settings::default(),
            (WIDTH, HEIGHT),
            view(view_model, WIDTH, Chrome::default()),
        )
    }

    /// Where the text `label` is drawn, or `None` when it is not.
    fn drawn(ui: &mut Ui<'_>, label: &str) -> Option<iced::Rectangle> {
        ui.find(label).ok().and_then(|found| found.visible_bounds())
    }

    fn peer(user: &str, is_self: bool, is_holder: bool) -> PeerRow {
        PeerRow {
            user_id: user.to_owned(),
            display_name: user.to_owned(),
            is_self,
            is_holder,
        }
    }

    /// `u2`'s panel on a drawing `u1` holds the edit right of.
    fn downgraded() -> CollabView {
        CollabView {
            readonly: true,
            peers: vec![peer("u1", false, true), peer("u2", true, false)],
            ..view_with(Vec::new())
        }
    }

    /// VAL-CLI-012: a downgraded session's panel draws the read-only badge,
    /// and draws the edit-right marker on the holder's row and no other.
    #[test]
    fn a_downgraded_session_draws_the_badge_and_marks_the_holders_row() {
        let view_model = downgraded();
        let mut ui = draw(&view_model);

        let badge = drawn(&mut ui, READONLY_LABEL).expect("the read-only badge is drawn");
        let marker = drawn(&mut ui, HOLDER_LABEL).expect("the edit-right marker is drawn");
        let holder = drawn(&mut ui, "u1").expect("the holder's row is drawn");
        let own = drawn(&mut ui, "u2").expect("this session's row is drawn");

        let same_line = |a: iced::Rectangle, b: iced::Rectangle| {
            (a.center_y() - b.center_y()).abs() < a.height.max(b.height) / 2.0
        };
        assert!(
            same_line(marker, holder),
            "the marker sits on the holder's row: marker {marker:?}, u1 {holder:?}"
        );
        assert!(
            !same_line(marker, own),
            "and not on this session's: marker {marker:?}, u2 {own:?}"
        );
        assert!(
            badge.y < holder.y,
            "the badge is in the banner, above the roster: badge {badge:?}, u1 {holder:?}"
        );
    }

    /// A session that may write draws neither: the badge is the state, so it
    /// must not be there when the state is not.
    #[test]
    fn a_session_that_may_write_draws_neither() {
        let view_model = CollabView {
            peers: vec![peer("u1", true, false), peer("u2", false, false)],
            ..view_with(Vec::new())
        };
        let mut ui = draw(&view_model);
        assert!(drawn(&mut ui, "u1").is_some(), "the roster itself is drawn");
        assert_eq!(drawn(&mut ui, READONLY_LABEL), None);
        assert_eq!(drawn(&mut ui, HOLDER_LABEL), None);
        assert_eq!(drawn(&mut ui, DIVERGED_LABEL), None);
    }

    /// The first one on a drawing sees itself, marked 本机, and is told
    /// nobody else is there; with a second, the note goes and both rows are
    /// drawn (`VAL-CROSS-006`: 「0 名其他用户」→「1 名其他用户、共 2 人」).
    #[test]
    fn the_roster_shows_this_session_and_says_when_nobody_else_is_there() {
        let alone = CollabView {
            peers: vec![peer("u1", true, false)],
            ..view_with(Vec::new())
        };
        let mut ui = draw(&alone);
        assert!(drawn(&mut ui, "u1").is_some());
        assert!(drawn(&mut ui, "本机").is_some());
        assert!(drawn(&mut ui, NO_OTHERS_LABEL).is_some());

        let two = CollabView {
            peers: vec![peer("u1", true, false), peer("u2", false, false)],
            ..view_with(Vec::new())
        };
        let mut ui = draw(&two);
        let own = drawn(&mut ui, "u1").expect("this session's row");
        let other = drawn(&mut ui, "u2").expect("the other's row");
        assert!(own.y < other.y, "this session first: {own:?}, {other:?}");
        assert_eq!(drawn(&mut ui, NO_OTHERS_LABEL), None);
    }

    /// `VAL-CROSS-007`: an entry awaiting a ruling is listed under a header
    /// of its own, below the roster, saying what is contended and the two
    /// values; with no entry there is no such section.
    #[test]
    fn a_conflict_awaiting_a_ruling_is_listed_with_what_is_contended() {
        let entry = ConflictRow {
            conflict_id: "c-7f3a".to_owned(),
            object_ref: "P-1001".to_owned(),
            field: "设计压力".to_owned(),
            value_a: "1.6MPa".to_owned(),
            value_b: "2.5MPa".to_owned(),
        };
        assert_eq!(conflict_bounds_id("c-7f3a"), "pid-collab-conflict-c-7f3a");
        let view_model = CollabView {
            peers: vec![peer("u1", true, false)],
            conflicts: vec![entry.clone()],
            ..view_with(Vec::new())
        };
        let mut ui = draw(&view_model);
        let header = drawn(&mut ui, CONFLICTS_LABEL).expect("the section header is drawn");
        let subject = drawn(&mut ui, "P-1001 · 设计压力").expect("what is contended is drawn");
        let values = drawn(&mut ui, "1.6MPa ↔ 2.5MPa").expect("the two values are drawn");
        let roster = drawn(&mut ui, "u1").expect("the roster is drawn");
        assert!(roster.y < header.y, "below the roster: u1 {roster:?}, header {header:?}");
        assert!(header.y < subject.y && subject.y < values.y, "{header:?} {subject:?} {values:?}");

        let none = CollabView {
            peers: vec![peer("u1", true, false)],
            ..view_with(Vec::new())
        };
        let mut ui = draw(&none);
        assert_eq!(drawn(&mut ui, CONFLICTS_LABEL), None);
    }

    /// A session whose drawing is short of the room's changes says so in the
    /// banner, above the roster, for as long as it is: nothing it does goes
    /// out any more, and the person has to know why.
    #[test]
    fn a_session_that_parted_from_the_room_draws_the_badge() {
        let view_model = CollabView {
            diverged: Some("收到修订 5，本地基线是 3".to_owned()),
            peers: vec![peer("u1", true, false)],
            ..view_with(Vec::new())
        };
        let mut ui = draw(&view_model);
        let badge = drawn(&mut ui, DIVERGED_LABEL).expect("the badge is drawn");
        let roster = drawn(&mut ui, "u1").expect("the roster is drawn");
        assert!(badge.y < roster.y, "badge {badge:?}, u1 {roster:?}");
    }
}
