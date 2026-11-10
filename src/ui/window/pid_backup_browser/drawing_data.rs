//! What a drawing's file holds beside its items, under the drawing.
//!
//! A drawing opened into its item types, and that was all the tree showed
//! of it. The read that fetches the items fetches more, and the drawing's
//! row now ends with it:
//!
//! ```text
//! D07
//!   Equipment (3)
//!   Instrument (12)
//!   ...
//!   Process points (38)
//!   Branch points (5)
//!   Representations (212)
//!   Ports, connections, OPC and pipelines
//!   Read statistics
//! ```
//!
//! The three lists are tables in the detail half: a process point with the
//! connector it is on, a branch point with the graphic it is drawn as, a
//! representation with the item it draws and the id of its graphic record.
//! The statistics are how much of the file the read understood.
//!
//! One row is there to say what is missing: the ports, the ends they
//! connect, the off-page connectors and the pipelines of a drawing are rows
//! of the backup's database, which the panel does not read.
//!
//! Everything comes with [`DrawingItems`], so the section reads nothing of
//! its own: its rows appear when the drawing's items do.

use std::borrow::Cow;
use std::sync::Arc;

use super::grid::{Column, Table};
use super::section::{Cx, Section};
use super::{DetailLine, ItemsState, NodeId, Row};
use crate::io::pid_backup_browse::drawing_data::{DrawingData, ValueSource};
use crate::io::pid_backup_browse::DrawingItems;
use crate::{t, tf};

/// The branch of a drawing that is not its items.
pub(super) struct DrawingDataSection;

/// The rows the section hangs under a drawing. A row's place in [`KINDS`]
/// is the second index of its node; the first is the drawing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Kind {
    ProcessPoints,
    BranchPoints,
    Representations,
    /// What the file does not hold: the database's side of the drawing.
    Database,
    Statistics,
}

/// The rows in display order.
const KINDS: [Kind; 5] = [
    Kind::ProcessPoints,
    Kind::BranchPoints,
    Kind::Representations,
    Kind::Database,
    Kind::Statistics,
];

impl Kind {
    fn title(self) -> Cow<'static, str> {
        match self {
            Kind::ProcessPoints => t!("Process points"),
            Kind::BranchPoints => t!("Branch points"),
            Kind::Representations => t!("Representations"),
            Kind::Database => t!("Ports, connections, OPC and pipelines"),
            Kind::Statistics => t!("Read statistics"),
        }
    }

    /// How many the drawing has of a kind that is a list.
    fn count(self, data: &DrawingData) -> Option<usize> {
        match self {
            Kind::ProcessPoints => Some(data.process_points.len()),
            Kind::BranchPoints => Some(data.branch_points.len()),
            Kind::Representations => Some(data.representations.len()),
            Kind::Database | Kind::Statistics => None,
        }
    }

    /// The row's text: a list with its length, as an item type has.
    fn label(self, data: &DrawingData) -> String {
        match self.count(data) {
            Some(count) => format!("{} ({count})", self.title()),
            None => self.title().into_owned(),
        }
    }

    /// Whether the row is drawn muted: a list with nothing in it, and the
    /// row that only says what the panel does not read.
    fn muted(self, data: &DrawingData) -> bool {
        self == Kind::Database || self.count(data) == Some(0)
    }
}

/// The drawing's read, once it is in and the file gave anything beside the
/// items.
fn read<'a>(cx: &Cx<'a>, drawing: usize) -> Option<&'a DrawingItems> {
    match cx.state.items.get(&drawing) {
        Some(ItemsState::Ready(items)) if !items.data.is_empty() => Some(items),
        _ => None,
    }
}

/// Where a value comes from, in words.
fn source_text(source: &ValueSource) -> String {
    match source {
        ValueSource::LabelRow(row) => tf!("label row {}", row).into_owned(),
        ValueSource::NameRule(rule) => tf!("naming rule {}", rule).into_owned(),
        ValueSource::NativeRule(address) => tf!("native rule at {}", address).into_owned(),
        ValueSource::DatabaseColumn { table, column } => format!("{table}.{column}"),
    }
}

fn field(label: Cow<'static, str>, value: impl ToString) -> DetailLine {
    DetailLine::Field(label.into_owned(), value.to_string())
}

fn marker(text: Cow<'static, str>) -> DetailLine {
    DetailLine::Marker(text.into_owned())
}

/// A list as a table, or the line that says the drawing has none.
fn list(columns: Vec<Column>, rows: Vec<Vec<String>>, lines: &mut Vec<DetailLine>) {
    if rows.is_empty() {
        lines.push(marker(t!("The drawing has none.")));
    } else {
        lines.push(DetailLine::Table(Arc::new(Table::new(columns, rows))));
    }
}

impl Section for DrawingDataSection {
    fn hosts(&self, cx: &Cx<'_>, host: &NodeId) -> bool {
        matches!(host, NodeId::Drawing(drawing) if read(cx, *drawing).is_some())
    }

    fn rows(&self, cx: &Cx<'_>, host: &NodeId, depth: usize, rows: &mut Vec<Row>) {
        let NodeId::Drawing(drawing) = host else {
            return;
        };
        let Some(items) = read(cx, *drawing) else {
            return;
        };
        for (at, kind) in KINDS.iter().enumerate() {
            rows.push(Row {
                id: cx.node([*drawing, at]),
                depth,
                label: kind.label(&items.data),
                expandable: false,
                expanded: false,
                disabled: kind.muted(&items.data),
            });
        }
    }

    fn details(&self, cx: &Cx<'_>, id: &NodeId, lines: &mut Vec<DetailLine>) {
        let Some([drawing, kind]) = cx.own(id) else {
            return;
        };
        let (Some(items), Some(kind)) = (read(cx, *drawing), KINDS.get(*kind)) else {
            return;
        };
        let data = &items.data;
        if let Some(entry) = cx.index.drawings.get(*drawing) {
            lines.push(field(t!("Drawing name"), &entry.name));
        }
        match kind {
            Kind::ProcessPoints => list(
                vec![
                    Column::text(t!("Process point UID")),
                    Column::text(t!("Connector name")),
                    Column::text(t!("Connector UID")),
                    Column::text(t!("Value source")),
                ],
                data.process_points
                    .iter()
                    .map(|point| {
                        vec![
                            point.uid.clone(),
                            point.connector.clone().unwrap_or_default(),
                            point.connector_uid.clone(),
                            source_text(&point.source),
                        ]
                    })
                    .collect(),
                lines,
            ),
            Kind::BranchPoints => list(
                vec![
                    Column::text(t!("Branch point UID")),
                    Column::text(t!("Branch point class")),
                    Column::text(t!("Representation UID")),
                    Column::text(t!("Value source")),
                ],
                data.branch_points
                    .iter()
                    .map(|point| {
                        vec![
                            point.uid.clone(),
                            point.class.clone(),
                            point.representation_uid.clone(),
                            source_text(&point.source),
                        ]
                    })
                    .collect(),
                lines,
            ),
            Kind::Representations => list(
                vec![
                    Column::text(t!("Representation UID")),
                    Column::text(t!("Item name")),
                    Column::text(t!("Item type")),
                    Column::text(t!("Item UID")),
                    Column::number(t!("Graphic OID")),
                ],
                data.representations
                    .iter()
                    .map(|representation| {
                        // A graphic with no item is a branch point drawn on
                        // a run; one whose item the drawing does not publish
                        // keeps the UID alone.
                        let name = match (&representation.item, &representation.item_uid) {
                            (Some(name), _) => name.clone(),
                            (None, None) => t!("branch point on a run").into_owned(),
                            (None, Some(_)) => String::new(),
                        };
                        vec![
                            representation.uid.clone(),
                            name,
                            representation.item_type.clone().unwrap_or_default(),
                            representation.item_uid.clone().unwrap_or_default(),
                            representation.graphic_oid.to_string(),
                        ]
                    })
                    .collect(),
                lines,
            ),
            Kind::Database => {
                lines.push(marker(t!(
                    "Ports, the runs they connect, off-page connectors and pipelines are in the backup's database, not in the drawing's file."
                )));
                lines.push(marker(t!(
                    "The panel does not read them from the database yet."
                )));
            }
            Kind::Statistics => {
                lines.push(field(t!("Drawing UID"), &items.drawing_uid));
                lines.push(field(t!("Item count"), items.item_count));
                for kind in [
                    Kind::ProcessPoints,
                    Kind::BranchPoints,
                    Kind::Representations,
                ] {
                    if let Some(count) = kind.count(data) {
                        lines.push(field(kind.title(), count));
                    }
                }
                lines.push(DetailLine::Heading(
                    t!("How much of the file was read").into_owned(),
                ));
                let stats = &data.stats;
                for (label, value) in [
                    (t!("Attribute records"), stats.attribute_records),
                    (t!("Records not decoded"), stats.undecoded_records),
                    (t!("Label values"), stats.label_values),
                    (
                        t!("Label values that reach an item"),
                        stats.label_values_reached,
                    ),
                    (
                        t!("Label values published as attributes"),
                        stats.label_values_published,
                    ),
                    (
                        t!("Attributes left out for conflicting labels"),
                        stats.conflicting_attributes,
                    ),
                    (
                        t!("Nominal diameter labels not published"),
                        stats.nominal_diameter_not_emitted,
                    ),
                    (
                        t!("Instruments classed by their symbol's folder"),
                        stats.instruments_classed_by_library,
                    ),
                ] {
                    lines.push(field(label, value));
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::super::tests::{items_of, ready_with_drawings_shown};
    use super::*;
    use crate::app::Message;
    use crate::io::pid_backup_browse::drawing_data::{
        BranchPoint, ProcessPoint, ReadStats, Representation,
    };
    use crate::ui::dock::Chrome;
    use crate::ui::window::pid_backup_browser::{
        detail_rows, update, view, visible_rows, PidBackupBrowser, PidBackupMsg,
    };

    /// One instrument, and what a file holds beside it.
    fn items() -> DrawingItems {
        let mut items = items_of("UID-1", "Instrument");
        items.data = DrawingData {
            process_points: vec![
                ProcessPoint {
                    uid: "RUN-2.PPT".to_string(),
                    connector_uid: "RUN-2".to_string(),
                    connector: Some("PipeRun #2".to_string()),
                    source: ValueSource::NativeRule("0x1003A2B0".to_string()),
                },
                ProcessPoint {
                    uid: "RUN-1.PPT".to_string(),
                    connector_uid: "RUN-1".to_string(),
                    connector: None,
                    source: ValueSource::LabelRow(41),
                },
            ],
            branch_points: Vec::new(),
            representations: vec![
                Representation {
                    uid: "REP-1".to_string(),
                    item_uid: Some("UID-1".to_string()),
                    item: Some("TT-101".to_string()),
                    item_type: Some("Instrument".to_string()),
                    graphic_oid: 4711,
                },
                Representation {
                    uid: "REP-2".to_string(),
                    item_uid: None,
                    item: None,
                    item_type: None,
                    graphic_oid: 52,
                },
                Representation {
                    uid: "REP-3".to_string(),
                    item_uid: Some("GONE".to_string()),
                    item: None,
                    item_type: None,
                    graphic_oid: 900,
                },
            ],
            stats: ReadStats {
                attribute_records: 120,
                undecoded_records: 2,
                label_values: 30,
                label_values_reached: 28,
                label_values_published: 25,
                conflicting_attributes: 1,
                nominal_diameter_not_emitted: 3,
                instruments_classed_by_library: 4,
            },
        };
        items
    }

    /// The first drawing open, its read in.
    fn read_drawing(items: DrawingItems) -> PidBackupBrowser {
        let mut state = ready_with_drawings_shown();
        update(&mut state, PidBackupMsg::Toggle(NodeId::Drawing(0)));
        update(
            &mut state,
            PidBackupMsg::ItemsLoaded(0, Ok(Arc::new(items))),
        );
        state
    }

    /// What the tree shows under the first drawing: label and whether muted.
    fn under_drawing(state: &PidBackupBrowser) -> Vec<(NodeId, String, bool)> {
        let rows = visible_rows(state);
        let at = rows
            .iter()
            .position(|row| row.id == NodeId::Drawing(0))
            .expect("the drawing has a row");
        rows[at + 1..]
            .iter()
            .take_while(|row| row.depth > rows[at].depth)
            .map(|row| (row.id.clone(), row.label.clone(), row.disabled))
            .collect()
    }

    /// The section's node of `kind` under the first drawing: read off the
    /// tree, as its slot is its place in the registry.
    fn node(state: &PidBackupBrowser, kind: Kind) -> NodeId {
        let at = KINDS.iter().position(|found| *found == kind).unwrap();
        under_drawing(state)
            .into_iter()
            .map(|(id, _, _)| id)
            .find(|id| matches!(id, NodeId::Section(_, path) if path[..] == [0, at]))
            .unwrap_or_else(|| panic!("{kind:?} has a row under the drawing"))
    }

    fn value<'a>(rows: &'a [(String, String)], key: &str) -> Option<&'a str> {
        rows.iter()
            .find(|(found, _)| found == key)
            .map(|(_, value)| value.as_str())
    }

    #[test]
    fn a_read_drawing_ends_with_what_its_file_holds_beside_the_items() {
        let state = read_drawing(items());
        let rows = under_drawing(&state);
        let shown: Vec<(String, bool)> = rows
            .iter()
            .map(|(_, label, muted)| (label.clone(), *muted))
            .collect();
        assert_eq!(
            shown,
            [
                // The drawing's own subtree first.
                ("Instrument (1)".to_string(), false),
                (format!("{} (2)", t!("Process points")), false),
                // A list with nothing in it is there, and muted.
                (format!("{} (0)", t!("Branch points")), true),
                (format!("{} (3)", t!("Representations")), false),
                // The row that says what the panel does not read.
                (
                    t!("Ports, connections, OPC and pipelines").into_owned(),
                    true
                ),
                (t!("Read statistics").into_owned(), false),
            ]
        );
        // None of them unfolds.
        let tree = visible_rows(&state);
        assert!(tree
            .iter()
            .filter(|row| matches!(row.id, NodeId::Section(..)))
            .all(|row| !row.expandable));
        // Folding the drawing takes them with it.
        let mut state = state;
        update(&mut state, PidBackupMsg::Toggle(NodeId::Drawing(0)));
        assert!(under_drawing(&state).is_empty());
    }

    #[test]
    fn the_lists_are_tables_with_the_items_named() {
        let state = read_drawing(items());
        let titles = |titles: &[Cow<'static, str>]| titles.join(" | ");

        let rows = detail_rows(&state, &node(&state, Kind::ProcessPoints));
        assert_eq!(
            rows,
            [
                (t!("Drawing name").into_owned(), "A-101".to_string()),
                (
                    titles(&[
                        t!("Process point UID"),
                        t!("Connector name"),
                        t!("Connector UID"),
                        t!("Value source"),
                    ]),
                    tf!("{} row(s)", 2).into_owned()
                ),
                (
                    "RUN-2.PPT".to_string(),
                    format!(
                        "PipeRun #2 | RUN-2 | {}",
                        tf!("native rule at {}", "0x1003A2B0")
                    )
                ),
                // A connector the drawing does not publish: its UID alone.
                (
                    "RUN-1.PPT".to_string(),
                    format!(" | RUN-1 | {}", tf!("label row {}", 41))
                ),
            ]
        );

        let rows = detail_rows(&state, &node(&state, Kind::Representations));
        assert_eq!(
            rows[1..],
            [
                (
                    titles(&[
                        t!("Representation UID"),
                        t!("Item name"),
                        t!("Item type"),
                        t!("Item UID"),
                        t!("Graphic OID"),
                    ]),
                    tf!("{} row(s)", 3).into_owned()
                ),
                (
                    "REP-1".to_string(),
                    "TT-101 | Instrument | UID-1 | 4711".to_string()
                ),
                (
                    "REP-2".to_string(),
                    format!("{} |  |  | 52", t!("branch point on a run"))
                ),
                ("REP-3".to_string(), " |  | GONE | 900".to_string()),
            ]
        );

        // A list the drawing has nothing of says so instead of an empty table.
        assert_eq!(
            detail_rows(&state, &node(&state, Kind::BranchPoints)),
            [
                (t!("Drawing name").into_owned(), "A-101".to_string()),
                (t!("The drawing has none.").into_owned(), String::new()),
            ]
        );
    }

    #[test]
    fn a_branch_point_shows_its_class_and_its_graphic() {
        let mut items = items();
        items.data.branch_points = vec![BranchPoint {
            uid: "REP-2.BPT".to_string(),
            representation_uid: "REP-2".to_string(),
            class: "PIDSignalBranchPoint".to_string(),
            source: ValueSource::DatabaseColumn {
                table: "T_SIGNALPOINT".to_string(),
                column: "SIGNALPOINTNUMBER".to_string(),
            },
        }];
        let state = read_drawing(items);
        let id = node(&state, Kind::BranchPoints);
        assert_eq!(
            under_drawing(&state)
                .into_iter()
                .find(|(found, _, _)| *found == id)
                .map(|(_, label, muted)| (label, muted)),
            Some((format!("{} (1)", t!("Branch points")), false))
        );
        assert_eq!(
            detail_rows(&state, &id)[2],
            (
                "REP-2.BPT".to_string(),
                "PIDSignalBranchPoint | REP-2 | T_SIGNALPOINT.SIGNALPOINTNUMBER".to_string()
            )
        );
    }

    #[test]
    fn the_statistics_say_how_much_of_the_file_was_read() {
        let state = read_drawing(items());
        let rows = detail_rows(&state, &node(&state, Kind::Statistics));
        let expected: Vec<(String, String)> = [
            (t!("Drawing name"), "A-101"),
            (t!("Drawing UID"), "UID-1"),
            (t!("Item count"), "1"),
            (t!("Process points"), "2"),
            (t!("Branch points"), "0"),
            (t!("Representations"), "3"),
            (t!("How much of the file was read"), ""),
            (t!("Attribute records"), "120"),
            (t!("Records not decoded"), "2"),
            (t!("Label values"), "30"),
            (t!("Label values that reach an item"), "28"),
            (t!("Label values published as attributes"), "25"),
            (t!("Attributes left out for conflicting labels"), "1"),
            (t!("Nominal diameter labels not published"), "3"),
            (t!("Instruments classed by their symbol's folder"), "4"),
        ]
        .into_iter()
        .map(|(key, value)| (key.into_owned(), value.to_string()))
        .collect();
        assert_eq!(rows, expected);
    }

    #[test]
    fn the_database_side_of_a_drawing_says_where_it_is() {
        let state = read_drawing(items());
        let rows = detail_rows(&state, &node(&state, Kind::Database));
        assert_eq!(rows.len(), 3);
        assert_eq!(value(&rows, &t!("Drawing name")), Some("A-101"));
        assert!(rows.contains(&(
            t!("The panel does not read them from the database yet.").into_owned(),
            String::new()
        )));
    }

    #[test]
    fn a_drawing_whose_file_gave_nothing_else_keeps_only_its_items() {
        // The items alone, as a drawing read before this section existed.
        let state = read_drawing(items_of("UID-1", "Instrument"));
        let labels: Vec<String> = under_drawing(&state)
            .into_iter()
            .map(|(_, label, _)| label)
            .collect();
        assert_eq!(labels, ["Instrument (1)"]);
        // Nothing at all: the drawing has no arrow, as before.
        let state = read_drawing(DrawingItems::default());
        let drawing = visible_rows(&state)
            .into_iter()
            .find(|row| row.id == NodeId::Drawing(0))
            .unwrap();
        assert!(!drawing.expandable);
        // A drawing that is not read, and one whose read failed, have none
        // of the section's rows and nothing to say about its nodes.
        let mut state = ready_with_drawings_shown();
        update(&mut state, PidBackupMsg::Toggle(NodeId::Drawing(0)));
        assert!(under_drawing(&state).is_empty());
        update(
            &mut state,
            PidBackupMsg::ItemsLoaded(0, Err("cannot read".to_string())),
        );
        assert!(under_drawing(&state).is_empty());
        let read = read_drawing(items());
        let statistics = node(&read, Kind::Statistics);
        assert!(detail_rows(&state, &statistics).is_empty());
    }

    #[test]
    fn a_file_with_no_items_still_shows_what_it_holds() {
        // Graphics and statistics, and not one published item: the drawing
        // gets its arrow from the section.
        let mut items = items();
        items.groups.clear();
        items.item_count = 0;
        let state = read_drawing(items);
        let drawing = visible_rows(&state)
            .into_iter()
            .find(|row| row.id == NodeId::Drawing(0))
            .unwrap();
        assert!(drawing.expandable && drawing.expanded);
        assert_eq!(under_drawing(&state).len(), KINDS.len());
    }

    /// The branch as the panel draws it: a click on a list's row selects it,
    /// its rows are a grid, and a click on a title sorts them.
    #[test]
    fn the_drawn_representations_row_is_clicked_and_its_table_sorts() {
        let _guard = crate::headless_ui_lock();
        let mut state = read_drawing(items());

        fn draw(state: &PidBackupBrowser) -> iced_test::simulator::Simulator<'_, Message> {
            iced_test::simulator::Simulator::with_size(
                iced::Settings::default(),
                (340.0, 700.0),
                view(state, 340.0, Chrome::default()),
            )
        }
        let click = |state: &PidBackupBrowser, at: iced::Point| -> Vec<PidBackupMsg> {
            let mut ui = draw(state);
            ui.point_at(at);
            let _ = ui.simulate(iced_test::simulator::click());
            ui.into_messages()
                .filter_map(|message| match message {
                    Message::PidBackup(message) => Some(message),
                    _ => None,
                })
                .collect()
        };
        let bounds = |state: &PidBackupBrowser, label: &str| -> iced::Rectangle {
            draw(state)
                .find(label)
                .unwrap_or_else(|error| panic!("{label} is drawn: {error:?}"))
                .visible_bounds()
                .unwrap_or_else(|| panic!("{label} is on screen"))
        };

        let label = format!("{} (3)", t!("Representations"));
        let clicked = click(&state, bounds(&state, &label).center());
        let id = node(&state, Kind::Representations);
        assert!(
            clicked
                .iter()
                .any(|message| matches!(message, PidBackupMsg::Select(found) if *found == id)),
            "{clicked:?}"
        );
        for message in clicked {
            update(&mut state, message);
        }

        // The rows are drawn in the order the file gives them.
        let top = |state: &PidBackupBrowser, label: &str| bounds(state, label).y;
        assert!(top(&state, "REP-1") < top(&state, "REP-2"));
        assert!(top(&state, "REP-2") < top(&state, "REP-3"));
        // The graphic ids sort as numbers: 52, 900, 4711. Their title is
        // the last one, off the right edge of a docked panel, so the table
        // is sorted by the title that is on screen: the first.
        let title = bounds(&state, t!("Representation UID").as_ref());
        let sorted = click(&state, title.center());
        assert!(
            sorted
                .iter()
                .any(|message| matches!(message, PidBackupMsg::Grid(_))),
            "{sorted:?}"
        );
        for message in sorted {
            update(&mut state, message);
        }
        let again = click(&state, title.center());
        for message in again {
            update(&mut state, message);
        }
        // Descending by UID.
        assert!(top(&state, "REP-3") < top(&state, "REP-2"));
        assert!(top(&state, "REP-2") < top(&state, "REP-1"));
    }
}
