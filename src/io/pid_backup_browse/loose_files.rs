//! The files of a backup folder that are none of its numbered parts.
//!
//! A backup is a manifest, a dump and the `RefData~` / `PlantData~` files
//! the manifest registers. The export leaves more beside them: the dump's
//! log (`Export.log`, which is where the row count of every table is) and
//! the plant's configuration (`PlantConfig.xml`). The folder scan does not
//! list them; this does, so the panel shows every file the folder holds.

use std::path::Path;

use super::labels::natural_cmp;
use super::manifest::format_of;
use super::{BackupFile, FileKind};

/// The files directly in `backup_root` that `listed` does not name, by name.
///
/// Folders are not files of the backup (a restore keeps its drawings in
/// them). A drawing has its row under the plant groups, and the restore's
/// own marker is not the plant's.
pub fn loose_files(backup_root: &Path, listed: &[BackupFile]) -> Vec<BackupFile> {
    let Ok(entries) = std::fs::read_dir(backup_root) else {
        return Vec::new();
    };
    let mut files: Vec<BackupFile> = entries
        .flatten()
        .filter(|entry| entry.file_type().is_ok_and(|kind| kind.is_file()))
        .filter_map(|entry| {
            let name = entry.file_name().to_string_lossy().into_owned();
            let known = listed
                .iter()
                .any(|file| file.name.eq_ignore_ascii_case(&name));
            if known
                || name == crate::io::pid_backup::DONE_MARKER
                || crate::io::pid_pdf::is_pid(&entry.path())
            {
                return None;
            }
            Some(BackupFile {
                format: format_of(&name).unwrap_or_default(),
                size: entry.metadata().map(|meta| meta.len()).unwrap_or(0),
                kind: FileKind::Other,
                name,
                ..BackupFile::default()
            })
        })
        .collect();
    files.sort_by(|left, right| natural_cmp(&left.name, &right.name));
    files
}

/// Where the loose files go among `files`: after the manifest and the dump,
/// before the numbered archives.
pub fn insert_at(files: &[BackupFile]) -> usize {
    files
        .iter()
        .rposition(|file| matches!(file.kind, FileKind::Manifest | FileKind::ExportDump))
        .map_or(0, |at| at + 1)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(name: &str) -> std::path::PathBuf {
        let root = std::env::temp_dir().join(format!(
            "ocs-pid-backup-loose-{name}-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        root
    }

    fn listed(name: &str, kind: FileKind) -> BackupFile {
        BackupFile {
            name: name.to_string(),
            kind,
            ..BackupFile::default()
        }
    }

    #[test]
    fn the_files_the_scan_leaves_out_are_listed_by_name() {
        let root = scratch("listed");
        for (name, bytes) in [
            ("Manifest.txt", &b"m"[..]),
            ("Export.dmp", b"dump"),
            ("PlantConfig.xml", b"<PlantConfig/>"),
            ("Export.log", b"Connected"),
            ("RefData~4~680", b"rules"),
            ("export10.LOG", b"ten"),
            ("export2.log", b"two"),
            ("notes", b"no extension"),
            // Not the plant's: the restore's marker, and a drawing.
            (crate::io::pid_backup::DONE_MARKER, b"key"),
            ("A01.pid", b"drawing"),
        ] {
            std::fs::write(root.join(name), bytes).unwrap();
        }
        // A folder is not a file of the backup.
        std::fs::create_dir_all(root.join("Ref")).unwrap();
        let known = [
            listed("Manifest.txt", FileKind::Manifest),
            listed("Export.dmp", FileKind::ExportDump),
            // The scan's name and the folder's may differ in case.
            listed("REFDATA~4~680", FileKind::RefData),
        ];
        let loose = loose_files(&root, &known);
        assert_eq!(
            loose
                .iter()
                .map(|file| (file.name.as_str(), file.format.as_str(), file.size))
                .collect::<Vec<_>>(),
            [
                ("Export.log", "LOG", 9),
                ("export2.log", "LOG", 3),
                ("export10.LOG", "LOG", 3),
                ("notes", "", 12),
                ("PlantConfig.xml", "XML", 14),
            ]
        );
        assert!(loose.iter().all(|file| file.kind == FileKind::Other
            && file.members.is_empty()
            && !file.missing
            && file.record.is_none()));
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn a_folder_that_cannot_be_read_has_no_loose_files() {
        let root = scratch("absent");
        assert!(loose_files(&root.join("not-there"), &[]).is_empty());
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn the_loose_files_follow_the_manifest_and_the_dump() {
        let files = [
            listed("Manifest.txt", FileKind::Manifest),
            listed("Export.dmp", FileKind::ExportDump),
            listed("RefData~4~680", FileKind::RefData),
            listed("PlantData~2~711.zip", FileKind::PlantData),
        ];
        assert_eq!(insert_at(&files), 2);
        // No dump: after the manifest. Neither: first.
        assert_eq!(insert_at(&files[..1]), 1);
        assert_eq!(insert_at(&files[2..]), 0);
        assert_eq!(insert_at(&[]), 0);
    }
}
