# 性能基准记录 — perf-open-baseline

> 数据来源：第一~六轮取自应用自带的开档分段耗时（`src/app/update/file.rs:531`），开档后从命令行
> 浮层抄录——该浮层的历史行 3 秒后淡出，得用连拍截屏抓。第七轮起改用 `examples/open_bench.rs`
> 无界面重放同一条 loader 线程流程并打到 stdout，两种口径已交叉验证一致（见第七轮）。

## 环境

| 项 | 值 |
|----|-----|
| 日期 | 2026-07-26 |
| 提交 | 第一~六轮：`fe49cc8e`（acadrust `bee1a58`）；第七轮：`34d0c377`（acadrust `8cc4793`） |
| 构建 | `cargo build --release --bin OpenCADStudio` |
| 二进制 | `D:\Rust\target\release\OpenCADStudio.exe`（由 `CARGO_TARGET_DIR` 决定） |
| rustc | 1.99.0-nightly (0e29c21d9 2026-07-21) |
| CPU / RAM | AMD Ryzen 9 7950X (16C/32T) / 64 GB |
| OS | Windows 11 26200 |
| 采集方式 | 第一~六轮：`--new-instance <file>` 冷启动 + 连拍命令行浮层；第七轮起改用 `examples/open_bench.rs`，直接打 stdout |

## 样例文件

| ID | 路径（相对 `../ACadSharp/samples`） | 大小 | 实体数 | 含 XREF |
|----|------|------|--------|---------|
| S | `aec_objects/AecObjects.dwg` | 414 KB | 15 | 否 |
| B | `sample_base/sample_base.dwg` | 1.0 MB | 341 | 否 |
| V\* | `sample_AC10xx.dwg`（7 个格式版本，同一图纸） | 1.0–1.3 MB | 341 | 否 |
| D | `sample_AC1032_ascii.dxf` | 2.5 MB | 341 | 否 |

**覆盖缺口**：本机没有 5–20MB 量级的真实图纸，也没有含外部参照的样例，
所以下表 `xref` 列全为 0，XREF 后台化的收益**尚未被这批数据验证**。

## 第一轮：`fe49cc8e` 原始四段计时（2026-07-26）

| 样例 | 实体数 | parse | purge | caches | xref | total | 未被任何阶段覆盖的差额 |
|------|--------|-------|-------|--------|------|-------|------|
| S `AecObjects.dwg` | 15 | 9 | 0 | 0 | 0 | 30 | 21 |
| B `sample_base.dwg` | 341 | 272 | 0 | 6 | 0 | 627 | 349 |
| D `AC1032_ascii.dxf` | 341 | 12 | 0 | 6 | 0 | 351 | 333 |

### DWG 格式版本矩阵（同一张图，341 实体）

| 版本 | 文件 | 大小 | parse | purge | caches | xref | total |
|------|------|------|-------|-------|--------|------|-------|
| R14 | `sample_AC1014.dwg` | 1.3 MB | **12 / 9** | 0 | 5 / 6 | 0 | 334 / 347 |
| R2000 | `sample_AC1015.dwg` | 1.2 MB | **11** | 0 | 6 | 0 | 336 |
| R2004 | `sample_AC1018.dwg` | 1.0 MB | **10** | 0 | 6 | 0 | 361 |
| R2007 | `sample_AC1021.dwg` | 1.0 MB | **12** | 0 | 6 | 0 | 358 |
| R2010 | `sample_AC1024.dwg` | 1.1 MB | **263** | 0 | 6 | 0 | 598 |
| R2013 | `sample_AC1027.dwg` | 1.0 MB | **261** | 0 | 6 | 0 | 607 |
| R2018 | `sample_AC1032.dwg` | 1.0 MB | **262 / 256** | 0 | 6 | 0 | 602 / 603 |

斜杠分隔的是同一样例的两次独立冷启动，重复性在 ±6ms 以内。

## 第二轮：加上 `prepare_ms` 之后（同机同日）

给 `OpenTimings` 补了 `prepare_open_geometry` 的计数器后重测，摘要行变成
`parse · purge · xref · caches · prepare · total`：

| 样例 | 实体数 | parse | purge | xref | caches | **prepare** | total | 未覆盖差额 |
|------|--------|-------|-------|------|--------|---------|-------|------|
| S `AecObjects.dwg` | 15 | 8 | 0 | 0 | 0 | **0** | 30 | 22 |
| B `sample_base.dwg` | 341 | 315 | 0 | 0 | 7 | **417** | 750 | 11 |
| V `AC1014.dwg` r1 | 341 | 10 | 0 | 0 | 6 | **344** | 371 | 11 |
| V `AC1014.dwg` r2 | 341 | 11 | 0 | 0 | 6 | **314** | 336 | 5 |
| V `AC1014.dwg` r3 | 341 | 11 | 0 | 0 | 6 | **314** | 339 | 8 |
| V `AC1032.dwg` r1 | 341 | 381 | 0 | 0 | 7 | **465** | 864 | 11 |
| V `AC1032.dwg` r2 | 341 | 255 | 0 | 0 | 6 | **321** | 592 | 10 |
| V `AC1032.dwg` r3 | 341 | 249 | 0 | 0 | 5 | **306** | 572 | 12 |

每组的 r1 都偏高（重新构建后首次运行，OS 文件缓存与 CPU 频率都没热起来），r2/r3 才是稳态。
补上计数器之后，**未覆盖差额从 320–350ms 降到 5–12ms**，摘要行现在能解释 97% 以上的开档耗时。

## 第三轮：把 `prepare` 再拆成 wires / index

摘要行进一步细化为 `prepare Xms (wires Yms, index Zms)`：

| 样例 | parse | caches | prepare | └ wires | └ index | total |
|------|-------|--------|---------|---------|---------|-------|
| `AC1014.dwg` | 10 | 6 | 321 | **320** | 0 | 346 |
| `AC1032.dwg` | 311 | 5 | 330 | **326** | 3 | 650 |

交互索引构建几乎免费（0–3ms），`prepare` 就等于 `model_tile_wires_arc` 这一次线框 tessellation。

## 第四轮：同一进程内连开两个文件（关键实验）

一次启动传入两个文件（`AC1014.dwg` 与 `AC1015.dwg`，同一张图的两个格式版本，都是 341 实体）：

| 顺序 | 文件 | parse | caches | prepare | └ wires | └ index | total |
|------|------|-------|--------|---------|---------|---------|-------|
| 第 1 个 | `AC1014.dwg` | 10 | 6 | **312** | 312 | 0 | 342 |
| 第 2 个 | `AC1015.dwg` | 7 | 6 | **26** | 22 | 2 | **50** |

**完全相同的 tessellation 工作量，第二次只要 22ms。**
也就是说所谓的「开档慢」里约 **290ms 根本不是开档成本，而是一次性的进程级预热**，
只是恰好被记在了第一个打开的文件头上。第二个文件的完整开档只要 50ms。

## 第五轮：启动时后台预热字体子系统之后

在 `main` 里加了 `scene::text::warm_up_fonts()`，用后台线程在窗口/GPU 初始化期间把
LFF 笔画字体、系统字体库、cosmic-text 字体系统三个 `OnceLock` 提前初始化。
预热线程自身耗时（`--log info` 打出）：

```
font warm-up: lff 78ms, sysfont 20ms, cosmic-text 21ms
```

首次开档对比（同机同样例）：

| 样例 | | parse | caches | prepare | └ wires | total |
|------|---|-------|--------|---------|---------|-------|
| `AC1014.dwg` | 改前 | 10 | 6 | 321 | 320 | 346 |
| `AC1014.dwg` | **改后** | 9 | 6 | **199** | 198 | **226** |
| `AC1032.dwg` | 改前 | 311 | 5 | 330 | 326 | 650 |
| `AC1032.dwg` | **改后** | 263 | 6 | **213** | 210 | **490** |

首次开档快了约 120ms，正好等于预热线程的 119ms——预热确实把这部分挪出了关键路径。

## 第六轮：剩下那 165–200ms 是什么（归因实验）

用改后的版本一次启动传入两个文件，比较「内容相同」与「内容不同」两种顺序：

| 实验 | 第 1 个文件 | prepare | 第 2 个文件 | prepare |
|------|------------|---------|------------|---------|
| 内容相同 | `AC1014.dwg` | 200ms | `AC1015.dwg`（同一张图） | **25ms** |
| 内容不同 | `export_sample.dwg` | 29ms | `AC1014.dwg` | **165ms** |

关键在第二行：`export_sample.dwg` 已经先把进程跑热了，`AC1014.dwg` 依然要 165ms。
**所以剩余开销是按图纸内容计的，不是全局懒加载**——预热能拿掉的部分已经拿完了。

这批图纸确实是文字密集型（`AC1014.dwg` 的 341 个实体里有 36 MText + 29 Text +
15 MultiLeader + 11 Dimension + 5 属性定义 + 3 形位公差 + 2 表格，约 100 个带文字的实体），
剩余成本落在按 `(font, text)` 缓存的字形整形与轮廓提取上——同内容再开命中缓存，换一张图就得重算。
要再压缩就得真正优化文字 tessellation 路径（例如并行化字形轮廓提取，或把缓存粒度从
「整段文本」下沉到「单字形」），已超出本 goal 的范围。

## 第七轮：合并上游 4 个提交后的回归（2026-07-26）

上游 `fe49cc8e..34d0c377` 四个提交里有三个动了开档相关路径
（`spline_tess.rs` +308、`solid3d_tess.rs` +119、`acis_to_truck.rs`、`tessellate.rs`、
`wire_arena.rs`、`pipeline/mod.rs`、`view/render.rs`），且 `a22681e7 fix(io): prevent lossy Save As`
把 acadrust pin 从 `bee1a58` 挪到了 `8cc4793`，因此重测。

**采集方式改了**：不再截屏抄数字。`examples/open_bench.rs` 在进程内按 loader 线程的顺序重放
`load_file → purge → xref → build_derived_caches → prepare_open_geometry`，把同样的分段耗时打到
stdout。命令行浮层里那一行仍然保留，但回归不再依赖它。

```
cargo build --release --example open_bench
D:\Rust\target\release\examples\open_bench.exe <file> [more files...]
OCS_BENCH_COLD=1   # 跳过启动预热，用来量预热本身值多少
```

| 样例 | parse | purge | xref | caches | prepare | └ wires | └ index | total |
|------|-------|-------|------|--------|---------|---------|---------|-------|
| `AC1014.dwg`（单跑） | 13 | 0 | 0 | 7 | 196 | 196 | 0 | 217 |
| `AC1032.dwg`（单跑） | 283 | 0 | 0 | 6 | 213 | 210 | 2 | 503 |
| `AC1014.dwg`（两文件之第 1 个） | 9 | 0 | 0 | 6 | 199 | 198 | 0 | 215 |
| `AC1015.dwg`（两文件之第 2 个） | 8 | 0 | 0 | 6 | **24** | 21 | 2 | **39** |
| `AC1014.dwg`（`OCS_BENCH_COLD=1`） | 9 | 0 | 0 | 6 | **301** | 300 | 0 | 317 |

**结论：上游这批提交没有让开档变慢。** 与第五轮 GUI 采集的数字逐项对齐——
`AC1014` prepare 199→196、total 226→217；`AC1032` prepare 213→213、total 490→503。
差异都在 ±13ms 的重复性范围内，而且两种采集方式互为交叉验证：
新的无界面口径能复现旧的 GUI 口径，说明历史基准数据可以继续沿用。

第四、六轮的两个结论也一并复现：同进程第二个文件的 prepare 只要 24ms（第一个 199ms）；
关掉预热后 prepare 从 196ms 涨到 301ms，预热确实值约 105ms。

预热自身耗时随 OS 文件缓存状态波动：当天第一次跑 `sysfont` 要 142ms（总 227ms），
之后稳定在 20ms 左右（总 103–106ms）。

## 两个值得追的结论

### 1. 那 250ms 是**一个表格实体**，而且它同时被解析错了

最初的观察是「R2010+ 比 R2007 慢 20 倍」（同图 parse 从 ~11ms 跳到 ~262ms，分界点在 AC1024）。
但这个归因是错的——**跟 DWG 版本没关系**。对照组：`export_sample.dwg`、`AecObjects.dwg`、
`geoloc.dwg`、`BLOCKPOINTPARAMETER.dwg` 全都是 AC1032（R2018），decode 分别只要 1.2 / 2.4 / 0.4 / 0.4ms。

用逐记录计时（把 acadrust 源码复制到临时目录改了几行，未动仓库）定位到罪魁：

```
[slowrec] handle=0x528 type_code=-16  283.3ms      ← ACAD_TABLE
[slowrec] handle=0xD65 type_code=38    11.2ms
[perf] dwg-build pass2=284.1ms decode=283.6ms records=731 threads=32
```

`type_code = -16` 就是 `OBJ_TABLE`（`object_reader/common.rs:124`）。**731 条记录里，一条表格
记录独占 283ms，其余全部加起来不到 3ms。**

再看解析结果，问题不止是慢：

| 同一张表 `handle=0x528` | rows | columns | cells |
|---|---|---|---|
| R2007 路径（`sample_AC1021.dwg`） | 7 | 3 | **21** ✓ |
| R2010+ 路径（`sample_AC1032.dwg`） | 7 | 3 | **9** ✗ |

一张 7×3 的表在 R2010+ 路径上只读出 9 个单元格。**解析器走偏了**，然后在
`safe_count`（`object_reader/mod.rs:29`，上限 `MAX_ARRAY_COUNT = 100_000`）兜住的循环里
空转掉那 283ms——记录之间按 offset 定位，所以它错了也不崩，只是又慢又丢数据。

定位位置：`src/io/dwg/dwg_stream_readers/object_reader/entities.rs` 的 `read_table` /
`read_table_content` R2010+ 分支（`entities.rs:2794` 起）。

**连带风险**：用 OCS 打开再另存，表格会按错误的 9 个单元格写回去——重存一次就掉数据。
（这也解释了为什么「删掉某类实体再另存」的对照实验全部变快：不是删对了东西，
而是只要经 OCS 的写出器走一遍，表格就被换成了它自己能快速读回的形态。）

后续动作：这是 `acadrust` fork 的 bug，修它需要对着 ODA 规范核 R2010+ 的表格内容位流。
收益是双份的——每个含表格的 R2010+ 图纸省几百毫秒，同时修掉一处静默的数据丢失。

**2026-07-26 复验：pin 升到 `8cc4793` 后这个 bug 依然在。** `a22681e7 fix(io): prevent lossy Save As`
把 acadrust 从 `bee1a58` 换成了 `8cc4793`（"verified DWG round-trip, I/O, and unified PERF fixes"），
但用 `examples/table_probe.rs` 直接读同两个样例，结果和换 pin 之前一样：

```
sample_AC1021.dwg (R2007) ->  15.6ms   TABLE 0x528 rows=7 columns=3 cells=21  ✓
sample_AC1032.dwg (R2018) -> 263.4ms   TABLE 0x528 rows=7 columns=3 cells=9   ✗
```

所以 [`acadrust-r2010-table-bug.md`](./acadrust-r2010-table-bug.md) 仍然成立，可以直接提交上游。

**2026-07-27 根因已定位并修复，全样本矩阵实测。** 根因是 `read_cad_value`
（`entities.rs:2424`）丢了 ACadSharp/ODA 对 R2007+ 值体的 `IsEmpty`（`Flags & 1`）门：空值在流里
没有值体，无条件读就过读，随后的 `Units/Format/FormattedValue` 全部错位，下一格读到垃圾 count 后
空转。修复见报告。在本地 fork 副本上 release 实测（同一张图的七种存法，每份都是 341 实体、两张表；
`0xA35` 全程 20 格不受影响）：

| 样例 | 版本 | `0x528` 格数 前→后 | 耗时 前→后 |
|---|---|---|---|
| `sample_AC1014.dwg` | R14 | 21 → 21 | 8.5ms → 10.0ms |
| `sample_AC1015.dwg` | R2000 | 21 → 21 | 5.4ms → 5.3ms |
| `sample_AC1018.dwg` | R2004 | 21 → 21 | 7.2ms → 6.8ms |
| `sample_AC1021.dwg` | R2007 | 21 → 21 | 6.6ms → 7.2ms |
| `sample_AC1024.dwg` | R2010 | **9 → 21** | **271.5ms → 7.0ms** |
| `sample_AC1027.dwg` | R2013 | **9 → 21** | **262.2ms → 6.0ms** |
| `sample_AC1032.dwg` | R2018 | **9 → 21** | **261.5ms → 6.4ms** |

修正上面几轮的一处认知：受影响的不只是 R2018，**三个 R2010+ 样例全都是 9 格 / ~265ms**——
之前只测了 R2007 与 R2018 两份，才显得像是「R2018 特有」。R2010 以前的四份格数与耗时都没变化，
说明这个门在旧路径上是惰性的。验证 crate：`../acadrust-tablecheck/`。

### 0. 结论先行：首次开档的成本分成两半，已经砍掉可砍的那一半

第四轮发现同一进程里第二个文件的 `prepare` 只要 22ms（第一个 312ms），说明首次开档背着
一笔一次性成本。第五、六轮把它拆成了两部分：

| 成分 | 量级 | 性质 | 处置 |
|------|------|------|------|
| 字体子系统懒加载 | ~120ms | 进程级，与图纸无关 | **已解决**：启动时后台预热 |
| 该图纸的字形整形与轮廓提取 | 165–200ms | 按内容计，同内容才命中缓存 | 需要真正优化文字 tessellation |

意外之处是量级分布：真正的系统字体扫描只有 41ms（sysfont 20 + cosmic-text 21），
反而是**解析内置 LFF 笔画字体花了 78ms**——这部分纯 CPU 解析，本来就该在启动时做掉。

净效果：`AC1014.dwg` 首次开档 346ms → 226ms，第二个文件依旧是 ~47ms。

### 2. 开档耗时的真正大头是 `prepare_open_geometry`（已补计数器并证实）

第一轮里 `parse + purge + caches + xref` 在 R14 样例上合计只有 ~18ms，而 `total` 是 334ms，
有 320–350ms 无人认领。补上 `prepare_ms` 后确认：这笔开销全在 loader 线程最后那步
`prepare_open_geometry`（`src/io/mod.rs:187`，模型线框预 tessellation + 交互索引构建）。

稳态数据：341 个实体的图纸 prepare 为 **306–344ms**，15 个实体的图纸为 0ms。
在 R14 样例上 prepare 占 `total` 的 **93%**，parse 只占 3%——
也就是说，此前围绕 parse / purge / xref 的所有优化讨论，针对的都不是主要矛盾。

约 1ms/实体的单价对普通几何来说偏高，最可疑的是文字路径
（SHX/TTF 字形首次加载与 SDF atlas 构建，见 `src/scene/text/`），这批样例含大量标注与文字。
下一步应把 prepare 内部再拆一层（或对这一步埋 puffin span），
确认到底是字形加载、tessellation 还是交互索引占大头。

## 备注

- `purge` 在所有样例上都是 0ms，说明去掉第二次全量 purge 的收益在小图上不可测；
  要验证需要真正的大图（数万实体）。
- `total` 的口径是 UI 侧从开档开始到 `FileOpened` 处理完，不含首帧渲染。
- 这批数字只能作为**小图基线**。拿到 5–20MB 的真实图纸后需要重测，届时各阶段占比可能完全不同。
- 采集脚本（冷启动 + 连拍浮层）留在 `C:\Users\dpc\AppData\Local\Temp\ocs_bench\run.ps1`，
  不入库；要复现直接按上面「采集方式」一节重建即可。
