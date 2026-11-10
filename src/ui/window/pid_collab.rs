//! Collaboration panel — docked (`PIDCOLLAB`).
//!
//! Two lists the user needs while a session is live:
//!
//! * **在线用户** — every peer on this drawing, this session marked, each row
//!   showing the user id and, when the server sent one, the display name.
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

/// One peer row.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PeerRow {
    pub user_id: String,
    pub display_name: String,
    pub is_self: bool,
}

/// One claim row.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClaimRow {
    pub tag: String,
    pub holder: String,
    /// This session owns the tag, so the row offers "release".
    pub is_self: bool,
}

/// Everything the panel draws, taken from the collaboration state so the view
/// function stays free of app types (and therefore testable).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CollabView {
    pub connected: bool,
    pub server_url: String,
    pub session_id: Option<String>,
    pub readonly: bool,
    pub peers: Vec<PeerRow>,
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

/// The peer list: this session first (marked 本机), then the others in the
/// order the server reported them.
fn peer_rows<'a>(view: &'a CollabView) -> Element<'a, Message> {
    let mut rows = column![].spacing(2);
    if view.peers.is_empty() {
        rows = rows.push(text("暂无其他在线用户").size(11).style(muted));
    }
    for peer in &view.peers {
        let name = if peer.display_name.is_empty() || peer.display_name == peer.user_id {
            peer.user_id.clone()
        } else {
            format!("{}（{}）", peer.display_name, peer.user_id)
        };
        let suffix = if peer.is_self { " 本机" } else { "" };
        rows = rows.push(
            container(
                row![
                    text(name).size(11),
                    Space::new().width(Fill),
                    text(suffix).size(10).style(muted),
                ]
                .align_y(iced::Center),
            )
            .width(Fill)
            .padding([2, 2]),
        );
    }
    rows.into()
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
        let hint = container(text("未配置协同服务器").size(12).style(muted))
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
    if view.readonly {
        banner_items = banner_items.push(text("只读").size(10).style(muted));
    }
    let banner = container(banner_items).width(Fill).padding([2, 2]);

    let mut body = column![banner].spacing(6);

    // ── Peers ────────────────────────────────────────────────────────────
    body = body.push(section_header("在线用户", view.peers.len()));
    body = body.push(peer_rows(view));

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
}
