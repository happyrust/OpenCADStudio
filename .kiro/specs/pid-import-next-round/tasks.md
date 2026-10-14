# Implementation Plan: pid-import-next-round（`.pid` 线收尾 + 加固）

## Overview

- **全部按序执行，一次一项。** 两仓共用工作树与同一个 `CARGO_TARGET_DIR`（`D:\Rust\target`），任何两项不得同时在同一仓跑。
- 阶段 A（1–8、15、9）现在就做，只写工作树 `D:\work\plant-code\cad\ocs-pid-0928`、pid-parse（外来文件除外）与本 spec 目录。15 是 09-30 盘点后加进来的小单 E，排在检查点 9 之前。
- G1（10）满足之前不碰主工作树 `d:\work\plant-code\cad\OpenCADStudio`；只读命令除外。
- 阶段 B（11–13）集成落地；阶段 C（14）推送，子任务全部可选，每一推单独点头——「全部运行」不会推送。
- 任一门槛不满足：停下报告实测值，不轮询、不等待。
- 命令与判据见 `design.md` D1–D11；测试数字以本轮实跑为准。

## Tasks

- [x] 1. 预检（只读）
  - [x] 1.1 核对 shell 与三处现场
    - `git --version` 要打印版本，否则停下报告
    - 工作树：分支 `pid-web-open`；HEAD 为 `eeee3ef9`，或其上只改计划文档的提交（`git diff --name-only eeee3ef9 HEAD` 只列 `docs/plans/2026-09-29-pid-import-next-round.md`）；`--no-optional-locks status --porcelain` 为空或只列这份计划文档
    - pid-parse：分支 `codex/phase32c-bundle-closeout`、HEAD `50560a1`、porcelain 只列四个外来文件
    - 主工作树：只读记下 HEAD、有没有 `MERGE_HEAD`、`stash list`、porcelain 条数，不写任何东西
    - 记下 `CARGO_TARGET_DIR` 与 D: 余量
    - 任一不符：停下报告，不修
    - _Requirements: 1.1, 1.2, 1.4, 1.7, 1.8_

- [x] 2. pid-parse：任意字节不 panic（P-D16 / P-D22）
  - [x] 2.1 写不 panic 套件 `tests/arbitrary_bytes_never_panic.rs`（名字暂定）
    - xorshift64 生成器、六类变异、线程执行器（`recv_timeout(10 s)`）、分配看门（计数 `#[global_allocator]`）、`SERIAL` 锁
    - 管线照 OCS `import()` 的调用序列：`parse_reader` → `build_normalized_geometry` → 文字样式入口
    - 对照组六张原图先跑并打印峰值；上限 = max(64 MiB, 峰值 × 8)
    - 默认批约 100 例；`#[ignore]` 扩展批数千例；每例生成两次比字节
    - 先跑一遍，把失败清单记下（此时允许红，2.4 修）
    - **Property 1: pid-parse 解析链对任意变异输入不 panic、不挂起、不超额分配**
    - **Validates: Requirements 4.1, 4.3, 4.4, 4.5, 4.6, 4.7**
  - [x] 2.2 预分配审计与钳位
    - 在 `src/` 搜 `with_capacity(` / `vec![0` / `resize(` / `reserve(` / `repeat(`，列出盘上计数驱动的每一处（清单进提交说明）
    - 加 `bounded_capacity`，逐处钳到剩余字节能容纳的元素数
    - `u32 as usize` 之后参与加、乘的偏移与长度改 `checked_*` 或先升 `u64`
    - _Requirements: 4.9, 4.10_
  - [x] 2.3 写 `bounded_capacity` 的属性测试
    - xorshift 生成一万组三元组，含 0、1、`usize::MAX`；乘积用 `u128` 算
    - **Property 3: 预分配有界**
    - **Validates: Requirements 4.9**
  - [x] 2.4 修套件找出的问题并收尾
    - 每处在源头修（返回 `Err` 或跳过并计入拒收），最小复现进回归测试
    - `cfb` 内部 panic：能低成本预检就预检；挡不住的复现标 `#[ignore]` 并注明「P-D16 触发 ①」，默认批排除该例并点名
    - 扩展批 `cargo test --release --test arbitrary_bytes_never_panic -- --ignored` 全绿
    - design D8 pid-parse 清单 1–6 全过；四组棘轮与 golden 不变
    - 外来示例编不过时改跑 `--lib --tests --bins` 并点名，不碰该文件
    - `CHANGELOG.md` / `task_plan.md` 各追一条；`git diff --cached --name-only` 只含本任务路径；提交
    - _Requirements: 4.8, 4.10, 4.11, 1.5, 11.4_

- [x] 3. pid-parse：`jsheet_oids` 与原生跳过位一致（P-D21）
  - [x] 3.1 `jsheet_oids` 改用 `parse_live_psm_header`
    - `src/parsers/sheet_records.rs` 约 4150 行；`sheet_record_starts` 不动
    - _Requirements: 8.1, 8.3_
  - [x] 3.2 写合成字节属性测试
    - 穷举 1–4 条记录的跳过位组合（30 条流）；已有 PSM 头构造器就复用
    - **Property 6: `jsheet_oids` 只收活记录**
    - **Validates: Requirements 8.1, 8.2, 8.3**
  - [x] 3.3 验证与提交
    - 四组棘轮与 golden 不变；design D8 pid-parse 清单 1–6 全过
    - `CHANGELOG.md` / `task_plan.md`；只 add 本任务路径；提交
    - _Requirements: 8.4, 1.5, 11.4_

- [x] 4. OCS：字节路不 panic（工作树）
  - [x] 4.1 写 `tests/pid_bytes_never_panic.rs`（名字暂定）
    - 生成器与执行器照 2.1 复制一份（不跨仓共享）；管线 = `load_pid_from_bytes(name, bytes)`
    - 语料取 `../pid-parse/test-file`，没有检出就软跳过（同 B3）
    - 另加例子测试：`D06.pid` 前 4096 字节走字节路得 `Err`（任务 12 用同一份截断）
    - **Property 2: OCS 字节路对任意变异输入不 panic、不挂起、不超额分配**
    - **Validates: Requirements 4.2, 4.3, 4.4, 4.5, 4.6, 4.7, 4.12**
  - [x] 4.2 修 OCS 导入器侧的问题并收尾
    - 问题在 pid-parse 里的，回 pid-parse 按 2.4 的做法修（另起提交）
    - 扩展批 release 全绿；`pid_import`、`pid_batch_baseline`、`--lib io::pid` 全绿
    - 批量报告六张 `dxf_sha256` = 基线 CSV；native 与 wasm32 check 过；clippy 触碰行零告警；rustfmt 干净
    - 只 add 本任务路径；提交
    - _Requirements: 4.8, 4.10, 3.2_

- [x] 5. OCS：网页另存字体表（P-D17）
  - [x] 5.1 实测语料点名的族，核文件名
    - 六张图去重列族名（原样）；参考机上 desktop `face_file_name` 的返回值
    - 注册表 `HKLM\SOFTWARE\Microsoft\Windows NT\CurrentVersion\Fonts` + `C:\Windows\Fonts` 核文件；对照微软字体清单判来源（Windows 默认 / Office）
    - 按 P-D17 收录判据定表；结果表（族、文件、来源）留给任务 8 写进计划文档
    - _Requirements: 5.2, 5.4_
  - [x] 5.2 实现 `web_face_file` / `choose_font_file` 并接进 `register_text_styles`
    - 调用点传 `cfg!(target_arch = "wasm32")`；`not_installed` 按 `counts_as_not_installed()` 累加
    - 命中表记 `info` 日志，点名另存会带上的文件名；没命中照旧
    - native 四图 `--export` 逐字节不变；wasm32 check 过
    - _Requirements: 5.1, 5.5, 5.6, 5.7, 5.8_
  - [x] 5.3 写网页字体表查找的属性测试
    - 表内族名的随机 ASCII 大小写变体都命中；随机非表名都得 None
    - 另加例子测试：表里每项与本机 `face_file_name` 一致（本机没装该族就跳过该项）
    - **Property 4: 网页字体表查找不分大小写、只认表内族名**
    - **Validates: Requirements 5.2, 5.3**
    - _Requirements: 5.4_（「表与本机一致」那条例子测试）
  - [x] 5.4 写字体选择策略的属性测试
    - 族名（表内变体与随机名）× `installed`（Some / None）× `web`（真 / 假）
    - **Property 5: 字体选择策略**
    - **Validates: Requirements 5.1, 5.5, 5.6, 5.7**
  - [x] 5.5 验证与提交
    - `--lib io::pid`、`pid_import` 全绿；批量报告六张哈希 = 基线 CSV；clippy 触碰行零告警；rustfmt 干净
    - 只 add 本任务路径；提交
    - _Requirements: 5.8_

- [x] 6. OCS：字体未安装行只在网页（P-D18）
  - [x] 6.1 `src/app/update/file.rs` 里那一推包进 `#[cfg(target_arch = "wasm32")]` 块
    - 读数放进块内，desktop 不留未用变量；摘要取走与 `ImportSummary::log` 两端照旧；locale key 不删
    - _Requirements: 6.1, 6.2, 6.3, 6.4, 6.5, 6.6_
  - [x] 6.2 改文档、验证、提交
    - `docs/user-guide.md` 导入汇总一段、`docs/native-vs-web.md` 的 `.pid` 一节：这一行只在网页版出现，desktop 看日志
    - native 与 wasm32 check 过、desktop 无新告警；`--lib i18n::tests` 3 / 3；`pid_import` 全绿
    - 只 add 本任务路径；提交
    - _Requirements: 6.5, 6.6, 6.7_

- [x] 7. S1 取证（pid-parse）
  - [x] 7.1 写探针 `examples/probe_item_tags_without_data_xml.rs`（名字暂定）
    - 按 design D6：答案键两跳；路 a `parent_ref`、路 b `DependencyObject`、路 c 符号路径 → 类
    - 类映射写死在探针里、打分前定；输出按路、按类与失败类别
    - 不改 `src`；用到 `backup` 模块就标 `required-features`
    - _Requirements: 9.1, 9.2, 9.3, 9.7_
  - [x] 7.2 跑探针、写分析、提交
    - `docs/analysis/<日期>-item-tags-without-data-xml.md` 一页：口径、复原率、走法、失败类别、结论
    - 位号总复原率 ≥ 80 %：提议实施单（不开）；< 80 %：登记不做并写理由
    - clippy 两种特性 `-D warnings` 过；`CHANGELOG.md` / `task_plan.md`；只 add 本任务路径；提交
    - _Requirements: 9.4, 9.5, 9.6, 1.5, 11.4_

- [x] 8. 刷新两份计划文档（工作树）
  - [x] 8.1 按 design D11 改上一单与计划文档并提交
    - 上一单（09-28）：标题括注；顶部引述（P-D12 已批已落）；门禁记录（B3 已落 `d1d93b29`）；W2 偏离 P-D5（P-D16 追认）；pid-parse `main` 落后数按实测（落地时 8 个）；末尾指向计划文档
    - 计划文档（09-29）：阶段 A 各项的「进度」段（提交哈希、验证数字、5.1 的字体表、7.2 的复原率）；门禁记录里有 Plannotator 的结论
    - 只 add 这两份计划文档；提交
    - _Requirements: 11.1, 11.2, 11.5_

- [x] 15. 小单 E：缓存本体画出椭圆弧（A01 封头；`docs/plans/2026-09-30-a-cached-body-draws-its-elliptical-arcs.md`，E-D1 – E-D6，design D12）
  - [x] 15.1 pid-parse：`0x007E` 解码器、`bspline::elliptical_arc`、缓存本体投影、测试、分析与台账（E1）
    - 已落 `6e393b1`（解码器 + 模型字段 + 投影 + 测试 + 分析，`--lib` 1133）与 `366b05c`（段数余量：A01 存的 π 比 `PI` 小一个 ulp，左封头原先切成五段 36°；`--lib` 1134）
    - design D8 pid-parse 清单 1–6 全过；golden 不变；A01 重签两处棘轮；不 panic 扩展批 release 全绿
    - _Requirements: 12.1, 12.2, 12.3, 11.4_
  - [x] 15.2 OCS 工作树：钉住 A01 封头、重签基线、提交（E2）
    - 工作树里已改好、未提交：`tests/pid_import.rs` 加 `a01s_vessel_draws_its_two_elliptical_heads`（53 → 54）；基线 CSV 只改 A01 行的 `drawn` / `layered_entities`（122 → 124）与 `dxf_sha256`（`3F07AA3B…60A7`）；evidence README 追「2026-09-30 重签」一条
    - 已核：`pid_import` 54 / 54、`pid_batch_baseline` 2 / 2；另五张导出逐字节不变；A01 逐实体比只多 2 条 LWPOLYLINE（各 33 点），附带差只有 design D12 接受的两项；`rustfmt --check tests/pid_import.rs` 干净；`src` 零改动
    - 只 add 这三个路径（`docs/evidence/2026-09-30-pid-parse-gaps/` 留给 15.3）；提交；不推送
    - _Requirements: 12.4, 12.5, 11.3_
  - [x] 15.3 证据归档、修好后的截图、计划文档（E3）
    - `docs/evidence/2026-09-30-pid-parse-gaps/` 已就位、未入库（README、两份报告、`gap-locations.csv`、19 张图）；README 里两处 `⟪E3b 待填⟫` 填 15.2 的提交哈希与新截图
    - 新截图：用 15.2 的 A01 导出 → `--plot-svg` → PNG，与 `01a` 同窗口，存进 `img/`（名字暂定 `01c-A01-equipment-heads-drawn.png`）；脚本不入库；只把边长 ≤ 1500 px 的副本交给子代理看
    - 小单计划文档：「进度」记 E1（`6e393b1` / `366b05c`）、E2（15.2 的哈希）、E3；「门禁记录」追一条；E-D2 改成实测布局（两个角在最前，只借扫向）；E-D5 的 `COORDINATE_LIMIT` 改成 `GLINE2D_COORDINATE_DOMAIN_LIMIT`；E1 写的「golden 只有 A01 变、重签」改成实际（golden 不变）；事实表的「4 段 / 9 个控制点 / 33 点」注明靠 `366b05c` 的余量；验收里记名字随本体右缘右移 12.979 mm（接受）
    - 09-29 计划文档：盘点一节把 A01 封头标「已修」，指向小单
    - 只 add 本任务路径；提交；不推送
    - _Requirements: 12.6, 12.7, 11.2, 11.3_

- [x] 9. 检查点：阶段 A 全绿
  - Ensure all tests pass, ask the user if questions arise.
  - pid-parse 的 porcelain 只剩四个外来文件；工作树干净；主工作树 porcelain 与任务 1 的记录一致
  - _Requirements: 1.1, 1.3_

- [ ] 10. G1 检查（只读）
  - [x] 10.1 逐条查 G1 四条并报告
    - ① 无 `MERGE_HEAD` / `REBASE_HEAD` / `CHERRY_PICK_HEAD`；② `88169c1d` 是 `main` 的祖先；③ main 历史里有加入 `src/io/rvt.rs` 的提交；④ 本分支文件 ∩ 主工作树 porcelain = ∅
    - 四条实测值写进报告；另记 `stash list` 与 `backup/main-before-merge-20260929` 的哈希
    - 任一不成立：停下报告，不轮询、不等待
    - _Requirements: 1.2, 1.4, 1.6, 1.7_

- [ ] 11. 集成（工作树）
  - [~] 11.1 把 `main` 合进 `pid-web-open` 并解冲突
    - 工作树干净 → `git merge main`；按 design D7 冲突热点逐文件解
    - `Cargo.lock` 取 main 一侧，`cargo check --lib` 重解析，与 main 只差 `pid-parse` 的依赖列表；有别的差就停
    - `is_read_only_source_path` 若含 `.rvt`：网页分支改为显式判 `.pid` 扩展名
    - 只 add 解决过的文件与 `Cargo.lock`；合并提交；核 `e527be78` / `d1d93b29` / `eeee3ef9` 都是 HEAD 的祖先
    - _Requirements: 2.1, 2.2, 2.3, 2.4, 2.6, 2.7, 2.8_
  - [~] 11.2 重验
    - design D8 OCS 清单 1–10 全过；pid-parse 只核 HEAD 与 porcelain 同阶段 A 记录（HEAD `366b05c`，任务 15.1 之后）
    - _Requirements: 3.1, 3.6_
  - [~] 11.3 哈希比较与归因
    - 六张 `dxf_sha256` 对基线 CSV（任务 15.2 重签后的）；全同就记「六张不变」
    - 有不同：按 D8 逐实体比，每处差异归到 main 新进的一个提交；全部归因后重签 CSV 变了的列、更新 evidence README 与计划文档，提交
    - 归不上：停下，不进 12 / 13
    - _Requirements: 3.2, 3.3, 3.4, 3.5, 11.3_

- [ ] 12. 网页版真机验收（P-D19）
  - [~] 12.1 构建并开四主图（design D9 第 1–3 步）
    - 环境按 D9 第 1 步；后台起 `trunk serve --release`
    - 每张主图：截图、抄命令行、对 CSV、字体行 `N of N`、记主线程导入耗时
    - _Requirements: 7.1, 7.2, 7.5, 7.8, 6.1_
  - [~] 12.2 截断图、另存往返、证据与提交（D9 第 4–6 步）
    - 截断图报 `P&ID import: …`，同一标签页再开一张主图、摘要正确
    - 网页另存 DWG → desktop 重开；族名命中网页字体表的样式，STYLE 表组码 3 与 desktop `--export` 同图相同；族名不在表里的样式不比（desktop 装了该字体会写文件名），网页另存里应保留 `txt`
    - `docs/evidence/<日期>-pid-web-open/` 与 README；只 add 本任务路径；提交
    - 任一条不符：停下，不进 13
    - _Requirements: 7.3, 7.4, 7.6, 7.7, 4.12, 5.9, 11.3_

- [ ] 13. 快进 main 并记门禁
  - [~] 13.1 工作树里记门禁记录并提交
    - 计划文档门禁记录：G1 实测、合并提交、重验数字、归因结论、真机验收结论、快进前的 `main` 哈希
    - _Requirements: 11.2, 2.7_
  - [~] 13.2 主工作树快进并核对
    - 先核主工作树 HEAD 在 `main`、G1 ④ 仍成立
    - `git -C d:\work\plant-code\cad\OpenCADStudio merge --ff-only pid-web-open`；核 `main` = `pid-web-open`、porcelain 里没有本分支文件
    - 被拒：停下报告，不改用别的合并方式；不推送
    - _Requirements: 2.5, 2.6, 2.8_

- [x] 14. 推送（每一推单独点头；子任务全部可选）
  - [ ]* 14.1 fork CI 只读核查（design D10）
    - 读集成后的 `.github/workflows/*.yml`（含上游带来的 `ci.yml`）；看 `pid-cad` 的 Actions 状态与最近运行
    - 判断推 `main` 会触发哪些 job、会不会因缺 `../pid-parse` 失败；会失败就登记跟进单，不改 CI
    - _Requirements: 10.5, 10.6_
  - [x]* 14.2 pid-parse 工作分支推送与 H1
    - ① 过 G2 → `git push origin codex/phase32c-bundle-closeout`，核远端 = 本地
    - ② 另过一次 G2 → `git fetch . codex/phase32c-bundle-closeout:main` → `git push origin main`，核远端 = 本地；不切分支
    - 任一步未批、被拒或失败：停下，后一步不做
    - 推送记录写进工作树计划文档的门禁记录（随 14.3 之前的快进进 main）
    - _Requirements: 10.1, 10.2, 10.3, 10.4, 10.7, 10.8_
  - [ ]* 14.3 OCS `main` 推 `pid-cad` 并记门禁
    - 14.2 两推都完成才做；工作树提交门禁记录（14.1 结论、14.2 记录、本推的批准）→ 主工作树 `merge --ff-only pid-web-open`
    - 过 G2（请求里附 14.1 结论）→ `git push pid-cad main` → 核 `pid-cad/main` = `main`
    - 被拒就停；另起提交改记录，不 amend
    - _Requirements: 10.1, 10.2, 10.3, 10.5, 10.7, 10.8, 11.2_

## Notes

- 只有 14.x 标了 `*`（可选），为的是「全部运行」不推送；不 panic 套件与各属性测试是本轮的交付物，不可跳过。
- 属性测试按 design「测试策略」的例数跑；每个属性测试注释里写 `Feature: pid-import-next-round, Property N: <标题>`。
- 所有提交只 add 自己的路径；pid-parse 的四个外来文件、主工作树的 `stash@{0}` 与 `backup/main-before-merge-20260929` 一律不动。
- 本 spec 目录在主工作树里、未入库；G1 前只许往主工作树写这个目录（任务勾选与 spec 修订，需求 1.1）。

## Task Dependency Graph

```json
{
  "waves": [
    { "id": 0, "tasks": ["1.1"] },
    { "id": 1, "tasks": ["2.1"] },
    { "id": 2, "tasks": ["2.2"] },
    { "id": 3, "tasks": ["2.3"] },
    { "id": 4, "tasks": ["2.4"] },
    { "id": 5, "tasks": ["3.1"] },
    { "id": 6, "tasks": ["3.2"] },
    { "id": 7, "tasks": ["3.3"] },
    { "id": 8, "tasks": ["4.1"] },
    { "id": 9, "tasks": ["4.2"] },
    { "id": 10, "tasks": ["5.1"] },
    { "id": 11, "tasks": ["5.2"] },
    { "id": 12, "tasks": ["5.3"] },
    { "id": 13, "tasks": ["5.4"] },
    { "id": 14, "tasks": ["5.5"] },
    { "id": 15, "tasks": ["6.1"] },
    { "id": 16, "tasks": ["6.2"] },
    { "id": 17, "tasks": ["7.1"] },
    { "id": 18, "tasks": ["7.2"] },
    { "id": 19, "tasks": ["8.1"] },
    { "id": 20, "tasks": ["15.1"] },
    { "id": 21, "tasks": ["15.2"] },
    { "id": 22, "tasks": ["15.3"] },
    { "id": 23, "tasks": ["9"] },
    { "id": 24, "tasks": ["10.1"] },
    { "id": 25, "tasks": ["11.1"] },
    { "id": 26, "tasks": ["11.2"] },
    { "id": 27, "tasks": ["11.3"] },
    { "id": 28, "tasks": ["12.1"] },
    { "id": 29, "tasks": ["12.2"] },
    { "id": 30, "tasks": ["13.1"] },
    { "id": 31, "tasks": ["13.2"] },
    { "id": 32, "tasks": ["14.1"] },
    { "id": 33, "tasks": ["14.2"] },
    { "id": 34, "tasks": ["14.3"] }
  ]
}
```
