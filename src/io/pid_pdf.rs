//! PIDPLOT's engine: a `.pid` drawing plotted to PDF by pid-parse's own
//! plotter (`pid-plot`).
//!
//! This is the sheet the way SmartPlant prints it -- the border template the
//! frame record links to, the library bodies of the placed symbols, and the
//! typefaces the style table names, subset into the file -- made from the
//! `.pid` on disk. The imported drawing and the PLOT pipeline only
//! approximate that page, and edits made since the file was opened are not
//! in it.
//!
//! Reference data comes from the drawing's own project first (the nearest
//! folder above it holding `Ref`). A project that lacks the border templates
//! -- a SmartPlant backup keeps them inside a `RefData~*` archive, not under
//! `Ref/Template Files` -- takes them, and fonts it does not carry, from the
//! reference folder ([`reference_root`]).

use std::io::Read;
use std::path::{Path, PathBuf};

/// How many folders above the drawing are searched for the project's `Ref`.
const CORPUS_SEARCH_DEPTH: usize = 5;

/// Names the reference folder; see [`reference_root`].
const REFERENCE_ENV: &str = "PID_PLOT_CORPUS";

/// The reference folder when [`REFERENCE_ENV`] is not set: the plant test
/// data on the machine this was set up for (symbols under `Ref/Symbols`,
/// templates in its `RefData~*` archives, plot faces under `fonts`).
const DEFAULT_REFERENCE_ROOT: &str = r"D:\work\plant-code\cad\pid-test-data";

/// Largest template file read out of an archive; real ones are under 200 KB.
const TEMPLATE_BYTES_MAX: u64 = 16 << 20;

/// What one plot wrote.
#[derive(Debug, Clone)]
pub struct PidPdfReceipt {
    pub path: PathBuf,
    pub bytes: usize,
    pub primitives: usize,
    /// Page size the PDF carries, in millimetres.
    pub page_mm: Option<[f64; 2]>,
    /// Border template the furniture came from, when the sheet has one.
    pub template: Option<PathBuf>,
    pub symbols_found: usize,
    pub symbols_referenced: usize,
    /// Symbol files the drawing names and no library root holds.
    pub missing_symbols: Vec<String>,
    /// The folder border templates were looked up under.
    pub corpus: PathBuf,
    /// Where reference data came from beyond the drawing's own project.
    pub notes: Vec<String>,
}

pub fn is_pid(path: &Path) -> bool {
    path.extension().is_some_and(|extension| extension.eq_ignore_ascii_case("pid"))
}

/// Where the plot goes when PIDPLOT is given no file: next to the drawing,
/// under a name of its own. `<name>.pdf` there is usually SmartPlant's
/// reference print of the same sheet, which must not be overwritten.
pub fn default_target(drawing: &Path) -> PathBuf {
    let stem = drawing
        .file_stem()
        .map_or_else(|| "drawing".into(), |stem| stem.to_string_lossy());
    drawing.with_file_name(format!("{stem}.pidplot.pdf"))
}

/// The drawing's project: the nearest folder, from the drawing's own folder
/// up, that holds a `Ref` folder (where a SmartPlant project keeps `Symbols`
/// and `Template Files`, next to its drawings), else the drawing's folder.
/// pid-plot looks for symbols in `<corpus>/Ref`, templates in
/// `<corpus>/Ref/Template Files` and fonts in `<corpus>/fonts`;
/// `PID_SYMBOL_LIBRARY` and `PID_PLOT_FONTS` still win.
pub fn corpus_for(drawing: &Path) -> PathBuf {
    let own = drawing.parent().map_or_else(PathBuf::new, Path::to_path_buf);
    let mut dir = Some(own.as_path());
    for _ in 0..CORPUS_SEARCH_DEPTH {
        let Some(at) = dir else { break };
        if at.join("Ref").is_dir() {
            return at.to_path_buf();
        }
        dir = at.parent();
    }
    own
}

/// The reference folder: `PID_PLOT_CORPUS`, else [`DEFAULT_REFERENCE_ROOT`],
/// whichever exists.
pub fn reference_root() -> Option<PathBuf> {
    std::env::var_os(REFERENCE_ENV)
        .map(PathBuf::from)
        .filter(|root| root.is_dir())
        .or_else(|| Some(PathBuf::from(DEFAULT_REFERENCE_ROOT)).filter(|root| root.is_dir()))
}

/// Where one plot takes its reference data from.
struct Sources {
    /// The folder pid-plot reads border templates under.
    corpus: PathBuf,
    /// Symbol roots, when they are stated rather than left to pid-plot.
    symbol_roots: Option<Vec<PathBuf>>,
    fonts: Vec<PathBuf>,
    notes: Vec<String>,
}

fn push_unique(dirs: &mut Vec<PathBuf>, dir: PathBuf) {
    if !dirs.contains(&dir) {
        dirs.push(dir);
    }
}

fn sources_for(drawing: &Path) -> Sources {
    let project = corpus_for(drawing);
    let reference = reference_root();
    let mut fonts = ::pid_plot::fonts::font_dirs(Some(&project));
    if let Some(root) = &reference {
        push_unique(&mut fonts, root.join("fonts"));
    }
    // Last resort: Arial, Arial Narrow and FangSong where Windows installs
    // them. SimSun is not among them: Windows ships it as a collection
    // (`simsun.ttc`), which pid-plot does not embed.
    if let Some(windows) = std::env::var_os("WINDIR") {
        push_unique(&mut fonts, PathBuf::from(windows).join("Fonts"));
    }
    let reference = match reference {
        Some(root) if !has_templates(&project) => root,
        _ => {
            return Sources {
                corpus: project,
                symbol_roots: None,
                fonts,
                notes: Vec::new(),
            };
        }
    };
    let mut notes = Vec::new();
    let corpus = if has_templates(&reference) {
        notes.push(format!("border templates from {}", reference.display()));
        reference.clone()
    } else {
        match archived_templates(&reference) {
            Ok(Some((cache, archive))) => {
                notes.push(format!(
                    "border templates read out of {} into {}",
                    archive.display(),
                    cache.display()
                ));
                cache
            }
            Ok(None) => {
                notes.push(format!(
                    "no border templates under {} or in its RefData archives",
                    reference.display()
                ));
                project.clone()
            }
            Err(error) => {
                notes.push(format!("border templates: {error}"));
                project.clone()
            }
        }
    };
    let roots = symbol_roots(drawing, &reference);
    notes.push(format!(
        "symbol library: {}",
        roots
            .iter()
            .map(|root| root.display().to_string())
            .collect::<Vec<_>>()
            .join("; ")
    ));
    Sources {
        corpus,
        symbol_roots: Some(roots),
        fonts,
        notes,
    }
}

fn holds_templates(dir: &Path) -> bool {
    std::fs::read_dir(dir).is_ok_and(|entries| {
        entries.flatten().any(|entry| {
            entry
                .path()
                .extension()
                .is_some_and(|extension| extension.eq_ignore_ascii_case("igr"))
        })
    })
}

/// Whether pid-plot finds a border template under `corpus`: the same four
/// folders it searches (`<corpus>/Ref/Template Files`, `<corpus>/Ref`, and
/// both under the corpus's parent).
fn has_templates(corpus: &Path) -> bool {
    let parent = corpus.parent();
    [
        Some(corpus.join("Ref").join("Template Files")),
        Some(corpus.join("Ref")),
        parent.map(|parent| parent.join("Ref").join("Template Files")),
        parent.map(|parent| parent.join("Ref")),
    ]
    .into_iter()
    .flatten()
    .any(|dir| holds_templates(&dir))
}

fn push_symbol_dirs(roots: &mut Vec<PathBuf>, holder: &Path) {
    let Ok(entries) = std::fs::read_dir(holder) else {
        return;
    };
    let mut found: Vec<PathBuf> = entries
        .flatten()
        .filter(|entry| entry.file_type().is_ok_and(|kind| kind.is_dir()))
        .filter(|entry| entry.file_name().to_string_lossy().to_lowercase().contains("symbol"))
        .map(|entry| entry.path())
        .collect();
    found.sort();
    for root in found {
        push_unique(roots, root);
    }
}

/// Symbol roots once the reference folder joins in: `PID_SYMBOL_LIBRARY`
/// alone when it is set (pid-plot's own rule), else the symbol folders from
/// the drawing up -- in each folder and in a `Ref` beside it, as the importer
/// finds them -- then the reference folder's. An earlier root wins a symbol
/// both hold, so the drawing's own project decides what it looks like.
fn symbol_roots(drawing: &Path, reference: &Path) -> Vec<PathBuf> {
    if let Some(list) = std::env::var_os("PID_SYMBOL_LIBRARY") {
        let roots: Vec<PathBuf> = std::env::split_paths(&list)
            .filter(|root| root.is_dir())
            .collect();
        if !roots.is_empty() {
            return roots;
        }
    }
    let mut roots = Vec::new();
    let mut dir = drawing.parent();
    for _ in 0..CORPUS_SEARCH_DEPTH {
        let Some(at) = dir else { break };
        push_symbol_dirs(&mut roots, at);
        push_symbol_dirs(&mut roots, &at.join("Ref"));
        dir = at.parent();
    }
    push_symbol_dirs(&mut roots, &reference.join("Ref"));
    roots
}

/// The `RefData~<schema>~<id>` files of a SmartPlant backup, path-sorted.
fn refdata_archives(root: &Path) -> Vec<PathBuf> {
    let Ok(entries) = std::fs::read_dir(root) else {
        return Vec::new();
    };
    let mut archives: Vec<PathBuf> = entries
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| {
            path.is_file()
                && path
                    .file_name()
                    .is_some_and(|name| name.to_string_lossy().starts_with("RefData~"))
        })
        .collect();
    archives.sort();
    archives
}

/// The `.igr` files of one archive, by file name. `None` when the file is
/// not a zip (a `RefData~` payload can also be a compound file or text).
fn templates_in(archive: &Path) -> Option<Vec<(String, Vec<u8>)>> {
    let mut file = std::fs::File::open(archive).ok()?;
    let mut magic = [0u8; 4];
    file.read_exact(&mut magic).ok()?;
    if &magic != b"PK\x03\x04" {
        return None;
    }
    let file = std::fs::File::open(archive).ok()?;
    let mut entries = zip::ZipArchive::new(file).ok()?;
    let mut templates = Vec::new();
    for index in 0..entries.len() {
        let Ok(entry) = entries.by_index(index) else {
            continue;
        };
        let name = entry.name().replace('\\', "/");
        let Some(base) = name
            .rsplit('/')
            .next()
            .filter(|base| base.to_ascii_lowercase().ends_with(".igr"))
            .map(str::to_string)
        else {
            continue;
        };
        let mut bytes = Vec::new();
        if entry.take(TEMPLATE_BYTES_MAX).read_to_end(&mut bytes).is_ok() {
            templates.push((base, bytes));
        }
    }
    Some(templates)
}

/// Read the border templates of a SmartPlant backup out of its `RefData~*`
/// archives into a cache folder laid out as a corpus
/// (`<cache>/Ref/Template Files`), and answer that folder with the archive
/// they came from. The first archive holding any `.igr` is used; the cache is
/// keyed by its path, size and time, so a later plot reuses it.
///
/// pid-plot picks a template by its extent and keeps the first of equal fits
/// in name order, so the CPE-SQ family -- the frames the reference prints are
/// drawn on -- is written under names that sort first.
fn archived_templates(root: &Path) -> Result<Option<(PathBuf, PathBuf)>, String> {
    for archive in refdata_archives(root) {
        let Some(templates) = templates_in(&archive) else {
            continue;
        };
        if templates.is_empty() {
            continue;
        }
        let cache = template_cache(&archive);
        let dir = cache.join("Ref").join("Template Files");
        std::fs::create_dir_all(&dir)
            .map_err(|error| format!("cannot create {}: {error}", dir.display()))?;
        for (name, bytes) in &templates {
            let rank = if name.to_ascii_uppercase().starts_with("CPE-SQ-") {
                1
            } else {
                2
            };
            let path = dir.join(format!("{rank} {name}"));
            if !path.is_file() {
                std::fs::write(&path, bytes)
                    .map_err(|error| format!("cannot write {}: {error}", path.display()))?;
            }
        }
        return Ok(Some((cache, archive)));
    }
    Ok(None)
}

fn template_cache(archive: &Path) -> PathBuf {
    use std::hash::{Hash, Hasher};
    let mut hasher = std::hash::DefaultHasher::new();
    archive.hash(&mut hasher);
    if let Ok(meta) = std::fs::metadata(archive) {
        meta.len().hash(&mut hasher);
        if let Ok(modified) = meta.modified() {
            modified.hash(&mut hasher);
        }
    }
    std::env::temp_dir()
        .join("ocs-pid-plot-templates")
        .join(format!("{:016x}", hasher.finish()))
}

/// Lay out `drawing`, write its PDF to `target` and say what went in.
pub fn plot_pdf(drawing: &Path, target: &Path) -> Result<PidPdfReceipt, String> {
    let sources = sources_for(drawing);
    let model = match &sources.symbol_roots {
        None => ::pid_plot::model::lay_out_drawing(drawing, &sources.corpus),
        Some(roots) => {
            let labels: Vec<String> = roots.iter().map(|root| root.display().to_string()).collect();
            ::pid_plot::model::lay_out_with_symbol_roots(
                drawing,
                &sources.corpus,
                roots.clone(),
                &labels,
            )
        }
    }
    .map_err(|error| error.to_string())?;
    let prepared = ::pid_plot::fonts::prepare_fonts(&[&model], &sources.fonts)?;
    let fonts: Vec<::pid_plot::write::PlotFont<'_>> = prepared
        .iter()
        .map(|face| ::pid_plot::write::PlotFont {
            key: &face.key,
            bytes: &face.bytes,
        })
        .collect();
    let pdf = ::pid_plot::write::write_plot(&model, &fonts).map_err(|error| error.to_string())?;
    if let Some(parent) = target.parent().filter(|parent| !parent.as_os_str().is_empty()) {
        std::fs::create_dir_all(parent)
            .map_err(|error| format!("cannot create {}: {error}", parent.display()))?;
    }
    std::fs::write(target, &pdf)
        .map_err(|error| format!("cannot write {}: {error}", target.display()))?;
    Ok(PidPdfReceipt {
        path: target.to_path_buf(),
        bytes: pdf.len(),
        primitives: model.primitives.len(),
        page_mm: model.resolved_page_mm(),
        template: model
            .template
            .as_ref()
            .map(|template| PathBuf::from(&template.path)),
        symbols_found: model.symbols_found,
        symbols_referenced: model.symbols_referenced,
        missing_symbols: model.missing_symbols.clone(),
        corpus: sources.corpus,
        notes: sources.notes,
    })
}

/// [`plot_pdf`] on a worker thread: parsing the sheet and its template and
/// subsetting CJK faces takes seconds, which the UI thread must not wait out.
pub async fn plot_pdf_in_background(
    drawing: PathBuf,
    target: PathBuf,
) -> Result<PidPdfReceipt, String> {
    let (sender, receiver) = iced::futures::channel::oneshot::channel();
    std::thread::Builder::new()
        .name("ocs-pid-plot".into())
        // The same stack the app's own threads get (RUST_MIN_STACK in
        // .cargo/config.toml): the record walk recurses through nested
        // symbol and template structures.
        .stack_size(64 * 1024 * 1024)
        .spawn(move || {
            let _ = sender.send(plot_pdf(&drawing, &target));
        })
        .map_err(|error| format!("cannot start the plot: {error}"))?;
    receiver
        .await
        .unwrap_or_else(|_| Err("the plot stopped before it finished".into()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(name: &str) -> PathBuf {
        let root = std::env::temp_dir().join(format!("ocs-pid-pdf-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        root
    }

    #[test]
    fn the_default_target_leaves_the_reference_print_alone() {
        let drawing = Path::new("D:/plant/Drawings/DWG-0201.pid");
        assert_eq!(
            default_target(drawing),
            Path::new("D:/plant/Drawings/DWG-0201.pidplot.pdf")
        );
    }

    #[test]
    fn the_corpus_is_the_nearest_folder_holding_ref() {
        let root = scratch("corpus");
        let drawings = root.join("Drawings").join("Unit 1");
        std::fs::create_dir_all(&drawings).unwrap();
        std::fs::create_dir_all(root.join("Ref").join("Symbols")).unwrap();
        let drawing = drawings.join("sheet.pid");
        assert_eq!(corpus_for(&drawing), root);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn without_ref_the_corpus_is_the_drawing_folder() {
        let root = scratch("plain");
        let drawing = root.join("sheet.pid");
        assert_eq!(corpus_for(&drawing), root);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn templates_count_only_where_pid_plot_looks() {
        let root = scratch("templates");
        let project = root.join("plant");
        std::fs::create_dir_all(project.join("Ref").join("Symbols")).unwrap();
        assert!(!has_templates(&project));
        let files = project.join("Ref").join("Template Files");
        std::fs::create_dir_all(&files).unwrap();
        std::fs::write(files.join("CPE-SQ-A1.igr"), b"").unwrap();
        assert!(has_templates(&project));
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn a_project_with_its_own_templates_keeps_pid_plots_lookup() {
        let root = scratch("own");
        let files = root.join("Ref").join("Template Files");
        std::fs::create_dir_all(&files).unwrap();
        std::fs::write(files.join("CPE-SQ-A1.igr"), b"").unwrap();
        let sources = sources_for(&root.join("sheet.pid"));
        assert_eq!(sources.corpus, root);
        assert!(sources.symbol_roots.is_none());
        assert!(sources.notes.is_empty());
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn symbol_roots_run_from_the_drawing_up_then_the_reference() {
        if std::env::var_os("PID_SYMBOL_LIBRARY").is_some() {
            return;
        }
        let root = scratch("roots");
        let project = root.join("acceptance");
        let own = project.join("Ref").join("Symbols");
        let reference = root.join("reference");
        let shared = reference.join("Ref").join("Symbols");
        std::fs::create_dir_all(&own).unwrap();
        std::fs::create_dir_all(&shared).unwrap();
        let roots = symbol_roots(&project.join("sheet.pid"), &reference);
        // The walk continues above the scratch folder, where the machine may
        // hold folders of its own; only the two ends are fixed.
        assert_eq!(roots.first(), Some(&own));
        assert_eq!(roots.last(), Some(&shared));
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn only_zip_refdata_payloads_are_searched_for_templates() {
        let root = scratch("refdata");
        std::fs::write(root.join("RefData~4~680"), b"\"Begin Rules\"").unwrap();
        std::fs::write(root.join("Manifest.txt"), b"").unwrap();
        assert_eq!(refdata_archives(&root), vec![root.join("RefData~4~680")]);
        assert!(templates_in(&root.join("RefData~4~680")).is_none());
        assert_eq!(archived_templates(&root), Ok(None));
        let _ = std::fs::remove_dir_all(&root);
    }
}
