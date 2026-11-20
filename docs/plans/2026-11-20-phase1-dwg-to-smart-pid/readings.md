# 阶段一读数记录（DWG 转智能 P&ID，导出 `.pid` 与 publish XML）

> 状态：**进行中**。本文件记录阶段一（[happyrust/pid-cad#106](https://github.com/happyrust/pid-cad/issues/106)）各任务的门禁基线与实测读数。
> 所属目录：`docs/plans/2026-11-20-phase1-dwg-to-smart-pid/`。
> 约定：原始日志、报告全文与导出产物保存在仓库外证据目录（`$OCS_PID_EVIDENCE_DIR`），本文件只记录可复现的数字、结论、原因与证据文件名，不包含客户数据、盘符、用户名或主机名。

---

## 1. L0-6（#109）：当前 `main` 门禁基线读数与差异说明（2026-11-20）

| 项 | 值 |
|---|---|
| 测量日期 | `2026-11-20` |
| 本仓库提交 | `6377fe2334da303bf390a38aae1469260f870eca`（入库提交 `9b5faf294`） |
| `pid-parse` 钉定提交 | `a85fa10285cc5379953495fa391f374981b9e587`（`Cargo.toml` 6 处 git 依赖） |
| 编译器版本（`rustc -V`） | `rustc 1.102.0-nightly (65ce8611f 2026-11-18)` |
| 证据目录子项 | `2026-11-20-issue109/`（`git-head.txt`、`rustc-version.txt`、`cargo-test-lib.txt`、`clippy-lib.txt`、`clippy-lib-test.txt`） |

### 1.1 四组核心门禁读数与配套基线

| 门禁项 | 命令 | 实测读数 | 后续任务「不退化」判据 |
|---|---|---|---|
| 编译器版本 | `rustc -V` | `rustc 1.102.0-nightly (65ce8611f 2026-11-18)` | 每次读数同记版本；若工具链变更须在同一提交先重测基线 |
| 库单元测试 | `cargo test --offline --locked --lib` | 共 **`7,145` 条**：**`7,089` 通过**、**`55` 忽略**、**`1` 失败**（耗时 `55.91s`） | 总条数 `>= 7,145`；失败条数不增加（含未提交设计稿工作区 `<= 1` 且仅限下述字重测试，干净检出为 `0`） |
| 库 `clippy` | `cargo clippy --offline --locked --lib` | `OpenCADStudio (lib)`：**`656` 条警告**（含 `4 duplicates`） | 同一编译器下 `<= 656`（零新增警告） |
| 测试配置 `clippy` | `cargo clippy --offline --locked --lib --profile test` | `OpenCADStudio (lib test)`：**`1,051` 条警告**（含 `4 duplicates`） | 同一编译器下 `<= 1,051`（零新增警告） |
| 分层检查（配套） | `python scripts/arch_check.py report` | `33 module pairs, 183 files`；`io` 起始四行：`io -> app 3`、`io -> modules 1`、`io -> patreon 1`、`io -> ui 1` | `io` 起始各行数字不增大 |
| `scripts/sppid` 测试（配套） | `python -m pytest scripts/sppid -q -p no:cacheprovider` | **`159 passed`** | 全绿，条数只增不减 |

补充说明：

- **唯一 1 条库测试失败归因**：`ui::style::pid_fonts::tests::the_faces_are_the_weights_the_design_uses`（工作区存在按约定不入库的本地 `docs/pid-platform/design/pid-platform.pen` 修改，设计稿中多出 `("JetBrains Mono", 500)` 一档字重；干净 `HEAD` 检出下该测试通过，全库 `0` 失败）。
- **同工作区子包 `clippy` 固定读数**：两次 `clippy` 中工作区子 crate 的既有警告恒为 `ocs_acis (lib)` `12` 条、`ocs_plugin_api (lib)` `4` 条、`iced_wgpu (lib)` `1` 条。

### 1.2 与备份包计划旧基线（2026-11-18，`64401e1ac` / `c399311e7`）的逐项差异归因

| 指标 | 旧基线（2026-11-18） | 当前基线（2026-11-20） | 变化量 | 逐项原因 |
|---|---:|---:|---:|---|
| 库测试总数 | `6,954` | `7,145` | `+191` | ① 提交 `bee5c33d1`（去掉私有依赖 `e3d-model` / `e3d-io` / `e3d-attlib` 及 `src/io/e3d/{producer,open}.rs`）移除 `9` 条测试；② 备份包读取与显示（`RD-1..14`）、PDF 导出（`PDF-2..7`）、publish 导出（`EXP-1..2`、`aba66b75a`）、自动化通道修复（`f4314f1af`）、默认亮色迁移（`6523d84d9`）及检查点 C 界面与审核修复（`UI-1..9`、`6377fe233`）合计新增 `200` 条测试（`+200 - 9 = +191`） |
| 库测试通过 / 忽略 | `6,901` 过 / `49` 忽略 | `7,089` 过 / `55` 忽略 | `+188` / `+6` | 新增的 `200` 条测试中有 `6` 条为依赖外部真实样本的 `#[ignore]` 测试；另有旧基线中 `3` 条原失败测试转为通过（`6,901 - 9 + 194 + 3 = 7,089`） |
| 库测试失败 | `4` | `1` | `-3` | 旧基线 `4` 条失败均源于工作区未提交的 `pid-platform.pen`；检查点 C（`UI-3..UI-9`）对齐外壳导航与设计变量后消除其中 `3` 条，仅剩 `pid_fonts` 字重核对 `1` 条 |
| `clippy --lib` | `630` | `656` | `+26` | ① 编译器由 `1.101.0-nightly` 升级至 `rustc 1.102.0-nightly (65ce8611f 2026-11-18)`，新 lint 规则使同一代码由 `630` 升至 `660`（`+30`）；② `bee5c33d1` 移除 `e3d` 两个模块减少 `4` 条既有警告（`660 - 4 = 656`）；此后所有提交新增警告为 `0` |
| `clippy --lib --profile test` | `965` | `1,051` | `+86` | ① 同上编译器升级使同一代码由 `965` 升至 `1,058`（`+93`）；② `bee5c33d1` 移除 `e3d` 测试减少 `7` 条既有警告（`1,058 - 7 = 1,051`）；此后所有提交新增警告为 `0` |

---

## 2. L0-2（#107）：语料索引与 SHA-256 校验工具 `corpus_index.py`（2026-11-20）

| 项 | 值 |
|---|---|
| 测量日期 | `2026-11-20` |
| 入库提交 | `538ec0112`（基线 `b87839522`） |
| `rustc -V` | `rustc 1.102.0-nightly (65ce8611f 2026-11-18)` |
| 新增文件 | `scripts/sppid/corpus_index.py`、`scripts/sppid/corpus-layout.md`、`scripts/sppid/test_corpus_index.py` |
| 证据目录子项 | `2026-11-20-issue107/`（`red.txt`、`green.txt`、`corpus.manifest.json`、`real-index.txt`、`real-verify.txt`、`tamper-verify.txt`、`unset-env.txt`） |

### 2.1 先红后绿与单测门禁

- **先红验证（`red.txt`）**：先落盘 `test_corpus_index.py` 8 条断言测试对最小骨架运行，`8 failed in 0.39s`（全部死在预期 `AssertionError`）。
- **实现后全绿（`green.txt`）**：`python -m pytest scripts/sppid -q -p no:cacheprovider` -> **`167 passed in 1.87s`**（原基线 `159` 条 + 新增 `8` 条全过）。

### 2.2 真实语料清单实测与全部验收项核对

| 验收项 | 实测结果 |
|---|---|
| 真实语料 `index` + `verify` | 通过外置 `--manifest` 只读扫描真实语料根目录（零改动源目录文件），索引 **`1,115` 个文件**（共 `406,863,955` 字节），`manifest_sha256` = `487b320319a9dfad20b1f9b70c8647ad2885ece4d2f8d3a217f71c88b9112038`；紧接执行 `verify` 输出 `verify OK: 1115 file(s) matched`，退出码 `0` |
| 各逻辑段文件统计 | `EXPORT_SET`：`282` 个文件（94 套 `.pid` + `_Data.xml` + `_Meta.xml`，`280,588,766` 字节）；`EXPORT_DXF`：`94` 个文件（`83,816,462` 字节）；`EXPORT_PDF`：`94` 个文件（`8,395,931` 字节）；`SYMBOLS`：`645` 个文件（644 个 `.sym` + 1 个 `CatalogIndex.xml`，`34,062,796` 字节） |
| 单字节篡改检出（`tamper-verify.txt`） | 翻转 1 个字节后 `verify` 退出码 `1`，精确输出 `MISMATCH [EXPORT_SET] export-set/01/sample_Data.xml (expected sha256=..., actual sha256=...)` |
| 未设 `OCS_PID_CORPUS_DIR`（`unset-env.txt`） | 退出码 `0`，输出 `未设置语料目录（OCS_PID_CORPUS_DIR），跳过语料清单操作。` |
| 清单零绝对路径审计 | 扫描 `corpus.manifest.json` 全文：反斜杠 `\` 计数 `0`、盘符冒号计数 `0`、绝对根路径计数 `0`；传入 `../outside` 或带盘符映射时返回退出码 `2` |

---

## 3. L4-0（#111）：publish XML 格式与结构校验器（2026-11-20）

| 项 | 值 |
|---|---|
| 测量日期 | `2026-11-20` |
| 基线提交 | `538ec0112`（`main`） |
| `rustc -V` | `rustc 1.102.0-nightly (65ce8611f 2026-11-18)` |
| 语料清单摘要 | `manifest_sha256 = 487b320319a9dfad20b1f9b70c8647ad2885ece4d2f8d3a217f71c88b9112038` |
| 新增 / 修改文件 | `assets/pid-publish-spec.json`、`src/io/mod.rs`、`src/io/pid_publish_check.rs`、`scripts/sppid/xml_spec_check.py`、`scripts/sppid/test_xml_spec_check.py` |
| 证据目录子项 | `2026-11-20-issue111/`（`red-rust.txt`、`red-py.txt`、`green-rust-fast.txt`、`green-rust-real220.txt`、`green-py.txt`、`real-xml-check.txt`、`gate-cargo-test-lib.txt`、`gate-clippy-lib.txt`、`gate-clippy-lib-test.txt`、`fmt-io-mod.txt`、`fmt-check.txt`、`arch-after.txt`） |

### 3.1 先红后绿与全批真实样本校验

| 检查项 | 实测结果 |
|---|---|
| 零客户数据审计 | `assets/pid-publish-spec.json`（`65,090` 字节）100% 纯 ASCII 模式标识，零非 ASCII 字符、零十六进制 UID 实例，覆盖 `21` 类 Data 对象、`15` 种 Data 关系、`3` 类 Meta 对象与 `3` 种 Meta 关系 |
| Rust 端先红后绿 | 先红 `red-rust.txt`：`1 passed; 9 failed; 1 ignored`（退出码 `101`，全部命中目标断言）→ 实现后 `green-rust-fast.txt`：**`10 passed; 0 failed; 1 ignored`**（退出码 `0`） |
| Python 端先红后绿 | 先红 `red-py.txt`：`3 failed, 1 passed`（退出码 `1`）→ 实现后 `green-py.txt`：`python -m pytest scripts/sppid -q -p no:cacheprovider` **`171 passed`**（原 `167` 条 + 新增 `4` 条全过，退出码 `0`） |
| 全批真实 SPPID XML 样本（`StrictVendor` 默认模式） | 对三批真实 SPPID 导出样本全量 **110 对（`220` 份 `_Data.xml` + `_Meta.xml`，含 `corpus.manifest.json` 中 `EXPORT_SET` 的 94 对 188 份 + 11 对厂商参考数据 22 份 + 5 对验证数据 10 份）**逐份校验：Rust 端 `green-rust-real220.txt` 输出 **`checked 220 XML file(s), violations=0`**，Python CLI `real-xml-check.txt` 输出 **`checked 220 file(s), 0 violation(s)`**，退出码均为 `0` |
| 7 类故意改坏精确报错 | 去掉 BOM（`MISSING_UTF8_BOM`）、改 LF（`LINE_ENDING_NOT_CRLF`）、错 UID 写法（`INVALID_OBJECT_UID`，含小写十六进制与缺端口 `.N`）、错关系前缀（`INVALID_REL_UID_PREFIX`）、关系两端类型不符（`DISALLOWED_REL_ENDPOINTS`）、容器固定属性被改（`CONTAINER_ATTR_MISMATCH`）、首元素非 `PIDDrawing`（`FIRST_ELEMENT_NOT_PID_DRAWING`）均逐条输出 `<文件>:<位置> [<错误码>]: expected <期望>, got <实际>` |
| `SchemaOnly` 与可配 `Plant` | 验证 `CheckMode::SchemaOnly`（及 `--schema-only`）兼容解析库 2 空格 LF 无 BOM 输出，且 `expected_plant` / `--plant` 精确校验工厂名 |

### 3.2 固定门禁读数

| 门禁项 | 改前基线（§1.1） | 改后实测 | 结论 |
|---|---|---|---|
| `cargo test --offline --locked --lib` | `7,145` 条（`7,089` 过 / `55` 忽略 / `1` `.pen` 失败） | **`7,156` 条**（**`7,099` 过** / **`56` 忽略** / **`1` 同一 `.pen` 失败**） | `+11` 条测试，**0 新增失败** |
| `cargo clippy --offline --locked --lib` | `656` 条警告 | **`656` 条警告** | **0 新增警告** |
| `cargo clippy --offline --locked --lib --profile test` | `1,051` 条警告 | **`1,051` 条警告** | **0 新增警告** |
| `scripts\fmt-newhunks.ps1` | — | `src/io/mod.rs` **`new=0`**，`src/io/pid_publish_check.rs` **`new=0`** | **0 格式偏差** |
| `python scripts/arch_check.py report` | `33 module pairs, 183 files`，`io` 四行 `3, 1, 1, 1` | `33 module pairs, 183 files`，`io` 四行 `3, 1, 1, 1` | **0 新增向上依赖** |
| `python -m pytest scripts/sppid -q -p no:cacheprovider` | `167 passed` | **`171 passed`** | 全绿 |
