//! 每实体 P&ID 语义：`PID_AUTHOR` XDATA 的读写层。
//!
//! 九键（`type` / `tag` / `tag_source` / `tag_status` / `props` / `ports` /
//! `owner` / `topology` / `connector_peer`）编码成 `key=value` 形式的
//! [`XDataValue::String`]，与 `io::pid_legend::xdata`、`scene::cache::properties`
//! 的既有约定一致：`ExtendedDataRecord.values` 没有键槽，首个 `=` 之前的
//! 文本是键，之后原样是值。
//!
//! 三条硬约束在这里落地：
//!
//! 1. **坐标不用 `Position3D` / `Displacement3D` / `Direction3D` /
//!    `Distance` / `ScaleFactor`。** `ExtendedData::apply_transform` 会随实体
//!    移动自动换算它们，且一旦换算成功调用方会丢弃整条 verbatim
//!    `raw_record`。端口坐标一律走 `String` / `Integer32`，由本模块自己解释。
//! 2. **先判定再落盘。** DWG 的 EED 用 i16 长度前缀，超 `i16::MAX` 的整条
//!    记录在写出时被静默丢弃（acadrust `object_writer/common.rs:1034`）。
//!    所以先算 [`encoded_len`]，超 [`XDATA_SOFT_LIMIT`] 就把 `props` 挪到
//!    模型空间扩展字典里的 XRecord，记录只留 `props_ref` 引用键。
//! 3. **`PID_AUTHOR` 是独立 APPID。** `io::pid_legend::xdata::purge` 只删
//!    `resolved` 以 `legend:` 开头的 `PID_SEMANTICS` 记录，两个命名空间不能混。
//!
//! 写入一律走 `scene::view::dispatch::set_entity_xdata`：它负责注册 APPID、
//! 把同一 APPID 的记录整条替换（而不是追加）、并作废该 APPID 的 verbatim
//! EED 块。

use codec::objects::{XRecordEntry, XRecordValue};
use codec::xdata::XDataValue;
use codec::{Document, Handle};

use crate::scene::view::dispatch::set_entity_xdata;

/// 本模块独占的 XDATA 应用名。
///
/// 必须与 `PID_SEMANTICS` 分开：识别器的 `purge` 会删掉 `PID_SEMANTICS` 下
/// `resolved` 以 `legend:` 开头的记录，混用会把作者数据一并清掉。
pub const PID_AUTHOR_APP: &str = "PID_AUTHOR";

/// 单实体记录的软上限（字节）。DWG 硬上限是 i16::MAX，这里留足余量：
/// 超过就把 `props` 挪进 XRecord，记录只留引用键。
pub const XDATA_SOFT_LIMIT: usize = 2048;

/// 落盘键名：`type` / `tag` / `tag_source` / `tag_status` / `props` /
/// `ports` / `owner` / `topology` / `connector_peer`，逐字如此。
pub const XDATA_KEYS: [&str; 9] = [
    "type",
    "tag",
    "tag_source",
    "tag_status",
    "props",
    "ports",
    "owner",
    "topology",
    "connector_peer",
];

/// `props` 溢出后记录里承载引用键的键名。
pub const PROPS_REF_KEY: &str = "props_ref";

/// 溢出 XRecord 的语义标记，和四个图级 key 区分开。
const KIND_ENTITY_PROPS: &str = "kind=entity_props";

/// DWG 侧 `Chunk` 单条上限是 `u8::MAX` 字节，超出静默截断，所以按此分片。
const CHUNK_BYTES: usize = 255;

/// 承载 `props` JSON 分片的组码（DXF 310 = 二进制块）。
const CHUNK_CODE: i32 = 310;

/// 一个实体的作者侧语义。字段名即落盘键名，禁止靠顺序约定拼键。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PidAuthor {
    /// 落盘键 `type`：类型编码，大写，如 `EQUIPMENT`。
    pub type_code: String,
    /// 落盘键 `tag`：位号。
    pub tag: String,
    /// 落盘键 `tag_source`：`auto` / `manual`。
    pub tag_source: String,
    /// 落盘键 `tag_status`：`ok` / `duplicated`。
    pub tag_status: String,
    /// 落盘键 `props`：属性键值对的 JSON 串。
    pub props: String,
    /// 落盘键 `ports`：端口角色，逗号分隔。
    pub ports: String,
    /// 落盘键 `owner`：宿主实体 handle。
    pub owner: String,
    /// 落盘键 `topology`：拓扑角色。
    pub topology: String,
    /// 落盘键 `connector_peer`：页间连接符对端；空串时该键不落盘。
    pub connector_peer: String,
}

impl PidAuthor {
    /// 一个只有类型与位号的作者记录，其余取默认。
    pub fn new(type_code: impl Into<String>, tag: impl Into<String>) -> Self {
        Self {
            type_code: type_code.into(),
            tag: tag.into(),
            ..Self::default()
        }
    }

    /// 把记录写进 `handle`。先判定长度，超限才把 `props` 挪进 XRecord。
    pub fn store(&self, doc: &mut Document, handle: Handle) {
        let mut values = encode(self);
        if encoded_len(&values) > XDATA_SOFT_LIMIT {
            let key = props_ref_key(handle);
            store_props_record(doc, &key, handle, &self.props);
            values.retain(|value| {
                as_str(value).is_none_or(|text| !text.starts_with("props="))
            });
            values.push(XDataValue::String(format!("{PROPS_REF_KEY}={key}")));
        }
        set_entity_xdata(doc, handle, PID_AUTHOR_APP, Some(values));
    }

    /// 从 `handle` 读回。记录不存在返回 `None`；`props_ref` 指向的 XRecord
    /// 存在时用它填回完整 `props`。
    pub fn load(doc: &Document, handle: Handle) -> Option<Self> {
        let record = doc
            .get_entity(handle)?
            .common()
            .extended_data
            .get_record(PID_AUTHOR_APP)?;
        let mut author = decode(&record.values)?;
        if let Some(key) = props_ref_of(&record.values) {
            if let Some(props) = load_props_record(doc, &key) {
                author.props = props;
            }
        }
        Some(author)
    }
}

/// 九键 → XDATA 值序列。纯函数：不碰文档，不判溢出。
///
/// 空串的 `ports` / `owner` / `topology` / `connector_peer` 不落盘（键缺失即
/// 「没有值」）；`type` / `tag` / `tag_source` / `tag_status` / `props` 恒写。
pub fn encode(author: &PidAuthor) -> Vec<XDataValue> {
    let mut values = Vec::with_capacity(XDATA_KEYS.len());
    for (key, value) in [
        ("type", &author.type_code),
        ("tag", &author.tag),
        ("tag_source", &author.tag_source),
        ("tag_status", &author.tag_status),
        ("props", &author.props),
    ] {
        values.push(XDataValue::String(format!("{key}={value}")));
    }
    for (key, value) in [
        ("ports", &author.ports),
        ("owner", &author.owner),
        ("topology", &author.topology),
        ("connector_peer", &author.connector_peer),
    ] {
        if !value.is_empty() {
            values.push(XDataValue::String(format!("{key}={value}")));
        }
    }
    values
}

/// XDATA 值序列 → 九键。没有任何可识别键时返回 `None`。
///
/// 首个 `=` 分隔键与值（`props` 的 JSON 里含 `:` 与 `"`，但不影响首个 `=`）。
pub fn decode(values: &[XDataValue]) -> Option<PidAuthor> {
    let mut author = PidAuthor::default();
    let mut found = false;
    for value in values {
        let Some(text) = as_str(value) else { continue };
        let Some((key, val)) = text.split_once('=') else {
            continue;
        };
        let slot = match key {
            "type" => &mut author.type_code,
            "tag" => &mut author.tag,
            "tag_source" => &mut author.tag_source,
            "tag_status" => &mut author.tag_status,
            "props" => &mut author.props,
            "ports" => &mut author.ports,
            "owner" => &mut author.owner,
            "topology" => &mut author.topology,
            "connector_peer" => &mut author.connector_peer,
            _ => continue,
        };
        *slot = val.to_string();
        found = true;
    }
    found.then_some(author)
}

/// 一条记录的估算字节数。这是软上限的判定口径，**不是** DWG EED 的真实
/// 字节数：每条值额外计 1 字节作为组码/长度前缀的保守估算。
///
/// 后续 feature 必须复用这一口径，重复实现会漂移。
pub fn encoded_len(values: &[XDataValue]) -> usize {
    let payload: usize = values
        .iter()
        .map(|value| match value {
            XDataValue::String(text) => text.len(),
            XDataValue::Integer32(_)
            | XDataValue::Real(_)
            | XDataValue::Distance(_)
            | XDataValue::ScaleFactor(_) => 4,
            XDataValue::Integer16(_) => 2,
            XDataValue::Handle(_) => 8,
            XDataValue::Point3D(_)
            | XDataValue::Position3D(_)
            | XDataValue::Displacement3D(_)
            | XDataValue::Direction3D(_) => 24,
            other => format!("{other:?}").len(),
        })
        .sum();
    payload + values.len()
}

/// 一个实体的溢出 XRecord key：`PID_AUTHOR_ENTITY_<HEX>`。
///
/// `Handle` 的 `Display` 是 `{:#X}`，所以后缀是**大写**十六进制。
pub fn props_ref_key(handle: Handle) -> String {
    format!("PID_AUTHOR_ENTITY_{handle}")
}

/// 记录的 `props_ref` 键值，若有。
pub fn props_ref_of(values: &[XDataValue]) -> Option<String> {
    values.iter().find_map(|value| {
        let text = as_str(value)?;
        let (key, val) = text.split_once('=')?;
        (key == PROPS_REF_KEY).then(|| val.to_string())
    })
}

/// `XDataValue::String` 的借用；其他变体返回 `None`。
pub fn as_str(value: &XDataValue) -> Option<&str> {
    match value {
        XDataValue::String(text) => Some(text),
        _ => None,
    }
}

/// 记录的所有者：模型空间 block record，每张图恰有一个。
fn owner(doc: &Document) -> Handle {
    doc.header.model_space_block_handle
}

/// 把 `props` JSON 写进模型空间扩展字典的 XRecord `key`，整条重写。
///
/// 分片理由：DWG 侧 `Chunk` 单条只写前 255 字节且不报错
/// （acadrust `object_writer/objects.rs:258`），DXF 侧无此限制，所以只用 DXF
/// 测不出这个坑。
fn store_props_record(doc: &mut Document, key: &str, handle: Handle, props_json: &str) {
    let owner = owner(doc);
    doc.ensure_xrecord(owner, key);
    if let Some(record) = doc.xrecord_mut(owner, key) {
        record.entries.clear();
        record.entries.push(XRecordEntry::string(1, KIND_ENTITY_PROPS));
        record
            .entries
            .push(XRecordEntry::string(1, format!("handle={}", handle.value())));
        for chunk in props_json.as_bytes().chunks(CHUNK_BYTES) {
            record
                .entries
                .push(XRecordEntry::new(CHUNK_CODE, XRecordValue::Chunk(chunk.to_vec())));
        }
    }
}

/// 读回 `key` 指向的 `props` JSON。记录不存在或不是本模块写的返回 `None`。
fn load_props_record(doc: &Document, key: &str) -> Option<String> {
    let record = doc.xrecord(owner(doc), key)?;
    let mut bytes = Vec::new();
    for entry in &record.entries {
        if let XRecordValue::Chunk(chunk) = &entry.value {
            bytes.extend_from_slice(chunk);
        }
    }
    if bytes.is_empty() {
        return None;
    }
    String::from_utf8(bytes).ok()
}

/// `props` JSON 串里的**真实属性字段数**。
///
/// 排除覆盖标记键 `__overrides`（`pid::propagate` 的 `OVERRIDES_KEY`）——
/// 它是字符串数组，不是属性值。用它数属性时不要把标记算进去，也不要在解析
/// 失败时静默返回 0：解析失败说明 props 损坏，返回 0 会让往返统计假通过。
///
/// 字面量就地写死而不用 `use crate::pid::propagate::OVERRIDES_KEY`：
/// `persist` 是更底层模块，反向依赖 `propagate` 会形成循环依赖。
pub fn props_len(props_json: &str) -> usize {
    let Ok(value) = serde_json::from_str::<serde_json::Value>(props_json) else {
        return 0;
    };
    let Some(object) = value.as_object() else {
        return 0;
    };
    object
        .keys()
        .filter(|key| key.as_str() != "__overrides")
        .count()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    use crate::command::EntityTransform;
    use crate::io::pid_legend;
    use codec::entities::Line;
    use codec::types::Vector3;
    use codec::xdata::ExtendedDataRecord;
    use glam::DVec3;

    const EIGHT_KEYS: [&str; 8] = [
        "type",
        "tag",
        "tag_source",
        "tag_status",
        "props",
        "ports",
        "owner",
        "topology",
    ];

    fn line(y: f64) -> codec::EntityType {
        codec::EntityType::Line(Line::from_points(
            Vector3::new(0.0, y, 0.0),
            Vector3::new(10.0, y, 0.0),
        ))
    }

    fn record_of(doc: &Document, handle: Handle) -> ExtendedDataRecord {
        doc.get_entity(handle)
            .unwrap()
            .common()
            .extended_data
            .get_record(PID_AUTHOR_APP)
            .unwrap_or_else(|| panic!("{PID_AUTHOR_APP} record missing on {handle}"))
            .clone()
    }

    /// 记录里的 `key=value` 对，用于打印差异。
    fn pairs(values: &[XDataValue]) -> Vec<String> {
        values
            .iter()
            .filter_map(|value| as_str(value).map(str::to_string))
            .collect()
    }

    fn value_of(values: &[XDataValue], key: &str) -> Option<String> {
        values.iter().find_map(|value| {
            let text = as_str(value)?;
            let (found, val) = text.split_once('=')?;
            (found == key).then(|| val.to_string())
        })
    }

    fn full_author() -> PidAuthor {
        PidAuthor {
            type_code: "EQUIPMENT".into(),
            tag: "P-1001".into(),
            tag_source: "auto".into(),
            tag_status: "ok".into(),
            props: r#"{"设计压力":"1.6MPa","公称直径":"DN50"}"#.into(),
            ports: "in1,out1".into(),
            owner: "1A2F".into(),
            topology: "pipe_line".into(),
            connector_peer: String::new(),
        }
    }

    #[test]
    fn pid_author_xdata_roundtrip_all_keys() {
        let mut doc = Document::new();
        let handle = doc.add_entity(line(0.0)).unwrap();
        let author = full_author();
        author.store(&mut doc, handle);

        let record = record_of(&doc, handle);
        let back = PidAuthor::load(&doc, handle).expect("record loads");
        let printed = pairs(&record.values);
        for key in EIGHT_KEYS {
            assert_eq!(
                value_of(&record.values, key).as_deref(),
                Some(
                    match key {
                        "type" => "EQUIPMENT",
                        "tag" => "P-1001",
                        "tag_source" => "auto",
                        "tag_status" => "ok",
                        "props" => r#"{"设计压力":"1.6MPa","公称直径":"DN50"}"#,
                        "ports" => "in1,out1",
                        "owner" => "1A2F",
                        "topology" => "pipe_line",
                        other => panic!("unexpected key {other}"),
                    }
                ),
                "key {key} differs; record holds {printed:?}"
            );
        }
        assert_eq!(back.type_code, "EQUIPMENT");
        assert_eq!(back.tag, "P-1001");
        assert_eq!(back.tag_source, "auto");
        assert_eq!(back.tag_status, "ok");
        assert_eq!(back.ports, "in1,out1");
        assert_eq!(back.owner, "1A2F");
        assert_eq!(back.topology, "pipe_line");

        let props: BTreeMap<String, String> =
            serde_json::from_str(&back.props).expect("props parses");
        assert_eq!(props.len(), 2, "props: {props:?}");
        assert_eq!(props.get("设计压力").map(String::as_str), Some("1.6MPa"));
        assert_eq!(props.get("公称直径").map(String::as_str), Some("DN50"));

        // 空 connector_peer：键不落盘，而不是落一个空值。
        assert!(
            !record
                .values
                .iter()
                .any(|v| as_str(v).is_some_and(|t| t.starts_with("connector_peer="))),
            "empty connector_peer must not be stored; record holds {printed:?}"
        );

        let mut with_peer = author.clone();
        with_peer.connector_peer = "PID-1102:2F3A".into();
        with_peer.store(&mut doc, handle);
        let record = record_of(&doc, handle);
        assert_eq!(
            value_of(&record.values, "connector_peer").as_deref(),
            Some("PID-1102:2F3A"),
            "record holds {:?}",
            pairs(&record.values)
        );
        assert_eq!(
            PidAuthor::load(&doc, handle).unwrap().connector_peer,
            "PID-1102:2F3A"
        );
    }

    #[test]
    fn pid_author_xdata_overflow_moves_props_to_xrecord() {
        let mut doc = Document::new();
        let handle = doc.add_entity(line(0.0)).unwrap();

        // 60 个键，键名与值各 20 字符。
        let key_name = |index: usize| format!("K{index:019}");
        let value = |index: usize| format!("V{index:019}");
        let mut props = BTreeMap::new();
        for index in 0..60 {
            props.insert(key_name(index), value(index));
        }
        let props_json = serde_json::to_string(&props).unwrap();
        let mut author = full_author();
        author.props = props_json.clone();

        let full = encode(&author);
        let before = encoded_len(&full);
        assert!(
            before > XDATA_SOFT_LIMIT,
            "the fixture must overflow: {before} <= {XDATA_SOFT_LIMIT}"
        );

        author.store(&mut doc, handle);
        let record = record_of(&doc, handle);
        let after = encoded_len(&record.values);
        let printed = pairs(&record.values);

        assert!(
            !record
                .values
                .iter()
                .any(|v| as_str(v).is_some_and(|t| t.starts_with("props="))),
            "overflow must drop the inline props; record holds {printed:?}"
        );
        let key = props_ref_key(handle);
        assert_eq!(
            value_of(&record.values, PROPS_REF_KEY).as_deref(),
            Some(key.as_str()),
            "record holds {printed:?}"
        );
        assert!(
            doc.xrecord(doc.header.model_space_block_handle, &key)
                .is_some(),
            "the overflow XRecord {key} must exist"
        );
        assert!(
            after <= XDATA_SOFT_LIMIT,
            "record still too long: {after} > {XDATA_SOFT_LIMIT}; holds {printed:?}"
        );

        let back = PidAuthor::load(&doc, handle).expect("record loads");
        assert_eq!(
            props_len(&back.props),
            60,
            "props keys: {} (json {} bytes)",
            props_len(&back.props),
            back.props.len()
        );
        let parsed: BTreeMap<String, String> = serde_json::from_str(&back.props).unwrap();
        assert_eq!(parsed, props, "props must survive byte for byte");

        // 溢出记录里 `props` 的字节数远超 Chunk 的 255 上限，说明分片生效。
        let stored = doc
            .xrecord(doc.header.model_space_block_handle, &key)
            .expect("xrecord");
        let chunks = stored
            .entries
            .iter()
            .filter(|entry| matches!(entry.value, XRecordValue::Chunk(_)))
            .count();
        println!(
            "overflow: encoded_len before={before} after={after} props_json={} bytes, chunks={chunks}",
            props_json.len()
        );
        assert!(chunks > 1, "props_json must be split: {chunks} chunk(s)");
    }

    #[test]
    fn pid_author_coordinates_survive_entity_transform() {
        let mut doc = Document::new();
        let handle = doc.add_entity(line(0.0)).unwrap();

        let author = full_author();
        let mut values = encode(&author);
        // 契约要求的端口坐标键：字符串与整数承载。
        values.push(XDataValue::String("port_in1=0,-12,0".into()));
        values.push(XDataValue::Integer32(100));
        // 对照组：这两个变体会被 apply_transform 改写。
        values.push(XDataValue::Position3D(Vector3::new(0.0, -12.0, 0.0)));
        values.push(XDataValue::Distance(12.0));
        set_entity_xdata(&mut doc, handle, PID_AUTHOR_APP, Some(values));

        // 先平移，再绕平移后的端口位置缩放：契约给的是
        // `Position3D` 变 `(50, 18, 0)`、`Distance` 变 `24.0`，也就是
        // 缩放中心落在平移后的那个点上。
        for step in [
            EntityTransform::Translate(DVec3::new(50.0, 30.0, 0.0)),
            EntityTransform::Scale {
                center: DVec3::new(50.0, 18.0, 0.0),
                factor: 2.0,
            },
        ] {
            let entity = doc.get_entity_mut(handle).unwrap();
            crate::scene::view::dispatch::apply_transform(entity, &step);
        }

        let record = record_of(&doc, handle);
        let printed = pairs(&record.values);
        assert_eq!(
            value_of(&record.values, "port_in1").as_deref(),
            Some("0,-12,0"),
            "string coordinates must not move; record holds {printed:?}"
        );
        let seq = record.values.iter().find_map(|value| match value {
            XDataValue::Integer32(v) => Some(*v),
            _ => None,
        });
        assert_eq!(seq, Some(100), "integer coordinates must not move");

        // 对照组：证明 apply_transform 确实生效（防假绿）。
        let position = record.values.iter().find_map(|value| match value {
            XDataValue::Position3D(p) => Some(*p),
            _ => None,
        });
        let distance = record.values.iter().find_map(|value| match value {
            XDataValue::Distance(d) => Some(*d),
            _ => None,
        });
        let position = position.expect("Position3D survives as a value");
        assert!(
            (position.x - 50.0).abs() < 1e-9
                && (position.y - 18.0).abs() < 1e-9
                && position.z.abs() < 1e-9,
            "Position3D must follow the transform, got {position:?}; record holds {printed:?}"
        );
        assert_eq!(distance, Some(24.0), "Distance must scale with the entity");
    }

    #[test]
    fn pid_author_record_survives_legend_purge() {
        let mut doc = Document::new();
        let both = doc.add_entity(line(0.0)).unwrap();
        let author_only = doc.add_entity(line(2.0)).unwrap();

        let author = full_author();
        author.store(&mut doc, both);
        author.store(&mut doc, author_only);

        let legend = vec![
            XDataValue::String("class=PIDProcessVessel".into()),
            XDataValue::String("resolved=legend:block".into()),
        ];
        set_entity_xdata(
            &mut doc,
            both,
            crate::io::PID_SEMANTICS_XDATA_APP,
            Some(legend),
        );

        let before = record_of(&doc, both).values.clone();
        let removed = pid_legend::purge(&mut doc);
        assert_eq!(
            removed, 1,
            "only the PID_SEMANTICS record counts; removed {removed}"
        );

        let after = doc
            .get_entity(both)
            .unwrap()
            .common()
            .extended_data
            .get_record(PID_AUTHOR_APP);
        let after = after
            .unwrap_or_else(|| panic!("{PID_AUTHOR_APP} was purged away"))
            .values
            .clone();
        assert_eq!(
            after,
            before,
            "PID_AUTHOR values changed; before {:?} after {:?}",
            pairs(&before),
            pairs(&after)
        );
        assert!(
            doc.get_entity(both)
                .unwrap()
                .common()
                .extended_data
                .get_record(crate::io::PID_SEMANTICS_XDATA_APP)
                .is_none(),
            "PID_SEMANTICS must be gone"
        );

        let empty = pid_legend::Recognition::default();
        pid_legend::attach(&mut doc, &empty);
        assert!(
            doc.get_entity(both)
                .unwrap()
                .common()
                .extended_data
                .get_record(PID_AUTHOR_APP)
                .is_some(),
            "attach must leave the author record alone"
        );
        assert!(
            doc.get_entity(author_only)
                .unwrap()
                .common()
                .extended_data
                .get_record(PID_AUTHOR_APP)
                .is_some(),
            "the author-only entity keeps its record"
        );
    }

    #[test]
    fn pid_author_single_record_per_entity() {
        let mut doc = Document::new();
        let handle = doc.add_entity(line(0.0)).unwrap();

        let mut first = full_author();
        first.tag = "P-1001".into();
        first.store(&mut doc, handle);
        let mut second = full_author();
        second.tag = "P-1002".into();
        second.store(&mut doc, handle);

        let records = doc
            .get_entity(handle)
            .unwrap()
            .common()
            .extended_data
            .records()
            .to_vec();
        let matching = records
            .iter()
            .filter(|record| record.application_name == PID_AUTHOR_APP)
            .count();
        assert_eq!(
            matching, 1,
            "one record per APPID; records {:?}",
            records
                .iter()
                .map(|record| record.application_name.clone())
                .collect::<Vec<_>>()
        );
        let record = record_of(&doc, handle);
        assert_eq!(value_of(&record.values, "tag").as_deref(), Some("P-1002"));
        assert!(
            !record
                .values
                .iter()
                .any(|value| as_str(value) == Some("tag=P-1001")),
            "the old tag must not linger; record holds {:?}",
            pairs(&record.values)
        );
    }

    #[test]
    fn pid_author_app_id_registered_and_roundtrips() {
        use codec::{Dwg, Format};

        let mut doc = Document::new();
        let handle = doc.add_entity(line(0.0)).unwrap();
        assert!(
            !doc.app_ids.contains(PID_AUTHOR_APP),
            "a fresh document must not carry {PID_AUTHOR_APP}"
        );

        let mut author = full_author();
        author.connector_peer = "PID-1102:2F3A".into();
        author.store(&mut doc, handle);

        assert!(
            doc.app_ids.contains(PID_AUTHOR_APP),
            "store must register {PID_AUTHOR_APP}"
        );
        assert!(
            doc.app_ids.get(PID_AUTHOR_APP).unwrap().handle.is_valid(),
            "the registered APPID needs a non-null handle"
        );

        let expected = record_of(&doc, handle).values.clone();
        let (bytes, losses) = Dwg.write(&doc).expect("DWG write");
        assert!(losses.is_empty(), "{losses:?}");
        let (back, read_losses) = Dwg.read(&bytes).expect("DWG read");
        assert!(read_losses.is_empty(), "{read_losses:?}");

        assert!(
            back.app_ids.contains(PID_AUTHOR_APP),
            "APPID lost in the DWG round trip; table {:?}",
            back.app_ids.iter().map(|app| app.name.clone()).collect::<Vec<_>>()
        );
        let record = back
            .get_entity(handle)
            .unwrap_or_else(|| panic!("entity {handle} lost"))
            .common()
            .extended_data
            .get_record(PID_AUTHOR_APP)
            .unwrap_or_else(|| panic!("{PID_AUTHOR_APP} record lost"))
            .clone();
        assert_eq!(
            record.values,
            expected,
            "nine keys changed; before {:?} after {:?}",
            pairs(&expected),
            pairs(&record.values)
        );
        let reloaded = PidAuthor::load(&back, handle).expect("record loads after the round trip");
        assert_eq!(reloaded.connector_peer, "PID-1102:2F3A");
        assert_eq!(reloaded.tag, "P-1001");
        assert_eq!(reloaded.type_code, "EQUIPMENT");
    }

    #[test]
    fn props_len_excludes_override_marks() {
        // 带覆盖标记：只数真实属性
        assert_eq!(props_len(r#"{"a":"1","b":"2","__overrides":["a"]}"#), 2);
        // 纯属性
        assert_eq!(props_len(r#"{"a":"1"}"#), 1);
        // 空串
        assert_eq!(props_len(""), 0);
        // 空对象
        assert_eq!(props_len("{}"), 0);
        // 只有覆盖标记
        assert_eq!(props_len(r#"{"__overrides":["a"]}"#), 0);
        // 多字段 + 多标记
        assert_eq!(
            props_len(r#"{"a":"1","b":"2","c":"3","__overrides":["a","c"]}"#),
            3
        );
    }
}
