//! How the panel shares its height between the tree and the details, and how
//! the tree keeps the selected row in view.
//!
//! The details hold tables and whole text files, the tree holds a plant: no
//! fixed share suits both. The two halves are the panes of a pane grid with
//! the bar between them to drag; the share lives in the panel's state, so it
//! stays as the user left it while backups are opened and closed.
//! `PIDBACKUPOPEN Split <percent>` sets it without a pointer.
//!
//! A row selected from the command line (`PIDBACKUPOPEN Select ...`) may be
//! far below what the tree shows. [`reveal_selected`] is a widget operation
//! that finds the selected row and the tree's scrollable in the laid-out
//! panel and scrolls just far enough for the row to show whole. It reads the
//! real layout, so it needs to know nothing about how tall a row is.

use iced::advanced::widget::operation::scrollable::{scroll_to, AbsoluteOffset};
use iced::advanced::widget::operation::{Outcome, Scrollable};
use iced::advanced::widget::{Id, Operation};
use iced::widget::pane_grid;
use iced::{Rectangle, Vector};

/// The share of the height the tree gets until the bar is moved.
pub const DEFAULT_TREE_SHARE: f32 = 0.6;
/// The least and the most the tree can be given: neither half disappears.
pub const TREE_SHARE_RANGE: (f32, f32) = (0.12, 0.88);
/// Height of the gap between the halves, which is the bar to drag.
pub const BAR: f32 = 7.0;
/// How far from the middle of the bar a press still takes hold of it.
pub const GRAB: f32 = 5.0;
/// The least height of a half, whatever the share says.
pub const MIN_HALF: f32 = 48.0;

/// The id of the scrollable that holds the tree.
pub const TREE_SCROLL: &str = "pid-backup-tree";
/// The id of the row that is selected.
pub const SELECTED_ROW: &str = "pid-backup-selected-row";

/// The two halves of the panel.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Half {
    /// The tree of the backup.
    Tree,
    /// The details of the selected node.
    Details,
}

/// The halves of the panel and the share of the height each has.
#[derive(Debug, Clone)]
pub struct Split {
    panes: pane_grid::State<Half>,
}

impl Default for Split {
    fn default() -> Self {
        Split {
            panes: pane_grid::State::with_configuration(pane_grid::Configuration::Split {
                axis: pane_grid::Axis::Horizontal,
                ratio: DEFAULT_TREE_SHARE,
                a: Box::new(pane_grid::Configuration::Pane(Half::Tree)),
                b: Box::new(pane_grid::Configuration::Pane(Half::Details)),
            }),
        }
    }
}

impl Split {
    /// The panes, for the grid that draws them.
    pub(super) fn panes(&self) -> &pane_grid::State<Half> {
        &self.panes
    }

    /// The share of the height the tree has, between 0 and 1.
    pub fn tree_share(&self) -> f32 {
        match self.panes.layout() {
            pane_grid::Node::Split { ratio, .. } => *ratio,
            pane_grid::Node::Pane(_) => DEFAULT_TREE_SHARE,
        }
    }

    /// Give the tree `share` of the height, within [`TREE_SHARE_RANGE`]. A
    /// share that is no number leaves the split as it is.
    pub(super) fn set_tree_share(&mut self, share: f32) {
        if !share.is_finite() {
            return;
        }
        if let pane_grid::Node::Split { id, .. } = self.panes.layout() {
            let id = *id;
            self.panes
                .resize(id, share.clamp(TREE_SHARE_RANGE.0, TREE_SHARE_RANGE.1));
        }
    }

    /// The bar was dragged.
    pub(super) fn dragged(&mut self, event: pane_grid::ResizeEvent) {
        self.set_tree_share(event.ratio);
    }
}

/// How far down the tree has to be scrolled for a row to show whole, when it
/// does not already: `None` leaves the tree where it is.
///
/// `row_top` and `row_bottom` are measured from the top of everything the
/// tree holds; the view shows what lies between `view_top` and `view_top +
/// view_height`. A row above the view comes to its top edge, one below to
/// its bottom edge; a row taller than the view shows its start.
pub fn reveal_offset(
    row_top: f32,
    row_bottom: f32,
    view_top: f32,
    view_height: f32,
) -> Option<f32> {
    if view_height <= 0.0 {
        return None;
    }
    let target = if row_top < view_top || row_bottom - row_top > view_height {
        row_top
    } else if row_bottom > view_top + view_height {
        row_bottom - view_height
    } else {
        return None;
    };
    let target = target.max(0.0);
    ((target - view_top).abs() > 0.5).then_some(target)
}

/// Where the tree's view and the selected row are, as laid out.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Placement {
    /// How far the tree is scrolled.
    pub view_top: f32,
    /// How tall the tree's view is.
    pub view_height: f32,
    /// The selected row, from the top of everything the tree holds.
    pub row_top: f32,
    /// Its lower edge, likewise.
    pub row_bottom: f32,
}

impl Placement {
    /// Whether the row shows whole.
    pub fn row_in_view(&self) -> bool {
        self.row_top >= self.view_top - 0.5
            && self.row_bottom <= self.view_top + self.view_height + 0.5
    }
}

/// The operation behind [`reveal_selected`]: one pass over the widgets to
/// find the view and the row, then the scroll they call for.
struct Reveal {
    tree: Id,
    row: Id,
    view: Option<(Rectangle, Rectangle, Vector)>,
    found: Option<Rectangle>,
}

impl Reveal {
    fn new() -> Self {
        Reveal {
            tree: Id::new(TREE_SCROLL),
            row: Id::new(SELECTED_ROW),
            view: None,
            found: None,
        }
    }

    fn placement(&self) -> Option<Placement> {
        let (bounds, content, translation) = self.view?;
        let row = self.found?;
        Some(Placement {
            view_top: translation.y,
            view_height: bounds.height,
            row_top: row.y - content.y,
            row_bottom: row.y - content.y + row.height,
        })
    }
}

impl<T: 'static> Operation<T> for Reveal {
    fn traverse(&mut self, operate: &mut dyn FnMut(&mut dyn Operation<T>)) {
        operate(self);
    }

    fn scrollable(
        &mut self,
        id: Option<&Id>,
        bounds: Rectangle,
        content_bounds: Rectangle,
        translation: Vector,
        _state: &mut dyn Scrollable,
    ) {
        if id == Some(&self.tree) {
            self.view = Some((bounds, content_bounds, translation));
        }
    }

    fn container(&mut self, id: Option<&Id>, bounds: Rectangle) {
        if id == Some(&self.row) {
            self.found = Some(bounds);
        }
    }

    fn finish(&self) -> Outcome<T> {
        let Some(placement) = self.placement() else {
            return Outcome::None;
        };
        match reveal_offset(
            placement.row_top,
            placement.row_bottom,
            placement.view_top,
            placement.view_height,
        ) {
            Some(y) => Outcome::Chain(Box::new(scroll_to(
                self.tree.clone(),
                AbsoluteOffset {
                    x: None,
                    y: Some(y),
                },
            ))),
            None => Outcome::None,
        }
    }
}

/// Scroll the tree so that the selected row shows whole. Nothing moves when
/// it already does, or when no row is selected.
pub fn reveal_selected<T: 'static>() -> impl Operation<T> + 'static {
    Reveal::new()
}

/// Where the view and the selected row are in a laid-out panel: what
/// [`reveal_selected`] goes by, for the tests to read.
#[cfg(test)]
pub(crate) fn placement_in(
    ui: &mut iced_runtime::UserInterface<'_, crate::app::Message, iced::Theme, iced::Renderer>,
    renderer: &iced::Renderer,
) -> Option<Placement> {
    let mut probe = Reveal::new();
    ui.operate(renderer, &mut probe as &mut dyn Operation<()>);
    probe.placement()
}

/// Where the tree's view is in the window of a laid-out panel.
#[cfg(test)]
pub(crate) fn tree_bounds_in(
    ui: &mut iced_runtime::UserInterface<'_, crate::app::Message, iced::Theme, iced::Renderer>,
    renderer: &iced::Renderer,
) -> Option<Rectangle> {
    let mut probe = Reveal::new();
    ui.operate(renderer, &mut probe as &mut dyn Operation<()>);
    probe.view.map(|(bounds, _, _)| bounds)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::Message;
    use crate::ui::dock::Chrome;
    use crate::ui::window::pid_backup_browser::{
        update, view, visible_rows, NodeId, PidBackupBrowser, PidBackupMsg,
    };

    #[test]
    fn the_share_starts_at_three_fifths_and_stays_within_bounds() {
        let mut split = Split::default();
        assert_eq!(split.tree_share(), DEFAULT_TREE_SHARE);
        split.set_tree_share(0.3);
        assert_eq!(split.tree_share(), 0.3);
        // Neither half can be dragged away, and a share that is no number
        // changes nothing.
        split.set_tree_share(0.0);
        assert_eq!(split.tree_share(), TREE_SHARE_RANGE.0);
        split.set_tree_share(7.0);
        assert_eq!(split.tree_share(), TREE_SHARE_RANGE.1);
        split.set_tree_share(f32::NAN);
        assert_eq!(split.tree_share(), TREE_SHARE_RANGE.1);
    }

    #[test]
    fn a_row_out_of_view_is_brought_to_the_nearer_edge() {
        // The view shows 100..300 of the content.
        // Whole in view: nothing moves, at either edge.
        assert_eq!(reveal_offset(100.0, 116.0, 100.0, 200.0), None);
        assert_eq!(reveal_offset(284.0, 300.0, 100.0, 200.0), None);
        // Above: its top comes to the top of the view.
        assert_eq!(reveal_offset(40.0, 56.0, 100.0, 200.0), Some(40.0));
        // Cut by the top edge: likewise.
        assert_eq!(reveal_offset(92.0, 108.0, 100.0, 200.0), Some(92.0));
        // Below, or cut by the bottom edge: its bottom to the bottom.
        assert_eq!(reveal_offset(900.0, 916.0, 100.0, 200.0), Some(716.0));
        assert_eq!(reveal_offset(290.0, 306.0, 100.0, 200.0), Some(106.0));
        // Taller than the view (a label wrapped over many lines): its start.
        assert_eq!(reveal_offset(500.0, 800.0, 100.0, 200.0), Some(500.0));
        // A view with no height has nothing to scroll.
        assert_eq!(reveal_offset(500.0, 516.0, 0.0, 0.0), None);
    }

    /// A backup with one archive of `members` files, the files group open
    /// and the archive unfolded: a tree far taller than the panel.
    fn tall(members: usize) -> PidBackupBrowser {
        use crate::io::pid_backup_browse::{BackupFile, BackupIndex, FileKind};
        let index = BackupIndex {
            files: vec![BackupFile {
                name: "RefData~4~681.zip".to_string(),
                kind: FileKind::RefData,
                format: "Zip".to_string(),
                members: (0..members)
                    .map(|at| format!("Symbols/Piping/Valve {at:03}.sym"))
                    .collect(),
                ..BackupFile::default()
            }],
            ..BackupIndex::default()
        };
        let mut state = PidBackupBrowser::default();
        update(
            &mut state,
            PidBackupMsg::Loaded(Ok(std::sync::Arc::new(index))),
        );
        update(&mut state, PidBackupMsg::Toggle(NodeId::Files));
        update(&mut state, PidBackupMsg::Toggle(NodeId::File(0)));
        state
    }

    fn renderer() -> iced::Renderer {
        use iced::advanced::renderer::Headless;
        let settings = iced::Settings::default();
        iced::futures::executor::block_on(<iced::Renderer as Headless>::new(
            iced::advanced::renderer::Settings {
                default_font: settings.default_font,
                default_text_size: settings.default_text_size,
                metrics_hinting: settings.metrics_hinting,
            },
            Some("tiny-skia"),
        ))
        .expect("a headless renderer")
    }

    /// Lay the panel out, run [`reveal_selected`] the way the runtime runs a
    /// widget operation (to its end, chained operations included), and say
    /// where the view and the selected row were before and after.
    fn reveal(state: &PidBackupBrowser) -> (Option<Placement>, Option<Placement>) {
        let mut renderer = renderer();
        let mut ui = iced_runtime::UserInterface::build(
            view(state, 340.0, Chrome::default()),
            iced::Size::new(340.0, 520.0),
            iced_runtime::user_interface::Cache::default(),
            &mut renderer,
        );
        let before = placement_in(&mut ui, &renderer);
        let mut operation: Box<dyn Operation<()>> = Box::new(reveal_selected::<()>());
        loop {
            ui.operate(&renderer, operation.as_mut());
            match operation.finish() {
                Outcome::Chain(next) => operation = next,
                _ => break,
            }
        }
        let after = placement_in(&mut ui, &renderer);
        (before, after)
    }

    #[test]
    fn the_selected_row_is_scrolled_into_the_trees_view() {
        let _guard = crate::headless_ui_lock();
        let mut state = tall(300);
        assert!(visible_rows(&state).len() > 300);

        // A row far below what the tree shows.
        update(&mut state, PidBackupMsg::Select(NodeId::FileMember(0, 250)));
        let (before, after) = reveal(&state);
        let (before, after) = (before.expect("laid out"), after.expect("laid out"));
        assert_eq!(before.view_top, 0.0);
        assert!(!before.row_in_view(), "{before:?}");
        assert!(after.row_in_view(), "{after:?}");
        // Just far enough: the row sits at the bottom edge of the view.
        assert!(
            (after.row_bottom - (after.view_top + after.view_height)).abs() < 1.0,
            "{after:?}"
        );

        // A row in view already: the tree stays where it is.
        update(&mut state, PidBackupMsg::Select(NodeId::FileMember(0, 3)));
        let (before, after) = reveal(&state);
        assert!(before.expect("laid out").row_in_view());
        assert_eq!(before, after);

        // No selection, and a selection the tree does not show (its level is
        // folded): nothing to find, nothing moves.
        update(&mut state, PidBackupMsg::Toggle(NodeId::File(0)));
        let (before, after) = reveal(&state);
        assert_eq!((before, after), (None, None));
    }

    /// The bar as it is drawn: a press on the gap under the tree takes hold
    /// of it, and moving the pointer up gives the details what the tree
    /// loses.
    #[test]
    fn the_drawn_bar_is_dragged_and_the_share_follows() {
        use iced::mouse;
        let _guard = crate::headless_ui_lock();
        const SIZE: (f32, f32) = (340.0, 520.0);
        let mut state = tall(300);
        update(&mut state, PidBackupMsg::Select(NodeId::FileMember(0, 3)));
        let tree = |state: &PidBackupBrowser| -> Rectangle {
            let mut renderer = renderer();
            let mut ui = iced_runtime::UserInterface::build(
                view(state, SIZE.0, Chrome::default()),
                iced::Size::new(SIZE.0, SIZE.1),
                iced_runtime::user_interface::Cache::default(),
                &mut renderer,
            );
            tree_bounds_in(&mut ui, &renderer).expect("the tree is laid out")
        };
        let before = tree(&state);
        // The middle of the gap right under the tree.
        let bar = iced::Point::new(SIZE.0 / 2.0, before.y + before.height + BAR / 2.0);
        let up = 120.0;

        let drag = |state: &PidBackupBrowser, from: iced::Point, to: iced::Point| {
            let mut ui = iced_test::simulator::Simulator::with_size(
                iced::Settings::default(),
                SIZE,
                view(state, SIZE.0, Chrome::default()),
            );
            ui.point_at(from);
            let _ = ui.simulate([iced::Event::Mouse(mouse::Event::ButtonPressed(
                mouse::Button::Left,
            ))]);
            ui.point_at(to);
            let _ = ui.simulate([
                iced::Event::Mouse(mouse::Event::CursorMoved { position: to }),
                iced::Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left)),
            ]);
            ui.into_messages()
                .filter_map(|message| match message {
                    Message::PidBackup(message) => Some(message),
                    _ => None,
                })
                .collect::<Vec<_>>()
        };

        let dragged = drag(&state, bar, iced::Point::new(bar.x, bar.y - up));
        assert!(
            dragged
                .iter()
                .any(|message| matches!(message, PidBackupMsg::SplitDragged(_))),
            "a press on the bar and a move drag it: {dragged:?}"
        );
        // Nothing else: the press did not land on a row under the bar.
        assert!(
            dragged
                .iter()
                .all(|message| matches!(message, PidBackupMsg::SplitDragged(_))),
            "{dragged:?}"
        );
        for message in dragged {
            update(&mut state, message);
        }
        let after = tree(&state);
        assert!(state.split().tree_share() < DEFAULT_TREE_SHARE);
        assert!(
            (before.height - after.height - up).abs() <= 2.0,
            "the tree was {} px tall and is {} px after the bar went up {up} px",
            before.height,
            after.height
        );

        // A press well inside the tree is a row's, not the bar's.
        let row = drag(
            &state,
            iced::Point::new(bar.x, after.y + 30.0),
            iced::Point::new(bar.x, after.y + 10.0),
        );
        assert!(
            !row.iter()
                .any(|message| matches!(message, PidBackupMsg::SplitDragged(_))),
            "{row:?}"
        );
    }

    /// The share is what the grid lays the halves out by: with more for the
    /// details the tree's view is shorter by as much.
    #[test]
    fn the_share_decides_how_tall_the_tree_is() {
        let _guard = crate::headless_ui_lock();
        let mut state = tall(300);
        update(&mut state, PidBackupMsg::Select(NodeId::FileMember(0, 3)));
        let at = |state: &PidBackupBrowser| reveal(state).0.expect("laid out").view_height;
        let default = at(&state);
        update(&mut state, PidBackupMsg::SetSplit(0.3));
        let small = at(&state);
        assert!(
            (small / default - 0.3 / DEFAULT_TREE_SHARE).abs() < 0.03,
            "the tree is {default} px tall at the default share and {small} px at 0.3"
        );
        // Closing the backup and opening another keeps the share.
        update(&mut state, PidBackupMsg::Close);
        assert_eq!(state.split().tree_share(), 0.3);
    }
}
