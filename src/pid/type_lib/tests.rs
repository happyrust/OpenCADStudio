//! 类型库契约断言（VAL-SEM-015 至 VAL-SEM-019），外加落盘与 JSON 形状的
//! 边界用例。

use super::*;
use codec::objects::XRecordValue;

// ── 夹具 ─────────────────────────────────────────────────────────────────

/// 契约里的类型树：`EQUIPMENT` → `VESSEL`、`PUMP`；`VESSEL` → `TANK`。
fn contract_tree() -> Vec<TypeNode> {
    vec![
        TypeNode::new("EQUIPMENT", None::<String>).with_label("设备"),
        TypeNode::new("VESSEL", Some("EQUIPMENT")).with_label("容器"),
        TypeNode::new("TANK", Some("VESSEL")).with_label("储罐"),
        TypeNode::new("PUMP", Some("EQUIPMENT")).with_label("泵"),
    ]
}

fn empty_document() -> Document {
    Document::new()
}

/// 记录的所有者，与实现里的 `owner` 同义，供断言用。
fn owner_of(doc: &Document) -> Handle {
    doc.header.model_space_block_handle
}

/// 一个占位实体：位号生成只关心 `PidAuthor` XDATA，不关心几何。
fn line() -> codec::EntityType {
    use codec::entities::Line;
    use codec::types::Vector3;
    codec::EntityType::Line(Line::from_points(
        Vector3::new(0.0, 0.0, 0.0),
        Vector3::new(10.0, 0.0, 0.0),
    ))
}

// ── VAL-SEM-015 ───────────────────────────────────────────────────────────

/// VAL-SEM-015：祖先链自底向上含自身；子类型按编码字典序；上溯必达根、无环。
#[test]
fn type_lib_tree_ancestry_and_children() {
    let lib = TypeLib::new(contract_tree()).expect("契约类型树应当构造成功");
    assert_eq!(lib.root_code(), "EQUIPMENT");

    let ancestors = lib.ancestors("TANK");
    assert_eq!(
        ancestors,
        ["TANK", "VESSEL", "EQUIPMENT"],
        "TANK 的祖先链（自底向上、含自身）实际为 {ancestors:?}"
    );

    let children = lib.children("EQUIPMENT");
    assert_eq!(
        children,
        ["PUMP", "VESSEL"],
        "EQUIPMENT 的子类型实际为 {children:?}（应按编码字典序）"
    );

    // 每个类型上溯必达根，步数有限。
    for node in lib.types() {
        let chain = lib.ancestors(&node.type_code);
        assert_eq!(
            chain.last().map(String::as_str),
            Some(lib.root_code()),
            "{} 的上溯链 {chain:?} 未到达根",
            node.type_code
        );
        assert!(chain.len() <= lib.len(), "{} 的上溯链过长", node.type_code);
    }

    // 环必须检测成 Err，不能死循环。
    let cyclic = TypeLib::new(vec![
        TypeNode::new("EQUIPMENT", None::<String>),
        TypeNode::new("VESSEL", Some("TANK")),
        TypeNode::new("TANK", Some("VESSEL")),
    ]);
    match cyclic {
        Err(err) => assert_eq!(
            err.code(),
            "TYPE_LIB_CYCLE",
            "环的报错编码实际为 {}",
            err.code()
        ),
        Ok(_) => panic!("成环的类型树必须返回 Err，实际构造成功"),
    }

    // 父类型缺失、多根、重复编码同样必须报错。
    let missing_parent = TypeLib::new(vec![
        TypeNode::new("EQUIPMENT", None::<String>),
        TypeNode::new("VESSEL", Some("NO_SUCH_TYPE")),
    ]);
    assert_eq!(
        missing_parent.map_err(|err| err.code()),
        Err("TYPE_LIB_UNKNOWN_PARENT")
    );

    let two_roots = TypeLib::new(vec![
        TypeNode::new("EQUIPMENT", None::<String>),
        TypeNode::new("VESSEL", None::<String>),
    ]);
    assert_eq!(
        two_roots.map_err(|err| err.code()),
        Err("TYPE_LIB_MULTIPLE_ROOTS")
    );

    let duplicated = TypeLib::new(vec![
        TypeNode::new("EQUIPMENT", None::<String>),
        TypeNode::new("EQUIPMENT", None::<String>),
    ]);
    assert_eq!(
        duplicated.map_err(|err| err.code()),
        Err("TYPE_LIB_TYPE_DUPLICATED")
    );
}

// ── VAL-SEM-016 ───────────────────────────────────────────────────────────

/// VAL-SEM-016：分组恰好 2 组、组序与组内字段序保序；字段重复构造返回 Err。
#[test]
fn type_lib_property_groups_preserve_order() {
    let equipment = TypeNode::new("EQUIPMENT", None::<String>)
        .with_label("设备")
        .with_group(PropertyGroup::new(
            "基本属性",
            ["位号", "名称", "描述"],
        ))
        .with_group(PropertyGroup::new(
            "工艺参数",
            ["设计压力", "设计温度", "操作压力"],
        ));
    let lib = TypeLib::new(vec![equipment]).expect("契约分组应当构造成功");

    let groups = lib.property_groups("EQUIPMENT");
    let names: Vec<&str> = groups.iter().map(|group| group.name.as_str()).collect();
    assert_eq!(names, ["基本属性", "工艺参数"], "分组名顺序实际为 {names:?}");

    let process: Vec<&str> = groups
        .iter()
        .find(|group| group.name == "工艺参数")
        .map(|group| group.fields.iter().map(String::as_str).collect())
        .unwrap_or_default();
    assert_eq!(
        process,
        ["设计压力", "设计温度", "操作压力"],
        "工艺参数字段顺序实际为 {process:?}"
    );

    // 同一字段落在两个分组：Err，不 panic、不部分写入。
    let duplicated = TypeLib::new(vec![TypeNode::new("EQUIPMENT", None::<String>)
        .with_group(PropertyGroup::new("基本属性", ["位号", "名称"]))
        .with_group(PropertyGroup::new("工艺参数", ["名称", "设计压力"]))]);
    match duplicated {
        Err(err) => assert_eq!(
            err.code(),
            "TYPE_LIB_FIELD_DUPLICATED",
            "重复字段的报错编码实际为 {}",
            err.code()
        ),
        Ok(_) => panic!("同一字段出现在两个分组时必须返回 Err"),
    }

    // 同一分组内重复同样算重复。
    let duplicated_within_group = TypeLib::new(vec![TypeNode::new("EQUIPMENT", None::<String>)
        .with_group(PropertyGroup::new("基本属性", ["位号", "位号"]))]);
    assert_eq!(
        duplicated_in_group_code(duplicated_within_group),
        "TYPE_LIB_FIELD_DUPLICATED"
    );
}

fn duplicated_in_group_code(result: Result<TypeLib, TypeLibError>) -> &'static str {
    match result {
        Ok(_) => "OK",
        Err(err) => err.code(),
    }
}

// ── VAL-SEM-017 ───────────────────────────────────────────────────────────

/// VAL-SEM-017：数据要素四项写读一致；模型无 iced 可构造可序列化；
/// `value_type` 只接受五个中文值。
#[test]
fn type_lib_data_element_roundtrip() {
    let equipment = TypeNode::new("EQUIPMENT", None::<String>)
        .with_label("设备")
        .with_element(DataElement::enumeration(
            "MAT",
            "材质",
            ["碳钢", "不锈钢"],
        ));
    let lib = TypeLib::new(vec![equipment]).expect("契约数据要素应当构造成功");

    let original = lib
        .data_element("EQUIPMENT", "MAT")
        .expect("EQUIPMENT 上应当有 MAT")
        .clone();
    assert_eq!(original.code, "MAT");
    assert_eq!(original.label, "材质");
    assert_eq!(original.value_type, ValueType::Enum);
    assert_eq!(original.options, ["碳钢", "不锈钢"]);

    // 纯 serde_json 往返：不依赖 GUI / Scene。
    let json = lib.to_json();
    let back = TypeLib::from_json(&json).expect("类型库 JSON 应当可往返");
    let read_back = back
        .data_element("EQUIPMENT", "MAT")
        .expect("往返后 EQUIPMENT 上应当仍有 MAT");
    assert_eq!(
        read_back, &original,
        "读回的数据要素与原始不一致：\n原始 {:?}\n读回 {:?}",
        original, read_back
    );

    // 数据要素本身的 JSON 形状：value_type 是中文值。
    let element_json = serde_json::to_string(&original).expect("数据要素应当可序列化");
    assert!(
        element_json.contains("\"value_type\":\"枚举\""),
        "value_type 应序列化成中文 枚举，实际 {element_json}"
    );

    // 写进 XRecord 再读回，四项完全一致。
    let mut doc = empty_document();
    lib.store(&mut doc);
    let loaded = TypeLib::load(&doc).expect("写入后应当能读回类型库");
    let stored = loaded
        .data_element("EQUIPMENT", "MAT")
        .expect("读回的库里应当有 MAT");
    assert_eq!(stored.code, "MAT", "读回 code 为 {}", stored.code);
    assert_eq!(stored.label, "材质", "读回 label 为 {}", stored.label);
    assert_eq!(
        stored.value_type,
        ValueType::Enum,
        "读回 value_type 为 {}",
        stored.value_type.name()
    );
    assert_eq!(
        stored.options,
        ["碳钢", "不锈钢"],
        "读回 options 为 {:?}",
        stored.options
    );
    assert_eq!(&loaded, &lib, "整库写读应当一致");

    // 记录挂在模型空间 block record 的扩展字典下。
    let dictionary = doc
        .extension_dictionary_handle(owner_of(&doc))
        .expect("模型空间 block record 应当有扩展字典");
    let record = doc
        .xrecord(owner_of(&doc), XRECORD_KEY)
        .expect("应当存在 PID_AUTHOR_TYPE_LIB 记录");
    assert_eq!(
        record.owner, dictionary,
        "记录的 owner 应当是扩展字典 handle"
    );

    // 五个中文值都接受，其他值反序列化失败。
    for name in ValueType::NAMES {
        let quoted = format!("\"{name}\"");
        let parsed: ValueType = serde_json::from_str(&quoted)
            .unwrap_or_else(|_| panic!("{name} 应当能反序列化"));
        assert_eq!(parsed.name(), name);
        assert_eq!(ValueType::from_name(name), Some(parsed));
    }
    for rejected in ["text", "Number", "枚举值", "", "枚举 "] {
        let quoted = format!("\"{rejected}\"");
        assert!(
            serde_json::from_str::<ValueType>(&quoted).is_err(),
            "{rejected:?} 不应能反序列化成 ValueType"
        );
        assert_eq!(ValueType::from_name(rejected), None);
    }

    // 库里写了非法 value_type，整库反序列化失败。
    let bad_json = r#"{"types":[{"type_code":"EQUIPMENT","label":"设备","parent":null,
        "groups":[],"elements":[{"code":"MAT","label":"材质","value_type":"枚举值","options":[]}],
        "tag_rule":null,"color_rule":null}]}"#;
    assert!(
        TypeLib::from_json(bad_json).is_err(),
        "非法 value_type 应当让整库反序列化失败"
    );
}

// ── VAL-SEM-018 ───────────────────────────────────────────────────────────

/// VAL-SEM-018：子类型规则表为空时，解析出的位号/颜色规则来自父类型，
/// 且结果携带生效来源类型编码。
///
/// 位号的**生成**由 `pid-tag-gen` 实现，本 feature 只保证解析出的规则是
/// 父的规则（`P-{序号}` / 起始 1001 / 增量 1），按该规则依次分配即得
/// `P-1001`、`P-1002`。
#[test]
fn type_lib_child_inherits_unoverridden_rules() {
    let mut rule = TagRule::new("P-{序号}", 1001);
    rule.name = "设备位号".to_string();
    rule.step = 1;
    let lib = TypeLib::new(vec![
        TypeNode::new("EQUIPMENT", None::<String>)
            .with_label("设备")
            .with_tag_rule(rule.clone())
            .with_color_rule("红"),
        TypeNode::new("VESSEL", Some("EQUIPMENT")).with_label("容器"),
    ])
    .expect("继承夹具应当构造成功");

    let resolved_tag = lib
        .resolve_tag_rule("VESSEL")
        .unwrap_or_else(|| panic!("VESSEL 应当解析到位号规则"));
    assert_eq!(resolved_tag.0, rule, "VESSEL 生效的位号规则应为父的规则");
    assert_eq!(
        resolved_tag.1, "EQUIPMENT",
        "生效来源类型编码实际为 {}",
        resolved_tag.1
    );
    assert_eq!(resolved_tag.0.pattern, "P-{序号}");
    assert_eq!(resolved_tag.0.start, 1001);
    assert_eq!(resolved_tag.0.step, 1);

    let resolved_color = lib
        .resolve_color_rule("VESSEL")
        .unwrap_or_else(|| panic!("VESSEL 应当解析到颜色规则"));
    assert_eq!(resolved_color.0, "红", "颜色规则值实际为 {}", resolved_color.0);
    assert_eq!(
        resolved_color.1, "EQUIPMENT",
        "颜色规则生效来源类型编码实际为 {}",
        resolved_color.1
    );

    // 根自己解析到的来源是自己。
    let own = lib.resolve_tag_rule("EQUIPMENT").expect("根应当有规则");
    assert_eq!(own.1, "EQUIPMENT");

    // 全链都没有规则时是 None，而不是凭空造一条。
    let bare = TypeLib::new(vec![TypeNode::new("EQUIPMENT", None::<String>)])
        .expect("单根库应当构造成功");
    assert_eq!(bare.resolve_tag_rule("EQUIPMENT"), None);
    assert_eq!(bare.resolve_color_rule("EQUIPMENT"), None);

    // 契约要求：对 VESSEL 生成 2 个实例，位号依次为 P-1001、P-1002。
    let mut doc = Document::new();
    let mut tags = Vec::new();
    for _ in 0..2 {
        let handle = doc.add_entity(line()).expect("实体应当能加入模型空间");
        let view = crate::pid::tag_engine::generate(&lib, &doc, "VESSEL", &BTreeMap::new())
            .expect("VESSEL 位号生成应当成功");
        crate::pid::persist::PidAuthor {
            type_code: "VESSEL".to_string(),
            tag: view.tag.clone(),
            tag_source: view.tag_source.clone(),
            tag_status: view.tag_status.clone(),
            props: "{}".to_string(),
            ..Default::default()
        }
        .store(&mut doc, handle);
        tags.push(view.tag);
    }
    assert_eq!(tags, ["P-1001", "P-1002"], "VESSEL 位号序列实际为 {tags:?}");
}

// ── VAL-SEM-019 ───────────────────────────────────────────────────────────

/// VAL-SEM-019：子类型显式覆盖优先，且不影响兄弟子类型。
#[test]
fn type_lib_child_override_wins_and_is_scoped() {
    let parent_rule = TagRule::new("P-{序号}", 1001);
    let vessel_rule = TagRule::new("V-{序号}", 1);
    let lib = TypeLib::new(vec![
        TypeNode::new("EQUIPMENT", None::<String>)
            .with_label("设备")
            .with_tag_rule(parent_rule.clone())
            .with_color_rule("红"),
        TypeNode::new("VESSEL", Some("EQUIPMENT"))
            .with_label("容器")
            .with_tag_rule(vessel_rule.clone()),
        TypeNode::new("PUMP", Some("EQUIPMENT")).with_label("泵"),
    ])
    .expect("覆盖夹具应当构造成功");

    let vessel = lib.resolve_tag_rule("VESSEL").expect("VESSEL 应当有规则");
    assert_eq!(vessel.0.pattern, "V-{序号}", "VESSEL 位号格式实际为 {}", vessel.0.pattern);
    assert_eq!(vessel.0.start, 1, "VESSEL 起始编码实际为 {}", vessel.0.start);
    assert_eq!(vessel.1, "VESSEL", "VESSEL 生效来源实际为 {}", vessel.1);

    let pump = lib.resolve_tag_rule("PUMP").expect("PUMP 应当继承到规则");
    assert_eq!(pump.0.pattern, "P-{序号}", "PUMP 位号格式实际为 {}", pump.0.pattern);
    assert_eq!(pump.0.start, 1001, "PUMP 起始编码实际为 {}", pump.0.start);
    assert_eq!(
        pump.1, "EQUIPMENT",
        "PUMP 生效来源实际为 {}（不应受 VESSEL 覆盖影响）",
        pump.1
    );

    // 颜色规则 VESSEL 未覆盖，仍继承父的 红。
    let vessel_color = lib
        .resolve_color_rule("VESSEL")
        .expect("VESSEL 应当继承颜色规则");
    assert_eq!(vessel_color, ("红".to_string(), "EQUIPMENT".to_string()));

    // 契约要求：VESSEL×2 → V-1、V-2；PUMP×1 → P-1001。
    let mut doc = Document::new();
    let mut vessel_tags = Vec::new();
    for _ in 0..2 {
        let handle = doc.add_entity(line()).expect("实体应当能加入模型空间");
        let view = crate::pid::tag_engine::generate(&lib, &doc, "VESSEL", &BTreeMap::new())
            .expect("VESSEL 位号生成应当成功");
        crate::pid::persist::PidAuthor {
            type_code: "VESSEL".to_string(),
            tag: view.tag.clone(),
            tag_source: view.tag_source.clone(),
            tag_status: view.tag_status.clone(),
            props: "{}".to_string(),
            ..Default::default()
        }
        .store(&mut doc, handle);
        vessel_tags.push(view.tag);
    }
    let handle = doc.add_entity(line()).expect("实体应当能加入模型空间");
    let pump_view = crate::pid::tag_engine::generate(&lib, &doc, "PUMP", &BTreeMap::new())
        .expect("PUMP 位号生成应当成功");
    crate::pid::persist::PidAuthor {
        type_code: "PUMP".to_string(),
        tag: pump_view.tag.clone(),
        tag_source: pump_view.tag_source.clone(),
        tag_status: pump_view.tag_status.clone(),
        props: "{}".to_string(),
        ..Default::default()
    }
    .store(&mut doc, handle);
    assert_eq!(vessel_tags, ["V-1", "V-2"], "VESSEL 位号序列实际为 {vessel_tags:?}");
    assert_eq!(pump_view.tag, "P-1001", "PUMP 位号实际为 {}", pump_view.tag);
}

// ── 落盘与数据形状 ────────────────────────────────────────────────────────

/// 内置 `assets/pid-types.json` 必须能构造出库，且契约里的四个类型都在。
#[test]
fn embedded_assets_parse_and_carry_the_contract_types() {
    let lib = TypeLib::embedded();
    assert_eq!(lib.root_code(), "EQUIPMENT");
    assert_eq!(lib.children("EQUIPMENT"), ["PUMP", "VESSEL"]);
    assert_eq!(lib.ancestors("TANK"), ["TANK", "VESSEL", "EQUIPMENT"]);
    assert_eq!(
        lib.data_element("EQUIPMENT", "MAT").map(|element| (
            element.code.as_str(),
            element.label.as_str(),
            element.value_type,
            element.options.clone()
        )),
        Some((
            "MAT",
            "材质",
            ValueType::Enum,
            vec!["碳钢".to_string(), "不锈钢".to_string()]
        ))
    );
    let groups = lib.property_groups("EQUIPMENT");
    assert_eq!(groups.len(), 2, "EQUIPMENT 应当恰有 2 个分组");
    assert_eq!(groups[0].name, "基本属性");
    assert_eq!(groups[1].fields, ["设计压力", "设计温度", "操作压力"]);
    assert_eq!(lib.resolve_color_rule("VESSEL").map(|(_, from)| from), Some("EQUIPMENT".to_string()));
}

/// 空库删掉记录而不是留一条空的；其他 key 的记录不受影响。
#[test]
fn storing_an_empty_library_removes_only_its_own_record() {
    let mut doc = empty_document();
    let lib = TypeLib::embedded();
    lib.store(&mut doc);
    assert!(doc.xrecord(owner_of(&doc), XRECORD_KEY).is_some());

    // 邻居记录：同一个扩展字典里别的 key。
    let owner = owner_of(&doc);
    doc.ensure_xrecord(owner, "SOME_OTHER_KEY");
    if let Some(other) = doc.xrecord_mut(owner, "SOME_OTHER_KEY") {
        other.entries.push(codec::objects::XRecordEntry::string(1, "keep me"));
    }

    // 用只有一个节点的空壳库模拟「库为空」的落盘路径。
    let emptied = TypeLib {
        types: Default::default(),
        root: String::new(),
    };
    assert!(emptied.is_empty());
    emptied.store(&mut doc);
    assert!(
        doc.xrecord(owner_of(&doc), XRECORD_KEY).is_none(),
        "空库应当删掉 PID_AUTHOR_TYPE_LIB 记录"
    );
    assert!(
        doc.xrecord(owner_of(&doc), "SOME_OTHER_KEY").is_some(),
        "删记录不应波及同一个扩展字典里的其他 key"
    );
    assert!(TypeLib::load(&doc).is_none(), "没有记录时读回应当是 None");
}

/// 大库的 JSON 超过单条 entry 的字节上限时按 UTF-8 边界切块，读回仍一致。
#[test]
fn a_large_library_is_chunked_on_utf8_boundaries() {
    let mut equipment = TypeNode::new("EQUIPMENT", None::<String>).with_label("设备");
    for index in 0..200 {
        equipment = equipment.with_element(DataElement::enumeration(
            format!("CODE{index:03}"),
            format!("要素{index:03}"),
            ["碳钢", "不锈钢", "铸铁"],
        ));
    }
    let lib = TypeLib::new(vec![equipment]).expect("大库应当构造成功");
    let json = lib.to_json();
    assert!(
        json.len() > CHUNK_BYTES,
        "夹具应当超过单块上限，实际 {} 字节",
        json.len()
    );

    let mut doc = empty_document();
    lib.store(&mut doc);
    let record = doc
        .xrecord(owner_of(&doc), XRECORD_KEY)
        .expect("应当写入记录");
    assert!(record.entries.len() > 1, "应当切成多块");

    // 统一到 graph_data 布局后，前两条是 kind= / key= 前缀，数据条目是
    // part=<n>:<片段>。只校验数据条目的正文长度。
    let data_entries: Vec<_> = record
        .entries
        .iter()
        .filter(|entry| {
            entry
                .value
                .as_string()
                .is_some_and(|text| text.starts_with("part="))
        })
        .collect();
    assert!(
        data_entries.len() > 1,
        "大库应当切成多条 part= 条目，实际 {} 条",
        data_entries.len()
    );
    for entry in &data_entries {
        assert_eq!(entry.code, CHUNK_CODE);
        match &entry.value {
            XRecordValue::String(text) => {
                assert!(
                    text.len() <= CHUNK_BYTES + 16,
                    "单块 {} 字节超上限",
                    text.len()
                );
                assert!(std::str::from_utf8(text.as_bytes()).is_ok());
            }
            other => panic!("块应当是字符串，实际 {other:?}"),
        }
    }

    let loaded = TypeLib::load(&doc).expect("分块写读应当一致");
    assert_eq!(loaded, lib, "分块写读后整库应当一致");
    assert_eq!(loaded.data_elements("EQUIPMENT").len(), 200);
}

/// 重复 store 是整 key 重写，不会追加出第二份内容。
#[test]
fn storing_twice_replaces_the_record_content() {
    let mut doc = empty_document();
    let small = TypeLib::new(vec![TypeNode::new("EQUIPMENT", None::<String>).with_label("甲")])
        .expect("小库应当构造成功");
    let large = TypeLib::embedded();
    small.store(&mut doc);
    large.store(&mut doc);
    let loaded = TypeLib::load(&doc).expect("应当能读回");
    assert_eq!(loaded, large, "第二次 store 应当整 key 覆盖");
    assert_eq!(
        doc.xrecord(owner_of(&doc), XRECORD_KEY)
            .map(|record| record.entries.len()),
        Some(crate::pid::graph_data::DATA_PREFIX_ENTRIES + 1),
        "小库内容不应残留：应当恰有 kind=/key= 两条前缀加一条数据"
    );
}
