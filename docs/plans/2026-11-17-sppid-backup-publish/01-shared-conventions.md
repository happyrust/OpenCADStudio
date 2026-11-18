# 共用约定（打开备份包、导出 PDF 和 publish 文件）

> 状态：**已确认**（2026-11-18）。同一天按你定的验收标准加了出 PDF 的内容（标着「2026-11-18 补」的几处），这些随 `SPEC-sppid-pdf.md` 一起也已确认。基线：本仓库 `main @ 5b4784058`（本地，未推送）；`pid-parse` 主线 `96ee00c`，本程序钉的是 `d0a58c64c413`。
> 这份文件只写这份计划自己的东西。技术栈、固定门禁、代码写法、测试写法、边界的通用部分沿用 `docs/plans/2026-11-16-pid-dev-spec/01-shared-conventions.md`（下文叫「通用约定」），这里不重复。
> 用到它的文件：`SPEC-sppid-deps.md`、`SPEC-sppid-read.md`、`SPEC-sppid-pdf.md`、`SPEC-sppid-export.md`、`02-screen-content.md`、`plan.md`、`todo.md`。

## 1. 叫法

| 叫法 | 指什么 |
|---|---|
| 备份包 | SmartPlant P&ID 的备份：一个目录或一个 zip，里面有 `Manifest.txt`（清单）、`Export.dmp`（数据库导出）、`PlantData~*`（图纸）、`RefData~*`（参考数据） |
| 包根目录 | 含 `Manifest.txt` 的那个目录。来源是目录时就是它自己；来源是 zip 时是解开后的目录。代码里是 `BackupIndex::backup_root` |
| 还原目录 | 程序把图纸和参考数据解出来放的目录。来源是 zip 时整个解到缓存里；来源是目录时只把图纸和参考数据解到缓存里，原目录不写。代码里是 `BackupIndex::restored` |
| publish 文件 | SmartPlant 格式的发布文件：每张图 `<图名>_Data.xml`、`<图名>_Meta.xml`、`<图名>_Coverage.json`，整包一份 `_Index.json`。由 `pid-parse` 写 |
| 平台发布包 | 本平台自己的发布包（`PIDPUBLISH`，`docs/plans/2026-11-16-pid-dev-spec/SPEC-pid-publish.md`）。和 publish 文件是两回事 |
| PDF | `pid-plot` 从 `.pid` 文件直接出的一页 PDF，一张图一份 `<图名>.pdf`。不经过画布（2026-11-18 补） |
| 厂商文件 | 厂商软件给的验收数据：每张图 `<图名>.pdf`、`<图名>.pid`、`<图名>_Data.xml`、`<图名>_Meta.xml`（2026-11-18 补） |
| 新界面、经典界面 | 启动默认进入的 P&ID 界面（`src/app/pid_shell/`）；加 `--classic` 进入的原界面 |
| 走查 | 不开窗口，把一个真实备份包的每个节点、每张图走一遍并出报告的那条测试（`app::commands::pidbackup::audit`） |
| 全厂包、小包、SQL Server 包 | §2 的三个样本 |

## 2. 样本、参考文件、对比用的程序

都在这台机器上，不在仓库里。

| 名字 | 位置 | 说明 |
|---|---|---|
| 全厂包 | `D:\work\plant-code\cad\pid-test-data` | Oracle 导出，53 张图。`Export.dmp` 71 MB，`PlantData~2~711.zip` 20 MB。`D:\work\cad\pid-test-data` 是内容相同的另一份（三个主要文件的校验和相同） |
| 小包 | `D:\work\plant-code\cad\pid-parse\test-file\backup-test\DWG-0202GP06-01_p`，同目录有它的 zip | Oracle 导出，7 张图 |
| SQL Server 包 | 同目录的 `TEST02_p`，也有 zip | SQL Server 备份。按文件夹列出 12 张，其中 11 张是图框模板 |
| 参考文件（厂商文件） | `D:\work\cad\pid-test-data\验收数据` | 厂商软件给的 11 张图，每张四个文件：`<图名>.pdf`、`<图名>.pid`、`<图名>_Data.xml`、`<图名>_Meta.xml`。子目录 `plot\` 是 `pid-plot` 自己出的 11 份 PDF 和它与厂商 PDF 的比对记录（2026-11-18 补）。`D:\work\plant-code\cad\pid-test-data\验收数据` 那份只有 5 张，不用它 |
| 历次交付 | `D:\work\cad\pid-test-data\publish`、`publish-v2` 到 `publish-v6` | 每份 53 张图、160 个文件。`publish-v6\BUILD.txt` 记着当时的命令和读数 |

- 用到样本的测试都标 `#[ignore]`，路径从环境变量取，默认值是上表的位置。
- **对比用的命令行程序** `pid_publish_xml` 从本程序钉的那个 `pid-parse` 提交构建：用 `git archive` 把那个提交导出到临时目录，在临时目录里构建。不切你的检出的分支，不改你的检出里的文件。
- 2026-11-18 补：出 PDF 用的 `pid_plot`、`pid_plot_compare` 同样办理，三个程序由 `scripts/sppid/build-pid-tools.ps1` 一起编。离线编不出来时（缺依赖的缓存）先报告；可以退到你检出里现成的程序，读数里写明它的构建时间。

## 3. 基线读数

### 3.1 门禁

| 门禁 | 命令 | 基线 |
|---|---|---|
| 库测试 | `cargo test --offline --locked --lib` | 6947 条：6894 过，49 忽略，4 失败。4 条失败是拿代码对设计稿文件的测试，原因是工作区里没提交的设计稿 |
| clippy | `cargo clippy --offline --locked --lib`，再加 `--profile test` 跑一次 | 警告不超过 630 和 965 |
| 格式 | `scripts\fmt-newhunks.ps1 <改前的副本> <改后的文件>` | 新增 0 |
| 协同两个 crate | `cargo test --offline --locked -p ocs_collab -p ocs_collab_server` | 95 + 159 + 12 条，全过 |
| 文档检查 | `python -m pytest scripts/pid-platform -q -p no:cacheprovider` | 590 条：587 过，3 失败（2026-11-18 实测） |

- 前四行是现行计划（`docs/plans/2026-11-16-pid-dev-spec/todo.md` §1）记的数，量的时候离线那批改动还没进工作区。`sppid-deps` 开始前在干净的工作区上重量一次，以那次为准。
- 文档检查的 3 条失败在这份计划动手之前就有：1 条来自没提交的设计稿；2 条来自离线那批没提交的改动（新加的三个错误码还没登记进需求文档的错误码表）。后 2 条随离线那批改动的收尾一起修。
- 文档检查只在动了 `docs/pid-platform/` 时跑。
- 2026-11-18 补：离线那批改动已经入库（`OFF-1`，四个提交，到 `c399311e7`）。现在的读数：库测试 6954 条，6901 过、49 忽略、4 失败；协同两个 crate 96 + 168 + 18 条，全过；文档检查 590 条，589 过、1 失败（没提交的设计稿）。clippy 的上限不变。`sppid-deps` 开始时仍然重量一次，以那次为准。

### 3.2 读取（2026-11-10 的记录，钉的这一版 `pid-parse`）

出处：`docs/plans/2026-11-10-pid-backup-ui-coverage.md`。

| 样本 | 包内文件 | 图纸 | 参考数据文件 | 读不出图内对象 | 打不开 |
|---|---:|---:|---:|---|---|
| 全厂包 | 15（1 个登记了但包里没有） | 53 | 645 | 2 张：`D03`、`D04` | 1 张：`D03` |
| 小包 | 15 | 7 | 0 | 1 张：`001` | 1 张：`001` |
| SQL Server 包 | 14 | 12 | — | 11 张（图框模板） | 2 张：`CPECCHBA2-new`、`CPECCHBA2` |

- 原因的原文：读不出是 `no P&IDAttributes row carries a DrawingNo`；打不开是 `No decoded geometry`。
- 全厂包：图内对象 14 894 个、属性 15 921 个；53 张图的图内对象全部读完 1.3 秒；53 张全部导入 36.5 秒，最慢的 `P井场` 6.2 秒；画出图元 149 310 个，9 条源记录没画出。第一次载入约 0.8 秒，解过的 0.12 秒。
- 小包：图内对象 152 个、属性 70 个。

### 3.3 导出（2026-11-18 实测，命令行程序）

| 样本 | 用时 | 峰值内存 | 写出 | 结论 |
|---|---:|---:|---|---|
| 全厂包 | 14.3 – 15.7 秒 | 约 270 MB | 160 个文件，51.1 MB | 写出 53 张，失败 0，2 张没有 `DrawingNo`。连跑两次，160 个文件逐字节相同 |
| 全厂包，带参考文件 | 14.7 秒 | 约 267 MB | 同上 | 53 张里 7 张一致，0 张有差异，4 张的参考是另一版图纸，42 张没有参考 |
| 小包，从目录 | 0.26 秒 | 约 38 MB | 22 个文件 | 写出 7 张，1 张没有 `DrawingNo` |
| 小包，从 zip 解出的目录 | 同上 | 同上 | 22 个文件 | 与从目录导的相比：`_Data.xml`、`_Meta.xml` 逐字节相同；`_Coverage.json`、`_Index.json` 只差里面记的绝对路径 |
| SQL Server 包 | 0.18 秒 | — | 4 个文件 | 写出 1 张；另 11 个 `.pid` 在清单里记为图框模板 |

- 用的是本机已有的一份 `pid_publish_xml.exe`（2026-11-17 构建，在 `D:\work\plant-code\cad\pid-parse-wt\pub-r` 里）。它对应哪个提交没有重新构建核实，所以这张表只用来定量级；验收时用 §2 说的那份程序重量。
- 历史对照：`publish-v6\BUILD.txt` 记的是 19.3 秒，结论同样是 7 张一致、0 张有差异、4 张另一版、42 张没有参考。

### 3.4 出 PDF（2026-11-18 实测，命令行程序）

| 样本 | 用时 | 写出 | 结论 |
|---|---:|---|---|
| 全厂包的 53 个 `.pid` | 94.9 秒 | 53 份，合计 8.70 MB | 失败 0；图框模板都找到；16 张图共 22 处符号文件找不到；存疑的、没有交代的图元都是 0 |

- 用的是本机已有的一份 `pid_plot.exe`（2026-11-15 构建，在你的 `pid-parse` 检出的 `target\agent-scripts\pdf-batch\` 里），整目录出（`--batch`），参考目录是全厂包目录。它对应哪个提交没有核实，这张表只用来定量级。
- 厂商 11 张图的 PDF，`pid-plot` 自己比过（版本 `6ecd1b4`）：10 张线条覆盖 100%；1 张 98.3%。记录在参考文件目录的 `plot\acceptance\`。
- 本程序自己的出图路径还没有整包的读数。

## 4. 命令

```powershell
# 门禁（每个任务结束时跑；通用约定 §2）
cargo test --offline --locked --lib
cargo clippy --offline --locked --lib
cargo clippy --offline --locked --lib --profile test
scripts\fmt-newhunks.ps1 <改前的副本> <改后的文件>
cargo test --offline --locked -p ocs_collab -p ocs_collab_server

# 动了 docs/pid-platform/ 时
python -m pytest scripts/pid-platform -q -p no:cacheprovider

# 分层检查：io 不引用 app、ui（实现前后各跑一次，io 开头各行的数字不变大）
python scripts/arch_check.py report

# 走查一个真实备份包（手动）
$env:OCS_PID_BACKUP_AUDIT = 'D:\work\plant-code\cad\pid-test-data'   # 目录或 zip
$env:OCS_PID_BACKUP_AUDIT_OUT = 'D:\scratch\ocs-sppid\<日期>-<说明>'   # 报告写到这里
$env:OCS_PID_BACKUP_AUDIT_LANG = 'zh-CN'
cargo test --offline --locked --lib app::commands::pidbackup::audit -- --ignored --nocapture

# 命令行导出（对比用）
pid_publish_xml <包根目录> --out-dir <输出目录> [--plant <工厂名>] [--diff-against <参考目录>]

# 命令行出 PDF、和参考 PDF 比（对比用；2026-11-18 补）
pid_plot --batch <放 .pid 的目录> <输出目录> [参考目录]
pid_plot_compare <参考.pdf> <我们的.pdf> <输出目录> <图.pid> [参考目录]
```

`SPEC-sppid-read.md`、`SPEC-sppid-pdf.md`、`SPEC-sppid-export.md` 各加一个把上面几步串起来的脚本，放在 `scripts/sppid/`。

## 5. 文件位置

```text
Cargo.toml、Cargo.lock                 钉的 pid-parse 版本（sppid-deps）
src/io/pid_backup.rs、pid_backup/      还原（已有，不重写）
src/io/pid_backup_browse.rs、…/        索引（已有，不重写）
src/io/pid_backup_export.rs            导出的引擎（sppid-export 新建，不碰界面）
src/io/pid_pdf.rs                      出一张 PDF 的引擎（已有，sppid-pdf 改三处）
src/io/pid_backup_pdf.rs               整包出 PDF 的引擎（sppid-pdf 新建，不碰界面）
src/app/commands/pidbackup.rs          PIDBACKUPOPEN（已有）
src/app/commands/pidbackup/audit.rs    走查（已有，sppid-read 加两个开关）
src/app/commands/pidbackupexport.rs    PIDBACKUPEXPORT（sppid-export 新建）
src/app/commands/pidplot.rs            PIDPLOT（已有，命令不改）
src/app/commands/pidbackuppdf.rs       PIDBACKUPPDF（sppid-pdf 新建）
src/app/pid_shell/                     新界面；这三块只往里加测试，不加画面
src/ui/window/pid_backup_browser*      经典面板（已有，不改）
scripts/sppid/                         这份计划的脚本（走查汇总、前后对比、编对比用的程序、PDF 对比、导出对比、成对检查）
docs/plans/2026-11-17-sppid-backup-publish/
  00-capability-map.md                 能力地图（已确认）
  01-shared-conventions.md             本文件
  02-screen-content.md                 给 Pencil 用的画面内容清单
  SPEC-sppid-deps.md / -read.md / -pdf.md / -export.md
  plan.md、todo.md                     实施计划和任务清单
  readings.md                          读数表和差异说明（sppid-deps 开始时新建）
docs/pid-platform/requirements/90-migration-and-interop.md   REQ-MIG-008、009、010
```

## 6. 代码写法

只写这份计划特有的，其余见通用约定 §4。

**后台的活**照还原那一段的写法：开一个有名字的线程，做完用一次性的通道把结果送回，界面那边用 `Task::perform` 接。不引入异步运行时。现有的样子（`src/io/pid_backup.rs`）：

```rust
pub async fn restore_in_background(
    source: PathBuf,
    target: Option<PathBuf>,
) -> Result<Restored, String> {
    let (sender, receiver) = iced::futures::channel::oneshot::channel();
    std::thread::Builder::new()
        .name("ocs-pid-backup".into())
        .spawn(move || {
            let _ = sender.send(restore(&source, target.as_deref()));
        })
        .map_err(|error| format!("cannot start the restore: {error}"))?;
    receiver
        .await
        .unwrap_or_else(|_| Err("the restore stopped before it finished".into()))
}
```

**文字和错误。** 新加的命令（`PIDBACKUPEXPORT`、`PIDBACKUPPDF`）给人看的文字直接写中文，错误是带码的枚举（`code()` 返回 `SPPID_EXPORT_…`、`SPPID_PDF_…`），与通用约定一致。经典面板的文字走的是另一套（源码里写英文，经 `src/locale_catalog.rs` 对到 23 个语言文件，`src/i18n.rs` 有测试盯着），那一套不动，新命令也不往里加。

**分层。** `src/io/` 下的新模块不引用 `app`、`ui`（`docs/architecture/RULES.md`）。

**转换的内容不在本程序里决定。** 写 publish 文件的只有 `pid-parse`。本程序不改、不补、不重排它写出的任何文件。PDF 的页面内容只由 `pid-plot` 决定，本程序只定出哪些图、放在哪、叫什么名字。

## 7. 测试办法

| 层 | 什么时候跑 | 用什么数据 |
|---|---|---|
| 快测试 | 随库测试，每个任务都跑 | 测试里现造的小备份目录（照 `src/app/commands/pidbackup.rs` 现有测试的造法）。仓库里没有真实的 `.pid` 文件 |
| 真实包测试 | 手动，带 `--ignored`。升级前后、检查点、动了读取或导出的代码之后 | §2 的样本 |
| 真实窗口 | 检查点 A、B 各一次 | 启动真实程序，用它自己的自动化接口发命令、取截图 |

- 先红后绿照通用约定。功能本来就通的地方，测试一写出来就是绿的；这种测试先用一个故意错的期望值看它失败，再改回来，两次的输出都留。
- **证据放哪。** 报告全文、截图、导出的文件放仓库外：`D:\scratch\ocs-sppid\<日期>-<说明>\`（脚本里可以用参数另指）。仓库里只提交读数表和差异说明，写在 `readings.md`，每组读数注明日期、本程序的提交、`pid-parse` 的提交。
- 改一条测试的期望值，必须写原因：哪条测试，原来的值，现在的值，为什么变。说不出原因的不改。

## 8. 边界

通用约定 §6 的三类照旧。这份计划另加：

**总是做**

- 样本包、参考文件、你的 `pid-parse` 检出只读。实测的输出写到仓库外。
- 读 `Manifest.txt`、数据库导出文件时，不把里面的账号、口令打到输出、日志、文档里。
- 升级前的读数先留好，再动 `Cargo.toml`。
- 报告里每个数写明是哪一天、哪个提交量的。

**先问**

- 联网。`sppid-deps` 取 `pid-parse` 新提交的那一次已经同意（2026-11-18）。做导出之前再升一次时还要联网一次，到时候再问。
- 钉的提交换成规格定的那个之外的。
- 改 `pid-parse` 仓库里的任何东西。
- 给导出加选图、进度、取消（要先改 `pid-parse`）。
- 改经典面板的行为。
- 把样本包或参考文件的内容放进仓库。
- 推送。

**不做**

- 改或提交设计稿文件 `docs/pid-platform/design/pid-platform.pen`。
- 在你的 `pid-parse` 检出里切分支或改文件。
- 删测试、加 `#[ignore]`、放宽断言来让它通过；为了让读数好看而改期望值。
- 在本程序里改写 `pid-parse` 写出的文件。
- 在本程序里改 PDF 的页面内容。
- 把 SQL Server 包的导出说成验收过的。
