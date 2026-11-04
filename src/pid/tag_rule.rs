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
