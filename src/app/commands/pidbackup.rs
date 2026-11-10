//! PIDBACKUPOPEN -- browse a SmartPlant P&ID backup in a docked panel
//! (`io::pid_backup_browse`).
//!
//! ```text
//! PIDBACKUPOPEN                      show the panel; ask for a source when it
//!                                    holds no backup yet
//! PIDBACKUPOPEN <source> [<folder>]  show the panel and index <source> (a
//!                                    backup zip or folder), restoring into
//!                                    <folder> if given
//! ```
//!
//! The panel is the browser: its tree, the details of the selected node and
//! the double click that opens a drawing all live in
//! [`crate::ui::window::pid_backup_browser`]. This module is only the
//! application side of it -- it shows the pallet and carries out the
//! [`Effect`] the panel's pure `update` asks for: index a backup off the UI
//! thread, read a drawing's items, open a `.pid` in a tab, ask for a source.
//!
//! With FILEDIA 0 or under a script nothing is shown: the source is asked for
//! on the command line and the drawings are opened from the panel.

use super::*;
#[cfg(not(target_arch = "wasm32"))]
use crate::ui::dock::PanelId;
#[cfg(not(target_arch = "wasm32"))]
use crate::ui::window::pid_backup_browser::{self, Effect, LoadState, PidBackupMsg};
#[cfg(not(target_arch = "wasm32"))]
use std::path::Path;
#[cfg(not(target_arch = "wasm32"))]
use std::sync::Arc;

#[cfg(not(target_arch = "wasm32"))]
fn unquoted(arg: &str) -> &str {
    let quoted = arg.len() >= 2
        && ((arg.starts_with('"') && arg.ends_with('"'))
            || (arg.starts_with('\'') && arg.ends_with('\'')));
    if quoted {
        &arg[1..arg.len() - 1]
    } else {
        arg
    }
}

/// What the words after PIDBACKUPOPEN ask for.
#[cfg(not(target_arch = "wasm32"))]
#[derive(Debug, PartialEq)]
enum Request {
    Zip,
    Folder,
    Restore(PathBuf, Option<PathBuf>),
}

/// Split `<source> [<folder>]`. A line that names an existing path whole is
/// the source alone, so an unquoted path with spaces still works; otherwise
/// the words split at whitespace outside quotes.
#[cfg(not(target_arch = "wasm32"))]
fn parse_request(rest: &str, exists: impl Fn(&Path) -> bool) -> Option<Request> {
    let rest = rest.trim();
    if rest.is_empty() {
        return None;
    }
    if ["Z", "ZIP"]
        .iter()
        .any(|word| rest.eq_ignore_ascii_case(word))
    {
        return Some(Request::Zip);
    }
    if ["F", "FOLDER"]
        .iter()
        .any(|word| rest.eq_ignore_ascii_case(word))
    {
        return Some(Request::Folder);
    }
    let whole = unquoted(rest);
    if exists(Path::new(whole)) {
        return Some(Request::Restore(PathBuf::from(whole), None));
    }
    let mut words = Vec::new();
    let mut word = String::new();
    let mut quote = None;
    for ch in rest.chars() {
        match quote {
            Some(open) if ch == open => quote = None,
            Some(_) => word.push(ch),
            None if ch == '"' || ch == '\'' => quote = Some(ch),
            None if ch.is_whitespace() => {
                if !word.is_empty() {
                    words.push(std::mem::take(&mut word));
                }
            }
            None => word.push(ch),
        }
    }
    if !word.is_empty() {
        words.push(word);
    }
    let mut words = words.into_iter();
    let source = PathBuf::from(words.next()?);
    let rest: Vec<String> = words.collect();
    let target = (!rest.is_empty()).then(|| PathBuf::from(rest.join(" ")));
    Some(Request::Restore(source, target))
}

impl OpenCADStudio {
    pub(super) fn dispatch_pidbackup(&mut self, cmd: &str, i: usize) -> Option<Task<Message>> {
        let rest = cmd.strip_prefix("PIDBACKUPOPEN")?;
        if !(rest.is_empty() || rest.starts_with(char::is_whitespace)) {
            return None;
        }
        Some(self.start_pid_backup(cmd, rest, i))
    }

    #[cfg(target_arch = "wasm32")]
    fn start_pid_backup(&mut self, _cmd: &str, _rest: &str, _i: usize) -> Task<Message> {
        self.command_line.push_error(
            crate::t!(
                "PIDBACKUPOPEN: opening a SmartPlant backup is not available in the web build."
            )
            .as_ref(),
        );
        Task::none()
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn start_pid_backup(&mut self, cmd: &str, rest: &str, i: usize) -> Task<Message> {
        // The start page draws no dock, so a panel opened there would have
        // nowhere to appear. Open a blank drawing first, the path QNEW takes,
        // and the browser docks into the drawing window that replaces it.
        if self.tabs.get(i).is_some_and(|tab| tab.is_start) {
            let _ = self.update(Message::TabNew);
        }
        let i = self.active_tab;
        let on_command_line = self.file_prompts_on_command_line();
        self.show_pid_backup_panel();
        if rest.trim().is_empty() {
            // A backup already in the panel is what the user asked to see.
            if matches!(
                self.pid_backup_browser.load,
                LoadState::Ready(_) | LoadState::Loading(_)
            ) {
                return Task::none();
            }
            // Bare verb: ask. Enter on the prompt comes back with a trailing
            // space and takes the default.
            if cmd.trim_end() == cmd {
                use crate::command::FreeTextValuePromptCommand;
                let c = if on_command_line {
                    FreeTextValuePromptCommand::new(
                        "PIDBACKUPOPEN",
                        "PIDBACKUPOPEN  Backup zip file or folder:",
                    )
                } else {
                    FreeTextValuePromptCommand::new(
                        "PIDBACKUPOPEN",
                        "PIDBACKUPOPEN  Backup source [Zip/Folder] <Zip>:",
                    )
                };
                self.command_line.push_info(&c.prompt());
                self.tabs[i].active_cmd = Some(Box::new(c));
                return self.finish_dispatch(cmd);
            }
            if on_command_line {
                return Task::none();
            }
            return self.on_pid_backup(PidBackupMsg::Open);
        }
        match parse_request(rest, Path::exists) {
            Some(Request::Restore(source, target)) => self.load_pid_backup(source, target),
            Some(Request::Zip | Request::Folder) if on_command_line => {
                // No dialogs here: the keyword is not a path either.
                self.command_line
                    .push_info(crate::t!("PIDBACKUPOPEN  Backup zip file or folder:").as_ref());
                Task::none()
            }
            Some(Request::Zip) => self.pick_backup_source(false),
            Some(Request::Folder) => self.pick_backup_source(true),
            None => Task::none(),
        }
    }

    /// Show the pallet where the layout has it, docking it on the right the
    /// first time. Unlike PIDPALETTE this never closes it: the command means
    /// "show the browser", not "toggle it".
    #[cfg(not(target_arch = "wasm32"))]
    pub(in crate::app) fn show_pid_backup_panel(&mut self) {
        if self.dock.location(PanelId::PidBackup).is_none() {
            self.dock
                .dock(PanelId::PidBackup, crate::app::config::DockSide::Right, 0);
        }
        self.show_pid_backup = true;
        self.dock_reveal(PanelId::PidBackup);
    }

    /// A panel message: apply it through the panel's pure state machine and
    /// carry out the effect it asks for.
    #[cfg(not(target_arch = "wasm32"))]
    pub(in crate::app) fn on_pid_backup(&mut self, msg: PidBackupMsg) -> Task<Message> {
        // A finished load, and a source the user just picked, both mean the
        // browser has something to show: reveal it, so a load that finished
        // after the pallet was closed cannot land invisibly.
        if matches!(
            &msg,
            PidBackupMsg::Loaded(_) | PidBackupMsg::SourcePicked(Some(_))
        ) {
            self.show_pid_backup_panel();
        }
        // The command line reports what a finished load found, and a read
        // that failed -- the backup's or one drawing's items; the panel's own
        // state is the answer to everything else.
        let (loaded, failure) = match &msg {
            PidBackupMsg::Loaded(Ok(_)) => (true, None),
            PidBackupMsg::Loaded(Err(error))
            | PidBackupMsg::ItemsLoaded(_, Err(error)) => (false, Some(error.clone())),
            _ => (false, None),
        };
        let effect = pid_backup_browser::update(&mut self.pid_backup_browser, msg);
        if let Some(error) = failure {
            self.command_line
                .push_error(crate::tf!("PIDBACKUPOPEN: {}", error).as_ref());
        } else if loaded {
            if let Some(index) = self.pid_backup_browser.index() {
                self.command_line.push_output(
                    crate::tf!(
                        "PIDBACKUPOPEN: {} drawing(s), {} warning(s)",
                        index.drawings.len(),
                        index.warnings.len()
                    )
                    .as_ref(),
                );
                if index.read_only {
                    self.command_line.push_info(
                        crate::t!("PIDBACKUPOPEN: the folder is used in place (read-only).")
                            .as_ref(),
                    );
                }
            }
        }
        self.run_pid_backup_effect(effect)
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn run_pid_backup_effect(&mut self, effect: Effect) -> Task<Message> {
        match effect {
            Effect::None => Task::none(),
            Effect::LoadItems(drawing, path) => Task::perform(
                crate::io::pid_backup_browse::load_items_in_background(path),
                move |result| {
                    Message::PidBackup(PidBackupMsg::ItemsLoaded(drawing, result.map(Arc::new)))
                },
            ),
            Effect::OpenDrawing(path) => Task::done(Message::OpenRecent(path)),
            // A picked source, a refresh: both rebuild the index from a path.
            // The panel remembers the source it was given, so a refresh keeps
            // the backup it was showing.
            Effect::Load(source) | Effect::Refresh(source) => {
                self.load_pid_backup(source, None)
            }
            Effect::PickSource => self.pick_backup_source(false),
        }
    }

    /// Put the panel in its loading state and index `source` off the UI
    /// thread. `target` is the restore folder, when the command named one.
    #[cfg(not(target_arch = "wasm32"))]
    fn load_pid_backup(&mut self, source: PathBuf, target: Option<PathBuf>) -> Task<Message> {
        // A relative restore folder sits beside the backup.
        let target = target.map(|target| match source.parent() {
            Some(parent) if target.is_relative() => parent.join(target),
            _ => target,
        });
        // The state machine owns the loading state, so the panel shows the
        // source it is reading without the command layer writing it twice.
        if !matches!(
            self.pid_backup_browser.load,
            LoadState::Loading(ref current) if *current == source
        ) {
            let _ = pid_backup_browser::update(
                &mut self.pid_backup_browser,
                PidBackupMsg::SourcePicked(Some(source.clone())),
            );
        }
        self.command_line
            .push_output(crate::tf!("PIDBACKUPOPEN: restoring {} ...", source.display()).as_ref());
        Task::perform(
            crate::io::pid_backup_browse::build_index_in_background(source, target),
            |result| Message::PidBackup(PidBackupMsg::Loaded(result.map(Arc::new))),
        )
    }

    /// The source dialog: a backup zip, or with `folder` a backup folder.
    #[cfg(not(target_arch = "wasm32"))]
    fn pick_backup_source(&mut self, folder: bool) -> Task<Message> {
        Task::perform(pick_backup_source_async(folder), |source| {
            Message::PidBackup(PidBackupMsg::SourcePicked(source))
        })
    }
}

/// The dialog for the backup itself, as the picker task the caller awaits.
#[cfg(not(target_arch = "wasm32"))]
async fn pick_backup_source_async(folder: bool) -> Option<PathBuf> {
    let dialog = crate::sys::file_dialog();
    let handle = if folder {
        dialog
            .set_title(crate::t!("Select P&ID Backup Folder").as_ref())
            .pick_folder()
            .await
    } else {
        dialog
            .set_title(crate::t!("Select P&ID Backup").as_ref())
            .add_filter(
                crate::t!("SmartPlant backup (*.zip)").as_ref(),
                &["zip", "ZIP"],
            )
            .pick_file()
            .await
    };
    handle.map(|handle| crate::sys::handle_path(&handle))
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod tests {
    use super::*;
    use crate::ui::window::pid_backup_browser::{NodeId, Row};
    use crate::ui::command_line::EntryKind;

    fn none(_: &Path) -> bool {
        false
    }

    #[test]
    fn keywords_pick_the_dialog() {
        assert_eq!(parse_request("z", none), Some(Request::Zip));
        assert_eq!(parse_request(" Zip ", none), Some(Request::Zip));
        assert_eq!(parse_request("f", none), Some(Request::Folder));
        assert_eq!(parse_request("FOLDER", none), Some(Request::Folder));
        assert_eq!(parse_request("  ", none), None);
    }

    #[test]
    fn a_source_and_a_restore_folder_split_at_whitespace_outside_quotes() {
        assert_eq!(
            parse_request(r#""D:/my backups/a.zip" D:/out"#, none),
            Some(Request::Restore(
                PathBuf::from("D:/my backups/a.zip"),
                Some(PathBuf::from("D:/out"))
            ))
        );
        assert_eq!(
            parse_request("D:/a.zip", none),
            Some(Request::Restore(PathBuf::from("D:/a.zip"), None))
        );
    }

    #[test]
    fn an_existing_path_with_spaces_is_the_source_alone() {
        let exists = |path: &Path| path == Path::new("D:/my backups/plant");
        assert_eq!(
            parse_request("D:/my backups/plant", exists),
            Some(Request::Restore(PathBuf::from("D:/my backups/plant"), None))
        );
    }

    // ── Application-level wiring ─────────────────────────────────────────
    //
    // The panel's pure functions are tested in its own module; these run the
    // real app so the message routing, the dock visibility and the command
    // line report are covered too.

    use std::sync::Arc;

    use crate::io::pid_backup_browse::{
        BackupFile, BackupIndex, DrawingEntry, FileKind, GroupNode, ManifestSummary, RefNode,
    };

    /// An app with a drawing open, so the dock has a window to appear in.
    fn app() -> OpenCADStudio {
        let mut app = OpenCADStudio::new_for_test();
        let _ = app.automation_op(r#"{"op":"new"}"#);
        app
    }

    /// A backup index with two plant groups, two drawings and one warning.
    fn index() -> Arc<BackupIndex> {
        let mut plant = GroupNode {
            name: "00".to_string(),
            path: "SQPlant / 00".to_string(),
            children: Vec::new(),
            drawings: vec![0, 1],
        };
        plant.children.push(GroupNode {
            name: "01".to_string(),
            path: "SQPlant / 00 / 01".to_string(),
            children: Vec::new(),
            drawings: Vec::new(),
        });
        let root = GroupNode {
            name: "SQPlant".to_string(),
            path: "SQPlant".to_string(),
            children: vec![plant],
            drawings: Vec::new(),
        };
        let drawing = |name: &str, file: Option<&str>| DrawingEntry {
            name: name.to_string(),
            pid_path: file.map(PathBuf::from),
            group_path: "SQPlant / 00".to_string(),
            meta: vec![
                ("id".to_string(), "7".to_string()),
                ("drawing_number".to_string(), name.to_string()),
            ],
        };
        Arc::new(BackupIndex {
            source: PathBuf::from("D:/backups/plant.zip"),
            restored: PathBuf::from("D:/tmp/restore"),
            backup_root: PathBuf::from("D:/tmp/restore"),
            read_only: false,
            manifest: Some(ManifestSummary {
                name: Some("Plant backup".to_string()),
                ..ManifestSummary::default()
            }),
            files: vec![BackupFile {
                name: "Manifest.txt".to_string(),
                kind: FileKind::Manifest,
                format: "Manifest".to_string(),
                size: 12,
                members: Vec::new(),
                version_line: None,
                missing: false,
            }],
            plant_groups: vec![root],
            drawings: vec![
                drawing("A-101", Some("D:/tmp/restore/00/00/a.pid")),
                drawing("B-202", None),
            ],
            ref_tree: RefNode::default(),
            warnings: vec!["no usable Export.dmp".to_string()],
        })
    }

    /// The rows the panel would draw, as `(label, disabled)` pairs.
    fn row_labels(app: &OpenCADStudio) -> Vec<(String, bool)> {
        pid_backup_browser::visible_rows(&app.pid_backup_browser)
            .into_iter()
            .map(|row: Row| (row.label, row.disabled))
            .collect()
    }

    #[test]
    fn pidbackupopen_shows_the_panel_without_closing_it_again() {
        let mut app = app();
        assert!(!app.dock_panel_visible(PanelId::PidBackup));
        let _ = app.dispatch_command("PIDBACKUPOPEN");
        assert!(
            app.dock_panel_visible(PanelId::PidBackup),
            "PIDBACKUPOPEN 打开面板"
        );
        assert_eq!(
            app.dock.location(PanelId::PidBackup).map(|(side, _)| side),
            Some(crate::app::config::DockSide::Right),
            "首次打开停靠在右侧"
        );
        // The command shows the browser; it is not a toggle, so running it
        // again leaves the panel where it is.
        let _ = app.dispatch_command("PIDBACKUPOPEN");
        assert!(app.dock_panel_visible(PanelId::PidBackup));
    }

    #[test]
    fn a_loaded_backup_becomes_visible_and_is_reported() {
        let mut app = app();
        let start = app.command_line.history.len();
        let _ = app.on_pid_backup(PidBackupMsg::Loaded(Ok(index())));
        assert!(app.dock_panel_visible(PanelId::PidBackup), "加载后面板可见");
        assert!(matches!(
            app.pid_backup_browser.load,
            LoadState::Ready(_)
        ));
        let report: Vec<&str> = app.command_line.history[start..]
            .iter()
            .map(|entry| entry.text.as_str())
            .collect();
        assert!(
            report.iter().any(|line| line.contains('2')
                && line.contains('1')
                && line.contains("drawing")),
            "命令行报告图纸数与警告数：{report:?}"
        );
        // The tree starts on the backup itself and its three groups.
        let rows = row_labels(&app);
        assert_eq!(rows.len(), 4, "{rows:?}");
        assert_eq!(rows[0].0, "Plant backup");
        assert!(!rows[0].1);
    }

    #[test]
    fn a_failed_load_is_reported_as_an_error() {
        let mut app = app();
        let _ = app.on_pid_backup(PidBackupMsg::Loaded(Err("no Manifest.txt".to_string())));
        let last = app.command_line.history.last().expect("一条历史");
        assert_eq!(last.kind, EntryKind::Error);
        assert!(last.text.contains("no Manifest.txt"), "{}", last.text);
        assert!(matches!(
            app.pid_backup_browser.load,
            LoadState::Idle
        ));
    }

    #[test]
    fn a_drawing_whose_items_cannot_be_read_is_reported_as_an_error() {
        let mut app = app();
        let _ = app.on_pid_backup(PidBackupMsg::Loaded(Ok(index())));
        let _ = app.on_pid_backup(PidBackupMsg::ItemsLoaded(
            0,
            Err("cannot read a.pid: no DrawingNo".to_string()),
        ));
        let last = app.command_line.history.last().expect("一条历史");
        assert_eq!(last.kind, EntryKind::Error);
        assert!(last.text.contains("no DrawingNo"), "{}", last.text);
        // One drawing's failure is not the backup's: the tree stays.
        assert!(matches!(
            app.pid_backup_browser.load,
            LoadState::Ready(_)
        ));
        assert_eq!(row_labels(&app).len(), 4);
    }

    #[test]
    fn the_start_page_opens_a_blank_drawing_before_showing_the_panel() {
        // A fresh app is on the welcome tab, which draws no dock.
        let mut app = OpenCADStudio::new_for_test();
        assert!(app.tabs[app.active_tab].is_start, "需要一个开始页");
        let tabs = app.tabs.len();
        let _ = app.dispatch_command("PIDBACKUPOPEN D:/backups/plant.zip");
        assert_eq!(app.tabs.len(), tabs + 1, "开始页执行新建了一个标签页");
        assert!(
            !app.tabs[app.active_tab].is_start,
            "新标签是空白图，不是开始页"
        );
        assert!(
            app.tabs[app.active_tab].scene.document.entities().count() == 0,
            "新图是空白图"
        );
        assert!(app.dock_panel_visible(PanelId::PidBackup), "面板可见");
        assert!(matches!(
            app.pid_backup_browser.load,
            LoadState::Loading(_)
        ));
    }

    #[test]
    fn activating_a_drawing_asks_for_the_drawing_to_open() {
        let mut app = app();
        let _ = app.on_pid_backup(PidBackupMsg::Loaded(Ok(index())));
        let task = app.on_pid_backup(PidBackupMsg::Activate(NodeId::Drawing(0)));
        app.drive_headless_task(task).unwrap();
        // `OpenRecent` of a file that is not on disk reports it, which proves
        // the message reached the open path with the drawing's own path.
        let report: Vec<&str> = app.command_line.history.iter().map(|e| e.text.as_str()).collect();
        assert!(
            report.iter().any(|line| line.contains("a.pid")),
            "OpenRecent 收到了图纸路径：{report:?}"
        );
        // A drawing with no file has nothing to open.
        let start = app.command_line.history.len();
        let task = app.on_pid_backup(PidBackupMsg::Activate(NodeId::Drawing(1)));
        app.drive_headless_task(task).unwrap();
        assert_eq!(app.command_line.history.len(), start, "缺文件图纸不可打开");
    }

    #[test]
    fn filedia_zero_asks_on_the_command_line_instead_of_a_dialog() {
        let mut app = app();
        crate::sys::set_file_dia(false);
        let start = app.command_line.history.len();
        let task = app.dispatch_command("PIDBACKUPOPEN");
        // Driving the task proves no dialog was scheduled: a dialog task
        // would never resolve without a window, and the prompt's focus is
        // the only widget action it carries.
        app.drive_headless_task(task).unwrap();
        let added: Vec<&str> = app.command_line.history[start..]
            .iter()
            .map(|entry| entry.text.as_str())
            .collect();
        assert!(
            added.iter().any(|line| line.contains("zip")
                || line.contains("folder")
                || line.contains("Zip")),
            "命令行要路径：{added:?}"
        );
        assert!(app.dock_panel_visible(PanelId::PidBackup), "面板仍然显示");
        assert!(
            app.tabs[app.active_tab]
                .active_cmd
                .as_ref()
                .is_some_and(|cmd| cmd.name() == "PIDBACKUPOPEN"),
            "命令行提示接管了输入"
        );
        crate::sys::set_file_dia(true);
    }

    #[test]
    fn a_path_loads_the_backup_without_a_dialog() {
        let mut app = app();
        let task = app.dispatch_command("PIDBACKUPOPEN D:/backups/plant.zip");
        // The index runs on a worker thread, so the command returns a task
        // rather than a widget; either way no file dialog is involved.
        assert!(app.dock_panel_visible(PanelId::PidBackup));
        assert!(matches!(
            app.pid_backup_browser.load,
            LoadState::Loading(_)
        ));
        drop(task);
    }

    #[test]
    fn closing_the_backup_returns_the_panel_to_its_hint_and_reloads() {
        let mut app = app();
        let _ = app.on_pid_backup(PidBackupMsg::Loaded(Ok(index())));
        let _ = app.on_pid_backup(PidBackupMsg::Close);
        assert!(matches!(
            app.pid_backup_browser.load,
            LoadState::Idle
        ));
        assert!(
            pid_backup_browser::visible_rows(&app.pid_backup_browser).is_empty(),
            "关闭后树为空"
        );
        // PIDBACKUPOPEN <source> loads again.
        let _ = app.dispatch_command("PIDBACKUPOPEN D:/backups/plant.zip");
        assert!(matches!(
            app.pid_backup_browser.load,
            LoadState::Loading(_)
        ));
        let _ = app.on_pid_backup(PidBackupMsg::Loaded(Ok(index())));
        assert_eq!(row_labels(&app).len(), 4, "树重新出现");
    }

    #[test]
    fn the_application_menu_still_opens_the_browser() {
        // The menu entry is unchanged: "Open P&ID Backup..." runs
        // PIDBACKUPOPEN, which is the command that opens the panel.
        assert!(crate::command::all_registered_command_names().contains(&"PIDBACKUPOPEN"));
        let mut app = app();
        let _ = app.dispatch_command("PIDBACKUPOPEN");
        assert!(app.dock_panel_visible(PanelId::PidBackup));
    }
}

/// A walk over a real backup through the application; run with `--ignored`.
#[cfg(all(test, not(target_arch = "wasm32")))]
mod audit;
