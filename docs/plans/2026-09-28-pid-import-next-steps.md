# `.pid` 解析与加载显示 · 进度复盘与下一步 · 开发计划（2026-09-28 开单，同日批准；R1 / W0 同日落地，R2 改向为 P-D12「死记录不画」并批准，pid-parse 侧已落地）

> 用户 2026-09-28（zhimo 会话 fable-5-1-4）：「继续分析 D:\work\plant-code\cad\OpenCADStudio 当前的PID解析和加载显示的开发进度，使用 plannator 制定下一步的开发方案。」
> 基线：OCS `07cec253`（`main`，2026-09-24；`.pid` 相关最后一个代码提交是 `6b629e2e`）；pid-parse `181d37e`（`codex/phase32c-bundle-closeout`，领先本地 `main` 2 个文档提交）。
> 工作树：OCS 主工作树正被另一会话的 `rvt` 接入改着（`src/io/mod.rs` / `src/app/update/file.rs` / 21 个 locale / `Cargo.toml` / `Cargo.lock`，未提交；`src/io/pid/` 干净）；
> pid-parse `src` 干净，四个文档 / 探针文件被另一会话改着。本单**不碰**这些文件。
> 开单时只做分析与计划，**没改任何代码**。上一单：`2026-09-24-pid-import-status-and-next-steps.md`。
> **2026-09-28 Plannotator 批准**（无批注）；用户「按推荐顺序开工（R1 先，W0 并行）」。同日 R1（pid-parse `cc6626e` 探针）与 W0（同一提交）落地——
> **R1 量出的东西改了主线**：11 条 run 打架的标签差别只在连字符 / 星号上（R2 不值一天），但顺带发现语料里 41 条**原生读取器会跳过的死记录**今天被画在屏幕上（P-D12，等批）。见「进度」。

## 一句话

09-24 单的十一条决策除 V2 / B2 外全部同日落地；之后只再落了一张小单（另存 DWG / DXF 后 TrueType 字体保住，`6b629e2e`）和三条与 `.pid` 无关、但让它打开更快的性能提交——
**09-25 起 `.pid` 线没有新的代码改动。** 在现有五张图上，「打得开、画得全、字对、层对、能另存」已经到头：语料里没有一条该画没画的记录，
今天还开着的两项（V2、B2）都在等用户的材料。下一步的问题不再是「修哪条偏差」，而是**往哪个方向扩**：
① 一条文字里多种字号 / 字体逐段保真（R，四图 11 条，是屏幕上最后一处已知偏差）；② 网页版也能开 `.pid`（W，功能缺口）；
③ 没有 `_Data.xml` 也给位号（S，先取证）；④ 语料与回归（B，等材料）。推荐 **R 先（半天–1 天，把文字线收干净）→ W（先量后做）**，S 只取证，B 材料到了就跑。

## 09-24 以来发生了什么

| 日期 | 仓 | 提交 | 与 `.pid` 的关系 |
|---|---|---|---|
| 09-24 | OCS | `6b629e2e` | 小单 F1–F4 落地：STYLE 组码 3 写字体文件名（`ARIALN.TTF`），`resolve_text_style` 按文件查回族名；`pid_import` 51 → 52；四图 `--export` 只有 STYLE 组码 3 变 |
| 09-24 | OCS | `c926b170` / `2582a0eb` | debug 构建里 pid-parse / acadrust 走 opt-level 2：0201 解析 ~290 → ~80 ms、`--export` 0.64 → 0.42 s；七个 `--export` 字节不变 |
| 09-24 | OCS | `5a567277` / `07cec253` | 文字 SDF 烘焙与轮廓三角化挪出锁外：大图冷镶嵌 7–8 s → 4.8 s；四图 `--export` 与 0201 SVG 字节不变（`.pid` 文字多，开图首帧受益） |
| 09-24 | pid-parse | `686c9d5` → `181d37e` | `DependencyObject` 成员数上界（census 0201 1 → 0）；其后三条文档提交记性能链 |
| 09-25 → 09-28 | 两仓 | — | 无 `.pid` 相关提交；OCS 工作树上是另一会话的 `rvt` 接入（未提交） |

## 现状（2026-09-28）

### 解析：pid-parse（本轮重跑）

- `cargo test --test render_gap_census --test parse_real_files --test style_link_ratchet --test geometry_profile` → **4 / 135 / 17 / 2 全绿**（HEAD `181d37e`，`src` 干净）。
- 五图拒收 0 / 4 / 8 / 0 / 0（0202 四条、工艺八条，全是 scope 3 两点重合的零长折线，09-24 H2 判正确拒收）；无解码器 0 / 0 / 0 / 0 / 1（A01 一条 `0x007B igGroup`，容器、无几何）。
  **语料上没有一条该画没画的记录。**
- 文字：段落 + run 两条路都交出（`text_styles_for_document` → `effective()` = run 的字高 / 颜色 / 字体 + 段落的对齐 / 行距）；四图 155 条索引 106 条按 run 改了字高 / 字体 / 颜色；
  run 打架 11 条压平；25 条落 0.254 mm 哨兵样式、正确拒收（OCS 回退 2.5 mm）。`igTextBox` DTO 只有插入点与方向，**没有文本框尺寸**。
- 其余（容器与流 / 15 族记录 / 样式 / 符号缓存本体与逐笔样式 / 图层与显示位 / 参数化 / `_Data.xml` 语义 / Geometry profile）同 09-24 单「现状」表，未变。

### 加载与显示：OCS

- 入口 `src/io/pid/` 九个文件 3 972 行：`load_pid` → `PidImport { document, summary }`，走通用打开管线（读在后台线程，摘要以文档属性穿管线、开图完成时取走）；`.pid` 只读，`Ctrl+S` 转另存。
- 用户看得到的：`Smart P&ID Files` 过滤 / 文件关联；导入摘要三行 + 单位回退一行（21 语种）；原图图层名与显隐、图层管理器「图纸图层」视图与角色开关（`PID_VIEW_FILTER` 随图保存）；
  特性面板 P&ID 组；`PID_SEMANTICS` XDATA 随 DWG / DXF 保存；文字按 run 取字号 / 字体 / 颜色、按段落取对齐 / 行距、多行拆行；符号先缓存本体后库、关闭层笔画不画、逐笔虚线；另存后 TrueType 字体保住。
- 测试：`pid_import` 52、`--lib io::pid` 54（`6b629e2e` 提交记录；本轮**未重跑**——共享 target 里没有现成的 OCS 产物，主工作树又带着别人未提交的 `rvt` 改动，冷编译加半成品风险，不插队）。
- 批量基线 `docs/evidence/2026-09-24-pid-batch-baseline/report.csv`：六张图导入 16–93 ms（debug、opt-level 2 之后），`missing` 0 / 0 / 4 / 1 / 4 / 8，`library_bodies` 六张全 0，四主图 DXF 哈希 = `6b629e2e` 之后的基线。
- 网页版：`io::pid` 与 `pid-parse` 都在 `cfg(not(target_arch = "wasm32"))` 后面，`ocs_web_worker` 只路由 `dwg | dxf`——**网页版今天开不了 `.pid`**（能开带 `PID_SEMANTICS` 的 DWG）。

### 还开着的

| # | 项 | 性质 | 卡在哪 |
|---|---|---|---|
| 1 | V2：逐笔线宽 vs 放置线宽裁决；Ball Valve Type 1 / Remarks / Item Note & Label 三例复核 | 待裁 | **等用户一张 SmartPlant 截图**（工艺 `Xa` OPC 特写或 D06 Ball Valve Type 1，一张就够） |
| 2 | B2：新语料 | 语料缺口 | **等用户提供**新的 `.pid`（最好连 `_Data.xml` 与 `Ref\Symbols`）；本轮全盘重扫（`D:\work` 深 6 层）仍只有 4 张不同的图 + A01 |
| 3 | 一条文字里多种样式（四图 11 条：工艺 10、0202 1） | 可见偏差（小） | 不卡：09-24 N-D3 的备选，要出 MTEXT |
| 4 | 25 条哨兵样式文字回退 2.5 mm（有回退日志的 0201 1 / 0202 4 / 工艺 16） | 未定性 | 没人量过：DTO 无文本框尺寸，要回字节；实际多大要看 SmartPlant |
| 5 | 网页版开 `.pid` | 功能缺口 | 09-24 登记不做；pid-parse 为读 SQL Server 备份带 `rusqlite(bundled)` + `oxidized-mdf`，不编 wasm；CFB 读取走路径不走字节 |
| 6 | 没有 `_Data.xml` 时实体没有 `class` / `label` / `oid`（本机四张主图旁都没有 `_Data.xml`，只有 publish 目录的 A01 / 0202 副本有） | 语义缺口 | 文件里「标签 ↔ 物项」的关联是否可达未取证（`DependencyObject` 四图 353 条、`igTextBox.parent_ref` 是线索） |
| 7 | pid-parse `main` 又落后工作分支 2 个（文档）提交 | 仓库卫生 | 动远端要点头 |
| 8 | B 系 / ANSI 页幅、英制、`igDimension` / `igBalloon` / `igLeader`、JDim 其余 7 种、OLE 站点 | 语料缺口 | 同 B2 |

## 候选方向的量

### R · 富文本逐段保真（MTEXT）

- 对象：四图 11 条 run 打架的标签（工艺 10、0202 1）。今天压平成覆盖字符最多的那种样式，计入 `ImportSummary.lettering_flattened` 与一行日志。
  样例：工艺管道号 `250-LNG-57602`（号段与 `-` 两种样式）、`m^3` 的上标 `3`（08-22 分析 §5）。
- 路：`build_entities` 对 `runs.len() > 1` 且样式不一致的记录出 `MText` 而不是 `Text`，内联码 `\f<族>|b0|i0;` / `\H<比例>x;` / `\C<色>;` / 上标 `\S3^;`；
  `entities/text_support.rs` 已解析这些码（`\S` 堆叠、`\A`、逐 span 字体与字高——现有 DWG 的 MTEXT 就是这么画的）。pid-parse 已交出 `runs` 与 `resolve_run_style`，**pid-parse 无改动**。
- 牵动：对齐（MTEXT 用 attachment point，不是 TEXT 的对齐 + 对齐点）、多行（MTEXT 自带 `\P`，但只改这 11 条、多行 TEXT 拆行不动最稳）、`pid_import` 里钉 TEXT 的断言、DXF 往返（acadrust 的 MTEXT 读写已在 DWG 路上跑）。
- 验收：四图 `--export` 逐实体比，只许这 11 条从 TEXT 变 MTEXT、其余零差异；`lettering_flattened` 归零（改成 `lettering_rich` 计数）；工艺 / 0202 特写各一张。

### W · 网页版打开 `.pid`

- pid-parse 侧：`backup`（`rusqlite` / `oxidized-mdf`）改成可选特性（默认开，OCS web 关）；CFB 读取器加字节入口（`cfb::CompoundFile::open(Cursor<Vec<u8>>)`；`api.rs` 12 处、`cfb/reader.rs` 6 处按 `Path` 走）；
  `symbol_library` / `style_link::*_for_file` 是路径薄壳，Geometry 路上本来就不用。`cargo check --target wasm32-unknown-unknown --no-default-features` 是量尺——**编不过的不一定只有 `backup`，先量**。
- OCS 侧：`io::pid` 拆出 `load_pid_from_bytes`（符号库查找、`sysfont::face_file_name`、`PID_SYMBOL_LIBRARY` 环境变量三处 native-only 分支）；`ocs_web_worker` 把 `pid` 路由过去；导入摘要走同一条属性通道；
  网页版没有 Arial Narrow，按现有 web 字体回退并在摘要里点名。
- 语料上六张图 `library_bodies` 全 0：网页版不带符号库不损失一笔。
- 验收：网页版打开四主图与 native 的实体数 / 记录数 / 图层表相同；native 四图 `--export` 字节不变；`trunk build` 通过。

### S · 没有 `_Data.xml` 也给位号（只取证）

- 今天 `class` / `label` / `oid` 只来自 `_Data.xml`（`GraphicOID` 两跳）。文件里 `DependencyObject` 把成员 oid 编组、`igTextBox.parent_ref` 指所属；
  若「标签 → 物项」在文件里可达，`label=` 可直接由标签文字给出、`class` 由符号库路径给出（`Valves\Ball Valve Type 1.sym` → 类）。
- 只取证：一支探针用 0202 publish 副本的 `_Data.xml` 当答案，数 `(GraphicOID, ItemTag)` 里有多少条能仅凭文件内关联从标签文字复原；复原率与走法写一页分析，**≥ 80 % 才开实施单**。

### B · 语料与回归

- B1 工具在（`examples/pid_batch_report.rs`），B2 等材料。可先做的一小步：把六张图基线 CSV 变成一条 OCS 集成测试（跳过 `import_ms` 与哈希），新图一来先跑它、老图不退。

## 决策（等批；⭕ = 推荐）

| # | 决策 | 结论 |
|---|---|---|
| P-D1 | 下一轮主线 | ⭕ **R 先、W 后**：R 半天–1 天收掉文字线最后一条可见偏差；W 是最大的功能扩展，先量（W0）再做。备选：① W 先（网页版是刚需时）；② 只做 S 取证 + 等材料（暂不再投入 `.pid` 线时） |
| P-D2 | R 的范围 | ⭕ 只把 run 打架的 11 条改 MTEXT，其余 144 条与多行拆行照旧 TEXT；不把整条文字线迁到 MTEXT |
| P-D3 | R 的对齐 | ⭕ MTEXT attachment 按段落对齐映射（左 / 中 / 右 × 基线），插入点从 TEXT 的对齐点算回；R1 先量这 11 条的对齐分布 |
| P-D4 | R 的上标 | ⭕ 语料只有 `m^3` 一类：run 字高明显小于主 run 时写 `\S3^;`；量不出垂直偏移就只缩小字号、不堆叠 |
| P-D5 | W 的顺序 | ⭕ W0 量尺（pid-parse 加 `backup` 特性 + wasm check，报编不过的清单）→ 你看清单再批 W1（pid-parse 字节入口）/ W2（OCS web 路由）；W0 不改行为，native 四图字节不变 |
| P-D6 | W 的字体 | ⭕ 网页版按现有 web 字体回退，摘要多一行「网页版字体回退 N 条」；不打包 Arial Narrow |
| P-D7 | S | ⭕ 只做取证探针 + 一页分析，不改 OCS；复原率 ≥ 80 % 再开单 |
| P-D8 | 哨兵字高 25 条 | ⭕ 登记不做：DTO 无文本框尺寸，要回字节；与 V2 同一批等 SmartPlant 截图看它们实际多大再定 |
| P-D9 | B1 进测试 | ⭕ 做（半天内）：`tests/pid_batch_baseline.rs` 读基线 CSV，比实体 / 记录 / 没画 / 拒收 / 单位 / 页幅，跳过耗时与哈希 |
| P-D10 | pid-parse `main` | ⭕ 快进 2 个提交并推 origin——**动远端，要你点头**；不点头只登记 |
| P-D11 | V2 / B2 | ⭕ 继续等材料，清单如下：V2 一张 SmartPlant 截图（工艺 `Xa` OPC 特写或 D06 Ball Valve Type 1）；B2 新 `.pid`（最好连 `_Data.xml` 与 `Ref\Symbols`） |

## 工作项

### R 线（主线，OCS）

- **R1**：探针量 11 条的 run 分布（字号 / 字体 / 颜色 / 对齐 / 是否上标）→ 数写进本单。
- **R2**：`build_entities` 对打架记录出 `MText`；`resolve_styles` 给逐 run 样式；`register_text_styles` 多收 run 点名的字体；`lettering_flattened` → `lettering_rich`；
  `pid_import` 钉工艺 `250-LNG-57602` 与 0202 那一条（内联码、attachment、字高）。
- **R3**：四图逐实体比、新基线哈希写进本单、特写两张进 `docs/evidence/`、user-guide「文字」一段补一句。

### W 线

- **W0**（pid-parse）：`backup` 改 `[features] default = ["backup"]`；`cargo check --target wasm32-unknown-unknown --no-default-features`，把编不过的模块清单写进本单；native 全部测试与 clippy 不变。
- **W1**（pid-parse，W0 后批）：字节入口（`parse_bytes` / `from_reader`），`api.rs` 的路径入口改成薄壳。
- **W2**（OCS，W1 后）：`load_pid_from_bytes`、三处 native-only 分支、`ocs_web_worker` 路由、`trunk build`、网页版四图实体数对 native。

### S / B / 卫生

- **S1**：取证探针 + 分析（pid-parse `examples/` + `docs/analysis/`）。
- **B3**：`tests/pid_batch_baseline.rs`。
- **H1**：pid-parse `main` 快进并推（点头后）。

## 执行顺序 ⭕

**R1 → R2 → R3**（半天–1 天）∥ **W0**（≤ 2 小时，pid-parse）→ 你看 W0 清单 → **W1 → W2**（1–2 天）；**B3** 穿插（半天）；**S1** 在 W1 等批时做；**H1** 点头即做。
OCS 主工作树被 `rvt` 会话占着：R / B3 的 OCS 侧在干净 worktree 里做与验（09-24 H2 的做法），提交只 `git add` 自己的路径。

## 验收

- R：四图 `--export` 逐实体比，只有 11 条 TEXT → MTEXT、其余零差异；`pid_import` 52 → 53+；`--lib io::pid` 54 不降；clippy 零告警；特写两张。
- W0：清单 + native 测试 / clippy 不变；W2：网页版四主图实体数 / 记录数 / 图层表 = native，native 四图字节不变。
- B3：六张图与基线 CSV 逐数一致。S1：复原率与结论一页。

## 登记不做（本轮）

| 项 | 理由 |
|---|---|
| 整条文字线迁 MTEXT（含多行） | R 只动 11 条；多行 TEXT 拆行 08-22 已验，改了要重验 145 条对齐 |
| 哨兵字高 25 条 | P-D8 |
| 网页版打包 Arial Narrow | P-D6 |
| 语义走 SmartPlant 数据库备份（pid-parse `backup`） | 没有备份文件语料；S1 先看文件内能复原多少 |
| 按视图过滤集分别呈现图层状态、`.pid` 图配图例识别规则、B 系页幅 / 英制 / 未见记录族、`0x0010` 子记录 | 同 09-24 单 |

## 验证（本单开单时）

- pid-parse `181d37e`：四组棘轮 `render_gap_census` 4 / `parse_real_files` 135 / `style_link_ratchet` 17 / `geometry_profile` 2 全绿（本轮重跑，`src` 干净，共享 target）。
- OCS：`git log -- src/io/pid tests/pid_import.rs examples/pid_batch_report.rs` 证实 `6b629e2e` 之后无改动；`pid_import` 52 / `io::pid` 54 取该提交记录，**未重跑**（原因见「加载与显示」）。
- 全盘（`D:\work`，深 6 层）按扩展名扫 `.pid`：仍是 4 张不同的图 + A01（其余是同图副本）。
- 网页版开不了 `.pid`：`crates/ocs_web_worker/src/lib.rs` 只 `matches!(ext, "dwg" | "dxf")`；`Cargo.toml` 注明 pid-parse native-only。
- MTEXT 内联码可画：`src/entities/text_support.rs` 解析 `\S` 堆叠 / `\A` / 逐 span 字体字高。

## 进度

### R1（pid-parse `cc6626e`，`examples/probe_run_conflicts.rs`）

- **11 条**（按 `(流, oid)` 计；流里原始记录 12 条，工艺 oid 6345 存了两次，见下）：工艺 10 条全是管道号（`50-LNG-58101-1D02-C` 这类，9 段 run），0202 1 条是容积注记 `27.0 m^3  3.0 m*3.0 m*3.0 m`（3 段）。
- run 的差别：工艺 10 条一律是号段 **2.822 mm Arial Narrow** 与连字符 **2.999 mm Arial**（1.06 倍）交替，44 个 `-`；0202 那条是一个 `*` 用 **3.175 mm 宋体**、其余 2.822 mm Arial。
  段落对齐 12/12 左、行距 1.0、旋转 0°、无换行、**没有一条 run 的字高低于最宽 run 的 0.85 倍**（`m^3` 的 `^` 是字面字符，不是上标）。
- 今天压平取最宽 run（Arial Narrow 2.822 / Arial 2.822）后，屏幕差别是 44 个连字符窄 0.18 mm、一个星号小 0.35 mm 换了字体。**R2 改 MTEXT 买不到肉眼看得见的东西**（P-D13）。
- **顺带发现——死记录被画**：工艺 oid 6345 在 `/Sheet6` 存了两条 `igTextBox`，同插入点 (0.1705, 0.2215)、同层，旧文本 `250-LNG-57602- - `（字节 1615..1799）与现文本 `250-LNG-57602-6D02-C`（46104..46294）；
  旧的那条 PSM 类型字带 **`0x8000` 位**（DTO `type_flags = 0x2`），原生 `PSMSerializeIn`（`radsrvitem.dll`，`docs/analysis/2026-05-14-radsrvitem-psm-serialize-bytes.md`）读到这一位就**整条跳过、不读 oid**。
  按这一位普查五图已解码记录：**41 条**——`igSymbol2d` 27（全在工艺：13 个 oid 各 1–3 份旧副本，与活副本同插入点、同定义本体，活副本总在更靠后的字节）、`igLine2d` 8（0202 `/Sheet6615` 4、A01 `/JSite204/Sheet6` 4）、
  `igSmartFrame2d` 5（0201 oid 947，页框已按范围去重）、`igTextBox` 1（上面那条）。pid-parse 里只有 `DependencyObject` 与 `JStyleOverride` 两个解码器拒 `type_flags != 0`，其余族照收，几何发射器照画——
  **OCS 今天在工艺上把 13 个符号多画了 1–3 遍、把旧管道号叠在新管道号上（`- - ` 的连字符压在 `6D02` 上）。**

### W0（pid-parse `cc6626e`）

- `rusqlite` / `oxidized-mdf` / `zip` / `sha2` 改可选，`[features] default = ["backup"]`；`backup` / `publish` / `export_bundle` 三个模块与 `export_bundle` 的再导出挂 `cfg(feature = "backup")`；
  五个二进制（`pid_backup_extract` / `pid_backup_probe` / `pid_inspect` / `pid_msda_probe` / `pid_publish_xml`）、`publish_walkthrough` 例子、`pid_pipeline` bench 标 `required-features`。默认特性下行为不变。
- `cargo check --lib --no-default-features`（native）通过；`cargo clippy --lib --no-default-features -- -D warnings` 零告警；默认特性 `cargo check --all-targets` 通过；四组棘轮 4 / 135 / 17 / 2 全绿。
- **`cargo check --lib --no-default-features --target wasm32-unknown-unknown`：只剩 6 处错误、一个原因**——`cfb` 0.14 在 wasm 上用 `web-time::SystemTime`，本仓 `cfb/reader.rs:91-95`（存储时间戳与 1601 纪元比较）与 `writer/cfb_write.rs:78-81`（写回时间戳）用的是 `std::time::SystemTime`。
  其余（`quick-xml` / `uuid v5` / `encoding_rs` / `schemars` / `env_logger` / `serde_json`）全部编过。**W1 由此缩成**：一个时间类型垫片（wasm 下 `use web_time::SystemTime`）+ `PidParser::parse_reader` 薄壳（`cfb::reader::parse_pid_package_from_reader` 已经在）——估计 ≤ 半天；W2 不变。
- 没做的：`tests/` 里 17 个 backup / publish 测试文件没标 `required-features`（`--no-default-features --tests` 会编不过），默认特性不受影响；W1 顺手加 `[[test]]` 条目或把它们搬进 `tests/backup/`。

### P-D12 · pid-parse 侧落地（`a68bf55`，用户批 P-D12 – P-D14 后同日）

- `PSM_TYPE_FLAG_NATIVE_SKIP` / `PsmHeader::native_reader_skips` / `parse_live_psm_header`：22 个族解码器的 `decode_at` 都从它起步——类型字带 `0x8000` 的记录哪个族都不解、几何不发射、`style_link` 不索引；
  `parse_psm_header` 照旧读出旗标（普查与探针要看见）。第三类普查 `native_skipped_record_census` → `SheetGeometry::skipped_records` → `NormalizedPidGeometry::skipped_graphic_records`，
  与「拒收」「无解码器」不相交、不进 warnings。
- 语料 **42 条**（比 R1 探针多一条：0202 `/Sheet6615` 的矩形本身也带位，整个孤儿存储已退役）：工艺 `igSymbol2d` 27 + `igTextBox` 1；0202 `igLine2d` 4 + `igRectangle2d` 1；0201 `igSmartFrame2d` 5；A01 `igLine2d` 4。
- 棘轮重钉：工艺放置 58 → **31**、放置笔画 (287, 237) → (181, 157)、显示笔画 237 → 157；0202 线 46 → 42、A01 80 → 76；线样式关联 669 → 638；调色板 `As Drawn` x43 → x17 等；
  文字形状 3 12 → 11、选择子 257 / 12 → 248 / 11；「156 条标签对 155 条索引」的 oid 6345 碰撞消失；活矩形 3 → 2。拒收 / 无解码器两表不动。golden 四份重签（0202 219 → 215、工艺 476 → 448、A01 117 → 113）。
  新棘轮 `the_records_the_native_reader_skips_are_counted_apart`。
- 验证：40 个测试二进制 **1476 全绿**（`--lib` 1121），`clippy --all-targets -D warnings` 两种特性零告警，rustfmt 干净。分析 `docs/analysis/2026-09-28-the-native-reader-skips-a-flagged-record.md`，CHANGELOG 两条（P-D12 / W0），`task_plan` 指针。
- **OCS 侧待做**（不改码）：干净 worktree 里 `pid_import` 重钉（工艺实体 / 放置数，0202 / A01 各少 4 条线）、批量基线 CSV 与 README、四图 `--export` 新哈希、逐实体比只许少不许多或挪。

### R1 之后的改向（2026-09-28 用户批准 P-D12 – P-D14 按推荐；⭕ = 推荐）

| # | 决策 | 结论 |
|---|---|---|
| P-D12 | 死记录不画 | ⭕ **做，替代 R2 成为 R 线主项**：pid-parse 各族解码器统一 `type_flags & 0x2`（`0x8000` 位）→ 拒收并计数（与 `DependencyObject` / `JStyleOverride` 今天的做法、与原生读取器一致），`render_gap_census` 多一档「原生跳过」；预期工艺 −27 放置 −1 文字、0202 嵌套表 −4 线、A01 −4 线、0201 −5 框（屏幕无变）。OCS 侧不改码，但 `pid_import` 钉的实体数 / 放置数、批量基线 CSV、四图 `--export` 哈希会变，逐实体比只许**少**、不许多或挪。备选：只在几何发射器按 `(流, oid)` 去重保留最后一条——不如按位判，位是原生的判据 |
| P-D13 | R2（MTEXT） | ⭕ **登记不做**：R1 量出差别在 0.18–0.35 mm 的连字符 / 星号上；`lettering_flattened` 计数与日志留着，新语料出现真上标 / 多字号再开 |
| P-D14 | W1 范围 | ⭕ 按 W0 结果缩成时间垫片 + `parse_reader` 薄壳 + 测试 `required-features`，不再单独等批，与 P-D12 一起放行即做 |

## 门禁记录

- 2026-09-28：用户经 zhimo「继续分析 … PID解析和加载显示的开发进度，使用 plannator 制定下一步的开发方案」→ 本单（会话 fable-5-1-4），送 Plannotator 批注。P-D1 – P-D11 等批。
- 2026-09-28：Plannotator 批准（无批注）；用户「按推荐顺序开工（R1 先，W0 并行）」→ R1 / W0 同日落地（pid-parse `cc6626e`）。R1 量出 R2 不值、死记录被画 → P-D12 – P-D14 送批（会话 fable-5-1-4）。
- 2026-09-28：用户「批 P-D12 / P-D13 / P-D14：死记录不画开工，W1 接着做，R2 登记不做」→ P-D12 pid-parse 侧同日落地（`a68bf55`）；OCS 侧重钉与 W1 接着做（会话 fable-5-1-4）。
