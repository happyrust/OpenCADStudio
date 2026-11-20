//! 项目索引（SPEC-pid-project-index）：先是一张图的摘要。
//!
//! [`summarize`] 只读一张图：带 `PID_AUTHOR` 记录的实体按 §3.1 的归类表
//! 归成对象，符号实例的每个端口各记一条。同一张图摘要两次得到逐字节相同的
//! JSON，摘要不改图。管段、信号线两端、连接符和端口的连接关系是 IX-2 的事。

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};

use document::entities::Insert;
use document::types::Vector3;
use document::{Document, EntityType, Handle};
use serde::{Deserialize, Serialize};

use super::persist::PidAuthor;
use super::propagate::split_props;
use super::symbols::{placed_ports, PortDir, SymbolLibrary};
use super::topology::{
    handle_text, parse_handle, TopologyTable, FITTING_BREAK, ROLE_CONNECTOR_IN, ROLE_CONNECTOR_OUT,
    ROLE_FITTING, ROLE_PIPE_LINE, ROLE_PIPE_RUN, ROLE_PIPE_SEGMENT, ROLE_SIGNAL_LINE, TYPE_FITTING,
    TYPE_PIPE_LINE, TYPE_PIPE_RUN, TYPE_SIGNAL_LINE, TYPE_VALVE,
};
use super::type_lib::TypeLib;

/// 摘要格式的版本。
pub const SUMMARY_VERSION: u32 = 1;

/// 两点算同一处的默认距离（图纸单位），与协同里判「同一处」的口径一致。
pub const DEFAULT_TOLERANCE: f64 = 1e-6;

/// 设备类型树的根：它和它下面的类型都算设备。
const EQUIPMENT: &str = "EQUIPMENT";

const INSTRUMENT: &str = "INSTRUMENT";

/// 以它开头的类型码都算阀门。
const VALVE_PREFIX: &str = "VALVE_";

/// 类型库里没有时用的几个内置类型的名字。
const BUILTIN_LABELS: [(&str, &str); 7] = [
    (TYPE_VALVE, "阀门"),
    ("VALVE_GATE", "闸阀"),
    (INSTRUMENT, "仪表"),
    (TYPE_PIPE_LINE, "管线"),
    (TYPE_PIPE_RUN, "管路"),
    (TYPE_SIGNAL_LINE, "信号线"),
    (TYPE_FITTING, "管件"),
];

/// 方格边长的下限，容差给成 0 或负数时格子也不至于退化。
const MIN_CELL: f64 = 1e-9;

/// 图纸是从哪里读的：打开着的标签页，还是磁盘上的文件。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SummarySource {
    Open,
    File,
}

/// 文件大小与修改时间（Unix 秒），用来判断文件变没变。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Fingerprint {
    pub size: u64,
    pub modified_at: i64,
}

/// 图纸内容以外、摘要要带上的几项，由调用方给。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SummaryMeta {
    pub drawing_id: String,
    pub file: Option<String>,
    pub pbs: Option<String>,
    pub source: SummarySource,
    pub fingerprint: Option<Fingerprint>,
}

/// 一张图的摘要（§3.1）。字段顺序就是 JSON 里的顺序。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DrawingSummary {
    pub version: u32,
    pub drawing_id: String,
    pub file: Option<String>,
    pub source: SummarySource,
    pub fingerprint: Option<Fingerprint>,
    pub pbs: Option<String>,
    pub length_unit: String,
    pub objects: Vec<SummaryObject>,
    pub ports: Vec<SummaryPort>,
}

/// 对象的种类，即 §3.1 归类表里的 `kind`。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ObjectKind {
    #[serde(rename = "设备")]
    Equipment,
    #[serde(rename = "阀门")]
    Valve,
    #[serde(rename = "仪表")]
    Instrument,
    #[serde(rename = "管件")]
    Fitting,
    #[serde(rename = "打断符")]
    Break,
    #[serde(rename = "管线")]
    PipeLine,
    #[serde(rename = "管路")]
    PipeRun,
    #[serde(rename = "信号线")]
    SignalLine,
    #[serde(rename = "其他")]
    Other,
}

impl ObjectKind {
    /// 这种对象的位号是不是它自己的：管件和打断符记的是所在管路的位号。
    #[must_use]
    pub fn has_own_tag(self) -> bool {
        !matches!(self, Self::Fitting | Self::Break)
    }
}

/// 一个对象（§3.1 对象表）。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SummaryObject {
    pub kind: ObjectKind,
    pub type_code: String,
    pub type_label: String,
    pub tag: String,
    pub own_tag: bool,
    pub tag_source: String,
    pub tag_status: String,
    pub symbol: Option<String>,
    pub symbol_known: bool,
    pub props: BTreeMap<String, String>,
    pub overrides: Vec<String>,
    pub owner: Option<String>,
    pub layer: String,
    pub at: [f64; 2],
    pub bbox: [f64; 4],
    pub handle: String,
}

/// 符号实例的一个端口（§3.1 `ports`）。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SummaryPort {
    pub owner_kind: ObjectKind,
    pub type_code: String,
    pub tag: String,
    pub port: String,
    pub direction: Option<PortDir>,
    pub at: [f64; 2],
    pub joined: bool,
}

/// 摘要一张图（§3.1）。只读，不改 `doc`。
///
/// `tolerance` 是两点算同一处的距离（图纸单位），平时用 [`DEFAULT_TOLERANCE`]。
#[must_use]
pub fn summarize(
    doc: &Document,
    library: &SymbolLibrary,
    meta: SummaryMeta,
    tolerance: f64,
) -> DrawingSummary {
    let types = TypeLib::embedded();
    let mut table = TopologyTable::load(doc);
    table.prune(doc);
    let records = Records::new(&table);
    let mut objects: Vec<(u64, SummaryObject)> = Vec::new();
    let mut ports: Vec<PortDraft> = Vec::new();
    let mut pipe_lines: BTreeMap<String, Vec<Piece>> = BTreeMap::new();
    let mut signal_lines: BTreeMap<String, Vec<Piece>> = BTreeMap::new();
    let mut run_boxes: HashMap<&str, [f64; 4]> = HashMap::new();
    let mut ends: Vec<[f64; 2]> = Vec::new();
    for entity in doc.entities() {
        let handle = entity.common().handle;
        let Some(author) = PidAuthor::load(doc, handle) else {
            continue;
        };
        let key = handle_text(handle);
        let is_break = records.breaks.contains(key.as_str());
        let class = classify(&author.topology, &author.type_code, is_break, &types);
        let (at, bbox) = placement(doc, entity);
        match class {
            Class::Connector => {}
            Class::Segment => {
                ends.extend(line_ends(entity));
                if let Some(run) = records.segment_run.get(key.as_str()) {
                    run_boxes
                        .entry(*run)
                        .and_modify(|merged| *merged = merge(*merged, bbox))
                        .or_insert(bbox);
                }
            }
            Class::PipeLine | Class::SignalLine => {
                ends.extend(line_ends(entity));
                let groups = if class == Class::PipeLine {
                    &mut pipe_lines
                } else {
                    &mut signal_lines
                };
                let layer = entity.common().layer.clone();
                groups.entry(author.tag.clone()).or_default().push(Piece {
                    handle,
                    author,
                    layer,
                    at,
                    bbox,
                });
            }
            Class::Object(kind) => {
                let owner = match kind {
                    ObjectKind::PipeRun => records.run_line.get(key.as_str()),
                    ObjectKind::Valve | ObjectKind::Fitting | ObjectKind::Break => {
                        records.fitting_run.get(key.as_str())
                    }
                    _ => None,
                };
                let insert = match entity {
                    EntityType::Insert(insert) => Some(insert),
                    _ => None,
                };
                let (props, overrides) = split_props(&author.props);
                let object = SummaryObject {
                    kind,
                    type_label: type_label(&types, &author.type_code),
                    type_code: author.type_code,
                    tag: author.tag,
                    own_tag: kind.has_own_tag(),
                    tag_source: author.tag_source,
                    tag_status: author.tag_status,
                    symbol: insert.map(|insert| insert.block_name.clone()),
                    symbol_known: insert
                        .is_none_or(|insert| library.symbol(&insert.block_name).is_some()),
                    props,
                    overrides: overrides.into_iter().collect(),
                    owner: owner.map(|tag| (*tag).to_string()),
                    layer: entity.common().layer.clone(),
                    at,
                    bbox,
                    handle: key,
                };
                if let Some(insert) = insert {
                    ports.extend(ports_of(doc, library, insert, &object));
                }
                objects.push((handle.value(), object));
            }
        }
    }
    for (_, object) in objects
        .iter_mut()
        .filter(|(_, object)| object.kind == ObjectKind::PipeRun)
    {
        if let Some(bbox) = run_boxes.get(object.tag.as_str()) {
            object.bbox = *bbox;
        }
    }
    for (tag, pieces) in pipe_lines {
        let main = records.line_handle.get(tag.as_str()).copied();
        objects.push(pipe_line_object(pieces, main, &types));
    }
    for pieces in signal_lines.into_values() {
        objects.push(signal_line_object(pieces, &types));
    }
    join_ports(&mut ports, &ends, &bound_ends(&table), tolerance);
    objects.sort_by(|(left_handle, left), (right_handle, right)| {
        left.type_code
            .cmp(&right.type_code)
            .then_with(|| left.tag.cmp(&right.tag))
            .then_with(|| compare_at(left.at, right.at))
            .then_with(|| left_handle.cmp(right_handle))
    });
    let mut ports: Vec<SummaryPort> = ports.into_iter().map(|draft| draft.port).collect();
    ports.sort_by(|left, right| {
        left.tag
            .cmp(&right.tag)
            .then_with(|| compare_at(left.at, right.at))
            .then_with(|| left.type_code.cmp(&right.type_code))
            .then_with(|| left.port.cmp(&right.port))
    });
    DrawingSummary {
        version: SUMMARY_VERSION,
        drawing_id: meta.drawing_id,
        file: meta.file,
        source: meta.source,
        fingerprint: meta.fingerprint,
        pbs: meta.pbs,
        length_unit: length_unit(doc.header.insertion_units).to_string(),
        objects: objects.into_iter().map(|(_, object)| object).collect(),
        ports,
    }
}

/// 拓扑表里按句柄、位号查的几样。
struct Records<'a> {
    /// 管路句柄 → 所在管线的位号。
    run_line: HashMap<&'a str, &'a str>,
    /// 管段句柄 → 所在管路的位号。
    segment_run: HashMap<&'a str, &'a str>,
    /// 阀门、管件、打断符的句柄 → 所在管路的位号。
    fitting_run: HashMap<&'a str, &'a str>,
    /// 打断符的句柄。
    breaks: HashSet<&'a str>,
    /// 管线位号 → 拓扑表记的那根线的句柄。
    line_handle: HashMap<&'a str, &'a str>,
}

impl<'a> Records<'a> {
    fn new(table: &'a TopologyTable) -> Self {
        Self {
            run_line: table
                .pipe_runs
                .iter()
                .map(|run| (run.handle.as_str(), run.line_tag.as_str()))
                .collect(),
            segment_run: table
                .pipe_segments
                .iter()
                .map(|segment| (segment.handle.as_str(), segment.run_tag.as_str()))
                .collect(),
            fitting_run: table
                .fittings
                .iter()
                .map(|fitting| (fitting.handle.as_str(), fitting.run_tag.as_str()))
                .collect(),
            breaks: table
                .fittings
                .iter()
                .filter(|fitting| fitting.fitting_type == FITTING_BREAK)
                .map(|fitting| fitting.handle.as_str())
                .collect(),
            line_handle: table
                .pipe_lines
                .iter()
                .map(|line| (line.tag.as_str(), line.handle.as_str()))
                .collect(),
        }
    }
}

/// 管线或信号线的一根线。
struct Piece {
    handle: Handle,
    author: PidAuthor,
    layer: String,
    at: [f64; 2],
    bbox: [f64; 4],
}

/// 一条管线：位号相同的几根线合成一个对象。拓扑表记的那根线是主线（不在
/// 就取句柄最小的），属性、定位点和句柄都取主线的。
fn pipe_line_object(
    mut pieces: Vec<Piece>,
    main: Option<&str>,
    types: &TypeLib,
) -> (u64, SummaryObject) {
    pieces.sort_by_key(|piece| piece.handle.value());
    let bbox = merged(&pieces);
    let index = main
        .and_then(|main| {
            pieces
                .iter()
                .position(|piece| handle_text(piece.handle) == main)
        })
        .unwrap_or(0);
    let main = pieces.swap_remove(index);
    let (props, overrides) = split_props(&main.author.props);
    composite(ObjectKind::PipeLine, main, props, overrides, bbox, types)
}

/// 一条信号线：位号相同的几根线合成一个对象。属性逐项取第一个非空值，
/// 句柄取最小的。
fn signal_line_object(mut pieces: Vec<Piece>, types: &TypeLib) -> (u64, SummaryObject) {
    pieces.sort_by_key(|piece| piece.handle.value());
    let bbox = merged(&pieces);
    let mut props: BTreeMap<String, String> = BTreeMap::new();
    let mut overrides = BTreeSet::new();
    for piece in &pieces {
        let (values, marks) = split_props(&piece.author.props);
        for (field, value) in values {
            let slot = props.entry(field).or_default();
            if slot.is_empty() {
                *slot = value;
            }
        }
        overrides.extend(marks);
    }
    let main = pieces.swap_remove(0);
    composite(ObjectKind::SignalLine, main, props, overrides, bbox, types)
}

/// 几根线合成的对象：位号、类型、图层、定位点和句柄取主线的。
fn composite(
    kind: ObjectKind,
    main: Piece,
    props: BTreeMap<String, String>,
    overrides: BTreeSet<String>,
    bbox: [f64; 4],
    types: &TypeLib,
) -> (u64, SummaryObject) {
    let object = SummaryObject {
        kind,
        type_label: type_label(types, &main.author.type_code),
        type_code: main.author.type_code,
        tag: main.author.tag,
        own_tag: true,
        tag_source: main.author.tag_source,
        tag_status: main.author.tag_status,
        symbol: None,
        symbol_known: true,
        props,
        overrides: overrides.into_iter().collect(),
        owner: None,
        layer: main.layer,
        at: main.at,
        bbox,
        handle: handle_text(main.handle),
    };
    (main.handle.value(), object)
}

fn merged(pieces: &[Piece]) -> [f64; 4] {
    pieces
        .iter()
        .map(|piece| piece.bbox)
        .reduce(merge)
        .unwrap_or_default()
}

/// 还没判 `joined` 的端口，带着它所在符号实例的句柄。
struct PortDraft {
    insert: Handle,
    port: SummaryPort,
}

/// 一个符号实例的端口：名字和流向按符号库里定义的顺序，位置是摆好后的世界
/// 坐标；库里没有这个块时按顺序叫 `#1`、`#2`，流向空着。
fn ports_of(
    doc: &Document,
    library: &SymbolLibrary,
    insert: &Insert,
    owner: &SummaryObject,
) -> Vec<PortDraft> {
    let defs = library
        .symbol(&insert.block_name)
        .map(|symbol| symbol.ports.as_slice())
        .unwrap_or_default();
    placed_ports(doc, insert)
        .into_iter()
        .enumerate()
        .map(|(index, (x, y))| {
            let def = defs.get(index);
            PortDraft {
                insert: insert.common.handle,
                port: SummaryPort {
                    owner_kind: owner.kind,
                    type_code: owner.type_code.clone(),
                    tag: owner.tag.clone(),
                    port: def.map_or_else(|| format!("#{}", index + 1), |def| def.name.clone()),
                    direction: def.map(|def| def.direction),
                    at: [x, y],
                    joined: false,
                },
            }
        })
        .collect()
}

/// 拓扑表里连着符号实例的管段端点（`peer` 是 `<句柄>:<端口名>`）：句柄和端点坐标。
fn bound_ends(table: &TopologyTable) -> Vec<(u64, [f64; 2])> {
    table
        .pipe_segments
        .iter()
        .flat_map(|segment| [&segment.start, &segment.end])
        .filter_map(|end| {
            let (handle, _) = end.peer.as_deref()?.split_once(':')?;
            Some((parse_handle(handle)?.value(), end.point))
        })
        .collect()
}

/// 端口接没接上：管段、管线自己的线或信号线的端点落在它上面，别的符号的端口
/// 和它重合，或者拓扑表里有管段端点连着它的符号。管件两侧的管段让开了一道缝，
/// 端点不在端口上，表里的端口名（「连接点1」）又不是符号库的，所以算离那个端点最近的
/// 端口。
fn join_ports(
    ports: &mut [PortDraft],
    ends: &[[f64; 2]],
    bound: &[(u64, [f64; 2])],
    tolerance: f64,
) {
    let grid = PointGrid::new(ports.iter().map(|draft| draft.port.at), tolerance);
    let mut joined = vec![false; ports.len()];
    for end in ends {
        for index in grid.near(*end) {
            joined[index] = true;
        }
    }
    for (index, draft) in ports.iter().enumerate() {
        if grid
            .near(draft.port.at)
            .any(|other| ports[other].insert != draft.insert)
        {
            joined[index] = true;
        }
    }
    let mut by_insert: HashMap<u64, Vec<usize>> = HashMap::new();
    for (index, draft) in ports.iter().enumerate() {
        by_insert
            .entry(draft.insert.value())
            .or_default()
            .push(index);
    }
    for (insert, end) in bound {
        let distance = |index: usize| {
            let at = ports[index].port.at;
            (at[0] - end[0]).hypot(at[1] - end[1])
        };
        let nearest = by_insert.get(insert).and_then(|indices| {
            indices
                .iter()
                .copied()
                .min_by(|left, right| distance(*left).total_cmp(&distance(*right)))
        });
        if let Some(index) = nearest {
            joined[index] = true;
        }
    }
    for (draft, joined) in ports.iter_mut().zip(joined) {
        draft.port.joined = joined;
    }
}

/// 按方格分桶的点：找离某处不超过容差的点只看周围九格，不用两两比。
struct PointGrid {
    cell: f64,
    tolerance: f64,
    points: Vec<[f64; 2]>,
    cells: HashMap<(i64, i64), Vec<usize>>,
}

impl PointGrid {
    fn new(points: impl IntoIterator<Item = [f64; 2]>, tolerance: f64) -> Self {
        let mut grid = Self {
            cell: tolerance.max(MIN_CELL),
            tolerance,
            points: Vec::new(),
            cells: HashMap::new(),
        };
        for point in points {
            let cell = grid.cell_of(point);
            grid.cells.entry(cell).or_default().push(grid.points.len());
            grid.points.push(point);
        }
        grid
    }

    /// 点所在的格子。`as` 饱和取整：极远的坐标挤进边上的格子，只是慢些，不会错。
    fn cell_of(&self, [x, y]: [f64; 2]) -> (i64, i64) {
        (
            (x / self.cell).floor() as i64,
            (y / self.cell).floor() as i64,
        )
    }

    /// 离 `at` 不超过容差的点的下标。
    fn near(&self, at: [f64; 2]) -> impl Iterator<Item = usize> + '_ {
        let (column, row) = self.cell_of(at);
        (column.saturating_sub(1)..=column.saturating_add(1))
            .flat_map(move |x| (row.saturating_sub(1)..=row.saturating_add(1)).map(move |y| (x, y)))
            .filter_map(|cell| self.cells.get(&cell))
            .flatten()
            .copied()
            .filter(move |index| {
                let point = self.points[*index];
                (point[0] - at[0]).hypot(point[1] - at[1]) <= self.tolerance
            })
    }
}

/// 实体的定位点与包围盒 `[minx, miny, maxx, maxy]`：符号取插入点，点取它自己，
/// 线取起点，圆取圆心，别的取包围盒的左下角。
fn placement(doc: &Document, entity: &EntityType) -> ([f64; 2], [f64; 4]) {
    let bounds = match entity {
        EntityType::Insert(insert) => symbol_bounds(doc, insert),
        other => {
            let bounds = other.as_entity().bounding_box();
            Some([bounds.min.x, bounds.min.y, bounds.max.x, bounds.max.y])
        }
    };
    let at = match entity {
        EntityType::Insert(insert) => xy(insert.insert_point),
        EntityType::Point(point) => xy(point.location),
        EntityType::Line(line) => xy(line.start),
        EntityType::Circle(circle) => xy(circle.center),
        _ => bounds.map_or([0.0, 0.0], |bounds| [bounds[0], bounds[1]]),
    };
    (at, bounds.unwrap_or([at[0], at[1], at[0], at[1]]))
}

/// 符号的包围盒：块定义的范围摆到插入的位置上。挂在插入上的属性（位号标签）
/// 不算：标签的宽是估的，重号时还会变成 `？`，算进来范围就跟着显示变。
fn symbol_bounds(doc: &Document, insert: &Insert) -> Option<[f64; 4]> {
    let local = crate::app::helpers::block_definition_bounds(doc, &insert.block_name)?;
    let transform = insert.get_transform();
    let mut bbox: Option<[f64; 4]> = None;
    for x in [local.min.x, local.max.x] {
        for y in [local.min.y, local.max.y] {
            for z in [local.min.z, local.max.z] {
                let corner = transform.apply(Vector3::new(x, y, z));
                let corner = [corner.x, corner.y, corner.x, corner.y];
                bbox = Some(bbox.map_or(corner, |bbox| merge(bbox, corner)));
            }
        }
    }
    bbox
}

/// 直线的两个端点；别的实体没有。
fn line_ends(entity: &EntityType) -> Vec<[f64; 2]> {
    match entity {
        EntityType::Line(line) => vec![xy(line.start), xy(line.end)],
        _ => Vec::new(),
    }
}

fn xy(point: Vector3) -> [f64; 2] {
    [point.x, point.y]
}

fn merge(left: [f64; 4], right: [f64; 4]) -> [f64; 4] {
    [
        left[0].min(right[0]),
        left[1].min(right[1]),
        left[2].max(right[2]),
        left[3].max(right[3]),
    ]
}

fn compare_at(left: [f64; 2], right: [f64; 2]) -> std::cmp::Ordering {
    left[0]
        .total_cmp(&right[0])
        .then_with(|| left[1].total_cmp(&right[1]))
}

/// 归类表的结果：一个对象，或是对象以外的几样（管段、管线自己的线、
/// 信号线、连接符）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Class {
    Connector,
    Segment,
    PipeLine,
    SignalLine,
    Object(ObjectKind),
}

/// 按 §3.1 归类表从上往下，第一条对上的算数。`is_break` 是拓扑表里这件
/// 管件的 `fitting_type` 是不是打断符。
fn classify(role: &str, type_code: &str, is_break: bool, types: &TypeLib) -> Class {
    match role {
        ROLE_CONNECTOR_IN | ROLE_CONNECTOR_OUT => return Class::Connector,
        ROLE_PIPE_SEGMENT => return Class::Segment,
        ROLE_PIPE_LINE => return Class::PipeLine,
        ROLE_PIPE_RUN => return Class::Object(ObjectKind::PipeRun),
        ROLE_SIGNAL_LINE => return Class::SignalLine,
        ROLE_FITTING if is_break => return Class::Object(ObjectKind::Break),
        _ => {}
    }
    let kind = if type_code == TYPE_VALVE || type_code.starts_with(VALVE_PREFIX) {
        ObjectKind::Valve
    } else if role == ROLE_FITTING {
        ObjectKind::Fitting
    } else if type_code == INSTRUMENT {
        ObjectKind::Instrument
    } else if types
        .ancestors(type_code)
        .iter()
        .any(|code| code == EQUIPMENT)
    {
        ObjectKind::Equipment
    } else {
        ObjectKind::Other
    };
    Class::Object(kind)
}

/// 类型的中文名：类型库里的名字优先，其次是几个内置类型的名字，都没有就是类型码。
fn type_label(types: &TypeLib, type_code: &str) -> String {
    if let Some(node) = types.get(type_code).filter(|node| !node.label.is_empty()) {
        return node.label.clone();
    }
    BUILTIN_LABELS
        .iter()
        .find(|(code, _)| *code == type_code)
        .map_or(type_code, |(_, label)| *label)
        .to_string()
}

/// `INSUNITS` 的值对应的长度单位。
fn length_unit(insunits: i16) -> &'static str {
    match insunits {
        1 => "in",
        2 => "ft",
        3 => "mi",
        4 => "mm",
        5 => "cm",
        6 => "m",
        7 => "km",
        8 => "uin",
        9 => "mil",
        10 => "yd",
        11 => "angstrom",
        12 => "nm",
        13 => "um",
        14 => "dm",
        15 => "dam",
        16 => "hm",
        17 => "Gm",
        18 => "au",
        19 => "ly",
        20 => "pc",
        21 => "us_ft",
        22 => "us_in",
        23 => "us_yd",
        24 => "us_mi",
        _ => "unitless",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::OpenCADStudio;
    use serde_json::{json, Value};
    use std::sync::atomic::{AtomicU64, Ordering};

    const PUMP: &str = "PID_SYM_equipment_pump";
    const VESSEL: &str = "PID_SYM_equipment_vessel";
    const GATE_VALVE: &str = "PID_SYM_valve_gate_valve";
    const TRANSMITTER: &str = "PID_SYM_instrument_transmitter";

    fn app() -> OpenCADStudio {
        let mut app = OpenCADStudio::new_for_test();
        let _ = app.automation_op(r#"{"op":"new"}"#);
        app
    }

    /// 协议 1 的一条控制请求，发给当前的图。
    fn request(app: &mut OpenCADStudio, mut req: Value) -> Value {
        static SERIAL: AtomicU64 = AtomicU64::new(0);
        let state = app.automation_op(r#"{"protocol":1,"op":"state"}"#);
        req["protocol"] = json!(1);
        req["document_id"] = state["document_id"].clone();
        req["revision"] = state["revision"].clone();
        req["client_id"] = json!("test");
        req["request_id"] = json!(format!("ix1-{}", SERIAL.fetch_add(1, Ordering::Relaxed)));
        app.automation_op(&req.to_string())
    }

    fn run(app: &mut OpenCADStudio, cmd: &str) {
        let done = request(app, json!({"op": "run", "cmd": cmd}));
        assert_eq!(done["ok"], true, "{cmd}：{done}");
    }

    /// 起 `cmd`，依次喂点，最后回车。
    fn draw(app: &mut OpenCADStudio, cmd: &str, points: &[[f64; 2]]) {
        run(app, cmd);
        for [x, y] in points {
            let point =
                json!({"op": "input", "kind": "point", "point": [x, y, 0.0], "space": "wcs"});
            let fed = request(app, point);
            assert_eq!(fed["ok"], true, "{cmd} 喂点：{fed}");
        }
        request(app, json!({"op": "input", "kind": "enter"}));
    }

    fn record(doc: &Document, handle: Handle) -> PidAuthor {
        PidAuthor::load(doc, handle).expect("P&ID 记录")
    }

    fn block_of(entity: &EntityType) -> Option<&str> {
        match entity {
            EntityType::Insert(insert) => Some(insert.block_name.as_str()),
            _ => None,
        }
    }

    /// 图上带记录、`pick` 认得的唯一那个实体。
    fn only(doc: &Document, pick: impl Fn(&EntityType, &PidAuthor) -> bool) -> Handle {
        let found: Vec<Handle> = doc
            .entities()
            .filter_map(|entity| {
                let handle = entity.common().handle;
                let author = PidAuthor::load(doc, handle)?;
                pick(entity, &author).then_some(handle)
            })
            .collect();
        assert_eq!(found.len(), 1, "应恰好一个：{found:?}");
        found[0]
    }

    fn ends_of(doc: &Document, handle: Handle) -> ([f64; 2], [f64; 2]) {
        doc.entities()
            .find_map(|entity| match entity {
                EntityType::Line(line) if line.common.handle == handle => {
                    Some(([line.start.x, line.start.y], [line.end.x, line.end.y]))
                }
                _ => None,
            })
            .expect("一根线")
    }

    /// 小图（§7）：一台泵左边接一个不在线上的阀门，一台容器；管线 L-1001
    /// 自己一根线，线头上一个流出连接符；管路 L-1001-01 从泵的出口到容器的
    /// 入口，上面一个阀门、一个不单独编号的管件（变送器符号）、一个打断符；
    /// 信号线 S-2001 两根线接到一台仪表的入口；还有一根挂了别的类型的线。
    fn small_drawing(app: &mut OpenCADStudio) {
        draw(app, "PIDINSERT pump", &[[0.0, 0.0]]);
        draw(app, "PIDINSERT gate_valve", &[[-28.0, 0.0]]);
        draw(app, "PIDINSERT vessel", &[[200.0, 12.0]]);
        draw(app, "PIDPIPE", &[[200.0, 100.0], [300.0, 100.0]]);
        draw(app, "PIDPIPE run", &[[14.0, 0.0], [200.0, 0.0]]);
        // 管件顺着管路方向放：在另一头已经接着管件的管段上再插管件，`PIDFITTING`
        // 拆出的新段终点不带原来的 `peer`，先放的那个管件这一侧就断了。
        draw(app, "PIDFITTING transmitter", &[[40.0, 0.0]]);
        draw(app, "PIDFITTING gate_valve", &[[100.0, 0.0]]);
        draw(app, "PIDBREAK", &[[150.0, 0.0]]);
        draw(
            app,
            "PIDPIPE signal",
            &[[0.0, 50.0], [50.0, 50.0], [50.0, 80.0]],
        );
        draw(app, "PIDINSERT transmitter", &[[58.0, 80.0]]);
        draw(app, "PIDCONNECTOR pipe_out", &[[300.0, 100.0]]);
        run(app, "PIDSETPROP L-1001 管道等级 A1A");
        run(app, "PIDSETPROP L-1001-01 公称直径 50");
        let pump = only(app.active_document_for_test(), |entity, _| {
            block_of(entity) == Some(PUMP)
        });
        let tag = record(app.active_document_for_test(), pump).tag;
        run(app, &format!("PIDSETPROP {tag} 设计温度 80"));
        draw(app, "LINE", &[[400.0, 0.0], [450.0, 0.0]]);
        let doc = app.active_document_for_test();
        let note = doc
            .entities()
            .filter(|entity| matches!(entity, EntityType::Line(_)))
            .map(|entity| entity.common().handle)
            .filter(|handle| PidAuthor::load(doc, *handle).is_none())
            .max_by_key(|handle| handle.value())
            .expect("刚画的线");
        let data: Vec<Value> = [
            "type=NOTE",
            "tag=N-1",
            "tag_source=manual",
            "tag_status=ok",
            r#"props={"说明":"图例"}"#,
        ]
        .iter()
        .map(|value| json!({"code": 1000, "value": value}))
        .collect();
        let handles = [handle_text(note)];
        let set = request(
            app,
            json!({"op": "xdata_set", "handles": handles, "app": "PID_AUTHOR", "data": data}),
        );
        assert_eq!(set["ok"], true, "{set}");
    }

    fn meta() -> SummaryMeta {
        SummaryMeta {
            drawing_id: "PID-1101".to_string(),
            file: Some("PID-1101.dwg".to_string()),
            pbs: Some("1001-01".to_string()),
            source: SummarySource::File,
            fingerprint: Some(Fingerprint {
                size: 482_133,
                modified_at: 1_794_800_000,
            }),
        }
    }

    fn summary_of(app: &OpenCADStudio) -> DrawingSummary {
        summarize(
            app.active_document_for_test(),
            &app.pid_symbols,
            meta(),
            DEFAULT_TOLERANCE,
        )
    }

    fn found(summary: &DrawingSummary, handle: Handle) -> &SummaryObject {
        let text = handle_text(handle);
        summary
            .objects
            .iter()
            .find(|object| object.handle == text)
            .unwrap_or_else(|| panic!("摘要里没有 {text}：{:#?}", summary.objects))
    }

    /// 期望的对象：记录原样的四项照抄（类型码先对一下），其余先是没有符号、
    /// 没有属性、不归谁的样子，由调用处改。
    fn plain(
        doc: &Document,
        handle: Handle,
        kind: ObjectKind,
        type_code: &str,
        type_label: &str,
    ) -> SummaryObject {
        let author = record(doc, handle);
        assert_eq!(author.type_code, type_code, "{}", handle_text(handle));
        SummaryObject {
            kind,
            type_code: author.type_code,
            type_label: type_label.to_string(),
            tag: author.tag,
            own_tag: true,
            tag_source: author.tag_source,
            tag_status: author.tag_status,
            symbol: None,
            symbol_known: true,
            props: BTreeMap::new(),
            overrides: Vec::new(),
            owner: None,
            layer: "0".to_string(),
            at: [0.0, 0.0],
            bbox: [0.0; 4],
            handle: handle_text(handle),
        }
    }

    fn props(pairs: &[(&str, &str)]) -> BTreeMap<String, String> {
        pairs
            .iter()
            .map(|(key, value)| ((*key).to_string(), (*value).to_string()))
            .collect()
    }

    fn port(
        owner: &SummaryObject,
        name: &str,
        direction: PortDir,
        at: [f64; 2],
        joined: bool,
    ) -> SummaryPort {
        SummaryPort {
            owner_kind: owner.kind,
            type_code: owner.type_code.clone(),
            tag: owner.tag.clone(),
            port: name.to_string(),
            direction: Some(direction),
            at,
            joined,
        }
    }

    #[test]
    fn a_small_drawing_gives_every_object_its_fields() {
        let mut app = app();
        small_drawing(&mut app);
        let summary = summary_of(&app);
        let doc = app.active_document_for_test();
        let table = TopologyTable::load(doc);

        let text = serde_json::to_string(&summary).expect("JSON");
        let head = format!(
            r#"{{"version":1,"drawing_id":"PID-1101","file":"PID-1101.dwg","source":"file","fingerprint":{{"size":482133,"modified_at":1794800000}},"pbs":"1001-01","length_unit":"{}","objects":[{{"#,
            length_unit(doc.header.insertion_units)
        );
        assert!(text.starts_with(&head), "{text}");
        assert!(text.contains(r#"}],"ports":[{"owner_kind":"#), "{text}");

        let pump = only(doc, |entity, _| block_of(entity) == Some(PUMP));
        assert!(record(doc, pump).tag.starts_with("P-"));
        let want = SummaryObject {
            symbol: Some(PUMP.to_string()),
            props: props(&[("设计温度", "80")]),
            at: [0.0, 0.0],
            bbox: [-14.0, -8.0, 14.0, 8.0],
            ..plain(doc, pump, ObjectKind::Equipment, "EQUIPMENT", "设备")
        };
        assert_eq!(found(&summary, pump), &want);

        let loose = only(doc, |entity, author| {
            block_of(entity) == Some(GATE_VALVE) && author.topology.is_empty()
        });
        let want = SummaryObject {
            symbol: Some(GATE_VALVE.to_string()),
            at: [-28.0, 0.0],
            bbox: [-42.0, -6.0, -14.0, 6.0],
            ..plain(doc, loose, ObjectKind::Valve, "VALVE", "阀门")
        };
        assert_eq!(found(&summary, loose), &want);

        let vessel = only(doc, |entity, _| block_of(entity) == Some(VESSEL));
        let want = SummaryObject {
            symbol: Some(VESSEL.to_string()),
            at: [200.0, 12.0],
            bbox: [192.0, 0.0, 208.0, 24.0],
            ..plain(doc, vessel, ObjectKind::Equipment, "EQUIPMENT", "设备")
        };
        assert_eq!(found(&summary, vessel), &want);

        let line = table.line_by_tag("L-1001").expect("管线 L-1001");
        let line = parse_handle(&line.handle).expect("句柄");
        let want = SummaryObject {
            props: props(&[("管道等级", "A1A")]),
            at: [200.0, 100.0],
            bbox: [200.0, 100.0, 300.0, 100.0],
            ..plain(doc, line, ObjectKind::PipeLine, "PIPE_LINE", "管线")
        };
        assert_eq!(found(&summary, line), &want);

        let run = table
            .pipe_runs
            .iter()
            .find(|run| run.tag == "L-1001-01")
            .expect("管路 L-1001-01");
        let run = parse_handle(&run.handle).expect("句柄");
        let want = SummaryObject {
            props: props(&[
                ("管线号", "L-1001"),
                ("管道等级", "A1A"),
                ("公称直径", "50"),
            ]),
            overrides: vec!["公称直径".to_string()],
            owner: Some("L-1001".to_string()),
            at: [14.0, 0.0],
            bbox: [14.0, 0.0, 200.0, 0.0],
            ..plain(doc, run, ObjectKind::PipeRun, "PIPE_RUN", "管路")
        };
        assert_eq!(found(&summary, run), &want);
        let json = serde_json::to_string(found(&summary, run)).expect("JSON");
        assert!(
            json.starts_with(r#"{"kind":"管路","type_code":"PIPE_RUN","type_label":"管路","tag":"L-1001-01","own_tag":true,"tag_source":"#),
            "{json}"
        );
        let order: Vec<usize> = [
            "kind",
            "type_code",
            "type_label",
            "tag",
            "own_tag",
            "tag_source",
            "tag_status",
            "symbol",
            "symbol_known",
            "props",
            "overrides",
            "owner",
            "layer",
            "at",
            "bbox",
            "handle",
        ]
        .iter()
        .map(|key| {
            json.find(&format!("\"{key}\":"))
                .unwrap_or_else(|| panic!("缺 {key}：{json}"))
        })
        .collect();
        assert!(
            order.windows(2).all(|pair| pair[0] < pair[1]),
            "字段顺序：{json}"
        );

        let valve = only(doc, |entity, author| {
            block_of(entity) == Some(GATE_VALVE) && author.topology == ROLE_FITTING
        });
        let want = SummaryObject {
            symbol: Some(GATE_VALVE.to_string()),
            owner: Some("L-1001-01".to_string()),
            at: [100.0, 0.0],
            bbox: [86.0, -6.0, 114.0, 6.0],
            ..plain(doc, valve, ObjectKind::Valve, "VALVE", "阀门")
        };
        assert_eq!(found(&summary, valve), &want);

        let fitting = only(doc, |entity, author| {
            block_of(entity) == Some(TRANSMITTER) && author.topology == ROLE_FITTING
        });
        assert_eq!(
            record(doc, fitting).tag,
            "L-1001-01",
            "不单独编号的管件记管路的位号"
        );
        let want = SummaryObject {
            own_tag: false,
            symbol: Some(TRANSMITTER.to_string()),
            // `PIDSETPROP` 按位号找对象，管件带着管路的位号，一起被写上。
            props: props(&[("公称直径", "50")]),
            owner: Some("L-1001-01".to_string()),
            at: [40.0, 0.0],
            bbox: [32.0, -8.0, 48.0, 8.0],
            ..plain(doc, fitting, ObjectKind::Fitting, "FITTING", "管件")
        };
        assert_eq!(found(&summary, fitting), &want);

        let mark = only(doc, |entity, author| {
            author.topology == ROLE_FITTING && matches!(entity, EntityType::Line(_))
        });
        let (a, b) = ends_of(doc, mark);
        let want = SummaryObject {
            own_tag: false,
            owner: Some("L-1001-01".to_string()),
            at: a,
            bbox: [
                a[0].min(b[0]),
                a[1].min(b[1]),
                a[0].max(b[0]),
                a[1].max(b[1]),
            ],
            ..plain(doc, mark, ObjectKind::Break, "FITTING", "管件")
        };
        assert_eq!(found(&summary, mark), &want);

        let mut signal: Vec<Handle> = doc
            .entities()
            .map(|entity| entity.common().handle)
            .filter(|handle| {
                PidAuthor::load(doc, *handle)
                    .is_some_and(|author| author.topology == ROLE_SIGNAL_LINE)
            })
            .collect();
        signal.sort_by_key(|handle| handle.value());
        assert_eq!(signal.len(), 2, "信号线画了两根");
        assert_eq!(record(doc, signal[0]).tag, "S-2001");
        let want = SummaryObject {
            at: [0.0, 50.0],
            bbox: [0.0, 50.0, 50.0, 80.0],
            ..plain(
                doc,
                signal[0],
                ObjectKind::SignalLine,
                "SIGNAL_LINE",
                "信号线",
            )
        };
        assert_eq!(found(&summary, signal[0]), &want);

        let instrument = only(doc, |entity, author| {
            block_of(entity) == Some(TRANSMITTER) && author.topology.is_empty()
        });
        let want = SummaryObject {
            symbol: Some(TRANSMITTER.to_string()),
            at: [58.0, 80.0],
            bbox: [50.0, 72.0, 66.0, 88.0],
            ..plain(
                doc,
                instrument,
                ObjectKind::Instrument,
                "INSTRUMENT",
                "仪表",
            )
        };
        assert_eq!(found(&summary, instrument), &want);

        let note = only(doc, |_, author| author.type_code == "NOTE");
        let want = SummaryObject {
            props: props(&[("说明", "图例")]),
            at: [400.0, 0.0],
            bbox: [400.0, 0.0, 450.0, 0.0],
            ..plain(doc, note, ObjectKind::Other, "NOTE", "NOTE")
        };
        assert_eq!(found(&summary, note), &want);

        let connector = only(doc, |_, author| author.topology == ROLE_CONNECTOR_OUT);
        assert_eq!(
            record(doc, connector).type_code,
            "PIPE_LINE",
            "连接符的类型码是管线的"
        );
        let connector = handle_text(connector);
        assert!(
            summary
                .objects
                .iter()
                .all(|object| object.handle != connector),
            "连接符不是对象：{:#?}",
            summary.objects
        );
        assert_eq!(summary.objects.len(), 11, "{:#?}", summary.objects);
        let kinds: Vec<ObjectKind> = summary.objects.iter().map(|object| object.kind).collect();
        assert_eq!(
            kinds
                .iter()
                .filter(|kind| **kind == ObjectKind::PipeLine)
                .count(),
            1,
            "连接符不算成管线"
        );
        let keys: Vec<(&str, &str)> = summary
            .objects
            .iter()
            .map(|object| (object.type_code.as_str(), object.tag.as_str()))
            .collect();
        assert!(
            keys.windows(2).all(|pair| pair[0] < pair[1]),
            "按类型码、位号排：{keys:?}"
        );

        let ports = [
            port(
                found(&summary, pump),
                "in1",
                PortDir::In,
                [-14.0, 0.0],
                true,
            ),
            port(
                found(&summary, pump),
                "out1",
                PortDir::Out,
                [14.0, 0.0],
                true,
            ),
            port(
                found(&summary, loose),
                "in1",
                PortDir::In,
                [-42.0, 0.0],
                false,
            ),
            port(
                found(&summary, loose),
                "out1",
                PortDir::Out,
                [-14.0, 0.0],
                true,
            ),
            port(
                found(&summary, vessel),
                "in1",
                PortDir::In,
                [200.0, 0.0],
                true,
            ),
            port(
                found(&summary, vessel),
                "out1",
                PortDir::Out,
                [200.0, 24.0],
                false,
            ),
            port(
                found(&summary, valve),
                "in1",
                PortDir::In,
                [86.0, 0.0],
                true,
            ),
            port(
                found(&summary, valve),
                "out1",
                PortDir::Out,
                [114.0, 0.0],
                true,
            ),
            port(
                found(&summary, fitting),
                "in1",
                PortDir::In,
                [32.0, 0.0],
                true,
            ),
            port(
                found(&summary, fitting),
                "out1",
                PortDir::Out,
                [48.0, 0.0],
                true,
            ),
            port(
                found(&summary, instrument),
                "in1",
                PortDir::In,
                [50.0, 80.0],
                true,
            ),
            port(
                found(&summary, instrument),
                "out1",
                PortDir::Out,
                [66.0, 80.0],
                false,
            ),
        ];
        assert_eq!(summary.ports.len(), ports.len(), "{:#?}", summary.ports);
        for want in &ports {
            assert!(
                summary.ports.contains(want),
                "缺 {want:?}：{:#?}",
                summary.ports
            );
        }
        let order: Vec<(&str, [f64; 2])> = summary
            .ports
            .iter()
            .map(|port| (port.tag.as_str(), port.at))
            .collect();
        assert!(
            order.windows(2).all(|pair| pair[0] <= pair[1]),
            "端口按位号、坐标排：{order:?}"
        );
    }

    #[test]
    fn props_past_the_record_limit_come_back_whole() {
        let mut app = app();
        draw(&mut app, "PIDINSERT pump", &[[0.0, 0.0]]);
        let pump = only(app.active_document_for_test(), |entity, _| {
            block_of(entity) == Some(PUMP)
        });
        let tag = record(app.active_document_for_test(), pump).tag;
        let long = "A".repeat(3000);
        run(&mut app, &format!("PIDSETPROP {tag} 设计温度 {long}"));
        let doc = app.active_document_for_test();
        let entity = doc
            .entities()
            .find(|entity| entity.common().handle == pump)
            .expect("泵");
        assert!(!PidAuthor::props_are_inline(entity), "属性应已挪进 XRecord");
        let summary = summary_of(&app);
        assert_eq!(found(&summary, pump).props.get("设计温度"), Some(&long));
    }

    #[test]
    fn the_same_drawing_summarizes_to_the_same_bytes_and_is_left_alone() {
        let mut app = app();
        small_drawing(&mut app);
        let doc = app.active_document_for_test();
        let before = format!("{doc:?}");
        let first = summarize(doc, &app.pid_symbols, meta(), DEFAULT_TOLERANCE);
        let second = summarize(doc, &app.pid_symbols, meta(), DEFAULT_TOLERANCE);
        let first = serde_json::to_string(&first).expect("JSON");
        assert_eq!(first, serde_json::to_string(&second).expect("JSON"));
        assert!(first.contains(r#""objects":[{"#), "摘要不该是空的：{first}");
        assert_eq!(format!("{doc:?}"), before, "摘要不改图");
    }

    #[test]
    fn the_classification_table_reads_top_down() {
        let types = TypeLib::embedded();
        let rows = [
            (ROLE_CONNECTOR_IN, "PIPE_LINE", false, Class::Connector),
            (ROLE_CONNECTOR_OUT, "SIGNAL_LINE", false, Class::Connector),
            (ROLE_PIPE_SEGMENT, "PIPE_SEGMENT", false, Class::Segment),
            (ROLE_PIPE_LINE, "PIPE_LINE", false, Class::PipeLine),
            (
                ROLE_PIPE_RUN,
                "PIPE_RUN",
                false,
                Class::Object(ObjectKind::PipeRun),
            ),
            (ROLE_SIGNAL_LINE, "SIGNAL_LINE", false, Class::SignalLine),
            (
                ROLE_FITTING,
                "FITTING",
                true,
                Class::Object(ObjectKind::Break),
            ),
            (
                ROLE_FITTING,
                "VALVE",
                true,
                Class::Object(ObjectKind::Break),
            ),
            (
                ROLE_FITTING,
                "VALVE",
                false,
                Class::Object(ObjectKind::Valve),
            ),
            ("", "VALVE", false, Class::Object(ObjectKind::Valve)),
            ("", "VALVE_GATE", false, Class::Object(ObjectKind::Valve)),
            (
                ROLE_FITTING,
                "FITTING",
                false,
                Class::Object(ObjectKind::Fitting),
            ),
            (
                ROLE_FITTING,
                "INSTRUMENT",
                false,
                Class::Object(ObjectKind::Fitting),
            ),
            (
                "",
                "INSTRUMENT",
                false,
                Class::Object(ObjectKind::Instrument),
            ),
            ("", "EQUIPMENT", false, Class::Object(ObjectKind::Equipment)),
            ("", "PUMP", false, Class::Object(ObjectKind::Equipment)),
            ("", "TANK", false, Class::Object(ObjectKind::Equipment)),
            ("", "VALVES", false, Class::Object(ObjectKind::Other)),
            ("", "NOTE", false, Class::Object(ObjectKind::Other)),
            ("", "", false, Class::Object(ObjectKind::Other)),
        ];
        for (role, type_code, is_break, want) in rows {
            assert_eq!(
                classify(role, type_code, is_break, &types),
                want,
                "{role:?} {type_code:?} {is_break}"
            );
        }
        assert!(!ObjectKind::Fitting.has_own_tag());
        assert!(!ObjectKind::Break.has_own_tag());
        assert!(ObjectKind::Valve.has_own_tag());
    }

    #[test]
    fn type_labels_come_from_the_library_then_the_builtin_names() {
        let types = TypeLib::embedded();
        for (code, label) in [
            ("EQUIPMENT", "设备"),
            ("PUMP", "泵"),
            ("TANK", "储罐"),
            ("VALVE", "阀门"),
            ("VALVE_GATE", "闸阀"),
            ("INSTRUMENT", "仪表"),
            ("PIPE_LINE", "管线"),
            ("PIPE_RUN", "管路"),
            ("SIGNAL_LINE", "信号线"),
            ("FITTING", "管件"),
            ("NOTE", "NOTE"),
        ] {
            assert_eq!(type_label(&types, code), label, "{code}");
        }
    }

    #[test]
    fn the_length_unit_follows_insunits() {
        for (insunits, unit) in [
            (0, "unitless"),
            (1, "in"),
            (2, "ft"),
            (4, "mm"),
            (5, "cm"),
            (6, "m"),
            (14, "dm"),
            (-1, "unitless"),
            (99, "unitless"),
        ] {
            assert_eq!(length_unit(insunits), unit, "{insunits}");
        }
    }
}
