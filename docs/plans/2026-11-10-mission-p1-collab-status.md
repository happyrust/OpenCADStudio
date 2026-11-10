# 智能 P&ID 平台 mission（P1 单机绘图闭环 + 多用户协同）：现状整理与 2026-11-10 合并记录

> 日期：2026-11-10
> 范围：Factory droid 跑的 mission「OpenCADStudio 智能 P&ID 平台：P1 单机绘图闭环 + 完整多用户协同」做到了哪一步、还剩什么、记录在案的欠账，以及今天把上游与 `upstream-merge-20261110` 并进 `main`、把中断时的半成品搬到新 `main` 上的经过和验证结果。本文不含新功能。
> mission 目录：`C:\Users\dpc\.factory\missions\ca49c5e7-cf3b-4a0a-a262-90f74b42a894`（missionId `mis_d77ca2e3`）。**这个目录只读，本次没有改过其中任何文件**；此后的进度记在本文。
> 基于：`main` = 本文所在的提交，其父提交是 `dd43f6aa2`（上游 `000814aff` 的合并）。mission 中断时的 `main` 是 `2e9e9dab2`。
> 证据：除标注「推断」「未复核」「mission 记录」的句子外，数字都是今天从 mission 文件、`git log` 和亲自跑的构建与测试里得到的；复算办法见 §12。

## 0. 一页结论

- **mission 没有在跑。** `state.json` 是 `paused`，最后一条进度记录是 13:48:28 的 `mission_paused`。19:45:45 那次写入是 Factory 桌面端恢复编排会话时刷新了时间戳，之后没有任何新的运行（§2）。
- **进度**：7 个里程碑里 M1–M5 已关闭（实现、代码评审、逐条断言取证都做完）；M6 的 4 项功能做完 3 项并已提交，第 4 项 `collab-client-broadcast` 是工作区里的半成品；M6 的里程碑验证还没跑；M7 的 4 项没有开始。`features.json` 共 57 条：52 completed / 1 in_progress / 4 pending（§3）。
- **断言**：契约 228 条，179 passed、49 pending，没有 failed 或 blocked。pending 的是 `VAL-CLI` 20 条（M6）、`VAL-CONF` 17 条与 `VAL-CROSS` 12 条（M7）（§4）。
- **代码**：`main` 现在 = 上游 `origin/main` 到 `000814aff` + 今天 `upstream-merge-20261110` 上的工作（到 `fc37fbaf2`）+ mission 的全部提交。半成品已原样搬回工作区，只为跟随上游改名改了 18 处路径，仍是未提交的改动（§10）。
- **验证**（mission 的工具链与构建目录）：只含合并时 6465 通过 / 1 失败 / 43 忽略；加上半成品 6483 / 4 / 43。失败的都是协同传输那组测试和两条并行时偶发的既有测试，单独跑全部通过（§10.4）。
- **新查明的一件事**：协同传输那组测试整套跑时失败，主因不是等待时间太短，而是进程内全局的停止标志被别的测试置位后不复位。单线程、机器正在编译的情况下它们 14/14 通过；把 `batch_exit_saves_and_stops` 排到它们前面，单线程也必然失败（§6.1）。
- **下一轮**从修这组测试的隔离开始，接着收尾 `collab-client-broadcast`，再跑 M6 验证，然后 M7（§8）。

## 1. mission 是什么

### 1.1 目标

在现有 OpenCADStudio（Rust + iced 桌面 CAD）上实现 PRD 的两块能力：

1. **P&ID 绘图作者功能**：从调色板插入符号、按规则自动分配位号、类型化属性面板、绘制管线 / 管路 / 管段并维护拓扑、插入阀门时管线自动断开、移动时自动补位、页间连接符、DWG 往返无损。
2. **多用户实时协同编辑**：新建协同服务器（HTTP + WebSocket + SQLite），位号级占用、变更广播、三级权限判定链（角色矩阵 × PBS 节点 ACL 继承 × 图纸状态）、心跳 / 超时 / 接管、冲突检测与裁决、只追加的审计日志。

最终形态（`mission.md` 原话的意思）：多个设计师同时打开同一张图，服务器按位号授予占用、广播变更、冲突可裁决、每一步写审计；整张图存回 DWG 后位号与属性零丢失。

### 1.2 架构上已定的事

| 决策 | 选择 |
|---|---|
| 服务器 | workspace 里两个 crate：`crates/ocs_collab`（协议与纯逻辑，只许 serde / serde_json / uuid / chrono）、`crates/ocs_collab_server`（axum + tokio + rusqlite 的独立二进制） |
| 客户端 | `src/app/collab/`，同步 `tungstenite` + `std::thread` + futures channel，**不引 tokio** |
| 同步单元 | P&ID 语义操作（`set_tag`、`set_property`…），服务器不理解 DWG |
| 每实体语义 | XDATA，APPID `PID_AUTHOR`（与识别器的 `PID_SEMANTICS` 分开），单实体 2 KB 以内，超出挪到 XRecord |
| 图级数据 | 模型空间扩展字典下的 XRecord（类型库、位号规则、拓扑表、连接符表） |
| 变更广播钩子 | `push_undo_entry`（`src/app/history.rs`），所有撤销步的唯一汇聚点 |
| 端口 | 协同服务器 3240，GUI 验证实例 3210–3239；整体只许用 3210–3259 |

### 1.3 明确不做、不许做的事

- 不改 `D:\work\plant-code\cad\iced`（本地 iced fork，只读依赖）；发现它的问题只记录。
- 不用 Docker / Postgres / Redis 或任何没在 `services.yaml` 登记的服务；服务只监听 `127.0.0.1`。
- 不新增实体类型（符号用 `Insert` 承载）；识别器（`src/io/pid_legend/`）是复用，不改造。
- 不引入清单之外的新依赖；需要时回到编排层决定。
- 服务器不做鉴权（本机回环）；多实例部署不在范围内（`revision` 的唯一性只保证单进程，见 §6.2）。
- 被编排层裁定「不属已批准范围」而搁下的：ALIGN 拒绝消息的本地化、`handle_align_selected` 死代码、GUI `/api/v1/save` 失败时的结构化错误码、旋转 / 缩放 / 镜像管件时管段不跟随、打断符只支持直线管段、符号库没有 elbow、占用行不显示剩余时间。
- PRD 里 P1、P3 之外的部分不在这次 mission 里（推断：`mission.md` 只写了这两块，没有单列「不做」清单）。

### 1.4 mission 目录里有什么

| 文件 | 内容 |
|---|---|
| `mission.md` | 目标、架构、七个里程碑、测试与验证策略 |
| `features.json` | 57 条 feature：描述、前置、期望行为、认领的断言、状态、worker 会话 |
| `validation-contract.md` | 228 条断言的正文（判据、工具、证据要求） |
| `validation-state.json` | 每条断言的状态、证据、备注 |
| `AGENTS.md` | 所有 worker 与验证器必须遵守的规则、边界、踩过的坑（721 行） |
| `architecture.md` | 系统设计；附录 A 是契约决策登记，冲突时以它为准 |
| `services.yaml` | 构建、测试、探针命令与服务启停的唯一事实源 |
| `skills\*\SKILL.md` | 五类 worker 的工作流程 |
| `library\` | 每个 feature 交付后的实测事实、裁决记录（`pid-m3-decisions.md`）、探针脚本、补丁 |
| `handoffs\`（39 份）、`validation\`、`evidence\` | 交接单、评审与取证产物 |
| `progress_log.jsonl`（258 行）、`state.json` | 运行日志与状态 |

worker 与验证器用的模型是 `custom:DeepSeek-Flash-1`（`model-settings.json`）。

## 2. 运行状态与时间线

### 2.1 现在是否在跑

判断：**已暂停，没有 worker 在改这棵树。** 依据：

- `state.json`：`"state": "paused"`。
- `progress_log.jsonl` 最后三条：11:57:27 `mission_run_started`（collab-client-broadcast）、11:57:28 `worker_started`、13:48:28 `mission_paused`。之后没有 `mission_resumed` / `mission_run_started` / `worker_started`。
- 19:45:45 的写入：编排会话文件末尾在这一刻多了一条 `SessionStart` 钩子记录（`hookMatcher: "resume"`，跑的是 hindsight 的会话启动钩子），同一秒 `state.json` 的 `updatedAt` 被刷新、`progress_log.jsonl` 被重写但没有新增行。也就是桌面端打开并恢复了这个会话，没有开始新的运行；这条记录之后没有任何 assistant 消息。
- 那个 worker 的会话文件（`fac4a5df…`）最后写于 13:51:00，Factory 桌面端在 13:51:05 重启。
- 主检出：20:01 与 20:03 两次 `git status` 相同，半成品文件最晚的修改时间是 13:40:21，最近 30 分钟没有任何文件变化。
- 当时在跑的 cargo / rustc 进程只有两组，输出目录分别是 `ocs-upstream-20261110\target`（另一条线）和 `pid-parse` 的构建目录，都不是这棵树。
- `droid.exe`（PID 54712，13:51:21 起）是桌面端的守护进程，这段时间在写的是另一个项目（`D:\work\erp`）的会话。

### 2.2 时间线（本地时间）

| 时间 | 事件 |
|---|---|
| 11-02 01:39 | mission 接受；02:22 开始第一轮（当时 43 个 feature、224 条断言） |
| 11-02 全天 | M1–M3 实现；验证顺序是 M3 → M2 → M1，各有一轮不过、修复后通过；22:11 暂停 |
| 11-05 11:19 – 11-08 21:10 | M4：7 项功能 + 1 项修复；代码评审一轮过；用户测试第一轮 2 条不过（契约口径），改契约后第二轮过 |
| 11-08 21:27 – 11-10 00:10 | M5：7 项功能；代码评审三轮（补位没接到真实路径 → 修；撤销不回退拓扑表、ALIGN 谎报 → 修）；用户测试通过 |
| 11-10 00:12 – 11:54 | M6：传输层 02:00、心跳 03:52、占用 11:54（中间 worker 失败一次，10:22 恢复） |
| 11-10 11:57 | 开始 `collab-client-broadcast` |
| 11-10 13:48:28 | mission 暂停；worker 的最后一次工具调用在 13:51:00 |
| 11-10 19:45:45 | 桌面端恢复会话，只刷新了时间戳 |

期间共暂停 13 次、恢复 12 次，worker 中途失败 6 次（都在恢复后补完）。

## 3. 里程碑与功能

57 条 feature = 47 条实现 / 修复 + 10 条验证器（M1–M5 各一条代码评审、一条用户测试）。M6、M7 的验证器条目现在还没有；从进度日志看，它们是在该里程碑最后一项实现完成时生成的。

| 里程碑 | 实现 / 修复 | 完成 | 进行中 | 未开始 | 里程碑验证 | 断言 |
|---|---:|---:|---:|---:|---|---|
| M1 协同服务器骨架 | 7 | 7 | 0 | 0 | 评审 2 轮、用户测试 1 轮，已过 | `VAL-SRV` 30/30、`VAL-INFRA` 14/14 |
| M2 三级权限与审计检索 | 2 | 2 | 0 | 0 | 评审 2 轮、用户测试 1 轮，已过 | `VAL-PERM` 32/32、`VAL-CONF-018` |
| M3 P&ID 语义与位号引擎 | 13 | 13 | 0 | 0 | 评审 2 轮、用户测试 2 轮，已过 | `VAL-SEM` 38/38 |
| M4 绘图作者功能 | 8 | 8 | 0 | 0 | 评审 1 轮、用户测试 2 轮，已过 | `VAL-DRAW` 31/31 |
| M5 管线拓扑与管件 | 9 | 9 | 0 | 0 | 评审 3 轮、用户测试 1 轮，已过 | `VAL-PIPE` 32/32、`VAL-SEM-039` |
| M6 桌面协同接入 | 4 | 3 | 1 | 0 | **没跑** | `VAL-CLI` 0/20 |
| M7 冲突检测与裁决 | 4 | 0 | 0 | 4 | 没跑 | `VAL-CONF` 0/17、`VAL-CROSS` 0/12 |
| 合计 | 47 | 42 | 1 | 4 | | 179/228 |

### 3.1 M1–M3（11-02）

mission 自己没有提交这三个里程碑：当时仓库正处在用户发起的上游合并里，worker 只做精确 `git add` 并把补丁导出到 `library\`。代码是随用户的两个提交进 `main` 的：`ff7bc36d9`（11-02 18:58，上游合并提交里带进了已暂存的内容）和 `3598b3c17`（11-04 20:30，「WIP: AVEVA RVM import, collab server ws/perm, P&ID tag engine tweaks」，含 M1 / M2 的修复与 M3 的 D24–D27）。两者都是 `fork/main`（`ccfbcaf26`）的祖先。

| 里程碑 | feature（括号里是认领的断言数） |
|---|---|
| M1 | `val-infra-probe-baseline`(14)、`collab-crate-scaffold`(0)、`collab-http-health-session`(3)、`collab-ws-data-plane`(13)、`collab-audit-append-only`(5)、`fix-m1-http-startup-health`(2)、`fix-m1-ws-data-plane`(7) |
| M2 | `perm-check-chain`(24)、`perm-audit-query`(9) |
| M3 | `pid-type-lib`(3)、`pid-tag-rule-engine`(13)、`pid-xdata-persist`(6)、`pid-xrecord-graph-data`(4)、`pid-symbol-library`(4)、`pid-dwg-roundtrip`(2)、`pid-property-propagation`(3)、`pid-performance`(0)，以及五条修复：`pid-tag-engine-sequencer`(1)、`pid-persist-props-len`(0)、`pid-tag-engine-parse-props`(0)、`pid-type-lib-generation-assertions`(2)、`pid-type-lib-storage-unify`(0) |

### 3.2 M4–M6（mission 自己提交）

| 里程碑 | feature | 断言 | 状态 | 提交 |
|---|---|---:|---|---|
| M4 | `drawing-palette-panel` | 6 | 完成 | `5ba70bef4`（11-05） |
| M4 | `drawing-insert-symbol` | 7 | 完成 | `d412da7ec`（11-05） |
| M4 | `drawing-insert-interaction` | 5 | 完成 | `3cb2b5beb`（11-06） |
| M4 | `fix-m4-run-cancel` | 0 | 完成 | `7d93625e0`（11-06） |
| M4 | `drawing-tag-display` | 5 | 完成 | `f7964d7a2`（11-06） |
| M4 | `drawing-property-panel` | 5 | 完成 | `96b888464`（11-06） |
| M4 | `drawing-performance` | 2 | 完成 | `162fb6105`（11-08） |
| M4 | `drawing-tag-bulk-performance` | 1 | 完成 | `848f31e5b`（11-08） |
| M5 | `pipe-line-run-segment` | 7 | 完成 | `e094239a7`（11-09） |
| M5 | `pipe-property-propagation` | 6 | 完成 | `b4479cb70`（11-09） |
| M5 | `pipe-insert-fitting-split` | 6 | 完成 | `874fe27cf`（11-09） |
| M5 | `pipe-move-fitting-refill` | 4 | 完成 | `6514c6a04`（11-09） |
| M5 | `pipe-connectors` | 5 | 完成 | `ae5c7c662`（11-09） |
| M5 | `pipe-topology-persistence` | 3 | 完成 | `3c4034b85` + `fcf42f4ef`（11-09） |
| M5 | `pipe-break-symbol` | 2 | 完成 | `3b3c1c5fd`（11-09） |
| M5 | `fix-m5-refill-wiring` | 0 | 完成 | `339921db4`（11-09） |
| M5 | `fix-m5-refill-undo-and-align` | 0 | 完成 | `7c501668f`（11-09） |
| M6 | `collab-client-transport` | 5 | 完成，未验证 | `387a53b12`（11-10） |
| M6 | `collab-client-heartbeat` | 4 | 完成，未验证 | `999be9572`（11-10） |
| M6 | `collab-client-claims` | 5 | 完成，未验证 | `2e9e9dab2`（11-10） |
| M6 | `collab-client-broadcast` | 6 | **进行中**（工作区半成品，§7） | — |

另有一个不挂在任何 feature 下的编排层提交：`0e2353778`（「M5 开工准备：新建 src/pid/topology.rs 占位模块并接线」）。`fork/main..2e9e9dab2` 的 34 个 first-parent 提交里，22 个是 mission 的，其余 12 个是同期另一条线的 P&ID 备份浏览与一次上游合并（`7c5619b37`）。

### 3.3 M7（未开始）

| feature | 断言 | 一句话 |
|---|---|---|
| `conflict-field-merge` | `VAL-CONF-001..005` | 不同字段并发修改自动合并；同字段按 `base_revision` 落后方让位、原值进审计；基线相同且值不同才生成冲突条目 |
| `conflict-object-topology` | `VAL-CONF-006..009`、`017` | 一方删除另一方修改时删除生效并通知；拓扑级冲突生成待裁决条目且裁决前双方拓扑不变；删除被引用对象返回 `OBJECT_REFERENCED` |
| `conflict-resolution` | `VAL-CONF-010..016` | `POST /conflicts/{id}/resolve`（采用甲方 / 采用乙方 / 人工改值）、审计、不可重复裁决、非管理员 403、`GET /conflicts` 检索 |
| `cross-area-flows` | `VAL-CROSS-001..012` | 十二条跨 GUI、服务器、持久化的端到端流程 |

## 4. 验证契约与断言

`validation-contract.md` 里 `### VAL-…` 标题共 228 个，`validation-state.json` 也是 228 条，每条恰好被一个 feature 认领。

| 区域 | 前缀 | 条数 | passed | pending | 在哪个里程碑取的证 |
|---|---|---:|---:|---:|---|
| 协同服务器骨架 | `VAL-SRV` | 30 | 30 | 0 | M1 |
| 验证基础设施 | `VAL-INFRA` | 14 | 14 | 0 | M1 |
| 三级权限与审计 | `VAL-PERM` | 32 | 32 | 0 | M2 |
| P&ID 语义与位号 | `VAL-SEM` | 39 | 39 | 0 | 38 条在 M3，`039` 在 M5 |
| 绘图作者功能 | `VAL-DRAW` | 31 | 31 | 0 | M4 |
| 管线拓扑与管件 | `VAL-PIPE` | 32 | 32 | 0 | M5 |
| 桌面协同接入 | `VAL-CLI` | 20 | 0 | 20 | — |
| 冲突检测与裁决 | `VAL-CONF` | 18 | 1 | 17 | `018` 在 M2 |
| 跨区域流程 | `VAL-CROSS` | 12 | 0 | 12 | — |
| 合计 | | 228 | 179 | 49 | |

- 状态文件里只有 `passed` 和 `pending` 两种值，**没有 failed，也没有 blocked**。179 条 passed 都带 `evidence` 字段。
- 49 条 pending 的原因只有两类：功能做了、worker 自己端到端跑过，但里程碑验证没跑（`VAL-CLI-001..008`、`014..017`、`019`、`020`，共 14 条）；功能没做完或没开始（`VAL-CLI-009..013`、`018`，以及 `VAL-CONF` 17 条、`VAL-CROSS` 12 条，共 35 条）。mission 的规则是断言状态不得预填，必须由用户测试验证器取证后才能标 passed。

曾经不过、后来修好或改了口径的（都已是 passed，列在这里是因为后面的工作会再碰到同样的坑）：

| 断言 | 当时的问题 | 处置 |
|---|---|---|
| `VAL-SRV-002/009/013/015/016/017` | M1 第一轮评审发现 5 处缺陷，原先的 passed 是假通过（端口冲突的失败启动会清空会话与占用、只读库的 `/health` 假健康、op 修订号读改写竞态、接管通知广播给了第三方、`claim()` 吞掉过期条目） | 全部修复，新增 `VAL-SRV-028/029/030` 锁定回归 |
| `VAL-PERM-017` | `POST /session` 建的会话对权限判定不可见 | 裁决 D31–D33，改读 `sessions` 表并补单人编辑上限 |
| `VAL-SEM-015/018/019` | 契约自相矛盾、缺位号序列断言、类型库两套落盘布局 | 裁决 D24–D27 |
| `VAL-SEM-038` | 两条子断言恒真 | 收窄，另立 `VAL-SEM-039`，M5 把 `gate_save` 接进真实保存路径后通过 |
| `VAL-DRAW-009/013` | 用 `capture` 的 `world_center` 当插入点（它是含位号文字的包围盒中点） | 改契约：位置读 `Insert.position` |
| `VAL-PIPE-019..022` | 补位只挂在 REST 通道，GUI 拖动、MOVE、grip_drag 都没接 | `fix-m5-refill-wiring`、`fix-m5-refill-undo-and-align` |

契约里已登记的口径（取证时照这些来，别按字面）：`VAL-DRAW-029` 是预留空号（M4 实际 31 条）；`VAL-CLI-003` 的第二个观测端可以用 Node 直连 WS 代替第二个 GUI；`VAL-CLI-019` 不禁 `smol`（它是 iced 自带依赖）；`VAL-CLI-008` 截图必须带 `wait_for_frame: true`，窗口截图的 `screen_bounds` 要除以 `scale_factor`；`VAL-DRAW-031` 走批量接口 `pid_insert_batch`。

## 5. mission 的记录与仓库实际不符的地方

1. **M1–M3 的 20 条 feature 标 completed，但 `workerSessionIds` 是空的**，进度日志里也没有对应的 `worker_completed` 与提交号（那一段是编排层自己和 Devin 做的）。代码在 `ff7bc36d9` / `3598b3c17` 里，不是 mission 的提交。`val-infra-probe-baseline`、`collab-crate-scaffold` 两份交接单的 `commitId` 写的是 `no-commit-merge-in-progress…`。
2. **`collab-client-broadcast` 标 in_progress，`currentWorkerSessionId` 指向 `fac4a5df…`**，那个会话已经随桌面端重启而结束。仓库这边它是未提交的改动。
3. **M6 的三条 completed feature，对应的 14 条断言还是 pending**：completed 的意思是 worker 交付并自查，不是验证通过。
4. **前置条件里引用了 5 个不存在的 feature id**：`collab-op-broadcast`、`perm-role-matrix`、`collab-ws-handshake`、`collab-heartbeat-timeout-takeover`、`collab-occupancy-claims`。这些是早期拆分时的名字，后来并进了 `collab-ws-data-plane`、`perm-check-chain`（推断）。M7 的三条 feature 还引用着其中两个。
5. **数量没跟着改**：`state.json` 的 `initialFeatureCount` 是 43，现在 57；契约「总览」表仍写合计 224 条、`VAL-SRV` 27 条、`VAL-SEM` 38 条，实际是 228、30、39。
6. **`pipe-topology-persistence` 只记了第二个提交** `fcf42f4ef`，第一个 `3c4034b85` 没记；`0e2353778` 不属于任何 feature。
7. **`AGENTS.md` 的 Git 一节还是任务开始时的描述**（工作树 313 项改动、合并进行中、不许 commit），后文「Known Pre-Existing Issues」才说 11-08 起可以正常提交。它还把 `D:\Rust\target-ocs-mission` 写成「118.8 GB 共享构建缓存」；今天量的是合并前 32.27 GB、合并后 44.49 GB，而 `D:\Rust\target` 现在只剩一个 `release` 目录、0.01 GB。
8. **M7 的部分地基其实已经在仓库里**（features.json 的前置条件看不出来）：`conflicts` 表与 `resolved_value` 列、`store.rs` 的 `insert_conflict` / `list_conflicts` / 裁决更新、协议 crate 的 `conflict.rs`（`resolve_change`、`commit`）、错误码 `OBJECT_REFERENCED`。还没有的是 `delete_object` 这个 op 类型、`/conflicts` 路由，以及把 `ws.rs` 里对 `base_revision` 的硬校验（不等就回 `STALE_BASE_REVISION`）换成三段判定。
9. **mission 之后 `main` 变了**：合并了 532 个上游提交，`codec` 改名 `document`、`app::document` 改名 `app::tab_document`，mission 文件里引用的行号（如 `src/app/mod.rs:589`、`src/app/view/mod.rs:3639`）大多已经不准，只能当线索用。

## 6. 已知问题、欠账、被推迟的事

### 6.1 协同传输那组测试整套跑会失败（mission 自己新加的测试；本轮没改）

现象：`cargo test` 整套并行跑时，`app::collab::transport::tests` 里有 1–4 条报 `the client never reported a session`；只跑 `collab::` 则 62/62 通过。合并前的 `2e9e9dab2` 上就这样。

会失败的是四条用真 socket 的测试：`queued_claims_and_releases_reach_the_wire_with_the_session_id`、`the_session_emits_heartbeats_carrying_its_session_id`、`quitting_sends_a_close_frame_and_joins_the_thread`、`cancelling_the_subscription_ends_the_thread_promptly`。

原因（今天实测）：`transport.rs` 的 `STOP` 是进程内全局的 `AtomicBool`，`collab::shutdown()` 把它置为 `true` 后不复位（设计如此，因为进程正在退出）。而 `exit_app()` 与 `leave_keeping_recovery()` 都会调 `shutdown()`，同一个测试程序里别的测试只要走到退出路径就把它置位了，例如上游的 `app::commands::fileops::tests::batch_exit_saves_and_stops`（批处理跑到 `EXIT`）。此后再启动的客户端一上来就看到停止标志，连都不连。全仓只有 `quitting_…` 这条测试在自己结尾把它复位。

| 实验（合并后、不含半成品的 lib 测试程序） | 结果 |
|---|---|
| 只跑 `app::collab::transport`，`--test-threads=1`，同时机器在做 `-j 10` 的全量编译 | 14/14 通过，1.27 秒 |
| 再加上 `batch_exit_saves_and_stops`，`--test-threads=1`，用 `--shuffle-seed 1..8` 换 8 种顺序 | 7 种顺序失败 1–4 条，1 种全过 |
| 逐条对照 8 种顺序 | 四条真 socket 测试里，排在 `batch_exit…` 之后的每一条都失败，排在它之前的每一条都通过；没有例外。`quitting_…` 排在后面时自己也连不上，走不到结尾的复位 |

所以负载不是主因。等待窗口确实偏短（等连接是 60 次 × 20 ms 的轮询，共 1.2 秒），机器很忙时可能另外造成超时，但今天没有观察到单纯由它引起的失败。

下一轮的改法（建议，未做）：让 `shutdown()` 在没有登记过传输线程时直接返回，或者把停止标志做成每个客户端一份而不是进程全局；等连接改成等事件到达并给宽裕的上限，不靠固定次数的短轮询。改完后整套并行跑应当不再出现这四条。

### 6.2 mission 记录在案、没有修的（未复核，除非另注）

协同服务器：

- `commit_op` 的修订号唯一性来自进程内的互斥量，不是 SQL 层的比较交换。两个服务器进程共用一个库时会各自发出同一个修订号（mission 实测 12 轮里 11 轮）。M7 的冲突裁决不能假设 `base_revision` 跨进程唯一。
- `/health` 每次调用都取一次写锁；外部持写锁时返回 503 且耗时约 5.5 秒。轮询它的地方超时不要低于 6 秒。
- `POST /session` 建的会话没有心跳续期路径，90 秒后自动脱离占用集合。
- `FIXTURE-PERM.json` 无法表达 `drawings.revision`，`VAL-PERM-018` 靠三次 op 把基线抬到 3。
- `probe-m1-ws-030` 不能在同一个库上重跑；WS 探针要先用默认阈值、再用收紧阈值各跑一轮。

P&ID 语义与拓扑：

- `persist` 的溢出路径只按整条记录长度判一次，其余八键本身超 2048 字节时仍可能整条 EED 被丢；`dwg_roundtrip` 的快照对 props 溢出路径有盲区；`split_props` 会把非字符串的 JSON 标量变成字符串、键序重排。
- 拓扑表在删除管路后不立即清理，要到下一次 `PIDPIPE` / `PIDFITTING` 提交才清（`VAL-PIPE-003` 按意图判过）。
- 移动补位只处理平移；旋转、缩放、镜像管件时管段不跟随。打断符只按直线管段算。
- 符号库 7 个符号，没有 elbow（契约 `VAL-PIPE-006` 的例子用不了，改用 transmitter）。
- 连接符的 `peer` 冒号前必须是数字 `document_id`，契约示例写的是 `PID-1102:2B7`；`PIDPAIR` 是契约里没有、自建的配对命令。
- `CmdResult::AlignSelected` 全仓没有生产者，`handle_align_selected` 是死代码；ALIGN 的拒绝消息没进 `src/locale_catalog.rs`。

GUI 控制面与取证：

- 两个 GUI 实例并存时偶发 GPU panic（`wgpu Surface::configure`，根因在只读的 iced fork，`--safe-mode` 无效）。处置是实例消失时换端口重试一次；启动时优先 `--safe-mode`。
- `capture`：`world_center` 是含位号文字的包围盒中点，不是插入点；`view:"region"` 时返回的矩阵与裁剪后的像素错开约 90 px；`scope:"window"` 的 `screen_bounds` 是物理像素而 `ui_bounds` 是逻辑像素；默认不等帧。
- GUI 的 `/api/v1/save` 失败只回纯文本，没有结构化 `code`。
- 不带 `--new` 启动的验证实例可能被单实例机制转交给已在运行的实例后自己退出（退出码 0、无日志）；带 `--new` 时活动文档可能是没有路径的 Start 页，截图整张空白。
- M5 有三组 GUI 探针脚本没有归档（`VAL-PIPE-001..006/028`、`007..012`、`013..018`），复验要重写。

协同客户端：

- 协同面板的「占用」按钮只对已经在占用列表里的行渲染，对任意位号发起占用要用 `PIDCLAIM <位号>`；占用行不显示剩余时间。

### 6.3 构建与环境

- **`Cargo.lock` 比清单少 4 行**，所以 `cargo --locked` 必然失败，mission 一律用 `--offline`，每次构建后还原锁文件。今天看到这 4 行是：`OpenCADStudio` 依赖列表里的 `ocs_collab`、`tungstenite`（M6 往 `Cargo.toml` 加了依赖却没提交锁记录），以及外部路径依赖 `e3d-model` 的 `memmap2`、`rayon`。前两行是这个仓库自己的账。
- PATH 上的 `cargo` 是 huacnlee 的分叉版，无视 `CARGO_TARGET_DIR`；必须用工具链目录里的 `cargo.exe`。
- `cargo clippy --workspace --all-targets` 是既有失败（`examples/open_bench.rs`、一个 bench），门禁用 `-p OpenCADStudio` 与两个协同 crate 的 `--all-targets`（mission 记录；合并后没有重跑 clippy）。
- 并发编译时首次 `cargo test` 偶发 rustc 崩溃（只有回溯、没有 `error[E…]`），等并发编译结束后原样重跑。
- 整套并行跑时偶发的既有测试（单独跑、整模块跑都通过；不是 mission 的，也不是合并带来的，合并前的日志里就有）：`io::pdf_export::tests::filled_ttf_glyphs_export_as_real_text`、`app::update::plugin_grips_tests::descriptor_moves_refused_by_the_model_keep_the_last_good_design`；另一条线今天还见过 `app::plugin_host::tests::quad_feed_has_no_v14_auto_tab_per_plugin` 与 `scene::resident_patch_shorter_runs_tests::a_layer_switched_off_inside_a_block_patches_the_references_in_place` 各一次。
- git 的全局代理配置指向 `127.0.0.1:7897`，今天那个端口没有监听，`git fetch` 要临时带 `-c http.proxy= -c https.proxy=` 才通（直连可达）。
- mission 留下的进程：三个 `pwsh`（`D:\scratch\m6-claims\start-e2e.ps1`、`start-ttl.ps1`、`start-e2e2.ps1`，分别起于 06:17、06:47、10:58）还活着；`D:\scratch\m6-broadcast\` 下有三个 `.ocs.lock`。本轮没有动它们。

## 7. 中断时的半成品：`collab-client-broadcast`

认领 `VAL-CLI-009`（A 改位号后 B 的本地文档跟着变）、`010`（双方 `base_revision` 同步）、`011`（只读降级后拒绝写）、`012`（只读在界面上可见）、`013`（在线列表随 `peer_joined` 增长）、`018`（同一用户重复连入是两个独立会话）。

改动量：11 个已跟踪文件 +415 / −10，两个新文件共 843 行，新增 21 条单测。

| 文件 | 内容 | 成形程度 |
|---|---|---|
| `src/app/collab/broadcast.rs`（新，477 行） | 纯函数：撤销步的 `(handle, before, after)` → `set_tag` / `set_property` 变更；`op` 载荷的组装与拆解；把变更写回文档 | 成形，12 条单测 |
| `src/app/collab/apply.rs`（新，366 行） | `broadcast_history_delta`（出站钩子）、`apply_remote_changes`（入站合并，每个远端 op 一个撤销步，合并期间不回灌）、`readonly_refuses_command`（只读白名单） | 成形，6 条单测 |
| `src/app/collab/mod.rs` | `CollabEvent::OpApplied`（带 `op_id` 与 `changes`）、`own_ops`、`pending_remote`、`applied_ops`、`request_op`，`/collab/status` 多三个字段 | 成形 |
| `src/app/collab/messages.rs`、`transport.rs` | `op_applied` 解析带上变更列表；出站 `Outbound::Op`、`next_op_id` | 成形，1 条单测 |
| `src/app/history.rs`、`mod.rs`、`update/mod.rs` | 在 `push_undo_entry` 里调出站钩子；`applying_remote` 标志；收到事件后合并远端变更 | 成形 |
| `src/app/commands/mod.rs`、`command_driver/utilities.rs` | 两处命令入口加只读闸门，拒绝时给错误码 `COLLAB_READONLY`；`PIDTAG <旧> <新>` 整行分发 | 成形 |
| `src/app/commands/pidlegend.rs` | `PIDTAG <旧> <新>` 改位号（一个撤销步），找不到旧位号时错误码 `tag_absent` | 成形，2 条单测 |
| `src/pid/persist.rs` | `PidAuthor::from_entity`（从实体快照读，不碰文档） | 成形 |
| `crates/ocs_collab_server/src/http.rs` | 图纸状态改为只读时向整个房间广播 `readonly_downgrade` | 写了，**没有测试** |

暂停时的自查结果（worker 会话记录，在 `2e9e9dab2` 上）：`check` 与 `clippy -p OpenCADStudio` 通过；`--lib collab::` 62 通过；`--lib pidlegend` 21 通过 2 忽略；`ocs_collab` 68 通过；`http.rs` 改完之后只 `build` 过，没跑服务器测试。今天搬到合并后的 `main` 上，这几组结果一样（§10.4）。

端到端探针（`D:\scratch\m6-broadcast\probe-broadcast.ps1`）最后一次运行：准备步骤、`VAL-CLI-013`（两条）、`VAL-CLI-018`（两条）通过；然后在「找到带位号的实体」这一步失败——种子图纸上句柄 391 的 `PID_AUTHOR` 读回是空的——所以 `VAL-CLI-009..012` 一条也没跑到。worker 正在写 `seedcheck.ps1` 查这件事时被暂停。原因未知，可能是探针取数的写法，也可能是种子图保存 / 重开后记录确实不在。

还空着或有疑问的（读代码得出，推断，没有端到端证据）：

1. **`VAL-CLI-011` 的后半句多半过不了。** 契约要求降级后编辑权持有者 `u1` 仍能写。现在服务器把 `readonly_downgrade` 发给房间里的每个会话，客户端收到就无条件 `readonly = true`，持有者自己也被锁住。要么服务器只发给非持有者，要么客户端认出 `holder` 是自己时不降级。
2. **`VAL-CLI-012` 只做了一半。** 协同面板的横幅会显示「只读」，但 `readonly_holder` 只进了状态与 `/collab/status`，没有投影到面板，「编辑权持有者被标出」还没有。
3. **`VAL-CLI-010` 的最后一句多半过不了。** 它要求 `sessions` 表两行的 `base_revision` 都是 2；服务器的 op 分支在提交后只发布 `op_applied`，没有把新修订号写回任何一行会话记录（`sessions.base_revision` 只在建会话、接管、退场、失效这些时刻随整行写入）。需要服务器在 op 生效后刷新同图活跃会话的这一列。
4. 新建或删除的实体不广播（`change_of` 对缺一侧镜像的情况返回 `None`）。六条断言不要求，但 `VAL-CROSS-003`（插阀门后双方拓扑一致）会需要。
5. 一个撤销步里同一实体只取一处变更（位号优先，其次第一个不同的属性）。
6. 交接单、`library\collab-client-broadcast.md`、提交，都还没有。

## 8. 剩下的工作，建议顺序

规模是推断，依据是前面同类 feature 的体量（M6 已交付的三项各 550–1400 行、worker 净用时 2–4 小时）。

| 顺序 | 工作 | 依赖 | 规模 | 断言 |
|---:|---|---|---|---|
| 0 | 修协同传输测试的隔离（§6.1），让整套并行跑稳定 | 无 | 小（几十行） | 不对应断言；M6 门禁 `--lib collab::` 与全量回归受益 |
| 1 | 收尾 `collab-client-broadcast`：定 §7 的 1–3 三点；面板标出持有者；给 `http.rs` 的广播补测试；查清种子图的 `PID_AUTHOR`；跑通端到端；写 library 笔记并提交 | 0（可并行） | 中小（代码大半已有） | `VAL-CLI-009..013`、`018` |
| 2 | M6 里程碑验证：代码评审 + 逐条取证，不过的建修复项 | 1 | 以往每个里程碑 1–3 轮 | `VAL-CLI` 全部 20 条 |
| 3 | `conflict-field-merge`：把 `ws.rs` 的基线硬校验换成三段判定，接上已有的 `conflict.rs` 与 `insert_conflict`，被覆盖的原值进审计 | 服务器侧，不依赖 2（mission 的排法是在 M6 之后） | 中 | `VAL-CONF-001..005` |
| 4 | `conflict-object-topology`：`delete_object`、对象级冲突的通知与审计、拓扑级待裁决条目、`OBJECT_REFERENCED` | 3 | 中到大（服务器要有「对象被谁引用」的概念） | `VAL-CONF-006..009`、`017` |
| 5 | `conflict-resolution`：`/conflicts` 的检索与裁决路由、管理员权限、审计、不可重复裁决 | 4 | 中（存储层函数已有） | `VAL-CONF-010..016` |
| 6 | `cross-area-flows`：十二条端到端流程，缺什么补什么 | 1、5 | 大，且不确定：`008`（断线期间离线编辑、重连对账）、`010`（并发插入的位号唯一性由服务器保证）、`003`（插阀门后双方拓扑一致）很可能要新功能 | `VAL-CROSS-001..012` |
| 7 | M7 里程碑验证 | 3–6 | 同 2 | `VAL-CONF` 17 条、`VAL-CROSS` 12 条 |
| 8 | 收尾：README、最终全量门禁（编排层待办清单的最后一项） | 7 | 小 | — |

穿插要处理的：`Cargo.lock` 的两行自家欠账（§6.3）；另一条线在 `upstream-merge-20261110` 上 `fc37fbaf2` 之后的提交要再并进 `main`（`main` 已经多了合并提交，不能再快进）。

## 9. mission 约定的工作方式（后面照这个来）

### 9.1 构建与测试

每条 cargo 命令都带固定工具链与独立构建目录，用工具链目录里的 cargo，不用 PATH 上那个：

```powershell
$env:RUSTUP_TOOLCHAIN = 'nightly-2026-10-25-x86_64-pc-windows-msvc'   # rustc 1.101.0-nightly
$env:CARGO_TARGET_DIR = 'D:\Rust\target-ocs-mission'
$c = 'D:\Rust\.rustup\toolchains\nightly-2026-10-25-x86_64-pc-windows-msvc\bin\cargo.exe'
& $c check  -p OpenCADStudio --offline
& $c clippy -p OpenCADStudio --offline
& $c test   -p OpenCADStudio --offline --lib collab::
& $c test   -p ocs_collab --offline
git checkout -- Cargo.lock        # 每组 --offline 命令之后立刻还原，绝不提交
```

- 可执行文件在 `D:\Rust\target-ocs-mission\debug\`（`OpenCADStudio.exe`、`ocs_collab_server.exe`）；仓库里 `.\target\debug\` 的是 PATH 上那个 cargo 的旧产物，不要用，也不要停掉在那里运行的实例（可能是用户自己开的）。
- worker 自查范围：`check -p <crate>` + `clippy -p <crate>` + 相关的一个测试目标；不跑 `--workspace` 的全量测试。
- 里程碑门禁：`check --workspace`；`clippy -p OpenCADStudio` 与 `clippy -p ocs_collab -p ocs_collab_server --all-targets`；该里程碑指定的测试目标（M6：`--lib collab::` + `-p ocs_collab`；M7：`-p ocs_collab` + `-p ocs_collab_server`）；`python -m pytest scripts/pid-platform -q`（558 条）。
- 其余基线：`--lib pid::` 96 通过 / 1 忽略；`--test pid_import` 54；`--test pid_legend` 29；`ocs_collab` 68；`ocs_collab_server` 100 + 11。今天合并后这些数都没变。
- 协同服务器必须先 `build` 再直接起 exe（`cargo run` 的进程号是 cargo 自己的，杀掉后真正的服务器成孤儿占着 3240）。
- 测试产物与证据一律放 `D:\scratch\`（`logs\`、`shots\`），不进仓库。

### 9.2 一项功能怎样算完成

1. 读 `features.json` 里这一项，把 `fulfills` 指向的断言逐条读清。
2. 新行为先写会失败的测试，再实现；测试不依赖网络和固定端口（用 `127.0.0.1:0`）。
3. 自查四条命令通过，还原 `Cargo.lock`。
4. 需要时做端到端：直接起服务器与 GUI 实例，按该 feature 自己那几行核对项验证，验证完杀掉自己起的进程。
5. 只暂存自己改的文件（绝不 `git add -A` / `git add .`），提交前看 `.git\MERGE_HEAD`、`CHERRY_PICK_HEAD`、`sequencer` 是否存在；存在就不提交，改为导出补丁。
6. 写交接单：做了什么、没做什么、跑过的命令与观察、交互核对、新增的测试、发现的问题（分 blocking / non_blocking / suggestion）；把实测事实与踩过的坑写进 `library\<feature>.md`。
7. 编排层逐条处置交接单里的问题：写进 `AGENTS.md` / 契约 / library，或者建 `fix-*` feature。

### 9.3 提交说明

中文。标题是「M<n> <主题>：<要点>」，例如「M6 客户端占用申请与显示：PIDCLAIM/PIDRELEASE、协同面板、占用着色」；正文用短横线列出改了什么，最后一段写端到端实测结果。droid 的提交带 `Co-authored-by: factory-droid[bot] …` 尾注。

### 9.4 里程碑验证

一个里程碑的实现全部完成后，先跑代码评审验证器（逐个 feature 评审 + 门禁命令），再跑用户测试验证器（按断言逐条取证，把状态与证据写进 `validation-state.json`）。任何一关不过就建修复 feature、重跑那一关。工具：服务器 HTTP 用 `curl.exe`；服务器 WS 用 Node 的 `.mjs` 脚本；GUI 用 `OpenCADStudio.exe <dwg> --http <port>` 加 REST（`/api/v1/run` 带显式 `document_id`、`/api/v1/capture`、`/api/v1/input`）。断言没有证据不得标 passed。最多 5 个验证器并发，Rust 编译与测试实际是串行的。

客户端协同代码的硬约束（改 `src/app/collab/` 前必读，出处 `AGENTS.md`「客户端线程模型硬约束」）：建连后设 100 ms 读超时；读超时当正常分支；收到 `Close` 立即退出循环；所有读错误都当断开；用 `TcpStream::connect_timeout` 加 `client()`，不用 `tungstenite::connect`；`close` 之后要 `flush`；线程里只能 `try_send`，并区分通道满与通道断开；订阅的 worker 必须是裸 `fn`。

## 10. 2026-11-10 的合并与验证

### 10.1 做了什么

| 步骤 | 结果 |
|---|---|
| 备份半成品 | 见 §11；补丁在干净的 `2e9e9dab2` 上还原出的 11 个文件与工作区逐字节相同（SHA-256），补丁应用后的树与 `git stash create` 的树同为 `7ddbb467…` |
| 半成品落成临时提交 | `backup/m6-wip-pre-merge-20261110` = `170aefb24`（13 个文件，不含 `.tgrep/`） |
| 取远端 | `origin/main`：`1f18cbbc1` → `000814aff`，新增 22 个提交；`fork/main` 没动（`ccfbcaf26`） |
| `main` 快进 | `2e9e9dab2` → `fc37fbaf2`（`upstream-merge-20261110` 当时的尖端：上游合并 `862ddb1e1` 加 7 个 P&ID 备份面板与文档提交） |
| 合并上游新提交 | `dd43f6aa2`「Merge upstream origin/main (000814aff) into the P&ID fork」 |
| 半成品搬回 | `git cherry-pick -n` 后取消暂存，仍是 11 个已修改文件加 2 个未跟踪文件 |

`upstream-merge-20261110` 那个分支与 worktree 没有被改动；那条线之后的提交需要再合并进 `main`。

### 10.2 上游 22 个提交的合并

内容：线型管理器（`-LINETYPE` 新建 / 改名 / 删除、`.lin` 写出、管理器界面）、ACIS 转换搬进新 crate `ocs_acis`（`opencadkernel` `d554319` → `802f751`，特性 `acis` → `brep`）、文字的 positioned glyph 共享与缓存上限、CHAMFER 的 Multiple / Polyline、VPCONTROL、注释性标注保存后保持大小、打印时大圆弧保持圆滑、三个基准测试。107 个文件，+14019 / −471。

冲突 3 个文件、4 处：

| 文件 | 两边各改了什么 | 取法 |
|---|---|---|
| `Cargo.lock` | 上游加 `ocs_acis`；fork 有 `ocs_collab`、`ocs_collab_server` | 都保留 |
| `src/app/update/mod.rs` | 上游加 `linetype_manager` 模块；fork 把 `file` 模块放宽为 `pub(in crate::app)` | 都保留 |
| `src/scene/text/ttf_glyph.rs`（两处） | fork 的 `build_shaped` 只在 `font_system` 锁内整形，轮廓展平与三角化在锁外做；上游在自己的锁内循环里加了 positioned glyph 缓存，`glyphs` 变成 `Vec<Arc<PlacedGlyph>>` | 保留 fork 的结构，把上游的缓存接进锁外的循环：`ShapedGlyph` 多带 `font_id`、`font_weight`，缓存键与上游逐字段相同 |

依赖：`opencadkernel` 的新版本联网取了一次（`cargo fetch`，给 cargo 调用的 git 临时去掉代理）。

拿不准的一处：`ttf_glyph.rs` 的取法是把两边的优化叠在一起，而不是二选一。缓存有自己的锁，只在查找与写入时持有，轮廓计算仍在所有锁之外；缓存只共享「字体、字重、face、字形、缩放、落点」逐位相同的字形，结果应与不共享时逐位一致。文字相关的测试都过了，但没有专门量过它对出图耗时与逐字节结果的影响。

### 10.3 半成品搬回时的冲突

只有一个文件有文本冲突：

| 文件 | 两边各改了什么 | 取法 |
|---|---|---|
| `src/app/command_driver/utilities.rs` | 同一条「或」条件链的末尾，上游加了 `ATTSYNC <块名>` 整行分发，半成品加了 `PIDTAG <旧> <新>` 整行分发 | 两条都保留，上游的在前 |

其余 10 个已跟踪文件自动合并。为跟随上游改名改了 18 处，别的一个字没动（逐行比对：把改名还原后，搬回前后的增删行完全相同）：

- `codec::` → `document::` 17 处：`apply.rs` 6、`broadcast.rs` 3、`pidlegend.rs` 7、`persist.rs` 1。
- `crate::app::document::DeltaSnapshot` → `crate::app::tab_document::DeltaSnapshot` 1 处（`apply.rs`）。

上游改了签名的那几个函数，半成品都没有调用。

### 10.4 验证

用的是 mission 的约定：工具链 `nightly-2026-10-25`（rustc 1.101.0-nightly）目录里的 cargo，`CARGO_TARGET_DIR=D:\Rust\target-ocs-mission`，`--offline`，`-j 10`。测试是 `cargo test --workspace --no-run` 之后逐个运行 20 个测试程序（另一条线今天用 stable 1.99.1 时是 18 个；多出的两个是上游新 crate `ocs_acis` 的单测与集成测试）。

| 状态 | `build --workspace` | `test --workspace --no-run` | 20 个测试程序合计 | 失败的 |
|---|---|---|---|---|
| 只含合并（`dd43f6aa2` 的内容，提交前） | 通过，356 秒 | 通过，274 秒 | **6465 通过 / 1 失败 / 43 忽略** | 传输组 1 条 |
| 合并 + 半成品 | 通过，144 秒 | 通过，186 秒 | **6483 通过 / 4 失败 / 43 忽略** | 传输组 3 条 + `filled_ttf_glyphs_export_as_real_text` |

对照：另一条线在 `925728489` 上的全量结果是 6313 / 3 / 43（3 条都是传输组）。多出来的通过数来自上游 22 个提交与备份面板后续提交新增的测试。合并 + 半成品比只含合并多的 18 条通过，是半成品自己的 21 条新单测扣掉这一次多失败的 3 条。

失败项的归类：

| 测试 | 归类 | 依据 |
|---|---|---|
| `app::collab::transport::tests::` 下的四条真 socket 测试（每次整套跑失败 1–4 条） | mission 自己的测试隔离问题，合并前就有；不是合并或半成品带来的 | §6.1；单独跑 14/14，`collab::` 整组 62/62 |
| `io::pdf_export::tests::filled_ttf_glyphs_export_as_real_text` | 并行时偶发的既有测试 | 单独跑 3/3、整模块 20/20 通过；另一条线合并前在 `2e9e9dab2` 上的一次运行里就失败过 |
| `app::update::plugin_grips_tests::descriptor_moves_refused_by_the_model_keep_the_last_good_design`（后面多跑的三遍里出现一次） | 并行时偶发的上游测试 | 单独跑 3/3、整模块 43/43 通过；`2e9e9dab2` 与上游都有这条测试 |

合并 + 半成品的 lib 测试程序整套多跑了三遍，每遍都是 5911 通过 / 4 失败 / 41 忽略：传输组 3–4 条，外加至多一条上表里的偶发项。

半成品相关的分组，在合并后的 `main` 上与暂停前在 `2e9e9dab2` 上的结果一致：

| 分组 | 结果 |
|---|---|
| `--lib collab::` | 62 通过 |
| `--lib pidlegend` | 21 通过 / 2 忽略 |
| `--lib pid::` | 96 通过 / 1 忽略 |
| `ocs_collab` | 68 通过 |
| `ocs_collab_server` | 100 + 11 通过 |
| `--test pid_import` / `pid_legend` | 54 / 29 通过 |
| `python -m pytest scripts/pid-platform -q` | 558 通过 |

没有做的验证：`clippy`；快进之后、合并上游新提交之前那个状态（`fc37fbaf2`）没有单独构建，它的依据是另一条线自己的测试记录；任何要开窗口的端到端验证（GUI 实例、协同服务器、双实例）；wasm 目标的检查。

## 11. 备份与回退

| 位置 | 内容 |
|---|---|
| `D:\work\plant-code\cad\ocs-mission-backup-20261110\` | `git-status-short.txt`、`HEAD.txt`、`m6-wip.worktree.patch`（合并前的半成品补丁，基于 `2e9e9dab2`）、`m6-wip.index.patch`（空，当时没有暂存内容）、`untracked\`（两个新文件的原样拷贝）、`wip-sha256.txt`；搬回之后的版本：`m6-wip.post-merge.worktree.patch`、`untracked-post-merge\`、`wip-post-merge-sha256.txt`；`logs\`（构建与测试日志、运行脚本）、`analysis\`（分析脚本与各目录体积） |
| 分支 `backup/m6-wip-pre-merge-20261110` | `170aefb24`：`2e9e9dab2` 加半成品的全部 13 个文件 |
| `refs/backup/m6-wip-20261110` | `b3601266d`：`git stash create` 的快照（合并前，只含已跟踪文件的改动） |
| `refs/backup/main-pre-merge-20261110` | `2e9e9dab2`：合并前的 `main` |
| `refs/backup/m6-wip-post-merge-20261110` | `fda58d013`：搬回并改名之后的快照（基于 `dd43f6aa2`，只含已跟踪文件的改动） |
| `refs/backup/main-post-merge-20261110` | 本文所在的提交，即合并后的 `main`（回退之后还想回来时用） |

回到合并前的样子（`main` 在 `2e9e9dab2`，半成品是未提交改动）。这两条会丢掉主检出里当前未提交的内容，那些内容在上表里都有备份：

```powershell
git -C D:\work\plant-code\cad\OpenCADStudio reset --hard backup/m6-wip-pre-merge-20261110
git -C D:\work\plant-code\cad\OpenCADStudio reset --mixed refs/backup/main-pre-merge-20261110
```

第一条把分支、索引、工作区都放到「`2e9e9dab2` + 半成品」，第二条把分支指回 `2e9e9dab2` 而保留工作区，两个新文件随之变回未跟踪。`main` 没有 push 过，这样做不影响任何远端。

## 12. 数字的出处与复算

- feature 的数量与状态、每条断言的认领关系、断言状态分布：`features.json` 与 `validation-state.json`。分析脚本在备份目录的 `analysis\`：`mission_report.py features | fulfills | vstate | timeline`、`check_features.py`、`handoff_issues.py issues | dismiss`。
- 断言总数与分区：数 `validation-contract.md` 里以 `### VAL-` 开头的标题。
- 时间线与提交号：`progress_log.jsonl` 的 `worker_completed` 记录（时间是 UTC，本文换成了本地时间）；与 `git log --first-parent fork/main..2e9e9dab2` 对照。
- 暂停时 worker 跑过的命令与结果：会话文件 `C:\Users\dpc\.factory\sessions\-D-work-plant-code-cad-OpenCADStudio\fac4a5df-….jsonl`（`analysis\session_grep.py`、`session_tail.py`）。
- 构建与测试：备份目录 `logs\` 下的 `s2-*`（只含合并）、`s3-*`（合并 + 半成品）、`reruns\`、`transport-experiments\`；`run-step.ps1`、`run-tests.ps1`、`sum-tests.ps1` 可以原样重跑。
- 另一条线的数字（6313 / 3 / 43、合并前后的偶发失败）：`D:\work\plant-code\cad\ocs-upstream-20261110\target\stage1-logs\` 下的 `final-test.log`、`base-test-lib-run*.log`、`merged-test-lib-run*.log`，只读。
