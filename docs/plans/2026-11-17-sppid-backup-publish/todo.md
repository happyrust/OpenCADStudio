# SmartPlant 备份包：打开、浏览、导出 PDF 和 publish 文件 · 任务清单

> 日期：2026-11-18
> 状态：**已确认**（2026-11-18）。从 `DEP-1` 开始，按顺序一个一个做，做完一个勾一个。
> 配套：`plan.md`（分段、依赖图、检查点、风险、需要你做的事）。命令、数据格式、成功标准写在各块的 `SPEC-sppid-*.md`，共用约定在 `01-shared-conventions.md`。
> 基线：`main @ c399311e7`。
> 任务是按规格里我建议的选项写的（P1–P4、R3），这五项都已按建议定。计划里没给建议的地方，我选最合适的一项接着做并说明理由（`plan.md` §3 末尾）。

## 0. 怎么读这份清单

共 19 个任务，分五段。

| 段 | 内容 | 任务 | 规格 | 做完是 |
|---|---|---|---|---|
| 0 | 升级解析库 | `DEP-1` – `DEP-4` | `SPEC-sppid-deps.md` | 两次汇报 |
| 1 | 读取与显示 | `RD-1` – `RD-4` | `SPEC-sppid-read.md` | 检查点 A |
| 2 | 导出 PDF | `PDF-1` – `PDF-6` | `SPEC-sppid-pdf.md` | |
| 3 | 导出 publish 文件 | `EXP-0` – `EXP-3` | `SPEC-sppid-export.md` | 检查点 B |
| 4 | 总验收 | `ACC-1` | `plan.md` §5 | 总验收 |

任务编号定下后不再改。规格里待决问题的编号（`D1`、`R3`、`X4`、`P2` 这类）是另一套。下文说「成功标准 N」，指的是该段规格 §6 的第 N 条。

每个任务写六项：

- **做什么**：要实现的行为，以及细节在规格的哪一节。
- **验收**：可以逐条打勾的结果。
- **验证**：证明它做完的命令。除这里列的之外，每个任务都要过 §1 的固定门禁。
- **依赖**：必须先做完的任务。
- **文件**：预计要动的文件。
- **规模**：S（1–2 个文件的小改动，或者只量读数）、M（一个新模块，或者 3–6 个文件）、L（范围事先说不清；只在拆不开时用，并写明原因）。

标题里的类别标记：

- 【纯】只动 `src/io/` 下的引擎。不碰界面，不碰命令。
- 【接线】要改共用文件：`src/app/commands/mod.rs`、`src/app/command_driver/utilities.rs`、放消息枚举和程序状态的文件。
- 【测试】只加测试，不加功能。
- 【依赖】改 `Cargo.toml`、`Cargo.lock`，要联网一次。
- 【脚本】加 `scripts/sppid/` 下的脚本，并用它量一次读数。
- 【文档】只动 `readings.md`。
- 【验收】在真实程序和真实包上走一遍，产出是读数和证据。

## 1. 每个任务都要过的固定门禁

下面各任务的「验证」不再重复这几条。命令在仓库根目录执行，Windows PowerShell 5.1。

1. **先红后绿。** 先写测试，看到它因为要测的那条断言失败（不是因为编译不过），再实现到通过。一写出来就是绿的测试（验证现有行为的那些），先用一个故意错的期望值看它失败，再改回来。红、绿两次的输出都留。
2. `cargo test --offline --locked --lib`：失败的仍然只是那 4 条比对设计稿文件的测试，测试条数不减少。现在的读数是 6954 条：6901 过、49 忽略、4 失败。`DEP-2` 重量一次；`DEP-4` 之后以升级后的读数为准。两组数都记在 `readings.md`。
3. `cargo clippy --offline --locked --lib` 警告不超过 630；加 `--profile test` 再跑一次，不超过 965。
4. 改动的文件不新增 `rustfmt` 不一致：`scripts\fmt-newhunks.ps1 <改前的副本> <改后的文件>`，新增 0。
5. 改了 `Cargo.lock` 的任务（`DEP-3`、`EXP-0`）加跑 `cargo test --offline --locked -p ocs_collab -p ocs_collab_server`：96 + 168 + 18 条，全过。
6. 动了 `src/io/` 或 `src/app/` 的任务，前后各跑一次 `python scripts/arch_check.py report`：`io` 开头各行的数字不变大。
7. 动了 `docs/pid-platform/` 才跑文档检查（现在 590 条：589 过，1 条因没提交的设计稿失败）。
8. **一个任务一个提交。** `git add` 只加本任务列出的文件，外加这份 `todo.md`（把本任务的验收项打勾）。`git commit -q -F <说明文件>`。`docs/pid-platform/design/pid-platform.pen` 不进提交。不推送。
9. **样本和证据不进仓库。** 样本包、导出的文件、截图、报告全文放 `D:\scratch\ocs-sppid\<日期>-<说明>\`。`readings.md` 只记数字、结论和证据的位置，每组读数带日期和两边的提交。
10. **全程离线**：`--offline --locked`。只有 `DEP-3`、`EXP-0` 各联网一次。
11. 实现与规格有出入时，在该块规格末尾加一节「实现记录」记下来，不悄悄改口径。
12. 【文档】【验收】任务不改 Rust 代码，第 2–4 条不必为它们重跑（`DEP-2`、`DEP-4` 本身就是量这几条）。

## 2. 测试怎么写、放在哪

- 引擎的测试写在同文件的 `#[cfg(test)] mod tests` 里。要读写文件的，用 `std::env::temp_dir()` 下带唯一后缀的目录，测完删掉。
- 命令的测试写在命令文件自己的测试模块里：`OpenCADStudio::new_for_test()` 起一个程序对象；备份目录在测试里现造，照 `src/app/commands/pidbackup.rs` 现有测试的造法。
- 「新界面下」的测试，按不加 `--classic` 的办法建程序，照 `src/app/pid_shell/pid_shell_startup.rs`。
- **替身。** 整包导出和单张出图都做成可以换掉的函数。快测试传替身（返回固定结果、固定错误，或者等一个信号才返回）；真实的转换只在真实包那一层跑。
- **真实包的测试**标 `#[ignore]`，路径从环境变量取，默认值是共用约定 §2 的位置。它们由 `scripts/sppid/` 下的脚本串起来：一条命令出一张表，有一格不过退出码非 0。
- PowerShell 脚本要能在 Windows PowerShell 5.1 下跑；脚本文件只用 ASCII 字符（5.1 读不带 BOM 的文件会把中文读坏），表头用英文。路径都能用参数改。
- Python 脚本的测试：`python -m pytest scripts/sppid -q -p no:cacheprovider`。
- 不加依赖。解析 PDF 用仓库已有的 `lopdf`。
- 验证命令里的过滤名（如 `pid_backup_pdf`）是建议的测试名前缀，写测试时照它起名。

---

## 第 0 段：升级解析库

目标：把 `pid-parse`、`pid-plot` 从 `d0a58c64c413` 升到主线 `96ee00c`，并证明读取和开图没有变差。规格：`SPEC-sppid-deps.md`。

### DEP-1 【脚本】对比两份走查报告的脚本

- **做什么**：`scripts/sppid/compare_walks.py`（规格 §4）。读两份走查报告 `report.txt`，逐张图列出四个数的变化（画出的图元、解出的记录、没画出的记录、图纸层），读不出和打不开名单的增减，总时间的比值。输出一张 Markdown 表，可以直接贴进 `readings.md`。有新冒出的读不出或打不开的图，或者总时间的比值超过上限（默认 1.5），退出码非 0。写解析之前，先拿一份真实的报告看清格式。
- **验收**：
  - [ ] 两段造的报告文本：数字变了的图逐张列出，前后的数都在。
  - [ ] 新增一张读不出或打不开的图：点名，退出码非 0。名单变短：点名，退出码 0。
  - [ ] 一张图只在其中一份报告里：点名，退出码非 0。
  - [ ] 总时间的比值超过上限：报出比值，退出码非 0。
  - [ ] 两份报告相同：输出「没有变化」，退出码 0。
  - [ ] 报告的格式不认识（缺要读的段落）：报错并说明缺什么，退出码非 0。不当作「没有变化」。
- **验证**：`python -m pytest scripts/sppid -q -p no:cacheprovider`
- **依赖**：无
- **文件**：新建 `scripts/sppid/compare_walks.py`、`scripts/sppid/test_compare_walks.py`
- **规模**：S

### DEP-2 【文档】升级前的读数

- **做什么**：在钉的版本（`d0a58c64c413`）上、干净的工作区里留一组读数（规格 §3.1 第 2 步）。门禁各跑一次。三个样本包各走查一遍。全厂包里按 `.pid` 文件大小取最小、居中、最大的 3 张，各出一份 PDF：用真实程序打开这张图，发 `PIDPLOT <输出路径>`，记页数和大小。报告全文和 PDF 存 `D:\scratch\ocs-sppid\<日期>-before\`。新建 `readings.md`，记：日期，本程序的提交，`pid-parse` 的提交，每项读数，3 张图的名单，证据的位置。
- **验收**：
  - [ ] 开始时 `git status` 除设计稿文件和 `.agent-tmp/` 之外是干净的。
  - [ ] `readings.md` 里有：库测试的条数（过、忽略、失败），两个 clippy 的警告数，协同两个 crate 的条数。
  - [ ] 三个包各一行：图纸数，读不出的名单，打不开的名单，图元总数，没画出的记录数，总耗时。
  - [ ] 3 张抽查图的名字、页数、文件大小。
  - [ ] 和共用约定 §3.2 的旧记录（2026-11-10）不一样的地方，逐项写明。
- **验证**：共用约定 §4 的走查命令，三个包各一次；`cargo test --offline --locked --lib`；两个 clippy；`cargo test --offline --locked -p ocs_collab -p ocs_collab_server`
- **依赖**：DEP-1
- **文件**：新建 `docs/plans/2026-11-17-sppid-backup-publish/readings.md`
- **规模**：S

### DEP-3 【依赖】换版本，改到编译和测试通过

- **做什么**：`Cargo.toml` 六处 `rev`（第 163–166、180、183 行）一起改成主线提交 `96ee00c` 的完整哈希。联网一次：`cargo fetch`，取新提交并更新 `Cargo.lock`。拿 `git diff Cargo.lock` 对照规格 §2.4 的预期；出现预期之外的包变化就停下报告，不往下做。此后一律 `--offline --locked`。`cargo check --offline --locked --lib` 的报错逐个改，记下改了几个文件、几处、各是什么变化。**编译通过后发第一次汇报**：改了多少代码；锁文件实际变了哪些包；有多少条测试因升级失败、各属哪一类。汇报后按 D4 接着做。测试按规格 §3.3 处理：因解析库行为变化而失败的可以改期望值，每条写明原值、新值、原因；说不出原因的不改，按变差处理；不删测试，不加 `#[ignore]`。换版本之前先看 `pid-parse` 主线现在的提交号（算在同一次联网里）：那 3 个导出提交已经在主线上、主线上又没有别的新提交，就钉这个新的主线提交，`EXP-0` 取消；否则钉 `96ee00c`，主线上有什么写进汇报。
- **验收**：
  - [ ] 六处 `rev` 相同；`cargo metadata --offline --locked` 成功。
  - [ ] `git diff Cargo.lock` 里变化的包都在预期之内；之外的逐个有原因，并经你同意。
  - [ ] `cargo check --offline --locked --lib` 没有错误；改动的文件数和处数写进 `readings.md`。
  - [ ] 库测试：失败的仍只有设计稿那 4 条，测试条数没有减少。改过期望值的测试逐条列在提交说明和 `readings.md` 里。
  - [ ] clippy 不超过 630 和 965；格式新增 0；协同两个 crate 的条数不变，全过。
  - [ ] 第一次汇报已发。
  - [ ] 提交里只有 `Cargo.toml`、`Cargo.lock`、为编译和测试通过而改的源文件、`readings.md`、本清单。
- **验证**：`cargo metadata --offline --locked --format-version 1 > $null`；`cargo check --offline --locked --lib`；§1 的门禁，含协同两个 crate
- **依赖**：DEP-2
- **文件**：`Cargo.toml`、`Cargo.lock`；编译报错指到的源文件（范围是规格 §2.3 的 21 个，名单以编译结果为准）；`readings.md`
- **规模**：L。改的只是六处版本号，但连带要改多少源文件事先不知道；拆不开，因为不改完就编译不过

### DEP-4 【文档】升级后的读数、对比和判定

- **做什么**：用 `DEP-2` 的办法再量一遍，存 `D:\scratch\ocs-sppid\<日期>-after\`。用 `compare_walks.py` 出三个包的逐张差异表，每一处差异写原因，指到 `pid-parse` 的提交或它的说明。按规格 §3.2 的表逐行判定。读不出、打不开的上限按 R2 降到新读数。**发第二次汇报**：开图的结果变了没有；`D03`、`D04` 读不读得出；判定结论。任何一行判为变差：停下，报告是哪个提交、影响哪几张图，请你定。
- **验收**：
  - [ ] 三个包升级后的图纸数仍是 53、7、12。
  - [ ] 读不出的名单、打不开的名单里没有新图；`compare_walks.py` 三次的退出码都是 0。
  - [ ] 逐张差异表在 `readings.md` 里，每处差异有原因。
  - [ ] `D03`、`D04` 现在读不读得出、打不打得开，写明。
  - [ ] 53 张图全部导入的总时间不超过升级前的 1.5 倍。
  - [ ] 3 张抽查图升级前后都出得来 PDF，页数相同；大小的变化记下。
  - [ ] 新的上限（读不出、打不开）写进 `readings.md`，只降不升。
  - [ ] 第二次汇报已发。
- **验证**：共用约定 §4 的走查命令，三个包各一次；`python scripts/sppid/compare_walks.py <前>\report.txt <后>\report.txt`，三个包各一次
- **依赖**：DEP-3
- **文件**：`readings.md`
- **规模**：S

---

## 第 1 段：读取与显示

目标：不加 `--classic`，用命令打开备份包、浏览、开图，三个样本包的读数达标；显示得对不对，有一份对照。规格：`SPEC-sppid-read.md`。做完是检查点 A。

### RD-1 【测试】新界面下打开备份包的快测试

- **做什么**：新文件 `src/app/pid_shell/pid_backup_in_shell.rs`，四组测试对应成功标准 1–4。程序按不加 `--classic` 的办法建；备份目录在测试里现造。按代码看，这些测试一写出来就是绿的：每条先用一个故意错的期望值看它失败，再改回来。**如果是真的红**，说明新界面下确实不可用：先报告是什么问题，再改（预计在 `src/app/view/mod.rs`、`src/app/dock/update.rs` 或 `src/app/pid_shell/editor.rs`）。
- **验收**：
  - [ ] 新界面下执行 `PIDBACKUPOPEN <目录>`：备份面板可见；当前画面仍是编辑器；索引里的图纸数等于造的数；命令行里有一行报出图纸数。
  - [ ] 面板打开时，特性面板仍然隐藏，编辑器的标签条还在；关掉面板后它不再可见。
  - [ ] 激活一个图纸节点：发出打开这张图的请求，指向还原出来的那个 `.pid`。
  - [ ] 给一个不存在的路径、一个不是备份的目录：命令行各有一条错误，程序继续运行；面板的状态和 `--classic` 下做同样的事之后相同。
  - [ ] `pid_backup`、`pidbackup` 现有的测试一条不改，全部通过（成功标准 10）。
- **验证**：`cargo test --offline --locked --lib pid_backup_in_shell`；`cargo test --offline --locked --lib pidbackup`；`cargo test --offline --locked --lib pid_backup`
- **依赖**：DEP-4
- **文件**：新建 `src/app/pid_shell/pid_backup_in_shell.rs`，在 `src/app/pid_shell/mod.rs` 登记
- **规模**：S

### RD-2 【测试】走查加两个开关

- **做什么**：`src/app/commands/pidbackup/audit.rs` 加两个环境变量（规格 §5.2）。`OCS_PID_BACKUP_AUDIT_SHELL=1`：按不加 `--classic` 的办法建程序再走查。`OCS_PID_BACKUP_AUDIT_EXPECT=<图纸数>,<读不出上限>,<打不开上限>`：走完后断言，不满足则测试失败，列出超出的图名和原因。新界面下的走查，每打开一张图多断言四件事（成功标准 7）：标签页数加 1，新标签页在最前，图元数大于 0，当前画面仍是编辑器。已有的四个环境变量不变；不给新开关时，走查的行为和现在完全一样。期望值的解析和判定写成纯函数，带快测试。
- **验收**：
  - [ ] 期望值的解析：三个数都对；少一个、多一个、不是数，各报错并给出用法。
  - [ ] 判定：都不超 → 通过；图纸数不等、读不出超限、打不开超限 → 各自失败，消息里有超出的图名和原因。
  - [ ] 不给两个新开关，小包走查的报告和改动前相同（耗时那几行除外）。
  - [ ] 给 `OCS_PID_BACKUP_AUDIT_SHELL=1` 走小包：报告里注明是新界面；打开的每张图都过了那四条断言。
  - [ ] 故意给错的期望值（比如 `8,0,0`）：测试失败并点名。
- **验证**：`cargo test --offline --locked --lib pidbackup::audit`（快测试）；共用约定 §4 的走查命令加两个新开关，对小包跑一次
- **依赖**：RD-1
- **文件**：`src/app/commands/pidbackup/audit.rs`
- **规模**：S

### RD-3 【脚本】三个样本包的走查脚本和读数

- **做什么**：`scripts/sppid/walk-samples.ps1`（规格 §5.3）。全厂包、小包（目录和 zip）、SQL Server 包各走查两遍，一遍经典界面，一遍新界面，都带期望值（`DEP-4` 定下的上限），共 8 次。汇总成一张表：包、哪个界面、图纸数、读不出、打不开、图元总数、耗时、过没过。另比两件事：同一个包两种界面的数字完全相同（成功标准 6）；小包从 zip 和从目录打开的名单相同（成功标准 8）。任何一格不过，退出码非 0。样本的位置从参数取，默认值是共用约定 §2 的位置；报告写到仓库外。跑一遍，读数记进 `readings.md`。
- **验收**：
  - [ ] 一条命令跑完 8 次走查，出一张表；全部通过时退出码为 0。
  - [ ] 新界面下三个包的图纸数是 53、7、12；读不出、打不开的张数不超过 `DEP-4` 定的上限；报告里每张读不出或打不开的图都有一句原因。
  - [ ] 同一个包，新界面和经典界面的图纸数、读不出的名单、打不开的名单、每张图的图元数完全相同。
  - [ ] 小包从 zip 打开和从目录打开：图纸名单相同（7 张），读不出和打不开的名单相同。
  - [ ] 把一个期望值改错再跑：对应的一格标不过，退出码非 0。
  - [ ] 读数表在 `readings.md` 里。
- **验证**：`powershell -NoProfile -ExecutionPolicy Bypass -File scripts\sppid\walk-samples.ps1`
- **依赖**：RD-2
- **文件**：新建 `scripts/sppid/walk-samples.ps1`；`readings.md`
- **规模**：S

### RD-4 【验收】真实窗口看一次，加显示对照

- **做什么**：启动真实程序，不加 `--classic`，用它自己的自动化接口发命令、取截图，不占你的鼠标键盘（R1）。两件事。
  (1) 成功标准 9：命令行打开全厂包 → 面板在图面右侧，列出 53 张 → 打开一张 → 多一个标签页并显示图面。
  (2) 成功标准 13（R3）：有厂商 PDF 的 11 张图逐张打开，缩放到全图，取画布截图；厂商 PDF 的那一页转成图片（用的办法写进 `readings.md`）；两张并排存到 `D:\scratch\ocs-sppid\<日期>-display\`。从哪里打开：先比包里还原出来的 `.pid` 和厂商文件夹里的 `.pid` 的校验和；相同的从备份面板打开；不同的直接打开厂商文件夹里的那个 `.pid`（已知全厂包里有 4 张和厂商的不是同一版），免得把版本的差别当成显示的差别。我逐张看，每张写一句结论：看不出差别，或者差在哪里（缺了什么、多了什么、位置、线型、文字）。
  **本任务里不修**：差别清单里每一项我给处理意见（现在修、以后修、不修）和理由，随检查点 A 的汇报发出；要修的另排任务（`plan.md` §5）。做完发检查点 A 的汇报。
- **验收**：
  - [ ] 截图里看得到：备份面板在图面右侧，列出 53 张；打开一张后多一个标签页，图面有内容。
  - [ ] 11 张图各有一张并排图，文件名带图名。
  - [ ] `readings.md` 里有 11 行：图名，从哪里打开的，结论，并排图的位置。打不开的图记为打不开并写原因，不算漏。
  - [ ] 差别清单：每项写明是哪张图、差在哪、原因可能在哪一层（读取、转换、画布）。没有查实的写明是推测。每项有处理意见和理由。
  - [ ] 检查点 A 的汇报已发，地图 §5 检查点 A 的 6 条逐条写了过没过。
- **验证**：真实程序走一遍；证据在 `D:\scratch\ocs-sppid\<日期>-display\`
- **依赖**：RD-3
- **文件**：`readings.md`
- **规模**：M。不改代码；量在 11 张图的逐张对照

---

## 第 2 段：导出 PDF

目标：一条命令把已打开的备份包每张图出成一份 PDF，每张图都有交代；出的 PDF 和 `pid_plot` 命令行工具出的逐字节相同。规格：`SPEC-sppid-pdf.md`。

### PDF-1 【脚本】从钉的提交编出对比用的命令行工具

- **做什么**：`scripts/sppid/build-pid-tools.ps1`（共用约定 §2）。读 `Cargo.toml` 里钉的 `pid-parse` 提交；用 `git archive` 把这个提交从你的 `pid-parse` 检出导出到仓库外的临时目录（不切分支，不改检出里的文件；检出里没有这个提交时，改用 cargo 取下来的那份源码）；在临时目录里离线编 `pid_plot`、`pid_plot_compare`、`pid_publish_xml` 三个程序（正式构建）；把三个程序和一份 `BUILD.txt`（提交、构建时间、命令）放到 `D:\scratch\ocs-sppid\tools\<提交前 7 位>\`。同一个提交编过就跳过。离线编不出来（缺依赖的缓存）：退出码非 0，报出缺什么；这时先报告，不自己联网。编好后用 `pid_plot --batch` 对全厂包出一遍，留作后面比字节的对照，读数记进 `readings.md`。
- **验收**：
  - [ ] 脚本结束后三个 `.exe` 都在；`BUILD.txt` 里的提交等于 `Cargo.toml` 里钉的提交。
  - [ ] 你的 `pid-parse` 检出：脚本前后 `git status --porcelain` 和 `git rev-parse HEAD` 都没变。
  - [ ] 再跑一次：认出已经编过，不重编，退出码 0。
  - [ ] `Cargo.toml` 的六处 `rev` 不全相同时：报错，退出码非 0。
  - [ ] `pid_plot --batch` 对全厂包的读数在 `readings.md` 里：写出几张，失败几张，缺符号几处，用时。
- **验证**：`powershell -NoProfile -ExecutionPolicy Bypass -File scripts\sppid\build-pid-tools.ps1`
- **依赖**：DEP-4
- **文件**：新建 `scripts/sppid/build-pid-tools.ps1`；`readings.md`
- **规模**：S

### PDF-2 【纯】单张出图和命令行工具对齐

- **做什么**：`src/io/pid_pdf.rs` 改三处（规格 §5.6）。(1) 写 PDF 时带标题，标题取 `.pid` 的文件名去掉扩展名，改用 `pid_plot::write::write_plot_titled`，和命令行工具一样。(2) 字体目录能由调用方多给，顺序照规格 §5.4。(3) 把「出一张」拆成整包也能调的函数：给 `.pid`、输出路径、参考目录、字体目录，返回这一页的读数（字节数、图元数、页面大小、用的模板、符号找到和引用的数、缺的符号、这一页的账），或者出不了的原因。`PIDPLOT` 的命令和回执不变。
  **先量再改**：先加一条真实样本的 `#[ignore]` 测试，对 `DEP-2` 定的 3 张图，把本程序出的 PDF 和 `PDF-1` 的 `pid_plot` 出的比字节。改之前它应当是红的（至少差一个标题），改完变绿。还有别的差别就逐个找原因；对不齐的列出来报告（P3）。
- **验收**：
  - [ ] 3 张抽查图：本程序出的 PDF 和同一版本 `pid_plot` 出的逐字节相同（同一个参考目录，同一个 `PID_PLOT_FONTS`）。
  - [ ] 出的 PDF 里有标题，等于图名。
  - [ ] 多给的字体目录排在规格说的位置，有快测试钉住顺序。
  - [ ] `PIDPLOT` 现有的测试：只改因标题而变的期望值，逐条写明；其余不动，全部通过。
  - [ ] 拆出来的函数不引用 `app`、`ui`。
- **验证**：`cargo test --offline --locked --lib pid_pdf`；`cargo test --offline --locked --lib pid_pdf_real -- --ignored --nocapture`（真实样本）
- **依赖**：PDF-1；检查点 A 的汇报已发
- **文件**：`src/io/pid_pdf.rs`；调用处 `src/app/commands/pidplot.rs` 可能要跟着改
- **规模**：S

### PDF-3 【纯】整包出 PDF 的引擎

- **做什么**：新文件 `src/io/pid_backup_pdf.rs`，接口照规格 §5.6。`check` 只查前提，不写任何东西（规格 §5.2 的目录规则，§5.5 的错误码）。`export` 逐张调「出一张」：一张出不了，记下原因接着做下一张；最后写 `_PdfIndex.json`（规格 §5.3 的字段，按包内路径排序，不写时间）。`PdfProgress` 是两个原子数。`export_in_background` 照 `plot_pdf_in_background` 的做法：一个有名字的线程，64 MB 的栈，逐张做。`summary_lines` 出命令行的汇总，出不了的多于 10 张时只列前 10 张。「出一张」是参数，快测试传假的。同名的第二张图不写，原因写进清单（P2）。文件先写成临时名再改名。
- **验收**：
  - [ ] `check`：输出路径是文件、在包根目录或还原目录里面 → `SPPID_PDF_TARGET_INVALID`。要写的文件有一个已存在 → `SPPID_PDF_TARGET_HAS_FILES`，消息里有前几个文件名，目录内容不变。没有一张有文件的图 → `SPPID_PDF_NOTHING_TO_PLOT`。
  - [ ] 目录里只有别的文件：照常导出，原有文件逐字节不变。
  - [ ] 清单的条数等于给的图数；每条要么有 `pdf` 要么有 `error`；目录里 PDF 的个数等于 `written`；库里登记了但没有文件的图计入 `without_file`。
  - [ ] 假的出图函数让其中一张失败：其余照写，返回成功，`failed` 为 1，这张的名字和原因在清单和汇总里。
  - [ ] 两张同名的图：第二张不写，清单里说明。
  - [ ] 同样的输入导到两个空目录：`_PdfIndex.json` 逐字节相同。
  - [ ] 进度：导出中途读到的「做完几张」只增不减，结束时等于共几张。
  - [ ] 清单写不了 → `SPPID_PDF_FAILED`；每种错误的码都有测试。
  - [ ] `python scripts/arch_check.py report` 里 `io` 开头各行的数字没有变大。
- **验证**：`cargo test --offline --locked --lib pid_backup_pdf`；`python scripts/arch_check.py report`
- **依赖**：PDF-2
- **文件**：新建 `src/io/pid_backup_pdf.rs`，在 `src/io/mod.rs` 登记
- **规模**：M

### PDF-4 【接线】命令 `PIDBACKUPPDF`

- **做什么**：新文件 `src/app/commands/pidbackuppdf.rs`（规格 §5.1）。`PIDBACKUPPDF <输出目录>`：从已打开的备份包取包根目录、还原目录、图纸名单，先 `check`，再在后台导出。不带参数：报三种状态之一（没导过；正在导，第几张共几张；上次的结果）。登记进分发链、命令名单、起始页允许的命令、整行命令的名单。程序状态加一项（进行中的进度，上次的结果），消息枚举加一项（导出结束）。导出进行中，`PIDBACKUPOPEN Cache=Clear` 被拒绝。导出函数在命令层可以换掉，快测试用替身。不加自动化操作、REST、MCP 的入口。
- **验收**：
  - [ ] 没有打开备份包：`SPPID_PDF_NO_PACKAGE`，不建目录，不写文件。
  - [ ] 路径带空格、两头带引号都对；整行只执行一次，只导到给的目录。
  - [ ] 命令返回时状态已经是「正在导」，而替身还没放行；这时不带参数的命令报「第几张，共几张」；再发一次导出 → `SPPID_PDF_BUSY`；`PIDBACKUPOPEN Cache=Clear` 被拒绝。放行后都恢复。
  - [ ] 结束后命令行有汇总：写出几张，出不了的逐张点名，用时，清单的位置。不带参数的命令原样再报一遍。
  - [ ] 替身返回错误：命令行有错误码和原因，程序继续运行，之后可以再导。
  - [ ] 导出前后，最前面那张图的「已修改」标记、撤销栈、标签页数都不变。
  - [ ] 新界面下（不加 `--classic` 的建法）同样可用，有一条测试。
  - [ ] `src/rest_openapi.json` 和 MCP 的摘要值没有变化。
- **验证**：`cargo test --offline --locked --lib pidbackuppdf`；`git diff --stat -- src/rest_openapi.json src/mcp.rs`（应为空）
- **依赖**：PDF-3
- **文件**：新建 `src/app/commands/pidbackuppdf.rs`；改 `src/app/commands/mod.rs`、`src/app/command_driver/utilities.rs`、`src/app/commands/pidbackup.rs`、放消息枚举和程序状态的文件
- **规模**：M

### PDF-5 【脚本】真实包上的 PDF 比对和读数

- **做什么**：`scripts/sppid/pdf-compare.ps1`，加几条走命令的真实包 `#[ignore]` 测试（规格 §7）。脚本出一张表，有一格不过退出码非 0，输出都在仓库外。做五件事。
  (1) 全厂包、小包（目录和 zip）各用本程序整包导一遍；用 `PDF-1` 的 `pid_plot --batch` 对同一批文件导一遍；两边设同一个 `PID_PLOT_FONTS`；逐个比字节（成功标准 5、9）。
  (2) 每份 PDF 用 `lopdf` 解析：恰好一页，页面大小和清单相差不超过 0.01 毫米（成功标准 6）。
  (3) 同一个包导两次，PDF 和 `_PdfIndex.json` 逐字节相同（成功标准 10）。
  (4) 厂商的 11 张：对厂商文件夹里的 11 个 `.pid`（厂商 PDF 对应的是这一版）用本程序的出图函数各出一份，用 `pid_plot_compare` 和厂商 PDF 比，读数和 `plot\acceptance\` 里的记录比；容差：覆盖率 0.001，长度 1 毫米（成功标准 8）。
  (5) 包根目录和还原目录在导出前后的文件数和校验和不变（成功标准 12 的后半）。
  读数和用时记进 `readings.md`，注明是哪种构建（成功标准 13）。量出来有不同的：停下，列出是哪几张、差在哪里，报告后再定。
- **验收**：
  - [ ] 全厂包：本程序写出的张数不少于 `pid_plot --batch`；两边都写出的图逐字节相同。张数和不同的图（应为 0）在表里。
  - [ ] 小包从目录和从 zip：PDF 名单相同，逐字节相同。SQL Server 包：能导就导，汇总里有「没有验收过」。
  - [ ] 每份 PDF 恰好一页，页面大小对得上。
  - [ ] 重跑两次的输出逐字节相同，含 `_PdfIndex.json`。
  - [ ] 厂商 11 张：每张的覆盖率、多画的长度、文字对上的条数，旁边是记录的数；没有一张比记录差。
  - [ ] 包根目录、还原目录在导出前后不变。
  - [ ] 全厂包的用时在 `readings.md` 里；测试构建下超过 600 秒就先报告。
  - [ ] 把一个期望值改错再跑：对应的一格标不过，退出码非 0。
- **验证**：`powershell -NoProfile -ExecutionPolicy Bypass -File scripts\sppid\pdf-compare.ps1`
- **依赖**：PDF-4
- **文件**：新建 `scripts/sppid/pdf-compare.ps1`；`src/app/commands/pidbackuppdf.rs`（真实包的 `#[ignore]` 测试）；`readings.md`
- **规模**：M

### PDF-6 【测试】新界面下单张出图

- **做什么**：成功标准 11。快测试：新界面下从备份面板打开一张图之后执行 `PIDPLOT <路径>`，命令走到出图那一步，用的图纸路径是还原目录里的那个 `.pid`。真实包：从全厂包打开 `DEP-2` 定的 3 张图中的一张，`PIDPLOT <路径>` 写出的 PDF 和 `PDF-5` 整包导出里这张图的 PDF 比字节。
- **验收**：
  - [ ] 快测试：新界面下 `PIDPLOT` 用的图纸路径是还原目录里的 `.pid`；没有打开图时的报错和 `--classic` 下相同。
  - [ ] 真实包：单张出的 PDF 和整包出的同一张逐字节相同。
  - [ ] 读数记进 `readings.md`。
- **验证**：`cargo test --offline --locked --lib pid_backup_in_shell`；`cargo test --offline --locked --lib pidplot_real -- --ignored --nocapture`（真实包）
- **依赖**：PDF-5
- **文件**：`src/app/pid_shell/pid_backup_in_shell.rs`；`src/app/commands/pidplot.rs`（真实包的 `#[ignore]` 测试）；`readings.md`
- **规模**：S

---

## 第 3 段：导出 publish 文件

目标：一条命令把已打开的备份包导成 publish 文件，每张图都有交代；写出的文件和 `pid_publish_xml` 命令行工具写出的逐字节相同。规格：`SPEC-sppid-export.md`。做完是检查点 B。

### EXP-0 【依赖】再升一次，带上导出的 3 个提交

- **做什么**：前提是你已经把 `d3fbe3e`、`3253301`、`6ecd1b4` 推到 `pid-parse` 主线，并同意再联网一次。六处 `rev` 改成你给的新主线提交，`cargo fetch` 一次，看 `Cargo.lock` 的变化（预期只有 `pid-parse` 仓库那几个包的提交号变）。这 3 个提交只改导出，读取和 PDF 不应当变：重跑 `walk-samples.ps1` 和 `pdf-compare.ps1`，读数要和升级前相同。重编命令行工具（`build-pid-tools.ps1`）。
  **到这一步时还没推送**：不等，先按已钉的版本做 `EXP-1` – `EXP-3`；本任务顺延到推送之后，`EXP-3` 的比对届时重跑一遍（`plan.md` §7）。如果 `DEP-3` 时已经钉了含这 3 个提交的版本，本任务取消。
- **验收**：
  - [ ] 六处 `rev` 相同，指向你给的新主线提交；`cargo metadata --offline --locked` 成功。
  - [ ] `git diff Cargo.lock` 只有 `pid-parse` 仓库那几个包的提交号变了；有别的变化就停下报告。
  - [ ] `walk-samples.ps1` 全过，读数和 `RD-3` 的相同。
  - [ ] `pdf-compare.ps1` 全过；PDF 和 `PDF-5` 那次逐字节相同。不同的逐张列出，指到引起变化的提交。
  - [ ] 门禁不退步；协同两个 crate 全过。
- **验证**：`cargo metadata --offline --locked --format-version 1 > $null`；`build-pid-tools.ps1`、`walk-samples.ps1`、`pdf-compare.ps1` 各一次；§1 的门禁，含协同两个 crate
- **依赖**：PDF-6；你推送那 3 个提交并同意联网
- **文件**：`Cargo.toml`、`Cargo.lock`；`readings.md`
- **规模**：S

### EXP-1 【纯】导出的引擎

- **做什么**：新文件 `src/io/pid_backup_export.rs`，接口照规格 §5.1。前提检查（规格 §3.3 的错误码）：来源不是备份包，输出目录不是空的，输出路径不能用，参考目录不能用。调 `pid_parse::publish::backup_package::publish_backup_package`，交的是包根目录，不是还原目录。导出后读 `_Index.json`（规格 §3.2 的字段），整理出结果：没写出的图和原因，带提醒的图，图框模板数，没对上的 `.pid`，整包问题，是不是 SQL Server 包。`_Index.json` 读不了按失败报。`summary_lines` 出命令行的汇总（20 条上限）。`pid-parse` 的两种错误对应到 `SPPID_EXPORT_UNSUPPORTED` 和 `SPPID_EXPORT_FAILED`。另有一个在后台线程执行的入口。
- **验收**：
  - [ ] 来源没有 `Manifest.txt` → `SPPID_EXPORT_NOT_A_BACKUP`。输出目录非空 → `SPPID_EXPORT_TARGET_NOT_EMPTY`，目录里的文件名单和校验和前后相同。输出路径是文件 → `SPPID_EXPORT_TARGET_INVALID`。参考目录不存在 → `SPPID_EXPORT_REFERENCE_INVALID`。四种情况都什么也不写。
  - [ ] 从一段 `_Index.json` 文本整理出结果：没写出的图（`files` 为空）带原因；写出但有提醒的图；模板数；没对上的 `.pid`；整包问题逐句照录；`export` 是 `SqlServerMtf` 时带「没有验收过」。
  - [ ] `_Index.json` 不存在，或者不是合法的 JSON → `SPPID_EXPORT_FAILED`。
  - [ ] `summary_lines`：句子和规格 §5.2 的样例一致；没写出的图超过 20 张时只列前 20 张，并说明总数。
  - [ ] 每种错误的码都有测试。
  - [ ] `python scripts/arch_check.py report` 里 `io` 开头各行的数字没有变大。
- **验证**：`cargo test --offline --locked --lib pid_backup_export`；`python scripts/arch_check.py report`
- **依赖**：EXP-0（那 3 个提交还没推送时不等它，先按已钉的版本做）
- **文件**：新建 `src/io/pid_backup_export.rs`，在 `src/io/mod.rs` 登记
- **规模**：M

### EXP-2 【接线】命令 `PIDBACKUPEXPORT`

- **做什么**：新文件 `src/app/commands/pidbackupexport.rs`（规格 §5.2）。`PIDBACKUPEXPORT [Plant=<工厂名>] [Ref=<参考目录>] <输出目录>`，工厂名默认 `P01`。不带参数：报状态（没导过；正在导出，已用多少秒；上次的结果）。登记进分发链、命令名单、起始页允许的命令、整行命令的名单。程序状态和消息枚举各加一项。导出进行中 `PIDBACKUPOPEN Cache=Clear` 被拒绝，和 `PDF-4` 加的那一处合成一个判断。导出函数可以换掉，快测试用替身。网页版报「网页版不支持」。publish 文件和 PDF 可以同时导，各有各的状态，互不拒绝（`SPEC-sppid-pdf.md` §5.5）。
- **验收**：
  - [ ] 没有打开的备份包，或者还在载入 → `SPPID_EXPORT_NO_PACKAGE`。
  - [ ] 路径带空格、带引号都对；`Plant=`、`Ref=` 两种顺序都对，值带空格时用引号；整行只导一次，只导到给的目录。
  - [ ] 命令返回时状态已经是「正在导出」，而替身还没放行；这时不带参数的命令报已用时间；再发一次导出 → `SPPID_EXPORT_BUSY`；`PIDBACKUPOPEN Cache=Clear` 被拒绝。
  - [ ] 替身返回错误：命令行有错误码和原因，程序继续运行，状态变为「上次的结果（失败）」，之后可以再导。
  - [ ] 成功时命令行的汇总和规格的样例一致；给了参考目录多一行比对结论；SQL Server 包多一句「没有验收过」。
  - [ ] 导出前后，最前面那张图的「已修改」状态、撤销栈、标签页数都不变。
  - [ ] PDF 正在导时发 `PIDBACKUPEXPORT <另一个目录>` 照常开始，反过来也是；两样各报各的状态。
  - [ ] `src/rest_openapi.json` 和 MCP 的摘要值没有变化。
- **验证**：`cargo test --offline --locked --lib pidbackupexport`；`git diff --stat -- src/rest_openapi.json src/mcp.rs`（应为空）
- **依赖**：EXP-1
- **文件**：新建 `src/app/commands/pidbackupexport.rs`；改 `src/app/commands/mod.rs`、`src/app/command_driver/utilities.rs`、`src/app/commands/pidbackup.rs`、放消息枚举和程序状态的文件
- **规模**：M

### EXP-3 【脚本】真实包上的导出比对和读数

- **做什么**：`scripts/sppid/export-compare.ps1`，加几条走命令的真实包 `#[ignore]` 测试（规格 §7）。用 `PDF-1` 的脚本编出的 `pid_publish_xml` 导一份；本程序走命令导一份（同一个包路径，同一个工厂名）；两个目录按校验和比；出一张表，有一格不过退出码非 0。覆盖成功标准 1–6、11–13。另在真实窗口里看一次（成功标准 8 的后半）：导出的十几秒里，图面能平移缩放，命令行能敲命令。读数记进 `readings.md`。做完发检查点 B 的汇报（`plan.md` §5）。文中的数（53 张、2 张没有 `DrawingNo`、7 张一致等）是 2026-11-18 用现成的命令行程序量的；验收时以钉的那个版本的命令行程序为准，数不同就写明。
- **验收**：
  - [ ] 全厂包：目录里恰好 160 个文件（53 + 53 + 53 + 1），没有临时文件。
  - [ ] 命令行报：写出 53 张，失败计数 0，`.pid` 53 个全部对上，2 张没有 `DrawingNo`。这些数与同一版本的 `pid_publish_xml` 报的相同。
  - [ ] 106 个 `_Data.xml`、`_Meta.xml` 逐字节相同；54 个 `_Coverage.json`、`_Index.json` 也逐字节相同。
  - [ ] 带参考目录（`Ref=D:\work\cad\pid-test-data\验收数据`）：比对结论与命令行程序加 `--diff-against` 的结论相同。
  - [ ] 小包从目录和从 zip：都是 22 个文件、写出 7 张；两次的 `_Data.xml`、`_Meta.xml` 逐字节相同。
  - [ ] 小包的副本里换坏一个 `.pid`：这张图在没写出的名单里并带原因，其余 6 张照常写出，失败计数不为 0，`_Index.json` 里这张图的 `files` 为空。
  - [ ] 导出前后，包根目录里的文件名单、大小、修改时间不变。
  - [ ] SQL Server 包：能导，写出 1 张，另 11 个 `.pid` 记为图框模板，结果里有「没有验收过」。
  - [ ] 全厂包导出在 60 秒内完成。
  - [ ] 真实窗口：导出期间图面能平移缩放，命令行能敲命令，有截图。
  - [ ] 把一个期望值改错再跑：对应的一格标不过，退出码非 0。
  - [ ] 检查点 B 的汇报已发，地图 §5 检查点 B 的 9 条逐条写了过没过。
- **验证**：`powershell -NoProfile -ExecutionPolicy Bypass -File scripts\sppid\export-compare.ps1`
- **依赖**：EXP-2
- **文件**：新建 `scripts/sppid/export-compare.ps1`；`src/app/commands/pidbackupexport.rs`（真实包的 `#[ignore]` 测试）；`readings.md`
- **规模**：M

---

## 第 4 段：总验收

目标：按你定的四条验收，用真实程序和全厂包从头走一遍，出一张对照表和一个交付目录。

### ACC-1 【验收】按四条验收从头走一遍

- **做什么**：用真实程序（不加 `--classic`）和全厂包，按 `plan.md` §5「总验收」的表走：`PIDBACKUPOPEN <全厂包>` → 选图纸，看属性和图内对象，打开图纸 → `PIDBACKUPEXPORT <交付目录>` → `PIDBACKUPPDF <同一个目录>` → 成对检查。小包从 zip 再走一遍打开和两样导出。成对检查写成脚本 `scripts/sppid/check-pairs.ps1`：读 `_Index.json` 和 `_PdfIndex.json`；两样都写出的图，`<图名>.pdf` 和 `<图名>_Data.xml` 一一对上；只写出一样的图逐张列出，原因取自对应的清单；有对不上的退出码非 0。把前面各段的读数汇成一张验收对照表写进 `readings.md`：四条验收各一行，过没过，证据的位置。检查点 A 留下的显示差别和处理结果照录。发总验收的汇报。
- **验收**：
  - [ ] 交付目录里：每张写出的图有 `_Data.xml`、`_Meta.xml`、`_Coverage.json`、`.pdf` 四个文件；另有 `_Index.json`、`_PdfIndex.json`。全厂包应当是 53 × 4 + 2 = 214 个文件；不是这个数就逐个说明。
  - [ ] 成对检查：对不上的 0 个；只写出一样的图都有原因。
  - [ ] 交付目录里的 publish 文件和 `EXP-3` 那次的逐字节相同，PDF 和 `PDF-5` 那次的逐字节相同（清单里记的绝对路径除外）。
  - [ ] 小包从 zip：打开 7 张，两样导出的名单成对。
  - [ ] `readings.md` 里有验收对照表：四条，每条有结论和证据的位置。
  - [ ] 有一条不过：汇报里写明差在哪，不写成通过。
- **验证**：真实程序走一遍；`powershell -NoProfile -ExecutionPolicy Bypass -File scripts\sppid\check-pairs.ps1 <交付目录>`
- **依赖**：EXP-3；检查点 B 的汇报已发
- **文件**：新建 `scripts/sppid/check-pairs.ps1`；`readings.md`
- **规模**：S

---

## 修订记录

| 日期 | 改动 |
|---|---|
| 2026-11-18 | 初稿，待确认 |
| 2026-11-18 | 确认（原话见 `plan.md` 文首），P1–P4、R3 按建议定。跟着 `plan.md` 改了四处说法：`DEP-3` 换版本前先看主线的提交号；`RD-4` 的差别清单每项带处理意见（对应的那条验收加了半句）；`EXP-0`、`EXP-1` 在那 3 个提交没推送时不等；`ACC-1` 照录的是处理结果。任务编号和其余的验收项都没有改 |
