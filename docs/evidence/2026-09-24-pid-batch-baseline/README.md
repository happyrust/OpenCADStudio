# `.pid` 批量回归基线（2026-09-24，计划 `2026-09-24-pid-import-status-and-next-steps.md` B1）

`report.csv` 是 `examples/pid_batch_report.rs` 在 pid-parse `test-file/` 上跑出来的一份：六张 `.pid`（四张主图、publish 目录的 A01 与
DWG-0202 副本），每张一行——`load_pid` 的导入摘要（实体 / 记录 / 没画 / 缓存与库本体 / 关闭层笔画 / 图纸图层 / 压平文字 / 样式表是否读出）、
同一次解析的拒收与无解码器记录按类型码计数、单位、页幅，以及照 `--export` 的路另存 DXF 的 SHA-256。

- 基线：OCS `72ae8f42` + pid-parse `686c9d5`（`DependencyObject` 按成员数校验之后，0201 的 `missing` 为 0），debug 构建。四张主图的 `dxf_sha256` 与计划里 T2 的新基线逐一相同；
  publish 目录那张 DWG-0202 旁边带 `_Data.xml`，语义进了 XDATA，所以哈希与 `test-file/` 那张不同。
- **2026-09-28 重签**（OCS `dbe8ffd8` 的干净 worktree + pid-parse `a68bf55`，计划 `2026-09-28-pid-import-next-steps.md` P-D12）：pid-parse 不再解码类型字带 `0x8000`
  的记录（原生读取器跳过的、文件已退役的旧副本），OCS 代码未动。变的三张：DWG-0202 `drawn` 313 → 309 / `decoded` 179 → 175（`/Sheet6615` 孤儿存储的四条边线）、
  A01 126 → 122 / 116 → 112（`/JSite204/Sheet6` 四条线）、工艺 686 → 578 / 427 → 399 / `cache_bodies` 58 → 31 / `hidden_strokes` 50 → 24（27 个退役放置副本 + 一条旧管道号）；
  D06 与 DWG-0201 的哈希一字不变（0201 的五份页框副本本来就按范围去重）。改前 / 改后的 DXF 逐实体比（去掉句柄类组码）：**改后是改前的子集**，没有一个实体新增或挪动，头变量除 `HANDSEED` 外相同；
  去掉的是 0202 四条 `PID-GEOMETRY` 线（该层随之不再建，图层表 15 → 14 条）、A01 四条 `Default` 线、工艺 80 条 `Default` 线 + 27 条 `PID-SYMBOL-LABEL` 名字 + `Labels` 上那条 `250-LNG-57602- - `。
- **2026-10-10 重签**（OCS `main` `aaedd334` 的干净工作树 + pid-parse `931ddca`，计划 pid-parse `docs/plans/2026-10-10-an-override-applies-its-own-width-colour-and-dash.md` O2）：pid-parse 让 `JStyleOverride` 自带的宽 / 色 / 虚线位压过基样式
  （0201 设备表 14 条格线从虚线变 0.13 / 0.18 mm 实线，与 SmartPlant 截图一致），OCS 代码未动。六行哈希全变，两层原因要分开看：
  1. **`main` 的基线自 09-28 起就没再签过**：10-09 合入上游后 codec 多写 `$XCLIPFRAME` / `$DWFFRAME` / `$DGNFRAME` 三个头变量（`pid-premerge` 分支 `b7029d48` 的彩排提交记录了同一件事），A01 行的 `drawn` 122 → 124 是 09-30 椭圆弧那单（E2 在 `pid-web-open` 上重签、没进 `main`）。
     所以先用改前的 pid-parse（`1ed140a` 的 `style_link.rs` / `symbol_library.rs`）在同一棵树上跑一遍当「改前」，再与改后比。
  2. **改前 → 改后**：17 列逐列相同；DXF 逐实体比（去 `5` / `105` / `330` / `340` / `350` / `360` 句柄类组码）六张的实体数一个不变（60 / 335 / 310 / 125 / 310 / 579），实体上**只有**线宽 `370`、颜色 `62` / `420`、线型 `6` 三类组码变：
     0201 14 条格线去掉 `PID-DASH-1`、宽 35 → 13（11）/ 18（3），3 条坡度标记 `#FE0060` → 黑；0202 8 条格线 + 阻火呼吸阀 8 笔（含 LWPOLYLINE 唇）去虚线，一个放置的 25 笔 `50 #0000FF` → `35 #800000`，其余 35 → 13 / 18；
     A01 46 条线 35 → 50（37）/ 70（9）；工艺 45 笔本体（9 圆 + 36 线）去掉 `PID-DASH-1`、6 条线 35 → 13；D06 的实体一字不变。
     LTYPE 表随之：D06 / A01 / 工艺各少一条再无人引用的 `PID-DASH-*`（`$HANDSEED` 随之变），0201 的两条线型互换编号——电信号线的 1.905 / 1.27 mm 现在是 `PID-DASH-1`，3.5 / 1.75 mm 只剩 `/JSite396` 本体那条 0.01 mm 发丝线在用。
- **2026-10-10 重签（标签锚点十字）**（OCS `main` `82f8684b` 之后的工作树 + pid-parse `1365f0e`，计划 `docs/plans/2026-10-10-a-label-placement-keeps-its-anchor-cross-off-the-sheet.md` L5）：标签类放置（库路径在 `Labels` / `Labels - …` 目录，或本厂的 `Design\Drawing Description.sym`）
  的缓存本体只是它被锚定的那个 0.01 mm 发丝十字，SmartPlant 屏幕不画，导入也不再画（不画本体、不退库、不落标记点；名字照写）。pid-parse 未动。变的**四行**、变的列只有 `drawn` / `cache_bodies` / `hidden_strokes` / `layered_entities` / `dxf_sha256`：
  0201 `drawn` 334 → 326 / `cache_bodies` 20 → 16 / `hidden_strokes` 31 → 26 / `layered_entities` 309 → 301；0202 与它的 publish 副本 309 → 299 / 23 → 18 / 26 → 20 / 309 → 299；工艺 578 → 543 / 31 → 19 / 24 → 12 / 578 → 543；D06 与 A01 一字不变。
  改前 / 改后 DXF 逐实体比（去 `5` / `105` / `330` / `340` / `350` / `360` 句柄类组码）：**改后是改前的子集**，少的只有 `Default` 层上的 LINE——0201 8 条（四个十字，中心 `(474.68, 304.32)` / `(479.88, 246.32)` / `(394.45, 244.00)` / `(340.79, 245.55)`，臂长 1.27）、0202 10 条（两份各 10，五个十字，放置样式 0.50 mm）、工艺 35 条（11 个 `Remarks` 各三笔 + `Xa Item Note & Label` 两笔）；
  头变量除 `$HANDSEED` 外，只有工艺的 `$EXTMAX` y 551.95 → 551.31（`(743.71, 551.95)` 那个 `Remarks` 十字本是图上最高的实体）。
- **2026-10-12 默认倍数仍是 1，哈希不变。** `paper_style_scale` 默认 1.0（V1 `99e6eda4` 已对六张 `dxf_sha256` 与本基线逐哈希相同；V2 只加选项面，不改 `src/io/pid`，也不把 5/3 写进默认值）。本轮没有重签 `report.csv`。
- `import_ms` 只作参考，随机器与负载变。

重跑与对比：

```powershell
cargo run --example pid_batch_report -- ..\pid-parse\test-file --export-dir $env:TEMP\pid-batch > report-new.csv
Compare-Object (Import-Csv docs\evidence\2026-09-24-pid-batch-baseline\report.csv | Select-Object * -ExcludeProperty import_ms) `
               (Import-Csv report-new.csv | Select-Object * -ExcludeProperty import_ms) -Property file, drawn, decoded, missing, refused, undecoded, dxf_sha256
```

新语料来了（B2）：把目录直接交给它，`status` 不是 `ok` 的行、`refused` / `undecoded` 里出现新类型码的行、`unit` 为 `assumed-metre` 或 `page_mm` 为 `-` 的行，
就是下一批要开的单。
