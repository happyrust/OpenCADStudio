# 复用映射

本文档把本规格包的新增能力逐条映射到仓库中真实存在的现有实现，说明复用于何处、还需改造什么，共 16 行；本文档只引用需求 ID，不定义任何需求，因此不含 `REQ-` 开头的 H3 标题。

## 复用映射表

下表逐行列出现有实现、仓库相对路径、复用位置与需改造点：

| 现有实现 | 路径 | 复用于 | 需改造点 |
| --- | --- | --- | --- |
| 图例识别器（块字典、图层前缀、松散圆识别、炸开图形形状哈希、人工分组覆盖） | `src/io/pid_legend/` | L2 符号实例化入口；存量图纸导入的识别预处理 | 识别结果须挂接类型与属性集；须输出置信度分级所需的识别路径与配对代价 |
| 位号配对（最小总距离一对一匹配） | `src/io/pid_legend/pairing.rs` | 存量图纸导入的位号绑定 | 须输出次优解与总代价，供置信度分级判定 |
| 统一导出 schema（JSON + 双表 CSV） | `src/io/pid_legend/export.rs` | JSON 与 CSV 交换出口 | 须增加 `schemaVersion` 与位号、类型、属性集字段 |
| 图例规则集 | `assets/pid-legend.json` | 图例库种子数据 | 15 条 block 规则、6 条 circle 规则、11 个 tag_class、50 条 `shapes.dictionary` 须按项目图例标准分套 |
| 管道连通性推断（笔画归并为 run、端口识别、三通直通判定、线号族归并） | `src/io/pid_pipes.rs` | L3 拓扑建模基础 | 拓扑结果须持久化，并支持页间连接符跨图合并 |
| 视图过滤（图纸图层 × 导入角色，存 XRecord） | `src/io/pid_view_filter.rs` | L2 显示规则 | 须与三级授权链的图纸状态联动 |
| P&ID 命令面（PIDLEGEND、PIDLINE、PIDTAG、PIDGROUP、PIDPLOT） | `src/app/commands/mod.rs` | L2 编辑器命令骨架 | 须注册 P&ID 语义命令并接入撤销快照 |
| MCP 操作清单（30 读 + 43 执行） | `src/mcp.rs` | 自动化接口的操作面 | 须补位号主数据与规则检查相关操作 |
| 行式 JSON 通道 | `src/app/automation.rs` | `--serve` 脚本与 CI 通道 | 操作集合窄于 MCP，须补齐导入导出所需操作 |
| REST 通道 | `src/rest.rs` | `--http` 通道 | 须增加版本化路径与批量导入端点 |
| 插件 API（带外进程 + 本地 socket IPC） | `crates/ocs_plugin_api` | 插件扩展点 | `api_version` 当前为 13，须新增 P&ID 语义扩展位 |
| 插件清单样板 | `docs/plugin-template-v2/plugin.toml` | 插件扩展的清单模板 | 须增加 P&ID 类型与规则包声明段 |
| 外部格式槽（`Format` trait 与 `EXTERNAL_FORMATS` 注册表） | `crates/ocs_save/src/lib.rs` | Excel 与 JSON 写出插槽 | 注册表当前为空，须注册 XLSX 与 JSON 写出器 |
| SVG 矢量出图 | `src/io/svg_export` | 图纸导出 SVG | 须支持位号图层与图层映射 |
| PDF 出图 | `src/io/pdf_export` | 图纸与报表导出 PDF | 须支持图幅参数与矢量文字子集 |
| 图纸编辑租约与外部变更指纹 | `src/io/edit_lock.rs` | 图纸状态与编辑会话一级的实现细节 | 文件租约须升级为会话心跳与位号级占用 |

## 改造点说明

**识别结果语义化（第 1、2 行）**：把块字典、图层前缀、形状哈希与位号配对的中间结果升级为挂接类型与属性集的 P&ID 对象候选，并让配对器输出次优解与总代价；改的原因是 `REQ-MIG-001` 的置信度分级需要识别路径与配对代价作为输入，改完之后置信度三档才可判定、`REQ-MIG-002` 的人工确认队列才有候选来源。

**交换格式版本化（第 3、13 行）**：统一导出 schema 增加 `schemaVersion` 顶层字段，`EXTERNAL_FORMATS` 注册表注册 XLSX 与 JSON 写出器；改的原因是 `REQ-IO-005` 的 `SCHEMA_VERSION_UNSUPPORTED` 行为依赖版本字段存在，`REQ-IO-001` 矩阵中 Excel（XLSX）与 JSON 列的落地依赖注册表挂接写出器。

**自动化通道补齐（第 8、9、10 行）**：MCP 操作清单补位号主数据与规则检查操作，`--serve` 行协议与 `--http` REST 同步补齐导入导出端点；改的原因是 `REQ-MIG-005` 的三通道契约要求导入导出可经通道执行，改完之后 `REQ-IO-002` 与 `REQ-IO-003` 才可被脚本与 CI 调用。

**插件扩展位（第 11、12 行）**：`api_version` 当前为 13，须新增 P&ID 语义扩展位，清单样板增加 P&ID 类型与规则包声明段；改的原因是 `REQ-MIG-006` 的版本拒绝行为与 XDATA 持久化都落在插件扩展点上，扩展位不到位则 P&ID 插件无法声明域数据。

**输出与锁的接线（第 4、5、6、7、14、15、16 行）**：图例规则集按项目图例标准分套以支撑 `REQ-IO-006` 的符号库导入校验；管道连通性结果持久化并支持页间连接符跨图合并，对应 `REQ-DRW-023` 的跨图跳转数据面；视图过滤接入三级授权链的图纸状态，对应 `REQ-COLL-004` 与 `REQ-COLL-007` 的只读降级；命令面注册语义命令并接入撤销快照，承接 DRW 区域编辑条目的命令落点；SVG 与 PDF 出图补位号图层与图幅参数，对应 `REQ-IO-003` 与 `REQ-DELV-006` 的导出入口；文件租约升级为会话心跳与位号级占用，对应 `REQ-COLL-010` 与 `REQ-COLL-006`，租约本身不构成协同协议。
