# 智能 PID 平台 · 阶段一实施方案（DWG 转智能 P&ID，导出 `.pid` 与 publish XML）

> 日期：2026-11-20
> 状态：**已确认**（2026-11-20 对齐当前 `main` 基线修订）。按第 8 节推进顺序执行。
> 基线：本仓库 `main @ 6377fe233`；解析库钉在 `pid-parse @ a85fa10285cc5379953495fa391f374981b9e587`（见 `Cargo.toml`）。
> 跟踪总单：[happyrust/pid-cad#106](https://github.com/happyrust/pid-cad/issues/106)。
> 写法约定：本文件只使用仓库相对路径；样本、源码检出与证据目录一律通过逻辑名和环境变量引用，不写盘符、用户名、主机名或客户图纸信息。

---

## 1. 目的与终点

**目的。** 不依赖 SmartPlant P&ID（SPPID）软件，把存量 DWG 格式的 P&ID 图纸转换成智能 P&ID（图形与数据成一块：类、位号、属性、管线、连通关系），并导出 `.pid` 与 publish XML（每张图一对 `<图名>_Data.xml`、`<图名>_Meta.xml`），送**外部数字化交付平台**测试。协同、三维接入、设计规则检查、材料清单等留到后续阶段。

**终点（一句话）。** 对验收批次：程序批量识别并生成智能 P&ID；导出的 XML 通过格式与结构校验器，且与 SPPID 产出的参考 XML 在对象、位号、类、属性、关系上逐项比对差异可解释并低于约定阈值；导出的 `.pid` 能被解析库重新读出、被本软件显示且与参考出图一致，并在 SPPID 验收环境里打开；整批送外部平台测试有回执结论。

**阶段一不做。** 协同与离线同步；三维模型接入；设计规则检查、材料清单、快照、发布签核；多套图例标准可视化编辑器；ISO 15926 / CFIHOS 交换；用户、角色、项目空间管理；PBS 树、Plant Explorer、报表、版本与 Workshare。

---

## 2. 功能点、基线对齐与优先级

### 2.1 七条工作线与当前真实基线（`main @ 6377fe233`）

| 线 | 功能 | 当前仓库真实状态（2026-11-20 核实） |
|---|---|---|
| **L0 基线与语料** | 门禁基线、语料索引清单、对应关系表、度量工具、现状基线、外部平台口径、`pid-parse` 能力核实（Q1–Q13） | 样本集与 `pid-parse @ a85fa10` 源码均已就位；门禁基线（`#109`）、语料索引（`#107`）与源码问答（`L0-8`）可直接执行 |
| **L1 读取与显示** | `.pid` 导入图面；`.sym` 符号解析；与 SPPID 出图一致；`PIDPLOT` / `PIDBACKUPPDF` 出 PDF | **备份包计划收尾已全部完成**：`EXP-0..3`、显示修正 `RD-7/11/8/14/9/13`、`ACC-1` 总验收及新界面检查点 C（`UI-1..9` + `6377fe233`）均已入库；单样式字符 run 覆盖段落默认已在 `src/io/pid/styles.rs` 接通；待做扩面对比（`L1-2`）、符号缺失清单（`#113`）与 `.sym` 全量解码普查（`#114`） |
| **L2 内部模型与点选编辑** | `PID_SEMANTICS` 与 `PID_AUTHOR` 合一；点选整块；属性面板；标注与几何联动 | 并行分支 `pid-symbol-identity` 正在落地「`.pid` 直读对象类型/名字 + 备份库补位号 + `0x00FA` 标注配对 + 检查栏只读查看」；**本线须等 `pid-symbol-identity` 合入 `main` 后在其底座上推进**，避免撞文件 |
| **L3 DWG 识别** | 图例、位号、管线、连通；图例到 SPPID 类/码表值映射；置信度三档与人工确认 | `src/io/pid_legend/` 四路识别、位号配对、线号外置与管线拓扑已有；缺对 SPPID 真值的准确率度量与图例到 SPPID 类变体的外置映射 |
| **L4 publish XML** | XML 格式与结构校验器；从统一模型导出 `_Data.xml` / `_Meta.xml` | 备份包整包导出已完备（`src/io/pid_backup_export.rs` + `PIDBACKUPEXPORT`）；`pid-parse::publish` 已有 `PublishDrawing` / `PidPublishDrawing` 写出器、`catalog`、`codelist_values`、`pipeline`、`unit_values` 与 `diff`；待做校验器（`#111`）及从自有模型到发布结构的映射与严格格式输出 |
| **L5 `.pid` 生成** | 种子加增补写出器，分级验收 | `pid-parse::writer` 目前支持 CFB 容器流级原样回写、摘要/元数据修改、整流替换与实验性 `sheet_patches`，尚无从高层模型生成图形簇与动态属性流的能力，属阶段一最高风险研发项 |
| **L6 整合与总验收** | 批量转换命令、交互编辑存回 `.pid`、总验收报告 | 未做 |
| **LX 外部验收** | SPPID 预检运行器、接受度实验、预检三关、外部平台减法送测 | 未做（依赖受控 SPPID 环境与外部平台送测渠道） |

### 2.2 优先级

| 优先级 | 内容 | 对应工作线 |
|---|---|---|
| **首要** | `.pid` 扩面一致性对比与 `.sym` 符号库普查 | L1（`L1-2`、`L1-3`、`L1-6`） |
| **首要，与上并行** | XML 校验与导出、`.pid` 写出与外部平台送测预检 | L4、L5、LX |
| 其次 | 识别到导出最小闭环走通（准确率不设硬门槛，未映射项进覆盖报告） | L3 最小部分、L2 的 M1/M2/M5、L5-8 |
| 靠后 | 识别准确率深化；点选编辑与属性面板联动 | L3 深化、L2 其余 |

### 2.3 贯穿全线的五条技术对齐决议（2026-11-20 核定）

1. **XML 与 `.pid` 共用同一套 UID 与 `GraphicOID` 分配。** 样本证明除合成管线外，XML 对象 UID 以 ASCII 文本存于 `.pid` 的 `/PSMcluster0` 与 `/Unclustered Dynamic Attributes` 中，两边绝不各自生成。
2. **XML 校验器（`L4-0` / `#111`）拆分「严格厂商字节格式层」与「XML 结构语义层」，且 `Plant` 不硬编码死 `"P01"`。** SPPID 原始导出为 UTF-8 带 BOM + CRLF + `version ="1.0"` + 3 空格缩进，而 `pid-parse` 内部写出器默认输出无 BOM + LF + 2 空格缩进；校验器同时提供严格模式（默认，用于外部平台交付门禁）与纯结构模式（用于内部模型/解析库对比），容器 `Plant` 默认要求非空并支持指定期望工厂名精确匹配。
3. **`L2` 内部模型合一严格排在 `pid-symbol-identity` 分支合入 `main` 之后。** 充分承接该分支已实现的 `.pid` 自带 `P&IDAttributes` 类型读取、备份库 `ItemTag` 补全与 `0x00FA` 标注配对，不重复造轮子、不产生合并冲突。
4. **`L5-3` 种子往返验收按 CFB 流级字节一致判定。** `pid-parse::writer::PidWriter` 重写复合文件时会刷新存储时间戳并按路径字典序排列目录项，因此往返判据定为「容器内全部流字节与存储 CLSID 100% 相同 + `pid-parse` 重读相等」，外层重打包容器能否被 SPPID 直接打开由 `LX-4` 实验实测确认。
5. **`L4` 与 `L0-4` 优先复用/对齐 `pid-parse @ a85fa10` 已有能力。** 导出侧优先评估复用 `pid-parse::publish` 的 DTO、接口目录、码表分级与单位换算规则；Python 度量脚本 `xml_diff.py` 的差异分类口径与 `pid-parse::publish::diff` 对齐；不同来源的 `.sym` 库（不同备份包或历史快照）统一通过 `corpus.manifest.json` 逻辑名绑定各自文件计数与哈希，不在文档里混用分母。

---

## 3. 验收口径

外部交付平台无本地源码，已知 SPPID 产出的 `.pid` 与配套 XML 可通过，其最低要求未知：

1. **代理标准：与已通过样本逐项一致**，差异清单必须逐条可解释。
2. **日常门禁（每任务必跑）：** `.pid` a 级（解析器重读相等）、b 级（本软件显示与参考 PDF 一致）；XML 通过 `pid_publish_check` 严格模式校验。
3. **送测前预检三关（全过才送）：** ① XML 与已通过样本逐项对照；② `.pid` 在 SPPID 中能打开（c1）；③ SPPID 发布该 `.pid` 产出的 XML 与直接导出的 XML 一致（d）。
4. **减法送测摸清最低要求：** 每轮送一组变体（每份仅去掉一类内容，附一份原样已通过样本作对照），在有限送测轮次内定位平台硬约束。
5. **汇报严格区分「本仓库解析器认」与「SPPID 原生认」**，缺 SPPID 环境时一律标「SPPID 未验证」，绝不混标通过。

---

## 4. 共用约定与门禁

### 4.1 环境变量与语料布局

样本含客户数据，**严禁入库**。脚本与测试统一通过环境变量定位外部目录：

| 环境变量 | 含义 | 未设置时行为 |
|---|---|---|
| `OCS_PID_CORPUS_DIR` | 语料根目录（含 `corpus.manifest.json`） | 依赖语料的测试与脚本退出码为 `0` 并打印 `未设置语料目录` |
| `OCS_PID_EVIDENCE_DIR` | 证据与读数原始输出目录 | 使用系统临时目录下带日期的子目录 |
| `OCS_PID_PARSE_SRC` | `pid-parse` 源码检出目录（供 `L0-8` 与工具构建） | 跳过依赖外部源码检出的检查并注明 |
| `OCS_PID_SPPID_RUNNER` | SPPID 受控验收运行器脚本 | `LX` 运行类任务标「未执行」，不标通过 |

约定逻辑名见 `scripts/sppid/corpus-layout.md`（`EXPORT_SET`、`EXPORT_DXF`、`EXPORT_PDF`、`SOURCE_DWG`、`BACKUP_ORACLE`、`BACKUP_TABLES`、`SYMBOLS`、`SEEDS`、`EXTERNAL`、`CONTROLLED`）。磁盘实际目录结构与默认子目录不一致时，在 `corpus.manifest.json` 中登记相对子目录映射，不搬移原始样本文件；清单内严禁出现绝对路径。

### 4.2 固定门禁（Windows PowerShell 5.1，使用 `;` 与 `$LASTEXITCODE`）

1. **先红后绿**：先写测试并确认因目标断言失败（非编译错误），再写实现至全绿；天然即绿的校验先置入故意错误期望验证红态，留存红绿读数。
2. `cargo test --offline --locked --lib`：失败条数不增加，测试总条数不减少（以 `L0-6` / `#109` 基线为准）。
3. `cargo clippy --offline --locked --lib` 与 `cargo clippy --offline --locked --lib --profile test`：警告数不超过同一 `rustc -V` 下的基线读数。
4. `scripts\fmt-newhunks.ps1 <改前副本> <改后文件>`：所有改动 Rust 文件新增格式差异为 `0`。
5. 若改动 `Cargo.lock`，加跑 `cargo test --offline --locked -p ocs_collab -p ocs_collab_server`。
6. 若改动 `src/io/` 或 `src/app/`，前后各跑 `python scripts/arch_check.py report`，`io` 起始各行向上依赖数不增加。
7. 若改动 `docs/pid-platform/`，加跑 `python -m pytest scripts/pid-platform -q -p no:cacheprovider`。
8. 若改动 `scripts/sppid/`，跑 `python -m pytest scripts/sppid -q -p no:cacheprovider` 全绿。
9. **一任务一独立提交**：只暂存本任务文件与读数账本；不碰 `.cargo/config.toml`、`docs/pid-platform/design/pid-platform.pen`、`src/rest_openapi.json`；**不执行 `git push`**。

---

## 5. 代码与文档落位

| 内容 | 仓库路径 |
|---|---|
| 阶段一实施方案、源码核实问答、实测读数账本 | `docs/plans/2026-11-20-phase1-dwg-to-smart-pid/`（`plan.md`、`03-parse-answers.md`、`readings.md`） |
| 语料布局说明与索引校验脚本 | `scripts/sppid/corpus-layout.md`、`scripts/sppid/corpus_index.py`、`scripts/sppid/test_corpus_index.py` |
| publish XML 机器规格摘要（纯结构，零客户数据） | `assets/pid-publish-spec.json` |
| publish XML 格式与结构校验器（Rust + CLI 脚本） | `src/io/pid_publish_check.rs`、`scripts/sppid/xml_spec_check.py`、`scripts/sppid/test_xml_spec_check.py` |
| 度量与普查脚本（`#108`、`#113`、`#114`） | `scripts/sppid/xml_diff.py`、`legend_vs_xml.py`、`render_vs_pdf.py` 等 |
| 模型到 XML 导出模块（阶段 2） | `src/io/pid_publish/` |
| `.pid` 种子增补写出模块（阶段 2） | `src/io/pid_write/` |
| 识别结果升格模块（阶段 3，待 `pid-symbol-identity` 合入后） | `src/pid/promote.rs` |
| 批量转换命令（阶段 3/5） | `src/app/commands/pidconvert.rs` |

---

## 6. 阶段与任务拆分

### 6.1 阶段 0：基线、语料与解析库核实（L0）

- **L0-6（[#109](https://github.com/happyrust/pid-cad/issues/109)）门禁基线（S）**：在当前 `main` 上实测 `cargo test --lib`、双 `clippy` 与 `rustc -V`，逐项说明与备份包计划旧基线的差异原因，记入 `readings.md` §1。
- **L0-2（[#107](https://github.com/happyrust/pid-cad/issues/107)）语料约定与索引工具（M）**：实现 `scripts/sppid/corpus_index.py`（`index` / `verify`，纯 Python 标准库）与 `corpus-layout.md`；支持自定义相对子目录映射、清单零绝对路径、单字节篡改点名失败、未设 `OCS_PID_CORPUS_DIR` 退出码 `0` 打印 `未设置语料目录`。
- **L0-8 `pid-parse @ a85fa10` 能力核实（M）**：对照 `pid-parse` 源码与测试逐条回答 Q1–Q13（写出能力、流解码台账、两跳连接、管线合成、名称拼接、公称直径换算、码表目录、XML DTO 输入、OID 与索引同步、摘要属性、`TaggedTxtData` 流差异、符号 UNC 路径、受控样本现状），每条附精确源码路径与行号，写入 `03-parse-answers.md`，锁定 `L4-1` 与 `L5-4` 实施方案。
- **L0-1 资料清点与缺口表（S）**：按 `RES-01`–`RES-17` 登记状态与阻塞关系，脱敏摘要写入读数文件。
- **L0-3 对应关系表（M）**：建立原始图 ↔ 拆分图 ↔ `.pid` ↔ `_Data.xml` / `_Meta.xml` ↔ DXF / PDF 对应表，无法自动确认项单列。
- **L0-4（[#108](https://github.com/happyrust/pid-cad/issues/108)）度量工具集（M）**：实现 `xml_diff.py`（对齐 `pid-parse::publish::diff` 分类）、`legend_vs_xml.py`、`render_vs_pdf.py`，均含同文件零差异自检与篡改检出单测。
- **L0-5 现状基线报告（M）**：在验收语料上跑通三类度量并列出 Top N 缺口。
- **L0-7 外部平台验收口径与减法送测预案（S）**：明确送测包结构与每轮变体矩阵，缺外部回执信息处标「待补」。
- **L0-9 `.pid` 对象与 XML 对象逐对象对应与类变体目录（L）**：依据 Q3 连接方式统计约 240 种类变体并关联符号，产出零客户数据的 `assets/pid-class-map.json` 初稿。

### 6.2 阶段 1：显示扩面与 `.sym` 普查（L1）

- **L1-1（[#112](https://github.com/happyrust/pid-cad/issues/112)）显示修正五项与总验收**：**已在 `main` 完成**（`RD-7/11/8/14/9/13` + `ACC-1`），不再重复执行。
- **L1-3（[#113](https://github.com/happyrust/pid-cad/issues/113)）符号缺失清单（S）**：扫描全批 `.pid` 引用的 `.sym` 路径对符号库求差集，逐项标注是否有图纸缓存本体可替代。
- **L1-6（[#114](https://github.com/happyrust/pid-cad/issues/114)）`.sym` 解码状态普查（M）**：逐个解码符号库全部 `.sym` 并分类统计，说明不同样本库数量口径差异。
- **L1-2 一致性对比扩面（M）**：将 PDF 三项指标扩展至整批样本，逐张归因偏差。
- **L1-4 多样式字符 run 混排核验（S）**：在 `styles.rs` 已接通单 run 覆盖的基础上，核查多样式混排展平是否造成可见字宽偏差，按需细化。
- **L1-5 批量打开性能基线（S）**：测量整批 `.pid` 导入耗时与热点。

### 6.3 阶段 2：导出线与预检（L4 + L5 + LX）

- **L4-0（[#111](https://github.com/happyrust/pid-cad/issues/111)）XML 规格摘要与校验器（M）**：落地 `assets/pid-publish-spec.json`、`src/io/pid_publish_check.rs` 与 `scripts/sppid/xml_spec_check.py`；支持严格厂商字节格式与结构语义双层检查，全批已通过样本零违规，7 类故意改坏逐条指明文件、位置、期望与实际。
- **L4-1 统一模型到 XML 发布输入层（L）**：按 `L0-8` Q8 结论对接 `pid-parse::publish` 结构并附加严格厂商字节格式化包装。
- **L4-2 类映射与接口属性模板（L）**、**L4-3 UID 与 `GraphicOID` 统一分配器（M）**、**L4-4 管线/端口/连通/分支点/页间连接符生成（L）**、**L4-5 码表值/公称直径/组合名称规则（M）**、**L4-6 导出命令与覆盖报告（M）**、**L4-7 参考 XML 白名单比对门禁（M）**。
- **L5-1 写入面普查（M）**、**L5-2 受控样本采集（M）**、**L5-3 种子文件策略与 CFB 流级往返验证（M）**、**L5-4 按记录族推进 `.pid` 写出器（L）**、**L5-5 索引表与引用图一致性自检（L）**、**L5-6 三层往返验证（M）**、**L5-7 SPPID 打开验证（M）**。
- **LX-1..LX-9**：受控环境快照与通道、运行器自动化、接受度实验 E0–E3、预检三关命令化与外部平台减法送测。

### 6.4 阶段 3–5：最小闭环、准确率与界面深化、总验收（L2 + L3 + L6）

- **阶段 3（最小闭环）**：在 `pid-symbol-identity` 合入 `main` 后推进 `L3-0`、`L3-1`（高频前 50 种变体）、`L2-1`（M1/M2/M5）、`L4-6`、`L5-8`，跑通单张简单图 `DWG → 识别 → 模型 → XML + .pid → 预检`。
- **阶段 4（准确率与点选编辑）**：`L3-1` 全量、`L3-2..L3-8`、`L2-1`（M3/M4）、`L2-2..L2-7`。
- **阶段 5（批量整合与总验收）**：`L5-9`、`L6-1`、`L6-2`、`L6-3`。

---

## 7. 当前第一批执行顺序（与并行工作树零文件重叠）

1. **`L0-6`（`#109`）**：记录当前 `main @ 6377fe233` 四组门禁基线读数与差异归因；
2. **`L0-2`（`#107`）**：实现并验证 `corpus_index.py` + `corpus-layout.md` + `test_corpus_index.py`；
3. **`L4-0`（`#111`）**：实现并验证 `assets/pid-publish-spec.json` + `src/io/pid_publish_check.rs` + `scripts/sppid/xml_spec_check.py`；
4. **`L0-8`**：对照 `pid-parse @ a85fa10` 源码逐条完成 Q1–Q13 核实报告（`03-parse-answers.md`）；
5. 随后接 `#108`（度量工具集）、`#113`（符号缺失清单）、`#114`（`.sym` 解码普查）。
