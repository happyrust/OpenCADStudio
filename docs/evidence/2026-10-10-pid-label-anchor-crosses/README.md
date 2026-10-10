# 标签放置的锚点十字不上图 · GUI 复核（2026-10-10）

对应计划 `docs/plans/2026-10-10-a-label-placement-keeps-its-anchor-cross-off-the-sheet.md`（L-D5 (a)、L6），收口 10-09 对 SmartPlant 参考图 `D:\work\cad\图片验证\pid-1.png` 逐实体比里的 **七条差异 #6「标签锚点十字 `+`：OCS 在 0201 画出 4 个、参考图不显示」**。

基线：OCS `main` `82f8684b` + L1–L5 未提交改动（`src/io/pid/{symbols,build,summary,mod,tests}.rs`、`tests/pid_import.rs`，+505/−49）；pid-parse 以 `path = "../pid-parse"` 依赖（L0 探针 `1365f0e`）。
本机 debug 版 OCS（`D:\Rust\target\debug\OpenCADStudio.exe`，10:09 构建，含上述 L1–L5），Vulkan，AMD Radeon RX590 GME，不走桌面自动化：`OpenCADStudio.exe <0201.pid> --http <port> --backend vulkan`（`OCS_API_TOKEN` 预设），REST `POST /api/v1/capture`（`document_id`、`scope: viewport`、`bounds`/`view: extents`）让程序自己缩放并回读视口；用完 `Stop-Process`。截图时间 10:10–10:15；`src/io/pid/*` 的 mtime 10:43 是之后一次 git/fmt 触碰，diff 内容与上述 L1–L5 一字不差，截图仍代表修复后的码。

**标定**：`pid-1.png` 1335 × 711，图框 x 34..1291.5 px ↔ 0..594.3 mm → **2.116 px/mm**；用设备表左上角 `(472.2, 402.0)` 与「说 明」`(507.7, 333.5)` 校准，误差 ≤ 5 px。参考图裁片用 `System.Drawing` 从 `pid-1.png` 按此标定裁，与 OCS 同窗并排（`sbs-*`）。OCS 修复前把这些十字按放置样式重涂成 0.35 mm 黑，比 SmartPlant 还显眼。

四个锚点十字修复前 OCS 画在（mm → px）：Drawing Description `(474.68, 304.32)`→`(1038, 296)`、Item Note 617 `(479.88, 246.32)`→`(1049, 419)`、Item Note 452 `(394.45, 244.00)`→`(869, 424)`、Item Note line1.5 `(340.79, 245.55)`→`(755, 421)`。

| 文件（`sbs-` 并排，`ocs-`/`ref-` 单窗） | 取景（mm） | 看什么 | 结论 |
|---|---|---|---|
| `sbs-0201-drawing-description.png` | (465, 295)–(485, 315) | oid 262 `Drawing Description` 锚点 | 两边都空，±10 px 窗口**零个暗像素**，无黑 `+` ✓ |
| `sbs-0201-item-note-617.png` | (470, 237)–(490, 255) | oid 617 `Item Note & Label` 锚点 | ±10 px **零个暗像素**；OCS 那格只有本就在的黄色流向箭头与管线，无黑 `+` ✓ |
| `sbs-0201-item-note-452.png` | (385, 235)–(405, 253) | oid 452 `Item Note & Label` 锚点 | 只有本就在的管线/设备像素与标签正文「量油孔」，无黑 `+` ✓ |
| `sbs-0201-item-note-line1.5.png` | (331, 236)–(351, 254) | oid 602 `Item Note & Label line1.5` 锚点 | 只有本就在的管线/设备像素与标签正文「人孔」，无黑 `+` ✓ |
| `sbs-0201-line2-hairline.png` | (130, 200)–(167, 231) | ` Line2.sym` 25.4 mm 发丝虚线（判据对照） | **两边都仍画**发丝虚线 ✓——判据是选择性的：砍标签锚点十字、发丝线照留 |
| `0201-full.png` | `view: extents` | 整图观感 | 图框、管线、设备、仪表齐全，与 10-09 整图同观感 |

**判据对照成立**：四处标签锚点十字在 OCS 全部消失，与参考图并排一致（计划验收 #4）；而非标签的 ` Line2.sym` 发丝线两边照画——证明不是「发丝线不画」，而是「标签放置的锚点不画」。

**顺带（范围外，新登记 #8）**：`sbs-0201-line2-hairline.png` 里参考图的 25.4 mm 虚线在 y ≈ 221.3 mm（472 px）、OCS 画在 209.61 mm，x 段分毫不差、y 低 11.7 mm；节距也对不上（参考 ~6.6/2.4 mm vs 文件 3.5/1.75 mm）。已在计划 §6 登记，另开单。

**测试 / 基线**（L4/L5，未提交）：`cargo test --test pid_import` **53/53**，`--lib io::pid` **56/56**，`report.csv` 重跑零差异（只变 0201/0202/publish-0202/工艺四行，D06/A01 哈希不变）；单文件 clippy/rustfmt 干净。

**文件清单**（16）：`0201-full.png`；OCS 单窗 `ocs-0201-{drawing-description,item-note-452,item-note-617,item-note-line1.5}-x8.png` + `ocs-0201-line2-hairline-x4.png`；参考图同窗同倍 `ref-0201-…`；并排 `sbs-0201-…`。
