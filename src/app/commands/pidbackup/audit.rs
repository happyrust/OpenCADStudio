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
//!   alone.
//!
//! A folder source is restored into the cache under the temp folder, which
//! is where the 100 MB of a plant's drawings go: point `TEMP` at a drive
//! with room for them.

use super::*;
use crate::io::pid_backup_browse::{BackupIndex, GroupNode, RefNode};
use crate::ui::window::pid_backup_browser::{detail_rows, visible_rows, ItemsState, NodeId};
use std::collections::BTreeMap;
use std::time::{Duration, Instant};

/// The plant test backup the two reading tests of `io::pid_backup*` use.
const DEFAULT_BACKUP: &str = r"D:\work\plant-code\cad\pid-test-data";

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
    rows
}

fn millis(duration: Duration) -> String {
    format!("{:.1} ms", duration.as_secs_f64() * 1000.0)
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
    for dir in &index.ref_tree.dirs {
        audit.line(format!("  {}: {} files", dir.name, dir.file_count()));
    }
    audit.line(format!("  by extension: {by_extension:?}"));
    if let Some(first) = dirs.first() {
        send(app, PidBackupMsg::Toggle(NodeId::RefDir(first.clone()))).unwrap();
        send(app, PidBackupMsg::Select(NodeId::RefDir(first.clone()))).unwrap();
    }
    audit.shot(app, "ref-open");
    send(app, PidBackupMsg::Toggle(NodeId::Ref)).unwrap();
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
    let mut total_items = 0;
    let mut total_attributes = 0;
    let mut without_attributes = 0;
    let mut failed: Vec<String> = Vec::new();
    let mut silent: Vec<String> = Vec::new();
    let mut slowest = (Duration::ZERO, String::new());
    let mut item_time = Duration::ZERO;
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
                        if item.attributes.is_empty() {
                            without_attributes += 1;
                        }
                    }
                }
                total_items += items.item_count;
                total_attributes += attributes;
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
    audit.line(format!("  item types across the backup: {item_types:?}"));
    audit.shot(&app, "drawing-items-open");
    if let Some(ItemsState::Ready(items)) = app.pid_backup_browser.items.get(&0).cloned() {
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
    let mut import_time = Duration::ZERO;
    let mut slowest_import = (Duration::ZERO, String::new());
    let mut entities = 0;
    let mut undrawn = 0;
    audit.line("== import of every drawing (name | entities | decoded | not drawn | sheet layers | time) ==");
    for drawing in &index.drawings {
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
            Err(error) => not_imported.push(format!("{}: {error}", drawing.name)),
        }
    }
    audit.line(format!(
        "  {entities} entities drawn, {undrawn} source records not drawn; all imported in {}, slowest {} ({})",
        millis(import_time),
        slowest_import.1,
        millis(slowest_import.0)
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
}
