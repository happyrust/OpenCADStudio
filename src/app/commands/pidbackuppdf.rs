//! PIDBACKUPPDF -- every drawing of the open backup package as a PDF
//! (`io::pid_backup_pdf`, SPEC-sppid-pdf §5).
//!
//! ```text
//! PIDBACKUPPDF <folder>  plot every drawing of the package PIDBACKUPOPEN
//!                        holds into <folder>, and write the index of what
//!                        became of each; the folder is the rest of the
//!                        line, so it may hold spaces without quotes
//! PIDBACKUPPDF           say where that stands: no export yet, how far the
//!                        one that runs has come, or what the last one came
//!                        to
//! ```
//!
//! A folder that is not given in full is taken from the folder the backup
//! lies in, as PIDBACKUPOPEN takes its restore folder; the line that says
//! the export has started names it in full.
//!
//! The export runs on a worker thread and the command returns once it has
//! started. What it came to is said when it ends, and again by the bare
//! verb. One export runs at a time, and while it runs the restore cache is
//! not cleared (`PIDBACKUPOPEN Cache=Clear`): the export reads the restore.
//!
//! An error is said with the code a script tells it by, in the line and as
//! the command line's error code (`SPPID_PDF_...`, SPEC-sppid-pdf §5.5).
//! The lines are English, as PIDPLOT's are.

use super::*;
#[cfg(not(target_arch = "wasm32"))]
use crate::io::pid_backup_browse::{BackupIndex, DrawingEntry};
#[cfg(not(target_arch = "wasm32"))]
use crate::io::pid_backup_pdf::{self, PdfDrawing, PdfError, PdfOutcome, PdfProgress, PdfRequest};
#[cfg(not(target_arch = "wasm32"))]
use crate::ui::command_line::CommandLine;

/// What an export comes to, once it has ended.
#[cfg(not(target_arch = "wasm32"))]
type Exported = iced::futures::future::BoxFuture<'static, Result<PdfOutcome, PdfError>>;

/// The export the command starts: given what to export, and where to count
/// the drawings it is through with.
#[cfg(not(target_arch = "wasm32"))]
type Export = Box<dyn Fn(PdfRequest, PdfProgress) -> Exported + Send + Sync>;

/// The command's share of the application's state. Of the application, not
/// of a tab: the export is of the package, whatever drawing is in front.
#[cfg(not(target_arch = "wasm32"))]
pub(in crate::app) struct BackupPdf {
    /// The export that runs; `None` while none does.
    running: Option<Running>,
    /// What the last export that ran came to; `None` before the first. One
    /// that was refused before it started is not among them.
    last: Option<Result<PdfOutcome, PdfError>>,
    /// The export itself: [`pid_backup_pdf::export_in_background`], or what
    /// a test puts in its place.
    export: Export,
}

/// An export on its way.
#[cfg(not(target_arch = "wasm32"))]
struct Running {
    /// Where it writes, as the command was given it.
    out_dir: PathBuf,
    /// How many drawings it was given.
    drawings: usize,
    /// How many of them it is through with.
    progress: PdfProgress,
}

#[cfg(not(target_arch = "wasm32"))]
impl Default for BackupPdf {
    fn default() -> Self {
        Self {
            running: None,
            last: None,
            export: Box::new(|request, progress| {
                Box::pin(pid_backup_pdf::export_in_background(request, progress))
            }),
        }
    }
}

/// What the open package gives an export into `out_dir`: its own folder,
/// the folder it is restored to, and its drawings as the panel lists them.
#[cfg(not(target_arch = "wasm32"))]
fn request_for(index: &BackupIndex, out_dir: PathBuf) -> PdfRequest {
    let drawing = |entry: &DrawingEntry| PdfDrawing {
        name: entry.name.clone(),
        pid: entry.pid_path.clone(),
    };
    PdfRequest {
        package_dir: index.backup_root.clone(),
        restored: index.restored.clone(),
        out_dir,
        drawings: index.drawings.iter().map(drawing).collect(),
    }
}

/// The folder an export is asked into, in full. One that is not given in
/// full is taken from the folder the backup lies in -- where PIDBACKUPOPEN
/// takes its restore folder from, and as PIDPLOT takes a PDF from the
/// drawing's -- and not from wherever the application was started.
#[cfg(not(target_arch = "wasm32"))]
fn folder_asked_for(index: &BackupIndex, folder: &str) -> PathBuf {
    let folder = PathBuf::from(folder);
    let asked = match index.source.parent() {
        Some(beside) if folder.is_relative() => beside.join(folder),
        _ => folder,
    };
    std::path::absolute(&asked).unwrap_or(asked)
}

/// What the bare verb says before any export has run.
#[cfg(not(target_arch = "wasm32"))]
const NO_EXPORT_YET: &str = "PIDBACKUPPDF: no export yet. PIDBACKUPPDF <folder> writes every \
     drawing of the open backup package as a PDF into <folder>.";

/// Say `error` with its code: in the line, and as the command line's own.
#[cfg(not(target_arch = "wasm32"))]
fn say_error(command_line: &mut CommandLine, error: &PdfError) {
    let code = error.code();
    command_line.push_error_coded(code, &format!("PIDBACKUPPDF: {code}: {error}"));
}

/// Say what an export came to: its summary, or why it did not get there.
#[cfg(not(target_arch = "wasm32"))]
fn say_result(command_line: &mut CommandLine, result: &Result<PdfOutcome, PdfError>) {
    match result {
        Ok(outcome) => {
            for line in pid_backup_pdf::summary_lines(outcome) {
                command_line.push_output(&format!("PIDBACKUPPDF: {line}"));
            }
        }
        Err(error) => say_error(command_line, error),
    }
}

impl OpenCADStudio {
    pub(super) fn dispatch_pidbackuppdf(&mut self, cmd: &str, _i: usize) -> Option<Task<Message>> {
        let rest = cmd.strip_prefix("PIDBACKUPPDF")?;
        if !(rest.is_empty() || rest.starts_with(char::is_whitespace)) {
            return None;
        }
        Some(self.pid_backup_pdf_command(rest.trim()))
    }

    #[cfg(target_arch = "wasm32")]
    fn pid_backup_pdf_command(&mut self, _folder: &str) -> Task<Message> {
        self.command_line.push_error(
            "PIDBACKUPPDF: exporting a backup package to PDF is not available in the web build.",
        );
        Task::none()
    }
}

#[cfg(not(target_arch = "wasm32"))]
impl OpenCADStudio {
    /// `folder` is what follows the verb: nothing asks where the export
    /// stands, anything else is the folder to export to.
    fn pid_backup_pdf_command(&mut self, folder: &str) -> Task<Message> {
        let folder = super::pidbackup::unquoted(folder);
        if folder.is_empty() {
            self.say_where_the_pid_backup_pdf_stands();
            return Task::none();
        }
        match self.start_pid_backup_pdf(folder) {
            Ok(export) => export,
            Err(error) => {
                say_error(&mut self.command_line, &error);
                Task::none()
            }
        }
    }

    /// Start the export of the open package into `folder` and say so. One
    /// that cannot start -- another runs, no package is open, the folder
    /// will not do -- makes nothing and is no export to speak of later.
    fn start_pid_backup_pdf(&mut self, folder: &str) -> Result<Task<Message>, PdfError> {
        if self.pid_backup_pdf.running.is_some() {
            return Err(PdfError::Busy);
        }
        let index = self.pid_backup_browser.index().ok_or(PdfError::NoPackage)?;
        let request = request_for(index, folder_asked_for(index, folder));
        pid_backup_pdf::check(&request)?;
        let (drawings, out_dir) = (request.drawings.len(), request.out_dir.clone());
        self.command_line.push_output(&format!(
            "PIDBACKUPPDF: exporting {drawings} drawing(s) to {} ...",
            out_dir.display()
        ));
        let progress = PdfProgress::default();
        self.pid_backup_pdf.running = Some(Running {
            out_dir,
            drawings,
            progress: progress.clone(),
        });
        let export = (self.pid_backup_pdf.export)(request, progress);
        Ok(Task::perform(export, Message::PidBackupPdfFinished))
    }

    /// The bare verb: how far the export that runs has come, what the last
    /// one came to, or that there has been none.
    fn say_where_the_pid_backup_pdf_stands(&mut self) {
        let BackupPdf { running, last, .. } = &self.pid_backup_pdf;
        if let Some(running) = running {
            // Until the export has counted its drawings, they are the ones
            // it was given.
            let total = match running.progress.total() {
                0 => running.drawings,
                counted => counted,
            };
            self.command_line.push_output(&format!(
                "PIDBACKUPPDF: exporting to {}: {} of {total} drawing(s) done",
                running.out_dir.display(),
                running.progress.done()
            ));
        } else if let Some(last) = last {
            say_result(&mut self.command_line, last);
        } else {
            self.command_line.push_output(NO_EXPORT_YET);
        }
    }

    /// The export ended: say what it came to, and keep it for the bare verb.
    pub(in crate::app) fn on_pid_backup_pdf_finished(
        &mut self,
        result: Result<PdfOutcome, PdfError>,
    ) {
        self.pid_backup_pdf.running = None;
        say_result(&mut self.command_line, &result);
        self.pid_backup_pdf.last = Some(result);
    }

    /// Whether the restore cache has to stay as it is: an export reads the
    /// restore while it runs. Says so when it has to, to the one who asked
    /// for the clearing.
    pub(super) fn pid_backup_pdf_keeps_the_cache(&mut self) -> bool {
        if self.pid_backup_pdf.running.is_none() {
            return false;
        }
        let code = PdfError::Busy.code();
        self.command_line.push_error_coded(
            code,
            &format!(
                "PIDBACKUPOPEN: {code}: the restore cache is not cleared while PIDBACKUPPDF \
                 exports a package"
            ),
        );
        true
    }
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod tests {
    use super::*;
    use crate::app::pid_shell::PidScreen;
    use crate::io::pid_backup_pdf::PdfPage;
    use crate::ui::command_line::EntryKind;
    use crate::ui::window::pid_backup_browser::{self, CacheMsg, NodeId, PidBackupMsg};
    use iced::futures::channel::oneshot;
    use std::path::Path;
    use std::sync::{Arc, Mutex};
    use std::time::Duration;

    // ── A folder, a package, an application ──────────────────────────────────

    /// A folder of one test's own under the temp folder. It goes when the
    /// test is over, however the test ended.
    struct Scratch(PathBuf);

    impl Scratch {
        fn new(name: &str) -> Self {
            let name = format!("ocs-pidbackuppdf-{name}-{}", std::process::id());
            let root = std::env::temp_dir().join(name);
            let _ = std::fs::remove_dir_all(&root);
            std::fs::create_dir_all(&root).expect("a scratch folder");
            Self(root)
        }

        fn path(&self) -> &Path {
            &self.0
        }

        fn join(&self, name: &str) -> PathBuf {
            self.0.join(name)
        }
    }

    impl Drop for Scratch {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    /// The names of what `folder` holds, sorted; none when it is not there.
    fn names(folder: &Path) -> Vec<String> {
        let Ok(entries) = std::fs::read_dir(folder) else {
            return Vec::new();
        };
        let mut names: Vec<String> = entries
            .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        names.sort();
        names
    }

    /// The index of a made-up package under `root`, as the panel holds one:
    /// the backup's own folder, the folder it is restored to, two drawings
    /// with a file there and one the backup holds no file of.
    fn package(root: &Path) -> Arc<BackupIndex> {
        let backup_root = root.join("package");
        let restored = root.join("restored");
        let filed = restored.join("00").join("00");
        std::fs::create_dir_all(&backup_root).expect("the backup's folder");
        std::fs::create_dir_all(&filed).expect("the restore folder");
        let drawing = |name: &str, has_file: bool| {
            let pid_path = has_file.then(|| filed.join(format!("{name}.pid")));
            if let Some(path) = &pid_path {
                std::fs::write(path, name).expect("a drawing's file");
            }
            DrawingEntry {
                name: name.to_string(),
                pid_path,
                group_path: "SQPlant / 00".to_string(),
                meta: Vec::new(),
            }
        };
        let drawings = vec![
            drawing("A-101", true),
            drawing("B-202", true),
            drawing("P-303", false),
        ];
        Arc::new(BackupIndex {
            source: backup_root.clone(),
            restored,
            backup_root,
            drawings,
            ..BackupIndex::default()
        })
    }

    /// An application in the classic window, with a drawing open.
    fn app() -> OpenCADStudio {
        let mut app = OpenCADStudio::new_for_test();
        let _ = app.automation_op(r#"{"op":"new"}"#);
        app
    }

    /// Have `app` keep what it restores, and so what it clears, in the
    /// scratch folder: the default cache is not a test's to touch. The
    /// folder it will use.
    fn restores_into(app: &mut OpenCADStudio, scratch: &Scratch) -> PathBuf {
        let cache = scratch.join("restores");
        pid_backup_browser::set_cache_folder(&mut app.pid_backup_browser, Some(cache.clone()));
        cache
    }

    /// Give `app` the made-up [`package`] under `scratch` as the backup its
    /// panel holds.
    fn open_package(app: &mut OpenCADStudio, scratch: &Scratch) {
        restores_into(app, scratch);
        let _ = app.on_pid_backup(PidBackupMsg::Loaded(Ok(package(scratch.path()))));
    }

    /// A zip of one drawing, as small as a backup that has to be unpacked
    /// gets.
    fn tiny_backup(path: &Path, drawing: &str) {
        use std::io::Write;
        let mut writer = zip::ZipWriter::new(std::fs::File::create(path).unwrap());
        let options = zip::write::SimpleFileOptions::default();
        let file = format!("00/00/{drawing}.pid");
        writer.start_file(file, options).unwrap();
        writer.write_all(drawing.as_bytes()).unwrap();
        writer.finish().unwrap();
    }

    /// A backup folder as small as one gets: the manifest, and three
    /// drawings in the archive a backup files its drawings in. A drawing's
    /// file holds its own name, which no plot takes for a drawing.
    fn backup_folder(scratch: &Scratch) -> PathBuf {
        use std::io::Write;
        let backup = scratch.join("backup");
        std::fs::create_dir_all(&backup).expect("the backup folder");
        let manifest = "BackupType<<|>>2\nVersion<<|>>7.02\nName<<|>>SQPlant\n";
        std::fs::write(backup.join("Manifest.txt"), manifest).expect("the manifest");
        let archive = backup.join("PlantData~2~711.zip");
        let archive = std::fs::File::create(archive).expect("the archive");
        let mut writer = zip::ZipWriter::new(archive);
        let options = zip::write::SimpleFileOptions::default();
        for file in ["00/00/A-101.pid", "00/00/B-202.pid", "01/01/C-303.pid"] {
            writer.start_file(file, options).expect("a drawing");
            writer.write_all(file.as_bytes()).expect("its bytes");
        }
        writer.finish().expect("the archive is written");
        backup
    }

    /// The application as a launch without `--classic` leaves it once the
    /// startup prompts are answered: in the P&ID module, on its editor.
    fn in_the_module() -> OpenCADStudio {
        let mut app = OpenCADStudio::new_for_test();
        app.default_assoc_prompted = true;
        app.donation_prompt_version = env!("OCS_APP_VERSION").to_owned();
        let launch = crate::cli::GuiConfig::default();
        assert!(
            !launch.classic,
            "a launch is in the module unless it says otherwise"
        );
        let first = app.open_first_screen(&launch);
        app.drive_headless_task(first)
            .expect("the first screen opens without a window");
        let _ = app.update(Message::WindowResized(1600.0, 1000.0));
        app
    }

    // ── An export that waits ─────────────────────────────────────────────────

    /// One call of the stand-in.
    struct Call {
        request: PdfRequest,
        progress: PdfProgress,
        /// Says what the call comes to; gone once it has been said.
        end: Option<oneshot::Sender<Result<PdfOutcome, PdfError>>>,
    }

    /// What stands in for the export: it keeps what the command asked of
    /// it, makes nothing, and each call waits until the test says what it
    /// came to.
    struct StandIn(Arc<Mutex<Vec<Call>>>);

    impl StandIn {
        /// Put a stand-in in the place of `app`'s export.
        fn put_into(app: &mut OpenCADStudio) -> Self {
            let calls = Arc::new(Mutex::new(Vec::new()));
            let kept = Arc::clone(&calls);
            app.pid_backup_pdf.export = Box::new(move |request, progress| {
                let (end, ended) = oneshot::channel();
                kept.lock().unwrap().push(Call {
                    request,
                    progress,
                    end: Some(end),
                });
                Box::pin(async move {
                    let let_go = || PdfError::Failed("the stand-in was let go of".to_string());
                    ended.await.unwrap_or_else(|_| Err(let_go()))
                })
            });
            Self(calls)
        }

        /// What the command asked of the export, call by call.
        fn requests(&self) -> Vec<PdfRequest> {
            let calls = self.0.lock().unwrap();
            calls.iter().map(|call| call.request.clone()).collect()
        }

        /// Where the latest call counts its drawings.
        fn progress(&self) -> PdfProgress {
            let calls = self.0.lock().unwrap();
            let latest = calls.last().expect("an export was started");
            latest.progress.clone()
        }

        /// Have the latest call come to `result`.
        fn end(&self, result: Result<PdfOutcome, PdfError>) {
            let mut calls = self.0.lock().unwrap();
            let latest = calls.last_mut().expect("an export was started");
            let end = latest.end.take().expect("the export still waits");
            end.send(result).expect("the command waits for its export");
        }
    }

    // ── The command line ─────────────────────────────────────────────────────

    /// One thing the command line said.
    #[derive(Debug, Clone, PartialEq)]
    enum Said {
        /// A plain line.
        Line(String),
        /// An error, without the mark every error starts with.
        Error(String),
        /// Anything else but the echo of a command: a note, a warning.
        Other(String),
    }

    fn line(text: impl Into<String>) -> Said {
        Said::Line(text.into())
    }

    fn error(text: impl Into<String>) -> Said {
        Said::Error(text.into())
    }

    /// What the command line holds, the echo of the commands left out.
    fn said(app: &OpenCADStudio) -> Vec<Said> {
        let entries = app.command_line.history.iter();
        entries
            .filter_map(|entry| {
                let text = entry.text.clone();
                match entry.kind {
                    EntryKind::Command => None,
                    EntryKind::Output => Some(Said::Line(text)),
                    EntryKind::Error => {
                        let unmarked = text
                            .split_once(": ")
                            .map_or(text.as_str(), |(_, said)| said);
                        Some(Said::Error(unmarked.to_string()))
                    }
                    _ => Some(Said::Other(text)),
                }
            })
            .collect()
    }

    /// The code of the error the command line said last, as a script reads
    /// it.
    fn code(app: &OpenCADStudio) -> Option<&str> {
        app.command_line.last_error_code.as_deref()
    }

    /// Send `typed` as the command line and a script send it. What it starts
    /// is given back, not run: an export has not ended when its command has.
    fn start(app: &mut OpenCADStudio, typed: &str) -> Task<Message> {
        app.command_line.history.clear();
        app.command_line.last_error_code = None;
        app.run_command_line(typed)
    }

    /// Send `typed` and let what it starts run to its end: what the command
    /// line said.
    fn say(app: &mut OpenCADStudio, typed: &str) -> Vec<Said> {
        let task = start(app, typed);
        app.drive_headless_task(task)
            .expect("the command needs no window");
        said(app)
    }

    /// Send a line that is to start nothing -- the bare verb, an export
    /// that is to be refused -- and what the command line said. What it
    /// started after all is not run: an export in the stand-in's place
    /// would wait for an end the test is not going to give it.
    fn ask(app: &mut OpenCADStudio, typed: &str) -> Vec<Said> {
        let _ = start(app, typed);
        said(app)
    }

    /// Let the export `task` waits for come to `result`, and run the task to
    /// its end: what the command line said of it.
    fn end(
        app: &mut OpenCADStudio,
        stand_in: &StandIn,
        task: Task<Message>,
        result: Result<PdfOutcome, PdfError>,
    ) -> Vec<Said> {
        stand_in.end(result);
        app.command_line.history.clear();
        app.command_line.last_error_code = None;
        app.drive_headless_task(task)
            .expect("an export ends without a window");
        said(app)
    }

    // ── What is typed, what an export comes to, what is said of it ──────────

    const NONE_YET: &str = "PIDBACKUPPDF: no export yet. PIDBACKUPPDF <folder> writes every drawing of the open backup package as a PDF into <folder>.";
    const NO_PACKAGE: &str = "PIDBACKUPPDF: SPPID_PDF_NO_PACKAGE: no backup package is open";
    const BUSY: &str =
        "PIDBACKUPPDF: SPPID_PDF_BUSY: the last export of the package to PDF has not finished";
    const CACHE_KEPT: &str = "PIDBACKUPOPEN: SPPID_PDF_BUSY: the restore cache is not cleared while PIDBACKUPPDF exports a package";

    /// The line that asks for an export into `out`, as it is typed.
    fn export_line(out: &Path) -> String {
        format!("PIDBACKUPPDF {}", out.display())
    }

    /// The line that opens the backup `source`, as it is typed.
    fn open_line(source: &Path) -> String {
        format!("PIDBACKUPOPEN \"{}\"", source.display())
    }

    /// What an export into `out` starts with.
    fn exporting(drawings: usize, out: &Path) -> Said {
        line(format!(
            "PIDBACKUPPDF: exporting {drawings} drawing(s) to {} ...",
            out.display()
        ))
    }

    /// What the bare verb says while an export into `out` runs.
    fn running(out: &Path, done: usize, total: usize) -> Said {
        line(format!(
            "PIDBACKUPPDF: exporting to {}: {done} of {total} drawing(s) done",
            out.display()
        ))
    }

    /// What an export of the made-up package into `out` comes to: one PDF,
    /// one drawing that could not be plotted, one without a file.
    fn outcome(out: &Path) -> PdfOutcome {
        let page = |name: &str, pid: Option<&str>, pdf: Option<&str>, error: &str| PdfPage {
            name: name.to_string(),
            pid: pid.map(str::to_string),
            pdf: pdf.map(str::to_string),
            error: Some(error.to_string()).filter(|error| !error.is_empty()),
            ..PdfPage::default()
        };
        let unfiled = "the backup holds no file of this drawing";
        PdfOutcome {
            pages: vec![
                page("A-101", Some("00/00/A-101.pid"), Some("A-101.pdf"), ""),
                page(
                    "B-202",
                    Some("00/00/B-202.pid"),
                    None,
                    "no face for its text",
                ),
                page("P-303", None, None, unfiled),
            ],
            written: 1,
            failed: 1,
            without_file: 1,
            index_path: out.join(pid_backup_pdf::INDEX_NAME),
            elapsed: Duration::from_millis(2500),
        }
    }

    /// What the command line says of [`outcome`].
    fn summary(out: &Path) -> [Said; 6] {
        let index = out.join("_PdfIndex.json");
        [
            line("PIDBACKUPPDF: wrote 1 of 3 drawing(s) as PDF in 2.5 s"),
            line("PIDBACKUPPDF: 1 drawing(s) could not be written:"),
            line("PIDBACKUPPDF:   B-202: no face for its text"),
            line("PIDBACKUPPDF: 1 drawing(s) have no file in the backup"),
            line("PIDBACKUPPDF: 0 page(s) miss symbols"),
            line(format!("PIDBACKUPPDF: index: {}", index.display())),
        ]
    }

    /// What an export must leave as it was: how many tabs there are, which
    /// is in front, and of that one whether it is modified, how much can be
    /// undone and redone in it, and how much it holds.
    fn front(app: &OpenCADStudio) -> (usize, usize, bool, usize, usize, usize) {
        let tab = &app.tabs[app.active_tab];
        (
            app.tabs.len(),
            app.active_tab,
            tab.dirty,
            tab.history.undo_stack.len(),
            tab.history.redo_stack.len(),
            tab.scene.document.entities().count(),
        )
    }

    // ── The tests ────────────────────────────────────────────────────────────

    #[test]
    fn without_an_open_package_the_export_is_refused_and_nothing_is_made() {
        let scratch = Scratch::new("no-package");
        let out = scratch.join("out");
        let mut app = app();
        // The export is the application's own here: what is refused never
        // gets as far as an export, whichever it is.
        let answer = say(&mut app, &export_line(&out));
        assert_eq!(answer, [error(NO_PACKAGE)]);
        assert_eq!(code(&app), Some("SPPID_PDF_NO_PACKAGE"));
        assert_eq!(names(scratch.path()), [""; 0], "nothing is made");
        // What was refused is no export: there has been none yet.
        assert_eq!(ask(&mut app, "PIDBACKUPPDF"), [line(NONE_YET)]);
        // A script on the automation channel is told the code.
        let run = serde_json::json!({ "op": "run", "cmd": export_line(&out) });
        let reply = app.automation_op(&run.to_string());
        assert_eq!(reply["code"], "SPPID_PDF_NO_PACKAGE", "{reply}");
        assert_eq!(names(scratch.path()), [""; 0], "nothing is made");
    }

    #[test]
    fn the_folder_is_the_rest_of_the_line_and_the_export_is_asked_for_once() {
        let scratch = Scratch::new("folder");
        let mut app = app();
        open_package(&mut app, &scratch);
        let stand_in = StandIn::put_into(&mut app);
        let index = app.pid_backup_browser.index().expect("the package is open");
        let index = Arc::clone(index);
        let drawings: Vec<PdfDrawing> = index
            .drawings
            .iter()
            .map(|drawing| PdfDrawing {
                name: drawing.name.clone(),
                pid: drawing.pid_path.clone(),
            })
            .collect();
        assert_eq!(drawings.len(), 3);
        let out = scratch.join("pdf out");
        // As it is typed: with its space and no quotes, in double quotes, in
        // single ones, and by a path from the folder the backup lies in.
        let typed = [
            format!("PIDBACKUPPDF {}", out.display()),
            format!("PIDBACKUPPDF \"{}\"", out.display()),
            format!("PIDBACKUPPDF   '{}'  ", out.display()),
            "PIDBACKUPPDF pdf out".to_string(),
        ];
        for (round, typed) in typed.iter().enumerate() {
            let export = start(&mut app, typed);
            // One line, the export's own: the verb was not run by itself
            // first, which would have said where the last export stands.
            assert_eq!(said(&app), [exporting(3, &out)], "{typed}");
            let requests = stand_in.requests();
            assert_eq!(requests.len(), round + 1, "{typed}");
            let request = &requests[round];
            assert_eq!(request.out_dir, out, "{typed}");
            assert_eq!(request.package_dir, index.backup_root);
            assert_eq!(request.restored, index.restored);
            assert_eq!(request.drawings, drawings);
            end(&mut app, &stand_in, export, Ok(outcome(&out)));
        }
        // The stand-in makes nothing, and neither does the command.
        assert_eq!(names(scratch.path()), ["package", "restored"]);
    }

    #[test]
    fn a_backup_opened_by_part_of_its_path_is_exported_to_a_folder_named_in_full() {
        let scratch = Scratch::new("in-full");
        let mut app = app();
        restores_into(&mut app, &scratch);
        // The backup was opened by a path from wherever the application was
        // started, and the folder is asked for by a path from the backup's.
        let mut index = package(scratch.path());
        let opened = Arc::get_mut(&mut index).expect("the index is no one else's yet");
        opened.source = Path::new("ocs-nowhere").join("package.zip");
        let _ = app.on_pid_backup(PidBackupMsg::Loaded(Ok(index)));
        let stand_in = StandIn::put_into(&mut app);
        let export = start(&mut app, "PIDBACKUPPDF pdf out");
        let asked = stand_in.requests()[0].out_dir.clone();
        // Beside the backup, and in full: the line says where the PDFs go.
        let beside = Path::new("ocs-nowhere").join("pdf out");
        assert!(asked.is_absolute(), "{}", asked.display());
        assert!(asked.ends_with(&beside), "{}", asked.display());
        assert_eq!(said(&app), [exporting(3, &asked)]);
        end(&mut app, &stand_in, export, Ok(outcome(&asked)));
    }

    #[test]
    fn while_the_export_runs_the_bare_verb_says_how_far_it_is_and_no_other_starts() {
        let scratch = Scratch::new("running");
        let mut app = app();
        open_package(&mut app, &scratch);
        let stand_in = StandIn::put_into(&mut app);
        let (out, other) = (scratch.join("out"), scratch.join("other"));
        let export = start(&mut app, &export_line(&out));
        // The command is back and has said so; the export has not ended.
        assert_eq!(said(&app), [exporting(3, &out)]);
        assert_eq!(stand_in.requests().len(), 1);
        // Until the export counts, the drawings are the ones it was given.
        assert_eq!(ask(&mut app, "PIDBACKUPPDF"), [running(&out, 0, 3)]);
        // The count is written on another thread, as the export's worker
        // writes it, and it is the export's count that is said.
        let progress = stand_in.progress();
        let worker = std::thread::spawn(move || {
            progress.begin(4);
            progress.step();
            progress.step();
        });
        worker.join().expect("the count is written");
        assert_eq!(ask(&mut app, "PIDBACKUPPDF"), [running(&out, 2, 4)]);

        // No second export while this one runs, whatever folder it names.
        let answer = ask(&mut app, &export_line(&other));
        assert_eq!(answer, [error(BUSY)]);
        assert_eq!(code(&app), Some("SPPID_PDF_BUSY"));
        assert_eq!(stand_in.requests().len(), 1);
        assert_eq!(ask(&mut app, "PIDBACKUPPDF"), [running(&out, 2, 4)]);

        // It ends, and the next one starts, its count from nothing.
        end(&mut app, &stand_in, export, Ok(outcome(&out)));
        let export = start(&mut app, &export_line(&other));
        assert_eq!(said(&app), [exporting(3, &other)]);
        assert_eq!(stand_in.requests().len(), 2);
        assert_eq!(ask(&mut app, "PIDBACKUPPDF"), [running(&other, 0, 3)]);
        end(&mut app, &stand_in, export, Ok(outcome(&other)));
        assert_eq!(names(scratch.path()), ["package", "restored"]);
    }

    #[test]
    fn no_restore_is_cleared_while_the_export_runs() {
        let scratch = Scratch::new("cache");
        let (first, second) = (scratch.join("first.zip"), scratch.join("second.zip"));
        tiny_backup(&first, "A-101");
        tiny_backup(&second, "B-202");
        let mut app = app();
        let cache = restores_into(&mut app, &scratch);
        let stand_in = StandIn::put_into(&mut app);
        let restored = |app: &OpenCADStudio| {
            let index = app.pid_backup_browser.index().expect("the backup is open");
            index.restored.clone()
        };

        // The export of one backup runs, and another backup is opened: from
        // here on a clearing would remove the restore the export reads.
        say(&mut app, &open_line(&first));
        let read = restored(&app);
        let drawing = read.join("00").join("00").join("A-101.pid");
        assert_eq!(read.parent(), Some(cache.as_path()));
        assert!(drawing.is_file());
        let out = scratch.join("out");
        let export = start(&mut app, &export_line(&out));
        assert_eq!(stand_in.requests()[0].restored, read);
        say(&mut app, &open_line(&second));
        let kept = restored(&app);
        assert_ne!(kept, read);
        // The backup's own row has the button that clears the others.
        let task = app.on_pid_backup(PidBackupMsg::Select(NodeId::Root));
        app.drive_headless_task(task)
            .expect("measuring needs no window");
        let button = crate::t!("Clear the other restores").into_owned();

        // Asked for on the command line, pressed by its label, clicked.
        let typed = [
            "PIDBACKUPOPEN Cache=Clear".to_string(),
            format!("PIDBACKUPOPEN Press {button}"),
        ];
        for typed in &typed {
            let answer = say(&mut app, typed);
            assert_eq!(answer.last(), Some(&error(CACHE_KEPT)), "{typed}");
            assert_eq!(code(&app), Some("SPPID_PDF_BUSY"), "{typed}");
            assert!(drawing.is_file(), "{typed}");
        }
        app.command_line.history.clear();
        let task = app.on_pid_backup(PidBackupMsg::Cache(CacheMsg::Clear));
        app.drive_headless_task(task)
            .expect("a refusal needs no window");
        assert_eq!(said(&app), [error(CACHE_KEPT)]);
        assert!(drawing.is_file());

        // The export has ended: a clearing clears again.
        end(&mut app, &stand_in, export, Ok(outcome(&out)));
        let answer = say(&mut app, "PIDBACKUPOPEN Cache=Clear");
        assert!(!read.exists(), "{answer:?}");
        assert!(kept.join("00").join("00").join("B-202.pid").is_file());
        let refused = |said: &Said| matches!(said, Said::Error(_));
        assert!(!answer.iter().any(refused), "{answer:?}");
    }

    #[test]
    fn when_the_export_ends_its_summary_is_said_and_the_bare_verb_says_it_again() {
        let scratch = Scratch::new("summary");
        let mut app = app();
        open_package(&mut app, &scratch);
        let stand_in = StandIn::put_into(&mut app);
        let out = scratch.join("out");
        assert_eq!(ask(&mut app, "PIDBACKUPPDF"), [line(NONE_YET)]);
        let export = start(&mut app, &export_line(&out));
        // How many were written, each that could not be by its name, how
        // long it took, where the index is.
        let answer = end(&mut app, &stand_in, export, Ok(outcome(&out)));
        assert_eq!(answer, summary(&out));
        // The same again from the bare verb, as often as it is asked.
        for _ in 0..2 {
            assert_eq!(ask(&mut app, "PIDBACKUPPDF"), summary(&out));
        }
        // An export that is refused -- the folder the package is restored
        // to is not one to export into -- is not the last export: the
        // summary stays what it was, and the export itself was not asked.
        let answer = ask(&mut app, &export_line(&scratch.join("restored")));
        assert!(matches!(answer.as_slice(), [Said::Error(_)]), "{answer:?}");
        assert_eq!(code(&app), Some("SPPID_PDF_TARGET_INVALID"), "{answer:?}");
        assert_eq!(stand_in.requests().len(), 1);
        assert_eq!(ask(&mut app, "PIDBACKUPPDF"), summary(&out));
    }

    #[test]
    fn an_export_that_fails_is_said_with_its_code_and_another_can_follow() {
        let scratch = Scratch::new("failed");
        let mut app = app();
        open_package(&mut app, &scratch);
        let stand_in = StandIn::put_into(&mut app);
        let out = scratch.join("out");
        let export = start(&mut app, &export_line(&out));
        let failed = PdfError::Failed("the index could not be written: disk full".to_string());
        let answer = end(&mut app, &stand_in, export, Err(failed));
        let failure = [error(
            "PIDBACKUPPDF: SPPID_PDF_FAILED: the index could not be written: disk full",
        )];
        assert_eq!(answer, failure);
        assert_eq!(code(&app), Some("SPPID_PDF_FAILED"));
        // That is what the last export came to, and the bare verb says so.
        assert_eq!(ask(&mut app, "PIDBACKUPPDF"), failure);
        assert_eq!(code(&app), Some("SPPID_PDF_FAILED"));

        // The application goes on: a line is drawn, and the next export
        // starts, into the same folder, and ends well.
        let held = front(&app).5;
        say(&mut app, "LINE 0,0 10,0");
        assert_eq!(front(&app).5, held + 1);
        let export = start(&mut app, &export_line(&out));
        assert_eq!(said(&app), [exporting(3, &out)]);
        assert_eq!(stand_in.requests().len(), 2);
        let answer = end(&mut app, &stand_in, export, Ok(outcome(&out)));
        assert_eq!(answer, summary(&out));
        assert_eq!(code(&app), None);
        assert_eq!(ask(&mut app, "PIDBACKUPPDF"), summary(&out));
    }

    #[test]
    fn the_drawing_in_front_is_left_as_it_was() {
        let scratch = Scratch::new("front");
        let mut app = app();
        open_package(&mut app, &scratch);
        let stand_in = StandIn::put_into(&mut app);
        // A drawing nothing was done to, then one with a line that can be
        // undone: neither is the export's to change.
        for (round, drawn) in [false, true].into_iter().enumerate() {
            if drawn {
                say(&mut app, "LINE 0,0 10,0");
            }
            let before = front(&app);
            let out = scratch.join(&format!("out {round}"));
            let export = start(&mut app, &export_line(&out));
            assert_eq!(front(&app), before, "started, round {round}");
            ask(&mut app, "PIDBACKUPPDF");
            assert_eq!(front(&app), before, "asked while it runs, round {round}");
            end(&mut app, &stand_in, export, Ok(outcome(&out)));
            assert_eq!(front(&app), before, "ended, round {round}");
            ask(&mut app, "PIDBACKUPPDF");
            assert_eq!(front(&app), before, "asked after it, round {round}");
        }
        // The line was a change, so there was something to lose.
        let (_, _, modified, undone, _, held) = front(&app);
        assert!(modified && undone > 0 && held > 0, "{:?}", front(&app));
        assert_eq!(stand_in.requests().len(), 2);
    }

    #[test]
    fn in_the_module_the_export_itself_plots_the_backup_the_panel_holds() {
        let scratch = Scratch::new("module");
        let backup = backup_folder(&scratch);
        let mut app = in_the_module();
        let cache = restores_into(&mut app, &scratch);
        assert_eq!(app.pid_shell.current(), Some(PidScreen::Editor));
        say(&mut app, &open_line(&backup));
        let index = app.pid_backup_browser.index().expect("the backup is open");
        let index = Arc::clone(index);
        assert_eq!(index.drawings.len(), 3);
        assert_eq!(index.restored.parent(), Some(cache.as_path()));
        // Border templates of the restore's own, so that the plot looks for
        // none on the machine the test runs on.
        let templates = index.restored.join("Ref").join("Template Files");
        std::fs::create_dir_all(&templates).expect("the restore's templates");
        std::fs::write(templates.join("A1.igr"), b"").expect("a template");
        let tabs = app.tabs.len();

        // No stand-in: the export is the one the application has.
        let out = scratch.join("pdf out");
        let answer = say(&mut app, &export_line(&out));

        assert_eq!(answer.len(), 8, "{answer:?}");
        let text = |at: usize| match &answer[at] {
            Said::Line(text) => text.as_str(),
            other => panic!("{other:?} is no plain line"),
        };
        assert_eq!(answer[0], exporting(3, &out));
        // Not one of the made-up files is a drawing, and each is named.
        let wrote = "PIDBACKUPPDF: wrote 0 of 3 drawing(s) as PDF in ";
        assert!(text(1).starts_with(wrote), "{answer:?}");
        assert_eq!(text(2), "PIDBACKUPPDF: 3 drawing(s) could not be written:");
        for (at, name) in ["A-101", "B-202", "C-303"].into_iter().enumerate() {
            let named = format!("PIDBACKUPPDF:   {name}: ");
            assert!(text(3 + at).starts_with(&named), "{answer:?}");
        }
        assert_eq!(text(6), "PIDBACKUPPDF: 0 page(s) miss symbols");
        assert!(text(7).starts_with("PIDBACKUPPDF: index: "), "{answer:?}");
        assert!(text(7).ends_with("_PdfIndex.json"), "{answer:?}");
        // The index is all the folder holds, and the backup is as it was.
        assert_eq!(names(&out), ["_PdfIndex.json"]);
        assert_eq!(names(&backup), ["Manifest.txt", "PlantData~2~711.zip"]);
        // The module is where it was.
        assert_eq!(app.pid_shell.current(), Some(PidScreen::Editor));
        assert_eq!(app.tabs.len(), tabs);
        // And the bare verb says again what the export came to.
        assert_eq!(say(&mut app, "PIDBACKUPPDF"), answer[1..]);
    }

    #[test]
    fn the_command_is_listed_for_autocomplete_and_runs_with_no_drawing_open() {
        let listed = crate::command::all_registered_command_names();
        assert!(listed.contains(&"PIDBACKUPPDF"));
        // The start page: the package is the application's, not a drawing's.
        let scratch = Scratch::new("start");
        let mut app = OpenCADStudio::new_for_test();
        assert!(app.tabs[app.active_tab].is_start);
        assert_eq!(ask(&mut app, "PIDBACKUPPDF"), [line(NONE_YET)]);
        open_package(&mut app, &scratch);
        let stand_in = StandIn::put_into(&mut app);
        let out = scratch.join("out");
        let export = start(&mut app, &export_line(&out));
        assert_eq!(said(&app), [exporting(3, &out)]);
        let answer = end(&mut app, &stand_in, export, Ok(outcome(&out)));
        assert_eq!(answer, summary(&out));
        assert!(app.tabs[app.active_tab].is_start);
    }

    #[test]
    fn a_longer_word_is_not_this_command() {
        let mut app = app();
        // It is answered as any word that is no command is, the word aside:
        // not as an export into a folder called `S`.
        let mut answered = |word: &str| match say(&mut app, word).as_slice() {
            [Said::Error(text)] => text.replace(word, "<word>"),
            other => panic!("{word}: {other:?}"),
        };
        let no_command = answered("PIDBACKUPNOSUCH");
        assert_eq!(answered("PIDBACKUPPDFS"), no_command);
    }
}
