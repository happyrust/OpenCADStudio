# ` Line2` 本体 y 偏半英寸、虚线节距偏大（P&ID 遗留 #8）

> 日期：2026-10-10
> 对应：GitHub issue `happyrust/pid-cad#3`（拆自 #1 的 P&ID 收口 #6 遗留项）
> 范围：根因未坐实，本计划**先确认后修**——确认阶段只读（probe + IDA 批处理），
> 修复阶段落 pid-parse 或 OCS（由确认结果定，见 §3 L-D1）。
> 承接：
> - `OpenCADStudio/docs/plans/2026-10-10-a-label-placement-keeps-its-anchor-cross-off-the-sheet.md` §6 #8（本单的登记处）
> - pid-parse `docs/analysis/2026-09-07-nested-site-curves-are-embedded-symbol-bodies.md`（嵌套 JSite = 符号本地坐标，经 igSymbol2d 放置上页）
> - pid-parse `docs/analysis/2026-09-18-the-parametric-chain-closes-on-the-template-not-the-instance.md`（§5 ` Line2` 实例本体 = 模板本体，逐坐标相同；§8 `SymbolInformation.extents` 疑似符号原点，未坐实）
> - pid-parse `docs/analysis/2026-09-20-jflavorholder-carries-the-placed-instances-parameters.md`（§3 ` Line2` flavour = `[0.0254]` = 默认）
> - pid-parse `docs/analysis/2026-08-07-jstyle-simple-dash-type-linetype.md`（0201 虚线图案 native-reader 解为 3.5/1.75 mm）

## 1. 现象与已确证的数据

` Line2.sym` 的 25.4 mm 发丝虚线是 #6 判据的「反例」——它不是标签锚点，#6 修复后 OCS 照画、SmartPlant 也画。但与参考图 `D:\work\cad\图片验证\pid-1.png` 并排（`OpenCADStudio/docs/evidence/2026-10-10-pid-label-anchor-crosses/sbs-0201-line2-hairline.png`）有两处对不上：

**① y 整体偏低（真正的定位 bug）**

- 放置（golden `tests/golden/geometry/dwg-0201gp06-01.json` 的 `igsymbol2d:17`）：`insertion=(0.03398, 0.05721) m = (33.98, 57.21) mm`、`rotation=0`、`scale=[1,1]`、`definition={site:396, sheet:119}`；该页 `page_transform` 是单位阵（origin (0,0)、scale [1,1]、单位 m、page max 594.3 mm）。
- OCS 实际画（`OpenCADStudio/tests/pid_import.rs::a_label_placement_keeps_its_anchor_cross_off_the_sheet`）：线 `start.y = 209.61 mm`、`max x = 161.04 mm`、长 25.4 mm → x 自 135.64 到 161.04 mm。
- 反推本体局部坐标（OCS 变换 `apply(x,y)=insertion+local`，见 §4）：x 自 **101.6 到 127.0 mm（4″→5″）**、y = **152.40 mm（整 6″）**。三个数都落在整英寸档（符号库作者单位是英寸，见承接的参数化分析 §3）——**说明本体解码与 OCS 渲染都忠实**。
- SmartPlant 参考图：该线在 **y ≈ 221–223 mm**（README 记 221.3；472 px ÷ 2.116 px/mm = 223.06；标定误差 ≤ 5 px ≈ ±2.4 mm），**x 分毫不差**。
- 偏移：纯 y 方向、整体偏低 **≈ 12.7 mm（0.5″）**（报告值 11.7 mm，在标定误差内）。x 为零。

**② 虚线节距偏大（疑为已知 view-scale，非定位 bug）**

- 文件字面：0201 的 14 条虚线图案 native-reader 解为 **3.5 / 1.75 mm**（划 / 空），` Line2` 这条经 `JStyleOverride` 的 `+78` 换成该 dash（承接的 dash 分析 §5 + §7.4）。
- 参考图：节距约 **6.6 / 2.4 mm**（并排图里参考 3 段粗虚线、OCS 多段细虚线）。
- 线索：OCS 已登记「SmartPlant 对 **style 声明的尺寸**（线宽 #4、点标记）做全图放大」——节距也是 style 声明的尺寸，极可能是**同一个 view-wide scale**，而非位置/解码错误（位置 x 为零偏即旁证：放大的是尺寸不是坐标）。待 §4 S2 用 `style.dll` 的 dash 渲染坐实。

## 2. 做 / 不做

**做**
- 先把偏移测准（S0）、把符号原点读出来（S1）、用 IDA 批处理坐实 SmartPlant 的放置合成到底多了什么变换（S2）。
- 坐实后按最小面落修（pid-parse 解原点 **或** OCS 应用原点，L-D1），两处嵌套 ` Line2`（sheet 119 放置 + 同图 `/JSite329` sheet 506 的另一份 25.4 mm 发丝线）一起验。
- 节距按 S2 的 `style.dll` dash 渲染结论定性：若是 view-scale 则归 #4、本单只记不修。

**不做**
- 不在根因坐实前改任何投影 / 放置代码（§6 风险：全局应用符号原点会动到现在位置正确的本体）。
- 不碰 #6（标签锚点，已收口）、#4（线宽显示）、#1（文字基线）、#3（白底）。
- 不改 pid-parse 已 byte-exact 对上库的圆 / 弧本体解码（09-07：20/24 曲线逐字节对上）。

## 3. 要你拍板的（S3：能否定的已写下，还开着的仍开着）

可证伪的一句：按 14 锚点标定（`docs/evidence/2026-10-11-line2-reference-calibration/README.md`），参考图 ` Line2` y = **209.74 ± 1.42 mm**，OCS **209.61 mm**，差 **+0.13 mm**（参考图 − OCS）。0 在界内，12.70 mm 与 11.53 mm 都不在。此前的 11.53 mm 来自 #6 单锚点标定把 x 的 2.116 px/mm 用于 y（实为 1.997 px/mm）且锚点错 6.7 mm。y 偏差不是 bug，是测量伪影。

| # | 问题 | 选项 | 倾向 |
|---|---|---|---|
| L-D1 | 根因坐实后修在哪仓 | (a) pid-parse：把符号原点（`SymbolInformation.extents` 或 igSymbol2d 字段）解进 DTO，本体坐标在解码时减原点；(b) OCS：`Placement::apply` 时减去原点；(c) 两仓都不改——若坐实是 SmartPlant 独有的 anchor 约定而文件本体坐标本就是对的 | **(c) 两仓都不改。** 页差是 +0.13 mm，在 T4 的 ±1.42 mm 里；文件本体坐标和 OCS 放置都对。减 `extents` 会再低 152 mm（S1），`JSymbol::Transform` 也不减 origin（S2）。(a)/(b) 不做 |
| L-D2 | 原点是否对所有符号放置生效 | (a) 全局（所有 igSymbol2d 本体都减原点）；(b) 只对确认受影响的族 | **不减、无需。** 没有要减的原点。Manifold 新鲜截图对参考图弧心 Δy = −0.003 mm、Δx = +0.044 / −0.049 mm（T1），不是页上的 0.5″。不全局减，也不按族减 extents |
| L-D3 | 节距偏大本单修不修 | (a) 坐实是 view-scale（同 #4）→ 不修、只登记；(b) 另有算法 → 本单或另开单修 | **节距是 x 向，x 标定正确，6.14/2.84 mm 仍有效，本单只登记。** 「同线宽 #4 的 view-scale」仍未坐实（S2：虚线那个 +24 不乘线宽）。不改节距代码 |

## 4. 步骤（确认优先）

**S0 — 精确重测偏移（只读，GUI）**
用本机 debug OCS（Vulkan + REST `/api/v1/capture`，不走桌面自动化，照 #6 evidence 的办法）截 ` Line2` 两处放置（sheet 119 的 (130,200)–(167,231) mm，和 `/JSite329` sheet 506 的另一份）与参考图同窗 ×8 裁片，把 SmartPlant 侧那条线的 y 测到 ±1 px，确认：偏移是否**恒为 0.5″ = 12.7 mm**、是否**两处一致**、x 是否**确为零偏**。用完 `Stop-Process`。
> 没测准之前不要信「11.7 还是 12.7」——S4 的判据依赖它恰好是 0.5″ 还是别的数。

**S1 — 读符号原点（只读，probe）**
跑 `cargo run --example probe_parametric_chain_resolves_a_cached_body`（输出含 `SymbolInformation.extents`）与 `probe_jflavorholder_carries_instance_parameters`，取：
- ` Line2`（site 396 / 实例 sheet 119、模板 `/JSite329` sheet 501）的 `extents` 两个 f64；
- 对照组 Manifold 的 `extents=(0.0635, 0.0889)`。
判据：若 ` Line2` 的 `extents=(x0, y0)` 满足 `x0 ≈ 0`、`y0 ≈ ±12.7 mm`，则**符号原点假设坐实**；同时看 Manifold 的 y0=88.9 mm 是否对应它在 OCS 的位置误差（控制组，关系到 L-D2）。
> 避开与并发构建抢共享 `target/`：开跑前确认没有别的会话在 build（看终端）；只跑这两个 example，别带 `--all-targets`。

**S2 — IDA 批处理坐实放置合成（只读，逆向）**
⚠ 本机 `ida-bridge` 是坏的（逆向教程明载：headless `idalib` 的 runner `import fcntl` 是 POSIX 专属、Windows 没有；UI 插件 `PLUGIN_ENTRY` 抛 `NameError`）。所以**走 IDA 批处理**：
```powershell
& "D:\IDA Professional 9.2\ida.exe" -A -Llog.txt -Sprobe.py <db>.i64   # -A 必须；-S 路径别加引号
```
DLL 库已在 `pid-parse/dlls/`：`radsrvitem.dll.i64`（igSymbol2d `0x00CE` 放置读取器 + igLine2d）、`style.dll.i64`、`imagdex.dex.i64`、`ugeom2d1.dll.i64`、`j2dsrv.dll.i64`；另 `_ida-probe-plant10-2026-08-05/` 有 `style.dll.i64` / `jengine.dll.i64`（已命名 425 函数）。probe 脚本骨架照教程（`idc.batch(1)` + `ida_auto.auto_wait()` + hexrays，落 `out.txt`）。两个确认目标：
1. **放置合成**：反编译 radsrvitem 的 igSymbol2d（`0x00CE`）读取器与其绘制 / 合成路径，确认把本体上页时是 `insertion + 2×2·local` 还是 `insertion + 2×2·(local − origin)`；若有 origin，定位它的来源字段（`SymbolInformation.extents` / igSymbol2d 的某偏移 / `JFlavorHolder`）。这一条直接判 §3 L-D1 的 (a)/(b)/(c)。
2. **dash 渲染**（判 ②）：反编译 `style.dll` 的 `JStyleSimpleDashType` 渲染侧（非 `0x1001BB50` 那个 Load worker，而是用 dash 画线的消费点），看划 / 空长度是否乘了一个 view / line scale——坐实节距偏大是不是和线宽 #4 同一个 view-scale。

**S3 — 定根因、落 L-D 决策**
把 S0/S1/S2 的结论写成一句可证伪的判据（「SmartPlant 放置本体时减去 `<字段>` 的 y=12.7 mm，OCS 没减」之类），更新 §3 三个 L-D。

**S4 — 实作（依 L-D1）**
- (a) pid-parse：解 `SymbolInformation.extents`（或坐实的原点字段）进 DTO，本体图元坐标减原点；棘轮钉 ` Line2` 两处与 Manifold 对照；golden 按预期改（只动受影响本体的坐标）。
- (b) OCS：`Placement::apply` 减去 `at.origin`（新字段，来自 pid-parse 的 DTO）；`PlacementMeasures` / extent 同步。
两条都要：Manifold 等控制组**不回归**。

**S5 — 测试 + GUI 复核**
`cargo test --test pid_import`、`--lib io::pid`、pid-parse `--test parse_real_files`；更新 `a_label_placement_keeps_its_anchor_cross_off_the_sheet` 里 ` Line2` 的 `start.y` 断言到新值；report.csv 重跑按预期差异。GUI 复核两处 ` Line2` 与参考图并排一致、x 不回归，证据进 `docs/evidence/`。clippy / rustfmt。

**S6 — 节距结算（依 L-D3）**
若 S2 坐实是 view-scale：在本单与 #4 处各记一行「` Line2` 节距偏大 = style 尺寸 view-scale，同线宽」，不改码。否则按 S2 结论另开单或本单修。

**S7 — 提交 + 回链**
OCS / pid-parse 各只 `git add` 本单明确碰过的路径（禁 `-A` / `.`）；提交信息 `refs happyrust/pid-cad#3`；在 issue #3 回链本计划与 evidence，勾掉待办。

## 5. 验收

1. ` Line2` 两处放置（sheet 119 + `/JSite329` sheet 506）在 OCS 的 y 与参考图并排一致（±2 px），**x 不回归**。
2. 控制组不回归：09-07 byte-exact 对上库的 20 条圆 / 弧本体、Manifold / 其余参数化本体的位置与 extent 不变（或按坐实的原点一并修正且都与 SmartPlant 对齐）。
3. `pid_import` / `--lib io::pid` / pid-parse `parse_real_files` 全绿；report.csv 只按预期差异变；clippy 两特性零告警、fmt 干净。
4. 每个数能从 S0/S1 的测量与 S2 的反编译指回去；节距按 L-D3 结算（归 #4 或修）。

## 6. 风险、边界与开口

- **全局应用符号原点的回归面（最高风险）**：若 L-D2 选全局，所有 igSymbol2d 本体都减原点——现在位置**正确**的本体（与 SmartPlant 比过的那些）会被改歪。必须先有「哪些本体现在位置是对的」的基线（S0 要把 Manifold 也测一遍），否则宁可只对确认受影响的族生效 (b)。
- **测量误差**：0.5″ = 12.7 mm 是推断，报告值 11.7 mm；标定 ±2.4 mm。S0 不先测准，S4 的判据立不住。
- **符号原点可能不是 `extents`**：09-18 §8 把 `extents` 记为「疑似符号原点，未坐实」，它在 Manifold 上 =(轴线 x, 弧心 y)——更像**几何锚**而非放置原点。S2 的反编译要能分清「放置减的那个数」到底是 `extents`、还是 igSymbol2d 自带的另一个字段、还是 `JFlavorHolder` 里 "Sheets" 后那两个未解 `u32`（` Line2` 是 `0x86 / …`）。
- **库本体 vs 缓存本体**：OCS 对 ` Line2` 先用缓存本体（site 396），库 `.sym` 兜底。两者若 y 不同，修哪个要对应 SmartPlant 实际画的那个（09-07：缓存 = SmartPlant 所放）。
- **ida-bridge 不可用**是硬约束，已在 S2 写明走批处理；别再试 headless idalib。

## 7. 进度

- 2026-10-10 开单（会话 fable-5-1-122）：GitHub issue `happyrust/pid-cad#3` 已建（label bug）；本计划据 #1 收口 #6 时登记的 #8 展开。
- 2026-10-10 根因调查（只读，未改码）：
  - OCS 变换链确认：`Projection::point` 无 y 翻转、仅乘 mm_per_unit；`Placement::apply` = insertion + 旋转/缩放(local)，x/y 对称（`src/io/pid/{page,symbols,build}.rs`）。
  - 由 OCS 实测 y=209.61 / x≤161.04 反推本体局部 (4″,5″,6″) 整英寸 → 解码 + 渲染忠实；偏移为纯 y ≈ 12.7 mm（0.5″），x 零偏。
  - 排除：解码布局 bug（x 完美）、实例参数重算位移（09-18 §5 / 09-20 §3：` Line2` 实例本体 = 模板、flavour=[0.0254]=默认）、嵌套 sheet 页变换（08-31/09-07：LdcSite 无自带页变换，本体为符号本地坐标经 igSymbol2d 放置）。
  - 剩余主假设：**未应用符号原点 / anchor**（`SymbolInformation.extents`，未坐实）；控制组 = Manifold（extents (0.0635, 0.0889)）。
  - 节距 ②：文件 3.5/1.75 native-reader 已解；参考偏大疑为已知 view-scale（同 #4），待 S2 坐实。
  - ida-bridge 本机不可用（fcntl / NameError），改走 IDA 批处理；DLL 库 `pid-parse/dlls/*.i64` 齐全、IDA Pro 9.2 已装。
- 2026-10-10 S1 读 extents（只读，会话 grok-4.7-7，未改码）。`pid-parse` v0.11.7 现编跑两个 example，都 exit 0；数字来自 `test-file/DWG-0201GP06-01.pid`。
  - ` Line2` 实例 `/JSite396` oid 117 与模板 `/JSite329` oid 513 的 `extents` 相同：`(0.10165148245596259, 0.1524)` m = (101.651 mm, 152.400 mm)。y0 是发丝线自己的 y（整 6″，JDim 499），x0 是 bbox 左端。计划里的判据「x0≈0 且 y0≈±12.7 mm」**没过**。
  - 实例 flavour oid 118 = `[0.025400]`，是默认 1″，不是 12.7 mm 偏移。
  - Manifold 模板 oid 77 / 实例 oid 13 的 `extents` = `(0.0635, 0.0889)` m，等于轴线 x、弧心 y（弧心 y 在拉伸后仍是 0.0889）。这是本体里的几何锚，不是页上的位置误差。本轮没有拿 Manifold 对参考图重测像素。
  - 把这对 `extents` 当原点减掉会把 ` Line2` 落到放置 y = 57.21 mm，比现在的 209.61 低 152.4 mm，也比参考图 ~221 mm 更远；Manifold 会整符平移 (−63.5, −88.9) mm。**不能按 L-D1(a) 减 `SymbolInformation.extents`。**
- 2026-10-10 S2 放置合成（只读 IDA 批处理，未改码）。`symbol.dex` 不在 `pid-parse/dlls/`，用的是 `D:\pid\RADInstallA~\symbol.dex`（32 位，临时库）。没走 ida-bridge。
  - `0x00CE` 的类是 `JSymbol`。`IJTypedGeometry4JSymbol::Transform`（`sub_10057FF0`，日志 `PS Transform a symbol` / `PS Transform a paper symbol`）把传入矩阵交给成员几何，或用 `gsumath1_341` 做矩阵合成。没有 `local − origin`，也不读 `extents`。
  - `JSymbol::IJGraphicImp::Display`（`sub_100510E0`）不减原点；只在标志 `0x10000` 且比例不是 1 时，用显示上下文 +456 的 double 做 SF scale。
  - 关键点索引 0（`IJKeyPoint` 槽 5，`sub_10058680`）才把一个 3 向量取负填进矩阵平移。Display / Transform 不调它。若画线减的是 S1 那个 extents，线会偏约 152 mm，对不上参考图只差约 12.7 mm。
  - 图形槽 6（`sub_10053AD0`）的 ±0.5 是退化外框（以矩阵平移为中心的 1×1 range），不是把线移 0.5″。
  - 结论：上页是矩阵作用到本体坐标，不是 `local − extents`。
- 2026-10-10 S2 虚线节距（只读，`style.dll.i64` 的临时副本，未改码）。
  - 画线消费点是 `JStyleLinePointGen::IJStyleDashTypeClientImp` 槽 4（`sub_1003AAD0`），不是 Load worker `sub_1001BB50`。每段先过 `sub_100030EE`（真身 `sub_1007B150`）做单位换算，再乘一个比例：画图上下文对象 `+8` 的 bit2（`& 4`）置位时，比例是它 `+24` 的 double，否则是 1。沿线铺开的 `sub_1001A470` 用同一判据。正数段和负数段分进两个累加器，符号就是划 / 空。
  - 线宽笔画不读这个 double。`JStyleLineSolid` 虚表槽 41 → `sub_1003EF60` 只把对象 +80 的线宽和 +60 的模式送进同一个单位换算。`sub_1004B460` 会把该 +24 缓存到线样式 +184 并打脏标志，但不乘线宽。点标记 `sub_1004A7B0` 才把 +136 的尺寸乘上这个 +24。
  - 单位上下文里的 `+24` 是另一个 dword 标志，管的是 +0 / +8 / +16 / +40 那几个换算系数，不是虚线乘的那个 double。
  - 单一比例会保持 3.5∶1.75 = 2∶1。参考图约 6.6 / 2.4 ≈ 2.75，对不上。L-D3「节距和线宽 #4 是同一个 view scale」**未坐实**。
- 2026-10-10 S0 参考图重测（只读，会话 grok-4.7-7，未改码）。量的是 `D:\work\cad\图片验证\pid-1.png` 原图 1335×711，不是放大后的裁片。标定沿用 #6：图框 x 34..1291.5 px ↔ 0..594.3 mm（2.116 px/mm），y 用 Drawing Description 锚点 (304.32 mm = 像素 296) 上翻。1 px = 0.47 mm。
  - 这条发丝线只落在第 **472** 行，上下两行在该 x 段零个暗像素，行号本身没有 ±1 px 的含糊。换算 **y = 221.14 mm**。相对 OCS 导入器钉住的 209.61 mm，差 **11.53 mm**。刚好 0.5″（12.70 mm）会落在约 469.5 行，比实测高 2.5 px（1.17 mm）。#6 记的标定松度是 ≤5 px（±2.4 mm），1.17 mm 还在那个松度里；按这套锚点算，不是刚好半英寸。
  - x：暗墨中点 147.93 mm，模型中点 148.34 mm，差 −0.4 mm ≈ −0.9 px。墨迹 136.11..159.74 mm，几何端点 135.64..161.04 mm，虚线没铺满整段。x 零偏仍在 1 px 内。
  - 节距：三段划 12 / 13 / 14 px = 5.67 / 6.14 / 6.62 mm（均 **6.14 mm**），两段空都是 6 px = **2.84 mm**。比约 2.16，离文件 3.5∶1.75 = 2∶1 大约 1 px。对文件尺寸的放大约 1.75（划）和 1.62（空），两者也差约 1 px。先前的 ~6.6 / 2.4 是最长那一划和估短了的空白。单一比例说得通，数值仍不是线宽笔画那个 +24。
  - debug OCS 不在盘上（`D:\Rust\target\debug\OpenCADStudio.exe` 没有），REST `/api/v1/capture` **没跑**。OCS 的 209.61 mm 来自导入器断言，不是新截图。` Line2.sym` 只放置一次（sheet 119）；计划里的 sheet 506 是模板 sheet 501 上的图层号，不是第二个页面放置。
- 2026-10-10 S3（只写 §3，未改码）：减 `SymbolInformation.extents` 已否定，L-D1 的 (a)/(b) 这一条不做；(c) 未勾，11.53 mm 还没有对应字段。L-D2 先不减。L-D3 的「同线宽 #4 的 view-scale」未坐实，本单不改节距代码。
- 2026-10-11 S3 收口（只写本节与 §3，并在 #6 README 追加订正；未改码）。用户 2026-10-11 拍板：t-63（T4）通过，t-62 的 IDA 上游追查撤单（前提不存在），本段替代原 t-64，不再等 IDA。
  - T4，`docs/evidence/2026-10-11-line2-reference-calibration/README.md`：参考图不是各向同性的。x 约 2.117 px/mm（拟合 `col = 33.584 + 2.11691 · mm`），y 为 1.997 px/mm（`mm = 445.8393 − 0.500729 · row`）。14 个锚点，最大残差 0.425 mm（0.85 px）。` Line2` 墨迹质心行 471.51 → **y = 209.74 ± 1.42 mm**。相对 OCS 断言 **209.61 mm**，差 **+0.13 mm**。12.70 mm 与 11.53 mm 都不在这个界内。旧的 11.53 mm = 锚点错 6.7 mm + 把 x 比例用到 y 上错了 4.9 mm。
  - T1，`docs/evidence/2026-10-11-line2-fresh-capture/README.md`：debug OCS（Vulkan + REST `/api/v1/capture`）新鲜截图，不是导入器断言。Line2 纯黑芯在第 708–710 行，中心 py = 709.5 → **209.617 mm**，比断言 209.61 mm 高 0.007 mm（0.23 px）。对 T4 的 209.741 mm，差 −0.124 mm。x 端点 135.64..161.04 mm 落在墨迹内，中点差 0.45 px；外缘多出的 1–2 px 是笔画栅格。Manifold 弧心 Δy = −0.003 mm（OCS 203.010 mm，参考图端帽质心行 484.95 → 203.013 mm），Δx = +0.044 mm（左弧心）/ −0.049 mm（右弧心）。轴线两边都没有墨。壳高 OCS 71.16 mm、参考图 71.15 mm。
  - T2，`pid-parse/docs/analysis/2026-10-11-line2-offset-field-hunt.md`：**命中字段：无。** 已倒 `igSymbol2d` 放置、`JFlavorHolder`（`"Sheets"` 后的 0x86 对上实例 `JSheet` 119 的 payload +4，是 sheet 字不是毫米）、`JSheet` 119 与 501、`SymbolInformation`。按米没有任何字段落在 11.53 mm 或 12.70 mm 的 ±0.5 mm 内。模板 sheet 501 payload +184 的 f32 = 0.5，按英寸 × 25.4 = 12.70 mm，离 T4 的 +0.13 mm 约 12.6 mm，不在 ±1.42 mm 界内；实例 sheet 119 没有这个 0.5。六条 `cargo run --example` 都是 exit 0。
- 待办：S4 / S5 不做（没有 y 偏差要修）。S6 节距照旧只登记，不改码。S7 提交与回链 issue #3 由用户做。解析和渲染代码没动。

可直接贴到 GitHub `happyrust/pid-cad#3` 的收口评论：

> y 偏差收口：不是 bug，是参考图标定伪影。14 锚点重标定（`OpenCADStudio/docs/evidence/2026-10-11-line2-reference-calibration/README.md`）给出参考图 ` Line2` y = 209.74 ± 1.42 mm，OCS 209.61 mm，差 +0.13 mm。0 在界内，此前的 11.53 mm 和 0.5″（12.70 mm）都不在。原因是 #6 把 x 的 2.116 px/mm 用到了 y 上（y 实际 1.997 px/mm），并且那个锚点本身偏了 6.7 mm。
>
> OCS 新鲜截图（`OpenCADStudio/docs/evidence/2026-10-11-line2-fresh-capture/README.md`）量到 Line2 y = 209.617 mm，与导入器断言差 0.007 mm；Manifold 对参考图弧心 Δy = −0.003 mm，Δx = +0.044 / −0.049 mm。文件侧（`pid-parse/docs/analysis/2026-10-11-line2-offset-field-hunt.md`）没有对得上 11.53 mm 或 12.70 mm 的字段。
>
> 决定：L-D1 (c) 两仓都不改；L-D2 不减、无需；节距（x 向，参考图约 6.14 / 2.84 mm）本单只登记、不改码。不提交代码。
