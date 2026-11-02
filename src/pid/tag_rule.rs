//! 位号规则 `TagRule`。
//!
//! 本 feature 只钉住规则的**数据形状**，让类型库能把规则挂在类型上并按
//! 继承链解析出「生效规则 + 生效来源类型编码」。位号的生成（序号分配、
//! 唯一性检查、拼接）由后续 feature `pid-tag-gen` 实现，这里不含任何
//! 生成逻辑。
//!
//! 八个字段对应契约的八项：
//! 格式名称 / 自动编码 / 起始编码 / 增量 / 后缀递增 / 层级选择 / 拼接字段 / 唯一校验。

use serde::{Deserialize, Serialize};

/// 一个类型的位号生成规则。
///
/// `pattern` 里 `{序号}` 是自增段占位符（例如 `P-{序号}` 生成 `P-1001`），
/// 其余字符按字面拼进位号。`start` 是序号首次分配的值，`step` 是每分配一个
/// 位号序号加多少，`pad` 是序号补零到几位（0 表示不补零）。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TagRule {
    /// 格式名称：人在界面上看到的规则名，例如 `设备位号`。
    #[serde(default)]
    pub name: String,
    /// 自动编码：位号格式，`{序号}` 是自增段占位符，例如 `P-{序号}`。
    #[serde(default)]
    pub pattern: String,
    /// 起始编码：序号首次分配的值。
    #[serde(default)]
    pub start: i64,
    /// 增量：每分配一个位号，序号加多少。
    #[serde(default = "default_step")]
    pub step: i64,
    /// 后缀递增：序号补零到几位，0 表示不补零（`1` → `001` 需要 `pad = 3`）。
    #[serde(default)]
    pub pad: u32,
    /// 层级选择：序号在哪一层共享/独立。取值约定 `GLOBAL` / `TYPE` /
    /// `PARENT` / `SHEET`，`None` 等同 `GLOBAL`；本 feature 不校验取值，
    /// 由 `pid-tag-gen` 解释。
    #[serde(default)]
    pub level: Option<String>,
    /// 拼接字段：除序号外，位号里还要按顺序拼进去的属性字段名。
    #[serde(default)]
    pub join: Vec<String>,
    /// 唯一校验：生成时是否必须整图唯一。
    #[serde(default)]
    pub unique: bool,
}

fn default_step() -> i64 {
    1
}

impl Default for TagRule {
    fn default() -> Self {
        Self {
            name: String::new(),
            pattern: String::new(),
            start: 0,
            step: default_step(),
            pad: 0,
            level: None,
            join: Vec::new(),
            unique: false,
        }
    }
}

impl TagRule {
    /// 位号格式里自增段的占位符。
    pub const SEQUENCE_PLACEHOLDER: &'static str = "{序号}";

    /// 一个只写了格式与起始值的规则，其余字段取默认。
    pub fn new(pattern: impl Into<String>, start: i64) -> Self {
        Self {
            pattern: pattern.into(),
            start,
            ..Self::default()
        }
    }

    /// 格式里是否写了自增段占位符。
    pub fn has_sequence(&self) -> bool {
        self.pattern.contains(Self::SEQUENCE_PLACEHOLDER)
    }
}
