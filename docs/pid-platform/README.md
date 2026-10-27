# P&ID 平台规格包

本目录是 OpenCADStudio 智能 P&ID 平台的规格包，集中登记产品需求、术语基线、架构设计、需求条目与原型屏幕等全部交付物。本文档是该规格包的唯一入口，面向评审者、产品与项目经理、架构师、需求与测试工程师说明交付物位置、推荐阅读顺序与原型的打开方式。

## 交付物索引

下表列出本规格包全部已交付文档，路径为仓库相对路径。

| 文档 | 路径 | 内容概要 |
| --- | --- | --- |
| PRD | `docs/pid-platform/PRD.md` | 产品定位、目标行业与客户、目标用户与角色、应用场景、功能架构、十三个功能模块清单、路线图与成功指标 |
| 术语表 | `docs/pid-platform/glossary.md` | 全部领域术语的中文名、英文名、定义、禁用别名与单一词源声明，是全部文档与原型用词的唯一权威 |
| 差距分析 | `docs/pid-platform/gap-analysis.md` | 逐项比对竞品手册能力与本项目现有实现，给出必建、改造、复用三类结论与复用锚点 |
| 需求来源索引 | `docs/pid-platform/reference/source-index.md` | 竞品手册章节与需求区域的映射表，登记每条需求的候选来源章节与读表规则 |
| 竞品能力摘要 | `docs/pid-platform/reference/reference-product-summary.md` | 五份竞品手册的能力归纳，按九个功能域整理其界面构成与操作流程 |
| 系统架构 | `docs/pid-platform/architecture/system-architecture.md` | 五层架构的职责划分、上游消费者、向下依赖与层间契约表，以及脱机与在线两种运行模式 |
| 数据模型 | `docs/pid-platform/architecture/data-model.md` | 对象、属性、拓扑、位号与图纸版本的实体关系与关键约束，含位号唯一性的实现口径 |
| 非功能需求 | `docs/pid-platform/architecture/non-functional.md` | 性能、容量、可用性、兼容性、可维护性与安全合规的量化指标与验收方式 |
| 需求总览 | `docs/pid-platform/requirements/00-overview.md` | 九个需求区域的代码与文件映射、需求编号规则、需求到屏幕的追溯矩阵与跨区域约束 |
| 平台层需求 | `docs/pid-platform/requirements/10-platform.md` | 用户、角色、项目空间、三级授权链、审计日志与系统配置的需求定义与验收标准 |
| 类型库需求 | `docs/pid-platform/requirements/20-type-library.md` | 类型树、属性分组、数据要素、必填与默认值、拼接表达式与规则继承的需求定义 |
| 符号库与项目设置需求 | `docs/pid-platform/requirements/30-symbol-library-and-project-settings.md` | 图例标准、符号分类、连接点、位号规则、颜色规则与项目主数据的需求定义 |
| 图纸与绘图需求 | `docs/pid-platform/requirements/40-drawing-and-drafting.md` | PBS 层级、图纸生命周期与版本、绘图命令、管线管路管段拓扑、标注与断线的需求定义 |
| 数据与规则引擎需求 | `docs/pid-platform/requirements/50-data-and-rule-engine.md` | 位号主数据、对象属性、检索、设计规则检查引擎与违规处置的需求定义 |
| 交付物与发布需求 | `docs/pid-platform/requirements/60-deliverables-and-publish.md` | 材料清单、报表模板、图纸输出、数据发布与导出矩阵的需求定义 |
| 协同与权限需求 | `docs/pid-platform/requirements/70-collaboration-and-permissions.md` | 三级授权链、图纸状态、并发编辑、冲突处置与权限矩阵的需求定义 |
| 导入导出矩阵需求 | `docs/pid-platform/requirements/80-import-export-matrix.md` | 支持格式的导入导出能力矩阵，逐格式登记支持程度与限制条件 |
| 迁移与互操作需求 | `docs/pid-platform/requirements/90-migration-and-interop.md` | 从既有图纸与既有平台迁移的路径、字段映射与互操作接口的需求定义 |
| 复用映射 | `docs/pid-platform/requirements/95-reuse-mapping.md` | 每条需求与现有代码锚点的对应关系，标明复用、改造或新建 |

## 阅读路径

以下四条路线按角色划分，每条为有序列表，排首的条目即该角色的先读文档。

### 产品与项目经理

1. `docs/pid-platform/PRD.md` — 先读产品定位、功能模块清单与路线图，建立范围与阶段口径。
2. `docs/pid-platform/requirements/00-overview.md` — 再读九个需求区域的划分与追溯矩阵，掌握需求面全貌。
3. `docs/pid-platform/gap-analysis.md` — 对照现有能力与目标平台的差距，评估复用、改造与新建的分布。
4. `docs/pid-platform/architecture/non-functional.md` — 末读量化指标，核对成功指标的验收口径。

### 架构师

1. `docs/pid-platform/architecture/system-architecture.md` — 先读五层职责、上下游消费者与依赖关系及跨层契约，判断边界划分。
2. `docs/pid-platform/architecture/data-model.md` — 再读领域对象分类、属性传递链与位号唯一性口径。
3. `docs/pid-platform/gap-analysis.md` — 核对每项能力的复用、改造、新增处置及其证据路径。
4. `docs/pid-platform/requirements/95-reuse-mapping.md` — 把处置结论逐条落到仓库代码锚点。
5. `docs/pid-platform/architecture/non-functional.md` — 末读性能与容量目标，校验架构决策与之匹配。

### 需求与测试工程师

1. `docs/pid-platform/requirements/00-overview.md` — 先读需求编号规则、区域映射与追溯矩阵，定位条目归属。
2. `docs/pid-platform/requirements/40-drawing-and-drafting.md` — 再读绘图主链路需求，这是屏幕覆盖最密的区域。
3. `docs/pid-platform/requirements/50-data-and-rule-engine.md` — 读检查引擎与违规处置，对应校验结果屏的验收。
4. `docs/pid-platform/requirements/60-deliverables-and-publish.md` — 读材料清单聚合与发布门禁，对应导出验收。
5. `docs/pid-platform/requirements/80-import-export-matrix.md` — 末读格式能力矩阵，对应导入导出用例的边界。

### 评审与合规

1. `docs/pid-platform/glossary.md` — 先读术语表与禁用别名列，建立用词裁决基线。
2. `docs/pid-platform/reference/source-index.md` — 再读候选来源章节与读表规则，确认需求来源可追溯。
3. `docs/pid-platform/reference/reference-product-summary.md` — 对照竞品能力基线，核对本规格包的取舍依据。
4. `docs/pid-platform/requirements/95-reuse-mapping.md` — 末读需求到代码锚点的映射，抽查复用结论的落点。

## 原型运行方法

原型为纯静态页面，提供以下两种打开方式。

### 直接打开

用浏览器直接打开 `docs/pid-platform/prototype/index.html`（`file://` 协议），适合快速浏览界面结构与导航，无需任何运行时依赖。该方式的限制：部分浏览器在 `file://` 协议下会限制脚本或本地资源加载，界面状态对象的读取行为与 HTTP 访问可能不一致，因此不能作为自动化校验的取证来源。

### 本地静态服务

在仓库根目录执行以下命令：

```
python -m http.server 3110 --bind 127.0.0.1 --directory docs/pid-platform/prototype
```

随后访问 `http://127.0.0.1:3110/index.html`。HTTP 协议下脚本、样式与路由行为与正式部署一致，自动化校验脚本可稳定读取界面发布的状态对象，因此这是推荐的取证方式。

### 路由与屏幕

原型采用 hash 路由，地址形如 `#/screen/<屏幕id>`；空 hash 或未知 hash 一律重定向到工作台屏幕。14 个屏幕的 id、界面名称与首条对应需求如下表所示。

| 屏幕 id | 界面名称 | 对应需求 |
| --- | --- | --- |
| `workbench` | 工作台 | REQ-PLAT-008 |
| `drawings` | 图纸管理 | REQ-DRW-001 |
| `editor` | P&ID 编辑器 | REQ-DRW-011 |
| `palette` | 符号调色板 | REQ-SYM-001 |
| `properties` | 属性面板 | REQ-DATA-001 |
| `typelib` | 业务类型库 | REQ-TYPE-001 |
| `symbols` | 标准图例定制 | REQ-TYPE-012 |
| `symbol-editor` | 符号编辑器 | REQ-SYM-004 |
| `tag-rules` | 位号规则 | REQ-TYPE-014 |
| `color-rules` | 颜色规则 | REQ-TYPE-015 |
| `masterdata` | 主数据管理 | REQ-SYM-013 |
| `validation` | 设计规则检查 | REQ-DATA-003 |
| `bom` | 材料清单 | REQ-DELV-001 |
| `permissions` | 权限管理 | REQ-PLAT-005 |

### 可编程测试面

`window.__proto` 是界面对外发布的状态对象，在每次界面切换后整体重建，其字段契约见 `docs/pid-platform/prototype/specs/test-surface.md`。
