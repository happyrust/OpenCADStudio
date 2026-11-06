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

use crate::app::Message;
use crate::io::pid_backup_browse::{BackupIndex, DrawingItems, GroupNode, RefNode};
use crate::t;
use crate::ui::dock::{Chrome, PanelId};
use iced::widget::{
    button, column, container, mouse_area, row, scrollable, text, text_input, Space,
};
use iced::{Element, Fill, Length, Theme};

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
        .unwrap_or_else(|| t!("Untitled backup").into_owned())
}

/// A plant group's name as the tree shows it.
///
/// The index files the drawings the drawing table does not mention under
/// [`crate::io::pid_backup_browse::UNREGISTERED_GROUP`] and the ones with no
/// folder at all under [`crate::io::pid_backup_browse::UNGROUPED`]; both are
/// fixed labels the tree translates, every other name is the plant's own.
fn group_label(name: &str) -> String {
    match name {
        crate::io::pid_backup_browse::UNREGISTERED_GROUP => t!("Unregistered").into_owned(),
        crate::io::pid_backup_browse::UNGROUPED => t!("(ungrouped)").into_owned(),
        other => other.to_string(),
    }
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
        label: group_label(&node.name),
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

impl PidBackupMsg {
    pub(crate) fn message(self) -> Message {
        Message::PidBackup(self)
    }
}

// ── View ──────────────────────────────────────────────────────────────────

/// `PosReport` id of the panel body, so a client can locate it on screen.
pub const PANEL_BOUNDS_ID: &str = "pid-backup-panel";

/// `PosReport` id prefix of the visible tree rows.
///
/// The suffix is the row's index among the visible rows: the tree is rebuilt
/// from [`visible_rows`] every frame, so a node's own identity (`NodeId`) can
/// name a row that is not on screen while its index always names the row a
/// screenshot shows.
pub const ROW_BOUNDS_PREFIX: &str = "pid-backup-row-";

/// Indentation one tree level costs, in pixels.
const INDENT: f32 = 12.0;

fn muted(theme: &Theme) -> iced::widget::text::Style {
    iced::widget::text::Style {
        color: Some(theme.palette().background.base.text.scale_alpha(0.6)),
    }
}

/// One key/value line of the detail table.
fn kv<'a>(key: String, value: String) -> Element<'a, Message> {
    row![
        text(key).size(10).width(Length::Fixed(96.0)).style(muted),
        text(value).size(10).width(Fill),
    ]
    .spacing(4)
    .align_y(iced::Top)
    .into()
}

/// A byte count as the panel shows it.
pub fn human_size(bytes: u64) -> String {
    const UNITS: [&str; 4] = ["B", "KB", "MB", "GB"];
    let mut value = bytes as f64;
    let mut unit = 0;
    while value >= 1024.0 && unit + 1 < UNITS.len() {
        value /= 1024.0;
        unit += 1;
    }
    if unit == 0 {
        format!("{bytes} B")
    } else {
        format!("{value:.1} {}", UNITS[unit])
    }
}

fn group_at<'a>(index: &'a BackupIndex, path: &[usize]) -> Option<&'a GroupNode> {
    let (first, rest) = path.split_first()?;
    let mut node = index.plant_groups.get(*first)?;
    for at in rest {
        node = node.children.get(*at)?;
    }
    Some(node)
}

fn ref_at<'a>(node: &'a RefNode, path: &[usize]) -> Option<&'a RefNode> {
    let mut node = node;
    for at in path {
        node = node.dirs.get(*at)?;
    }
    Some(node)
}

fn group_drawing_count(node: &GroupNode) -> usize {
    node.drawings.len()
        + node
            .children
            .iter()
            .map(group_drawing_count)
            .sum::<usize>()
}

/// One row of the tree: its indentation, its expand arrow and its label.
fn tree_row<'a>(row: &Row, at: usize) -> Element<'a, Message> {
    let arrow: Element<'a, Message> = if row.expandable {
        let icon = if row.expanded {
            crate::ui::icons::themed_arrow_down(10.0)
        } else {
            crate::ui::icons::themed_arrow_right(10.0)
        };
        button(icon)
            .on_press(PidBackupMsg::Toggle(row.id.clone()).message())
            .style(button::text)
            .padding(0)
            .into()
    } else {
        Space::new().width(Length::Fixed(12.0)).into()
    };
    let label = if row.disabled {
        text(row.label.clone()).size(11).style(muted)
    } else {
        text(row.label.clone()).size(11)
    };
    let line = row![
        Space::new().width(Length::Fixed(row.depth as f32 * INDENT)),
        arrow,
        label,
    ]
    .spacing(2)
    .align_y(iced::Center);
    // A press selects; the second press of a double click activates. The row
    // stays clickable when it is greyed (its details still explain why), and
    // `update` is what refuses to open a drawing with no file.
    let area = mouse_area(container(line).width(Fill).padding([1, 4]))
        .interaction(iced::mouse::Interaction::Pointer)
        .on_press(PidBackupMsg::Select(row.id.clone()).message())
        .on_double_click(PidBackupMsg::Activate(row.id.clone()).message());
    crate::ui::wrap_bar::PosReport::owned(format!("{ROW_BOUNDS_PREFIX}{at}"), area).into()
}

/// One line of the detail half, before it is laid out.
///
/// [`detail_rows`] projects these to `(key, value)` pairs so the content can
/// be asserted without a window; [`details`] draws each kind its own way.
enum DetailLine {
    /// A key/value field.
    Field(String, String),
    /// A plain muted line: a warning, or "registered but missing".
    Marker(String),
    /// A section heading, set off from the fields above it.
    Heading(String),
    /// The "open drawing" button for a drawing that has a file.
    OpenDrawing(String),
}

/// How [`details`] draws one line of the detail half.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DetailLineKind {
    /// A key/value field.
    Field,
    /// A plain muted line: a warning, or "registered but missing".
    Marker,
    /// A section heading, set off from the fields above it.
    Heading,
    /// The "open drawing" button for a drawing that has a file.
    OpenDrawing,
}

impl DetailLine {
    /// The `(key, value)` pair this line shows.
    fn into_pair(self) -> (String, String) {
        match self {
            DetailLine::Field(key, value) => (key, value),
            DetailLine::Marker(key) | DetailLine::Heading(key) | DetailLine::OpenDrawing(key) => {
                (key, String::new())
            }
        }
    }

    fn kind(&self) -> DetailLineKind {
        match self {
            DetailLine::Field(..) => DetailLineKind::Field,
            DetailLine::Marker(_) => DetailLineKind::Marker,
            DetailLine::Heading(_) => DetailLineKind::Heading,
            DetailLine::OpenDrawing(_) => DetailLineKind::OpenDrawing,
        }
    }
}

/// The lower half: what the selected node is.
///
/// Rendering only: the content and the shape of every line come from
/// [`detail_rows`] and [`detail_lines`].
fn details<'a>(state: &'a PidBackupBrowser) -> Element<'a, Message> {
    if state.index().is_none() {
        return text(t!("No backup is open.")).size(11).style(muted).into();
    }
    let Some(id) = state.selected.as_ref() else {
        return text(t!("Select a node to see its details."))
            .size(11)
            .style(muted)
            .into();
    };
    let kinds: Vec<DetailLineKind> = detail_lines(state, id)
        .into_iter()
        .map(|line| line.kind())
        .collect();
    let mut body = column![].spacing(2);
    for ((key, value), kind) in detail_rows(state, id).into_iter().zip(kinds) {
        body = match kind {
            DetailLineKind::Field => body.push(kv(key, value)),
            DetailLineKind::Marker => body.push(text(key).size(10).style(muted)),
            DetailLineKind::Heading => {
                body = body.push(Space::new().height(4));
                body.push(text(key).size(10))
            }
            DetailLineKind::OpenDrawing => body.push(
                button(text(key).size(11))
                    .on_press(PidBackupMsg::Activate(id.clone()).message())
                    .padding([3, 10]),
            ),
        };
    }
    body.into()
}

/// The lines the detail half shows for `id`, in display order.
///
/// Pure and free of `iced` types: every value is already localized and
/// formatted, so the content can be asserted without a window. An `id` the
/// index no longer holds, and an idle panel, yield no lines.
fn detail_lines(state: &PidBackupBrowser, id: &NodeId) -> Vec<DetailLine> {
    let Some(index) = state.index() else {
        return Vec::new();
    };
    let mut lines: Vec<DetailLine> = Vec::new();
    match id {
        NodeId::Root => {
            if let Some(manifest) = &index.manifest {
                for (key, value) in [
                    ("Backup name", manifest.name.clone()),
                    ("Backup version", manifest.version.clone()),
                    ("Backup type", manifest.backup_type.clone()),
                    ("Date created", manifest.date_created.clone()),
                    ("SPID", manifest.spid.clone()),
                ] {
                    if let Some(value) = value {
                        lines.push(DetailLine::Field(t!(key).into_owned(), value));
                    }
                }
            }
            lines.push(DetailLine::Field(
                t!("Backup source").into_owned(),
                index.source.display().to_string(),
            ));
            lines.push(DetailLine::Field(
                t!("Restored").into_owned(),
                index.restored.display().to_string(),
            ));
            // The marker keeps the key/value layout with an empty value: it
            // states a property of the backup, not a message.
            if index.read_only {
                lines.push(DetailLine::Field(
                    t!("Backup is read-only").into_owned(),
                    String::new(),
                ));
            }
            lines.push(DetailLine::Heading(t!("Warnings").into_owned()));
            if index.warnings.is_empty() {
                lines.push(DetailLine::Marker(t!("No warnings.").into_owned()));
            }
            for warning in &index.warnings {
                lines.push(DetailLine::Marker(warning.clone()));
            }
        }
        NodeId::Files => {
            lines.push(DetailLine::Field(
                t!("File count").into_owned(),
                index.files.len().to_string(),
            ));
        }
        NodeId::File(at) => {
            if let Some(file) = index.files.get(*at) {
                lines.push(DetailLine::Field(
                    t!("Backup file name").into_owned(),
                    file.name.clone(),
                ));
                if !file.format.is_empty() {
                    lines.push(DetailLine::Field(
                        t!("File format").into_owned(),
                        file.format.clone(),
                    ));
                }
                lines.push(DetailLine::Field(
                    t!("File size").into_owned(),
                    human_size(file.size),
                ));
                lines.push(DetailLine::Field(
                    t!("Members").into_owned(),
                    file.members.len().to_string(),
                ));
                if let Some(line) = &file.version_line {
                    lines.push(DetailLine::Field(
                        t!("Version line").into_owned(),
                        line.clone(),
                    ));
                }
                if file.missing {
                    lines.push(DetailLine::Marker(
                        t!("Registered but missing").into_owned(),
                    ));
                }
            }
        }
        NodeId::FileMember(file, member) => {
            if let Some(file) = index.files.get(*file) {
                lines.push(DetailLine::Field(
                    t!("Backup file name").into_owned(),
                    file.name.clone(),
                ));
                if let Some(name) = file.members.get(*member) {
                    lines.push(DetailLine::Field(t!("Member").into_owned(), name.clone()));
                }
            }
        }
        NodeId::Groups => {
            lines.push(DetailLine::Field(
                t!("Group count").into_owned(),
                index.plant_groups.len().to_string(),
            ));
            lines.push(DetailLine::Field(
                t!("Drawing count").into_owned(),
                index.drawings.len().to_string(),
            ));
        }
        NodeId::Group(path) => {
            if let Some(node) = group_at(index, path) {
                lines.push(DetailLine::Field(
                    t!("Group name").into_owned(),
                    group_label(&node.name),
                ));
                lines.push(DetailLine::Field(
                    t!("Group path").into_owned(),
                    node.path.clone(),
                ));
                lines.push(DetailLine::Field(
                    t!("Drawing count").into_owned(),
                    group_drawing_count(node).to_string(),
                ));
            }
        }
        NodeId::Drawing(drawing) => {
            if let Some(entry) = index.drawings.get(*drawing) {
                lines.push(DetailLine::Field(
                    t!("Drawing name").into_owned(),
                    entry.name.clone(),
                ));
                lines.push(DetailLine::Field(
                    t!("Group path").into_owned(),
                    entry.group_path.clone(),
                ));
                match &entry.pid_path {
                    Some(path) => {
                        lines.push(DetailLine::Field(
                            t!("File path").into_owned(),
                            path.display().to_string(),
                        ));
                        lines.push(DetailLine::OpenDrawing(
                            t!("Open drawing").into_owned(),
                        ));
                    }
                    // A row the plant knows about but the backup does not
                    // carry: there is nothing to open, so the panel says why.
                    None => {
                        lines.push(DetailLine::Marker(
                            t!("Registered but missing").into_owned(),
                        ));
                    }
                }
                // Every `T_DRAWING` column, empty values included: the panel
                // is the only place the backup's own metadata is visible.
                for (key, value) in &entry.meta {
                    lines.push(DetailLine::Field(key.clone(), value.clone()));
                }
            }
        }
        NodeId::ItemType(drawing, group) => {
            if let Some(ItemsState::Ready(items)) = state.items.get(drawing) {
                if let Some(group) = items.groups.get(*group) {
                    lines.push(DetailLine::Field(
                        t!("Item type").into_owned(),
                        group.item_type.clone(),
                    ));
                    lines.push(DetailLine::Field(
                        t!("Item count").into_owned(),
                        group.items.len().to_string(),
                    ));
                }
            }
        }
        NodeId::Item(drawing, group, item) => {
            let found = match state.items.get(drawing) {
                Some(ItemsState::Ready(items)) => items
                    .groups
                    .get(*group)
                    .and_then(|group| group.items.get(*item)),
                _ => None,
            };
            if let Some(entry) = found {
                lines.push(DetailLine::Field(
                    t!("Item UID").into_owned(),
                    entry.uid.clone(),
                ));
                lines.push(DetailLine::Field(
                    t!("Item type").into_owned(),
                    entry.item_type.clone(),
                ));
                if let Some(class) = &entry.class {
                    lines.push(DetailLine::Field(
                        t!("Item class").into_owned(),
                        class.clone(),
                    ));
                }
                lines.push(DetailLine::Heading(
                    t!("Item attributes").into_owned(),
                ));
                for attribute in &entry.attributes {
                    lines.push(DetailLine::Field(
                        format!("{}.{}", attribute.interface, attribute.name),
                        attribute.value.clone(),
                    ));
                }
            }
        }
        NodeId::Ref => {
            lines.push(DetailLine::Field(
                t!("Reference name").into_owned(),
                index.ref_tree.name.clone(),
            ));
            lines.push(DetailLine::Field(
                t!("Reference file count").into_owned(),
                index.ref_tree.file_count().to_string(),
            ));
        }
        NodeId::RefDir(path) => {
            if let Some(node) = ref_at(&index.ref_tree, path) {
                lines.push(DetailLine::Field(
                    t!("Reference name").into_owned(),
                    node.name.clone(),
                ));
                lines.push(DetailLine::Field(
                    t!("Reference file count").into_owned(),
                    node.file_count().to_string(),
                ));
            }
        }
        NodeId::RefFile(path, at) => {
            if let Some(file) = ref_at(&index.ref_tree, path).and_then(|node| node.files.get(*at)) {
                lines.push(DetailLine::Field(
                    t!("Reference file name").into_owned(),
                    file.name.clone(),
                ));
                lines.push(DetailLine::Field(
                    t!("File size").into_owned(),
                    human_size(file.size),
                ));
            }
        }
    }
    lines
}

/// The `(key, value)` pairs the detail half shows for `id`, in display order.
///
/// The testable projection of [`detail_lines`]: an empty value marks a line
/// that is not a plain field (a warning, a heading, "registered but missing",
/// the "open drawing" action).
pub(crate) fn detail_rows(state: &PidBackupBrowser, id: &NodeId) -> Vec<(String, String)> {
    detail_lines(state, id)
        .into_iter()
        .map(DetailLine::into_pair)
        .collect()
}

/// Build the docked panel.
pub fn view<'a>(state: &'a PidBackupBrowser, width: f32, chrome: Chrome) -> Element<'a, Message> {
    let title_bar = crate::ui::dock::title_bar(
        PanelId::PidBackup,
        t!("P&ID Backup").into_owned(),
        chrome,
    );

    let toolbar = row![
        button(text(t!("Open…")).size(11))
            .on_press(PidBackupMsg::Open.message())
            .padding([3, 8]),
        button(text(t!("Refresh backup")).size(11))
            .on_press(PidBackupMsg::Refresh.message())
            .padding([3, 8]),
        button(text(t!("Close backup")).size(11))
            .on_press(PidBackupMsg::Close.message())
            .padding([3, 8]),
    ]
    .spacing(4)
    .align_y(iced::Center);

    let search = text_input(t!("Search drawings…").as_ref(), &state.filter)
        .on_input(|text| PidBackupMsg::Filter(text).message())
        .size(11)
        .padding([3, 6]);

    // The upper half is the tree, built only from the rows the user opened;
    // the lower half is what the selected node is.
    let tree: Element<'a, Message> = match &state.load {
        LoadState::Idle => {
            let mut hint = column![
                text(t!("No backup is open.")).size(11).style(muted),
            ]
            .spacing(2);
            if let Some(error) = &state.error {
                hint = hint.push(text(error.clone()).size(10).style(muted));
            }
            container(hint).width(Fill).padding([4, 6]).into()
        }
        LoadState::Loading(source) => container(
            column![
                text(t!("Loading backup…")).size(11).style(muted),
                text(source.display().to_string()).size(10).style(muted),
            ]
            .spacing(2),
        )
        .width(Fill)
        .padding([4, 6])
        .into(),
        LoadState::Ready(_) => {
            let rows = visible_rows(state);
            let mut list = column![].spacing(1);
            for (at, row) in rows.iter().enumerate() {
                list = list.push(tree_row(row, at));
            }
            scrollable(list)
                .direction(scrollable::Direction::Vertical(
                    scrollable::Scrollbar::default(),
                ))
                .width(Fill)
                .height(Length::FillPortion(3))
                .into()
        }
    };

    let details = scrollable(details(state))
        .direction(scrollable::Direction::Vertical(
            scrollable::Scrollbar::default(),
        ))
        .width(Fill)
        .height(Length::FillPortion(2));

    let content = column![toolbar, search, tree, details]
        .spacing(4)
        .height(Fill);
    let reported = crate::ui::wrap_bar::PosReport::new(PANEL_BOUNDS_ID, content);
    crate::ui::dock::frame(title_bar, reported, width)
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
    fn a_group_row_shows_its_own_name_and_translates_the_two_fixed_ones() {
        // Every other name belongs to the plant and is shown as it is; the
        // two the index synthesises are labels the panel translates.
        assert_eq!(group_label("SQPlant"), "SQPlant");
        assert_eq!(group_label("00"), "00");
        assert_eq!(
            group_label(crate::io::pid_backup_browse::UNREGISTERED_GROUP),
            t!("Unregistered").into_owned()
        );
        assert_eq!(
            group_label(crate::io::pid_backup_browse::UNGROUPED),
            t!("(ungrouped)").into_owned()
        );
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

    /// A backup richer than [`index`]: every manifest field, a file the
    /// manifest declares but the backup does not carry, the two synthesised
    /// group names, and drawings with and without a file.
    fn detail_index() -> Arc<BackupIndex> {
        let mut plant = group("00", "SQPlant / 00", vec![0, 2]);
        plant.children.push(group("01", "SQPlant / 00 / 01", vec![1]));
        let mut root = group("SQPlant", "SQPlant", Vec::new());
        root.children.push(plant);
        let unregistered = group(
            crate::io::pid_backup_browse::UNREGISTERED_GROUP,
            crate::io::pid_backup_browse::UNREGISTERED_GROUP,
            Vec::new(),
        );
        let ungrouped = group(
            crate::io::pid_backup_browse::UNGROUPED,
            crate::io::pid_backup_browse::UNGROUPED,
            Vec::new(),
        );

        Arc::new(BackupIndex {
            source: PathBuf::from("D:/backups/plant.zip"),
            restored: PathBuf::from("D:/tmp/restore"),
            backup_root: PathBuf::from("D:/tmp/restore"),
            read_only: true,
            manifest: Some(ManifestSummary {
                name: Some("Plant backup".to_string()),
                version: Some("7".to_string()),
                date_created: Some("2026-01-02".to_string()),
                backup_type: Some("Full".to_string()),
                spid: Some("SQPlant".to_string()),
            }),
            files: vec![
                BackupFile {
                    name: "Export.dmp".to_string(),
                    kind: FileKind::ExportDump,
                    format: "Oracle exp (EXPORT:V12.01.00)".to_string(),
                    size: 2048,
                    members: Vec::new(),
                    version_line: Some("EXPORT:V12.01.00".to_string()),
                    missing: false,
                },
                BackupFile {
                    name: "PlantData~2~711.zip".to_string(),
                    kind: FileKind::PlantData,
                    format: "zip".to_string(),
                    size: 1024,
                    members: vec!["00/00/a.pid".to_string(), "00/00/b.pid".to_string()],
                    version_line: None,
                    missing: false,
                },
                BackupFile {
                    name: "RefData~4~824.zip".to_string(),
                    kind: FileKind::RefData,
                    format: "zip".to_string(),
                    size: 0,
                    members: Vec::new(),
                    version_line: None,
                    missing: true,
                },
            ],
            plant_groups: vec![root, unregistered, ungrouped],
            drawings: vec![
                DrawingEntry {
                    name: "A-101".to_string(),
                    pid_path: Some(PathBuf::from("D:/tmp/restore/00/00/a.pid")),
                    group_path: "SQPlant / 00".to_string(),
                    meta: vec![
                        ("id".to_string(), "1".to_string()),
                        ("PATH".to_string(), "00/00/a.pid".to_string()),
                    ],
                },
                DrawingEntry {
                    name: "B-202".to_string(),
                    pid_path: Some(PathBuf::from("D:/tmp/restore/00/00/01/b.pid")),
                    group_path: "SQPlant / 00 / 01".to_string(),
                    meta: vec![("id".to_string(), "2".to_string())],
                },
                DrawingEntry {
                    name: "C-303".to_string(),
                    pid_path: None,
                    group_path: "SQPlant / 00".to_string(),
                    meta: vec![("id".to_string(), "3".to_string())],
                },
            ],
            ref_tree: RefNode {
                name: "Ref".to_string(),
                dirs: vec![RefNode {
                    name: "Symbols".to_string(),
                    dirs: Vec::new(),
                    files: vec![
                        RefFile {
                            name: "pump.svg".to_string(),
                            size: 4096,
                        },
                        RefFile {
                            name: "valve.svg".to_string(),
                            size: 2048,
                        },
                    ],
                }],
                files: vec![RefFile {
                    name: "readme.txt".to_string(),
                    size: 12,
                }],
            },
            warnings: vec![
                "no usable Export.dmp".to_string(),
                "one declared file is missing".to_string(),
            ],
        })
    }

    fn detail_state() -> PidBackupBrowser {
        let mut state = PidBackupBrowser::default();
        assert_eq!(
            update(&mut state, PidBackupMsg::Loaded(Ok(detail_index()))),
            Effect::None
        );
        state
    }

    /// The detail rows as the panel shows them: `(key, value)` pairs.
    fn detail_rows_of(state: &PidBackupBrowser, id: &NodeId) -> Vec<(String, String)> {
        detail_rows(state, id)
    }

    fn detail_keys(state: &PidBackupBrowser, id: &NodeId) -> Vec<String> {
        detail_rows_of(state, id)
            .into_iter()
            .map(|(key, _)| key)
            .collect()
    }

    /// Only the rows that carry a value: the plain `(key, value)` fields.
    fn detail_fields(state: &PidBackupBrowser, id: &NodeId) -> Vec<(String, String)> {
        detail_rows_of(state, id)
            .into_iter()
            .filter(|(_, value)| !value.is_empty())
            .collect()
    }

    /// A literal value as the detail rows carry it.
    fn plain(value: &str) -> String {
        value.to_string()
    }

    #[test]
    fn the_root_details_list_the_manifest_read_only_marker_and_every_warning() {
        let state = detail_state();
        assert_eq!(
            detail_fields(&state, &NodeId::Root),
            vec![
                (t!("Backup name").into_owned(), plain("Plant backup")),
                (t!("Backup version").into_owned(), plain("7")),
                (t!("Backup type").into_owned(), plain("Full")),
                (t!("Date created").into_owned(), plain("2026-01-02")),
                (t!("SPID").into_owned(), plain("SQPlant")),
                (t!("Backup source").into_owned(), plain("D:/backups/plant.zip")),
                (t!("Restored").into_owned(), plain("D:/tmp/restore")),
            ],
            "the manifest fields come first, then where the backup came from"
        );
        assert_eq!(
            detail_keys(&state, &NodeId::Root),
            vec![
                t!("Backup name").into_owned(),
                t!("Backup version").into_owned(),
                t!("Backup type").into_owned(),
                t!("Date created").into_owned(),
                t!("SPID").into_owned(),
                t!("Backup source").into_owned(),
                t!("Restored").into_owned(),
                t!("Backup is read-only").into_owned(),
                t!("Warnings").into_owned(),
                plain("no usable Export.dmp"),
                plain("one declared file is missing"),
            ],
            "the read-only marker and every warning follow in order"
        );
    }

    #[test]
    fn a_root_without_warnings_says_so() {
        let mut state = detail_state();
        update(
            &mut state,
            PidBackupMsg::Loaded(Ok(Arc::new(BackupIndex {
                warnings: Vec::new(),
                read_only: false,
                ..(*detail_index()).clone()
            }))),
        );
        let keys = detail_keys(&state, &NodeId::Root);
        assert!(keys.contains(&t!("No warnings.").into_owned()));
        assert!(!keys.contains(&t!("Backup is read-only").into_owned()));
    }

    #[test]
    fn the_files_group_details_count_the_backup_files() {
        let state = detail_state();
        assert_eq!(
            detail_rows_of(&state, &NodeId::Files),
            vec![(t!("File count").into_owned(), plain("3"))]
        );
    }

    #[test]
    fn a_file_details_show_its_name_format_size_members_and_version_line() {
        let state = detail_state();
        assert_eq!(
            detail_rows_of(&state, &NodeId::File(0)),
            vec![
                (t!("Backup file name").into_owned(), plain("Export.dmp")),
                (
                    t!("File format").into_owned(),
                    plain("Oracle exp (EXPORT:V12.01.00)")
                ),
                (t!("File size").into_owned(), human_size(2048)),
                (t!("Members").into_owned(), plain("0")),
                (
                    t!("Version line").into_owned(),
                    plain("EXPORT:V12.01.00")
                ),
            ],
            "the dump carries its EXPORT line"
        );
    }

    #[test]
    fn an_archive_file_details_count_its_members() {
        let state = detail_state();
        assert_eq!(
            detail_rows_of(&state, &NodeId::File(1)),
            vec![
                (
                    t!("Backup file name").into_owned(),
                    plain("PlantData~2~711.zip")
                ),
                (t!("File format").into_owned(), plain("zip")),
                (t!("File size").into_owned(), human_size(1024)),
                (t!("Members").into_owned(), plain("2")),
            ]
        );
    }

    #[test]
    fn a_file_the_backup_does_not_carry_says_it_is_registered_but_missing() {
        let state = detail_state();
        let rows = detail_rows_of(&state, &NodeId::File(2));
        assert_eq!(
            rows,
            vec![
                (
                    t!("Backup file name").into_owned(),
                    plain("RefData~4~824.zip")
                ),
                (t!("File format").into_owned(), plain("zip")),
                (t!("File size").into_owned(), human_size(0)),
                (t!("Members").into_owned(), plain("0")),
                (t!("Registered but missing").into_owned(), String::new()),
            ]
        );
    }

    #[test]
    fn a_file_member_details_name_its_file_and_the_member() {
        let state = detail_state();
        assert_eq!(
            detail_rows_of(&state, &NodeId::FileMember(1, 1)),
            vec![
                (
                    t!("Backup file name").into_owned(),
                    plain("PlantData~2~711.zip")
                ),
                (t!("Member").into_owned(), plain("00/00/b.pid")),
            ]
        );
    }

    #[test]
    fn the_groups_group_details_count_groups_and_drawings() {
        let state = detail_state();
        assert_eq!(
            detail_rows_of(&state, &NodeId::Groups),
            vec![
                (t!("Group count").into_owned(), plain("3")),
                (t!("Drawing count").into_owned(), plain("3")),
            ]
        );
    }

    #[test]
    fn a_group_details_name_its_path_and_its_drawing_subtree() {
        let state = detail_state();
        assert_eq!(
            detail_rows_of(&state, &NodeId::Group(vec![0, 0])),
            vec![
                (t!("Group name").into_owned(), plain("00")),
                (t!("Group path").into_owned(), plain("SQPlant / 00")),
                (t!("Drawing count").into_owned(), plain("3")),
            ],
            "the count covers the group's own drawings and its child's"
        );
        assert_eq!(
            detail_rows_of(&state, &NodeId::Group(vec![0])),
            vec![
                (t!("Group name").into_owned(), plain("SQPlant")),
                (t!("Group path").into_owned(), plain("SQPlant")),
                (t!("Drawing count").into_owned(), plain("3")),
            ],
            "the parent counts the drawings of its whole subtree"
        );
    }

    #[test]
    fn a_group_details_translate_the_two_names_the_index_synthesises() {
        let state = detail_state();
        assert_eq!(
            detail_fields(&state, &NodeId::Group(vec![1]))[0],
            (
                t!("Group name").into_owned(),
                t!("Unregistered").into_owned()
            )
        );
        assert_eq!(
            detail_fields(&state, &NodeId::Group(vec![2]))[0],
            (
                t!("Group name").into_owned(),
                t!("(ungrouped)").into_owned()
            )
        );
    }

    #[test]
    fn a_drawing_details_show_its_file_path_metadata_and_open_action() {
        let state = detail_state();
        assert_eq!(
            detail_rows_of(&state, &NodeId::Drawing(0)),
            vec![
                (t!("Drawing name").into_owned(), plain("A-101")),
                (t!("Group path").into_owned(), plain("SQPlant / 00")),
                (
                    t!("File path").into_owned(),
                    plain("D:/tmp/restore/00/00/a.pid")
                ),
                (t!("Open drawing").into_owned(), String::new()),
                (plain("id"), plain("1")),
                (plain("PATH"), plain("00/00/a.pid")),
            ],
            "the file path, the open action, then every T_DRAWING column"
        );
    }

    #[test]
    fn a_missing_drawing_details_say_why_and_offer_no_open_action() {
        let state = detail_state();
        assert_eq!(
            detail_rows_of(&state, &NodeId::Drawing(2)),
            vec![
                (t!("Drawing name").into_owned(), plain("C-303")),
                (t!("Group path").into_owned(), plain("SQPlant / 00")),
                (t!("Registered but missing").into_owned(), String::new()),
                (plain("id"), plain("3")),
            ]
        );
        let keys = detail_keys(&state, &NodeId::Drawing(2));
        assert!(!keys.contains(&t!("Open drawing").into_owned()));
        assert!(!keys.contains(&t!("File path").into_owned()));
    }

    #[test]
    fn an_item_type_details_name_it_and_count_its_items() {
        let mut state = detail_state();
        update(
            &mut state,
            PidBackupMsg::ItemsLoaded(0, Ok(Arc::new(items_of("UID-1", "Instrument")))),
        );
        assert_eq!(
            detail_rows_of(&state, &NodeId::ItemType(0, 0)),
            vec![
                (t!("Item type").into_owned(), plain("Instrument")),
                (t!("Item count").into_owned(), plain("1")),
            ]
        );
    }

    #[test]
    fn an_item_details_show_its_uid_type_class_and_every_attribute() {
        let mut state = detail_state();
        update(
            &mut state,
            PidBackupMsg::ItemsLoaded(0, Ok(Arc::new(items_of("UID-1", "Instrument")))),
        );
        assert_eq!(
            detail_rows_of(&state, &NodeId::Item(0, 0, 0)),
            vec![
                (t!("Item UID").into_owned(), plain("UID-1")),
                (t!("Item type").into_owned(), plain("Instrument")),
                (t!("Item class").into_owned(), plain("PIDInstrument")),
                (t!("Item attributes").into_owned(), String::new()),
                (
                    plain("INamedInstrument.MeasuredVariable"),
                    plain("T")
                ),
            ]
        );
    }

    #[test]
    fn the_reference_group_details_count_every_reference_file() {
        let state = detail_state();
        assert_eq!(
            detail_rows_of(&state, &NodeId::Ref),
            vec![
                (t!("Reference name").into_owned(), plain("Ref")),
                (t!("Reference file count").into_owned(), plain("3")),
            ],
            "the count covers the whole Ref subtree"
        );
    }

    #[test]
    fn a_reference_directory_details_name_it_and_count_its_subtree() {
        let state = detail_state();
        assert_eq!(
            detail_rows_of(&state, &NodeId::RefDir(vec![0])),
            vec![
                (t!("Reference name").into_owned(), plain("Symbols")),
                (t!("Reference file count").into_owned(), plain("2")),
            ]
        );
    }

    #[test]
    fn a_reference_file_details_show_its_name_and_size() {
        let state = detail_state();
        assert_eq!(
            detail_rows_of(&state, &NodeId::RefFile(vec![], 0)),
            vec![
                (t!("Reference file name").into_owned(), plain("readme.txt")),
                (t!("File size").into_owned(), human_size(12)),
            ]
        );
        assert_eq!(
            detail_rows_of(&state, &NodeId::RefFile(vec![0], 1)),
            vec![
                (t!("Reference file name").into_owned(), plain("valve.svg")),
                (t!("File size").into_owned(), human_size(2048)),
            ]
        );
    }

    #[test]
    fn a_node_out_of_range_and_an_idle_panel_have_no_details() {
        let state = detail_state();
        assert!(detail_rows_of(&state, &NodeId::File(99)).is_empty());
        assert!(detail_rows_of(&state, &NodeId::Group(vec![9])).is_empty());
        assert!(detail_rows_of(&state, &NodeId::RefDir(vec![7])).is_empty());
        assert!(detail_rows_of(&state, &NodeId::Item(0, 0, 0)).is_empty());
        assert!(detail_rows_of(&PidBackupBrowser::default(), &NodeId::Root).is_empty());
    }
}
