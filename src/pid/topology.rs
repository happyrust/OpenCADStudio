//! 管线拓扑表：管线 → 管路 → 管段三级对象、管件端口绑定、页间连接符对端。
//!
//! 落盘载体是 XRecord key [`crate::pid::graph_data::PID_AUTHOR_TOPOLOGY`]，
//! 实体 handle 做外键。JSON 形状见 `validation-contract.md` §0.4（逐字为判据，
//! 不要另起字段别名）。
//!
//! 本模块只做「表的读写 + 结构性维护」，属性下传在 [`crate::pid::propagate`]，
//! 位号生成在 [`crate::pid::tag_engine`]，实体创建与撤销由调用方负责。
//!
//! 三条落盘约定：
//!
//! 1. **整表一个 JSON 对象**，交给 [`graph_data::store`] 作为一条 entry 写入。
//!    `graph_data` 负责分片、`kind=`/`key=` 前缀与空表删除。
//! 2. **handle 一律十六进制字符串**，与 REST 的 `handle` 字段同编码。
//! 3. **两种 `peer` 语义不同**：`pipe_segments[].start/end.peer` 是
//!    `<本图 handle>:<端口名>`；`connectors[].peer` 是 `<drawing_id>:<handle>`。

use std::collections::BTreeMap;

use codec::{Document, Handle};
use serde::{Deserialize, Serialize};

use super::graph_data::{self, PID_AUTHOR_TOPOLOGY};

/// 拓扑表版本号。
pub const VERSION: u32 = 1;

/// 管线实体的拓扑角色。
pub const ROLE_PIPE_LINE: &str = "pipe_line";
/// 管路实体的拓扑角色。
pub const ROLE_PIPE_RUN: &str = "pipe_run";
/// 管段实体的拓扑角色。
pub const ROLE_PIPE_SEGMENT: &str = "pipe_segment";
/// 管件（阀门、打断符）实体的拓扑角色。
pub const ROLE_FITTING: &str = "fitting";
/// 信号线实体的拓扑角色。
pub const ROLE_SIGNAL_LINE: &str = "signal_line";
/// 流入连接符实体的拓扑角色。
pub const ROLE_CONNECTOR_IN: &str = "connector_in";
/// 流出连接符实体的拓扑角色。
pub const ROLE_CONNECTOR_OUT: &str = "connector_out";

/// 管线的作者类型编码。
pub const TYPE_PIPE_LINE: &str = "PIPE_LINE";
/// 管路的作者类型编码。
pub const TYPE_PIPE_RUN: &str = "PIPE_RUN";
/// 管段的作者类型编码。
pub const TYPE_PIPE_SEGMENT: &str = "PIPE_SEGMENT";
/// 信号线的作者类型编码。
pub const TYPE_SIGNAL_LINE: &str = "SIGNAL_LINE";
/// 管件的作者类型编码（无独立位号）。
pub const TYPE_FITTING: &str = "FITTING";
/// 阀门的作者类型编码（有独立位号 `V-{序号}`）。
pub const TYPE_VALVE: &str = "VALVE";

/// 阀门两个拓扑端口的别名，顺序即 `连接点1` → `连接点2`。
pub const VALVE_PORT_NAMES: [&str; 2] = ["连接点1", "连接点2"];

/// 页间连接符的四个 `kind` 字面量，逐字照抄契约（`PIDCONNECTOR <kind>`）。
pub const KIND_PIPE_IN: &str = "pipe_in";
/// 管道流出连接符。
pub const KIND_PIPE_OUT: &str = "pipe_out";
/// 信号线流入连接符。
pub const KIND_SIGNAL_IN: &str = "signal_in";
/// 信号线流出连接符。
pub const KIND_SIGNAL_OUT: &str = "signal_out";

/// 同一线号同图第二个流入连接符的错误码（HTTP 409）。
pub const DUPLICATE_INFLOW_CONNECTOR: &str = "DUPLICATE_INFLOW_CONNECTOR";
/// 管道连接符选到信号线（或反向）的错误码（HTTP 400）。
pub const CONNECTOR_KIND_MISMATCH: &str = "CONNECTOR_KIND_MISMATCH";
/// 落点不在任何管线/信号线上。
pub const CONNECTOR_NO_LINE: &str = "CONNECTOR_NO_LINE";
/// 配对的两个连接符种类不相配（都流入、都流出，或管道对信号线）。
pub const CONNECTOR_PEER_INVALID: &str = "CONNECTOR_PEER_INVALID";

/// 四种页间连接符。
///
/// `topology` 只有 `connector_in` / `connector_out` 两个取值，管道与信号线
/// 靠实体 XDATA 的 `type` 键区分（`PIPE_LINE` vs `SIGNAL_LINE`）——
/// `VAL-PIPE-023` 明写「若 `topology` + `type` 无法区分这 4 种，记 FAIL」。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConnectorKind {
    /// 管道流入。
    PipeIn,
    /// 管道流出。
    PipeOut,
    /// 信号线流入。
    SignalIn,
    /// 信号线流出。
    SignalOut,
}

impl ConnectorKind {
    /// 从命令参数解析。大小写不敏感；非法值返回 `None`。
    #[must_use]
    pub fn parse(text: &str) -> Option<Self> {
        match text.trim().to_ascii_lowercase().as_str() {
            KIND_PIPE_IN => Some(Self::PipeIn),
            KIND_PIPE_OUT => Some(Self::PipeOut),
            KIND_SIGNAL_IN => Some(Self::SignalIn),
            KIND_SIGNAL_OUT => Some(Self::SignalOut),
            _ => None,
        }
    }

    /// 契约字面量，写进拓扑表的 `kind`。
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::PipeIn => KIND_PIPE_IN,
            Self::PipeOut => KIND_PIPE_OUT,
            Self::SignalIn => KIND_SIGNAL_IN,
            Self::SignalOut => KIND_SIGNAL_OUT,
        }
    }

    /// 实体 XDATA 的 `topology` 角色。
    #[must_use]
    pub fn role(self) -> &'static str {
        match self {
            Self::PipeIn | Self::SignalIn => ROLE_CONNECTOR_IN,
            Self::PipeOut | Self::SignalOut => ROLE_CONNECTOR_OUT,
        }
    }

    /// 实体 XDATA 的 `type` 编码，也是它与线体种类匹配的判据。
    #[must_use]
    pub fn type_code(self) -> &'static str {
        match self {
            Self::PipeIn | Self::PipeOut => TYPE_PIPE_LINE,
            Self::SignalIn | Self::SignalOut => TYPE_SIGNAL_LINE,
        }
    }

    /// 载体线的拓扑角色：管道连接符挂在 `pipe_line` 上，信号线连接符挂在
    /// `signal_line` 上。
    #[must_use]
    pub fn host_role(self) -> &'static str {
        match self {
            Self::PipeIn | Self::PipeOut => ROLE_PIPE_LINE,
            Self::SignalIn | Self::SignalOut => ROLE_SIGNAL_LINE,
        }
    }

    /// 是否是流入连接符（同一线号同图至多一个）。
    #[must_use]
    pub fn is_inflow(self) -> bool {
        matches!(self, Self::PipeIn | Self::SignalIn)
    }
}

/// `handle` → 十六进制字符串，与 REST 的 `handle` 字段同编码。
#[must_use]
pub fn handle_text(handle: Handle) -> String {
    format!("{:X}", handle.value())
}

/// 十六进制字符串 → `handle`。接受 `0x` 前缀。
#[must_use]
pub fn parse_handle(text: &str) -> Option<Handle> {
    u64::from_str_radix(text.trim().trim_start_matches("0x"), 16)
        .ok()
        .map(Handle::new)
}

// ── 表结构 ───────────────────────────────────────────────────────────────

/// 拓扑表根对象。字段名逐字照抄契约 §0.4。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TopologyTable {
    /// 版本号，当前恒为 [`VERSION`]。
    #[serde(default = "default_version")]
    pub version: u32,
    /// 管线。
    #[serde(default)]
    pub pipe_lines: Vec<PipeLineRecord>,
    /// 管路。
    #[serde(default)]
    pub pipe_runs: Vec<PipeRunRecord>,
    /// 管段。
    #[serde(default)]
    pub pipe_segments: Vec<PipeSegmentRecord>,
    /// 管件（阀门、打断符）。
    #[serde(default)]
    pub fittings: Vec<FittingRecord>,
    /// 页间连接符。
    #[serde(default)]
    pub connectors: Vec<ConnectorRecord>,
}

fn default_version() -> u32 {
    VERSION
}

impl Default for TopologyTable {
    fn default() -> Self {
        Self {
            version: VERSION,
            pipe_lines: Vec::new(),
            pipe_runs: Vec::new(),
            pipe_segments: Vec::new(),
            fittings: Vec::new(),
            connectors: Vec::new(),
        }
    }
}

impl TopologyTable {
    /// 表里是否没有任何拓扑对象（空表与无表同义，见 D17）。
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.pipe_lines.is_empty()
            && self.pipe_runs.is_empty()
            && self.pipe_segments.is_empty()
            && self.fittings.is_empty()
            && self.connectors.is_empty()
    }

    /// 从文档读回。无表、或条目解析失败时返回空表。
    #[must_use]
    pub fn load(doc: &Document) -> Self {
        let Some(items) = graph_data::load(doc, PID_AUTHOR_TOPOLOGY) else {
            return Self::default();
        };
        let Some(first) = items.into_iter().next() else {
            return Self::default();
        };
        let mut table: Self = serde_json::from_value(first).unwrap_or_default();
        if table.version == 0 {
            table.version = VERSION;
        }
        table
    }

    /// 整表写回。空表走 `graph_data::store(&[])`，它会删掉该 key 的 XRecord。
    pub fn store(&self, doc: &mut Document) {
        if self.is_empty() {
            graph_data::store(doc, PID_AUTHOR_TOPOLOGY, &[]);
            return;
        }
        let Ok(value) = serde_json::to_value(self) else {
            return;
        };
        graph_data::store(doc, PID_AUTHOR_TOPOLOGY, &[value]);
    }

    /// 按位号找管线。
    #[must_use]
    pub fn line_by_tag(&self, tag: &str) -> Option<&PipeLineRecord> {
        self.pipe_lines.iter().find(|line| line.tag == tag)
    }

    /// 按位号找管线（可变）。
    pub fn line_by_tag_mut(&mut self, tag: &str) -> Option<&mut PipeLineRecord> {
        self.pipe_lines.iter_mut().find(|line| line.tag == tag)
    }

    /// 按 handle 找管路。
    #[must_use]
    pub fn run_by_handle(&self, handle: Handle) -> Option<&PipeRunRecord> {
        let text = handle_text(handle);
        self.pipe_runs.iter().find(|run| run.handle == text)
    }

    /// 按 handle 找管段。
    #[must_use]
    pub fn segment_by_handle(&self, handle: Handle) -> Option<&PipeSegmentRecord> {
        let text = handle_text(handle);
        self.pipe_segments
            .iter()
            .find(|segment| segment.handle == text)
    }

    /// 按 handle 找管段（可变）。
    pub fn segment_by_handle_mut(&mut self, handle: Handle) -> Option<&mut PipeSegmentRecord> {
        let text = handle_text(handle);
        self.pipe_segments
            .iter_mut()
            .find(|segment| segment.handle == text)
    }

    /// 文档中**最后创建**的管线（`pipe_lines` 的顺序即创建序）。
    #[must_use]
    pub fn last_line(&self) -> Option<&PipeLineRecord> {
        self.pipe_lines.last()
    }

    /// 某条管线的管路位号列表（按创建序）。
    #[must_use]
    pub fn runs_of(&self, line_tag: &str) -> Vec<String> {
        self.line_by_tag(line_tag)
            .map(|line| line.runs.clone())
            .unwrap_or_default()
    }

    /// 按 handle 反查管线位号。
    #[must_use]
    pub fn line_tag_of_handle(&self, handle: Handle) -> Option<String> {
        let text = handle_text(handle);
        self.pipe_lines
            .iter()
            .find(|line| line.handle == text)
            .map(|line| line.tag.clone())
    }

    /// 按 handle 反查管路位号。
    #[must_use]
    pub fn run_tag_of_handle(&self, handle: Handle) -> Option<String> {
        self.run_by_handle(handle).map(|run| run.tag.clone())
    }

    /// 一条管线的全部管路 handle。
    ///
    /// 两个来源取并集：管线自己的 `runs` 位号表，以及 `line_tag` 指回它的
    /// 管路记录。表是外部写入的（`xdata_set` 之后由下传重算），两者可能短暂
    /// 不一致，下传链不能因此漏掉一条管路。
    #[must_use]
    pub fn run_handles_of(&self, line_tag: &str) -> Vec<Handle> {
        let mut tags: Vec<&str> = self
            .line_by_tag(line_tag)
            .map(|line| line.runs.iter().map(String::as_str).collect())
            .unwrap_or_default();
        for run in &self.pipe_runs {
            if run.line_tag == line_tag && !tags.contains(&run.tag.as_str()) {
                tags.push(run.tag.as_str());
            }
        }
        tags.iter()
            .filter_map(|tag| {
                self.pipe_runs
                    .iter()
                    .find(|run| &run.tag == tag)
                    .and_then(|run| parse_handle(&run.handle))
            })
            .collect()
    }

    /// 一条管路的全部管段 handle。两个来源取并集，理由同 [`Self::run_handles_of`]。
    #[must_use]
    pub fn segment_handles_of(&self, run_tag: &str) -> Vec<Handle> {
        let mut texts: Vec<&str> = self
            .pipe_runs
            .iter()
            .find(|run| run.tag == run_tag)
            .map(|run| run.segments.iter().map(String::as_str).collect())
            .unwrap_or_default();
        for segment in &self.pipe_segments {
            if segment.run_tag == run_tag && !texts.contains(&segment.handle.as_str()) {
                texts.push(segment.handle.as_str());
            }
        }
        texts
            .iter()
            .filter_map(|text| parse_handle(text))
            .collect()
    }

    /// 用文档里 `PID_AUTHOR` 的现值刷新表内的 `props` / `overrides` 副本。
    ///
    /// 权威值在实体 XDATA 上，表里那两份只是副本（`VAL-PIPE-007` 读 XDATA，
    /// `VAL-PIPE-008` 读表里的 `overrides`），所以每次下传写入之后都要回灌
    /// 一次，两者才不会漂移。返回表是否有变化。
    pub fn sync_props_from_document(&mut self, doc: &Document) -> bool {
        use crate::pid::propagate::{overrides_of, props_of};
        let mut changed = false;
        for line in &mut self.pipe_lines {
            let Some(handle) = parse_handle(&line.handle) else {
                continue;
            };
            let props = props_of(doc, handle);
            if props != line.props {
                line.props = props;
                changed = true;
            }
        }
        for run in &mut self.pipe_runs {
            let Some(handle) = parse_handle(&run.handle) else {
                continue;
            };
            let props = props_of(doc, handle);
            if props != run.props {
                run.props = props;
                changed = true;
            }
            let overrides: Vec<String> = overrides_of(doc, handle).into_iter().collect();
            if overrides != run.overrides {
                run.overrides = overrides;
                changed = true;
            }
        }
        changed
    }

    /// 追加一条管线，返回它的可变引用。
    pub fn append_line(
        &mut self,
        tag: &str,
        handle: Handle,
        props: BTreeMap<String, String>,
    ) -> &mut PipeLineRecord {
        self.pipe_lines.push(PipeLineRecord {
            tag: tag.to_string(),
            handle: handle_text(handle),
            props,
            runs: Vec::new(),
            next_run_ordinal: 1,
        });
        self.pipe_lines.last_mut().expect("just pushed")
    }

    /// 追加一条管路，并把它登记进宿主管线的 `runs`。
    pub fn append_run(
        &mut self,
        line_tag: &str,
        run_tag: &str,
        handle: Handle,
        props: BTreeMap<String, String>,
        overrides: Vec<String>,
    ) {
        self.pipe_runs.push(PipeRunRecord {
            tag: run_tag.to_string(),
            handle: handle_text(handle),
            line_tag: line_tag.to_string(),
            props,
            overrides,
            segments: Vec::new(),
        });
        if let Some(line) = self.line_by_tag_mut(line_tag) {
            line.runs.push(run_tag.to_string());
        }
    }

    /// 追加一条管段，并把它登记进宿主管路的 `segments`。
    pub fn append_segment(
        &mut self,
        run_tag: &str,
        handle: Handle,
        start: SegmentEnd,
        end: SegmentEnd,
    ) {
        self.pipe_segments.push(PipeSegmentRecord {
            handle: handle_text(handle),
            run_tag: run_tag.to_string(),
            start,
            end,
        });
        if let Some(run) = self.pipe_runs.iter_mut().find(|run| run.tag == run_tag) {
            run.segments.push(handle_text(handle));
        }
    }

    /// 删除 handle 已不存在的对象，并清掉管线 `runs` 与管路 `segments` 里的
    /// 悬空引用。返回删掉几条。
    ///
    /// 调用点：每次创建新对象前。删除实体时没有钩子更新拓扑表，所以这里
    /// 用「文档里找不到该 handle」作为唯一判据 —— 契约 §0.4 的语义事实是
    /// 连接关系，悬空引用不是事实。
    pub fn prune(&mut self, doc: &Document) -> usize {
        let exists = |text: &str| -> bool {
            parse_handle(text).is_some_and(|handle| doc.get_entity(handle).is_some())
        };
        let before = self.pipe_lines.len()
            + self.pipe_runs.len()
            + self.pipe_segments.len()
            + self.fittings.len()
            + self.connectors.len();

        self.pipe_lines.retain(|line| exists(&line.handle));
        self.pipe_runs.retain(|run| exists(&run.handle));
        self.pipe_segments.retain(|segment| exists(&segment.handle));
        self.fittings.retain(|fitting| exists(&fitting.handle));
        self.connectors.retain(|connector| exists(&connector.handle));

        let run_tags: Vec<String> = self.pipe_runs.iter().map(|r| r.tag.clone()).collect();
        let segment_handles: Vec<String> =
            self.pipe_segments.iter().map(|s| s.handle.clone()).collect();
        for line in &mut self.pipe_lines {
            line.runs.retain(|tag| run_tags.contains(tag));
        }
        for run in &mut self.pipe_runs {
            run.segments.retain(|h| segment_handles.contains(h));
        }

        let after = self.pipe_lines.len()
            + self.pipe_runs.len()
            + self.pipe_segments.len()
            + self.fittings.len()
            + self.connectors.len();
        before.saturating_sub(after)
    }

    /// 一条管路上各管段的两个端点，按 `segments` 顺序。
    #[must_use]
    pub fn segment_ends_of(&self, run_tag: &str) -> Vec<(SegmentEnd, SegmentEnd)> {
        let Some(run) = self.pipe_runs.iter().find(|run| run.tag == run_tag) else {
            return Vec::new();
        };
        run.segments
            .iter()
            .filter_map(|handle| {
                self.pipe_segments
                    .iter()
                    .find(|segment| &segment.handle == handle)
                    .map(|segment| (segment.start.clone(), segment.end.clone()))
            })
            .collect()
    }

    /// 按 handle 找连接符。
    #[must_use]
    pub fn connector_by_handle(&self, handle: Handle) -> Option<&ConnectorRecord> {
        let text = handle_text(handle);
        self.connectors.iter().find(|item| item.handle == text)
    }

    /// 按 handle 找连接符（可变）。
    pub fn connector_by_handle_mut(&mut self, handle: Handle) -> Option<&mut ConnectorRecord> {
        let text = handle_text(handle);
        self.connectors.iter_mut().find(|item| item.handle == text)
    }

    /// 同一线号在同一张图上是否已有该方向的连接符。
    ///
    /// 契约只约束**流入**方向（`VAL-PIPE-024`），流出可重复。
    #[must_use]
    pub fn has_inflow_connector(&self, line_tag: &str) -> bool {
        self.connectors.iter().any(|item| {
            item.line_tag == line_tag
                && item
                    .kind_of()
                    .is_some_and(|kind| kind.is_inflow())
        })
    }

    /// 按线号找该线上全部连接符。
    #[must_use]
    pub fn connectors_of(&self, line_tag: &str) -> Vec<&ConnectorRecord> {
        self.connectors
            .iter()
            .filter(|item| item.line_tag == line_tag)
            .collect()
    }

    /// 落一条连接符。同一 handle 已存在时原地替换（重跑幂等）。
    pub fn upsert_connector(&mut self, record: ConnectorRecord) {
        if let Some(existing) = self
            .connectors
            .iter_mut()
            .find(|item| item.handle == record.handle)
        {
            *existing = record;
            return;
        }
        self.connectors.push(record);
    }
}

/// 一条管线。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PipeLineRecord {
    /// 管线位号，如 `L-1001`。
    pub tag: String,
    /// 实体 handle，十六进制字符串。
    pub handle: String,
    /// 五个下传字段的当前值。
    #[serde(default)]
    pub props: BTreeMap<String, String>,
    /// 它的管路位号，按创建序。
    #[serde(default)]
    pub runs: Vec<String>,
    /// 下一条管路的后缀序号（1 起）。**持久化**，删除管路后不回退，
    /// 这就是「不复用已删编号」的唯一依据。
    #[serde(default = "default_ordinal")]
    pub next_run_ordinal: u32,
}

fn default_ordinal() -> u32 {
    1
}

/// 一条管路。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PipeRunRecord {
    /// 管路位号，如 `L-1001-01`。
    pub tag: String,
    /// 实体 handle，十六进制字符串。
    pub handle: String,
    /// 宿主管线位号。
    pub line_tag: String,
    /// 五个下传字段的当前值。
    #[serde(default)]
    pub props: BTreeMap<String, String>,
    /// 被手工改过、不再接受下传的字段名。
    #[serde(default)]
    pub overrides: Vec<String>,
    /// 它的管段 handle，按创建序。
    #[serde(default)]
    pub segments: Vec<String>,
}

/// 管段的一个端点。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SegmentEnd {
    /// 平面坐标 `[x, y]`。
    pub point: [f64; 2],
    /// 同图内连接：`<handle>:<端口名>`；`null` 表示未连接。
    #[serde(default)]
    pub peer: Option<String>,
}

impl SegmentEnd {
    /// 一个未连接的端点。
    #[must_use]
    pub fn open(point: [f64; 2]) -> Self {
        Self { point, peer: None }
    }

    /// 一个连到 `<handle>:<端口名>` 的端点。
    #[must_use]
    pub fn joined(point: [f64; 2], handle: Handle, port: &str) -> Self {
        Self {
            point,
            peer: Some(format!("{}:{}", handle_text(handle), port)),
        }
    }
}

/// 一条管段。**没有 `tag` 字段** —— 管段无独立位号（`VAL-PIPE-005`）。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PipeSegmentRecord {
    /// 实体 handle，十六进制字符串。
    pub handle: String,
    /// 宿主管路位号。
    pub run_tag: String,
    /// 起点。
    pub start: SegmentEnd,
    /// 终点。
    pub end: SegmentEnd,
}

/// 一个管件（阀门、打断符）。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FittingRecord {
    /// 实体 handle，十六进制字符串。
    pub handle: String,
    /// 宿主管路位号。
    pub run_tag: String,
    /// 管件类型（符号 id，如 `gate_valve`）。
    pub fitting_type: String,
    /// 独立位号；无独立位号的管件不落此键。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tag: Option<String>,
    /// 端口 → 绑定管段 handle（`null` 表示该端口未接）。
    #[serde(default)]
    pub ports: BTreeMap<String, Option<String>>,
}

/// 一个页间连接符。`peer` 是 `<drawing_id>:<handle>`，与管段端点的
/// `<handle>:<端口名>` 不是一回事。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ConnectorRecord {
    /// 实体 handle，十六进制字符串。
    pub handle: String,
    /// `pipe_in` / `pipe_out` / `signal_in` / `signal_out`。
    pub kind: String,
    /// 所在线号。
    pub line_tag: String,
    /// 对端 `<drawing_id>:<handle>`。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub peer: Option<String>,
    /// 对端图纸标识（`peer` 冒号前那一段的副本，便于只读跳转）。
    /// `VAL-PIPE-027` 要求连接符条目含可读的图纸跳转信息。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub peer_drawing: Option<String>,
    /// 上游/下游头尾信息：本侧线体在配对前的首尾 handle 位号。
    /// 管道连接符传宿主管路位号，信号线连接符传宿主线位号。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub upstream: Option<String>,
    /// 下游头尾信息。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub downstream: Option<String>,
}

impl ConnectorRecord {
    /// 一条尚未配对的连接符。
    #[must_use]
    pub fn new(handle: String, kind: ConnectorKind, line_tag: String) -> Self {
        Self {
            handle,
            kind: kind.as_str().to_string(),
            line_tag,
            peer: None,
            peer_drawing: None,
            upstream: None,
            downstream: None,
        }
    }

    /// 本条记录的 `kind` 解析回枚举。
    #[must_use]
    pub fn kind_of(&self) -> Option<ConnectorKind> {
        ConnectorKind::parse(&self.kind)
    }
}

// ── 命令请求 ─────────────────────────────────────────────────────────────

/// `PIDPIPE` 的子模式。三种子模式共用同一个命令名，由首参数区分。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PipeMode {
    /// 建管线：两点 + enter。
    Line,
    /// 建管路：两点以上 + enter；`line_tag` 省略时取最后创建的管线。
    Run {
        /// 宿主管线位号。
        line_tag: Option<String>,
    },
    /// 建信号线：两点以上 + enter；`tag` 省略时自动 `S-{序号}`（2001 起）。
    Signal {
        /// 信号线位号。
        tag: Option<String>,
    },
}

/// 一个 `PIDPIPE` 请求：模式 + 拾取到的点。
///
/// 命令只负责收点；建实体、写 XDATA、维护拓扑表都在
/// `src/app/commands/pidpipe.rs` 的提交路径里完成。
#[derive(Debug, Clone, PartialEq)]
pub struct PipeRequest {
    /// 子模式。
    pub mode: PipeMode,
    /// 世界坐标点，按拾取顺序。
    pub points: Vec<[f64; 3]>,
}

/// 一个 `PIDFITTING` 请求：符号 + 拾取到的落点。
#[derive(Debug, Clone, PartialEq)]
pub struct FittingRequest {
    /// 符号 id（`gate_valve`）或块名（`PID_SYM_valve_gate_valve`）。
    pub symbol: String,
    /// 符号块名，已解析。
    pub block: String,
    /// 世界坐标落点。
    pub point: [f64; 3],
}

/// 一个 `PIDCONNECTOR` 请求：连接符种类 + 落点 + 可选对端。
#[derive(Debug, Clone, PartialEq)]
pub struct ConnectorRequest {
    /// 四种连接符之一。
    pub kind: ConnectorKind,
    /// 世界坐标落点（必须落在宿主管线/信号线上）。
    pub point: [f64; 3],
    /// 可选对端 `<drawing_id>:<handle>`；给了就顺手配对。
    pub peer: Option<String>,
}

/// 把 `(drawing_id, handle)` 拼成跨图 `peer` 文本。
#[must_use]
pub fn peer_text(drawing_id: u64, handle: Handle) -> String {
    format!("{drawing_id}:{}", handle_text(handle))
}

/// 解析跨图 `peer` 文本为 `(drawing_id, handle)`。
///
/// `drawing_id` 取 `GET /api/v1/documents` 的 `document_id`（不是文件名），
/// 冒号后是本图连接符的十六进制 handle（大小写不敏感）。
#[must_use]
pub fn parse_peer(text: &str) -> Option<(u64, Handle)> {
    let (drawing, handle) = text.trim().split_once(':')?;
    let drawing = drawing.trim().parse::<u64>().ok()?;
    parse_handle(handle).map(|handle| (drawing, handle))
}

/// 一个 P&ID 拓扑命令交给宿主的提交请求。
///
/// 命令本身只收点，落盘（建实体、写 `PID_AUTHOR`、维护拓扑表）由
/// `src/app/commands/pidpipe.rs` 的 `commit_pid_pipeline` 完成 —— 它需要
/// `&mut Document`，而 `CadCommand` 没有文档访问权。
#[derive(Debug, Clone, PartialEq)]
pub enum PipeCommit {
    /// `PIDPIPE` 的三种子模式。
    Pipe(PipeRequest),
    /// `PIDFITTING`：在命中的管段上插管件。
    Fitting(FittingRequest),
    /// `PIDCONNECTOR`：在命中的管线/信号线上插页间连接符。
    Connector(ConnectorRequest),
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pid::persist::PidAuthor;
    use codec::entities::Line;
    use codec::types::Vector3;

    fn line(y: f64) -> codec::EntityType {
        codec::EntityType::Line(Line::from_points(
            Vector3::new(0.0, y, 0.0),
            Vector3::new(200.0, y, 0.0),
        ))
    }

    fn author(doc: &mut Document, handle: Handle, type_code: &str, tag: &str, topology: &str) {
        PidAuthor {
            type_code: type_code.to_string(),
            tag: tag.to_string(),
            tag_source: "auto".to_string(),
            tag_status: "ok".to_string(),
            topology: topology.to_string(),
            ..PidAuthor::default()
        }
        .store(doc, handle);
    }

    /// 空表与无表同义：`store` 空表后 `load` 回来仍是空表，且文档里没有
    /// 该 key 的 XRecord。
    #[test]
    fn an_empty_table_stores_nothing_and_loads_back_empty() {
        let mut doc = Document::new();
        let table = TopologyTable::default();
        table.store(&mut doc);
        assert!(
            doc.xrecord(doc.header.model_space_block_handle, PID_AUTHOR_TOPOLOGY)
                .is_none(),
            "空表不该留 XRecord"
        );
        assert!(TopologyTable::load(&doc).is_empty());
    }

    /// 整表往返：管线、管路、管段、管件、连接符逐字段相等。
    #[test]
    fn the_table_roundtrips_through_the_xrecord() {
        let mut doc = Document::new();
        let line_handle = doc.add_entity(line(0.0)).unwrap();
        let run_handle = doc.add_entity(line(10.0)).unwrap();
        let seg_a = doc.add_entity(line(20.0)).unwrap();
        let seg_b = doc.add_entity(line(30.0)).unwrap();
        let valve = doc.add_entity(line(40.0)).unwrap();
        author(&mut doc, line_handle, TYPE_PIPE_LINE, "L-1001", ROLE_PIPE_LINE);

        let mut table = TopologyTable::default();
        table.append_line("L-1001", line_handle, BTreeMap::new());
        table.append_run(
            "L-1001",
            "L-1001-01",
            run_handle,
            BTreeMap::new(),
            vec!["公称直径".to_string()],
        );
        table.append_segment(
            "L-1001-01",
            seg_a,
            SegmentEnd::open([0.0, 0.0]),
            SegmentEnd::joined([100.0, 0.0], valve, VALVE_PORT_NAMES[0]),
        );
        table.append_segment(
            "L-1001-01",
            seg_b,
            SegmentEnd::joined([100.0, 0.0], valve, VALVE_PORT_NAMES[1]),
            SegmentEnd::open([200.0, 0.0]),
        );
        let mut ports = BTreeMap::new();
        ports.insert(VALVE_PORT_NAMES[0].to_string(), Some(handle_text(seg_a)));
        ports.insert(VALVE_PORT_NAMES[1].to_string(), Some(handle_text(seg_b)));
        table.fittings.push(FittingRecord {
            handle: handle_text(valve),
            run_tag: "L-1001-01".to_string(),
            fitting_type: "gate_valve".to_string(),
            tag: Some("V-1001".to_string()),
            ports,
        });
        table.connectors.push(ConnectorRecord {
            handle: handle_text(line_handle),
            kind: "pipe_in".to_string(),
            line_tag: "L-1001".to_string(),
            peer: Some("7:2B7".to_string()),
            peer_drawing: Some("7".to_string()),
            upstream: Some("L-1001".to_string()),
            downstream: Some("L-2001".to_string()),
        });
        table.store(&mut doc);

        let back = TopologyTable::load(&doc);
        assert_eq!(back, table);
        assert_eq!(back.pipe_lines[0].runs, vec!["L-1001-01".to_string()]);
        assert_eq!(
            back.pipe_runs[0].segments,
            vec![handle_text(seg_a), handle_text(seg_b)]
        );
        assert_eq!(back.pipe_segments.len(), 2);
        assert_eq!(back.pipe_segments[0].end.peer.as_deref(), Some(
            format!("{}:连接点1", handle_text(valve)).as_str()
        ));
        assert_eq!(back.fittings[0].ports.len(), 2);
        assert_eq!(back.connectors[0].peer.as_deref(), Some("7:2B7"));
        assert_eq!(back.connectors[0].peer_drawing.as_deref(), Some("7"));
        assert_eq!(back.connectors[0].upstream.as_deref(), Some("L-1001"));
    }

    /// 管段记录序列化后**没有** `tag` 键（`VAL-PIPE-005` 的拓扑表判据）。
    #[test]
    fn a_segment_record_carries_no_tag_field() {
        let record = PipeSegmentRecord {
            handle: "1B1".to_string(),
            run_tag: "L-1001-01".to_string(),
            start: SegmentEnd::open([0.0, 0.0]),
            end: SegmentEnd::open([200.0, 0.0]),
        };
        let json = serde_json::to_value(&record).unwrap();
        assert!(json.get("tag").is_none(), "管段不该有 tag 字段：{json}");
        assert_eq!(json["run_tag"], "L-1001-01");
        assert_eq!(json["start"]["point"], serde_json::json!([0.0, 0.0]));
        assert!(json["start"]["peer"].is_null());
    }

    /// 悬空引用被清掉：实体删了，拓扑表里的记录与父级引用一并消失。
    #[test]
    fn prune_drops_records_whose_entity_is_gone() {
        let mut doc = Document::new();
        let line_handle = doc.add_entity(line(0.0)).unwrap();
        let run_handle = doc.add_entity(line(10.0)).unwrap();
        let seg = doc.add_entity(line(20.0)).unwrap();

        let mut table = TopologyTable::default();
        table.append_line("L-1001", line_handle, BTreeMap::new());
        table.append_run("L-1001", "L-1001-01", run_handle, BTreeMap::new(), vec![]);
        table.append_segment(
            "L-1001-01",
            seg,
            SegmentEnd::open([0.0, 0.0]),
            SegmentEnd::open([200.0, 0.0]),
        );
        assert_eq!(table.prune(&doc), 0, "实体都在时不该删任何记录");

        doc.remove_entity(seg);
        assert_eq!(table.prune(&doc), 1);
        assert!(table.pipe_segments.is_empty());
        assert!(
            table.pipe_runs[0].segments.is_empty(),
            "管路的 segments 不该留下悬空 handle：{:?}",
            table.pipe_runs[0].segments
        );
        assert_eq!(table.pipe_lines[0].runs, vec!["L-1001-01".to_string()]);
    }

    /// `next_run_ordinal` 落盘往返：它是「不复用已删编号」的唯一依据。
    #[test]
    fn the_run_ordinal_survives_a_roundtrip() {
        let mut doc = Document::new();
        let line_handle = doc.add_entity(line(0.0)).unwrap();
        let mut table = TopologyTable::default();
        table.append_line("L-1001", line_handle, BTreeMap::new());
        table.line_by_tag_mut("L-1001").unwrap().next_run_ordinal = 3;
        table.store(&mut doc);
        assert_eq!(
            TopologyTable::load(&doc).line_by_tag("L-1001").unwrap().next_run_ordinal,
            3
        );
    }

    /// handle 编解码与 REST 同形。
    #[test]
    fn handles_round_trip_through_hex_text() {
        let handle = Handle::new(0x1A2F);
        assert_eq!(handle_text(handle), "1A2F");
        assert_eq!(parse_handle("1A2F"), Some(handle));
        assert_eq!(parse_handle("0x1a2f"), Some(handle));
        assert_eq!(parse_handle("not-hex"), None);
    }
}
