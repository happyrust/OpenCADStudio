//! The enumerations of a data map, under the file.
//!
//! A plant's reference data carries `SPPIDDataMap.xml` and usually a few
//! dated copies of it. The tree shows each as a file, and its details show
//! its text: three megabytes of XML. What the file says is easier read as a
//! table, and the file's row opens into it:
//!
//! ```text
//! SmartPlant Resources
//!   SPPIDDataMap.xml
//!     Enumeration map (1924)
//! ```
//!
//! The row's details list every entry of every enumeration list with the
//! value a published drawing carries for it. The file is read for that when
//! its row is unfolded, off the UI thread.

use std::sync::Arc;

use super::grid::{Column, Table};
use super::section::{Cx, Event, EventKind, Section, SectionData, SectionLoad};
use super::{DetailLine, NodeId, Row};
use crate::io::pid_backup_browse::datamap::{is_data_map, read_enum_map, EnumMap};
use crate::io::pid_backup_browse::{BackupIndex, RefFile};
use crate::t;

/// The data maps of the reference tree.
pub(super) struct DataMapSection;

/// The address of the section's node under the file `file` of the
/// reference directory at `dirs`: the directory's path, then the file.
fn address(dirs: &[usize], file: usize) -> Vec<usize> {
    let mut path = dirs.to_vec();
    path.push(file);
    path
}

/// The directory and the file an address names.
fn addressed(path: &[usize]) -> Option<(&[usize], usize)> {
    let (file, dirs) = path.split_last()?;
    Some((dirs, *file))
}

/// The file at `dirs` / `file` of the reference tree, when it is a data map.
fn data_map<'a>(index: &'a BackupIndex, dirs: &[usize], file: usize) -> Option<&'a RefFile> {
    let mut node = &index.ref_tree;
    for at in dirs {
        node = node.dirs.get(*at)?;
    }
    node.files.get(file).filter(|file| is_data_map(&file.name))
}

/// A data map as the detail half shows it.
struct MapView {
    lists: usize,
    mappings: usize,
    table: Arc<Table>,
}

fn map_view(map: EnumMap) -> MapView {
    MapView {
        lists: map.lists,
        mappings: map.mappings.len(),
        table: Arc::new(Table::new(
            vec![
                Column::text(t!("Enumeration list")),
                Column::text(t!("Entry UID")),
                Column::number(t!("Entry number")),
                Column::text(t!("Published as")),
            ],
            map.mappings
                .into_iter()
                .map(|mapping| {
                    vec![
                        // A list the file does not name goes by its UID.
                        if mapping.list.is_empty() {
                            mapping.list_uid
                        } else {
                            mapping.list
                        },
                        mapping.uid,
                        mapping
                            .number
                            .map(|number| number.to_string())
                            .unwrap_or_default(),
                        mapping.published,
                    ]
                })
                .collect(),
        )),
    }
}

fn nothing_to_read() -> String {
    t!("this node has nothing to read").into_owned()
}

impl Section for DataMapSection {
    fn hosts(&self, cx: &Cx<'_>, host: &NodeId) -> bool {
        matches!(host, NodeId::RefFile(dirs, file) if data_map(cx.index, dirs, *file).is_some())
    }

    fn rows(&self, cx: &Cx<'_>, host: &NodeId, depth: usize, rows: &mut Vec<Row>) {
        let NodeId::RefFile(dirs, file) = host else {
            return;
        };
        if data_map(cx.index, dirs, *file).is_none() {
            return;
        }
        let id = cx.node(address(dirs, *file));
        // The count comes with the read.
        let read = match cx.load(&id) {
            Some(SectionLoad::Ready(data)) => data.get::<MapView>(),
            _ => None,
        };
        rows.push(Row {
            label: match read {
                Some(view) => format!("{} ({})", t!("Enumeration map"), view.mappings),
                None => t!("Enumeration map").into_owned(),
            },
            id,
            depth,
            expandable: false,
            expanded: false,
            disabled: false,
        });
    }

    fn details(&self, cx: &Cx<'_>, id: &NodeId, lines: &mut Vec<DetailLine>) {
        let Some((dirs, file)) = cx.own(id).and_then(addressed) else {
            return;
        };
        let Some(entry) = data_map(cx.index, dirs, file) else {
            return;
        };
        let marker = |text: std::borrow::Cow<'static, str>| DetailLine::Marker(text.into_owned());
        lines.push(DetailLine::Field(
            t!("Reference file name").into_owned(),
            entry.name.clone(),
        ));
        match cx.load(id) {
            None => lines.push(marker(t!("Select this row to read the map."))),
            Some(SectionLoad::Loading) => lines.push(marker(t!("Reading the file…"))),
            Some(SectionLoad::Failed(error)) => lines.push(DetailLine::Marker(error.clone())),
            Some(SectionLoad::Ready(data)) => {
                let Some(view) = data.get::<MapView>() else {
                    return;
                };
                lines.push(DetailLine::Field(
                    t!("Enumeration lists").into_owned(),
                    view.lists.to_string(),
                ));
                lines.push(marker(t!(
                    "Each entry of an enumeration list, with the value a published drawing carries for it."
                )));
                if view.mappings == 0 {
                    lines.push(marker(t!("The file maps no enumeration.")));
                } else {
                    lines.push(DetailLine::Table(Arc::clone(&view.table)));
                }
            }
        }
    }

    /// The map is read when its file is unfolded -- so the row under it
    /// comes with its count -- or when the row itself is selected.
    fn wants(&self, cx: &Cx<'_>, event: &Event<'_>) -> Option<NodeId> {
        match event.node {
            NodeId::RefFile(dirs, file) => (event.kind == EventKind::Toggled
                && cx.is_open(event.node)
                && data_map(cx.index, dirs, *file).is_some())
            .then(|| cx.node(address(dirs, *file))),
            node => {
                let (dirs, file) = cx.own(node).and_then(addressed)?;
                (event.kind == EventKind::Selected && data_map(cx.index, dirs, file).is_some())
                    .then(|| node.clone())
            }
        }
    }

    fn load(&self, index: &BackupIndex, node: &NodeId) -> Result<SectionData, String> {
        let NodeId::Section(_, path) = node else {
            return Err(nothing_to_read());
        };
        let (dirs, file) = addressed(path).ok_or_else(nothing_to_read)?;
        data_map(index, dirs, file).ok_or_else(nothing_to_read)?;
        let path = index
            .ref_file_path(dirs, file)
            .ok_or_else(nothing_to_read)?;
        read_enum_map(&path).map(|map| SectionData::new(map_view(map)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::Message;
    use crate::io::pid_backup_browse::RefNode;
    use crate::tf;
    use crate::ui::dock::Chrome;
    use crate::ui::window::pid_backup_browser::{
        detail_rows, update, view, visible_rows, Effect, PidBackupBrowser, PidBackupMsg,
    };

    const XML: &str = r#"<Container>
   <SPMapEnumListDef>
      <IObject UID="SP_20" Name="Fluid System" Description="Fluid"/>
   </SPMapEnumListDef>
   <Rel>
      <IRel UID1="SP_20_12" UID2="F12" DefUID="MapEnumToEnum"/>
   </Rel>
   <Rel>
      <IRel UID1="SP_20_3" UID2="F3" DefUID="MapEnumToEnum"/>
   </Rel>
   <Rel>
      <IRel UID1="SP_99_1" UID2="X" DefUID="MapEnumToEnum"/>
   </Rel>
</Container>
"#;

    fn file(name: &str) -> RefFile {
        RefFile {
            name: name.to_string(),
            size: 1,
        }
    }

    /// A reference tree on disk: `SmartPlant Resources` with a data map, a
    /// copy of it that maps nothing, and a file that is no data map.
    fn ready(name: &str) -> (PidBackupBrowser, std::path::PathBuf) {
        let root = std::env::temp_dir().join(format!("ocs-datamap-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let folder = root.join("SmartPlant Resources");
        std::fs::create_dir_all(&folder).unwrap();
        std::fs::write(folder.join("SPPIDDataMap.xml"), XML).unwrap();
        std::fs::write(folder.join("SPPIDDataMap-1.xml"), "<Container/>").unwrap();
        std::fs::write(folder.join("rules.xml"), "<Rules/>").unwrap();
        let index = BackupIndex {
            ref_root: root.clone(),
            ref_tree: RefNode {
                name: "Ref".to_string(),
                dirs: vec![RefNode {
                    name: "SmartPlant Resources".to_string(),
                    dirs: Vec::new(),
                    files: vec![
                        file("SPPIDDataMap.xml"),
                        file("SPPIDDataMap-1.xml"),
                        file("rules.xml"),
                    ],
                }],
                files: Vec::new(),
            },
            ..BackupIndex::default()
        };
        let mut state = PidBackupBrowser::default();
        update(&mut state, PidBackupMsg::Loaded(Ok(Arc::new(index))));
        update(&mut state, PidBackupMsg::Toggle(NodeId::Ref));
        update(&mut state, PidBackupMsg::Toggle(NodeId::RefDir(vec![0])));
        (state, root)
    }

    /// Unfold the data map that is file `file` of the folder, and read it
    /// as the worker thread would. Returns the node of the row under it.
    fn unfold(state: &mut PidBackupBrowser, file: usize) -> NodeId {
        let host = NodeId::RefFile(vec![0], file);
        let Effect::LoadSection {
            generation,
            section,
            node,
        } = update(state, PidBackupMsg::Toggle(host))
        else {
            panic!("unfolding a data map reads it");
        };
        let index = state.index().cloned().expect("the backup is ready");
        let result = DataMapSection.load(&index, &node);
        update(
            state,
            PidBackupMsg::SectionLoaded {
                generation,
                section,
                node: node.clone(),
                result,
            },
        );
        node
    }

    fn under_folder(state: &PidBackupBrowser) -> Vec<(usize, String, bool)> {
        let rows = visible_rows(state);
        let at = rows
            .iter()
            .position(|row| row.id == NodeId::RefDir(vec![0]))
            .expect("the folder has a row");
        rows[at + 1..]
            .iter()
            .take_while(|row| row.depth > rows[at].depth)
            .map(|row| {
                (
                    row.depth - rows[at].depth,
                    row.label.clone(),
                    row.expandable,
                )
            })
            .collect()
    }

    #[test]
    fn a_data_map_opens_into_its_enumerations_and_other_files_stay_leaves() {
        let (mut state, root) = ready("opens");
        assert_eq!(
            under_folder(&state),
            [
                (1, "SPPIDDataMap.xml".to_string(), true),
                (1, "SPPIDDataMap-1.xml".to_string(), true),
                (1, "rules.xml".to_string(), false),
            ]
        );
        // A file that is no data map unfolds nothing and reads nothing.
        assert_eq!(
            update(
                &mut state,
                PidBackupMsg::Toggle(NodeId::RefFile(vec![0], 2))
            ),
            Effect::None
        );

        let map = unfold(&mut state, 0);
        assert_eq!(
            under_folder(&state)[..2],
            [
                (1, "SPPIDDataMap.xml".to_string(), true),
                (2, format!("{} (3)", t!("Enumeration map")), false),
            ]
        );
        let titles = [
            t!("Enumeration list"),
            t!("Entry UID"),
            t!("Entry number"),
            t!("Published as"),
        ]
        .join(" | ");
        assert_eq!(
            detail_rows(&state, &map),
            [
                (
                    t!("Reference file name").into_owned(),
                    "SPPIDDataMap.xml".to_string()
                ),
                (t!("Enumeration lists").into_owned(), "1".to_string()),
                (
                    t!("Each entry of an enumeration list, with the value a published drawing carries for it.")
                        .into_owned(),
                    String::new()
                ),
                (titles, tf!("{} row(s)", 3).into_owned()),
                // A list the file does not name goes by its UID.
                ("SP_99".to_string(), "SP_99_1 | 1 | @{X}".to_string()),
                // By number: 3 before 12.
                (
                    "Fluid System".to_string(),
                    "SP_20_3 | 3 | @{F3}".to_string()
                ),
                (
                    "Fluid System".to_string(),
                    "SP_20_12 | 12 | @{F12}".to_string()
                ),
            ]
        );
        // Folding the file and unfolding it again reads nothing anew.
        let host = NodeId::RefFile(vec![0], 0);
        assert_eq!(
            update(&mut state, PidBackupMsg::Toggle(host.clone())),
            Effect::None
        );
        assert_eq!(update(&mut state, PidBackupMsg::Toggle(host)), Effect::None);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn a_map_with_no_enumeration_and_one_that_is_gone_say_so() {
        let (mut state, root) = ready("empty");
        let empty = unfold(&mut state, 1);
        // The first map, the second, then the row under the second.
        assert_eq!(
            under_folder(&state)[2],
            (2, format!("{} (0)", t!("Enumeration map")), false)
        );
        assert_eq!(
            detail_rows(&state, &empty).last(),
            Some(&(
                t!("The file maps no enumeration.").into_owned(),
                String::new()
            ))
        );
        // The file went away after the backup was indexed: the row says
        // what could not be read, and has no count.
        std::fs::remove_file(root.join("SmartPlant Resources").join("SPPIDDataMap.xml")).unwrap();
        let gone = unfold(&mut state, 0);
        let rows = detail_rows(&state, &gone);
        assert_eq!(rows.len(), 2);
        assert!(rows[1].0.contains("SPPIDDataMap.xml"), "{rows:?}");
        assert!(under_folder(&state)
            .iter()
            .any(|(depth, label, _)| *depth == 2 && *label == t!("Enumeration map")));
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn a_row_that_was_not_read_yet_is_read_when_selected() {
        let (mut state, root) = ready("selected");
        // Unfolded while the read is still out: the row is there, uncounted.
        let host = NodeId::RefFile(vec![0], 0);
        let Effect::LoadSection { node, .. } = update(&mut state, PidBackupMsg::Toggle(host))
        else {
            panic!("unfolding a data map reads it");
        };
        assert_eq!(
            under_folder(&state)[1],
            (2, t!("Enumeration map").into_owned(), false)
        );
        assert_eq!(
            detail_rows(&state, &node)[1],
            (t!("Reading the file…").into_owned(), String::new())
        );
        // Selecting it does not ask twice.
        assert_eq!(
            update(&mut state, PidBackupMsg::Select(node.clone())),
            Effect::None
        );
        // A backup opened anew has the row unread until something asks.
        let (mut state, second) = ready("selected-again");
        state.expanded.insert(NodeId::RefFile(vec![0], 0));
        assert_eq!(
            detail_rows(&state, &node)[1],
            (
                t!("Select this row to read the map.").into_owned(),
                String::new()
            )
        );
        assert!(matches!(
            update(&mut state, PidBackupMsg::Select(node.clone())),
            Effect::LoadSection { .. }
        ));
        let _ = std::fs::remove_dir_all(&root);
        let _ = std::fs::remove_dir_all(&second);
    }

    /// The branch as the panel draws it: the arrow of the data map is where
    /// a press lands, and the row under it is one a click selects.
    #[test]
    fn the_drawn_data_map_unfolds_and_its_row_is_clicked() {
        let _guard = crate::headless_ui_lock();
        let (mut state, root) = ready("drawn");

        fn click(state: &PidBackupBrowser, label: &str, left_of: bool) -> Vec<PidBackupMsg> {
            let mut ui = iced_test::simulator::Simulator::with_size(
                iced::Settings::default(),
                (340.0, 700.0),
                view(state, 340.0, Chrome::default()),
            );
            let bounds = ui
                .find(label)
                .unwrap_or_else(|error| panic!("{label} is drawn: {error:?}"))
                .visible_bounds()
                .unwrap_or_else(|| panic!("{label} is on screen"));
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
        }

        let pressed = click(&state, "SPPIDDataMap.xml", true);
        assert!(
            pressed.iter().any(|message| matches!(
                message,
                PidBackupMsg::Toggle(NodeId::RefFile(dirs, 0)) if dirs[..] == [0]
            )),
            "{pressed:?}"
        );
        // The press unfolds the file; read it as the worker would. (The
        // helper toggles, so fold the file again first.)
        for message in pressed {
            update(&mut state, message);
        }
        update(
            &mut state,
            PidBackupMsg::Toggle(NodeId::RefFile(vec![0], 0)),
        );
        state.section_data.clear();
        let map = unfold(&mut state, 0);

        let label = format!("{} (3)", t!("Enumeration map"));
        let clicked = click(&state, &label, false);
        assert!(
            clicked
                .iter()
                .any(|message| matches!(message, PidBackupMsg::Select(found) if *found == map)),
            "{clicked:?}"
        );
        for message in clicked {
            update(&mut state, message);
        }
        // Its entries are drawn as a grid.
        let mut ui = iced_test::simulator::Simulator::with_size(
            iced::Settings::default(),
            (340.0, 700.0),
            view(&state, 340.0, Chrome::default()),
        );
        assert!(ui.find("SP_20_3").is_ok());
        let _ = std::fs::remove_dir_all(&root);
    }
}
