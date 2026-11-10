//! `Manifest.txt`, unfolded: everything the backup says about itself.
//!
//! The file's row in "backup files" opens into four parts:
//!
//! ```text
//! Manifest.txt
//!   Summary              the one-off facts: name, version, dates, the
//!                        export command, ...
//!   Database objects     what the backup exported
//!     Database tables (154)
//!     Database views (35)
//!     Database roles (1)
//!     Database rights (78)
//!     Database files (2)   SQL Server plants only
//!   File register (14)   every file and folder the backup packed, with
//!                        where it came from and how big it was
//!   Connections          how the plant reached its databases
//! ```
//!
//! The content is [`ManifestModel`], read once when the backup is indexed.
//! The credentials a manifest carries never reach it (see
//! [`crate::io::pid_backup_browse::manifest`]): what the panel shows in
//! their place is `•••`, and how many fields that is.

use std::sync::Arc;

use super::grid::{Column, Table};
use super::section::{Cx, Section};
use super::{human_size, DetailLine, NodeId, Row};
use crate::io::pid_backup_browse::manifest::{FileRecordKind, ManifestModel, HIDDEN};
use crate::io::pid_backup_browse::{BackupIndex, FileKind};
use crate::{t, tf};

/// The manifest's branch of the tree.
pub(super) struct ManifestSection;

// The first level under `Manifest.txt`, and the second under the objects.
const SUMMARY: usize = 0;
const OBJECTS: usize = 1;
const FILES: usize = 2;
const CONNECTIONS: usize = 3;
const TABLES: usize = 0;
const VIEWS: usize = 1;
const ROLES: usize = 2;
const RIGHTS: usize = 3;
const DATABASE_FILES: usize = 4;

/// The model, when `host` is the manifest's own row.
fn model_under<'a>(index: &'a BackupIndex, host: &NodeId) -> Option<&'a ManifestModel> {
    let NodeId::File(at) = host else {
        return None;
    };
    let file = index.files.get(*at)?;
    (file.kind == FileKind::Manifest)
        .then_some(index.manifest_model.as_ref())
        .flatten()
}

fn counted(label: impl std::fmt::Display, count: usize) -> String {
    format!("{label} ({count})")
}

/// A table of lines whose fields the manifest does not name: a column per
/// field, numbered. `first` titles a leading column of its own.
fn numbered<'a>(
    first: Option<String>,
    rows: impl Iterator<Item = (Option<&'a str>, &'a [String])>,
) -> DetailLine {
    let rows: Vec<Vec<String>> = rows
        .map(|(lead, fields)| {
            lead.map(str::to_string)
                .into_iter()
                .chain(fields.iter().cloned())
                .collect()
        })
        .collect();
    let lead = usize::from(first.is_some());
    let fields = rows.iter().map(Vec::len).max().unwrap_or(lead) - lead;
    let columns = first
        .into_iter()
        .map(Column::text)
        .chain((1..=fields).map(|at| Column::text(tf!("Field {}", at))))
        .collect();
    DetailLine::Table(Arc::new(Table::new(columns, rows)))
}

/// The note under a part that shows `•••` somewhere.
fn hidden_note(count: usize, lines: &mut Vec<DetailLine>) {
    if count > 0 {
        lines.push(DetailLine::Marker(
            tf!(
                "{} credential field(s) are hidden and shown as {}",
                count,
                HIDDEN
            )
            .into_owned(),
        ));
    }
}

impl Section for ManifestSection {
    fn hosts(&self, cx: &Cx<'_>, host: &NodeId) -> bool {
        model_under(cx.index, host).is_some()
    }

    fn rows(&self, cx: &Cx<'_>, host: &NodeId, depth: usize, rows: &mut Vec<Row>) {
        let Some(model) = model_under(cx.index, host) else {
            return;
        };
        let mut push = |path: &[usize], depth: usize, label: String, expandable: bool| -> bool {
            let id = cx.node(path);
            let expanded = expandable && cx.is_open(&id);
            rows.push(Row {
                id,
                depth,
                label,
                expandable,
                expanded,
                disabled: false,
            });
            expanded
        };
        push(&[SUMMARY], depth, t!("Summary").into_owned(), false);
        if push(&[OBJECTS], depth, t!("Database objects").into_owned(), true) {
            let parts = [
                (TABLES, t!("Database tables"), model.tables.len()),
                (VIEWS, t!("Database views"), model.views.len()),
                (ROLES, t!("Database roles"), model.roles.len()),
                (RIGHTS, t!("Database rights"), model.rights.len()),
                (
                    DATABASE_FILES,
                    t!("Database files"),
                    model.database_files.len(),
                ),
            ];
            for (part, label, count) in parts {
                // An Oracle plant has no database files to list.
                if part == DATABASE_FILES && count == 0 {
                    continue;
                }
                push(&[OBJECTS, part], depth + 1, counted(label, count), false);
            }
        }
        push(
            &[FILES],
            depth,
            counted(t!("File register"), model.files.len()),
            false,
        );
        push(&[CONNECTIONS], depth, t!("Connections").into_owned(), false);
    }

    fn details(&self, cx: &Cx<'_>, id: &NodeId, lines: &mut Vec<DetailLine>) {
        let (Some(path), Some(model)) = (cx.own(id), cx.index.manifest_model.as_ref()) else {
            return;
        };
        let count = |label: std::borrow::Cow<'static, str>, count: usize| {
            DetailLine::Field(label.into_owned(), count.to_string())
        };
        let objects =
            |title: std::borrow::Cow<'static, str>,
             objects: &[crate::io::pid_backup_browse::manifest::DbObject]| {
                DetailLine::Table(Arc::new(Table::new(
                    vec![Column::text(t!("Schema")), Column::text(title)],
                    objects
                        .iter()
                        .map(|object| vec![object.schema.clone(), object.name.clone()])
                        .collect(),
                )))
            };
        match path {
            [SUMMARY] => {
                for (key, value) in &model.summary {
                    lines.push(DetailLine::Field(key.clone(), value.clone()));
                }
                hidden_note(
                    model
                        .summary
                        .iter()
                        .filter(|(_, value)| value.contains(HIDDEN))
                        .count(),
                    lines,
                );
            }
            [OBJECTS] => {
                lines.push(count(t!("Database tables"), model.tables.len()));
                lines.push(count(t!("Database views"), model.views.len()));
                lines.push(count(t!("Database roles"), model.roles.len()));
                lines.push(count(t!("Database rights"), model.rights.len()));
                if !model.database_files.is_empty() {
                    lines.push(count(t!("Database files"), model.database_files.len()));
                }
            }
            [OBJECTS, TABLES] => lines.push(objects(t!("Table name"), &model.tables)),
            [OBJECTS, VIEWS] => lines.push(objects(t!("View name"), &model.views)),
            [OBJECTS, ROLES] => lines.push(numbered(
                None,
                model.roles.iter().map(|fields| (None, fields.as_slice())),
            )),
            [OBJECTS, RIGHTS] => lines.push(numbered(
                None,
                model.rights.iter().map(|fields| (None, fields.as_slice())),
            )),
            [OBJECTS, DATABASE_FILES] => lines.push(numbered(
                None,
                model
                    .database_files
                    .iter()
                    .map(|fields| (None, fields.as_slice())),
            )),
            [FILES] => {
                let number =
                    |value: Option<u64>| value.map(|value| value.to_string()).unwrap_or_default();
                lines.push(DetailLine::Table(Arc::new(Table::new(
                    vec![
                        Column::text(t!("Stored as")),
                        Column::text(t!("Original path")),
                        Column::text(t!("Entry kind")),
                        Column::text(t!("Backup status")),
                        Column::number(t!("Files in the source")),
                        Column::number(t!("Folders in the source")),
                        Column::number(t!("Bytes before packing")),
                        Column::text(t!("Source path")),
                    ],
                    model
                        .files
                        .iter()
                        .map(|file| {
                            vec![
                                file.stored_name().unwrap_or_default().to_string(),
                                file.original.clone(),
                                match file.kind {
                                    FileRecordKind::Folder => t!("Folder").into_owned(),
                                    FileRecordKind::SingleFile => t!("Single file").into_owned(),
                                    FileRecordKind::Other => String::new(),
                                },
                                // A line that failed says why, in the
                                // plant's own words.
                                if file.backed_up {
                                    t!("Backed up").into_owned()
                                } else {
                                    file.stored.clone()
                                },
                                number(file.file_count),
                                number(file.folder_count),
                                number(file.bytes),
                                file.source.clone(),
                            ]
                        })
                        .collect(),
                ))));
            }
            [CONNECTIONS] => {
                hidden_note(
                    model
                        .connections
                        .iter()
                        .flat_map(|(_, fields)| fields)
                        .filter(|field| *field == HIDDEN)
                        .count(),
                    lines,
                );
                lines.push(numbered(
                    Some(t!("Manifest key").into_owned()),
                    model
                        .connections
                        .iter()
                        .map(|(key, fields)| (Some(key.as_str()), fields.as_slice())),
                ));
            }
            _ => {}
        }
    }
}

/// What the manifest adds to the details of a backup file: where it came
/// from, and what it held before it was packed.
pub(super) fn file_record_lines(index: &BackupIndex, file: usize, lines: &mut Vec<DetailLine>) {
    let Some(record) = index.files.get(file).and_then(|file| file.record.as_ref()) else {
        return;
    };
    let mut field = |label: std::borrow::Cow<'static, str>, value: String| {
        if !value.is_empty() {
            lines.push(DetailLine::Field(label.into_owned(), value));
        }
    };
    field(t!("Original path"), record.original.clone());
    field(t!("Source path"), record.source.clone());
    let number = |value: Option<u64>| value.map(|value| value.to_string()).unwrap_or_default();
    // A single file is one file: only a folder's counts say anything.
    if record.kind == FileRecordKind::Folder {
        field(t!("Files in the source"), number(record.file_count));
        field(t!("Folders in the source"), number(record.folder_count));
    }
    field(
        t!("Size before packing"),
        record.bytes.map(human_size).unwrap_or_default(),
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::Message;
    use crate::io::pid_backup_browse::manifest::manifest_model;
    use crate::io::pid_backup_browse::BackupFile;
    use crate::ui::dock::Chrome;
    use crate::ui::window::pid_backup_browser::{
        detail_rows, update, view, visible_rows, PidBackupBrowser, PidBackupMsg,
    };

    /// Made up for the tests; what they stand for must never be drawn.
    const OBFUSCATED: &str = "Qr#####made-up-obfuscated-value#####";
    const PASSWORD: &str = "made-up-password";

    fn manifest_text(sql_server: bool) -> String {
        let mut lines = vec![
            "Name<<|>>SQPlant".to_string(),
            "Version<<|>>7.02".to_string(),
            format!("BackupCommand<<|>>Exp.exe system/{PASSWORD}@ORCL OWNER=(SQPlant)"),
            format!("SiteConnInfo<<|>>{OBFUSCATED}<<|>>1<<|>>ORCL<<|>>SQSite<<|>>{OBFUSCATED}<<|>>system"),
            format!("PlantConnInfo<<|>>SQPlant<<|>>2<<|>>ORCL<<|>>SQPlant<<|>>{OBFUSCATED}<<|>>system"),
            "DBUids<<|>>SQPlantpid,SQPlant".to_string(),
            format!("DBPwds<<|>>{OBFUSCATED}"),
            "Role<<|>>E54C<<|>>0<<|>>host\\Administrators".to_string(),
            "Right<<|>>E54C<<|>>0<<|>>3<<|>>100".to_string(),
            "Right<<|>>E54C<<|>>0<<|>>3<<|>>301".to_string(),
            "Table<<|>>SQPlantpid<<|>>T_DRAWING".to_string(),
            "Table<<|>>SQPlantpid<<|>>T_MODELITEM".to_string(),
            "Table<<|>>SQPlant<<|>>T_PLANTGROUP".to_string(),
            "View<<|>>SQPlantpid<<|>>V_NOZZLELIST".to_string(),
            "File<<|>>2<<|>>1<<|>>711<<|>>1<<|>>\\\\host\\P\\SQPlant\\Plant<<|>>1<<|>>PlantData~2~711.zip".to_string(),
            "FileSize<<|>>2<<|>>711<<|>>54<<|>>6<<|>>101929575".to_string(),
            "File<<|>>4<<|>>2<<|>>680<<|>>1<<|>>\\\\host\\P\\SQPlant\\Ref\\rules.rul<<|>>1<<|>>RefData~4~680".to_string(),
            "FileSize<<|>>4<<|>>680<<|>>0<<|>>0<<|>>331977".to_string(),
            "File<<|>>4<<|>>2<<|>>824<<|>>1<<|>>\\\\host\\P\\SQPlant\\Ref\\Jacket.XML<<|>>2<<|>>Failed to access file.".to_string(),
        ];
        if sql_server {
            lines.push(
                "DatabaseFiles<<|>>Plant_dat<<|>>1<<|>>C:\\Data\\Plant.mdf<<|>>PRIMARY".to_string(),
            );
        }
        lines.join("\n")
    }

    /// A ready panel whose backup has a manifest, the drawing archive and
    /// two reference files, one of which the backup failed to read.
    fn ready(sql_server: bool) -> PidBackupBrowser {
        let model = manifest_model(&pid_parse::backup::parse_manifest(&manifest_text(
            sql_server,
        )));
        let file = |name: &str, kind: FileKind, format: &str, id: &str| BackupFile {
            name: name.to_string(),
            kind,
            format: format.to_string(),
            size: 10,
            record: model.files.iter().find(|record| record.id == id).cloned(),
            ..BackupFile::default()
        };
        let index = BackupIndex {
            files: vec![
                file("Manifest.txt", FileKind::Manifest, "Manifest", ""),
                file("PlantData~2~711.zip", FileKind::PlantData, "Zip", "711"),
                file("RefData~4~680", FileKind::RefData, "RUL", "680"),
            ],
            manifest_model: Some(model),
            ..BackupIndex::default()
        };
        let mut state = PidBackupBrowser::default();
        update(&mut state, PidBackupMsg::Loaded(Ok(Arc::new(index))));
        update(&mut state, PidBackupMsg::Toggle(NodeId::Files));
        state
    }

    /// The manifest section's slot: the first one registered.
    fn node(path: &[usize]) -> NodeId {
        NodeId::Section(0, path.to_vec())
    }

    fn labels(state: &PidBackupBrowser) -> Vec<(usize, String)> {
        visible_rows(state)
            .into_iter()
            .map(|row| (row.depth, row.label))
            .collect()
    }

    #[test]
    fn the_manifest_row_opens_into_its_four_parts() {
        let mut state = ready(false);
        let manifest = visible_rows(&state)
            .into_iter()
            .find(|row| row.id == NodeId::File(0))
            .expect("the manifest's row");
        assert!(manifest.expandable && !manifest.expanded);

        update(&mut state, PidBackupMsg::Toggle(NodeId::File(0)));
        assert_eq!(
            labels(&state)[2..8],
            [
                (2, "Manifest.txt".to_string()),
                (3, t!("Summary").into_owned()),
                (3, t!("Database objects").into_owned()),
                (3, format!("{} (3)", t!("File register"))),
                (3, t!("Connections").into_owned()),
                (2, format!("PlantData~2~711.zip — Plant")),
            ]
        );
        update(&mut state, PidBackupMsg::Toggle(node(&[OBJECTS])));
        assert_eq!(
            labels(&state)[5..9],
            [
                (4, format!("{} (3)", t!("Database tables"))),
                (4, format!("{} (1)", t!("Database views"))),
                (4, format!("{} (1)", t!("Database roles"))),
                (4, format!("{} (2)", t!("Database rights"))),
            ],
            "an Oracle plant lists no database files"
        );
        // A SQL Server plant does.
        let mut state = ready(true);
        update(&mut state, PidBackupMsg::Toggle(NodeId::File(0)));
        update(&mut state, PidBackupMsg::Toggle(node(&[OBJECTS])));
        assert_eq!(
            labels(&state)[9],
            (4, format!("{} (1)", t!("Database files")))
        );
    }

    #[test]
    fn a_backup_without_a_manifest_model_leaves_the_row_a_leaf() {
        let mut state = PidBackupBrowser::default();
        update(
            &mut state,
            PidBackupMsg::Loaded(Ok(Arc::new(BackupIndex {
                files: vec![BackupFile {
                    name: "Manifest.txt".to_string(),
                    kind: FileKind::Manifest,
                    ..BackupFile::default()
                }],
                ..BackupIndex::default()
            }))),
        );
        update(&mut state, PidBackupMsg::Toggle(NodeId::Files));
        let manifest = &visible_rows(&state)[2];
        assert_eq!(manifest.id, NodeId::File(0));
        assert!(!manifest.expandable);
    }

    #[test]
    fn every_part_states_what_the_manifest_holds() {
        let state = ready(true);
        assert_eq!(
            detail_rows(&state, &node(&[SUMMARY]))[..2],
            [
                ("Name".to_string(), "SQPlant".to_string()),
                ("Version".to_string(), "7.02".to_string()),
            ]
        );
        assert_eq!(
            detail_rows(&state, &node(&[OBJECTS])),
            [
                (t!("Database tables").into_owned(), "3".to_string()),
                (t!("Database views").into_owned(), "1".to_string()),
                (t!("Database roles").into_owned(), "1".to_string()),
                (t!("Database rights").into_owned(), "2".to_string()),
                (t!("Database files").into_owned(), "1".to_string()),
            ]
        );
        assert_eq!(
            detail_rows(&state, &node(&[OBJECTS, TABLES])),
            [
                (
                    format!("{} | {}", t!("Schema"), t!("Table name")),
                    tf!("{} row(s)", 3).into_owned()
                ),
                ("SQPlantpid".to_string(), "T_DRAWING".to_string()),
                ("SQPlantpid".to_string(), "T_MODELITEM".to_string()),
                ("SQPlant".to_string(), "T_PLANTGROUP".to_string()),
            ]
        );
        assert_eq!(
            detail_rows(&state, &node(&[OBJECTS, VIEWS]))[1],
            ("SQPlantpid".to_string(), "V_NOZZLELIST".to_string())
        );
        // Lines whose fields have no names: numbered columns.
        assert_eq!(
            detail_rows(&state, &node(&[OBJECTS, RIGHTS])),
            [
                (
                    (1..=4)
                        .map(|at| tf!("Field {}", at).into_owned())
                        .collect::<Vec<_>>()
                        .join(" | "),
                    tf!("{} row(s)", 2).into_owned()
                ),
                ("E54C".to_string(), "0 | 3 | 100".to_string()),
                ("E54C".to_string(), "0 | 3 | 301".to_string()),
            ]
        );
        assert_eq!(
            detail_rows(&state, &node(&[OBJECTS, DATABASE_FILES]))[1],
            (
                "Plant_dat".to_string(),
                "1 | C:\\Data\\Plant.mdf | PRIMARY".to_string()
            )
        );
    }

    #[test]
    fn the_file_register_says_what_each_stored_name_was() {
        let state = ready(false);
        let rows = detail_rows(&state, &node(&[FILES]));
        assert_eq!(rows[0].1, tf!("{} row(s)", 3).into_owned());
        assert_eq!(
            rows[1],
            (
                "PlantData~2~711.zip".to_string(),
                format!(
                    "Plant | {} | {} | 54 | 6 | 101929575 | \\\\host\\P\\SQPlant\\Plant",
                    t!("Folder"),
                    t!("Backed up")
                )
            )
        );
        assert_eq!(
            rows[2],
            (
                "RefData~4~680".to_string(),
                format!(
                    "Ref\\rules.rul | {} | {} | 0 | 0 | 331977 | \\\\host\\P\\SQPlant\\Ref\\rules.rul",
                    t!("Single file"),
                    t!("Backed up")
                )
            )
        );
        // A file the backup could not read has no stored name, and says why
        // where the others say they were backed up.
        assert_eq!(
            rows[3],
            (
                String::new(),
                format!(
                    "Ref\\Jacket.XML | {} | Failed to access file. |  |  |  | \\\\host\\P\\SQPlant\\Ref\\Jacket.XML",
                    t!("Single file")
                )
            )
        );
    }

    #[test]
    fn a_backup_file_is_labelled_and_described_by_what_it_was() {
        let state = ready(false);
        assert_eq!(
            labels(&state)[2..5],
            [
                (2, "Manifest.txt".to_string()),
                (2, "PlantData~2~711.zip — Plant".to_string()),
                (2, "RefData~4~680 — Ref\\rules.rul".to_string()),
            ]
        );
        let folder = detail_rows(&state, &NodeId::File(1));
        for expected in [
            (t!("Original path").into_owned(), "Plant".to_string()),
            (
                t!("Source path").into_owned(),
                "\\\\host\\P\\SQPlant\\Plant".to_string(),
            ),
            (t!("Files in the source").into_owned(), "54".to_string()),
            (t!("Folders in the source").into_owned(), "6".to_string()),
            (
                t!("Size before packing").into_owned(),
                "97.2 MB".to_string(),
            ),
        ] {
            assert!(folder.contains(&expected), "{expected:?} in {folder:?}");
        }
        // A single file: no counts of what is in it, and no members.
        let single = detail_rows(&state, &NodeId::File(2));
        assert!(single.contains(&(t!("File format").into_owned(), "RUL".to_string())));
        assert!(single.contains(&(
            t!("Original path").into_owned(),
            "Ref\\rules.rul".to_string()
        )));
        for absent in [
            t!("Files in the source"),
            t!("Folders in the source"),
            t!("Members"),
        ] {
            assert!(
                !single.iter().any(|(key, _)| *key == absent),
                "{absent} in {single:?}"
            );
        }
    }

    /// The credentials of a manifest are not in anything the panel can
    /// show: not the details of any node, not a row, not the state itself.
    #[test]
    fn no_credential_reaches_the_panel() {
        let mut state = ready(true);
        update(&mut state, PidBackupMsg::Toggle(NodeId::File(0)));
        update(&mut state, PidBackupMsg::Toggle(node(&[OBJECTS])));
        let rows = visible_rows(&state);
        let mut shown = String::new();
        for row in &rows {
            shown.push_str(&row.label);
            for (key, value) in detail_rows(&state, &row.id) {
                shown.push_str(&key);
                shown.push_str(&value);
            }
        }
        shown.push_str(&format!("{state:?}"));
        for secret in [OBFUSCATED, PASSWORD, "#####"] {
            assert!(!shown.contains(secret), "a credential is shown");
        }
        // The parts say that something is hidden, and how much.
        let note = |count: usize| {
            (
                tf!(
                    "{} credential field(s) are hidden and shown as {}",
                    count,
                    HIDDEN
                )
                .into_owned(),
                String::new(),
            )
        };
        let connections = detail_rows(&state, &node(&[CONNECTIONS]));
        assert_eq!(connections[0], note(4));
        assert_eq!(
            connections[2],
            (
                "SiteConnInfo".to_string(),
                format!("{HIDDEN} | 1 | ORCL | SQSite | {HIDDEN} | system")
            )
        );
        assert_eq!(connections[5], ("DBPwds".to_string(), HIDDEN.to_string()));
        let summary = detail_rows(&state, &node(&[SUMMARY]));
        assert!(summary.contains(&(
            "BackupCommand".to_string(),
            format!("Exp.exe system/{HIDDEN}@ORCL OWNER=(SQPlant)")
        )));
        assert_eq!(summary.last(), Some(&note(1)));
    }

    /// The manifest's rows as they are drawn: the arrow its row gained is
    /// where a press lands, its parts take clicks, and a part's table is on
    /// screen with its titles.
    #[test]
    fn the_drawn_manifest_row_unfolds_and_its_parts_show_their_tables() {
        let _guard = crate::headless_ui_lock();
        let mut state = ready(false);
        let press = |state: &PidBackupBrowser, label: &str, left_of: bool| -> Vec<PidBackupMsg> {
            let mut ui = iced_test::simulator::Simulator::with_size(
                iced::Settings::default(),
                (340.0, 700.0),
                view(state, 340.0, Chrome::default()),
            );
            let bounds = ui
                .find(label)
                .unwrap_or_else(|error| panic!("\"{label}\" is drawn: {error:?}"))
                .visible_bounds()
                .expect("on screen");
            let x = if left_of {
                bounds.x - 7.0
            } else {
                bounds.center_x()
            };
            ui.point_at(iced::Point::new(x, bounds.center_y()));
            let _ = ui.simulate(iced_test::simulator::click());
            ui.into_messages()
                .filter_map(|message| match message {
                    Message::PidBackup(message) => Some(message),
                    _ => None,
                })
                .collect()
        };
        let apply = |state: &mut PidBackupBrowser, messages: Vec<PidBackupMsg>| {
            for message in messages {
                update(state, message);
            }
        };

        let pressed = press(&state, "Manifest.txt", true);
        assert!(
            pressed
                .iter()
                .any(|message| matches!(message, PidBackupMsg::Toggle(NodeId::File(0)))),
            "{pressed:?}"
        );
        apply(&mut state, pressed);

        let register = format!("{} (3)", t!("File register"));
        let clicked = press(&state, &register, false);
        assert!(
            clicked.iter().any(
                |message| matches!(message, PidBackupMsg::Select(id) if *id == node(&[FILES]))
            ),
            "{clicked:?}"
        );
        apply(&mut state, clicked);

        // Its table is drawn: the titles and a cell of each row.
        let mut ui = iced_test::simulator::Simulator::with_size(
            iced::Settings::default(),
            (340.0, 700.0),
            view(&state, 340.0, Chrome::default()),
        );
        for drawn in [
            t!("Stored as").into_owned(),
            t!("Original path").into_owned(),
            "Ref\\rules.rul".to_string(),
            "Ref\\Jacket.XML".to_string(),
        ] {
            assert!(ui.find(drawn.as_str()).is_ok(), "\"{drawn}\" is drawn");
        }
    }
}
