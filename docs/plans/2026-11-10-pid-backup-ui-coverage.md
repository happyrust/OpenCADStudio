# P&ID 备份包：数据清点与界面覆盖方案

> 日期：2026-11-10
> 范围：SmartPlant P&ID 备份包里到底有哪些数据、现在在界面上各自看得到看不到、看不到的原因，以及让「各种数据都能通过 UI 查看到」的做法。本文只定方案，不含界面实现。
> 基于：分支 `upstream-merge-20261110`（上游合并 `862ddb1e1`，其上两次提交 `a13e85732`、`925728489`）；`pid-parse` / `pid-plot` 钉在 `d0a58c64c413`（pid-parse 0.11.7，默认特性 `backup` = `backup-exp` + `backup-mdf`）。
> 承接：`docs/plans/2026-10-16-pid-backup-package.md`。它的 D1（SQL Server 包暂不支持）、D2（不做完整工厂树）、D5（摘除 GPL 依赖）在这次的要求下需要重新表态，见 §4。
> 证据：除标注「推断」「未验证」的句子外，本文的数字和现象都是在本分支上亲自跑出来、看到的；复现办法在 §5。

## 1. 现状

### 1.1 加载路径

```
PIDBACKUPOPEN <目录|zip>
  └ io::pid_backup::restore            解 PlantData~*.zip、拷贝包旁已解压的 Ref\ → <TEMP>\ocs-pid-backups\<hash>
  └ io::pid_backup_browse::build_index scan_backup_folder + read_backup_drawings + read_plant_groups
  └ ui::window::pid_backup_browser     树（备份文件 / 装置组 / 参考数据）+ 详情键值表，纯函数 update / visible_rows / detail_rows
  └ app::commands::pidbackup           on_pid_backup 落地 Effect；展开图纸 → load_publish_from_pid 读图元；双击 → 打开为标签页
```

整条链只占用 `Message::PidBackup(PidBackupMsg)` 一个消息和 `pid_backup_browser` 一个状态字段。

### 1.2 实测

三个包都走了两遍。先是经应用的无窗口走查（`app::commands::pidbackup::audit`）：每个节点取详情、每张图读图元、每张图过导入器，下表的数字出自它。再是真实窗口（Vulkan 后端，点击是投递到编辑器自己窗口的鼠标消息）：每一种节点各点开看过——根、各种备份文件、归档成员、装置组、图纸、图元类型、图元、读不出的图纸、参考目录与文件、搜索框、刷新与关闭、双击开图——但没有把每个节点都点一遍。

| 包 | 载入 | 备份文件 | 图纸 | 参考文件 | 图元 / 属性 | 图元读不出 | 导入不了 |
|---|---|---:|---:|---:|---|---|---|
| 全厂包 `D:\work\plant-code\cad\pid-test-data` | 首次约 0.8 s（含还原 130 MB），缓存后 0.12 s；真实窗口发命令到出树 0.21–0.24 s | 15（1 个登记但缺失） | 53 | 645 | 14 894 / 15 921（8 056 个图元没有属性） | `D03`、`D04` | `D03` |
| `DWG-0202GP06-01_p` | 0.07 s | 15 | 7 | **0** | 152 / 70 | `001` | `001` |
| `TEST02_p`（SQL Server MTF） | 0.05 s | 14 | 12（按文件夹列出） | **0** | 3 / 6 | 11 张模板图 | `CPECCHBA2-new`、`CPECCHBA2` |

全厂包 53 张图逐张读图元共 1.3 s；逐张导入共 36.5 s（最慢 `P井场` 6.2 s），149 310 个实体，9 条源记录没画出来。读不出的原因都是 pid-parse 给的 `no P&IDAttributes row carries a DrawingNo`，导入不了的是 `No decoded geometry`。

### 1.3 发现的问题

**已修（本分支，带测试）**

- 图纸行没有展开箭头，而箭头是读图元的唯一入口：真实面板里一张图的图元和属性根本点不开。基线 `2e9e9dab2` 的真实窗口同样如此（`target\ui-audit\gui-plant-baseline`）。原有单测直接发 `Toggle` 消息，绕过了画出来的行。修复 `a13e85732`：未读、在读的图纸带箭头；读取失败把原因写进详情区和命令行。

**未修，属于第二阶段的界面问题**（真实窗口截图为证）

1. 详情键值表的键列固定 96 px，长键不换行，值直接压在键上：`INamedInstrument.InstrTagSequenceNo` 与它的值重叠，`DOCUMENTCATEGORY` 也被值压住。
2. 树里看不出当前选中的是哪一行。
3. 图元行的标签是 32 位 UID 加类名，折成两行还被右边界截断；位号（`IObject.Name`，如 `MM-222`）只在详情里。
4. 归档成员是带目录项的平铺长列表（`RefData~4~681.zip` 742 行），没有大小，也看不了内容。
5. 备份文件只显示存储名：`RefData~4~680` 是 `Ref\rules.rul`、`~683` 是 `ProjectStyles.spp`、`~709` 是 `InsulationSpec.isl`、`PlantData~2~722` 是 `SPEMDataMap.xml`，界面上看不出来；`~703` 是 `exportlayer.xlsx`，却显示成「Zip，12 个成员」。
6. 「参考数据」只认包旁已解压的 `Ref\` 目录。原样的备份包（两个小包）这一支是空的；导入时也就用不上包里的符号库（对画出来的图有多大影响没有量）。
7. 关闭备份后面板里「尚未打开备份。」出现两遍；组详情的路径用 ` / `，图纸详情用 ` \ `；同组图纸按库里的行序排，不按名字。
8. 读不出、打不开的原因是 pid-parse 的英文原句，带整条缓存路径。
9. 还原缓存固定在 `%TEMP%\ocs-pid-backups`（C 盘），全厂包 130 MB，从不清理。

**与备份无关但会挡住验收**

- **DirectX 12 下模型视口不出图。** 本机自动探测选中 dx12（`[gpu] backend: dx12 (AMD Radeon RX590 GME)`），此时任何图纸（`.pid`、`.dxf`）的视口都是空的：没有实体，没有栅格，没有 ViewCube；屏幕上和 `/capture` 里一样。`--backend vulkan` 下正常。基线与合并后表现相同（`target\ui-audit\gui-backends`，视口非背景像素占比：dx12 0.28% / 0.28%，vulkan 1.51% / 3.48%，两个版本逐项相同），所以不是合并带来的。原因没有查。
- 经自动化通道执行 `PIDBACKUPOPEN` 之后，当前图纸带上修改标记（标题出现 ●），关窗口会弹「未保存的更改」，尽管这条命令不改图纸；`ZOOM E` 也一样，基线上 `PIDBACKUPOPEN` 同样。自动化通道和界面命令行走的是同一个 `run_command_line`（读代码所得），所以在命令行里敲应该也如此，这一点没有在窗口里试。

## 2. 数据清点

以全厂包为准：`Manifest.txt` 322 行、`Export.dmp` 74 489 856 B（Oracle exp V12.01.00，4 个库 154 张表 365 530 行）、`PlantData~2~711.zip`（53 张 `.pid`）、11 个 `RefData~4~*` 卷、`PlantConfig.xml`、`Export.log`。

「界面」一列：✅ 看得到；◐ 看得到一部分；❌ 看不到。「原因」一列只有三种：**无接口**（钉版 pid-parse 没有公开读法）、**未接**（有接口，界面没用）、**已接有缺**（接了，显示不全或不对）。

| # | 数据类 | 包里有什么 | 钉版 pid-parse 接口 | 界面 | 在哪里 / 原因 |
|---:|---|---|---|:-:|---|
| 1 | Manifest 概况 | 22 个单值键（Name、Version、BackupType、DateCreated、Spid、OracleVersion、Characterset、BackupCommand、ProjectType、Rootitem…） | `backup::parse_manifest_bytes` → `Manifest { lines }`、`first_field` | ◐ | 根节点详情显示 5 个；其余 17 个**未接** |
| 2 | Manifest 数据库对象清单 | Table 154（SQPlant 22 / SQPlantd 25 / SQPlantpid 82 / SQPlantpidd 25）、View 35、Right 78、Role 1 | `Manifest::tables()`、`views()`、`all(key)` | ❌ | **未接** |
| 3 | Manifest 文件登记 | File 14 行（库号、目录/文件、编号、原始 UNC 路径、状态、存储名）+ FileSize 11 行（文件数、目录数、字节数） | `Manifest::all("File")`（通用行） | ◐ | 只用来标出「登记但缺失」；原始路径、文件数**未接** |
| 4 | Manifest 连接与账号 | PlantConnInfo 4、SiteConnInfo 2、TableSpace 2、DBUids、DBPwds（混淆过的口令） | 通用行 | ❌ | **未接**。接的时候口令类字段必须遮住，见 D6 |
| 5 | 备份卷清单 | `Export.dmp` + 2 个 PlantData 卷 + 11 个 RefData 卷：格式、大小、成员数 | `backup::scan_backup_folder` | ✅ | 「备份文件」节点及各文件详情 |
| 6 | 归档成员 | 8 个归档的中央目录，共 861 项（681：742，682：20，684：10，685：3，703：12，804：10，809：4，711：60） | `backup::list_zip_entries`（`ZipListing`） | ✅ | 展开备份文件；平铺、无大小 |
| 7 | 包根目录其他文件 | `PlantConfig.xml`（ISO 15926 线型映射、伴热介质）、`Export.log`（导出日志，含每表行数） | 无（纯文本 / XML） | ❌ | `scan_backup_folder` 不列它们；**无接口**，也不需要：OCS 自己读得了 |
| 8 | 转储头 | 格式、版本行 `EXPORT:V12.01.00` | `ExportDumpScan` | ✅ | `Export.dmp` 详情 |
| 9 | 数据库表目录与行数 | 4 库 154 表 365 530 行（§5.1） | 表名：`Manifest::tables()`；行数：只在 `Export.log` 里 | ❌ | **未接**（表名）/ **无接口**（行数，OCS 自己解析日志即可） |
| 10 | 图纸登记 `T_DRAWING` | 53 行，面板取 20 列；全厂包里 10 列有值、10 列全空 | `oracle_exp::read_backup_drawings` | ✅ | 图纸详情 |
| 11 | 装置层级 `T_PLANTGROUP` | 6 行 | `read_plant_groups`、`plant_group_path` | ✅ | 「装置组」树与组详情 |
| 12 | 工厂与项目 | SQPLANT 库：`T_PLANT`、`T_PROJECT`、`T_UNIT` 3、`T_AREA` 2、`T_PLANTSETTING` 14、`T_ROLEACCESS` 12、`T_HIERARCHYLEVELS` 3、`T_DRAWINGSITE` 53… | 无 | ❌ | **无接口** |
| 13 | 格式与单位 | `T_FORMATS` 385、`T_FORMATTYPES` 41 | 无 | ❌ | **无接口** |
| 14 | 图元主表与分类表 | `T_MODELITEM` 47 124、`T_PLANTITEM` 13 687、`T_PIPERUN` 4 686、`T_INLINECOMP` 4 533、`T_INSTRUMENT` 3 718、`T_INSTRFUNCTION` 3 718、`T_PIPINGCOMP` 3 503、`T_SIGNALRUN` 1 001、`T_EQUIPMENT` 402、`T_NOZZLE` 377、`T_EQUIPMENTOTHER` 225、`T_MECHANICAL` 94、`T_VESSEL` 71、`T_EXCHANGER` 12、`T_CASE` 209、`T_CASEPROCESS` 301 | `ExpTables::pipe_runs()`（`T_PIPERUN` 的公称直径、介质代码、材料等级、顺序号、流体系统、保温规范）；`T_MODELITEM`、`T_NOZZLE`、`T_SIGNALRUN` 只在内部读 | ❌ | `pipe_runs` **未接**，其余**无接口**。面板里的图元来自 `.pid` 文件本身（#26），不是库 |
| 15 | 关系与连接 | `T_RELATIONSHIP` 24 664、`T_SIGNALPOINT` 22 057、`T_CONNECTOR` 11 757、`T_PIPINGPOINT` 9 628、`T_CONNECTORVERTEX` 5 181 | `ExpTables::port_tables()`、`connectivity_ends(图纸)`、`composition_relations()` | ❌ | **未接**；`T_RELATIONSHIP`、`T_CONNECTORVERTEX` **无接口** |
| 16 | 图形表示与标注 | `T_REPRESENTATION` 42 540、`T_SYMBOL` 18 199、`T_LABELPERSIST` 12 584、`T_LEADERVERTEX` 12 584、`T_SMARTFRAME` 12 | `T_REPRESENTATION` 只在内部读 | ❌ | **无接口** |
| 17 | 跨图连接 OPC | `T_OPC` 952 | `ExpTables::opc_points(图纸)` | ❌ | **未接** |
| 18 | 注释 | `T_NOTE` 800、`T_ITEMNOTE` 800 | 只在内部读 | ❌ | **无接口**（库侧）；图内的 ItemNote 图元经 #26 看得到 |
| 19 | 一致性与规则状态 | `T_RULEREFERENCE` 28 460、`T_PROPERTYBREAK` 14 963、`T_INCONSISTENCY` 4 624 | 无 | ❌ | **无接口** |
| 20 | 历史与版本 | `T_HISTORY` 30 931、`T_GLOBALDRAWING` 53、`T_DRAWINGPROJECT` 53、`T_DRAWINGVERSION` 15 | 无 | ❌ | **无接口** |
| 21 | 选项设置 | `T_OPTIONSYMBOLOGY` 77、`T_OPTIONSETTING` 66、`T_OPTIONAUTOGAP` 44、`T_OPTIONFORMAT` 39、`T_OPTIONHEATTRACE` 22、`T_OPTIONDISPSET` 11 | 无 | ❌ | **无接口** |
| 22 | 数据字典 | SQPLANTPIDD / SQPLANTD 两库各 25 表：`ITEMATTRIBUTIONS` 29 824 / 354、`CODELISTS` 3 407 / 130、`UNIQUEATTS` 1 527 / 131、`ATTRIBUTES` 1 004 / 85、`ENUMERATIONS` 133 / 13、`ENTITIES` 80 / 20、`FILTER*`… | `ExpTables::codelist_short_text(表号)`、`codelist_row_count()` | ❌ | 代码表**未接**，其余**无接口** |
| 23 | SQL Server 转储里的库 | `TEST02_p` 的 `Export.dmp`（MTF，19.1 MB） | `publish::mdf_load::open_mtf_dump_as_sqlite`、`sqlite_load::*`（`backup-mdf`，现在是链进来的） | ❌ | **未接**：面板提示「不读取其数据库」，图纸按文件夹列出，只有 PATH、FILE 两项 |
| 24 | 图纸文件 | `PlantData~2~711.zip` 内 53 张 `.pid`（`00\00` 43、`00\02` 4、`test\U01` 6） | 还原后按路径与 `T_DRAWING` 配对 | ✅ | 装置组下的图纸行 |
| 25 | 图纸图形 | 每张图的图形 | OCS `io::pid::load_pid` | ✅ | 双击图纸或「打开图纸」→ 新标签页（Vulkan 下看到了图；DX12 见 §1.3） |
| 26 | 图内图元与属性 | `.pid` 自带的发布数据：11 种类型，14 894 个图元 | `pid_load::load_publish_from_pid` → `PidPublishDrawing.items` | ✅ | 图纸 → 类型 → 图元 → 属性（修复之后）。**已接有缺**：标签是 UID，键值重叠 |
| 27 | 图内连接、端口、管线、OPC、图形表示、统计 | `PidPublishDrawing` 的 `process_points`、`branch_points`、`ports`、`connectivity_ends`、`exp_relations`、`opc_points`、`pipelines`、`representations`、`stats` | 同上；配合库还有 `attach_oracle_ports`、`pipeline` | ❌ | **未接**：读图元时这些字段一并读出来了，随后丢掉 |
| 28 | 解析诊断 | 读不出的原因、导入摘要（画出 / 解码 / 未画出 / 图层）、未解码记录 | `PidLoadError`、OCS `ImportSummary`、`pid_parse::inspect`、`byte_audit` | ◐ | 读不出的原因在图纸详情和命令行（修复之后）；导入摘要只在打开之后的命令行里；`inspect` **未接** |
| 29 | `SPEMDataMap.xml` | `PlantData~2~722`，711.zip 里还有一份 | 无专用接口 | ◐ | 只是一个文件行，内容看不了 |
| 30 | 符号库 | `RefData~4~681.zip`：644 个 `.sym` + `CatalogIndex.xml` | `symbol_library::read_symbol_geometry(路径)`；`CatalogIndex.xml` 的解析钉版没有 | ◐ | 包旁有解压好的 `Ref\` 时「参考数据」里是文件树（名字、大小）；原样的包是空的；没有预览。**已接有缺** |
| 31 | 图框模板 | `RefData~4~682.zip`：10 个 `.igr` + 10 个 `.pid` | `.igr` 能否用 `read_symbol_geometry` 读，未验证 | ◐ | 只有成员名 |
| 32 | 规则文件 | `RefData~4~680` = `rules.rul`（332 KB 文本） | 无 | ◐ | 一个不知其名的文件行，只有大小 |
| 33 | 样式库 | `RefData~4~683` = `ProjectStyles.spp`（OLE 容器） | 无专用接口：`style_link` 解的是图纸自带的样式表，并记下它来自哪个 `.spp`；`style_names_for_file` 能否直接读 `.spp` 未验证 | ◐ | 同上 |
| 34 | 报表模板 | `RefData~4~684.zip`：10 个 `.xlsm` | 无 | ◐ | 成员名 |
| 35 | 装配 | `RefData~4~685.zip`：3 个空目录 | — | ✅ | 成员名（本来就没有内容） |
| 36 | 图层导出表 | `RefData~4~703` = `exportlayer.xlsx` | 无 | ◐ | **已接有缺**：显示成「Zip，12 个成员」 |
| 37 | 保温规范 | `RefData~4~709` = `InsulationSpec.isl`（XML） | 无 | ◐ | 文件行，只有大小 |
| 38 | 数据映射 | `RefData~4~804.zip`：`SPPIDDataMap.xml` 及 7 个变体、`CatalogIndex.mdb` | `backup::read_datamap(路径)` → `DataMap { enum_to_enum, enum_list_names }` | ◐ | 成员名；映射内容**未接** |
| 39 | 导入映射 | `RefData~4~809.zip`：4 个 XML | 无 | ◐ | 成员名 |
| 40 | 登记但缺失的文件 | `RefData~4~824` = `JacketNDCoreND.XML`，备份时就没取到 | Manifest File 行的状态 | ✅ | 文件行标「登记但缺失」，根节点有警告 |
| 41 | 索引警告 | 缺文件、SQL Server 库未读、无法配对的图纸 | `ScanProblem` + OCS 自己的检查 | ✅ | 根节点详情的「警告」 |

合计 41 类：看得到 11 类（5、6、8、10、11、24、25、26、35、40、41），看得到一部分 13 类，看不到 17 类。看不到的 17 类里，12 类是数据库表（#9、#12–#22），另外 5 类是 Manifest 的对象清单与连接信息、根目录文件、SQL Server 库、图内连接数据。

## 3. 方案

### 3.1 做法

- **不加新面板、新窗口、新消息。** 一切都落在现有的「P&ID 备份」面板里：树上多几支，详情区多一种表格显示。停靠面板可以浮出、拖边角拉大（`src/ui/dock.rs` 的 `FloatOut`、`FloatResizeGrab`；读代码所得，没有在窗口里试），宽表靠这个。这样 `src/app/mod.rs`、`src/app/update/mod.rs`、`src/app/commands/mod.rs` 三个文件一行都不用动。
- **新代码放新文件。** `pid_backup_browser.rs` 已经 79 KB，`pid_backup_browse.rs` 1 664 行。新东西放进同名子目录：`src/ui/window/pid_backup_browser/*.rs`、`src/io/pid_backup_browse/*.rs`、`src/io/pid_backup/*.rs`、`src/app/commands/pidbackup/*.rs`（`audit.rs` 已经在这儿）。
- **先开扩展点（W0），再并行。** 现在每加一种节点都要改 `NodeId`、`visible_rows`、`detail_lines`、`update` 四处的 `match`，几个人同时加就会互相撞。W0 把「一支子树」收成一个接口，之后每个工作项只加一个文件和注册表里的一行。
- **重活在后台线程，结果缓存在面板状态里**，沿用读图元的做法（`load_items_in_background` + `ItemsState`）。
- **每个工作项都要让走查测试覆盖到。** `audit.rs` 现在断言「没有不带详情的节点、没有静默失败」；每加一类数据，走查就多走一支，报告里多一段。

### 3.2 工作项

规模：S 约 150 行以内，M 150–500 行，L 500 行以上（含测试）。

| # | 工作项 | 解决清点表的 | 依赖 | 规模 | 要动 pid-parse |
|---|---|---|---|:-:|:-:|
| W0 | 扩展点：子树接口与注册表 | —（给后面铺路） | — | M | 否 |
| W1 | 详情区：键值不重叠、选中高亮、表格显示、图元用位号做标签 | #26 的缺口；所有表格类数据的载体 | W0 | M | 否 |
| W2 | Manifest 全量 + 备份文件显示原名 | #1–#4、#32–#37 的名字、#36 的误标 | W0 | M | 否 |
| W3 | 根目录其他文件 + 文本预览 | #7、#29、#32、#37、#39 的内容 | W0 | S | 否 |
| W4 | 数据库表目录与行数 | #9 | W0 | S | 否 |
| W5 | 数据库：钉版已有的类型化视图 | #14（管线）、#15、#17、#22（代码表） | W1 | M–L | 否 |
| W6 | 图内数据补全 | #27 | W1 | M | 否 |
| W7 | 图元与图形联动 | #25、#26 之间 | W6 | M | 否 |
| W8 | 还原时把 RefData 卷放回 `Ref\` | #30、#31、#38 在原样的包里也有；导入用上包里的符号 | — | M | 否 |
| W9 | 符号与模板预览 | #30、#31 | W8 | M | 否（`CatalogIndex.xml` 的目录要换钉版） |
| W10 | 数据映射表 | #38 | W1、W8 | S | 否 |
| W11 | SQL Server 包 | #23 | W1，D4 | L | 可能（见 §3.4） |
| W12 | 解析诊断 | #28 | W1 | M | 否 |
| W13 | 搜索扩到图号、位号、文件名 | — | W6 | S–M | 否 |
| W14 | 还原缓存：位置可配、可清理 | §1.3 第 9 条 | — | S–M | 否 |
| W15 | 任意表的内容浏览 | #12、#13、#16、#18–#21，以及 #14、#15、#22 的其余表 | W4、W1 | L | **是** |
| W16 | 文案与图标 | §1.3 第 7、8 条；分支自己的三个旧图标 | — | S | 否 |

### 3.3 逐项说明

**W0 扩展点。** 新文件 `src/ui/window/pid_backup_browser/section.rs`：一支子树实现「给出自己的行」「给出某个节点的详情」「处理点到自己头上的消息并返回 `Effect`」三个函数；`NodeId` 增加一个通用变体（子树编号 + 下标路径），老变体不动。`visible_rows` / `detail_lines` / `update` 在各自 `match` 的末尾转给注册表。现有三支（备份文件、装置组、参考数据）先不搬，等后面的工作项碰到再搬。测试：一支假子树，验证行、详情、折叠、过滤都经过它。

**W1 详情区。** 改 `pid_backup_browser.rs` 里的 `kv`、`tree_row`、`details` 三个函数，新文件 `pid_backup_browser/grid.rs` 放表格。
- 键值行：键列随面板宽度取 30%–40%，键和值都换行；键比列宽长就上下排。
- 选中行加底色（照 `sheet_set.rs` 的做法：选中时用 `palette().primary.weak` 作背景）。
- `DetailLine` 增加一种「表」：列名 + 行，带横向滚动，点列名排序，行数多时只建可见的行（树现在是全量 `column`，742 行还撑得住，47 000 行不行）。
- 图元行的标签改成位号 / 名字，UID 留在详情里（D8）。
- 测试沿用 `detail_rows` 的纯函数断言，外加 `iced_test` 里按画出来的位置点击（这次修箭头时已经有先例）。

**W2 Manifest。** 新文件 `src/io/pid_backup_browse/manifest.rs`（模型：概况、按库分组的表和视图、权限、文件登记与 FileSize 合并、连接信息）和 `pid_backup_browser/manifest.rs`（`Manifest.txt` 行可以展开：概况 / 数据库对象 / 文件登记 / 连接信息）。`describe_files` 用文件登记给每个备份文件补上原始相对路径（`RefData~4~680 — Ref\rules.rul`），格式一栏按原名的扩展名说话，`exportlayer.xlsx` 不再叫 Zip。口令类字段按 D6 遮住，测试里钉死「详情区和报告里不出现原文」。

**W3 其他文件与文本预览。** 新文件 `src/io/pid_backup_browse/loose_files.rs`：包根目录里 `scan_backup_folder` 没认领的文件（`PlantConfig.xml`、`Export.log`）也列进「备份文件」。`pid_backup_browser/text_preview.rs`：文本类文件（按内容嗅探 UTF-16 / UTF-8 / GBK）在详情区显示前若干 KB，zip 成员按需解出再显示。`rules.rul`、`InsulationSpec.isl`、各个映射 XML、`Export.log` 都靠它。

**W4 表目录。** 新文件 `src/io/pid_backup_browse/export_log.rs`：解析 `Export.log` 的 `about to export <库>'s tables` 与 `exporting table <名> <n> rows exported`（清点时已经用脚本做过一遍，数字对得上 Manifest 的 154 张表）。`Export.dmp` 行可以展开：库 → 表（行数）。没有 `Export.log` 时退回 Manifest 的表名，不带行数。

**W5 类型化视图。** 新文件 `src/io/pid_backup_browse/exp_views.rs` 与 `pid_backup_browser/database.rs`。`ExpTables::read` 整个转储读一次（后台线程，结果留在会话里；74 MB 的读取耗时和内存先量再定是否常驻），然后：
- 全厂级：管线表（`pipe_runs`）、端口表（`port_tables`）、组成关系（`composition_relations`）、代码表（`codelist_short_text`）。挂在 W4 的表目录上，对应的表行点开就是表格。
- 图纸级：某张图的连接端（`connectivity_ends`）、OPC（`opc_points`）。挂在图纸节点下。
- 表里的代码值能翻成文字的翻成文字（`flow_spelling`、`nominal_spelling`、代码表）。

**W6 图内数据。** 新文件 `src/io/pid_backup_browse/drawing_data.rs`：`load_items` 现在只留 `items`，改成把 `PidPublishDrawing` 的其余集合也带回来。`pid_backup_browser/drawing_data.rs`：图纸节点下除了「图元」再加「连接点」「端口」「管线」「OPC」「图形表示」「统计」，各是一张表。包里有 Oracle 转储时用库补全端口和管线（`attach_oracle_ports`、`pipeline`），这一步依赖 W5 读好的 `ExpTables`。

**W7 联动。** 导入的实体在 `PID_SEMANTICS` XDATA 里带着 `oid`（发布的 GraphicOID），`PidPublishRepresentation` 又给出图元 UID 与 `graphic_oid` 的对应，所以数据是够的。面板里选中图元 → 如果这张图已经打开，就缩放到对应实体：`run_pid_backup_effect` 里转给现成的 `Message::PidLegendJump { min, max }`，不加新消息；要不要同时把实体选中，做的时候看现有的选择接口。反方向（在图上选实体 → 面板定位到图元）要在选择变化处挂钩，会碰 `src/app/update/mod.rs`，建议留到那边的改动收尾之后。

**W8 RefData 还原。** 新文件 `src/io/pid_backup/refdata.rs`，`restore` 里调用。按 Manifest 的 File 行把每个 RefData 卷放回它原来的相对位置：目录卷（`Symbols`、`Template Files`、`Report Files`、`SmartPlant Resources`、`Import Map Files`）解到 `<缓存>\Ref\<原目录>`，单文件卷拷成原名。包旁已经有解压好的 `Ref\` 时照旧用它。全厂包的 RefData 解开共约 66 MB（FileSize 行的合计），其中符号 34 MB 现在已经随 `Ref\` 拷进缓存，所以净增约 32 MB。这样「参考数据」在任何包里都是完整的，导入也能用上包里的符号和模板，不必换钉版。

**W9 预览。** 新文件 `pid_backup_browser/symbol_preview.rs`：选中 `.sym` 时在详情区画缩略图。画法照 `pid_palette.rs` 的 canvas 单元格，几何来自 `read_symbol_geometry`。`.igr` 先验证钉版读不读得了；读不了就只列文件，等换钉版。模板里的 `.pid` 可以双击打开。

**W10 数据映射。** `src/io/pid_backup_browse/datamap.rs`：`read_datamap` 读 `SPPIDDataMap.xml`，在该文件的详情里显示枚举映射表（W1 的表格）。

**W11 SQL Server 包。** `src/io/pid_backup_browse/mtf.rs`：`open_mtf_dump_as_sqlite` 把转储变成内存里的 SQLite，再查出图纸和装置组，喂给现有的配对逻辑。两件事要先定：许可（D4），以及「列出全部图纸」的查询放在哪边：钉版只有按 UID 取单张图的函数，OCS 自己写 SQL 就要直接依赖 `rusqlite`，否则请 pid-parse 加一个列表函数。

**W12 诊断。** `src/io/pid_backup_browse/diagnostics.rs`：图纸节点下加「诊断」：读图元的结果、`ImportSummary`（要导入一次，单张 0.01–6 s）、`pid_parse::inspect` 的报告文本。根节点加「检查全部图纸」：后台逐张读图元并导入，出一张表（哪张读不出、哪张导不进、各自原因），就是现在走查报告的界面版。全厂包约 40 s，所以是手动触发、带进度（D9）。

**W13 搜索。** 过滤现在只看图纸名，而且会把「备份文件」和「参考数据」两支藏起来。扩到图号、标题、备份文件与参考文件的名字，以及已经读过图元的图纸里的位号；匹配到图元时展开到那一行。

**W14 缓存。** `src/io/pid_backup.rs` 的 `restore_cache` 读一个设置项决定根目录（默认不变）；根节点详情显示缓存大小，加「清理缓存」。设置项落在 `src/app/config.rs`，选项页要不要加入口见 D5。

**W15 任意表。** 需要 pid-parse 把现在私有的 `Table` / `read_table_tolerant` 公开成「按表名读出列名和行」。有了它，W4 目录里的每张表都能点开（W1 的表格，分页或只建可见行）。

**W16 文案与图标。** 「尚未打开备份。」去重；路径分隔符统一；同组图纸按名字排；读不出、打不开的原因包一层本地化的说法，缓存路径换成图纸名；`pid_palette.svg`、`ui/collab.svg`、`ui/folder.svg` 三个图标按上游这次重画的 16 px 风格重画（现在只是补了宽高让测试通过）。

### 3.4 需要 pid-parse 配合的

| 事项 | 要什么 | 影响的工作项 |
|---|---|---|
| 任意表读取 | 公开一个按表名读表的函数（列名 + 行）。钉版和本地最新（`59e3469`，比钉版多 190 个提交）都没有 | W15 |
| SQL Server 库的图纸列表 | 一个「列出全部图纸 / 装置组」的函数，或者同意 OCS 直接用 `rusqlite` 查 | W11 |
| `CatalogIndex.xml` 目录、直接从 RefData zip 读 `.igr` / `.sym` | 本地最新已有（`publish::catalog_index`、`refdata_archive`），要换钉版才用得上 | W9 的目录部分；W8 可以因此省掉解压 |

换钉版本身是 D3。

### 3.5 顺序与并行

```
W0 ──┬── W2 ─┐
     ├── W3  │
     ├── W4 ─┼── W15（等 pid-parse）
     └── W1 ─┼── W5 ── W6 ── W7
             │         └──── W13
             ├── W10（还要 W8）
             ├── W11（等 D4）
             └── W12
W8 ── W9
W14、W16 随时
```

- W0 一个人先做完。之后 W1、W2、W3、W4、W8、W14、W16 七项互不相干，可以同时开。
- W1 落地之后 W5、W6、W10、W12 可以同时开。
- 会相撞的只有本地化：新标签都是 `pid-backup` 这一条 Fluent 消息的属性，要同时改 `src/locale_catalog.rs`、`src/i18n.rs` 的键清单和 23 个 `locales/*/opencadstudio.ftl`。建议每个工作项把自己的键集中成一段追加在末尾，合并时只是相邻追加。

### 3.6 怎么算做完

- 清点表 41 行每一行在界面上都有去处；确实做不了的（等 pid-parse 的那几类）在面板里说明「这类数据暂不能显示」而不是不出现。
- `app::commands::pidbackup::audit` 对三个包都通过，报告里每类数据有一段。
- 真实窗口里对全厂包逐类点开，截图留在 `target\ui-audit`，中文界面下没有英文原句和重叠。

## 4. 需要拍板的

| # | 问题 | 选项 | 建议 |
|---|---|---|---|
| D1 | 表格类数据放哪儿 | (a) 仍在「P&ID 备份」面板里，宽表靠浮动拉大；(b) 另开一个数据窗口或数据标签页 | **(a)**。不碰那三个正在改的文件，一个面板一套状态；(b) 看宽表更舒服，但要加窗口种类和消息，正好落在冲突区。先做 (a)，表格做成独立部件，以后要搬到窗口里只是换个容器 |
| D2 | 数据库表做到什么程度 | (a) 这一轮做表目录、行数和钉版已有的类型化视图（W4、W5），任意表等 pid-parse（W15）；(b) 等 pid-parse 出了通用读法再一起做；(c) OCS 自己再写一个 exp 解析器 | **(a)**。154 张表里只有 13 张钉版读得了；(a) 先让人看到库里有什么、多大，以及最有用的管线、连接、OPC、代码表。(c) 是把 pid-parse 的活重做一遍，不建议 |
| D3 | 换不换 pid-parse 钉版 | (a) 这一轮不换，参考数据靠 OCS 自己解压（W8）；(b) 换到最新 | **(a)**。最新比钉版多 190 个提交，换了之后 `pid_import` 54 条、`pid_legend` 29 条以及出图的对比都要重新过一遍，应该单独做、单独验。W8 之后原样的包也有完整的参考数据，不依赖换钉版 |
| D4 | SQL Server 包读不读 | (a) 读（W11），GPL-3.0 的 `oxidized-mdf` 继续链在程序里；(b) 维持「按文件夹列出」，并按 10-16 方案的 D5 把 `backup-mdf` 摘掉；(c) 维持现状，许可的事另议 | 这是许可问题，要你定。现状是 `Cargo.toml` 用默认特性，GPL 的部分已经链进来了，10-16 的 D5 没有落到 main 上。pid-parse 自己的说明是「内部使用可以，对外分发要先对齐许可」：照这个说法，只在内部用选 (a)，工作量 L；要对外分发选 (b) |
| D5 | 还原缓存 | (a) 默认位置不变，加设置项和「清理缓存」；(b) 默认改到别的盘或数据目录；(c) 不动 | **(a)**。这台机器 C 盘一度只剩 0.9 GB，全厂包一次还原约 130 MB，做了 W8 之后约 170 MB。入口先只放在面板根节点的详情里，选项页以后再加 |
| D6 | Manifest 里的口令 | (a) 一律遮住，只显示「有 n 项」；(b) 遮住，但给一个「显示」开关 | **(a)**。它们只是混淆过，不是加密；截图、报告、日志都可能带出去 |
| D7 | DX12 视口不出图 | (a) 另立一项先查；(b) 分支里先把 Windows 的默认探测顺序改成 Vulkan 在前；(c) 不管，验收时加 `--backend vulkan` | **(a)**，查清之前验收用 (c)。它不是备份功能的问题，也不是这次合并带来的，但「双击开图能看到图」是验收的一部分，本机默认就踩中。只在这一台机器（AMD RX590）、dev 配置的构建上见过，别的显卡和发布构建没试 |
| D8 | 图元行显示什么 | (a) 位号 / 名字，没有就显示类型加序号，UID 放详情；(b) 保持 UID | **(a)** |
| D9 | 全部图纸的体检 | (a) 手动触发，带进度；(b) 打开备份后自动在后台跑 | **(a)**。全厂包要 40 s 左右，占一个核 |

## 5. 附

### 5.1 全厂包的数据库表（取自 `Export.log`）

| 库 | 表数 | 行数 | 有数据的表 |
|---|---:|---:|---|
| SQPLANTPID（P&ID 应用数据） | 82 | 324 925 | 46 张：T_MODELITEM 47124、T_REPRESENTATION 42540、T_HISTORY 30931、T_RULEREFERENCE 28460、T_RELATIONSHIP 24664、T_SIGNALPOINT 22057、T_SYMBOL 18199、T_PROPERTYBREAK 14963、T_PLANTITEM 13687、T_LABELPERSIST 12584、T_LEADERVERTEX 12584、T_CONNECTOR 11757、T_PIPINGPOINT 9628、T_CONNECTORVERTEX 5181、T_PIPERUN 4686、T_INCONSISTENCY 4624、T_INLINECOMP 4533、T_INSTRFUNCTION 3718、T_INSTRUMENT 3718、T_PIPINGCOMP 3503、T_SIGNALRUN 1001、T_OPC 952、T_ITEMNOTE 800、T_NOTE 800、T_EQUIPMENT 402、T_NOZZLE 377、T_CASEPROCESS 301、T_EQUIPMENTOTHER 225、T_CASE 209、T_MECHANICAL 94、MAX_ID 80、T_OPTIONSYMBOLOGY 77、T_VESSEL 71、T_OPTIONSETTING 66、T_DRAWING 53、T_DRAWINGPROJECT 53、T_GLOBALDRAWING 53、T_OPTIONAUTOGAP 44、T_OPTIONFORMAT 39、T_OPTIONHEATTRACE 22、T_DRAWINGVERSION 15、T_EXCHANGER 12、T_SMARTFRAME 12、T_SMARTFRAMESTORAGE 12、T_OPTIONDISPSET 11、T_OPTIONDISPSETFOLDER 3 |
| SQPLANTPIDD（P&ID 数据字典） | 25 | 39 197 | 25 张：ITEMATTRIBUTIONS 29824、CODELISTS 3407、UNIQUEATTS 1527、ATTRIBUTES 1004、SPTPATTRIBUTES 700、SOURCEDESTOBJECTRELS 512、FILTERCRITERIA 401、FILTEREDRELATIONSHIP 383、FILTERINSTANCE 247、FILTERDEFINITION 242、CATEGORYFILTER 223、ENUMERATIONS 133、RELATIONSHIPS 101、SPSORTS 87、SPTPVIEWS 81、ENTITIES 80、ITEM 80、SPTPLAYOUTS 69、CORRELATTS 38、MAX_ID 24、CATEGORY 23、APPLICATIONS 5、VERSIONINFO 4、INTERSECTS 1、VERSION 1 |
| SQPLANTD（工厂数据字典） | 25 | 858 | 15 张：ITEMATTRIBUTIONS 354、UNIQUEATTS 131、CODELISTS 130、ATTRIBUTES 85、MAX_ID 24、ENTITIES 20、ITEM 20、SPTPVIEWS 20、RELATIONSHIPS 18、SOURCEDESTOBJECTRELS 16、ENUMERATIONS 13、CORRELATTS 12、VERSION 6、APPLICATIONS 5、VERSIONINFO 4 |
| SQPLANT（工厂） | 22 | 550 | 16 张：T_FORMATS 385、T_DRAWINGSITE 53、T_FORMATTYPES 41、MAX_ID 20、T_PLANTSETTING 14、T_ROLEACCESS 12、T_PLANTGROUP 6、T_PROJECTPLANTGROUP 6、T_HIERARCHYLEVELS 3、T_UNIT 3、T_AREA 2、T_ACTIVEWSSITE 1、T_APPPROJECT 1、T_PLANT 1、T_PROJECT 1、T_WSSITE 1 |
| 合计 | 154 | 365 530 | 102 张 |

钉版 `ExpTables::read` 读其中 13 张：`T_PIPINGPOINT`、`T_SIGNALPOINT`、`T_CONNECTOR`、`T_REPRESENTATION`、`T_PIPERUN`、`T_SIGNALRUN`、`T_OPC`、`T_NOZZLE`、`T_NOTE`、`T_ITEMNOTE`、`T_MODELITEM`、`CODELISTS`、`T_DRAWING`；`read_plant_groups` 另读 `T_PLANTGROUP`。

### 5.2 复现

工具链用 rustup 的 `stable`（1.99.1），不用 PATH 上那个改过的 cargo；`CARGO_TARGET_DIR` 指到本工作树的 `target`；`TEMP` / `TMP` 指到 D 盘（还原缓存写在临时目录下）。

```
rustup run stable cargo test --lib --no-run --offline -j 10
# 经应用走查一个真实备份包，写报告和面板图片
set OCS_PID_BACKUP_AUDIT=D:\work\plant-code\cad\pid-test-data
set OCS_PID_BACKUP_AUDIT_OUT=<输出目录>
set OCS_PID_BACKUP_AUDIT_LANG=zh-CN
<lib 测试可执行文件> app::commands::pidbackup::audit --ignored --nocapture
# 原有的两条真实数据测试
<lib 测试可执行文件> plant_test_backup --ignored --nocapture
```

`cargo test` 每次都会重编根 crate（`build.rs` 监视的 `packaging/windows/AppIcon.ico` 在调试构建里不存在），所以上面直接运行 `--no-run` 产出的可执行文件。

真实窗口：`OpenCADStudio.exe <图纸> --backend vulkan --http <端口>`，环境变量 `OCS_API_TOKEN` 给令牌，`POST /api/v1/commands {"cmd":"PIDBACKUPOPEN <路径>"}`，`POST /api/v1/ui_bounds {"prefix":"pid-backup-row-"}` 取行的位置，`POST /api/v1/capture` 存图。面板里的点击只能靠窗口消息：自动化通道的 `click` / `pointer` 只管绘图区。

### 5.3 这次留下的图

都在本工作树的 `target\ui-audit` 下（不入库）：

| 目录 | 内容 |
|---|---|
| `headless-plant`、`headless-plant-zh`、`headless-dwg0202`、`headless-test02` | 走查报告 `report.txt` 与面板在各状态下的图 |
| `gui-plant`、`gui-plant-search`、`gui-dwg0202`、`gui-test02` | 真实窗口逐步截图；`*-screen.png` 是屏幕上实际看到的 |
| `gui-plant-baseline` | 基线程序：图纸行没有箭头 |
| `gui-backends` | 基线与合并后、dx12 与 vulkan 各自的窗口 |
| `gui-close` | 执行过命令后关窗口弹出的「未保存的更改」 |

## 6. 执行记录

D1–D9 还没有拍板。第一批（2a）按 §4 的建议先走：D1 (a)、D2 (a)、D6 (a)、D8 (a)、D9 (a)；D3、D4、D7 不动，W11、W15 不做。每项一个提交，提交号在这一批收尾时补进本表。

| 项 | 提交 | 做了什么 | 清点表的变化 |
|---|---|---|---|
| W0 | 「P&ID 备份面板：子树扩展点（section）」 | `pid_backup_browser/section.rs`：一支子树一个 `Section`（挂在哪一行下面、自己的行、节点详情与给别的节点追加的详情、要在后台读的数据），`NodeId::Section(槽位, 路径)`；`visible_rows` / `detail_lines` / `update` 各自在末尾转给注册表；后台读取走 `Effect::LoadSection` → `PidBackupMsg::SectionLoaded`，带代数，换了备份之后迟到的结果丢弃。注册表此时是空的，面板没有可见变化。 | 无 |
| 附：`Select` | 「PIDBACKUPOPEN Select：命令行按标签路径定位节点」 | 方案里没有的一项，为的是不动鼠标也能看真实窗口：`PIDBACKUPOPEN Select 装置组 > SQPlant > test > U01 > D07 > #1 > #1` 逐级展开并选中，与逐级点击留下的状态相同（`pid_backup_browser/goto.rs`）；遇到要先读图元的图纸就等读完再接着走。真实窗口的检查因此可以只靠自动化通道发命令加 `/capture`。 | 无 |
| 附：修改标记 | 「停靠面板不再把图纸标为已修改」 | §1.3 里「`PIDBACKUPOPEN` 之后图纸带上修改标记」的原因：面板停靠时 `dock_keep_view`（`src/app/dock/update.rs`）把相机平移半个面板宽、让图在屏幕上不动，下一个 tick（`src/app/update/viewport.rs` 的 `on_tick`）发现相机变了，写回文档并置 `dirty`。与备份无关，任何停靠面板都如此。改在 `dock_keep_view`：相机原本与文档一致时，平移后当场写回文档并记为已同步，不置 `dirty`。三个约定不动的文件没有碰。 | 无（§1.3 的一条） |
| W1 | 「P&ID 备份面板：详情区键值不再重叠、选中行有底色、图元行显示位号、详情区表格」 | 键列取面板宽度的 36%（96–240 px），键比键列长就独占一行、值在下一行的值列，两者都在自己的框里换行；选中行用调色板的 `primary.weak` 作底色；图元行显示发布出来的名字（`IObject.Name`，注释取 `INote.NoteText`），没有名字的显示「类型 #序号」，UID 放进详情（`io/pid_backup_browse/labels.rs`）；`DetailLine::Table` 与独立的表格部件 `pid_backup_browser/grid.rs`（表头固定、列宽按内容、横向滚动、点列名排序、只建可见的行），图元类型的详情用它列出该类型的全部图元。全厂包 14 894 个图元里 3 425 个有名字。 | #26 的两处缺口（标签是 UID、键值重叠）补上，状态仍是 ✅；§1.3 的 1、2、3 条 |
| W2 | 「P&ID 备份面板：Manifest.txt 全量可看，备份文件显示原名，口令一律不出现」 | `io/pid_backup_browse/manifest.rs` 把 Manifest 的每一行归位（概况、表 / 视图 / 角色 / 权限 / 数据库文件、文件登记与 FileSize 合并、连接信息），口令类字段在读入时就换成 `•••`，之后的索引、面板、日志都拿不到原文；`pid_backup_browser/manifest.rs` 是第一支 section：`Manifest.txt` 行可以展开成 概况 / 数据库对象 / 文件登记 / 连接信息。备份文件行显示「存储名 — 原始路径」（`RefData~4~680 — Ref\rules.rul`），详情里有来源路径、源内文件数与目录数、打包前大小；单个文件按原名的扩展名说格式，不再把 `exportlayer.xlsx` 叫成有 12 个成员的 Zip。 | #1 ◐→✅，#2 ❌→✅，#3 ◐→✅，#4 ❌→✅（口令按 D6 遮住）；#32、#33、#36、#37 的文件现在看得出是什么（内容仍看不了，状态不变）；§1.3 的第 5 条 |
| W3 | 「P&ID 备份面板：包根目录的其他文件也列出来，文本文件点开就能读」 | `io/pid_backup_browse/loose_files.rs` 把扫描没认领的根目录文件（`Export.log`、`PlantConfig.xml`）列进「备份文件」，排在转储之后。`io/pid_backup_browse/text_file.rs` 读文本：编码按字节判断（UTF-16 / UTF-8 的 BOM，其次合法 UTF-8，再次 GBK，最后 Windows-1252），最多读 8 MiB（已见语料最大的文本是 3.7 MB 的数据映射），二进制文件只看开头 8 KB。`pid_backup_browser/text_preview.rs` 是第二支 section，不加行，只给已有的三种文件节点（备份文件、归档成员、参考文件）的详情加内容：编码、行数、逐行的表格（行号 + 文本，横竖都能滚，只建可见行）；不是文本的文件明说「不是文本文件」。读取在工作线程上，只保留当前选中文件的文本（`Section::keeps_only_selected`）。`Manifest.txt` 不走这条路（内容在它自己的子树里，口令已遮住）；别的文本里长得像口令的值同样换成 `•••` 并说明换了几处。 | #7 ❌→✅，#29 ◐→✅，#32 ◐→✅，#37 ◐→✅，#39 ◐→✅；#38 的 8 份映射 XML、#30 的 `CatalogIndex.xml` 原文可读（结构化视图仍未接，状态不变）；#6 的文本成员能看内容 |
| W4 | 「P&ID 备份面板：Export.dmp 展开成库和表，带行数」 | `io/pid_backup_browse/export_log.rs` 解析 `Export.log`（`about to export <库>'s tables`、`exporting table <名> <n> rows exported`，分区表按分区累加，`EXP-` / `ORA-` 消息、连接的数据库、字符集、导出结果一并取出），再与 Manifest 的 `Table` 行不分大小写地合并成 `TableCatalog`：库按 Manifest 的顺序和写法，表按名字。`pid_backup_browser/database.rs` 是第三支 section：`Export.dmp` 行可以展开成 库（表数）→ 表（行数），空表置灰；转储的详情增加库数、表数、有数据的表、总行数、导出信息和各库一览表；库的详情是它的表（可按表名或行数排序）；表的详情是库、表名、行数。没有 `Export.log` 的包（SQL Server）只列表名并说明原因。全厂包：4 库 154 表 365 530 行，与清点一致。 | #9 ❌→✅（SQL Server 包只有表名，没有行数）；#12–#22 的表现在看得到名字和行数，内容仍等 W15，状态不变 |
