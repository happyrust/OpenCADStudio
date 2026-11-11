//! How a drawing of a backup fares when it is read and imported.
//!
//! The panel reads a drawing twice over its life: for its items when its
//! row is unfolded, and for its geometry when it is opened in a tab. Either
//! can fail, and either can succeed with something left out: records that
//! were not decoded, source records that were not drawn, a border template
//! that was not found. The first was said in the drawing's details, the
//! second only on the command line after the drawing was open.
//!
//! [`import_report`] imports one drawing without opening it and keeps what
//! the import says, together with the parser's own report of the file.
//! [`check_all`] reads and imports every drawing of a backup, which is the
//! report the walk over a real backup prints, as data.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use pid_parse::inspect::commands::InspectCommand;

use super::text_file::{decode, Readable, TextFile};
use super::{load_items, BackupIndex};
use crate::io::pid::{ImportSummary, ImportUnit};

/// What reading a drawing for its items gave.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ItemsRead {
    /// Its model items, and how many types they are of.
    pub items: usize,
    pub types: usize,
    /// The attribute records of the file, and those that were not decoded.
    pub attribute_records: usize,
    pub undecoded_records: usize,
}

/// What importing a drawing gave: the part of [`ImportSummary`] a reader of
/// the panel can act on.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Imported {
    /// Entities drawn, the source records they came from, and the source
    /// records that were seen and not drawn.
    pub drawn: usize,
    pub decoded: usize,
    pub missing: usize,
    /// The sheet layers the drawing names, and those it starts with off.
    pub sheet_layers: usize,
    pub sheet_layers_off: usize,
    /// The border template that was found and drawn, what it drew, and
    /// what of it was not drawn.
    pub border_template: Option<String>,
    pub template_entities: usize,
    pub template_missing: usize,
    /// Symbol bodies drawn from the drawing's own cache and from a library.
    pub cache_bodies: usize,
    pub library_bodies: usize,
    /// The symbol libraries the import found.
    pub symbol_library: Vec<PathBuf>,
    /// The unit the drawing states; `None` when it states none and the
    /// metre was assumed.
    pub unit: Option<String>,
    /// A style table did not read: line work or lettering is on fallbacks.
    pub style_tables_failed: bool,
}

impl From<&ImportSummary> for Imported {
    fn from(summary: &ImportSummary) -> Self {
        Imported {
            drawn: summary.drawn,
            decoded: summary.decoded,
            missing: summary.missing,
            sheet_layers: summary.sheet_layer_names,
            sheet_layers_off: summary.sheet_layers_off,
            border_template: summary.border_template.clone(),
            template_entities: summary.template_entities,
            template_missing: summary.template_missing,
            cache_bodies: summary.cache_bodies,
            library_bodies: summary.library_bodies,
            symbol_library: summary.symbol_library.clone(),
            unit: match &summary.unit {
                ImportUnit::Stated { unit, .. } => Some(unit.clone()),
                ImportUnit::AssumedMetre => None,
            },
            style_tables_failed: summary.style_tables_failed,
        }
    }
}

/// Import the drawing at `path` as opening it would, and let the document
/// go: what the import says of itself is what is kept, and how long it took.
fn import(path: &Path) -> (Result<Imported, String>, Duration) {
    let started = Instant::now();
    let import = crate::io::pid::load_pid(path).map(|import| Imported::from(&import.summary));
    (import, started.elapsed())
}

/// The parser's own report of the file at `path`: its streams, what was
/// decoded of each and what was not, as the text `pid_inspect` prints. Read
/// as any text of a backup is, so what looks like a credential in it is
/// hidden the same way.
fn parse_report(path: &Path) -> Result<TextFile, String> {
    // The file by its name: its folder is the restore cache's.
    let fail = |error: &dyn std::fmt::Display| {
        crate::tf!(
            "cannot read {}: {}",
            super::file_label(path),
            super::name_the_file_only(&error.to_string(), path)
        )
    };
    let package = pid_parse::PidParser::new()
        .parse_package(path)
        .map_err(|error| fail(&error).into_owned())?;
    let report = InspectCommand::Report
        .run(Some(&package))
        .map_err(|error| fail(&error).into_owned())?
        .render_text();
    match decode(report.as_bytes(), report.len() as u64) {
        Readable::Text(text) => Ok(text),
        Readable::Binary(_) => Err(crate::t!("The parser's report is not text.").into_owned()),
    }
}

/// One drawing imported without being opened, and the parser's report of
/// its file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImportReport {
    /// What the import gave, or why it failed, and how long it took.
    pub import: Result<Imported, String>,
    pub import_time: Duration,
    /// The parser's report of the file, or why there is none.
    pub parse_report: Result<TextFile, String>,
}

/// Import the drawing at `path` and have the parser report on its file.
pub fn import_report(path: &Path) -> ImportReport {
    let (import, import_time) = import(path);
    ImportReport {
        import,
        import_time,
        parse_report: parse_report(path),
    }
}

/// Both reads of one drawing: what each gave or why it failed, and how long
/// it took.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Diagnosis {
    pub items: Result<ItemsRead, String>,
    pub items_time: Duration,
    pub import: Result<Imported, String>,
    pub import_time: Duration,
}

impl Diagnosis {
    /// Whether both reads gave something, with nothing left out that the
    /// reads themselves count: no undecoded record, no source record left
    /// undrawn, and at least one entity.
    pub fn is_clean(&self) -> bool {
        self.items
            .as_ref()
            .is_ok_and(|items| items.undecoded_records == 0)
            && self
                .import
                .as_ref()
                .is_ok_and(|import| import.missing == 0 && import.drawn > 0)
    }
}

/// Read the drawing at `path` for its items, and import it.
pub fn diagnose(path: &Path) -> Diagnosis {
    let started = Instant::now();
    let items = load_items(path).map(|items| ItemsRead {
        items: items.item_count,
        types: items.groups.len(),
        attribute_records: items.data.stats.attribute_records,
        undecoded_records: items.data.stats.undecoded_records,
    });
    let items_time = started.elapsed();
    let (import, import_time) = import(path);
    Diagnosis {
        items,
        items_time,
        import,
        import_time,
    }
}

/// One drawing of a check of the whole backup.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Checked {
    /// The drawing's name, and the plant group it is filed under.
    pub name: String,
    pub group: String,
    /// What its reads gave; `None` for a drawing the backup has no file of.
    pub diagnosis: Option<Diagnosis>,
}

/// How far a check of every drawing has come: written by the check on its
/// worker thread, read by the panel while it waits.
#[derive(Debug, Clone, Default)]
pub struct CheckProgress(Arc<(AtomicUsize, AtomicBool)>);

impl CheckProgress {
    /// How many drawings the check has done.
    pub fn done(&self) -> usize {
        self.0 .0.load(Ordering::Relaxed)
    }

    /// Whether a check ran to its end. From then on its result is on its
    /// way to the panel, or there: nothing more changes here.
    pub fn finished(&self) -> bool {
        self.0 .1.load(Ordering::Relaxed)
    }
}

/// Diagnose every drawing of `index`, in the index's order. Tens of seconds
/// for a plant: the caller is a worker thread, and how far the check has
/// come is in [`BackupIndex::check_progress`] as it goes.
pub fn check_all(index: &BackupIndex) -> Vec<Checked> {
    let (done, finished) = &*index.check_progress.0;
    done.store(0, Ordering::Relaxed);
    finished.store(false, Ordering::Relaxed);
    let checked = index
        .drawings
        .iter()
        .map(|drawing| {
            let checked = Checked {
                name: drawing.name.clone(),
                group: drawing.group_path.clone(),
                diagnosis: drawing.pid_path.as_deref().map(diagnose),
            };
            done.fetch_add(1, Ordering::Relaxed);
            checked
        })
        .collect();
    finished.store(true, Ordering::Relaxed);
    checked
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::io::pid_backup_browse::DrawingEntry;

    fn drawing(name: &str, path: Option<PathBuf>) -> DrawingEntry {
        DrawingEntry {
            name: name.to_string(),
            pid_path: path,
            group_path: "Plant / 00".to_string(),
            meta: Vec::new(),
        }
    }

    #[test]
    fn a_file_that_is_no_drawing_fails_every_read_and_says_why() {
        let folder = std::env::temp_dir().join(format!("ocs-diagnose-{}", std::process::id()));
        std::fs::create_dir_all(&folder).unwrap();
        let path = folder.join("broken.pid");
        std::fs::write(&path, b"not a compound file").unwrap();
        let diagnosis = diagnose(&path);
        let items = diagnosis.items.clone().unwrap_err();
        assert!(items.contains("broken.pid"), "{items}");
        assert!(diagnosis.import.is_err());
        assert!(!diagnosis.is_clean());
        // One drawing on its own: the import fails, and so does the parser's
        // report, each with its reason.
        let report = import_report(&path);
        assert!(report.import.is_err());
        let why = report.parse_report.unwrap_err();
        assert!(why.contains("broken.pid"), "{why}");

        // A check of a backup keeps the order of its drawings, says nothing
        // of one that has no file, and counts as it goes.
        let index = BackupIndex {
            drawings: vec![
                drawing("A", Some(path.clone())),
                drawing("B", None),
                drawing("C", Some(folder.join("absent.pid"))),
            ],
            ..BackupIndex::default()
        };
        assert!(!index.check_progress.finished());
        let checked = check_all(&index);
        assert_eq!(index.check_progress.done(), 3);
        assert!(index.check_progress.finished());
        // A copy of the index reads the same progress.
        assert!(index.clone().check_progress.finished());
        assert_eq!(
            checked
                .iter()
                .map(|drawing| (drawing.name.as_str(), drawing.diagnosis.is_some()))
                .collect::<Vec<_>>(),
            [("A", true), ("B", false), ("C", true)]
        );
        assert_eq!(checked[0].group, "Plant / 00");
        assert!(checked[2]
            .diagnosis
            .as_ref()
            .is_some_and(|diagnosis| diagnosis.items.is_err() && diagnosis.import.is_err()));
        let _ = std::fs::remove_dir_all(&folder);
    }

    /// Why a drawing did not read named the whole path of the file -- which
    /// for a backup is a folder of the restore cache, a key and all -- and
    /// was the reader's English sentence. The reason now names the file,
    /// and what the reader most often says is said in the user's language.
    #[test]
    fn a_failure_names_the_file_and_not_the_folder_it_lies_in() {
        let folder = std::env::temp_dir().join(format!("ocs-diagnose-names-{}", std::process::id()));
        std::fs::create_dir_all(&folder).unwrap();
        let path = folder.join("broken.pid");
        std::fs::write(&path, b"not a compound file").unwrap();
        let folder_text = folder.display().to_string();
        let diagnosis = diagnose(&path);
        let report = import_report(&path);
        for (what, why) in [
            ("the items", diagnosis.items.clone().unwrap_err()),
            ("the import", diagnosis.import.clone().unwrap_err()),
            ("the parser's report", report.parse_report.clone().unwrap_err()),
        ] {
            assert!(why.contains("broken.pid"), "{what}: {why}");
            assert!(!why.contains(&folder_text), "{what}: {why}");
        }
        let _ = std::fs::remove_dir_all(&folder);

        // A drawing that states no drawing number: the reader's one reason
        // of its own, in the application's words.
        use crate::io::pid_backup_browse::read_failure;
        use pid_parse::publish::pid_load::PidLoadError;
        assert_eq!(
            read_failure(
                Path::new("D:/cache/0123456789abcdef/00/D03.pid"),
                &PidLoadError::NoDrawing
            ),
            crate::tf!(
                "cannot read {}: {}",
                "D03.pid",
                crate::t!("it states no drawing number (no P&IDAttributes row carries a DrawingNo)")
            )
        );
    }

    #[test]
    fn a_drawing_is_clean_when_nothing_was_left_out() {
        let clean = Diagnosis {
            items: Ok(ItemsRead {
                items: 19,
                types: 4,
                attribute_records: 182,
                undecoded_records: 0,
            }),
            items_time: Duration::from_millis(12),
            import: Ok(Imported {
                drawn: 240,
                decoded: 230,
                ..Imported::default()
            }),
            import_time: Duration::from_millis(80),
        };
        assert!(clean.is_clean());
        let mut undecoded = clean.clone();
        undecoded.items = Ok(ItemsRead {
            undecoded_records: 1,
            ..ItemsRead::default()
        });
        assert!(!undecoded.is_clean());
        let mut undrawn = clean.clone();
        undrawn.import = Ok(Imported {
            drawn: 240,
            missing: 3,
            ..Imported::default()
        });
        assert!(!undrawn.is_clean());
        // An import that drew nothing is no import to speak of.
        let mut empty = clean.clone();
        empty.import = Ok(Imported::default());
        assert!(!empty.is_clean());
        let mut failed = clean;
        failed.import = Err("cannot read".to_string());
        assert!(!failed.is_clean());
    }

    /// A drawing of the parser's own fixtures, beside this checkout as the
    /// importer's tests expect them: it imports, and the parser reports on
    /// its file as text.
    #[test]
    fn a_real_drawing_is_imported_and_reported_on() {
        let path = Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .map(|dir| dir.join("pid-parse").join("test-file").join("D06.pid"))
            .filter(|path| path.is_file());
        let Some(path) = path else {
            return; // fixture absent: soft-skip like the corpus tests do
        };
        let report = import_report(&path);
        let import = report.import.expect("the fixture imports");
        assert!(import.drawn > 0);
        let text = report
            .parse_report
            .expect("the parser reports on the fixture");
        assert!(text.line_count > 10, "{} line(s)", text.line_count);
        // Every line has a row, and a long one more than one.
        assert!(text.rows.len() >= text.line_count);
        assert_eq!(text.lines().len(), text.line_count);
    }
}
