use serde::{Deserialize, Serialize};

/// 层级选择：序号在哪一层共享/独立。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum HierarchyMode {
    /// 当前类型依次递增：子类型从自身起点独立计数（契约默认）。
    #[serde(rename = "当前类型依次递增", alias = "TYPE", alias = "CurrentTypeIncrement")]
    CurrentTypeIncrement,
    /// 父类型递增：子类型接续父类型已用最大序号。
    #[serde(rename = "父类型递增", alias = "PARENT", alias = "ParentTypeIncrement")]
    ParentTypeIncrement,
}

impl Default for HierarchyMode {
    fn default() -> Self {
        Self::CurrentTypeIncrement
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TagRule {
    #[serde(default)]
    pub name: String,
    #[serde(default = "default_auto_code")]
    pub auto_code: bool,
    #[serde(default)]
    pub pattern: String,
    #[serde(default)]
    pub start: i64,
    #[serde(default = "default_step")]
    pub step: i64,
    #[serde(default)]
    pub suffix_increment: bool,
    #[serde(default)]
    pub level: Option<HierarchyMode>,
    #[serde(default)]
    pub join: Option<String>,
    #[serde(default)]
    pub unique: bool,
    #[serde(default)]
    pub pad: u32,
}

fn default_auto_code() -> bool {
    true
}

fn default_step() -> i64 {
    1
}

impl Default for TagRule {
    fn default() -> Self {
        Self {
            name: String::new(),
            auto_code: default_auto_code(),
            pattern: String::new(),
            start: 0,
            step: default_step(),
            suffix_increment: false,
            level: None,
            join: None,
            unique: false,
            pad: 0,
        }
    }
}

impl TagRule {
    pub const SEQUENCE_PLACEHOLDER: &'static str = "{序号}";
    pub const SUFFIX_PLACEHOLDER: &'static str = "{后缀}";

    pub fn new(pattern: impl Into<String>, start: i64) -> Self {
        Self {
            pattern: pattern.into(),
            start,
            ..Self::default()
        }
    }

    pub fn has_sequence(&self) -> bool {
        self.pattern.contains(Self::SEQUENCE_PLACEHOLDER)
    }

    pub fn has_suffix(&self) -> bool {
        self.pattern.contains(Self::SUFFIX_PLACEHOLDER)
    }

    pub fn hierarchy(&self) -> HierarchyMode {
        self.level.unwrap_or_default()
    }
}

/// `architecture.md` 附录 A.9 的默认位号规则表。
///
/// 类型库（`assets/pid-types.json`）只给根 `EQUIPMENT` 定义了规则：`VALVE`
/// 与 `INSTRUMENT` 不进那棵树（D4 / D16：`TypeLib` 强制单根，且
/// `VAL-SEM-015` 断言 `EQUIPMENT` 的子类型恰为 `["PUMP","VESSEL"]`），
/// 而符号库的 `symbol_type_map` 又会把它们作为作者类型编码写进
/// `PID_AUTHOR` 的 `type` 键。于是**库外的类型编码**在解析位号规则时按这张
/// 表兜底，库内的类型编码仍只认类型树（库里有节点但整条祖先链都没规则时
/// 仍报 `TAG_RULE_MISSING`，见 `empty_rule_reports_missing_rule`）。
///
/// 每条都是 `unique = true`：唯一键是 `(类型编码, 位号)` 复合键，同类型同
/// 位号即冲突。
pub fn default_tag_rule(type_code: &str) -> Option<TagRule> {
    let code = type_code.trim().to_ascii_uppercase();
    let rule = match code.as_str() {
        // 设备：`P-1001`、`P-1002`…
        "EQUIPMENT" => TagRule::new("P-{序号}", 1001),
        // 阀门：`V-1001` 起，与设备的序号空间互不相干。
        "VALVE" => TagRule::new("V-{序号}", 1001),
        // 仪表：前缀取 `仪表功能` 属性（`TI` / `PI` / `FI`…），故 `TI-1001`。
        "INSTRUMENT" => {
            let mut rule = TagRule::new("{仪表功能}-{序号}", 1001);
            rule.join = Some("仪表功能".to_string());
            rule
        }
        // 管线：`L-1001`…
        "PIPE_LINE" => TagRule::new("L-{序号}", 1001),
        // 管路：`{管线号}-01`、`{管线号}-02`…，后缀两位补零、递增。
        "PIPE_RUN" => {
            let mut rule = TagRule::new("{管线号}-{后缀}", 1);
            rule.join = Some("管线号".to_string());
            rule.suffix_increment = true;
            rule
        }
        _ => return None,
    };
    Some(TagRule {
        unique: true,
        ..rule
    })
}
