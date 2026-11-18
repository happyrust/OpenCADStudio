//! What a drawing's file holds beside its items, and what the backup's
//! database knows of the drawing, under the drawing.
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
//!     Ports (96)
//!     Connections (61)
//!     Nozzles and notes (4)
//!     Off-page connectors (2)
//!     Pipelines (7)
//!   Read statistics
//! ```
//!
//! The three lists are tables in the detail half: a process point with the
//! connector it is on, a branch point with the graphic it is drawn as, a
//! representation with the item it draws and the id of its graphic record.
//! The statistics are how much of the file the read understood. All of
//! that comes with [`DrawingItems`]: those rows appear when the drawing's
//! items do, and nothing more is read for them.
//!
//! The ports of a drawing, the ends of the runs they connect, its nozzles
//! and notes, its off-page connectors and its pipelines are not in its
//! file: they are rows of the backup's database. Their row opens into them
//! once the dump is read, which opening or selecting the row starts
//! ([`crate::io::pid_backup_browse::exp_views`]). A backup whose dump the
//! panel does not read keeps the row muted, and says why.

use std::borrow::Cow;
use std::sync::Arc;

use super::grid::{Column, Table};
use super::section::{Cx, Event, EventKind, Section, SectionData, SectionLoad};
use super::{DetailLine, ItemsState, NodeId, Row};
use crate::io::pid_backup_browse::drawing_data::{DrawingData, ValueSource};
use crate::io::pid_backup_browse::exp_views::DrawingDump;
use crate::io::pid_backup_browse::{load_items, BackupIndex, DrawingItems};
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
}

/// What the database knows of a drawing, under [`Kind::Database`]. A view's
/// place in [`VIEWS`] is the third index of its node.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum View {
    Ports,
    Connections,
    Relations,
    Opc,
    Pipelines,
}

/// The views in display order.
const VIEWS: [View; 5] = [
    View::Ports,
    View::Connections,
    View::Relations,
    View::Opc,
    View::Pipelines,
];

impl View {
    fn title(self) -> Cow<'static, str> {
        match self {
            View::Ports => t!("Ports"),
            View::Connections => t!("Run connections"),
            View::Relations => t!("Nozzles and notes"),
            View::Opc => t!("Off-page connectors"),
            View::Pipelines => t!("Pipelines"),
        }
    }
}

/// One view as the detail half shows it.
struct ViewTable {
    /// How many rows the view has; the table is there when it has any.
    rows: usize,
    /// What stands above the rows.
    notes: Vec<String>,
    table: Arc<Table>,
}

/// What the database knows of one drawing: read off the UI thread, kept
/// for as long as the backup is open.
struct DrawingViews {
    /// One for each of [`VIEWS`]: the view, or why the reader has none.
    views: [Result<ViewTable, String>; 5],
}

/// The reader's own words for why it stopped, with what they are about.
fn unread(why: String) -> String {
    tf!("The reader could not follow the dump: {}", why).into_owned()
}

fn view_table(columns: Vec<Column>, rows: Vec<Vec<String>>, notes: Vec<String>) -> ViewTable {
    ViewTable {
        rows: rows.len(),
        notes,
        table: Arc::new(Table::new(columns, rows)),
    }
}

/// The views of a drawing as tables.
fn drawing_views(dump: DrawingDump) -> DrawingViews {
    let ports = dump.ports.map_err(unread).map(|ports| {
        view_table(
            vec![
                Column::text(t!("Port UID")),
                Column::text(t!("Item name")),
                Column::text(t!("Item UID")),
                Column::text(t!("Port class")),
                Column::number(t!("Port number")),
                Column::text(t!("Flow direction code")),
                Column::text(t!("Nominal diameter")),
            ],
            ports
                .into_iter()
                .map(|port| {
                    vec![
                        port.uid,
                        port.item.unwrap_or_default(),
                        port.item_uid,
                        port.class.to_string(),
                        port.number.to_string(),
                        port.flow,
                        port.nominal,
                    ]
                })
                .collect(),
            Vec::new(),
        )
    });
    let connections = dump.connections.map_err(unread).map(|connections| {
        view_table(
            vec![
                Column::text(t!("Connected UID")),
                Column::text(t!("Run name")),
                Column::text(t!("Run UID")),
                Column::text(t!("Run end")),
                Column::text(t!("Relation definition")),
            ],
            connections
                .into_iter()
                .map(|connection| {
                    let end = match (connection.on_start, connection.on_end) {
                        (true, true) => t!("at its start and its end").into_owned(),
                        (true, false) => t!("at its start").into_owned(),
                        (false, true) => t!("at its end").into_owned(),
                        (false, false) => String::new(),
                    };
                    vec![
                        connection.symbol_uid,
                        connection.run.unwrap_or_default(),
                        connection.run_uid,
                        end,
                        connection.definition.to_string(),
                    ]
                })
                .collect(),
            Vec::new(),
        )
    });
    let relations = dump.relations.map_err(unread).map(|relations| {
        view_table(
            vec![
                Column::text(t!("Relation definition")),
                Column::text(t!("Nozzle or note UID")),
                Column::text(t!("Item name")),
                Column::text(t!("Item UID")),
            ],
            relations
                .into_iter()
                .map(|relation| {
                    vec![
                        relation.definition.to_string(),
                        relation.from_uid,
                        relation.to.unwrap_or_default(),
                        relation.to_uid,
                    ]
                })
                .collect(),
            Vec::new(),
        )
    });
    let opc = dump.opc.map_err(unread).map(|points| {
        view_table(
            vec![
                Column::text(t!("OPC UID")),
                Column::text(t!("OPC tag")),
                Column::text(t!("Description")),
                Column::text(t!("To / from text")),
                Column::text(t!("Representation UID")),
            ],
            points
                .into_iter()
                .map(|point| {
                    vec![
                        point.uid,
                        point.name,
                        point.description,
                        point.to_from_text,
                        point.representation_uid,
                    ]
                })
                .collect(),
            Vec::new(),
        )
    });
    let pipelines = dump.pipelines.map_err(unread).map(|pipelines| {
        let notes = vec![tf!(
            "{} piping connector(s): {} with no tag sequence number, {} whose fluid code the list does not spell, {} not among the pipe runs",
            pipelines.connectors,
            pipelines.without_sequence,
            pipelines.fluid_code_missing,
            pipelines.not_in_piperun
        )
        .into_owned()];
        view_table(
            vec![
                Column::text(t!("Pipeline name")),
                Column::number(t!("Connector count")),
                Column::text(t!("Connectors")),
                Column::text(t!("Pipeline UID")),
            ],
            pipelines
                .rows
                .into_iter()
                .map(|pipeline| {
                    vec![
                        pipeline.name,
                        pipeline.connectors.len().to_string(),
                        pipeline.connectors.join(", "),
                        pipeline.uid,
                    ]
                })
                .collect(),
            notes,
        )
    });
    DrawingViews {
        views: [ports, connections, relations, opc, pipelines],
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

/// The views of the drawing whose database row is `database`, once read.
fn views<'a>(cx: &Cx<'a>, database: &NodeId) -> Option<&'a DrawingViews> {
    match cx.state.section_data.get(&(cx.slot, database.clone())) {
        Some(SectionLoad::Ready(data)) => data.get::<DrawingViews>(),
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
        // The reader's own word, untranslated: it states this source for a
        // drawing's constant attributes, which no list here shows, and for
        // no point.
        ValueSource::ClassConstant => "ClassConstant".to_string(),
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

fn nothing_to_read() -> String {
    t!("this node has nothing to read").into_owned()
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
            let id = cx.node([*drawing, at]);
            if *kind != Kind::Database {
                rows.push(Row {
                    id,
                    depth,
                    label: kind.label(&items.data),
                    expandable: false,
                    expanded: false,
                    // A list with nothing in it is there, and muted.
                    disabled: kind.count(&items.data) == Some(0),
                });
                continue;
            }
            // The database's side: it opens when the panel reads the dump,
            // and is muted when it does not.
            let readable = cx.index.dump_unreadable().is_none();
            let open = readable && cx.is_open(&id);
            let found = if open { views(cx, &id) } else { None };
            rows.push(Row {
                id,
                depth,
                label: kind.label(&items.data),
                expandable: readable,
                expanded: open,
                disabled: !readable,
            });
            let Some(found) = found else {
                continue;
            };
            for (view_at, (view, table)) in VIEWS.iter().zip(&found.views).enumerate() {
                rows.push(Row {
                    id: cx.node([*drawing, at, view_at]),
                    depth: depth + 1,
                    label: match table {
                        Ok(table) => format!("{} ({})", view.title(), table.rows),
                        Err(_) => view.title().into_owned(),
                    },
                    expandable: false,
                    expanded: false,
                    disabled: !matches!(table, Ok(table) if table.rows > 0),
                });
            }
        }
    }

    fn details(&self, cx: &Cx<'_>, id: &NodeId, lines: &mut Vec<DetailLine>) {
        let Some(path) = cx.own(id) else {
            return;
        };
        let (Some(drawing), Some(kind)) = (path.first(), path.get(1)) else {
            return;
        };
        let (Some(items), Some(kind)) = (read(cx, *drawing), KINDS.get(*kind)) else {
            return;
        };
        // One of the database's views of the drawing.
        if let Some(view) = path.get(2) {
            let database = cx.node([path[0], path[1]]);
            let (true, Some(found)) = (path.len() == 3, views(cx, &database)) else {
                return;
            };
            let Some(table) = found.views.get(*view) else {
                return;
            };
            if let Some(entry) = cx.index.drawings.get(*drawing) {
                lines.push(field(t!("Drawing name"), &entry.name));
            }
            match table {
                Err(why) => lines.push(DetailLine::Marker(why.clone())),
                Ok(table) => {
                    for note in &table.notes {
                        lines.push(DetailLine::Marker(note.clone()));
                    }
                    if table.rows == 0 {
                        lines.push(marker(t!("The drawing has none.")));
                    } else {
                        lines.push(DetailLine::Table(Arc::clone(&table.table)));
                    }
                }
            }
            return;
        }
        if path.len() != 2 {
            return;
        }
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
                if let Some(why) = cx.index.dump_unreadable() {
                    lines.push(DetailLine::Marker(why));
                    return;
                }
                match cx.load(id) {
                    None => {
                        lines.push(marker(t!(
                            "The panel does not read them from the database yet."
                        )));
                        lines.push(marker(t!(
                            "Open this row to read them; the first read of a dump takes a few seconds."
                        )));
                    }
                    Some(SectionLoad::Loading) => {
                        lines.push(marker(t!("Reading the database dump…")));
                    }
                    Some(SectionLoad::Failed(error)) => {
                        lines.push(DetailLine::Marker(error.clone()));
                    }
                    Some(SectionLoad::Ready(data)) => {
                        let Some(found) = data.get::<DrawingViews>() else {
                            return;
                        };
                        for (view, table) in VIEWS.iter().zip(&found.views) {
                            match table {
                                Ok(table) => lines.push(field(view.title(), table.rows)),
                                Err(why) => lines.push(field(view.title(), why)),
                            }
                        }
                    }
                }
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

    /// The database's side of a drawing is read when its row is opened or
    /// selected, once for the drawing.
    fn wants(&self, cx: &Cx<'_>, event: &Event<'_>) -> Option<NodeId> {
        let [drawing, kind] = cx.own(event.node)? else {
            return None;
        };
        if KINDS.get(*kind) != Some(&Kind::Database)
            || read(cx, *drawing).is_none()
            || cx.index.dump_unreadable().is_some()
        {
            return None;
        }
        let asked = match event.kind {
            EventKind::Toggled => cx.is_open(event.node),
            EventKind::Selected => true,
            EventKind::Activated => false,
        };
        asked.then(|| event.node.clone())
    }

    fn load(&self, index: &BackupIndex, node: &NodeId) -> Result<SectionData, String> {
        let drawing = match node {
            NodeId::Section(_, path) if path.len() == 2 => path[0],
            _ => return Err(nothing_to_read()),
        };
        let path = index
            .drawings
            .get(drawing)
            .and_then(|entry| entry.pid_path.clone())
            .ok_or_else(nothing_to_read)?;
        let dump = index.dump()?;
        // The panel's own read of the drawing stays with the UI; the file
        // is read again here for the items the database's rows hang on.
        let items = load_items(&path)?;
        let plant = index
            .manifest
            .as_ref()
            .and_then(|manifest| manifest.name.clone())
            .unwrap_or_default();
        Ok(SectionData::new(drawing_views(
            dump.of_drawing(&items, &plant),
        )))
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
    use crate::io::pid_backup_browse::exp_views::{
        Connection, Pipeline, Pipelines, Port, Relation,
    };
    use crate::io::pid_backup_browse::{BackupFile, FileKind};
    use crate::ui::dock::Chrome;
    use crate::ui::window::pid_backup_browser::{
        detail_rows, update, view, visible_rows, Effect, PidBackupBrowser, PidBackupMsg,
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

    /// The first drawing of `state` open, its read in.
    fn read_into(mut state: PidBackupBrowser, items: DrawingItems) -> PidBackupBrowser {
        update(&mut state, PidBackupMsg::Toggle(NodeId::Drawing(0)));
        update(
            &mut state,
            PidBackupMsg::ItemsLoaded(0, Ok(Arc::new(items))),
        );
        state
    }

    /// The first drawing open, its read in, in a backup with no dump.
    fn read_drawing(items: DrawingItems) -> PidBackupBrowser {
        read_into(ready_with_drawings_shown(), items)
    }

    /// The same in a backup that has an Oracle dump.
    fn read_drawing_beside_a_dump(items: DrawingItems) -> PidBackupBrowser {
        let plain = ready_with_drawings_shown();
        let mut index = (**plain.index().expect("the backup is ready")).clone();
        index.files.push(BackupFile {
            name: "Export.dmp".to_string(),
            kind: FileKind::ExportDump,
            version_line: Some("EXPORT:V12.01.00".to_string()),
            ..BackupFile::default()
        });
        let mut state = PidBackupBrowser::default();
        update(&mut state, PidBackupMsg::Loaded(Ok(Arc::new(index))));
        update(&mut state, PidBackupMsg::Toggle(NodeId::Groups));
        for group in [vec![0], vec![0, 0], vec![0, 0, 0]] {
            update(&mut state, PidBackupMsg::Toggle(NodeId::Group(group)));
        }
        read_into(state, items)
    }

    /// What the tree shows under the first drawing: how deep under it, the
    /// label, and whether muted.
    fn under_drawing(state: &PidBackupBrowser) -> Vec<(NodeId, usize, String, bool)> {
        let rows = visible_rows(state);
        let at = rows
            .iter()
            .position(|row| row.id == NodeId::Drawing(0))
            .expect("the drawing has a row");
        rows[at + 1..]
            .iter()
            .take_while(|row| row.depth > rows[at].depth)
            .map(|row| {
                (
                    row.id.clone(),
                    row.depth - rows[at].depth,
                    row.label.clone(),
                    row.disabled,
                )
            })
            .collect()
    }

    /// The section's node of `kind` under the first drawing: read off the
    /// tree, as its slot is its place in the registry.
    fn node(state: &PidBackupBrowser, kind: Kind) -> NodeId {
        let at = KINDS.iter().position(|found| *found == kind).unwrap();
        under_drawing(state)
            .into_iter()
            .map(|(id, ..)| id)
            .find(|id| matches!(id, NodeId::Section(_, path) if path[..] == [0, at]))
            .unwrap_or_else(|| panic!("{kind:?} has a row under the drawing"))
    }

    /// The node of `view` under the database row of the first drawing.
    fn view_node(state: &PidBackupBrowser, view: View) -> NodeId {
        let NodeId::Section(slot, mut path) = node(state, Kind::Database) else {
            unreachable!("a section's node");
        };
        path.push(VIEWS.iter().position(|found| *found == view).unwrap());
        NodeId::Section(slot, path)
    }

    fn value<'a>(rows: &'a [(String, String)], key: &str) -> Option<&'a str> {
        rows.iter()
            .find(|(found, _)| found == key)
            .map(|(_, value)| value.as_str())
    }

    fn titles(titles: &[Cow<'static, str>]) -> String {
        titles.join(" | ")
    }

    #[test]
    fn a_read_drawing_ends_with_what_its_file_holds_beside_the_items() {
        let state = read_drawing(items());
        let shown: Vec<(String, bool)> = under_drawing(&state)
            .into_iter()
            .map(|(_, _, label, muted)| (label, muted))
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
                // The database's side, which this backup has no dump for.
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
                .find(|(found, ..)| *found == id)
                .map(|(_, _, label, muted)| (label, muted)),
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
    fn a_backup_with_no_dump_says_why_the_database_side_is_not_there() {
        let mut state = read_drawing(items());
        let database = node(&state, Kind::Database);
        assert_eq!(
            detail_rows(&state, &database),
            [
                (t!("Drawing name").into_owned(), "A-101".to_string()),
                (
                    t!("Ports, the runs they connect, off-page connectors and pipelines are in the backup's database, not in the drawing's file.")
                        .into_owned(),
                    String::new()
                ),
                (
                    t!("This backup has no database dump.").into_owned(),
                    String::new()
                ),
            ]
        );
        // Nothing to open, and nothing is read for it.
        assert_eq!(
            update(&mut state, PidBackupMsg::Select(database.clone())),
            Effect::None
        );
        assert_eq!(
            update(&mut state, PidBackupMsg::Toggle(database)),
            Effect::None
        );
        assert_eq!(under_drawing(&state).len(), 1 + KINDS.len());
    }

    /// What the database knows of the drawing, as the reader hands it over.
    fn database_side() -> DrawingDump {
        DrawingDump {
            ports: Ok(vec![
                Port {
                    uid: "UID-1.1".to_string(),
                    item_uid: "UID-1".to_string(),
                    item: Some("TT-101".to_string()),
                    class: "PIDSignalPort",
                    number: 1,
                    flow: String::new(),
                    nominal: String::new(),
                },
                Port {
                    uid: "RUN-2.2".to_string(),
                    item_uid: "RUN-2".to_string(),
                    item: None,
                    class: "PIDPipingPort",
                    number: 2,
                    flow: "1".to_string(),
                    nominal: "25 mm".to_string(),
                },
            ]),
            connections: Ok(vec![Connection {
                symbol_uid: "UID-1.1".to_string(),
                run_uid: "RUN-2".to_string(),
                run: Some("PipeRun #2".to_string()),
                on_start: true,
                on_end: false,
                definition: "SignalEnd1Conn",
            }]),
            relations: Ok(vec![Relation {
                definition: "PBSItemNotes",
                from_uid: "NOTE-1".to_string(),
                to_uid: "UID-1".to_string(),
                from: None,
                to: Some("TT-101".to_string()),
            }]),
            opc: Ok(Vec::new()),
            pipelines: Ok(Pipelines {
                rows: vec![Pipeline {
                    uid: "PIPELINE-1".to_string(),
                    name: "25 mm-PG-1001-F9".to_string(),
                    connectors: vec!["PipeRun #2".to_string(), "RUN-3".to_string()],
                }],
                connectors: 3,
                without_sequence: 1,
                fluid_code_missing: 0,
                not_in_piperun: 0,
            }),
        }
    }

    /// Open the database row of the first drawing and hand the panel `read`
    /// as the worker thread would.
    fn open_database(state: &mut PidBackupBrowser, read: Result<DrawingDump, String>) -> NodeId {
        let database = node(state, Kind::Database);
        let NodeId::Section(slot, _) = database else {
            unreachable!("a section's node");
        };
        // Opening the row asks for the read, once.
        assert_eq!(
            update(state, PidBackupMsg::Toggle(database.clone())),
            Effect::LoadSection {
                generation: state.generation,
                section: slot,
                node: database.clone(),
            }
        );
        assert_eq!(
            update(state, PidBackupMsg::Select(database.clone())),
            Effect::None
        );
        let generation = state.generation;
        update(
            state,
            PidBackupMsg::SectionLoaded {
                generation,
                section: slot,
                node: database.clone(),
                result: read.map(|dump| SectionData::new(drawing_views(dump))),
            },
        );
        database
    }

    #[test]
    fn beside_a_dump_the_database_row_opens_into_its_views() {
        let mut state = read_drawing_beside_a_dump(items());
        let database = node(&state, Kind::Database);
        // Before anything is read: the row is there to be opened.
        let row = visible_rows(&state)
            .into_iter()
            .find(|row| row.id == database)
            .unwrap();
        assert!(row.expandable && !row.expanded && !row.disabled);
        assert_eq!(
            detail_rows(&state, &database)[2..],
            [
                (
                    t!("The panel does not read them from the database yet.").into_owned(),
                    String::new()
                ),
                (
                    t!("Open this row to read them; the first read of a dump takes a few seconds.")
                        .into_owned(),
                    String::new()
                ),
            ]
        );

        // While the dump is read the row is open and says so.
        let NodeId::Section(slot, _) = database else {
            unreachable!("a section's node");
        };
        update(&mut state, PidBackupMsg::Toggle(database.clone()));
        assert_eq!(
            detail_rows(&state, &database)[2..],
            [(t!("Reading the database dump…").into_owned(), String::new())]
        );
        assert_eq!(under_drawing(&state).len(), 1 + KINDS.len());
        let generation = state.generation;
        update(
            &mut state,
            PidBackupMsg::SectionLoaded {
                generation,
                section: slot,
                node: database.clone(),
                result: Ok(SectionData::new(drawing_views(database_side()))),
            },
        );

        // The views, each with its count; one with nothing in it is muted.
        let shown: Vec<(usize, String, bool)> = under_drawing(&state)
            .into_iter()
            .filter(|(id, ..)| matches!(id, NodeId::Section(_, path) if path.len() == 3))
            .map(|(_, depth, label, muted)| (depth, label, muted))
            .collect();
        assert_eq!(
            shown,
            [
                (2, format!("{} (2)", t!("Ports")), false),
                (2, format!("{} (1)", t!("Run connections")), false),
                (2, format!("{} (1)", t!("Nozzles and notes")), false),
                (2, format!("{} (0)", t!("Off-page connectors")), true),
                (2, format!("{} (1)", t!("Pipelines")), false),
            ]
        );
        // The row's own details count them.
        assert_eq!(
            detail_rows(&state, &database)[2..],
            [
                (t!("Ports").into_owned(), "2".to_string()),
                (t!("Run connections").into_owned(), "1".to_string()),
                (t!("Nozzles and notes").into_owned(), "1".to_string()),
                (t!("Off-page connectors").into_owned(), "0".to_string()),
                (t!("Pipelines").into_owned(), "1".to_string()),
            ]
        );
        // Folding the row hides the views and keeps what was read.
        assert_eq!(
            update(&mut state, PidBackupMsg::Toggle(database.clone())),
            Effect::None
        );
        assert_eq!(under_drawing(&state).len(), 1 + KINDS.len());
        assert_eq!(
            update(&mut state, PidBackupMsg::Toggle(database)),
            Effect::None
        );
        assert_eq!(under_drawing(&state).len(), 1 + KINDS.len() + VIEWS.len());
    }

    #[test]
    fn the_views_of_the_database_are_tables() {
        let mut state = read_drawing_beside_a_dump(items());
        open_database(&mut state, Ok(database_side()));

        assert_eq!(
            detail_rows(&state, &view_node(&state, View::Ports)),
            [
                (t!("Drawing name").into_owned(), "A-101".to_string()),
                (
                    titles(&[
                        t!("Port UID"),
                        t!("Item name"),
                        t!("Item UID"),
                        t!("Port class"),
                        t!("Port number"),
                        t!("Flow direction code"),
                        t!("Nominal diameter"),
                    ]),
                    tf!("{} row(s)", 2).into_owned()
                ),
                (
                    "UID-1.1".to_string(),
                    "TT-101 | UID-1 | PIDSignalPort | 1".to_string()
                ),
                (
                    "RUN-2.2".to_string(),
                    " | RUN-2 | PIDPipingPort | 2 | 1 | 25 mm".to_string()
                ),
            ]
        );
        assert_eq!(
            detail_rows(&state, &view_node(&state, View::Connections))[2],
            (
                "UID-1.1".to_string(),
                format!(
                    "PipeRun #2 | RUN-2 | {} | SignalEnd1Conn",
                    t!("at its start")
                )
            )
        );
        assert_eq!(
            detail_rows(&state, &view_node(&state, View::Relations))[2],
            (
                "PBSItemNotes".to_string(),
                "NOTE-1 | TT-101 | UID-1".to_string()
            )
        );
        // A view with nothing in it says so.
        assert_eq!(
            detail_rows(&state, &view_node(&state, View::Opc))[1..],
            [(t!("The drawing has none.").into_owned(), String::new())]
        );
        // The pipelines say how the connectors fared, then list themselves.
        let pipelines = detail_rows(&state, &view_node(&state, View::Pipelines));
        assert_eq!(
            pipelines[1],
            (
                tf!(
                    "{} piping connector(s): {} with no tag sequence number, {} whose fluid code the list does not spell, {} not among the pipe runs",
                    3,
                    1,
                    0,
                    0
                )
                .into_owned(),
                String::new()
            )
        );
        assert_eq!(
            pipelines[3],
            (
                "25 mm-PG-1001-F9".to_string(),
                "2 | PipeRun #2, RUN-3 | PIPELINE-1".to_string()
            )
        );
    }

    #[test]
    fn a_view_the_reader_could_not_follow_says_why_and_the_others_are_there() {
        let mut state = read_drawing_beside_a_dump(items());
        let mut side = database_side();
        side.connections = Err("T_PIPERUN: row 0 NUMBER is not an integer".to_string());
        let database = open_database(&mut state, Ok(side));
        let why = tf!(
            "The reader could not follow the dump: {}",
            "T_PIPERUN: row 0 NUMBER is not an integer"
        )
        .into_owned();

        let connections = view_node(&state, View::Connections);
        let row = visible_rows(&state)
            .into_iter()
            .find(|row| row.id == connections)
            .unwrap();
        // No count to show, and muted.
        assert_eq!(row.label, t!("Run connections").as_ref());
        assert!(row.disabled);
        assert_eq!(
            detail_rows(&state, &connections)[1..],
            [(why.clone(), String::new())]
        );
        assert_eq!(
            value(&detail_rows(&state, &database), &t!("Run connections")),
            Some(why.as_str())
        );
        // The ports are read all the same.
        assert_eq!(
            value(&detail_rows(&state, &database), &t!("Ports")),
            Some("2")
        );
    }

    #[test]
    fn a_dump_that_does_not_read_is_said_on_the_row() {
        let mut state = read_drawing_beside_a_dump(items());
        let database = open_database(&mut state, Err("cannot read Export.dmp".to_string()));
        assert_eq!(
            detail_rows(&state, &database)[2..],
            [("cannot read Export.dmp".to_string(), String::new())]
        );
        // No views, and no second read.
        assert_eq!(under_drawing(&state).len(), 1 + KINDS.len());
        assert_eq!(
            update(&mut state, PidBackupMsg::Select(database)),
            Effect::None
        );
    }

    #[test]
    fn a_drawing_whose_file_gave_nothing_else_keeps_only_its_items() {
        // The items alone, as a drawing read before this section existed.
        let state = read_drawing(items_of("UID-1", "Instrument"));
        let labels: Vec<String> = under_drawing(&state)
            .into_iter()
            .map(|(_, _, label, _)| label)
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

    fn draw(state: &PidBackupBrowser) -> iced_test::simulator::Simulator<'_, Message> {
        iced_test::simulator::Simulator::with_size(
            iced::Settings::default(),
            (340.0, 700.0),
            view(state, 340.0, Chrome::default()),
        )
    }

    fn click(state: &PidBackupBrowser, at: iced::Point) -> Vec<PidBackupMsg> {
        let mut ui = draw(state);
        ui.point_at(at);
        let _ = ui.simulate(iced_test::simulator::click());
        ui.into_messages()
            .filter_map(|message| match message {
                Message::PidBackup(message) => Some(message),
                _ => None,
            })
            .collect()
    }

    fn bounds(state: &PidBackupBrowser, label: &str) -> iced::Rectangle {
        draw(state)
            .find(label)
            .unwrap_or_else(|error| panic!("{label} is drawn: {error:?}"))
            .visible_bounds()
            .unwrap_or_else(|| panic!("{label} is on screen"))
    }

    /// The branch as the panel draws it: a click on a list's row selects it,
    /// its rows are a grid, and a click on a title sorts them.
    #[test]
    fn the_drawn_representations_row_is_clicked_and_its_table_sorts() {
        let _guard = crate::headless_ui_lock();
        let mut state = read_drawing(items());

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

    /// The database's row as the panel draws it: its arrow is where a press
    /// lands, and a view under it is a row a click selects and a table.
    #[test]
    fn the_drawn_database_row_unfolds_and_a_view_under_it_is_clicked() {
        let _guard = crate::headless_ui_lock();
        let mut state = read_drawing_beside_a_dump(items());
        let database = node(&state, Kind::Database);

        // The arrow stands left of the label.
        let label = bounds(&state, t!("Ports, connections, OPC and pipelines").as_ref());
        let pressed = click(&state, iced::Point::new(label.x - 7.0, label.center_y()));
        assert!(
            pressed.iter().any(
                |message| matches!(message, PidBackupMsg::Toggle(found) if *found == database)
            ),
            "{pressed:?}"
        );
        // The press asks for the read; hand it over as the worker would.
        let NodeId::Section(slot, _) = database else {
            unreachable!("a section's node");
        };
        for message in pressed {
            update(&mut state, message);
        }
        let generation = state.generation;
        update(
            &mut state,
            PidBackupMsg::SectionLoaded {
                generation,
                section: slot,
                node: database,
                result: Ok(SectionData::new(drawing_views(database_side()))),
            },
        );

        let ports = format!("{} (2)", t!("Ports"));
        let clicked = click(&state, bounds(&state, &ports).center());
        let id = view_node(&state, View::Ports);
        assert!(
            clicked
                .iter()
                .any(|message| matches!(message, PidBackupMsg::Select(found) if *found == id)),
            "{clicked:?}"
        );
        for message in clicked {
            update(&mut state, message);
        }
        // Its rows are drawn as a grid, in the order the reader gives them.
        assert!(bounds(&state, "UID-1.1").y < bounds(&state, "RUN-2.2").y);
    }
}
