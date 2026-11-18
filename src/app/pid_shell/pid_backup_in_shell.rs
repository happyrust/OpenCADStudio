//! `PIDBACKUPOPEN` in the module: a SmartPlant P&ID backup opened from the
//! command line of the editor screen.
//!
//! The backup browser is a dock panel of the general interface, and the
//! module draws the general interface's drawing area, its docks with it, in
//! a frame of its own (`rebase`). What is checked here is that the panel
//! comes up beside the drawing and inside that frame, that the module's own
//! parts stay where they were, that a drawing opened from the panel asks for
//! the file the restore made of it, and that a source that is no backup
//! leaves the module in the state it leaves a `--classic` window in.
//!
//! The backups are made up here, the way the tests of
//! `app/commands/pidbackup.rs` and `io/pid_backup_browse.rs` make theirs,
//! and what a test unpacks goes into a folder of the test's own.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use iced::{Point, Rectangle, Size};

use super::*;
use crate::app::config::DockSide;
use crate::cli::GuiConfig;
use crate::ui::command_line::EntryKind;
use crate::ui::dock::{DockMsg, PanelId};
use crate::ui::window::pid_backup_browser::{self, LoadState, NodeId, PidBackupMsg};

/// The design's frame.
const WINDOW: Size = Size::new(1600.0, 1000.0);

/// The drawings of the made-up backup: the folder of the archive each is
/// filed in, and its name.
const DRAWINGS: [(&str, &str); 3] = [("00/00", "A-101"), ("00/00", "B-202"), ("01/01", "C-303")];

/// What reports where the drawing itself is.
const VIEWPORT: &str = crate::app::view::VIEWPORT_CAPTURE_BOUNDS_ID;

// ── An application in a window ───────────────────────────────────────────

/// The application as a launch leaves it once the startup prompts have been
/// answered, in a window of the design's size: in the module, or with
/// `--classic` and a drawing.
fn launched(classic: bool) -> OpenCADStudio {
    let mut app = OpenCADStudio::new_for_test();
    app.default_assoc_prompted = true;
    app.donation_prompt_version = env!("OCS_APP_VERSION").to_owned();
    let cfg = GuiConfig {
        classic,
        new: classic,
        ..GuiConfig::default()
    };
    let first = app.open_first_screen(&cfg);
    app.drive_headless_task(first)
        .expect("the first screen opens without a window");
    let _ = app.update(AppMessage::WindowResized(WINDOW.width, WINDOW.height));
    app
}

fn headless() -> iced::Renderer {
    use iced_test::core::renderer;
    iced_test::futures::futures::executor::block_on(<iced::Renderer as renderer::Headless>::new(
        renderer::Settings::default(),
        std::env::var("ICED_TEST_BACKEND").ok().as_deref(),
    ))
    .expect("headless renderer")
}

/// Draw the window once; where every reporting widget is, in window
/// coordinates.
fn drawn(app: &OpenCADStudio) -> BTreeMap<String, Rectangle> {
    use iced_test::core::{mouse, renderer, shell, window as core_window};
    use iced_test::runtime::user_interface::{Cache, UserInterface};
    crate::ui::wrap_bar::forget_bounds("");
    let mut renderer = headless();
    let mut ui = UserInterface::build(app.view_main(), WINDOW, Cache::default(), &mut renderer);
    let _ = ui.update(
        &core_window::Headless,
        &shell::Waker::noop(),
        &[iced::Event::Window(iced::window::Event::RedrawRequested(
            iced::time::Instant::now(),
        ))],
        mouse::Cursor::Unavailable,
        &mut renderer,
        &mut Vec::new(),
    );
    ui.draw(
        &mut renderer,
        &app.active_theme,
        &renderer::Style {
            text_color: iced::Color::BLACK,
        },
        mouse::Cursor::Unavailable,
    );
    crate::ui::wrap_bar::recorded_bounds().into_iter().collect()
}

/// Where the widget reporting as `id` is.
fn widget_at(parts: &BTreeMap<String, Rectangle>, id: &str) -> Rectangle {
    *parts
        .get(id)
        .unwrap_or_else(|| panic!("{id} is drawn: {:?}", parts.keys().collect::<Vec<_>>()))
}

/// Where the part of the editor called `name` is.
fn part_at(parts: &BTreeMap<String, Rectangle>, name: &str) -> Rectangle {
    widget_at(parts, &editor::part(name))
}

/// A click of the left button at `place`: what the widgets publish.
/// Nothing is given to the application.
fn press(app: &OpenCADStudio, place: Point) -> Vec<AppMessage> {
    use iced_test::core::{mouse, shell, window as core_window};
    use iced_test::runtime::user_interface::{Cache, UserInterface};
    let mut renderer = headless();
    let mut cache = Cache::default();
    let mut published = Vec::new();
    for event in [
        mouse::Event::CursorMoved { position: place },
        mouse::Event::ButtonPressed(mouse::Button::Left),
        mouse::Event::ButtonReleased(mouse::Button::Left),
    ] {
        let mut ui = UserInterface::build(app.view_main(), WINDOW, cache, &mut renderer);
        let _ = ui.update(
            &core_window::Headless,
            &shell::Waker::noop(),
            &[iced::Event::Mouse(event)],
            mouse::Cursor::Available(place),
            &mut renderer,
            &mut published,
        );
        cache = ui.into_cache();
    }
    published
}

/// Give the application `message` and let what it starts run to its end.
fn give(app: &mut OpenCADStudio, message: AppMessage) {
    let task = app.update(message);
    app.drive_headless_task(task)
        .expect("what the message starts runs without a window");
}

/// Run `task` as the runtime does, the application being given every
/// message it publishes but the requests to open a file: those are
/// returned, unanswered.
fn opens_asked_for(app: &mut OpenCADStudio, task: Task<AppMessage>) -> Vec<PathBuf> {
    use iced::futures::StreamExt;
    let mut asked = Vec::new();
    let mut streams = Vec::new();
    streams.extend(iced_runtime::task::into_stream(task));
    while let Some(stream) = streams.last_mut() {
        match iced::futures::executor::block_on(stream.next()) {
            Some(iced_runtime::Action::Output(AppMessage::OpenRecent(path))) => asked.push(path),
            Some(iced_runtime::Action::Output(message)) => {
                let next = app.update(message);
                streams.extend(iced_runtime::task::into_stream(next));
            }
            Some(_) => {}
            None => {
                streams.pop();
            }
        }
    }
    asked
}

// ── A backup, and the command line ───────────────────────────────────────

/// A folder of one test's own under the temp folder. It goes when the test
/// is over, however the test ended.
struct Scratch(PathBuf);

impl Scratch {
    fn new(name: &str) -> Self {
        let root = std::env::temp_dir().join(format!(
            "ocs-pid-backup-in-shell-{name}-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).expect("a scratch folder");
        Self(root)
    }

    fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// A backup folder as small as one gets: the manifest, and [`DRAWINGS`] in
/// the archive a backup files its drawings in. There is no dump, so the
/// drawings are listed by the folders they are filed in. A drawing's file
/// holds its name, which is how a test tells whose file it was given.
fn backup_folder(scratch: &Path) -> PathBuf {
    use std::io::Write;
    let backup = scratch.join("backup");
    std::fs::create_dir_all(&backup).expect("the backup folder");
    std::fs::write(
        backup.join("Manifest.txt"),
        "BackupType<<|>>2\nVersion<<|>>7.02\nName<<|>>SQPlant\n",
    )
    .expect("the manifest");
    let archive = std::fs::File::create(backup.join("PlantData~2~711.zip")).expect("the archive");
    let mut writer = zip::ZipWriter::new(archive);
    let options = zip::write::SimpleFileOptions::default();
    for (folder, name) in DRAWINGS {
        writer
            .start_file(format!("{folder}/{name}.pid"), options)
            .expect("a drawing of the archive");
        writer.write_all(name.as_bytes()).expect("its bytes");
    }
    writer.finish().expect("the archive is written");
    backup
}

/// Have `app` keep what it unpacks in the scratch folder, as the setting
/// for it would (`PIDBACKUPOPEN Cache=<folder>`), and not in the default
/// folder, which is not the test's to fill. The folder it will use.
fn restores_into(app: &mut OpenCADStudio, scratch: &Path) -> PathBuf {
    let cache = scratch.join("restores");
    pid_backup_browser::set_cache_folder(&mut app.pid_backup_browser, Some(cache.clone()));
    cache
}

/// The line that opens `source`, as it is typed.
fn open_line(source: &Path) -> String {
    format!("PIDBACKUPOPEN \"{}\"", source.display())
}

/// What the command line has said since entry `from`.
fn said_since(app: &OpenCADStudio, from: usize) -> Vec<(EntryKind, String)> {
    app.command_line.history[from..]
        .iter()
        .filter(|entry| entry.kind != EntryKind::Command)
        .map(|entry| (entry.kind.clone(), entry.text.clone()))
        .collect()
}

/// Type `line` on the command line and let what it starts run to its end:
/// what the command line said.
fn say(app: &mut OpenCADStudio, line: &str) -> Vec<(EntryKind, String)> {
    let from = app.command_line.history.len();
    let task = app.run_command_line(line);
    app.drive_headless_task(task)
        .expect("a backup is read without a window");
    said_since(app, from)
}

/// The errors among what the command line said.
fn errors(said: &[(EntryKind, String)]) -> Vec<&str> {
    said.iter()
        .filter(|(kind, _)| *kind == EntryKind::Error)
        .map(|(_, text)| text.as_str())
        .collect()
}

/// What the panel holds and where it is: all that tells one state of it
/// from another without drawing it.
#[derive(Debug, PartialEq)]
struct PanelState {
    visible: bool,
    side: Option<DockSide>,
    load: String,
    error: Option<String>,
    rows: Vec<(String, bool)>,
    selected: Option<NodeId>,
}

fn panel_state(app: &OpenCADStudio) -> PanelState {
    let browser = &app.pid_backup_browser;
    PanelState {
        visible: app.dock_panel_visible(PanelId::PidBackup),
        side: app.dock.location(PanelId::PidBackup).map(|(side, _)| side),
        load: match &browser.load {
            LoadState::Idle => "idle".to_owned(),
            LoadState::Loading(source) => format!("loading {}", source.display()),
            LoadState::Ready(index) => format!(
                "{} drawing(s), {} warning(s), used in place: {}",
                index.drawings.len(),
                index.warnings.len(),
                index.read_only
            ),
        },
        error: browser.error.clone(),
        rows: pid_backup_browser::visible_rows(browser)
            .into_iter()
            .map(|row| (row.label, row.disabled))
            .collect(),
        selected: browser.selected.clone(),
    }
}

// ── Opening a backup ─────────────────────────────────────────────────────

/// `PIDBACKUPOPEN <folder>` in the module: the panel comes up on the right
/// of the drawing, inside the drawing's frame, and the module stays on its
/// editor; the index holds the drawings of the backup, and the command line
/// says how many there are.
#[test]
fn a_backup_folder_opened_in_the_module_is_listed_in_the_panel_beside_the_drawing() {
    let _one = crate::headless_ui_lock();
    let scratch = Scratch::new("listed");
    let backup = backup_folder(scratch.path());
    let mut app = launched(false);
    let cache = restores_into(&mut app, scratch.path());
    assert_eq!(app.pid_shell.current(), Some(PidScreen::Editor));
    assert!(!app.dock_panel_visible(PanelId::PidBackup));
    let before = drawn(&app);
    assert!(!before.contains_key(pid_backup_browser::PANEL_BOUNDS_ID));
    assert_eq!(widget_at(&before, VIEWPORT), part_at(&before, "drawing"));

    let said = say(&mut app, &open_line(&backup));

    assert!(app.dock_panel_visible(PanelId::PidBackup), "{said:?}");
    assert_eq!(
        app.dock.location(PanelId::PidBackup).map(|(side, _)| side),
        Some(DockSide::Right)
    );
    assert_eq!(app.pid_shell.current(), Some(PidScreen::Editor));
    // The index holds the drawings that were made up, unpacked into the
    // folder restores are kept in.
    let index = app
        .pid_backup_browser
        .index()
        .unwrap_or_else(|| panic!("the backup is loaded: {said:?}"))
        .clone();
    assert_eq!(index.drawings.len(), DRAWINGS.len());
    let mut names: Vec<&str> = index
        .drawings
        .iter()
        .map(|drawing| drawing.name.as_str())
        .collect();
    names.sort_unstable();
    assert_eq!(names, DRAWINGS.map(|(_, name)| name));
    assert_eq!(index.restored.parent(), Some(cache.as_path()));
    // The command line says how many there are, once, and reports no error.
    let report = crate::tf!(
        "PIDBACKUPOPEN: {} drawing(s), {} warning(s)",
        DRAWINGS.len(),
        index.warnings.len()
    )
    .into_owned();
    let reports = said
        .iter()
        .filter(|(kind, text)| *kind == EntryKind::Output && *text == report)
        .count();
    assert_eq!(reports, 1, "{report}: {said:?}");
    assert!(errors(&said).is_empty(), "{said:?}");

    // Drawn: the panel is in the drawing's frame and has a size to read it
    // at; the drawing gave up the room it takes and lies to its left; the
    // module's inspector is still to the right of both.
    let parts = drawn(&app);
    let frame = part_at(&parts, "drawing");
    let panel = widget_at(&parts, pid_backup_browser::PANEL_BOUNDS_ID);
    let canvas = widget_at(&parts, VIEWPORT);
    let inspector = part_at(&parts, "inspector");
    assert!(
        panel.width >= 200.0 && panel.height >= frame.height / 2.0,
        "{panel:?}"
    );
    assert!(
        panel.x >= frame.x
            && panel.y >= frame.y
            && panel.x + panel.width <= frame.x + frame.width
            && panel.y + panel.height <= frame.y + frame.height,
        "the panel {panel:?} is in the drawing's frame {frame:?}"
    );
    assert!(
        canvas.width > 0.0 && canvas.x + canvas.width <= panel.x,
        "the drawing {canvas:?} is left of the panel {panel:?}"
    );
    assert!(
        frame.width - canvas.width >= panel.width,
        "the drawing {canvas:?} gave up the room of the panel {panel:?} in {frame:?}"
    );
    assert!(
        panel.x + panel.width <= inspector.x,
        "the panel {panel:?} is left of the inspector {inspector:?}"
    );
}

/// With the panel open the module's own parts are where they were, the tab
/// of the drawing among them, and the Properties pallet stays out of the
/// module though it is wanted; the panel's close button takes the panel
/// away and gives the drawing its room back, the backup staying loaded.
#[test]
fn the_panel_leaves_the_modules_own_parts_where_they_were_and_closes() {
    let _one = crate::headless_ui_lock();
    let scratch = Scratch::new("parts");
    let backup = backup_folder(scratch.path());
    let mut app = launched(false);
    restores_into(&mut app, scratch.path());
    let own = ["toolbar", "palette", "tabs", "drawing", "inspector"];
    let tab = editor::part(&format!("tab:{}", app.tabs[app.active_tab].id));
    let before = drawn(&app);
    assert!(
        before.contains_key(&tab),
        "the drawing has its tab in the strip: {:?}",
        before.keys().collect::<Vec<_>>()
    );

    say(&mut app, &open_line(&backup));
    assert!(app.dock_panel_visible(PanelId::PidBackup));

    // The Properties pallet, wanted here and in a `--classic` window that
    // opened the same backup: docked there, and not beside the module's
    // own inspector.
    let mut general = launched(true);
    restores_into(&mut general, scratch.path());
    say(&mut general, &open_line(&backup));
    assert!(general.dock_panel_visible(PanelId::PidBackup));
    general.show_properties = true;
    app.show_properties = true;
    assert!(general.dock_panel_visible(PanelId::Properties));
    assert!(!app.dock_panel_visible(PanelId::Properties));

    // The module's own parts did not move, and the drawing kept its tab.
    let open = drawn(&app);
    assert!(open.contains_key(pid_backup_browser::PANEL_BOUNDS_ID));
    for name in own {
        assert_eq!(part_at(&open, name), part_at(&before, name), "{name}");
    }
    assert_eq!(open.get(&tab), before.get(&tab));

    // Closed by its close button: it is gone from the window, the drawing
    // has its room back, and the backup is still loaded.
    give(
        &mut app,
        AppMessage::Dock(DockMsg::Close(PanelId::PidBackup)),
    );
    assert!(!app.dock_panel_visible(PanelId::PidBackup));
    let closed = drawn(&app);
    assert!(!closed.contains_key(pid_backup_browser::PANEL_BOUNDS_ID));
    assert_eq!(widget_at(&closed, VIEWPORT), widget_at(&before, VIEWPORT));
    for name in own {
        assert_eq!(part_at(&closed, name), part_at(&before, name), "{name}");
    }
    assert_eq!(
        app.pid_backup_browser
            .index()
            .map(|index| index.drawings.len()),
        Some(DRAWINGS.len())
    );
    // The bare command shows it again, with the backup in it.
    say(&mut app, "PIDBACKUPOPEN");
    assert!(app.dock_panel_visible(PanelId::PidBackup));
    assert!(drawn(&app).contains_key(pid_backup_browser::PANEL_BOUNDS_ID));
    assert!(!pid_backup_browser::visible_rows(&app.pid_backup_browser).is_empty());
}

// ── Opening a drawing of the backup ──────────────────────────────────────

/// A drawing activated in the panel asks for a file to be opened, and the
/// file is the one the restore made of that drawing: by the button in the
/// drawing's details, and by what a double click on its row sends. The
/// pointer reaches the rows where the module draws them.
#[test]
fn a_drawing_activated_in_the_panel_asks_for_the_file_the_restore_made_of_it() {
    let _one = crate::headless_ui_lock();
    let scratch = Scratch::new("activated");
    let backup = backup_folder(scratch.path());
    let mut app = launched(false);
    let cache = restores_into(&mut app, scratch.path());
    say(&mut app, &open_line(&backup));
    let index = app
        .pid_backup_browser
        .index()
        .expect("the backup is loaded")
        .clone();
    // A drawing's place in the index and its file: in the folder restores
    // are kept in, not in the backup, and holding what the archive held.
    let drawing = |name: &str| -> (usize, PathBuf) {
        let at = index
            .drawings
            .iter()
            .position(|drawing| drawing.name == name)
            .unwrap_or_else(|| panic!("{name} is listed"));
        let file = index.drawings[at]
            .pid_path
            .clone()
            .unwrap_or_else(|| panic!("{name} has a file"));
        assert!(
            file.starts_with(&cache) && !file.starts_with(&backup),
            "{}",
            file.display()
        );
        assert_eq!(
            std::fs::read(&file).ok().as_deref(),
            Some(name.as_bytes()),
            "{}",
            file.display()
        );
        (at, file)
    };
    let (first, first_file) = drawing("A-101");
    let (second, second_file) = drawing("B-202");
    assert_ne!(first_file, second_file);

    // Walk to the second drawing as a script does: its group unfolds, and
    // the rows of both drawings are in the tree.
    let groups = crate::t!("Plant groups");
    say(
        &mut app,
        &format!("PIDBACKUPOPEN Select {groups} > 00 > 00 > B-202"),
    );
    assert_eq!(
        app.pid_backup_browser.selected,
        Some(NodeId::Drawing(second))
    );

    // A press on the first drawing's row, where the window shows it,
    // selects that drawing.
    let row = pid_backup_browser::visible_rows(&app.pid_backup_browser)
        .iter()
        .position(|row| row.id == NodeId::Drawing(first))
        .expect("the first drawing has a row");
    let row = format!("{}{row}", pid_backup_browser::ROW_BOUNDS_PREFIX);
    let published = press(&app, widget_at(&drawn(&app), &row).center());
    let selected: Vec<&NodeId> = published
        .iter()
        .filter_map(|message| match message {
            AppMessage::PidBackup(PidBackupMsg::Select(id)) => Some(id),
            _ => None,
        })
        .collect();
    assert_eq!(selected, [&NodeId::Drawing(first)]);
    for message in published {
        give(&mut app, message);
    }
    assert_eq!(
        app.pid_backup_browser.selected,
        Some(NodeId::Drawing(first))
    );

    // The button in its details, pressed from the command line, asks for
    // the first drawing's file.
    let button = crate::t!("Open drawing");
    let task = app.run_command_line(&format!("PIDBACKUPOPEN Press {button}"));
    assert_eq!(opens_asked_for(&mut app, task), [first_file.as_path()]);

    // What a double click on the second drawing's row sends asks for the
    // second drawing's.
    let task = app.update(AppMessage::PidBackup(PidBackupMsg::Activate(
        NodeId::Drawing(second),
    )));
    assert_eq!(opens_asked_for(&mut app, task), [second_file.as_path()]);

    // The request is taken up as any request to open a file is: the open
    // under way is of that file, and the module is still on its editor.
    assert!(app.opening.is_none());
    let _ = app.update(AppMessage::OpenRecent(second_file.clone()));
    assert_eq!(
        app.opening
            .as_ref()
            .and_then(|open| open.source_path.clone()),
        Some(second_file)
    );
    assert_eq!(app.pid_shell.current(), Some(PidScreen::Editor));
}

// ── A source that is no backup ───────────────────────────────────────────

/// A path that is not there: one error on the command line, naming it; the
/// panel shown, idle, with the reason in it. The same lines and the same
/// panel as in a `--classic` window, and the module takes the next command.
#[test]
fn a_path_that_is_not_there_is_refused_in_the_module_as_in_a_classic_window() {
    let scratch = Scratch::new("missing");
    let backup = backup_folder(scratch.path());
    let missing = scratch.path().join("nowhere");
    let mut module = launched(false);
    let mut general = launched(true);
    restores_into(&mut module, scratch.path());
    restores_into(&mut general, scratch.path());
    let tabs = module.tabs.len();

    let in_module = say(&mut module, &open_line(&missing));
    let in_general = say(&mut general, &open_line(&missing));

    // One error: the reason, after the command line's own mark of an error.
    let reason = format!("{} does not exist", missing.display());
    let refused = crate::tf!("PIDBACKUPOPEN: {}", reason.clone()).into_owned();
    let wrong = errors(&in_module);
    assert_eq!(wrong.len(), 1, "{in_module:?}");
    assert!(wrong[0].ends_with(&refused), "{refused}: {in_module:?}");
    assert_eq!(in_module, in_general);
    let state = panel_state(&module);
    assert!(state.visible);
    assert_eq!(state.side, Some(DockSide::Right));
    assert_eq!(state.load, "idle");
    assert_eq!(state.error, Some(reason));
    assert!(state.rows.is_empty(), "{:?}", state.rows);
    assert_eq!(state, panel_state(&general));

    // The module is where it was, and the next command is taken.
    assert_eq!(module.pid_shell.current(), Some(PidScreen::Editor));
    assert_eq!(module.tabs.len(), tabs);
    let said = say(&mut module, &open_line(&backup));
    assert!(errors(&said).is_empty(), "{said:?}");
    let state = panel_state(&module);
    assert_eq!(state.error, None);
    assert!(
        state
            .load
            .starts_with(&format!("{} drawing(s)", DRAWINGS.len())),
        "{}",
        state.load
    );
}

/// A folder that is no backup is not refused. The reading takes a folder as
/// it lies, loose drawings and all, so one with nothing of a backup in it is
/// listed as a backup of no drawing, with a warning and without an error.
/// Whether that ought to be an error is not settled here: what is held to is
/// that the module says and shows what a `--classic` window does, and takes
/// the next command.
#[test]
fn a_folder_that_is_no_backup_is_listed_empty_in_the_module_as_in_a_classic_window() {
    let scratch = Scratch::new("no-backup");
    let backup = backup_folder(scratch.path());
    let folder = scratch.path().join("holiday");
    std::fs::create_dir_all(&folder).expect("the folder");
    std::fs::write(folder.join("notes.txt"), "nothing of a backup").expect("a file in it");
    let mut module = launched(false);
    let mut general = launched(true);
    restores_into(&mut module, scratch.path());
    restores_into(&mut general, scratch.path());

    let in_module = say(&mut module, &open_line(&folder));
    let in_general = say(&mut general, &open_line(&folder));

    // No error: a count of no drawing, and a warning at the least.
    assert!(errors(&in_module).is_empty(), "{in_module:?}");
    let warnings = module
        .pid_backup_browser
        .index()
        .unwrap_or_else(|| panic!("the folder is listed: {in_module:?}"))
        .warnings
        .clone();
    assert!(!warnings.is_empty(), "{in_module:?}");
    let report = crate::tf!(
        "PIDBACKUPOPEN: {} drawing(s), {} warning(s)",
        0,
        warnings.len()
    )
    .into_owned();
    assert!(
        in_module.contains(&(EntryKind::Output, report.clone())),
        "{report}: {in_module:?}"
    );
    assert_eq!(in_module, in_general);
    let state = panel_state(&module);
    assert!(state.visible);
    assert_eq!(state.side, Some(DockSide::Right));
    assert!(
        state.load.starts_with("0 drawing(s), "),
        "{}: {warnings:?}",
        state.load
    );
    assert_eq!(state.error, None);
    assert_eq!(state, panel_state(&general));

    // The module is where it was, and the next command is taken.
    assert_eq!(module.pid_shell.current(), Some(PidScreen::Editor));
    let said = say(&mut module, &open_line(&backup));
    assert!(errors(&said).is_empty(), "{said:?}");
    let state = panel_state(&module);
    assert!(
        state
            .load
            .starts_with(&format!("{} drawing(s)", DRAWINGS.len())),
        "{}",
        state.load
    );
}
