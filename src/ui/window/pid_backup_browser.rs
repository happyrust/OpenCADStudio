//! P&ID backup browser — panel state and the pure functions behind it.
//!
//! A SmartPlant backup is four trees at once: the backup's own files (with the
//! member lists of its archives), the plant-group tree with the drawings in
//! it, the model items a drawing holds, and the `Ref` reference data. The
//! panel draws exactly one of them at a time as a flat list of rows, so the
//! list is [`visible_rows`]: a pure function of the state that walks only the
//! nodes the user expanded — a backup can carry 645 reference files and
//! thousands of model items, none of which a collapsed row may cost.
//!
//! Every click is a message, and [`update`] is the pure function that applies
//! it and says what the application should do next ([`Effect`]): read a
//! drawing's items off the UI thread, open a drawing in a tab, ask for a
//! source, rebuild the index. The panel therefore has no IO of its own and is
//! testable without a window, which is what `visible_rows`/`update` are for.
//!
//! The state lives on the application, not on a tab, so switching documents
//! leaves the tree where the user left it.

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::Arc;

use crate::io::pid_backup_browse::{BackupIndex, DrawingItems, GroupNode, RefNode};
use crate::t;

/// Where the panel is in loading a backup.
#[derive(Debug, Clone, Default)]
pub enum LoadState {
    /// Nothing loaded yet: the panel shows its "open a backup" hint.
    #[default]
    Idle,
    /// An index is being built for this source.
    Loading(PathBuf),
    /// The index is ready to browse.
    Ready(Arc<BackupIndex>),
}

/// What the panel knows about one drawing's model items.
#[derive(Debug, Clone)]
pub enum ItemsState {
    /// The read is in flight; the drawing's subtree stays empty until it lands.
    Loading,
    /// The drawing's items, grouped by `ModelItemType`.
    Ready(Arc<DrawingItems>),
    /// The read failed; the drawing's subtree stays empty.
    Failed(String),
}

/// A node of the browse tree.
///
/// Indices address the ready [`BackupIndex`]: a drawing by its position in
/// [`BackupIndex::drawings`], a file by its position in
/// [`BackupIndex::files`], a group by its path of child indices from the
/// group roots, a `Ref` directory by its path of directory indices, an item by
/// `(drawing, item-type group, item)`.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum NodeId {
    /// The backup itself: manifest summary and warnings.
    Root,
    /// The "backup files" group.
    Files,
    /// One file of the backup root.
    File(usize),
    /// One member of an archive, `(file, member)`.
    FileMember(usize, usize),
    /// The "plant groups" group.
    Groups,
    /// One group of the plant-group tree, as the child indices from the root.
    Group(Vec<usize>),
    /// One drawing, by its index into [`BackupIndex::drawings`].
    Drawing(usize),
    /// One `ModelItemType` group of a drawing, `(drawing, group)`.
    ItemType(usize, usize),
    /// One model item, `(drawing, group, item)`.
    Item(usize, usize, usize),
    /// The "reference data" group.
    Ref,
    /// One directory of the `Ref` tree, as the child indices from its root.
    RefDir(Vec<usize>),
    /// One file of the `Ref` tree, `(directory path, file)`.
    RefFile(Vec<usize>, usize),
}

/// What the user did in the panel.
///
/// No `PartialEq`: two loads carry whole indexes, which the panel never
/// compares — the messages only travel one way.
#[derive(Debug, Clone)]
pub enum PidBackupMsg {
    /// The "open…" button: ask for a backup zip or folder.
    Open,
    /// The source dialog came back; `None` when the user cancelled.
    SourcePicked(Option<PathBuf>),
    /// The index finished building, or failed to.
    Loaded(Result<Arc<BackupIndex>, String>),
    /// One drawing's items finished reading, or failed to.
    ItemsLoaded(usize, Result<Arc<DrawingItems>, String>),
    /// The expand arrow of a node.
    Toggle(NodeId),
    /// A click on a row: select it.
    Select(NodeId),
    /// A double click on a row: open the drawing it names.
    Activate(NodeId),
    /// The "refresh" button: rebuild the index from the current source.
    Refresh,
    /// The "close backup" button: forget the index and go back to idle.
    Close,
    /// The search box: narrow the tree to the drawings whose name matches.
    Filter(String),
}

/// What the application should do after [`update`] applied a message.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Effect {
    /// Nothing to do: the state change is the whole answer.
    None,
    /// Read the items of drawing `usize` (index into
    /// [`BackupIndex::drawings`]) from the `.pid` at the path.
    LoadItems(usize, PathBuf),
    /// Open the `.pid` at the path in a tab.
    OpenDrawing(PathBuf),
    /// Build the index of this source.
    Load(PathBuf),
    /// Rebuild the index from the current source.
    Refresh(PathBuf),
    /// Ask the user for a backup source.
    PickSource,
}

/// One row of the flattened tree.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Row {
    /// The node the row stands for.
    pub id: NodeId,
    /// Indentation level: 0 is the backup itself.
    pub depth: usize,
    /// The text the row shows.
    pub label: String,
    /// Whether the row has an expand arrow.
    pub expandable: bool,
    /// Whether the row's subtree is open.
    pub expanded: bool,
    /// Whether the row is greyed and cannot be activated (a missing drawing).
    pub disabled: bool,
}

/// The panel's state.
#[derive(Debug, Clone, Default)]
pub struct PidBackupBrowser {
    /// Loading state of the backup itself.
    pub load: LoadState,
    /// Why the last load failed, kept beside the idle state so the panel can
    /// say what went wrong after it goes back to its hint.
    pub error: Option<String>,
    /// The nodes the user opened.
    pub expanded: HashSet<NodeId>,
    /// The row the user selected.
    pub selected: Option<NodeId>,
    /// One drawing's items, by index into [`BackupIndex::drawings`].
    pub items: HashMap<usize, ItemsState>,
    /// The search box's text.
    pub filter: String,
}

impl PidBackupBrowser {
    /// The ready index, when there is one.
    pub fn index(&self) -> Option<&Arc<BackupIndex>> {
        match &self.load {
            LoadState::Ready(index) => Some(index),
            _ => None,
        }
    }

    /// Forget everything the last backup put in the panel.
    fn clear(&mut self) {
        self.expanded.clear();
        self.selected = None;
        self.items.clear();
        self.filter.clear();
    }
}

/// The rows the panel shows, in display order.
///
/// Only the nodes [`PidBackupBrowser::expanded`] holds are walked, so a
/// collapsed `Ref` tree of 645 files costs one row. A non-empty filter keeps
/// only the drawings whose name contains it (case-insensitively) and the
/// groups above them, which are shown open; the backup files and the `Ref`
/// tree carry no drawings and drop out.
pub fn visible_rows(state: &PidBackupBrowser) -> Vec<Row> {
    let Some(index) = state.index() else {
        return Vec::new();
    };
    let filter = state.filter.trim().to_lowercase();
    let filtering = !filter.is_empty();
    let mut rows = Vec::new();
    rows.push(Row {
        id: NodeId::Root,
        depth: 0,
        label: root_label(index),
        // The root always shows its three groups: folding them would leave
        // the panel with nothing to click.
        expandable: true,
        expanded: true,
        disabled: false,
    });
    if !filtering {
        push_files(state, index, &mut rows);
    }
    push_groups(state, index, &filter, filtering, &mut rows);
    if !filtering {
        push_ref(state, index, &mut rows);
    }
    rows
}

/// Apply `msg` to `state` and say what the application should do next.
///
/// Pure: no file is read, no task is started — the caller turns the returned
/// [`Effect`] into work.
pub fn update(state: &mut PidBackupBrowser, msg: PidBackupMsg) -> Effect {
    match msg {
        PidBackupMsg::Open => Effect::PickSource,
        PidBackupMsg::SourcePicked(None) => Effect::None,
        PidBackupMsg::SourcePicked(Some(source)) => {
            state.error = None;
            state.clear();
            state.load = LoadState::Loading(source.clone());
            Effect::Load(source)
        }
        PidBackupMsg::Loaded(Ok(index)) => {
            state.error = None;
            state.clear();
            state.load = LoadState::Ready(index);
            Effect::None
        }
        PidBackupMsg::Loaded(Err(error)) => {
            state.clear();
            state.load = LoadState::Idle;
            state.error = Some(error);
            Effect::None
        }
        PidBackupMsg::ItemsLoaded(drawing, Ok(items)) => {
            state.items.insert(drawing, ItemsState::Ready(items));
            Effect::None
        }
        PidBackupMsg::ItemsLoaded(drawing, Err(error)) => {
            state.items.insert(drawing, ItemsState::Failed(error));
            Effect::None
        }
        PidBackupMsg::Toggle(id) => toggle(state, id),
        PidBackupMsg::Select(id) => {
            state.selected = Some(id);
            Effect::None
        }
        PidBackupMsg::Activate(id) => activate(state, id),
        PidBackupMsg::Refresh => match &state.load {
            LoadState::Ready(index) => Effect::Refresh(index.source.clone()),
            LoadState::Loading(source) => Effect::Refresh(source.clone()),
            LoadState::Idle => Effect::None,
        },
        PidBackupMsg::Close => {
            state.load = LoadState::Idle;
            state.error = None;
            state.clear();
            Effect::None
        }
        PidBackupMsg::Filter(text) => {
            state.filter = text;
            Effect::None
        }
    }
}

/// The root row's text: the backup's own name when the manifest states one.
fn root_label(index: &BackupIndex) -> String {
    index
        .manifest
        .as_ref()
        .and_then(|manifest| manifest.name.clone())
        .unwrap_or_else(|| t!("Backup").into_owned())
}

fn push_files(state: &PidBackupBrowser, index: &BackupIndex, rows: &mut Vec<Row>) {
    let expanded = state.expanded.contains(&NodeId::Files);
    rows.push(Row {
        id: NodeId::Files,
        depth: 1,
        label: t!("Backup files").into_owned(),
        expandable: !index.files.is_empty(),
        expanded,
        disabled: false,
    });
    if !expanded {
        return;
    }
    for (at, file) in index.files.iter().enumerate() {
        let id = NodeId::File(at);
        let open = state.expanded.contains(&id);
        rows.push(Row {
            id,
            depth: 2,
            label: file.name.clone(),
            expandable: !file.members.is_empty(),
            expanded: open,
            disabled: false,
        });
        if !open {
            continue;
        }
        for (member, name) in file.members.iter().enumerate() {
            rows.push(Row {
                id: NodeId::FileMember(at, member),
                depth: 3,
                label: name.clone(),
                expandable: false,
                expanded: false,
                disabled: false,
            });
        }
    }
}

fn push_groups(
    state: &PidBackupBrowser,
    index: &BackupIndex,
    filter: &str,
    filtering: bool,
    rows: &mut Vec<Row>,
) {
    let expanded = state.expanded.contains(&NodeId::Groups);
    rows.push(Row {
        id: NodeId::Groups,
        depth: 1,
        label: t!("Plant groups").into_owned(),
        expandable: !index.plant_groups.is_empty(),
        expanded: expanded || filtering,
        disabled: false,
    });
    if !(expanded || filtering) {
        return;
    }
    for (at, node) in index.plant_groups.iter().enumerate() {
        push_group(state, index, node, &[at], 2, filter, filtering, rows);
    }
}

fn push_group(
    state: &PidBackupBrowser,
    index: &BackupIndex,
    node: &GroupNode,
    path: &[usize],
    depth: usize,
    filter: &str,
    filtering: bool,
    rows: &mut Vec<Row>,
) {
    if filtering && !group_has_match(index, node, filter) {
        return;
    }
    let id = NodeId::Group(path.to_vec());
    let open = state.expanded.contains(&id) || filtering;
    rows.push(Row {
        id,
        depth,
        label: node.name.clone(),
        expandable: !node.children.is_empty() || !node.drawings.is_empty(),
        expanded: open,
        disabled: false,
    });
    if !open {
        return;
    }
    for (at, child) in node.children.iter().enumerate() {
        let mut child_path = path.to_vec();
        child_path.push(at);
        push_group(
            state,
            index,
            child,
            &child_path,
            depth + 1,
            filter,
            filtering,
            rows,
        );
    }
    for &drawing in &node.drawings {
        push_drawing(state, index, drawing, depth + 1, filter, filtering, rows);
    }
}

fn push_drawing(
    state: &PidBackupBrowser,
    index: &BackupIndex,
    drawing: usize,
    depth: usize,
    filter: &str,
    filtering: bool,
    rows: &mut Vec<Row>,
) {
    let Some(entry) = index.drawings.get(drawing) else {
        return;
    };
    if filtering && !entry.name.to_lowercase().contains(filter) {
        return;
    }
    let items = state.items.get(&drawing);
    let id = NodeId::Drawing(drawing);
    let open = state.expanded.contains(&id);
    rows.push(Row {
        id,
        depth,
        label: entry.name.clone(),
        expandable: matches!(items, Some(ItemsState::Ready(items)) if !items.groups.is_empty()),
        expanded: open,
        disabled: entry.pid_path.is_none(),
    });
    if !open {
        return;
    }
    let Some(ItemsState::Ready(items)) = items else {
        return;
    };
    for (at, group) in items.groups.iter().enumerate() {
        let id = NodeId::ItemType(drawing, at);
        let open = state.expanded.contains(&id);
        rows.push(Row {
            id,
            depth: depth + 1,
            label: format!("{} ({})", group.item_type, group.items.len()),
            expandable: !group.items.is_empty(),
            expanded: open,
            disabled: false,
        });
        if !open {
            continue;
        }
        for (item, entry) in group.items.iter().enumerate() {
            rows.push(Row {
                id: NodeId::Item(drawing, at, item),
                depth: depth + 2,
                label: match &entry.class {
                    Some(class) => format!("{} ({class})", entry.uid),
                    None => entry.uid.clone(),
                },
                expandable: false,
                expanded: false,
                disabled: false,
            });
        }
    }
}

/// Whether any drawing in `node`'s subtree matches `filter`.
fn group_has_match(index: &BackupIndex, node: &GroupNode, filter: &str) -> bool {
    node.drawings.iter().any(|drawing| {
        index
            .drawings
            .get(*drawing)
            .is_some_and(|entry| entry.name.to_lowercase().contains(filter))
    }) || node
        .children
        .iter()
        .any(|child| group_has_match(index, child, filter))
}

fn push_ref(state: &PidBackupBrowser, index: &BackupIndex, rows: &mut Vec<Row>) {
    let expanded = state.expanded.contains(&NodeId::Ref);
    let tree = &index.ref_tree;
    rows.push(Row {
        id: NodeId::Ref,
        depth: 1,
        label: t!("Reference data").into_owned(),
        expandable: !(tree.dirs.is_empty() && tree.files.is_empty()),
        expanded,
        disabled: false,
    });
    if expanded {
        push_ref_children(state, tree, &[], 2, rows);
    }
}

fn push_ref_children(
    state: &PidBackupBrowser,
    node: &RefNode,
    path: &[usize],
    depth: usize,
    rows: &mut Vec<Row>,
) {
    for (at, dir) in node.dirs.iter().enumerate() {
        let mut child_path = path.to_vec();
        child_path.push(at);
        let id = NodeId::RefDir(child_path.clone());
        let open = state.expanded.contains(&id);
        rows.push(Row {
            id,
            depth,
            label: dir.name.clone(),
            expandable: !(dir.dirs.is_empty() && dir.files.is_empty()),
            expanded: open,
            disabled: false,
        });
        if open {
            push_ref_children(state, dir, &child_path, depth + 1, rows);
        }
    }
    for (at, file) in node.files.iter().enumerate() {
        rows.push(Row {
            id: NodeId::RefFile(path.to_vec(), at),
            depth,
            label: file.name.clone(),
            expandable: false,
            expanded: false,
            disabled: false,
        });
    }
}

/// Fold or unfold a node; a drawing's first unfold reads its items.
fn toggle(state: &mut PidBackupBrowser, id: NodeId) -> Effect {
    if id == NodeId::Root {
        return Effect::None;
    }
    if let NodeId::Drawing(drawing) = id {
        let Some(path) = state
            .index()
            .and_then(|index| index.drawings.get(drawing))
            .and_then(|entry| entry.pid_path.clone())
        else {
            // A drawing with no file has nothing to read.
            return Effect::None;
        };
        if state.items.contains_key(&drawing) {
            flip(state, NodeId::Drawing(drawing));
            return Effect::None;
        }
        state.items.insert(drawing, ItemsState::Loading);
        state.expanded.insert(NodeId::Drawing(drawing));
        return Effect::LoadItems(drawing, path);
    }
    flip(state, id);
    Effect::None
}

fn flip(state: &mut PidBackupBrowser, id: NodeId) {
    if !state.expanded.remove(&id) {
        state.expanded.insert(id);
    }
}

/// Open a drawing; anything else, and a drawing without a file, does nothing.
fn activate(state: &PidBackupBrowser, id: NodeId) -> Effect {
    let NodeId::Drawing(drawing) = id else {
        return Effect::None;
    };
    state
        .index()
        .and_then(|index| index.drawings.get(drawing))
        .and_then(|entry| entry.pid_path.clone())
        .map_or(Effect::None, Effect::OpenDrawing)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::io::pid_backup_browse::{
        BackupFile, DrawingEntry, DrawingItem, FileKind, ItemAttribute, ItemGroup, ManifestSummary,
        RefFile,
    };

    fn group(name: &str, path: &str, drawings: Vec<usize>) -> GroupNode {
        GroupNode {
            name: name.to_string(),
            path: path.to_string(),
            children: Vec::new(),
            drawings,
        }
    }

    fn drawing(name: &str, file: Option<&str>) -> DrawingEntry {
        DrawingEntry {
            name: name.to_string(),
            pid_path: file.map(PathBuf::from),
            group_path: "SQPlant / 00 / 00".to_string(),
            meta: vec![("id".to_string(), "1".to_string())],
        }
    }

    fn file(name: &str, members: &[&str]) -> BackupFile {
        BackupFile {
            name: name.to_string(),
            kind: FileKind::PlantData,
            format: "zip".to_string(),
            size: 10,
            members: members.iter().map(|m| m.to_string()).collect(),
            version_line: None,
            missing: false,
        }
    }

    fn ref_file(name: &str) -> RefFile {
        RefFile {
            name: name.to_string(),
            size: 1,
        }
    }

    /// A backup with two plant groups, three drawings (one missing its file),
    /// two backup files (one an archive with two members) and a `Ref` tree.
    fn index() -> Arc<BackupIndex> {
        // `SQPlant / 00` holds two drawings, its child `01` holds the third.
        let mut plant = group("00", "SQPlant / 00", vec![0, 2]);
        plant
            .children
            .push(group("01", "SQPlant / 00 / 01", vec![1]));
        let mut root = group("SQPlant", "SQPlant", Vec::new());
        root.children.push(plant);

        let symbols = RefNode {
            name: "Symbols".to_string(),
            dirs: Vec::new(),
            files: vec![ref_file("valve.svg"), ref_file("pump.svg")],
        };
        let ref_tree = RefNode {
            name: "Ref".to_string(),
            dirs: vec![symbols],
            files: vec![ref_file("readme.txt")],
        };

        Arc::new(BackupIndex {
            source: PathBuf::from("D:/backups/plant.zip"),
            restored: PathBuf::from("D:/tmp/restore"),
            backup_root: PathBuf::from("D:/tmp/restore"),
            read_only: false,
            manifest: Some(ManifestSummary {
                name: Some("Plant backup".to_string()),
                ..ManifestSummary::default()
            }),
            files: vec![
                file("PlantData~2~711.zip", &["00/00/a.pid", "00/00/b.pid"]),
                file("RefData~4~1.zip", &[]),
            ],
            plant_groups: vec![root],
            drawings: vec![
                drawing("A-101", Some("D:/tmp/restore/00/00/a.pid")),
                drawing("B-202", Some("D:/tmp/restore/00/00/b.pid")),
                drawing("C-303", None),
            ],
            ref_tree,
            warnings: vec!["no usable Export.dmp".to_string()],
        })
    }

    fn ready() -> PidBackupBrowser {
        let mut state = PidBackupBrowser::default();
        assert_eq!(
            update(&mut state, PidBackupMsg::Loaded(Ok(index()))),
            Effect::None
        );
        state
    }

    fn items_of(uid: &str, item_type: &str) -> DrawingItems {
        DrawingItems {
            drawing_uid: "UID-1".to_string(),
            drawing_name: "A-101".to_string(),
            groups: vec![ItemGroup {
                item_type: item_type.to_string(),
                items: vec![DrawingItem {
                    uid: uid.to_string(),
                    item_type: item_type.to_string(),
                    class: Some("PIDInstrument".to_string()),
                    attributes: vec![ItemAttribute {
                        interface: "INamedInstrument".to_string(),
                        name: "MeasuredVariable".to_string(),
                        value: "T".to_string(),
                    }],
                }],
            }],
            item_count: 1,
        }
    }

    fn ids(rows: &[Row]) -> Vec<NodeId> {
        rows.iter().map(|row| row.id.clone()).collect()
    }

    #[test]
    fn ready_state_starts_with_the_root_and_its_three_groups() {
        let rows = visible_rows(&ready());
        assert_eq!(
            ids(&rows),
            vec![NodeId::Root, NodeId::Files, NodeId::Groups, NodeId::Ref]
        );
        assert_eq!(
            rows.iter().map(|row| row.depth).collect::<Vec<_>>(),
            vec![0, 1, 1, 1]
        );
        assert!(rows[0].expanded, "the root is always open");
        assert!(rows[1].expandable && rows[2].expandable && rows[3].expandable);
        assert!(!rows[1].expanded && !rows[2].expanded && !rows[3].expanded);
    }

    #[test]
    fn toggling_a_group_shows_its_children_one_level_deeper() {
        let mut state = ready();
        assert_eq!(
            update(&mut state, PidBackupMsg::Toggle(NodeId::Files)),
            Effect::None
        );
        let rows = visible_rows(&state);
        assert_eq!(
            ids(&rows),
            vec![
                NodeId::Root,
                NodeId::Files,
                NodeId::File(0),
                NodeId::File(1),
                NodeId::Groups,
                NodeId::Ref,
            ]
        );
        assert_eq!(rows[2].depth, 2);
        assert_eq!(rows[3].depth, 2);
        // The archive has members, the plain file has none.
        assert!(rows[2].expandable);
        assert!(!rows[3].expandable);

        assert_eq!(
            update(&mut state, PidBackupMsg::Toggle(NodeId::Files)),
            Effect::None
        );
        assert_eq!(
            ids(&visible_rows(&state)),
            vec![NodeId::Root, NodeId::Files, NodeId::Groups, NodeId::Ref]
        );
    }

    #[test]
    fn a_group_opens_its_children_and_drawings_in_order() {
        let mut state = ready();
        update(&mut state, PidBackupMsg::Toggle(NodeId::Groups));
        update(&mut state, PidBackupMsg::Toggle(NodeId::Group(vec![0])));
        let rows = visible_rows(&state);
        assert_eq!(
            ids(&rows),
            vec![
                NodeId::Root,
                NodeId::Files,
                NodeId::Groups,
                NodeId::Group(vec![0]),
                NodeId::Group(vec![0, 0]),
                NodeId::Ref,
            ]
        );
        assert_eq!(rows[4].depth, 3);

        // The group holding the drawings shows them after its subgroups.
        update(
            &mut state,
            PidBackupMsg::Toggle(NodeId::Group(vec![0, 0])),
        );
        let rows = visible_rows(&state);
        let tail = ids(&rows);
        assert_eq!(
            &tail[4..],
            &[
                NodeId::Group(vec![0, 0]),
                NodeId::Group(vec![0, 0, 0]),
                NodeId::Drawing(0),
                NodeId::Drawing(2),
                NodeId::Ref,
            ]
        );
        assert_eq!(rows[6].depth, 4);
        // The drawing with no file is greyed; the one with a file is not.
        assert!(!rows[6].disabled);
        assert!(rows[7].disabled);
    }

    #[test]
    fn a_collapsed_ref_tree_costs_three_rows() {
        let mut state = ready();
        update(&mut state, PidBackupMsg::Toggle(NodeId::Ref));
        assert_eq!(
            ids(&visible_rows(&state)),
            vec![
                NodeId::Root,
                NodeId::Files,
                NodeId::Groups,
                NodeId::Ref,
                NodeId::RefDir(vec![0]),
                NodeId::RefFile(vec![], 0),
            ]
        );
        update(&mut state, PidBackupMsg::Toggle(NodeId::RefDir(vec![0])));
        let rows = visible_rows(&state);
        assert_eq!(
            &ids(&rows)[4..],
            &[
                NodeId::RefDir(vec![0]),
                NodeId::RefFile(vec![0], 0),
                NodeId::RefFile(vec![0], 1),
                NodeId::RefFile(vec![], 0),
            ]
        );
        assert_eq!(rows[5].depth, 3);
    }

    #[test]
    fn a_filter_keeps_matching_drawings_and_their_ancestors() {
        let mut state = ready();
        update(&mut state, PidBackupMsg::Filter("b-2".to_string()));
        let rows = visible_rows(&state);
        assert_eq!(
            ids(&rows),
            vec![
                NodeId::Root,
                NodeId::Groups,
                NodeId::Group(vec![0]),
                NodeId::Group(vec![0, 0]),
                NodeId::Group(vec![0, 0, 0]),
                NodeId::Drawing(1),
            ],
            "only the branch holding B-202 stays, opened"
        );
        assert!(rows.iter().all(|row| row.id != NodeId::Files && row.id != NodeId::Ref));

        // Clearing the filter restores the untouched tree.
        update(&mut state, PidBackupMsg::Filter(String::new()));
        assert_eq!(
            ids(&visible_rows(&state)),
            vec![NodeId::Root, NodeId::Files, NodeId::Groups, NodeId::Ref]
        );
    }

    #[test]
    fn a_filter_that_matches_nothing_keeps_only_the_root_and_the_group() {
        let mut state = ready();
        update(&mut state, PidBackupMsg::Filter("zzz".to_string()));
        assert_eq!(
            ids(&visible_rows(&state)),
            vec![NodeId::Root, NodeId::Groups]
        );
    }

    #[test]
    fn activating_a_drawing_opens_its_file_and_a_missing_one_does_nothing() {
        let mut state = ready();
        assert_eq!(
            update(&mut state, PidBackupMsg::Activate(NodeId::Drawing(0))),
            Effect::OpenDrawing(PathBuf::from("D:/tmp/restore/00/00/a.pid"))
        );
        assert_eq!(
            update(&mut state, PidBackupMsg::Activate(NodeId::Drawing(2))),
            Effect::None
        );
        assert_eq!(
            update(&mut state, PidBackupMsg::Activate(NodeId::Root)),
            Effect::None
        );
        assert_eq!(
            update(&mut state, PidBackupMsg::Activate(NodeId::ItemType(0, 0))),
            Effect::None
        );
    }

    #[test]
    fn the_first_expand_of_a_drawing_asks_for_its_items_once() {
        let mut state = ready();
        assert_eq!(
            update(&mut state, PidBackupMsg::Toggle(NodeId::Drawing(0))),
            Effect::LoadItems(0, PathBuf::from("D:/tmp/restore/00/00/a.pid"))
        );
        assert!(matches!(state.items.get(&0), Some(ItemsState::Loading)));

        // Folding and unfolding again does not read the file a second time.
        update(&mut state, PidBackupMsg::Toggle(NodeId::Drawing(0)));
        assert_eq!(
            update(&mut state, PidBackupMsg::Toggle(NodeId::Drawing(0))),
            Effect::None
        );

        // A drawing with no file never reads anything.
        assert_eq!(
            update(&mut state, PidBackupMsg::Toggle(NodeId::Drawing(2))),
            Effect::None
        );
        assert!(!state.items.contains_key(&2));
    }

    #[test]
    fn loaded_items_show_their_type_groups_and_items() {
        let mut state = ready();
        update(&mut state, PidBackupMsg::Toggle(NodeId::Drawing(0)));
        update(
            &mut state,
            PidBackupMsg::ItemsLoaded(0, Ok(Arc::new(items_of("UID-1", "Instrument")))),
        );
        update(&mut state, PidBackupMsg::Toggle(NodeId::Groups));
        update(&mut state, PidBackupMsg::Toggle(NodeId::Group(vec![0])));
        update(
            &mut state,
            PidBackupMsg::Toggle(NodeId::Group(vec![0, 0])),
        );
        let rows = visible_rows(&state);
        let drawing_at = rows
            .iter()
            .position(|row| row.id == NodeId::Drawing(0))
            .expect("the drawing row");
        assert_eq!(rows[drawing_at + 1].id, NodeId::ItemType(0, 0));
        assert_eq!(rows[drawing_at + 1].depth, rows[drawing_at].depth + 1);
        assert!(
            rows[drawing_at + 1].label.contains("Instrument")
                && rows[drawing_at + 1].label.contains('1'),
            "the type group counts its items: {}",
            rows[drawing_at + 1].label
        );

        update(&mut state, PidBackupMsg::Toggle(NodeId::ItemType(0, 0)));
        let rows = visible_rows(&state);
        let type_at = rows
            .iter()
            .position(|row| row.id == NodeId::ItemType(0, 0))
            .expect("the type row");
        assert_eq!(rows[type_at + 1].id, NodeId::Item(0, 0, 0));
        assert!(rows[type_at + 1].label.contains("UID-1"));
    }

    #[test]
    fn a_failed_item_read_leaves_the_drawing_without_a_subtree() {
        let mut state = ready();
        update(&mut state, PidBackupMsg::Toggle(NodeId::Drawing(0)));
        update(
            &mut state,
            PidBackupMsg::ItemsLoaded(0, Err("cannot read".to_string())),
        );
        update(&mut state, PidBackupMsg::Toggle(NodeId::Groups));
        update(&mut state, PidBackupMsg::Toggle(NodeId::Group(vec![0])));
        update(
            &mut state,
            PidBackupMsg::Toggle(NodeId::Group(vec![0, 0])),
        );
        let rows = visible_rows(&state);
        let drawing_at = rows
            .iter()
            .position(|row| row.id == NodeId::Drawing(0))
            .expect("the drawing row");
        assert!(!rows[drawing_at].expandable);
        assert!(!rows
            .iter()
            .any(|row| matches!(row.id, NodeId::ItemType(..))));
        assert!(matches!(
            state.items.get(&0),
            Some(ItemsState::Failed(_))
        ));
    }

    #[test]
    fn selecting_sets_the_row_and_close_forgets_everything() {
        let mut state = ready();
        assert_eq!(
            update(&mut state, PidBackupMsg::Select(NodeId::File(0))),
            Effect::None
        );
        assert_eq!(state.selected, Some(NodeId::File(0)));
        update(&mut state, PidBackupMsg::Toggle(NodeId::Files));
        update(&mut state, PidBackupMsg::Toggle(NodeId::Drawing(0)));
        update(&mut state, PidBackupMsg::Filter("a".to_string()));

        assert_eq!(update(&mut state, PidBackupMsg::Close), Effect::None);
        assert!(matches!(state.load, LoadState::Idle));
        assert!(state.expanded.is_empty());
        assert_eq!(state.selected, None);
        assert!(state.items.is_empty());
        assert!(state.filter.is_empty());
        assert!(visible_rows(&state).is_empty());
    }

    #[test]
    fn a_failed_load_records_the_error_and_stays_idle() {
        let mut state = PidBackupBrowser::default();
        assert_eq!(
            update(
                &mut state,
                PidBackupMsg::Loaded(Err("no Manifest.txt".to_string()))
            ),
            Effect::None
        );
        assert!(matches!(state.load, LoadState::Idle));
        assert_eq!(state.error.as_deref(), Some("no Manifest.txt"));
        assert!(visible_rows(&state).is_empty());
    }

    #[test]
    fn a_picked_source_asks_for_its_index_and_a_cancelled_one_does_nothing() {
        let mut state = PidBackupBrowser::default();
        assert_eq!(
            update(&mut state, PidBackupMsg::SourcePicked(None)),
            Effect::None
        );
        assert!(matches!(state.load, LoadState::Idle));
        assert_eq!(
            update(
                &mut state,
                PidBackupMsg::SourcePicked(Some(PathBuf::from("D:/backups/a.zip")))
            ),
            Effect::Load(PathBuf::from("D:/backups/a.zip"))
        );
        assert!(matches!(state.load, LoadState::Loading(_)));
        assert_eq!(update(&mut state, PidBackupMsg::Open), Effect::PickSource);
    }

    #[test]
    fn refresh_and_close_ask_the_command_layer_for_the_work() {
        let mut state = ready();
        assert_eq!(
            update(&mut state, PidBackupMsg::Refresh),
            Effect::Refresh(PathBuf::from("D:/backups/plant.zip"))
        );
        // Refreshing an idle panel has no source to rebuild from.
        let mut idle = PidBackupBrowser::default();
        assert_eq!(update(&mut idle, PidBackupMsg::Refresh), Effect::None);
    }

    #[test]
    fn a_new_backup_replaces_the_old_tree() {
        let mut state = ready();
        update(&mut state, PidBackupMsg::Toggle(NodeId::Files));
        update(&mut state, PidBackupMsg::Select(NodeId::File(0)));
        update(&mut state, PidBackupMsg::Filter("a".to_string()));
        update(
            &mut state,
            PidBackupMsg::Loaded(Ok(Arc::new(BackupIndex {
                source: PathBuf::from("D:/backups/other.zip"),
                ..(*index()).clone()
            }))),
        );
        assert!(state.expanded.is_empty());
        assert_eq!(state.selected, None);
        assert!(state.filter.is_empty());
        assert_eq!(state.error, None);
    }
}
