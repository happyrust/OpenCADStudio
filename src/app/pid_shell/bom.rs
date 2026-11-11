//! 13 材料清单 — screen `bom`; in the design, frame `JIwZW` with
//! its content in `L7Z1N`. What the design draws there and where each
//! piece would come from is in
//! `docs/plans/2026-11-11-pid-ui-overhaul-map.md`, section 3.13.
//!
//! Not implemented: [`VIEW`] is `None`, so the screen is not in the
//! navigation, cannot be shown, and nothing is drawn for it. The task that
//! builds it writes `view` in this file, sets [`VIEW`] to `Some(view)` and
//! gives [`State`] and [`Message`] what the screen needs (see "What a screen
//! module is" in [`super`]).

use iced::Task;

use super::ViewFn;
use crate::app::{Message as AppMessage, OpenCADStudio};

/// What the screen remembers between frames.
#[derive(Debug, Default)]
pub struct State {}

/// What the screen's widgets send.
#[derive(Debug, Clone)]
pub enum Message {}

/// The function that draws the screen: none yet.
pub(crate) const VIEW: Option<ViewFn> = None;

/// Take one of the screen's messages.
pub(crate) fn update(_app: &mut OpenCADStudio, message: Message) -> Task<AppMessage> {
    match message {}
}
