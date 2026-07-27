# 目标：开档性能基线 + 首轮低风险优化

> **状态（2026-07-26 二次复核）**：已合入上游 `fe49cc8e..34d0c377`（4 个提交，含 acadrust pin
> `bee1a58` → `8cc4793`），release 重建通过、GUI 冒烟通过，开档基准无回归——逐项对齐见
> `benchmarks.md` 第七轮。回归采集已脚本化（`examples/open_bench.rs`），不再依赖截屏抄录。
>
> **状态（2026-07-26 复核）**：本 goal 原定的三项代码改动（分段计时、去掉二次 purge、XREF 后台化）
> **已由上游 `origin/main` 实现**，作为拉取 99 个提交的一部分落地；同一批提交里上游还删除了
> `ROADMAP.md`（commit `99f1a267 remove perf roadmap`），因此本包早先对 ROADMAP 章节号的引用已失效。
> 剩余范围收敛为两项：**补齐可复现的开档基准数据**，以及**可选的 puffin span 级剖析**。
> 逐条对照见 [`plan.md`](./plan.md) 各步骤开头的状态标记。

## articulated goal（1–3 句）

在 OpenCADStudio 已能在 Windows 本地稳定构建运行、且开档路径已具备分段计时与后台 XREF 的前提下，
建立**可量化、可复现的开档/首帧性能基线**，使后续优化有据可依；在基准数据证明值得之前，不再追加
新的开档优化改动。

## 共享理解（facts）

见 [`facts.md`](./facts.md)。

## Done 条件

- [x] 打开任意 DWG/DXF 后，命令行输出分段耗时（parse / purge / caches / xref / total）——上游已实现，
      见 `src/app/update/file.rs:531`。
- [x] XREF 解析移出 UI 线程；`FileOpened` 不再二次全量 `purge_corrupt_entities`——上游已实现，
      见 `src/io/mod.rs:136`（唯一一次 purge）与 `src/io/mod.rs:155`（loader 线程内解析 xref）。
- [x] `benchmarks.md` 至少记录一组 release 构建下的实测数据，含样例文件、实体数与各阶段耗时——
      2026-07-26 已填入 12 组冷启动实测（含 7 个 DWG 格式版本矩阵）。
- [x] `OpenTimings` 覆盖 `prepare_open_geometry`——已加 `prepare_ms` 并打进摘要行；
      补上后未被覆盖的差额从 320–350ms 降到 5–12ms，摘要行能解释 97% 以上的开档耗时。
- [x] 把 `prepare_open_geometry` 内部再拆一层——已加 `wires` / `index` 两个计数器，
      定位到全部开销都在首次线框 tessellation（index 只要 0–3ms）。
- [x] **消掉首次开档的一次性预热**——已在 `main` 加 `scene::text::warm_up_fonts()`，
      用后台线程在窗口/GPU 初始化期间加载 LFF / 系统字体库 / cosmic-text。
      `AC1014.dwg` 首次开档 346ms → 226ms。
- [ ] 剩余的 165–200ms 是该图纸自身的字形整形与轮廓提取（归因实验见 `benchmarks.md` 第六轮），
      要再压缩需优化文字 tessellation 本身（并行化轮廓提取，或把缓存粒度下沉到单字形）——
      已超出本 goal 范围，建议另起一个目标包。
- [x] 查清「R2010+ DWG 解析慢 20 倍」的真实原因——**与版本无关**，是 `acadrust` 的 R2010+
      ACAD_TABLE 内容解析器走偏：一条表格记录独占 283ms，同时把 7×3 的表只读出 9 个单元格
      （R2007 路径读出正确的 21 个）。详见 `benchmarks.md` 结论 1。
- [ ] 修 `acadrust` fork 的 R2010+ 表格解析。既是性能问题也是数据丢失问题：
      OCS 打开再另存会把表格按错误的 9 个单元格写回。
      报告（含根因、补丁、验证矩阵）：[`acadrust-r2010-table-bug.md`](./acadrust-r2010-table-bug.md)。
      **2026-07-26 复验：上游把 pin 升到 `8cc4793` 之后依然复现。**
      **2026-07-27 根因已定位并修复、验证通过，尚未落地**：
      根因是 `read_cad_value`（`object_reader/entities.rs:2424`）丢了 ACadSharp/ODA 对 R2007+ 值体的
      `IsEmpty`（`Flags & 1`）门——空值在流里没有值体，无条件读就过读，
      使随后的 `Units/Format/FormattedValue` 错位、下一格读到垃圾 count 后空转。
      修复：把空值路由到 `match` 的 no-op 分支；另加独立的健壮性兑底
      `DwgMergedReader::read_bounded_count()`（按剩余位流 clamp，替换 object_reader 里 75 处
      `safe_count(reader.read_bit_long())`）。
      全样本矩阵实测：R2010/R2013/R2018 三个样本**都**从 9 格 / ~265ms 恢复为 21 格 / 6–7ms
      （原报告只测了 R2018，实际三个 R2010+ 样本同样受影响）；R14/R2000/R2004/R2007 四个样本
      格数与耗时均无变化。
      改动只在本地副本 `../acadifc-fork/`（未推未提交），完整 diff 见 `../acadifc-fork.changes.diff`，
      验证 crate `../acadrust-tablecheck/`（`cargo run --release`）。
      **剩余动作**：把修复推到 `OpenAEC-Foundation/acadifc`（或改本地 path patch）并把 OCS
      `Cargo.toml` 的 `[patch]` rev 指过去。
- [ ] 核实正确性普查里尚未定性的分歧——同一张图的 R2007 与 R2018 两条读取路径，330 个实体里有
      34 个字段级不一致，但**逐个验证后大多是两种格式的存法不同，不是缺陷**：
      23 个实体的 EED 差异是信息从 EED 挪进了原生字段；Spline 那条是同一条曲线的两种编码
      （R2018 的两个拟合点正是 R2007 四个控制点的首尾，节点向量 `[0,0,0,0,1,1,1,1]` 就是一段三次
      Bezier）；Text 的 `"0 @ 1"` 是 R2007 文件里真有这个兼容图层（AutoCAD 为表达视口图层替代而生成）。
      **真正确认的缺陷只有表格一条。** 仍待核实：MultiLeader 的 `text_attachment_point`（14 个实体）、
      `wireframe_isolines` 读出负数、多行属性字段。
      清单与验证状态见 [`acadrust-version-path-diff.md`](./acadrust-version-path-diff.md)。
- [ ] （可选）`--features profile` 下用 puffin 看 span——只在上面两条靠读码查不出结论时才做。

## 范围边界

- **做**：基准样例选定与实测记录（原步骤 0/5）；可选的 puffin span（原步骤 2）。
- **已由上游完成、本包不再实施**：原步骤 1（OpenTiming）、步骤 3（去二次 purge）、步骤 4（XREF 后台化）。
- **不做**：单遍实体扫描、磁盘缓存、渲染批处理、acadrust 并行解析——这些留待基准数据驱动。
- **不重构**：`update/` 与 `commands/` 的巨型分发结构；不在本里程碑动 MVU 架构。

## 前置条件与环境事实

- 构建：`cargo build --release --bin OpenCADStudio`。
- 产物路径由环境变量 `CARGO_TARGET_DIR` 决定，当前机器为 `D:\Rust\target`，
  故 release 产物在 `D:\Rust\target\release\OpenCADStudio.exe`。
  **注意**：`.cargo/config.toml` 里已不存在 `[build] target-dir`，早先文档写的
  `E:/cargo-target/OpenCADStudio` 路径在本机不存在。
- `.cargo/config.toml` 本地保留一处改动：msvc target 加 `/STACK:67108864`（深递归的 DWG/场景需要）。
