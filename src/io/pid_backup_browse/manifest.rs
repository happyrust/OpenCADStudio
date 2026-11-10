//! Everything `Manifest.txt` says, ready to show -- and nothing it should not.
//!
//! The manifest is the backup's own account of itself: some thirty kinds of
//! line, of which the panel used to show five fields. This module sorts all
//! of them into what a reader looks for: the one-off facts about the backup,
//! the database objects it exported, the files it packed (with where each
//! came from, which is the only place a `RefData~4~680` is called
//! `rules.rul`), the database files of a SQL Server plant, and how the plant
//! connected to its databases.
//!
//! That last part carries credentials: obfuscated passwords in the
//! connection lines, the table-space lines and `DBPwds`, and a clear-text
//! `user/password@host` in the export command. They are replaced by
//! [`HIDDEN`] here, while the manifest is read, so that no later stage --
//! the index, the panel, a log line, a `Debug` print -- can show what it
//! never received. [`ManifestModel::hidden`] says how many fields that was.

use pid_parse::backup::Manifest;

/// What stands where a credential was.
pub const HIDDEN: &str = "•••";

/// The schema number of reference data in a `File` line (`RefData~4~…`).
const REFDATA_SCHEMA: &str = "4";

/// `Manifest.txt`, sorted for the panel.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ManifestModel {
    /// The one-off facts, in the manifest's order: every line that is not
    /// one of the kinds below, as its key and its fields joined with ` | `.
    pub summary: Vec<(String, String)>,
    /// `Table` lines.
    pub tables: Vec<DbObject>,
    /// `View` lines.
    pub views: Vec<DbObject>,
    /// `Role` lines, each as its fields.
    pub roles: Vec<Vec<String>>,
    /// `Right` lines, each as its fields.
    pub rights: Vec<Vec<String>>,
    /// `DatabaseFiles` lines (SQL Server plants), each as its fields.
    pub database_files: Vec<Vec<String>>,
    /// `File` lines, each with the `FileSize` line of the same number.
    pub files: Vec<FileRecord>,
    /// `SiteConnInfo`, `PlantConnInfo`, `TableSpace`, `DBUids` and `DBPwds`
    /// lines: the key, then the fields, credentials replaced by [`HIDDEN`].
    pub connections: Vec<(String, Vec<String>)>,
    /// How many fields were replaced by [`HIDDEN`], in all of the above.
    pub hidden: usize,
}

/// A table or a view the backup exported.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DbObject {
    /// The schema (database user) it belongs to, e.g. `SQPlantpid`.
    pub schema: String,
    /// Its name, e.g. `T_DRAWING`.
    pub name: String,
}

/// What a `File` line packed: a folder (as a zip) or a single file.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FileRecordKind {
    /// A folder, stored as a zip of its content.
    Folder,
    /// One file, stored as it was under another name.
    SingleFile,
    /// A kind this module has not met.
    Other,
}

/// One `File` line: something the backup packed, or tried to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileRecord {
    /// The schema number: `2` plant data, `4` reference data.
    pub schema: String,
    /// The number in the stored name (`RefData~4~<id>`).
    pub id: String,
    /// Folder or single file.
    pub kind: FileRecordKind,
    /// Where it was on the plant's server, as the manifest states it.
    pub source: String,
    /// `source` from the folder all the files of its schema share, e.g.
    /// `Ref\Symbols\Assemblies`; empty when the manifest states no source.
    pub original: String,
    /// Whether the backup got it.
    pub backed_up: bool,
    /// The name it is stored under; empty for a folder the backup lists
    /// without packing. For a failed line, the reason instead.
    pub stored: String,
    /// From the `FileSize` line: files and folders in it, and its bytes
    /// before packing.
    pub file_count: Option<u64>,
    pub folder_count: Option<u64>,
    pub bytes: Option<u64>,
}

impl FileRecord {
    /// The name the file has in the backup folder, when it was stored.
    pub fn stored_name(&self) -> Option<&str> {
        (self.backed_up && !self.stored.is_empty()).then_some(self.stored.as_str())
    }

    /// The name the backup would have stored it under.
    pub fn expected_name(&self) -> String {
        let family = if self.schema == REFDATA_SCHEMA {
            "RefData"
        } else {
            "PlantData"
        };
        format!("{family}~{}~{}", self.schema, self.id)
    }
}

/// Whether `field` is an obfuscated value: the plant writes those as a run of
/// `#` among other characters.
fn is_obfuscated(field: &str) -> bool {
    field.contains("###")
}

/// The fields of a line of `key` that hold a credential by their position,
/// whatever they look like.
fn credential_positions(key: &str) -> &'static [usize] {
    match key {
        "SiteConnInfo" => &[0, 4],
        "PlantConnInfo" => &[4],
        "TableSpace" => &[1],
        _ => &[],
    }
}

/// `text` with every `user/password@host` word reduced to
/// `user/•••@host`. Returns whether anything was hidden.
fn hide_logins(text: &str) -> (String, bool) {
    let mut hidden = false;
    let words: Vec<String> = text
        .split(' ')
        .map(|word| {
            let (Some(slash), Some(at)) = (word.find('/'), word.rfind('@')) else {
                return word.to_string();
            };
            // Something before the slash, between the two and after the at.
            if slash == 0 || at <= slash + 1 || at + 1 >= word.len() {
                return word.to_string();
            }
            hidden = true;
            format!("{}/{HIDDEN}{}", &word[..slash], &word[at..])
        })
        .collect();
    (words.join(" "), hidden)
}

/// One field as it may be shown, counting what was hidden.
pub(super) fn shown(key: &str, at: usize, field: &str, hidden: &mut usize) -> String {
    if key == "DBPwds" || credential_positions(key).contains(&at) || is_obfuscated(field) {
        // An empty field hides nothing.
        if field.is_empty() {
            return String::new();
        }
        *hidden += 1;
        return HIDDEN.to_string();
    }
    let (text, masked) = hide_logins(field);
    if masked {
        *hidden += 1;
    }
    text
}

/// Sort a parsed manifest into [`ManifestModel`], hiding its credentials.
pub fn manifest_model(manifest: &Manifest) -> ManifestModel {
    let mut model = ManifestModel::default();
    // `FileSize` lines by `(schema, id)`.
    let sizes: Vec<(&str, &str, [Option<u64>; 3])> = manifest
        .all("FileSize")
        .filter_map(|line| {
            let number = |at: usize| line.fields.get(at).and_then(|field| field.trim().parse().ok());
            Some((
                line.fields.first()?.as_str(),
                line.fields.get(1)?.as_str(),
                [number(2), number(3), number(4)],
            ))
        })
        .collect();
    for line in &manifest.lines {
        let mut hidden = 0;
        let fields: Vec<String> = line
            .fields
            .iter()
            .enumerate()
            .map(|(at, field)| shown(&line.key, at, field, &mut hidden))
            .collect();
        model.hidden += hidden;
        let object = |fields: &[String]| -> Option<DbObject> {
            Some(DbObject {
                schema: fields.first()?.clone(),
                name: fields.get(1)?.clone(),
            })
        };
        match line.key.as_str() {
            "Table" => model.tables.extend(object(&fields)),
            "View" => model.views.extend(object(&fields)),
            "Role" => model.roles.push(fields),
            "Right" => model.rights.push(fields),
            "DatabaseFiles" => model.database_files.push(fields),
            "SiteConnInfo" | "PlantConnInfo" | "TableSpace" | "DBUids" | "DBPwds" => {
                model.connections.push((line.key.clone(), fields));
            }
            // Joined to their `File` line above.
            "FileSize" => {}
            "File" => {
                let field = |at: usize| fields.get(at).cloned().unwrap_or_default();
                let (schema, id) = (field(0), field(2));
                let size = sizes
                    .iter()
                    .find(|(size_schema, size_id, _)| *size_schema == schema && *size_id == id)
                    .map(|(_, _, numbers)| *numbers)
                    .unwrap_or([None; 3]);
                model.files.push(FileRecord {
                    kind: match field(1).as_str() {
                        "1" => FileRecordKind::Folder,
                        "2" => FileRecordKind::SingleFile,
                        _ => FileRecordKind::Other,
                    },
                    source: field(4),
                    original: String::new(),
                    backed_up: field(5) == "1",
                    stored: field(6),
                    file_count: size[0],
                    folder_count: size[1],
                    bytes: size[2],
                    schema,
                    id,
                });
            }
            _ => model.summary.push((line.key.clone(), fields.join(" | "))),
        }
    }
    name_originals(&mut model.files);
    model
}

/// The components of a path the manifest states: `\\host\share\a\b`.
fn components(path: &str) -> Vec<&str> {
    path.split(['\\', '/']).filter(|part| !part.is_empty()).collect()
}

/// Give every record its [`FileRecord::original`]: its source from the
/// folder the stored entries of its schema share, that folder's own name
/// included. A packed folder counts as itself and a single file as the
/// folder it lies in, so a lone `...\Ref\rules.rul` is still `Ref\rules.rul`.
/// Lines the backup did not store (a folder listed without a zip, a failed
/// file) are named the same way but do not decide the folder.
fn name_originals(files: &mut [FileRecord]) {
    let mut schemas: Vec<String> = files.iter().map(|file| file.schema.clone()).collect();
    schemas.sort();
    schemas.dedup();
    for schema in schemas {
        // The longest run of components every stored source starts with.
        let mut shared: Option<Vec<String>> = None;
        for file in files.iter().filter(|file| file.schema == schema) {
            if file.stored_name().is_none() || file.source.is_empty() {
                continue;
            }
            let mut parts = components(&file.source);
            if file.kind == FileRecordKind::SingleFile {
                parts.pop();
            }
            shared = Some(match shared {
                None => parts.iter().map(|part| part.to_string()).collect(),
                Some(shared) => shared
                    .iter()
                    .zip(&parts)
                    .take_while(|(left, right)| left.eq_ignore_ascii_case(right))
                    .map(|(left, _)| left.clone())
                    .collect(),
            });
        }
        let shared = shared.unwrap_or_default();
        for file in files.iter_mut().filter(|file| file.schema == schema) {
            let parts = components(&file.source);
            if parts.is_empty() {
                continue;
            }
            let under = shared.len() <= parts.len()
                && shared
                    .iter()
                    .zip(&parts)
                    .all(|(left, right)| left.eq_ignore_ascii_case(right));
            // From the shared folder's own name on; a source somewhere else
            // (the plant's backup folder) keeps its last two components.
            let from = if under && !shared.is_empty() {
                shared.len() - 1
            } else {
                parts.len().saturating_sub(2)
            };
            file.original = parts[from..].join("\\");
        }
    }
}

/// The extension of a single file's original name, as a format label:
/// `rules.rul` is `RUL`.
pub fn format_of(original: &str) -> Option<String> {
    let name = original.rsplit(['\\', '/']).next()?;
    let (_, extension) = name.rsplit_once('.')?;
    (!extension.is_empty() && extension.len() <= 8).then(|| extension.to_uppercase())
}

#[cfg(test)]
mod tests {
    use super::*;
    use pid_parse::backup::parse_manifest;

    /// What a credential looks like in a manifest, made up for the tests: an
    /// obfuscated field, and a login in the export command.
    const OBFUSCATED: &str = "Zx#####made-up-obfuscated-value#####";
    const PASSWORD: &str = "made-up-password";

    fn sample() -> String {
        [
            "Name<<|>>SQPlant".to_string(),
            "Version<<|>>7.02".to_string(),
            "Rootitem<<|>>F8D4<<|>>SQPlant<<|>><<|>>\\\\host\\Project\\SQPlant\\Plant<<|>>Plant".to_string(),
            format!("SiteConnInfo<<|>>{OBFUSCATED}<<|>>1<<|>>ORCL<<|>>SQSite<<|>>{OBFUSCATED}<<|>>system<<|>><<|>>2"),
            // A password that does not look obfuscated is hidden all the same.
            "PlantConnInfo<<|>>SQPlant<<|>>2<<|>>ORCL<<|>>SQPlant<<|>>plain-secret<<|>>system<<|>><<|>>2".to_string(),
            "Role<<|>>E54C<<|>>0<<|>>host\\Administrators<<|>><<|>>".to_string(),
            "Right<<|>>E54C<<|>>0<<|>>3<<|>>100".to_string(),
            "Right<<|>>E54C<<|>>0<<|>>3<<|>>301".to_string(),
            format!("TableSpace<<|>>Permanent<<|>>{OBFUSCATED}"),
            "DBUids<<|>>SQPlantpid,SQPlant".to_string(),
            format!("DBPwds<<|>>{OBFUSCATED}"),
            "Table<<|>>SQPlantpid<<|>>T_DRAWING".to_string(),
            "Table<<|>>SQPlant<<|>>T_PLANTGROUP".to_string(),
            "View<<|>>SQPlantpid<<|>>V_NOZZLELIST".to_string(),
            format!("BackupCommand<<|>>Exp.exe system/{PASSWORD}@ORCL OWNER=(SQPlant) FILE='\\\\host\\Backup\\Export.dmp'"),
            "File<<|>>2<<|>>1<<|>>711<<|>>13087<<|>>\\\\host\\Project\\SQPlant\\Plant<<|>>1<<|>>PlantData~2~711.zip".to_string(),
            "FileSize<<|>>2<<|>>711<<|>>54<<|>>6<<|>>101929575".to_string(),
            "File<<|>>2<<|>>2<<|>>722<<|>>13296<<|>>\\\\host\\Project\\SQPlant\\Plant\\Resources\\Map.xml<<|>>1<<|>>PlantData~2~722".to_string(),
            "File<<|>>2<<|>>1<<|>>714<<|>>13089<<|>>\\\\host\\Project\\Backup<<|>>1<<|>>".to_string(),
            "File<<|>>4<<|>>1<<|>>681<<|>>20681<<|>>\\\\HOST\\Project\\SQPlant\\Ref\\Symbols<<|>>1<<|>>RefData~4~681.zip".to_string(),
            "FileSize<<|>>4<<|>>681<<|>>645<<|>>97<<|>>34062796".to_string(),
            "File<<|>>4<<|>>1<<|>>685<<|>>20685<<|>>\\\\host\\Project\\SQPlant\\Ref\\Symbols\\Assemblies<<|>>1<<|>>RefData~4~685.zip".to_string(),
            "File<<|>>4<<|>>2<<|>>680<<|>>20680<<|>>\\\\host\\Project\\SQPlant\\Ref\\rules.rul<<|>>1<<|>>RefData~4~680".to_string(),
            "File<<|>>4<<|>>2<<|>>824<<|>>20824<<|>>\\\\host\\Project\\SQPlant\\Ref\\Jacket.XML<<|>>2<<|>>Failed to access file.".to_string(),
        ]
        .join("\r\n")
    }

    fn model() -> ManifestModel {
        manifest_model(&parse_manifest(&sample()))
    }

    #[test]
    fn every_line_lands_in_its_place() {
        let model = model();
        assert_eq!(
            model.summary.iter().map(|(key, _)| key.as_str()).collect::<Vec<_>>(),
            ["Name", "Version", "Rootitem", "BackupCommand"]
        );
        // A line of several fields reads as one, in order, empty ones kept.
        assert_eq!(
            model.summary[2].1,
            "F8D4 | SQPlant |  | \\\\host\\Project\\SQPlant\\Plant | Plant"
        );
        assert_eq!(
            model.tables,
            [
                DbObject { schema: "SQPlantpid".to_string(), name: "T_DRAWING".to_string() },
                DbObject { schema: "SQPlant".to_string(), name: "T_PLANTGROUP".to_string() },
            ]
        );
        assert_eq!(model.views.len(), 1);
        assert_eq!(model.roles.len(), 1);
        assert_eq!(model.rights, [["E54C", "0", "3", "100"], ["E54C", "0", "3", "301"]]);
        assert_eq!(
            model.connections.iter().map(|(key, _)| key.as_str()).collect::<Vec<_>>(),
            ["SiteConnInfo", "PlantConnInfo", "TableSpace", "DBUids", "DBPwds"]
        );
        assert_eq!(model.files.len(), 7);
        assert!(model.database_files.is_empty());
    }

    #[test]
    fn credentials_are_hidden_where_the_manifest_is_read() {
        let model = model();
        // Two in the site line, one in the plant line (by position: it does
        // not look obfuscated), the table space, DBPwds, and the login in
        // the export command.
        assert_eq!(model.hidden, 6);
        let printed = format!("{model:?}");
        for secret in [OBFUSCATED, PASSWORD, "plain-secret", "#####"] {
            assert!(!printed.contains(secret), "the model still holds a credential");
        }
        // What is around a credential stays readable.
        assert_eq!(
            model.connections[0].1,
            [HIDDEN, "1", "ORCL", "SQSite", HIDDEN, "system", "", "2"]
        );
        assert_eq!(model.connections[3].1, ["SQPlantpid,SQPlant"]);
        assert_eq!(model.connections[4].1, [HIDDEN]);
        let command = &model.summary[3].1;
        assert!(command.starts_with(&format!("Exp.exe system/{HIDDEN}@ORCL OWNER=(SQPlant)")));
        assert!(command.contains("Export.dmp"));
    }

    #[test]
    fn a_login_is_recognised_and_other_words_are_left_alone() {
        assert_eq!(
            hide_logins("exp scott/tiger@db FULL=y"),
            (format!("exp scott/{HIDDEN}@db FULL=y"), true)
        );
        for plain in [
            "BACKUP DATABASE Plant TO Disk",
            "FILE='\\\\host\\share\\Export.dmp'",
            "a/b",
            "user@host",
            "/x@y",
            "x/@y",
            "x/y@",
        ] {
            assert_eq!(hide_logins(plain), (plain.to_string(), false), "{plain}");
        }
    }

    #[test]
    fn a_file_line_is_joined_to_its_size_and_named_from_the_shared_folder() {
        let model = model();
        let by_id = |id: &str| model.files.iter().find(|file| file.id == id).unwrap();

        let symbols = by_id("681");
        assert_eq!(symbols.kind, FileRecordKind::Folder);
        assert_eq!(symbols.stored_name(), Some("RefData~4~681.zip"));
        assert_eq!(
            (symbols.file_count, symbols.folder_count, symbols.bytes),
            (Some(645), Some(97), Some(34_062_796))
        );
        // The reference files share `...\Ref`, whatever the case of the host.
        assert_eq!(symbols.original, "Ref\\Symbols");
        assert_eq!(by_id("685").original, "Ref\\Symbols\\Assemblies");
        let rules = by_id("680");
        assert_eq!(rules.kind, FileRecordKind::SingleFile);
        assert_eq!(rules.original, "Ref\\rules.rul");
        assert_eq!(rules.bytes, None);

        // A file the backup failed to read: named like the others, with the
        // reason where the stored name would be.
        let failed = by_id("824");
        assert!(!failed.backed_up);
        assert_eq!(failed.stored_name(), None);
        assert_eq!(failed.stored, "Failed to access file.");
        assert_eq!(failed.original, "Ref\\Jacket.XML");
        assert_eq!(failed.expected_name(), "RefData~4~824");

        // The plant data: the folder that was packed is the shared one, so
        // it goes by its own name and the file inside it by its path below.
        assert_eq!(by_id("711").original, "Plant");
        assert_eq!(by_id("722").original, "Plant\\Resources\\Map.xml");
        // A folder listed without a zip, somewhere else on the server.
        let listed = by_id("714");
        assert_eq!(listed.stored_name(), None);
        assert_eq!(listed.original, "Project\\Backup");
        assert_eq!(listed.expected_name(), "PlantData~2~714");
    }

    #[test]
    fn a_single_files_format_is_its_original_extension() {
        assert_eq!(format_of("Ref\\rules.rul").as_deref(), Some("RUL"));
        assert_eq!(format_of("Ref\\exportlayer.xlsx").as_deref(), Some("XLSX"));
        assert_eq!(format_of("Plant\\Resources\\Map.xml").as_deref(), Some("XML"));
        assert_eq!(format_of("Ref\\Symbols"), None);
        assert_eq!(format_of("Ref\\odd.name-that-is-no-extension"), None);
        assert_eq!(format_of(""), None);
    }

    #[test]
    fn an_empty_manifest_is_an_empty_model() {
        assert_eq!(manifest_model(&parse_manifest("")), ManifestModel::default());
    }
}
