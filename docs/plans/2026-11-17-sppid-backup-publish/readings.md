# 读数记录（SmartPlant 备份包：升级解析库、读取、出 PDF、导出）

> 状态：**进行中**。§1 是升级 `pid-parse` 之前的一组（`DEP-2`，2026-11-18）；§2 是换版本这一步的记录（`DEP-3`，2026-11-18）。升级之后的一组和逐张对比由 `DEP-4` 补在后面。
> 用途：`SPEC-sppid-deps.md` §3.2 的「不变差」拿升级前后两组数判；后面各块的读数也接着记在这里。
> 规矩：数都出自当次命令的输出。报告全文、日志、截图、PDF 在仓库外，这里只记数和位置。

## 1. 升级前（2026-11-18）

| 项 | 值 |
|---|---|
| 本程序的提交 | `64401e1ac`（`main`，本地，未推送）。开始时工作区除没提交的设计稿文件和不入库的 `.agent-tmp/` 之外是干净的 |
| `pid-parse` 的提交 | `d0a58c64c413`（`Cargo.toml` 六处 `rev`） |
| 证据 | `D:\scratch\ocs-sppid\2026-11-18-before\`，下面叫「证据目录」 |
| 机器 | 32 个逻辑处理器。全厂包的走查开始前，本机没有别的 `cargo`、`rustc` 在跑 |

### 1.1 门禁

日志在证据目录的 `gates\`。

| 门禁 | 读数 |
|---|---|
| `cargo test --offline --locked --lib` | 6954 条：6901 过，49 忽略，4 失败 |
| `cargo clippy --offline --locked --lib` | 630 条警告 |
| `cargo clippy --offline --locked --lib --profile test` | 965 条警告 |
| `cargo test --offline --locked -p ocs_collab -p ocs_collab_server` | 96 + 168 + 18 条，全过 |

失败的 4 条都是拿代码对设计稿文件的测试，原因是工作区里没提交的设计稿：

- `app::pid_shell::pid_editor_screen_canvas::the_grid_is_drawn_the_way_the_design_draws_it`
- `ui::style::pid_tokens::tests::every_variable_of_the_design_has_its_value_in_code_in_every_theme`
- `ui::style::pid_tokens::tests::the_code_states_nothing_the_design_does_not`
- `ui::style::pid_fonts::tests::the_faces_are_the_weights_the_design_uses`

四项都和共用约定 §3.1「2026-11-18 补」记的数相同。

### 1.2 三个包的走查

共用约定 §4 的走查命令，界面语言 `zh-CN`，带逐张导入，三个包各一次，三次都通过。每个包的 `report.txt`、`cargo-test.log` 和截图在证据目录的 `plant\`、`small\`、`sqlserver\`。

| 包 | 图纸 | 读不出图内对象 | 打不开 | 画出的图元 | 没画出的记录 | 全部导入用时 | 最慢的一张 |
|---|---:|---|---|---:|---:|---:|---|
| 全厂包 | 53 | 2 张：`D03`、`D04` | 1 张：`D03` | 149 310 | 9 | 35.6 秒（35 648.4 ms） | `P井场 注采阀组工艺及自控流程图`，3.7 秒 |
| 小包 | 7 | 1 张：`001` | 1 张：`001` | 1 697 | 0 | 0.59 秒（585.2 ms） | `DWG-0201GP01-01`，0.24 秒 |
| SQL Server 包 | 12 | 11 张：`A0-W-New`、`A0-W`、`A1-W-New`、`A1-W`、`A2-W-New`、`A2-W`、`A3-W-New`、`A3-W`、`CPECCHBA2-new`、`CPECCHBA2`、`XIONGANA2` | 2 张：`CPECCHBA2-new`、`CPECCHBA2` | 920 | 49 | 2.9 秒（2 938.7 ms） | `XIONGANA2`，2.8 秒 |

- 原因的原文（中文界面）：读不出是「图纸里没有图号（P&IDAttributes 里没有带 DrawingNo 的行）」；打不开是「里没有可画的图形：解析器在里面没有找到图形」。读不出的没有一张是不声不响的。
- 升级后的上限：全厂包 53 张全部导入不超过 53.5 秒（35.6 秒的 1.5 倍）。小包和 SQL Server 包的用时不足 5 秒，只记不判（`SPEC-sppid-deps.md` §11）。
- 每张图的四个数（画出的图元、解出的记录、没画出的记录、图纸层）在各自的 `report.txt` 里，升级后由对比脚本逐张比。

同一次走查里的其他数：

| 包 | 载入 | 包内文件 | 参考数据 | 图内对象 / 属性 | 读完图内对象 | 带图框的图 | 口令检查 |
|---|---|---:|---|---|---:|---|---|
| 全厂包 | 1.18 秒（第一次还原，160.3 MB） | 17 | 102 个目录，695 个文件 | 14 894 / 15 921（8 056 个对象没有属性） | 1.39 秒 | 45 张，4 366 个图元 | 0 处 |
| 小包 | 0.12 秒（还原过的；当天第一次还原是 0.55 秒，68.5 MB） | 17 | 102 个目录，677 个文件 | 152 / 70（116 个没有属性） | 0.10 秒 | 4 张，309 个图元 | 0 处 |
| SQL Server 包 | 1.13 秒（第一次还原，133.1 MB） | 15 | 102 个目录，662 个文件 | 3 / 6（1 个没有属性） | 0.13 秒 | 1 张，102 个图元 | 0 处 |

- 三个包的包内文件里各有 1 个登记了但包里没有（都是 `RefData~4~824`，原始文件是 `JacketNDCoreND.XML`），各有 1 条警告说这件事。SQL Server 包另有 1 条警告：转储是 SQL Server 的，不读它的数据库，图纸按文件夹列出。
- 符号本体全部取自图纸自己的缓存（8 750、92、2 个），没有一个取自符号库。
- 对比脚本拿三份报告各自和自己比过：三份都读得进去，输出「没有变差」，退出码 0。

### 1.3 `PIDPLOT` 抽查的图

办法：用这个提交编出的程序（`cargo build --offline --locked --bin OpenCADStudio`，debug 构建，2026-11-18 13:27 编成），不开窗口，一张图一次：`OpenCADStudio.exe --each <脚本> <图.pid>`，脚本只有一行 `PIDPLOT "<输出.pdf>"`。程序用单独的 `APPDATA` 和 `TEMP`（在证据目录里），不碰这台机器上的设置。图取自全厂包目录下 `backup-plant-data\` 里的 53 个 `.pid`，按文件大小排序（名次从 0 起）。页数用 PyMuPDF 读。输出、日志和 `summary.txt` 在证据目录的 `pdf\`。

| 名次 | 图纸 | `.pid` 大小 | 结果 | PDF 大小 | 页数 | 纸面 | 图元 | 符号文件 |
|---:|---|---:|---|---:|---:|---|---:|---|
| 0（最小） | `D03` | 61 440 | 出不来：打开失败，「D03.pid 里没有可画的图形」，退出码 1 | — | — | — | — | — |
| 1 | `D04` | 151 552 | 写出 | 349 974 | 1 | 594 × 421 mm | 541 | 用到 0 个 |
| 26（居中） | `集注站 低压气及轻烃处理单元 轻烃分离器二工艺及自控流程图` | 1 302 528 | 写出 | 479 148 | 1 | 841 × 594 mm | 1 030 | 35 个都找到 |
| 52（最大） | `EF井场 注采阀组工艺及自控流程图` | 6 717 440 | 写出 | 878 349 | 1 | 1189 × 841 mm | 7 366 | 62 个里找到 61 个 |

- **抽查的名单比规格多一张。** 规格 §3.4 要最小、居中、最大的 3 张。最小的 `D03` 正是打不开的那张，出不了 PDF，没法比升级前后的页数。所以 `D03` 照样试并记下结果，另加第二小的 `D04`。升级后用同样的 4 张。
- 最大的那张有 1 个符号文件在符号库里找不到：`piping\specialty components\misc\pulsation damper.sym`，纸面上画成方框，程序给了一条警告。
- 图框模板三张都找到：从全厂包目录的 `RefData~4~682.zip` 里读出来用。符号库是全厂包目录下的 `Ref\Symbols`。
- 连出两次，三份 PDF 逐字节相同（SHA-256 的前 16 位：`0dcded2c2b3d423e`、`35f9267809ea47c7`、`19f257b80121c4de`）。
- 用时 3.4、9.1、7.5 秒，含程序启动和打开图纸；这是 debug 构建的数，只作参考。

### 1.4 和 2026-11-10 的记录不一样的地方

旧记录在共用约定 §3.2，出处是 `docs/plans/2026-11-10-pid-backup-ui-coverage.md` §1.2。

| 项 | 2026-11-10 | 现在 | 原因 |
|---|---|---|---|
| 包内文件数（全厂包、小包、SQL Server 包） | 15、15、14 | 17、17、15 | 那次记录之后，包根目录里的 `Export.log`、`PlantConfig.xml` 也列进来了（同一份文档的 W3，`3ccc72782`）。SQL Server 包没有 `Export.log`，所以只多 1 个。三份报告里的文件名单核对过 |
| 参考数据文件数 | 645、0、— | 695、677、662 | 那次记录之后，还原时把备份自带的参考数据卷都放回 `Ref`（同一份文档的 W8，`4d389009b`），各卷的文件都算。旧数 645 等于符号那一卷登记的文件数 |
| 全厂包 53 张全部导入 | 36.5 秒 | 35.6 秒 | 同一版解析库，两次测量之间的差 |
| 全厂包最慢的一张 | `P井场`，6.2 秒 | `P井场`，3.7 秒 | 没有查原因。最慢的仍是同一张 |
| 全厂包读完图内对象 | 1.3 秒 | 1.39 秒 | 两次测量之间的差 |
| 载入 | 全厂包第一次约 0.8 秒（还原 130 MB），解过的 0.12 秒；小包 0.07 秒；SQL Server 包 0.05 秒 | 全厂包第一次 1.18 秒（还原 160.3 MB）；小包解过的 0.12 秒；SQL Server 包第一次 1.13 秒（还原 133.1 MB） | 还原的东西多了参考数据卷（W8）。SQL Server 包的旧数是不是第一次还原，旧记录没写，不能直接比 |

没有变的：三个包的图纸数（53、7、12）；读不出的名单（2、1、11 张）和打不开的名单（1、1、2 张），连图名都相同；画出的图元（149 310、1 697、920）；没画出的记录（9、0、49）；图内对象和属性（14 894 / 15 921、152 / 70、3 / 6）。

## 2. 换版本（`DEP-3`，2026-11-18）

| 项 | 值 |
|---|---|
| `pid-parse` 的新提交 | `96ee00cf7555`（`Cargo.toml` 六处 `rev`）。当天查过 GitHub 的 `main`，就是它；导出那 3 个提交（`d3fbe3e`、`3253301`、`6ecd1b4`）不在远端，`EXP-0` 保留 |
| 联网 | 一次：看远端，`cargo fetch` 取新提交。之后都离线 |
| 证据 | `D:\scratch\ocs-sppid\2026-11-18-dep3\`：旧锁文件 `Cargo.lock.before`，取新提交的日志 `cargo-fetch.log`、`cargo-fetch-offline.log`，编译检查 `check-1\`、`check-2\`、`check-3\`，门禁 `gates\`，格式 `fmt-before\`，分层检查 `arch\` |

### 2.1 锁文件实际变了什么

两份锁文件逐包比对：1019 个包变成 1017 个，10 个换了版本或来源，2 个去掉，没有新增。

| 包 | 升级前 | 升级后 | 在规格 §2.4 的预期里 |
|---|---|---|---|
| `pid-parse` 0.11.7、`pid-plot` 0.1.0、`hayro` 0.7.1、`hayro-interpret` 0.7.0、`hayro-syntax` 0.7.2、`lopdf` 0.45.0、`oxidized-mdf` 0.3.1 | 来自 `d0a58c64c413` | 来自 `96ee00cf7555`，版本号都没变 | 在 |
| `rusqlite` | 0.39.0 | 0.41.1 | 在 |
| `libsqlite3-sys` | 0.37.0 | 0.39.1 | 在 |
| `sqlite-wasm-rs`、`rsqlite-vfs` | 0.5.5、0.1.1 | 去掉 | 在 |
| `hashlink` | 0.11.1 | 0.12.2 | **不在**。停下问过，同意（2026-11-18） |

- `hashlink` 升级前后都只被 `rusqlite` 依赖（它的预编译语句缓存用）。`rusqlite` 0.39.0 的清单写的是 `hashlink` 0.11，0.41.1 写的是 0.12，所以它跟着 `rusqlite` 走。它依赖的 `hashbrown` 0.17.1 原来就在锁文件里。
- 清单另改了一行，规格没有料到：`crates/ocs_collab_server/Cargo.toml` 的 `rusqlite` 从 0.39 改成 0.41（关默认特性，开 `bundled`、`cache`，和 `pid-parse` 的写法相同）。不改的话依赖解不出来，原因见 `SPEC-sppid-deps.md` §11。

### 2.2 为编译改的地方

`cargo check` 一共报了 13 处错：非测试代码 4 处，测试代码 9 处。改了 5 个源文件，都是跟着接口改写法。

| 文件 | 处 | 库那边变了什么 | 本程序怎么跟 |
|---|---:|---|---|
| `src/io/pid_pdf.rs` | 2 | `pid_plot::model::lay_out_with_symbol_roots` 从 4 个参数变成 5 个，多的是排在符号目录之后去找的 `RefData~*.zip` 名单；`pid_plot::write::PlotFont` 多了字段 `subset` | 名单传空；`subset` 照 `prepare_fonts` 给每个字体的值传 |
| `src/io/pid_backup_browse/exp_views.rs` | 1 | `PidPublishDrawing` 多了字段 `drawing_attributes` | 给空（这里造的图纸只用来合成管线名） |
| `src/io/pid_backup_browse/drawing_data.rs` | 1 | `PidValueSource` 多了 `CodelistCell`、`ClassConstant`、`PlacedColumn` 三种 | 两种带表名列名的归到已有的 `DatabaseColumn`；`ClassConstant` 在本程序的 `ValueSource` 里加一种 |
| `src/ui/window/pid_backup_browser/drawing_data.rs` | 1 | 无（跟着上一行改） | 显示来源的函数认 `ClassConstant`，显示库自己的词，不进翻译表 |
| `src/io/pid/tests.rs`（测试） | 9 | `PrimitiveStyle` 多了字段 `paper`（3 处）；`LineSymbology` 多了 `paper`（4 处）；`SymbolLettering` 多了 `height_mode`、`width_permille`（2 处） | 测试里造的数据填 `false`、`None` |
| `src/io/pid_backup_browse/drawing_data.rs`（测试） | 1 | `PidPublishDrawing` 多了字段 `drawing_attributes` | 填空。另在 `every_kind_of_source_is_kept` 里补 3 条断言，覆盖新增的三种来源 |

`cargo check --offline --locked` 在 `--lib`、`--lib --profile test`、`--all-targets` 和协同两个 crate 的 `--all-targets` 上都没有错误。各处为什么这样跟，见 `SPEC-sppid-deps.md` §11。

### 2.3 测试和门禁

**因升级失败的测试：0 条。没有改任何期望值。**

| 门禁 | 升级前（§1.1） | 换版本后 |
|---|---|---|
| `cargo test --offline --locked --lib` | 6954 条：6901 过，49 忽略，4 失败 | 相同；失败的还是设计稿那 4 条 |
| `cargo clippy --offline --locked --lib` | 630 | 630 |
| `cargo clippy --offline --locked --lib --profile test` | 965 | 965 |
| `cargo test --offline --locked -p ocs_collab -p ocs_collab_server` | 96 + 168 + 18，全过 | 相同（在 `rusqlite` 0.41 上） |
| `scripts\fmt-newhunks.ps1`，5 个改过的源文件 | — | 新增 0 |
| `python scripts/arch_check.py report` | 33 对模块、183 个文件；`io` 开头的四行是 3、1、1、1 | 逐行相同 |
| `cargo metadata --offline --locked` | — | 成功 |

- 测试编译时的编译器警告数也没有变（52 条）。
- 库测试跑完的时间从 37.4 秒变成 47.6 秒，各量了一次。是不是解析变慢了，这一步不判，以 `DEP-4` 走查里的导入用时为准。

## 修订记录

| 日期 | 改了什么 |
|---|---|
| 2026-11-18 | 新建。记升级前的一组读数（`DEP-2`） |
| 2026-11-18 | 加 §2：换版本这一步的记录（`DEP-3`），锁文件实际的变化、为编译改的地方、测试和门禁 |
