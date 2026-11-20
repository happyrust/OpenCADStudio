//! 项目主数据（`SPEC-pid-master-data.md`）：公称直径、管道等级、绝热等级、
//! 流体介质、PBS 节点树和项目锁定的图例标准。
//!
//! 这里是数据对象与 JSON 形状（§3.1）、默认内容、校验（§3.6，错误带出错
//! 位置）和属性值与主数据的比法（§3.5）。纯数据加纯函数，不碰图纸。

use std::collections::{HashMap, HashSet};
use std::fmt;

use serde_json::{json, Map, Value};

/// 程序认识的格式版本（§3.1 的 `version`）。
pub const VERSION: u64 = 1;
/// 默认内容的公称直径序列（§3.1）。
pub const DEFAULT_DIAMETERS: [u32; 19] = [
    15, 20, 25, 32, 40, 50, 65, 80, 100, 125, 150, 200, 250, 300, 350, 400, 450, 500, 600,
];
/// 公称直径的单位；本期只有毫米。
pub const DIAMETER_UNIT: &str = "mm";
/// 公称直径标签最多几个字符。
const LABEL_MAX_CHARS: usize = 16;
/// 三张表里的名称最多几个字符。
const NAME_MAX_CHARS: usize = 64;

/// 一份项目主数据（§3.1）。
#[derive(Debug, Clone, PartialEq)]
pub struct MasterData {
    pub version: u64,
    /// 每次写入加 1。
    pub revision: u64,
    /// 最近一次写入的时间，Unix 秒。
    pub updated_at: i64,
    /// 项目锁定的图例标准；`None` 是没有锁定。
    pub legend_standard: Option<String>,
    /// 公称直径表，单位是 [`DIAMETER_UNIT`]。
    pub nominal_diameters: Vec<Diameter>,
    pub pipe_classes: Vec<Entry>,
    pub insulation_classes: Vec<Entry>,
    pub fluid_codes: Vec<Entry>,
    pub pbs: Vec<PbsNode>,
}

/// 公称直径表的一行。
#[derive(Debug, Clone, PartialEq)]
pub struct Diameter {
    pub dn: u32,
    /// 显示标签，不超过 16 个字符。
    pub label: String,
    /// 外径，可不填。
    pub outer_diameter: Option<f64>,
    /// 为真表示本项目不许用这个直径。
    pub hidden: bool,
}

/// 管道等级、绝热等级、流体介质表的一行：名称加扩展属性。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    pub name: String,
    /// 属性名 → 文本，按文件里的顺序。
    pub attrs: Vec<(String, String)>,
}

impl Entry {
    /// 扩展属性 `name` 的值。
    pub fn attr(&self, name: &str) -> Option<&str> {
        self.attrs
            .iter()
            .find(|(key, _)| key == name)
            .map(|(_, value)| value.as_str())
    }
}

/// PBS 节点树的一个节点。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PbsNode {
    pub id: String,
    pub label: String,
    /// 上级节点号；`None` 是根。
    pub parent: Option<String>,
}

/// 名称加扩展属性的那三张表。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Table {
    PipeClass,
    InsulationClass,
    FluidCode,
}

impl Table {
    /// 这张表在 JSON 里的键。
    pub fn key(self) -> &'static str {
        match self {
            Self::PipeClass => "pipe_classes",
            Self::InsulationClass => "insulation_classes",
            Self::FluidCode => "fluid_codes",
        }
    }
}

// ── 错误 ─────────────────────────────────────────────────────────────────

/// 主数据读不出来的原因。每个变体有 §3.6 的错误码（[`MasterDataError::code`]）
/// 和出错位置（[`MasterDataError::at`]，如 `pipe_classes[2].name`；整个对象是空串）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MasterDataError {
    /// JSON 结构不对：缺键、类型不对，或者取值超出约束。
    Schema { at: String, reason: String },
    /// 名称或 PBS 节点号为空。
    RequiredFieldMissing { at: String },
    /// 同一张表里名称、`dn` 或 PBS 节点号重复；`at` 是后出现的那一个。
    Duplicate { at: String, value: String },
    /// PBS 的 `parent` 指向不存在的节点，或者沿上级成环。
    PbsInvalid { at: String, reason: String },
    /// `version` 比程序认识的高。
    VersionUnsupported { found: u64 },
}

impl MasterDataError {
    /// §3.6 的错误码。
    pub fn code(&self) -> &'static str {
        match self {
            Self::Schema { .. } => "MASTERDATA_SCHEMA_ERROR",
            Self::RequiredFieldMissing { .. } => "REQUIRED_FIELD_MISSING",
            Self::Duplicate { .. } => "MASTERDATA_DUPLICATE",
            Self::PbsInvalid { .. } => "MASTERDATA_PBS_INVALID",
            Self::VersionUnsupported { .. } => "MASTERDATA_VERSION_UNSUPPORTED",
        }
    }

    /// 出错位置。
    pub fn at(&self) -> &str {
        match self {
            Self::Schema { at, .. }
            | Self::RequiredFieldMissing { at }
            | Self::Duplicate { at, .. }
            | Self::PbsInvalid { at, .. } => at,
            Self::VersionUnsupported { .. } => "version",
        }
    }
}

impl fmt::Display for MasterDataError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let at = if self.at().is_empty() {
            "顶层"
        } else {
            self.at()
        };
        match self {
            Self::Schema { reason, .. } | Self::PbsInvalid { reason, .. } => {
                write!(f, "{}: {at} {reason}", self.code())
            }
            Self::RequiredFieldMissing { .. } => write!(f, "{}: {at} 不能为空", self.code()),
            Self::Duplicate { value, .. } => {
                write!(f, "{}: {at} 的 {value} 与前面的重复", self.code())
            }
            Self::VersionUnsupported { found } => write!(
                f,
                "{}: 格式版本 {found} 比程序认识的 {VERSION} 高",
                self.code()
            ),
        }
    }
}

impl std::error::Error for MasterDataError {}

// ── 默认内容、比法 ────────────────────────────────────────────────────────

/// §3.1 的默认内容：19 个公称直径（标签 `DN<值>`，都不隐藏，外径不填），
/// 图例标准取 `legend_standard`，其余三张表和 PBS 为空。还没写过，`revision`
/// 与 `updated_at` 都是 0。
pub fn defaults(legend_standard: &str) -> MasterData {
    MasterData {
        version: VERSION,
        revision: 0,
        updated_at: 0,
        legend_standard: Some(legend_standard.to_owned()),
        nominal_diameters: DEFAULT_DIAMETERS
            .iter()
            .map(|&dn| Diameter {
                dn,
                label: format!("DN{dn}"),
                outer_diameter: None,
                hidden: false,
            })
            .collect(),
        pipe_classes: Vec::new(),
        insulation_classes: Vec::new(),
        fluid_codes: Vec::new(),
        pbs: Vec::new(),
    }
}

/// §3.5 的公称直径读法：去掉首尾空白、开头的 `DN`（不分大小写）和它后面的空白，
/// 余下的应当全是 ASCII 数字。`DN80`、`dn 80`、`80` 都读成 80；`八十`、`DN8 0`、
/// `+80`、超出 `u32` 的读不出来，是 `None`。
pub fn parse_dn(value: &str) -> Option<u32> {
    let value = value.trim();
    let digits = match value.get(..2) {
        Some(prefix) if prefix.eq_ignore_ascii_case("dn") => value[2..].trim_start(),
        _ => value,
    };
    if digits.is_empty() || !digits.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    digits.parse().ok()
}

impl MasterData {
    /// §3.1 的 JSON 形状，键按规格里的顺序。
    pub fn to_json(&self) -> Value {
        let items: Vec<Value> = self
            .nominal_diameters
            .iter()
            .map(|diameter| {
                json!({
                    "dn": diameter.dn,
                    "label": diameter.label,
                    "outer_diameter": diameter.outer_diameter,
                    "hidden": diameter.hidden,
                })
            })
            .collect();
        let pbs: Vec<Value> = self
            .pbs
            .iter()
            .map(|node| json!({ "id": node.id, "label": node.label, "parent": node.parent }))
            .collect();
        json!({
            "version": self.version,
            "revision": self.revision,
            "updated_at": self.updated_at,
            "legend_standard": self.legend_standard,
            "nominal_diameters": { "unit": DIAMETER_UNIT, "items": items },
            "pipe_classes": entries_json(&self.pipe_classes),
            "insulation_classes": entries_json(&self.insulation_classes),
            "fluid_codes": entries_json(&self.fluid_codes),
            "pbs": pbs,
        })
    }

    /// §3.5：这个公称直径在不在表里。读不出来、表里没有、表里标了隐藏，都算不在。
    pub fn has_diameter(&self, value: &str) -> bool {
        parse_dn(value).is_some_and(|dn| {
            self.nominal_diameters
                .iter()
                .any(|diameter| diameter.dn == dn && !diameter.hidden)
        })
    }

    /// 一张表的全部行。
    pub fn table(&self, table: Table) -> &[Entry] {
        match table {
            Table::PipeClass => &self.pipe_classes,
            Table::InsulationClass => &self.insulation_classes,
            Table::FluidCode => &self.fluid_codes,
        }
    }

    /// §3.5：名称与 `name`（去掉首尾空白后）逐字相等的那一行，区分大小写。
    pub fn entry(&self, table: Table, name: &str) -> Option<&Entry> {
        let name = name.trim();
        self.table(table).iter().find(|entry| entry.name == name)
    }

    /// 一张表的名称，按表里的顺序。
    pub fn names(&self, table: Table) -> Vec<&str> {
        self.table(table)
            .iter()
            .map(|entry| entry.name.as_str())
            .collect()
    }

    /// PBS 节点 `id` 自身加它的所有下级：先节点后下级，同级按表里的顺序。
    /// 没有这个节点时为空。
    pub fn pbs_subtree(&self, id: &str) -> Vec<&str> {
        let Some(root) = self.pbs.iter().find(|node| node.id == id) else {
            return Vec::new();
        };
        let mut children: HashMap<&str, Vec<&str>> = HashMap::new();
        for node in &self.pbs {
            if let Some(parent) = &node.parent {
                children
                    .entry(parent.as_str())
                    .or_default()
                    .push(node.id.as_str());
            }
        }
        // 字段是公开的，没校验过的数据可能成环：走过的节点不再走。
        let mut seen = HashSet::new();
        let mut subtree = Vec::new();
        let mut stack = vec![root.id.as_str()];
        while let Some(node) = stack.pop() {
            if !seen.insert(node) {
                continue;
            }
            subtree.push(node);
            if let Some(below) = children.get(node) {
                stack.extend(below.iter().rev());
            }
        }
        subtree
    }
}

/// 名称加扩展属性那三张表之一的 JSON 形状。
fn entries_json(rows: &[Entry]) -> Value {
    rows.iter()
        .map(|row| {
            let attrs: Map<String, Value> = row
                .attrs
                .iter()
                .map(|(name, value)| (name.clone(), Value::String(value.clone())))
                .collect();
            json!({ "name": row.name, "attrs": attrs })
        })
        .collect()
}

// ── 校验 ─────────────────────────────────────────────────────────────────

/// 校验一个 JSON 值，得出主数据；不对时返回第一处错，带 §3.6 的错误码与出错位置。
///
/// 先看 `version`：比程序认识的高就不再往下读。§3.1 的键都要有，只有
/// `legend_standard`、`outer_diameter`、`hidden`、`attrs`、`parent` 可以不写（当作 `null`、
/// `null`、`false`、空对象、`null`）；认不得的键不理。
pub fn validate(value: &Value) -> Result<MasterData, MasterDataError> {
    let root = JsonObject::read(value, String::new())?;
    let version = root.unsigned("version")?;
    if version > VERSION {
        return Err(MasterDataError::VersionUnsupported { found: version });
    }
    if version == 0 {
        return Err(schema(root.at("version"), "格式版本从 1 起"));
    }
    Ok(MasterData {
        version,
        revision: root.unsigned("revision")?,
        updated_at: root.integer("updated_at")?,
        legend_standard: root.optional_text("legend_standard")?.map(str::to_owned),
        nominal_diameters: diameters(&root)?,
        pipe_classes: entries(&root, Table::PipeClass)?,
        insulation_classes: entries(&root, Table::InsulationClass)?,
        fluid_codes: entries(&root, Table::FluidCode)?,
        pbs: pbs_nodes(&root)?,
    })
}

fn diameters(root: &JsonObject<'_>) -> Result<Vec<Diameter>, MasterDataError> {
    let table = root.object("nominal_diameters")?;
    if table.text("unit")? != DIAMETER_UNIT {
        return Err(schema(
            table.at("unit"),
            format!("单位应为 {DIAMETER_UNIT}"),
        ));
    }
    let mut seen = HashSet::new();
    let mut diameters = Vec::new();
    for (at, item) in table.elements("items")? {
        let item = JsonObject::read(item, at)?;
        let dn = u32::try_from(item.unsigned("dn")?)
            .ok()
            .filter(|&dn| dn > 0)
            .ok_or_else(|| schema(item.at("dn"), "应为正整数"))?;
        if !seen.insert(dn) {
            return Err(MasterDataError::Duplicate {
                at: item.at("dn"),
                value: dn.to_string(),
            });
        }
        let label = item.text("label")?;
        if label.chars().count() > LABEL_MAX_CHARS {
            return Err(schema(
                item.at("label"),
                format!("超过 {LABEL_MAX_CHARS} 个字符"),
            ));
        }
        diameters.push(Diameter {
            dn,
            label: label.to_owned(),
            outer_diameter: item.optional_number("outer_diameter")?,
            hidden: item.flag("hidden")?,
        });
    }
    Ok(diameters)
}

fn entries(root: &JsonObject<'_>, table: Table) -> Result<Vec<Entry>, MasterDataError> {
    let mut seen = HashSet::new();
    let mut rows = Vec::new();
    for (at, row) in root.elements(table.key())? {
        let row = JsonObject::read(row, at)?;
        let name = row.required_text("name")?;
        if name.chars().count() > NAME_MAX_CHARS {
            return Err(schema(
                row.at("name"),
                format!("超过 {NAME_MAX_CHARS} 个字符"),
            ));
        }
        if !seen.insert(name) {
            return Err(MasterDataError::Duplicate {
                at: row.at("name"),
                value: name.to_owned(),
            });
        }
        let attrs = match row.map.get("attrs") {
            None => Vec::new(),
            Some(attrs) => {
                let attrs = JsonObject::read(attrs, row.at("attrs"))?;
                attrs
                    .map
                    .iter()
                    .map(|(key, value)| match value {
                        Value::String(text) => Ok((key.clone(), text.clone())),
                        _ => Err(schema(attrs.at(key), "应为文本")),
                    })
                    .collect::<Result<Vec<_>, _>>()?
            }
        };
        rows.push(Entry {
            name: name.to_owned(),
            attrs,
        });
    }
    Ok(rows)
}

fn pbs_nodes(root: &JsonObject<'_>) -> Result<Vec<PbsNode>, MasterDataError> {
    let mut index_of = HashMap::new();
    let mut nodes = Vec::new();
    for (at, node) in root.elements("pbs")? {
        let node = JsonObject::read(node, at)?;
        let id = node.required_text("id")?;
        if index_of.insert(id, nodes.len()).is_some() {
            return Err(MasterDataError::Duplicate {
                at: node.at("id"),
                value: id.to_owned(),
            });
        }
        nodes.push(PbsNode {
            id: id.to_owned(),
            label: node.text("label")?.to_owned(),
            parent: node.optional_text("parent")?.map(str::to_owned),
        });
    }
    let parent_at = |index: usize| format!("{}[{index}].parent", root.at("pbs"));
    let mut up = Vec::with_capacity(nodes.len());
    for (index, node) in nodes.iter().enumerate() {
        let Some(parent) = &node.parent else {
            up.push(None);
            continue;
        };
        let Some(&above) = index_of.get(parent.as_str()) else {
            return Err(MasterDataError::PbsInvalid {
                at: parent_at(index),
                reason: format!("上级节点 {parent} 不存在"),
            });
        };
        up.push(Some(above));
    }
    if let Some(cycle) = first_cycle(&up) {
        let mut path: Vec<&str> = cycle
            .iter()
            .map(|&index| nodes[index].id.as_str())
            .collect();
        path.push(path[0]);
        return Err(MasterDataError::PbsInvalid {
            at: parent_at(cycle[0]),
            reason: format!("沿上级成环：{}", path.join(" → ")),
        });
    }
    Ok(nodes)
}

/// 沿上级（`up[i]` 是节点 `i` 的上级）走出来的第一个环，从环上下标最小的节点起。
fn first_cycle(up: &[Option<usize>]) -> Option<Vec<usize>> {
    #[derive(Clone, Copy)]
    enum Mark {
        New,
        OnPath,
        Done,
    }
    let mut marks = vec![Mark::New; up.len()];
    for start in 0..up.len() {
        let mut path = Vec::new();
        let mut next = Some(start);
        while let Some(index) = next {
            match marks[index] {
                Mark::New => {
                    marks[index] = Mark::OnPath;
                    path.push(index);
                    next = up[index];
                }
                Mark::OnPath => {
                    let from = path.iter().position(|&on_path| on_path == index)?;
                    let mut cycle = path.split_off(from);
                    let lowest = (0..cycle.len()).min_by_key(|&at| cycle[at])?;
                    cycle.rotate_left(lowest);
                    return Some(cycle);
                }
                Mark::Done => break,
            }
        }
        for index in path {
            marks[index] = Mark::Done;
        }
    }
    None
}

/// 校验时的一个 JSON 对象和它的位置。
struct JsonObject<'a> {
    map: &'a Map<String, Value>,
    at: String,
}

impl<'a> JsonObject<'a> {
    fn read(value: &'a Value, at: String) -> Result<Self, MasterDataError> {
        match value {
            Value::Object(map) => Ok(Self { map, at }),
            _ => Err(schema(at, "应为对象")),
        }
    }

    /// 键 `key` 的位置。
    fn at(&self, key: &str) -> String {
        if self.at.is_empty() {
            key.to_owned()
        } else {
            format!("{}.{key}", self.at)
        }
    }

    fn get(&self, key: &str) -> Result<&'a Value, MasterDataError> {
        self.map
            .get(key)
            .ok_or_else(|| schema(self.at(key), "缺少这个键"))
    }

    fn object(&self, key: &str) -> Result<JsonObject<'a>, MasterDataError> {
        JsonObject::read(self.get(key)?, self.at(key))
    }

    /// 键 `key` 下的数组，每个元素配上自己的位置 `key[i]`。
    fn elements(
        &self,
        key: &str,
    ) -> Result<impl Iterator<Item = (String, &'a Value)>, MasterDataError> {
        let at = self.at(key);
        match self.get(key)? {
            Value::Array(items) => Ok(items
                .iter()
                .enumerate()
                .map(move |(index, item)| (format!("{at}[{index}]"), item))),
            _ => Err(schema(at, "应为数组")),
        }
    }

    fn text(&self, key: &str) -> Result<&'a str, MasterDataError> {
        self.get(key)?
            .as_str()
            .ok_or_else(|| schema(self.at(key), "应为文本"))
    }

    /// 不能为空的文本：去掉首尾空白后为空是 `REQUIRED_FIELD_MISSING`。
    fn required_text(&self, key: &str) -> Result<&'a str, MasterDataError> {
        let text = self.text(key)?;
        if text.trim().is_empty() {
            return Err(MasterDataError::RequiredFieldMissing { at: self.at(key) });
        }
        Ok(text)
    }

    /// 可以不写或写 `null` 的文本。
    fn optional_text(&self, key: &str) -> Result<Option<&'a str>, MasterDataError> {
        match self.map.get(key) {
            None | Some(Value::Null) => Ok(None),
            Some(Value::String(text)) => Ok(Some(text.as_str())),
            Some(_) => Err(schema(self.at(key), "应为文本或 null")),
        }
    }

    /// 可以不写或写 `null` 的数。
    fn optional_number(&self, key: &str) -> Result<Option<f64>, MasterDataError> {
        match self.map.get(key) {
            None | Some(Value::Null) => Ok(None),
            Some(value) => value
                .as_f64()
                .map(Some)
                .ok_or_else(|| schema(self.at(key), "应为数或 null")),
        }
    }

    /// 可以不写的真假值，不写是假。
    fn flag(&self, key: &str) -> Result<bool, MasterDataError> {
        match self.map.get(key) {
            None => Ok(false),
            Some(value) => value
                .as_bool()
                .ok_or_else(|| schema(self.at(key), "应为 true 或 false")),
        }
    }

    fn unsigned(&self, key: &str) -> Result<u64, MasterDataError> {
        self.get(key)?
            .as_u64()
            .ok_or_else(|| schema(self.at(key), "应为非负整数"))
    }

    fn integer(&self, key: &str) -> Result<i64, MasterDataError> {
        self.get(key)?
            .as_i64()
            .ok_or_else(|| schema(self.at(key), "应为整数"))
    }
}

fn schema(at: String, reason: impl Into<String>) -> MasterDataError {
    MasterDataError::Schema {
        at,
        reason: reason.into(),
    }
}

#[cfg(test)]
mod tests {
    use std::time::{Duration, Instant};

    use serde_json::json;

    use super::*;

    /// §3.1 的例子，多一个直径和几个 PBS 节点：两个根，一棵三层的树。
    fn sample() -> Value {
        json!({
            "version": 1,
            "revision": 3,
            "updated_at": 1794800000,
            "legend_standard": "内置示例标准",
            "nominal_diameters": {
                "unit": "mm",
                "items": [
                    { "dn": 15, "label": "DN15", "outer_diameter": null, "hidden": false },
                    { "dn": 50, "label": "DN50", "outer_diameter": 60.3, "hidden": false },
                    { "dn": 65, "label": "DN65", "outer_diameter": null, "hidden": true },
                    { "dn": 80, "label": "DN80", "outer_diameter": 88.9, "hidden": false }
                ]
            },
            "pipe_classes": [
                {
                    "name": "A1A",
                    "attrs": {
                        "压力等级": "PN16",
                        "最大公称压力": "16",
                        "材质": "碳钢",
                        "公称直径下限": "15",
                        "公称直径上限": "600"
                    }
                },
                { "name": "B1B", "attrs": {} }
            ],
            "insulation_classes": [ { "name": "H", "attrs": { "说明": "保温" } } ],
            "fluid_codes": [ { "name": "工艺水", "attrs": { "代号": "PW" } } ],
            "pbs": [
                { "id": "1001", "label": "常压蒸馏装置", "parent": null },
                { "id": "1001-01", "label": "原料罐区", "parent": "1001" },
                { "id": "1001-02", "label": "常压塔区", "parent": "1001" },
                { "id": "1001-01-A", "label": "原油罐", "parent": "1001-01" },
                { "id": "2001", "label": "公用工程", "parent": null }
            ]
        })
    }

    /// `sample()` 改一处：`pointer` 是 JSON Pointer。
    fn with(pointer: &str, value: Value) -> Value {
        let mut changed = sample();
        *changed
            .pointer_mut(pointer)
            .unwrap_or_else(|| panic!("{pointer} is in the sample")) = value;
        changed
    }

    /// `sample()` 去掉一个键：`object` 是它所在对象的 JSON Pointer。
    fn without(object: &str, key: &str) -> Value {
        let mut changed = sample();
        changed
            .pointer_mut(object)
            .and_then(Value::as_object_mut)
            .and_then(|map| map.remove(key))
            .unwrap_or_else(|| panic!("{object}/{key} is in the sample"));
        changed
    }

    fn refused(value: &Value) -> (&'static str, String) {
        let error = validate(value).expect_err("the value is refused");
        (error.code(), error.at().to_owned())
    }

    fn read(value: &Value) -> MasterData {
        validate(value).unwrap_or_else(|error| panic!("the value is read: {error}"))
    }

    #[test]
    fn master_data_defaults_hold_the_19_diameters_and_nothing_else() {
        let data = defaults("内置示例标准");
        let dns: Vec<u32> = data.nominal_diameters.iter().map(|d| d.dn).collect();
        assert_eq!(
            dns,
            [
                15, 20, 25, 32, 40, 50, 65, 80, 100, 125, 150, 200, 250, 300, 350, 400, 450, 500,
                600
            ]
        );
        for diameter in &data.nominal_diameters {
            assert_eq!(diameter.label, format!("DN{}", diameter.dn));
            assert_eq!((diameter.outer_diameter, diameter.hidden), (None, false));
        }
        assert_eq!(data.legend_standard.as_deref(), Some("内置示例标准"));
        assert_eq!((data.version, data.revision, data.updated_at), (1, 0, 0));
        assert!(data.pipe_classes.is_empty());
        assert!(data.insulation_classes.is_empty());
        assert!(data.fluid_codes.is_empty());
        assert!(data.pbs.is_empty());
        assert_eq!(read(&data.to_json()), data, "the defaults are valid");
    }

    #[test]
    fn master_data_sample_is_read_and_written_back_as_it_was() {
        let data = read(&sample());
        assert_eq!(
            (data.version, data.revision, data.updated_at),
            (1, 3, 1794800000)
        );
        assert_eq!(data.nominal_diameters[1].outer_diameter, Some(60.3));
        assert!(data.nominal_diameters[2].hidden);
        assert_eq!(data.pipe_classes[0].attr("最大公称压力"), Some("16"));
        assert_eq!(data.pipe_classes[0].attr("材质"), Some("碳钢"));
        assert_eq!(data.pipe_classes[0].attr("不存在"), None);
        assert_eq!(data.pbs[3].parent.as_deref(), Some("1001-01"));

        assert_eq!(data.to_json(), sample(), "written back key for key");
        assert_eq!(read(&data.to_json()), data);
        let text = serde_json::to_string_pretty(&data.to_json()).expect("serializes");
        let parsed: Value = serde_json::from_str(&text).expect("parses");
        assert_eq!(read(&parsed), data, "through text and back");
        let attrs: Vec<&str> = data.pipe_classes[0]
            .attrs
            .iter()
            .map(|(name, _)| name.as_str())
            .collect();
        assert_eq!(
            attrs,
            [
                "压力等级",
                "最大公称压力",
                "材质",
                "公称直径下限",
                "公称直径上限"
            ]
        );

        let mut extra = sample();
        extra["备注"] = json!("认不得的键不理");
        assert_eq!(read(&extra), data);
        let mut terse = without("/nominal_diameters/items/0", "outer_diameter");
        for (object, key) in [
            ("", "legend_standard"),
            ("/nominal_diameters/items/0", "hidden"),
            ("/pipe_classes/1", "attrs"),
            ("/pbs/0", "parent"),
        ] {
            terse
                .pointer_mut(object)
                .and_then(Value::as_object_mut)
                .and_then(|map| map.remove(key))
                .unwrap_or_else(|| panic!("{object}/{key} is in the sample"));
        }
        let terse = read(&terse);
        assert_eq!(terse.legend_standard, None);
        assert_eq!(terse.nominal_diameters[0], data.nominal_diameters[0]);
        assert_eq!(terse.pipe_classes[1], data.pipe_classes[1]);
        assert_eq!(terse.pbs[0], data.pbs[0]);
    }

    #[test]
    fn master_data_schema_errors_say_where() {
        let cases = [
            (json!([]), ""),
            (with("/version", json!("1")), "version"),
            (with("/version", json!(0)), "version"),
            (with("/updated_at", json!(1.5)), "updated_at"),
            (with("/legend_standard", json!(7)), "legend_standard"),
            (without("", "pbs"), "pbs"),
            (
                with("/nominal_diameters/unit", json!("in")),
                "nominal_diameters.unit",
            ),
            (
                with("/nominal_diameters/items", json!({})),
                "nominal_diameters.items",
            ),
            (
                with("/nominal_diameters/items/2/dn", json!("65")),
                "nominal_diameters.items[2].dn",
            ),
            (
                with("/nominal_diameters/items/2/dn", json!(0)),
                "nominal_diameters.items[2].dn",
            ),
            (
                with(
                    "/nominal_diameters/items/0/label",
                    json!("DN15（这个标签超过十六个字符了）"),
                ),
                "nominal_diameters.items[0].label",
            ),
            (
                with("/nominal_diameters/items/1/outer_diameter", json!("60.3")),
                "nominal_diameters.items[1].outer_diameter",
            ),
            (
                with("/nominal_diameters/items/1/hidden", json!(0)),
                "nominal_diameters.items[1].hidden",
            ),
            (
                with("/pipe_classes/1/name", json!(7)),
                "pipe_classes[1].name",
            ),
            (without("/pipe_classes/1", "name"), "pipe_classes[1].name"),
            (
                with("/fluid_codes/0/name", json!("类".repeat(65))),
                "fluid_codes[0].name",
            ),
            (
                with("/pipe_classes/0/attrs/最大公称压力", json!(16)),
                "pipe_classes[0].attrs.最大公称压力",
            ),
            (
                with("/insulation_classes/0/attrs", json!([])),
                "insulation_classes[0].attrs",
            ),
            (with("/pbs/1", json!("1001-01")), "pbs[1]"),
            (with("/pbs/1/parent", json!(1001)), "pbs[1].parent"),
            (without("/pbs/2", "label"), "pbs[2].label"),
        ];
        for (value, at) in cases {
            assert_eq!(
                refused(&value),
                ("MASTERDATA_SCHEMA_ERROR", at.to_owned()),
                "{value}"
            );
        }
        let message = validate(&with("/pipe_classes/1/name", json!(7)))
            .expect_err("refused")
            .to_string();
        assert!(
            message.contains("MASTERDATA_SCHEMA_ERROR") && message.contains("pipe_classes[1].name"),
            "{message}"
        );
        assert_eq!(
            read(&with("/fluid_codes/0/name", json!("类".repeat(64)))).fluid_codes[0]
                .name
                .chars()
                .count(),
            64
        );
        assert_eq!(
            read(&with(
                "/nominal_diameters/items/0/label",
                json!("十六个字符的标签是可以的对吧对吧")
            ))
            .nominal_diameters[0]
                .label
                .chars()
                .count(),
            16
        );
    }

    #[test]
    fn master_data_an_empty_name_is_a_required_field_missing() {
        for (value, at) in [
            (
                with("/insulation_classes/0/name", json!("")),
                "insulation_classes[0].name",
            ),
            (
                with("/pipe_classes/1/name", json!("   ")),
                "pipe_classes[1].name",
            ),
            (with("/pbs/2/id", json!("")), "pbs[2].id"),
        ] {
            assert_eq!(
                refused(&value),
                ("REQUIRED_FIELD_MISSING", at.to_owned()),
                "{value}"
            );
        }
    }

    #[test]
    fn master_data_duplicates_are_refused_where_the_second_one_stands() {
        for (value, at) in [
            (
                with("/pipe_classes/1/name", json!("A1A")),
                "pipe_classes[1].name",
            ),
            (
                with("/nominal_diameters/items/3/dn", json!(50)),
                "nominal_diameters.items[3].dn",
            ),
            (with("/pbs/4/id", json!("1001-02")), "pbs[4].id"),
        ] {
            assert_eq!(
                refused(&value),
                ("MASTERDATA_DUPLICATE", at.to_owned()),
                "{value}"
            );
        }
        // 只在一张表里算重复，而且区分大小写。
        assert_eq!(
            read(&with("/fluid_codes/0/name", json!("A1A"))).fluid_codes[0].name,
            "A1A"
        );
        assert_eq!(
            read(&with("/pipe_classes/1/name", json!("a1a"))).pipe_classes[1].name,
            "a1a"
        );
    }

    #[test]
    fn master_data_pbs_parents_must_exist_and_lead_to_a_root() {
        for (value, at) in [
            (with("/pbs/1/parent", json!("9999")), "pbs[1].parent"),
            // 1001 → 1001-01-A → 1001-01 → 1001：报在环上下标最小的节点。
            (with("/pbs/0/parent", json!("1001-01-A")), "pbs[0].parent"),
            (with("/pbs/4/parent", json!("2001")), "pbs[4].parent"),
        ] {
            assert_eq!(
                refused(&value),
                ("MASTERDATA_PBS_INVALID", at.to_owned()),
                "{value}"
            );
        }
        let message = validate(&with("/pbs/0/parent", json!("1001-01-A")))
            .expect_err("refused")
            .to_string();
        assert!(
            message.contains("1001 → 1001-01-A → 1001-01 → 1001"),
            "{message}"
        );
        assert_eq!(
            read(&sample())
                .pbs
                .iter()
                .filter(|node| node.parent.is_none())
                .count(),
            2
        );
    }

    #[test]
    fn master_data_newer_versions_are_not_read() {
        assert_eq!(
            refused(&with("/version", json!(2))),
            ("MASTERDATA_VERSION_UNSUPPORTED", "version".to_owned())
        );
        let mut newer = with("/version", json!(2));
        newer["pipe_classes"] = json!("新版本可以换写法");
        assert_eq!(
            refused(&newer),
            ("MASTERDATA_VERSION_UNSUPPORTED", "version".to_owned())
        );
    }

    #[test]
    fn master_data_diameters_compare_by_number_after_dn() {
        let data = read(&sample());
        for value in ["DN80", "dn 80", "80", " Dn80 ", "DN  80", "080", "DN50"] {
            assert!(data.has_diameter(value), "{value:?} is in the table");
        }
        for value in [
            "八十", "DN65", "65", "DN100", "DN", "", "80mm", "DN-80", "+80", "DN8 0", "８０",
        ] {
            assert!(!data.has_diameter(value), "{value:?} is not in the table");
        }
        assert_eq!(parse_dn("dn 80"), Some(80));
        assert_eq!(parse_dn("DN65"), Some(65));
        assert_eq!(parse_dn("八十"), None);
        assert_eq!(parse_dn("99999999999"), None);
    }

    #[test]
    fn master_data_names_compare_exactly_and_case_sensitively() {
        let data = read(&sample());
        let class = data
            .entry(Table::PipeClass, "A1A")
            .expect("A1A is a pipe class");
        assert_eq!(class.attr("材质"), Some("碳钢"));
        assert!(
            data.entry(Table::PipeClass, " A1A ").is_some(),
            "the value is trimmed"
        );
        assert!(data.entry(Table::PipeClass, "a1a").is_none());
        assert!(data.entry(Table::PipeClass, "A1").is_none());
        assert!(data.entry(Table::InsulationClass, "A1A").is_none());
        assert!(data.entry(Table::FluidCode, "工艺水").is_some());
        assert_eq!(data.names(Table::PipeClass), ["A1A", "B1B"]);
        assert_eq!(data.names(Table::InsulationClass), ["H"]);
        assert_eq!(data.names(Table::FluidCode), ["工艺水"]);
        assert_eq!(data.table(Table::PipeClass).len(), 2);
    }

    #[test]
    fn master_data_pbs_subtree_is_the_node_and_everything_below() {
        let data = read(&sample());
        assert_eq!(
            data.pbs_subtree("1001"),
            ["1001", "1001-01", "1001-01-A", "1001-02"]
        );
        assert_eq!(data.pbs_subtree("1001-01"), ["1001-01", "1001-01-A"]);
        assert_eq!(data.pbs_subtree("2001"), ["2001"]);
        assert!(data.pbs_subtree("9999").is_empty());
    }

    /// 契约规模：2,000 行管道等级，每行 5 个扩展属性。
    const PIPE_CLASSES: usize = 2000;
    /// 预算：解析加校验不超过 50 ms（成功标准第 8 条）。
    const BUDGET: Duration = Duration::from_millis(50);
    /// 采样次数：取中位数。
    const RUNS: usize = 3;

    #[test]
    #[ignore]
    fn master_data_2000_pipe_classes_parse_and_validate_within_50_ms() {
        let mut data = defaults("内置示例标准");
        data.pipe_classes = (0..PIPE_CLASSES)
            .map(|index| Entry {
                name: format!("C{index:04}"),
                attrs: [
                    ("压力等级", "PN16"),
                    ("最大公称压力", "16"),
                    ("材质", "碳钢"),
                    ("公称直径下限", "15"),
                    ("公称直径上限", "600"),
                ]
                .iter()
                .map(|(name, value)| ((*name).to_owned(), (*value).to_owned()))
                .collect(),
            })
            .collect();
        let text = serde_json::to_string_pretty(&data.to_json()).expect("serializes");

        let mut samples = Vec::with_capacity(RUNS);
        for run in 1..=RUNS {
            let started = Instant::now();
            let value: Value = serde_json::from_str(&text).expect("parses");
            let parsed = validate(&value);
            let elapsed = started.elapsed();
            let rows = parsed.map(|read| read.pipe_classes.len());
            println!(
                "MD-1 run {run}/{RUNS}: {:.2} ms, {} bytes, rows {rows:?}",
                elapsed.as_secs_f64() * 1e3,
                text.len()
            );
            assert_eq!(rows, Ok(PIPE_CLASSES), "run {run}");
            samples.push(elapsed);
        }
        samples.sort();
        let median = samples[RUNS / 2];
        let shown: Vec<String> = samples
            .iter()
            .map(|sample| format!("{:.2}", sample.as_secs_f64() * 1e3))
            .collect();
        println!(
            "MD-1 median {:.2} ms of {shown:?}, budget {} ms",
            median.as_secs_f64() * 1e3,
            BUDGET.as_millis()
        );
        assert!(
            median <= BUDGET,
            "median {median:?} of {shown:?} is over {BUDGET:?}"
        );
    }
}
