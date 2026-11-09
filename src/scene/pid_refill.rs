//! 移动管件后的管段自动补位（**scene 层**）。
//!
//! 拖动阀门时两侧管段的**内侧**端点收到阀门端口的新坐标上，外侧端点不动 ——
//! 管段数与端口绑定都不变（不重建管段），只改端点坐标。拓扑表是权威判据
//! （`VAL-PIPE-019/020` 读 `pipe_segments[].start/end.point`），管段实体的
//! 几何随之同步，截图才与表一致。
//!
//! **为什么住在 scene 层**：真实产品的移动路径有三条 —— GUI 拖动 grip
//! （`commit_active_grip_edit`）、`MOVE` 命令（`handle_transform_selected`）、
//! `grip_drag` op（同一个 grip 提交）—— 它们各自直接改实体，只有把判定与
//! 落表放进 [`crate::scene::Scene::transform_entities`] 与 grip 提交里，三条
//! 路径才共享同一套语义。只挂在 REST 控制面上的实现只能证明验证通道可用，
//! 产品路径上不可达（M5 scrutiny round 1 的 blocking）。
//!
//! 内侧端点取**阀门端口的世界坐标**，不是「原坐标 + 平移量」。插入时两侧
//! 端点留了 `pidpipe::FITTING_CLEARANCE`（让截图判据严格有序），补位按契约
//! `VAL-PIPE-019` 把端点吸附回端口上；阀门外形区间 `[v0,v1]` 因此就是两个
//! 端口之间的距离，长度守恒 `end_A + (200 - start_B) + (v1 - v0) == 200`
//! 精确成立（`VAL-PIPE-020`）。
//!
//! **先校验、后写入**：越界移动（会让某段反向，或让两侧段沿管段方向重叠）
//! 返回结构化失败，此时一个字节都不写，几何保持移动前的状态
//! （`VAL-PIPE-022` 的「无部分写入」）。
//!
//! 补位不重建管段、不重绑端口、不改位号 —— `VAL-PIPE-021` 断言这三件事
//! 在移动前后完全相同。

use crate::pid::graph_data::PID_AUTHOR_TOPOLOGY;
use crate::pid::symbols::{block_point_locations, place};
use crate::pid::topology::{
    handle_text, parse_handle, SegmentEnd, TopologyTable, VALVE_PORT_NAMES,
};
use codec::objects::ObjectType;
use codec::{Document, Handle};
use glam::DVec3;
use serde_json::{json, Value};

/// 补位被拒时带的结构化错误。
pub(crate) struct RefillError {
    /// 契约里可判定的错误码。
    pub(crate) code: &'static str,
    /// 人读的原因。
    pub(crate) message: String,
}

impl RefillError {
    /// 控制面响应体：与 `control::failure` 同形。
    pub(crate) fn into_value(self) -> Value {
        json!({
            "ok": false,
            "status": "failed",
            "code": self.code,
            "error": self.message,
        })
    }
}

/// 一次补位的预演结果：阀门平移后各管段端点的新坐标。
pub(crate) struct RefillPlan {
    /// 需要改写的管段：handle → (start, end)。
    pub(crate) segments: Vec<(Handle, SegmentEnd, SegmentEnd)>,
}

impl RefillPlan {
    /// 没有需要改写的管段（这次移动与管件无关）。
    pub(crate) fn is_empty(&self) -> bool {
        self.segments.is_empty()
    }
}

/// 移动被拒的错误码：平移把管段推过了外侧端点，或让两侧段重叠。
pub(crate) const REFILL_OUT_OF_RANGE: &str = "FITTING_REFILL_OUT_OF_RANGE";

/// 一个被移动管件的端口表：`(端口名, 世界坐标)`，按主轴排序后下标即
/// `VALVE_PORT_NAMES` 的顺序。
type PortTable = Vec<(String, [f64; 2])>;

/// 全部被移动管件的端口表：`(管件 handle 文本, 端口表)`。
type PortTables = Vec<(String, PortTable)>;

/// 统一平移量的预演：所有被移动实体共用一个 `delta`，几何尚未写入。
pub(crate) fn plan_refill_uniform(
    document: &Document,
    moved: &[Handle],
    delta: DVec3,
) -> Result<RefillPlan, RefillError> {
    let moved: Vec<(Handle, DVec3)> = moved.iter().map(|handle| (*handle, delta)).collect();
    plan_refill(document, &moved, false)
}

/// 预演一次平移带来的补位。**只读**，失败时不碰文档。
///
/// `moved` 是被平移的实体与各自的平移量；其中只有拓扑表 `fittings` 里登记
/// 的管件会带动补位。返回空计划表示这次移动与管件无关（普通实体、或在管段
/// 端点插入、只绑了一个端口的管件）。
///
/// `applied` 说明几何是否**已经**写进文档：
///
/// - `false` —— 变换尚未执行（`transform_entities`、`MOVE` 命令），端口落在
///   `insert_point + delta`；
/// - `true` —— 变换已由 grip 预览写入（拖动提交），端口就在当前
///   `insert_point` 上。
///
/// 端口的世界坐标从**块定义里的 `Point` 实体**推出（[`block_point_locations`]），
/// 与 [`crate::pid::symbols::placed_ports`] 同源：`SymbolDefinition::block_entities`
/// 按 `ports` 顺序逐条生成 `Point`，所以块内点序就是端口序。这样补位不需要
/// 持有符号库，scene 层可以独立调用。
pub(crate) fn plan_refill(
    document: &Document,
    moved: &[(Handle, DVec3)],
    applied: bool,
) -> Result<RefillPlan, RefillError> {
    let mut plan = RefillPlan {
        segments: Vec::new(),
    };
    if moved.is_empty() {
        return Ok(plan);
    }
    let table = TopologyTable::load(document);
    let moved_text: Vec<String> = moved.iter().map(|(handle, _)| handle_text(*handle)).collect();
    let fittings: Vec<_> = table
        .fittings
        .iter()
        .filter(|fitting| moved_text.contains(&fitting.handle))
        .cloned()
        .collect();
    if fittings.is_empty() {
        return Ok(plan);
    }

    // 每个被移动管件：把块内端口点平移到新位置，并按段主轴决定哪个端口是
    // 「连接点1」。插入时 `连接点1` 落在「段起点 → 落点」的反侧，所以主轴
    // 方向取「段 A 外侧端点 → 段 B 外侧端点」，与插入同源。
    let mut port_points: PortTables = Vec::new();
    for fitting in &fittings {
        let Some(handle) = parse_handle(&fitting.handle) else {
            continue;
        };
        let Some(codec::EntityType::Insert(insert)) = document.get_entity(handle) else {
            continue;
        };
        let delta = moved
            .iter()
            .find(|(moved_handle, _)| *moved_handle == handle)
            .map(|(_, delta)| *delta)
            .unwrap_or(DVec3::ZERO);
        if delta.x == 0.0 && delta.y == 0.0 {
            continue;
        }
        let base = document
            .block_records
            .get(&insert.block_name)
            .map(|record| (record.base_point.x, record.base_point.y))
            .unwrap_or((0.0, 0.0));
        let scale = (insert.x_scale(), insert.y_scale());
        // `applied` 时几何已由 grip 预览写入，端口就在当前插入点上；否则按
        // 尚未执行的平移量推。
        let at = if applied {
            (insert.insert_point.x, insert.insert_point.y)
        } else {
            (
                insert.insert_point.x + delta.x,
                insert.insert_point.y + delta.y,
            )
        };
        let axis = fitting_axis(&table, fitting);
        // 端口先落到世界坐标，再按主轴投影排序：投影小的在段起点侧，
        // 即 `连接点1`。用世界坐标排序而不是块内坐标，插入带旋转时也成立。
        let mut world_ports: Vec<[f64; 2]> = block_point_locations(document, &insert.block_name)
            .into_iter()
            .map(|local| {
                let world = place(local, base, scale, insert.rotation, at);
                [world.0, world.1]
            })
            .collect();
        world_ports.sort_by(|a, b| {
            let key = |point: &[f64; 2]| point[0] * axis[0] + point[1] * axis[1];
            key(a)
                .partial_cmp(&key(b))
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        let points: Vec<(String, [f64; 2])> = VALVE_PORT_NAMES
            .iter()
            .enumerate()
            .filter_map(|(index, name)| {
                world_ports
                    .get(index)
                    .map(|point| ((*name).to_string(), *point))
            })
            .collect();
        port_points.push((fitting.handle.clone(), points));
    }
    if port_points.is_empty() {
        return Ok(plan);
    }

    // 只有 `peer` 指向被移动管件端口的端点才动 —— 另一侧是外侧端点，保持原位。
    for segment in &table.pipe_segments {
        let mut start = segment.start.clone();
        let mut end = segment.end.clone();
        let mut touched = false;
        if let Some(point) = port_of(&port_points, start.peer.as_deref()) {
            start.point = point;
            touched = true;
        }
        if let Some(point) = port_of(&port_points, end.peer.as_deref()) {
            end.point = point;
            touched = true;
        }
        if !touched {
            continue;
        }
        let Some(handle) = parse_handle(&segment.handle) else {
            continue;
        };
        plan.segments.push((handle, start, end));
    }
    if plan.is_empty() {
        return Ok(plan);
    }

    // 判据一：段本身不得反向，也不得退化成一个点 —— 内侧端点不能越过外侧端点。
    for (handle, start, end) in &plan.segments {
        let text = handle_text(*handle);
        let Some(old) = table.pipe_segments.iter().find(|s| s.handle == text) else {
            continue;
        };
        let old_dir = [
            old.end.point[0] - old.start.point[0],
            old.end.point[1] - old.start.point[1],
        ];
        let new_dir = [
            end.point[0] - start.point[0],
            end.point[1] - start.point[1],
        ];
        let along = old_dir[0] * new_dir[0] + old_dir[1] * new_dir[1];
        if along <= 0.0 || new_dir[0].hypot(new_dir[1]) <= 1e-9 {
            return Err(RefillError {
                code: REFILL_OUT_OF_RANGE,
                message: format!(
                    "PID 补位：平移让管段 {text} 越过它的外侧端点，几何保持不变。"
                ),
            });
        }
    }

    // 判据二：同一管件两侧的段不得沿管段方向重叠（内侧点必须严格有序）。
    for fitting in &fittings {
        let port = |name: &str| fitting.ports.get(name).and_then(|slot| slot.clone());
        let (Some(port_a), Some(port_b)) =
            (port(VALVE_PORT_NAMES[0]), port(VALVE_PORT_NAMES[1]))
        else {
            // 端点插入的管件只绑一个端口，没有第二段可重叠。
            continue;
        };
        let (Some(inner_a), Some(inner_b)) = (
            inner_point(&plan, &port_a, &fitting.handle, VALVE_PORT_NAMES[0]),
            inner_point(&plan, &port_b, &fitting.handle, VALVE_PORT_NAMES[1]),
        ) else {
            continue;
        };
        let axis = axis_of(&plan, &port_a).unwrap_or([1.0, 0.0]);
        let gap = (inner_b[0] - inner_a[0]) * axis[0] + (inner_b[1] - inner_a[1]) * axis[1];
        if gap <= 0.0 {
            return Err(RefillError {
                code: REFILL_OUT_OF_RANGE,
                message: format!(
                    "PID 补位：平移会让管件 {} 两侧的管段重叠，几何保持不变。",
                    fitting.handle
                ),
            });
        }
    }

    Ok(plan)
}

/// 拓扑表 XRecord 在 `document.objects` 里的当前镜像：`(记录 handle, 对象)`。
///
/// 补位除了改管段 `Line` 几何，还会 [`TopologyTable::store`] 整 key 重写
/// `PID_AUTHOR_TOPOLOGY` XRecord —— 那是 `document.objects` 里的对象，
/// `record_undo_before` 登记的实体镜像覆盖不到它。调用方在写表**之前**取
/// 一次本函数的结果交给 `record_undo_object_before`，撤销才能把表一起退回去。
///
/// 表不存在时返回 `None`。补位只会在表里已有 `fittings` 记录时发生
/// （[`plan_refill`] 从表里挑管件），所以「计划非空」必然伴随 `Some`。
pub(crate) fn topology_record_image(document: &Document) -> Option<(Handle, ObjectType)> {
    let owner = document.header.model_space_block_handle;
    let dictionary_handle = document.extension_dictionary_handle(owner)?;
    let record_handle = match document.objects.get(&dictionary_handle) {
        Some(ObjectType::Dictionary(dictionary)) => dictionary.get(PID_AUTHOR_TOPOLOGY)?,
        _ => return None,
    };
    let before = document.objects.get(&record_handle)?.clone();
    matches!(before, ObjectType::XRecord(_)).then_some((record_handle, before))
}

/// `handles` 里是否有**绑定过管段端口**的管件（阀门）。
///
/// 组合变换（含旋转/缩放的 ALIGN）不能安全补位，命令层用这条判据决定是
/// 「拒绝并提示用 MOVE」还是「按无补位执行」。
pub(crate) fn binds_pipe_segments(document: &Document, handles: &[Handle]) -> bool {
    if handles.is_empty() {
        return false;
    }
    let table = TopologyTable::load(document);
    let selected: Vec<String> = handles.iter().map(|handle| handle_text(*handle)).collect();
    table.fittings.iter().any(|fitting| {
        selected.contains(&fitting.handle) && fitting.ports.values().any(|slot| slot.is_some())
    })
}

/// 把预演结果落到拓扑表与管段几何上，返回被改写的管段 handle。
///
/// 只改端点坐标：不新建/删除实体、不动 `ports` 绑定、不碰位号
/// （`VAL-PIPE-021`）。不负责 undo 快照、`dirty` 与重绘 —— 由调用方按各自的
/// 历史与重绘机制包围。
pub(crate) fn apply_geometry(document: &mut Document, plan: &RefillPlan) -> Vec<Handle> {
    if plan.is_empty() {
        return Vec::new();
    }
    let mut changed: Vec<Handle> = Vec::with_capacity(plan.segments.len());
    let mut table = TopologyTable::load(document);
    for (handle, start, end) in &plan.segments {
        if let Some(record) = table.segment_by_handle_mut(*handle) {
            record.start = start.clone();
            record.end = end.clone();
        }
        // 几何同步：管段实体是 `Line`，端点跟拓扑表走。
        if let Some(codec::EntityType::Line(line)) = document.get_entity_mut(*handle) {
            line.start = codec::types::Vector3::new(start.point[0], start.point[1], line.start.z);
            line.end = codec::types::Vector3::new(end.point[0], end.point[1], line.end.z);
        }
        changed.push(*handle);
    }
    table.store(document);
    changed
}

/// 被移动管件所在管路的主轴方向：从段 A 的外侧端点指向段 B 的外侧端点。
///
/// 插入阀门时内侧端点就是沿这条轴分列两侧的（`pidpipe::fitting_port_points`
/// 用「段起点 → 落点」定向），补位沿用同一根轴，`连接点1` 才能稳定落在
/// 起点侧 —— 否则段反向绘制时两个端口会互换，`VAL-PIPE-020` 的
/// `end_A.x < start_B.x` 会翻。
fn fitting_axis(table: &TopologyTable, fitting: &crate::pid::topology::FittingRecord) -> [f64; 2] {
    let port = |name: &str| fitting.ports.get(name).and_then(|slot| slot.clone());
    let outer = |segment_text: &str, port_name: &str| -> Option<[f64; 2]> {
        let handle = parse_handle(segment_text)?;
        let segment = table
            .pipe_segments
            .iter()
            .find(|s| s.handle == handle_text(handle))?;
        let peer = format!("{}:{}", fitting.handle, port_name);
        if segment.start.peer.as_deref() == Some(peer.as_str()) {
            Some(segment.end.point)
        } else if segment.end.peer.as_deref() == Some(peer.as_str()) {
            Some(segment.start.point)
        } else {
            None
        }
    };
    let (Some(a), Some(b)) = (
        port(VALVE_PORT_NAMES[0]).and_then(|text| outer(&text, VALVE_PORT_NAMES[0])),
        port(VALVE_PORT_NAMES[1]).and_then(|text| outer(&text, VALVE_PORT_NAMES[1])),
    ) else {
        return [1.0, 0.0];
    };
    let dir = [b[0] - a[0], b[1] - a[1]];
    let length = dir[0].hypot(dir[1]);
    if length > 1e-12 {
        [dir[0] / length, dir[1] / length]
    } else {
        [1.0, 0.0]
    }
}

/// `peer` 指向的端口平移后的坐标。`peer` 形如 `<handle>:<端口名>`。
fn port_of(port_points: &PortTables, peer: Option<&str>) -> Option<[f64; 2]> {
    let peer = peer?;
    let (handle, port) = peer.rsplit_once(':')?;
    let (_, points) = port_points
        .iter()
        .find(|(candidate, _)| candidate == handle)?;
    points
        .iter()
        .find(|(name, _)| name == port)
        .map(|(_, point)| *point)
}

/// 某段落在管件某一端口上的端点坐标（内侧端点）。按 `peer` 判方向，不假定
/// 它一定是 `end` —— 管段可能反向绘制。
fn inner_point(
    plan: &RefillPlan,
    segment_text: &str,
    fitting_text: &str,
    port: &str,
) -> Option<[f64; 2]> {
    let handle = parse_handle(segment_text)?;
    let (_, start, end) = plan
        .segments
        .iter()
        .find(|(candidate, _, _)| *candidate == handle)?;
    let peer = format!("{fitting_text}:{port}");
    if start.peer.as_deref() == Some(peer.as_str()) {
        Some(start.point)
    } else if end.peer.as_deref() == Some(peer.as_str()) {
        Some(end.point)
    } else {
        None
    }
}

/// 段的方向单位向量（平移后的新方向）。
fn axis_of(plan: &RefillPlan, segment_text: &str) -> Option<[f64; 2]> {
    let handle = parse_handle(segment_text)?;
    let (_, start, end) = plan
        .segments
        .iter()
        .find(|(candidate, _, _)| *candidate == handle)?;
    let dir = [end.point[0] - start.point[0], end.point[1] - start.point[1]];
    let length = dir[0].hypot(dir[1]);
    (length > 1e-12).then(|| [dir[0] / length, dir[1] / length])
}
