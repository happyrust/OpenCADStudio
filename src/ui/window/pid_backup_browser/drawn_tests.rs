//! The panel as it is drawn: where a click lands, what a line's box is, what
//! colour a row is. The pure tests of the panel send messages themselves,
//! which is how rows that drew no arrow and keys drawn over their values
//! went unnoticed; these press and measure what is on screen.

use std::sync::Arc;

use super::tests::ready_with_drawings_shown;
use super::*;
use crate::io::pid_backup_browse::{DrawingItem, DrawingItems, ItemAttribute, ItemGroup};

/// The panel's docked width, and a height that fits what the tests open.
const WIDTH: f32 = 340.0;
const HEIGHT: f32 = 700.0;

/// Three instruments as a drawing publishes them, in the order the index
/// gives them: by name.
fn three_instruments() -> DrawingItems {
    let item = |uid: &str, name: &str, variable: &str, sequence: &str| DrawingItem {
        uid: uid.to_string(),
        label: name.to_string(),
        item_type: "Instrument".to_string(),
        class: Some("PIDInstrument".to_string()),
        attributes: [
            ("INamedInstrument", "InstrTagSequenceNo", sequence),
            ("INamedInstrument", "MeasuredVariable", variable),
            ("IObject", "Name", name),
        ]
        .into_iter()
        .map(|(interface, name, value)| ItemAttribute {
            interface: interface.to_string(),
            name: name.to_string(),
            value: value.to_string(),
        })
        .collect(),
    };
    DrawingItems {
        drawing_uid: "UID-D".to_string(),
        drawing_name: "A-101".to_string(),
        groups: vec![ItemGroup {
            item_type: "Instrument".to_string(),
            items: vec![
                item("UID-C", "FT-12", "FT", "0012"),
                item("UID-A", "PT-7", "PT", "0007"),
                item("UID-B", "TT-101", "TT", "0101"),
            ],
        }],
        item_count: 3,
        ..DrawingItems::default()
    }
}

/// The tree open down to the three instruments of the first drawing.
fn instruments_shown() -> PidBackupBrowser {
    let mut state = ready_with_drawings_shown();
    update(&mut state, PidBackupMsg::Toggle(NodeId::Drawing(0)));
    update(
        &mut state,
        PidBackupMsg::ItemsLoaded(0, Ok(Arc::new(three_instruments()))),
    );
    update(&mut state, PidBackupMsg::Toggle(NodeId::ItemType(0, 0)));
    state
}

type Ui<'a> = iced_test::simulator::Simulator<'a, Message>;

fn draw(state: &PidBackupBrowser) -> Ui<'_> {
    iced_test::simulator::Simulator::with_size(
        iced::Settings::default(),
        (WIDTH, HEIGHT),
        view(state, WIDTH, Chrome::default()),
    )
}

/// Where the text `label` is drawn.
fn bounds_of(ui: &mut Ui<'_>, label: &str) -> iced::Rectangle {
    ui.find(label)
        .unwrap_or_else(|error| panic!("\"{label}\" is drawn: {error:?}"))
        .visible_bounds()
        .unwrap_or_else(|| panic!("\"{label}\" is on screen"))
}

/// The panel messages a click at `point` produces.
fn click_at(
    state: &PidBackupBrowser,
    point: impl Fn(&mut Ui<'_>) -> iced::Point,
) -> Vec<PidBackupMsg> {
    let mut ui = draw(state);
    let at = point(&mut ui);
    ui.point_at(at);
    let _ = ui.simulate(iced_test::simulator::click());
    ui.into_messages()
        .filter_map(|message| match message {
            Message::PidBackup(message) => Some(message),
            _ => None,
        })
        .collect()
}

/// The pixels of the panel as the headless renderer draws it (two per
/// logical pixel).
fn pixels(state: &PidBackupBrowser, name: &str) -> image::RgbaImage {
    let folder = std::env::temp_dir().join(format!(
        "ocs-pid-backup-drawn-{name}-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&folder);
    let theme = crate::ui::style::fusion_theme::fusion_black();
    let mut ui = draw(state);
    let snapshot = ui.snapshot(&theme).expect("the panel draws headless");
    snapshot
        .matches_image(folder.join("panel.png"))
        .expect("the picture is written");
    let written = std::fs::read_dir(&folder)
        .expect("the picture's folder")
        .flatten()
        .map(|entry| entry.path())
        .find(|path| path.extension().is_some_and(|ext| ext == "png"))
        .expect("one picture");
    let image = image::open(&written)
        .expect("the picture reads back")
        .to_rgba8();
    let _ = std::fs::remove_dir_all(&folder);
    image
}

fn pixel(image: &image::RgbaImage, x: f32, y: f32) -> [u8; 4] {
    image.get_pixel((x * 2.0) as u32, (y * 2.0) as u32).0
}

#[test]
fn an_item_row_is_the_items_name_and_its_type_lists_the_items_as_a_table() {
    let state = instruments_shown();
    let labels: Vec<String> = visible_rows(&state)
        .into_iter()
        .filter(|row| matches!(row.id, NodeId::Item(..)))
        .map(|row| row.label)
        .collect();
    assert_eq!(labels, ["FT-12", "PT-7", "TT-101"]);

    let rows = detail_rows(&state, &NodeId::ItemType(0, 0));
    assert_eq!(
        rows[2..],
        [
            (
                format!(
                    "{} | {} | {}",
                    t!("Item name"),
                    t!("Item class"),
                    t!("Item UID")
                ),
                crate::tf!("{} row(s)", 3).into_owned()
            ),
            ("FT-12".to_string(), "PIDInstrument | UID-C".to_string()),
            ("PT-7".to_string(), "PIDInstrument | UID-A".to_string()),
            ("TT-101".to_string(), "PIDInstrument | UID-B".to_string()),
        ]
    );
    // The item itself states its name first, then the UID the row left out.
    assert_eq!(
        detail_rows(&state, &NodeId::Item(0, 0, 1))[..2],
        [
            (t!("Item name").into_owned(), "PT-7".to_string()),
            (t!("Item UID").into_owned(), "UID-A".to_string()),
        ]
    );
}

/// The published attributes are `Interface.Name`: longer than any key
/// column. Drawn in a fixed 96 px box they ran on over the value beside
/// them. A key that long gets the line, the value goes under it at the value
/// column, and no text is wider than the box it is drawn in.
#[test]
fn a_key_longer_than_its_column_is_not_drawn_over_its_value() {
    let _guard = crate::headless_ui_lock();
    let mut state = instruments_shown();
    update(&mut state, PidBackupMsg::Select(NodeId::Item(0, 0, 0)));
    let mut ui = draw(&state);

    let long_key = "INamedInstrument.InstrTagSequenceNo";
    let key = bounds_of(&mut ui, long_key);
    let value = bounds_of(&mut ui, "0012");
    let natural = crate::ui::style::text_metrics::width(long_key, DETAIL_TEXT, false);
    assert!(
        natural <= key.width + 0.5,
        "the key is {natural} px of text in a box {} px wide",
        key.width
    );
    assert!(
        value.y >= key.y + key.height - 0.5,
        "the value starts below the key: key {key:?}, value {value:?}"
    );

    // A key that fits sits beside its value, clear of it.
    let short_key = bounds_of(&mut ui, t!("Item class").as_ref());
    let short_value = bounds_of(&mut ui, "PIDInstrument");
    assert!((short_key.y - short_value.y).abs() < 1.0);
    assert!(short_key.x + short_key.width <= short_value.x + 0.5);
    assert!(
        crate::ui::style::text_metrics::width(t!("Item class").as_ref(), DETAIL_TEXT, false)
            <= short_key.width + 0.5
    );
    // Stacked or not, the values start at the same column.
    assert!(
        (value.x - short_value.x).abs() < 0.5,
        "{} and {}",
        value.x,
        short_value.x
    );
}

#[test]
fn the_key_column_grows_with_the_panel_and_long_keys_stack() {
    assert_eq!(key_column_width(200.0), 96.0);
    assert!((key_column_width(340.0) - 122.4).abs() < 0.01);
    assert_eq!(key_column_width(2000.0), 240.0);
    assert!(!key_takes_its_own_line("SP_ID", 96.0));
    assert!(key_takes_its_own_line(
        "INamedInstrument.InstrTagSequenceNo",
        key_column_width(340.0)
    ));
}

/// A click on a row selects it, and the selected row is the one the panel
/// draws on a highlight: the pure state knew which row was selected all
/// along, the picture did not show it.
#[test]
fn a_click_on_a_drawn_row_selects_it_and_the_row_is_highlighted() {
    let _guard = crate::headless_ui_lock();
    let mut state = instruments_shown();
    // Right of the label, inside the row: no text there, only the row.
    let beside = |ui: &mut Ui<'_>, label: &str| -> iced::Point {
        let bounds = bounds_of(ui, label);
        iced::Point::new(bounds.x + bounds.width + 30.0, bounds.center_y())
    };
    let probe = |state: &PidBackupBrowser, name: &str| -> ([u8; 4], [u8; 4]) {
        let (first, second) = {
            let mut ui = draw(state);
            (beside(&mut ui, "PT-7"), beside(&mut ui, "TT-101"))
        };
        let image = pixels(state, name);
        (
            pixel(&image, first.x, first.y),
            pixel(&image, second.x, second.y),
        )
    };

    let (plain, other) = probe(&state, "unselected");
    assert_eq!(plain, other, "no row is selected, so the two look alike");

    let clicked = click_at(&state, |ui| beside(ui, "PT-7"));
    assert!(
        clicked
            .iter()
            .any(|message| matches!(message, PidBackupMsg::Select(NodeId::Item(0, 0, 1)))),
        "{clicked:?}"
    );
    for message in clicked {
        update(&mut state, message);
    }
    let (selected, other) = probe(&state, "selected");
    assert_eq!(other, plain, "the row that was not clicked is as it was");
    assert_ne!(selected, plain, "the clicked row is drawn on a highlight");
    let weak = crate::ui::style::fusion_theme::fusion_black()
        .palette()
        .primary
        .weak
        .color
        .into_rgba8();
    for channel in 0..3 {
        assert!(
            selected[channel].abs_diff(weak[channel]) <= 2,
            "the highlight is the palette's weak primary: {selected:?} vs {weak:?}"
        );
    }
}

/// A type with hundreds of items: the detail half builds the rows in view,
/// not the table. The first frame builds a bounded first part and learns
/// how tall the view is; from then on only what the view shows.
#[test]
fn a_long_table_in_the_details_builds_only_the_rows_in_view() {
    let _guard = crate::headless_ui_lock();
    let mut state = ready_with_drawings_shown();
    let mut items = three_instruments();
    let template = items.groups[0].items[0].clone();
    items.groups[0].items = (0..400)
        .map(|at| DrawingItem {
            uid: format!("UID-{at:04}"),
            label: format!("FT-{at:04}"),
            ..template.clone()
        })
        .collect();
    items.item_count = 400;
    update(&mut state, PidBackupMsg::Toggle(NodeId::Drawing(0)));
    update(
        &mut state,
        PidBackupMsg::ItemsLoaded(0, Ok(Arc::new(items))),
    );
    update(&mut state, PidBackupMsg::Select(NodeId::ItemType(0, 0)));

    let built = |state: &PidBackupBrowser| -> (Vec<bool>, Vec<PidBackupMsg>) {
        let mut ui = draw(state);
        let theme = crate::ui::style::fusion_theme::fusion_black();
        let _ = ui.snapshot(&theme).expect("the panel draws headless");
        let found = ["UID-0000", "UID-0030", "UID-0199", "UID-0399"]
            .iter()
            .map(|uid| ui.find(*uid).is_ok())
            .collect();
        let messages = ui
            .into_messages()
            .filter_map(|message| match message {
                Message::PidBackup(message) => Some(message),
                _ => None,
            })
            .collect();
        (found, messages)
    };

    let (found, messages) = built(&state);
    assert_eq!(found, [true, true, true, false], "the first frame");
    let reported: Vec<&PidBackupMsg> = messages
        .iter()
        .filter(|message| {
            matches!(
                message,
                PidBackupMsg::Grid(grid::GridMsg::Scrolled { height, .. }) if *height > 0.0
            )
        })
        .collect();
    assert_eq!(
        reported.len(),
        1,
        "the view reports its height: {messages:?}"
    );
    for message in messages {
        update(&mut state, message);
    }
    let (found, _) = built(&state);
    assert_eq!(found, [true, false, false, false], "once the view is known");
}

/// The table in the detail half, drawn: a click on a title sorts by that
/// column, the rows change places on screen, and another node's table
/// starts unsorted again.
#[test]
fn a_click_on_a_drawn_title_sorts_the_table_of_the_selected_node() {
    let _guard = crate::headless_ui_lock();
    let mut state = instruments_shown();
    update(&mut state, PidBackupMsg::Select(NodeId::ItemType(0, 0)));

    let order = |state: &PidBackupBrowser| -> Vec<&'static str> {
        let mut ui = draw(state);
        let mut uids: Vec<(f32, &str)> = ["UID-A", "UID-B", "UID-C"]
            .into_iter()
            .map(|uid| (bounds_of(&mut ui, uid).y, uid))
            .collect();
        uids.sort_by(|left, right| left.0.total_cmp(&right.0));
        uids.into_iter().map(|(_, uid)| uid).collect()
    };
    // As the index orders them: by name.
    assert_eq!(order(&state), ["UID-C", "UID-A", "UID-B"]);

    let title = t!("Item UID").into_owned();
    let clicked = click_at(&state, |ui| bounds_of(ui, &title).center());
    assert!(
        clicked
            .iter()
            .any(|message| matches!(message, PidBackupMsg::Grid(grid::GridMsg::Sort(2)))),
        "{clicked:?}"
    );
    for message in clicked {
        update(&mut state, message);
    }
    assert_eq!(state.grid.sort(), Some((2, false)));
    assert_eq!(order(&state), ["UID-A", "UID-B", "UID-C"]);

    // Another node, another table: unsorted, from its top.
    update(&mut state, PidBackupMsg::Select(NodeId::Drawing(0)));
    assert_eq!(state.grid.sort(), None);
    update(&mut state, PidBackupMsg::Select(NodeId::ItemType(0, 0)));
    assert_eq!(order(&state), ["UID-C", "UID-A", "UID-B"]);
}

/// The width of the bar that scrolls a half of the panel (the toolkit's
/// own default).
const SCROLL_BAR: f32 = 10.0;

/// The lines of the details wrap at the right edge of their half. When they
/// outgrow it a scroll bar appears at that edge, and it was drawn over the
/// last characters of every line: the end of a long path, of the SPID. The
/// lines now end where the bar begins.
#[test]
fn the_scroll_bar_of_the_details_is_not_drawn_over_the_lines() {
    let _guard = crate::headless_ui_lock();
    let mut state = ready_with_drawings_shown();
    // A sliver for the details: one line fits in it, the backup's own
    // lines do not.
    update(&mut state, PidBackupMsg::SetSplit(split::TREE_SHARE_RANGE.1));
    update(&mut state, PidBackupMsg::Select(NodeId::Files));
    let fits = bounds_of(&mut draw(&state), "2");
    update(&mut state, PidBackupMsg::Select(NodeId::Root));
    let scrolls = bounds_of(&mut draw(&state), "D:/backups/plant.zip");
    // A value's box starts at the value column and runs to the right edge
    // of the lines.
    assert!(
        (scrolls.x - fits.x).abs() < 0.5,
        "both values start at the value column: {fits:?} and {scrolls:?}"
    );
    assert!(
        scrolls.x + scrolls.width <= fits.x + fits.width - SCROLL_BAR + 0.5,
        "a line beside the scroll bar ends at {}; without a bar it ends at {}, and the bar is {SCROLL_BAR} px wide",
        scrolls.x + scrolls.width,
        fits.x + fits.width
    );
}

/// The same for the tree: a row is drawn up to the bar that scrolls the
/// rows, not under it.
#[test]
fn the_scroll_bar_of_the_tree_is_not_drawn_over_the_rows() {
    let _guard = crate::headless_ui_lock();
    let mut state = ready_with_drawings_shown();
    update(&mut state, PidBackupMsg::Select(NodeId::Root));
    let selected_row = |state: &PidBackupBrowser| -> iced::Rectangle {
        draw(state)
            .find(iced::widget::Id::new(split::SELECTED_ROW))
            .expect("the selected row is drawn")
            .visible_bounds()
            .expect("the selected row is on screen")
    };
    // Most of the panel for the tree: its rows fit.
    update(&mut state, PidBackupMsg::SetSplit(split::TREE_SHARE_RANGE.1));
    let fits = selected_row(&state);
    // A sliver of it: they scroll.
    update(&mut state, PidBackupMsg::SetSplit(split::TREE_SHARE_RANGE.0));
    let scrolls = selected_row(&state);
    assert!(
        scrolls.x + scrolls.width <= fits.x + fits.width - SCROLL_BAR + 0.5,
        "a row beside the scroll bar ends at {}; without a bar it ends at {}, and the bar is {SCROLL_BAR} px wide",
        scrolls.x + scrolls.width,
        fits.x + fits.width
    );
}
