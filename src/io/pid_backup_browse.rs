//! What a SmartPlant P&ID backup holds, in one index the browser panel draws.
//!
//! [`crate::io::pid_backup::restore`] makes a backup browsable; this module
//! then reads it once: the `Manifest.txt` summary, the backup files (with the
//! member lists of the archives), the plant-group tree, the `T_DRAWING` rows
//! matched to the `.pid` files the restore produced, and the `Ref` reference
//! data tree.
//!
//! The database side is optional. A backup whose `Export.dmp` is absent, is a
//! SQL Server MTF stream, or cannot be read still lists its drawings -- by the
//! folder they were unpacked into -- and says so in [`BackupIndex::warnings`].
//! A `T_DRAWING` row whose file is not on disk is kept, with
//! [`DrawingEntry::pid_path`] empty, so the panel can show it as missing rather
//! than hide a drawing the plant knows about. A `.pid` the table does not
//! mention is listed too, under [`UNREGISTERED_GROUP`], so nothing the backup
//! carries is lost from the tree.
//!
//! Matching a row to a file is [`match_drawings`]: a pure function over the
//! rows and the `.pid` paths, testable without an Oracle dump.

use std::path::{Path, PathBuf};

use pid_parse::backup::{
    scan_backup_folder, BackupFolderScan, ExportDumpFormat, ExportDumpScan, ZipListing,
};
use pid_parse::publish::oracle_exp::{
    backup_path_key, plant_group_path, read_backup_drawings, read_plant_groups, BackupDrawing,
};

/// Group the drawings the table does not mention are filed under.
pub const UNREGISTERED_GROUP: &str = "Unregistered";

/// Group a drawing with no folder or plant group is filed under.
const UNGROUPED: &str = "(ungrouped)";

/// How deep the `Ref` tree is walked before a runaway directory is cut off.
const REF_SEARCH_DEPTH: usize = 16;

/// One backup, indexed for the browser panel.
#[derive(Debug, Clone)]
pub struct BackupIndex {
    /// The zip or folder the user picked.
    pub source: PathBuf,
    /// The folder the restore produced (a loose-drawing folder source is its
    /// own restore folder).
    pub restored: PathBuf,
    /// The folder holding `Manifest.txt` / `Export.dmp` / `PlantData~*`.
    pub backup_root: PathBuf,
    /// `true` when the backup is a folder used where it lies: never written.
    pub read_only: bool,
    /// The `Manifest.txt` singleton fields the panel summarises.
    pub manifest: Option<ManifestSummary>,
    /// `Manifest.txt`, `Export.dmp`, every `RefData~*` and `PlantData~*`.
    pub files: Vec<BackupFile>,
    /// The plant-group tree; every drawing index appears under one group.
    pub plant_groups: Vec<GroupNode>,
    /// One entry per `T_DRAWING` row plus every unmatched `.pid`.
    pub drawings: Vec<DrawingEntry>,
    /// The `Ref` reference-data tree.
    pub ref_tree: RefNode,
    /// Everything the panel should tell the user went wrong or was skipped.
    pub warnings: Vec<String>,
}

/// The `Manifest.txt` fields a backup always carries.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ManifestSummary {
    pub name: Option<String>,
    pub version: Option<String>,
    pub date_created: Option<String>,
    pub backup_type: Option<String>,
    pub spid: Option<String>,
}

/// One file in the backup root, with its format and archive members.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BackupFile {
    /// File name as it appears on disk.
    pub name: String,
    /// What the file is: `Manifest`, `Export.dmp`, `RefData` or `PlantData`.
    pub kind: FileKind,
    /// Human-readable format, e.g. `Oracle exp (EXPORT:V12.01.00)`.
    pub format: String,
    /// Length in bytes.
    pub size: u64,
    /// Member names when the file is a zip; empty otherwise.
    pub members: Vec<String>,
    /// The `EXPORT:V…` line for an Oracle `exp` dump.
    pub version_line: Option<String>,
}

/// Which of the backup's file families a [`BackupFile`] belongs to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FileKind {
    Manifest,
    ExportDump,
    RefData,
    PlantData,
}

/// One node of the plant-group (or, degraded, folder) tree.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct GroupNode {
    /// Group name alone, e.g. `00`.
    pub name: String,
    /// Full path from the root, e.g. `SQPlant / 00 / 00`.
    pub path: String,
    /// Nested groups.
    pub children: Vec<GroupNode>,
    /// Indices into [`BackupIndex::drawings`] filed directly here.
    pub drawings: Vec<usize>,
}

/// One drawing of the backup.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DrawingEntry {
    /// The name the table (or the file) gives the drawing.
    pub name: String,
    /// The restored `.pid`, or `None` when the row's file is missing.
    pub pid_path: Option<PathBuf>,
    /// `plant_group_path` of its group, or its folder when degraded.
    pub group_path: String,
    /// Every `T_DRAWING` column, empty values included.
    pub meta: Vec<(String, String)>,
}

impl DrawingEntry {
    /// Whether the drawing has a file to open.
    pub fn is_missing(&self) -> bool {
        self.pid_path.is_none()
    }
}

/// The `Ref` reference-data tree.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RefNode {
    /// Directory or (for the root) `Ref` name.
    pub name: String,
    /// Subdirectories, name-sorted.
    pub dirs: Vec<RefNode>,
    /// Files directly in this directory, name-sorted.
    pub files: Vec<RefFile>,
}

impl RefNode {
    /// Every file in this subtree, depth first.
    pub fn file_count(&self) -> usize {
        self.files.len() + self.dirs.iter().map(RefNode::file_count).sum::<usize>()
    }
}

/// One file in the `Ref` tree.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RefFile {
    pub name: String,
    pub size: u64,
}

/// A `T_DRAWING` row reduced to what matching needs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DrawingRow {
    pub name: String,
    /// [`backup_path_key`] of the row's `PATH`.
    pub path_key: String,
    /// `plant_group_path` of the row's group.
    pub group_path: String,
    /// Every `T_DRAWING` column, empty values included.
    pub meta: Vec<(String, String)>,
}

/// [`match_drawings`]'s outcome: the rows in order, and the `.pid` files no
/// row claimed.
#[derive(Debug, Clone, Default)]
pub struct MatchedDrawings {
    pub entries: Vec<DrawingEntry>,
    pub unmatched: Vec<PathBuf>,
}

/// Build the index of `source` -- a backup zip or folder -- restoring it into
/// `target` (default [`crate::io::pid_backup::restore_cache`]) when it has to
/// be unpacked.
pub fn build_index(source: &Path, target: Option<&Path>) -> Result<BackupIndex, String> {
    let restored = crate::io::pid_backup::restore(source, target)?;
    let read_only = source.is_dir();
    let backup_root = if read_only && is_backup_folder(source) {
        source.to_path_buf()
    } else {
        find_backup_root(&restored.root)
    };
    let scan = scan_backup_folder(&backup_root)
        .map_err(|error| format!("cannot scan {}: {error}", backup_root.display()))?;

    let mut warnings = Vec::new();
    let files = describe_files(&backup_root, &scan, &mut warnings);
    let manifest = scan.manifest.as_ref().map(manifest_summary);

    let mut drawings = Vec::new();
    let mut group_paths = Vec::new();
    match dump_drawings(&backup_root, &scan, &mut warnings) {
        Some(rows) => {
            let (entries, unmatched) =
                assemble_drawings(&rows, &restored.drawings, &backup_root, &restored.root);
            group_paths.extend(entries.iter().map(|entry| entry.group_path.clone()));
            drawings.extend(entries);
            if !unmatched.is_empty() {
                warnings.push(format!(
                    "{} .pid file(s) are not in the drawing table; they are listed under \"{UNREGISTERED_GROUP}\"",
                    unmatched.len()
                ));
                for entry in &unmatched {
                    group_paths.push(entry.group_path.clone());
                }
                drawings.extend(unmatched);
            }
        }
        None => {
            let entries = folder_entries(&restored.drawings, &backup_root, &restored.root);
            group_paths.extend(entries.iter().map(|entry| entry.group_path.clone()));
            drawings.extend(entries);
        }
    }
    let plant_groups = build_group_tree(&group_paths);
    // A backup folder carries its own `Ref`; a restore of an archive has one
    // copied beside the drawings, which is the one to show when the backup
    // root has none.
    let mut ref_tree = read_ref_tree(&backup_root.join("Ref"));
    if ref_tree.dirs.is_empty() && ref_tree.files.is_empty() {
        let restored_ref = restored.root.join("Ref");
        if restored_ref.is_dir() {
            ref_tree = read_ref_tree(&restored_ref);
        }
    }

    Ok(BackupIndex {
        source: source.to_path_buf(),
        restored: restored.root,
        backup_root,
        read_only,
        manifest,
        files,
        plant_groups,
        drawings,
        ref_tree,
        warnings,
    })
}

/// Whether `dir` looks like the folder of a backup: it holds the manifest, the
/// dump, or the drawing archives.
pub fn is_backup_folder(dir: &Path) -> bool {
    if dir.join("Manifest.txt").is_file() || dir.join("Export.dmp").is_file() {
        return true;
    }
    std::fs::read_dir(dir).is_ok_and(|entries| {
        entries.flatten().any(|entry| {
            entry
                .file_name()
                .to_string_lossy()
                .starts_with("PlantData~")
        })
    })
}

/// The layer of a restore folder that holds the backup itself: the folder
/// itself when it is one, else the first folder (two levels down) that is.
pub fn find_backup_root(root: &Path) -> PathBuf {
    if is_backup_folder(root) {
        return root.to_path_buf();
    }
    let mut level: Vec<PathBuf> = vec![root.to_path_buf()];
    for _ in 0..2 {
        let mut next = Vec::new();
        for dir in &level {
            let Ok(entries) = std::fs::read_dir(dir) else {
                continue;
            };
            let mut children: Vec<PathBuf> = entries
                .flatten()
                .filter(|entry| entry.file_type().is_ok_and(|kind| kind.is_dir()))
                .map(|entry| entry.path())
                .collect();
            children.sort();
            for child in children {
                if is_backup_folder(&child) {
                    return child;
                }
                next.push(child);
            }
        }
        level = next;
    }
    root.to_path_buf()
}

fn manifest_summary(manifest: &pid_parse::backup::Manifest) -> ManifestSummary {
    let field = |key: &str| manifest.first_field(key).map(str::to_string);
    ManifestSummary {
        name: field("Name"),
        version: field("Version"),
        date_created: field("DateCreated"),
        backup_type: field("BackupType"),
        spid: field("Spid"),
    }
}

/// The backup files, in the panel's order: manifest, dump, then the archives.
fn describe_files(
    backup_root: &Path,
    scan: &BackupFolderScan,
    warnings: &mut Vec<String>,
) -> Vec<BackupFile> {
    let mut files = Vec::new();
    if scan.manifest.is_some() {
        let path = backup_root.join("Manifest.txt");
        files.push(BackupFile {
            name: "Manifest.txt".to_string(),
            kind: FileKind::Manifest,
            format: "Manifest".to_string(),
            size: file_size(&path),
            members: Vec::new(),
            version_line: None,
        });
    }
    if let Some(export) = &scan.export {
        files.push(export_file(export));
    }
    for entry in &scan.refdata {
        let (format, members) = zip_parts(&entry.zip);
        files.push(BackupFile {
            name: entry.entry.file_name.clone(),
            kind: FileKind::RefData,
            format,
            size: entry.entry.size,
            members,
            version_line: None,
        });
    }
    for plant in &scan.plant_data {
        let (format, members) = zip_parts(&plant.zip);
        files.push(BackupFile {
            name: plant.file_name.clone(),
            kind: FileKind::PlantData,
            format,
            size: plant.size,
            members,
            version_line: None,
        });
    }
    for problem in &scan.problems {
        warnings.push(format!(
            "{}: {}",
            problem.path.display(),
            problem.detail
        ));
    }
    files
}

fn export_file(export: &ExportDumpScan) -> BackupFile {
    let (format, version_line) = match &export.format {
        ExportDumpFormat::OracleExp { version_line } => (
            format!("Oracle exp ({version_line})"),
            Some(version_line.clone()),
        ),
        ExportDumpFormat::SqlServerMtf(_) => ("SQL Server MTF".to_string(), None),
        ExportDumpFormat::Unrecognized { detail } => {
            (format!("Unrecognized ({detail})"), None)
        }
    };
    BackupFile {
        name: "Export.dmp".to_string(),
        kind: FileKind::ExportDump,
        format,
        size: export.size,
        members: Vec::new(),
        version_line,
    }
}

fn zip_parts(zip: &ZipListing) -> (String, Vec<String>) {
    match zip {
        ZipListing::Listed(entries) => (
            "Zip".to_string(),
            entries.iter().map(|entry| entry.name.clone()).collect(),
        ),
        ZipListing::NotAZip => (String::new(), Vec::new()),
        ZipListing::Unreadable => ("Zip (unreadable)".to_string(), Vec::new()),
    }
}

fn file_size(path: &Path) -> u64 {
    std::fs::metadata(path).map(|meta| meta.len()).unwrap_or(0)
}

/// The `T_DRAWING` rows and plant groups, when the dump can supply them.
///
/// A dump that is absent, is a SQL Server stream, or cannot be read pushes one
/// warning and returns `None`, which sends the caller to the folder listing.
fn dump_drawings(
    backup_root: &Path,
    scan: &BackupFolderScan,
    warnings: &mut Vec<String>,
) -> Option<Vec<DrawingRow>> {
    let Some(export) = &scan.export else {
        warnings.push(
            "no Export.dmp in the backup folder; drawings are listed by folder".to_string(),
        );
        return None;
    };
    match &export.format {
        ExportDumpFormat::SqlServerMtf(_) => {
            warnings.push(
                "Export.dmp is a SQL Server MTF dump; its database is not read, so drawings are listed by folder"
                    .to_string(),
            );
            return None;
        }
        ExportDumpFormat::Unrecognized { detail } => {
            warnings.push(format!(
                "Export.dmp is not a recognizable Oracle exp dump ({detail}); drawings are listed by folder"
            ));
            return None;
        }
        ExportDumpFormat::OracleExp { .. } => {}
    }
    let bytes = match std::fs::read(&export.path) {
        Ok(bytes) => bytes,
        Err(error) => {
            warnings.push(format!(
                "cannot read {}: {error}; drawings are listed by folder",
                export.path.display()
            ));
            return None;
        }
    };
    let rows = match read_backup_drawings(&bytes) {
        Ok(rows) => rows,
        Err(error) => {
            warnings.push(format!(
                "cannot read the drawing table from {}: {error:?}; drawings are listed by folder",
                backup_root.join("Export.dmp").display()
            ));
            return None;
        }
    };
    let groups = read_plant_groups(&bytes).unwrap_or_else(|error| {
        warnings.push(format!(
            "cannot read the plant groups from Export.dmp: {error:?}; group paths are left empty"
        ));
        Vec::new()
    });
    let rows = rows
        .into_iter()
        .map(|row| DrawingRow {
            group_path: plant_group_path(&groups, &row.plant_group_id),
            meta: drawing_meta(&row),
            name: row.name.clone(),
            path_key: row.path_key.clone(),
        })
        .collect();
    Some(rows)
}

/// Every `T_DRAWING` column as a label/value pair, empty values included.
pub fn drawing_meta(row: &BackupDrawing) -> Vec<(String, String)> {
    let number = |value: Option<i64>| value.map(|value| value.to_string()).unwrap_or_default();
    vec![
        ("SP_ID".to_string(), row.id.clone()),
        ("NAME".to_string(), row.name.clone()),
        ("PATH".to_string(), row.path.clone()),
        ("DRAWINGNUMBER".to_string(), row.drawing_number.clone()),
        ("TITLE".to_string(), row.title.clone()),
        ("DESCRIPTION".to_string(), row.description.clone()),
        ("REVISION".to_string(), row.revision.clone()),
        ("VERSION".to_string(), row.version.clone()),
        ("DOCUMENTTYPE".to_string(), number(row.document_type)),
        ("DOCUMENTCATEGORY".to_string(), number(row.document_category)),
        ("ITEMSTATUS".to_string(), number(row.item_status)),
        ("TEMPLATE".to_string(), row.template.clone()),
        ("DESIGNPHASE".to_string(), row.design_phase.clone()),
        ("PROJECTNUMBER".to_string(), row.project_number.clone()),
        ("DATECREATED".to_string(), row.date_created.clone()),
        ("DRAWINGDATE".to_string(), row.drawing_date.clone()),
        ("DATEOFDRAWING".to_string(), row.date_of_drawing.clone()),
        ("LASTPUBLISHTIME".to_string(), row.last_publish_time.clone()),
        ("LASTARCHIVETIME".to_string(), row.last_archive_time.clone()),
        ("SP_PLANTGROUPID".to_string(), row.plant_group_id.clone()),
    ]
}

/// Pair each row with the restored `.pid` it names.
///
/// A row matches by [`backup_path_key`] of the file's path relative to `base`
/// (the table's `PATH` is `\00\00\name.pid`, the unpacked file is
/// `00/00/name.pid`). A row whose key does not match falls back to a unique
/// file-name match, which covers a plant that stores its `PATH` with a prefix
/// the backup does not carry. A row that matches nothing keeps its metadata
/// with an empty [`DrawingEntry::pid_path`].
pub fn match_drawings(rows: &[DrawingRow], pids: &[PathBuf], base: &Path) -> MatchedDrawings {
    match_drawings_under(rows, pids, &[base])
}

/// [`match_drawings`] with several candidate bases: a `.pid` is keyed relative
/// to the first base it lies under. A backup folder source keeps its drawings
/// in the restore folder, so the backup root and the restore folder are both
/// offered.
pub fn match_drawings_under(
    rows: &[DrawingRow],
    pids: &[PathBuf],
    bases: &[&Path],
) -> MatchedDrawings {
    let mut by_key: Vec<(String, usize)> = Vec::with_capacity(pids.len());
    let mut by_name: Vec<(String, usize)> = Vec::with_capacity(pids.len());
    for (index, pid) in pids.iter().enumerate() {
        let base = bases
            .iter()
            .find(|base| pid.starts_with(base))
            .copied()
            .unwrap_or_else(|| bases.first().copied().unwrap_or(Path::new("")));
        by_key.push((backup_path_key(&relative_key(pid, base)), index));
        by_name.push((file_name_lower(pid), index));
    }
    let mut used = vec![false; pids.len()];
    let mut entries = Vec::with_capacity(rows.len());
    for row in rows {
        let mut found = None;
        if !row.path_key.is_empty() {
            found = by_key
                .iter()
                .find(|(key, index)| key == &row.path_key && !used[*index])
                .map(|(_, index)| *index);
        }
        if found.is_none() {
            let wanted = file_name_of_key(&row.path_key).to_lowercase();
            found = by_name
                .iter()
                .find(|(name, index)| !used[*index] && name == &wanted)
                .map(|(_, index)| *index);
        }
        if let Some(index) = found {
            used[index] = true;
        }
        entries.push(DrawingEntry {
            name: row.name.clone(),
            pid_path: found.map(|index| pids[index].clone()),
            group_path: row.group_path.clone(),
            meta: row.meta.clone(),
        });
    }
    let unmatched = pids
        .iter()
        .enumerate()
        .filter(|(index, _)| !used[*index])
        .map(|(_, pid)| pid.clone())
        .collect();
    MatchedDrawings { entries, unmatched }
}

/// [`match_drawings`] plus the entries the unmatched `.pid` files become: each
/// is listed under [`UNREGISTERED_GROUP`], so a drawing the table does not
/// know about still appears in the tree.
pub fn assemble_drawings(
    rows: &[DrawingRow],
    pids: &[PathBuf],
    base: &Path,
    restored: &Path,
) -> (Vec<DrawingEntry>, Vec<DrawingEntry>) {
    let matched = match_drawings_under(rows, pids, &[base, restored]);
    let unregistered = matched
        .unmatched
        .iter()
        .map(|pid| DrawingEntry {
            name: file_stem(pid),
            pid_path: Some(pid.clone()),
            group_path: UNREGISTERED_GROUP.to_string(),
            meta: vec![
                ("PATH".to_string(), relative_key(pid, base)),
                ("FILE".to_string(), pid.display().to_string()),
            ],
        })
        .collect();
    (matched.entries, unregistered)
}

/// The drawings of a backup with no usable drawing table, filed by folder.
pub fn folder_entries(pids: &[PathBuf], backup_root: &Path, restored: &Path) -> Vec<DrawingEntry> {
    pids.iter()
        .map(|pid| {
            let base = if pid.starts_with(backup_root) {
                backup_root
            } else {
                restored
            };
            let folder = folder_key(pid, base);
            DrawingEntry {
                name: file_stem(pid),
                pid_path: Some(pid.clone()),
                group_path: if folder.is_empty() {
                    UNGROUPED.to_string()
                } else {
                    folder
                },
                meta: vec![
                    ("PATH".to_string(), relative_key(pid, base)),
                    ("FILE".to_string(), pid.display().to_string()),
                ],
            }
        })
        .collect()
}

/// `path` relative to `base`, slash-folded; the whole path when it is outside.
fn relative_key(path: &Path, base: &Path) -> String {
    let relative = path.strip_prefix(base).unwrap_or(path);
    backup_path_key(&relative.to_string_lossy())
}

/// The folder part of `path` relative to `base`, slash-folded.
fn folder_key(path: &Path, base: &Path) -> String {
    let key = relative_key(path, base);
    key.rsplit_once('/')
        .map(|(folder, _)| folder.to_string())
        .unwrap_or_default()
}

fn file_name_of_key(key: &str) -> String {
    key.rsplit_once('/')
        .map(|(_, name)| name.to_string())
        .unwrap_or_else(|| key.to_string())
}

fn file_name_lower(path: &Path) -> String {
    path.file_name()
        .map(|name| name.to_string_lossy().to_lowercase())
        .unwrap_or_default()
}

fn file_stem(path: &Path) -> String {
    path.file_stem()
        .map(|stem| stem.to_string_lossy().into_owned())
        .unwrap_or_else(|| path.display().to_string())
}

/// Fold the group paths into a tree; a path's segments nest under each other.
pub fn build_group_tree(paths: &[String]) -> Vec<GroupNode> {
    let mut roots: Vec<GroupNode> = Vec::new();
    for (index, path) in paths.iter().enumerate() {
        let parts = split_group_path(path);
        if parts.is_empty() {
            insert_group(&mut roots, &[UNGROUPED.to_string()], "", index);
        } else {
            insert_group(&mut roots, &parts, "", index);
        }
    }
    roots
}

/// Split a `plant_group_path` (`SQPlant \ 00 \ 00`) or a folder key
/// (`00/00`) into its segments.
fn split_group_path(path: &str) -> Vec<String> {
    path.replace(" \\ ", "/")
        .replace('\\', "/")
        .split('/')
        .map(str::trim)
        .filter(|part| !part.is_empty())
        .map(str::to_string)
        .collect()
}

fn insert_group(nodes: &mut Vec<GroupNode>, parts: &[String], prefix: &str, index: usize) {
    let Some((head, rest)) = parts.split_first() else {
        return;
    };
    let path = if prefix.is_empty() {
        head.clone()
    } else {
        format!("{prefix} / {head}")
    };
    let at = match nodes.iter().position(|node| node.name == *head) {
        Some(at) => at,
        None => {
            nodes.push(GroupNode {
                name: head.clone(),
                path,
                children: Vec::new(),
                drawings: Vec::new(),
            });
            nodes.len() - 1
        }
    };
    if rest.is_empty() {
        nodes[at].drawings.push(index);
        return;
    }
    let child_prefix = nodes[at].path.clone();
    insert_group(&mut nodes[at].children, rest, &child_prefix, index);
}

/// The `Ref` reference-data tree under `dir`; empty when the folder is absent.
pub fn read_ref_tree(dir: &Path) -> RefNode {
    let name = dir
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| "Ref".to_string());
    let mut node = RefNode {
        name,
        dirs: Vec::new(),
        files: Vec::new(),
    };
    fill_ref_tree(dir, &mut node, REF_SEARCH_DEPTH);
    node
}

fn fill_ref_tree(dir: &Path, node: &mut RefNode, depth: usize) {
    if depth == 0 {
        return;
    }
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let Ok(kind) = entry.file_type() else {
            continue;
        };
        let name = entry.file_name().to_string_lossy().into_owned();
        if kind.is_dir() {
            let mut child = RefNode {
                name,
                dirs: Vec::new(),
                files: Vec::new(),
            };
            fill_ref_tree(&entry.path(), &mut child, depth - 1);
            node.dirs.push(child);
        } else if kind.is_file() {
            node.files.push(RefFile {
                name,
                size: entry.metadata().map(|meta| meta.len()).unwrap_or(0),
            });
        }
    }
    node.dirs.sort_by(|left, right| left.name.cmp(&right.name));
    node.files.sort_by(|left, right| left.name.cmp(&right.name));
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn scratch(name: &str) -> PathBuf {
        let root = std::env::temp_dir().join(format!(
            "ocs-pid-backup-browse-{name}-{}",
            std::process::id()
        ));
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

    /// A backup folder with a manifest, a drawing archive of two folders, and
    /// a `Ref` tree -- and no `Export.dmp`.
    fn synthetic_backup(root: &Path) -> PathBuf {
        let backup = root.join("backup");
        std::fs::create_dir_all(backup.join("Ref").join("Symbols")).unwrap();
        std::fs::write(
            backup.join("Manifest.txt"),
            "BackupType<<|>>2\nVersion<<|>>7.02\nDateCreated<<|>>07/19/2026 20:13:24\nName<<|>>SQPlant\nSpid<<|>>F8D466206C55478D9BB9482FA7769A46\n",
        )
        .unwrap();
        std::fs::write(backup.join("Ref").join("Symbols").join("valve.sym"), b"s").unwrap();
        std::fs::write(backup.join("Ref").join("readme.txt"), b"r").unwrap();
        write_zip(
            &backup.join("PlantData~2~711.zip"),
            &[
                ("00/00/A.pid", b"a"),
                ("00/00/B.pid", b"b"),
                ("01/01/C.pid", b"c"),
            ],
        );
        write_zip(
            &backup.join("RefData~4~681.zip"),
            &[("Symbols/valve.sym", b"s"), ("Symbols/pump.sym", b"p")],
        );
        backup
    }

    fn manifest_of(path: &Path) -> Vec<String> {
        let mut listed: Vec<String> = std::fs::read_dir(path)
            .unwrap()
            .flatten()
            .map(|entry| {
                let meta = entry.metadata().unwrap();
                format!(
                    "{}:{}",
                    entry.file_name().to_string_lossy(),
                    if meta.is_dir() { -1 } else { meta.len() as i64 }
                )
            })
            .collect();
        listed.sort();
        listed
    }

    #[test]
    fn a_backup_without_a_dump_lists_its_files_groups_and_ref_tree() {
        let root = scratch("nodes");
        let backup = synthetic_backup(&root);
        let index = build_index(&backup, Some(&root.join("out"))).unwrap();

        let manifest = index.manifest.as_ref().unwrap();
        assert_eq!(manifest.name.as_deref(), Some("SQPlant"));
        assert_eq!(manifest.version.as_deref(), Some("7.02"));
        assert_eq!(
            manifest.date_created.as_deref(),
            Some("07/19/2026 20:13:24")
        );
        assert_eq!(manifest.backup_type.as_deref(), Some("2"));
        assert_eq!(
            manifest.spid.as_deref(),
            Some("F8D466206C55478D9BB9482FA7769A46")
        );

        let names: Vec<&str> = index.files.iter().map(|file| file.name.as_str()).collect();
        assert_eq!(
            names,
            vec!["Manifest.txt", "RefData~4~681.zip", "PlantData~2~711.zip"]
        );
        let refdata = index
            .files
            .iter()
            .find(|file| file.kind == FileKind::RefData)
            .unwrap();
        assert_eq!(refdata.format, "Zip");
        assert_eq!(
            refdata.members,
            vec!["Symbols/valve.sym", "Symbols/pump.sym"]
        );
        let plant = index
            .files
            .iter()
            .find(|file| file.kind == FileKind::PlantData)
            .unwrap();
        assert_eq!(plant.format, "Zip");
        assert_eq!(
            plant.members,
            vec!["00/00/A.pid", "00/00/B.pid", "01/01/C.pid"]
        );
        assert!(plant.size > 0);
        let manifest_file = index
            .files
            .iter()
            .find(|file| file.kind == FileKind::Manifest)
            .unwrap();
        assert_eq!(manifest_file.size, file_size(&backup.join("Manifest.txt")));

        assert_eq!(index.drawings.len(), 3);
        assert!(index
            .drawings
            .iter()
            .all(|drawing| drawing.pid_path.is_some()));
        assert_eq!(index.ref_tree.file_count(), 2);
        assert!(index
            .ref_tree
            .dirs
            .iter()
            .any(|dir| dir.name == "Symbols" && dir.files.len() == 1));
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn a_backup_without_a_dump_groups_its_drawings_by_folder() {
        let root = scratch("degraded");
        let backup = synthetic_backup(&root);
        let index = build_index(&backup, Some(&root.join("out"))).unwrap();

        assert!(
            index
                .warnings
                .iter()
                .any(|warning| warning.contains("no Export.dmp")),
            "{:?}",
            index.warnings
        );
        let groups: Vec<&str> = index
            .plant_groups
            .iter()
            .map(|group| group.name.as_str())
            .collect();
        assert_eq!(groups, vec!["00", "01"]);
        assert_eq!(index.plant_groups[0].children[0].name, "00");
        assert_eq!(index.plant_groups[0].children[0].drawings.len(), 2);
        assert_eq!(index.plant_groups[1].children[0].drawings.len(), 1);
        let mut names: Vec<&str> = index.plant_groups[0].children[0]
            .drawings
            .iter()
            .map(|at| index.drawings[*at].name.as_str())
            .collect();
        names.sort();
        assert_eq!(names, vec!["A", "B"]);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn a_folder_source_is_left_untouched() {
        let root = scratch("readonly");
        let backup = synthetic_backup(&root);
        let before = manifest_of(&backup);
        let index = build_index(&backup, Some(&root.join("out"))).unwrap();

        assert!(index.read_only);
        assert_eq!(index.backup_root, backup);
        assert_eq!(manifest_of(&backup), before);
        assert!(!backup.join("00").exists(), "the source is never written");
        assert!(!backup.join(".ocs-restored").exists());
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn a_zip_source_finds_the_backup_root_below_the_restore_folder() {
        let root = scratch("zipped");
        let inner = root.join("PlantData~2~711.zip");
        write_zip(&inner, &[("00/00/A.pid", b"a")]);
        let inner_bytes = std::fs::read(&inner).unwrap();
        let outer = root.join("backup.zip");
        write_zip(
            &outer,
            &[
                (
                    "Backup/Manifest.txt",
                    b"Name<<|>>SQPlant\nVersion<<|>>7.02\n",
                ),
                ("Backup/PlantData~2~711.zip", &inner_bytes),
                ("Backup/Ref/Symbols/valve.sym", b"s"),
            ],
        );
        let target = root.join("out");
        let index = build_index(&outer, Some(&target)).unwrap();

        assert_eq!(index.restored, target);
        assert_eq!(index.backup_root, target.join("Backup"));
        assert!(!index.read_only);
        assert_eq!(
            index.manifest.as_ref().unwrap().name.as_deref(),
            Some("SQPlant")
        );
        assert_eq!(index.drawings.len(), 1);
        let drawing = &index.drawings[0];
        assert_eq!(drawing.name, "A");
        assert!(drawing.pid_path.as_ref().unwrap().is_file());
        assert_eq!(index.ref_tree.file_count(), 1);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn a_database_row_without_a_file_is_marked_missing() {
        let pids = vec![
            PathBuf::from("/restore/00/00/A.pid"),
            PathBuf::from("/restore/00/00/B.pid"),
        ];
        let rows = vec![
            DrawingRow {
                name: "A".to_string(),
                path_key: "00/00/A.pid".to_string(),
                group_path: "SQPlant / 00 / 00".to_string(),
                meta: vec![("SP_ID".to_string(), "1".to_string())],
            },
            DrawingRow {
                name: "Gone".to_string(),
                path_key: "00/00/Gone.pid".to_string(),
                group_path: "SQPlant / 00 / 00".to_string(),
                meta: vec![("SP_ID".to_string(), "2".to_string())],
            },
        ];
        let matched = match_drawings(&rows, &pids, Path::new("/restore"));
        assert_eq!(matched.entries.len(), 2);
        assert_eq!(
            matched.entries[0].pid_path.as_deref(),
            Some(Path::new("/restore/00/00/A.pid"))
        );
        assert!(matched.entries[1].is_missing());
        assert_eq!(matched.entries[1].meta.len(), 1);
        // B matched nothing either, so it is offered back as unregistered.
        assert_eq!(matched.unmatched, vec![PathBuf::from("/restore/00/00/B.pid")]);
    }

    #[test]
    fn a_row_falls_back_to_its_file_name() {
        let pids = vec![PathBuf::from("/restore/00/00/Drawing.pid")];
        let rows = vec![DrawingRow {
            name: "Drawing".to_string(),
            path_key: "test/U01/Drawing.pid".to_string(),
            group_path: String::new(),
            meta: Vec::new(),
        }];
        let matched = match_drawings(&rows, &pids, Path::new("/restore"));
        assert_eq!(
            matched.entries[0].pid_path.as_deref(),
            Some(Path::new("/restore/00/00/Drawing.pid"))
        );
        assert!(matched.unmatched.is_empty());
    }

    #[test]
    fn an_unregistered_drawing_is_still_listed() {
        let pids = vec![
            PathBuf::from("/restore/00/00/A.pid"),
            PathBuf::from("/restore/00/00/Loose.pid"),
        ];
        let rows = vec![DrawingRow {
            name: "A".to_string(),
            path_key: "00/00/A.pid".to_string(),
            group_path: "SQPlant \\ 00 \\ 00".to_string(),
            meta: Vec::new(),
        }];
        let (entries, unregistered) =
            assemble_drawings(&rows, &pids, Path::new("/restore"), Path::new("/restore"));
        assert_eq!(entries.len(), 1);
        assert_eq!(unregistered.len(), 1);
        assert_eq!(unregistered[0].name, "Loose");
        assert_eq!(unregistered[0].group_path, UNREGISTERED_GROUP);
        assert_eq!(
            unregistered[0].pid_path.as_deref(),
            Some(Path::new("/restore/00/00/Loose.pid"))
        );
        // The unregistered drawing is grouped under its own node.
        let tree = build_group_tree(&[UNREGISTERED_GROUP.to_string()]);
        assert_eq!(tree.len(), 1);
        assert_eq!(tree[0].drawings, vec![0]);
    }

    #[test]
    fn a_backup_lists_a_drawing_the_table_does_not_know() {
        let root = scratch("unregistered-index");
        let backup = synthetic_backup(&root);
        // A loose `.pid` in the backup folder is not in any archive, so a
        // folder source's restore leaves it where it is; a `.pid` added to
        // the restore folder is what an unregistered drawing looks like.
        let index = build_index(&backup, Some(&root.join("out"))).unwrap();
        std::fs::write(index.restored.join("00").join("00").join("Extra.pid"), b"x").unwrap();
        let index = build_index(&backup, Some(&root.join("out"))).unwrap();
        assert_eq!(index.drawings.len(), 4);
        assert!(index
            .drawings
            .iter()
            .any(|drawing| drawing.name == "Extra" && drawing.pid_path.is_some()));
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn a_folder_of_loose_drawings_is_indexed_in_place() {
        let root = scratch("loose");
        let backup = root.join("loose");
        std::fs::create_dir_all(backup.join("00").join("00")).unwrap();
        std::fs::write(backup.join("00").join("00").join("A.pid"), b"a").unwrap();
        std::fs::write(
            backup.join("Manifest.txt"),
            "Name<<|>>SQPlant\nVersion<<|>>7.02\n",
        )
        .unwrap();
        let before = manifest_of(&backup);
        let index = build_index(&backup, None).unwrap();

        assert!(index.read_only);
        assert_eq!(index.restored, backup);
        assert_eq!(index.backup_root, backup);
        assert_eq!(manifest_of(&backup), before);
        assert_eq!(index.drawings.len(), 1);
        assert_eq!(index.drawings[0].name, "A");
        assert_eq!(
            index.drawings[0].pid_path.as_deref(),
            Some(backup.join("00").join("00").join("A.pid").as_path())
        );
        assert_eq!(index.plant_groups[0].name, "00");
        assert_eq!(index.plant_groups[0].children[0].drawings, vec![0]);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn a_row_matches_a_drawing_unpacked_beside_the_backup_folder() {
        // A folder source: the backup root is the source folder, the drawings
        // land in the restore folder, and the row's PATH is relative to that.
        let pids = vec![PathBuf::from("/restore/00/00/A.pid")];
        let rows = vec![DrawingRow {
            name: "A".to_string(),
            path_key: "00/00/A.pid".to_string(),
            group_path: "SQPlant \\ 00 \\ 00".to_string(),
            meta: Vec::new(),
        }];
        let matched = match_drawings_under(
            &rows,
            &pids,
            &[Path::new("/backup"), Path::new("/restore")],
        );
        assert_eq!(
            matched.entries[0].pid_path.as_deref(),
            Some(Path::new("/restore/00/00/A.pid"))
        );
        assert!(matched.unmatched.is_empty());
    }

    #[test]
    fn group_paths_nest_under_their_parents() {
        let tree = build_group_tree(&[
            "SQPlant \\ 00 \\ 00".to_string(),
            "SQPlant \\ 00 \\ 01".to_string(),
            "SQPlant \\ 01 \\ 00".to_string(),
            String::new(),
        ]);
        assert_eq!(tree.len(), 2);
        assert_eq!(tree[0].name, "SQPlant");
        assert_eq!(tree[0].path, "SQPlant");
        assert_eq!(tree[0].children.len(), 2);
        assert_eq!(tree[0].children[0].path, "SQPlant / 00");
        assert_eq!(tree[0].children[0].children[0].path, "SQPlant / 00 / 00");
        assert_eq!(tree[0].children[0].children[0].drawings, vec![0]);
        assert_eq!(tree[0].children[0].children[1].drawings, vec![1]);
        assert_eq!(tree[1].name, UNGROUPED);
        assert_eq!(tree[1].drawings, vec![3]);
    }
}
