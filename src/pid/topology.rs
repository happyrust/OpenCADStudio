//! 管线拓扑表：管线 → 管路 → 管段三级对象、管件端口绑定、页间连接符对端。
//!
//! 落盘载体是 XRecord key [`crate::pid::graph_data::PID_AUTHOR_TOPOLOGY`]，
//! 实体 handle 做外键。JSON 形状见 `validation-contract.md` §0.4（逐字为判据，
//! 不要另起字段别名）。
//!
//! 本模块只做「表的读写 + 结构性维护」，属性下传在 [`crate::pid::propagate`]，
//! 位号生成在 [`crate::pid::tag_engine`]，实体创建与撤销由调用方负责。
//!
//! 待实现（M5 的 `pipe-topology-worker` 填充）：
//! - `TopologyTable` 的 serde 结构与 `load` / `store`（整 key 重写）
//! - 管线 / 管路 / 管段的增删与位号关联
//! - 管段拆分（保留原 handle 作第一段）与合并
//! - 管件端口绑定、页间连接符对端
//! - 悬空引用校验（拓扑表 handle 必须都能在文档里找到）
