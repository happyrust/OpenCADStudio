//! The tables of the backup's database, under the dump that holds them.
//!
//! `Export.dmp` is most of what a backup weighs, and the tree could only
//! say how big it is. Its row now opens into the schemas of the database and
//! their tables:
//!
//! ```text
//! Export.dmp
//!   SQPlantpidd (25)
//!   SQPlantd (25)
//!   SQPlant (22)
//!   SQPlantpid (82)
//!     T_DRAWING — 53 row(s)
//!     T_MODELITEM — 47124 row(s)
//!     ...
//! ```
//!
//! The dump's own details add the totals, what the export log says of the
//! export, and the schemas side by side; a schema's are its tables as a
//! grid that sorts by name or by rows; a table's are its row count. A table
//! with no row is drawn muted, so the ones that hold something stand out.
//!
//! The content is [`TableCatalog`], read once when the backup is indexed:
//! the names from `Manifest.txt`, the counts from `Export.log`. A backup
//! without the log (a SQL Server one) lists its tables without counts, and
//! says why. The rows themselves are not read here.

use std::sync::Arc;

use super::grid::{Column, Table};
use super::section::{Cx, Event, EventKind, Section, SectionData, SectionLoad};
use super::{DetailLine, NodeId, Row};
use crate::io::pid_backup_browse::exp_views::{Dump, Port};
use crate::io::pid_backup_browse::export_log::{CatalogTable, TableCatalog};
use crate::io::pid_backup_browse::{BackupIndex, FileKind};
use crate::{t, tf};

/// The database's branch of the tree.
pub(super) struct DatabaseSection;

/// The tables the reader has a view of: not their rows as they are stored,
/// the columns one of its rules needs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Typed {
    PipeRuns,
    PipingPorts,
    SignalPorts,
    Nozzles,
    ItemNotes,
    CodeLists,
}

impl Typed {
    fn of(table: &str) -> Option<Typed> {
        match table.to_ascii_uppercase().as_str() {
            "T_PIPERUN" => Some(Typed::PipeRuns),
            "T_PIPINGPOINT" => Some(Typed::PipingPorts),
            "T_SIGNALPOINT" => Some(Typed::SignalPorts),
            "T_NOZZLE" => Some(Typed::Nozzles),
            "T_ITEMNOTE" => Some(Typed::ItemNotes),
            "CODELISTS" => Some(Typed::CodeLists),
            _ => None,
        }
    }
}

/// The view the reader has of the table at `path` of the section, if any.
/// A name two schemas have (`CODELISTS`) is the reader's in the schema that
/// comes first in the dump.
fn typed_at(index: &BackupIndex, path: &[usize]) -> Option<Typed> {
    let [schema, table] = path else {
        return None;
    };
    let catalog = index.table_catalog.as_ref()?;
    let name = &catalog.schemas.get(*schema)?.tables.get(*table)?.name;
    let typed = Typed::of(name)?;
    (catalog.first_schema_with(name) == Some(*schema)).then_some(typed)
}

/// What the reader gives of a table: read off the UI thread, kept for as
/// long as the backup is open.
struct TypedView {
    /// What the view holds and leaves out, above the rows.
    notes: Vec<String>,
    table: Arc<Table>,
}

fn marker_line(text: std::borrow::Cow<'static, str>) -> DetailLine {
    DetailLine::Marker(text.into_owned())
}

/// The reader's own words for why it stopped, with what they are about.
fn unread(why: String) -> String {
    tf!("The reader could not follow the dump: {}", why).into_owned()
}

fn port_cells(port: &Port) -> Vec<String> {
    vec![
        port.uid.clone(),
        port.item_uid.clone(),
        port.number.to_string(),
        port.flow.clone(),
        port.nominal.clone(),
    ]
}

/// The view of `typed`, as a table.
fn typed_view(dump: &Dump, typed: Typed) -> Result<TypedView, String> {
    let mut notes = Vec::new();
    let table = match typed {
        Typed::PipeRuns => {
            let runs = dump.pipe_runs().map_err(unread)?;
            notes.push(
                tf!(
                    "{} of the {} run(s) belong to a pipeline",
                    runs.iter().filter(|run| !run.pipeline.is_empty()).count(),
                    runs.len()
                )
                .into_owned(),
            );
            Table::new(
                vec![
                    Column::text("SP_ID"),
                    Column::text(t!("Pipeline name")),
                    Column::text(t!("Nominal diameter")),
                    Column::text(t!("Fluid code")),
                    Column::text(t!("Materials class")),
                    Column::text(t!("Tag sequence number")),
                    Column::text(t!("Fluid system code")),
                    Column::text(t!("Insulation spec")),
                ],
                runs.into_iter()
                    .map(|run| {
                        vec![
                            run.id,
                            run.pipeline,
                            run.nominal,
                            run.fluid,
                            run.materials_class,
                            run.tag_sequence,
                            run.fluid_system,
                            run.insulation,
                        ]
                    })
                    .collect(),
            )
        }
        Typed::PipingPorts | Typed::SignalPorts => {
            let piping = typed == Typed::PipingPorts;
            let (ports, rows) = if piping {
                dump.piping_ports()
            } else {
                dump.signal_ports()
            }
            .map_err(unread)?;
            if ports.len() != rows {
                notes.push(
                    tf!("{} of the table's {} row(s) are ports", ports.len(), rows).into_owned(),
                );
            }
            let mut columns = vec![
                Column::text(t!("Port UID")),
                Column::text(t!("Item UID")),
                Column::number(t!("Port number")),
            ];
            if piping {
                columns.push(Column::text(t!("Flow direction code")));
                columns.push(Column::text(t!("Nominal diameter")));
            }
            let width = columns.len();
            Table::new(
                columns,
                ports
                    .iter()
                    .map(|port| {
                        let mut cells = port_cells(port);
                        cells.truncate(width);
                        cells
                    })
                    .collect(),
            )
        }
        Typed::Nozzles | Typed::ItemNotes => {
            let (definition, from, to) = if typed == Typed::Nozzles {
                (
                    "EquipmentComponentComposition",
                    t!("Nozzle UID"),
                    t!("Equipment UID"),
                )
            } else {
                ("PBSItemNotes", t!("Note UID"), t!("Item UID"))
            };
            let relations = dump.relations().map_err(unread)?;
            Table::new(
                vec![Column::text(from), Column::text(to)],
                relations
                    .iter()
                    .filter(|relation| relation.definition == definition)
                    .map(|relation| vec![relation.from_uid.clone(), relation.to_uid.clone()])
                    .collect(),
            )
        }
        Typed::CodeLists => {
            let lists = dump.code_lists().map_err(unread)?;
            notes.push(
                tf!(
                    "{} entries of {} list(s); the table has {} row(s)",
                    lists.entries.len(),
                    lists.lists,
                    lists.rows
                )
                .into_owned(),
            );
            notes.push(
                tf!(
                    "Entries with no short text, and lists numbered above {}, are left out.",
                    lists.probed_to
                )
                .into_owned(),
            );
            Table::new(
                vec![
                    Column::number(t!("List number")),
                    Column::number(t!("List index")),
                    Column::text(t!("Short text")),
                ],
                lists
                    .entries
                    .into_iter()
                    .map(|entry| vec![entry.list.to_string(), entry.index.to_string(), entry.text])
                    .collect(),
            )
        }
    };
    notes.push(
        t!("These are the columns the reader knows of the table, not all of it.").into_owned(),
    );
    Ok(TypedView {
        notes,
        table: Arc::new(table),
    })
}

/// The catalog, when `host` is the dump's own row.
fn catalog_under<'a>(index: &'a BackupIndex, host: &NodeId) -> Option<&'a TableCatalog> {
    let NodeId::File(at) = host else {
        return None;
    };
    let file = index.files.get(*at)?;
    (file.kind == FileKind::ExportDump)
        .then_some(index.table_catalog.as_ref())
        .flatten()
}

/// A count as a cell or a field; nothing when the log states none.
fn number(value: Option<u64>) -> String {
    value.map(|value| value.to_string()).unwrap_or_default()
}

fn count(label: std::borrow::Cow<'static, str>, value: impl ToString) -> DetailLine {
    DetailLine::Field(label.into_owned(), value.to_string())
}

/// A table's row: its name and, when the log counted them, its rows.
fn table_label(table: &CatalogTable) -> String {
    match table.rows {
        Some(rows) => format!("{} — {}", table.name, tf!("{} row(s)", rows)),
        None => table.name.clone(),
    }
}

impl Section for DatabaseSection {
    fn hosts(&self, cx: &Cx<'_>, host: &NodeId) -> bool {
        catalog_under(cx.index, host).is_some()
    }

    fn rows(&self, cx: &Cx<'_>, host: &NodeId, depth: usize, rows: &mut Vec<Row>) {
        let Some(catalog) = catalog_under(cx.index, host) else {
            return;
        };
        for (at, schema) in catalog.schemas.iter().enumerate() {
            let id = cx.node([at]);
            let expanded = cx.is_open(&id);
            rows.push(Row {
                id,
                depth,
                label: format!("{} ({})", schema.name, schema.tables.len()),
                expandable: true,
                expanded,
                disabled: false,
            });
            if !expanded {
                continue;
            }
            for (table_at, table) in schema.tables.iter().enumerate() {
                rows.push(Row {
                    id: cx.node([at, table_at]),
                    depth: depth + 1,
                    label: table_label(table),
                    expandable: false,
                    expanded: false,
                    // An empty table is there, and has nothing to show.
                    disabled: table.rows == Some(0),
                });
            }
        }
    }

    fn details(&self, cx: &Cx<'_>, id: &NodeId, lines: &mut Vec<DetailLine>) {
        // The dump's own row: the database as a whole.
        if let Some(catalog) = catalog_under(cx.index, id) {
            // Once something had the dump read: how long that took, and
            // whether the reader followed all of it.
            if let Some(dump) = cx.index.dump_if_read() {
                lines.push(DetailLine::Field(
                    t!("Dump read in").into_owned(),
                    format!("{:.1} s", dump.read_in.as_secs_f64()),
                ));
                if let Some(why) = dump.partial() {
                    lines.push(DetailLine::Marker(unread(why.to_string())));
                    lines.push(marker_line(t!(
                        "The views that need the table the reader stopped at are missing."
                    )));
                }
            }
            database_lines(catalog, lines);
            return;
        }
        let (Some(path), Some(catalog)) = (cx.own(id), cx.index.table_catalog.as_ref()) else {
            return;
        };
        let Some(schema) = path.first().and_then(|at| catalog.schemas.get(*at)) else {
            return;
        };
        match path {
            [_] => {
                lines.push(count(t!("Schema"), &schema.name));
                lines.push(count(t!("Table count"), schema.tables.len()));
                if catalog.has_log {
                    lines.push(count(t!("Tables with rows"), schema.filled()));
                    lines.push(count(t!("Row count"), number(schema.rows())));
                }
                let mut columns = vec![Column::text(t!("Table name"))];
                if catalog.has_log {
                    columns.push(Column::number(t!("Row count")));
                }
                lines.push(DetailLine::Table(Arc::new(Table::new(
                    columns,
                    schema
                        .tables
                        .iter()
                        .map(|table| {
                            let mut cells = vec![table.name.clone()];
                            if catalog.has_log {
                                cells.push(number(table.rows));
                            }
                            cells
                        })
                        .collect(),
                ))));
            }
            [_, table] => {
                let Some(table) = schema.tables.get(*table) else {
                    return;
                };
                lines.push(count(t!("Schema"), &schema.name));
                lines.push(count(t!("Table name"), &table.name));
                let marker =
                    |text: std::borrow::Cow<'static, str>| DetailLine::Marker(text.into_owned());
                match table.rows {
                    Some(rows) => lines.push(count(t!("Row count"), rows)),
                    None if catalog.has_log => lines.push(marker(t!(
                        "The export log states no row count for this table."
                    ))),
                    None => {}
                }
                if catalog.has_log && !table.in_log {
                    lines.push(marker(t!("Named in Manifest.txt only")));
                }
                if catalog.has_log && !table.in_manifest {
                    lines.push(marker(t!("Named in Export.log only")));
                }
                if typed_at(cx.index, path).is_none() {
                    lines.push(marker(t!("The rows of the table are not shown yet.")));
                    return;
                }
                // A table the reader has a view of: the view, once the dump
                // is read, which selecting the table starts.
                match cx.load(id) {
                    Some(SectionLoad::Loading) => {
                        lines.push(marker(t!("Reading the database dump…")));
                    }
                    Some(SectionLoad::Failed(error)) => {
                        lines.push(DetailLine::Marker(error.clone()));
                    }
                    Some(SectionLoad::Ready(data)) => {
                        if let Some(view) = data.get::<TypedView>() {
                            for note in &view.notes {
                                lines.push(DetailLine::Marker(note.clone()));
                            }
                            lines.push(DetailLine::Table(Arc::clone(&view.table)));
                        }
                    }
                    None => match cx.index.dump_unreadable() {
                        Some(why) => lines.push(DetailLine::Marker(why)),
                        None => lines.push(marker(t!(
                            "The reader has a view of this table; select it to read the dump."
                        ))),
                    },
                }
            }
            _ => {}
        }
    }

    fn wants(&self, cx: &Cx<'_>, event: &Event<'_>) -> Option<NodeId> {
        let path = cx.own(event.node)?;
        (event.kind == EventKind::Selected
            && typed_at(cx.index, path).is_some()
            && cx.index.dump_unreadable().is_none())
        .then(|| event.node.clone())
    }

    fn load(&self, index: &BackupIndex, node: &NodeId) -> Result<SectionData, String> {
        let typed = match node {
            NodeId::Section(_, path) => typed_at(index, path),
            _ => None,
        };
        let Some(typed) = typed else {
            return Err(t!("this node has nothing to read").into_owned());
        };
        let dump = index.dump()?;
        typed_view(&dump, typed).map(SectionData::new)
    }
}

/// What the database is as a whole, under the dump's own details.
fn database_lines(catalog: &TableCatalog, lines: &mut Vec<DetailLine>) {
    let marker = |text: std::borrow::Cow<'_, str>| DetailLine::Marker(text.into_owned());
    lines.push(count(t!("Schema count"), catalog.schemas.len()));
    lines.push(count(t!("Table count"), catalog.table_count()));
    if catalog.has_log {
        lines.push(count(
            t!("Tables with rows"),
            catalog
                .schemas
                .iter()
                .map(|schema| schema.filled())
                .sum::<usize>(),
        ));
        lines.push(count(t!("Row count"), number(catalog.rows())));
        for (label, value) in [
            (t!("Exported from"), &catalog.server),
            (t!("Character sets"), &catalog.character_sets),
            (t!("Export outcome"), &catalog.outcome),
        ] {
            if let Some(value) = value {
                lines.push(DetailLine::Field(label.into_owned(), value.clone()));
            }
        }
        let (manifest_only, log_only) = catalog.one_sided();
        if manifest_only > 0 {
            lines.push(marker(tf!(
                "{} table(s) are named in Manifest.txt only",
                manifest_only
            )));
        }
        if log_only > 0 {
            lines.push(marker(tf!(
                "{} table(s) are named in Export.log only",
                log_only
            )));
        }
        if !catalog.messages.is_empty() {
            lines.push(DetailLine::Heading(t!("Export messages").into_owned()));
            for message in &catalog.messages {
                lines.push(DetailLine::Marker(message.clone()));
            }
        }
    } else {
        lines.push(marker(t!(
            "This backup has no Export.log: the tables are listed without row counts."
        )));
    }
    let mut columns = vec![
        Column::text(t!("Schema")),
        Column::number(t!("Table count")),
    ];
    if catalog.has_log {
        columns.push(Column::number(t!("Tables with rows")));
        columns.push(Column::number(t!("Row count")));
    }
    lines.push(DetailLine::Table(Arc::new(Table::new(
        columns,
        catalog
            .schemas
            .iter()
            .map(|schema| {
                let mut cells = vec![schema.name.clone(), schema.tables.len().to_string()];
                if catalog.has_log {
                    cells.push(schema.filled().to_string());
                    cells.push(number(schema.rows()));
                }
                cells
            })
            .collect(),
    ))));
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::Message;
    use crate::io::pid_backup_browse::export_log::{parse_export_log, table_catalog};
    use crate::io::pid_backup_browse::manifest::DbObject;
    use crate::io::pid_backup_browse::BackupFile;
    use crate::ui::dock::Chrome;
    use crate::ui::window::pid_backup_browser::{
        detail_rows, update, view, visible_rows, Effect, PidBackupBrowser, PidBackupMsg,
    };

    const LOG: &str = "
Connected to: Oracle Database 12c Enterprise Edition Release 12.1.0.2.0 - 64bit Production
Export done in AL32UTF8 character set and AL16UTF16 NCHAR character set
. about to export SQPLANTD's tables via Conventional Path ...
. . exporting table                   APPLICATIONS          5 rows exported
. . exporting table                     INTERSECTS          0 rows exported
. about to export SQPLANTPID's tables via Conventional Path ...
. . exporting table                      T_DRAWING         53 rows exported
. . exporting table                    T_MODELITEM      47124 rows exported
. . exporting table                         T_NOTE        800 rows exported
. . exporting table                      T_AREABRK          0 rows exported
Export terminated successfully without warnings.
";

    fn object(schema: &str, name: &str) -> DbObject {
        DbObject {
            schema: schema.to_string(),
            name: name.to_string(),
        }
    }

    fn tables() -> Vec<DbObject> {
        vec![
            object("SQPlantd", "APPLICATIONS"),
            object("SQPlantd", "INTERSECTS"),
            object("SQPlantpid", "T_NOTE"),
            object("SQPlantpid", "T_MODELITEM"),
            object("SQPlantpid", "T_DRAWING"),
            object("SQPlantpid", "T_AREABRK"),
        ]
    }

    fn file(name: &str, kind: FileKind) -> BackupFile {
        BackupFile {
            name: name.to_string(),
            kind,
            size: 74_489_856,
            ..BackupFile::default()
        }
    }

    /// A backup whose dump is its second file, with or without the log that
    /// counts the rows.
    fn ready(with_log: bool) -> PidBackupBrowser {
        let log = with_log.then(|| parse_export_log(LOG.lines()));
        let index = BackupIndex {
            files: vec![
                file("Manifest.txt", FileKind::Manifest),
                file("Export.dmp", FileKind::ExportDump),
                file("PlantData~2~711.zip", FileKind::PlantData),
            ],
            table_catalog: table_catalog(&tables(), log.as_ref()),
            ..BackupIndex::default()
        };
        let mut state = PidBackupBrowser::default();
        update(&mut state, PidBackupMsg::Loaded(Ok(Arc::new(index))));
        update(&mut state, PidBackupMsg::Toggle(NodeId::Files));
        state
    }

    const DUMP: NodeId = NodeId::File(1);

    /// The section's node at `path`. Its slot is its place in the registry:
    /// read off a row of the branch rather than written down here.
    fn node(path: &[usize]) -> NodeId {
        let mut state = ready(true);
        update(&mut state, PidBackupMsg::Toggle(DUMP));
        let slot = visible_rows(&state)
            .iter()
            .find_map(|row| match &row.id {
                NodeId::Section(slot, _) => Some(*slot),
                _ => None,
            })
            .expect("the dump opens into the section's rows");
        NodeId::Section(slot, path.to_vec())
    }

    fn under_dump(state: &PidBackupBrowser) -> Vec<(usize, String, bool)> {
        let rows = visible_rows(state);
        let at = rows
            .iter()
            .position(|row| row.id == DUMP)
            .expect("the dump has a row");
        rows[at + 1..]
            .iter()
            .take_while(|row| row.depth > rows[at].depth)
            .map(|row| (row.depth - rows[at].depth, row.label.clone(), row.disabled))
            .collect()
    }

    fn value<'a>(rows: &'a [(String, String)], key: &str) -> Option<&'a str> {
        rows.iter()
            .find(|(found, _)| found == key)
            .map(|(_, value)| value.as_str())
    }

    #[test]
    fn the_dump_opens_into_its_schemas_and_a_schema_into_its_tables() {
        let mut state = ready(true);
        let dump = visible_rows(&state)
            .into_iter()
            .find(|row| row.id == DUMP)
            .expect("the dump has a row");
        assert!(dump.expandable && !dump.expanded);
        update(&mut state, PidBackupMsg::Toggle(DUMP));
        assert_eq!(
            under_dump(&state),
            [
                (1, "SQPlantd (2)".to_string(), false),
                (1, "SQPlantpid (4)".to_string(), false),
            ]
        );
        update(&mut state, PidBackupMsg::Toggle(node(&[1])));
        let rows = |count: u64| tf!("{} row(s)", count).into_owned();
        assert_eq!(
            under_dump(&state),
            [
                (1, "SQPlantd (2)".to_string(), false),
                (1, "SQPlantpid (4)".to_string(), false),
                // By name; the empty table is muted.
                (2, format!("T_AREABRK — {}", rows(0)), true),
                (2, format!("T_DRAWING — {}", rows(53)), false),
                (2, format!("T_MODELITEM — {}", rows(47_124)), false),
                (2, format!("T_NOTE — {}", rows(800)), false),
            ]
        );
        // Folding the dump takes the whole branch with it.
        update(&mut state, PidBackupMsg::Toggle(DUMP));
        assert!(under_dump(&state).is_empty());
    }

    #[test]
    fn the_dumps_details_total_the_database_and_list_its_schemas() {
        let state = ready(true);
        let rows = detail_rows(&state, &DUMP);
        // The file's own line first.
        assert_eq!(
            rows[0],
            (
                t!("Backup file name").into_owned(),
                "Export.dmp".to_string()
            )
        );
        assert_eq!(value(&rows, &t!("Schema count")), Some("2"));
        assert_eq!(value(&rows, &t!("Table count")), Some("6"));
        assert_eq!(value(&rows, &t!("Tables with rows")), Some("4"));
        assert_eq!(value(&rows, &t!("Row count")), Some("47982"));
        assert_eq!(
            value(&rows, &t!("Exported from")),
            Some("Oracle Database 12c Enterprise Edition Release 12.1.0.2.0 - 64bit Production")
        );
        assert_eq!(
            value(&rows, &t!("Export outcome")),
            Some("Export terminated successfully without warnings.")
        );
        let titles = [
            t!("Schema"),
            t!("Table count"),
            t!("Tables with rows"),
            t!("Row count"),
        ]
        .join(" | ");
        let table = rows
            .iter()
            .position(|(key, _)| *key == titles)
            .expect("the schemas are a table");
        assert_eq!(
            rows[table + 1..],
            [
                ("SQPlantd".to_string(), "2 | 1 | 5".to_string()),
                ("SQPlantpid".to_string(), "4 | 3 | 47977".to_string()),
            ]
        );
        // Every other file's details are what they were.
        assert_eq!(
            value(&detail_rows(&state, &NodeId::File(2)), &t!("Schema count")),
            None
        );
    }

    #[test]
    fn a_schema_lists_its_tables_and_a_table_states_its_rows() {
        let state = ready(true);
        let rows = detail_rows(&state, &node(&[1]));
        assert_eq!(
            rows[..4],
            [
                (t!("Schema").into_owned(), "SQPlantpid".to_string()),
                (t!("Table count").into_owned(), "4".to_string()),
                (t!("Tables with rows").into_owned(), "3".to_string()),
                (t!("Row count").into_owned(), "47977".to_string()),
            ]
        );
        assert_eq!(
            rows[4].0,
            format!("{} | {}", t!("Table name"), t!("Row count"))
        );
        assert_eq!(
            rows[5..],
            [
                ("T_AREABRK".to_string(), "0".to_string()),
                ("T_DRAWING".to_string(), "53".to_string()),
                ("T_MODELITEM".to_string(), "47124".to_string()),
                ("T_NOTE".to_string(), "800".to_string()),
            ]
        );

        assert_eq!(
            detail_rows(&state, &node(&[1, 2])),
            [
                (t!("Schema").into_owned(), "SQPlantpid".to_string()),
                (t!("Table name").into_owned(), "T_MODELITEM".to_string()),
                (t!("Row count").into_owned(), "47124".to_string()),
                (
                    t!("The rows of the table are not shown yet.").into_owned(),
                    String::new()
                ),
            ]
        );
        // A node the catalog does not hold has nothing to say.
        assert!(detail_rows(&state, &node(&[7])).is_empty());
        assert!(detail_rows(&state, &node(&[1, 99])).is_empty());
    }

    #[test]
    fn a_backup_without_the_log_lists_the_tables_without_counts_and_says_why() {
        let mut state = ready(false);
        update(&mut state, PidBackupMsg::Toggle(DUMP));
        update(&mut state, PidBackupMsg::Toggle(node(&[0])));
        assert_eq!(
            under_dump(&state)[..3],
            [
                (1, "SQPlantd (2)".to_string(), false),
                (2, "APPLICATIONS".to_string(), false),
                (2, "INTERSECTS".to_string(), false),
            ]
        );
        let rows = detail_rows(&state, &DUMP);
        assert_eq!(value(&rows, &t!("Table count")), Some("6"));
        assert_eq!(value(&rows, &t!("Row count")), None);
        assert!(rows.contains(&(
            t!("This backup has no Export.log: the tables are listed without row counts.")
                .into_owned(),
            String::new()
        )));
        // The schema's table has the names alone.
        let rows = detail_rows(&state, &node(&[1]));
        assert_eq!(rows[..2].len(), 2);
        assert_eq!(rows[2].0, t!("Table name").as_ref());
        assert_eq!(rows[3], ("T_AREABRK".to_string(), String::new()));
        assert_eq!(
            detail_rows(&state, &node(&[1, 0])),
            [
                (t!("Schema").into_owned(), "SQPlantpid".to_string()),
                (t!("Table name").into_owned(), "T_AREABRK".to_string()),
                (
                    t!("The rows of the table are not shown yet.").into_owned(),
                    String::new()
                ),
            ]
        );
    }

    #[test]
    fn a_table_only_one_source_names_says_so() {
        let mut log = parse_export_log(LOG.lines());
        log.schemas[1].tables.push(("T_EXTRA".to_string(), None));
        log.messages
            .push("EXP-00091: Exporting questionable statistics.".to_string());
        let mut manifest = tables();
        manifest.push(object("SQPlantpid", "T_UNLOGGED"));
        let index = BackupIndex {
            files: vec![file("Export.dmp", FileKind::ExportDump)],
            table_catalog: table_catalog(&manifest, Some(&log)),
            ..BackupIndex::default()
        };
        let mut state = PidBackupBrowser::default();
        update(&mut state, PidBackupMsg::Loaded(Ok(Arc::new(index))));
        let rows = detail_rows(&state, &NodeId::File(0));
        for expected in [
            tf!("{} table(s) are named in Manifest.txt only", 1).into_owned(),
            tf!("{} table(s) are named in Export.log only", 1).into_owned(),
            t!("Export messages").into_owned(),
            "EXP-00091: Exporting questionable statistics.".to_string(),
        ] {
            assert!(
                rows.contains(&(expected.clone(), String::new())),
                "{expected}: {rows:?}"
            );
        }
        // SQPlantpid by name: T_AREABRK, T_DRAWING, T_EXTRA, T_MODELITEM,
        // T_NOTE, T_UNLOGGED.
        let extra = detail_rows(&state, &node(&[1, 2]));
        assert!(extra.contains(&(t!("Named in Export.log only").into_owned(), String::new())));
        assert!(extra.contains(&(
            t!("The export log states no row count for this table.").into_owned(),
            String::new()
        )));
        let unlogged = detail_rows(&state, &node(&[1, 5]));
        assert!(unlogged.contains(&(t!("Named in Manifest.txt only").into_owned(), String::new())));
    }

    #[test]
    fn a_dump_with_no_catalog_and_other_files_stay_leaves() {
        let index = BackupIndex {
            files: vec![
                file("Export.dmp", FileKind::ExportDump),
                file("Export.log", FileKind::Other),
            ],
            ..BackupIndex::default()
        };
        let mut state = PidBackupBrowser::default();
        update(&mut state, PidBackupMsg::Loaded(Ok(Arc::new(index))));
        update(&mut state, PidBackupMsg::Toggle(NodeId::Files));
        assert!(visible_rows(&state)
            .iter()
            .filter(|row| matches!(row.id, NodeId::File(_)))
            .all(|row| !row.expandable));
        assert_eq!(
            value(&detail_rows(&state, &NodeId::File(0)), &t!("Table count")),
            None
        );
        // With a catalog, only the dump opens.
        let state = ready(true);
        let expandable: Vec<String> = visible_rows(&state)
            .into_iter()
            .filter(|row| row.expandable && matches!(row.id, NodeId::File(_)))
            .map(|row| row.label)
            .collect();
        assert_eq!(expandable, ["Export.dmp"]);
    }

    /// A backup with the tables the reader has views of, and a dump that is
    /// an Oracle export or (`oracle` false) is not.
    fn ready_with_views(oracle: bool) -> PidBackupBrowser {
        let log = parse_export_log(
            "
. about to export SQPLANTPIDD's tables via Conventional Path ...
. . exporting table                      CODELISTS       3407 rows exported
. about to export SQPLANTD's tables via Conventional Path ...
. . exporting table                      CODELISTS        130 rows exported
. about to export SQPLANTPID's tables via Conventional Path ...
. . exporting table                    T_MODELITEM      47124 rows exported
. . exporting table                      T_PIPERUN       4686 rows exported
"
            .lines(),
        );
        // In the manifest's order: SQPlantd, SQPlantpidd, SQPlantpid.
        let manifest = vec![
            object("SQPlantd", "CODELISTS"),
            object("SQPlantpidd", "CODELISTS"),
            object("SQPlantpid", "T_PIPERUN"),
            object("SQPlantpid", "T_MODELITEM"),
        ];
        let index = BackupIndex {
            files: vec![BackupFile {
                version_line: oracle.then(|| "EXPORT:V12.01.00".to_string()),
                ..file("Export.dmp", FileKind::ExportDump)
            }],
            table_catalog: table_catalog(&manifest, Some(&log)),
            ..BackupIndex::default()
        };
        let mut state = PidBackupBrowser::default();
        update(&mut state, PidBackupMsg::Loaded(Ok(Arc::new(index))));
        state
    }

    /// What the reader would hand over for the pipe runs.
    fn pipe_runs_view() -> SectionData {
        SectionData::new(TypedView {
            notes: vec![tf!("{} of the {} run(s) belong to a pipeline", 1, 2).into_owned()],
            table: Arc::new(Table::new(
                vec![Column::text("SP_ID"), Column::text(t!("Pipeline name"))],
                vec![
                    vec!["RUN-1".to_string(), "25 mm-PG-1001-F9".to_string()],
                    vec!["RUN-2".to_string(), String::new()],
                ],
            )),
        })
    }

    #[test]
    fn a_table_the_reader_knows_is_read_when_selected_and_shown_as_its_view() {
        let mut state = ready_with_views(true);
        let NodeId::Section(slot, _) = node(&[0]) else {
            unreachable!("a section's node");
        };
        // SQPlantpid's tables by name: T_MODELITEM, T_PIPERUN.
        let runs = node(&[2, 1]);
        let fields = [
            (t!("Schema").into_owned(), "SQPlantpid".to_string()),
            (t!("Table name").into_owned(), "T_PIPERUN".to_string()),
            (t!("Row count").into_owned(), "4686".to_string()),
        ];
        // Not selected yet: the table says there is something to read.
        let mut expected = fields.to_vec();
        expected.push((
            t!("The reader has a view of this table; select it to read the dump.").into_owned(),
            String::new(),
        ));
        assert_eq!(detail_rows(&state, &runs), expected);

        // Selecting it asks for the dump, once.
        assert_eq!(
            update(&mut state, PidBackupMsg::Select(runs.clone())),
            Effect::LoadSection {
                generation: state.generation,
                section: slot,
                node: runs.clone(),
            }
        );
        assert_eq!(
            update(&mut state, PidBackupMsg::Select(runs.clone())),
            Effect::None
        );
        let mut expected = fields.to_vec();
        expected.push((t!("Reading the database dump…").into_owned(), String::new()));
        assert_eq!(detail_rows(&state, &runs), expected);

        // The view: what it holds, then its rows.
        let generation = state.generation;
        update(
            &mut state,
            PidBackupMsg::SectionLoaded {
                generation,
                section: slot,
                node: runs.clone(),
                result: Ok(pipe_runs_view()),
            },
        );
        let mut expected = fields.to_vec();
        expected.extend([
            (
                tf!("{} of the {} run(s) belong to a pipeline", 1, 2).into_owned(),
                String::new(),
            ),
            (
                format!("SP_ID | {}", t!("Pipeline name")),
                tf!("{} row(s)", 2).into_owned(),
            ),
            ("RUN-1".to_string(), "25 mm-PG-1001-F9".to_string()),
            ("RUN-2".to_string(), String::new()),
        ]);
        assert_eq!(detail_rows(&state, &runs), expected);

        // A table the reader has no view of reads nothing.
        let items = node(&[2, 0]);
        assert_eq!(
            update(&mut state, PidBackupMsg::Select(items.clone())),
            Effect::None
        );
        assert_eq!(
            detail_rows(&state, &items).last(),
            Some(&(
                t!("The rows of the table are not shown yet.").into_owned(),
                String::new()
            ))
        );
    }

    #[test]
    fn a_table_two_schemas_have_is_the_readers_in_the_one_the_export_wrote_first() {
        let mut state = ready_with_views(true);
        // SQPlantpidd is the second schema of the manifest and the first of
        // the export: its CODELISTS is the one the reader reads.
        let read = node(&[1, 0]);
        let other = node(&[0, 0]);
        assert!(matches!(
            update(&mut state, PidBackupMsg::Select(read)),
            Effect::LoadSection { .. }
        ));
        assert_eq!(
            update(&mut state, PidBackupMsg::Select(other.clone())),
            Effect::None
        );
        assert_eq!(
            detail_rows(&state, &other).last(),
            Some(&(
                t!("The rows of the table are not shown yet.").into_owned(),
                String::new()
            ))
        );
    }

    #[test]
    fn a_view_the_reader_has_none_of_and_a_dump_that_is_not_read_say_why() {
        let mut state = ready_with_views(true);
        let NodeId::Section(slot, _) = node(&[0]) else {
            unreachable!("a section's node");
        };
        let runs = node(&[2, 1]);
        update(&mut state, PidBackupMsg::Select(runs.clone()));
        let why = unread("T_PIPERUN: row 0 NUMBER is not an integer".to_string());
        let generation = state.generation;
        update(
            &mut state,
            PidBackupMsg::SectionLoaded {
                generation,
                section: slot,
                node: runs.clone(),
                result: Err(why.clone()),
            },
        );
        assert_eq!(
            detail_rows(&state, &runs).last(),
            Some(&(why, String::new()))
        );

        // A dump that is no Oracle export: the table says so, and selecting
        // it reads nothing.
        let mut state = ready_with_views(false);
        assert_eq!(
            update(&mut state, PidBackupMsg::Select(runs.clone())),
            Effect::None
        );
        assert_eq!(
            detail_rows(&state, &runs).last(),
            Some(&(
                t!("The dump is not an Oracle export; the panel does not read its database.")
                    .into_owned(),
                String::new()
            ))
        );
    }

    /// The branch as the panel draws it: the dump's arrow is where a press
    /// lands, a schema's row selects it, and its tables are drawn as a grid
    /// whose title sorts them.
    #[test]
    fn the_drawn_dump_unfolds_and_a_schema_shows_its_tables_sorted_by_a_click() {
        let _guard = crate::headless_ui_lock();
        let mut state = ready(true);

        fn draw(state: &PidBackupBrowser) -> iced_test::simulator::Simulator<'_, Message> {
            iced_test::simulator::Simulator::with_size(
                iced::Settings::default(),
                (340.0, 700.0),
                view(state, 340.0, Chrome::default()),
            )
        }
        let press = |state: &PidBackupBrowser, label: &str, left_of: bool| -> Vec<PidBackupMsg> {
            let mut ui = draw(state);
            let bounds = ui
                .find(label)
                .unwrap_or_else(|error| panic!("{label} is drawn: {error:?}"))
                .visible_bounds()
                .unwrap_or_else(|| panic!("{label} is on screen"));
            let x = if left_of {
                bounds.x - 7.0
            } else {
                bounds.center_x()
            };
            ui.point_at(iced::Point::new(x, bounds.center_y()));
            let _ = ui.simulate(iced_test::simulator::click());
            ui.into_messages()
                .filter_map(|message| match message {
                    Message::PidBackup(message) => Some(message),
                    _ => None,
                })
                .collect()
        };
        let apply = |state: &mut PidBackupBrowser, messages: Vec<PidBackupMsg>| {
            for message in messages {
                update(state, message);
            }
        };

        // The arrow the section gave the dump.
        let pressed = press(&state, "Export.dmp", true);
        assert!(
            pressed
                .iter()
                .any(|message| matches!(message, PidBackupMsg::Toggle(id) if *id == DUMP)),
            "{pressed:?}"
        );
        apply(&mut state, pressed);
        // A schema's row selects it.
        let clicked = press(&state, "SQPlantpid (4)", false);
        assert!(
            clicked
                .iter()
                .any(|message| matches!(message, PidBackupMsg::Select(id) if *id == node(&[1]))),
            "{clicked:?}"
        );
        apply(&mut state, clicked);

        // Its tables are a grid under its fields, in name order.
        let top = |state: &PidBackupBrowser, label: &str| -> f32 {
            draw(state)
                .find(label)
                .unwrap_or_else(|error| panic!("{label} is drawn: {error:?}"))
                .visible_bounds()
                .unwrap_or_else(|| panic!("{label} is on screen"))
                .y
        };
        assert!(top(&state, "T_AREABRK") < top(&state, "T_MODELITEM"));
        assert!(top(&state, "T_MODELITEM") < top(&state, "T_NOTE"));
        // A click on the rows' title sorts by rows; a second one descending,
        // which puts the largest table first.
        // "Row count" is drawn twice, as a field and as the column's title;
        // the title is found from its neighbour's, which is drawn once.
        let sort = |state: &PidBackupBrowser| -> Vec<PidBackupMsg> {
            let mut ui = draw(state);
            let name = ui
                .find(t!("Table name").as_ref())
                .expect("the name column's title is drawn")
                .visible_bounds()
                .expect("the title is on screen");
            // The rows' title sits right of the name's, on the same line.
            ui.point_at(iced::Point::new(
                name.x + name.width + 40.0,
                name.center_y(),
            ));
            let _ = ui.simulate(iced_test::simulator::click());
            ui.into_messages()
                .filter_map(|message| match message {
                    Message::PidBackup(message) => Some(message),
                    _ => None,
                })
                .collect()
        };
        let sorted = sort(&state);
        assert!(
            sorted
                .iter()
                .any(|message| matches!(message, PidBackupMsg::Grid(_))),
            "{sorted:?}"
        );
        apply(&mut state, sorted);
        let again = sort(&state);
        apply(&mut state, again);
        assert!(top(&state, "T_MODELITEM") < top(&state, "T_NOTE"));
        assert!(top(&state, "T_NOTE") < top(&state, "T_DRAWING"));
        assert!(top(&state, "T_DRAWING") < top(&state, "T_AREABRK"));
    }
}
