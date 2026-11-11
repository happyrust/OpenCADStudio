//! PIDBACKUPOPEN's engine: a SmartPlant P&ID backup -- a zip of one, or the
//! backup folder itself -- made into a folder whose `.pid` drawings open like
//! any other.
//!
//! A backup keeps its drawings in `PlantData~<schema>~<id>.zip` archives
//! beside the `Ref` reference data (symbols under `Ref/Symbols`). A folder
//! that already holds loose `.pid` files is used where it is, read-only. One
//! whose drawings are still archived, and any zip, is unpacked into a restore
//! folder: by default a cache under the temp folder keyed by the source, so a
//! second open of the same backup reuses it. The cache may be another folder,
//! and can be measured and cleared ([`cache`]). The drawings land beside a `Ref`
//! there -- the backup's own, copied in for a folder source -- so the
//! importer's walk up from a drawing finds the symbol library.
//!
//! Zip member names are written by Windows tools in the system code page
//! (GBK on the plants this serves), not UTF-8; they are decoded as pid-parse
//! decodes them, and a name that would leave the restore folder refuses the
//! whole restore.

pub mod cache;
mod refdata;

pub use cache::{default_cache_root, restore_cache_in};
pub use refdata::REFERENCE_DIR;

use std::io::Read;
use std::path::{Component, Path, PathBuf};

/// What a restore folder holds, as a number: a restore made under an older
/// layout is made again. 2: the reference data volumes are unpacked into
/// `Ref`.
const RESTORE_LAYOUT: u32 = 2;

/// Written last into a finished restore folder; holds the source's cache key.
pub(crate) const DONE_MARKER: &str = ".ocs-restored";

/// How deep under the restore folder `.pid` drawings are looked for. A backup
/// files them two plant-group levels down (`00/00/<name>.pid`).
const DRAWING_SEARCH_DEPTH: usize = 8;

/// File-name prefix of the archives a backup keeps its drawings in.
const PLANT_DATA_PREFIX: &str = "PlantData~";

/// What a restore produced.
#[derive(Debug, Clone)]
pub struct Restored {
    /// The folder the drawings were found under.
    pub root: PathBuf,
    /// Whether the backup was unpacked (`false`: a folder used in place).
    pub extracted: bool,
    /// Whether an earlier, finished restore of the same source was reused.
    pub reused: bool,
    /// The `.pid` drawings under [`Restored::root`], path-sorted.
    pub drawings: Vec<PathBuf>,
}

impl Restored {
    /// Where a drawing picker should start: the folder all drawings share,
    /// else the restore root.
    pub fn drawing_folder(&self) -> PathBuf {
        let mut parents = self.drawings.iter().filter_map(|drawing| drawing.parent());
        let Some(first) = parents.next() else {
            return self.root.clone();
        };
        let mut common = first.to_path_buf();
        for parent in parents {
            while !parent.starts_with(&common) {
                if !common.pop() {
                    return self.root.clone();
                }
            }
        }
        if common.starts_with(&self.root) {
            common
        } else {
            self.root.clone()
        }
    }
}

/// Whether `path` is a file starting with the zip local-header magic.
pub fn is_zip(path: &Path) -> bool {
    let Ok(mut file) = std::fs::File::open(path) else {
        return false;
    };
    let mut magic = [0u8; 4];
    file.read_exact(&mut magic).is_ok() && &magic == b"PK\x03\x04"
}

/// The default restore folder of `source`: `<temp>/ocs-pid-backups/<hash>`
/// ([`restore_cache_in`] the [`default_cache_root`]).
pub fn restore_cache(source: &Path) -> PathBuf {
    restore_cache_in(&default_cache_root(), source)
}

fn cache_key(source: &Path) -> String {
    use std::hash::{Hash, Hasher};
    let mut hasher = std::hash::DefaultHasher::new();
    RESTORE_LAYOUT.hash(&mut hasher);
    let mut stamp = |path: &Path| {
        path.hash(&mut hasher);
        if let Ok(meta) = std::fs::metadata(path) {
            meta.len().hash(&mut hasher);
            if let Ok(modified) = meta.modified() {
                modified.hash(&mut hasher);
            }
        }
    };
    stamp(source);
    if source.is_dir() {
        for archive in plant_data_archives(source) {
            stamp(&archive);
        }
        // The reference data is restored too: a volume or a manifest that
        // changed makes the restore stale.
        for file in refdata::stamped_files(source) {
            stamp(&file);
        }
    }
    format!("{:016x}", hasher.finish())
}

/// Make `source` -- a backup zip or folder -- browsable, unpacking it into
/// `target` (default [`restore_cache`]) when it has to be.
pub fn restore(source: &Path, target: Option<&Path>) -> Result<Restored, String> {
    restore_in(source, target, None)
}

/// [`restore`] with the cache in `cache_root` instead of the default folder:
/// without a `target` the backup is unpacked into its own folder there.
pub fn restore_in(
    source: &Path,
    target: Option<&Path>,
    cache_root: Option<&Path>,
) -> Result<Restored, String> {
    if source.is_dir() {
        let archives = plant_data_archives(source);
        if archives.is_empty() {
            return Ok(Restored {
                root: source.to_path_buf(),
                extracted: false,
                reused: false,
                drawings: pid_drawings(source),
            });
        }
        let reference = source.join(REFERENCE_DIR);
        return build(source, target, cache_root, |staging| {
            for archive in &archives {
                extract_zip(archive, staging)?;
            }
            // A `Ref` folder somebody unpacked beside the backup first, then
            // the backup's own volumes over it: what the backup holds is the
            // backup, what lies beside it only fills in.
            if reference.is_dir() {
                copy_dir(&reference, &staging.join(REFERENCE_DIR))?;
            }
            refdata::restore_reference_data(source, staging)?;
            Ok(())
        });
    }
    if !source.exists() {
        return Err(format!("{} does not exist", source.display()));
    }
    if !is_zip(source) {
        return Err(format!(
            "{} is not a zip file or a folder",
            source.display()
        ));
    }
    build(source, target, cache_root, |staging| {
        extract_zip(source, staging)?;
        // A zip of a whole backup still files its drawings in PlantData
        // archives; unpack those where they lie so each lands beside its Ref.
        for root in backup_roots(staging) {
            for archive in plant_data_archives(&root) {
                extract_zip(&archive, &root)?;
            }
            refdata::restore_reference_data(&root, &root)?;
        }
        Ok(())
    })
}

/// [`restore`] on a worker thread: unpacking a plant's drawings and symbol
/// library is tens of megabytes the UI thread must not wait out.
pub async fn restore_in_background(
    source: PathBuf,
    target: Option<PathBuf>,
) -> Result<Restored, String> {
    let (sender, receiver) = iced::futures::channel::oneshot::channel();
    std::thread::Builder::new()
        .name("ocs-pid-backup".into())
        .spawn(move || {
            let _ = sender.send(restore(&source, target.as_deref()));
        })
        .map_err(|error| format!("cannot start the restore: {error}"))?;
    receiver
        .await
        .unwrap_or_else(|_| Err("the restore stopped before it finished".into()))
}

/// The `.pid` drawings under `root`, path-sorted.
pub fn pid_drawings(root: &Path) -> Vec<PathBuf> {
    fn walk(dir: &Path, depth: usize, found: &mut Vec<PathBuf>) {
        let Ok(entries) = std::fs::read_dir(dir) else {
            return;
        };
        for entry in entries.flatten() {
            let Ok(kind) = entry.file_type() else {
                continue;
            };
            let path = entry.path();
            if kind.is_dir() {
                if depth > 0 {
                    walk(&path, depth - 1, found);
                }
            } else if kind.is_file() && crate::io::pid_pdf::is_pid(&path) {
                found.push(path);
            }
        }
    }
    let mut found = Vec::new();
    walk(root, DRAWING_SEARCH_DEPTH, &mut found);
    found.sort();
    found
}

/// Fill a restore folder through a staging sibling, so a failed or cut-short
/// restore never leaves a folder that looks finished.
fn build(
    source: &Path,
    target: Option<&Path>,
    cache_root: Option<&Path>,
    fill: impl FnOnce(&Path) -> Result<(), String>,
) -> Result<Restored, String> {
    let key = cache_key(source);
    let owned = target.is_none();
    let target = target.map_or_else(
        || match cache_root {
            Some(root) => restore_cache_in(root, source),
            None => restore_cache(source),
        },
        Path::to_path_buf,
    );
    let marker = target.join(DONE_MARKER);
    if std::fs::read_to_string(&marker).is_ok_and(|stamp| stamp.trim() == key) {
        return Ok(Restored {
            drawings: restored_drawings(&target),
            root: target,
            extracted: true,
            reused: true,
        });
    }
    if target.exists() {
        let empty = std::fs::read_dir(&target)
            .map_err(|error| format!("cannot read {}: {error}", target.display()))?
            .next()
            .is_none();
        if owned || empty {
            // A cache folder is this module's own: a leftover is an earlier
            // restore of another state of the source.
            std::fs::remove_dir_all(&target)
                .map_err(|error| format!("cannot clear {}: {error}", target.display()))?;
        } else {
            return Err(format!(
                "{} is not empty; restore into an empty or new folder",
                target.display()
            ));
        }
    }
    let name = target.file_name().map_or_else(
        || "restore".into(),
        |name| name.to_string_lossy().into_owned(),
    );
    let staging = target.with_file_name(format!("{name}.partial-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&staging);
    std::fs::create_dir_all(&staging)
        .map_err(|error| format!("cannot create {}: {error}", staging.display()))?;
    let filled = fill(&staging).and_then(|()| {
        std::fs::write(staging.join(DONE_MARKER), &key)
            .map_err(|error| format!("cannot finish {}: {error}", staging.display()))
    });
    if let Err(error) = filled {
        let _ = std::fs::remove_dir_all(&staging);
        return Err(error);
    }
    if let Err(error) = std::fs::rename(&staging, &target) {
        let _ = std::fs::remove_dir_all(&staging);
        return Err(format!(
            "cannot move the restore to {}: {error}",
            target.display()
        ));
    }
    Ok(Restored {
        drawings: restored_drawings(&target),
        root: target,
        extracted: true,
        reused: false,
    })
}

/// The drawings of a restore folder: every `.pid` under it but the ones in
/// its reference data. A border template is a `.pid` file as well, and it is
/// the plant's reference data, not one of its drawings.
fn restored_drawings(target: &Path) -> Vec<PathBuf> {
    let mut reference = vec![target.join(REFERENCE_DIR)];
    reference.extend(
        backup_roots(target)
            .into_iter()
            .map(|root| root.join(REFERENCE_DIR)),
    );
    let mut drawings = pid_drawings(target);
    drawings.retain(|drawing| !reference.iter().any(|folder| drawing.starts_with(folder)));
    drawings
}

/// The `PlantData~*` zip archives directly in `dir`, path-sorted.
fn plant_data_archives(dir: &Path) -> Vec<PathBuf> {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut archives: Vec<PathBuf> = entries
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| {
            path.is_file()
                && path
                    .file_name()
                    .is_some_and(|name| name.to_string_lossy().starts_with(PLANT_DATA_PREFIX))
                && is_zip(path)
        })
        .collect();
    archives.sort();
    archives
}

/// Folders of an unpacked zip that hold `PlantData~*` archives: its top level,
/// or the one folder a zip of a backup folder wraps it in.
fn backup_roots(root: &Path) -> Vec<PathBuf> {
    let mut roots = vec![root.to_path_buf()];
    if let Ok(entries) = std::fs::read_dir(root) {
        let mut children: Vec<PathBuf> = entries
            .flatten()
            .filter(|entry| entry.file_type().is_ok_and(|kind| kind.is_dir()))
            .map(|entry| entry.path())
            .collect();
        children.sort();
        roots.extend(children);
    }
    roots.retain(|root| !plant_data_archives(root).is_empty());
    roots
}

/// Where the member named `name` is written under `root`; `None` for a name
/// that is absolute, carries a drive or reaches above `root`.
fn member_path(root: &Path, name: &str) -> Option<PathBuf> {
    if name.contains('\0') || name.starts_with(['/', '\\']) {
        return None;
    }
    let mut path = root.to_path_buf();
    for part in name.split(['/', '\\']) {
        if part.is_empty() || part == "." {
            continue;
        }
        let mut components = Path::new(part).components();
        match (components.next(), components.next()) {
            (Some(Component::Normal(normal)), None) => path.push(normal),
            _ => return None,
        }
    }
    (path != root).then_some(path)
}

/// Unpack every member of `archive` under `root`. Any member whose name would
/// land outside `root` fails the whole archive before anything is written.
fn extract_zip(archive: &Path, root: &Path) -> Result<(), String> {
    let open = |path: &Path| {
        std::fs::File::open(path)
            .map_err(|error| format!("cannot open {}: {error}", path.display()))
    };
    let mut zip = zip::ZipArchive::new(open(archive)?)
        .map_err(|error| format!("{} is not a readable zip: {error}", archive.display()))?;
    let mut plan = Vec::with_capacity(zip.len());
    for index in 0..zip.len() {
        let entry = zip
            .by_index_raw(index)
            .map_err(|error| format!("{}: {error}", archive.display()))?;
        let name = pid_parse::publish::backup_package::decode_zip_entry_name(entry.name_raw());
        // The zip crate's own containment check, on top of the decoded name:
        // its CP437 reading of a GBK name differs, but the separators and
        // `..` it sees are the same bytes.
        let path = entry
            .enclosed_name()
            .and_then(|_| member_path(root, &name))
            .ok_or_else(|| {
                format!(
                    "{}: refusing the member \"{name}\", whose path leaves the restore folder",
                    archive.display()
                )
            })?;
        plan.push((path, entry.is_dir(), entry.is_symlink()));
    }
    for (index, (path, is_dir, is_symlink)) in plan.into_iter().enumerate() {
        if is_symlink {
            continue;
        }
        if is_dir {
            std::fs::create_dir_all(&path)
                .map_err(|error| format!("cannot create {}: {error}", path.display()))?;
            continue;
        }
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|error| format!("cannot create {}: {error}", parent.display()))?;
        }
        let mut entry = zip
            .by_index(index)
            .map_err(|error| format!("{}: {error}", archive.display()))?;
        let mut out = std::fs::File::create(&path)
            .map_err(|error| format!("cannot write {}: {error}", path.display()))?;
        std::io::copy(&mut entry, &mut out)
            .map_err(|error| format!("cannot unpack {}: {error}", path.display()))?;
    }
    Ok(())
}

fn copy_dir(from: &Path, to: &Path) -> Result<(), String> {
    std::fs::create_dir_all(to)
        .map_err(|error| format!("cannot create {}: {error}", to.display()))?;
    let entries = std::fs::read_dir(from)
        .map_err(|error| format!("cannot read {}: {error}", from.display()))?;
    for entry in entries.flatten() {
        let Ok(kind) = entry.file_type() else {
            continue;
        };
        let target = to.join(entry.file_name());
        if kind.is_dir() {
            copy_dir(&entry.path(), &target)?;
        } else if kind.is_file() {
            std::fs::copy(entry.path(), &target)
                .map_err(|error| format!("cannot copy to {}: {error}", target.display()))?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn scratch(name: &str) -> PathBuf {
        let root =
            std::env::temp_dir().join(format!("ocs-pid-backup-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        root
    }

    fn write_zip(path: &Path, members: &[(&str, &[u8])]) {
        let mut writer = zip::ZipWriter::new(std::fs::File::create(path).unwrap());
        let options = zip::write::SimpleFileOptions::default()
            .compression_method(zip::CompressionMethod::Deflated);
        for (name, bytes) in members {
            writer.start_file(*name, options).unwrap();
            writer.write_all(bytes).unwrap();
        }
        writer.finish().unwrap();
    }

    #[test]
    fn a_zip_is_unpacked_and_its_drawings_listed() {
        let root = scratch("unpack");
        let zip = root.join("backup.zip");
        write_zip(
            &zip,
            &[
                ("00/00/A.pid", b"a"),
                ("00/00/B.PID", b"b"),
                ("Ref/Symbols/v.sym", b"s"),
                ("notes.txt", b"n"),
            ],
        );
        let target = root.join("out");
        let restored = restore(&zip, Some(&target)).unwrap();
        assert!(restored.extracted && !restored.reused);
        assert_eq!(restored.root, target);
        assert_eq!(
            restored.drawings,
            vec![target.join("00/00/A.pid"), target.join("00/00/B.PID")]
        );
        assert_eq!(
            std::fs::read(target.join("Ref/Symbols/v.sym")).unwrap(),
            b"s"
        );
        assert_eq!(restored.drawing_folder(), target.join("00/00"));
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn a_member_leaving_the_restore_folder_refuses_the_restore() {
        let root = scratch("slip");
        let zip = root.join("evil.zip");
        write_zip(&zip, &[("ok.pid", b"a"), ("../escape.pid", b"x")]);
        let target = root.join("nested").join("out");
        let error = restore(&zip, Some(&target)).unwrap_err();
        assert!(error.contains("leaves the restore folder"), "{error}");
        assert!(!root.join("nested").join("escape.pid").exists());
        assert!(
            !target.exists(),
            "a refused restore leaves no folder behind"
        );
        for name in ["/abs.pid", "C:/x.pid", "a/../../b.pid", "a\\..\\..\\b.pid"] {
            assert_eq!(member_path(&target, name), None, "{name}");
        }
        assert_eq!(
            member_path(&target, "a\\b/./c.pid"),
            Some(target.join("a").join("b").join("c.pid"))
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn a_second_restore_of_the_same_zip_reuses_the_first() {
        let root = scratch("reuse");
        let zip = root.join("backup.zip");
        write_zip(&zip, &[("A.pid", b"a")]);
        let target = root.join("out");
        assert!(!restore(&zip, Some(&target)).unwrap().reused);
        // A file added after the first restore shows the folder is untouched.
        std::fs::write(target.join("extra.pid"), b"e").unwrap();
        let again = restore(&zip, Some(&target)).unwrap();
        assert!(again.reused);
        assert_eq!(again.drawings.len(), 2);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn a_folder_with_loose_drawings_is_used_in_place() {
        let root = scratch("folder");
        std::fs::create_dir_all(root.join("00")).unwrap();
        std::fs::write(root.join("00").join("A.pid"), b"a").unwrap();
        std::fs::write(root.join("readme.txt"), b"r").unwrap();
        let restored = restore(&root, None).unwrap();
        assert!(!restored.extracted);
        assert_eq!(restored.root, root);
        assert_eq!(restored.drawings, vec![root.join("00").join("A.pid")]);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn a_backup_folder_unpacks_its_plant_data_beside_a_copy_of_its_ref() {
        let root = scratch("plantdata");
        let backup = root.join("backup");
        std::fs::create_dir_all(backup.join("Ref").join("Symbols")).unwrap();
        std::fs::write(backup.join("Ref").join("Symbols").join("v.sym"), b"s").unwrap();
        write_zip(
            &backup.join("PlantData~2~711.zip"),
            &[("00/00/A.pid", b"a")],
        );
        // Not a zip: a backup's other PlantData payloads stay where they are.
        std::fs::write(backup.join("PlantData~2~722"), b"<?xml?>").unwrap();
        let target = root.join("out");
        let restored = restore(&backup, Some(&target)).unwrap();
        assert!(restored.extracted);
        assert_eq!(restored.drawings, vec![target.join("00/00/A.pid")]);
        assert_eq!(
            std::fs::read(target.join("Ref/Symbols/v.sym")).unwrap(),
            b"s"
        );
        assert!(!target.join("PlantData~2~722").exists());
        // The source folder is only read.
        assert!(backup.join("PlantData~2~711.zip").is_file());
        assert!(!backup.join("00").exists());
        let _ = std::fs::remove_dir_all(&root);
    }

    /// A manifest for the reference data volumes of the tests below.
    fn manifest(lines: &[&str]) -> String {
        let mut text = String::from("Name<<|>>SQPlant\r\n");
        for line in lines {
            text.push_str(line);
            text.push_str("\r\n");
        }
        text
    }

    /// The backup as SmartPlant leaves it: drawings in one archive, the
    /// reference data in numbered volumes, and nothing unpacked beside them.
    fn backup_with_volumes(backup: &Path) {
        std::fs::create_dir_all(backup).unwrap();
        std::fs::write(
            backup.join("Manifest.txt"),
            manifest(&[
                "File<<|>>2<<|>>1<<|>>711<<|>>1<<|>>\\\\host\\P\\SQPlant\\Plant<<|>>1<<|>>PlantData~2~711.zip",
                "File<<|>>4<<|>>1<<|>>681<<|>>1<<|>>\\\\host\\P\\SQPlant\\Ref\\Symbols<<|>>1<<|>>RefData~4~681.zip",
                "File<<|>>4<<|>>1<<|>>682<<|>>1<<|>>\\\\host\\P\\SQPlant\\Ref\\Template Files<<|>>1<<|>>RefData~4~682.zip",
                "File<<|>>4<<|>>2<<|>>680<<|>>1<<|>>\\\\host\\P\\SQPlant\\Ref\\rules.rul<<|>>1<<|>>RefData~4~680",
                "File<<|>>4<<|>>1<<|>>685<<|>>1<<|>>\\\\host\\P\\SQPlant\\Ref\\Symbols\\Assemblies<<|>>1<<|>>RefData~4~685.zip",
                "File<<|>>4<<|>>2<<|>>824<<|>>1<<|>>\\\\host\\P\\SQPlant\\Ref\\Jacket.XML<<|>>2<<|>>Failed to access file.",
            ]),
        )
        .unwrap();
        write_zip(&backup.join("PlantData~2~711.zip"), &[("00/00/A.pid", b"a")]);
        write_zip(
            &backup.join("RefData~4~681.zip"),
            &[
                ("Piping/Valves/gate.sym", b"gate from the volume"),
                ("CatalogIndex.xml", b"<catalog/>"),
            ],
        );
        write_zip(
            &backup.join("RefData~4~682.zip"),
            &[("A1.igr", b"border"), ("A1.pid", b"template drawing")],
        );
        std::fs::write(backup.join("RefData~4~680"), b"\"Begin Rules\",1").unwrap();
        write_zip(&backup.join("RefData~4~685.zip"), &[("Piping/pump.sym", b"assembly")]);
    }

    #[test]
    fn a_backup_folder_gets_its_reference_data_volumes_back_under_ref() {
        let root = scratch("refdata");
        let backup = root.join("backup");
        backup_with_volumes(&backup);
        let target = root.join("out");
        let restored = restore(&backup, Some(&target)).unwrap();
        let read = |relative: &str| std::fs::read(target.join(relative)).ok();
        // A folder volume lands where the manifest says the folder was, a
        // single file under its own name, a folder inside another inside it.
        assert_eq!(
            read("Ref/Symbols/Piping/Valves/gate.sym").as_deref(),
            Some(&b"gate from the volume"[..])
        );
        assert_eq!(read("Ref/Symbols/CatalogIndex.xml").as_deref(), Some(&b"<catalog/>"[..]));
        assert_eq!(read("Ref/Template Files/A1.igr").as_deref(), Some(&b"border"[..]));
        assert_eq!(read("Ref/rules.rul").as_deref(), Some(&b"\"Begin Rules\",1"[..]));
        assert_eq!(
            read("Ref/Symbols/Assemblies/Piping/pump.sym").as_deref(),
            Some(&b"assembly"[..])
        );
        // The file the backup never got is not made up.
        assert!(!target.join("Ref/Jacket.XML").exists());
        // The volumes themselves are not copied, and the source is only read.
        assert!(!target.join("RefData~4~681.zip").exists());
        assert!(!backup.join("Ref").exists());
        // A border template is a `.pid` too: it is reference data, not one
        // of the plant's drawings.
        assert!(target.join("Ref/Template Files/A1.pid").is_file());
        assert_eq!(restored.drawings, vec![target.join("00/00/A.pid")]);
        // And a restore that is reused lists the same.
        let again = restore(&backup, Some(&target)).unwrap();
        assert!(again.reused);
        assert_eq!(again.drawings, restored.drawings);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn a_ref_folder_beside_the_backup_fills_in_what_the_volumes_do_not_hold() {
        let root = scratch("refdata-beside");
        let backup = root.join("backup");
        backup_with_volumes(&backup);
        let symbols = backup.join("Ref").join("Symbols");
        std::fs::create_dir_all(symbols.join("Piping").join("Valves")).unwrap();
        // One the volume holds too, and one only the folder has.
        std::fs::write(symbols.join("Piping/Valves/gate.sym"), b"gate from the folder").unwrap();
        std::fs::write(symbols.join("Piping/Valves/own.sym"), b"only beside").unwrap();
        let target = root.join("out");
        restore(&backup, Some(&target)).unwrap();
        assert_eq!(
            std::fs::read(target.join("Ref/Symbols/Piping/Valves/gate.sym")).unwrap(),
            b"gate from the volume"
        );
        assert_eq!(
            std::fs::read(target.join("Ref/Symbols/Piping/Valves/own.sym")).unwrap(),
            b"only beside"
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn a_zipped_backup_gets_its_reference_data_back_beside_its_drawings() {
        let root = scratch("refdata-zip");
        let backup = root.join("stage").join("MyPlant");
        backup_with_volumes(&backup);
        // The whole folder in one zip, as a backup is passed around.
        let outer = root.join("backup.zip");
        let mut members: Vec<(String, Vec<u8>)> = Vec::new();
        for entry in std::fs::read_dir(&backup).unwrap().flatten() {
            members.push((
                format!("MyPlant/{}", entry.file_name().to_string_lossy()),
                std::fs::read(entry.path()).unwrap(),
            ));
        }
        let borrowed: Vec<(&str, &[u8])> = members
            .iter()
            .map(|(name, bytes)| (name.as_str(), bytes.as_slice()))
            .collect();
        write_zip(&outer, &borrowed);
        let target = root.join("out");
        let restored = restore(&outer, Some(&target)).unwrap();
        let plant = target.join("MyPlant");
        assert_eq!(
            std::fs::read(plant.join("Ref/Symbols/Piping/Valves/gate.sym")).unwrap(),
            b"gate from the volume"
        );
        assert_eq!(std::fs::read(plant.join("Ref/rules.rul")).unwrap(), b"\"Begin Rules\",1");
        // The template drawing under `Ref` is not one of the drawings.
        assert_eq!(restored.drawings, vec![plant.join("00/00/A.pid")]);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn a_volume_whose_path_leaves_the_restore_folder_is_not_unpacked() {
        let root = scratch("refdata-outside");
        let backup = root.join("backup");
        std::fs::create_dir_all(&backup).unwrap();
        // Three volumes share `\\host\P\Ref`; two of the paths lead above it.
        std::fs::write(
            backup.join("Manifest.txt"),
            manifest(&[
                "File<<|>>4<<|>>1<<|>>681<<|>>1<<|>>\\\\host\\P\\Ref\\Symbols<<|>>1<<|>>RefData~4~681.zip",
                "File<<|>>4<<|>>2<<|>>690<<|>>1<<|>>\\\\host\\P\\Ref\\..\\..\\..\\outside.txt<<|>>1<<|>>RefData~4~690",
                "File<<|>>4<<|>>1<<|>>691<<|>>1<<|>>\\\\host\\P\\Ref\\..\\..\\outside<<|>>1<<|>>RefData~4~691.zip",
            ]),
        )
        .unwrap();
        write_zip(&backup.join("PlantData~2~711.zip"), &[("A.pid", b"a")]);
        write_zip(&backup.join("RefData~4~681.zip"), &[("gate.sym", b"gate")]);
        std::fs::write(backup.join("RefData~4~690"), b"must stay inside").unwrap();
        write_zip(&backup.join("RefData~4~691.zip"), &[("inside.txt", b"must stay inside")]);
        let target = root.join("nested").join("out");
        restore(&backup, Some(&target)).unwrap();
        // Whatever was restored is under the target; nothing beside or above.
        let mut found = Vec::new();
        let mut pending = vec![root.clone()];
        while let Some(dir) = pending.pop() {
            for entry in std::fs::read_dir(&dir).unwrap().flatten() {
                let path = entry.path();
                if path.is_dir() {
                    pending.push(path);
                } else if std::fs::read(&path).is_ok_and(|bytes| bytes == b"must stay inside") {
                    found.push(path);
                }
            }
        }
        for path in &found {
            assert!(
                path.starts_with(&target) || path.starts_with(&backup),
                "{} is outside the restore",
                path.display()
            );
        }
        assert!(!root.join("outside.txt").exists());
        assert!(!root.join("nested").join("outside.txt").exists());
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn a_changed_volume_makes_the_restore_stale() {
        let root = scratch("refdata-stale");
        let backup = root.join("backup");
        backup_with_volumes(&backup);
        let key = cache_key(&backup);
        assert_eq!(key, cache_key(&backup));
        // A volume grows: the cache of the old one is not the backup's any
        // more.
        std::fs::write(backup.join("RefData~4~680"), b"\"Begin Rules\",2\r\nmore").unwrap();
        assert_ne!(key, cache_key(&backup));
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn a_restore_goes_into_the_cache_folder_it_is_given() {
        let root = scratch("cache-root");
        let zip = root.join("backup.zip");
        write_zip(&zip, &[("00/00/A.pid", b"a")]);
        let cache = root.join("my cache");
        let restored = restore_in(&zip, None, Some(&cache)).unwrap();
        assert_eq!(restored.root, restore_cache_in(&cache, &zip));
        assert_eq!(restored.root.parent(), Some(cache.as_path()));
        assert_eq!(restored.drawings, vec![restored.root.join("00/00/A.pid")]);
        // The folder is the cache's own: a second open reuses it, and what
        // the cache holds says so.
        assert!(restore_in(&zip, None, Some(&cache)).unwrap().reused);
        let usage = cache::usage(&cache, Some(&restored.root));
        assert_eq!(usage.others, 0);
        assert_eq!(usage.current.map(|restore| restore.files), Some(2));
        // A target named outright wins over the cache folder.
        let named = root.join("named");
        let restored = restore_in(&zip, Some(&named), Some(&cache)).unwrap();
        assert_eq!(restored.root, named);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn a_zip_of_a_backup_folder_unpacks_the_plant_data_inside_it() {
        let root = scratch("zipped-backup");
        let inner = root.join("PlantData~2~711.zip");
        write_zip(&inner, &[("00/00/A.pid", b"a")]);
        let inner_bytes = std::fs::read(&inner).unwrap();
        let outer = root.join("backup.zip");
        write_zip(
            &outer,
            &[
                ("Backup/PlantData~2~711.zip", &inner_bytes),
                ("Backup/Ref/Symbols/v.sym", b"s"),
            ],
        );
        let target = root.join("out");
        let restored = restore(&outer, Some(&target)).unwrap();
        assert_eq!(restored.drawings, vec![target.join("Backup/00/00/A.pid")]);
        assert!(target.join("Backup/Ref/Symbols/v.sym").is_file());
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn a_gbk_member_name_is_decoded() {
        let root = scratch("gbk");
        let zip = root.join("backup.zip");
        write_zip(&zip, &[("XXXXXX.pid", b"a")]);
        // A backup's member names are GBK bytes with no UTF-8 flag: swap the
        // ASCII placeholder (same length) for "流程图" in GBK. Names are not
        // covered by the CRC.
        let gbk: &[u8] = b"\xc1\xf7\xb3\xcc\xcd\xbc";
        let mut bytes = std::fs::read(&zip).unwrap();
        let mut at = 0;
        while let Some(found) = bytes[at..].windows(6).position(|w| w == b"XXXXXX") {
            bytes[at + found..at + found + 6].copy_from_slice(gbk);
            at += found + 6;
        }
        std::fs::write(&zip, bytes).unwrap();
        let restored = restore(&zip, Some(&root.join("out"))).unwrap();
        let names: Vec<String> = restored
            .drawings
            .iter()
            .map(|path| path.file_name().unwrap().to_string_lossy().into_owned())
            .collect();
        assert_eq!(names, vec!["流程图.pid".to_string()]);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn a_file_that_is_not_a_zip_is_refused() {
        let root = scratch("notzip");
        let file = root.join("backup.zip");
        std::fs::write(&file, b"not a zip").unwrap();
        let error = restore(&file, None).unwrap_err();
        assert!(error.contains("not a zip file or a folder"), "{error}");
        assert!(restore(&root.join("missing.zip"), None).is_err());
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn a_non_empty_folder_of_the_users_is_not_overwritten() {
        let root = scratch("occupied");
        let zip = root.join("backup.zip");
        write_zip(&zip, &[("A.pid", b"a")]);
        let target = root.join("out");
        std::fs::create_dir_all(&target).unwrap();
        std::fs::write(target.join("mine.txt"), b"keep").unwrap();
        let error = restore(&zip, Some(&target)).unwrap_err();
        assert!(error.contains("not empty"), "{error}");
        assert_eq!(std::fs::read(target.join("mine.txt")).unwrap(), b"keep");
        let _ = std::fs::remove_dir_all(&root);
    }
    /// The plant test backup, restored from its folder: drawings out of
    /// `PlantData~2~711.zip` beside a copy of its `Ref`. Soft-skips when the
    /// backup is not on this machine.
    #[test]
    #[ignore = "unpacks the 20 MB plant backup; run with --ignored"]
    fn the_plant_test_backup_restores() {
        let backup = Path::new(r"D:\work\plant-code\cad\pid-test-data");
        if !backup.join("PlantData~2~711.zip").is_file() {
            eprintln!("skipped: no plant backup at {}", backup.display());
            return;
        }
        let root = scratch("real");
        let target = root.join("out");
        let started = std::time::Instant::now();
        let restored = restore(backup, Some(&target)).unwrap();
        eprintln!(
            "{} drawings in {:?}, first {}",
            restored.drawings.len(),
            started.elapsed(),
            restored.drawings[0].display()
        );
        assert!(restored.drawings.len() >= 50, "{}", restored.drawings.len());
        assert!(target.join("Ref").join("Symbols").is_dir());
        let first = &restored.drawings[0];
        assert!(first
            .file_name()
            .unwrap()
            .to_string_lossy()
            .contains("流程图"));
        // The drawing imports, and the walk up from it finds the copied Ref.
        let import = crate::io::pid::load_pid(first).unwrap();
        eprintln!(
            "{} entities from symbol roots {:?}",
            import.summary.drawn, import.summary.symbol_library
        );
        assert!(import.summary.drawn > 0);
        assert!(
            import
                .summary
                .symbol_library
                .iter()
                .any(|root| root.starts_with(&target)),
            "{:?}",
            import.summary.symbol_library
        );
        let _ = std::fs::remove_dir_all(&root);
    }
}
