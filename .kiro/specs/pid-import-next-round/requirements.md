# Requirements Document

## Introduction

pid-import-next-round（`.pid` 线收尾 + 加固）

上一单 `docs/plans/2026-09-28-pid-import-next-steps.md`（P-D1 – P-D14，Plannotator 已批）落到了 W2 / B3：

- 已落地：R1 + W0（pid-parse `cc6626e`）；P-D12 死记录不画（pid-parse `a68bf55` + OCS `08ec2f1c`）；W1（pid-parse `50560a1`）。pid-parse 的这几个提交在工作分支 `codex/phase32c-bundle-closeout` 上，不在 pid-parse `main`（`a6a2e6e`，落后工作分支 5 个提交）；OCS `08ec2f1c` 在 OCS 本地 `main` 上，未推送。
- 只在分支 `pid-web-open`（工作树 `ocs-pid-0928`，未推、未进 main）：W2 `e527be78`（网页版能开 `.pid`）、B3 `d1d93b29`（`tests/pid_batch_baseline.rs`）、`eeee3ef9`（`scripts/build-web-worker.sh` 两处 Windows 修正）。

本轮范围是「收尾 + 加固」，2026-09-29 clarify 时用户选定：

- 收尾：主工作树的上游合并与 rvt 提交完成（G1）后，把分支集成进 main、重验、基线归因、网页真机验收、快进 main。推送逐推点头。
- 加固：网页解析任意字节不 panic（P-D16）；网页另存按固定表写字体文件名（P-D17，已批）；「字体未安装」行只在网页版（P-D18，已批）；`jsheet_oids` 与原生跳过位一致（P-D21）；S1 取证（P-D7，09-28 已批）。
- 用户已被告知：上游合并与 rvt 提交完成之前不碰 `main`，所有工作在工作树 `ocs-pid-0928` 与 pid-parse 里做。

决策 P-D15 – P-D22 见 `design.md`；送 Plannotator 批注的是计划文档（`docs/plans/2026-09-29-pid-import-next-round.md`，在工作树），决策以批注后的计划文档为准，批注改了哪条，本 spec 跟着改。本单开单时没有重跑任何测试：本会话 shell 不回输出，数字取自提交记录。小单 E 的决策 E-D1 – E-D6 见它自己的计划文档，本 spec 只收它的接口（需求 12、design D12、任务 15）。

## Glossary

- **执行者**：按 `tasks.md` 干活的代理或人（任一会话）。
- **主工作树**：`d:\work\plant-code\cad\OpenCADStudio`。开单时正在 `git merge origin/main`（MERGE_HEAD `88169c1d`，比 merge-base `fd2062bd` 多 357 个提交，44 个文件冲突）；HEAD `a651940f`（= `backup/main-before-merge-20260929`）；rvt 未提交工作在 `stash@{0}`。
- **工作树**：`D:\work\plant-code\cad\ocs-pid-0928`，分支 `pid-web-open`，开单时在 `eeee3ef9`；除计划文档（未提交）外干净。
- **pid-parse 仓**：`D:\work\plant-code\cad\pid-parse`，工作分支 `codex/phase32c-bundle-closeout`，开单时在 `50560a1`、领先 origin 3 个提交；本地 `main` = `origin/main` = `a6a2e6e`，落后工作分支 5 个提交（上一单写的是 2 个）。
- **外来文件**：pid-parse 仓里另一会话改着的四个文件——`docs/analysis/2026-09-20-jflavorholder-carries-the-placed-instances-parameters.md`、`docs/analysis/2026-09-21-parsing-pipeline-audit.md`、`docs/plans/2026-09-21-style-link-reads-the-parsed-document.md`、`examples/probe_jflavorholder_carries_instance_parameters.rs`。
- **本分支文件**：`git diff --name-only main...pid-web-open` 列出的路径（随阶段 A 增长）。
- **G1（集成门槛）**：四条同时成立——① 主工作树没有 `MERGE_HEAD`（也没有进行中的变基 / 拣选）；② 上游合并已提交（`88169c1d` 是 `main` 的祖先）；③ rvt 工作已提交（`main` 的历史里有加入 `src/io/rvt.rs` 的提交）；④ 主工作树没有与本分支文件重叠的未提交改动（含未跟踪文件）。
- **G2（推送门槛）**：用户对某一次推送的明确批准，批准里点名远端与分支。每一推单独过一次。
- **路径路**：`io::pid::load_pid(path)`，`cfg(not(target_arch = "wasm32"))`。
- **字节路**：`io::pid::load_pid_from_bytes(name, bytes)`。用 `PidParser::with_options(ParseOptions::geometry()).parse_reader(Cursor::new(bytes))` 解析，不找符号库，不读 `_Data.xml`。
- **网页打开**：wasm32 上 `io::load_web_bytes` 遇 `.pid`（`is_read_only_source_path(name)`）时，在页面主线程调字节路，结果经 `pid_read_outcome` 包装。失败是终态 `OpenLoadError`「P&ID import: …」。
- **解析链**：pid-parse 侧 `parse_reader` → `build_normalized_geometry` → OCS 调的文字样式入口（`text_styles_for_document` 一路）。
- **变异输入**：六类——空字节、随机字节、语料截断、定种翻字节、整扇区清零、长度字段写巨值。
- **不 panic 套件**：本轮两仓各写一套的属性测试。pid-parse 那套把变异输入喂给解析链，OCS 那套喂给字节路整条导入。
- **样式登记**：`io::pid::text::register_text_styles`，返回 `(names, Typefaces { named, not_installed })`。
- **网页字体表**：本轮新增的纯函数 `web_face_file(family) -> Option<&'static str>` 及其固定表。
- **打开完成处理**：`src/app/update/file.rs` 里取走导入摘要属性、往命令行推行的那一段。
- **字体未安装行**：命令行 `P&ID typefaces: {missing} of {named} the drawing names are not installed here; that lettering draws in the fallback font`（locale key `common.pid-typefaces-not-installed-here-fallback-font`）。
- **四主图**：pid-parse `test-file/` 下的 `D06.pid`、`DWG-0201GP06-01.pid`、`DWG-0202GP06-01.pid`、`工艺管道及仪表流程-1.pid`。
- **publish 副本**：`test-file/export-test/publish-data/A01/A01.pid`、`test-file/export-test/publish-data/DWG-0202GP06-01/DWG-0202GP06-01.pid`，旁边有 `_Data.xml`。
- **批量报告**：OCS `examples/pid_batch_report.rs`。
- **基线 CSV**：OCS `docs/evidence/2026-09-24-pid-batch-baseline/report.csv`（六张图，P-D12 后重签）。
- **原生跳过位**：PSM 类型字的 `0x8000` 位（DTO `type_flags & 0x2`）。`parse_live_psm_header` 见位即拒，22 个族解码器都从它起步（P-D12）。
- **棘轮**：pid-parse 的 `render_gap_census` / `parse_real_files` / `style_link_ratchet` / `geometry_profile` 四组测试。
- **S1 探针**：本轮在 pid-parse `examples/` 下新增的取证探针。
- **答案键**：publish 副本旁的 `_Data.xml` 按 GraphicOID 两跳（pid-parse `docs/analysis/2026-08-07-graphic-oid-is-the-semantic-join.md`）落到文件内图形记录后，得到的 `(图形 oid, ItemTag, class)`。
- **计划文档**：工作树里本轮的 `docs/plans/2026-09-29-pid-import-next-round.md`，送 Plannotator 批注；P-D15 – P-D22、进度与门禁记录都记在这里。
- **上一单**：工作树里的 `docs/plans/2026-09-28-pid-import-next-steps.md`（最新副本只在 `pid-web-open`）。
- **小单 E**：工作树里的 `docs/plans/2026-09-30-a-cached-body-draws-its-elliptical-arcs.md`（2026-09-30 开单，会话内批准，未走 Plannotator；决策 E-D1 – E-D6 以它为准）。它修 09-30 解析缺口盘点里唯一看得见的真缺口：A01 设备 `V 010121A` 两端的 2:1 椭圆封头没画。

## Requirements

### Requirement 1: 现场保护与门槛

**User Story:** 作为正在主工作树做上游合并与 rvt 接入的另一会话，我要本轮在 G1 前不碰主工作树、也不碰 pid-parse 的外来文件，以便我的合并现场与 stash 不被打乱。

#### Acceptance Criteria

1. WHILE G1 未满足，执行者 SHALL NOT 在主工作树执行任何写操作（含 `git checkout` / `merge` / `stash` / `add` / `commit` / `reset` / `worktree`，以及会改写 `Cargo.lock` 的 `cargo` 命令）；本 spec 目录 `.kiro/specs/pid-import-next-round/` 内的文件除外（任务勾选与 spec 自身的修订；该目录未入库，不碰合并现场）。
2. WHEN 执行者需要读主工作树状态 THEN 执行者 SHALL 只用只读命令（`git --no-optional-locks status --porcelain`、`git rev-parse`、`git log`、`git merge-base --is-ancestor`、`git stash list`）。
3. WHILE G1 未满足，执行者 SHALL 把写操作限定在工作树、pid-parse 仓（外来文件除外）、`CARGO_TARGET_DIR`（`D:\Rust\target`）、系统临时目录与本 spec 目录。
4. WHEN 执行者判 G1 THEN 执行者 SHALL 逐条检查术语表列的四条，四条同时成立才判满足，并把每条的实测值写进报告。
5. WHEN 执行者在 pid-parse 仓提交 THEN 执行者 SHALL 只 `git add` 本任务自己的路径；外来文件 SHALL NOT 被暂存、改写、还原或 stash。
6. 执行者 SHALL NOT 动主工作树的 `stash@{0}` 与分支 `backup/main-before-merge-20260929`。
7. IF 任一门槛（任务 1 预检、G1、G2）不满足 THEN 执行者 SHALL 停下当前任务，报告哪一条不满足及实测值，SHALL NOT 轮询、sleep 或重试等待。
8. IF shell 不回输出（`git --version` 不打印版本） THEN 执行者 SHALL 停下并报告，SHALL NOT 凭记忆假定命令结果。

### Requirement 2: 集成与落地（P-D15）

**User Story:** 作为维护者，我要 `e527be78` / `d1d93b29` / `eeee3ef9` 原样进 main，以便计划文档与证据里引用的哈希继续有效。

#### Acceptance Criteria

1. WHEN G1 满足 THEN 执行者 SHALL 在工作树把当时的 `main` 合进 `pid-web-open`（merge，不 rebase），`e527be78` / `d1d93b29` / `eeee3ef9` 三个提交对象保持不变。
2. WHEN 合并在 `Cargo.lock` 上冲突 THEN 执行者 SHALL 取 `main` 一侧的 `Cargo.lock`、由 cargo 重解析，SHALL NOT 手工逐行合并。
3. WHEN 重解析完成 THEN `Cargo.lock` 与 `main` 的差 SHALL 只落在 `pid-parse` 条目的依赖列表（预期多 `web-time`；main 已有则无差）。
4. WHEN 解决冲突 THEN 执行者 SHALL 按 design D7「冲突热点」逐文件处理；`Cargo.toml` SHALL 同时留下上游的改动、rvt 的依赖与本分支的 wasm32 目标行 `pid-parse = { path = "../pid-parse", default-features = false }` 及其注释。
5. WHEN 需求 3 与需求 7 全部通过 THEN 执行者 SHALL 在主工作树用 `git merge --ff-only pid-web-open` 落地。
6. IF `Cargo.lock` 出现 2.3 以外的差、`git merge --ff-only` 被拒或主工作树 HEAD 不在 `main` THEN 执行者 SHALL 停下报告，SHALL NOT 改用非快进合并、rebase、reset 或切分支。
7. WHEN 执行者提交 THEN 执行者 SHALL 只 `git add` 本任务自己的路径（合并提交只 add 解决过的冲突文件与 `Cargo.lock`）。
8. 本需求的任何一步 SHALL NOT 推送。

### Requirement 3: 集成后重验与基线归因

**User Story:** 作为维护者，我要集成后 `.pid` 线的数字要么与基线相同、要么每处差异都能归到 main 新进的某个提交，以便上游 357 个提交不会在无人知情时改掉导入结果。

#### Acceptance Criteria

1. WHEN 集成合并提交完成 THEN 执行者 SHALL 在工作树跑完 design D8「重验清单」的 OCS 部分，全部通过。
2. WHEN 重验 THEN 执行者 SHALL 用批量报告导出六张图的 DXF，把 `dxf_sha256` 与基线 CSV 逐张比较。
3. IF 任一张 `dxf_sha256` 与基线 CSV 不同 THEN 执行者 SHALL 做逐实体比（去句柄类组码后比多重集，同 P-D12），并把每一处差异归到 main 新进的一个具体提交（上游或 rvt），写出提交哈希与原因。
4. WHEN 所有差异都已归因 THEN 执行者 SHALL 重签基线 CSV 里变了的列，并在 evidence README 与计划文档记下新旧哈希与归因。
5. IF 有差异归不到任何提交 THEN 执行者 SHALL 停下、不落地（不执行 2.5），报告差异清单。
6. WHEN 重验 THEN `pid_batch_baseline` 的 17 列 SHALL 与基线 CSV（或按 3.4 重签后的 CSV）一致。

### Requirement 4: 网页解析任意字节不 panic（P-D16）

**User Story:** 作为网页版用户，我要打开坏的或截断的 `.pid` 时只看到一条错误、页面照常可用，以便一张坏图不把整个网页应用带崩。背景：wasm32 上 panic 即 abort，没有 unwinding，`catch_unwind` 不起作用；DWG / DXF 的 panic 只丢 worker，`.pid` 却在页面主线程。

#### Acceptance Criteria

1. pid-parse 的不 panic 套件 SHALL 对每个变异输入依次跑解析链；每一步 SHALL 以 `Ok` 或 `Err` 返回，不 panic。
2. OCS 的不 panic 套件 SHALL 对每个变异输入跑字节路整条导入（解析、样式、实体、摘要）；`load_pid_from_bytes` SHALL 以 `Ok` 或 `Err` 返回，不 panic。
3. 两套不 panic 套件 SHALL 覆盖全部六类变异输入，语料取四主图与 publish 副本。
4. 两套不 panic 套件 SHALL 只用固定种子生成变异；同一提交上重跑 SHALL 得到逐字节相同的输入。
5. IF 一个用例 10 s 内没有返回 THEN 套件 SHALL 判它挂起、判失败；失败信息 SHALL 含文件、变异类别、变异参数与种子。
6. IF 一个用例 panic THEN 套件 SHALL 判它失败；失败信息 SHALL 含文件、变异类别、变异参数、种子与 panic 信息。
7. IF 一个用例的单次分配请求超过 design D2 定的上限 THEN 套件 SHALL 判它失败（wasm32 上这种分配会 abort，native 上未必显形）。
8. WHEN 套件发现一处 panic、挂起或超额分配 THEN 执行者 SHALL 在出事的源头修（返回 `Err`，或跳过该记录并计数），并把最小复现留作回归测试。
9. WHEN 一处预分配（`with_capacity` / `vec![0; n]` / `resize` / `reserve`）的大小取自盘上计数 THEN 执行者 SHALL 把它钳到剩余字节数能容纳的元素数以内。
10. 修复 SHALL NOT 改变语料结果：pid-parse 棘轮与 golden 不变，批量报告六张 `dxf_sha256` 与基线 CSV 相同。
11. IF 一处 panic 在第三方库（如 `cfb` 0.14）内部，且无法以低成本在调用前预检排除 THEN 执行者 SHALL 留下复现、按 P-D16 登记 worker 单的触发，SHALL NOT 在本 spec 内改第三方库。
12. WHEN 网页打开的 `.pid` 解析或导入失败 THEN 网页打开 SHALL 显示 `P&ID import: …` 错误，同一标签页 SHALL 随后能打开另一张图。

### Requirement 5: 网页另存保住 TrueType 字体（P-D17，已批）

**User Story:** 作为网页版用户，我要网页另存的 DWG / DXF 在 desktop 重开时文字仍是原来的 TrueType 字体，以便网页另存不把 `6b629e2e` 的效果退回 `txt`。

#### Acceptance Criteria

1. WHERE 目标是 wasm32，WHEN 样式登记遇到 `face_file_name` 返回 None 且族名在网页字体表里 THEN 样式登记 SHALL 把表里的文件名写进 `style.font_file`。
2. 网页字体表 SHALL 只收语料点名、且文件名按 P-D17 收录判据核过的族。
3. 网页字体表的族名匹配 SHALL 不分大小写。
4. 网页字体表里每一项的文件名 SHALL 与参考机上 desktop `face_file_name(族名)` 的返回值相同（忽略大小写）。
5. WHERE 目标是 wasm32，IF 族名不在网页字体表里 THEN 样式登记 SHALL 让该样式保留 `txt`，并计入 `typefaces_not_installed`。
6. WHERE 目标是 wasm32，WHEN 族名命中网页字体表 THEN 样式登记 SHALL 仍把它计入 `typefaces_not_installed`（网页仍用回退字体画），日志 SHALL 点名另存会带上的文件名。
7. WHERE 目标是 desktop，样式登记 SHALL 保持 09-24 单的策略：`Some(file)` 写文件名；None 保留 `txt`、记日志、不猜文件名。
8. WHEN P-D17 落地 THEN 四主图 `--export` DXF SHALL 与基线逐字节相同。
9. WHEN 网页另存的 DWG 在装有该字体的 desktop 上重开 THEN 命中网页字体表的样式 SHALL 用该 TrueType 字体画。

### Requirement 6: 「字体未安装」行只在网页版（P-D18，已批）

**User Story:** 作为 desktop 用户，我要打开 `.pid` 时命令行照 W2 之前的样子（缺字体只进日志），以便 P-D6 给网页版写的那一行不出现在 desktop 上。

#### Acceptance Criteria

1. WHERE 目标是 wasm32，WHEN 打开完成且 `typefaces_not_installed > 0` THEN 打开完成处理 SHALL 推字体未安装行。
2. WHERE 目标是 desktop，打开完成处理 SHALL NOT 推字体未安装行。
3. 导入摘要 SHALL 在两种目标上照旧带 `typefaces_named` / `typefaces_not_installed`；`ImportSummary::log` SHALL 照旧写这两个数。
4. 打开完成处理 SHALL 在两种目标上照旧从文档属性里取走导入摘要（摘要属性不随另存写出）。
5. 21 个 `locales/*/opencadstudio.ftl` 与 `src/locale_catalog.rs` SHALL 保留该 key；`--lib i18n::tests` SHALL 保持 3 / 3。
6. WHEN P-D18 落地 THEN desktop 构建 SHALL NOT 因此新增告警（未用变量等）。
7. WHEN P-D18 落地 THEN `docs/user-guide.md` 与 `docs/native-vs-web.md` SHALL 写明这一行只在网页版出现、desktop 看日志。

### Requirement 7: 网页版真机验收（P-D19）

**User Story:** 作为维护者，我要在真浏览器里开过四主图、坏图与另存往返，以便「网页 = native」不再只靠导入器层的间接棘轮。

#### Acceptance Criteria

1. WHEN 集成后的代码经 `trunk serve --release` 起来 THEN 执行者 SHALL 在 Chromium 里依次打开四主图。
2. WHEN 每张主图打开完成 THEN 命令行导入摘要里的每个数 SHALL 与基线 CSV 该图的对应列相同，字体未安装行 SHALL 读作 `N of N`。
3. WHEN 打开一个截断的 `.pid` THEN 页面 SHALL 显示 `P&ID import: …` 错误，同一标签页 SHALL 随后打开另一张主图且摘要正确。
4. WHEN 网页另存一张主图为 DWG 并在 desktop 重开 THEN 命中网页字体表的样式 SHALL 显示该字体，这些样式的 STYLE 表组码 3 SHALL 与 desktop 自己 `--export` 同图的值相同；族名不在网页字体表里的样式不做这项比较（desktop 装了该字体会写它的文件名，与网页本来就不同），这些样式在网页另存里的组码 3 SHALL 保留 `txt`。
5. 执行者 SHALL 记录每张主图在页面主线程上的导入耗时（P-D16 的第二个触发条件看它）。
6. 执行者 SHALL 把截图、命令行文本、耗时、另存文件哈希与 STYLE 摘录放进 `docs/evidence/<日期>-pid-web-open/`，并写 README（环境、版本、步骤、结果、偏差）。
7. IF 任一条不符 THEN 执行者 SHALL 停下、不落地（不执行 2.5），报告差异。
8. 执行者 SHALL NOT 为本验收新增依赖或无头浏览器框架。

### Requirement 8: `jsheet_oids` 与原生跳过位一致（P-D21）

**User Story:** 作为 pid-parse 维护者，我要 `jsheet_oids` 与 22 个族解码器用同一判据，以便将来带跳过位的旧 JSheet 不被当成活 sheet。

#### Acceptance Criteria

1. `jsheet_oids`（`src/parsers/sheet_records.rs`）SHALL 用 `parse_live_psm_header` 过滤；带原生跳过位的 JSheet SHALL NOT 进结果。
2. pid-parse SHALL 有一条合成字节测试：活 JSheet 与带位 JSheet 混排时，结果恰为活记录的 oid。
3. `sheet_record_starts` SHALL 保持不过滤（它走链，要看见每条记录）。
4. WHEN 改动落地 THEN 四组棘轮与 golden SHALL 不变（语料里没有带位的 JSheet）。

### Requirement 9: S1 取证（P-D7，09-28 已批）

**User Story:** 作为维护者，我要知道没有 `_Data.xml` 时只凭文件内关联能复原多少位号，以便决定值不值得开实施单。

#### Acceptance Criteria

1. S1 探针 SHALL 放在 pid-parse `examples/`，以 0202 publish 副本的 `_Data.xml` 为答案键；WHERE A01 副本的 `_Data.xml` 带位号，S1 探针 SHALL 一并计分并分开报。
2. S1 探针 SHALL 只用文件内路径复原：`igTextBox.parent_ref`、`DependencyObject` 编组（四主图共 353 条）、放置的符号库路径 → 类；`_Data.xml` SHALL 只用于打分。
3. S1 探针 SHALL 按路径、按类报复原率，并列出失败类别（计数与一例）。
4. WHEN 探针跑完 THEN 执行者 SHALL 在 pid-parse `docs/analysis/` 写一页分析（口径、复原率、走法、失败类别、结论）。
5. IF 位号总复原率 ≥ 80 % THEN 分析 SHALL 提议一张实施单（范围与验收），本 spec SHALL NOT 开单或实施。
6. IF 位号总复原率 < 80 % THEN 分析 SHALL 把 S 登记不做，并写明理由。
7. S1 SHALL NOT 改任一仓的 `src`。

### Requirement 10: 推送与仓库卫生（P-D20）

**User Story:** 作为仓库所有者，我要每一次推送都由我点名批准、按依赖顺序推，以便远端上 OCS 引用的 pid-parse 提交都已在远端。

#### Acceptance Criteria

1. 执行者 SHALL NOT 推送，除非该次推送过了 G2。
2. WHEN 推送 THEN 执行者 SHALL 按顺序：① pid-parse `codex/phase32c-bundle-closeout` → `origin`；② H1：`git fetch . codex/phase32c-bundle-closeout:main` 后 `git push origin main`；③ OCS `main` → `pid-cad`。
3. IF 前一步未完成（未批、被拒、失败） THEN 执行者 SHALL NOT 执行后一步。
4. WHEN 执行 H1 THEN 执行者 SHALL NOT 在 pid-parse 仓切换分支。
5. WHEN 推 OCS 之前 THEN 执行者 SHALL 只读核查 `pid-cad` 上的 CI（含上游带来的 `.github/workflows/ci.yml`）如何对待 `path = "../pid-parse"`，结论写进该推的 G2 请求。
6. IF 核查判定 CI 会失败 THEN 执行者 SHALL 登记一张跟进单，SHALL NOT 在本 spec 内改 CI。
7. 执行者 SHALL NOT 使用 `--force` / `--force-with-lease` / `--no-verify`。
8. WHEN 每一推完成 THEN 执行者 SHALL 核对远端分支哈希等于本地，并在计划文档门禁记录里记下远端、分支与推前推后哈希。

### Requirement 11: 文档与台账

**User Story:** 作为后来接手的会话，我要计划文档、证据 README 与 pid-parse 的 CHANGELOG / task_plan 反映真实状态，以便不再从过时的引述推断进度。

#### Acceptance Criteria

1. 执行者 SHALL 在工作树的上一单里改：标题的进度括注；顶部引述（P-D12 已批已落，不再「等批」）；门禁记录（B3 已落 `d1d93b29`）；W2 偏离 P-D5（走页面主线程，由 P-D16 追认）；pid-parse `main` 落后数（2 → 实测数；任务 8.1 落地时为 8）；指向计划文档的指针。P-D15 – P-D22 SHALL 只记在计划文档里。
2. WHEN 本轮一项落地 THEN 执行者 SHALL 在计划文档「进度」或「门禁记录」追一段（提交哈希、验证数字）。
3. WHEN 新建证据目录或重签基线 THEN 执行者 SHALL 更新对应的 evidence README。
4. WHEN pid-parse 有提交落地 THEN 执行者 SHALL 更新 pid-parse `CHANGELOG.md` 与 `task_plan.md`（只 add 自己的路径）。
5. 文档 SHALL 沿用计划文档的体例：短句、事实钉提交哈希、决策表 ⭕。

### Requirement 12: 缓存本体画出椭圆弧（小单 E，E-D1 – E-D6）

**User Story:** 作为 A01 的读者，我要设备 `V 010121A` 两端的 2:1 椭圆封头画出来，以便卧式容器不再画成直角方箱。

#### Acceptance Criteria

1. pid-parse SHALL 解缓存本体（嵌套 `JSite*/PSMcluster0`）里的 `0x007E igEllipticalArc2d`：从 `parse_live_psm_header` 起步、btf = 75、七个 f64 有限且不超出 `GLINE2D_COORDINATE_DOMAIN_LIMIT`、长半轴非零、`0 < ratio ≤ 1`；不合 SHALL 拒收该条，SHALL NOT panic。
2. WHEN 缓存本体带椭圆弧 THEN pid-parse SHALL 把每条弧投成一条精确的有理二次 `SymbolPrimitive::BSpline`（每段 ≤ 45°，扫角只差舍入误差时取那个整数倍），接在本体自己的 B 样条之后；SHALL NOT 新增 `SymbolPrimitive` 变体，SHALL NOT 改 `PidSymbolDefinition`（主工作树与工作树编同一个 `../pid-parse` 检出）。
3. 扫向 SHALL 照 `igArc2d`：从起角顺时针扫到止角；WHEN 读 A01 THEN 四条弧 SHALL 都向本体外鼓。
4. WHEN OCS 导入 A01 THEN 设备 SHALL 多出两条 LWPOLYLINE（各 33 点；左封头顶点 x ≈ 200.571、右 ≈ 360.030；端点落在本体矩形的角上；点落在椭圆上），`drawn` / `layered_entities` SHALL 由 122 变 124；另五张图的 `--export` SHALL 逐字节不变；OCS `src` SHALL 零改动。
5. WHEN 基线 CSV 为此重签 THEN 只有 A01 行的 `drawn`、`layered_entities`、`dxf_sha256` SHALL 变，evidence README SHALL 追一条重签记录。
6. 09-30 解析缺口盘点的证据 SHALL 归进工作树 `docs/evidence/2026-09-30-pid-parse-gaps/`：路径相对化、加 README；脚本、临时 DXF / SVG 与工作日志 SHALL NOT 入库；SHALL 补一张修好后 A01 封头的截图。
7. 小单 E 的计划文档 SHALL 记进度与门禁；E-D2「照 `igArc2d` 的布局」SHALL 更正为实测布局（两个角在最前，只借扫向）；09-29 计划文档的盘点一节 SHALL 把 A01 封头标为已修。
8. 本需求 SHALL NOT 推送；推送仍按需求 10。

## 登记不做（本轮）

| 项 | 理由 |
|---|---|
| worker 路由（先把 `io::pid` 抽成独立 crate） | 另开单；触发条件见 P-D16 |
| R2 MTEXT | P-D13 |
| 哨兵字高 25 条 | P-D8 |
| V2 / B2 | 等用户材料（SmartPlant 截图；新 `.pid`） |
| 网页版打包 Arial Narrow | P-D6 |
| 工厂数据库语义（pid-parse `backup`） | 同 09-28 单：没有备份语料，S1 先看文件内能复原多少 |
| desktop 按表猜字体文件名 | 09-24 单的策略保留；表只在网页用（P-D17） |
| S 实施 | S1 只取证（P-D7）；≥ 80 % 也只提议 |
| 任何 CI 改动 | P-D20：只读核查，会失败就登记跟进单 |
| 任何新依赖（含 proptest / quickcheck / cargo-fuzz / 无头浏览器） | P-D19 / P-D22 |
| `SymbolPrimitive::EllipticalArc` + DXF `ELLIPSE` | 小单 E 的 E-D1 备选：阶段 B 两棵树合流后另开单 |
| Sheet 流 / `.sym` 库的 `0x007E`、`igEllipse2d 0x0063` | 小单 E 的 E-D4：`.pid` 语料里只有缓存本体带 `0x007E` |
