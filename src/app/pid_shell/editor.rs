//! 03 P&ID 编辑器 — screen `editor`; in the design, frame `t4QrJ` with its
//! content in `Ga1Mo`. What the design draws there and where each piece
//! comes from is in `docs/plans/2026-11-11-pid-ui-overhaul-map.md`,
//! section 3.3.
//!
//! Implemented as what the program already is: the drawing interface — the
//! canvas with its tabs, ribbon, panels, command line and status bar — on
//! which every P&ID command works (`PIDINSERT`, `PIDPIPE`, `PIDFITTING`, …).
//! The toolbar, the small palette and the inspector the design draws are not
//! there yet. What is there that the design does not have: the general
//! interface's fixed Start tab, first in the tab bar, which shows the general
//! Start page when it is picked or when the last drawing is closed.
//!
//! [`view`] is the whole general interface, and that interface places its
//! dropdowns, menus and dialogs by window coordinates. So it is not put in
//! the shell's content area as it is: it is given a frame of its own there
//! ([`super::rebase`]), in which the area's top-left corner is its origin
//! and the area's size is its "window size". Until the editor's own layout
//! replaces it, the shell therefore shows two of some things: the general
//! interface's status bar above the shell's, and its tab bar and ribbon
//! under the shell's title bar.

use iced::{Element, Task};

use super::ViewFn;
use crate::app::{Message as AppMessage, OpenCADStudio};

/// What the screen remembers between frames. Nothing of its own yet: the
/// drawings, the tabs and the panels are the application's.
#[derive(Debug, Default)]
pub struct State {}

/// What the screen's widgets send. None of its own yet: the drawing
/// interface sends the application's messages.
#[derive(Debug, Clone)]
pub enum Message {}

/// The function that draws the screen.
pub(crate) const VIEW: Option<ViewFn> = Some(view);

/// Take one of the screen's messages.
pub(crate) fn update(_app: &mut OpenCADStudio, message: Message) -> Task<AppMessage> {
    match message {}
}

/// Draw the editor: today, the general drawing interface, in a frame of its
/// own inside the shell's content area ([`super::rebase`]).
pub(crate) fn view(app: &OpenCADStudio) -> Element<'_, AppMessage> {
    super::rebase::Rebase::new(app.view_classic()).into()
}
