//! The folder restores are kept in: where it is, what it holds, and letting
//! go of what it holds.
//!
//! A backup whose drawings are archived is unpacked before it is browsed, and
//! the unpacked copy is kept so that the next open of the same backup is
//! instant ([`super::restore`]). The copies add up: a plant is 160 MB, and
//! every state of every backup that was ever opened has one. They sat under
//! the temp folder, which is the system drive on most machines, with nothing
//! to say how much they were or to remove them.
//!
//! The folder can now be another one ([`restore_cache_in`]), what it holds
//! can be measured ([`usage`]) and removed ([`clear`]). Only what a restore
//! put there is counted and removed: a folder named by a cache key that
//! carries the mark of a finished restore, and the staging folder of a
//! restore that was cut short long enough ago to be nobody's work in
//! progress. A cache folder somebody pointed at a place with other things in
//! it loses none of them.

use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use super::{cache_key, DONE_MARKER};

/// The name of the cache folder under the temp folder.
const CACHE_DIR: &str = "ocs-pid-backups";

/// How long a staging folder must have lain untouched before it is taken
/// for a restore that was cut short. A restore is seconds; another editor's
/// restore under way is never this old.
const ABANDONED_AFTER: Duration = Duration::from_secs(600);

/// The folder restores are kept in when no other is set:
/// `<temp>/ocs-pid-backups`.
pub fn default_cache_root() -> PathBuf {
    std::env::temp_dir().join(CACHE_DIR)
}

/// The restore folder of `source` in the cache folder `root`: `<root>/<key>`,
/// keyed as `pid_pdf`'s template cache is (path, size, modification time). A
/// folder source is keyed by its drawing archives as well.
pub fn restore_cache_in(root: &Path, source: &Path) -> PathBuf {
    root.join(cache_key(source))
}

/// One restore a cache folder holds.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CachedRestore {
    /// Its folder.
    pub folder: PathBuf,
    /// The bytes of its files, and how many they are.
    pub bytes: u64,
    pub files: usize,
}

/// What a cache folder holds.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CacheUsage {
    /// The cache folder.
    pub root: PathBuf,
    /// The restore the caller is using, when it lies in the cache folder.
    pub current: Option<CachedRestore>,
    /// Every other restore in the cache folder: how many, and their bytes.
    pub others: usize,
    pub other_bytes: u64,
    /// What a [`clear`] just removed: how many restores, and their bytes.
    pub freed: Option<(usize, u64)>,
}

/// Whether `name` is the name a cache key gives a restore folder.
fn is_key(name: &str) -> bool {
    name.len() == 16
        && name
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

/// What lies under a folder, at any depth.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(super) struct Contents {
    /// The bytes of the files, and how many they are.
    pub(super) bytes: u64,
    pub(super) files: usize,
    /// How many folders, the folder itself not among them.
    pub(super) folders: usize,
}

/// The files and the folders under `folder`. A restore is marked with these
/// numbers when it is finished ([`super::restore`]), and is whole while it
/// still measures as much.
pub(super) fn measure(folder: &Path) -> Contents {
    let mut found = Contents::default();
    let mut pending = vec![folder.to_path_buf()];
    while let Some(dir) = pending.pop() {
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let Ok(kind) = entry.file_type() else {
                continue;
            };
            if kind.is_dir() {
                found.folders += 1;
                pending.push(entry.path());
            } else if let Ok(meta) = entry.metadata() {
                found.bytes += meta.len();
                found.files += 1;
            }
        }
    }
    found
}

/// The folders of `root` that restores put there: the finished ones, and
/// the staging folders nobody has touched for [`ABANDONED_AFTER`].
fn restore_folders(root: &Path, now: SystemTime) -> Vec<PathBuf> {
    let Ok(entries) = std::fs::read_dir(root) else {
        return Vec::new();
    };
    let mut folders: Vec<PathBuf> = entries
        .flatten()
        .filter(|entry| entry.file_type().is_ok_and(|kind| kind.is_dir()))
        .filter(|entry| {
            let name = entry.file_name().to_string_lossy().into_owned();
            if is_key(&name) {
                return entry.path().join(DONE_MARKER).is_file();
            }
            // `<key>.partial-<process id>`: a restore on its way.
            let Some((key, process)) = name.split_once(".partial-") else {
                return false;
            };
            is_key(key)
                && !process.is_empty()
                && process.bytes().all(|b| b.is_ascii_digit())
                && entry
                    .metadata()
                    .and_then(|meta| meta.modified())
                    .ok()
                    .and_then(|modified| now.duration_since(modified).ok())
                    .is_some_and(|age| age >= ABANDONED_AFTER)
        })
        .map(|entry| entry.path())
        .collect();
    folders.sort();
    folders
}

/// What the cache folder `root` holds. `current` is the restore folder the
/// caller is using: when it lies in `root` it is told apart from the others.
pub fn usage(root: &Path, current: Option<&Path>) -> CacheUsage {
    let mut usage = CacheUsage {
        root: root.to_path_buf(),
        ..CacheUsage::default()
    };
    for folder in restore_folders(root, SystemTime::now()) {
        let Contents { bytes, files, .. } = measure(&folder);
        if current == Some(folder.as_path()) {
            usage.current = Some(CachedRestore {
                folder,
                bytes,
                files,
            });
        } else {
            usage.others += 1;
            usage.other_bytes += bytes;
        }
    }
    usage
}

/// Remove every restore in the cache folder `root` but `current`, and say
/// what the folder holds afterwards and what was removed. A restore that
/// cannot be removed (a file of it is open somewhere) stays, counted among
/// the others; the first such failure is the error when nothing at all
/// could be removed.
pub fn clear(root: &Path, current: Option<&Path>) -> Result<CacheUsage, String> {
    let mut freed = (0, 0);
    let mut failure = None;
    for folder in restore_folders(root, SystemTime::now()) {
        if current == Some(folder.as_path()) {
            continue;
        }
        let bytes = measure(&folder).bytes;
        match std::fs::remove_dir_all(&folder) {
            Ok(()) => {
                freed.0 += 1;
                freed.1 += bytes;
            }
            Err(error) => {
                failure.get_or_insert_with(|| {
                    crate::tf!("cannot remove {}: {}", folder.display(), error).into_owned()
                });
            }
        }
    }
    match failure {
        Some(error) if freed.0 == 0 => Err(error),
        _ => Ok(CacheUsage {
            freed: Some(freed),
            ..usage(root, current)
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(name: &str) -> PathBuf {
        let root = std::env::temp_dir().join(format!("ocs-pid-cache-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        root
    }

    /// A finished restore of `bytes` bytes in two files.
    fn restore_of(root: &Path, key: &str, bytes: usize) -> PathBuf {
        let folder = root.join(key);
        std::fs::create_dir_all(folder.join("00")).unwrap();
        std::fs::write(folder.join(DONE_MARKER), key).unwrap();
        std::fs::write(folder.join("00").join("A.pid"), vec![0u8; bytes - key.len()]).unwrap();
        folder
    }

    #[test]
    fn a_cache_folder_is_measured_and_the_restore_in_use_told_apart() {
        let root = scratch("usage");
        let mine = restore_of(&root, "00000000000000aa", 1000);
        restore_of(&root, "00000000000000bb", 300);
        restore_of(&root, "00000000000000cc", 50);
        let found = usage(&root, Some(&mine));
        assert_eq!(
            found,
            CacheUsage {
                root: root.clone(),
                current: Some(CachedRestore {
                    folder: mine.clone(),
                    bytes: 1000,
                    files: 2,
                }),
                others: 2,
                other_bytes: 350,
                freed: None,
            }
        );
        // A restore that lies elsewhere is not in this folder: all three
        // are others then.
        let elsewhere = usage(&root, Some(Path::new("D:/elsewhere/00000000000000aa")));
        assert_eq!(elsewhere.current, None);
        assert_eq!((elsewhere.others, elsewhere.other_bytes), (3, 1350));
        // A folder that is not there holds nothing.
        assert_eq!(
            usage(&root.join("absent"), None),
            CacheUsage {
                root: root.join("absent"),
                ..CacheUsage::default()
            }
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn a_folder_is_measured_by_its_files_and_its_folders() {
        let root = scratch("measure");
        std::fs::create_dir_all(root.join("a").join("b")).unwrap();
        std::fs::create_dir_all(root.join("empty")).unwrap();
        std::fs::write(root.join("top.txt"), b"12345").unwrap();
        std::fs::write(root.join("a").join("b").join("deep.txt"), b"123").unwrap();
        // Every folder below it counts, an empty one too; itself does not.
        assert_eq!(
            measure(&root),
            Contents {
                bytes: 8,
                files: 2,
                folders: 3,
            }
        );
        // A folder that is not there holds nothing.
        assert_eq!(measure(&root.join("absent")), Contents::default());
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn clearing_removes_the_other_restores_and_nothing_else() {
        let root = scratch("clear");
        let mine = restore_of(&root, "00000000000000aa", 1000);
        let other = restore_of(&root, "00000000000000bb", 300);
        // What is in the folder and is no restore: a file, a folder with
        // another kind of name, a folder with a key's name that carries no
        // mark of a finished restore, a staging folder touched just now.
        std::fs::write(root.join("notes.txt"), b"keep").unwrap();
        std::fs::create_dir_all(root.join("My drawings")).unwrap();
        std::fs::write(root.join("My drawings").join("A.pid"), b"keep").unwrap();
        std::fs::create_dir_all(root.join("00000000000000dd")).unwrap();
        std::fs::write(root.join("00000000000000dd").join("A.pid"), b"keep").unwrap();
        std::fs::create_dir_all(root.join("00000000000000ee.partial-4242")).unwrap();

        let cleared = clear(&root, Some(&mine)).unwrap();
        assert_eq!(cleared.freed, Some((1, 300)));
        assert_eq!(cleared.others, 0);
        assert_eq!(cleared.current.as_ref().map(|restore| restore.bytes), Some(1000));
        assert!(mine.join("00").join("A.pid").is_file());
        assert!(!other.exists());
        for kept in [
            "notes.txt",
            "My drawings/A.pid",
            "00000000000000dd/A.pid",
            "00000000000000ee.partial-4242",
        ] {
            assert!(root.join(kept).exists(), "{kept} is gone");
        }
        // Nothing left to remove: nothing is, and that is no error.
        assert_eq!(clear(&root, Some(&mine)).unwrap().freed, Some((0, 0)));
        // With no restore in use the last one goes too.
        assert_eq!(clear(&root, None).unwrap().freed, Some((1, 1000)));
        assert!(!mine.exists());
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn a_staging_folder_counts_once_it_was_left_long_ago() {
        let root = scratch("staging");
        let staging = root.join("00000000000000ee.partial-4242");
        std::fs::create_dir_all(&staging).unwrap();
        std::fs::write(staging.join("A.pid"), vec![0u8; 70]).unwrap();
        // Just made: somebody's restore under way.
        assert!(restore_folders(&root, SystemTime::now()).is_empty());
        // Seen from eleven minutes later: left behind.
        let later = SystemTime::now() + Duration::from_secs(660);
        assert_eq!(restore_folders(&root, later), [staging.clone()]);
        // Names that only look like it are never taken.
        for name in [
            "00000000000000ee.partial-",
            "00000000000000ee.partial-12a",
            "0000.partial-12",
            "00000000000000EE",
        ] {
            std::fs::create_dir_all(root.join(name)).unwrap();
        }
        assert_eq!(restore_folders(&root, later), [staging]);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn a_restore_folder_is_its_key_under_the_cache_folder() {
        let root = scratch("key");
        let source = root.join("backup.zip");
        std::fs::write(&source, b"PK\x03\x04").unwrap();
        let folder = restore_cache_in(&root, &source);
        assert_eq!(folder.parent(), Some(root.as_path()));
        let name = folder.file_name().unwrap().to_string_lossy().into_owned();
        assert!(is_key(&name), "{name}");
        // The default folder is under the temp folder, as it always was.
        assert_eq!(
            super::super::restore_cache(&source),
            std::env::temp_dir().join("ocs-pid-backups").join(&name)
        );
        let _ = std::fs::remove_dir_all(&root);
    }
}
