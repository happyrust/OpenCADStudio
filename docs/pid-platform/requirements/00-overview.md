# 需求体系总纲

本文档定义 P&ID 平台需求文档体系的编号规范、文档分工、追溯矩阵与机械校验规则。需求条目按区域拆分至 `docs/pid-platform/requirements/` 目录下的九份区域文档，本文档只引用需求 ID，不定义需求。

## 需求 ID 规范

需求 ID 格式为 `REQ-<AREA>-NNN`：`REQ` 为固定前缀；`AREA` 为区域代码，恰有九类取值；`NNN` 为三位数字序号，每个区域内从 `001` 起连续编号，不跳号、不复用已删除编号。ID 全局唯一，`REQ-PLAT-*` 只能定义在 `docs/pid-platform/requirements/10-platform.md`，`REQ-TYPE-*` 只能定义在 `docs/pid-platform/requirements/20-type-library.md`，其余区域同此规则。

九个区域及其覆盖范围：

| 区域 | 名称 | 覆盖范围 |
| --- | --- | --- |
| PLAT | 平台层 | 身份与会话、角色与成员、项目空间、权限模型、审计日志、系统配置 |
| TYPE | 类型库 | 业务类型树、属性分组、数据要素与数据类型、属性级别与默认值、属性拼接、类型与符号关系、规则继承 |
| SYM | 符号与项目设置 | 图例标准、符号库、符号编辑器、项目级绘图与规则设置 |
| DRW | 图纸与绘图 | 图纸管理、画布编辑、对象放置、管路走线与拓扑连接 |
| DATA | 数据与规则检查 | 对象数据、位号唯一性、设计规则检查与检查结果模型 |
| DELV | 交付与发布 | 材料清单、报表、图纸版本快照、发布与交付物输出 |
| COLL | 权限与协同 | 协同编辑、并发冲突处理、协作态权限细化 |
| IO | 导入导出 | 各数据格式的导入导出能力矩阵 |
| MIG | 迁移与互操作 | 历史数据迁移与外部系统互操作 |

全仓库用于识别需求 ID 的锚定正则（含否定环视）为：

`(?<![A-Za-z-])REQ-(PLAT|TYPE|SYM|DRW|DATA|DELV|COLL|IO|MIG)-\d{3}(?!\d)`

`VAL-REQ-001` 这类断言 ID 不是需求 ID，锚定正则的否定环视就是为了避免误匹配：`VAL-REQ-001` 中 `REQ` 之前紧邻连字符 `-`，被前置否定环视 `(?<![A-Za-z-])` 排除；后置否定环视 `(?!\d)` 则排除三位数字之后仍跟数字的误匹配。

## 区域与文件映射

每个区域恰好对应一份需求文档：

| 区域 | 文档 | 范围 |
| --- | --- | --- |
| PLAT | `docs/pid-platform/requirements/10-platform.md` | 平台层需求，共 15 条 |
| TYPE | `docs/pid-platform/requirements/20-type-library.md` | 类型库与属性模型需求，共 15 条 |
| SYM | `docs/pid-platform/requirements/30-symbol-library-and-project-settings.md` | 符号库与项目设置需求，共 18 条 |
| DRW | `docs/pid-platform/requirements/40-drawing-and-drafting.md` | 图纸与绘图需求 |
| DATA | `docs/pid-platform/requirements/50-data-and-rule-engine.md` | 数据与设计规则检查需求 |
| DELV | `docs/pid-platform/requirements/60-deliverables-and-publish.md` | 交付与发布需求 |
| COLL | `docs/pid-platform/requirements/70-collaboration-and-permissions.md` | 协同与权限需求 |
| IO | `docs/pid-platform/requirements/80-import-export-matrix.md` | 导入导出矩阵需求 |
| MIG | `docs/pid-platform/requirements/90-migration-and-interop.md` | 迁移与互操作需求 |

`docs/pid-platform/requirements/95-reuse-mapping.md` 不属于九个区域，只引用需求 ID、不定义需求，故不列入本表。

## 需求追溯矩阵

下表汇总本批次已定义的全部需求 ID 及其界面落点；屏幕列取原型屏幕 ID，无对应屏幕时填 `无界面` 并说明理由。

| 需求 ID | 区域 | 屏幕 ID 或 无界面 | 说明 |
| --- | --- | --- | --- |
| REQ-PLAT-001 | PLAT | 无界面 | 登录失败锁定由平台认证服务承载，14 屏原型不含登录屏 |
| REQ-PLAT-002 | PLAT | 无界面 | 密码有效期校验在认证侧执行，14 屏原型不含登录屏 |
| REQ-PLAT-003 | PLAT | 无界面 | 找回密码流程在认证侧完成，14 屏原型不含登录屏 |
| REQ-PLAT-004 | PLAT | 无界面 | 会话超时与并发控制在认证服务执行，14 屏原型不含会话屏 |
| REQ-PLAT-005 | PLAT | permissions | 预置角色在权限屏角色列表呈现并受删除保护 |
| REQ-PLAT-006 | PLAT | permissions | 自定义角色与成员继承在权限屏角色管理区维护 |
| REQ-PLAT-007 | PLAT | permissions | 角色删除保护与引用检查在权限屏操作 |
| REQ-PLAT-008 | PLAT | workbench | 项目空间创建入口与项目列表落在工作台屏 |
| REQ-PLAT-009 | PLAT | workbench | 项目空间状态展示与只读降级在工作台屏生效 |
| REQ-PLAT-010 | PLAT | permissions | 三级授权链的权限配置落在权限屏 |
| REQ-PLAT-011 | PLAT | permissions | 功能权限勾选与菜单可见性在权限屏配置 |
| REQ-PLAT-012 | PLAT | 无界面 | 审计日志在平台管理端呈现，14 屏原型不含审计屏 |
| REQ-PLAT-013 | PLAT | 无界面 | 日志保留与清理在平台管理端配置，14 屏原型不含审计屏 |
| REQ-PLAT-014 | PLAT | 无界面 | 运行参数在平台管理端配置，14 屏原型不含参数屏 |
| REQ-PLAT-015 | PLAT | 无界面 | 配置包发布在部署侧完成，14 屏原型不含发布屏 |
| REQ-TYPE-001 | TYPE | typelib | 类型树与类型编码编辑在类型库屏呈现 |
| REQ-TYPE-002 | TYPE | typelib | 类型删除与受影响节点提示在类型库屏操作 |
| REQ-TYPE-003 | TYPE | typelib | 属性分组创建与拖拽排序在类型库屏属性区完成 |
| REQ-TYPE-004 | TYPE | typelib | 属性定义移除操作在类型库屏属性表执行 |
| REQ-TYPE-005 | TYPE | typelib | 数据要素名称与标签维护在类型库屏完成 |
| REQ-TYPE-006 | TYPE | typelib | 数据定义的类型选择在类型库屏数据要素区配置 |
| REQ-TYPE-007 | TYPE | typelib | 使用情况查询与删除保护在类型库屏呈现 |
| REQ-TYPE-008 | TYPE | typelib | 属性级别下拉在类型库屏属性表编辑 |
| REQ-TYPE-009 | TYPE | typelib | 默认值上溯结果在类型库屏属性表与实例面板展示 |
| REQ-TYPE-010 | TYPE | typelib | 拼接表达式编辑与校验在类型库屏完成 |
| REQ-TYPE-011 | TYPE | typelib | 拼接求值与循环引用错误提示在类型库屏显示 |
| REQ-TYPE-012 | TYPE | symbols | 类型与符号映射及符号标签配置在符号屏呈现 |
| REQ-TYPE-013 | TYPE | typelib | 类型变更影响提示在类型库屏保存时给出 |
| REQ-TYPE-014 | TYPE | tag-rules | 位号规则继承来源与层级在位号规则屏预览 |
| REQ-TYPE-015 | TYPE | color-rules | 子类型覆盖标记与预览在颜色规则屏维护 |
| REQ-SYM-001 | SYM | symbols | 图例标准选定与不可切换规则在符号库屏呈现 |
| REQ-SYM-002 | SYM | symbols | 符号分类树与选项板分组同步在符号库屏维护 |
| REQ-SYM-003 | SYM | symbols | 符号属性七项在符号库屏属性对话框编辑 |
| REQ-SYM-004 | SYM | symbol-editor | 符号编辑器绘图与常用选项卡在符号编辑器屏呈现 |
| REQ-SYM-005 | SYM | symbol-editor | 连接点三类型与三方向组合在符号编辑器屏连接点表定义 |
| REQ-SYM-006 | SYM | symbol-editor | 符号标签四字段在符号编辑器屏属性标签表编辑 |
| REQ-SYM-007 | SYM | tag-rules | 位号规则八字段与格式拼接在位号规则屏定义 |
| REQ-SYM-008 | SYM | tag-rules | 自动编码、层级选择与后缀递增在位号规则屏配置 |
| REQ-SYM-009 | SYM | tag-rules | 位号唯一校验与自动弹窗开关在位号规则屏设置 |
| REQ-SYM-010 | SYM | tag-rules | 位号规则继承来源与导入导出在位号规则屏呈现 |
| REQ-SYM-011 | SYM | color-rules | 颜色规则条件表达式在颜色规则屏条件编辑区配置 |
| REQ-SYM-012 | SYM | color-rules | 颜色规则继承与启动规则开关在颜色规则屏维护 |
| REQ-SYM-013 | SYM | masterdata | 公称直径序列在项目主数据屏公称直径页签维护 |
| REQ-SYM-014 | SYM | masterdata | 管道等级在项目主数据屏管道等级页签维护 |
| REQ-SYM-015 | SYM | masterdata | 绝热等级在项目主数据屏绝热等级页签维护 |
| REQ-SYM-016 | SYM | masterdata | 流体介质在项目主数据屏流体介质页签维护 |
| REQ-SYM-017 | SYM | 无界面 | 相交断线权重属 P&ID 基础设置，14 屏原型未含基础设置屏 |
| REQ-SYM-018 | SYM | 无界面 | 打断符样式与高度属 P&ID 基础设置，14 屏原型未含基础设置屏 |

## 校验规则

需求文档接受以下四条机械校验：

1. **锚定正则**：全仓库需求 ID 必须匹配 `(?<![A-Za-z-])REQ-(PLAT|TYPE|SYM|DRW|DATA|DELV|COLL|IO|MIG)-\d{3}(?!\d)`；正则带否定环视，避免把 `VAL-REQ-001` 误判为需求。
2. **唯一性**：同一 ID 在全仓库只允许定义一次；重复定义报错并指出两处位置。
3. **每区下限**：九个区域中每一个已存在的区域文档至少定义 5 条需求。
4. **区域与文件一一对应**：`REQ-<AREA>-*` 只能出现在该区域对应的文件中；`00-overview.md` 与 `95-reuse-mapping.md` 只引用、不定义。

需求追溯矩阵与各区域文档之间要求双向闭合：矩阵列出的 ID 集合必须与各区域文档定义的 ID 集合完全相等，无遗漏、无孤儿；新增或删除需求时矩阵须同步更新。
