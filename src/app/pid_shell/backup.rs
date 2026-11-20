//! 备份数据包 — screen `backup`; in the design, frames `BkF01`..`BkF11`
//! (and their light counterparts `BkL01`..`BkL11`).
//!
//! Opens a local SmartPlant P&ID backup `.zip` or folder, browses its plant
//! groups, drawings, database columns and drawing items, opens readable
//! drawings in the editor, and drives paired publish / PDF batch exports.

use iced::widget::{container, text};
use iced::{font, Background, Element, Fill, Task};

use super::ViewFn;
use crate::app::{Message as AppMessage, OpenCADStudio};
use crate::ui::style::pid_fonts;
use crate::ui::style::pid_tokens::ColorToken;

/// What the screen remembers between frames.
#[derive(Debug, Default)]
pub struct State {}

/// What the screen's widgets send.
#[derive(Debug, Clone)]
pub enum Message {}

/// The function that draws the screen.
pub(crate) const VIEW: Option<ViewFn> = Some(view);

/// Take one of the screen's messages.
pub(crate) fn update(_app: &mut OpenCADStudio, message: Message) -> Task<AppMessage> {
    match message {}
}

/// Draw the backup package screen inside the shell's content area.
pub(crate) fn view(app: &OpenCADStudio) -> Element<'_, AppMessage> {
    let theme = app.pid_shell.theme;
    container(
        text("备份数据包")
            .size(16)
            .font(pid_fonts::ui(theme, font::Weight::Semibold))
            .color(ColorToken::TextPrimary.color(theme)),
    )
    .width(Fill)
    .height(Fill)
    .padding([20, 24])
    .style(move |_| container::Style {
        background: Some(Background::Color(ColorToken::BgApp.color(theme))),
        ..container::Style::default()
    })
    .into()
}
