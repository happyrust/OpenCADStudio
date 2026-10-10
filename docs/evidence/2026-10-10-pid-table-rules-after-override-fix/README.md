# `.pid` GUI 复核（2026-10-10，pid-parse `931ddca` 让 `JStyleOverride` 自带的宽 / 色 / 虚线位生效之后）

对照 `2026-10-09-pid-gui-check-after-upstream-merge/`（同一棵 `main` `aaedd334`、旧 pid-parse）与 10-09 会话对 SmartPlant 参考图 `D:\work\cad\图片验证\pid-1.png` 的逐实体对比：
那次七条差异里的 **#2「设备表 14 条格线 OCS 画成虚线、参考图是实线」** 和坡度标记的颜色，是这一单修的。计划：pid-parse `docs/plans/2026-10-10-an-override-applies-its-own-width-colour-and-dash.md` O3。

本机 debug 版 OCS（`D:\Rust\target\debug\OpenCADStudio.exe`，`main` `aaedd334` + pid-parse `931ddca`），Vulkan，AMD Radeon RX590 GME。
不走桌面自动化：`OpenCADStudio.exe <pid> --http 18112 --backend vulkan`（`OCS_API_TOKEN` 预设），REST `POST /api/v1/capture`（`document_id: 2`、`scope: viewport`，`bounds` 或 `view: extents`）让程序自己缩放并回读视口；用完 `Stop-Process`。
取景窗口按改后 DXF 里 14 条格线的坐标定：设备表 x 472.2–573.8 / y 344.9–402.0 mm。

| 文件 | 取景（mm） | 看什么 | 结论 |
|---|---|---|---|
| `0201-table.png` | (468, 340)–(578, 406) | 设备表 | **10 条横线 + 4 条竖线全是细实线**，没有一段虚线。按改后 DXF 的 `370` 核：0.13 mm 的 11 条 = 10 条横线 + 中间 x=510.3 那条竖线，0.18 mm 的 3 条 = x=472.2 / 525.8 / 573.8 三条竖线（左右边框与第二条分栏线），全部 `420`=0 真彩黑、线型 `Continuous`；10-09 这里是 3.5 / 1.75 mm 虚线。格内文字仍骑在本行上沿（#1，X2 在 `pid-web-open` 上等提交）；线和字都是真彩黑、在深色画布上偏暗（#3，未动） |
| `0201-slope-2.png` | (270, 219)–(290, 233) | `i=0.3 %` 坡度三角 | **黑色细线**；10-09 是粉红 `#FE0060`（基样式的色），参考图里那一块没有粉色像素 |
| `0201-full.png` | `view: extents` | 整图 | 图框、管线、设备、仪表都在，与 10-09 的 `0201-full-after-merge-vulkan.png` 同一观感；LIA–LIT 电信号线仍是虚线 |

`cargo test --test pid_import` 52 过（三处重钉：0201 调色盘、图纸虚线只剩电信号线、缓存本体虚线 工艺 45 / 0202 12 → 0 / 3 + 0201 1）；`--lib io::pid` 54 过；批量基线重签见 `2026-09-24-pid-batch-baseline/README.md` 的 2026-10-10 段。
