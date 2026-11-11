//! A walk over a real SmartPlant backup through the application, as a user
//! working the panel would do it: the load PIDBACKUPOPEN starts, every node
//! of the tree with its details, every drawing's items, the search box, and
//! one drawing opened in a tab.
//!
//! The synthetic tests beside the panel prove its rules; this one says what
//! a real plant backup looks like in it, and is the place to see a class of
//! data the panel does not show yet. Run by hand:
//!
//! ```text
//! cargo test --lib app::commands::pidbackup::audit -- --ignored --nocapture
//! ```
//!
//! - `OCS_PID_BACKUP_AUDIT` names the backup, a folder or a zip (default: the
//!   plant test backup beside the checkout);
//! - `OCS_PID_BACKUP_AUDIT_OUT` names a folder that receives `report.txt` and
//!   a PNG of the panel in each state the walk passes through;
//! - `OCS_PID_BACKUP_AUDIT_LANG=zh-CN` draws the panel in Chinese. The UI
//!   language is one process-wide loader, so only set it when this test runs
//!   alone;
//! - `OCS_PID_BACKUP_AUDIT_SKIP_IMPORT` (any value) leaves out the import of
//!   every drawing, the slow half that a change to the panel does not touch.
//!
//! A folder source is restored into the cache under the temp folder, which
//! is where the 100 MB of a plant's drawings go: point `TEMP` at a drive
//! with room for them.

use super::*;
use crate::io::pid_backup_browse::{BackupIndex, GroupNode, RefNode};
use crate::ui::window::pid_backup_browser::{
    detail_buttons, detail_rows, visible_rows, ItemsState, NodeId, Row, SectionLoad,
};
use std::cell::RefCell;
use std::collections::{BTreeMap, HashSet};
use std::time::{Duration, Instant};

thread_local! {
    /// Every key and value the walk has seen in a node's details, and how
    /// many nodes that was: what a credential must not turn up in.
    static SHOWN: RefCell<(String, usize)> = RefCell::new((String::new(), 0));
}

/// The plant test backup the two reading tests of `io::pid_backup*` use.
const DEFAULT_BACKUP: &str = r"D:\work\plant-code\cad\pid-test-data";

/// How many nodes of one branch the report shows in full.
const SECTION_NODES_SHOWN: usize = 12;

/// The docked panel's default width, and a height a 1080p window leaves it.
const PANEL_W: f32 = 340.0;
const PANEL_H: f32 = 900.0;

/// The report lines and the numbered panel pictures of one walk.
struct Audit {
    out: Option<PathBuf>,
    report: Vec<String>,
    shots: usize,
}

impl Audit {
    fn line(&mut self, text: impl Into<String>) {
        let text = text.into();
        eprintln!("{text}");
        self.report.push(text);
    }

    /// Draw the panel as it stands and keep the picture, when asked to.
    fn shot(&mut self, app: &OpenCADStudio, name: &str) {
        let Some(folder) = self.out.clone() else {
            return;
        };
        let _guard = crate::headless_ui_lock();
        // The application draws its text in the desktop's interface font; so
        // does the picture, or its line breaks would not be the window's.
        crate::sys::ui_font::apply(crate::sys::ui_font::detect().as_ref());
        let mut ui = iced_test::simulator::Simulator::with_size(
            iced::Settings {
                fonts: vec![std::borrow::Cow::Borrowed(iced_aw::ICED_AW_FONT_BYTES)],
                ..iced::Settings::default()
            },
            (PANEL_W, PANEL_H),
            pid_backup_browser::view(
                &app.pid_backup_browser,
                PANEL_W,
                crate::ui::dock::Chrome::default(),
            ),
        );
        let theme = crate::ui::style::fusion_theme::fusion_black();
        let snapshot = ui.snapshot(&theme).expect("the panel draws headless");
        self.shots += 1;
        let stem = format!("{:02}-{name}", self.shots);
        // `matches_image` writes the picture only when there is none to
        // compare with, under `<stem>-<renderer>.png`: clear the last walk's.
        let pictures = |folder: &Path| -> Vec<PathBuf> {
            std::fs::read_dir(folder)
                .map(|entries| {
                    entries
                        .flatten()
                        .map(|entry| entry.path())
                        .filter(|path| {
                            path.file_name().is_some_and(|file| {
                                let file = file.to_string_lossy();
                                file.starts_with(&format!("{stem}-")) && file.ends_with(".png")
                            })
                        })
                        .collect()
                })
                .unwrap_or_default()
        };
        for old in pictures(&folder) {
            let _ = std::fs::remove_file(old);
        }
        snapshot
            .matches_image(folder.join(format!("{stem}.png")))
            .expect("the picture is written");
        for written in pictures(&folder) {
            self.line(format!("  [picture] {}", written.display()));
        }
    }
}

/// Send one panel message through the application and run what it starts.
fn send(app: &mut OpenCADStudio, msg: PidBackupMsg) -> Result<(), String> {
    let task = app.on_pid_backup(msg);
    app.drive_headless_task(task)
}

/// Every group of the plant-group tree, as the child-index path the panel
/// names it by, parents before children.
fn group_paths(groups: &[GroupNode]) -> Vec<Vec<usize>> {
    fn walk(node: &GroupNode, path: Vec<usize>, out: &mut Vec<Vec<usize>>) {
        out.push(path.clone());
        for (at, child) in node.children.iter().enumerate() {
            let mut next = path.clone();
            next.push(at);
            walk(child, next, out);
        }
    }
    let mut out = Vec::new();
    for (at, node) in groups.iter().enumerate() {
        walk(node, vec![at], &mut out);
    }
    out
}

/// Every directory of the `Ref` tree below its root, parents before children.
fn ref_dir_paths(root: &RefNode) -> Vec<Vec<usize>> {
    fn walk(node: &RefNode, path: Vec<usize>, out: &mut Vec<Vec<usize>>) {
        for (at, dir) in node.dirs.iter().enumerate() {
            let mut next = path.clone();
            next.push(at);
            out.push(next.clone());
            walk(dir, next, out);
        }
    }
    let mut out = Vec::new();
    walk(root, Vec::new(), &mut out);
    out
}

fn ref_dir<'a>(root: &'a RefNode, path: &[usize]) -> &'a RefNode {
    path.iter().fold(root, |node, at| &node.dirs[*at])
}

/// The node's details, which every node the tree can show must have.
fn details_of(
    app: &OpenCADStudio,
    id: &NodeId,
    empty: &mut Vec<String>,
) -> Vec<(String, String)> {
    let rows = detail_rows(&app.pid_backup_browser, id);
    if rows.is_empty() {
        empty.push(format!("{id:?}"));
    }
    SHOWN.with(|shown| {
        let mut shown = shown.borrow_mut();
        shown.1 += 1;
        for (key, value) in &rows {
            shown.0.push_str(key);
            shown.0.push('\n');
            shown.0.push_str(value);
            shown.0.push('\n');
        }
    });
    rows
}

/// Open everything the sections hang under `host`, level by level, as a user
/// working down the branch would: select each node (which starts whatever
/// read it needs), report its details, unfold it. Returns how many nodes
/// that was. `host` is folded again afterwards.
fn walk_sections(
    app: &mut OpenCADStudio,
    audit: &mut Audit,
    host: &NodeId,
    empty: &mut Vec<String>,
) -> usize {
    send(app, PidBackupMsg::Toggle(host.clone())).unwrap();
    let mut seen: HashSet<NodeId> = HashSet::new();
    // What the walk unfolded, to fold again when it is done.
    let mut opened: Vec<NodeId> = Vec::new();
    loop {
        let rows = visible_rows(&app.pid_backup_browser);
        let Some(at) = rows.iter().position(|row| row.id == *host) else {
            break;
        };
        let depth = rows[at].depth;
        let next: Option<Row> = rows[at + 1..]
            .iter()
            .take_while(|row| row.depth > depth)
            .find(|row| matches!(row.id, NodeId::Section(..)) && !seen.contains(&row.id))
            .cloned();
        let Some(row) = next else {
            break;
        };
        seen.insert(row.id.clone());
        send(app, PidBackupMsg::Select(row.id.clone())).unwrap();
        let details = details_of(app, &row.id, empty);
        let indent = "  ".repeat(row.depth - depth);
        let cut = |text: &str| -> String {
            if text.chars().count() > 96 {
                format!("{}…", text.chars().take(96).collect::<String>())
            } else {
                text.to_string()
            }
        };
        // Every node is selected and must have details; the report shows
        // the first ones of a branch in full and counts the rest.
        if seen.len() <= SECTION_NODES_SHOWN {
            audit.line(format!("{indent}{} [{} detail line(s)]", row.label, details.len()));
            for (key, value) in details.iter().take(6) {
                audit.line(format!("{indent}    {} = {}", cut(key), cut(value)));
            }
        }
        if row.expandable && !row.expanded {
            send(app, PidBackupMsg::Toggle(row.id.clone())).unwrap();
            opened.push(row.id.clone());
        }
    }
    for id in opened.into_iter().rev() {
        send(app, PidBackupMsg::Toggle(id)).unwrap();
    }
    if seen.len() > SECTION_NODES_SHOWN {
        audit.line(format!(
            "    ... and {} more node(s), each with details",
            seen.len() - SECTION_NODES_SHOWN
        ));
    }
    send(app, PidBackupMsg::Toggle(host.clone())).unwrap();
    seen.len()
}

/// The rows the tree shows directly under `host`, which must be open.
fn rows_under(app: &OpenCADStudio, host: &NodeId) -> Vec<Row> {
    let rows = visible_rows(&app.pid_backup_browser);
    let Some(at) = rows.iter().position(|row| row.id == *host) else {
        return Vec::new();
    };
    let depth = rows[at].depth;
    rows[at + 1..]
        .iter()
        .take_while(|row| row.depth > depth)
        .filter(|row| row.depth == depth + 1)
        .cloned()
        .collect()
}

/// The database branch under the dump: what the catalog holds, that the
/// panel shows the same, and pictures of the dump, a schema and a table.
fn walk_database(app: &mut OpenCADStudio, audit: &mut Audit, index: &BackupIndex) {
    let Some(catalog) = &index.table_catalog else {
        audit.line("== database tables: the backup names none ==");
        return;
    };
    audit.line(format!(
        "== database tables: {} schema(s), {} table(s), {} ==",
        catalog.schemas.len(),
        catalog.table_count(),
        match catalog.rows() {
            Some(rows) => format!("{rows} row(s) by Export.log"),
            None => "no row counts (no Export.log)".to_string(),
        }
    ));
    for schema in &catalog.schemas {
        audit.line(format!(
            "  {}: {} table(s), {} with rows, {}",
            schema.name,
            schema.tables.len(),
            schema.filled(),
            schema
                .rows()
                .map_or_else(|| "no row counts".to_string(), |rows| format!("{rows} row(s)")),
        ));
    }
    let (manifest_only, log_only) = catalog.one_sided();
    audit.line(format!(
        "  named in Manifest.txt only: {manifest_only}; in Export.log only: {log_only}; export: {}; {} message(s)",
        catalog.outcome.as_deref().unwrap_or("-"),
        catalog.messages.len()
    ));
    let Some(dump) = index
        .files
        .iter()
        .position(|file| file.kind == crate::io::pid_backup_browse::FileKind::ExportDump)
        .map(NodeId::File)
    else {
        audit.line("  the backup has no Export.dmp to hang the tables under");
        return;
    };
    // The panel says what the catalog holds.
    let details = detail_rows(&app.pid_backup_browser, &dump);
    let shown = |key: std::borrow::Cow<'static, str>| {
        details
            .iter()
            .find(|(found, _)| *found == key)
            .map(|(_, value)| value.clone())
    };
    assert_eq!(
        shown(crate::t!("Table count")),
        Some(catalog.table_count().to_string()),
        "the dump's details total the tables"
    );
    assert_eq!(
        shown(crate::t!("Row count")),
        catalog.rows().map(|rows| rows.to_string()),
        "the dump's details total the rows"
    );
    send(app, PidBackupMsg::Toggle(NodeId::Files)).unwrap();
    send(app, PidBackupMsg::Toggle(dump.clone())).unwrap();
    let schemas = rows_under(app, &dump);
    assert_eq!(schemas.len(), catalog.schemas.len(), "a row per schema");
    for (row, schema) in schemas.iter().zip(&catalog.schemas) {
        // Fields, the table's titles, then a line per table.
        let details = detail_rows(&app.pid_backup_browser, &row.id);
        let fields = if catalog.has_log { 4 } else { 2 };
        assert_eq!(
            details.len(),
            fields + 1 + schema.tables.len(),
            "{} lists every table",
            schema.name
        );
    }
    send(app, PidBackupMsg::Select(dump.clone())).unwrap();
    audit.shot(app, "database-schemas");
    // The schema with the most tables, and its largest table.
    let Some((at, schema)) = catalog
        .schemas
        .iter()
        .enumerate()
        .max_by_key(|(_, schema)| schema.tables.len())
    else {
        return;
    };
    let schema_row = schemas[at].clone();
    send(app, PidBackupMsg::Select(schema_row.id.clone())).unwrap();
    audit.shot(app, "database-schema-tables");
    send(app, PidBackupMsg::Toggle(schema_row.id.clone())).unwrap();
    let tables = rows_under(app, &schema_row.id);
    assert_eq!(tables.len(), schema.tables.len(), "a row per table");
    let largest = schema
        .tables
        .iter()
        .enumerate()
        .max_by_key(|(_, table)| table.rows.unwrap_or(0))
        .map_or(0, |(at, _)| at);
    send(app, PidBackupMsg::Select(tables[largest].id.clone())).unwrap();
    audit.line(format!("  largest table of {}: {}", schema.name, tables[largest].label));
    audit.shot(app, "database-table");
    send(app, PidBackupMsg::Toggle(schema_row.id)).unwrap();
    send(app, PidBackupMsg::Toggle(dump)).unwrap();
    send(app, PidBackupMsg::Toggle(NodeId::Files)).unwrap();
    send(app, PidBackupMsg::Select(NodeId::Root)).unwrap();
}

/// The tables the reader has a view of (`pid_backup_browser::database`).
const TYPED_TABLES: [&str; 6] = [
    "T_PIPERUN",
    "T_PIPINGPOINT",
    "T_SIGNALPOINT",
    "T_NOZZLE",
    "T_ITEMNOTE",
    "CODELISTS",
];

/// Where what a section reads about `node` is.
fn section_load(app: &OpenCADStudio, node: &NodeId) -> Option<SectionLoad> {
    app.pid_backup_browser
        .section_data
        .iter()
        .find(|((_, found), _)| found == node)
        .map(|(_, load)| load.clone())
}

/// The views the reader has of the dump's tables. Each typed table is
/// selected, which reads the dump the first time: it then shows its rows,
/// or says why the reader has none. A dump the panel does not read says so
/// on every one of them, and nothing is read.
fn walk_dump_views(
    app: &mut OpenCADStudio,
    audit: &mut Audit,
    index: &BackupIndex,
    empty: &mut Vec<String>,
) {
    let Some(catalog) = &index.table_catalog else {
        return;
    };
    let Some(dump) = index
        .files
        .iter()
        .position(|file| file.kind == crate::io::pid_backup_browse::FileKind::ExportDump)
        .map(NodeId::File)
    else {
        return;
    };
    let unreadable = index.dump_unreadable();
    audit.line(format!(
        "== views of the database: the {} tables the reader knows{} ==",
        TYPED_TABLES.len(),
        unreadable
            .as_ref()
            .map_or(String::new(), |why| format!("; not read: {why}"))
    ));
    send(app, PidBackupMsg::Toggle(NodeId::Files)).unwrap();
    send(app, PidBackupMsg::Toggle(dump.clone())).unwrap();
    let schemas = rows_under(app, &dump);
    let mut read = 0;
    let mut refused = 0;
    for name in TYPED_TABLES {
        let Some(at) = catalog.first_schema_with(name) else {
            audit.line(format!("  {name}: the backup names no such table"));
            continue;
        };
        let schema_row = schemas[at].clone();
        send(app, PidBackupMsg::Toggle(schema_row.id.clone())).unwrap();
        let tables = rows_under(app, &schema_row.id);
        let table_at = catalog.schemas[at]
            .tables
            .iter()
            .position(|table| table.name.eq_ignore_ascii_case(name))
            .expect("the schema has the table");
        let row = tables[table_at].clone();
        let started = Instant::now();
        send(app, PidBackupMsg::Select(row.id.clone())).unwrap();
        let took = started.elapsed();
        let details = details_of(app, &row.id, empty);
        // The lines after the catalog's own: schema, name, and the count
        // when the log has one.
        let own = details
            .iter()
            .position(|(key, _)| *key == crate::t!("Row count"))
            .or_else(|| {
                details
                    .iter()
                    .position(|(key, _)| *key == crate::t!("Table name"))
            })
            .map_or(0, |at| at + 1);
        let shown = |text: &str| details.iter().any(|(key, _)| key == text);
        match (section_load(app, &row.id), &unreadable) {
            (None, Some(why)) => {
                assert!(shown(why), "{name} does not say why the dump is not read");
                audit.line(format!("  {}.{name}: {why}", catalog.schemas[at].name));
            }
            (Some(SectionLoad::Ready(_)), None) => {
                read += 1;
                // The view is a table: its titles, with how many rows.
                let table = details
                    .iter()
                    .skip(own)
                    .find(|(titles, _)| titles.contains(" | "))
                    .unwrap_or_else(|| panic!("{name} shows no table: {details:?}"));
                audit.line(format!(
                    "  {}.{name}: [{}] {} (selected in {})",
                    catalog.schemas[at].name,
                    table.0,
                    table.1,
                    millis(took)
                ));
                for (note, _) in details
                    .iter()
                    .skip(own)
                    .take_while(|(titles, _)| !titles.contains(" | "))
                {
                    audit.line(format!("      {note}"));
                }
                if matches!(name, "T_PIPERUN" | "CODELISTS") {
                    audit.shot(app, &format!("dump-view-{}", name.to_lowercase()));
                }
            }
            (Some(SectionLoad::Failed(why)), None) => {
                refused += 1;
                assert!(shown(&why), "{name} failed without saying so: {why}");
                audit.line(format!("  {}.{name}: {why}", catalog.schemas[at].name));
            }
            (state, _) => panic!("{name} was left in {state:?}"),
        }
        send(app, PidBackupMsg::Toggle(schema_row.id)).unwrap();
    }
    if unreadable.is_none() {
        let dump_read = index.dump_if_read().expect("selecting a view reads the dump");
        audit.line(format!(
            "  the dump ({} bytes) was read in {}; {read} view(s) shown, {refused} the reader has none of{}",
            dump_read.bytes,
            millis(dump_read.read_in),
            dump_read
                .partial()
                .map_or(String::new(), |why| format!(" (not read whole: {why})"))
        ));
        // A dump the reader followed to the end has every view.
        if dump_read.partial().is_none() {
            assert_eq!(refused, 0, "a view is missing of a dump that was read whole");
        }
        // The dump's own details say how long the read took.
        let details = detail_rows(&app.pid_backup_browser, &dump);
        assert!(
            details.iter().any(|(key, _)| *key == crate::t!("Dump read in")),
            "the dump's details do not say it was read"
        );
    }
    send(app, PidBackupMsg::Toggle(dump)).unwrap();
    send(app, PidBackupMsg::Toggle(NodeId::Files)).unwrap();
    send(app, PidBackupMsg::Select(NodeId::Root)).unwrap();
}

/// The data maps of the reference data: each file is unfolded, which reads
/// it, and the row under it selected. It shows the enumerations the file
/// maps, or says why it could not be read.
fn walk_data_maps(
    app: &mut OpenCADStudio,
    audit: &mut Audit,
    index: &BackupIndex,
    empty: &mut Vec<String>,
) {
    // Every data map of the tree: the directories down to it, and its place.
    fn collect(node: &RefNode, dirs: &mut Vec<usize>, found: &mut Vec<(Vec<usize>, usize, String)>) {
        for (at, file) in node.files.iter().enumerate() {
            if crate::io::pid_backup_browse::datamap::is_data_map(&file.name) {
                found.push((dirs.clone(), at, file.name.clone()));
            }
        }
        for (at, dir) in node.dirs.iter().enumerate() {
            dirs.push(at);
            collect(dir, dirs, found);
            dirs.pop();
        }
    }
    let mut maps = Vec::new();
    collect(&index.ref_tree, &mut Vec::new(), &mut maps);
    audit.line(format!(
        "== data maps: {} file(s) of the reference data ==",
        maps.len()
    ));
    if maps.is_empty() {
        return;
    }
    // Unfold a node unless it is open already; what was unfolded here is
    // folded again on the way out.
    fn open(app: &mut OpenCADStudio, id: NodeId, opened: &mut Vec<NodeId>) {
        if !app.pid_backup_browser.expanded.contains(&id) {
            send(app, PidBackupMsg::Toggle(id.clone())).unwrap();
            opened.push(id);
        }
    }
    let mut opened: Vec<NodeId> = Vec::new();
    open(app, NodeId::Ref, &mut opened);
    let mut pictured = false;
    for (dirs, file, name) in &maps {
        for depth in 1..=dirs.len() {
            open(app, NodeId::RefDir(dirs[..depth].to_vec()), &mut opened);
        }
        let host = NodeId::RefFile(dirs.clone(), *file);
        let started = Instant::now();
        open(app, host.clone(), &mut opened);
        let took = started.elapsed();
        let rows = rows_under(app, &host);
        assert_eq!(rows.len(), 1, "the rows under {name}: {rows:?}");
        let row = rows[0].clone();
        send(app, PidBackupMsg::Select(row.id.clone())).unwrap();
        let details = details_of(app, &row.id, empty);
        match section_load(app, &row.id) {
            Some(SectionLoad::Ready(_)) => {
                let lists = details
                    .iter()
                    .find(|(key, _)| *key == crate::t!("Enumeration lists"))
                    .map_or("?", |(_, lists)| lists.as_str());
                audit.line(format!(
                    "  {name}: {} of {lists} list(s), read in {}",
                    row.label,
                    millis(took)
                ));
                // The row's count is the table's.
                if let Some((_, count)) = details.iter().find(|(titles, _)| titles.contains(" | ")) {
                    assert!(
                        row.label.ends_with(&format!(
                            "({})",
                            count.split_whitespace().next().unwrap_or_default()
                        )),
                        "{} shows {count}",
                        row.label
                    );
                }
                if !pictured {
                    for (key, value) in details.iter().skip(3).take(4) {
                        audit.line(format!("      {key} = {value}"));
                    }
                    audit.shot(app, "data-map-enumerations");
                    pictured = true;
                }
            }
            Some(SectionLoad::Failed(why)) => {
                assert!(details.iter().any(|(key, _)| *key == why), "{details:?}");
                audit.line(format!("  {name}: {why}"));
            }
            other => panic!("the map of {name} was left in {other:?}"),
        }
    }
    for id in opened.into_iter().rev() {
        send(app, PidBackupMsg::Toggle(id)).unwrap();
    }
    send(app, PidBackupMsg::Select(NodeId::Root)).unwrap();
}

/// The values of `Manifest.txt` that are credentials: every obfuscated
/// field, the fields that hold a password by their position, and the
/// password of a login in the export command. Read from the file itself, so
/// the check does not depend on the code it checks.
fn manifest_credentials(backup_root: &Path) -> Vec<(String, String)> {
    let Ok(bytes) = std::fs::read(backup_root.join("Manifest.txt")) else {
        return Vec::new();
    };
    let manifest = pid_parse::backup::parse_manifest_bytes(&bytes);
    let mut found: Vec<(String, String)> = Vec::new();
    let mut add = |key: &str, value: &str| {
        // A value a few characters long is not looked for: it would turn up
        // everywhere by accident, and no password of the samples is.
        if value.chars().count() >= 6 && !found.iter().any(|(_, known)| known == value) {
            found.push((key.to_string(), value.to_string()));
        }
    };
    for line in &manifest.lines {
        for (at, field) in line.fields.iter().enumerate() {
            let by_position = match line.key.as_str() {
                "DBPwds" => true,
                "SiteConnInfo" => at == 0 || at == 4,
                "PlantConnInfo" => at == 4,
                "TableSpace" => at == 1,
                _ => false,
            };
            if by_position || field.contains("###") {
                add(&line.key, field);
            }
            for word in field.split(' ') {
                if let (Some(slash), Some(sign)) = (word.find('/'), word.rfind('@')) {
                    if slash > 0 && sign > slash + 1 && sign + 1 < word.len() {
                        add(&line.key, &word[slash + 1..sign]);
                    }
                }
            }
        }
    }
    found
}

/// What selecting a file node showed of the file's content.
enum Content {
    /// Its lines: the encoding, how many, and what the panel remarks.
    Text {
        encoding: String,
        lines: String,
        notes: Vec<String>,
    },
    /// The panel says it is not a text file.
    NotText,
    /// Nothing was read: the node has no text of its own.
    NotRead,
    /// The read failed, and the details say why.
    Failed(String),
}

/// Select `id` as a click on its row does, wait for the read that starts,
/// and say what the details show of the file's content.
fn read_file_node(app: &mut OpenCADStudio, id: &NodeId, empty: &mut Vec<String>) -> Content {
    send(app, PidBackupMsg::Select(id.clone())).unwrap();
    let rows = details_of(app, id, empty);
    let value = |key: &str| {
        rows.iter()
            .find(|(found, _)| found == key)
            .map(|(_, value)| value.clone())
    };
    if let Some(encoding) = value(&crate::t!("Text encoding")) {
        // The remarks stand between the line count and the lines.
        let from = rows
            .iter()
            .position(|(key, _)| *key == crate::t!("Line count"))
            .map_or(rows.len(), |at| at + 1);
        let notes = rows[from..]
            .iter()
            .take_while(|(_, value)| value.is_empty())
            .map(|(key, _)| key.clone())
            .collect();
        return Content::Text {
            encoding,
            lines: value(&crate::t!("Line count")).unwrap_or_default(),
            notes,
        };
    }
    if value(&crate::t!("Not a text file; its content is not shown.")).is_some() {
        return Content::NotText;
    }
    let read = app
        .pid_backup_browser
        .section_data
        .iter()
        .find(|((_, node), _)| node == id)
        .map(|(_, load)| load.clone());
    match read {
        None => Content::NotRead,
        Some(SectionLoad::Failed(error)) => {
            assert!(
                rows.iter().any(|(key, _)| *key == error),
                "a read that failed says so in the details: {error}"
            );
            Content::Failed(error)
        }
        Some(other) => panic!("the read of {id:?} is left in {other:?}"),
    }
}

/// How many files of one kind showed what, and the report lines of the ones
/// that are text.
#[derive(Default)]
struct ContentTally {
    text: Vec<String>,
    not_text: usize,
    not_read: usize,
    failed: Vec<String>,
}

impl ContentTally {
    fn add(&mut self, label: &str, content: Content) {
        match content {
            Content::Text {
                encoding,
                lines,
                notes,
            } => {
                let mut line = format!("{label} | {encoding} | {lines} line(s)");
                for note in notes {
                    line.push_str(" | ");
                    line.push_str(&note);
                }
                self.text.push(line);
            }
            Content::NotText => self.not_text += 1,
            Content::NotRead => self.not_read += 1,
            Content::Failed(error) => self.failed.push(format!("{label}: {error}")),
        }
    }

    fn report(&self, audit: &mut Audit, what: &str, not_read: &str) {
        audit.line(format!(
            "  {what}: {} text, {} not text, {} {not_read}, {} could not be read",
            self.text.len(),
            self.not_text,
            self.not_read,
            self.failed.len()
        ));
        for line in &self.text {
            audit.line(format!("    {line}"));
        }
        for line in &self.failed {
            audit.line(format!("    could not be read: {line}"));
        }
    }
}

/// The content of every file the tree names: each backup file, each member
/// of the archives, each reference file, selected in turn as a click on its
/// row would. What is text is read and shown; the report says which.
fn walk_text(
    app: &mut OpenCADStudio,
    audit: &mut Audit,
    index: &BackupIndex,
    empty: &mut Vec<String>,
) {
    let started = Instant::now();
    audit.line("== text of the files (read when a file is selected) ==");
    send(app, PidBackupMsg::Toggle(NodeId::Files)).unwrap();
    let labels: Vec<String> = {
        let rows = visible_rows(&app.pid_backup_browser);
        (0..index.files.len())
            .map(|at| {
                rows.iter()
                    .find(|row| row.id == NodeId::File(at))
                    .map_or_else(|| index.files[at].name.clone(), |row| row.label.clone())
            })
            .collect()
    };
    let mut files = ContentTally::default();
    let mut members = ContentTally::default();
    // The first of each kind, for the pictures.
    let mut text_file = None;
    let mut text_member = None;
    let mut binary_file = None;
    let mut manifest_left_alone = false;
    for (at, file) in index.files.iter().enumerate() {
        let id = NodeId::File(at);
        let content = read_file_node(app, &id, empty);
        // The manifest holds the plant's credentials in the clear. Its
        // content is the branch under its row, where each is hidden; the
        // file itself is never read into the details as text.
        if file.kind == crate::io::pid_backup_browse::FileKind::Manifest {
            assert!(
                matches!(content, Content::NotRead),
                "selecting {} read it as text",
                file.name
            );
            manifest_left_alone = true;
        }
        match &content {
            Content::Text { .. } if text_file.is_none() && file.record.is_some() => {
                text_file = Some(id.clone());
            }
            Content::NotText if binary_file.is_none() => binary_file = Some(id.clone()),
            _ => {}
        }
        files.add(&labels[at], content);
        for (member, name) in file.members.iter().enumerate() {
            let id = NodeId::FileMember(at, member);
            let content = read_file_node(app, &id, empty);
            if matches!(content, Content::Text { .. }) && text_member.is_none() {
                text_member = Some((at, id.clone()));
            }
            members.add(&format!("{} > {name}", file.name), content);
        }
    }
    files.report(
        audit,
        "backup files",
        "with no text of their own (the manifest, the dump, archives, missing files)",
    );
    members.report(audit, "archive members", "folders");
    if manifest_left_alone {
        audit.line("  Manifest.txt: selected like the others, not read as text");
    }

    let mut reference = ContentTally::default();
    let mut text_reference = None;
    let mut dirs = vec![Vec::new()];
    dirs.extend(ref_dir_paths(&index.ref_tree));
    for path in &dirs {
        let node = ref_dir(&index.ref_tree, path);
        let folder: Vec<&str> = path
            .iter()
            .scan(&index.ref_tree, |node, at| {
                let next = &node.dirs[*at];
                *node = next;
                Some(next.name.as_str())
            })
            .collect();
        for (at, file) in node.files.iter().enumerate() {
            let id = NodeId::RefFile(path.clone(), at);
            let content = read_file_node(app, &id, empty);
            if matches!(content, Content::Text { .. }) && text_reference.is_none() {
                text_reference = Some((path.clone(), id.clone()));
            }
            let mut label = folder.join("\\");
            if !label.is_empty() {
                label.push('\\');
            }
            label.push_str(&file.name);
            reference.add(&label, content);
        }
    }
    reference.report(audit, "reference files", "not read");
    audit.line(format!(
        "  all selected and read in {}",
        millis(started.elapsed())
    ));

    // The pictures: a text file, the export log, a member of an archive, a
    // file that is not text, a reference file.
    if let Some(id) = text_file {
        send(app, PidBackupMsg::Select(id)).unwrap();
        audit.shot(app, "text-file");
    }
    if let Some(at) = index.files.iter().position(|file| file.name == "Export.log") {
        send(app, PidBackupMsg::Select(NodeId::File(at))).unwrap();
        audit.shot(app, "text-export-log");
    }
    if let Some(id) = binary_file {
        send(app, PidBackupMsg::Select(id)).unwrap();
        audit.shot(app, "text-not-a-text-file");
    }
    if let Some((at, id)) = text_member {
        send(app, PidBackupMsg::Toggle(NodeId::File(at))).unwrap();
        send(app, PidBackupMsg::Select(id)).unwrap();
        audit.shot(app, "text-archive-member");
        send(app, PidBackupMsg::Toggle(NodeId::File(at))).unwrap();
    }
    send(app, PidBackupMsg::Toggle(NodeId::Files)).unwrap();
    if let Some((path, id)) = text_reference {
        send(app, PidBackupMsg::Toggle(NodeId::Ref)).unwrap();
        for depth in 1..=path.len() {
            send(app, PidBackupMsg::Toggle(NodeId::RefDir(path[..depth].to_vec()))).unwrap();
        }
        send(app, PidBackupMsg::Select(id)).unwrap();
        audit.shot(app, "text-reference-file");
        for depth in (1..=path.len()).rev() {
            send(app, PidBackupMsg::Toggle(NodeId::RefDir(path[..depth].to_vec()))).unwrap();
        }
        send(app, PidBackupMsg::Toggle(NodeId::Ref)).unwrap();
    }
    send(app, PidBackupMsg::Select(NodeId::Root)).unwrap();
}

/// What the files of the backup's drawings hold beside their items, added
/// up over the drawings.
#[derive(Default)]
struct DataTally {
    /// Drawings the tree hangs the rows under, and how many rows that is.
    drawings: usize,
    nodes: usize,
    process_points: usize,
    /// Process points on a connector that is one of the drawing's items.
    named_connectors: usize,
    branch_points: usize,
    piping_branch_points: usize,
    representations: usize,
    /// Representations of an item of the drawing, of a branch point drawn
    /// on a run, and of an item the drawing does not publish.
    of_items: usize,
    on_runs: usize,
    of_unpublished: usize,
    stats: crate::io::pid_backup_browse::drawing_data::ReadStats,
    /// The first drawing that shows the rows: the one the pictures are of.
    first: Option<usize>,
}

/// The rows the tree hangs under the open drawing `at` for what its file
/// holds beside the items: there when the read gave any of it, each with
/// details, and a list's table as long as the read says the list is.
fn walk_drawing_data(
    app: &OpenCADStudio,
    at: usize,
    items: &crate::io::pid_backup_browse::DrawingItems,
    tally: &mut DataTally,
    empty: &mut Vec<String>,
) {
    let data = &items.data;
    let rows: Vec<Row> = rows_under(app, &NodeId::Drawing(at))
        .into_iter()
        .filter(|row| matches!(row.id, NodeId::Section(..)))
        .collect();
    if data.is_empty() {
        assert!(
            rows.is_empty(),
            "drawing {at} gave nothing beside its items and shows {rows:?}"
        );
        return;
    }
    let lists = [
        (crate::t!("Process points"), Some(data.process_points.len())),
        (crate::t!("Branch points"), Some(data.branch_points.len())),
        (crate::t!("Representations"), Some(data.representations.len())),
        (crate::t!("Ports, connections, OPC and pipelines"), None),
        (crate::t!("Read statistics"), None),
    ];
    assert_eq!(rows.len(), lists.len(), "the rows under drawing {at}: {rows:?}");
    for (row, (title, count)) in rows.iter().zip(&lists) {
        let details = details_of(app, &row.id, empty);
        let Some(count) = count else {
            assert_eq!(row.label, *title);
            continue;
        };
        assert_eq!(row.label, format!("{title} ({count})"));
        // The drawing's name, then the list: a table with a line for its
        // titles and one for each row, or the line that says there is none.
        if *count == 0 {
            assert_eq!(
                details.last().map(|(key, _)| key.as_str()),
                Some(crate::t!("The drawing has none.").as_ref()),
                "{} of drawing {at}",
                row.label
            );
        } else {
            assert_eq!(
                details.get(1).map(|(_, rows)| rows.clone()),
                Some(crate::tf!("{} row(s)", count).into_owned()),
                "{} of drawing {at}",
                row.label
            );
            assert_eq!(details.len(), count + 2, "{} of drawing {at}", row.label);
        }
    }
    tally.drawings += 1;
    tally.nodes += rows.len();
    tally.first.get_or_insert(at);
    tally.process_points += data.process_points.len();
    tally.named_connectors += data
        .process_points
        .iter()
        .filter(|point| point.connector.is_some())
        .count();
    tally.branch_points += data.branch_points.len();
    tally.piping_branch_points += data
        .branch_points
        .iter()
        .filter(|point| point.class == "PIDPipingBranchPoint")
        .count();
    tally.representations += data.representations.len();
    for representation in &data.representations {
        match (&representation.item, &representation.item_uid) {
            (Some(_), _) => tally.of_items += 1,
            (None, None) => tally.on_runs += 1,
            (None, Some(_)) => tally.of_unpublished += 1,
        }
    }
    let (total, stats) = (&mut tally.stats, &data.stats);
    total.attribute_records += stats.attribute_records;
    total.undecoded_records += stats.undecoded_records;
    total.label_values += stats.label_values;
    total.label_values_reached += stats.label_values_reached;
    total.label_values_published += stats.label_values_published;
    total.conflicting_attributes += stats.conflicting_attributes;
    total.nominal_diameter_not_emitted += stats.nominal_diameter_not_emitted;
    total.instruments_classed_by_library += stats.instruments_classed_by_library;
}

/// What the database knows of the backup's drawings, added up.
#[derive(Default)]
struct DatabaseTally {
    /// Drawings whose database row was opened, and those it gave views for.
    drawings: usize,
    read: usize,
    /// Rows of each view over the drawings: ports, connections, nozzles and
    /// notes, off-page connectors, pipelines.
    rows: [usize; 5],
    /// Drawings a view was not there for, and the first reason given.
    missing: [usize; 5],
    why: [Option<String>; 5],
    /// Drawings the read failed for altogether, and the first reason.
    failed: usize,
    failure: Option<String>,
    /// How long opening the rows took together, and the slowest.
    time: Duration,
    slowest: (Duration, String),
}

/// The database's row under the open drawing `at`: muted and saying why
/// when the panel does not read the dump; otherwise opened, which reads
/// the drawing's side of the database, and every view under it visited.
fn walk_drawing_database(
    app: &mut OpenCADStudio,
    index: &BackupIndex,
    at: usize,
    tally: &mut DatabaseTally,
    empty: &mut Vec<String>,
) {
    let title = crate::t!("Ports, connections, OPC and pipelines");
    let Some(row) = rows_under(app, &NodeId::Drawing(at))
        .into_iter()
        .find(|row| row.label == title)
    else {
        return;
    };
    if let Some(why) = index.dump_unreadable() {
        assert!(!row.expandable && row.disabled, "{row:?}");
        let details = details_of(app, &row.id, empty);
        assert!(details.iter().any(|(key, _)| *key == why), "{details:?}");
        return;
    }
    assert!(row.expandable && !row.disabled, "{row:?}");
    tally.drawings += 1;
    let started = Instant::now();
    send(app, PidBackupMsg::Toggle(row.id.clone())).unwrap();
    let took = started.elapsed();
    tally.time += took;
    if took > tally.slowest.0 {
        tally.slowest = (took, index.drawings[at].name.clone());
    }
    let details = details_of(app, &row.id, empty);
    match section_load(app, &row.id) {
        Some(SectionLoad::Ready(_)) => {
            tally.read += 1;
            let views = rows_under(app, &row.id);
            assert_eq!(views.len(), 5, "the views under {}: {views:?}", row.label);
            for (view_at, view) in views.iter().enumerate() {
                let shown = details_of(app, &view.id, empty);
                // A view with rows carries their count in its label; one
                // the reader has none of says why in its details.
                let count = view
                    .label
                    .rsplit_once(" (")
                    .and_then(|(_, count)| count.strip_suffix(')'))
                    .and_then(|count| count.parse::<usize>().ok());
                match count {
                    Some(count) => {
                        tally.rows[view_at] += count;
                        if count > 0 {
                            let stated = crate::tf!("{} row(s)", count).into_owned();
                            assert!(
                                shown.iter().any(|(_, rows)| *rows == stated),
                                "{} of drawing {at}: {shown:?}",
                                view.label
                            );
                        }
                    }
                    None => {
                        tally.missing[view_at] += 1;
                        let why = shown.last().map(|(why, _)| why.clone()).unwrap_or_default();
                        assert!(!why.is_empty(), "{} of drawing {at} says nothing", view.label);
                        tally.why[view_at].get_or_insert(why);
                    }
                }
            }
        }
        Some(SectionLoad::Failed(why)) => {
            tally.failed += 1;
            assert!(details.iter().any(|(key, _)| *key == why), "{details:?}");
            tally.failure.get_or_insert(why);
        }
        other => panic!("the database row of drawing {at} was left in {other:?}"),
    }
    send(app, PidBackupMsg::Toggle(row.id)).unwrap();
}

fn millis(duration: Duration) -> String {
    format!("{:.1} ms", duration.as_secs_f64() * 1000.0)
}

/// The restore cache as the backup's details state it, against the folder
/// itself, and as the command line reports it. Nothing is cleared here: the
/// cache is the one every walk reuses.
fn walk_cache(
    app: &mut OpenCADStudio,
    audit: &mut Audit,
    index: &BackupIndex,
    empty: &mut Vec<String>,
) {
    use pid_backup_browser::human_size;
    // Selecting the backup measures the cache, off the UI thread.
    send(app, PidBackupMsg::Select(NodeId::Root)).unwrap();
    let usage = pid_backup_browser::cache_usage(&app.pid_backup_browser)
        .cloned()
        .expect("selecting the backup measures the restore cache");
    let configured =
        pid_backup_browser::cache_folder(&app.pid_backup_browser).map(Path::to_path_buf);
    let root = configured
        .clone()
        .unwrap_or_else(crate::io::pid_backup::default_cache_root);
    // What the panel holds is what the folder holds, measured apart from it.
    assert_eq!(
        usage,
        crate::io::pid_backup::cache::usage(&root, Some(&index.restored)),
        "the panel's measurement is the folder's"
    );
    // A backup that had to be unpacked has its restore in the cache; a
    // folder of loose drawings is read where it lies.
    let unpacked = index.restored != index.source;
    let this_backup = match &usage.current {
        Some(restore) => {
            assert!(unpacked, "a folder read in place has no restore");
            assert_eq!(restore.folder, index.restored);
            let drawings = index
                .drawings
                .iter()
                .filter(|drawing| !drawing.is_missing())
                .count();
            assert!(
                restore.files > drawings && restore.bytes > 0,
                "the restore holds the {drawings} drawing(s) and its mark: {restore:?}"
            );
            crate::tf!("{} in {} file(s)", human_size(restore.bytes), restore.files).into_owned()
        }
        None => {
            assert!(!unpacked, "the restore of an unpacked backup is in the cache");
            crate::t!("the folder is read where it lies; nothing is cached").into_owned()
        }
    };

    let details = details_of(app, &NodeId::Root, empty);
    let heading = details
        .iter()
        .position(|(key, _)| *key == crate::t!("Restore cache"))
        .expect("the backup's details have the restore cache");
    let end = details
        .iter()
        .position(|(key, _)| *key == crate::t!("Check of the drawings"))
        .unwrap_or(details.len());
    let lines = &details[heading + 1..end];
    audit.line("== restore cache (in the backup's details) ==");
    for (key, value) in lines {
        audit.line(if value.is_empty() {
            format!("  [{key}]")
        } else {
            format!("  {key} = {value}")
        });
    }
    let shown = |key: std::borrow::Cow<'static, str>| -> String {
        lines
            .iter()
            .find(|(found, _)| *found == key)
            .map(|(_, value)| value.clone())
            .unwrap_or_else(|| panic!("the restore cache does not state {key}"))
    };
    assert_eq!(
        shown(crate::t!("Cache folder")),
        if configured.is_none() {
            crate::tf!("{} (the default folder)", root.display()).into_owned()
        } else {
            root.display().to_string()
        }
    );
    assert_eq!(shown(crate::t!("This backup's restore")), this_backup);
    assert_eq!(
        shown(crate::t!("Other restores")),
        if usage.others == 0 {
            crate::t!("no other restores").into_owned()
        } else {
            crate::tf!("{}, {} together", usage.others, human_size(usage.other_bytes))
                .into_owned()
        }
    );
    // Clearing is offered when there is something to clear; moving the
    // cache always.
    let buttons: Vec<String> = pid_backup_browser::selected_buttons(&app.pid_backup_browser)
        .into_iter()
        .map(|(label, _)| label)
        .collect();
    assert_eq!(
        buttons.contains(&crate::t!("Clear the other restores").into_owned()),
        usage.others > 0,
        "{buttons:?}"
    );
    assert!(
        buttons.contains(&crate::t!("Change the cache folder…").into_owned()),
        "{buttons:?}"
    );
    // The command line says the same when asked.
    let history = app.command_line.history.len();
    let task = app.run_command_line("PIDBACKUPOPEN Cache");
    app.drive_headless_task(task)
        .expect("measuring the cache needs no window");
    let said = &app.command_line.history[history..];
    for entry in said {
        audit.line(format!("  command line [{:?}]: {}", entry.kind, entry.text));
    }
    assert!(
        said.iter().any(|entry| {
            entry.kind == crate::ui::command_line::EntryKind::Output
                && entry.text.contains(&root.display().to_string())
                && entry.text.contains(&human_size(usage.other_bytes))
        }),
        "the command line reports the cache"
    );
    // With most of the panel for the details the whole of them shows.
    let share = app.pid_backup_browser.split().tree_share();
    send(app, PidBackupMsg::SetSplit(0.3)).unwrap();
    audit.shot(app, "restore-cache");
    send(app, PidBackupMsg::SetSplit(share)).unwrap();
}

/// The file-level half of the walk: what the index holds, node by node.
fn walk_index(
    app: &mut OpenCADStudio,
    audit: &mut Audit,
    index: &BackupIndex,
    empty: &mut Vec<String>,
) {
    // The backup itself.
    send(app, PidBackupMsg::Select(NodeId::Root)).unwrap();
    audit.line("== the backup (root node) ==");
    for (key, value) in details_of(app, &NodeId::Root, empty) {
        audit.line(format!("  {key}: {value}"));
    }
    audit.shot(app, "root-details");
    walk_cache(app, audit, index, empty);

    // Backup files, and the members of the archives among them.
    send(app, PidBackupMsg::Toggle(NodeId::Files)).unwrap();
    details_of(app, &NodeId::Files, empty);
    audit.line(format!("== backup files ({}) ==", index.files.len()));
    let mut members = 0;
    for (at, file) in index.files.iter().enumerate() {
        let rows = details_of(app, &NodeId::File(at), empty);
        let shown: Vec<String> = rows
            .iter()
            .skip(1)
            .map(|(key, value)| {
                if value.is_empty() {
                    key.clone()
                } else {
                    format!("{key}={value}")
                }
            })
            .collect();
        audit.line(format!("  {}: {}", file.name, shown.join("; ")));
        for member in 0..file.members.len() {
            details_of(app, &NodeId::FileMember(at, member), empty);
            members += 1;
        }
    }
    audit.line(format!("  {members} archive members, each with details"));
    audit.shot(app, "files-open");
    if let Some(at) = index.files.iter().position(|file| file.name == "Export.dmp") {
        send(app, PidBackupMsg::Select(NodeId::File(at))).unwrap();
        audit.shot(app, "file-export-dmp");
    }
    if let Some(at) = index.files.iter().position(|file| !file.members.is_empty()) {
        send(app, PidBackupMsg::Toggle(NodeId::File(at))).unwrap();
        send(app, PidBackupMsg::Select(NodeId::FileMember(at, 0))).unwrap();
        audit.shot(app, "file-archive-members");
        send(app, PidBackupMsg::Toggle(NodeId::File(at))).unwrap();
    }
    if let Some(at) = index.files.iter().position(|file| file.missing) {
        send(app, PidBackupMsg::Select(NodeId::File(at))).unwrap();
        audit.shot(app, "file-missing");
    }

    // What the sections hang under the backup files: the manifest's parts,
    // and whatever later ones add.
    audit.line("== sections under the backup files ==");
    let mut nodes = 0;
    for (at, file) in index.files.iter().enumerate() {
        let host = NodeId::File(at);
        let hosted = visible_rows(&app.pid_backup_browser)
            .iter()
            .any(|row| row.id == host && row.expandable && file.members.is_empty());
        if !hosted {
            continue;
        }
        audit.line(format!("  {}", file.name));
        nodes += walk_sections(app, audit, &host, empty);
    }
    audit.line(format!("  {nodes} section node(s), each with details"));
    // The manifest's parts, drawn.
    if let Some(at) = index
        .files
        .iter()
        .position(|file| file.kind == crate::io::pid_backup_browse::FileKind::Manifest)
    {
        let host = NodeId::File(at);
        let section_row = |app: &OpenCADStudio, nth: usize| -> Option<Row> {
            let rows = visible_rows(&app.pid_backup_browser);
            let at = rows.iter().position(|row| row.id == host)?;
            let depth = rows[at].depth;
            rows[at + 1..]
                .iter()
                .take_while(|row| row.depth > depth)
                .filter(|row| row.depth == depth + 1)
                .nth(nth)
                .cloned()
        };
        send(app, PidBackupMsg::Toggle(host.clone())).unwrap();
        for (nth, name) in [
            (0, "manifest-summary"),
            (2, "manifest-file-register"),
            (3, "manifest-connections"),
        ] {
            if let Some(row) = section_row(app, nth) {
                send(app, PidBackupMsg::Select(row.id)).unwrap();
                audit.shot(app, name);
            }
        }
        if let Some(objects) = section_row(app, 1) {
            if !objects.expanded {
                send(app, PidBackupMsg::Toggle(objects.id.clone())).unwrap();
            }
            let rows = visible_rows(&app.pid_backup_browser);
            if let Some(tables) = rows
                .iter()
                .position(|row| row.id == objects.id)
                .and_then(|at| rows.get(at + 1))
            {
                send(app, PidBackupMsg::Select(tables.id.clone())).unwrap();
                audit.shot(app, "manifest-tables");
            }
            send(app, PidBackupMsg::Toggle(objects.id)).unwrap();
        }
        send(app, PidBackupMsg::Toggle(host)).unwrap();
    }
    send(app, PidBackupMsg::Toggle(NodeId::Files)).unwrap();

    // The reference data tree.
    send(app, PidBackupMsg::Toggle(NodeId::Ref)).unwrap();
    details_of(app, &NodeId::Ref, empty);
    let dirs = ref_dir_paths(&index.ref_tree);
    let mut by_extension: BTreeMap<String, usize> = BTreeMap::new();
    let mut files = 0;
    let mut count_files = |node: &RefNode, path: &[usize], app: &OpenCADStudio, empty: &mut Vec<String>| {
        for (at, file) in node.files.iter().enumerate() {
            details_of(app, &NodeId::RefFile(path.to_vec(), at), empty);
            let extension = std::path::Path::new(&file.name)
                .extension()
                .map(|ext| ext.to_string_lossy().to_lowercase())
                .unwrap_or_default();
            *by_extension.entry(extension).or_default() += 1;
            files += 1;
        }
    };
    count_files(&index.ref_tree, &[], &*app, &mut *empty);
    for path in &dirs {
        details_of(app, &NodeId::RefDir(path.clone()), empty);
        count_files(ref_dir(&index.ref_tree, path), path, &*app, &mut *empty);
    }
    audit.line(format!(
        "== reference data: {} directories, {files} files ==",
        dirs.len()
    ));
    // The backup's own reference data volumes, which a restore unpacks into
    // the tree: a backup that carries any has a tree to show.
    let volumes = index
        .files
        .iter()
        .filter(|file| {
            file.kind == crate::io::pid_backup_browse::FileKind::RefData && !file.missing
        })
        .count();
    audit.line(format!(
        "  read from {} ({volumes} reference data volume(s) in the backup)",
        index.ref_root.display()
    ));
    if volumes > 0 && index.restored != index.source {
        assert!(
            files > 0,
            "the backup carries {volumes} reference data volume(s) and the tree is empty"
        );
    }
    for dir in &index.ref_tree.dirs {
        audit.line(format!("  {}: {} files", dir.name, dir.file_count()));
    }
    audit.line(format!("  by extension: {by_extension:?}"));
    if let Some(first) = dirs.first() {
        send(app, PidBackupMsg::Toggle(NodeId::RefDir(first.clone()))).unwrap();
        send(app, PidBackupMsg::Select(NodeId::RefDir(first.clone()))).unwrap();
    }
    audit.shot(app, "ref-open");
    if let Some(first) = dirs.first() {
        send(app, PidBackupMsg::Toggle(NodeId::RefDir(first.clone()))).unwrap();
    }
    send(app, PidBackupMsg::Toggle(NodeId::Ref)).unwrap();

    walk_text(app, audit, index, empty);
    walk_database(app, audit, index);
    walk_dump_views(app, audit, index, empty);
    walk_data_maps(app, audit, index, empty);
    walk_split(app, audit, index);
}

/// The bar between the tree and the details, moved from the command line as
/// a script moves it: the share is the panel's, and with less for the tree
/// a text file has room to be read.
fn walk_split(app: &mut OpenCADStudio, audit: &mut Audit, index: &BackupIndex) {
    let share = |app: &OpenCADStudio| app.pid_backup_browser.split().tree_share();
    let default = share(app);
    audit.line(format!("== split: the tree has {default:.2} of the panel's height =="));
    // The first backup file that is text: the plant's rules, where there
    // is one.
    send(app, PidBackupMsg::Toggle(NodeId::Files)).unwrap();
    let text = (0..index.files.len()).map(NodeId::File).find(|id| {
        send(app, PidBackupMsg::Select(id.clone())).unwrap();
        detail_rows(&app.pid_backup_browser, id)
            .iter()
            .any(|(key, _)| *key == crate::t!("Text encoding"))
            && matches!(id, NodeId::File(at) if index.files[*at].record.is_some())
    });
    if text.is_some() {
        audit.shot(app, "split-default-text-file");
    }
    let history = app.command_line.history.len();
    let task = app.run_command_line("PIDBACKUPOPEN Split 30");
    app.drive_headless_task(task).expect("moving the bar needs no window");
    for entry in &app.command_line.history[history..] {
        audit.line(format!("  command line [{:?}]: {}", entry.kind, entry.text));
    }
    assert_eq!(share(app), 0.3, "PIDBACKUPOPEN Split 30 gives the tree 30%");
    if text.is_some() {
        audit.shot(app, "split-30-text-file");
    }
    // Refreshing the backup keeps the bar where the user put it.
    let task = app.run_command_line(&format!(
        "PIDBACKUPOPEN Split {}",
        (default * 100.0).round()
    ));
    app.drive_headless_task(task).expect("moving the bar needs no window");
    assert_eq!(share(app), default);
    send(app, PidBackupMsg::Toggle(NodeId::Files)).unwrap();
    send(app, PidBackupMsg::Select(NodeId::Root)).unwrap();
}

#[test]
#[ignore = "reads a real SmartPlant backup; run alone with --ignored --nocapture"]
fn a_real_backup_is_walked_node_by_node() {
    let source = std::env::var_os("OCS_PID_BACKUP_AUDIT")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(DEFAULT_BACKUP));
    if !source.exists() {
        eprintln!("skipped: no backup at {}", source.display());
        return;
    }
    let out = std::env::var_os("OCS_PID_BACKUP_AUDIT_OUT").map(PathBuf::from);
    if let Some(folder) = &out {
        std::fs::create_dir_all(folder).expect("the output folder can be made");
    }
    let mut audit = Audit {
        out,
        report: Vec::new(),
        shots: 0,
    };
    let mut empty: Vec<String> = Vec::new();

    let mut app = OpenCADStudio::new_for_test();
    let _ = app.automation_op(r#"{"op":"new"}"#);
    // After the application is up: starting it sets the configured language.
    if std::env::var("OCS_PID_BACKUP_AUDIT_LANG").is_ok_and(|lang| lang == "zh-CN") {
        crate::i18n::set_language(crate::i18n::Language::ZhCn)
            .expect("Chinese Fluent resources must parse");
    }

    // ── The load, as the command starts it ───────────────────────────────
    audit.line(format!("source: {}", source.display()));
    let started = Instant::now();
    let task = app.dispatch_command(&format!("PIDBACKUPOPEN {}", source.display()));
    assert!(
        matches!(app.pid_backup_browser.load, LoadState::Loading(_)),
        "the command puts the panel in its loading state"
    );
    audit.shot(&app, "loading");
    app.drive_headless_task(task)
        .expect("indexing a backup needs no window");
    let load = started.elapsed();
    let index = app
        .pid_backup_browser
        .index()
        .cloned()
        .unwrap_or_else(|| panic!("the backup did not load: {:?}", app.pid_backup_browser.error));
    assert!(app.dock_panel_visible(PanelId::PidBackup));
    audit.line(format!(
        "loaded in {}: {} backup files, {} drawings, {} reference files, {} warnings; restored to {}{}",
        millis(load),
        index.files.len(),
        index.drawings.len(),
        index.ref_tree.file_count(),
        index.warnings.len(),
        index.restored.display(),
        if index.read_only { " (source read in place)" } else { "" },
    ));
    for warning in &index.warnings {
        audit.line(format!("  warning: {warning}"));
    }
    for entry in app.command_line.history.iter().rev().take(4).rev() {
        audit.line(format!("  command line [{:?}]: {}", entry.kind, entry.text));
    }
    audit.shot(&app, "ready");

    walk_index(&mut app, &mut audit, &index, &mut empty);

    // ── Plant groups and the drawings in them ────────────────────────────
    send(&mut app, PidBackupMsg::Toggle(NodeId::Groups)).unwrap();
    details_of(&app, &NodeId::Groups, &mut empty);
    let groups = group_paths(&index.plant_groups);
    audit.line(format!("== plant groups ({}) ==", groups.len()));
    for path in &groups {
        send(&mut app, PidBackupMsg::Toggle(NodeId::Group(path.clone()))).unwrap();
        let rows = details_of(&app, &NodeId::Group(path.clone()), &mut empty);
        let text: Vec<String> = rows.iter().map(|(key, value)| format!("{key}={value}")).collect();
        audit.line(format!("  {}", text.join("; ")));
    }
    audit.shot(&app, "groups-open");

    // Which drawing-table columns the backup actually fills.
    let mut filled: BTreeMap<String, usize> = BTreeMap::new();
    let mut order: Vec<String> = Vec::new();
    let mut missing_files = 0;
    for (at, drawing) in index.drawings.iter().enumerate() {
        details_of(&app, &NodeId::Drawing(at), &mut empty);
        if drawing.is_missing() {
            missing_files += 1;
        }
        for (key, value) in &drawing.meta {
            if !order.contains(key) {
                order.push(key.clone());
            }
            if !value.is_empty() {
                *filled.entry(key.clone()).or_default() += 1;
            }
        }
    }
    audit.line(format!(
        "== drawings ({}; {missing_files} without a file) ==",
        index.drawings.len()
    ));
    audit.line("  drawing-table columns filled, of all drawings:");
    for key in &order {
        audit.line(format!(
            "    {key}: {}/{}",
            filled.get(key).copied().unwrap_or(0),
            index.drawings.len()
        ));
    }
    if !index.drawings.is_empty() {
        send(&mut app, PidBackupMsg::Select(NodeId::Drawing(0))).unwrap();
        audit.shot(&app, "drawing-details");
    }

    // ── Every drawing's items ────────────────────────────────────────────
    let mut item_types: BTreeMap<String, usize> = BTreeMap::new();
    // What the items of each type publish: how many carry each attribute,
    // which is what a row's label and a search by tag can be built on.
    let mut attribute_keys: BTreeMap<String, BTreeMap<String, usize>> = BTreeMap::new();
    let mut total_items = 0;
    let mut total_attributes = 0;
    let mut without_attributes = 0;
    // How the rows of the items read: by the name the item publishes, or by
    // its type and number when it publishes none.
    let mut named = 0;
    let mut numbered = 0;
    let mut failed: Vec<String> = Vec::new();
    let mut silent: Vec<String> = Vec::new();
    let mut slowest = (Duration::ZERO, String::new());
    let mut item_time = Duration::ZERO;
    let mut data_tally = DataTally::default();
    let mut database_tally = DatabaseTally::default();
    audit.line("  items per drawing (name | items | types | attributes | read):");
    for (at, drawing) in index.drawings.iter().enumerate() {
        if drawing.is_missing() {
            continue;
        }
        let started = Instant::now();
        send(&mut app, PidBackupMsg::Toggle(NodeId::Drawing(at))).unwrap();
        let took = started.elapsed();
        item_time += took;
        match app.pid_backup_browser.items.get(&at).cloned() {
            Some(ItemsState::Ready(items)) => {
                let mut attributes = 0;
                for (group_at, group) in items.groups.iter().enumerate() {
                    *item_types.entry(group.item_type.clone()).or_default() += group.items.len();
                    details_of(&app, &NodeId::ItemType(at, group_at), &mut empty);
                    for (item_at, item) in group.items.iter().enumerate() {
                        details_of(&app, &NodeId::Item(at, group_at, item_at), &mut empty);
                        attributes += item.attributes.len();
                        if item.label.starts_with(&format!("{} #", group.item_type)) {
                            numbered += 1;
                        } else {
                            named += 1;
                        }
                        assert!(
                            !item.label.is_empty() && item.label != item.uid,
                            "an item's row is its name or its type and number, not its UID: {item:?}"
                        );
                        let keys = attribute_keys.entry(group.item_type.clone()).or_default();
                        for attribute in &item.attributes {
                            if !attribute.value.is_empty() {
                                *keys
                                    .entry(format!("{}.{}", attribute.interface, attribute.name))
                                    .or_default() += 1;
                            }
                        }
                        if item.attributes.is_empty() {
                            without_attributes += 1;
                        }
                    }
                }
                total_items += items.item_count;
                total_attributes += attributes;
                walk_drawing_data(&app, at, &items, &mut data_tally, &mut empty);
                walk_drawing_database(&mut app, &index, at, &mut database_tally, &mut empty);
                if took > slowest.0 {
                    slowest = (took, drawing.name.clone());
                }
                audit.line(format!(
                    "    {} | {} | {} | {attributes} | {}",
                    drawing.name,
                    items.item_count,
                    items.groups.len(),
                    millis(took)
                ));
            }
            Some(ItemsState::Failed(error)) => {
                // A read that fails must not fail silently: the drawing's
                // details carry the reason, and so does the command line.
                let rows = details_of(&app, &NodeId::Drawing(at), &mut empty);
                let shown = rows.iter().any(|(key, _)| *key == error);
                let reported = app
                    .command_line
                    .history
                    .last()
                    .is_some_and(|entry| entry.text.contains(&error));
                if !(shown && reported) {
                    silent.push(drawing.name.clone());
                }
                if failed.is_empty() {
                    send(&mut app, PidBackupMsg::Select(NodeId::Drawing(at))).unwrap();
                    audit.shot(&app, "drawing-items-unreadable");
                }
                failed.push(format!("{}: {error}", drawing.name));
            }
            other => {
                silent.push(drawing.name.clone());
                failed.push(format!("{}: left in {other:?}", drawing.name));
            }
        }
        // Keep the first drawing open for the pictures; fold the rest so the
        // tree stays the size a user would have it.
        if at != 0 {
            send(&mut app, PidBackupMsg::Toggle(NodeId::Drawing(at))).unwrap();
        }
    }
    audit.line(format!(
        "  {total_items} items, {total_attributes} attributes ({without_attributes} items carry none); all read in {}, slowest {} ({})",
        millis(item_time),
        slowest.1,
        millis(slowest.0)
    ));
    audit.line(format!(
        "  item rows: {named} by the name the item publishes, {numbered} by type and number"
    ));
    audit.line(format!("  item types across the backup: {item_types:?}"));
    audit.line("  attributes with a value, by item type (attribute: items):");
    for (item_type, keys) in &attribute_keys {
        let mut keys: Vec<(&String, &usize)> = keys.iter().collect();
        keys.sort_by(|left, right| right.1.cmp(left.1).then(left.0.cmp(right.0)));
        let text: Vec<String> = keys.iter().map(|(key, count)| format!("{key}: {count}")).collect();
        audit.line(format!(
            "    {item_type} ({}): {}",
            item_types.get(item_type).copied().unwrap_or(0),
            text.join(", ")
        ));
    }
    audit.shot(&app, "drawing-items-open");

    // ── What the files hold beside the items ─────────────────────────────
    audit.line(format!(
        "== drawing data: {} drawing(s) show what their file holds beside the items, in {} node(s) ==",
        data_tally.drawings, data_tally.nodes
    ));
    audit.line(format!(
        "  process points: {} ({} on a connector the drawing names)",
        data_tally.process_points, data_tally.named_connectors
    ));
    audit.line(format!(
        "  branch points: {} ({} piping, {} signal)",
        data_tally.branch_points,
        data_tally.piping_branch_points,
        data_tally.branch_points - data_tally.piping_branch_points
    ));
    audit.line(format!(
        "  representations: {} ({} of an item, {} of a branch point on a run, {} of an item the drawing does not publish)",
        data_tally.representations,
        data_tally.of_items,
        data_tally.on_runs,
        data_tally.of_unpublished
    ));
    let read = &data_tally.stats;
    audit.line(format!(
        "  read: {} attribute records, {} not decoded; {} label values, {} reach an item, {} published; {} attributes left out for conflicting labels; {} nominal diameter labels not published; {} instruments classed by their symbol's folder",
        read.attribute_records,
        read.undecoded_records,
        read.label_values,
        read.label_values_reached,
        read.label_values_published,
        read.conflicting_attributes,
        read.nominal_diameter_not_emitted,
        read.instruments_classed_by_library
    ));
    if let Some(at) = data_tally.first {
        // The first drawing is still open; any other is opened for its
        // pictures and folded again.
        if at != 0 {
            send(&mut app, PidBackupMsg::Toggle(NodeId::Drawing(at))).unwrap();
        }
        let rows: Vec<Row> = rows_under(&app, &NodeId::Drawing(at))
            .into_iter()
            .filter(|row| matches!(row.id, NodeId::Section(..)))
            .collect();
        for (row, name) in rows.iter().zip([
            "drawing-data-process-points",
            "drawing-data-branch-points",
            "drawing-data-representations",
            "drawing-data-database",
            "drawing-data-statistics",
        ]) {
            send(&mut app, PidBackupMsg::Select(row.id.clone())).unwrap();
            let details = detail_rows(&app.pid_backup_browser, &row.id);
            audit.line(format!("  {} [{} detail line(s)]", row.label, details.len()));
            for (key, value) in details.iter().take(4) {
                audit.line(format!("      {key} = {value}"));
            }
            audit.shot(&app, name);
        }
        if at != 0 {
            send(&mut app, PidBackupMsg::Toggle(NodeId::Drawing(at))).unwrap();
        }
    }

    // ── What the database knows of the drawings ──────────────────────────
    match index.dump_unreadable() {
        Some(why) => audit.line(format!(
            "== database side of the drawings: not read ({why}); the row says so under each of {} drawing(s) ==",
            data_tally.drawings
        )),
        None => {
            let tally = &database_tally;
            audit.line(format!(
                "== database side of the drawings: {} of {} drawing(s) read in {}, slowest {} ({}) ==",
                tally.read,
                tally.drawings,
                millis(tally.time),
                tally.slowest.1,
                millis(tally.slowest.0)
            ));
            for (at, name) in ["ports", "connections", "nozzles and notes", "off-page connectors", "pipelines"]
                .iter()
                .enumerate()
            {
                audit.line(format!(
                    "  {name}: {} row(s){}",
                    tally.rows[at],
                    tally.why[at].as_ref().map_or(String::new(), |why| format!(
                        "; not there for {} drawing(s): {why}",
                        tally.missing[at]
                    ))
                ));
            }
            if let Some(why) = &tally.failure {
                audit.line(format!("  the read failed for {} drawing(s): {why}", tally.failed));
            }
            // No drawing is left without an answer.
            assert_eq!(tally.read + tally.failed, tally.drawings);
            // The pictures: the first drawing that has the rows.
            if let Some(at) = data_tally.first {
                if at != 0 {
                    send(&mut app, PidBackupMsg::Toggle(NodeId::Drawing(at))).unwrap();
                }
                let title = crate::t!("Ports, connections, OPC and pipelines");
                if let Some(row) = rows_under(&app, &NodeId::Drawing(at))
                    .into_iter()
                    .find(|row| row.label == title)
                {
                    send(&mut app, PidBackupMsg::Toggle(row.id.clone())).unwrap();
                    send(&mut app, PidBackupMsg::Select(row.id.clone())).unwrap();
                    audit.shot(&app, "drawing-database-open");
                    let views = rows_under(&app, &row.id);
                    for (view, name) in views.iter().zip([
                        "drawing-database-ports",
                        "drawing-database-connections",
                        "drawing-database-relations",
                        "drawing-database-opc",
                        "drawing-database-pipelines",
                    ]) {
                        send(&mut app, PidBackupMsg::Select(view.id.clone())).unwrap();
                        let details = detail_rows(&app.pid_backup_browser, &view.id);
                        audit.line(format!("  {} [{} detail line(s)]", view.label, details.len()));
                        for (key, value) in details.iter().take(4) {
                            audit.line(format!("      {key} = {value}"));
                        }
                        audit.shot(&app, name);
                    }
                    send(&mut app, PidBackupMsg::Toggle(row.id)).unwrap();
                }
                if at != 0 {
                    send(&mut app, PidBackupMsg::Toggle(NodeId::Drawing(at))).unwrap();
                }
            }
        }
    }
    // ── One drawing's import, from the button in its own details ─────────
    // The first drawing whose items read, and the first whose items did
    // not: that one has no subtree to look into, so the button is all
    // there is to learn more about it from.
    let unread_first = (0..index.drawings.len()).find(|at| {
        matches!(
            app.pid_backup_browser.items.get(at),
            Some(ItemsState::Failed(_))
        )
    });
    for (at, what, picture) in [
        (data_tally.first, "its items read", "drawing-import-diagnosis"),
        (
            unread_first,
            "its items did not read",
            "drawing-import-diagnosis-items-unread",
        ),
    ] {
        let Some(at) = at else {
            continue;
        };
        let drawing = NodeId::Drawing(at);
        send(&mut app, PidBackupMsg::Select(drawing.clone())).unwrap();
        let buttons = detail_buttons(&app.pid_backup_browser, &drawing);
        assert_eq!(
            buttons.len(),
            1,
            "a drawing with a file offers its import: {buttons:?}"
        );
        let started = Instant::now();
        send(&mut app, PidBackupMsg::Activate(buttons[0].1.clone())).unwrap();
        let took = started.elapsed();
        assert!(
            matches!(
                section_load(&app, &buttons[0].1),
                Some(SectionLoad::Ready(_))
            ),
            "the import of {} was left unfinished",
            index.drawings[at].name
        );
        let details = details_of(&app, &drawing, &mut empty);
        let position = |key: std::borrow::Cow<'static, str>| {
            details
                .iter()
                .position(|(found, _)| *found == key)
                .unwrap_or_else(|| panic!("the drawing's details have no \"{key}\""))
        };
        // The import stands under its heading, with the button that opens
        // the drawing and above the drawing's columns; it ends with how
        // long it took, and one that failed says why on the line before.
        // The parser's report is at the end of the details.
        let heading = position(crate::t!("Importing it"));
        let imported = position(crate::t!("Imported in"));
        let report = position(crate::t!("Parser's report of the file"));
        assert!(heading < imported && imported < report, "{details:?}");
        audit.line(format!(
            "== import diagnosis of {} ({what}; the button in its details; {}) ==",
            index.drawings[at].name,
            millis(took)
        ));
        for (key, value) in &details[heading + 1..=imported] {
            audit.line(format!("  {key} = {value}"));
        }
        // The parser's report: how long it is and its lines, or why there
        // is none.
        let lines = &details[report + 1..];
        match lines.first() {
            Some((key, count)) if *key == crate::t!("Line count") => {
                let rows = lines
                    .iter()
                    .position(|(titles, _)| titles.contains(" | "))
                    .map_or(&[][..], |table| &lines[table + 1..]);
                assert!(!rows.is_empty(), "a report of {count} line(s) shows none");
                audit.line(format!(
                    "  parser's report: {count} line(s) in {} row(s)",
                    rows.len()
                ));
                for (number, text) in rows.iter().take(6) {
                    audit.line(format!("      {number:>3} {text}"));
                }
            }
            Some((why, _)) => audit.line(format!("  parser's report: {why}")),
            None => panic!("the drawing's details say nothing of the parser's report"),
        }
        // The button is gone: the drawing is imported once.
        assert!(detail_buttons(&app.pid_backup_browser, &drawing).is_empty());
        audit.shot(&app, picture);
    }
    if let Some(ItemsState::Ready(items)) = app.pid_backup_browser.items.get(&0).cloned() {
        // A type's details list its items as a table.
        if !items.groups.is_empty() {
            send(&mut app, PidBackupMsg::Select(NodeId::ItemType(0, 0))).unwrap();
            let rows = detail_rows(&app.pid_backup_browser, &NodeId::ItemType(0, 0));
            audit.line(format!(
                "== item type \"{}\" of the first drawing: table {} ==",
                items.groups[0].item_type,
                rows.get(2).map_or(String::new(), |(titles, count)| format!("[{titles}] {count}")),
            ));
            for (name, rest) in rows.iter().skip(3).take(4) {
                audit.line(format!("  {name} | {rest}"));
            }
            audit.shot(&app, "item-type-table");
        }
        // The first item that publishes attributes: its details are the
        // richest the panel has.
        let found = items.groups.iter().enumerate().find_map(|(group_at, group)| {
            group
                .items
                .iter()
                .position(|item| !item.attributes.is_empty())
                .map(|item_at| (group_at, item_at))
        });
        if let Some((group_at, item_at)) = found {
            send(&mut app, PidBackupMsg::Toggle(NodeId::ItemType(0, group_at))).unwrap();
            send(&mut app, PidBackupMsg::Select(NodeId::Item(0, group_at, item_at))).unwrap();
            audit.shot(&app, "item-details");
        }
    }

    // ── The search box ───────────────────────────────────────────────────
    let rows_before = visible_rows(&app.pid_backup_browser).len();
    let needle = index
        .drawings
        .first()
        .map(|drawing| drawing.name.chars().take(2).collect::<String>())
        .unwrap_or_default();
    send(&mut app, PidBackupMsg::Filter(needle.clone())).unwrap();
    let matching = index
        .drawings
        .iter()
        .filter(|drawing| drawing.name.to_lowercase().contains(&needle.to_lowercase()))
        .count();
    audit.line(format!(
        "== search \"{needle}\": {} rows (were {rows_before}), {matching} drawings match ==",
        visible_rows(&app.pid_backup_browser).len()
    ));
    audit.shot(&app, "search");
    send(&mut app, PidBackupMsg::Filter(String::new())).unwrap();

    // ── A node reached from the command line ─────────────────────────────
    // `PIDBACKUPOPEN Select a > b > c`, as a script or the automation
    // channel sends it: down to the first item of the first drawing that
    // has one, by the labels the tree shows.
    let readable = index.drawings.iter().enumerate().position(|(at, _)| {
        matches!(
            app.pid_backup_browser.items.get(&at),
            Some(ItemsState::Ready(items)) if items.item_count > 0
        )
    });
    if let Some(at) = readable {
        let rows = visible_rows(&app.pid_backup_browser);
        let row_at = rows
            .iter()
            .position(|row| row.id == NodeId::Drawing(at))
            .expect("every group is open, so the drawing has a row");
        // The labels above the row: the nearest row of each smaller depth.
        let mut labels = vec![rows[row_at].label.clone()];
        let mut depth = rows[row_at].depth;
        for row in rows[..row_at].iter().rev() {
            if row.depth < depth && row.depth > 0 {
                labels.push(row.label.clone());
                depth = row.depth;
            }
        }
        labels.reverse();
        let path = format!("{} > #1 > #1", labels.join(" > "));
        let history = app.command_line.history.len();
        let task = app.run_command_line(&format!("PIDBACKUPOPEN Select {path}"));
        app.drive_headless_task(task).expect("a walk needs no window");
        audit.line(format!("== select \"{path}\" =="));
        for entry in &app.command_line.history[history..] {
            audit.line(format!("  command line [{:?}]: {}", entry.kind, entry.text));
        }
        assert_eq!(
            app.pid_backup_browser.selected,
            Some(NodeId::Item(at, 0, 0)),
            "the walk ends on the first item of the drawing's first type"
        );
        audit.shot(&app, "select-by-path");
        // A path that names no node is refused where it stops.
        let task = app.run_command_line(&format!(
            "PIDBACKUPOPEN Select {} > no such node",
            labels.join(" > ")
        ));
        app.drive_headless_task(task).expect("a walk needs no window");
        let last = app.command_line.history.last().expect("the walk reports");
        audit.line(format!("  command line [{:?}]: {}", last.kind, last.text));
        assert_eq!(last.kind, crate::ui::command_line::EntryKind::Error);
    }

    // ── A drawing opened from the panel ──────────────────────────────────
    if let Some((at, drawing)) = index
        .drawings
        .iter()
        .enumerate()
        .find(|(_, drawing)| !drawing.is_missing())
    {
        let tabs = app.tabs.len();
        let started = Instant::now();
        let opened = send(&mut app, PidBackupMsg::Activate(NodeId::Drawing(at)));
        let took = started.elapsed();
        let tab = &app.tabs[app.active_tab];
        let entities = tab.scene.document.entities().count();
        audit.line(format!(
            "== open \"{}\": {} in {}; tabs {tabs} -> {}; active tab shows {:?} with {entities} entities ==",
            drawing.name,
            match &opened {
                Ok(()) => "the open ran to its end".to_string(),
                Err(error) => format!("stopped headless at: {error}"),
            },
            millis(took),
            app.tabs.len(),
            tab.current_path.as_ref().map(|path| path.display().to_string()),
        ));
        for entry in app.command_line.history.iter().rev().take(6).rev() {
            audit.line(format!("  command line [{:?}]: {}", entry.kind, entry.text));
        }
    }

    // ── Every drawing through the importer a double click runs ───────────
    let mut not_imported: Vec<String> = Vec::new();
    // Imports that failed outright: one that drew nothing is not among them.
    let mut import_errors = 0;
    let mut import_time = Duration::ZERO;
    let mut slowest_import = (Duration::ZERO, String::new());
    let mut entities = 0;
    let mut undrawn = 0;
    // What the importer made of the reference data: symbol bodies drawn from
    // the library, and the border templates overlaid.
    let mut library_bodies = 0;
    let mut cache_bodies = 0;
    let mut bordered = 0;
    let mut template_entities = 0;
    let mut libraries: Vec<PathBuf> = Vec::new();
    // The slow half of the walk (a plant's drawings take half a minute), and
    // the one a change to the panel does not touch: a run that is only
    // after the panel can leave it out.
    let import = std::env::var_os("OCS_PID_BACKUP_AUDIT_SKIP_IMPORT").is_none();
    audit.line(if import {
        "== import of every drawing (name | entities | decoded | not drawn | sheet layers | time) =="
    } else {
        "== import of every drawing: skipped (OCS_PID_BACKUP_AUDIT_SKIP_IMPORT) =="
    });
    for drawing in &index.drawings {
        if !import {
            break;
        }
        let Some(path) = &drawing.pid_path else {
            continue;
        };
        let started = Instant::now();
        let imported = crate::io::pid::load_pid(path);
        let took = started.elapsed();
        import_time += took;
        match imported {
            Ok(import) => {
                let summary = &import.summary;
                entities += summary.drawn;
                undrawn += summary.missing;
                library_bodies += summary.library_bodies;
                cache_bodies += summary.cache_bodies;
                bordered += usize::from(summary.border_template.is_some());
                template_entities += summary.template_entities;
                for root in &summary.symbol_library {
                    if !libraries.contains(root) {
                        libraries.push(root.clone());
                    }
                }
                if took > slowest_import.0 {
                    slowest_import = (took, drawing.name.clone());
                }
                audit.line(format!(
                    "    {} | {} | {} | {} | {} | {}",
                    drawing.name,
                    summary.drawn,
                    summary.decoded,
                    summary.missing,
                    summary.sheet_layers,
                    millis(took)
                ));
                if summary.drawn == 0 {
                    not_imported.push(format!("{}: imported with no entity", drawing.name));
                }
            }
            Err(error) => {
                import_errors += 1;
                not_imported.push(format!("{}: {error}", drawing.name));
            }
        }
    }
    audit.line(format!(
        "  {entities} entities drawn, {undrawn} source records not drawn; all imported in {}, slowest {} ({})",
        millis(import_time),
        slowest_import.1,
        millis(slowest_import.0)
    ));
    if import {
        audit.line(format!(
            "  reference data in the import: {library_bodies} symbol bodies from the library, {cache_bodies} from the drawings' own caches; {bordered} drawing(s) with their border template ({template_entities} entities); symbol library: {}",
            if libraries.is_empty() {
                "none found".to_string()
            } else {
                libraries
                    .iter()
                    .map(|root| root.display().to_string())
                    .collect::<Vec<_>>()
                    .join("; ")
            }
        ));
    }

    // ── The check of every drawing, from the backup's own details ────────
    send(&mut app, PidBackupMsg::Select(NodeId::Root)).unwrap();
    let buttons = detail_buttons(&app.pid_backup_browser, &NodeId::Root);
    let offered = index.drawings.iter().any(|drawing| !drawing.is_missing());
    assert_eq!(
        buttons.len(),
        usize::from(offered),
        "a backup with a drawing to read offers the check: {buttons:?}"
    );
    if !import || buttons.is_empty() {
        audit.line(format!(
            "== check of the drawings: {} ==",
            if buttons.is_empty() {
                "nothing to check"
            } else {
                "offered, not run (the imports are skipped)"
            }
        ));
    } else {
        let started = Instant::now();
        send(&mut app, PidBackupMsg::Activate(buttons[0].1.clone())).unwrap();
        let took = started.elapsed();
        assert!(
            matches!(
                section_load(&app, &buttons[0].1),
                Some(SectionLoad::Ready(_))
            ),
            "the check was left unfinished"
        );
        let details = details_of(&app, &NodeId::Root, &mut empty);
        let heading = details
            .iter()
            .position(|(key, _)| *key == crate::t!("Check of the drawings"))
            .expect("the backup's details have the check");
        let lines = &details[heading + 1..];
        let shown = |key: std::borrow::Cow<'static, str>| -> String {
            lines
                .iter()
                .find(|(found, _)| *found == key)
                .map(|(_, value)| value.clone())
                .unwrap_or_else(|| panic!("the check does not state {key}"))
        };
        audit.line(format!(
            "== check of the drawings (the button in the backup's details; {}) ==",
            millis(took)
        ));
        for (key, value) in lines.iter().take_while(|(titles, _)| !titles.contains(" | ")) {
            audit.line(format!("  {key} = {value}"));
        }
        // What the panel checked is what this walk read and imported.
        assert!(
            shown(crate::t!("Drawings checked")).starts_with(&index.drawings.len().to_string()),
            "{} drawing(s) in the index",
            index.drawings.len()
        );
        assert_eq!(
            shown(crate::t!("Imported entities")),
            crate::tf!(
                "{} entities drawn, {} source record(s) not drawn",
                entities,
                undrawn
            )
        );
        assert_eq!(
            shown(crate::t!("Failed or missing")),
            crate::tf!(
                "items did not read: {}; did not import: {}; registered but missing: {}",
                failed.len(),
                import_errors,
                index
                    .drawings
                    .iter()
                    .filter(|drawing| drawing.is_missing())
                    .count()
            )
        );
        // A row for every drawing, and a reason on each that failed.
        let table = lines
            .iter()
            .position(|(titles, _)| titles.contains(" | "))
            .expect("the check lists the drawings");
        assert_eq!(lines.len() - table - 1, index.drawings.len());
        for (name, rest) in lines[table + 1..]
            .iter()
            .filter(|(_, rest)| !rest.starts_with(" | "))
        {
            audit.line(format!(
                "    {name}: {}",
                rest.split(" | ").next().unwrap_or_default()
            ));
        }
        // The button is gone: the check ran once.
        assert!(detail_buttons(&app.pid_backup_browser, &NodeId::Root).is_empty());
        audit.shot(&app, "check-of-the-drawings");
    }

    // ── Credentials ──────────────────────────────────────────────────────
    // A manifest carries passwords: obfuscated ones, and one in clear text
    // in the export command. None may be anywhere a person, a script or a
    // log could read it. Which value turned up is not printed either.
    let credentials = manifest_credentials(&index.backup_root);
    // The check has teeth only if the values are what the file holds: each
    // must be found in the manifest's own text.
    if let Ok(bytes) = std::fs::read(index.backup_root.join("Manifest.txt")) {
        let raw = if bytes.starts_with(&[0xFF, 0xFE]) {
            let units: Vec<u16> = bytes[2..]
                .chunks_exact(2)
                .map(|pair| u16::from_le_bytes([pair[0], pair[1]]))
                .collect();
            String::from_utf16_lossy(&units)
        } else {
            String::from_utf8_lossy(&bytes).into_owned()
        };
        assert!(
            credentials.iter().all(|(_, value)| raw.contains(value.as_str())),
            "a value the walk looks for is not in Manifest.txt"
        );
    }
    let (shown, shown_nodes) = SHOWN.with(|shown| shown.borrow().clone());
    let places = [
        ("the details of a node", shown),
        ("the report", audit.report.join("\n")),
        (
            "the command line",
            app.command_line
                .history
                .iter()
                .map(|entry| entry.text.as_str())
                .collect::<Vec<_>>()
                .join("\n"),
        ),
        ("the index", format!("{index:?}")),
        ("the panel's state", format!("{:?}", app.pid_backup_browser)),
    ];
    let mut leaked: Vec<String> = Vec::new();
    for (key, value) in &credentials {
        for (place, text) in &places {
            if text.contains(value.as_str()) {
                leaked.push(format!("a {key} value in {place}"));
            }
        }
    }
    audit.line(format!(
        "== credentials: {} value(s) of Manifest.txt looked for in the details of {shown_nodes} node(s), the report, the command line, the index and the panel's state: {} found ==",
        credentials.len(),
        leaked.len()
    ));

    // ── Closing ──────────────────────────────────────────────────────────
    send(&mut app, PidBackupMsg::Close).unwrap();
    assert!(visible_rows(&app.pid_backup_browser).is_empty());
    audit.shot(&app, "closed");

    audit.line(format!("== {} node(s) without details ==", empty.len()));
    for id in empty.iter().take(20) {
        audit.line(format!("  {id}"));
    }
    audit.line(format!(
        "== {} drawing(s) whose items did not read ({} of them silently) ==",
        failed.len(),
        silent.len()
    ));
    for line in &failed {
        audit.line(format!("  {line}"));
    }
    audit.line(format!("== {} drawing(s) that did not import ==", not_imported.len()));
    for line in &not_imported {
        audit.line(format!("  {line}"));
    }
    if let Some(folder) = &audit.out {
        std::fs::write(folder.join("report.txt"), audit.report.join("\n") + "\n")
            .expect("the report is written");
    }
    // What the panel owes any backup. Which drawings a given backup's
    // files let the reader through is that backup's business: the report
    // lists them, and only a failure the user is not told about fails here.
    assert!(empty.is_empty(), "nodes without details: {empty:?}");
    assert!(
        silent.is_empty(),
        "drawings whose items failed to read without saying so: {silent:?}"
    );
    assert!(leaked.is_empty(), "credentials shown: {leaked:?}");
}
