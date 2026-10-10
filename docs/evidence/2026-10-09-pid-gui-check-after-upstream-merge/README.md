# `.pid` GUI 复核（2026-10-09，上游合并 `e02c7124` 之后；对照 `2026-09-24-pid-gui-check`）

本机 debug 版 OCS（`D:\Rust\target\debug\OpenCADStudio.exe`，main `e02c7124`，2026.40.1），AMD Radeon RX590 GME，窗口 1558 × 1208（显示缩放 150%）。
不走桌面自动化：`OpenCADStudio.exe <pid> --http <port> [--backend …]`，用 REST `POST /api/v1/capture`（`bounds` 取 9/24 同一窗口 (350, 248)–(395, 292) mm，`scope: viewport`）让程序自己缩放并从 GPU 回读视口；`GET /api/v1/state` 读相机。驱动脚本在 `%TEMP%\ocs-merge\pid-probe.ps1`，不入库。

| 文件 | 后端 | 看什么 | 结论 |
|---|---|---|---|
| `0201-tags-after-merge-vulkan.png` | Vulkan | 位号特写，与 9/24 的 `0201-tags-after-pid.png` 同一窗口 | 字形 / 字高 / 位置与 9/24 一致：Arial Narrow TrueType 填充，`LIA 060101`、`H=1600 mm`、`L=300 mm`、`LIT`、`LG` 都放得进气泡。**只有颜色不同**：9/24 白字，现在黑字（见下） |
| `0201-full-after-merge-vulkan.png` | Vulkan | 打开后的整图 | 图框、管线、设备、仪表都在；REST `ZOOM W` 之后视口立即更新，相机 (297.15, 210.16) / 975 → (372.5, 270) / 100.83 |
| `0201-tags-after-merge-dx12.png` | dx12 | 同一窗口 | **视口是坏的**：满屏重复的彩色小方块（每行 8 个一样的格子），看不到线 / 圆 / 图框；`/state` 里相机已到 (372.5, 270)，画面却与整图视图一字不差。`--compat-renderer` 一样；无 panic、无报错 |
| `0201-tags-after-merge-dx12-after-resize.png` | dx12 | 同上，窗口 resize 之后 | 视口全黑 |
| `0201-full-pre-merge-dx12.png` | dx12 | **合并前** main `b17fd973`（2026.39，临时工作树编的）打开同一张图，外部截屏 | 同样是坏的（彩色方块加白块；程序自己回读的视口则全黑）。**dx12 的坏不是这次合并带来的**，是本机 dx12 + wgpu 30 原有的问题；9/24 验收用 Vulkan 也是这个缘故 |

黑字：这些位号文字是 pid 导入写的真彩 `RGB(0,0,0)`（图层 `Labels` 本身是 7 号色）。9/29 的 `60f35e2b`「只有 7 号色随背景翻转，真彩色按原样画」(#1500) 之后，真彩黑不再翻白。该提交在合并前的 main 里就有，合并没有改这条规则（`git diff b17fd973 e02c7124 -- src/scene/view/render.rs` 不含 `adapt_to_bg`，`tess_util.rs` 未动），所以是 9/24 → 9/29 之间的有意改变，不是合并回归。要让 P&ID 的黑字在深色画布上可读，得在 pid 导入处把黑色文字写成 ACI 7 / ByLayer。

顺带：相机一动文档就标脏（`on_tick` 把相机同步进文档），标签页出现 ●，是既有设计，不是合并问题。
