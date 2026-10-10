# 标签放置的锚点十字不上图 · 修复计划（2026-10-10 开单，同日 Plannotator 批准，六条决策按推荐）

> 用户 2026-10-10（zhimo 会话 fable-5-1-25）：「接着开 B 项（标签锚点十字）的小单，先用 Plannotator 出计划」。开单时只做计划，**没改任何代码**；量数字用的是临时探针（跑完已删），数字全在 §1，落地时按 L0 把探针正式入库。
> **2026-10-10 Plannotator 批准**（`{"decision":"approved"}`，无批注）；用户同时在会话里说「批 B 项计划，L-D1 到 L-D6 都按推荐，从 L0 开始」——§3 六条决策全部按推荐项执行。
> 来源：10-09 用 `main` `aaedd334` 的 exe 开 `DWG-0201GP06-01.pid` 与 SmartPlant 参考图 `D:\work\cad\图片验证\pid-1.png` 逐实体对比，七条差异里的 **#6「标签锚点十字 `+` OCS 画出 4 个、参考图不显示」**；
> 10-10 覆盖单（pid-parse `docs/plans/2026-10-10-an-override-applies-its-own-width-colour-and-dash.md`）A3 顺带认出：这两笔是 `Item Note & Label` 缓存本体里 0.01 mm、随层色的锚点十字，库读取器只走 `Sheet*` 拿不到。
> 基线：OCS `main` `82f8684b`（干净，只有未跟踪 `.kiro/`）；pid-parse `e0cbd0a`（`codex/phase32c-bundle-closeout`，OCS 以 `path = "../pid-parse"` 依赖）。`ocs-pid-0928`（`pid-web-open`）仍被另一会话的 X2 未提交改动占着 `src/io/pid/{build,mod,tests,text}.rs` 与 `tests/pid_import.rs`，**本单不碰那棵树**。
> 上一单：pid-parse `2026-10-10-an-override-…`（O1–O4 已在 OCS `main` `b795b0f7` 落地）。本单与它不相交（它改线型样式，本单改放置本体画不画）。

## 一句话

0201 多出来的 4 个 `+` 是**标签类放置**（`Design\Drawing Description.sym` ×1、`Design\Annotation\Labels\Item Note & Label.sym` ×2、`…\Item Note & Label line1.5.sym` ×1）的缓存本体：每个本体在显示层 `Default` 上只有**两笔 0.01 mm 发丝线拼成的 2.54 mm 十字**（工艺的 `Remarks.sym` 是三笔、1.27 mm），其余笔画（`NULL` 模板文字、「说 明」的下划线）在文件关掉的 `Label` 层。
文件字面说十字显示，SmartPlant 屏幕一个都没画（参考图 2.12 px/mm 下四处逐像素为空）；而同一张图里 ` Line2.sym` 的 0.01 mm 发丝线 SmartPlant **画了**——所以不是「发丝线不画」，是「标签放置的锚点十字不画」。
修法在 OCS `src/io/pid/{symbols,build}.rs`：库路径说它是标签（`Labels` / `Labels - …` 目录，或本厂放在 `Design\` 下的 `Drawing Description.sym`）、且缓存本体可见笔画全是发丝线时，不画本体、不退库、不落标记点；pid-parse 不改码。OCS 现在把这些十字按放置样式重涂成 **0.35 mm 黑**（`apply_symbology`），所以在 OCS 里比在 SmartPlant 里还显眼。

## 1. 根因与证据（本单临时探针 + 参考图逐像素，corpus + 屏幕两证）

| 事实 | 证据 |
|---|---|
| 语料里**恰好 26 个**放置的可见本体是这种小十字，而且**恰好**就是库路径为标签类的 26 个 | 0201 4 个：oid 262 `Design\Drawing Description.sym`（本体 `/JSite329` sheet 1059，4 笔显 2）、452 / 617 `…\Labels\Item Note & Label.sym`（sheet 327，3 笔显 2）、602 `…\Labels\Item Note & Label line1.5.sym`（sheet 525，3 笔显 2）；0202 5 个：1662 / 2267 / 3406 / 6191 `Item Note & Label`（`/JSite793` sheet 2527）、3298 `Drawing Description`（sheet 4811），publish 副本相同；工艺 12 个：6302 `\\Spid\xa_lng_1\…\Design\Annotation\Labels\Xa Item Note & Label.sym`（`/JSite7559` sheet 384）+ 11 个 `\\SPID\XaLNG\P&ID Reference Data\Symbols\Piping\Labels - Piping Segments\Remarks.sym`（sheet 190，4 笔显 3：oid 6971 6987 7002 7019 7034 7052 7066 7084 7104 7122 7138）；D06 / A01 0。其余 56 个放置的可见本体最小是法兰管口 2.54 × 3.81 mm、0.13 mm |
| 十字的几何 | Item Note 族 / Drawing Description：`(0.10541,0.13970)-(0.10795,0.13970)` 与 `(0.10668,0.14097)-(0.10668,0.13843)`，以 `(0.10668,0.13970)` 为心臂长 1.27 mm；`Remarks`：`(0.10732,0.13970)-(0.10795,0.13970)`、`(0.10795,0.14034)-(0.10795,0.13907)` 加一笔 0.04 mm 的短桩 `(0.10855,0.13970)-(0.10859,0.13970)`。全部在本体的 `Default` 层（`0x0057 Top ViewFilterSet` displayed=true），隐藏的是 `Label` 层上的 `NULL` 文字（Drawing Description 还有一条 22.2 mm 下划线） |
| 十字的**样式是语料里独一份的**：覆盖掩码 `0x3`，`+42` 宽 1e-5 m，**`+34` 颜色 `0x80000000`（随层色哨兵）**，基样式 1 `Normal` 0.35 黑 | 每个装标签本体的缓存存储恰好一条这样的覆盖：0201 `/JSite329` #21、0202 `/JSite793` #71、工艺 `/JSite7559` #35，只被这些十字笔画引用。普查 6 个文件 77 个存储 69 条覆盖（含 0202 的 publish 副本，它的 `/JSite793` #71 算第二次）：**随层色只出现在这三条（4 次）**；别的发丝线覆盖（0201 `/JSite329` #47 / #48、`/JSite396` #9 / #10——` Line2.sym` 本体的 25.4 mm 虚线与 3.81 mm 线）颜色都是明写的黑 `0x000000`。缓存里没有任何 `LayerDisplayOverride`，随层色在文件里无色可随 |
| SmartPlant 屏幕**一个十字都没画** | 参考图 `pid-1.png` 1335 × 711，图框 x 34..1291.5 px ↔ 0..594.3 mm → **2.116 px/mm**，用设备表左上角 `(472.2, 402.0)` 与「说 明」`(507.7, 333.5)` 校准，误差 ≤ 5 px。OCS 把四个十字画在 `(474.68, 304.32)` / `(479.88, 246.32)` / `(394.45, 244.00)` / `(340.79, 245.55)` mm（0.35 mm 黑），对应 px `(1038,296)` / `(1049,419)` / `(869,424)` / `(755,421)`：±10 px 窗口里前两处**零个暗像素**，后两处只有本来就在的管线像素；一个 2.54 mm 十字该是 5.4 px 的 `+` |
| 但 SmartPlant **画发丝线**，所以不是宽度的事 | ` Line2.sym` 本体那条 25.4 mm、0.01 mm、虚线（覆盖 #9 把 `Normal` 变虚）的线，OCS 画在 x 135.64–161.04 / y 209.61；参考图在**同一 x 段**（321–375 px）有一条 1 px 的虚线（三段，y = 472 px）。见 §6 的新登记：它的 y 对不上 |
| `.sym` 的结构说这十字是符号作者的原点标记，不是图面内容 | `Item Note & Label.sym`：`/Sheet6` 空；`/Sheet12` 8 条线 + 文字 `XXX` 在**第二个**图层管理器的 `Default`（19 × 9.5 mm 方框加锯齿——目录预览）；`/PSMcluster0` 里才是这两笔十字（第一个管理器的 `Default`，层 5）+ `NULL`（`Label`，层 25）。图纸缓存的是后者。`Remarks.sym`：十字在 `/Sheet6`（层 8），两笔引线在 `/Sheet37`（第二管理器层 39），后者不缓存。`Drawing Description.sym`：`/Sheet12` 有「说 明」+ 下划线在 `Label`（层 20）。库读取器 `library.resolve` 对 Item Note 给 8 笔（预览方框）、Drawing Description 4 笔、Remarks 15 笔——**退库会画错东西**，跳过必须连库和标记点一起短路 |
| 文件里说「这是标签」的只有路径 | `igSymbol2d` 的 `sub_type`（`0x0000` / `0x0010`）标签与非标签都有；JSite 的 `JProperties` 只有 `OLES` + UNC 路径 + 相对路径。pid-parse 语义规则 10（文件名含 `note` → `PIDNote`）只盖得住 Item Note |
| 标签的正文另有来处 | 「说 明」是图纸 `/Sheet6` 自己的文字记录（OCS DXF：`(507.7, 333.5)` h 6.35、`Labels` 层），`Remarks` 的正文同理；本体上的 `NULL` 是模板占位，文件已关掉 |

**预计影响**（落地时以 L0 探针与 `pid_batch_report` 为准）：符号层笔画 0201 81 → 73、0202 120 → 110、工艺 157 → 122、D06 32 不变；`cache_bodies` 20 / 23 / 31 → 16 / 18 / 19；`drawn` 334 → 326、309 → 299（两份）、578 → 543；0201 符号调色盘 `" 35 #000000"` 9 → 1（只剩 ` Line2` 那笔）。

## 2. 范围

**做**：OCS `src/io/pid/symbols.rs`（分类与判定）、`build.rs`（`SymbolInstance` 分支短路）、`summary.rs` / `mod.rs`（新计数、`report_import` 一行、属性往返）、`tests.rs` 单测、`tests/pid_import.rs` 重钉 + 一条按 oid 钉的新测试、批量基线四行重签 + README 一段、GUI 复核进 evidence、本计划「进度」段；pid-parse 新探针一枚（L0，把 §1 的数字正式入库）。
**不做**：pid-parse `src` 不改（L-D1 选 (c) 时例外）；#8 ` Line2` 的 y 偏差（§6，另开）；#1 文字基线（X2）、#3 白底、#4 线宽显示；库 `.sym` 第二管理器那张预览页；标签正文（已是图纸文字）。

## 3. 要你拍板的

| # | 问题 | 选项 | 推荐 |
|---|---|---|---|
| L-D1 | 不画的判据 | (a) **路径说是标签 ∧ 可见本体全是发丝线**（宽 < 0.1 mm，比 ISO 最细的 0.13 还细）；(b) 只看路径（标签放置一律不画本体——SmartPlant 的标签可以带方框 / 流向箭头之类真图形，语料里没有，一刀切会把它们也砍了）；(c) 让 pid-parse 把「随层色」暴露出来（`PrimitiveStyle` 加字段），OCS 按「随层色 ∧ 发丝线」判——最像文件自己的话，但两仓同改，OCS `tests.rs:403` 字面构造 `PrimitiveStyle` 会先红；(d) 只看几何（发丝线 ∧ 外接 ≤ 3 mm），不看路径 | **(a)**：两个都是文件写明的事实，26 / 26 命中、` Line2` 不中，标签带真图形时照画并记一行 log；(c) 登记为「SmartPlant 可能真正按的规则」，等到语料里出现别的随层色笔画再议 |
| L-D2 | `Design\Drawing Description.sym` 不在 `Labels` 目录里 | (a) 常量点名（`LABEL_SYMBOLS_FILED_ELSEWHERE = ["Drawing Description.sym"]`，注释写明是本厂的归档习惯）；(b) 不管它，只修 4 个里的 3 个 | **(a)**：它的 `.sym` 和缓存与 Item Note 一个模子（十字在 `Default`、正文与下划线在 `Label`），只是放错了文件夹 |
| L-D3 | `PID-SYMBOL-LABEL` 上的符号名还写不写 | (a) 照写，锚在插入点（证据层本来默认关着；`a_placement_states_its_extent…` 的 20 个名字、工艺 11 个 `Remarks` 名字都不动）；(b) 连名字一起不写 | **(a)**：放置是真的、SmartPlant 也放了，只是本体不上图；名字是 OCS 自己的审计痕迹 |
| L-D4 | 计数怎么记 | (a) `SymbolBodies` / `ImportSummary` 新增 `label_anchors_undrawn`；这些放置**不**计入 `cache_bodies`，它们 `Label` 层上的隐藏笔画也**不**计入 `hidden_strokes_skipped`（整个本体都没动，不算「查过缓存后留下的」）；(b) 仍算 `cache_bodies`、画 0 笔 | **(a)**：`cache + library + markers + label_anchors == 放置数` 这条恒等式让测试一眼能对 |
| L-D5 | GUI 复核 | (a) 重编、Vulkan 开 0201，REST 截 Item Note 452 `(385–405, 235–253)` 与 Drawing Description `(465–485, 295–315)` 两窗，旁边放参考图同窗 ×8 裁片；(b) 只信 DXF 逐实体比 | **(a)**：这条是肉眼发现的，收口也肉眼看一次（约 15 min，含编译） |
| L-D6 | 发现要不要进 pid-parse 的文档 | (a) 只在 pid-parse 入一枚探针（L0），分析页不写，数字以本计划 §1 与 OCS evidence README 为准；(b) pid-parse 另写分析页 | **(a)**：格式层面没有新解码，只是把三个已知事实（路径、宽、随层色）摆到一起；探针入库是为了数字可复现 |

## 4. 步骤

- **L0 探针入库**（pid-parse `examples/probe_label_placements_cache_only_an_anchor_cross.rs`）：六个文件逐张列出每个放置的库路径、本体 `(site, sheet)`、可见 / 总笔画、可见外接尺寸、最大宽度、样式链（覆盖 id / 掩码 / `+34` / `+42` / 基样式），并普查全部存储里 `+34 == 0x80000000` 的覆盖；输出要能逐项对回 §1。`src` 不动；clippy 两种特性过；单独提交到 pid-parse。
- **L1 `symbols.rs`**：
  - `pub(super) fn is_label_symbol(path: &str) -> bool`：按 `\` / `/` 切段，任一**目录段**等于 `Labels` 或以 `Labels - ` 开头 → 真；否则文件名在 `LABEL_SYMBOLS_FILED_ELSEWHERE` 里 → 真。只看目录段，文件名含 `Labels` 的符号不算。
  - `pub(super) const UNDER_ISO_WIDTH_MM: f64 = 0.1`；`pub(super) fn is_anchor_cross_only(body: &PidSymbolDefinition) -> bool`：`visible_strokes()` 非空，每笔都是 `Line` 且自带样式宽 `< UNDER_ISO_WIDTH_MM`（没有样式的笔算不是）。
  - 文档注释写清三件事与 §1 的数字（26 / 26、随层色独一份、参考图四处为空、` Line2` 发丝线照画）。
- **L2 `build.rs`** `SymbolInstance` 分支：取 `cached` 后先判 `symbol_path.is_some_and(is_label_symbol) && cached.is_some_and(is_anchor_cross_only)` → `symbol_bodies.label_anchors += 1`，`built = Vec::new()`，不进 `cached_body_entities` / `library_body_entities` / 标记点；名字照 L-D3 写。两种半命中各记一行 `log::info!`：标签路径但本体有真笔画（照画）、非标签路径但本体只有发丝十字（照画）——留给下一批语料判断 SmartPlant 到底按哪条。
- **L3 计数**：`SymbolBodies.label_anchors` → `ImportSummary.label_anchors_undrawn`（文档注释、`properties()` 键 `label_anchors_undrawn`、`set` 分支、`Default`）→ `report_import` 一行「N label placements' anchor crosses left undrawn」只在 N > 0 时打；`mod.rs:575` 一带装配。
- **L4 测试**：
  - `tests.rs`：`is_label_symbol` 六例（`…\Design\Annotation\Labels\Item Note & Label.sym` 真、`…\Piping\Labels - Piping Segments\Remarks.sym` 真、`…\Equipment\Labels - Equipment\Descriptions\Vessel Description.sym` 真、`…\Design\Drawing Description.sym` 真、`…\Piping\Piping OPC's\Off-Drawing.sym` 假、`…\Piping\Valves\Labels Valve.sym` 假）；`is_anchor_cross_only` 三例（合成十字 0.01 → 真；同十字 0.13 → 假；带一笔无样式 → 假）；summary 属性往返含新键。
  - `tests/pid_import.rs` 重钉：`a_placement_draws_the_body_the_drawing_carries_and_skips_its_hidden_layers`（:3530 笔画 81 / 120 / 157 → 73 / 110 / 122，`cache_bodies` 20 / 23 / 31 → 16 / 18 / 19，`hidden_strokes_skipped` 31 / 26 / 24 → 26 / 20 / 12，加 `label_anchors_undrawn` 4 / 5 / 0 / 12 与恒等式）、`a_symbol_body_draws_in_the_style_its_placement_names`（:2705 `" 35 #000000"` 9 → 1）、`the_layer_manager_summary_…` / `every_entity_is_filed_under_its_authored_layer…`（0201 `Default` 120 → 112）、`the_import_leaves_a_summary_the_app_can_show`、`fixtures_import_with_visible_drawing_content` 若钉了总数。
  - 新测试 `a_label_placement_keeps_its_anchor_cross_off_the_sheet`：0201 四个标签放置的插入点附近没有任何符号层笔画、` Line2` 的 25.4 mm 发丝线仍在（且仍虚）、四个名字仍在 `PID-SYMBOL-LABEL`；工艺 11 个 `Remarks` 同理。
  - `cargo test --test pid_import`（52 → 53）、`--lib io::pid`（54 → ≥ 57）、`clippy -D warnings`、fmt。
- **L5 批量基线重签**：`cargo run --example pid_batch_report -- ..\pid-parse\test-file --export-dir $env:TEMP\pid-batch > $env:TEMP\report-new.csv`，按 README 比。预期**只变四行**（0201、0202、publish-0202、工艺），变的列只有 `drawn` / `layered_entities` / `cache_bodies` / `hidden_strokes` / `dxf_sha256`；D06 / A01 一字不变。改前 / 改后 DXF 逐实体比（去 `5` / `105` / `330` / `340` / `350` / `360`）：**改后是改前的子集**，少的只有 LINE——0201 8 条、0202 10 条（两份）、工艺 35 条，坐标与 §1 的十字一一对上；头变量除 `$HANDSEED` 外相同。`report.csv` 覆盖、README 加「2026-10-10 重签（标签锚点十字）」一段。
- **L6 GUI 复核**（L-D5 (a)）：`cargo build`，`OpenCADStudio.exe <0201> --http <port> --backend vulkan`，REST `capture` 两窗；参考图同窗 ×8 裁片用 `System.Drawing` 从 `pid-1.png` 按 §1 的标定裁；证据进 `docs/evidence/2026-10-10-pid-label-anchor-crosses/`（README 写标定公式与四处像素结论）。用完 `Stop-Process`。
- **L7 提交**：OCS 只 add `src/io/pid/{symbols,build,summary,mod,tests}.rs`、`tests/pid_import.rs`、`docs/evidence/2026-09-24-pid-batch-baseline/{report.csv,README.md}`、新 evidence 目录、本计划；pid-parse 只 add L0 探针。`.kiro/` 与别的会话的文件不碰。

## 5. 验收

1. 0201：符号层少且只少 8 条 LINE（四个十字），` Line2` 的发丝线仍在仍虚，其余实体一字不动；0202 少 10 条、工艺少 35 条；D06 / A01 不变。
2. `pid_import` 全绿（含新测试），`--lib io::pid` 全绿，clippy 两种特性零告警，fmt 干净。
3. `report.csv` 只有四行、只有 §4 L5 列出的五列变；DXF 逐实体比只有 LINE 删除。
4. 肉眼：OCS 在四处不再有 `+`，与参考图同窗并排一致（L-D5 (a)）。
5. 每条数字能从 L0 探针输出指回去；两种半命中的 log 行在语料上**一行都不出现**（26 个全命中，其余 56 个两条件都不满足）。

## 6. 风险、边界与新登记

- **判据的两种漏法**（L-D1）：(i) 标签带真图形——(a) 照画，若 SmartPlant 其实连它们也不画，会多画；(ii) 非标签符号用随层色发丝线——(a) 照画，若 SmartPlant 其实按随层色不画（(c) 的假设），会多画。两种都有 log 行，下一批语料见到再定。
- `Drawing Description.sym` 是本厂归档习惯，常量点名只对这一家对；别家的「说明」标签若放在 `Labels` 下，走目录规则即可。
- `pid-web-open`（`ocs-pid-0928`）上 X2 未提交的 `build.rs` / `tests.rs` 与本单改同一文件：本单落 `main`，X2 合回时 `SymbolInstance` 分支与 X2 的文字块多半不重叠，但要人工看一眼。
- **新登记 #8（另开）**：参考图里 ` Line2.sym` 本体那条 25.4 mm 虚线在 **y = 221.3 mm**（472 px），OCS 画在 **209.61 mm**——x 段分毫不差、y 低了 11.7 mm；该放置 `(34.0, 57.2)`、`rot 0`、`scale [1,1]`，本体 `/JSite396` sheet 119，同图 `/JSite329` sheet 506 还有一份同样的 25.4 mm 发丝线。另外参考图的虚线节距约 6.6 / 2.4 mm，不是文件的 3.5 / 1.75——SmartPlant 的虚线比例另有算法，也一起记着。
- 顺带见到：根表与缓存里各有一两条宽 `0.000 mm` 的 `JStyleSimpleLine`（0201 根 #26 / `/JSite329` #34、0202 #54 / #10、工艺 #53 / #30），本单没查它们被谁引用。

## 7. 进度

- 2026-10-10 开单（OCS `main` `82f8684b`、pid-parse `e0cbd0a`，会话 fable-5-1-25）；待 Plannotator。
- 2026-10-10 Plannotator 批准（`approved`，无批注）；L-D1～L-D6 全按推荐。
- 2026-10-10 L0 探针入库 pid-parse `1365f0e`（`src` 不改）。
- 2026-10-10 L1–L5 在 OCS `main` 工作树落地（**未提交**）：`src/io/pid/{symbols,build,summary,mod,tests}.rs` + `tests/pid_import.rs` + 批量基线 `README.md`/`report.csv`，+505/−49；`pid_import` 53/53、`--lib io::pid` 56/56、`report.csv` 重跑零差异（只变 0201/0202/publish-0202/工艺四行，D06/A01 哈希不变）、单文件 clippy/rustfmt 干净（fable-5-1-25 落地、fable-5-1-32 复核）。
- 2026-10-10 **L-D3 实作偏差（待追认）**：符号名锚在「十字本该在的位置旁」，非字面「插入点」——让名字落在标签正文旁而非一百多毫米外的插入点；上一班判定更合理、未改码，留待用户追认。
- 2026-10-10 L6 GUI 复核（opus-4-8-4 会话）：debug exe（`D:\Rust\target\debug`，10:09 构建，含 L1–L5）Vulkan 开 0201，REST 截五窗 + full，参考图同窗裁片、side-by-side 入 `docs/evidence/2026-10-10-pid-label-anchor-crosses/`（16 图 + README 已补）；四处标签锚点十字在 OCS 全部消失、` Line2` 发丝线仍在仍虚（#8 y 偏差范围外），与参考图并排一致（验收 #4 过）。
- 2026-10-10 GitHub issue：`happyrust/pid-cad#1` 开单（#6 标签锚点十字），正文含现象 / 根因 / 判据 (L-D1 a) / 影响 / 验证 / 遗留（#8、L-D3）。
- 2026-10-10 **L7 提交**（OCS `main`，会话 fable-5-1-90）：按 §4 L7 只 `git add` 明确路径 —— `src/io/pid/{symbols,build,summary,mod,tests}.rs`、`tests/pid_import.rs`、`docs/evidence/2026-09-24-pid-batch-baseline/{README.md,report.csv}`、本 evidence 目录、本计划；不含 `.kiro/` 与别会话未提交文件。pid-parse L0 探针 `1365f0e` 另仓已提。本会话未复跑测试（避免与并发构建抢共享 `target/`），功能验证沿用 fable-5-1-25 落地 / fable-5-1-32 复核：`pid_import` 53/53、`--lib io::pid` 56/56、`report.csv` 零差异；本会话仅核提交清单（`git show --stat` 恰为上列路径、`.kiro/` 等仍未跟踪）。
- 待办：推送（`main` 未推，含本 L7）、L-D3 追认、#8 ` Line2` y 偏差另开。
