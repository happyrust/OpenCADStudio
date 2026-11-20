# 阶段一读数记录（DWG 转智能 P&ID，导出 `.pid` 与 publish XML）

> 状态：**进行中**。本文件记录阶段一（[happyrust/pid-cad#106](https://github.com/happyrust/pid-cad/issues/106)）各任务的门禁基线与实测读数。
> 约定：原始日志、报告全文与导出产物保存在仓库外证据目录（`$OCS_PID_EVIDENCE_DIR`），本文件只记录可复现的数字、结论、原因与证据文件名，不包含客户数据、盘符、用户名或主机名。

---

## 1. L0-6（#109）：当前 `main` 门禁基线读数与差异说明（2026-11-20）

| 项 | 值 |
|---|---|
| 测量日期 | `2026-11-20` |
| 本仓库提交 | `6377fe2334da303bf390a38aae1469260f870eca`（`main`） |
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
| 基线提交 | `b87839522`（`main`） |
| `rustc -V` | `rustc 1.102.0-nightly (65ce8611f 2026-11-18)` |
| 新增文件 | `scripts/sppid/corpus_index.py`、`scripts/sppid/corpus-layout.md`、`scripts/sppid/test_corpus_index.py` |
| 证据目录子项 | `2026-11-20-issue107/`（`red.txt`、`green.txt`、`corpus.manifest.json`、`real-index.txt`、`real-verify.txt`、`tamper-verify.txt`、`unset-env.txt`） |

### 2.1 先红后绿与单测门禁

- **先红验证（`red.txt`）**：先落盘 `test_corpus_index.py` 8 条断言测试对最小骨架运行，`8 failed in 0.39s`（全部死在预期 `AssertionError`，非语法或导入错误）。
- **实现后全绿（`green.txt`）**：`python -m pytest scripts/sppid -q -p no:cacheprovider` -> **`167 passed in 1.87s`**（原基线 `159` 条 + 新增 `8` 条全过）。

### 2.2 真实语料清单实测与全部验收项核对

| 验收项 | 实测结果 |
|---|---|
| 真实语料 `index` + `verify` | 通过外置 `--manifest` 只读扫描真实语料根目录（零改动源目录文件），索引 **`1,115` 个文件**（共 `406,863,955` 字节），`manifest_sha256` = `487b320319a9dfad20b1f9b70c8647ad2885ece4d2f8d3a217f71c88b9112038`；紧接执行 `verify` 输出 `verify OK: 1115 file(s) matched`，退出码 `0` |
| 各逻辑段文件统计 | `EXPORT_SET`：`282` 个文件（94 套 `.pid` + `_Data.xml` + `_Meta.xml`，`280,588,766` 字节）；`EXPORT_DXF`：`94` 个文件（`83,816,462` 字节）；`EXPORT_PDF`：`94` 个文件（`8,395,931` 字节）；`SYMBOLS`：`645` 个文件（644 个 `.sym` + 1 个 `CatalogIndex.xml`，`34,062,796` 字节） |
| 单字节篡改检出（`tamper-verify.txt`） | 翻转 1 个字节后 `verify` 退出码 `1`，精确输出 `MISMATCH [EXPORT_SET] export-set/01/sample_Data.xml (expected sha256=..., actual sha256=...)` |
| 未设 `OCS_PID_CORPUS_DIR`（`unset-env.txt`） | 退出码 `0`，输出 `未设置语料目录（OCS_PID_CORPUS_DIR），跳过语料清单操作。` |
| 清单零绝对路径审计 | 扫描 `corpus.manifest.json` 全文：反斜杠 `\` 计数 `0`、盘符冒号计数 `0`、绝对根路径计数 `0`；传入 `../outside` 或带盘符映射时返回退出码 `2` |
