//! Every drawing of an opened backup package as a PDF, and an index of what
//! became of each one (SPEC-sppid-pdf §5).
//!
//! A page is the one [`pid_pdf`] plots for a single drawing. This module
//! walks the package's drawings, goes on past a drawing that cannot be
//! plotted, and writes `_PdfIndex.json` beside the PDFs. The folder is the
//! user's: a file that is there already is never written over.

use std::collections::hash_map::Entry;
use std::collections::HashMap;
use std::path::{Component, Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use serde::Serialize;

use crate::io::pid_pdf::{self, PageLedger, PidPdfReceipt};

/// The index an export ends with, beside the PDFs.
pub const INDEX_NAME: &str = "_PdfIndex.json";

/// The thread a background export runs on.
const THREAD_NAME: &str = "ocs-pid-backup-pdf";

/// How many of the files in the way an error names; past that it counts.
const FILES_NAMED: usize = 5;

/// How many drawings without a PDF the summary lists; the index has all.
const FAILURES_LISTED: usize = 10;

/// What to export, and where to.
#[derive(Debug, Clone)]
pub struct PdfRequest {
    /// The folder that holds the backup's own files (`Manifest.txt`, ...).
    pub package_dir: PathBuf,
    /// The folder the backup was restored to: its drawings and its `Ref`.
    pub restored: PathBuf,
    /// The folder the PDFs and the index go to.
    pub out_dir: PathBuf,
    /// The package's drawings, as its index lists them.
    pub drawings: Vec<PdfDrawing>,
}

/// One drawing of the package.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PdfDrawing {
    /// The name the plant's database gives it.
    pub name: String,
    /// Its restored `.pid`; `None` when the backup holds no file of it.
    pub pid: Option<PathBuf>,
}

/// What became of one drawing: its entry in `_PdfIndex.json`.
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct PdfPage {
    /// The drawing's name: its file's, or the registered one when the
    /// backup holds no file of it.
    pub name: String,
    /// Where its `.pid` lies inside the package, `/`-separated; `None`
    /// without a file.
    pub pid: Option<String>,
    /// The PDF's file name; `None` when none was written.
    pub pdf: Option<String>,
    /// The PDF's size, and how many primitives its page holds.
    pub bytes: usize,
    pub primitives: usize,
    /// The page's size in millimetres.
    pub page_mm: Option<[f64; 2]>,
    /// The file name of the border template the page is set in.
    pub template: Option<String>,
    /// How many of the symbol files the drawing names were found, out of
    /// how many, and the ones that were not.
    pub symbols_found: usize,
    pub symbols_referenced: usize,
    pub missing_symbols: Vec<String>,
    /// `pid-plot`'s account of the page.
    #[serde(serialize_with = "ledger_fields")]
    pub ledger: PageLedger,
    /// What the plot had to say, e.g. where reference data came from.
    pub notes: Vec<String>,
    /// Why no PDF was written; `None` when one was.
    pub error: Option<String>,
}

/// A page's ledger as the index writes it: the five counts by name.
fn ledger_fields<S: serde::Serializer>(ledger: &PageLedger, to: S) -> Result<S::Ok, S::Error> {
    use serde::ser::SerializeStruct;
    let mut fields = to.serialize_struct("ledger", 5)?;
    fields.serialize_field("entities", &ledger.entities)?;
    fields.serialize_field("on_page", &ledger.on_page)?;
    fields.serialize_field("intentional", &ledger.intentional)?;
    fields.serialize_field("suspect", &ledger.suspect)?;
    fields.serialize_field("unattributed", &ledger.unattributed)?;
    fields.end()
}

/// What an export came to.
#[derive(Debug, Clone, PartialEq)]
pub struct PdfOutcome {
    /// One entry for every drawing given, in the index's order.
    pub pages: Vec<PdfPage>,
    /// How many PDFs were written; how many drawings with a file got none;
    /// how many drawings the backup holds no file of.
    pub written: usize,
    pub failed: usize,
    pub without_file: usize,
    /// Where the index was written.
    pub index_path: PathBuf,
    /// How long the export took. The index does not say: two exports of
    /// one package write the same bytes.
    pub elapsed: Duration,
}

/// Why an export did not happen, or did not come to its end. A drawing
/// that cannot be plotted is none of these: the index says why, and the
/// export goes on.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PdfError {
    /// No backup package is open. The command's to raise: an export that
    /// got this far was given one.
    NoPackage,
    /// Not one of the package's drawings has a file to plot.
    NothingToPlot { drawings: usize },
    /// The folder cannot be used: it is a file, it cannot be made, or it
    /// lies in the package or in the folder the package is restored to.
    TargetInvalid(String),
    /// Files the export writes are in `folder` already: `count` of them,
    /// `names` the first few. Nothing was written.
    TargetHasFiles {
        folder: PathBuf,
        names: Vec<String>,
        count: usize,
    },
    /// The last export has not finished. The command's to raise.
    Busy,
    /// The export as a whole could not be done: the index could not be
    /// written, or the thread it ran on is gone.
    Failed(String),
}

impl PdfError {
    /// The code scripts and tests tell the errors apart by (SPEC-sppid-pdf
    /// §5.5).
    pub fn code(&self) -> &'static str {
        match self {
            Self::NoPackage => "SPPID_PDF_NO_PACKAGE",
            Self::NothingToPlot { .. } => "SPPID_PDF_NOTHING_TO_PLOT",
            Self::TargetInvalid(_) => "SPPID_PDF_TARGET_INVALID",
            Self::TargetHasFiles { .. } => "SPPID_PDF_TARGET_HAS_FILES",
            Self::Busy => "SPPID_PDF_BUSY",
            Self::Failed(_) => "SPPID_PDF_FAILED",
        }
    }
}

impl std::fmt::Display for PdfError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NoPackage => f.write_str("no backup package is open"),
            Self::NothingToPlot { drawings } => {
                write!(
                    f,
                    "none of the package's {drawings} drawing(s) has a file to plot"
                )
            }
            Self::TargetInvalid(why) | Self::Failed(why) => f.write_str(why),
            Self::TargetHasFiles {
                folder,
                names,
                count,
            } => {
                let folder = folder.display();
                let listed = names.join(", ");
                write!(
                    f,
                    "{count} file(s) the export writes are already in {folder}: {listed}"
                )?;
                if *count > names.len() {
                    write!(f, " and {} more", count - names.len())?;
                }
                f.write_str("; nothing was written")
            }
            Self::Busy => f.write_str("the last export of the package to PDF has not finished"),
        }
    }
}

impl std::error::Error for PdfError {}

/// How far an export has come: written by the export on its worker thread,
/// read by the command while it waits.
#[derive(Debug, Clone, Default)]
pub struct PdfProgress(Arc<Counts>);

#[derive(Debug, Default)]
struct Counts {
    done: AtomicUsize,
    total: AtomicUsize,
}

impl PdfProgress {
    /// How many drawings the export is through with: written, or found
    /// unable to be.
    pub fn done(&self) -> usize {
        self.0.done.load(Ordering::Relaxed)
    }

    /// How many drawings the export was given; zero until it starts.
    pub fn total(&self) -> usize {
        self.0.total.load(Ordering::Relaxed)
    }

    fn begin(&self, total: usize) {
        self.0.done.store(0, Ordering::Relaxed);
        self.0.total.store(total, Ordering::Relaxed);
    }

    fn step(&self) {
        self.0.done.fetch_add(1, Ordering::Relaxed);
    }
}

/// `path` in full, its `.` and `..` taken out by name: the folder may not
/// exist yet to be asked what it is.
fn full(path: &Path) -> PathBuf {
    let whole = std::path::absolute(path).unwrap_or_else(|_| path.to_path_buf());
    let mut plain = PathBuf::new();
    for part in whole.components() {
        match part {
            Component::CurDir => {}
            Component::ParentDir => {
                plain.pop();
            }
            part => plain.push(part),
        }
    }
    plain
}

/// [`full`] with the links in the part of the path that exists followed,
/// so that two spellings of one folder come out the same. For comparing
/// only: Windows answers with a `\\?\` path.
fn resolved(path: &Path) -> PathBuf {
    let full = full(path);
    let real = full.ancestors().find_map(|existing| {
        let real = existing.canonicalize().ok()?;
        Some(real.join(full.strip_prefix(existing).ok()?))
    });
    real.unwrap_or(full)
}

/// Whether `path` is `folder` or lies in it, both [`resolved`]. Windows
/// takes two names that differ in letter case for one, so they are one
/// here.
fn lies_in(path: &Path, folder: &Path) -> bool {
    let name = |part: Component<'_>| {
        let name = part.as_os_str().to_string_lossy();
        if cfg!(windows) {
            name.to_lowercase()
        } else {
            name.into_owned()
        }
    };
    let mut inner = path.components().map(name);
    let mut outer = folder.components().map(name).peekable();
    outer.peek().is_some() && outer.all(|part| inner.next() == Some(part))
}

/// Why the export cannot write to `out_dir` -- the request's, in [`full`]
/// -- when it cannot (SPEC-sppid-pdf §5.2, the last rule). Nothing is made
/// to find out.
fn unusable(out_dir: &Path, request: &PdfRequest) -> Option<String> {
    if request.out_dir.as_os_str().is_empty() {
        return Some("no folder to export to was given".to_string());
    }
    // What the folder would be made in decides whether it can be made.
    let existing = out_dir
        .ancestors()
        .find(|path| path.symlink_metadata().is_ok());
    if let Some(existing) = existing.filter(|path| !path.is_dir()) {
        let folder = out_dir.display();
        return Some(if existing == out_dir {
            format!("{folder} is a file, not a folder")
        } else {
            format!("{folder} cannot be made: {} is a file", existing.display())
        });
    }
    let target = resolved(out_dir);
    let holders = [
        (&request.package_dir, "the backup package"),
        (&request.restored, "the folder the backup is restored to"),
    ];
    let (holder, what) = holders
        .into_iter()
        .find(|(holder, _)| lies_in(&target, &resolved(holder)))?;
    let (folder, holder) = (out_dir.display(), holder.display());
    Some(format!(
        "{folder} lies in {what} ({holder}); export to a folder outside of it"
    ))
}

/// The entries of the index before anything is plotted, in the index's
/// order, each with the `.pid` to plot when a PDF is to be written for it.
/// What is settled here: the name every page gets, and which drawings get
/// no page because they have no file or their name is taken.
fn plan(request: &PdfRequest) -> Vec<(PdfPage, Option<&Path>)> {
    let mut plan: Vec<(PdfPage, Option<&Path>)> = request
        .drawings
        .iter()
        .map(|drawing| {
            let source = drawing.pid.as_deref();
            let own = source.and_then(Path::file_stem);
            let own = own.map(|name| name.to_string_lossy().into_owned());
            let page = PdfPage {
                name: own.unwrap_or_else(|| drawing.name.clone()),
                pid: source.map(|pid| package_path(pid, request)),
                ..PdfPage::default()
            };
            (page, source)
        })
        .collect();
    // Drawings with a file by where they lie in the package, then the ones
    // without by name.
    plan.sort_by_cached_key(|(page, _)| (page.pid.is_none(), page.pid.clone(), page.name.clone()));
    // One folder takes every PDF, and Windows takes `A.pdf` and `a.pdf`
    // for one file: the first drawing of a name has it.
    let mut taken: HashMap<String, (String, String)> = HashMap::new();
    for (page, source) in &mut plan {
        let Some(pid) = page.pid.clone() else {
            page.error = Some("the backup holds no file of this drawing".to_string());
            continue;
        };
        let pdf = format!("{}.pdf", page.name);
        match taken.entry(pdf.to_lowercase()) {
            Entry::Vacant(free) => {
                free.insert((pdf.clone(), pid));
                page.pdf = Some(pdf);
            }
            Entry::Occupied(first) => {
                let (pdf, of) = first.get();
                let why = format!("{pdf} is the PDF of {of}, a drawing of the same name");
                page.error = Some(why);
                *source = None;
            }
        }
    }
    plan
}

/// Where `pid` lies inside the package, `/`-separated: under the package's
/// own folder or under the one it is restored to, whichever holds it. A
/// file under neither keeps its whole path.
fn package_path(pid: &Path, request: &PdfRequest) -> String {
    let inside = [&request.package_dir, &request.restored]
        .into_iter()
        .find_map(|holder| pid.strip_prefix(holder).ok());
    let inside = inside.unwrap_or(pid).to_string_lossy();
    inside.replace('\\', "/")
}

/// The name a file goes by until it is whole.
fn temporary(name: &str) -> String {
    format!("{name}.tmp")
}

/// The files the export writes that `out_dir` holds already: every PDF
/// and the index, each also under the name it goes by until it is whole.
fn in_the_way(out_dir: &Path, plan: &[(PdfPage, Option<&Path>)]) -> Vec<String> {
    let pdfs = plan.iter().filter_map(|(page, _)| page.pdf.clone());
    pdfs.chain([INDEX_NAME.to_string()])
        .flat_map(|name| {
            let unfinished = temporary(&name);
            [name, unfinished]
        })
        .filter(|name| out_dir.join(name).symlink_metadata().is_ok())
        .collect()
}

/// Whether the export of `request` can start: the package has a drawing
/// to plot, the folder can be used, and none of the files the export
/// writes is in it yet (SPEC-sppid-pdf §5.2). Only looks: nothing is made
/// or written.
pub fn check(request: &PdfRequest) -> Result<(), PdfError> {
    let has_file = |drawing: &PdfDrawing| drawing.pid.as_deref().is_some_and(Path::is_file);
    if !request.drawings.iter().any(has_file) {
        let drawings = request.drawings.len();
        return Err(PdfError::NothingToPlot { drawings });
    }
    let folder = full(&request.out_dir);
    if let Some(why) = unusable(&folder, request) {
        return Err(PdfError::TargetInvalid(why));
    }
    let mut names = in_the_way(&folder, &plan(request));
    if names.is_empty() {
        return Ok(());
    }
    let count = names.len();
    names.truncate(FILES_NAMED);
    Err(PdfError::TargetHasFiles {
        folder,
        names,
        count,
    })
}

/// Plot every drawing of the package into the request's folder and write
/// the index of what became of each. A drawing that cannot be plotted is
/// in the index with the reason, and the export goes on to the next one;
/// what [`check`] refuses is refused here too, before anything is made.
pub fn export(request: &PdfRequest, progress: &PdfProgress) -> Result<PdfOutcome, PdfError> {
    let (project, fonts) = plot_sources(request);
    export_with(request, progress, |drawing, target| {
        pid_pdf::plot_pdf_against(drawing, target, project, &fonts)
    })
}

/// What the plot of a drawing of this package is given beside the drawing:
/// the folder holding the package's `Ref` -- the one it is restored to --
/// and the package's own `fonts` as one more folder to look for faces in
/// (SPEC-sppid-pdf §5.4).
fn plot_sources(request: &PdfRequest) -> (&Path, [PathBuf; 1]) {
    (&request.restored, [request.package_dir.join("fonts")])
}

/// [`export`] with the plot of one drawing given: `plot(drawing, target)`
/// writes the PDF of `drawing` to `target` and says what went in.
fn export_with<F>(
    request: &PdfRequest,
    progress: &PdfProgress,
    mut plot: F,
) -> Result<PdfOutcome, PdfError>
where
    F: FnMut(&Path, &Path) -> Result<PidPdfReceipt, String>,
{
    let started = Instant::now();
    check(request)?;
    let out_dir = full(&request.out_dir);
    std::fs::create_dir_all(&out_dir).map_err(|error| {
        PdfError::TargetInvalid(format!("{} cannot be made: {error}", out_dir.display()))
    })?;
    let plan = plan(request);
    progress.begin(plan.len());
    let mut pages = Vec::with_capacity(plan.len());
    for (mut page, source) in plan {
        if let Some(source) = source {
            write_page(&mut page, source, &out_dir, &mut plot);
        }
        pages.push(page);
        progress.step();
    }
    let written = pages.iter().filter(|page| page.pdf.is_some()).count();
    let without_file = pages.iter().filter(|page| page.pid.is_none()).count();
    let failed = pages.len() - written - without_file;
    let index = IndexFile {
        package: request.package_dir.display().to_string(),
        drawings: pages.len(),
        written,
        failed,
        without_file,
        pages: &pages,
    };
    let index_path = write_index(&out_dir, &index).map_err(PdfError::Failed)?;
    Ok(PdfOutcome {
        pages,
        written,
        failed,
        without_file,
        index_path,
        elapsed: started.elapsed(),
    })
}

/// Plot `source` into `out_dir` under the name `page` has for it, and put
/// into `page` what came of it. The PDF goes by another name until it is
/// whole, so a file called `<name>.pdf` is always a finished page.
fn write_page<F>(page: &mut PdfPage, source: &Path, out_dir: &Path, plot: &mut F)
where
    F: FnMut(&Path, &Path) -> Result<PidPdfReceipt, String>,
{
    let Some(pdf) = page.pdf.take() else {
        return;
    };
    let unfinished = out_dir.join(temporary(&pdf));
    // A plot that panics on one drawing costs that page, not the export.
    let plotted =
        std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| plot(source, &unfinished)));
    let plotted = plotted.unwrap_or_else(|panic| {
        let said = panic_text(panic.as_ref());
        Err(format!("the plot panicked: {said}"))
    });
    let named =
        plotted.and_then(
            |receipt| match std::fs::rename(&unfinished, out_dir.join(&pdf)) {
                Ok(()) => Ok(receipt),
                Err(error) => Err(format!("{pdf} could not be given its name: {error}")),
            },
        );
    match named {
        Ok(receipt) => {
            let template = receipt.template.as_deref().and_then(Path::file_name);
            page.template = template.map(|name| name.to_string_lossy().into_owned());
            page.pdf = Some(pdf);
            page.bytes = receipt.bytes;
            page.primitives = receipt.primitives;
            page.page_mm = receipt.page_mm;
            page.symbols_found = receipt.symbols_found;
            page.symbols_referenced = receipt.symbols_referenced;
            page.missing_symbols = receipt.missing_symbols;
            page.ledger = receipt.ledger;
            page.notes = receipt.notes;
        }
        Err(why) => {
            let _ = std::fs::remove_file(&unfinished);
            page.error = Some(why);
        }
    }
}

/// What a panic had to say.
fn panic_text(panic: &(dyn std::any::Any + Send)) -> &str {
    let literal = panic.downcast_ref::<&str>().copied();
    let formatted = || panic.downcast_ref::<String>().map(String::as_str);
    literal.or_else(formatted).unwrap_or("no message")
}

/// `_PdfIndex.json`: the counts, then an entry for every drawing. No time
/// and no duration in it, so the same package gives the same bytes.
#[derive(Serialize)]
struct IndexFile<'a> {
    package: String,
    drawings: usize,
    written: usize,
    failed: usize,
    without_file: usize,
    pages: &'a [PdfPage],
}

/// Write the index into `out_dir` -- last of all, and like a page under
/// another name until it is whole -- and say where it is.
fn write_index(out_dir: &Path, index: &IndexFile<'_>) -> Result<PathBuf, String> {
    let path = out_dir.join(INDEX_NAME);
    let unfinished = out_dir.join(temporary(INDEX_NAME));
    let written = serde_json::to_string_pretty(index)
        .map_err(std::io::Error::other)
        .and_then(|text| std::fs::write(&unfinished, text + "\n"))
        .and_then(|()| std::fs::rename(&unfinished, &path));
    match written {
        Ok(()) => Ok(path),
        Err(error) => {
            let _ = std::fs::remove_file(&unfinished);
            let path = path.display();
            Err(format!("the index {path} could not be written: {error}"))
        }
    }
}

/// [`export`] on a worker thread: a plant's drawings take minutes, which
/// the UI thread must not wait out. `progress` says how far it has come.
pub async fn export_in_background(
    request: PdfRequest,
    progress: PdfProgress,
) -> Result<PdfOutcome, PdfError> {
    in_background(move || export(&request, &progress)).await
}

/// Run `job` -- an export -- on a thread of its own and wait for what it
/// comes to. One drawing after the other on that one thread: the plots
/// are not run side by side.
async fn in_background<J>(job: J) -> Result<PdfOutcome, PdfError>
where
    J: FnOnce() -> Result<PdfOutcome, PdfError> + Send + 'static,
{
    let (sender, receiver) = iced::futures::channel::oneshot::channel();
    std::thread::Builder::new()
        .name(THREAD_NAME.to_string())
        // The stack a single plot gets (`pid_pdf::plot_pdf_in_background`):
        // the record walk recurses through nested symbols and templates.
        .stack_size(64 * 1024 * 1024)
        .spawn(move || {
            let _ = sender.send(job());
        })
        .map_err(|error| PdfError::Failed(format!("the export could not be started: {error}")))?;
    let stopped = || PdfError::Failed("the export stopped before it finished".to_string());
    receiver.await.unwrap_or_else(|_| Err(stopped()))
}

/// What the command line says of a finished export: how many PDFs were
/// written and how long it took, the drawings that got none with the
/// reason -- the first ten; the index has them all --, how many pages
/// miss symbols, and where the index is.
pub fn summary_lines(outcome: &PdfOutcome) -> Vec<String> {
    let seconds = outcome.elapsed.as_secs_f64();
    let (written, drawings) = (outcome.written, outcome.pages.len());
    let mut lines = vec![format!(
        "wrote {written} of {drawings} drawing(s) as PDF in {seconds:.1} s"
    )];
    if outcome.failed > 0 {
        let failed = outcome.failed;
        lines.push(format!("{failed} drawing(s) could not be written:"));
        let unwritten = |page: &&PdfPage| page.pid.is_some() && page.pdf.is_none();
        for page in outcome.pages.iter().filter(unwritten).take(FAILURES_LISTED) {
            let why = page.error.as_deref().unwrap_or("no reason given");
            lines.push(format!("  {}: {why}", page.name));
        }
        if failed > FAILURES_LISTED {
            let more = failed - FAILURES_LISTED;
            lines.push(format!("  ... and {more} more, see the index"));
        }
    }
    if outcome.without_file > 0 {
        let unfiled = outcome.without_file;
        lines.push(format!("{unfiled} drawing(s) have no file in the backup"));
    }
    let short = |page: &&PdfPage| !page.missing_symbols.is_empty();
    let short = outcome.pages.iter().filter(short).count();
    lines.push(format!("{short} page(s) miss symbols"));
    lines.push(format!("index: {}", outcome.index_path.display()));
    lines
}

#[cfg(test)]
mod tests {
    use super::*;
    use iced::futures::executor::block_on;

    /// An empty folder of one test's own.
    fn scratch(name: &str) -> PathBuf {
        let name = format!("ocs-backup-pdf-{name}-{}", std::process::id());
        let root = std::env::temp_dir().join(name);
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        root
    }

    /// A request for a made-up package under `root`: each of `files` is a
    /// drawing in the restore folder, each of `unfiled` a registered
    /// drawing the backup holds no file of. The PDFs go to `root/out`.
    fn package(root: &Path, files: &[&str], unfiled: &[&str]) -> PdfRequest {
        let restored = root.join("restored");
        std::fs::create_dir_all(root.join("package")).unwrap();
        let mut drawings = Vec::new();
        for file in files {
            // By its parts, so that the path is spelled the way the machine
            // spells one: with `\` on Windows.
            let path = file
                .split('/')
                .fold(restored.clone(), |path, part| path.join(part));
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(&path, format!("drawing {file}")).unwrap();
            drawings.push(PdfDrawing {
                name: format!("registered {file}"),
                pid: Some(path),
            });
        }
        for name in unfiled {
            drawings.push(PdfDrawing {
                name: (*name).to_string(),
                pid: None,
            });
        }
        PdfRequest {
            package_dir: root.join("package"),
            restored,
            out_dir: root.join("out"),
            drawings,
        }
    }

    /// A plot that needs no real drawing: the "PDF" is the drawing's own
    /// bytes, and a drawing whose name starts with `bad` cannot be plotted.
    fn fake(drawing: &Path, target: &Path) -> Result<PidPdfReceipt, String> {
        let name = drawing.file_stem().unwrap().to_string_lossy();
        if name.starts_with("bad") {
            return Err(format!("no face for the text of {name}"));
        }
        let bytes = std::fs::read(drawing).map_err(|error| error.to_string())?;
        std::fs::write(target, &bytes).map_err(|error| error.to_string())?;
        Ok(PidPdfReceipt {
            path: target.to_path_buf(),
            bytes: bytes.len(),
            primitives: 7,
            page_mm: Some([841.0, 594.0]),
            template: Some(Path::new("Ref").join("Template Files").join("A1.pid")),
            symbols_found: 3,
            symbols_referenced: 4,
            missing_symbols: vec!["valve.sym".to_string()],
            corpus: PathBuf::from("corpus"),
            notes: vec!["border templates of the reference folder".to_string()],
            ledger: PageLedger {
                entities: 9,
                on_page: 5,
                intentional: 3,
                suspect: 1,
                unattributed: 0,
            },
        })
    }

    /// [`export_with`] and the made-up plot, with no one reading the count.
    fn run(request: &PdfRequest) -> Result<PdfOutcome, PdfError> {
        export_with(request, &PdfProgress::default(), fake)
    }

    /// What `folder` holds, by name: each file with its bytes, a folder
    /// with none.
    fn listing(folder: &Path) -> Vec<(String, Option<Vec<u8>>)> {
        let mut entries: Vec<_> = std::fs::read_dir(folder)
            .unwrap()
            .map(|entry| {
                let path = entry.unwrap().path();
                let name = path.file_name().unwrap().to_string_lossy().into_owned();
                (name, std::fs::read(&path).ok())
            })
            .collect();
        entries.sort();
        entries
    }

    fn names(folder: &Path) -> Vec<String> {
        let entries = listing(folder);
        entries.into_iter().map(|(name, _)| name).collect()
    }

    /// The index `outcome` wrote, parsed.
    fn index_of(outcome: &PdfOutcome) -> serde_json::Value {
        serde_json::from_slice(&std::fs::read(&outcome.index_path).unwrap()).unwrap()
    }

    #[test]
    fn a_target_that_is_a_file_or_lies_in_the_package_is_refused() {
        let root = scratch("target");
        let mut request = package(&root, &["00/00/A.pid"], &[]);
        let file = root.join("taken.txt");
        std::fs::write(&file, "x").unwrap();
        let mut refused = vec![
            file.clone(),
            file.join("below"),
            request.package_dir.clone(),
            request.package_dir.join("pdf"),
            request.restored.join("00").join("pdf"),
            // Another spelling of a folder in the package.
            root.join("out").join("..").join("package").join("pdf"),
        ];
        if cfg!(windows) {
            refused.push(root.join("PACKAGE").join("pdf"));
        }
        for target in refused {
            request.out_dir.clone_from(&target);
            let error = check(&request).unwrap_err();
            assert_eq!(
                error.code(),
                "SPPID_PDF_TARGET_INVALID",
                "{}",
                target.display()
            );
            assert_eq!(run(&request).unwrap_err(), error);
        }
        assert_eq!(std::fs::read(&file).unwrap(), b"x");
        assert_eq!(names(&request.package_dir), Vec::<String>::new());
        assert_eq!(names(&request.restored.join("00")), ["00"]);

        // The folder next to the package is none of these, and a check of
        // it writes nothing.
        request.out_dir = root.join("out");
        assert_eq!(check(&request), Ok(()));
        assert!(!root.join("out").exists());
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn files_the_export_would_write_stop_it_before_anything_is_written() {
        let root = scratch("present");
        let request = package(&root, &["00/00/A.pid", "00/00/B.pid", "00/02/C.pid"], &[]);
        std::fs::create_dir_all(&request.out_dir).unwrap();
        std::fs::write(request.out_dir.join("B.pdf"), "an older B").unwrap();
        std::fs::write(request.out_dir.join("other.txt"), "kept").unwrap();
        let before = listing(&request.out_dir);

        let error = check(&request).unwrap_err();
        assert_eq!(error.code(), "SPPID_PDF_TARGET_HAS_FILES");
        let said = error.to_string();
        assert!(
            said.contains("B.pdf") && !said.contains("other.txt"),
            "{said}"
        );
        let mut plotted = 0;
        let again = export_with(&request, &PdfProgress::default(), |drawing, target| {
            plotted += 1;
            fake(drawing, target)
        });
        assert_eq!(again.unwrap_err(), error);
        assert_eq!(plotted, 0);
        assert_eq!(listing(&request.out_dir), before);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn an_older_index_and_a_page_left_half_written_are_in_the_way_too() {
        let root = scratch("leftover");
        let files: Vec<String> = (1..=8).map(|n| format!("00/00/D{n}.pid")).collect();
        let files: Vec<&str> = files.iter().map(String::as_str).collect();
        let mut request = package(&root, &files, &[]);
        let leftovers = [
            ("index", INDEX_NAME),
            ("page", "D3.pdf.tmp"),
            ("half", "_PdfIndex.json.tmp"),
        ];
        for (folder, leftover) in leftovers {
            request.out_dir = root.join(folder);
            std::fs::create_dir_all(&request.out_dir).unwrap();
            std::fs::write(request.out_dir.join(leftover), "left over").unwrap();
            let error = check(&request).unwrap_err();
            let PdfError::TargetHasFiles { names, count, .. } = &error else {
                panic!("{error:?}");
            };
            assert_eq!(*names, [leftover]);
            assert_eq!(*count, 1);
        }

        // Only the first few of many are named; the rest are counted.
        request.out_dir = root.join("many");
        std::fs::create_dir_all(&request.out_dir).unwrap();
        for n in 1..=8 {
            std::fs::write(request.out_dir.join(format!("D{n}.pdf")), "older").unwrap();
        }
        let error = check(&request).unwrap_err();
        let PdfError::TargetHasFiles { names, count, .. } = &error else {
            panic!("{error:?}");
        };
        assert_eq!((names.len(), *count), (FILES_NAMED, 8));
        assert_eq!(names[..2], ["D1.pdf", "D2.pdf"]);
        assert!(error.to_string().contains("D5.pdf and 3 more"), "{error}");
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn a_package_without_a_drawing_file_has_nothing_to_plot() {
        let root = scratch("nothing");
        let mut request = package(&root, &[], &["P-001", "P-002"]);
        assert_eq!(
            check(&request).unwrap_err().code(),
            "SPPID_PDF_NOTHING_TO_PLOT"
        );
        // A drawing whose file is gone from the restore folder is no file
        // to plot either.
        request.drawings.push(PdfDrawing {
            name: "P-003".to_string(),
            pid: Some(request.restored.join("00").join("gone.pid")),
        });
        assert_eq!(
            check(&request).unwrap_err().code(),
            "SPPID_PDF_NOTHING_TO_PLOT"
        );
        assert_eq!(
            run(&request).unwrap_err().code(),
            "SPPID_PDF_NOTHING_TO_PLOT"
        );
        assert!(!request.out_dir.exists());
        // And that is said first, whatever the folder: there is nothing to
        // put in one.
        request.out_dir = request.package_dir.join("pdf");
        assert_eq!(
            check(&request).unwrap_err().code(),
            "SPPID_PDF_NOTHING_TO_PLOT"
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn other_files_in_the_folder_stay_as_they_are() {
        let root = scratch("beside");
        let request = package(&root, &["00/00/A.pid", "00/00/B.pid"], &[]);
        std::fs::create_dir_all(request.out_dir.join("sub")).unwrap();
        for (name, text) in [
            ("_Index.json", "{}"),
            ("A_Data.xml", "<a/>"),
            ("notes.txt", "mine"),
        ] {
            std::fs::write(request.out_dir.join(name), text).unwrap();
        }
        let before = listing(&request.out_dir);

        let outcome = run(&request).unwrap();
        assert_eq!(outcome.written, 2);
        let after = listing(&request.out_dir);
        for entry in &before {
            assert!(after.contains(entry), "{} is not as it was", entry.0);
        }
        let all = [
            "A.pdf",
            "A_Data.xml",
            "B.pdf",
            "_Index.json",
            INDEX_NAME,
            "notes.txt",
            "sub",
        ];
        assert_eq!(names(&request.out_dir), all);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn the_index_accounts_for_every_drawing() {
        let root = scratch("index");
        let files = ["test/U01/C.pid", "00/02/B.pid", "00/00/A.pid"];
        let request = package(&root, &files, &["P-9", "P-1"]);
        let outcome = run(&request).unwrap();
        assert_eq!(
            (outcome.written, outcome.failed, outcome.without_file),
            (3, 0, 2)
        );
        assert_eq!(outcome.pages.len(), request.drawings.len());
        assert_eq!(outcome.index_path, request.out_dir.join(INDEX_NAME));
        for page in &outcome.pages {
            assert_ne!(page.pdf.is_some(), page.error.is_some(), "{page:?}");
        }
        let pdfs = names(&request.out_dir);
        let pdfs = pdfs.iter().filter(|name| name.ends_with(".pdf")).count();
        assert_eq!(pdfs, outcome.written);

        let index = index_of(&outcome);
        assert_eq!(index["package"], request.package_dir.display().to_string());
        let counts = ["drawings", "written", "failed", "without_file"].map(|key| &index[key]);
        assert_eq!(counts, [5, 3, 0, 2]);
        // Drawings with a file by where they lie in the package, under
        // their files' names; then the ones without, under the registered.
        let pages = index["pages"].as_array().unwrap();
        let listed: Vec<_> = pages
            .iter()
            .map(|page| {
                (
                    page["name"].as_str(),
                    page["pid"].as_str(),
                    page["pdf"].as_str(),
                )
            })
            .collect();
        let expected = [
            (Some("A"), Some("00/00/A.pid"), Some("A.pdf")),
            (Some("B"), Some("00/02/B.pid"), Some("B.pdf")),
            (Some("C"), Some("test/U01/C.pid"), Some("C.pdf")),
            (Some("P-1"), None, None),
            (Some("P-9"), None, None),
        ];
        assert_eq!(listed, expected);
        assert!(pages[0]["error"].is_null());
        assert!(pages[3]["pid"].is_null() && pages[3]["pdf"].is_null());
        let error = pages[3]["error"].as_str().unwrap();
        assert!(error.contains("no file"), "{error}");
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn an_entry_of_the_index_says_what_the_plot_reported() {
        let root = scratch("entry");
        let request = package(&root, &["00/00/A.pid", "00/00/bad.pid"], &[]);
        let outcome = run(&request).unwrap();
        let text = std::fs::read_to_string(&outcome.index_path).unwrap();
        assert!(
            text.starts_with("{\n  \"package\": ") && text.ends_with("\n}\n"),
            "{text}"
        );
        // The fields of an entry, in the order the specification lists them.
        let keys: Vec<&str> = text
            .lines()
            .filter_map(|line| line.strip_prefix("      \""))
            .filter_map(|rest| rest.split('"').next())
            .collect();
        let fields = [
            "name",
            "pid",
            "pdf",
            "bytes",
            "primitives",
            "page_mm",
            "template",
            "symbols_found",
            "symbols_referenced",
            "missing_symbols",
            "ledger",
            "notes",
            "error",
        ];
        assert_eq!(keys, [fields, fields].concat());

        let written = serde_json::json!({
            "name": "A",
            "pid": "00/00/A.pid",
            "pdf": "A.pdf",
            "bytes": "drawing 00/00/A.pid".len(),
            "primitives": 7,
            "page_mm": [841.0, 594.0],
            "template": "A1.pid",
            "symbols_found": 3,
            "symbols_referenced": 4,
            "missing_symbols": ["valve.sym"],
            "ledger": {
                "entities": 9,
                "on_page": 5,
                "intentional": 3,
                "suspect": 1,
                "unattributed": 0,
            },
            "notes": ["border templates of the reference folder"],
            "error": null,
        });
        let index = index_of(&outcome);
        assert_eq!(index["pages"][0], written);
        // A drawing without a PDF has nothing to report but the reason.
        let unwritten = serde_json::json!({
            "name": "bad",
            "pid": "00/00/bad.pid",
            "pdf": null,
            "bytes": 0,
            "primitives": 0,
            "page_mm": null,
            "template": null,
            "symbols_found": 0,
            "symbols_referenced": 0,
            "missing_symbols": [],
            "ledger": {
                "entities": 0,
                "on_page": 0,
                "intentional": 0,
                "suspect": 0,
                "unattributed": 0,
            },
            "notes": [],
            "error": "no face for the text of bad",
        });
        assert_eq!(index["pages"][1], unwritten);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn a_drawing_is_listed_by_where_it_lies_in_the_package() {
        let root = scratch("where");
        let mut request = package(&root, &["00/00/A.pid"], &[]);
        // The package's own folder lies in the restore folder here, with a
        // drawing in it; one more drawing lies in neither.
        request.package_dir = request.restored.join("Backup");
        let own = request.package_dir.join("00").join("02").join("K.pid");
        let stray = root.join("elsewhere").join("L.pid");
        for path in [&own, &stray] {
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, "drawing").unwrap();
            request.drawings.push(PdfDrawing {
                name: String::new(),
                pid: Some(path.clone()),
            });
        }
        let outcome = run(&request).unwrap();
        let listed = |name: &str| {
            let page = outcome.pages.iter().find(|page| page.name == name);
            page.and_then(|page| page.pid.clone())
        };
        assert_eq!(listed("A").as_deref(), Some("00/00/A.pid"));
        // Under the package's folder before the restore folder's.
        assert_eq!(listed("K").as_deref(), Some("00/02/K.pid"));
        // A file in neither keeps its whole path.
        let whole = stray.to_string_lossy().replace('\\', "/");
        assert_eq!(listed("L"), Some(whole));
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn a_drawing_that_cannot_be_plotted_is_named_and_the_rest_are_written() {
        let root = scratch("failed");
        let files = ["00/00/A.pid", "00/00/bad one.pid", "00/02/C.pid"];
        let request = package(&root, &files, &[]);
        let outcome = run(&request).unwrap();
        assert_eq!(
            (outcome.written, outcome.failed, outcome.without_file),
            (2, 1, 0)
        );
        assert_eq!(names(&request.out_dir), ["A.pdf", "C.pdf", INDEX_NAME]);
        let page = &outcome.pages[1];
        assert_eq!((page.name.as_str(), page.pdf.as_deref()), ("bad one", None));
        assert_eq!(
            page.error.as_deref(),
            Some("no face for the text of bad one")
        );
        let index = index_of(&outcome);
        assert_eq!(index["failed"], 1);
        assert_eq!(
            index["pages"][1]["error"],
            "no face for the text of bad one"
        );
        let summary = summary_lines(&outcome).join("\n");
        assert!(
            summary.contains("  bad one: no face for the text of bad one"),
            "{summary}"
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn a_plot_that_panics_costs_only_its_own_drawing() {
        let root = scratch("panic");
        let files = ["00/00/A.pid", "00/00/B.pid", "00/00/C.pid"];
        let request = package(&root, &files, &[]);
        let outcome = export_with(&request, &PdfProgress::default(), |drawing, target| {
            if drawing.ends_with("B.pid") {
                panic!("a record of B is out of range");
            }
            fake(drawing, target)
        })
        .unwrap();
        assert_eq!((outcome.written, outcome.failed), (2, 1));
        assert_eq!(names(&request.out_dir), ["A.pdf", "C.pdf", INDEX_NAME]);
        let error = outcome.pages[1].error.as_deref().unwrap();
        assert!(error.contains("a record of B is out of range"), "{error}");
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn a_page_goes_by_another_name_until_it_is_whole() {
        let root = scratch("whole");
        let request = package(&root, &["00/00/A.pid", "00/00/B.pid"], &[]);
        let mut targets = Vec::new();
        let outcome = export_with(&request, &PdfProgress::default(), |drawing, target| {
            targets.push(target.to_path_buf());
            std::fs::write(target, "half a page").unwrap();
            if drawing.ends_with("B.pid") {
                return Err("the plot broke off".to_string());
            }
            fake(drawing, target)
        })
        .unwrap();
        let unfinished = ["A.pdf.tmp", "B.pdf.tmp"].map(|name| request.out_dir.join(name));
        assert_eq!(targets, unfinished);
        // What is left is whole pages and the index: no half of B.
        assert_eq!(names(&request.out_dir), ["A.pdf", INDEX_NAME]);
        let page = std::fs::read(request.out_dir.join("A.pdf")).unwrap();
        assert_eq!(page, b"drawing 00/00/A.pid");
        assert_eq!((outcome.written, outcome.failed), (1, 1));
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn the_second_drawing_of_a_name_is_not_written() {
        let root = scratch("twice");
        let files = ["00/02/d04.pid", "00/00/D04.pid", "00/00/E05.pid"];
        let request = package(&root, &files, &[]);
        let outcome = run(&request).unwrap();
        assert_eq!((outcome.written, outcome.failed), (2, 1));
        assert_eq!(names(&request.out_dir), ["D04.pdf", "E05.pdf", INDEX_NAME]);
        let page = std::fs::read(request.out_dir.join("D04.pdf")).unwrap();
        assert_eq!(page, b"drawing 00/00/D04.pid");
        // The one that lies later in the package is the one left out, and
        // its entry says which drawing has the name.
        let second = &outcome.pages[2];
        assert_eq!(second.pid.as_deref(), Some("00/02/d04.pid"));
        assert_eq!(second.pdf, None);
        let error = second.error.as_deref().unwrap();
        assert!(
            error.contains("D04.pdf") && error.contains("00/00/D04.pid"),
            "{error}"
        );
        assert_eq!(index_of(&outcome)["pages"][2]["error"], error);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn the_same_package_gives_the_same_index_byte_for_byte() {
        let root = scratch("same");
        let files = ["00/00/A.pid", "00/00/bad.pid", "00/02/A.pid"];
        let mut request = package(&root, &files, &["P-1"]);
        let mut indexes = Vec::new();
        for (folder, turned) in [("one", false), ("two", false), ("three", true)] {
            request.out_dir = root.join(folder);
            if turned {
                // The index has its own order, not the one the drawings
                // were given in.
                request.drawings.reverse();
            }
            let outcome = run(&request).unwrap();
            indexes.push(std::fs::read(&outcome.index_path).unwrap());
        }
        assert!(indexes[0].len() > 1000, "{} bytes", indexes[0].len());
        assert!(
            indexes[0] == indexes[1],
            "the second export wrote another index"
        );
        assert!(indexes[0] == indexes[2], "the order of the drawings shows");
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn the_count_of_drawings_done_only_grows_and_ends_at_the_total() {
        let root = scratch("progress");
        let files = ["00/00/A.pid", "00/00/bad.pid", "00/00/C.pid"];
        let request = package(&root, &files, &["P-1"]);
        let progress = PdfProgress::default();
        assert_eq!((progress.done(), progress.total()), (0, 0));
        let watching = progress.clone();
        let mut seen = Vec::new();
        let outcome = export_with(&request, &progress, |drawing, target| {
            seen.push((watching.done(), watching.total()));
            fake(drawing, target)
        })
        .unwrap();
        assert_eq!(seen, [(0, 4), (1, 4), (2, 4)]);
        assert_eq!(
            (progress.done(), progress.total()),
            (4, outcome.pages.len())
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn an_index_that_cannot_be_written_fails_the_export() {
        let root = scratch("no-index");
        let mut request = package(&root, &["00/00/A.pid"], &[]);
        // A folder turns up while the pages are plotted: where the index
        // belongs, or where it is written until it is whole.
        for (folder, taken) in [("one", INDEX_NAME), ("two", "_PdfIndex.json.tmp")] {
            request.out_dir = root.join(folder);
            let in_the_way = request.out_dir.join(taken);
            let outcome = export_with(&request, &PdfProgress::default(), |drawing, target| {
                std::fs::create_dir_all(&in_the_way).unwrap();
                fake(drawing, target)
            });
            let error = outcome.unwrap_err();
            assert_eq!(error.code(), "SPPID_PDF_FAILED");
            assert!(error.to_string().contains(INDEX_NAME), "{error}");
            // The page stays, and no index or half of one beside it.
            assert_eq!(names(&request.out_dir), ["A.pdf", taken]);
            assert!(in_the_way.is_dir());
        }
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn every_error_has_its_code() {
        let in_the_way = PdfError::TargetHasFiles {
            folder: PathBuf::from("out"),
            names: vec!["A.pdf".to_string()],
            count: 1,
        };
        let errors = [
            (PdfError::NoPackage, "SPPID_PDF_NO_PACKAGE"),
            (
                PdfError::NothingToPlot { drawings: 0 },
                "SPPID_PDF_NOTHING_TO_PLOT",
            ),
            (
                PdfError::TargetInvalid("a file".to_string()),
                "SPPID_PDF_TARGET_INVALID",
            ),
            (in_the_way, "SPPID_PDF_TARGET_HAS_FILES"),
            (PdfError::Busy, "SPPID_PDF_BUSY"),
            (PdfError::Failed("no index".to_string()), "SPPID_PDF_FAILED"),
        ];
        for (error, code) in errors {
            assert_eq!(error.code(), code);
            assert!(!error.to_string().is_empty(), "{code}");
        }
    }

    #[test]
    fn the_summary_names_ten_drawings_without_a_pdf_and_counts_the_rest() {
        let root = scratch("summary");
        let mut files: Vec<String> = (1..=12).map(|n| format!("00/00/bad {n:02}.pid")).collect();
        files.push("00/00/A.pid".to_string());
        let files: Vec<&str> = files.iter().map(String::as_str).collect();
        let request = package(&root, &files, &["P-1"]);
        let outcome = run(&request).unwrap();
        let lines = summary_lines(&outcome);
        let text = lines.join("\n");
        let seconds = outcome.elapsed.as_secs_f64();
        assert_eq!(
            lines[0],
            format!("wrote 1 of 14 drawing(s) as PDF in {seconds:.1} s")
        );
        assert_eq!(lines[1], "12 drawing(s) could not be written:", "{text}");
        let named = |line: &&String| line.contains(": no face for the text of bad ");
        assert_eq!(lines.iter().filter(named).count(), 10, "{text}");
        assert_eq!(
            lines[2], "  bad 01: no face for the text of bad 01",
            "{text}"
        );
        assert_eq!(
            lines[11], "  bad 10: no face for the text of bad 10",
            "{text}"
        );
        assert_eq!(lines[12], "  ... and 2 more, see the index", "{text}");
        assert_eq!(
            lines[13], "1 drawing(s) have no file in the backup",
            "{text}"
        );
        assert_eq!(lines[14], "1 page(s) miss symbols", "{text}");
        assert_eq!(
            lines[15],
            format!("index: {}", outcome.index_path.display())
        );
        assert_eq!(lines.len(), 16, "{text}");
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn a_background_export_runs_on_a_thread_of_its_own() {
        let root = scratch("thread");
        let request = package(&root, &["00/00/A.pid"], &[]);
        let (named, name) = std::sync::mpsc::channel();
        let job = move || {
            export_with(&request, &PdfProgress::default(), |drawing, target| {
                let thread = std::thread::current();
                named.send(thread.name().map(str::to_string)).unwrap();
                fake(drawing, target)
            })
        };
        let outcome = block_on(in_background(job)).unwrap();
        assert_eq!(outcome.written, 1);
        assert_eq!(name.recv().unwrap().as_deref(), Some(THREAD_NAME));
        assert_ne!(std::thread::current().name(), Some(THREAD_NAME));

        // A thread that dies is an error, not a wait without end.
        let died = block_on(in_background(|| panic!("the export thread died")));
        assert_eq!(died.unwrap_err().code(), "SPPID_PDF_FAILED");
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn the_real_plot_is_given_the_restore_folder_and_the_fonts_of_the_package() {
        let root = scratch("real");
        let request = package(&root, &["00/00/A.pid", "00/00/B.pid"], &["P-1"]);
        let (project, fonts) = plot_sources(&request);
        assert_eq!(project, request.restored);
        assert_eq!(fonts, [request.package_dir.join("fonts")]);

        // The restore folder has border templates of its own, so the plot
        // looks nowhere else; what it is given is no drawing, and it says so
        // for each.
        let templates = request.restored.join("Ref").join("Template Files");
        std::fs::create_dir_all(&templates).unwrap();
        std::fs::write(templates.join("A1.igr"), b"").unwrap();
        let progress = PdfProgress::default();
        let export = export_in_background(request.clone(), progress.clone());
        let outcome = block_on(export).unwrap();
        assert_eq!(
            (outcome.written, outcome.failed, outcome.without_file),
            (0, 2, 1)
        );
        assert_eq!((progress.done(), progress.total()), (3, 3));
        assert_eq!(names(&request.out_dir), [INDEX_NAME]);
        let error = outcome.pages[0].error.as_deref().unwrap();
        assert!(!error.is_empty() && !error.contains("panic"), "{error}");

        // What stops an export stops a background one the same way.
        let again = block_on(export_in_background(request, PdfProgress::default()));
        assert_eq!(again.unwrap_err().code(), "SPPID_PDF_TARGET_HAS_FILES");
        let _ = std::fs::remove_dir_all(&root);
    }
}
