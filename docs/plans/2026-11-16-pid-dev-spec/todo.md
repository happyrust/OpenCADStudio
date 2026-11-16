# P&ID 协同绘图开发方案 · 任务清单

> 日期：2026-11-16
> 状态：**已确认**（2026-11-16）。从 `CC-1` 开始串行做，做完一个勾一个。
> 配套：`plan.md`（阶段、依赖、检查点、风险、哪些能同时做）。数据格式与数据怎么传写在各模块的 `SPEC-*.md`，共用约定在 `01-shared-conventions.md`。
> 基线：`main @ 9b5327f96`。

## 0. 怎么读这份清单

共 67 个任务，分五个阶段。阶段用字母 A–E，是为了不和已批准计划（`docs/plans/2026-11-16-pid-platform-next-steps-plan.md`）里的「阶段 1–4」混：A 是那份计划阶段 2 的收尾，B–E 合起来是它的阶段 3。

| 阶段 | 内容 | 任务 | 对应已批准计划 |
|---|---|---|---|
| A | 协同收口 | `CC-1` – `CC-8` | T2.4 ③④ |
| B | 主数据与项目索引 | `MD-1` – `MD-4`、`IX-1` – `IX-8` | T3.1 |
| C | 规则检查、材料清单、快照 | `DRC-1` – `DRC-14`、`BOM-1` – `BOM-6`、`SNAP-1` – `SNAP-6` | T3.2，T3.3 的前两块 |
| D | 发布 | `PUB-1` – `PUB-7` | T3.3 的第三块 |
| E | 「校验与交付」面板与收尾 | `PNL-1` – `PNL-13`、`END-1` | T3.4 |

任务编号是「模块前缀 + 序号」，定下后不再改。规格里待决问题的编号（`C3`、`S1`、`D4` 这类单个字母加数字）是另一套，本文提到时都带上规格名。

每个任务写六项：

- **做什么**：要实现的行为，以及数据格式在规格的哪一节。
- **验收**：可以逐条打勾的结果。
- **验证**：证明它做完的命令。除这里列的之外，每个任务都要过 §1 的固定门禁。
- **依赖**：必须先做完的任务。
- **文件**：预计要动的文件。
- **规模**：XS（一个文件里的小改动）、S（1–2 个文件）、M（3–5 个文件）、L（5 个以上；只在拆不开时用，并写明原因）。

标题里的类别标记决定任务能不能同时做：

- 【纯】只动本模块在 `src/pid/` 下自己的文件，外加在 `src/pid/mod.rs` 登记一行。不同模块的【纯】任务可以同时做。
- 【接线】要改七个共用文件中的任何一个：`src/app/commands/mod.rs`、`src/app/command_driver/utilities.rs`、`src/app/collab/apply.rs`、`src/app/control/mod.rs`、`src/rest.rs`、`src/rest_openapi.json`、`src/mcp.rs`。每个模块都要在这几个文件里各加几行，`src/mcp.rs` 里还有一个钉死的摘要值 `TOOL_SCHEMA_DIGEST`，两个会话同时改一定冲突。**同一时间只让一个会话做【接线】任务。**
- 【界面】停靠面板与属性表。
- 【协同】`src/app/collab/` 与两个协同 crate。
- 【存盘】改保存路径（`src/io/mod.rs`、`src/app/automation.rs`、`src/app/update/file.rs`）。只有 `DRC-11` 一个，是整份清单里最容易伤到现有功能的任务，单独做。
- 【验收】只加测试、不加功能（`PUB-7`）。
- 【脚本】【文档】不动 Rust 代码。

## 1. 每个任务都要过的固定门禁

下面各任务的「验证」不再重复这几条。命令在仓库根目录执行，Windows PowerShell 5.1；读数的出处是共用约定 §2。

1. **先红后绿。** 先写测试，看到它因为要测的那条断言失败（不是因为编译不过），再实现到通过。红、绿两次的输出留作证据。
2. `cargo test --offline --locked --lib`：失败的仍然只是那 4 条比对设计文件的测试，通过数不减少（基线 6865 通过、49 忽略、4 失败）。
3. `cargo clippy --offline --locked --lib` 告警不超过 630；`cargo clippy --offline --locked --lib --profile test` 不超过 965。
4. 改动的文件不新增 `rustfmt` 不一致（`D:\scratch\ocs-main-20261116\fmt-newhunks.ps1 <改前> <改后>`；这个脚本在仓库外，`CC-5` 把它收进仓库，之后用 `scripts\fmt-newhunks.ps1`）。
5. 标【协同】的任务加跑 `cargo test --offline --locked -p ocs_collab -p ocs_collab_server`（基线 95；159 + 12，全过）。
6. **一个任务一个提交。** `git add` 只加本任务列出的文件（不用 `git add -A`），外加这份 `todo.md`：把本任务的验收项打勾，随同一个提交进去。`git commit -q -F <说明文件>`。`docs/pid-platform/design/pid-platform.pen` 不进提交。不推送。
7. 实现与规格有出入时，在该模块规格末尾加一节「实现记录」记下来，不悄悄改口径。

## 2. 测试怎么写、放在哪

- 纯函数的测试写在同文件的 `#[cfg(test)] mod tests` 里。
- 命令和自动化操作的测试写在命令文件自己的测试模块里，写法照 `src/app/control/p1_tests.rs`：`OpenCADStudio::new_for_test()` 起一个程序对象，用 `automation_op` 发 `{"op":"run","cmd":"…"}` 或具体操作，断言应答里的 `ok`、`code` 和数据。
- 要读写文件的测试用 `std::env::temp_dir()` 下带唯一后缀的目录（照 `src/app/control/save_tests.rs`），测完删掉。
- **不加依赖。** 仓库已有 `serde_json`、`regex`、`sha2`、`calamine`；没有随机数库和临时目录库。要随机输入的测试自带一个小的伪随机生成器。确实需要新依赖时先停下来问。
- 性能预算测试标 `#[ignore]`，取 3 次的中位数，写法照 `src/pid/perf_budget.rs`，但写在本模块自己的文件里，免得同时做的模块撞同一个文件。2,000 个对象的合成图由 `IX-4` 提供，后面的模块都用它。
- 验证命令里的过滤名（如 `master_data`）是建议的测试名前缀，写测试时照它起名。

---

## 阶段 A：协同收口

目标：把协同这条链收干净——大属性对象两端一致，服务器能让别的机器连，228 条验收契约全部有证据，状态文档结项。

### CC-1 【协同】大属性符号：新建时带全属性（已批准计划 T2.4 ③）

- **做什么**：属性文本超过 2048 字节的对象，属性不存在实体上，而在扩展字典的溢出记录里（`SPEC-pid-author-store.md` §2.3）。发送端读的是实体的前后影像，读不到这部分，所以现在新建这样的符号时 `insert_symbol.props` 发出去是空对象（`src/app/collab/broadcast.rs` 的 `placed_symbol`）。`collect_step_changes` 手里有这一步做完后的图：属性溢出的新符号改成用 `PidAuthor::load` 从图里读全。撤销一次删除等于把符号放回来，走的是 `collect_undone_step_changes`，同样处理。协议不改。背景见 `SPEC-collab-client.md` §8.1。
- **验收**：
  - [x] 一步里新建属性超过 2 KB 的符号，读出的 `insert_symbol.props` 含全部字段。
  - [x] 接收端收到这样的 `insert_symbol`，写进本图后属性逐项相等，并且同样存在扩展字典里。
  - [x] 两端往返：A 新建 → B 落地，两张图读出的记录相等；服务器的对象读回里这些字段都在。
- **验证**：`cargo test --offline --locked --lib collab_large_props`
- **依赖**：无
- **文件**：`src/app/collab/broadcast.rs`；新建测试文件 `src/app/collab/collab_large_props.rs`，在 `src/app/collab/mod.rs` 登记
- **规模**：S
- **补（2026-11-16）**：管线、管路、管件有同样的缺口（`src/app/collab/pipes.rs` 三处），上面的任务文字没写到，实现时发现，紧接着单独补了一个提交，测试在同一个文件里（过滤名不变，共 11 个）。记录见 `SPEC-collab-client.md` §10.1。

### CC-2 【协同】大属性对象：之后改属性也广播（`SPEC-collab-client.md` 待决 C3）

- **做什么**：现在只要前后任一侧的属性不在影像里，`changes_of` 就不发 `set_property`，也没有任何提示。改成：「改后」的属性从这一步做完后的图里读；「改前」的属性从撤销步骤里读——步骤的 `structure` 字段存着这一步动过的非实体内容在另一侧的样子（`src/app/tab_document.rs` 的 `DeltaSnapshot`），溢出记录属于这一类。撤销、重做各有一处收集变更的调用（`src/app/history.rs`），一并改。协议不改。
  **第一步先写一个测试确认前提**：改这类对象的一个属性，产生的步骤里确实带着溢出记录改前的内容。这一点我只读了数据结构，没有跑过。前提不成立就停下来报告——那说明这类对象的属性修改本身撤销不干净，是另一个问题，要先商量怎么处理。
- **验收**：
  - [ ] A 改大属性对象的一个属性 → B 的该字段跟着变，其余字段不动；发出的是一条带正确旧值的 `set_property`。
  - [ ] 属性从 2 KB 以内改到超过、或从超过改回以内的那一步，同样只发改动的字段。
  - [ ] A 撤销、重做这一步，B 跟着退回、再恢复。
- **验证**：`cargo test --offline --locked --lib collab_large_props`
- **依赖**：CC-1
- **文件**：`src/app/collab/broadcast.rs`、`src/app/collab/apply.rs`（`broadcast_history_delta`）、`src/app/history.rs`（撤销、重做两处调用）、`src/app/collab/collab_large_props.rs`；可能要在 `src/app/tab_document.rs` 加一个读取函数
- **规模**：M

### CC-3 【协同】协同服务器加 `--bind <地址>`（`SPEC-collab-server.md` 待决 S1）

- **做什么**：服务器现在写死监听 `127.0.0.1`（`crates/ocs_collab_server/src/main.rs`），别的机器连不上。加启动参数 `--bind <IP 地址>`，默认仍是 `127.0.0.1`。用法说明和服务器规格里写明：服务器没有登录验证、不加密，监听局域网地址只适合可信内网。这一条放宽了当初 mission 定的「服务只监听 `127.0.0.1`」，默认行为不变。
- **验收**：
  - [ ] 不带 `--bind` 时监听地址与现在相同；`--bind 0.0.0.0` 解析成功并用于监听；`--bind abc` 报错并给出用法。
  - [ ] 启动时打印的监听地址就是实际绑定的地址。
- **验证**：`cargo test --offline --locked -p ocs_collab_server`；手工：带 `--bind 0.0.0.0` 启动后，用本机的局域网地址访问它的 HTTP 接口能得到应答
- **依赖**：无
- **文件**：`crates/ocs_collab_server/src/main.rs`、`docs/plans/2026-11-16-pid-dev-spec/SPEC-collab-server.md`
- **规模**：S

> **检查点 CP-A1**（CC-1 – CC-3 之后）：固定门禁全过，含协同两个 crate；大属性对象新建、改属性、撤销重做三种情形两端一致。

### CC-4 【文档】228 条验收契约与测试、脚本的对照表

- **做什么**：结项要求 228 条契约每条都有证据。这一步先不跑，只对照：契约正文在 mission 目录的 `validation-contract.md`（只读，路径见状态文档开头），各区的条数见状态文档 §4。逐条对到「由哪个单元测试、或哪个端到端脚本的哪一项覆盖」，做成一张表。端到端脚本现在在仓库外三个目录：`D:\scratch\m6-validation\`、`D:\scratch\m7-validation\`、`D:\scratch\ocs-collab-ops\e2e\scripts\`。重点是跨区流程 12 条：按文件名看，`VAL-CROSS-003`（画管线后两端一致）、`VAL-CROSS-008`（离线编辑后对账）没有单独的脚本，`VAL-CROSS-009` 的脚本在第三个目录。表里另列两类缺口：没有脚本的契约；有脚本、但脚本早于后来的改动（离线排队、管线广播、跨图连接符、`existing_tags`、`overrides`、`tag_source`、大属性对象）因而要补断言的契约。
- **验收**：
  - [ ] 228 条每条一行，写明覆盖它的测试名，或脚本名加项目号；没有覆盖的明确标「缺」。
  - [ ] 各区的行数与状态文档 §4 的条数一致。
  - [ ] 缺口单独列出，每条写明要补什么。
- **验证**：数表的行数；与状态文档 §4 逐区核对
- **依赖**：无
- **文件**：新建 `scripts/collab-e2e/README.md`（对照表，加怎么跑）
- **规模**：M

### CC-5 【脚本】端到端脚本收进仓库（`SPEC-collab-client.md` 待决 C5）

- **做什么**：把 CC-4 对照表里用到的脚本、脚手架（`m6h.py`、`m7h.py`）、种子文件、总入口（`run_all.ps1`、`run_cross.ps1`）复制到 `scripts/collab-e2e/`。脚本里写死的本机路径（证据目录、可执行文件、种子图）改成从参数或环境变量取。截图、日志、数据库、可执行文件一律不收，证据仍然写到仓库外。收之前逐个文件检查，不得带进口令、令牌、内网地址：已知证据日志里有一行带服务器 IP 的窗口标题（状态文档 §18.6 之 4），日志本来就不收，脚本里若有同类内容要去掉。固定门禁第 4 条用的格式检查脚本 `fmt-newhunks.ps1`（35 行）也在仓库外，一并收成 `scripts/fmt-newhunks.ps1`：它拿一个文件改前的副本和改后的文件各跑一次 `rustfmt --check`，列出新增的不一致；里面写死了 `rustfmt.exe` 的本机路径，改成从 `PATH` 找（可用参数另指）。此后门禁用仓库里的这一份。
- **验收**：
  - [ ] 从仓库里的位置执行冒烟场景（`t00_smoke`）通过，证据写在仓库外；仓库里的格式检查脚本与仓库外那份对同一对文件的输出相同。
  - [ ] `scripts/collab-e2e/` 里没有图片、日志、数据库文件；它和 `scripts/fmt-newhunks.ps1` 里全文搜不到写死的 `D:\scratch`、用户目录路径，以及 `127.0.0.1` 之外的 IP 地址。
  - [ ] `git status` 里除这个目录和 `scripts/fmt-newhunks.ps1` 外没有别的新增文件。
- **验证**：执行冒烟场景；`Get-ChildItem scripts\collab-e2e, scripts\fmt-newhunks.ps1 -Recurse -File | Select-String -Pattern 'D:\\scratch|D:\\Rust|C:\\Users'` 没有输出；任取一个在 `9b5327f96` 里改过的 `.rs` 文件，用 `git show 9b5327f96~1:<路径>` 存出改前的副本，两份格式检查脚本各跑一次 `<脚本> <改前的副本> <现在的文件>`，输出相同
- **依赖**：CC-4
- **文件**：`scripts/collab-e2e/` 下约 40 个脚本与种子文件；新建 `scripts/fmt-newhunks.ps1`
- **规模**：M（文件多，但都是搬运加改路径，不改逻辑）

### CC-6 【脚本】补齐对照表里缺的场景

- **做什么**：按 CC-4 的缺口清单补脚本或补断言。预计是 `VAL-CROSS-003`、`VAL-CROSS-008` 两个新脚本，把 `VAL-CROSS-009` 的脚本并进总入口，再给已有脚本补上大属性对象的断言；以对照表为准。新脚本照现有脚本的写法：起真实的服务器和两个桌面端，只走 `--http`，每一项断言打印通过与否并落证据。**缺的脚本超过 3 个时先停下来说明，再拆任务。**
- **验收**：
  - [ ] 对照表里不再有标「缺」的行。
  - [ ] 新增和改过的每个脚本单独执行全部通过。
- **验证**：逐个执行，查看各自的结论文件（`logs\<场景>.result.txt`）
- **依赖**：CC-5；CC-1、CC-2（大属性的断言要等代码改完才写得出）
- **文件**：`scripts/collab-e2e/` 下新增 2–3 个脚本，改总入口与 `README.md`
- **规模**：M

> **检查点 CP-A2**（CC-4 – CC-6 之后）：脚本都在仓库里，换一台机器按 `README.md` 能跑；对照表没有缺口。

### CC-7 【脚本】全量复验（已批准计划 T2.4 ④）

- **做什么**：在阶段 A 最后一个代码提交上从头跑一遍：全部单元测试与协同两个 crate 的测试；客户端八个场景（110 项）、冲突三个场景（61 项）、跨区流程 12 个场景。每条契约记下证据：跑的什么命令，输出在哪个文件。不通过的不许带过：能修的另开一个修复任务（先写复现它的测试），修不了的记为已知限制并请你同意。
- **验收**：
  - [ ] 228 条契约每条有证据位置，没有「没跑却算过」的。
  - [ ] 端到端场景全部通过；不通过的每一个都有去向（修复它的提交号，或经你同意的已知限制）。
  - [ ] 记下复验所在的提交号。
- **验证**：`cargo test --offline --locked --lib`；`cargo test --offline --locked -p ocs_collab -p ocs_collab_server`；`scripts\collab-e2e\run_all.ps1`、`scripts\collab-e2e\run_cross.ps1`，查看汇总文件
- **依赖**：CC-1、CC-2、CC-3、CC-6
- **文件**：不改代码；证据在仓库外；`scripts/collab-e2e/README.md` 的对照表补上证据位置
- **规模**：M

### CC-8 【文档】状态文档结项

- **做什么**：把 `docs/plans/2026-11-10-mission-p1-collab-status.md` 更新到 228/228：改 §0 一页结论、§3 里程碑表、§4 断言表；新增 §19，写 `ea7f4357b` 之后做了什么（离线排队、管线广播、跨图连接符、`existing_tags`、`overrides`、`tag_source`、大属性对象、`--bind`）和复验的证据。`SPEC-collab-server.md` 待决 S2 的四处实现选择写成已确认（保持现状）。`SPEC-collab-client.md` 待决 C1、C2、C4 写进已知限制。mission 目录是只读的，不改它里面的文件；这里记的断言状态是「有证据的建议状态」。同时把已批准计划里的 T2.4 标为完成。
- **验收**：
  - [ ] 状态文档里的数字与 CC-7 的证据一致，写明复验的提交号。
  - [ ] 四处实现选择、三条已知限制都有记载。
- **验证**：对照 CC-7 的汇总文件逐项核数
- **依赖**：CC-7
- **文件**：`docs/plans/2026-11-10-mission-p1-collab-status.md`、`docs/plans/2026-11-16-pid-platform-next-steps-plan.md`
- **规模**：S

> **检查点 CP-A（阶段 A 结束，请你确认）**：228 条契约全部有证据；固定门禁全过；状态文档结项。我汇报各门禁读数和复验结果，由你决定要不要把到这里为止的提交推送到 `fork/main`。

---

## 阶段 B：主数据与项目索引

目标：有一份项目主数据可以读写；能把一张图读成不依赖句柄的摘要，能扫描整个图纸目录给出跨图重号和连接符配对，能按位号定位。后面的检查、清单、快照、发布都吃这里产出的摘要。

### MD-1 【纯】主数据：数据对象、默认内容、校验、比法

- **做什么**：新建 `src/pid/master_data.rs`。数据对象与 JSON 形状（`SPEC-pid-master-data.md` §3.1）；`defaults(legend_standard)`，预置 19 个公称直径；`validate(value)`，按 §3.6 报错，错误里带出错位置（如 `pipe_classes[2].name`）；属性值与主数据的比法（§3.5）：`has_diameter`、`entry`、`names`；`pbs_subtree`。
- **验收**：
  - [ ] §3.6 里校验类的每个错误码各有一个反例，断言错误码和出错位置；合法输入经 JSON 往返后逐项相等。
  - [ ] §3.5 的比法：`DN80`、`dn 80`、`80` 都算 80；`八十` 和标了隐藏的直径算不在表里；等级和介质的名称逐字相等才算，区分大小写。
  - [ ] 2,000 行管道等级的主数据，解析加校验不超过 50 ms（性能预算测试）。
- **验证**：`cargo test --offline --locked --lib master_data`；`cargo test --offline --locked --lib -- --ignored master_data_2000`
- **依赖**：无
- **文件**：新建 `src/pid/master_data.rs`；`src/pid/mod.rs`
- **规模**：M

### MD-2 【纯】主数据：两处存放与取用顺序

- **做什么**：图内记录 `PID_AUTHOR_MASTERDATA`，用现有的图级记录布局 `graph_data::store`，不改 `graph_data::keys()`（§3.4）。项目文件 `pid-project.json` 的读与原子写（§3.3、§4）：先比磁盘上的 `revision`，再加 1，写临时文件后改名；保留不认识的顶层键；同样的输入写出逐字节相同的文件。`resolve(doc, drawing_dir)` 按「项目文件优先 → 图内记录 → 未配置」取（§3.2）；项目文件坏了就报读不出来，不退回去用图内那份。
- **验收**：
  - [ ] §3.2 的四种情形（只有项目文件、只有图内、两处都有、项目文件坏了）各得出规格写的结果；写进图内记录、存成 DWG、再打开，逐项相等。
  - [ ] 写项目文件：磁盘上的 `revision` 已变 → `MASTERDATA_CONFLICT`，文件逐字节不变；目录不可写时不留临时文件；写回后不认识的顶层键还在。
  - [ ] 同样的主数据（`updated_at` 相同）两次写出的文件逐字节相同。
- **验证**：`cargo test --offline --locked --lib master_data`
- **依赖**：MD-1
- **文件**：`src/pid/master_data.rs`；需要时把 `src/pid/dwg_roundtrip.rs` 里「存盘再读回」的辅助函数放宽成 `pub(crate)`（只改可见性）
- **规模**：M

### MD-3 【接线】命令 `PIDMASTERDATA`

- **做什么**：新建 `src/app/commands/pidmasterdata.rs`：`STATUS`（不带子命令同它）、`INIT [PROJECT|DRAWING]`、`EXPORT <文件>`、`IMPORT <文件> [PROJECT|DRAWING]`、`EMBED`（§5.2）。写图内记录是一次图纸修改，先 `push_undo_snapshot` 再写，所以能撤销。只读会话里只放行不带子命令、`STATUS`、`EXPORT`，照 `PIDLEGEND` 的写法在 `readonly_refuses_command` 加一条。命令带参数，要登记成「整行当一条命令」。
- **验收**：
  - [ ] 一条龙：`INIT` → `STATUS` → `EXPORT` → 改文件 → `IMPORT`，每一步的输出与落盘内容正确；目标位置已有主数据时 `INIT` 报 `MASTERDATA_EXISTS`。
  - [ ] 写图内记录后撤销一步，图内没有这条记录。
  - [ ] 只读会话里 `INIT`、`IMPORT`、`EMBED` 被拒，`STATUS`、`EXPORT` 可用；从命令行执行 `PIDMASTERDATA INIT DRAWING` 只执行一次。
- **验证**：`cargo test --offline --locked --lib pidmasterdata`
- **依赖**：MD-2
- **文件**：新建 `src/app/commands/pidmasterdata.rs`；`src/app/commands/mod.rs`、`src/app/command_driver/utilities.rs`、`src/app/collab/apply.rs`
- **规模**：M

### MD-4 【接线】主数据的操作与接口

- **做什么**：两个自动化操作（§5.3）：`pid_masterdata`（`GET /api/v1/pid/master-data`，可选 `usage`）、`pid_masterdata_set`（`PUT /api/v1/pid/master-data`，带 `target`、`master_data`、`expected_revision`）。按共用约定 §7.6 的第 3–5 处接线：`control_request` 的操作名与能力清单、REST 路由与 `map_status`、`rest_openapi.json`、MCP 的操作名单与 `TOOL_SCHEMA_DIGEST`。
- **验收**：
  - [ ] 读操作的三种 `state`（`configured`、`unreadable`、`not_configured`）各一例；带 `usage: true` 时，计数等于当前图上取值相同的对象个数。
  - [ ] 写操作：`expected_revision` 不对 → HTTP 409、`MASTERDATA_CONFLICT`；只读会话里被拒。
  - [ ] `rest_openapi.json` 里有这两条路径；测试 `tool_schema_digest_is_pinned` 通过。
- **验证**：`cargo test --offline --locked --lib pid_masterdata`；`cargo test --offline --locked --lib tool_schema_digest_is_pinned`；手工：`cargo run --offline --locked -- --http 8090` 后请求 `GET /api/v1/pid/master-data`
- **依赖**：MD-3
- **文件**：`src/app/control/mod.rs`、`src/rest.rs`、`src/rest_openapi.json`、`src/mcp.rs`
- **规模**：M

> **检查点 CP-B1**（MD-1 – MD-4 之后）：命令行和 REST 都能建、读、改主数据；固定门禁全过。

### IX-1 【纯】图纸摘要之一：对象、端口与属性

- **做什么**：新建 `src/pid/project_index.rs`。`summarize` 先做对象这一半：摘要里的 `objects[]` 与 `ports[]`（`SPEC-pid-project-index.md` §3.1）。实体按归类表归类（先看拓扑角色，再看类型编码）；`own_tag` 标出它有没有自己的编号；属性用 `PidAuthor::load` 读，溢出到扩展字典的也要读全；输出顺序固定。只读，不写图。图号与 PBS 节点由调用方经 `SummaryMeta` 传入，所以这个任务不依赖主数据。
- **验收**：
  - [ ] 用命令建一张小图（一台泵、一个容器、一条管线带一条管路和一个阀门、一条信号线、一个连接符），摘要里每个对象的字段逐项符合 §3.1；归类表每一行有例子（连接符不被当成管线，不单独编号的管件 `own_tag` 为假）。
  - [ ] 属性超过 2 KB 的对象，摘要里的 `props` 是全的。
  - [ ] 同一张图做两次摘要，JSON 逐字节相同；做摘要前后图纸内容不变。
- **验证**：`cargo test --offline --locked --lib project_index`
- **依赖**：无
- **文件**：新建 `src/pid/project_index.rs`；`src/pid/mod.rs`
- **规模**：M

### IX-2 【纯】图纸摘要之二：线段、接点与连通图

- **做什么**：`summarize` 的另一半：`segments[]`、`signal_ends[]`、`connectors[]`。每个端点接到了什么，按坐标现算（§3.2 的七种情形，按规格写的先后顺序判，容差 `1e-6`，用网格索引，不两两比较）。`graph(summary)` 产出连通图（§3.3）。这是本模块风险最高的一块（几何判定），所以排在索引之前。
- **验收**：
  - [ ] §3.2 的七种情形各有一个端点被判对；支管接在主管中间不算悬空；管线自己的线接到连接符不算悬空。
  - [ ] 连接符的 `peer_kind` 四种取值各一例；连通图的节点与边符合 §3.3，输出顺序稳定。
- **验证**：`cargo test --offline --locked --lib project_index`
- **依赖**：IX-1
- **文件**：`src/pid/project_index.rs`
- **规模**：M

### IX-3 【纯】项目索引：跨图重号与连接符配对

- **做什么**：`list_drawings`（§3.7：目录下直接放着的 `.dwg`，跳过以 `.` 或 `~$` 开头的，带上项目文件给的图号与 PBS 节点）；`build_index`（§3.4 – §3.6：`complete`、`drawings[]`、`duplicates[]`、`links[]`、`unresolved[]`）；`ProjectIndex::places`。都是只吃摘要的纯函数。
- **验收**：
  - [ ] 两张图各有一台 `P-1001` → 报一处跨图重号；两张图都有管线 `L-1001` → 不报。
  - [ ] 连接符配对的五种结果各一例：配成对、`one_sided`、`target_missing`、`unpaired`、`ambiguous`。
  - [ ] 一张图读不出来时 `complete` 为假、其余照常；两个文件得出同一个图号 → `INDEX_DRAWING_ID_DUPLICATED`。
- **验证**：`cargo test --offline --locked --lib project_index`
- **依赖**：IX-2；MD-2（`list_drawings` 要读项目文件里的 `drawings[]`）
- **文件**：`src/pid/project_index.rs`
- **规模**：M

### IX-4 【纯】摘要的文件往返、合成图与性能预算

- **做什么**：三件事。往返测试：建图 → 存 DWG → 读回 → 摘要，与存盘前相比只有 `source`、`fingerprint`、`handle` 不同。合成图：写一个生成 2,000 个对象的图的测试支撑函数，标 `#[cfg(test)] pub(crate)`，后面检查、清单、快照、发布、面板的性能测试都用它。性能预算测试。
- **验收**：
  - [ ] 往返前后的摘要相等（上面三项除外）。
  - [ ] 2,000 个对象的图做一次摘要不超过 150 ms。
- **验证**：`cargo test --offline --locked --lib project_index`；`cargo test --offline --locked --lib -- --ignored project_index_summary_2000`
- **依赖**：IX-2
- **文件**：`src/pid/project_index.rs`；需要时放宽 `src/pid/dwg_roundtrip.rs` 里辅助函数的可见性
- **规模**：S

> **检查点 CP-B2**（IX-1 – IX-4 之后）：摘要与索引的纯函数齐了，摘要性能达标。从这里起，阶段 C 三个模块的【纯】任务可以开工。

### IX-5 【接线】项目扫描、摘要缓存与 `PIDINDEX`

- **做什么**：新建 `src/app/commands/pidindex.rs`。扫描项目目录得到各图的摘要：正在程序里打开的图读内存里的那份（含没保存的修改），没打开的读磁盘文件；磁盘图的摘要按「路径 + 大小 + 修改时间」缓存在内存里，不往目录里写缓存文件（§4）。这一步先做成同步的扫描函数，挪到后台在 IX-6。命令 `PIDINDEX`、`PIDINDEX DUPLICATES`、`PIDINDEX LINKS`（§5.2）不改图，进只读会话的允许名单；带参数，登记成整行命令。
- **验收**：
  - [ ] 临时目录里放两张图，`PIDINDEX` 输出的图数、重号数、配对数正确；当前图没保存过 → `INDEX_NO_PROJECT_DIR`。
  - [ ] 文件没变时第二次扫描不重读文件（数读文件的次数）；改了一个文件只重读那一个；打开着的图里没保存的修改，索引里看得到。
  - [ ] 只读会话里三个命令都能用；从命令行执行 `PIDINDEX LINKS` 只跑一次。
- **验证**：`cargo test --offline --locked --lib pidindex`
- **依赖**：IX-3
- **文件**：新建 `src/app/commands/pidindex.rs`；`src/app/commands/mod.rs`、`src/app/command_driver/utilities.rs`、`src/app/collab/apply.rs`
- **规模**：M

### IX-6 【接线】后台扫描：进度、取消、不卡界面

- **做什么**：把「读磁盘文件并做摘要」挪进后台任务（§4）：只把摘要传回界面线程，进度报到命令行，可以取消。扫描没做完时，等结果的操作应答 `{"ok":true,"status":"scanning","done":…,"total":…}`，调用方稍后重取；命令则是先扫、扫完自动接着做。后面的全项目检查、全项目清单、发布都用这一套。后台任务照程序里现有的做法（比如打开文件那条路），不引入新的异步运行时。
- **验收**：
  - [ ] 扫描期间程序照常处理别的操作（测试里用一个慢的假读取函数：扫描没完成时发别的操作能得到应答）。
  - [ ] 取消后不再读剩下的文件，已经缓存的摘要不丢；再扫一次接着用缓存。
  - [ ] 100 张图的项目在文件都没变时重扫不超过 1 秒（性能预算测试）。
- **验证**：`cargo test --offline --locked --lib pidindex`；`cargo test --offline --locked --lib -- --ignored pidindex_rescan_100`
- **依赖**：IX-5
- **文件**：`src/app/commands/pidindex.rs`；后台任务的消息可能要动 `src/app/mod.rs`、`src/app/update/mod.rs`
- **规模**：M

### IX-7 【接线】`PIDLOCATE`：按位号定位

- **做什么**：`PIDLOCATE <位号> [<图号>]`（§5.2）。目标图没打开就先打开，切到那个标签页；按 `(类型编码, 位号)` 在目标图里现找对象，不用摘要里的句柄；选中并缩放到它（`Scene::zoom_to_entities`）；没有位号的东西按坐标缩放。不给图号时先在当前图找；当前图没有而项目里只有一处就去那一处；有多处就列出来。
- **验收**：
  - [ ] 位号在当前图上：被选中的就是那个对象，视图缩放到它。
  - [ ] 位号在另一张没打开的图上：打开它、切过去、选中。
  - [ ] 哪张图上都没有 → `LOCATE_NOT_FOUND`；有多处又没给图号 → `LOCATE_AMBIGUOUS`，并列出各处。
- **验证**：`cargo test --offline --locked --lib pidlocate`
- **依赖**：IX-5
- **文件**：`src/app/commands/pidindex.rs`；`src/app/commands/mod.rs`、`src/app/command_driver/utilities.rs`、`src/app/collab/apply.rs`
- **规模**：M

### IX-8 【接线】索引的操作与接口

- **做什么**：三个自动化操作（§5.3）：`pid_summary`（`GET /api/v1/pid/summary`）、`pid_index`（`GET /api/v1/pid/index`，可选 `tag`、`type_code`）、`pid_locate`（`POST /api/v1/pid/locate`）。按共用约定 §7.6 的第 3–5 处接线。
- **验收**：
  - [ ] 三个操作的应答符合 §5.3；`pid_index` 带 `tag` 时有 `places`；扫描没做完时应答 `scanning`。
  - [ ] 状态码：`INDEX_NO_PROJECT_DIR`、`INDEX_DRAWING_ID_DUPLICATED`、`LOCATE_AMBIGUOUS` 是 409，`LOCATE_NOT_FOUND` 是 404。
  - [ ] `rest_openapi.json` 里有这三条路径；`tool_schema_digest_is_pinned` 通过。
- **验证**：`cargo test --offline --locked --lib pid_index`；`cargo test --offline --locked --lib tool_schema_digest_is_pinned`；手工：`--http` 启动后请求 `GET /api/v1/pid/summary`
- **依赖**：IX-6、IX-7
- **文件**：`src/app/control/mod.rs`、`src/rest.rs`、`src/rest_openapi.json`、`src/mcp.rs`
- **规模**：M

> **检查点 CP-B（阶段 B 结束，请你确认）**：演示——一个目录里放两张图，各有一台 `P-1001` 和一对连接符：`PIDINDEX` 报出一处跨图重号和一对配好的连接符，`PIDLOCATE` 能跳到另一张图上的对象。固定门禁全过，汇报读数。

---

## 阶段 C：规则检查、材料清单、快照

目标：能检查一张图或整个项目并给出结果；能出材料清单；能存快照、比对两个版本。三个模块都只吃阶段 B 产出的摘要，【纯】任务可以三条线同时做；【接线】任务仍然一次一个。不并行时的默认顺序是规则检查 → 材料清单 → 快照。

### DRC-1 【纯】规则表达式：解析与求值

- **做什么**：新建 `src/pid/drc/` 下的 `mod.rs`、`model.rs`、`dsl.rs`。`dsl::parse(expr, target)` 按 `SPEC-pid-drc.md` §3.3 的文法把表达式解析成语法树；求值是三值逻辑（真、假、不知道），取值经一个「事实提供者」接口拿，后面每加一组事实都不用动解析器。各种类能用哪些事实名、关系名（§3.4）这张名字表在这一步建好，算法在 DRC-4 到 DRC-8 逐组补。解析错误是 `DSL_PARSE_ERROR`，带 `offset` 和 `expected`；不认识的关系名、坏的正则在解析时就报。
- **验收**：
  - [ ] 文法的每个产生式、运算符优先级与括号、`NOT IN`、`NOT MATCH` 各有用例；三值逻辑的真值表逐格断言；`NULL` 与空串、数值与文本的比较符合 §3.3。
  - [ ] `管道等级 = NULL` 解析成功；`管道等级 ==` 报 `DSL_PARSE_ERROR` 且 `offset` 正确；`所属泵.位号` 报 `DSL_PARSE_ERROR` 并指向 `所属泵`。
- **验证**：`cargo test --offline --locked --lib drc::dsl`
- **依赖**：无（可以和阶段 B 同时做）
- **文件**：新建 `src/pid/drc/mod.rs`、`src/pid/drc/model.rs`、`src/pid/drc/dsl.rs`；`src/pid/mod.rs`
- **规模**：M

### DRC-2 【纯】规则包：内置 41 条与项目包叠加

- **做什么**：新建内置规则包 `assets/pid-drc-rules.json`（格式见 §3.1；内容是 §3.5 的 41 条：基础规则 `DR-001` – `DR-025`，工程扩展 `DR-101` – `DR-116`）和 `src/pid/drc/builtin_rules.rs`：`builtin()` 读编进程序的内置包；`load(drawing_dir)` 把图纸目录里的项目包 `pid-drc-rules.json` 叠上去（§3.2）；`validate_project_pack(value)`；规则集的版本号与摘要值 `rule_set_digest`。
- **验收**：
  - [ ] 内置 41 条全部能解析；基础规则恰好 25 条、编号连续；`DR-114` 默认停用。
  - [ ] §3.2 叠加规则的每一行各有用例；项目包想改内置规则的表达式 → `RULE_BUILTIN_READONLY`；项目包有错时 `load` 报错，不退回去只用内置包。
  - [ ] §3.8 里规则包类的错误码（`RULESET_SCHEMA_ERROR`、`RULESET_VERSION_UNSUPPORTED`、`REQUIRED_FIELD_EMPTY`、`RULE_KIND_INVALID`、`SEVERITY_INVALID`、`RULE_ID_RESERVED`、`RULE_ID_DUPLICATED`）各有一个反例。
- **验证**：`cargo test --offline --locked --lib drc::builtin_rules`
- **依赖**：DRC-1
- **文件**：新建 `assets/pid-drc-rules.json`、`src/pid/drc/builtin_rules.rs`；`src/pid/drc/model.rs`、`src/pid/drc/mod.rs`
- **规模**：M

### DRC-3 【纯】检查器骨架与结果格式

- **做什么**：新建 `src/pid/drc/runner.rs` 与 `facts.rs`。`runner::check(&CheckInput) -> CheckReport`（§5.1）：对每条启用的规则，从摘要里选出作用对象（按种类名或类型编码），逐个求值，为真即命中；产出 §3.6 的检查结果：每条结果的对象引用、对象键、消息与整改建议、`points`、`fields`，以及 `summary` 计数、`blocked`、`rules_run` / `rules_skipped`、`notices`，按规格写的顺序排序。事实这一步只接对象自己的属性和最基本的几个（`位号`、`位号来源`、`独立编号`、`种类`、`类型编码`、`类型`、`图层`），够跑通第一批纯属性规则。
  `fields` 是 §3.6 结果格式的一部分，算法在这里一次做全（表达式里直接写在对象身上的属性名，用到位号的加 `位号`，沿关系取的不算）；属性表怎么用它高亮在 PNL-11、PNL-12。
  同时建一个表驱动测试 `every_builtin_rule_has_a_hit_and_a_miss`：遍历内置规则，要求每条都有一个命中的例子和一个不命中的例子；还没做到的规则先列在一张「未覆盖」名单里，这张名单到 DRC-8 必须清空。
- **验收**：
  - [ ] `DR-001`、`002`、`003`、`005`、`017`、`025`、`101` 各有命中与不命中的例子；需求给的例子成立：管线 `设计温度` 260、`流体介质` 工艺水、没有绝热等级 → `DR-025` 命中一条。
  - [ ] 停用 `DR-016` 后结果里没有它、其余照常，重新启用后恢复；停用整个规则集同理（`DR-016` 本期没有对象，用手写的摘要让它命中）。
  - [ ] 同一份输入检查两次，`rule_set_version`、`rule_set_digest`、`results` 相同；`fields` 的四种情形（单属性、多属性、用到位号、沿关系取的不算）各有用例。
- **验证**：`cargo test --offline --locked --lib drc::runner`
- **依赖**：DRC-2；IX-2（摘要的数据结构）
- **文件**：新建 `src/pid/drc/runner.rs`、`src/pid/drc/facts.rs`；`src/pid/drc/model.rs`、`src/pid/drc/mod.rs`
- **规模**：M

> **检查点 CP-C1**（DRC-1 – DRC-3 之后）：从表达式文本到检查结果整条跑通，第一批 7 条规则有正反例。

### DRC-4 【纯】事实之一：位号、必填项、设备、符号、图纸、仪表回路

- **做什么**：补 §3.4 里这些事实的算法：`位号合规`（新增函数 `tag_rule::matches`，按各类型自己的位号规则模板判）、`位号.长度`、`位号.同类型重复数`、`必填项缺失数`（按规则包的 `required_fields`，含上级类型；消息和 `fields` 里列出缺的字段名）、`管口数`、`设备类别`、`符号.状态`、图纸的 `图号` / `图例标准` / `PBS` / `对象数`、仪表回路的分组与 `回路号` / `仪表数` / `控制器数` / `测量元件数`（按 `loop_roles`）；所有种类都能用的根 `图纸.图号`、`图纸.PBS`、`参数.<名>`。
- **验收**：
  - [ ] `DR-004`、`010`、`013`、`018`、`019`、`023`、`024`、`113`、`114` 各有命中与不命中的例子（`DR-114` 默认停用，测试里先启用并给参数）；`DR-016` 按规格 §6 第 2 条用手写的摘要测。
  - [ ] `tag_rule::matches`：字面部分逐字比；`{序号}`、`{后缀}` 对一串数字；其余占位符对同名属性的当前值，属性为空时对任意非空文本；位号为空、没有自己编号、或这个类型没有位号规则时结论是「不知道」。
  - [ ] 「未覆盖」名单相应缩短。
- **验证**：`cargo test --offline --locked --lib drc`；`cargo test --offline --locked --lib tag_rule`
- **依赖**：DRC-3
- **文件**：`src/pid/drc/facts.rs`、`src/pid/drc/runner.rs`（测试）、`src/pid/tag_rule.rs`
- **规模**：M

### DRC-5 【纯】事实之二：管线、管路、管段

- **做什么**：`端口未连接数`（管线、管路、管段；接到打断符不算悬空）、`长度`、`管路数`、`管段数`、`后缀`、`重叠管段数`、`直连异径管路数`、`未标记覆盖数`，以及关系 `所属管路`、`所属管线`（算法见 §3.4 的算法表）。一条结果涉及多个位置时 `points` 逐个列出。
- **验收**：
  - [ ] `DR-011`、`012`、`014`、`105`、`110`、`115` 各有命中与不命中的例子。
  - [ ] 一条管线有三个悬空端点 → `DR-014` 只出一条结果，`points` 里有三个坐标。
  - [ ] 「未覆盖」名单相应缩短。
- **验证**：`cargo test --offline --locked --lib drc`
- **依赖**：DRC-3
- **文件**：`src/pid/drc/facts.rs`、`src/pid/drc/runner.rs`（测试）
- **规模**：M

### DRC-6 【纯】事实之三：符号端口、管口、信号线、线上的阀门与管件

- **做什么**：符号的 `未连接端口数`；管口这一种类（设备符号的每个端口：`名称`、`方向`、`连接状态`、`位号前缀`，关系 `所属设备`）；信号线的 `起点仪表`、`终点仪表`（属性里有就用属性，没有就看端点接在哪个仪表的端口上）、`端口未连接数`、`接到设备管口数`；线上的阀门、管件经 `所属管路`、`所属管线` 取属性；阀门的 `流向相反`（本期恒为不知道）。
- **验收**：
  - [ ] `DR-008`、`015`、`106`、`107`、`108`、`111` 各有命中与不命中的例子；阀门没填直径时 `DR-106` 不命中。
  - [ ] `DR-020`、`DR-109` 用手写的摘要测；用命令建出来的图上它们不出结果。
  - [ ] 「未覆盖」名单相应缩短。
- **验证**：`cargo test --offline --locked --lib drc`
- **依赖**：DRC-3
- **文件**：`src/pid/drc/facts.rs`、`src/pid/drc/runner.rs`（测试）
- **规模**：M

### DRC-7 【纯】事实之四：主数据

- **做什么**：所有种类都能用的根 `项目.管道等级表`、`项目.公称直径表`、`项目.绝热等级表`、`项目.流体介质表`、`项目.图例标准`；从属性值到主数据条目的关系（`管道等级.<扩展属性>`、`绝热等级.<扩展属性>`、`流体介质.<扩展属性>`），可以与别的关系连用，如 `所属管线.管道等级.最大公称压力`。取值在不在表里，用主数据规格 §3.5 的比法。主数据没配置或读不出来时，「在不在表里」的结论是不知道：这些规则不出结果，计入 `rules_skipped`，`notices` 里给一条说明。
- **验收**：
  - [ ] `DR-007`、`009`、`021`、`022`、`103`、`104` 各有命中与不命中的例子。
  - [ ] 主数据没配置：`DR-007`、`021`、`022`、`103`、`104` 不出结果并计入 `rules_skipped`，`notices` 里有 `MASTERDATA_NOT_CONFIGURED`，`DR-009` 命中；主数据读不出来时 `notices` 里是 `MASTERDATA_UNREADABLE`。
  - [ ] 「未覆盖」名单相应缩短。
- **验证**：`cargo test --offline --locked --lib drc`
- **依赖**：DRC-3；MD-2
- **文件**：`src/pid/drc/facts.rs`、`src/pid/drc/runner.rs`
- **规模**：M

### DRC-8 【纯】事实之五：连接符与项目级规则；检查范围

- **做什么**：页间连接符这一种类（`连接符种类`、`对端图纸`、`对端位号`、`配对状态`，关系 `所在线`、`对端线`）；`位号.跨图重复数`。用到 `位号.跨图重复数`、`配对状态`、`对端线` 的是项目级规则（内置的是 `DR-102`、`112`、`116`）：解析时自动认出；输入里没有项目索引时跳过、计入 `rules_skipped`，`notices` 给 `PROJECT_RULES_SKIPPED`。检查范围三种（§4.2）：`drawing`（带 `cross_drawing` 时跑项目级规则，但只报当前图上的对象）、`pbs`、`project`；规则自己的 `scope` 与 `scope_ref`；索引不完整时 `notices` 给 `INDEX_INCOMPLETE`。
- **验收**：
  - [ ] `DR-006`、`102`、`112`、`116` 各有命中与不命中的例子。
  - [ ] 没有项目索引时三条项目级规则被跳过并有说明；`drawing` 范围带 `cross_drawing` 时只报当前图上的对象；规则的四种作用域各一例。
  - [ ] 「未覆盖」名单清空：`every_builtin_rule_has_a_hit_and_a_miss` 对 41 条全部成立。
- **验证**：`cargo test --offline --locked --lib drc`
- **依赖**：DRC-4、DRC-5、DRC-6、DRC-7；IX-3
- **文件**：`src/pid/drc/facts.rs`、`src/pid/drc/runner.rs`
- **规模**：M

### DRC-9 【纯】豁免

- **做什么**：图级记录 `PID_AUTHOR_DRC_WAIVERS`（§3.7；经 `graph_data::store`，与主数据记录同一种布局）：`waivers(doc)`、`set_waivers(doc, …)`。对象键的四种写法。检查器按 `(rule_id, object_key)` 套豁免：对上的结果 `waived` 为真并带 `waiver`，不计入三个严重度的计数，`blocked` 只看没豁免的错误；图里记着、但这次没有对应结果的豁免进 `stale_waivers`。理由必填，不超过 200 字。
- **验收**：
  - [ ] 四种对象键各有用例；豁免后那条结果 `waived` 为真、不计入错误数，`blocked` 随之变化。
  - [ ] 空理由 → `WAIVER_REASON_REQUIRED`；超过 200 字 → `WAIVER_REASON_TOO_LONG`；对象改了位号后，原来那条豁免进 `stale_waivers`。
  - [ ] 豁免记录写进图、存成 DWG、再打开，逐项相等。
- **验证**：`cargo test --offline --locked --lib drc`
- **依赖**：DRC-3
- **文件**：`src/pid/drc/mod.rs`、`src/pid/drc/runner.rs`、`src/pid/drc/model.rs`
- **规模**：M

### DRC-10 【纯】规则检查的性能预算

- **做什么**：用 IX-4 的 2,000 对象合成图，测单图全量检查（含做摘要、41 条规则里启用的全部）的耗时。超了就在这个任务里优化（比如每个事实按对象算一次并缓存、空间索引复用摘要里的），不放宽预算。
- **验收**：
  - [ ] 2,000 个对象的图，单图全量检查（含做摘要）不超过 500 ms。
- **验证**：`cargo test --offline --locked --lib -- --ignored drc_check_2000`
- **依赖**：DRC-8、DRC-9；IX-4
- **文件**：`src/pid/drc/runner.rs`（测试；要优化时连同 `facts.rs`）
- **规模**：S

> **检查点 CP-C2**（DRC-4 – DRC-10 之后）：41 条规则逐条有正反例；豁免能记、能套、能往返；性能达标。检查器的纯函数部分完成。

### DRC-11 【存盘】保存门禁

- **做什么**：规则包里的 `save_gate` 三档（§4.3）。`block`：闸放在保存函数里、重复位号那道闸的旁边（`src/io/mod.rs` 两处、`src/app/automation.rs` 一处），由 `drc::gate_save(doc)` 取规则、主数据、摘要，只跑单图规则；有未豁免的错误就返回 `RULE_BLOCKED`，文件不写，消息列出前 5 条。`warn`（默认）：保存函数里不检查，保存成功后由程序层（`src/app/update/file.rs`）跑一次，在命令行给一句汇总。`off`：不检查。自动保存和备份副本不过这道闸：动手前先查清 `on_autosave` 走的是哪个保存函数，必要时给保存函数加一个「过不过规则闸」的参数。已有的重复位号闸不动。
  **补规格没写的一条**：图里没有任何 P&ID 对象时，三档都不检查、不出汇总。否则默认的 `warn` 会让每一张普通 CAD 图每次保存都报一条错误「图纸未设图例标准」（`DR-009`），`block` 档下普通图干脆存不了。判断要便宜：先看图里有没有 P&ID 记录，没有就直接放行，不做摘要。这一条已补进 `SPEC-pid-drc.md` §4.3，请你确认。
  这是整个方案里最容易伤到现有功能的一个任务（改的是存盘路径），所以排在命令之前做，让后面每个任务的全量测试都经过它。
- **验收**：
  - [ ] `block` 档：有未豁免的错误时保存返回 `RULE_BLOCKED`，磁盘上的文件一个字节不变；改掉错误或豁免之后能存；自动保存照常工作。
  - [ ] `warn` 档保存成功并有一句汇总；`off` 档不检查；每次保存最多检查一次。
  - [ ] 不含 P&ID 对象的图，三档下保存都不检查、不出汇总；现有的存盘测试全部照旧通过。
- **验证**：`cargo test --offline --locked --lib save_gate`；`cargo test --offline --locked --lib save_tests`
- **依赖**：DRC-8、DRC-9
- **文件**：`src/pid/drc/mod.rs`、`src/io/mod.rs`、`src/app/automation.rs`、`src/app/update/file.rs`；测试加在 `src/app/control/save_tests.rs`
- **规模**：M

### DRC-12 【接线】命令 `PIDCHECK`：检查、看规则、导出

- **做什么**：新建 `src/app/commands/piddrc.rs`。一个入口函数把「取规则 → 取主数据 → 做摘要（容差取规则包的 `connect_tolerance`）→ 范围要项目就扫描建索引 → 检查」串起来（§4.1），命令、接口、以后的面板都调它。命令（§5.2）：`PIDCHECK`（当前图，输出一句汇总和前 10 条）、`PIDCHECK PROJECT`、`PIDCHECK PBS <节点号>`、`PIDCHECK RULES`、`PIDCHECK EXPORT <文件>`。最近一次检查结果只留在内存里：范围是当前图的放在这张图的标签页状态里，范围是全项目或节点的按项目目录放在程序状态里；不写进图（面板以后直接读这两处）。检查不改图。命令进只读允许名单；带参数，登记成整行命令。
- **验收**：
  - [ ] 用命令建一张有若干缺陷的图，`PIDCHECK` 的汇总数与逐条结果正确；检查前后图纸的「已修改」状态和撤销栈不变。
  - [ ] `PIDCHECK PROJECT` 能查出另一张图造成的跨图重号；没保存过的图 → `INDEX_NO_PROJECT_DIR`；节点号不存在 → `PBS_NODE_UNKNOWN`；项目包有错时 `PIDCHECK RULES` 把错报出来。
  - [ ] 只读会话里这五个命令都能用；`PIDCHECK EXPORT <文件>` 写出的 JSON 就是最近一次的结果；从命令行执行 `PIDCHECK PROJECT` 只检查一次。
- **验证**：`cargo test --offline --locked --lib piddrc`
- **依赖**：DRC-8、DRC-9；IX-6（项目扫描）
- **文件**：新建 `src/app/commands/piddrc.rs`；`src/app/commands/mod.rs`、`src/app/command_driver/utilities.rs`、`src/app/collab/apply.rs`、`src/app/tab_document.rs`（跟着图走的结果）
- **规模**：M

### DRC-13 【接线】命令 `PIDCHECK`：启停规则与豁免

- **做什么**：`PIDCHECK ENABLE <规则号>` / `DISABLE <规则号>`：改写项目包（没有就新建），先校验后写，写临时文件再改名。`PIDCHECK WAIVE <规则号> <对象键> <理由>`（理由取到行尾）与 `PIDCHECK UNWAIVE <规则号> <对象键>`：只能豁免当前图最近一次检查结果里有的条目；写豁免是改图，先记撤销快照；成功后就地改留着的那份结果（那一条的 `waived`、计数、`blocked`），不重新检查。这几个子命令在只读会话里拒绝（在 `readonly_refuses_command` 里按子命令词判，照 `PIDLEGEND`）。
- **验收**：
  - [ ] 在一张 `DR-014` 会命中的图上：`PIDCHECK DISABLE DR-014` 后检查结果里没有它，`ENABLE` 后恢复（规格 §6 第 5 条举的 `DR-016` 在真图上本来就不出结果，命令层的测试换一条会命中的，否则断言是空的）；规则号不存在 → `RULE_NOT_FOUND`。
  - [ ] 豁免后不重新检查，计数与 `blocked` 立即变化；存盘再打开豁免还在；撤销一步豁免消失；要豁免的条目不在最近一次结果里 → `WAIVER_TARGET_NOT_FOUND`。
  - [ ] 只读会话里 `ENABLE`、`DISABLE`、`WAIVE`、`UNWAIVE` 被拒，`PIDCHECK` 照常。
- **验证**：`cargo test --offline --locked --lib piddrc`
- **依赖**：DRC-12
- **文件**：`src/app/commands/piddrc.rs`、`src/app/collab/apply.rs`
- **规模**：M

### DRC-14 【接线】规则检查的操作与接口

- **做什么**：五个自动化操作（§5.3）：`pid_check`（`POST /api/v1/pid/check`）、`pid_check_rules`（`GET /api/v1/pid/check/rules`）、`pid_check_rules_set`（`PUT /api/v1/pid/check/rules`，整包替换，先校验后写）、`pid_check_waive`（`POST /api/v1/pid/check/waivers`）、`pid_check_unwaive`（`DELETE /api/v1/pid/check/waivers`）。按共用约定 §7.6 的第 3–5 处接线；§3.8 的错误码进 `map_status`。
- **验收**：
  - [ ] `pid_check` 的应答就是 §3.6 的检查结果，三种范围各一例；项目扫描没做完时应答 `scanning`。
  - [ ] 状态码符合 §3.8：`RULE_BUILTIN_READONLY`、`RULE_ID_RESERVED`、`RULE_BLOCKED` 是 409；`RULE_NOT_FOUND`、`WAIVER_TARGET_NOT_FOUND`、`PBS_NODE_UNKNOWN` 是 404；`DSL_PARSE_ERROR` 是 400。
  - [ ] `rest_openapi.json` 里有这五条路径；`tool_schema_digest_is_pinned` 通过。
- **验证**：`cargo test --offline --locked --lib pid_check`；`cargo test --offline --locked --lib tool_schema_digest_is_pinned`；手工：`--http` 启动后请求 `POST /api/v1/pid/check`
- **依赖**：DRC-13
- **文件**：`src/app/control/mod.rs`、`src/rest.rs`、`src/rest_openapi.json`、`src/mcp.rs`
- **规模**：M

> **检查点 CP-C3**（DRC-11 – DRC-14 之后）：命令行和 REST 都能检查、启停规则、豁免；保存门禁三档可用；固定门禁全过。

### BOM-1 【纯】材料清单：归行与计数

- **做什么**：新建 `src/pid/bom.rs`。`build(drawings, options)` 先做不涉及长度的部分（`SPEC-pid-bom.md` §3.1）：设备、仪表、阀门、其他这四类每个位号一行；不单独编号的管件按所在管路合成一行（行键 `FITTING` 加管路位号，名称写各种符号及个数）；位号为空的不出行，列进 `skipped`；四类里同一个（类型编码，位号）有两个以上实例（同图或跨图）时整份清单不产出，返回 `BOM_DUPLICATE_TAG` 并列出各处位置；名称、规格、`instances`、`drawings`、行序与 `seq`、`totals`。清单 JSON 见 §3.2。在 `build` 的出口断言键不重复。
- **验收**：
  - [ ] 阀门 `V-1001`、`V-1002` 各一行、数量各为 1；管路 `L-1001-01` 上三个管件合成一行、数量 3；泵 `P-1001` 与阀门 `P-1001` 各占一行。
  - [ ] 同一类型下两个实例同位号（同图、跨图各一例）→ `BOM_DUPLICATE_TAG`，没有清单；位号为空的仪表不出行、列在 `skipped`。
  - [ ] 不变量：随机生成的摘要（含重复位号、空位号、多张图），要么报 `BOM_DUPLICATE_TAG`，要么产出的清单里键不重复；同样的输入两次结果相同。
- **验证**：`cargo test --offline --locked --lib pid::bom::`
- **依赖**：IX-2
- **文件**：新建 `src/pid/bom.rs`；`src/pid/mod.rs`
- **规模**：M

### BOM-2 【纯】材料清单：管线行、管长与副表

- **做什么**：管线行：名下所有线段（管路的管段和管线自己的线）的长度按图纸单位换算成米，先累加再四舍五入到 2 位小数（舍入前加 `1e-9`）；五种单位；没设单位按毫米算，`length_unit_assumed` 置真并给提示 `LENGTH_UNIT_ASSUMED`；同一条管线画在几张图上时合成一行，各图各自换算后相加；规格写 `<管道等级> DN<公称直径>`；提示 `LENGTH_FROM_DRAWING`（管长是图面长度）。副表「管道汇总」`pipe_summary`（§3.3）：每根线段按它的主人取（管道等级，公称直径，绝热等级），三个值都相同的归成一行。
- **验收**：
  - [ ] 三根线段 10.005、8.5、6 米 → 管线行数量 24.51；舍入边界 24.505、24.504999、0.005 各一例；一条管线跨多段、跨两条管路、连同它自己的线，合成一行。
  - [ ] 图纸单位是毫米时 24510 个图纸单位显示 24.51 m；五种单位各一例；没设单位时按毫米算并有提示；两张单位不同的图各自换算后相加。
  - [ ] 副表：管段取所属管路的当前值（管路覆盖了直径的单独成组）；各行长度之和与主表管线行长度之和一致（每行 0.005 米以内的舍入差）。
- **验证**：`cargo test --offline --locked --lib pid::bom::`
- **依赖**：BOM-1
- **文件**：`src/pid/bom.rs`
- **规模**：S

### BOM-3 【纯】材料清单：选列、CSV 字节与性能预算

- **做什么**：`rows(bom, columns)`：默认九列（§3.4，前六列是需求定的标准表头），`columns` 选列和排列，「图纸」一格有多张图时用分号隔开；`pipe_summary_rows(bom)`；`csv_bytes(rows)`：UTF-8、文件头带 BOM，转义复用 `rows_to_csv`（把 `src/app/annotation_data.rs` 里这个函数放宽到模块外可用）。性能预算测试用 IX-4 的合成图。
- **验收**：
  - [ ] 默认九列的列名与顺序同 §3.4；选列、换序生效；不认识的列名 → `BOM_COLUMN_UNKNOWN`。
  - [ ] CSV 字节以 BOM 头（`EF BB BF`）开始；含逗号、引号、换行的格子转义正确。
  - [ ] 2,000 个对象的图算一份清单（含做摘要）不超过 300 ms。
- **验证**：`cargo test --offline --locked --lib pid::bom::`；`cargo test --offline --locked --lib -- --ignored bom_build_2000`
- **依赖**：BOM-2；IX-4
- **文件**：`src/pid/bom.rs`、`src/app/annotation_data.rs`
- **规模**：S

### BOM-4 【接线】命令 `PIDBOM`

- **做什么**：新建 `src/app/commands/pidbom.rs`。一个入口函数「取范围内各图的摘要（当前图现做，其余图经项目扫描）→ `bom::build`」，命令、接口、以后的面板和发布都调它。命令（§5.2）：`PIDBOM`、`PIDBOM PROJECT`、`PIDBOM PBS <节点号>`、`PIDBOM EXPORT <文件> [PROJECT | PBS <节点号>]`。导出的格式由扩展名决定：CSV 用 `csv_bytes`，XLSX 用 `write_extraction_file`，JSON 是 §3.2 的对象缩进 2 格；副表另写到同目录的 `<文件名>-pipe-summary.<扩展名>`（JSON 除外）；目标文件已存在时覆盖；有错就不写文件。命令不改图，整个进只读允许名单；带参数，登记成整行命令。（`PIDBOM SNAPSHOT` 在 BOM-6。）
- **验收**：
  - [ ] 用命令建图（泵、容器、带阀门的管路、仪表），`PIDBOM` 报出的各类行数、管长合计、跳过数正确；前后图纸的「已修改」状态不变。
  - [ ] 三种格式导出后读回与清单一致（XLSX 用 `calamine` 读回，JSON 解析回同一份清单）；有重复位号时报 `BOM_DUPLICATE_TAG` 且没有写文件；扩展名不支持 → `BOM_FORMAT_UNSUPPORTED`。
  - [ ] 项目范围的行数等于各图对象归并后的行数；`PIDBOM PBS` 不给节点号 → `SCOPE_NOT_SELECTED`；范围内有图读不出来 → `BOM_SCOPE_INCOMPLETE`；只读会话里能算、能导出。
- **验证**：`cargo test --offline --locked --lib pidbom`
- **依赖**：BOM-3；IX-6
- **文件**：新建 `src/app/commands/pidbom.rs`；`src/app/commands/mod.rs`、`src/app/command_driver/utilities.rs`、`src/app/collab/apply.rs`
- **规模**：M

### BOM-5 【接线】材料清单的操作与接口

- **做什么**：两个自动化操作（§5.3）：`pid_bom`（`GET /api/v1/pid/bom`；`scope`、`pbs`）、`pid_bom_export`（`POST /api/v1/pid/bom/export`；`path`、范围参数、可选 `columns`）。按共用约定 §7.6 的第 3–5 处接线；§3.5 的错误码进 `map_status`。
- **验收**：
  - [ ] `pid_bom` 的应答就是 §3.2 的清单，与同一张图上 `PIDBOM` 的结果一致；项目扫描没做完时应答 `scanning`。
  - [ ] `pid_bom_export` 的应答带 `path`、`format`、`rows`、`bytes`、`pipe_summary_path`，`columns` 选列生效；状态码：`BOM_DUPLICATE_TAG`、`BOM_SCOPE_INCOMPLETE` 是 409，`BOM_COLUMN_UNKNOWN`、`BOM_FORMAT_UNSUPPORTED`、`SCOPE_NOT_SELECTED` 是 400。
  - [ ] `rest_openapi.json` 里有这两条路径；`tool_schema_digest_is_pinned` 通过。
- **验证**：`cargo test --offline --locked --lib pid_bom`；`cargo test --offline --locked --lib tool_schema_digest_is_pinned`；手工：`--http` 启动后请求 `GET /api/v1/pid/bom`
- **依赖**：BOM-4
- **文件**：`src/app/control/mod.rs`、`src/rest.rs`、`src/rest_openapi.json`、`src/mcp.rs`
- **规模**：M

> **检查点 CP-C4**（BOM-1 – BOM-5 之后）：命令行和 REST 都能出清单、导出三种格式；重复位号时不出清单；性能达标。

### SNAP-1 【纯】快照：内容与校验

- **做什么**：新建 `src/pid/snapshot.rs`。`capture(summary, graph, meta)` 由摘要和连通图做出一份快照（`SPEC-pid-snapshot.md` §3.2）：`counts`、`body_sha256`（把 `summary` 和 `graph` 按「键名排序、不带空白」序列化后拼起来算 SHA-256）；`parse(bytes)` 解析并重算校验和。文件是 UTF-8、不带 BOM、不缩进的 JSON。
- **验收**：
  - [ ] `capture` 后序列化再 `parse` 得到相同的快照，`counts` 与正文一致。
  - [ ] 改动 `summary` 或 `graph` 里任一字符后 `parse` 报 `SNAPSHOT_CORRUPT`。
  - [ ] JSON 坏了、缺字段、`kind` 不对 → `SNAPSHOT_FORMAT_INVALID`；`version` 比程序认识的高 → `SNAPSHOT_VERSION_UNSUPPORTED`；原因超过 200 字 → `SNAPSHOT_REASON_TOO_LONG`。
- **验证**：`cargo test --offline --locked --lib pid::snapshot::`
- **依赖**：IX-2
- **文件**：新建 `src/pid/snapshot.rs`；`src/pid/mod.rs`
- **规模**：S

### SNAP-2 【纯】快照：存取

- **做什么**：`snapshot::store`（§5.1）：`dir_for`、`save`、`save_to_file`、`list`、`load`、`load_file`、`export`。放在 `<图纸目录>/pid-snapshots/<图号>/`（§3.1，图号里文件名不允许的字符换成 `_`）；目录清单 `index.json`（§3.3）；写入规则（§3.4）：新序号 = max(`last_seq`，目录里文件名的最大序号) + 1，用「只在不存在时创建」的方式写，写完设成只读，再更新清单（临时文件再改名）。有哪些快照以目录里的文件为准：清单丢了、坏了，或与目录里的文件有增减，就读文件补齐重写；清单里记的校验和与文件不同则是 `SNAPSHOT_CORRUPT`，不重建。
- **验收**：
  - [ ] 连存三次，列表里序号是 1、2、3；删掉 `0003.json` 后再存，序号是 4。
  - [ ] 任何路径都不覆盖已有的快照文件：目标序号的文件已存在时换下一个序号，试三次仍不行报 `SNAPSHOT_IMMUTABLE`；`save_to_file` 遇到已存在的文件拒绝；写完的文件是只读的。
  - [ ] 清单丢失、损坏、与目录不一致时能重建；清单里有而文件不在的标 `missing`；导出序号 2 得到的文件与 `0002.json` 逐字节相同。
- **验证**：`cargo test --offline --locked --lib pid::snapshot::`
- **依赖**：SNAP-1
- **文件**：`src/pid/snapshot.rs`
- **规模**：M

### SNAP-3 【纯】快照：比对

- **做什么**：`diff(a, b, move_tolerance)`。谁和谁是同一个对象按 §3.5 的键认，不看句柄；比什么按 §3.6（属性、位置、长度、管件、连接、对端）；结果按 §3.7（`identical`、三组计数与 `by_what`、`added` / `removed` / `modified`、`edges`、`notices`），各列表顺序固定。
- **验收**：
  - [ ] 同一份摘要做两份快照，比对 `identical` 为真、三组计数全为 0。
  - [ ] 五类变化各做一处（新增一个阀门、删一台仪表、改一个属性、挪一台设备、改一处连接），每一处都在对应的组里，属性变化带旧值和新值；往管路里插一个阀门时，阀门在「新增」，管路在「修改」并带两条 `连接` 新增。
  - [ ] §3.5 的每种键、重复键的配对、容差边界（差恰好等于容差不算变化）、两份图号不同时的 `DRAWING_ID_DIFFERS` 各有用例；同样两份快照两次比对结果相同。
- **验证**：`cargo test --offline --locked --lib pid::snapshot::`
- **依赖**：SNAP-1
- **文件**：`src/pid/snapshot.rs`
- **规模**：M

### SNAP-4 【纯】快照：文件往返与性能预算

- **做什么**：往返测试：建图 → 做快照 → 把图另存成 DWG、重新读回（句柄可能变）→ 再做快照 → 比对。性能预算测试用 IX-4 的合成图。
- **验收**：
  - [ ] 另存、重新打开后做的快照，与之前的比对 `identical` 为真。
  - [ ] 2,000 个对象的图存一份快照（含做摘要）不超过 300 ms；两份 2,000 对象的快照比对不超过 200 ms（不含读文件）。
- **验证**：`cargo test --offline --locked --lib pid::snapshot::`；`cargo test --offline --locked --lib -- --ignored snapshot_2000`
- **依赖**：SNAP-2、SNAP-3；IX-4
- **文件**：`src/pid/snapshot.rs`
- **规模**：S

### SNAP-5 【接线】命令 `PIDSNAPSHOT` 与 `PIDDIFF`

- **做什么**：新建 `src/app/commands/pidsnapshot.rs`。取程序里此刻的图（不要求先保存，`dirty` 记下有没有未保存的修改）→ 摘要与连通图 → `capture` → 存。生成人的取法与豁免人相同。命令（§5.2）：`PIDSNAPSHOT` 或 `PIDSNAPSHOT SAVE [<原因>]`、`PIDSNAPSHOT LIST`、`PIDSNAPSHOT EXPORT <序号> <文件>`、`PIDSNAPSHOT FILE <文件> [<原因>]`；`PIDDIFF <甲> [<乙>]`（甲、乙是序号或快照文件路径，不写乙就是与当前图比）、`PIDDIFF <甲> <乙> EXPORT <文件>`（与当前图比时乙写 `LIVE`）。两个命令都不改图，整个进只读允许名单；都带参数，登记成整行命令。
- **验收**：
  - [ ] 用命令建图 → `PIDSNAPSHOT SAVE 送审前` → 改图 → `PIDDIFF 1`：三组计数与明细正确；从命令行执行 `PIDSNAPSHOT SAVE <原因>` 只存出一份。
  - [ ] 做快照前后图纸的「已修改」状态和撤销栈不变；没保存过的图 `PIDSNAPSHOT` 报 `INDEX_NO_PROJECT_DIR`，`PIDSNAPSHOT FILE <文件>` 可用；只读会话里两个命令都能用。
  - [ ] `PIDDIFF 2 2` → `DIFF_SAME_SNAPSHOT`；序号不存在 → `SNAPSHOT_NOT_FOUND`；`PIDDIFF 1 LIVE EXPORT <文件>` 写出的 JSON 就是 §3.7 的比对结果。
- **验证**：`cargo test --offline --locked --lib pidsnapshot`
- **依赖**：SNAP-2、SNAP-3
- **文件**：新建 `src/app/commands/pidsnapshot.rs`；`src/app/commands/mod.rs`、`src/app/command_driver/utilities.rs`、`src/app/collab/apply.rs`
- **规模**：M

### SNAP-6 【接线】快照的操作与接口

- **做什么**：五个自动化操作（§5.3）：`pid_snapshots`（`GET /api/v1/pid/snapshots`）、`pid_snapshot`（`GET /api/v1/pid/snapshots/{seq}`）、`pid_snapshot_save`（`POST /api/v1/pid/snapshots`，状态 201）、`pid_snapshot_export`（`POST /api/v1/pid/snapshots/{seq}/export`）、`pid_diff`（`GET /api/v1/pid/diff`）。按共用约定 §7.6 的第 3–5 处接线；§3.8 的错误码进 `map_status`。
- **验收**：
  - [ ] 五个操作的应答符合 §5.3；`pid_snapshot_save` 的状态是 201，带 `path` 时就是 `FILE` 形式；`pid_diff` 的 `b` 默认 `live`。
  - [ ] 状态码符合 §3.8：`SNAPSHOT_IMMUTABLE`、`SNAPSHOT_CORRUPT`、`INDEX_NO_PROJECT_DIR` 是 409，`SNAPSHOT_NOT_FOUND` 是 404，其余是 400。
  - [ ] `rest_openapi.json` 里有这五条路径；`tool_schema_digest_is_pinned` 通过。
- **验证**：`cargo test --offline --locked --lib pid_snapshot`；`cargo test --offline --locked --lib pid_diff`；`cargo test --offline --locked --lib tool_schema_digest_is_pinned`；手工：`--http` 启动后请求 `POST /api/v1/pid/snapshots`
- **依赖**：SNAP-5
- **文件**：`src/app/control/mod.rs`、`src/rest.rs`、`src/rest_openapi.json`、`src/mcp.rs`
- **规模**：M

### BOM-6 【接线】按快照出材料清单

- **做什么**：`PIDBOM SNAPSHOT <序号> [EXPORT <文件>]`，以及 `pid_bom`、`pid_bom_export` 的 `snapshot` 参数：读这张图已存的那份快照，用里面的 `summary` 算清单；应答的 `source` 为 `snapshot`，`snapshot_seq` 是序号。只支持单图范围。排在快照之后，是因为它要读快照。
- **验收**：
  - [ ] 存一份快照后改图：`PIDBOM SNAPSHOT 1` 的清单是改图之前的，`source` 为 `snapshot`、`snapshot_seq` 为 1；`PIDBOM` 的清单是改图之后的。
  - [ ] 序号不存在 → `SNAPSHOT_NOT_FOUND`（404）；`pid_bom` 带 `snapshot` 参数的结果与命令一致。
  - [ ] `rest_openapi.json` 里有这个参数；`tool_schema_digest_is_pinned` 通过。
- **验证**：`cargo test --offline --locked --lib pidbom`；`cargo test --offline --locked --lib tool_schema_digest_is_pinned`
- **依赖**：BOM-5；SNAP-2
- **文件**：`src/app/commands/pidbom.rs`、`src/app/control/mod.rs`、`src/rest.rs`、`src/rest_openapi.json`、`src/mcp.rs`
- **规模**：M（每个文件只加几行）

> **检查点 CP-C5**（SNAP-1 – SNAP-6 与 BOM-6 之后）：能存快照、列表、导出、比对；能按快照出清单；性能达标。

> **检查点 CP-C（阶段 C 结束，请你确认）**：演示——在一张有缺陷的图上 `PIDCHECK` 出结果、豁免一条；`PIDBOM EXPORT` 出 CSV；`PIDSNAPSHOT` 后改图再 `PIDDIFF 1`；同样的事经 REST 再做一遍。三个模块的性能预算读数。固定门禁全过。

---

## 阶段 D：发布

目标：一张图过了门禁就能出一个发布包（图纸、出图、材料清单、连接关系、快照、主数据、检查报告、发布清单），有错误的图一律发不出去。发布不改图。

### PUB-1 【纯】发布：包名、修订号、重复位号、门禁

- **做什么**：新建 `src/pid/publish_pkg.rs`（`SPEC-pid-publish.md` §5.1）：`package_name`、`next_revision`、`list_packages`；`duplicate_tags`（不经过规则，直接数：本图里同一个（类型编码，位号）出现两次以上，或本图的某个键在项目的另一张图上也有；只算有自己编号的设备、阀门、仪表、其他）；`gates`（§3.4 的四项门禁和各项计数）。
- **验收**：
  - [ ] `package_name("PID-1101", 3)` 是 `PID-1101-r003`，图号里不能做文件名的字符换成 `_`；`next_revision` 在目标目录里没有这张图的包时是 1，已有 r001、r003 时是 4；`list_packages` 只列这张图的包。
  - [ ] 重复位号的数法：本图内、跨图各一例算；不同类型同位号不算；没有自己编号的不算；管线、管路、信号线不算。
  - [ ] 门禁四项的组合：全过时 `result` 为 `放行`；有未豁免的错误、有重复位号、主数据 `unreadable`、索引不完整，任一项都拦；警告、提示、已豁免的不拦；主数据 `not_configured` 不单独拦。
- **验证**：`cargo test --offline --locked --lib publish_pkg`
- **依赖**：DRC-9（检查报告）；IX-3；MD-2
- **文件**：新建 `src/pid/publish_pkg.rs`；`src/pid/mod.rs`
- **规模**：S

### PUB-2 【纯】发布：写包、落位、校验

- **做什么**：`write_package(staging, input)` 往临时目录里写除两张出图以外的文件（出图由调用方先放进去）：图纸文件（源文件原样复制）、`bom.csv`、`bom.json`、`pipe-summary.csv`、`topology.json`（§3.3）、`snapshot.json`、`master-data.json`（没配置时写 `{"version":1,"configured":false}`）、`drc-report.json`，最后写发布清单 `manifest.json`（§3.2，`contents[]` 按规格的顺序，每项带字节数和 SHA-256）。`commit(staging, final_dir)`：先自检再改名到位，目标已存在则拒绝，落位后文件设为只读。`verify_package(dir)`（§3.5）。
- **验收**：
  - [ ] 写出的包恰好十一个文件（测试里两张出图用占位文件）；清单 `contents[]` 十项，顺序、类别、字节数、校验和都对；图纸文件与源文件逐字节相同；`topology.json` 的边与 `snapshot.json` 里的 `graph.edges` 相同。
  - [ ] `verify_package`：完好的包通过；改动任一文件的一个字节 → `PACKAGE_CORRUPT` 并列出 `mismatched`；删掉任一文件 → `PACKAGE_INCOMPLETE` 并列出 `missing`；多出来的文件只列在 `extra`；目录不存在或没有清单 → `PACKAGE_NOT_FOUND`。
  - [ ] `commit`：目标已存在 → `PUBLISH_TARGET_EXISTS`，原包不变；自检不过就不改名；落位后的文件是只读的。
- **验证**：`cargo test --offline --locked --lib publish_pkg`
- **依赖**：PUB-1；BOM-3；SNAP-1
- **文件**：`src/pid/publish_pkg.rs`
- **规模**：M

> **检查点 CP-D1**（PUB-1 – PUB-2 之后）：给定摘要、检查报告、清单、快照，能在临时目录里写出一个完整的包并校验通过。

### PUB-3 【接线】`PIDPUBLISH CHECK`：前提与门禁

- **做什么**：新建 `src/app/commands/pidpublish.rs`，先做流程的前半段（§4）。前提（§3.4）：图保存过且没有未保存的修改，否则 `PUBLISH_UNSAVED`；现取的文件指纹与标签页记的 `disk_fingerprint` 不同或标签页没记到指纹，`PUBLISH_FILE_CHANGED`；只读会话 `COLLAB_READONLY`；目标目录与修订号不合法是 `PUBLISH_TARGET_INVALID` / `PUBLISH_TARGET_EXISTS` / `PUBLISH_REVISION_INVALID`。前提满足后：当前图的摘要只做一次，扫描项目建索引，**现做一次**规则检查（`scope: drawing`、`cross_drawing: true`、触发时机 `发布前`，调 DRC-12 的入口函数），数重复位号，读主数据，算四项门禁。`PIDPUBLISH CHECK` 只做到这里，不写任何东西。`PIDPUBLISH` 带参数，登记成整行命令；只读会话里 `CHECK`、`LIST`、`VERIFY` 允许，发布本身拒绝（按子命令词判）。
- **验收**：
  - [ ] 有未保存的修改 → `PUBLISH_UNSAVED`；文件被别的程序改过 → `PUBLISH_FILE_CHANGED`；只读会话里发布 → `COLLAB_READONLY`，而 `PIDPUBLISH CHECK` 仍可用；都不写任何东西。
  - [ ] 一张有错误的图：`PIDPUBLISH CHECK` 报出四项门禁的计数和前若干条错误，目标目录里没有新增任何文件或目录，也没有新快照；检查是现做的（先留下一份「没有错误」的旧结果，再改出错误并保存，仍然被拦）。
  - [ ] 2,000 个对象的图，门禁部分（不含项目扫描）不超过 1 秒。
- **验证**：`cargo test --offline --locked --lib pidpublish`；`cargo test --offline --locked --lib -- --ignored publish_gates_2000`
- **依赖**：PUB-1；DRC-12；IX-6；IX-4（合成图）
- **文件**：新建 `src/app/commands/pidpublish.rs`；`src/app/commands/mod.rs`、`src/app/command_driver/utilities.rs`、`src/app/collab/apply.rs`
- **规模**：M

### PUB-4 【接线】`PIDPUBLISH`：出包

- **做什么**：流程的后半段，`PIDPUBLISH [<目标目录>] [REV <修订号>]`。门禁任一项不过：返回 `PUBLISH_BLOCKED`，应答带 `gates`、`duplicates` 和完整的检查报告，目标目录里一个文件都不写，也不存快照。全过：存一份快照（原因写「发布前自动快照（修订 N）」）→ 在目标目录里建临时目录（名字以 `.` 开头、带不重复的后缀；仓库没有随机数库，用时间加进程号；上次崩溃留下的临时目录顺手清掉）→ 用现有的出图操作（`control_plot`，`src/app/control/actions.rs`）出 PDF 和 SVG → 材料清单从刚存的快照算 → `write_package` → 自检 → `commit`。任何一步失败：删掉临时目录，返回 `PUBLISH_FAILED` 并带 `step`；快照留着。应答里带各步耗时。发布不改图。
- **验收**：
  - [ ] 一张干净的图发布成功：包里恰好十一个文件，`verify_package` 通过；图纸文件与源文件逐字节相同；`bom.json` 的 `snapshot_seq` 等于清单里的 `snapshot_seq`；警告和已豁免的结果都在 `drc-report.json` 里。
  - [ ] 有未豁免的错误 → `PUBLISH_BLOCKED`，目标目录里没有新增任何文件或目录，也没有新快照；改掉或豁免后再发布成功。连发两次得到修订 1、2，两个包都在；`REV` 给的修订号不大于已有的最大值 → `PUBLISH_REVISION_INVALID`。
  - [ ] 出包中途失败（测试里让某一步报错）→ `PUBLISH_FAILED` 并带 `step`，目标目录里既没有包也没有残留的临时目录；发布前后图纸的「已修改」状态、撤销栈、源文件的校验和不变；从命令行执行 `PIDPUBLISH <目录>` 只发布一次、只发到给定的目录。
- **验证**：`cargo test --offline --locked --lib pidpublish`
- **依赖**：PUB-2、PUB-3；SNAP-2；BOM-3
- **文件**：`src/app/commands/pidpublish.rs`；需要时 `src/app/control/actions.rs`（让出图能从程序内部调用）
- **规模**：M

### PUB-5 【接线】`PIDPUBLISH LIST` / `VERIFY` 与「发布到哪一版了」

- **做什么**：`PIDPUBLISH LIST [<目标目录>]` 列出这张图已发布的各版和当前状态。状态不记在图里，由目标目录里的包推出（§4 末尾的表）：取修订号最大的包，比它的 `source.sha256` 与当前图纸文件的校验和。`PIDPUBLISH VERIFY <包目录>` 校验一个包。
- **验收**：
  - [ ] 没发布过显示「未发布」；发布后显示「已发布，修订 N」；改图并保存后显示「修订 N 之后有改动」；有未保存的修改时也是「之后有改动」。
  - [ ] `PIDPUBLISH VERIFY`：完好的包通过；动过一个字节 → `PACKAGE_CORRUPT`；目录不存在 → `PACKAGE_NOT_FOUND`；只读会话里 `LIST`、`VERIFY` 可用。
- **验证**：`cargo test --offline --locked --lib pidpublish`
- **依赖**：PUB-4
- **文件**：`src/app/commands/pidpublish.rs`
- **规模**：S

### PUB-6 【接线】发布的操作与接口

- **做什么**：四个自动化操作（§5.3）：`pid_publish_check`（`GET /api/v1/pid/publish/check`）、`pid_publish`（`POST /api/v1/pid/publish`，状态 201；`dir`、`revision`、`plot`）、`pid_publications`（`GET /api/v1/pid/publications`）、`pid_publish_verify`（`POST /api/v1/pid/publish/verify`）。项目扫描没做完时应答 `scanning`，调用方稍后用同样的请求再发一次。按共用约定 §7.6 的第 3–5 处接线；§3.6 的错误码进 `map_status`。
- **验收**：
  - [ ] `pid_publish_check` 的应答带 `can_publish`、`preconditions`、`gates`、`target_dir`、`next_revision`；`pid_publish` 成功是 201，带 `package_id`、`revision`、`snapshot_seq`、`files`、`gates`；`plot` 参数传到了出图；`pid_publications` 带 `state` 与各包。
  - [ ] 状态码符合 §3.6：`PUBLISH_BLOCKED`、`PUBLISH_UNSAVED`、`PUBLISH_FILE_CHANGED`、`PUBLISH_TARGET_EXISTS`、`PACKAGE_INCOMPLETE`、`PACKAGE_CORRUPT` 是 409，`PUBLISH_FAILED` 是 500，`PACKAGE_NOT_FOUND` 是 404，`PUBLISH_REVISION_INVALID`、`PUBLISH_TARGET_INVALID` 是 400。
  - [ ] `rest_openapi.json` 里有这四条路径；`tool_schema_digest_is_pinned` 通过。
- **验证**：`cargo test --offline --locked --lib pid_publish`；`cargo test --offline --locked --lib tool_schema_digest_is_pinned`；手工：`--http` 启动后请求 `GET /api/v1/pid/publish/check`
- **依赖**：PUB-5
- **文件**：`src/app/control/mod.rs`、`src/rest.rs`、`src/rest_openapi.json`、`src/mcp.rs`
- **规模**：M

### PUB-7 【验收】拦截率与一致性

- **做什么**：只加测试。**拦截率**：内置规则里错误级的共 20 条，去掉本期不出结果的 `DR-020`、`DR-109`，还有 18 条；每条造一张能让它命中的图（要主数据的配上主数据，要项目的在目录里放第二张图），保存后逐张发布。**重复位号**：项目规则包把 `DR-004`、`DR-102` 停用后再发布。**一致性**：包里的图纸文件是从磁盘复制的，清单和报告是从程序里的图算的，两者说的是同一份东西的依据是「保存后重新读回，摘要不变」，在程序的保存路径上再验一次（纯函数层面已在 IX-4 验过）。
- **验收**：
  - [ ] 18 张图逐张发布，全部 `PUBLISH_BLOCKED`（拦截率 100%），每张的检查报告里都有对应规则的未豁免结果，目标目录都没有新增内容。
  - [ ] 停用 `DR-004` 后，本图有重复位号仍被拦；停用 `DR-102` 后，另一张图上有同类型同位号仍被拦。
  - [ ] 经程序的保存路径存盘后重新读回，摘要与保存前相同（`source`、`fingerprint`、`handle` 除外）。
- **验证**：`cargo test --offline --locked --lib pidpublish::acceptance`
- **依赖**：PUB-4；DRC-13（停用规则）
- **文件**：新建测试文件 `src/app/commands/pidpublish_acceptance.rs`，由 `src/app/commands/pidpublish.rs` 用 `#[cfg(test)] #[path = …] mod acceptance;` 挂进去（不动共用的 `commands/mod.rs`）
- **规模**：M

> **检查点 CP-D（阶段 D 结束，请你确认）**：演示——一张带错误的图发布被拦并列出原因；整改后发布成功，包里十一个文件，`PIDPUBLISH VERIFY` 通过；改包里一个字节再校验报 `PACKAGE_CORRUPT`；`PIDPUBLISH LIST` 显示状态。拦截率测试的读数。固定门禁全过。

---

## 阶段 E：「校验与交付」面板与收尾

目标：把前面几个模块的结果摆到一个停靠面板上（规则检查、材料清单、快照与发布三个页签），能筛、能点、能定位到图上的对象。面板不带自己的业务逻辑：每个按钮调的都是命令和接口在用的那个入口函数。面板文字用中文字面量（面板规格待决问题 V2）。

面板的测试主要靠两样：纯函数 `build_view`（界面状态 + 结果 → 要画什么）的单元测试；程序级测试里用 `action` 操作替人按按钮、用 `pid_verify_panel` 读面板此刻显示的内容。所以接口那两个任务（PNL-3、PNL-4）排在各页签之前。

### PNL-1 【界面】面板登记与空壳

- **做什么**：登记停靠面板 `PanelId::PidVerifyDeliver`（`SPEC-pid-verify-deliver-panel.md` §5.1、§7 的接入点清单，照「协同」面板）：标题「校验与交付」、新画一个图标、默认宽度 360、`ALL` 由 21 项变 22 项。新建 `src/ui/window/pid_verify_deliver.rs`：`Tab`、`PidVerifyMsg`（先只有切页签）、`VerifyUi`（§3.1 的界面状态）、`VerifyView`、纯函数 `build_view`、`view`（画三个页签，内容先是「还没有……」的空状态文字）。文字直接写中文，不动 `tests/pid_panel_localization.rs`。
- **验收**：
  - [ ] `PanelId::ALL` 有 22 项且包含 `PidVerifyDeliver`；标题、图标、默认宽度正确。
  - [ ] 面板能从停靠区通用的面板开关打开和关闭；三个页签能切；经典界面和外壳的编辑屏里都能显示（人工看一眼并截图留证）。
  - [ ] `build_view` 的单元测试：切页签后要画的内容跟着变；`tests/pid_panel_localization.rs` 没有改动且通过。
- **验证**：`cargo test --offline --locked --lib dock::tests`；`cargo test --offline --locked --lib pid_verify_deliver`；`cargo test --offline --locked --test pid_panel_localization`
- **依赖**：无（不依赖前面的模块，可以提前做）
- **文件**：`src/ui/dock.rs`、`src/ui/window/mod.rs`、新建 `src/ui/window/pid_verify_deliver.rs`、新建 `assets/icons/ui/verify.svg`、`src/app/mod.rs`、`src/app/dock/update.rs`、`src/app/dock/view.rs`、`src/app/update/mod.rs`、`src/app/dock/tests.rs`
- **规模**：L。拆不开：面板枚举加一项，所有按面板分支的地方都要补上，否则编译不过。每个文件的改动都很小。

### PNL-2 【接线】命令 `PIDVERIFY` 与控件位置

- **做什么**：新建 `src/app/commands/pidverify.rs`：`show_pid_verify_panel`（显示，第一次停在右侧）、`dispatch_pidverify`、`on_pid_verify`（处理面板消息）、`refresh_pid_verify_view`（重建要画的内容），名字和分工照「协同」面板。命令（§5.2）：`PIDVERIFY` 或 `PIDVERIFY ON`、`PIDVERIFY OFF`、`PIDVERIFY CHECK` / `BOM` / `DELIVER`；不改图，进只读允许名单；带参数，登记成整行命令。控件位置标识（§3.4，前缀 `pid-verify-`）：这一步报面板主体和三个页签；面板关掉时清掉所有 `pid-verify-` 开头的位置记录。
- **验收**：
  - [ ] `PIDVERIFY` 显示面板，`PIDVERIFY OFF` 隐藏，`PIDVERIFY BOM` 显示并切到材料清单页签；从命令行执行 `PIDVERIFY BOM` 只执行一次。
  - [ ] 面板关掉后，`pid-verify-` 开头的位置记录全部清掉（真实窗口里「显示时读得到」在 PNL-13 的端到端脚本里验）。
  - [ ] 只读会话里命令可用。
- **验证**：`cargo test --offline --locked --lib pidverify`
- **依赖**：PNL-1
- **文件**：新建 `src/app/commands/pidverify.rs`；`src/app/commands/mod.rs`、`src/app/command_driver/utilities.rs`、`src/app/collab/apply.rs`、`src/app/dock/update.rs`
- **规模**：M

### PNL-3 【接线】面板状态操作 `pid_verify_panel`

- **做什么**：`panel_state(view, limit, offset)` 把要画的内容转成 §3.2 的 JSON；操作 `pid_verify_panel`（`GET /api/v1/pid/verify-panel`，可选 `limit`、`offset`，`rows` 默认最多 200 行）。画面板和回答这个操作用的是同一份 `VerifyView`，所以接口读到的就是人看到的。这一步先有公共字段和三个页签的骨架，各页签的内容由后面的任务填，那时不用再动接口文件。按共用约定 §7.6 的第 3–5 处接线。
- **验收**：
  - [ ] 应答有 §3.2 的公共字段（`visible`、`tab`、`document_id`、`drawing_id`、`writable`、`busy`、`notice`、`buttons`）和 `check`、`bom`、`deliver` 三块的骨架；面板没显示时也能读。
  - [ ] `PIDVERIFY BOM` 之后应答里 `tab` 是 `bom`；切换图纸标签页后 `document_id`、`drawing_id` 跟着变。
  - [ ] `rest_openapi.json` 里有这条路径；`tool_schema_digest_is_pinned` 通过。
- **验证**：`cargo test --offline --locked --lib pid_verify_panel`；`cargo test --offline --locked --lib tool_schema_digest_is_pinned`；手工：`--http` 启动后请求 `GET /api/v1/pid/verify-panel`
- **依赖**：PNL-2
- **文件**：`src/ui/window/pid_verify_deliver.rs`、`src/app/control/mod.rs`、`src/rest.rs`、`src/rest_openapi.json`、`src/mcp.rs`
- **规模**：M

### PNL-4 【接线】动作通道：`action` 操作的 `pid_verify`

- **做什么**：用现有的 `action` 操作替人按面板上的东西（§3.3）：在 `control_ui_action`（`src/app/control/actions.rs`）里加 `name` 为 `pid_verify` 的分支；`parse_action(value, view)` 把「键」或「键=值」转成面板消息。这一步先支持 `show`、`hide`、`tab=…`，并把三种失败定好：按钮是灰的 → `PANEL_ACTION_UNAVAILABLE`（带 `reason`）；行下标、快照序号、修订号不在面板当前的列表里 → `PANEL_ROW_NOT_FOUND`；键不认识或值的格式不对 → 现有的 `invalid_value`。两个新码进 `map_status`。后面各页签任务只在 `parse_action` 里加自己的键。
- **验收**：
  - [ ] `show`、`hide`、`tab=check|bom|deliver` 生效，`pid_verify_panel` 的 `visible`、`tab` 跟着变；动作的应答只是 `{"ok":true}`。
  - [ ] 不认识的键、格式不对的值 → `invalid_value`（400）。
  - [ ] `map_status` 里 `PANEL_ACTION_UNAVAILABLE` 是 409、`PANEL_ROW_NOT_FOUND` 是 404（这两个码到后面的页签任务才真正发出，这里先用 `parse_action` 的单元测试和状态码映射测试盖住）。
- **验证**：`cargo test --offline --locked --lib pid_verify`
- **依赖**：PNL-3
- **文件**：`src/ui/window/pid_verify_deliver.rs`、`src/app/control/actions.rs`、`src/rest.rs`
- **规模**：M

> **检查点 CP-E1**（PNL-1 – PNL-4 之后）：面板能开关、能切页签，能经接口读状态、经接口驱动；还没有业务内容。从这里起，各页签的任务不再碰七个共用文件。

### PNL-5 【界面】规则检查页签

- **做什么**：§4.2 的整个页签。【检查当前图】【检查全项目】调 DRC-12 的入口函数；结果按 §3.1 存（当前图范围的跟着那张图，全项目范围的跟着项目目录；两份都有时显示较新的）；命令行、接口、面板触发的检查更新的是同一份。状态行；严重度筛选、对象种类筛选、「显示已豁免」；提示行；按严重度分组的结果列表（组标题带筛选后的条数、可折叠、每组先画 200 行）；四种没有列表的情形；过期标记（记下算结果时的 `edit_revision`，不自动重查）；导出检查结果。动作键：`check=drawing|project`、`severity=…`、`kind=…`、`show_waived=0|1`、`check_export=…`、`more=check`。面板状态的 `check` 一块。全项目检查的进度条和取消在 PNL-10。
- **验收**：
  - [ ] 一张有 1 条错误、1 条警告、1 条已豁免的图：`check=drawing` 后三个计数与 `pid_check` 的 `summary` 一致，列表 2 行；关掉「警告」剩 1 行；打开「显示已豁免」变 3 行。
  - [ ] 在命令行执行 `PIDCHECK` 之后，不点面板的按钮，面板显示的就是这次的结果；检查之后改一下图 `stale` 变真，重新检查后变假。
  - [ ] `build_view` 的单元测试：筛选与分组、组计数、行名的写法、两份结果取较新的、200 行截断与「再显示 200 条」、四种没有列表的情形。
- **验证**：`cargo test --offline --locked --lib pid_verify`
- **依赖**：PNL-4；DRC-12
- **文件**：`src/ui/window/pid_verify_deliver.rs`、`src/app/commands/pidverify.rs`、`src/app/tab_document.rs`、`src/app/mod.rs`
- **规模**：M

### PNL-6 【界面】点一条结果：定位与豁免

- **做什么**：定位（§4.3）：点一条结果，用 IX-7 的定位入口按（类型编码，位号）在图里现找，选中并缩放；对象在另一张图上就先打开或切过去（全项目结果的列表还在）；没有位号的按结果里记的坐标缩放；带多个位置的缩放到这些点的外包范围；没找到就缩放到坐标并在消息栏说「对象已不在图上，结果可能过期」。豁免（§4.4）：行尾【豁免】展开理由输入框，【确认】调 DRC-13 的豁免入口；入口报的错显示在消息栏；成功后这一行挪到「已豁免」组，不重新检查；【取消豁免】同理；对不上的豁免（`stale_waivers`）可以逐条删。能不能点按 §4.8。动作键：`locate=N`、`waive=N:理由`、`unwaive=N`。
- **验收**：
  - [ ] `locate=N`：被选中的就是那个对象（用 `get_selection` 验证），视图缩放到它；对象在另一张图上时先切到那张图，全项目结果的列表还在；下标不在列表里 → `PANEL_ROW_NOT_FOUND`。
  - [ ] 豁免：理由为空被拒，`notice` 显示 `WAIVER_REASON_REQUIRED`；填了理由后这一行挪到「已豁免」组，计数立即更新且没有重新检查；撤销一步后图里的豁免没了。
  - [ ] 只读会话里 `can_waive` 为假，`waive=…` 返回 `PANEL_ACTION_UNAVAILABLE`（`reason` 为 `readonly`）；结果不在当前图上是 `not_on_this_drawing`；已经豁免的是 `already_waived`。
- **验证**：`cargo test --offline --locked --lib pid_verify`
- **依赖**：PNL-5；DRC-13；IX-7
- **文件**：`src/ui/window/pid_verify_deliver.rs`、`src/app/commands/pidverify.rs`
- **规模**：M

> **检查点 CP-E2**（PNL-5 – PNL-6 之后）：规则检查页签可用——检查、筛选、点击定位、豁免。请你在界面上试一遍，页面布局有意见这时提，后面两个页签照同样的样子做。

### PNL-7 【界面】材料清单页签

- **做什么**：§4.5 的整个页签。【当前图】【全项目】调 BOM-4 的入口函数；状态行与过期标记；搜索框（子串、不分大小写，比位号、名称、规格、类型编码四栏）；提示行与「有 N 个对象没有位号」；按种类分组的列表（可折叠、每组先画 200 行）；「管道汇总」折叠区；底部合计；【导出…】（弹系统的保存对话框，可选 CSV、XLSX、JSON；导出的是整份清单，不受搜索和折叠影响；接口驱动时直接给路径）；点一行定位；清单出不来时的两种显示（重复位号列表、读不出来的图）。动作键：`bom=drawing|project`、`bom_search=…`、`bom_group=…`、`bom_locate=N`、`bom_export=…`、`more=bom`。面板状态的 `bom` 一块。
- **验收**：
  - [ ] 面板的行与 `pid_bom` 的 `rows` 一致；`bom_search=P-10` 只剩位号、名称、规格或类型编码里含这段文字的行。
  - [ ] `bom_export=<文件>.csv` 导出的文件与 `PIDBOM EXPORT` 导出的逐字节相同，且不受搜索和折叠影响；还没有清单时 `bom_export` 返回 `PANEL_ACTION_UNAVAILABLE`（`no_result`）。
  - [ ] 有重复位号时不显示表，`bom.error` 里是 `BOM_DUPLICATE_TAG` 和重复列表；`bom_locate=N` 选中对应的对象（不单独编号的管件那一行定位到所在管路）。
- **验证**：`cargo test --offline --locked --lib pid_verify`
- **依赖**：PNL-4；BOM-4；IX-7
- **文件**：`src/ui/window/pid_verify_deliver.rs`、`src/app/commands/pidverify.rs`、`src/app/tab_document.rs`
- **规模**：M

### PNL-8 【界面】快照与比对

- **做什么**：「快照与发布」页签的上半（§4.6）。原因输入框和【存快照】；快照列表（新的在上，带勾选框；文件丢了的标「文件缺失」且不能勾；最上面固定一行「当前图（未存快照）」）；【比对】恰好勾了两项才可用（旧的是甲、新的是乙，有「当前图」时它是乙）；比对结果的摘要和三组明细，完全相同时显示「无差异」；点明细里的一项定位（已经删掉的缩放到原来的位置）；【导出比对结果】；图没保存过时的显示。列表在切到这个页签时、存快照后、发布后重读磁盘；勾选变化时清掉比对结果；切换图纸标签页时清空勾选。都调 SNAP-5 的入口函数。动作键：`snapshot_save=…`、`pick=N|live`、`diff`、`diff_locate=组:N`、`diff_export=…`、`more=diff`。
- **验收**：
  - [ ] `snapshot_save=送审前` 后列表多一行；勾 0、1、3 项时 `buttons.diff` 为假，`diff` 动作返回 `PANEL_ACTION_UNAVAILABLE`（`not_two_picked`），恰好 2 项时可用。
  - [ ] 两份相同的快照比对，`deliver.diff.identical` 为真；改图后拿快照与 `live` 比，三组计数正确；`diff_locate=modified:0` 选中对应的对象。
  - [ ] 图没保存过时 `buttons.snapshot_save` 为假（`no_project_dir`）；勾选变化后比对结果清掉；切换图纸标签页后勾选清空。
- **验证**：`cargo test --offline --locked --lib pid_verify`
- **依赖**：PNL-4；SNAP-5；IX-7
- **文件**：`src/ui/window/pid_verify_deliver.rs`、`src/app/commands/pidverify.rs`、`src/app/tab_document.rs`
- **规模**：M

### PNL-9 【界面】发布

- **做什么**：「快照与发布」页签的下半（§4.6）。状态行（未发布 / 已发布，修订 N / 修订 N 之后有改动）；发布目录输入框、【浏览…】和「下一个修订号」；八项条件清单（四个前提随时重算，四道门禁点【检查发布条件】才算）；门禁没过时「错误」那一项能跳到规则检查页签，「重复位号」那一项能展开并定位；【发布】先在面板里问一句再执行，它灰不灰只看四个前提；发布之后的三种结果（成功时刷新状态行、发布记录、快照列表；被门禁拦下时按返回的 `gates` 更新条件清单，返回的检查报告成为这张图的检查结果；其他失败显示错误码和停在哪一步）；发布记录与每行的【校验】。都调 PUB-3 到 PUB-5 的入口函数。动作键：`publish_dir=…`、`publish_check`、`publish`、`package_verify=N`。
- **验收**：
  - [ ] 图有未存修改时 `buttons.publish` 为假，`publish` 动作返回 `PANEL_ACTION_UNAVAILABLE`（`precondition`）；`publish_check` 后 `deliver.publish.checked` 为真、`gates` 有值。
  - [ ] 有没被豁免的错误时发布：`notice` 是 `PUBLISH_BLOCKED`，条件清单标出是哪一道门禁没过、几条，规则检查页签里能看到这些错误，目标目录里没有新增任何东西。
  - [ ] 都满足时发布成功：`state` 变成「已发布，修订 1」，发布记录和快照列表各多一行；改图并保存后变成「修订 1 之后有改动」；`package_verify=1` 显示「完好」。
- **验证**：`cargo test --offline --locked --lib pid_verify`
- **依赖**：PNL-5、PNL-8；PUB-5
- **文件**：`src/ui/window/pid_verify_deliver.rs`、`src/app/commands/pidverify.rs`、`src/app/tab_document.rs`
- **规模**：M

### PNL-10 【界面】进行中的事：进度、取消、按钮置灰

- **做什么**：§4.7 与 §4.8。要读项目里别的图的事（检查全项目、全项目清单、检查发布条件、发布）先用 IX-6 的后台扫描，面板显示进度条「正在读项目图纸 12 / 40」和【取消】，扫完回到界面线程把剩下的做完。同一时刻只做一件：有事在做时，会启动新事的按钮都是灰的，筛选、折叠、定位照常。取消后原来的结果不动。这件事是哪张图发起的，结果就记到那张图（或它的项目目录）名下；期间那张图被关掉了，结果丢弃。面板状态的 `busy`；进度条的位置标识；动作键 `cancel`。顺带把 §4.8 整张表过一遍，补齐灰按钮上的提示文字。
- **验收**：
  - [ ] 全项目检查进行中，`busy` 里有 `what`、`done`、`total`、`cancellable`；这时再发会启动新事的动作，返回 `PANEL_ACTION_UNAVAILABLE`（`busy`），筛选和定位照常。
  - [ ] `cancel` 后 `busy` 回到 `null`，原来的结果不动，`notice` 是「已取消」；发起的那张图中途被关掉时结果丢弃，不记到别的图上。
  - [ ] `build_view` 的单元测试把 §4.8 的表逐条过一遍：每个按钮、每个置灰条件各一例。
- **验证**：`cargo test --offline --locked --lib pid_verify`
- **依赖**：PNL-5、PNL-7、PNL-9；IX-6
- **文件**：`src/ui/window/pid_verify_deliver.rs`、`src/app/commands/pidverify.rs`、`src/app/update/mod.rs`
- **规模**：M

> **检查点 CP-E3**（PNL-7 – PNL-10 之后）：三个页签都可用，全项目的操作有进度、能取消。

### PNL-11 【界面】属性表高亮之一：高亮请求与 `properties` 操作

- **做什么**：§3.6（面板规格待决问题 V3）。程序里留一份「高亮请求」`{document_id, handle, fields}`，至多一份：点一条带 `fields` 的结果并找到对象时设置，新的顶掉旧的。属性行的行号是 `pid_props_<属性名>`（属性名在列表里）或 `pid_tag`（列表里有 `位号`）的算被高亮。列表里的属性对象身上还没有时，属性表临时补一个空行：填了就走现有的提交路径（会把键建出来），不填不写图。何时清掉：选中的对象变了；那个字段被填上了（只清这一个字段）；设置了新的请求；那张图关了。读属性的 `properties` 操作里，被高亮的行多一个键 `"highlighted": true`。这一步不碰画法。
- **验收**：
  - [ ] 点一条 `DR-010` 的结果：`properties` 的应答里缺的那个字段有一行、且带 `highlighted`；点一条 `DR-001` 的结果，`管道等级` 那一行带 `highlighted`；用到位号的规则高亮 `pid_tag` 那一行。
  - [ ] 往临时空行填上值：照常提交、键被建出来，这一行不再高亮，重新检查后这条结果没有了；不填就不写图（图的「已修改」状态不变）。
  - [ ] 选中别的对象、设置新的请求、关掉那张图，高亮都清掉；没有高亮请求时 `properties` 的应答与改动前完全相同。
- **验证**：`cargo test --offline --locked --lib pid_verify_highlight`
- **依赖**：PNL-6
- **文件**：`src/app/mod.rs`、`src/app/commands/pidprops.rs`、`src/app/control/actions.rs`、`src/app/commands/pidverify.rs`
- **规模**：M

### PNL-12 【界面】属性表高亮之二：两处属性表的画法

- **做什么**：经典界面的「特性」面板（行在 `src/app/properties.rs` 里组装，画在 `src/ui/properties.rs`）和外壳的属性编辑器（`src/app/pid_shell/editor.rs`）：被高亮的行用警示底色画。不改属性行的数据结构，画的时候按行号查高亮请求。这两个文件很大（经典面板的两个文件合计约 450 KB），只在画行的那一处加判断，不顺手改别的。
- **验收**：
  - [ ] 「行号 → 是否高亮」的判断函数有单元测试，两处属性表用的是同一个函数。
  - [ ] 没有高亮请求时，两处属性表与改动前完全相同（现有的属性表测试全部通过）。
  - [ ] 人工：点一条 `DR-010` 的结果，两处属性表里缺的字段那一行是警示底色，截图留证。
- **验证**：`cargo test --offline --locked --lib pid_verify_highlight`；`cargo test --offline --locked --lib properties`；手工看两处属性表
- **依赖**：PNL-11
- **文件**：`src/ui/properties.rs`、`src/app/properties.rs`、`src/app/pid_shell/editor.rs`
- **规模**：M

### PNL-13 【脚本】面板的性能预算与端到端脚本

- **做什么**：性能预算测试：2,000 条结果时重建要画的内容的耗时。端到端脚本（起真实程序，经 `--http` 接口驱动，不模拟鼠标、不靠截图），放在 `scripts/pid-verify-e2e/`，写法照 `scripts/collab-e2e/`（CC-5）：计划点名的那一条——切页签 → 触发检查 → 读违规列表 → 导出材料清单；另加一次全项目检查，验证进度与取消；再验一次控件位置（这个只有真实窗口里才有）。脚本不写死本机路径，不留证据文件在仓库里。
- **验收**：
  - [ ] 2,000 条结果时重建要画的内容不超过 50 ms；每组只画前 200 行。
  - [ ] 脚本跑通：切页签 → 触发检查 → 读到的违规列表与 `pid_check` 一致 → 导出的材料清单文件存在且行数正确。
  - [ ] 脚本跑通：全项目检查时读到 `busy.done` / `busy.total`，取消后原结果不动；面板显示时 `ui_bounds` 按前缀 `pid-verify-` 读得到面板和三个页签，隐藏后读不到。
- **验证**：`cargo test --offline --locked --lib -- --ignored pid_verify_build_view_2000`；`python scripts/pid-verify-e2e/run.py`（脚本自己用 `cargo run --offline --locked -- --http 8090` 起程序）
- **依赖**：PNL-10、PNL-12
- **文件**：`src/ui/window/pid_verify_deliver.rs`（性能测试）；新建 `scripts/pid-verify-e2e/run.py`、`scripts/pid-verify-e2e/README.md`
- **规模**：M

### END-1 【文档】全量门禁与文档收尾

- **做什么**：把所有门禁全量跑一遍：`--lib` 测试、两个协同 crate、clippy 两档、`rustfmt` 新增差异、全部 `--ignored` 的性能预算测试、`scripts/collab-e2e` 与 `scripts/pid-verify-e2e`。把已批准计划里的 T3.1 – T3.4 标成完成并写上提交号；能力地图 §2 里各模块的「现状」从「未开始」改成「已完成」，各新模块规格开头的状态从「待建」改成「已建成」；把各规格的「实现记录」汇总一遍，实现与规格有出入的地方列给你；写一份阶段 3 结项报告（做了什么、各项读数、已知限制、阶段 4 从哪里接）。
- **验收**：
  - [ ] 结项报告里有每一项门禁的读数：`--lib` 测试失败的仍只是那 4 条；clippy 在 630 / 965 以内；每个性能预算测试都达标并列出实测值。
  - [ ] 已批准计划的 T3.1 – T3.4、能力地图的状态都已更新；实现与规格的每一处出入都有记录。
  - [ ] `git status --short` 只剩那个不提交的 `.pen`（本目录的文档在开工前已经单独提交，见 `plan.md` §11）；没有推送，等你决定。
- **验证**：§1 的全部命令；`cargo test --offline --locked --lib -- --ignored`（性能预算）；两套端到端脚本；`git status --short`
- **依赖**：前面全部任务
- **文件**：`docs/plans/2026-11-16-pid-platform-next-steps-plan.md`、`docs/plans/2026-11-16-pid-dev-spec/00-capability-map.md`、各 `SPEC-*.md`（只加「实现记录」）、新建 `docs/plans/2026-11-16-pid-dev-spec/report-phase3.md`
- **规模**：S

> **检查点 CP-E（阶段 E 结束，请你确认）**：演示——打开面板，检查一张图，点一条「必填项为空」的结果，对象被选中、属性表里缺的字段被标出来；填上、重新检查、出清单、存快照、发布，全程在面板里完成。结项报告。由你决定是否推送。
