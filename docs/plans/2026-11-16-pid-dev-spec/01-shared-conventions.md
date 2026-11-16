# 共用约定

> 状态：**已确认**（2026-11-16，随各模块规格一起确认）。基线 `main @ 9b5327f96`。
> 本文是各模块规格（`SPEC-<模块 id>.md`）共用的部分：技术栈、命令、目录、代码风格、测试策略、边界，以及所有模块都遵守的数据约定。模块规格只写自己特有的内容，与本文冲突时以模块规格为准并在那里写明原因。

## 1. 技术栈

| 部分 | 用什么 | 出处 |
|---|---|---|
| 语言 | Rust，edition 2021；本机工具链 `rustc 1.101.0-nightly (2026-11-04)`，仓库里没有 `rust-toolchain` 文件 | `Cargo.toml` |
| 桌面端 | `OpenCADStudio` 2026.46.0；界面用 `iced`（本地路径依赖 `../iced`，只读）；图纸模型用 `document`（`opencaddocument`，git `ae40a59`） | `Cargo.toml` |
| 协同客户端 | `tungstenite` 0.29，同步，跑在独立的 `std::thread` 上；桌面端不引入异步运行时 | `src/app/collab/transport.rs` |
| 协同协议 | `ocs_collab` 0.1.0：`serde`、`serde_json`、`uuid`、`chrono` 0.4，没有 IO | `crates/ocs_collab` |
| 协同服务器 | `ocs_collab_server` 0.1.0：`axum` 0.8（含 WebSocket）、`tokio` 1、`rusqlite` 0.39（自带 SQLite） | `crates/ocs_collab_server` |
| 阶段 3 可直接用的现有依赖 | `serde_json`、`regex` 1（规则里的模式匹配）、`sha2` 0.10（发布清单校验和）、`zip` 8（写表格文件）、`calamine` 0.36（读 Excel） | `Cargo.toml` |

阶段 3 默认不加新的依赖，需要的能力现有代码里都有：

- **写表格文件**：`src/app/annotation_data.rs` 的 `write_extraction_file(path, rows)` 按扩展名写 `.xlsx`（单工作表、全部是文本单元格）、`.csv`、`.txt`，输入是「行的列表，每行是字符串列表」。材料清单直接复用，不另写。
- **出图**：用通用的出图管线（自动化操作 `plot`，按扩展名写 PDF 或 SVG；`src/io/pdf_export.rs`、`src/io/svg_export.rs`）。**不是 `PIDPLOT`**——那个命令只给 SmartPlant 的 `.pid` 文件出图，读的是磁盘上的 `.pid`，与正在编辑的 DWG 无关。
- **不开界面读一张图**：`crate::io::load_file(path)` 返回 `Document`，项目索引扫描同目录图纸时用。
- **发布包里的图纸文件**：把磁盘上那个文件原样复制一份（`std::fs::copy`），不重新保存（见 `SPEC-pid-publish.md`）。`crate::io::save_as_version(doc, path, version)` 是「另存一份」，会先过保存闸，发布不用它。
- **时间**：桌面端没有直接依赖日期时间库（`chrono` 只经 `ocs_collab` 间接带入），时间一律记 Unix 秒（见 §7.4），取当前时间用标准库 `std::time::SystemTime`。

确需加依赖时先问。

## 2. 命令

都在仓库根目录执行，Windows PowerShell 5.1（没有 `&&`，用 `;`）。

```powershell
# 桌面端全部单元测试
cargo test --offline --locked --lib

# 协同两个 crate 的测试
cargo test --offline --locked -p ocs_collab -p ocs_collab_server

# 只跑一组测试（按名字过滤）
cargo test --offline --locked --lib collab_override_marks

# 性能预算（带 #[ignore] 的测试）
cargo test --offline --locked --lib -- --ignored <测试名>

# 静态检查：只看告警数有没有增加
cargo clippy --offline --locked --lib
cargo clippy --offline --locked --lib --profile test

# 启动桌面端并开本机 REST（每个请求要带 Authorization: Bearer <令牌>）
cargo run --offline --locked -- --http 8090

# 启动协同服务器
cargo run --offline --locked -p ocs_collab_server -- --port 3240 --db <库文件> --seed <种子 JSON>
```

门禁读数（`9b5327f96` 时）：

| 门禁 | 读数 | 说明 |
|---|---|---|
| 桌面端单元测试 | 6918 条：6865 通过，49 忽略，4 失败 | 4 条失败是比对设计文件的测试，因为工作区里那份 `pid-platform.pen` 有未提交的改动；只要失败的还是这 4 条就算通过 |
| `clippy` 告警数 | 630；带测试 965 | 不许增加 |
| 协同 crate 测试 | `ocs_collab` 95；`ocs_collab_server` 159 + 12 | 全过 |
| 格式 | 改动的文件不新增 `rustfmt` 不一致 | 仓库有存量不一致，所以比的是「新增」；仓库里的脚本 `scripts\fmt-newhunks.ps1 <改前> <改后>`（原来在仓库外，`CC-5` 收进来的） |
| 平台规格包 | `pytest scripts/pid-platform` 558 条 | 只在动了 `docs/pid-platform/` 时跑；本方案不动那个目录 |

端到端场景脚本不在仓库里，在本机 `D:\scratch\m6-validation\` 及其复验目录（一个场景一个 `.py`，`run_all.ps1` 顺序跑）。它们起真实的桌面端与服务器进程，走 `--http`。

## 3. 目录

```
src/pid/                      P&ID 语义层：纯数据 + 纯函数，不依赖界面
  persist.rs graph_data.rs    图内记录的读写
  topology.rs propagate.rs    拓扑表、属性下传与保存闸
  type_lib.rs tag_rule.rs tag_engine.rs symbols.rs
  master_data.rs project_index.rs              ← 待建
  drc/{mod,model,dsl,facts,builtin_rules,runner}.rs  ← 待建
  bom.rs snapshot.rs publish_pkg.rs            ← 待建
src/app/collab/               协同客户端（传输、会话、发出与落地）
src/app/commands/pid*.rs      P&ID 命令（待建：pidmasterdata.rs、pidindex.rs、piddrc.rs、pidbom.rs、
                              pidsnapshot.rs、pidpublish.rs、pidverify.rs）
src/app/command_driver/utilities.rs  命令行执行；带参数的命令要在这里登记成「整行当一条命令」
src/app/control/mod.rs        自动化操作的总分发 control_request：REST、MCP、标准输入都落到这里
src/app/automation.rs         较早的一组操作处理函数（如 pid_legend），由上面转过来
src/rest.rs  src/rest_openapi.json  src/mcp.rs
src/ui/dock.rs  src/ui/window/pid_*.rs         停靠面板（待建：pid_verify_deliver.rs）
crates/ocs_collab/            协议、判定、权限、占用（没有 IO）
crates/ocs_collab_server/     服务器（HTTP、WebSocket、SQLite）
assets/pid-types.json  assets/pid-symbols.json  内置类型库与符号库（待建：pid-drc-rules.json）
locales/*/opencadstudio.ftl   语言包
docs/plans/                   计划与设计文档；本方案在 2026-11-16-pid-dev-spec/
```

## 4. 代码风格

照现有代码写。下面是 `src/pid/persist.rs` 里的一段，新代码向它看齐：

```rust
/// 把记录写进 `handle`。先判定长度，超限才把 `props` 挪进 XRecord。
pub fn store(&self, doc: &mut Document, handle: Handle) {
    let mut values = encode(self);
    if encoded_len(&values) > XDATA_SOFT_LIMIT {
        let key = props_ref_key(handle);
        store_props_record(doc, &key, handle, &self.props);
        values.retain(|value| {
            as_str(value).is_none_or(|text| !text.starts_with("props="))
        });
        values.push(XDataValue::String(format!("{PROPS_REF_KEY}={key}")));
    }
    set_entity_xdata(doc, handle, PID_AUTHOR_APP, Some(values));
}
```

- `src/pid/` 下的模块是纯数据加纯函数，不碰 `iced`，不碰场景；只有读写图内记录的函数接 `&Document`。这样没有界面也能测。
- 落盘键名、报文键名用常量，不靠顺序拼。
- 错误是枚举，每个变体有一个 `code()`，返回大写下划线的契约错误码（如 `TAG_DUPLICATED`）。断言与日志认错误码，不认文案。
- 需要稳定顺序的地方用 `BTreeMap` / `BTreeSet`，写出去的 JSON 与文件可以逐字节比对。
- 注释写约束与原因，不复述代码。注释语言跟所在文件：`src/pid/` 下是中文，`src/app/collab/` 与两个 crate 是英文。
- 给用户看的文案是中文；属性名就是中文的数据要素名（`管道等级`、`公称直径`），不译成英文。
- 测试名是一句话，说清场景与结论，如 `collab_override_marks_a_peers_run_keeps_the_field_changed_by_hand`。

## 5. 测试策略

| 层 | 放在哪里 | 测什么 |
|---|---|---|
| 单元 | 同文件的 `#[cfg(test)] mod tests` | 纯函数：编码解码、判定、聚合、规则求值；每种数据格式都有往返测试（写出去再读回来相等） |
| 两个客户端经服务器判定 | `src/app/collab/collab_*.rs` | 一端的改动经真实的判定逻辑到另一端，不起进程、不走网络 |
| 服务器 | 两个 crate 各自的 `tests` 与模块内测试 | 路由、库表、受理管线、重启后重放 |
| 性能预算 | `#[ignore]` 测试，写法照 `src/pid/perf_budget.rs`（取 3 次的中位数） | 2,000 对象规模的耗时 |
| 端到端 | 本机场景脚本，走 `--http` | 真实进程下的整条流程 |

每个任务先写会失败的测试，看到它因为要测的断言而失败，再实现到通过；红、绿两次的输出都留作证据。每个任务完成时跑 §2 的全部门禁。

## 6. 边界

**总是做**

- 先测试后实现；每个任务单独提交，提交前跑全部门禁。
- 新的数据格式先写进模块规格再实现；实现与规格有出入时，在规格的「实现记录」一节记下来。
- 新格式带 `version` 字段；读到不认识的键忽略，读到更高的 `version` 报错而不是猜。
- 写文件先写到临时文件再改名，失败不留半成品。

**先问再做**

- 推送到远端。
- 加新的依赖。
- 给协同协议加消息类型，或改已有键的含义（加可选键不用问）。
- 改图内记录的既有键、四个图级键的布局，或服务器库表的既有列。
- 动 `docs/pid-platform/` 下的文件。

**绝不做**

- 提交或改动 `docs/pid-platform/design/pid-platform.pen`。
- 改 `../iced`；引入 Docker、Postgres、Redis 等外部服务；用 `sccache`。
- 在桌面端传输层引入异步运行时。
- `git stash`、`git add -A`；为了让测试通过而删测试或放宽断言。
- 在设置文件、日志、报文里留口令或令牌明文；协同服务器地址不接受带用户名密码的写法。

## 7. 共用数据约定

### 7.1 对象怎么称呼

- **位号是全局连接键。** 跨图拓扑、材料清单、规则检查结果、协同报文、占用、审计都按位号找对象。
- **唯一键是 `(类型编码, 位号)`。** 不同类型允许同值位号（管线 `L-1001` 与仪表回路 `L-1001` 互不相干）。协同报文里的 `object_ref` 目前只带位号文本，这是已知的收窄，见 `SPEC-collab-protocol.md`。
- **没有位号的东西按「属于谁 + 在哪里」称呼**：管段按所属管路与端点坐标，连接符按所在管线与流向。
- **句柄不当标识。** 实体句柄只在一张图的一份副本里有效，两台机器上同一个对象的句柄不同。报文、跨图索引、快照、清单里不用句柄做标识（图纸摘要和检查结果里带的 `handle` 只是给本机定位用的提示，比对和匹配一律不看它）；图内记录里的句柄写成大写十六进制字符串（如 `"2A4"`）。

### 7.2 坐标与数值

- 坐标是模型空间的世界坐标，写成 `[x, y]` 两个数。比较两个点是否同一处用容差 `1e-6`（`src/app/collab/pipes.rs` 的 `SAME_PLACE`）。
- 图内记录里的坐标不用会随实体变换自动换算的扩展数据类型，只用字符串或整数，由读写模块自己解释（`src/pid/persist.rs` 文件头第 1 条）。
- 属性值一律是字符串；空字符串等于没有这个属性。要按数值比较时（规则、清单）由使用方解析，解析不出来按「未知」处理，不当成 0。

### 7.3 JSON 与文件

- JSON 的结构键用英文 `snake_case`；属性名、枚举的业务取值（角色、图纸状态、裁决方式）保持中文原样。
- 文件一律 UTF-8。给程序读的 JSON 不带 BOM；给人用表格软件打开的 CSV 带 BOM（见 `SPEC-pid-bom.md`）。
- 写给人看的 JSON 缩进 2 格、键按固定顺序，同样的输入产出逐字节相同的文件。

### 7.4 时间

时间戳是 Unix 秒（整数，UTC），键名以 `_at` 结尾（`expires_at`、`checked_at`、`created_at`）。协同协议与服务器库表已经这样用，阶段 3 的新格式沿用。界面显示时再换算成日期时间文字；项目里没有日期库、标准库也拿不到本机时区，所以按 UTC 显示并标明 `UTC`（现有的 `src/ui/window/xref_manager.rs` 显示文件日期也是这样算的）。

### 7.5 错误码

大写下划线，跨通道不变：命令行提示、REST 应答、日志里是同一个码。各模块的错误码表写在各自的规格里；新模块优先用需求文档已经起好的名字（`RULE_BLOCKED`、`PUBLISH_BLOCKED`、`BOM_DUPLICATE_TAG`、`DSL_PARSE_ERROR` 等）。

### 7.6 桌面端的对外接口怎么加

桌面端对外有四个入口：命令行命令、REST（`--http`）、MCP、标准输入的自动化通道。后三个都落到 `src/app/control/mod.rs` 的 `control_request`，按操作名 `op` 分发。请求是 `{"protocol":1,"op":"…", …}`，应答是带 `"ok"` 的 JSON 对象，失败时另带 `code`（错误码）与 `error`（说明）。新加一个能力，固定改下面五处：

1. **内部函数**。逻辑写在 `src/pid/` 的纯函数里；`src/app/commands/pid*.rs` 里写一个薄入口。命令、面板按钮、自动化操作都调这一个入口，不各写一份。
2. **命令**。`src/app/commands/mod.rs` 的分发链加一行 `dispatch_pidxxx`，同文件的命令名单加命令名。只读会话里允许用的命令，还要加进 `src/app/collab/apply.rs` 的 `READONLY_ALLOWED`——那是一份明确的允许名单，名单外的命令在只读会话里一律被拒。**带参数的命令**还要在 `src/app/command_driver/utilities.rs` 的 `run_command_line_impl` 里登记成「整行当一条命令」（照 `PIDCOLLAB`、`PIDSETPROP` 那几行）：不登记的话，命令行遇到「命令词 + 参数」会先拿不带参数的命令词试跑一遍，再跑整行——`PIDSNAPSHOT SAVE …` 会存出两份快照，`PIDPUBLISH <目录>` 会先往默认目录发布一次。阶段 3 的九个命令词（`PIDMASTERDATA`、`PIDINDEX`、`PIDLOCATE`、`PIDCHECK`、`PIDBOM`、`PIDSNAPSHOT`、`PIDDIFF`、`PIDPUBLISH`、`PIDVERIFY`）都要登记。
3. **自动化操作**。在 `control_request` 里加操作名（如 `pid_check`）。只读的操作加进它的「查询类」名单（自动化被停用时也能用，`pid_legend`、`collab_status` 就在里面）；会改图或写文件的加进改图分发（写法照 `pid_insert_batch`）。能力清单 `capabilities` 里加一项。
4. **REST**。`src/rest.rs` 的 `plan` 路由表加一条资源式路径（如 `("POST", ["pid", "check"])`，即 `POST /api/v1/pid/check`）：只读的走 `Plan::Run`，会改图或写文件的走 `Plan::Mutate`。同文件的 `map_status` 把错误码映射成 HTTP 状态码，没列的一律 400；新错误码要在这里定状态码。`POST /api/v1/{op}` 直通写法自动可用。`src/rest_openapi.json` 补接口说明。
5. **MCP**。MCP 不是一个能力一个工具，而是 `ocs_read`、`ocs_execute` 两个门面工具各带一份操作名单（`src/mcp.rs` 的 `READ_OPS`、`EXECUTE_OPS`）。只读操作加进 `READ_OPS`，其余加进 `EXECUTE_OPS`，参数补进对应门面工具的输入说明。工具面有一个钉死的摘要 `TOOL_SCHEMA_DIGEST`，改了工具面必须同步更新它（测试 `tool_schema_digest_is_pinned` 把关）。
阶段 3 新增的错误码统一这样映射：「请求没错，是图纸当前的状态不允许」的（`PUBLISH_BLOCKED`、`RULE_BLOCKED`、`BOM_DUPLICATE_TAG`）返回 409，改好图后同一个请求就能成功；请求本身有错的（`DSL_PARSE_ERROR`、参数不对）返回 400。

## 8. 术语

| 术语 | 意思 |
|---|---|
| 位号 | 对象在项目里的编号，如 `P-1001`；与类型编码一起组成唯一键 |
| 类型编码 | 对象的类型，大写英文，如 `PUMP`、`VALVE_GATE`、`PIPE_LINE` |
| 管线 / 管路 / 管段 | 管线（如 `L-1001`）下有若干管路（`L-1001-01`），管路由管段与管件组成；管段没有位号 |
| 管件 | 吸附在管段之间的阀门、法兰、异径管等；阀门有自己的位号，其余随所属管路 |
| 页间连接符 | 把两张图上的同一条线联起来的符号，分流出与流入 |
| 管级属性 | 从管线下传的五个属性：`管道等级`、`公称直径`、`绝热等级`、`流体介质`、`管线类型` |
| 覆盖标记 | 管路上某个管级属性被手工改过的记号；带记号的字段不再跟随管线 |
| 图内记录 | 存在 DWG 文件内部的 P&ID 语义数据：每个实体上的 `PID_AUTHOR` 扩展数据，和图级的 XRecord |
| `op` | 协同时一个客户端发给服务器的一次改动；一个撤销步对应一个 `op` |
| 修订号 | 服务器给一张图的 `op` 排的序号，每生效一个加 1 |
| 占用 | 一个会话对一个位号的临时独占编辑权，到期自动释放 |
| 规则检查 | 设计规则检查，英文缩写 DRC |
| 材料清单 | 英文缩写 BOM |
