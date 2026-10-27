# 原型可编程测试面说明

本文档登记原型外壳对外暴露的可编程测试面（test surface）契约，供自动化校验脚本在
不依赖界面文案的前提下读取运行状态。本文件同时登记 `data-testid` 清单与 14 条
界面路由。

## 1. 总体约定

- `window.__proto` 是当前界面对外发布的状态对象，在任何界面渲染完成后均可读取。
- 该对象在**每次界面切换后整体重建并重新发布**，`Object.keys(window.__proto)`
  恒等于下表所列的 8 个字段，不多不少；其中 `screen` 字段始终等于当前路由
  界面 id。
- 外壳阶段所有集合类与计数字段均为空种子值：`symbolCount` 为 `0`，
  `selectedTag` 为 `""`，`selectedProps`、`violations`、`bomRows` 为 `[]`，
  `paletteItemCount` 为 `0`，`tagRulePreview` 为 `""`。后续各界面实现通过
  `window.__protoSet(partial)` 合并并填充这些字段。
- `window.__protoSet` 是独立全局函数，不作为 `__proto` 的键存在；它只合并上表
  所列的字段（`screen` 由路由统一管理，partial 中的 `screen` 会被忽略），合并后
  不触发界面重绘。
- `window.__protoErrors` 在任何其他脚本运行前初始化为 `[]`，并监听 `window`
  的 `error` 事件，把每个未捕获错误以字符串形式追加进数组。遍历全部 14 条
  路由后该数组应保持为空。

## 2. `window.__proto` 字段契约

| 字段 | 类型 | 含义 | 更新时机 |
| --- | --- | --- | --- |
| `screen` | string | 当前路由界面 id，如 `workbench` | 每次界面切换重建对象时由路由写入 |
| `symbolCount` | number | 图面画布上的符号实例数量 | 编辑器发布后更新；外壳阶段固定为 `0` |
| `selectedTag` | string | 当前选中对象的位号；无选中时为空串 | 图面选中状态变化时 |
| `selectedProps` | `Array<{ name: string, value: string, required: boolean }>` | 选中对象的属性列表：属性名、取值、是否必选 | 图面选中状态变化时由属性面板发布 |
| `violations` | `Array<{ ruleId: string, severity: string, tag: string, sheet: string, message: string, suggestion: string }>` | 设计规则检查结果集合：规则 id、严重级别、位号、所在图纸、问题描述、修改建议 | 设计规则检查执行并发布结果时 |
| `bomRows` | `Array<{ no: string, type: string, tag: string, name: string, qty: string, unit: string, note: string }>` | 材料清单行集合：序号、对象类型、位号、名称、数量、单位、备注 | 材料清单统计发布时 |
| `paletteItemCount` | number | 符号调色板中可见符号条目的数量 | 调色板数据发布后 |
| `tagRulePreview` | string | 按当前位号规则生成的位号预览文本 | 位号规则编辑后 |
| `window.__protoErrors` | `string[]` | 页面级未捕获错误的收集数组 | `window` `error` 事件触发时追加 |

## 3. `data-testid` 清单

| data-testid | 所在区域 | 说明 |
| --- | --- | --- |
| `app-shell` | 根节点 | 包裹整个应用的壳容器 |
| `app-bar` | 顶栏 | 顶部应用栏，承载品牌、主导航、项目选择、搜索与用户区 |
| `main-nav` | 顶栏内部 | 主导航，**恰好 3 个链接**：工作台、图纸管理、P&ID 编辑器 |
| `main-nav-item` | `main-nav` 内每个链接 | 主导航链接，共 3 个，均带 `data-screen` |
| `module-nav` | 左侧栏 | 模块导航，按分组列出全部 14 个界面链接 |
| `module-nav-item` | `module-nav` 内每个链接 | 模块导航链接，共 14 个，均带 `data-screen` |
| `project-selector` | 顶栏 | 项目选择器，`<select>`，至少 3 个项目选项 |
| `search-entry` | 顶栏 | 搜索入口，`<input>`，带占位提示 |
| `user-area` | 顶栏 | 用户区，显示用户姓名与角色标签 |
| `screen-body` | 主内容区 | 按路由整体换出的界面主体容器 |
| `screen-title` | 界面主体内部 | 当前界面的 `<h1>`，每个渲染界面恰好一个 |
| `boot-status` | 底部状态栏 | 外壳引导状态文本，就绪后为「外壳就绪」 |

## 4. 路由登记表

| 序号 | 界面 id | 路由 | 界面标题 |
| --- | --- | --- | --- |
| 1 | `workbench` | `#/screen/workbench` | 工作台 |
| 2 | `drawings` | `#/screen/drawings` | 图纸管理 |
| 3 | `editor` | `#/screen/editor` | P&ID 编辑器 |
| 4 | `palette` | `#/screen/palette` | 符号调色板 |
| 5 | `properties` | `#/screen/properties` | 属性面板 |
| 6 | `typelib` | `#/screen/typelib` | 业务类型库 |
| 7 | `symbols` | `#/screen/symbols` | 标准图例定制 |
| 8 | `symbol-editor` | `#/screen/symbol-editor` | 符号编辑器 |
| 9 | `tag-rules` | `#/screen/tag-rules` | 位号规则 |
| 10 | `color-rules` | `#/screen/color-rules` | 颜色规则 |
| 11 | `masterdata` | `#/screen/masterdata` | 主数据管理 |
| 12 | `validation` | `#/screen/validation` | 设计规则检查 |
| 13 | `bom` | `#/screen/bom` | 材料清单 |
| 14 | `permissions` | `#/screen/permissions` | 权限管理 |

## 5. 路由与渲染规则

- 空 hash 或未知 hash 一律经 `location.replace` 重定向到 `#/screen/workbench`，
  不渲染错误页。
- 点击导航链接只改变 `location.hash`，界面主体随之换出，不发生整页加载。
- 刷新同一 URL 落在同一界面：hash 是路由的唯一事实来源，路由器分别在
  `DOMContentLoaded` 与 `hashchange` 时执行。
- 每个渲染界面在外壳内恰好包含一个 `<h1>`（即 `screen-title`）。
- 界面切换后 `window.__proto` 整体重建，`screen` 字段与当前路由 id 保持一致。

## 6. 界面级 `data-testid` 补充登记

以下 `data-testid` 随具体界面实现挂载，仅在对应界面渲染完成后出现。符号编辑器两张表的
枚举取值与类型库「级别」列的取值都直接抄自 owning 需求文档的 `Enum(...)`，改代码前先改需求。

| data-testid | 所在界面 | 所在区域 | 说明 |
| --- | --- | --- | --- |
| `overview-stats` | `workbench` | 概览指标条 | 含且仅含 4 个指标项：项目名、图纸数、对象数、待处理问题数；取值由种子数据计算得出 |
| `recent-drawings` | `workbench` | 最近图纸列表 | `<ul>` 列表，每行含文件名、生命周期状态、版本、修改人、修改时间，至少 3 行 |
| `activity` | `workbench` | 最近动态列表 | `<ul>` 列表，每行含时间、操作人、事件描述，至少 3 行 |
| `quick-actions` | `workbench` | 快捷入口区 | 至少 2 个入口链接，分别将 `location.hash` 置为 `#/screen/drawings` 与 `#/screen/editor` |
| `pbs-tree` | `drawings` | 左侧 PBS 树区 | 至少 3 级嵌套 `<ul>/<li>`：工厂 → 装置 → 单元；任一时刻恰有一个节点带 `.is-selected` |
| `pbs-node` | `drawings` | PBS 树内每个节点 | 可点击节点，携带 `data-pbs-id` 与 `data-level` |
| `drawing-list` | `drawings` | 图纸清单表格 | `<table>`，表头恰为 文件名、状态、版本、修改人、修改时间 五列；数据行携带 `data-drawing-id` 与 `data-pbs-id` |
| `drawing-preview` | `drawings` | 图纸预览区 | 显示当前选中图纸的名称、版本、生命周期状态与内联 SVG 示意图 |
| `drawings-toolbar` | `drawings` | 顶部工具栏 | 恰含 6 个按钮，顺序固定：新建图纸、导入、复制、移动、权限、版本比对 |
| `attribute-level` | `typelib` | 属性表「级别」列 | 每行一个 `<select>`，选项恰为 `可选信息`、`必选信息`，`位号` 行默认 `必选信息`，其余行默认 `可选信息`；切换后 `tlState.levels` 记住该行取值 |
| `rule-hierarchy-select` | `tag-rules` | 规则编辑区 | 层级选择下拉，选项恰为 `当前类型依次递增`、`父类型递增`，默认 `当前类型依次递增`（REQ-SYM-007） |
| `rule-step` | `tag-rules` | 规则编辑区 | 增量输入框，默认 `1`，保存时写入 `window.__tagRuleConfig` 的 `step`（REQ-SYM-007） |
| `connection-point-table` | `symbol-editor` | 连接点表 | 列头恰为 类型、方向、坐标；类型取 `等径`/`异径`/`信号线`，方向取 `双向点`/`流入点`/`流出点`（REQ-SYM-005） |
| `symbol-attribute-table` | `symbol-editor` | 属性标签表 | 列头恰为 标记、提示、默认、位置；位置取 `上`/`下`/`左`/`右`（REQ-SYM-006） |

## 7. `window.__proto.symbolCount` 填充说明

- `drawings` 界面在每次渲染后（路由进入、PBS 节点选中变更、图纸行选中变更），将
  当前预览图纸的 `objectCount` 经 `window.__protoSet({ symbolCount: n })` 写入
  `window.__proto.symbolCount`。路由进入时该值经外壳的一次性合并机制写入；
  界面内交互重绘时由界面脚本直接调用 `window.__protoSet`。
- 其余 13 个界面当前不修改该字段，`symbolCount` 保持路由重建时的种子值 `0`。
