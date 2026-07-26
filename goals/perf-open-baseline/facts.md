# Facts — 开档性能基线与首轮优化

> 每条 fact 应可测试/可验收。`[V]` 表示推荐自动化或脚本化验证。
> 本文件于 **2026-07-26** 按 `origin/main`（`fe49cc8e`）重新核对；带 ~~删除线~~ 的是已失效的旧事实。

## 环境与构建

1. **[V]** Release 二进制在 Windows x64 上可启动且不栈溢出；debug 亦可运行，`.cargo/config.toml`
   给 msvc target 加了 `/STACK:67108864`，debug 与 release 都吃这个链接参数。
2. ~~`.cargo/config.toml` 固定 `target-dir = E:/cargo-target/OpenCADStudio`~~
   **已失效**：config 里没有 `[build] target-dir`，产物位置由环境变量 `CARGO_TARGET_DIR` 决定；
   本机为 `D:\Rust\target`，release 产物落在 `D:\Rust\target\release\OpenCADStudio.exe`。
3. 项目无 `.planning/` GSDD 目录；本 goal 包位于 `goals/perf-open-baseline/`，不引入外部规划框架依赖。

## 当前开档数据流（代码事实，2026-07-26 核对）

4. **[V]** `io::open_path_with_phase`（`src/io/mod.rs:100`）在名为 `ocs-file-open` 的专用线程上按顺序执行：
   `load_file_with_progress` → `purge_corrupt_entities`（:136）→ `resolve_xrefs_with_progress`（:155）
   → `build_derived_caches_with_progress`（:175）→ `prepare_open_geometry`（:186）。
5. ~~UI 线程 `FileOpened` 处理器再次调用 `resolve_xrefs` 与第二次 `purge_corrupt_entities`~~
   **已修复**：`FileOpened`（`src/app/update/file.rs`）只消费后台线程产出的 `caches.xrefs` 打日志，
   既不解析 xref 也不做第二次 purge。全文档 purge 在整条开档路径上只发生一次。
6. XREF 块合并通过 `xref::merge_xref_into_block` 写入 host 文档；xref 内容可能含 parser junk，
   其 corrupt 计数单独汇总为 `caches.xref_dropped`，在命令行以独立 warning 打印（`file.rs:448`）。
7. ~~开档阶段 UI 仅显示 4 个 phase 标签，尚无 XREF 专用 phase~~
   **已实现**：`app/mod.rs:179–183` 定义 READING=0 / PARSING=1 / **XREF=2** / CACHING=3 / FINALIZING=4，
   `ui/window/open_progress.rs:24` 对应标签 `"Loading references…"`。
8. ~~caches 在 purge 之后、xref 之前构建~~
   **已修正为正确顺序**：purge → xref → caches（`io/mod.rs:136 / 155 / 175`），
   caches 反映的是合并 xref 之后的文档。
9. **[V]** 首帧代价已大幅前移：loader 线程在 `prepare_open_geometry`（`io/mod.rs:186`）里预先完成模型
   线框 tessellation 与交互索引，结果放进 `caches.prepared_geometry`，UI 首帧不再整图重建。
10. **[V]** `scene::OpenTimings`（`src/scene/mod.rs:408`）只有四个字段：`parse_ms` / `purge_ms` /
    `caches_ms` / `xref_ms`。**没有 first_frame 字段**——命令行里的 `total` 是 UI 侧从点击 Open
    到 `FileOpened` 处理完的 wall time（`file.rs:528`），不等于首帧可交互时间。
11. **[V]** 开档完成后命令行打印两行：`Opened "x.dwg" — N entities`（`file.rs:441`）与
    `  parse Xms · purge Yms · caches Zms · xref Wms · total Tms`（`file.rs:531`）。
    这两行只进应用内命令行面板，**不写 stdout / 日志文件**，所以基准采集需要截屏或人工抄录。
12. `Cargo.toml` 中**没有** puffin/puffin_http 依赖，也没有 `profile` feature——span 级剖析尚未接入。

## 规划来源（产品事实）

13. ~~ROADMAP Phase 5 必须先于 Phase 1–4~~
    `ROADMAP.md` **已被上游删除**（commit `99f1a267 remove perf roadmap`；此前 `ebae6716`
    已把完成项折叠、去掉 1.2 条目）。本包不再引用其章节号；「先量后改」的原则仍然保留。

## 验收行为

14. **[V]** 含外部参照的大文件打开时，进度 overlay 在 XREF 阶段仍更新，主窗口可响应（不长时间卡死）。
15. XREF 手动重载（`XATTACH` / `insert/xattach.rs:119`）走 `xref.rs` 同一套公开 API，与自动开档解析一致，不回归。
16. **[V]** 基准数据必须注明构建 profile：debug 与 release 的解析耗时差一个数量级，混用会让基准失去意义。

## 风险与约束

17. `acadrust` 来自 patched git fork（`Cargo.toml` patch）；不在本 goal 改 parser API。
18. 本机可用样例仅 `../ACadSharp/samples`（最大 1.3MB DWG / 2.7MB ASCII DXF），
    **缺少 5–20MB 量级与含 XREF 的真实样例**；基准表需如实标注这一覆盖缺口。
19. 无 CI 性能门禁时，至少保留手工基准记录（样例文件名 + 构建 profile + 耗时）。
