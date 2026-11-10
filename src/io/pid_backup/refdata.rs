//! Putting a backup's reference data back where its drawings look for it.
//!
//! A SmartPlant backup stores the plant's reference data -- the symbol
//! library, the border templates, the rules, the report templates, the data
//! maps -- as numbered volumes beside the drawings: `RefData~4~681.zip` is
//! the folder `Ref\Symbols`, `RefData~4~680` is the file `Ref\rules.rul`.
//! Only `Manifest.txt` says which is which (its `File` lines).
//!
//! A drawing names its symbols and its border template by their path on the
//! plant's server, and the importer finds local copies by walking up from the
//! drawing to a `Ref` folder ([`crate::io::pid`]). So a restore that unpacks
//! the drawings alone opens them without their symbols' own geometry and
//! without their title blocks, unless somebody had unpacked a `Ref` folder
//! beside the backup by hand.
//!
//! [`restore_reference_data`] unpacks every volume the backup carries to the
//! place the manifest says it came from, under `Ref` in the restore folder:
//!
//! ```text
//! RefData~4~681.zip   Ref\Symbols             ->  <restore>\Ref\Symbols\...
//! RefData~4~682.zip   Ref\Template Files      ->  <restore>\Ref\Template Files\...
//! RefData~4~680       Ref\rules.rul           ->  <restore>\Ref\rules.rul
//! ```
//!
//! The folder the volumes came from is the one their paths on the server
//! have in common. It is not always called `Ref` (`ZGSY P&ID Reference Data`
//! is one plant's name for it); in the restore it always is, since that is
//! the name the importer looks for.

use std::path::{Component, Path, PathBuf};

use crate::io::pid_backup_browse::manifest::{manifest_model, FileRecord, FileRecordKind};

/// The folder of a restore that holds the reference data.
pub const REFERENCE_DIR: &str = "Ref";

/// The manifest's schema number of the reference data volumes
/// (`RefData~4~…`); `2` is the plant's drawings.
const REFERENCE_SCHEMA: &str = "4";

/// The `File` lines of the manifest in `backup_root` that are reference data
/// the backup carries: each with the file it is stored in. Empty when there
/// is no manifest to read.
fn reference_volumes(backup_root: &Path) -> Vec<(FileRecord, PathBuf)> {
    let Ok(bytes) = std::fs::read(backup_root.join("Manifest.txt")) else {
        return Vec::new();
    };
    let manifest = pid_parse::backup::parse_manifest_bytes(&bytes);
    manifest_model(&manifest)
        .files
        .into_iter()
        .filter(|record| record.schema == REFERENCE_SCHEMA && record.backed_up)
        .filter_map(|record| {
            let stored = backup_root.join(record.stored_name()?);
            stored.is_file().then_some((record, stored))
        })
        .collect()
}

/// The parts of a path as the manifest writes it (`\\host\share\Ref`).
fn parts(source: &str) -> Vec<&str> {
    source
        .split(['\\', '/'])
        .filter(|part| !part.is_empty())
        .collect()
}

/// How many leading parts the folders the volumes lay in have in common:
/// the reference data folder of the plant. A volume's own name is never
/// part of it, so a backup with a single volume (`...\Ref\Symbols`) still
/// knows what the volume is called.
fn shared_depth(sources: &[&str]) -> usize {
    let mut shared: Option<Vec<&str>> = None;
    for source in sources {
        let mut folder = parts(source);
        folder.pop();
        shared = Some(match shared {
            None => folder,
            Some(shared) => shared
                .iter()
                .zip(&folder)
                .take_while(|(left, right)| left.eq_ignore_ascii_case(right))
                .map(|(left, _)| *left)
                .collect(),
        });
    }
    shared.map_or(0, |shared| shared.len())
}

/// Where under `reference` a volume goes: its path on the server from below
/// the first `shared` parts. `None` for a path that leaves `reference` (one
/// that goes up, or names a drive: the manifest is plain text, and nothing
/// else checks what it says), and for one with nothing below the shared
/// folder.
fn destination(reference: &Path, source: &str, shared: usize) -> Option<PathBuf> {
    let mut path = reference.to_path_buf();
    for part in parts(source).into_iter().skip(shared) {
        let mut components = Path::new(part).components();
        match (components.next(), components.next()) {
            (Some(Component::Normal(normal)), None) => path.push(normal),
            _ => return None,
        }
    }
    (path != reference).then_some(path)
}

/// Unpack the reference data volumes of the backup in `backup_root` under
/// `<into>\Ref`, each where the manifest says it came from. Returns how many
/// volumes that was.
///
/// What is already under `<into>\Ref` stays unless a volume brings the same
/// file: a `Ref` folder somebody unpacked beside the backup fills in what
/// the backup's own volumes do not hold, and does not replace what they do.
pub(super) fn restore_reference_data(backup_root: &Path, into: &Path) -> Result<usize, String> {
    let reference = into.join(REFERENCE_DIR);
    let volumes = reference_volumes(backup_root);
    let sources: Vec<&str> = volumes
        .iter()
        .map(|(record, _)| record.source.as_str())
        .collect();
    let shared = shared_depth(&sources);
    let mut restored = 0;
    for (record, stored) in &volumes {
        let Some(target) = destination(&reference, &record.source, shared) else {
            continue;
        };
        match record.kind {
            FileRecordKind::Folder if super::is_zip(stored) => {
                super::extract_zip(stored, &target)?;
            }
            FileRecordKind::SingleFile => {
                if let Some(parent) = target.parent() {
                    std::fs::create_dir_all(parent)
                        .map_err(|error| format!("cannot create {}: {error}", parent.display()))?;
                }
                std::fs::copy(stored, &target)
                    .map_err(|error| format!("cannot copy to {}: {error}", target.display()))?;
            }
            // A folder that is no archive: nothing this knows how to put
            // back.
            _ => continue,
        }
        restored += 1;
    }
    Ok(restored)
}

/// The files of `backup_root` a change of which makes a restore stale, beside
/// the drawing archives: the manifest and the reference data volumes.
pub(super) fn stamped_files(backup_root: &Path) -> Vec<PathBuf> {
    let mut files = vec![backup_root.join("Manifest.txt")];
    files.extend(
        reference_volumes(backup_root)
            .into_iter()
            .map(|(_, stored)| stored),
    );
    files.sort();
    files
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Where each of `sources` goes under `Ref`, as text with `/` between
    /// the parts; `None` for one that is refused.
    fn placed(sources: &[&str]) -> Vec<Option<String>> {
        let reference = Path::new("Ref");
        let shared = shared_depth(sources);
        sources
            .iter()
            .map(|source| {
                destination(reference, source, shared).map(|path| {
                    path.components()
                        .map(|part| part.as_os_str().to_string_lossy().into_owned())
                        .collect::<Vec<_>>()
                        .join("/")
                })
            })
            .collect()
    }

    fn some(paths: &[&str]) -> Vec<Option<String>> {
        paths.iter().map(|path| Some((*path).to_string())).collect()
    }

    #[test]
    fn a_volume_goes_where_it_lay_under_the_folder_the_volumes_share() {
        assert_eq!(
            placed(&[
                r"\\host\P\SQPlant\Ref\Symbols",
                r"\\host\P\SQPlant\Ref\Template Files",
                r"\\host\P\SQPlant\Ref\rules.rul",
                r"\\host\P\SQPlant\Ref\Symbols\Assemblies",
            ]),
            some(&[
                "Ref/Symbols",
                "Ref/Template Files",
                "Ref/rules.rul",
                "Ref/Symbols/Assemblies",
            ])
        );
        // The shared folder has the plant's own name; here it is `Ref`. The
        // server's spelling of it need not agree in case from line to line.
        assert_eq!(
            placed(&[
                r"\\HOST\p\ZGSY P&ID Reference Data\Template Files",
                r"\\host\P\zgsy p&id reference data\rules.rul",
            ]),
            some(&["Ref/Template Files", "Ref/rules.rul"])
        );
        // One volume alone keeps its name: the folder it lay in is the
        // shared one.
        assert_eq!(placed(&[r"\\host\P\Ref\Symbols"]), some(&["Ref/Symbols"]));
        assert_eq!(
            placed(&["D:/Plant/Ref/rules.rul"]),
            some(&["Ref/rules.rul"])
        );
        // Only volumes nested in one another: the outer one's folder.
        assert_eq!(
            placed(&[r"\\h\P\Ref\Symbols", r"\\h\P\Ref\Symbols\Assemblies"]),
            some(&["Ref/Symbols", "Ref/Symbols/Assemblies"])
        );
        assert_eq!(placed(&[]), Vec::<Option<String>>::new());
    }

    #[test]
    fn nothing_is_placed_outside_the_reference_folder() {
        let placed = placed(&[
            r"\\host\P\Ref\Symbols",
            r"\\host\P\Ref\..\..\..\outside.txt",
            r"\\host\P\Ref\Symbols\..\..\..\outside",
            r"\\host\P\Ref\.\rules.rul",
            r"\\host\P\Ref\C:\Windows\outside",
        ]);
        assert_eq!(placed[0].as_deref(), Some("Ref/Symbols"));
        assert!(placed[1..].iter().all(Option::is_none), "{placed:?}");
        // The shared folder itself is no volume: nothing lies below it.
        assert_eq!(destination(Path::new("Ref"), r"\\host\P\Ref", 3), None);
    }
}
