# 协同端到端脚本：验收契约对照表与运行方法

这份文件回答一个问题：228 条验收契约（`VAL-…`）里，每一条现在由哪条测试、或哪个端到端脚本的哪些检查项盯着；哪些没有，要补什么。

- 对照的基准：提交 `278439b5d`（2026-11-17）。此后为补缺口加的测试和场景，表里照常引用；补到哪一类了，见第 5.1 节。
- 契约原文：mission 目录里的 `validation-contract.md`，路径见 `docs/plans/2026-11-10-mission-p1-collab-status.md` 开头。各区条数与那份文档的第 4 节一致。
- 脚本就在本目录里（2026-11-17 收进仓库，`docs/plans/2026-11-16-pid-dev-spec/todo.md` 的 CC-5）。证据（日志、截图、数据库、图纸副本）不进仓库，写到运行时指定的目录，见第 3.2 节。

## 1. 口径

**「覆盖」一列的三种取值**

| 取值 | 意思 |
|---|---|
| 有 | 契约的每条判据，在所列的测试或脚本检查项里都有断言 |
| 部分 | 一部分判据有断言；「说明」里写了哪一部分没有 |
| 缺 | 没有能重跑的检查 |

**读这张表要记住的三件事**

1. **表里说的是「有没有能重跑的检查」，不是「这一条验过没有」。** mission 的验证记录里，前六个区的 178 条和 `VAL-CONF-018` 当时都由验证器用一次性的命令和截图取过证；后三个区的其余 49 条功能做了、脚本也过了，但没有经过 mission 的验证器。两种状态列在第 2 节的合计表里。
2. **「有」里只列了单元测试的，断言是在进程里做的。** 契约写的取证工具是黑盒的（curl、WebSocket 客户端、GUI 的 REST 接口、截图）。单元测试把判据逐条断言到了，但「经 REST 或 WebSocket 走一遍也是这个结果」这一层，只有 mission 当时的一次性取证。前六个区基本都是这种情况，合计表里单列了一栏。凡是判据落在截图、耗时上、单元测试够不着的，标成了「部分」或「缺」；落在真实进程上的六条原来也是，2026-11-17 补了四个场景（第 5.1 节（三））。
3. **「有」不等于「现在是绿的」。** 单元测试的名字取自全量测试日志（基准提交上的一份，加上补缺口时重跑的），当时都通过。端到端脚本的项数取自 2026-11-12 那次全量执行，当时都通过；脚本收进仓库后（2026-11-17）又跑过两遍，两遍都有同一项因为后来的改动不再通过（第 3.3 节）。服务器进程的四个场景是 2026-11-17 新写的，项数取自当天它们经总入口跑的那一次。脚手架与入口规程的四个场景、收尾的整轮检查，和 `t20_session` 里标 `VAL-INFRA-012` 的一项，也是 2026-11-17 写的，项数取自当天总入口全量跑的那一遍（第 3.3 节）。后来的那些改动，脚本里还没有对应的断言（第 5.2 节）。

**测试名怎么读**

| 写法 | 在哪里 | 怎么单独跑 |
|---|---|---|
| 不带前缀，如 `app::collab::…` | 主程序的库 | `cargo test --lib <名字>` |
| `ocs_collab::…` | `crates/ocs_collab` | `cargo test -p ocs_collab <去掉前缀的名字>` |
| `ocs_collab_server::…` | `crates/ocs_collab_server` 的库 | `cargo test -p ocs_collab_server --lib <去掉前缀的名字>` |
| `ocs_collab_server[main.rs]::…` | `crates/ocs_collab_server/src/main.rs` | `cargo test -p ocs_collab_server --bin ocs_collab_server <去掉前缀的名字>` |
| 后面带「（默认不跑）」 | 标了 `#[ignore]` | 命令末尾加 `-- --ignored` |

**脚本的检查项怎么读**

- 脚本里的检查项不编号。每一项带一个标签，标签就是契约编号；另有 `setup`、`seed`、`harness` 这样不对应契约的标签。
- 表里写「脚本名 N 项」：N 是结果文件里带该契约编号的检查项数（取的是哪一次执行，见上面的第 3 件事）。
- 结果文件是 `logs/<场景名>.result.txt`，每行依次是 `PASS` 或 `FAIL`、标签、这一项的说明。

**脚本里的截图是怎么查的**

| 查法 | 用在哪里 |
|---|---|
| 只查截图请求成功、文件写出来了 | 大多数截图 |
| 读程序上报的界面元素位置，判断元素画出来了 | 协同面板的只读标识、编辑权持有者、占用行、冲突条目、未配置提示 |
| 在截图上量像素 | 只有 `VAL-CLI-008` 的占用色 |

图上的文字没有任何脚本去认。

## 2. 合计

| 区 | 条数 | 有 | 其中只靠单元测试 | 部分 | 缺 | mission 记录：已取证 | mission 记录：未取证 |
|---|---:|---:|---:|---:|---:|---:|---:|
| 协同服务器（`VAL-SRV`） | 30 | 30 | 24 | 0 | 0 | 30 | 0 |
| 验证基础设施（`VAL-INFRA`） | 14 | 12 | 6 | 2 | 0 | 14 | 0 |
| 权限与审计（`VAL-PERM`） | 32 | 32 | 32 | 0 | 0 | 32 | 0 |
| 语义层（`VAL-SEM`） | 39 | 38 | 38 | 1 | 0 | 39 | 0 |
| 绘图工具（`VAL-DRAW`） | 31 | 16 | 16 | 14 | 1 | 31 | 0 |
| 管线与拓扑（`VAL-PIPE`） | 32 | 31 | 31 | 0 | 1 | 32 | 0 |
| 桌面端协同客户端（`VAL-CLI`） | 20 | 20 | 0 | 0 | 0 | 0 | 20 |
| 冲突与裁决（`VAL-CONF`） | 18 | 18 | 1 | 0 | 0 | 1 | 17 |
| 跨区流程（`VAL-CROSS`） | 12 | 12 | 0 | 0 | 0 | 0 | 12 |
| **合计** | **228** | **209** | **148** | **17** | **2** | **179** | **49** |

- 「有」209 条，「部分」17 条，「缺」2 条。后两种共 19 条，按要做的事分类，逐条列在第 5.1 节。对照表刚做出来时是 39 条、分五类，其中补单元测试的一类（7 条）、要真实服务器进程的一类（6 条）、验收规程与脚手架的一类（7 条）已经补完。
- 「有」的 209 条里，61 条有端到端脚本，148 条只靠单元测试（第 1 节的第 2 件事）。
- `VAL-DRAW-029` 是契约里预留的空号，所以绘图区是 31 条。
- 「mission 记录」两栏照抄 mission 的状态文件：已取证是它的 `passed`，未取证是它的 `pending`。

## 3. 怎么跑

### 3.1 单元测试

```
cargo test --offline --locked --lib
cargo test --offline --locked -p ocs_collab -p ocs_collab_server
```

读数（2026-11-17，补完 7 条单元测试之后）：主程序库 6945 条（6892 通过、49 默认不跑、4 失败，失败的 4 条是设计稿比对测试，与协同无关，表里没有引用）；`ocs_collab` 95 条、`ocs_collab_server` 的库 165 条、`main.rs` 18 条，全部通过。基准提交上主程序库是 6944 条（6891 通过），`ocs_collab_server` 的库是 159 条，其余相同。

### 3.2 端到端脚本

**需要什么**

- Windows，Python 3（只用标准库）。
- 编好的两个可执行文件：`OpenCADStudio.exe` 和 `ocs_collab_server.exe`（debug 构建即可）。在仓库根目录执行 `cargo build --offline --locked --bin OpenCADStudio` 和 `cargo build --offline --locked -p ocs_collab_server`。
- 一个仓库之外的目录，用来放证据。
- `cargo` 和 `git` 在路径上：总入口收尾的整轮检查要跑一次 `cargo tree`（离线，不编译）和只读的 `git status`；证据目录里没有基线图时，还要用 `cargo test` 生成它（见下）。
- GUI 起在单独的隐藏桌面上，只走本机的 REST 通道（`--http`），不动鼠标键盘，跑的时候机器可以照常用。

**路径和端口怎么给**

脚本里没有写死的本机路径。三个入口（`run19.ps1`、`run_all.ps1`、`run_cross.ps1`）都认下面三个参数，不给参数时看环境变量；直接用 `python` 跑单个场景时只看环境变量。

| 参数 | 环境变量 | 是什么 | 不给时 |
|---|---|---|---|
| `-EvidenceDir` | `OCS_E2E_EVIDENCE_DIR` | 证据目录。日志、截图、数据库、图纸副本写在它下面的 `m6\`（桌面端客户端那一组、脚手架与入口规程那一组）和 `m7\`（其余三组和收尾的整轮检查）；`t04_entry_rerun` 用的替身场景另写在 `t04\` 下 | 报错退出；给的目录在仓库里面也报错退出 |
| `-BinDir` | `OCS_E2E_BIN_DIR` | 两个可执行文件所在的目录 | 仓库的 `target\debug` |
| `-PortShift` | `OCS_E2E_PORT_SHIFT` | 加在每个端口上的平移量 | 0，即契约的端口段：GUI 3210–3239、服务器 3240 |

- 另有两个只认环境变量的：`OCS_E2E_BASE_DWG`（场景拿来复制的基础图，不给时用仓库里的 `tests/fixtures/plot/page-setups-metric.dwg`）、`OCS_E2E_EXTRA_ARGS`（追加给每个 GUI 实例的命令行参数）。
- 契约的端口段被占用时（比如同一台机器上另一个工作区也在跑），给 `-PortShift 100`，整段挪到 3310–3359。检查项的说明文字里写的仍是契约的端口号，实际用的端口看控制台输出里的 `port=`。
- 例：`powershell -File scripts\collab-e2e\run19.ps1 -EvidenceDir <仓库外的目录> -Label <这一轮的名字>`。

**30 个场景，分五组，另有一项收尾的整轮检查**

| 组 | 场景 | 入口 |
|---|---|---|
| 脚手架与入口规程（4 个） | `t00_smoke`、`t02_harness_rules`、`t03_vanish_retry`、`t04_entry_rerun` | `run_all.ps1 -Only <场景名> -New` |
| 冲突与跨区流程（11 个） | `c01_field_merge`、`c02_object_topology`、`c03_resolution`、`x01_journey`、`x02_claim_exclusive`、`x04_readonly_role`、`x05_m3_persistence`、`x06_first_visit`、`x07_conflict_closure`、`x10_concurrent_insert`、`x12_after_session` | `run_cross.ps1` |
| 桌面端客户端（8 个） | `t20_session`、`t21_claims`、`t23_stale`、`t24_reconnect`、`t25_offline`、`t10_broadcast`、`t11_same_user`、`t22_ttl` | `run_all.ps1` |
| 后补的跨区流程（3 个） | `y03_pipe_topology`、`y08_offline_reconcile`、`y09_connector_cross_drawing` | `run_cross.ps1 -Only <场景名> -New` |
| 服务器进程（4 个） | `s01_restart`、`s02_port_in_use`、`s03_disconnect`、`s04_concurrent_ops` | `run_cross.ps1 -Only <场景名> -New` |
| 收尾的整轮检查（1 项） | `r01_entry_rules` | 总入口每一轮都在最后跑它 |

- **总入口 `run19.ps1`**：五组按表里的顺序依次跑，最后跑 `r01_entry_rules`。除上面三个参数外还有 `-Label`（这一轮的名字）、`-OnlyHarness`（只跑第一组）、`-Only19`（只跑第二、三组）、`-OnlyNew`（只跑第四组）、`-OnlyServer`（只跑第五组）；几个一起给时，跑的是各自那几组合起来。不管挑了哪几组，收尾的 `r01_entry_rules` 都照跑。
- **脚手架与入口规程那一组**判的不是产品功能，是「验收是不是照规程做的」（规程见下面的「脚手架和入口照着做的规程」）：
  - `t00_smoke`：带一张真实图纸在端口 3210 起一个 GUI，查就绪（60 秒之内、响应的五个字段）和启动前查过端口，再截一张图。
  - `t02_harness_rules`：把脚手架里的三条规则逐条试一遍。它有意去要段外的端口、占住端口、起不会就绪的实例、强行结束实例，控制台输出里的「没有就绪」「消失」是它要的。
  - `t03_vanish_retry`：建实体的流程做到一半，把实例从外面结束，看入口把整个场景重跑一遍。**它的第一次尝试一定以退出码 75 收场**，汇总里它占两行，这是正常的。要经入口跑：重跑是入口做的。
  - `t04_entry_rerun`：不起 GUI，用四个只会按设定的退出码退出的替身场景，试入口的重跑规则。
- **收尾的整轮检查 `r01_entry_rules`**：总入口一开头先执行 `python r01_entry_rules.py --begin`，记下开跑的时刻、`git status --porcelain` 和脚本旁边已有的字节码缓存；最后再跑它，核对五件事：
  - 这一轮没有往仓库里写东西（`VAL-INFRA-013`）；
  - 丢过实例的场景都重跑了一次、只重跑了一次、第一次的结果留着，没丢的都只跑了一次（`VAL-INFRA-007`）；
  - 这一轮每一次 GUI 启动的端口都在段内、启动前都查过（`VAL-INFRA-008`）；
  - 桌面程序的依赖树里没有 `tokio`、`async-std`，有 `tungstenite`（`VAL-CLI-019`）；
  - 两份脚手架里这几条规则的代码一字不差。

  它跑的 `git status` 是只读的（带 `--no-optional-locks`，连索引也不刷新）。单独跑它（`run_cross.ps1 -Only r01_entry_rules -New`）之前，要先执行带 `--begin` 的那一步，否则第一项失败并说明原因。
- **服务器进程那一组**不起 GUI、不用图纸，只起 `ocs_collab_server.exe` 和几条 WebSocket 连接；入口照样要求两个可执行文件都在。
- **种子图和基线图**：证据目录里没有时，总入口先生成。两张种子图用 `x00_seed.py`、`t01_seed.py`；`x05_m3_persistence`、`x12_after_session` 用的基线图 `PID-1101-m3.dwg` 由一条默认不跑的测试写出（`write_the_m3_baseline_drawing`），总入口用 `cargo test` 跑它，第一次要等测试程序编出来（几分钟）。分组入口不生成这三张图：换了新的证据目录时，先跑一次总入口。只跑第一组、第五组（`-OnlyHarness`、`-OnlyServer`）时总入口也不生成，这两组用不着。
- **单独跑几个场景**：`run_all.ps1 -Only <场景名>`、`run_cross.ps1 -Only <场景名>`，多个场景名用逗号隔开；第一、四、五组的场景和 `r01_entry_rules` 要再加 `-New`（表里「入口」一列写着）。
- **结果落在哪里**：都在证据目录里。每个场景一份控制台输出（`logs/<场景名>.console.txt`）和一份 `logs/<场景名>.result.txt`，场景结束时打印一行 `=== <场景名>: N PASS / M FAIL ===`；总入口收尾在证据目录的根上写 `summary-<Label>-<时间>.txt`，每个场景一行 `pass=`、`fail=`、`exit=`，每组一行小计。有检查项失败、或者进程的退出码不是 0 的场景，小计里都记成不干净（`not clean`）；重跑过的场景，第一次尝试单占一行，不计入小计。
- **入口还记什么**：每个场景结束后记下耗时、残留进程数（场景结束后还在跑、可执行文件在 `-BinDir` 或证据目录下的进程；`run_cross.ps1` 另记留下的自动化通道描述文件数），以及场景自己报的桌面记录——场景前后各看一次指针位置与前台窗口，写「没变」或「变了」。这几样只写进汇总，入口不拿它们判失败，要人看一眼。桌面记录分不出是场景动的还是当时用机器的人动的。总入口另在开头和结尾各记一次机器上不属于这一轮的同名进程数，出了失败好对照。
- **要多久**：五组加收尾检查一遍约 15 分钟（2026-11-17 那一轮：脚手架与入口规程一分半，冲突与跨区流程一分半，桌面端客户端十分半，后补的跨区流程不到一分钟，服务器进程和收尾检查合起来十来秒）。最慢的是 `t22_ttl`（真等 300 秒的占用到期），其次是 `t23_stale`、`t25_offline`、`t20_session`、`t02_harness_rules`（各一分多钟）。证据目录是新的时候，前面还要加上生成基线图的几分钟。
- **直接用 `python` 跑单个场景**：先设环境变量 `OCS_E2E_EVIDENCE_DIR`，再执行 `python scripts\collab-e2e\<场景名>.py`。这样跑没有入口的重跑和记账，还会在本目录留下 `__pycache__`（已被 `.gitignore` 忽略；先设 `PYTHONDONTWRITEBYTECODE=1` 就不会留）。经入口跑不会留，`r01_entry_rules` 会查这一点。
- **贴日志之前先看一眼**：入口记「桌面有没有被动过」时，会把前台窗口的标题写进控制台输出和汇总文件——也就是当时正在用的窗口。

**脚手架和入口照着做的规程**

验收契约里有几条管的是「怎么验」，不是产品行为。它们写在两个脚手架（`m6h.py`、`m7h.py`，两份里这部分代码相同）和三个入口共用的 `e2e_env.ps1` 里，对所有场景生效。

| 规程 | 做法 | 留下什么（都在证据目录的 `m6\logs`、`m7\logs` 里） |
|---|---|---|
| GUI 的端口（`VAL-INFRA-008`） | 端口只能在 3210–3239 里（平移时整段跟着平移），要段外的端口直接报错。每次启动前先查要的端口有没有人在监听（去连本机回环地址，连得上就是有）；有人在监听、或者同一个场景里另一个实例占着，就从 3239 往下另挑一个空的 | `gui-starts.jsonl`，每次启动一行：命令行、要的端口、实际用的端口、查过哪些端口和各自的结果 |
| 等就绪的上限（`VAL-INFRA-014`） | 到上限还没有就绪、或者没就绪进程就退出了：结束进程，留下日志尾部，换一个端口再起一次，再等一个上限；第二次仍不就绪就结束它，没有第三次。上限在冒烟场景里是契约的 60 秒，其余场景是 180 秒（第 5.3 节） | `gui-retries.jsonl`，每次重试一行：原来的端口、等了多久、轮询了几次、日志尾部、重试用的端口和结果 |
| 实例消失（`VAL-INFRA-007`），脚手架这一半 | 就绪过的实例，场景没有结束它、也没有让它退出，它却不在了：进程已经退出；或者请求被拒绝之后，隔半秒连探 3 次 `/api/v1/ready` 都被拒绝。记下现象，场景收尾后以退出码 75 退出。请求超时不算；场景自己结束的、让它退出的实例不算；还没就绪就退出的归上一条 | `vanished-<场景名>-<实例名>-<时间>.json`：怎么发现的、退出码、时刻、当时在做的请求、命令行、日志尾部 |
| 实例消失，入口这一半 | 场景以退出码 75 退出，就把它整个重跑一次：脚本从头再跑，要的还是原来的端口，图按场景开头的做法准备（`t03_vanish_retry` 接着用第一次那张图）。第二次又丢实例，记失败，不跑第三次。别的退出码不重跑 | 第一次的控制台输出和结果另存为 `<场景名>.attempt1.console.txt`、`<场景名>.attempt1.result.txt`；`<场景名>.runs.txt` 每次尝试一行（第几次、退出码、秒数、残留进程数） |
| 产物不进仓库（`VAL-INFRA-013`） | 证据目录不给、或给在仓库里面，入口和脚手架都直接退出；入口不让 Python 写字节码缓存；一轮开头和结尾各看一次 `git status`（见上面的 `r01_entry_rules`） | `m7\logs` 里的 `r01-git-status.txt`（跑完后的全文）、`r01-files-written.txt`（这一轮写下的文件清单） |

**这些脚本是从哪里来的**

脚本是 2026-11-17 从仓库外的工作目录收进来的，收的是最近执行的那一份，对照表对的也是它。那一份是从更早的原件复制出来的，复制时改过三处：可执行文件目录改成可以指定；端口整体加了 100，好和另一条开发线在同一台机器上同时跑；残留进程只看那条线的。收进来时：

- 端口改回了契约的端口段（服务器 3240、GUI 3210–3239），要错开时用 `-PortShift`。
- 写死的本机路径（证据目录、可执行文件目录、基础图、隐藏桌面库的位置）都改成由参数、环境变量和脚本自己所在的位置给出。基础图改用仓库里已有的 `tests/fixtures/plot/page-setups-metric.dwg`，它和原来仓库外的那一张逐字节相同。
- 基线图 `PID-1101-m3.dwg` 原来是从一个写死的路径复制的，改成缺的时候现生成。
- 三个入口共用的部分（解析上面三个参数、数残留进程）放在 `e2e_env.ps1`。`run_all.ps1` 的 `-Only` 也改成可以用逗号隔开多个场景名（`run_cross.ps1` 原来就可以）。
- 与原件逐个文件比过。有原件的 22 个脚本里，17 个把端口减回去之后与原件一字不差；5 个各改了一处：`t20_session.py`、`t23_stale.py`、`t25_offline.py`、`x06_first_visit.py` 里各有一处绕过脚手架直接写端口号的地方，改成跟着平移量走；`x05_m3_persistence.py` 的说明文字里有本机路径，改写了。两个脚手架（`m6h.py`、`m7h.py`）只改了路径和端口的取法。后补的三个场景和它们共用的 `yh.py` 没有更早的原件。
- 逐个文件查过内容：没有口令，没有 `127.0.0.1` 之外的地址。脚手架给每个 GUI 实例的本机 REST 通道用的访问令牌是每次运行随机生成的，经环境变量传给实例，不写进任何文件。
- 没有收的：日志、截图、数据库、可执行文件，以及当初制作副本用的一次性工具。
- 收进来时没有做的：场景的断言一处没动，对照表里记的缺口原样留着（此后补了哪些，见第 5.1 节）。
- 收进来之后新写的：服务器进程的四个场景（`s01`–`s04`），是为补第 5.1 节（三）的缺口写的，没有原件。为它们给脚手架 `m7h.py` 加了一个「不发 close 帧、直接销毁套接字」的做法，给总入口加了服务器进程这一组。
- 为补第 5.1 节（五）的缺口改的和新写的（2026-11-17）：两个脚手架加了端口、等就绪的上限、实例消失三条规则；三个入口里「跑一个场景」的那一段收进了 `e2e_env.ps1`，加了「退出码 75 重跑一次」；总入口加了第一组和收尾的整轮检查，汇总里开始记退出码。新写了四个脚本（`t02_harness_rules`、`t03_vanish_retry`、`t04_entry_rerun`、`r01_entry_rules`），没有原件。冒烟场景 `t00_smoke` 从端口 3230 改到契约写的 3210，等就绪的上限从 180 秒改成 60 秒，加了 5 项检查，挂进了总入口；`t20_session` 加了 1 项（`VAL-INFRA-012`）。原有的检查项没有改。

### 3.3 最近的执行

**2026-11-17，五组全量加收尾的整轮检查**（可执行文件是提交 `b3b366072` 的源码的 debug 构建，脚本是补完验收规程之后的；端口不平移，经总入口 `run19.ps1`）

- 30 个场景加收尾的整轮检查，共 491 项检查：490 项通过、1 项失败。失败的仍是 `t20_session` 里那一项（说明在下面 2026-11-17 原有 22 个场景那次执行的记录后面），其余 29 个场景和收尾检查全部通过。
- 各组：脚手架与入口规程 30 项（`t00` 7、`t02` 13、`t03` 5、`t04` 5）；冲突与跨区流程 208 项；桌面端客户端 110 项通过、1 项失败（`t20` 现在是 16 项：原有 15 项加 `VAL-INFRA-012` 一项）；后补的跨区流程 81 项；服务器进程 56 项；收尾的整轮检查 5 项。
- `t03_vanish_retry` 跑了两次：第一次按设计丢了实例、以退出码 75 结束（1 项通过，控制台和结果另存为 `attempt1`，不计入上面的数），入口重跑，第二次 5 项全过。别的场景都只跑了一次；退出码除 `t20_session` 是 1 之外都是 0。
- 收尾检查记下的：这一轮写了 1394 个文件，都在证据目录里；`git status` 前后都是 14 行，没有多出一行；没有新的字节码缓存。GUI 启动 54 次（24 个脚本），端口都在 3210–3239 之内、启动前都查过；其中 5 次在段内换过端口，都是 `t02_harness_rules` 有意做的，别的场景没有一次换端口、没有一次就绪超时重试。依赖树 572 个包，没有 `tokio`、`async-std`，有 `tungstenite` 0.29.0。
- 没有残留进程，没有留下描述文件，开始和结束时机器上都没有别的实例。桌面记录全部是「没变」。从第一个场景到收尾约 15 分钟；此前生成基线图用了不到 5 分钟（要先把测试程序编出来）。
- 原有 22 个场景和服务器进程的 4 个场景，各场景各标签的项数与此前记下的逐个比过：除 `t20_session` 多出 `VAL-INFRA-012` 一项、它过时的那一项仍然失败之外，全部相同。
- 这一遍各项的证据没有逐条核对。正式的全量复验是任务清单里的 CC-7。

**2026-11-17，只跑服务器进程那一组**（服务器是提交 `412f10a2c` 的 debug 构建，端口不平移，经总入口 `run19.ps1 -OnlyServer`）

- 4 个场景 56 项全部通过：`s01` 19、`s02` 12、`s03` 9、`s04` 16。退出码都是 0，没有残留进程，桌面记录四个都是「没变」。整组 13 秒。
- 此前各自重复跑过（`s04` 12 遍，其余各 4 遍），每一遍都全部通过。
- 其余各组这一次没有跑。

**2026-11-17，原有的 22 个场景（现在的第二、三、四组），用本目录里的脚本**（提交 `6b1be96ac` 的 debug 构建，端口不平移，经总入口 `run19.ps1`）

- 22 个场景里 21 个全部通过，`t20_session` 有 1 项失败：共 398 项通过、1 项失败。
- 每个场景的检查项、各标签下的项数，都与 2026-11-12 那次相同。
- 没有残留进程，没有留下描述文件，开始和结束时机器上都没有别的实例；跑完之后仓库里没有多出文件。桌面记录 11 个场景是「没变」、11 个是「变了」。
- 这一遍是为了确认收进来的入口能用，各项的证据没有逐条核对。正式的全量复验是任务清单里的 CC-7。

失败的那一项在 `t20_session` 里，是标 `VAL-CLI-001` 的五项之一（「服务器日志出现一条入站 hello」）。它数服务器日志里含「入站 hello」的行，要求正好一行。加入时上报已有位号的改动（第 5.2 节表里的第一行）之后，服务器对同一次 hello 记两行：上报的结果一行，发出 `welcome` 一行，所以数出来是 2。同一标签下的另外四项（会话表正好新增一行、客户端收到的 `welcome`、状态接口、`welcome` 帧的形状）都通过。这是脚本的写法落后于改动，不是功能退了。收进仓库时只搬运、不改断言，所以没有动它；要改的写在第 5.2 节。

**2026-11-12，用仓库外的副本**（可执行文件构建自提交 `9bf874a34`）

22 个场景 399 项全部通过，退出码都是 0，没有残留进程，没有留下描述文件。桌面记录 6 个场景是「没变」、16 个是「变了」（分不出是谁动的，见 3.2）。对照表里原有 22 个场景的项数取自这一次。

| 组 | 各场景的项数 | 小计 |
|---|---|---:|
| 冲突与跨区流程 | `c01` 24、`c02` 17、`c03` 20、`x01` 13、`x02` 20、`x04` 26、`x05` 14、`x06` 21、`x07` 19、`x10` 15、`x12` 19 | 208 |
| 桌面端客户端 | `t20` 15、`t21` 15、`t23` 9、`t24` 11、`t25` 13、`t10` 32、`t11` 7、`t22` 8 | 110 |
| 后补的跨区流程 | `y03` 26、`y08` 33、`y09` 22 | 81 |

这一次是在协同分支合进 `main`（`af23cffef`）和合并上游（`2278e4fba`）之前跑的。从 `9bf874a34` 到基准提交，有 10 个提交动过协同的代码（第 5.2 节）。

## 4. 对照表

### 4.1 协同服务器 `VAL-SRV`（30 条）

契约的取证工具是 curl 和 WebSocket 客户端，对着真实的服务器进程。多数条目由服务器自己的测试覆盖（进程内起路由和存储）；要真的重启进程、抢端口、断套接字、并发提交的六条（`024`–`028`、`030`）另有四个对着真实进程的场景 `s01`–`s04`，只起服务器和 WebSocket 连接，不起 GUI。另有三条（`008`、`013`、`016`）在桌面端的场景里有真实进程上的检查，写在「说明」里。

| 编号 | 契约标题 | 单元测试 | 端到端脚本 | 覆盖 | 说明 |
|---|---|---|---|---|---|
| `VAL-SRV-001` | 健康检查返回服务就绪 | `ocs_collab_server::http::tests::health_reports_ok_with_a_working_database`<br>`ocs_collab_server::store::tests::health_check_passes_on_a_writable_database`<br>`ocs_collab_server::tests::a_memory_store_reports_a_healthy_database` | — | 有 | 各端到端场景开头的 `setup` 项也在真实进程上等 `/health` 成功 |
| `VAL-SRV-002` | 健康检查在数据库不可写时报告不健康 | `ocs_collab_server::http::tests::health_reports_failure_without_a_database`<br>`ocs_collab_server::store::tests::health_check_fails_on_a_readonly_database`<br>`ocs_collab_server[main.rs]::tests::a_database_path_that_is_a_directory_yields_an_unhealthy_state` | — | 有 |  |
| `VAL-SRV-003` | HTTP 建会话返回 session_id | `ocs_collab_server::http::tests::reposting_the_same_session_refreshes_instead_of_duplicating`<br>`ocs_collab_server::http::tests::a_session_created_over_http_blocks_a_second_editor` | — | 有 | 状态码与 `session_id` 有断言；`base_revision` 是整数由响应结构体的类型保证，没有单独断言 |
| `VAL-SRV-004` | WebSocket 建连并完成 hello 握手 | `ocs_collab_server::ws::tests::hello_opens_a_session_and_returns_welcome`<br>`ocs_collab::protocol::tests::hello_round_trips_through_json_with_workspace` | — | 有 |  |
| `VAL-SRV-005` | 未完成 hello 前的请求返回结构化错误 | `ocs_collab_server::ws::tests::a_message_before_hello_is_refused` | — | 有 |  |
| `VAL-SRV-006` | 未知消息类型返回结构化错误 | `ocs_collab_server::ws::tests::unparseable_frames_map_to_invalid_message`<br>`ocs_collab::protocol::tests::unknown_message_type_fails_to_deserialize` | — | 有 |  |
| `VAL-SRV-007` | 心跳维持会话活跃 | `ocs_collab::session::tests::heartbeat_within_the_timeout_keeps_the_session_active` | — | 有 | 时刻是注入的（3 次间隔 15 秒的心跳），不是真等 45 秒 |
| `VAL-SRV-008` | 超过超时阈值未心跳的会话判失效 | `ocs_collab::session::tests::heartbeat_after_the_timeout_marks_the_session_stale`<br>`ocs_collab::session::tests::a_heartbeat_timeout_from_config_drives_the_sweep`<br>`ocs_collab_server::ws::tests::sweeping_a_stale_session_releases_claims_and_broadcasts_peer_left`<br>`ocs_collab_server::ws::tests::a_heartbeat_on_a_stale_session_reports_expired` | — | 有 | 时刻是注入的；真实进程上等 90 秒判失效见 `VAL-CLI-003`（`t23_stale.py`） |
| `VAL-SRV-009` | 同图同模式会话可被接管 | `ocs_collab::session::tests::same_drawing_same_mode_takes_over_a_stale_session`<br>`ocs_collab_server::ws::tests::takeover_releases_the_old_sessions_claims_and_notifies_the_room` | — | 有 |  |
| `VAL-SRV-010` | 首次位号占用申请获批 | `ocs_collab_server::ws::tests::claim_after_hello_is_granted_and_mirrored`<br>`ocs_collab::occupancy::tests::first_claim_is_granted_and_visible` | — | 有 |  |
| `VAL-SRV-011` | 第二位申请者被拒且拿到占用者信息 | `ocs_collab_server::ws::tests::a_second_session_claiming_the_tag_is_denied`<br>`ocs_collab::occupancy::tests::second_claimant_is_denied_with_holder_and_expiry`<br>`ocs_collab::protocol::tests::claim_denied_reports_holder_and_expiry` | — | 有 |  |
| `VAL-SRV-012` | 主动释放后他人可立即占用 | `ocs_collab_server::ws::tests::release_broadcasts_claim_released_to_the_room`<br>`ocs_collab::occupancy::tests::release_frees_the_tag_for_the_next_claimant` | — | 有 |  |
| `VAL-SRV-013` | 占用过期自动释放并广播 | `ocs_collab_server::ws::tests::an_expired_claim_broadcasts_claim_released`<br>`ocs_collab::occupancy::tests::ttl_expiry_releases_the_tag_and_reports_it` | — | 有 | 时刻是注入的；真实进程上等占用到期见 `VAL-CLI-006`（`t22_ttl.py`） |
| `VAL-SRV-014` | 提交语义操作生效并回执 op_applied | `ocs_collab_server::ws::tests::ops_advance_the_revision_monotonically`<br>`ocs_collab_server::store::tests::commit_op_advances_the_revision_and_writes_one_audit_row` | — | 有 |  |
| `VAL-SRV-015` | 同一操作的 revision 单调递增 | `ocs_collab_server::ws::tests::ops_advance_the_revision_monotonically` | — | 有 | 测试里连续提交 2 次（契约写 3 次） |
| `VAL-SRV-016` | 变更广播 fanout 到同图全部会话 | `ocs_collab_server::bus::tests::a_message_reaches_every_subscriber_of_the_drawing`<br>`ocs_collab_server::bus::tests::an_async_subscriber_receives_the_published_message` | — | 有 | 真实进程上两端都收到广播见 `VAL-CLI-009`（`t10_broadcast.py`） |
| `VAL-SRV-017` | 变更不广播到其他图纸的会话 | `ocs_collab_server::bus::tests::a_message_does_not_cross_drawings` | — | 有 |  |
| `VAL-SRV-018` | 在线用户变化广播 peer_joined 与 peer_left | `ocs_collab_server::ws::tests::hello_opens_a_session_and_returns_welcome`<br>`ocs_collab_server::ws::tests::an_explicit_close_retires_the_session_and_tells_the_room_once`<br>`ocs_collab_server::ws::tests::own_peer_events_are_not_forwarded`<br>`ocs_collab::protocol::tests::peer_joined_carries_the_peer_inline` | — | 有 |  |
| `VAL-SRV-019` | welcome 中的 peers 与 claims 反映当前在线与占用状态 | `ocs_collab_server::ws::tests::welcome_lists_active_peers_and_outstanding_claims`<br>`ocs_collab_server::ws::tests::welcome_payload_contains_peers_and_claims`<br>`ocs_collab::occupancy::tests::claims_are_filtered_by_drawing_for_welcome` | — | 有 |  |
| `VAL-SRV-020` | 审计日志写入完整 9 字段 | `ocs_collab_server::ws::tests::an_op_writes_a_complete_audit_row`<br>`ocs_collab_server::store::tests::audit_rows_round_trip_with_all_nine_fields` | — | 有 |  |
| `VAL-SRV-021` | 审计日志不可修改与不可删除 | `ocs_collab_server::store::tests::audit_log_rejects_update_and_delete_at_the_database_level`<br>`ocs_collab_server::http::tests::audit_mutation_returns_405` | — | 有 |  |
| `VAL-SRV-022` | 审计按位号检索返回最近一次修改 | `ocs_collab_server::store::tests::audit_by_tag_returns_the_newest_first`<br>`ocs_collab_server::http::tests::audit_tag_filters_the_same_as_object_ref` | — | 有 |  |
| `VAL-SRV-023` | SQLite 落盘且启用 WAL 模式 | `ocs_collab_server::store::tests::opening_a_file_database_enables_wal`<br>`ocs_collab_server::store::tests::a_write_produces_the_wal_and_shm_sidecars`<br>`ocs_collab_server[main.rs]::tests::a_writable_path_yields_a_wal_backed_store` | — | 有 |  |
| `VAL-SRV-024` | 服务器重启后审计与图纸登记恢复 | `ocs_collab_server::store::tests::clearing_ephemeral_drops_sessions_and_claims_but_keeps_drawings`<br>`ocs_collab_server::ops::tests::the_drawing_is_rebuilt_from_the_op_log` | `s01_restart.py` 7 项 | 有 | 场景强行结束服务器进程，用同一条命令行、同一个库再起，逐项比对重启前后的 `/audit` 响应（含 `seq`、`change_text`）、图纸修订、操作日志和服务器重建出的图面 |
| `VAL-SRV-025` | 服务器重启后旧会话不可继续使用 | `ocs_collab_server::store::tests::clearing_ephemeral_drops_sessions_and_claims_but_keeps_drawings`<br>`ocs_collab::session::tests::heartbeat_on_an_unknown_session_reports_unknown`<br>`ocs_collab_server::ws::tests::a_message_before_hello_is_refused`<br>`ocs_collab_server::ws::tests::a_foreign_session_id_is_rejected` | `s01_restart.py` 6 项 | 有 | 场景在重启后拿旧会话号发心跳，两种情形回的都是带 `code` 的 `error`：连接还没握手时是 `HELLO_REQUIRED`，握手之后是 `SESSION_UNKNOWN`。另查了库里旧的会话行和占用行已清、旧占用的位号可以重新占用 |
| `VAL-SRV-026` | 端口由参数决定且默认不与其他服务冲突 | `ocs_collab_server[main.rs]::tests::port_and_db_are_taken_from_the_command_line`<br>`ocs_collab_server[main.rs]::tests::defaults_match_the_architecture_numbers`<br>`ocs_collab_server[main.rs]::tests::the_address_reported_at_startup_is_the_one_bound` | `s02_port_in_use.py` 5 项 | 有 | 场景在已被占用的端口上再起一个实例：进程以非零码退出，stderr 报「无法监听」和地址，没有改到别的端口上监听 |
| `VAL-SRV-027` | 断连不立即销毁会话 | — | `s03_disconnect.py` 7 项 | 有 | 场景直接销毁套接字（不发 close 帧），等过三轮清扫后同一用户重新握手：`welcome.claims` 里原占用还在、持有者是原来的会话，新会话去占被拒；另有发 close 帧退出的对照。原会话要到心跳超时才失效，场景不等那一段 |
| `VAL-SRV-028` | 端口被占用导致的启动失败不得破坏既有会话与占用数据 | — | `s02_port_in_use.py` 6 项 | 有 | 场景里第二个实例用同一端口、同一个库启动失败后，`sessions`、`claims` 两张表逐行不变，第二个实例没有打开过数据库，随后的 `POST /session` 不把原会话当成失效的 |
| `VAL-SRV-029` | 会话接管通知只送达被接管的会话 | `ocs_collab_server::ws::tests::takeover_error_does_not_reach_a_third_peer` | — | 有 |  |
| `VAL-SRV-030` | 并发提交的 revision 分配唯一且不丢帧 | `ocs_collab_server::store::tests::commit_op_serializes_concurrent_callers`<br>`ocs_collab_server::ops::tests::an_applied_op_is_announced_before_the_drawing_is_unlocked` | `s04_concurrent_ops.py` 12 项 | 有 | 场景按契约的下限跑：8 个会话，两段各 30 条（一段抢同一修订，一段各写各的）。规模再大服务器会丢广播，见第 5.3 节 |

### 4.2 验证基础设施 `VAL-INFRA`（14 条）

这一区一半是产品行为（GUI 的 REST 接口），一半是验收规程（端口段、超时、重试、产物放哪里）。规程类的几条没有可断言的产品行为，判的是「脚手架和入口有没有照着做」：规则写在脚手架和入口里（第 3.2 节的规程表），`t02`、`t03`、`t04` 逐条去试，`r01` 在一轮跑完后核对整轮的记录。

| 编号 | 契约标题 | 单元测试 | 端到端脚本 | 覆盖 | 说明 |
|---|---|---|---|---|---|
| `VAL-INFRA-001` | GUI 实例带真实图纸启动后 ready 可用且给出 document_id | `rest::tests::ready_refuses_the_start_tab_until_a_drawing_is_active`<br>`app::automation::tests::ready_line_carries_the_session_id` | `t00_smoke.py` 3 项 | 有 | 冒烟场景带一张真实图纸在端口 3210 起 GUI，轮询 `/api/v1/ready`：从启动进程算起 60 秒内回 200；逐个核对五个字段的值和类型（`ok`、`ready` 为真，`version`、`session_id` 是非空字符串，`document_id` 是整数，并且就是命令行上那张图所在的文档）；命令行、轮询次数、首次成功的时刻、完整的响应都写在检查项的说明里。其余场景等就绪的上限仍是 180 秒（2026-11-17 的裁决，第 5.3 节） |
| `VAL-INFRA-002` | REST 控制面拒绝无 token 请求 | `rest::tests::listener_answers_while_one_connection_stalls` | — | 有 | 这个测试起真实监听端口，其中断言了不带令牌、令牌不对都回 401 |
| `VAL-INFRA-003` | documents 端点返回当前文档清单 | `rest::tests::documents_carries_document_id_alongside_id`<br>`rest::tests::rest_p1_document_surface_routes_end_to_end` | — | 有 |  |
| `VAL-INFRA-004` | 用 run 加显式 document_id 能执行命令并使实体数增加 | `rest::tests::headless_session_runs_commands_after_start_tab_conversion`<br>`rest::tests::rest_surface_round_trips_real_entities`<br>`rest::tests::run_route_keeps_the_op_the_body_names` | — | 有 |  |
| `VAL-INFRA-005` | `POST /api/v1/commands` 带 document_id 时命令真正执行 | `app::control::http_bridge::tests::bridge_commands_with_a_document_id_draw_on_that_document`<br>`rest::tests::commands_forward_document_id`<br>`rest::tests::commands_run_forwards_document_id` | — | 有 | 第一条经界面托管的 REST 接口发一条画圆命令，断言响应 `ok`、实体数加一；后两条断言 `document_id` 被转进执行请求 |
| `VAL-INFRA-006` | 无头实例也能执行 run（缺陷 2 已修） | `rest::tests::headless_session_runs_commands_after_start_tab_conversion`<br>`app::automation::tests::envelope_mutations_work_headless_without_a_session_id` | — | 有 |  |
| `VAL-INFRA-007` | 实例意外消失时重试一次仍能完成测试 | — | `t03_vanish_retry.py` 4 项<br>`t02_harness_rules.py` 3 项<br>`t04_entry_rerun.py` 4 项<br>`r01_entry_rules.py` 1 项 | 有 | 分三层。脚手架：就绪过的实例进程退出了、或者请求被拒绝后连探 3 次都被拒绝，就记下现象（退出码、日志尾部、时刻）并让场景以退出码 75 收场（`t02`，连同三种不算消失的情形）。入口：见到 75 就把整个场景重跑一次、只重跑一次，第一次的控制台和结果另存（`t04` 用替身场景试；`r01` 在一轮跑完后核对每个场景各跑了几次）。`t03` 是真实的一遍：建实体的流程做到一半，实例被从外面结束；入口重跑，第二次在同一个端口上、用同一张图把流程走完，实体数加一。与契约字面的出入见第 5.3 节 |
| `VAL-INFRA-008` | GUI 端口限定在 3210 至 3239 区间 | — | `t00_smoke.py` 1 项<br>`t02_harness_rules.py` 4 项<br>`r01_entry_rules.py` 1 项 | 有 | 规则在脚手架里：端口段之外的端口直接拒绝；启动前先查要的端口有没有人在监听，有（别的进程在监听，或同一个场景里先起的实例占着）就在段内另挑一个；每次启动的命令行、要的端口、实际端口、查端口的结果记进 `gui-starts.jsonl`（`t02` 逐条试，`t00` 查自己那一次）。`r01` 在一轮跑完后核对这一轮的每一次启动：端口都在 3210–3239 之内（平移时按平移后的段算），启动前都查过。查端口的办法与契约写的不同，见第 5.3 节 |
| `VAL-INFRA-009` | capture 返回可用的 PNG 文件 | `app::control::tests::capture_parks_for_the_next_frame_then_finishes_with_its_result`<br>`rest::tests::capture_hoists_its_envelope_fields_to_the_top_level` | `t00_smoke.py` 2 项（标签 `harness`） | 部分 | `t00_smoke.py` 在真实进程上断言截图请求成功、文件存在；PNG 文件头与宽高为正整数没有断言 |
| `VAL-INFRA-010` | capture 返回世界到屏幕的坐标映射与实体屏幕坐标 | `app::control::vision::tests::the_capture_annotation_covers_where_a_reference_draws`<br>`app::control::vision::tests::test_compute_grounding_identifies_visible_entities` | — | 部分 | 标注框怎么算有单测；「用矩阵换算出的像素与标注坐标相差 ≤ 20 像素」要真实截图，没有自动化 |
| `VAL-INFRA-011` | state 端点给出 revision 与当前 document_id | `app::control::tests::control_retry_is_idempotent_and_stale_state_rejected`<br>`app::control::tests::geometry_revision_moves_only_when_the_drawing_changes` | — | 有 | 单测是间接断言：改图之后拿旧的 `revision` 再请求会被拒（说明版本号动了） |
| `VAL-INFRA-012` | GUI 与协同服务器可同时监听互不冲突 | — | `t20_session.py` 1 项 | 有 | `t20_session.py` 开头：GUI（3210）和服务器（3240）同时在跑，GUI 的 `/api/v1/ready` 和服务器的 `/health` 都回 200，并用 `Get-NetTCPConnection` 查到两个端口的监听方正是这两个进程。其余带服务器的场景开头也同时等两边就绪，不另带本条编号 |
| `VAL-INFRA-013` | 测试产物一律写入 scratch 目录 | — | `r01_entry_rules.py` 1 项 | 有 | 总入口开跑前记一次 `git status --porcelain` 和脚本旁边的字节码缓存，收尾时再看：`git status` 没有多出一行，缓存文件没有多、也没有被重写（被忽略的文件 `git status` 看不见，所以另比）；这一轮写下的文件清单和 `git status` 的全文留在证据目录里。证据目录不给、或给在仓库里面，入口直接拒绝执行。契约写的固定路径前缀换成了「运行时指定的、仓库之外的目录」，见第 5.3 节 |
| `VAL-INFRA-014` | 实例启动超时预算内未就绪即判定不可用 | — | `t00_smoke.py` 1 项<br>`t02_harness_rules.py` 4 项 | 有 | 60 秒的上限只在冒烟场景里断言（2026-11-17 的裁决）：从启动进程到 `/api/v1/ready` 首次回 200 不超过 60 秒。「到上限没有就绪就结束进程、留下日志尾部、换一个端口重试一次、不试第三次」写在脚手架里，对所有场景生效（其余场景的上限是 180 秒）；`t02` 走同一段代码、把上限给成 6 秒去试：图纸不存在的实例一直不就绪，被结束、换端口重启、记进 `gui-retries.jsonl`，第二次仍不就绪就到此为止。冒烟场景里第一次到上限没有就绪、照规则重试后 60 秒内就绪的，记通过并把重试写出来 |

### 4.3 权限与审计 `VAL-PERM`（32 条）

契约的取证工具是 curl。由 `ocs_collab` 的权限矩阵测试和服务器的 HTTP、存储测试覆盖。

| 编号 | 契约标题 | 单元测试 | 端到端脚本 | 覆盖 | 说明 |
|---|---|---|---|---|---|
| `VAL-PERM-001` | 系统管理员行的 8 个操作全部允许 | `ocs_collab::permission::tests::role_matrix_all_40_cells_are_decided`<br>`ocs_collab_server::perm::tests::the_role_matrix_is_evaluated_for_all_40_cells` | — | 有 |  |
| `VAL-PERM-002` | 项目管理员行仅「主数据维护」拒绝 | `ocs_collab::permission::tests::role_matrix_all_40_cells_are_decided`<br>`ocs_collab_server::perm::tests::the_role_matrix_is_evaluated_for_all_40_cells` | — | 有 |  |
| `VAL-PERM-003` | 工艺设计人员行 4 允许 4 拒绝 | `ocs_collab::permission::tests::role_matrix_all_40_cells_are_decided`<br>`ocs_collab_server::perm::tests::the_role_matrix_is_evaluated_for_all_40_cells` | — | 有 |  |
| `VAL-PERM-004` | 校对人员行 3 允许 5 拒绝 | `ocs_collab::permission::tests::role_matrix_all_40_cells_are_decided`<br>`ocs_collab_server::perm::tests::the_role_matrix_is_evaluated_for_all_40_cells` | — | 有 |  |
| `VAL-PERM-005` | 只读查看者行 2 允许 6 拒绝 | `ocs_collab::permission::tests::role_matrix_all_40_cells_are_decided`<br>`ocs_collab_server::perm::tests::the_role_matrix_is_evaluated_for_all_40_cells` | — | 有 |  |
| `VAL-PERM-006` | 40 格全部有确定的允许/拒绝结果，无未定义格、无 5xx | `ocs_collab_server::perm::tests::the_role_matrix_is_evaluated_for_all_40_cells`<br>`ocs_collab::permission::tests::role_matrix_all_40_cells_are_decided` | — | 有 |  |
| `VAL-PERM-007` | 校对人员编辑图纸被角色级拒绝并返回稳定错误码 | `ocs_collab::permission::tests::proofreader_cannot_edit_drawing`<br>`ocs_collab_server::perm::tests::the_role_matrix_is_evaluated_for_all_40_cells` | — | 有 |  |
| `VAL-PERM-008` | 矩阵封闭性——未知角色与未知操作返回 400 而非 500 | `ocs_collab_server::perm::tests::an_unknown_user_is_an_unknown_subject`<br>`ocs_collab_server::perm::tests::an_unknown_operation_is_rejected_before_the_chain`<br>`ocs_collab_server::perm::tests::each_perm_error_carries_its_contract_status_and_code`<br>`ocs_collab_server::http::tests::permission_check_maps_errors_to_their_statuses` | — | 有 |  |
| `VAL-PERM-009` | 目标节点有显式 ACL 时直接生效且不标记继承 | `ocs_collab_server::perm::tests::an_acl_on_the_target_node_is_the_effective_node`<br>`ocs_collab::permission::tests::acl_on_the_target_node_is_not_marked_inherited` | — | 有 |  |
| `VAL-PERM-010` | 目标节点无 ACL 时沿树向上取最近祖先 | `ocs_collab::permission::tests::acl_inherits_nearest_ancestor_not_root`<br>`ocs_collab_server::perm::tests::an_inherited_acl_reports_the_ancestor_and_a_root_first_path` | — | 有 |  |
| `VAL-PERM-011` | 多级祖先均命中时取最近的一条，不是最顶层 | `ocs_collab::permission::tests::acl_inherits_nearest_ancestor_not_root` | — | 有 |  |
| `VAL-PERM-012` | 整条路径（含根）均无配置时该级拒绝 | `ocs_collab_server::perm::tests::a_path_without_any_acl_row_denies_at_pbs`<br>`ocs_collab::permission::tests::acl_path_without_any_config_denies` | — | 有 |  |
| `VAL-PERM-013` | subject 为角色时按角色匹配 ACL | `ocs_collab::permission::tests::role_level_acl_applies_when_no_user_row_exists` | — | 有 |  |
| `VAL-PERM-014` | subject 为具体用户时按用户匹配 ACL | `ocs_collab_server::perm::tests::a_user_allow_row_wins_over_a_role_deny_row_on_the_same_node`<br>`ocs_collab::permission::tests::user_level_acl_wins_over_role_level_on_the_same_node` | — | 有 |  |
| `VAL-PERM-015` | 响应返回生效节点路径与继承标记 | `ocs_collab::permission::tests::acl_path_is_complete_when_the_target_node_decides`<br>`ocs_collab::permission::tests::acl_inherits_nearest_ancestor_not_root`<br>`ocs_collab_server::perm::tests::an_inherited_acl_reports_the_ancestor_and_a_root_first_path` | — | 有 |  |
| `VAL-PERM-016` | 只读状态下全部写操作被状态级拒绝 | `ocs_collab_server::perm::tests::a_readonly_drawing_denies_the_five_write_operations_at_state`<br>`ocs_collab::permission::tests::readonly_state_denies_all_five_write_operations` | — | 有 |  |
| `VAL-PERM-017` | 单人编辑状态对第二个编辑会话拒绝或降级 | `ocs_collab_server::http::tests::a_session_created_over_http_blocks_a_second_editor`<br>`ocs_collab_server::http::tests::permission_check_denies_a_second_editor_on_a_solo_drawing`<br>`ocs_collab_server::perm::tests::single_editor_denies_a_second_editors_write_with_the_holder` | — | 有 |  |
| `VAL-PERM-018` | 协同编辑状态允许多会话共享基线并放行写操作 | `ocs_collab_server::http::tests::a_coediting_drawing_takes_a_second_editing_session_and_both_may_edit`<br>`ocs_collab::session::tests::collaborative_mode_allows_two_live_sessions_on_one_drawing`<br>`ocs_collab_server::ws::tests::an_applied_op_moves_every_live_session_on_the_drawing_to_its_revision`<br>`ocs_collab_server::perm::tests::the_role_matrix_is_evaluated_for_all_40_cells` | — | 有 | 第一条在一个测试里走完：图到第 3 版后两人各建一个会话，基线都是 3，各做一次「编辑图纸」的权限检查都放行 |
| `VAL-PERM-019` | 状态迁移到只读后，原放行的编辑操作转为状态级拒绝 | `ocs_collab_server::perm::tests::flipping_a_drawing_to_readonly_flips_the_same_request_to_a_state_denial`<br>`ocs_collab_server::http::tests::state_change_persists_a_legal_value` | — | 有 |  |
| `VAL-PERM-020` | 非法状态值与非法迁移被拒并返回稳定错误码 | `ocs_collab_server::http::tests::state_change_requires_configure_authorization_permission`<br>`ocs_collab_server::http::tests::state_change_with_an_illegal_value_is_rejected_without_persisting` | — | 有 |  |
| `VAL-PERM-021` | 三级全拒绝时返回第一级（role）的拒绝原因 | `ocs_collab_server::perm::tests::a_role_denial_short_circuits_even_when_state_would_deny_too`<br>`ocs_collab::permission::tests::role_denial_short_circuits_before_the_acl_tier` | — | 有 |  |
| `VAL-PERM-022` | role 允许但 pbs 拒绝时返回 pbs 级原因 | `ocs_collab_server::perm::tests::a_path_without_any_acl_row_denies_at_pbs`<br>`ocs_collab::permission::tests::acl_denial_short_circuits_before_the_state_tier` | — | 有 |  |
| `VAL-PERM-023` | role 与 pbs 均允许但 state 拒绝时返回 state 级原因 | `ocs_collab_server::perm::tests::role_and_acl_allow_then_the_state_tier_denies` | — | 有 |  |
| `VAL-PERM-024` | 三级均允许时放行且不带拒绝级别 | `ocs_collab_server::http::tests::permission_check_allows_a_permitted_write`<br>`ocs_collab_server::perm::tests::the_role_matrix_is_evaluated_for_all_40_cells` | — | 有 |  |
| `VAL-PERM-025` | 审计记录 9 个字段全部非空且语义正确 | `ocs_collab_server::http::tests::an_edit_leaves_an_audit_row_with_all_nine_fields_stated`<br>`ocs_collab_server::ws::tests::an_op_writes_a_complete_audit_row`<br>`ocs_collab_server::store::tests::audit_rows_round_trip_with_all_nine_fields` | — | 有 | 第一条在用户表里播种了公司，经 `GET /audit` 读回后逐字段断言，`company` 等于用户表里的值 |
| `VAL-PERM-026` | 对 /audit 的 PUT / PATCH / DELETE 均返回 405 | `ocs_collab_server::http::tests::audit_mutation_returns_405`<br>`ocs_collab_server::store::tests::audit_log_rejects_update_and_delete_at_the_database_level` | — | 有 |  |
| `VAL-PERM-027` | 按位号检索返回该位号最近一次修改（at DESC 排序） | `ocs_collab_server::store::tests::audit_by_tag_returns_the_newest_first`<br>`ocs_collab_server::http::tests::audit_tag_filters_the_same_as_object_ref` | — | 有 |  |
| `VAL-PERM-028` | 按用户检索只返回该用户的记录 | `ocs_collab_server::http::tests::audit_user_id_answers_with_that_users_rows_and_no_others` | — | 有 | `u-check` 是校对人员，不能编辑：他的两条是「写入被拒」的记录（`write_denied`），不是成功的修改。按用户检索各返回本人的两条，没有对方的 |
| `VAL-PERM-029` | 按时间区间检索使用闭区间边界 | `ocs_collab_server::store::tests::audit_from_and_to_are_inclusive` | — | 有 |  |
| `VAL-PERM-030` | change_text 含变更前后值 | `ocs_collab_server::http::tests::a_change_is_recorded_with_the_old_value_the_new_value_and_the_field`<br>`ocs_collab_server::ws::tests::an_op_writes_a_complete_audit_row` | — | 有 | 第一条测的是有旧值的情形：先写 DN50 再改 DN80，记录里旧值、新值、字段名都在 |
| `VAL-PERM-031` | 检索无匹配返回 200 与空数组，不是 404 或 500 | `ocs_collab_server::http::tests::an_audit_query_nothing_matches_answers_200_with_an_empty_list` | — | 有 |  |
| `VAL-PERM-032` | 审计的 ip / workspace / company 取自服务端会话上下文，不可由客户端自报覆盖 | `ocs_collab_server::http::tests::an_audit_row_takes_nothing_from_keys_the_client_slipped_in`<br>`ocs_collab_server::ws::tests::an_op_writes_a_complete_audit_row` | — | 有 | 帧里多出来的 `ip`、`workspace`、`company` 键都不读，第一条有断言。`workspace` 另是建会话时的正式字段，写在那里的值会被采纳，与契约字面有出入，见 5.3 |

### 4.4 语义层 `VAL-SEM`（39 条）

契约的取证工具就是单元测试（36 条）和 DWG 往返（3 条），与测试一一对应。

| 编号 | 契约标题 | 单元测试 | 端到端脚本 | 覆盖 | 说明 |
|---|---|---|---|---|---|
| `VAL-SEM-001` | 增量 1 的位号序列 | `pid::tag_engine::tests::tag_rule_increment_one_sequence` | — | 有 |  |
| `VAL-SEM-002` | 增量 5 的位号序列 | `pid::tag_engine::tests::tag_rule_increment_five_sequence` | — | 有 |  |
| `VAL-SEM-003` | 起始编码与增量独立生效 | `pid::tag_engine::tests::tag_rule_start_and_increment_are_independent` | — | 有 |  |
| `VAL-SEM-004` | 拼接字段取当前实例属性值 | `pid::tag_engine::tests::tag_rule_concat_field_uses_instance_property` | — | 有 |  |
| `VAL-SEM-005` | 拼接字段源属性缺失时拒绝生成且不占用序号 | `pid::tag_engine::tests::tag_rule_missing_concat_field_rejects_without_consuming_sequence` | — | 有 |  |
| `VAL-SEM-006` | 后缀递增生成管路位号 | `pid::tag_engine::tests::tag_rule_suffix_increment_pipe_run_sequence` | — | 有 |  |
| `VAL-SEM-007` | 后缀递增关闭时管路位号重复被检出 | `pid::tag_engine::tests::tag_rule_suffix_disabled_marks_duplicate` | — | 有 |  |
| `VAL-SEM-008` | 层级选择「当前类型依次递增」 | `pid::tag_engine::tests::tag_rule_hierarchy_self_increment` | — | 有 |  |
| `VAL-SEM-009` | 层级选择「父类型递增」 | `pid::tag_engine::tests::tag_rule_hierarchy_parent_increment` | — | 有 |  |
| `VAL-SEM-010` | 唯一校验拒绝同类型同位号 | `pid::tag_engine::tests::tag_unique_rejects_same_type_same_tag` | — | 有 |  |
| `VAL-SEM-011` | 唯一校验允许不同类型同位号 | `pid::tag_engine::tests::tag_unique_allows_cross_type_same_tag` | — | 有 |  |
| `VAL-SEM-012` | 手工位号不被规则引擎覆盖 | `pid::tag_engine::tests::tag_rule_never_overwrites_manual_tag` | — | 有 |  |
| `VAL-SEM-013` | 自动编码关闭时不生成位号 | `pid::tag_engine::tests::tag_rule_auto_code_disabled_leaves_tag_empty` | — | 有 |  |
| `VAL-SEM-014` | 2000 实例位号生成性能 | `pid::perf_budget::tests::tag_rule_bulk_2000_under_three_seconds`（默认不跑） | — | 部分 | 性能预算测试，标了 `#[ignore]`，平时的 `cargo test --lib` 不跑；要加 `-- --ignored` 单独跑 |
| `VAL-SEM-015` | 类型树父子关系 | `pid::type_lib::tests::type_lib_tree_ancestry_and_children` | — | 有 |  |
| `VAL-SEM-016` | 属性分组与组内顺序 | `pid::type_lib::tests::type_lib_property_groups_preserve_order` | — | 有 |  |
| `VAL-SEM-017` | 数据要素（无 UI，纯模型） | `pid::type_lib::tests::type_lib_data_element_roundtrip` | — | 有 |  |
| `VAL-SEM-018` | 子类型未覆盖时上溯继承父类型规则 | `pid::type_lib::tests::type_lib_child_inherits_unoverridden_rules` | — | 有 |  |
| `VAL-SEM-019` | 子类型显式覆盖时优先且不影响兄弟 | `pid::type_lib::tests::type_lib_child_override_wins_and_is_scoped` | — | 有 |  |
| `VAL-SEM-020` | 九键写入读回一致 | `pid::persist::tests::pid_author_xdata_roundtrip_all_keys` | — | 有 |  |
| `VAL-SEM-021` | 超过 2 KB 时转存 XRecord 并留引用键 | `pid::persist::tests::pid_author_xdata_overflow_moves_props_to_xrecord` | — | 有 |  |
| `VAL-SEM-022` | 坐标字段移动实体后不变（apply_transform 回归） | `pid::persist::tests::pid_author_coordinates_survive_entity_transform` | — | 有 |  |
| `VAL-SEM-023` | `PID_AUTHOR` 不被识别器 purge 清除 | `pid::persist::tests::pid_author_record_survives_legend_purge` | — | 有 |  |
| `VAL-SEM-024` | 同一 APPID 只保留一条记录 | `pid::persist::tests::pid_author_single_record_per_entity` | — | 有 |  |
| `VAL-SEM-025` | APPID 登记与 DWG 往返后仍可解析 | `pid::persist::tests::pid_author_app_id_registered_and_roundtrips` | — | 有 |  |
| `VAL-SEM-026` | 四个 key 各自写入读回 | `pid::graph_data::tests::pid_author_xrecord_four_keys_roundtrip` | — | 有 |  |
| `VAL-SEM-027` | 整 key 重写不残留旧内容 | `pid::graph_data::tests::pid_author_xrecord_rewrite_drops_stale_entries` | — | 有 |  |
| `VAL-SEM-028` | 四个 key 互不覆盖 | `pid::graph_data::tests::pid_author_xrecord_keys_are_independent` | — | 有 |  |
| `VAL-SEM-029` | 数据清空时删除该 key 的 XRecord | `pid::graph_data::tests::pid_author_xrecord_removed_when_empty` | — | 有 |  |
| `VAL-SEM-030` | 加载 JSON 后每个符号生成块定义 | `pid::symbols::tests::pid_symbols_load_generates_block_definitions` | — | 有 |  |
| `VAL-SEM-031` | 端口点从符号定义推导 | `pid::symbols::tests::pid_symbol_ports_derive_from_definition` | — | 有 |  |
| `VAL-SEM-032` | 符号与类型多对多 | `pid::symbols::tests::pid_symbol_type_many_to_many` | — | 有 |  |
| `VAL-SEM-033` | 图例标准项目级唯一 | `pid::symbols::tests::pid_legend_standard_locked_after_first_instance` | — | 有 |  |
| `VAL-SEM-034` | DWG 2018 往返位号与属性零丢失 | `pid::dwg_roundtrip::tests::pid_author_dwg2018_roundtrip_zero_loss` | — | 有 |  |
| `VAL-SEM-035` | 七个 DWG 版本往返丢失数为 0 | `pid::dwg_roundtrip::tests::pid_author_dwg_all_versions_zero_loss` | — | 有 |  |
| `VAL-SEM-036` | 管线改值下传到未覆盖的管路 | `pid::propagate::tests::pipe_line_property_propagates_to_runs` | — | 有 |  |
| `VAL-SEM-037` | 覆盖标记阻止后续下传，管段始终取管路当前值 | `pid::propagate::tests::run_override_blocks_inheritance_and_segment_follows_run` | — | 有 |  |
| `VAL-SEM-038` | 设备改位号后管口前缀重算并阻断冲突保存 | `pid::propagate::tests::equipment_tag_change_recomputes_nozzles_and_blocks_save` | — | 有 |  |
| `VAL-SEM-039` | 冲突保存被真实保存路径阻断且不落新文件 | `pid::propagate::tests::conflicting_save_is_blocked_on_the_real_save_path`<br>`app::automation::pid_save_gate_tests::the_save_operation_surfaces_the_pid_gate_code`<br>`app::automation::pid_save_gate_tests::the_verified_save_surfaces_the_pid_gate_code` | — | 有 |  |

### 4.5 绘图工具 `VAL-DRAW`（31 条）

契约的取证工具是 GUI 的 REST 接口加截图。逻辑由主程序的库测试覆盖；判据落在截图和耗时上的标「部分」或「缺」。

| 编号 | 契约标题 | 单元测试 | 端到端脚本 | 覆盖 | 说明 |
|---|---|---|---|---|---|
| `VAL-DRAW-001` | 调色板面板可打开，且分类页签与符号库一致 | `ui::window::pid_palette::tests::tabs_come_from_the_symbol_library_categories`<br>`app::commands::pidpalette::tests::pidpalette_toggles_one_panel` | — | 部分 | 页签集合与面板开关有单测；「截图里出现面板、页签文字可见」没有自动化 |
| `VAL-DRAW-002` | 调色板面板可关闭，且重复打开不叠加 | `app::commands::pidpalette::tests::pidpalette_toggles_one_panel` | — | 部分 | 开关与不叠加有单测；截图判据没有自动化 |
| `VAL-DRAW-003` | 分类页签可切换，网格内容随页签变化 | `ui::window::pid_palette::tests::a_categorys_grid_holds_exactly_its_symbols`<br>`ui::window::pid_palette::tests::the_active_tab_is_clamped_to_the_library` | — | 部分 | 格数等于该类符号数有单测；两张截图的对比没有自动化 |
| `VAL-DRAW-004` | 网格每个符号都有可见图标，无空单元格 | `ui::window::pid_palette::tests::every_symbol_has_a_drawable_bounds`<br>`app::commands::pidpalette::tests::every_palette_symbol_resolves` | — | 部分 | 每个符号都有可画的外包框有单测；「格内有非背景色像素」没有自动化 |
| `VAL-DRAW-005` | 点击符号进入插入模式，命令行为 `PIDINSERT <符号>` | `app::commands::pidpalette::tests::a_cell_click_starts_pidinsert_and_the_tab_survives_a_document_switch`<br>`app::commands::pidpalette::tests::the_command_names_itself_and_asks_for_a_point` | — | 有 | 单测里「点击」是直接发面板消息，不是真实鼠标 |
| `VAL-DRAW-006` | 面板状态在文档切换时保持 | `app::commands::pidpalette::tests::a_cell_click_starts_pidinsert_and_the_tab_survives_a_document_switch` | — | 部分 | 页签跨文档保持有单测；截图判据没有自动化 |
| `VAL-DRAW-007` | `PIDINSERT` 插入设备符号后实体数 +1 | `app::commands::pidpalette::tests::pidinsert_commits_an_insert_carrying_its_author_record` | — | 有 |  |
| `VAL-DRAW-008` | 新实体是 `Insert` 且指向 `PID_SYM_<category>_<id>` 块 | `app::commands::pidpalette::tests::pidinsert_commits_an_insert_carrying_its_author_record`<br>`app::commands::pidpalette::tests::the_command_carries_the_block_and_its_type` | — | 有 |  |
| `VAL-DRAW-009` | 插入位置与喂入坐标一致（插入点读回 + 矩阵反算） | `app::commands::pidpalette::tests::pidinsert_commits_an_insert_carrying_its_author_record` | — | 部分 | 插入点读回（偏差 ≤ 0.5）有单测；截图矩阵反算那一半没有自动化 |
| `VAL-DRAW-010` | 插入后符号在截图里可见且能被 `highlight_handles` 圈出 | — | — | 缺 | 判据全在截图上（可见、高亮色、`visible_entities`），没有单测也没有脚本 |
| `VAL-DRAW-011` | 插入的实体带 XDATA `PID_AUTHOR` 的 `type` 与 `tag` | `app::commands::pidpalette::tests::pidinsert_commits_an_insert_carrying_its_author_record` | — | 有 |  |
| `VAL-DRAW-012` | `PIDINSERT` 未知符号名被拒绝且不建实体 | `app::commands::pidpalette::tests::pidinsert_refuses_an_unknown_symbol_with_a_code_and_no_entity`<br>`app::commands::pidpalette::tests::pidinsert_resolves_the_bare_id_and_refuses_an_unknown_one` | — | 有 |  |
| `VAL-DRAW-013` | 同一符号连续插入两次得到两个独立实体 | `app::commands::pidpalette::tests::two_consecutive_inserts_are_two_independent_entities` | — | 有 |  |
| `VAL-DRAW-014` | 预览跟随鼠标移动 | `app::commands::pidpalette::tests::the_preview_follows_the_cursor` | — | 部分 | 预览图元随光标变化有单测；截图里的像素位移没有自动化 |
| `VAL-DRAW-015` | `空格` 使预览旋转 90° | `app::commands::pidpalette::tests::space_rotates_the_preview_a_quarter_turn_about_its_centre`<br>`app::commands::pidpalette::tests::a_placement_commits_the_rotation_the_preview_showed` | — | 部分 | 预览旋转与落点角度有单测；截图对比没有自动化 |
| `VAL-DRAW-016` | `Ctrl+空格` 切换插入点 | `app::commands::pidpalette::tests::ctrl_space_switches_the_insertion_point` | — | 部分 | 插入基准点切换有单测；截图对比没有自动化 |
| `VAL-DRAW-017` | `Esc` 取消插入模式，实体数不变 | `app::commands::pidpalette::tests::escape_cancels_without_a_placement_and_a_later_insert_still_commits`<br>`rest::tests::run_cancel_escapes_a_live_command` | — | 有 |  |
| `VAL-DRAW-018` | 取消后再次插入仍能正常提交 | `app::commands::pidpalette::tests::escape_cancels_without_a_placement_and_a_later_insert_still_commits` | — | 有 |  |
| `VAL-DRAW-019` | 首个设备位号符合「前缀 + 序号」规则 | `app::commands::pidpalette::tests::consecutive_placements_increment_and_each_category_starts_at_its_own_origin` | — | 有 |  |
| `VAL-DRAW-020` | 连续插入同类型位号递增 | `app::commands::pidpalette::tests::consecutive_placements_increment_and_each_category_starts_at_its_own_origin` | — | 有 |  |
| `VAL-DRAW-021` | 位号唯一冲突时显示 `？` | `app::commands::pidpalette::tests::a_manual_duplicate_shows_the_conflict_mark_and_undoes`<br>`app::commands::pidprops::tests::a_conflicting_tag_edit_shows_the_conflict_mark_in_the_group` | — | 部分 | 状态、保留原位号、显示属性变 `？` 有单测；截图里的文字没有自动化 |
| `VAL-DRAW-022` | 位号在截图中可见（文字渲染） | `app::commands::pidpalette::tests::each_placement_shows_its_tag_above_the_symbol` | — | 部分 | 每个落点带一个显示位号的属性、位置在符号上方有单测；「截图里能辨认」没有自动化 |
| `VAL-DRAW-023` | 不同类型各自独立编号 | `app::commands::pidpalette::tests::consecutive_placements_increment_and_each_category_starts_at_its_own_origin`<br>`app::commands::pidpalette::tests::a_valve_placement_records_the_valve_category_code` | — | 有 |  |
| `VAL-DRAW-024` | 选中带 `PID_AUTHOR` 的实体后属性面板出现 `P&ID 属性` 分组 | `app::commands::pidprops::tests::the_group_shows_the_author_record_and_only_for_authored_entities` | — | 有 |  |
| `VAL-DRAW-025` | `P&ID 属性` 分组显示 `type`/`tag`/`tag_source`/`tag_status` 与 `props` 键值对 | `app::commands::pidprops::tests::the_group_shows_the_author_record_and_only_for_authored_entities` | — | 有 |  |
| `VAL-DRAW-026` | 编辑属性值后提交，XDATA 读回已更新 | `app::commands::pidprops::tests::a_props_edit_lands_in_xdata_and_leaves_the_other_keys` | — | 有 |  |
| `VAL-DRAW-027` | 编辑位号后 `tag_source` 变为 `manual` | `app::commands::pidprops::tests::a_tag_edit_is_manual_and_undoes_with_its_label` | — | 有 |  |
| `VAL-DRAW-028` | 属性面板编辑走 undo，可回滚到旧值 | `app::commands::pidprops::tests::a_tag_edit_is_manual_and_undoes_with_its_label` | — | 有 |  |
| `VAL-DRAW-030` | 5000 实体画布上符号插入 ≤ 100 ms（中位数） | `app::commands::pidpalette::tests::an_automated_insert_settles_each_step_without_an_async_task` | — | 部分 | 单测只保证机制前提（三步都同步结算）；5000 实体底图上 20 次插入的中位数 ≤ 100 ms 没有自动化的测量 |
| `VAL-DRAW-031` | 2000 实例位号生成 ≤ 3 秒 | `app::commands::pidbulk::tests::a_batch_of_two_thousand_tags_runs_to_p_3000` | — | 部分 | 单测按契约规模跑 2000 个并核对位号，耗时只打印不断言；≤ 3000 ms 没有自动化的断言 |
| `VAL-DRAW-032` | 插入性能不受既有 P&ID 实体数量线性拖垮 | `app::commands::pidpalette::tests::an_automated_insert_settles_each_step_without_an_async_task` | — | 部分 | 同 030：只有机制前提，没有「已有 200 个 P&ID 实体后中位数不劣化」的测量 |

### 4.6 管线与拓扑 `VAL-PIPE`（32 条）

契约的取证工具是 GUI 的 REST 接口。判据几乎都是坐标、拓扑表和属性，库测试能逐条断言到；只有一条判据全在截图上。

| 编号 | 契约标题 | 单元测试 | 端到端脚本 | 覆盖 | 说明 |
|---|---|---|---|---|---|
| `VAL-PIPE-001` | 创建管线产生独立位号 `L-1001` | `app::commands::pidpipe::tests::pidpipe_creates_a_pipe_line_tagged_l_1001`<br>`app::commands::pidpipe::tests::the_run_response_reports_how_many_entities_were_added` | — | 有 |  |
| `VAL-PIPE-002` | 管线首条管路位号是 `L-1001-01` | `app::commands::pidpipe::tests::a_run_on_the_line_is_tagged_l_1001_01` | — | 有 |  |
| `VAL-PIPE-003` | 同一管线第二条管路位号递增为 `L-1001-02`，且不复用已删编号 | `app::commands::pidpipe::tests::run_suffix_increments_and_deleted_numbers_are_not_reused` | — | 有 |  |
| `VAL-PIPE-004` | 不同管线的管路位号互不干扰 | `app::commands::pidpipe::tests::runs_of_different_lines_do_not_interfere` | — | 有 |  |
| `VAL-PIPE-005` | 管段无独立位号 | `app::commands::pidpipe::tests::a_segment_has_no_tag_of_its_own`<br>`pid::topology::tests::a_segment_record_carries_no_tag_field` | — | 有 |  |
| `VAL-PIPE-006` | 管件继承宿主管路位号，阀门另有独立位号 | `app::commands::pidpipe::tests::a_valve_has_its_own_tag_and_an_owner_pointing_at_the_run` | — | 有 |  |
| `VAL-PIPE-007` | 管路创建时全量继承管线五个字段 | `app::commands::pidpipe::tests::a_new_run_inherits_all_five_line_fields` | — | 有 |  |
| `VAL-PIPE-008` | 管路手工改字段后打覆盖标记 | `app::commands::pidpipe::tests::a_manual_run_edit_marks_the_field_as_overridden`<br>`app::commands::pidprops::tests::a_run_edit_from_the_property_panel_marks_the_override_and_cascades` | — | 有 |  |
| `VAL-PIPE-009` | 已打覆盖标记的字段不再被管线改值覆盖 | `app::commands::pidpipe::tests::an_overridden_field_stops_taking_the_line_value` | — | 有 |  |
| `VAL-PIPE-010` | 管段始终取管路当前值（管段无覆盖标记） | `app::commands::pidpipe::tests::a_segment_follows_its_run_and_carries_no_override_mark` | — | 有 |  |
| `VAL-PIPE-011` | 管段不直连管线——改管线只经管路下传 | `app::commands::pidpipe::tests::a_line_edit_never_reaches_a_segment_behind_an_overridden_run` | — | 有 |  |
| `VAL-PIPE-012` | 新建管路继承的是管线**当前**值，不是创建管线时的初值 | `app::commands::pidpipe::tests::a_late_run_inherits_the_lines_current_value` | — | 有 |  |
| `VAL-PIPE-013` | 管段上插入阀门后管段数从 1 变 2 | `app::commands::pidpipe::tests::a_fitting_at_the_midpoint_splits_the_segment_in_two` | — | 有 |  |
| `VAL-PIPE-014` | 阀门端口为「连接点1」「连接点2」 | `app::commands::pidpipe::tests::a_fitting_at_the_midpoint_splits_the_segment_in_two`<br>`app::commands::pidpipe::tests::a_valve_has_its_own_tag_and_an_owner_pointing_at_the_run` | — | 有 |  |
| `VAL-PIPE-015` | 两个管段各连阀门一个端口（连接关系写进拓扑表） | `app::commands::pidpipe::tests::a_fitting_at_the_midpoint_splits_the_segment_in_two` | — | 有 |  |
| `VAL-PIPE-016` | 断开后几何端点分列阀门两侧且外侧端点不动 | `app::commands::pidpipe::tests::a_fitting_at_the_midpoint_splits_the_segment_in_two` | — | 有 |  |
| `VAL-PIPE-017` | 截图可见断开——管线不再贯穿阀门 | — | — | 缺 | 判据全在截图上（两段的 `screen_bounds` 不贯穿阀门），没有单测也没有脚本；几何上的不重叠由 016 的单测保证 |
| `VAL-PIPE-018` | 在管段端点插阀门不拆分，阀门绑定该端点 | `app::commands::pidpipe::tests::a_fitting_on_a_segment_endpoint_does_not_split` | — | 有 |  |
| `VAL-PIPE-019` | 移动阀门后两侧管段端点跟随 | `app::commands::pidmove::tests::moving_the_valve_drags_the_inner_endpoints_onto_its_ports` | — | 有 |  |
| `VAL-PIPE-020` | 补位后仍首尾相接，无重叠无缺口 | `app::commands::pidmove::tests::the_refilled_segments_stay_joined_without_overlap_or_gap` | — | 有 |  |
| `VAL-PIPE-021` | 移动阀门不改变管段数与端口绑定 | `app::commands::pidmove::tests::moving_the_valve_keeps_the_segment_count_and_port_bindings` | — | 有 |  |
| `VAL-PIPE-022` | 反向移动与越界移动的稳定性 | `app::commands::pidmove::tests::a_reverse_move_keeps_the_topology_intact`<br>`app::commands::pidmove::tests::an_out_of_range_move_fails_without_partial_writes` | — | 有 |  |
| `VAL-PIPE-023` | 四种连接符各自可创建，且类型可区分 | `app::commands::pidconnector::tests::four_connector_kinds_are_created_and_distinguishable` | — | 有 |  |
| `VAL-PIPE-024` | 同一线号同图第二个流入连接符被拒，错误码 `DUPLICATE_INFLOW_CONNECTOR` | `app::commands::pidconnector::tests::a_second_inflow_connector_on_the_same_line_is_refused` | — | 有 |  |
| `VAL-PIPE-025` | 管道连接符选到信号线上被拒，错误码 `CONNECTOR_KIND_MISMATCH` | `app::commands::pidconnector::tests::a_connector_on_the_wrong_line_kind_is_refused` | — | 有 |  |
| `VAL-PIPE-026` | 连接符对端 XDATA `connector_peer` 格式为 `<drawing_id>:<handle>` | `app::commands::pidconnector::tests::pairing_writes_cross_drawing_peers_on_both_sides` | — | 有 |  |
| `VAL-PIPE-027` | 一对流入/流出连接符建立后属性可传递；信号线不含 `管道等级` | `app::commands::pidconnector::tests::pairing_transfers_pipe_properties_to_the_inflow_side`<br>`app::commands::pidconnector::tests::signal_connectors_transfer_instruments_and_never_a_pipe_grade` | — | 有 |  |
| `VAL-PIPE-028` | `PID_AUTHOR_TOPOLOGY` XRecord 承载拓扑表且结构完整 | `app::commands::pidpipe::tests::the_topology_lives_in_the_xrecord_and_not_on_the_segment_entity`<br>`pid::topology::tests::the_table_roundtrips_through_the_xrecord` | — | 有 |  |
| `VAL-PIPE-029` | 保存重开后拓扑表读回一致 | `pid::topology::tests::topology_and_entity_xdata_survive_a_dwg_roundtrip` | — | 有 |  |
| `VAL-PIPE-030` | 重开后实体 XDATA 语义零丢失 | `pid::topology::tests::topology_and_entity_xdata_survive_a_dwg_roundtrip` | — | 有 |  |
| `VAL-PIPE-031` | 打断符插入管段后管段被分段且拓扑表更新 | `app::commands::pidpipe::tests::pidbreak_splits_the_segment_into_two_non_overlapping_pieces` | — | 有 |  |
| `VAL-PIPE-032` | 打断符两侧不连通 | `app::commands::pidpipe::tests::pidbreak_leaves_both_inner_ends_disconnected` | — | 有 |  |

### 4.7 桌面端协同客户端 `VAL-CLI`（20 条）

脚本起真实的服务器和桌面端，从 REST 接口、服务器的库和两边的日志取证，计时类的判据（心跳周期、启动 5 秒、60 秒无连接、断开 3 秒内发现、退避递增、占用到期）都有断言。这一区有七条的契约原文与实现有字面出入，列在第 5.3 节。

| 编号 | 契约标题 | 单元测试 | 端到端脚本 | 覆盖 | 说明 |
|---|---|---|---|---|---|
| `VAL-CLI-001` | 客户端连服务器后收到 welcome 并拿到会话 | — | `t20_session.py` 5 项 | 有 | 2026-11-17 那一遍里，五项中数服务器日志行的那一项不通过：脚本的写法落后于后来的改动，见第 3.3 节 |
| `VAL-CLI-002` | 客户端发送 heartbeat 的周期为 15 秒 | `app::collab::transport::tests::the_session_emits_heartbeats_carrying_its_session_id`<br>`app::collab::transport::tests::the_heartbeat_cadence_lands_inside_the_contract_window` | `t20_session.py` 3 项 | 有 |  |
| `VAL-CLI-003` | 服务器在 90 秒无心跳后判会话失效并广播 peer_left | — | `t23_stale.py` 6 项 | 有 | 第二个观测端用的是普通 WebSocket 客户端（契约已登记的口径）。字面出入见第 5.3 节 |
| `VAL-CLI-004` | 占用申请获批后返回 claim_granted 并落库 | `app::collab::tests::a_granted_claim_publishes_this_sessions_user_id`<br>`app::collab::transport::tests::queued_claims_and_releases_reach_the_wire_with_the_session_id` | `t21_claims.py` 2 项 | 有 | 占用用的是命令 `PIDCLAIM`，面板上的按钮没有端到端点过 |
| `VAL-CLI-005` | 占用被拒时返回 claim_denied 并携带 holder 与 expires_at | `app::collab::tests::a_denied_claim_records_the_holder_and_never_claims_it_for_us` | `t21_claims.py` 4 项 | 有 | 字面出入见第 5.3 节 |
| `VAL-CLI-006` | 占用在 300 秒 TTL 到期后自动释放并广播 claim_released | `app::collab::tests::a_release_clears_the_tag_and_keeps_its_reason` | `t22_ttl.py` 4 项 | 有 | 真等默认的 300 秒到期，这个场景最慢 |
| `VAL-CLI-007` | 主动释放占用返回 claim_released 且 reason 为主动 | — | `t21_claims.py` 2 项 | 有 | 释放用的是命令 `PIDRELEASE`，面板上的按钮没有端到端点过 |
| `VAL-CLI-008` | 占用状态在 GUI 面板可见且可被 REST 读回 | `app::collab::claims::tests::a_claimed_tag_colours_the_entities_that_carry_it`<br>`app::collab::claims::tests::this_sessions_own_claim_is_coloured_as_occupied`<br>`app::collab::claims::tests::a_released_claim_row_stops_reporting_its_rectangle` | `t21_claims.py` 3 项 | 有 | 占用色像素在视口截图上量（契约已登记的口径） |
| `VAL-CLI-009` | 实例 A 的位号变更在实例 B 本地文档生效 | `app::collab::apply::tests::a_remote_tag_change_reaches_the_local_document`<br>`app::collab::broadcast::tests::a_set_tag_change_rewrites_the_local_tag` | `t10_broadcast.py` 6 项 | 有 | 脚本早于「改位号带真实来源」，对端读回的 `tag_source` 没有断言（见第 5.2 节）。字面出入见第 5.3 节 |
| `VAL-CLI-010` | 远端变更后双方 base_revision 同步刷新 | `app::collab::apply::tests::a_remote_op_moves_the_baseline_to_the_server_revision`<br>`ocs_collab_server::ws::tests::an_applied_op_moves_every_live_session_on_the_drawing_to_its_revision` | `t10_broadcast.py` 4 项 | 有 |  |
| `VAL-CLI-011` | readonly_downgrade 后 GUI 拒绝写操作 | `app::collab::apply::tests::a_readonly_session_refuses_writes_and_keeps_inspection`<br>`app::collab::apply::tests::commands_that_write_are_refused_and_the_forms_that_read_are_not`<br>`app::collab::apply::tests::every_command_a_readonly_session_may_run_leaves_the_drawing_as_it_was`<br>`app::collab::apply::tests::a_write_that_gets_past_the_command_gate_is_taken_back`<br>`ocs_collab_server::http::tests::switching_to_readonly_downgrades_everyone_but_the_holder` | `t10_broadcast.py` 5 项 | 有 | 脚本只查了契约给的那一条写命令（`PIDTAG`）；「写命令作为一类被拒」由单测盯着：两条盯放行名单，一条盯名单之外的总闸（漏过名单的写入会被收回）。字面出入见第 5.3 节 |
| `VAL-CLI-012` | 只读降级在 GUI 上可见 | `app::collab::claims::tests::a_downgrade_marks_the_holding_session_in_the_panel`<br>`ui::window::pid_collab::tests::a_downgraded_session_draws_the_badge_and_marks_the_holders_row` | `t10_broadcast.py` 4 项 | 有 |  |
| `VAL-CLI-013` | 在线用户列表随 peer_joined 增长 | `app::collab::tests::the_roster_is_the_others_and_the_head_count_includes_this_session`<br>`ocs_collab_server::ws::tests::hello_opens_a_session_and_returns_welcome` | `t10_broadcast.py` 7 项 | 有 | 字面出入见第 5.3 节 |
| `VAL-CLI-014` | 服务器重启后客户端自动重连且退避递增 | — | `t24_reconnect.py` 4 项 | 有 | 重连间隔是从客户端日志读的（服务器停着时没有服务器日志可读） |
| `VAL-CLI-015` | 服务器不可达时客户端进入离线态且不阻塞 UI | — | `t25_offline.py` 6 项 | 有 |  |
| `VAL-CLI-016` | 客户端优雅退出后服务器侧会话判失效且无线程泄漏 | `app::collab::transport::tests::quitting_sends_a_close_frame_and_joins_the_thread`<br>`ocs_collab_server::ws::tests::an_explicit_close_retires_the_session_and_tells_the_room_once` | `t20_session.py` 4 项 | 有 | 退出用的是命令 `QUIT`，窗口的关闭按钮没有端到端点过 |
| `VAL-CLI-017` | 客户端以 --collab 缺省时不建立任何连接 | `app::collab::collab_offline_queue::a_client_that_never_joined_queues_nothing`<br>`ui::window::pid_collab::tests::a_panel_with_no_server_says_so_and_how_to_set_one` | `t25_offline.py` 4 项 | 有 |  |
| `VAL-CLI-018` | 同一用户重复连入同一图纸产生两个独立会话 | — | `t11_same_user.py` 4 项 | 有 | 字面出入见第 5.3 节 |
| `VAL-CLI-019` | 客户端协同传输不引入新的异步运行时 | `app::collab::tests::nothing_under_this_module_is_asynchronous_code` | `r01_entry_rules.py` 1 项 | 有 | 协同目录下没有异步代码由单测钉住。依赖树由 `r01_entry_rules.py` 检查：`cargo tree --offline --locked -p OpenCADStudio --edges normal` 的输出里没有 `tokio`、`async-std`，有 `tungstenite`。「冷构建不超过 20 秒」这一句没有检查：按字面不成立，留到 CP-A 裁决（第 5.3 节） |
| `VAL-CLI-020` | 服务器强杀后客户端按错误分支重连而非误判正常关闭 | — | `t24_reconnect.py` 4 项 | 有 |  |

### 4.8 冲突与裁决 `VAL-CONF`（18 条）

脚本起真实的服务器，用 WebSocket 客户端和 HTTP 请求取证，不起 GUI。

| 编号 | 契约标题 | 单元测试 | 端到端脚本 | 覆盖 | 说明 |
|---|---|---|---|---|---|
| `VAL-CONF-001` | 不同字段并发修改自动合并且不产生冲突条目 | `ocs_collab::conflict::tests::a_field_nobody_else_wrote_since_the_baseline_merges`<br>`ocs_collab_server::ops::tests::a_write_behind_the_revision_to_a_field_nobody_else_wrote_merges`<br>`ocs_collab_server::ops::tests::inside_the_window_writes_to_different_fields_both_commit` | `c01_field_merge.py` 6 项 | 有 |  |
| `VAL-CONF-002` | 同字段并发修改按 base_revision 判定落后方让位且无冲突条目 | `ocs_collab::conflict::tests::a_field_someone_else_wrote_since_the_baseline_is_kept_and_the_change_yields`<br>`ocs_collab_server::ops::tests::a_write_behind_the_revision_to_a_field_someone_else_wrote_yields` | `c01_field_merge.py` 5 项 | 有 |  |
| `VAL-CONF-003` | 被覆盖的原值写入审计日志 | `ocs_collab_server::ops::tests::a_write_behind_the_revision_to_a_field_someone_else_wrote_yields` | `c01_field_merge.py` 4 项 | 有 |  |
| `VAL-CONF-004` | base_revision 相同且值不同时生成冲突条目 | `ocs_collab::conflict::tests::two_writes_collide_only_on_one_field_of_one_object_with_different_values`<br>`ocs_collab_server::ops::tests::two_writes_against_one_revision_inside_the_window_make_a_conflict_entry` | `c01_field_merge.py` 5 项 | 有 |  |
| `VAL-CONF-005` | 属性级落后方让位时不生成冲突条目 | `ocs_collab_server::ops::tests::a_write_two_revisions_behind_yields` | `c01_field_merge.py` 4 项 | 有 |  |
| `VAL-CONF-006` | 对象级冲突——一方删除另一方修改，删除生效 | `ocs_collab::conflict::tests::a_change_to_an_object_someone_else_deleted_since_the_baseline_does_not_happen`<br>`ocs_collab_server::ops::tests::a_change_to_an_object_deleted_meanwhile_does_not_happen` | `c02_object_topology.py` 4 项 | 有 |  |
| `VAL-CONF-007` | 对象级冲突不生成冲突条目而记入审计 | `ocs_collab_server::ops::tests::a_change_to_an_object_deleted_meanwhile_does_not_happen` | `c02_object_topology.py` 4 项 | 有 |  |
| `VAL-CONF-008` | 拓扑级冲突生成待裁决条目 | `ocs_collab::conflict::tests::changes_to_one_lines_connections_contend_for_the_line`<br>`ocs_collab_server::ops::tests::two_changes_to_one_lines_connections_inside_the_window_make_a_conflict_entry` | `c02_object_topology.py` 3 项 | 有 |  |
| `VAL-CONF-009` | 拓扑级冲突在裁决前不改变任何一方的拓扑 | — | `c02_object_topology.py` 2 项 | 有 |  |
| `VAL-CONF-010` | 管理员裁决 resolution=采用甲方 | `ocs_collab_server::http::tests::a_ruling_writes_the_value_it_picks_and_is_on_record` | `c03_resolution.py` 4 项 | 有 |  |
| `VAL-CONF-011` | 管理员裁决 resolution=采用乙方 | `ocs_collab_server::http::tests::a_ruling_writes_the_value_it_picks_and_is_on_record`<br>`ocs_collab_server::http::tests::a_ruling_on_a_topology_entry_applies_the_change_it_picks` | `c03_resolution.py` 3 项 | 有 |  |
| `VAL-CONF-012` | 管理员裁决 resolution=人工改值 | `ocs_collab_server::http::tests::a_ruling_writes_the_value_it_picks_and_is_on_record`<br>`ocs_collab_server::store::tests::resolving_with_a_manual_value_stores_it` | `c03_resolution.py` 3 项 | 有 |  |
| `VAL-CONF-013` | 裁决结果与双方原值写入审计 | `ocs_collab_server::http::tests::a_ruling_writes_the_value_it_picks_and_is_on_record` | `c03_resolution.py` 4 项 | 有 |  |
| `VAL-CONF-014` | 裁决后 status 变更且不可重复裁决 | `ocs_collab_server::http::tests::a_second_ruling_is_refused_and_changes_nothing`<br>`ocs_collab_server::store::tests::conflicts_are_created_pending_and_resolved_once` | `c03_resolution.py` 2 项 | 有 |  |
| `VAL-CONF-015` | 非管理员角色裁决被拒绝 | `ocs_collab_server::http::tests::a_ruling_by_someone_who_may_not_configure_is_refused_at_the_role_tier` | `c03_resolution.py` 2 项 | 有 |  |
| `VAL-CONF-016` | 冲突条目可按图纸与状态检索 | `ocs_collab_server::http::tests::conflicts_are_listed_by_drawing_and_status` | `c03_resolution.py` 2 项 | 有 |  |
| `VAL-CONF-017` | 删除被引用对象被拒绝 | `ocs_collab::conflict::tests::an_object_at_the_end_of_a_pipe_run_cannot_be_deleted`<br>`ocs_collab_server::ops::tests::deleting_an_object_a_pipe_run_ends_on_is_refused` | `c02_object_topology.py` 4 项 | 有 |  |
| `VAL-CONF-018` | 审计日志改删请求返回 405 | `ocs_collab_server::http::tests::audit_seq_requests_return_405_and_leave_the_log_untouched`<br>`ocs_collab_server::http::tests::audit_mutation_returns_405` | — | 有 | 没有脚本；`audit_seq_requests_return_405_and_leave_the_log_untouched` 这个单测起真实端口发请求。mission 记录里这一条已是通过 |

### 4.9 跨区流程 `VAL-CROSS`（12 条）

脚本起真实的服务器和两三个桌面端。`003`、`008`、`009` 三条在 mission 交回时还没有做，后来补做（管线拓扑进入协同、离线排队与重连对账、连接符跨图），脚本是 `y03`、`y08`、`y09`，已经在 2026-11-12 那次全量执行里通过。任务清单里「`VAL-CROSS-003`、`008` 没有脚本」的说法已经过时。

| 编号 | 契约标题 | 单元测试 | 端到端脚本 | 覆盖 | 说明 |
|---|---|---|---|---|---|
| `VAL-CROSS-001` | 设计师从打开到保存的完整协同旅程零丢失 | `app::collab::apply::tests::placing_a_symbol_and_setting_its_property_are_said_to_the_room` | `x01_journey.py` 11 项 | 有 | 图是种子图的副本；「重新打开该文件」在同一个实例里做（关掉文档再打开）。脚本早于大属性对象和「改位号带真实来源」（见第 5.2 节） |
| `VAL-CROSS-002` | 占用排他性贯穿 GUI 与服务器 | `app::collab::apply::tests::a_write_to_a_tag_somebody_else_holds_is_taken_back`<br>`ocs_collab_server::http::tests::an_op_on_a_tag_another_session_holds_is_refused_until_it_is_released` | `x02_claim_exclusive.py` 16 项 | 有 |  |
| `VAL-CROSS-003` | 管线插阀门后双方拓扑一致 | `app::collab::collab_pipe_ops::a_peer_follows_a_line_a_run_and_a_valve`<br>`ocs_collab::conflict::tests::collab_pipe_ops_an_account_of_a_run_is_what_the_run_is_now`<br>`ocs_collab_server::http::tests::collab_pipe_ops_a_held_line_refuses_changes_to_its_connections` | `y03_pipe_topology.py` 17 项 | 有 | 契约写「两条 `op_applied`」，实现是三条（管线、管路、管件各一条）。同一个脚本还查了删除（标签 `CT-13-delete`，5 项） |
| `VAL-CROSS-004` | 无编辑权限角色被降级、写被拒且审计留痕 | `ocs_collab_server::http::tests::a_role_that_does_not_edit_joins_as_a_viewer_and_is_told` | `x04_readonly_role.py` 9 项 | 有 |  |
| `VAL-CROSS-005` | M3 持久化数据穿越 M6 协同会话后完整 | — | `x05_m3_persistence.py` 11 项 | 有 | 基线图由仓库里一条默认不跑的测试写出（`write_the_m3_baseline_drawing`）；「插入阀门」是独立放置的阀门，不是插在管路上的管件。脚本早于大属性对象（见第 5.2 节） |
| `VAL-CROSS-006` | 首次访问流程从空列表到两人 | `app::collab::claims::tests::naming_the_server_saves_it_and_starts_the_session`<br>`ui::window::pid_collab::tests::the_roster_shows_this_session_and_says_when_nobody_else_is_there` | `x06_first_visit.py` 17 项 | 有 | 「在设置中」配服务器地址是用命令做的 |
| `VAL-CROSS-007` | 冲突生成到裁决的跨区域闭环 | `app::collab::apply::tests::a_ruling_reaches_the_drawing_and_takes_the_entry_off_the_panel`<br>`app::collab::tests::a_conflict_is_kept_until_its_ruling_arrives`<br>`ui::window::pid_collab::tests::a_conflict_awaiting_a_ruling_is_listed_with_what_is_contended` | `x07_conflict_closure.py` 14 项 | 有 |  |
| `VAL-CROSS-008` | 断线期间离线编辑，重连后与服务器对账 | `app::collab::collab_offline_queue::steps_made_with_the_session_away_wait_and_go_out_in_order_one_at_a_time`<br>`app::collab::collab_offline_queue::what_the_room_did_meanwhile_arrives_before_anything_is_sent`<br>`app::collab::collab_offline_queue::a_step_the_server_refuses_is_taken_back` | `y08_offline_reconcile.py` 10 项 | 有 | 脚本里两边各离线改一个字段（设计压力、设计温度），契约写的是一边改公称直径。同一个脚本还查了被拒的修改撤回（标签 `CT-14-rejected`，18 项） |
| `VAL-CROSS-009` | 页间连接符跨图协同与属性传递 | `app::collab::collab_connector_cross_drawing::two_drawings_are_paired_by_the_server_and_the_lines_field_arrives_on_the_other`<br>`ocs_collab::link::tests::collab_connector_cross_drawing_an_outflow_and_the_inflow_it_names_are_paired`<br>`ocs_collab_server::ops::tests::collab_connector_cross_drawing_the_server_pairs_an_outflow_with_the_inflow_on_the_drawing_it_names` | `y09_connector_cross_drawing.py` 18 项 | 有 |  |
| `VAL-CROSS-010` | 双实例并发插入的位号唯一性由服务器保证 | `app::collab::apply::tests::a_symbol_the_server_renumbered_is_renamed_where_it_was_placed`<br>`app::collab::collab_existing_tags::a_peers_symbol_placed_under_a_listed_tag_is_given_another`<br>`ocs_collab::conflict::tests::a_symbol_placed_under_a_taken_tag_is_given_the_next_free_one`<br>`ocs_collab_server::http::tests::two_symbols_placed_under_one_tag_end_up_with_two_tags` | `x10_concurrent_insert.py` 11 项 | 有 | 脚本早于「加入时上报图里已有的位号」：两份图本来就不一样的情形只有单测，没有端到端断言（见第 5.2 节） |
| `VAL-CROSS-011` | 只读会话的截图与审计交叉验证 | — | `x04_readonly_role.py` 11 项 | 有 | 只读标识和属性面板的行是从程序上报的界面状态读的，截图本身只查了写出来 |
| `VAL-CROSS-012` | 协同会话结束后本地文件可独立打开且无协同残留 | `ui::window::pid_collab::tests::a_panel_with_no_server_says_so_and_how_to_set_one` | `x12_after_session.py` 15 项 | 有 |  |

## 5. 缺口

### 5.1 没有覆盖或只覆盖一部分的（19 条）

按要做的事分五类。「现状」一列是对照表里的取值；各条缺什么，见对照表的「说明」。补完的类只留一段话，不再列表。

**（一）判据在截图上，要起 GUI 的场景（15 条）**

大致是五个新场景：调色板、插入、预览、位号显示、管线断开，再加冒烟场景里的两处补充。

| 编号 | 契约标题 | 现状 | 要补什么 |
|---|---|---|---|
| `VAL-INFRA-009` | capture 返回可用的 PNG 文件 | 部分 | 在冒烟场景 `t00_smoke.py` 里补断言：写出的文件以 PNG 文件头开头、响应里的宽高是正整数 |
| `VAL-INFRA-010` | capture 返回世界到屏幕的坐标映射与实体屏幕坐标 | 部分 | 在冒烟场景里补断言：截图后用响应里的矩阵把一个已知实体的世界坐标换算成像素，与响应给的该实体屏幕坐标比，相差不超过 20 像素 |
| `VAL-DRAW-001` | 调色板面板可打开，且分类页签与符号库一致 | 部分 | 补一个调色板场景：`PIDPALETTE` 打开后读界面元素位置（面板 `pid-palette-panel` 已经上报），断言面板出现、开关前后实体数不变；页签文字要让页签也上报位置，或在截图上判断 |
| `VAL-DRAW-002` | 调色板面板可关闭，且重复打开不叠加 | 部分 | 同一个调色板场景：关、再开，断言面板的上报位置消失、再出现且只有一份 |
| `VAL-DRAW-003` | 分类页签可切换，网格内容随页签变化 | 部分 | 同一个调色板场景：切换页签，断言格子（`pid-palette-cell-*`，已经上报位置）的个数等于该类的符号数、两个页签的格子不同 |
| `VAL-DRAW-004` | 网格每个符号都有可见图标，无空单元格 | 部分 | 同一个调色板场景：按每个格子的上报矩形在截图上数非背景色像素，断言都大于零 |
| `VAL-DRAW-006` | 面板状态在文档切换时保持 | 部分 | 同一个调色板场景：新建第二个文档再切回，断言面板仍在、仍是原来的页签 |
| `VAL-DRAW-009` | 插入位置与喂入坐标一致（插入点读回 + 矩阵反算） | 部分 | 补一个插入场景：带 `focus_handles` 截图，用返回的矩阵把实体的屏幕像素反算回世界坐标，与响应里的 `world_center` 相差不超过 0.5 |
| `VAL-DRAW-010` | 插入后符号在截图里可见且能被 `highlight_handles` 圈出 | 缺 | 同一个插入场景：不带、带 `highlight_handles` 各截一张，断言实体在 `visible_entities` 里、它的 `screen_bounds` 内两张图的像素不同 |
| `VAL-DRAW-014` | 预览跟随鼠标移动 | 部分 | 补一个预览场景（默认视口截图）：放置中喂两个点各截一张，断言预览的像素位移与矩阵换算的结果相差不超过 8 像素、实体数不变 |
| `VAL-DRAW-015` | `空格` 使预览旋转 90° | 部分 | 同一个预览场景：`SPACE` 前后各截一张，断言预览包围盒的宽高比互换、中心不动（不超过 8 像素） |
| `VAL-DRAW-016` | `Ctrl+空格` 切换插入点 | 部分 | 同一个预览场景：`CTRL+SPACE` 前后各截一张，断言预览相对光标点的位置变了（不少于 3 像素） |
| `VAL-DRAW-021` | 位号唯一冲突时显示 `？` | 部分 | 补一个位号场景：做出位号冲突后带 `focus_handles` 截图，断言该实体旁有位号文字的像素；显示的是 `？` 由单测保证，截图上认字不做 |
| `VAL-DRAW-022` | 位号在截图中可见（文字渲染） | 部分 | 同一个位号场景：三个符号各自的 `screen_bounds` 外扩 40 像素的区域内，非背景像素比只有符号几何时多，且三块文字区域互不重叠 |
| `VAL-PIPE-017` | 截图可见断开——管线不再贯穿阀门 | 缺 | 补一个场景：管线上插阀门后截图，读两段管段的 `screen_bounds`，断言它们分在阀门两侧、都不穿过阀门的矩形 |

**（二）判据是耗时（4 条）**

| 编号 | 契约标题 | 现状 | 要补什么 |
|---|---|---|---|
| `VAL-SEM-014` | 2000 实例位号生成性能 | 部分 | 把这条性能测试接进一个固定会跑的入口（总入口里加一步，带 `-- --ignored` 指名跑它），否则平时没有人跑 |
| `VAL-DRAW-030` | 5000 实体画布上符号插入 ≤ 100 ms（中位数） | 部分 | 补一个带计时的场景：5000 条线的底图上连续插入 20 次，记下 20 个样本，断言中位数不超过 100 毫秒（测前确认没有编译进程在跑） |
| `VAL-DRAW-031` | 2000 实例位号生成 ≤ 3 秒 | 部分 | 给现有的 2000 个批量测试加上耗时断言（不超过 3000 毫秒），做成默认不跑的性能测试，与 `VAL-SEM-014` 一起接进固定入口 |
| `VAL-DRAW-032` | 插入性能不受既有 P&ID 实体数量线性拖垮 | 部分 | 接在 `VAL-DRAW-030` 的场景里：先放 200 个设备符号再测一组 20 次，断言中位数不超过 100 毫秒、比前一组增长不超过 100% |

**（三）要真实的服务器进程或套接字（原 6 条，2026-11-17 补完）**

`VAL-SRV-024`–`028`、`030` 补了四个不起 GUI 的场景：`s01_restart`（重启）、`s02_port_in_use`（端口被占）、`s03_disconnect`（断线）、`s04_concurrent_ops`（并发提交），挂在总入口里（现在是第五组），对照表里已改成「有」。各项检查试过会不会红，做法和结果见第 5.4 节。做并发场景时发现服务器在更大的规模下会丢广播，列在第 5.3 节。

**（四）补或改一条单元测试就够（原 7 条，2026-11-17 补完）**

`VAL-INFRA-005`、`VAL-PERM-018`、`025`、`028`、`030`、`031`、`032` 各补了一条单元测试，对照表里已改成「有」，测试名见第 4.2、4.3 节。每一条都试过会不会红：把它盯着的那处代码临时改坏，测试在对应的断言上失败，改回去才通过。`VAL-PERM-032` 与契约字面有一处出入，列在第 5.3 节。

**（五）验收规程与脚手架（原 7 条，2026-11-17 补完）**

`VAL-INFRA-001`、`007`、`008`、`012`、`013`、`014` 和 `VAL-CLI-019`。动手前先裁决了三处（2026-11-17）：「60 秒内就绪」只在冒烟场景里断言，其余场景等就绪的上限仍是 180 秒，超时后结束进程、记下日志、换一个端口重试一次，契约的字面不改；「实例消失后重试一次」做进总入口，两次的结果都留；冷构建加一步依赖检查，「不超过 20 秒」那一句留到 CP-A 裁决。

做法：规则写进脚手架和入口（第 3.2 节的规程表）；新写四个脚本去试（`t02_harness_rules`、`t03_vanish_retry`、`t04_entry_rerun`、`r01_entry_rules`），冒烟场景和 `t20_session` 各加了检查项；都挂在总入口里。对照表里已改成「有」。各项检查试过会不会红，做法和结果见第 5.4 节；做法与契约字面不一样的地方列在第 5.3 节。`VAL-CLI-019` 里「冷构建不超过 20 秒」这一句没有检查。

### 5.2 脚本早于后来的改动

2026-11-12 那次全量执行之后合进来的改动，单元测试都有，脚本里没有对应的断言（在脚本目录里检索过关键字）。这些不改变对照表里的取值，但相关的几条契约在端到端这一层只验到了改动之前的行为。其中第一行的改动还让一项已有的断言不再通过（第 3.3 节）。

| 改动 | 提交 | 脚本的现状 | 要补什么 |
|---|---|---|---|
| 加入时上报图里已有的位号，服务器据此避开 | `1682b84c3`、`93433f7d2`、`b34bccf7e`、`ecdd8dd91` | 没有断言。另外 `t20_session.py` 里数「入站 hello」日志行的那一项（`VAL-CLI-001`）现在不通过：同一次 hello 服务器记两行 | 扩 `x10_concurrent_insert.py`（`VAL-CROSS-010`）：一方的图里原本就有服务器没见过的位号时，新符号不会被改派到那个位号上；为上报而重新加入后，原先的占用还在。改 `t20_session.py` 的那一项：只数发出 `welcome` 的那一行，上报结果的那一行另写一项断言 |
| 属性变更带覆盖标记，改位号带真实来源 | `9b5327f96` | 只在发起方读了 `tag_source`，接收方没有断言 | 扩 `t10_broadcast.py` 或 `x01_journey.py`（`VAL-CLI-009`、`VAL-CROSS-001`）：接收方读回的 `tag_source` 与覆盖标记和发起方一致 |
| 大属性对象（属性多到存进扩展字典的符号、管线、管路、管件）新建、改属性、撤销、重做都带全属性 | `0744f67b2`、`e24b3f947`、`68f08721e` | 没有断言 | 扩 `x01_journey.py` 或 `x05_m3_persistence.py`（`VAL-CROSS-001`、`VAL-CROSS-005`）：建一个属性多到溢出的符号，改属性、撤销、重做，两端读回一致 |
| 服务器的 `--bind`（对外监听） | `278439b5d` | 不在契约里；脚本都在本机回环地址上跑 | 两台机器的流程没有跑过，记作已知未验证；脚本做不了，要人在两台机器上试 |
| 协同分支合进 `main`、合并上游 | `af23cffef`、`2278e4fba`、`5b2c2a51f` | 合并之后跑过两遍（2026-11-17：脚本收进仓库时跑的原有 22 个场景，补完验收规程后跑的五组全量；第 3.3 节），证据都没有逐条核对 | 全量复验（CC-7） |

### 5.3 契约字面与实现有出入的（不算覆盖缺口，结项前要裁决）

脚本过了，但契约原文里有一句按字面不成立，或者脚本的做法与原文不完全一样。前七条来自状态文档 §13.3（当时记作「13 条成立，7 条部分成立」），其余来自 §18.2 和这次对照；`VAL-INFRA` 的四行是 2026-11-17 补验收规程的缺口时重写或新加的，写的是脚手架和入口的做法与契约字面不一样的地方（`VAL-CLI-019` 那一行也在那时改过）；最后两条是补缺口时发现的（`VAL-PERM-032` 在补单元测试时，`VAL-SRV-030` 在补并发场景时）。

| 编号 | 出入 |
|---|---|
| `VAL-CLI-003` | 原文「后加入的一方收到关于先加入者的 `peer_joined`」不成立：后加入者是从 `welcome.peers` 里知道先加入者的。脚本按 2026-11-11 定的口径断言。第二个观测端用普通 WebSocket 客户端，是契约已登记的口径 |
| `VAL-CLI-005` | `claim_denied` 帧里的 `holder` 是会话号，不是原文写的「显示名或用户号」；桌面端再把它解析成用户显示 |
| `VAL-CLI-009` | 原文「双方均确认收到对方的 `peer_joined`」不成立，与 `003` 是同一件事 |
| `VAL-CLI-011` | 当时只读会话的放行名单里混有会改图的命令，「写命令被拒」作为一类不成立。后来修了（状态文档 §15、§18.4），现在由两条单元测试盯着放行名单、一条盯着名单之外的总闸；脚本只查了契约给的那一条命令（`PIDTAG`）。记录里的「部分成立」没有改回 |
| `VAL-CLI-013` | 当时服务器把加入者自己也列进 `peers`，「首次连入为空数组」「长度为 1」两句不成立。后来修了，脚本现在按这两句的字面断言并通过；记录里的「部分成立」没有改回 |
| `VAL-CLI-018` | 原文「两个实例互相收到 `peer_joined`」只对先连入的一方成立，与 `003` 是同一件事 |
| `VAL-CLI-019` | 「冷构建不超过 20 秒」不成立，这一句没有检查，留到 CP-A 裁决（改字面，还是删掉这一句）。依赖树的检查已经有了（`r01_entry_rules.py`）。`smol` 按 2026-11-09 的裁决不计入（它是界面库自带的依赖），检查里只把它记下来 |
| `VAL-CROSS-001` | 图是种子图的副本，不是契约写的那个路径；「重新打开该文件」在同一个实例里做（关掉文档再打开） |
| `VAL-CROSS-003` | 原文写「两条 `op_applied`」，实现是三条：管线、管路、管件各一条 |
| `VAL-CROSS-005` | 基线图由生成器写出，不是契约写的那个路径上的文件；「插入阀门」是独立放置的阀门，不是插在管路上的管件 |
| `VAL-CROSS-006` | 「在设置中」配服务器地址，脚本是用命令配的 |
| `VAL-CROSS-008` | 原文是一方离线改公称直径；脚本是两方各离线改一个字段（设计压力、设计温度）。判的是同一件事：离线的修改重连后提交、对方读回、不静默丢弃 |
| `VAL-CLI-004`、`007`、`016` | 契约允许「点面板或用等价命令」，脚本用的都是命令；面板上的占用、释放按钮和窗口的关闭按钮没有端到端点过 |
| `VAL-INFRA-001`、`014` | 契约写 60 秒内就绪。按 2026-11-17 的裁决，60 秒只在冒烟场景里断言，其余场景等就绪的上限仍是 180 秒；超时后的处置（结束进程、留日志、换端口重试一次）所有场景一样。契约的字面不改。另有一处要知道：冒烟场景里第一次启动到 60 秒没有就绪、照规则重试后 60 秒内就绪的，记通过（说明里写出重试）；两次都不就绪才记失败 |
| `VAL-INFRA-007` | 契约写「验证器记下现象、同端口重启同一张图、整个流程重跑一次」。脚本的做法：现象记在证据目录里，不是契约写的那个固定路径；「重跑」是入口把整个场景脚本再跑一遍；「`/ready` 连续 3 次拒绝」是在某个请求被拒绝之后才去连探 3 次，平时不轮询。`t03` 重跑时用的是第一次那张图（不重新复制，只删掉被结束的实例留在旁边的锁文件和恢复文件）；别的场景没有为重跑另写代码，重跑就是从头再跑一遍，图按各自开头的做法重新准备 |
| `VAL-INFRA-008`、`012` | 2026-11-12 那次执行用的副本端口整体加了 100（3310–3359）。收进仓库时已改回契约的端口段（见 3.2 末），不用裁决。`008` 还有一处做法上的出入：启动前查端口，契约写的是 `Get-NetTCPConnection`，脚手架用的是去连本机回环地址（连得上就是有人在监听）；只绑定还没有开始监听的、或只监听在别的地址上的端口，会被当成空闲。`012` 的监听方是用 `Get-NetTCPConnection` 查的 |
| `VAL-INFRA-013` | 契约写产物都放在一个固定的本机路径下。脚本里没有写死的路径，产物写在运行时指定的证据目录里，只要求它在仓库之外。`git status` 比的是「比开跑前多出来的行」，开跑前工作区里已有的改动不算；开跑前就已经列在改动里的文件再被改一次，这样比看不出来 |
| `VAL-PERM-032` | 原文要求建会话时塞进载荷的伪造 `workspace` 不被采纳。实现里 `workspace` 是 `hello` 和 `POST /session` 的正式字段，会话就是在那里声明自己的工作区的（`VAL-PERM-025` 的 `ws-alpha` 也是这样来的），写在那里的值原样记进审计，服务器分不出「声明」和「伪造」。成立的部分：帧里多出来的 `ip`、`company` 键，以及提交修改的帧里的 `workspace` 键，服务器都不读；`ip` 取连接的对端地址，`company` 取用户表。单元测试断言的是成立的这部分。要裁决：把原文改成「`workspace` 由会话声明，另两项不可自报」，还是让服务器按一张工作区名单校验 |
| `VAL-SRV-030` | 契约写「不少于 8 个会话、各不少于 30 个 op」。按下限（8 个会话、每批各 30 条）成立，场景就是这样跑的。规模再大就不一定：服务器给每张图的广播队列只留最近 256 条，某条连接积压超过这个数，最早的那些广播就被丢掉（服务器在 stderr 记一行 `subscriber lagged`；代码注释写明这是有意的，丢了的靠客户端重新对齐）。连着发 op 的会话转发广播慢，积压会接近整整一批。10 个会话各连发 30 条（一批 300 条）时，这台机器上 9 次里有 4 次丢了广播，其中一次有 19 条已经生效的 op、发送方自己没有收到 `op_applied`。丢的只是通知：那一次库里的图纸修订、操作日志、审计行都是全的。要裁决：维持现状、把规模上限写进契约，还是改服务器（发送方的回执单独发、不走广播队列；或转发时先把积压的发完；或加大队列）。复现：把 `s04_concurrent_ops.py` 里的 `N` 改成 10，多跑几次 |

### 5.4 这张表是怎么核对的

- 228 个编号各出现一次，各区条数与状态文档第 4 节相同；mission 记录的两栏合计 179 与 49，也与那一节相同。
- 表里引用了 287 个单元测试，每个名字都在全量测试日志里唯一地找到，并且是通过的（标「默认不跑」的一条除外）。日志是基准提交上的一份；补缺口加的 7 条取自 2026-11-17 补完它们之后重跑的那一次。
- 表里引用了 31 个脚本（30 个场景加收尾的整轮检查），都存在。原有 22 个场景的项数取自 2026-11-12 那次执行的结果文件；服务器进程的四个场景取自 2026-11-17 它们经总入口跑的那一次；脚手架与入口规程的四个场景、收尾的整轮检查，以及 `t20_session` 里后加的 `VAL-INFRA-012` 那一项，取自 2026-11-17 的五组全量执行（第 3.3 节）。取数的这几次执行里，表里引用到的项都没有失败的；`t20_session` 现在有一项失败（第 3.3 节），表里它原有的几行仍是 2026-11-12 的项数。
- 服务器进程的四个场景，逐项试过会不会红：把服务器在某一处临时改坏、重新编出来（共 19 处，一次只改一处；改坏的可执行文件只放在仓库外，源码改完即还原），让场景对着它跑，共 21 次；另有一次不重启服务器的对照。22 次运行每一次都有检查失败。带契约编号的 43 项检查里，41 项至少在一次这样的运行里失败过；剩下 2 项没有做出让它失败的改法：`s02` 的「原来的实例照常服务、原来的连接没有收到 `error`」和 `s03` 的「重新握手成功，且落在销毁之后 3 到 30 秒之间」。
- 脚手架与入口规程的四个场景、收尾的整轮检查和 `t20_session` 里后加的那一项，也逐项试过会不会红（2026-11-17）。做了 62 次对照运行，一次只改一处，改的都是放在仓库外的副本：30 次改脚手架（把某一条规则拿掉或改错），10 次改入口的重跑规则，5 次改场景自己（重跑时换一个端口、换一张图，流程里的命令不建实体；检查之前先把服务器结束、把 GUI 冻结），1 次不改脚本、让真实例连丢两次，6 次针对收尾的整轮检查（一轮中间往仓库里放一个文件、放一份字节码缓存，跑完即删；不做开头的记录；换成另外两个包的依赖树；让两份脚手架差一行），10 次让冒烟场景对着一个改过的 GUI 跑（`/api/v1/ready` 的字段给错 7 种、在哪个端口上都要过 65 秒才就绪、只在 3210 上这样、什么都不改；改过的可执行文件只放在仓库外，源码改完即逐字节还原）。62 次的结果都与事先写下的预期一致：60 次有检查失败；另 2 次是应当全部通过的对照（改过的 GUI 什么都不改；只在 3210 上过 65 秒才就绪，第一次到上限被结束、换端口重试后 60 秒内就绪），确实全部通过。这几个脚本一遍跑下来是 36 项检查（`t00` 7、`t02` 13、`t03` 5、`t04` 5、`r01` 5，加 `t20` 的 1 项），其中 29 项带契约编号：这 29 项都至少在一次对照里失败过；没有失败过的 5 项都是不带契约编号的前提项（实例就绪三项、替身场景跑完、截图接口回 200）。另有一项只在失败时才会出现的检查（`t03` 的「实例已经被结束，脚手架却没有发现」），没有一次对照走到它：把脚手架的探测去掉之后，请求直接报连接错误，场景记的是「没有跑完」，红的是另一行。
- 2026-11-17 的两遍（收进仓库时跑的原有 22 个场景、补完验收规程后跑的五组全量），原有 22 个场景的结果文件都与 2026-11-12 的逐个场景、逐个标签比过通过和失败的项数：除 `t20_session` 之外全部相同。`t20_session` 里过时的那一项两遍都是失败（2026-11-12 是通过），第二遍另多出后加的 `VAL-INFRA-012` 一项。
- 「有」「部分」「缺」是逐条对着契约正文和测试、脚本的代码判断的，判断时没有重新执行端到端脚本。前六个区逐条读了契约正文和测试代码；后三个区以状态文档当时的逐条结论为准，对带计时、截图判据的和后补的几条另读了脚本的检查项。
