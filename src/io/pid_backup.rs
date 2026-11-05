//! PIDBACKUPOPEN's engine: a SmartPlant P&ID backup -- a zip of one, or the
//! backup folder itself -- made into a folder whose `.pid` drawings open like
//! any other.
//!
//! A backup keeps its drawings in `PlantData~<schema>~<id>.zip` archives
//! beside the `Ref` reference data (symbols under `Ref/Symbols`). A folder
//! that already holds loose `.pid` files is used where it is, read-only. One
//! whose drawings are still archived, and any zip, is unpacked into a restore
//! folder: by default a cache under the temp folder keyed by the source, so a
//! second open of the same backup reuses it. The drawings land beside a `Ref`
//! there -- the backup's own, copied in for a folder source -- so the
//! importer's walk up from a drawing finds the symbol library.
//!
//! Zip member names are written by Windows tools in the system code page
//! (GBK on the plants this serves), not UTF-8; they are decoded as pid-parse
//! decodes them, and a name that would leave the restore folder refuses the
//! whole restore.

use std::io::Read;
use std::path::{Component, Path, PathBuf};

/// Written last into a finished restore folder; holds the source's cache key.
const DONE_MARKER: &str = ".ocs-restored";

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

/// The default restore folder of `source`: `<temp>/ocs-pid-backups/<hash>`,
/// keyed as `pid_pdf`'s template cache is (path, size, modification time).
/// A folder source is keyed by its drawing archives as well.
pub fn restore_cache(source: &Path) -> PathBuf {
    std::env::temp_dir()
        .join("ocs-pid-backups")
        .join(cache_key(source))
}

fn cache_key(source: &Path) -> String {
    use std::hash::{Hash, Hasher};
    let mut hasher = std::hash::DefaultHasher::new();
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
    }
    format!("{:016x}", hasher.finish())
}

/// Make `source` -- a backup zip or folder -- browsable, unpacking it into
/// `target` (default [`restore_cache`]) when it has to be.
pub fn restore(source: &Path, target: Option<&Path>) -> Result<Restored, String> {
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
        let reference = source.join("Ref");
        return build(source, target, |staging| {
            for archive in &archives {
                extract_zip(archive, staging)?;
            }
            if reference.is_dir() {
                copy_dir(&reference, &staging.join("Ref"))?;
            }
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
    build(source, target, |staging| {
        extract_zip(source, staging)?;
        // A zip of a whole backup still files its drawings in PlantData
        // archives; unpack those where they lie so each lands beside its Ref.
        for root in backup_roots(staging) {
            for archive in plant_data_archives(&root) {
                extract_zip(&archive, &root)?;
            }
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
    fill: impl FnOnce(&Path) -> Result<(), String>,
) -> Result<Restored, String> {
    let key = cache_key(source);
    let owned = target.is_none();
    let target = target.map_or_else(|| restore_cache(source), Path::to_path_buf);
    let marker = target.join(DONE_MARKER);
    if std::fs::read_to_string(&marker).is_ok_and(|stamp| stamp.trim() == key) {
        return Ok(Restored {
            drawings: pid_drawings(&target),
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
        drawings: pid_drawings(&target),
        root: target,
        extracted: true,
        reused: false,
    })
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
