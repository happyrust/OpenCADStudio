"""Structure contract tests for ``docs/pid-platform/PRD.md``.

Covers VAL-PRD-001 (eight chapters with substance), VAL-PRD-002 (nine-module
table with disposition and layer) and VAL-PRD-003 (decidable roadmap plus the
four architecture differences). The negative controls below mutate a synthetic
but valid PRD so a validator that always returns ``[]`` cannot pass this file.
"""
from __future__ import annotations

import pytest

from conftest import DOCS
from docs_contract import NINE_MODULES
from prd_contract import (
    LAYERS,
    PRD_CHAPTERS,
    PRD_DIFF_SECTION,
    chapter_stats,
    prd_chapter_stats,
    validate_prd,
)

PRD = DOCS / "PRD.md"


# ---------------------------------------------------------------------------
# Real document
# ---------------------------------------------------------------------------

@pytest.fixture(scope="module")
def prd_text() -> str:
    assert PRD.exists(), "PRD.md missing at " + str(PRD)
    text = PRD.read_text(encoding="utf-8")
    assert len(text) > 3000, (
        "PRD.md is %d characters; too small for an eight-chapter PRD" % len(text)
    )
    return text


def test_prd_structure_document_satisfies_the_contract(prd_text):
    errors = validate_prd(prd_text)
    assert errors == [], "PRD contract violations:\n" + "\n".join(errors)


def test_prd_structure_chapter_stats_cover_all_eight(prd_text):
    stats = prd_chapter_stats(prd_text)
    missing = [chapter for chapter in PRD_CHAPTERS if chapter not in stats]
    assert missing == [], "PRD chapters without a body: %s" % missing
    thin = {
        chapter: value
        for chapter, value in stats.items()
        if value["chars"] < 150 and value["tables"] < 1
    }
    assert thin == {}, (
        "these chapters have neither 150 prose characters nor a table: %s" % thin
    )
    summary = ", ".join(
        "%s=%d字/%d表" % (chapter, value["chars"], value["tables"])
        for chapter, value in stats.items()
    )
    assert len(stats) == 8, "expected eight chapters, got %d (%s)" % (len(stats), summary)


def test_prd_structure_nine_module_table_is_complete(prd_text):
    from prd_contract import _find_table, _row_dicts, MODULE_COLUMNS
    from docs_contract import section_body

    body = section_body(prd_text, 2, "功能架构")
    assert body is not None, "功能架构 chapter is missing"
    table, header = _find_table(body, MODULE_COLUMNS)
    assert table is not None, (
        "功能架构 has no table with columns 模块 | 处置 | 所属层 | 说明"
    )
    rows, row_errors = _row_dicts(table, header, "nine-module table")
    assert row_errors == [], row_errors
    assert len(rows) == 9, "nine-module table has %d rows" % len(rows)
    names = [row["模块"].strip() for row in rows]
    assert names == NINE_MODULES or sorted(names) == sorted(NINE_MODULES), (
        "module names must be the nine normative names verbatim; got %s" % names
    )
    dispositions = [row["处置"].strip() for row in rows]
    assert all(d in ("复用", "改造", "新增") for d in dispositions), dispositions
    assert dispositions.count("复用") >= 1, dispositions
    assert dispositions.count("新增") >= 3, dispositions
    layers = [row["所属层"].strip() for row in rows]
    assert all(layer in LAYERS for layer in layers), (
        "illegal layer names: %s" % [l for l in layers if l not in LAYERS]
    )


def test_prd_structure_roadmap_exit_conditions_are_decidable(prd_text):
    from docs_contract import section_body
    from prd_contract import ROADMAP_COLUMNS, _find_table, _has_number_or_state, _row_dicts

    body = section_body(prd_text, 2, "路线图")
    assert body is not None, "路线图 chapter is missing"
    table, header = _find_table(body, ROADMAP_COLUMNS)
    assert table is not None, (
        "路线图 has no table with columns 阶段 | 阶段目标 | 交付能力 | 退出条件 | 覆盖区域"
    )
    rows, row_errors = _row_dicts(table, header, "roadmap table")
    assert row_errors == [], row_errors
    assert len(rows) >= 5, "roadmap has %d rows, expected at least 5" % len(rows)
    for index, row in enumerate(rows, start=2):
        exit_condition = row["退出条件"].strip()
        assert exit_condition, "roadmap row %d has an empty 退出条件" % index
        assert "基本完成" not in exit_condition and "大致可用" not in exit_condition, (
            "roadmap row %d uses a vague exit condition: %s" % (index, exit_condition)
        )
        assert _has_number_or_state(exit_condition), (
            "roadmap row %d has no number or observable state: %s"
            % (index, exit_condition)
        )


def test_prd_structure_difference_table_states_the_four_decisions(prd_text):
    from docs_contract import section_body
    from prd_contract import DIFF_COLUMNS, _find_table, _row_dicts

    body = section_body(prd_text, 2, PRD_DIFF_SECTION)
    assert body is not None, "missing section " + PRD_DIFF_SECTION
    table, header = _find_table(body, DIFF_COLUMNS)
    assert table is not None, (
        "architecture-difference table must use columns 维度 | 参考产品 | 本产品 | 理由"
    )
    rows, row_errors = _row_dicts(table, header, "architecture-difference table")
    assert row_errors == [], row_errors
    assert len(rows) == 4, (
        "architecture-difference table must have exactly four rows, got %d" % len(rows)
    )


def test_prd_structure_metrics_have_targets_and_measurement(prd_text):
    from docs_contract import section_body
    from prd_contract import METRIC_COLUMNS, _find_table, _row_dicts

    body = section_body(prd_text, 2, "成功指标")
    assert body is not None, "成功指标 chapter is missing"
    table, header = _find_table(body, METRIC_COLUMNS)
    assert table is not None, (
        "成功指标 has no table with columns 指标 | 目标值 | 度量方式"
    )
    rows, row_errors = _row_dicts(table, header, "metrics table")
    assert row_errors == [], row_errors
    assert len(rows) >= 5, "metrics table has %d rows, expected at least 5" % len(rows)
    for index, row in enumerate(rows, start=2):
        assert row["指标"].strip(), "metrics row %d has an empty 指标" % index
        assert row["目标值"].strip(), (
            "metrics row %d (%s) has an empty 目标值" % (index, row["指标"].strip())
        )
        assert len(row["度量方式"].strip()) >= 6, (
            "metrics row %d (%s) has too short a 度量方式"
            % (index, row["指标"].strip())
        )


# ---------------------------------------------------------------------------
# Synthetic fixture and negative controls
# ---------------------------------------------------------------------------

def _nine_module_rows() -> str:
    dispositions = {
        "业务类型库及属性管理": ("新增", "L2 P&ID 语义"),
        "标准图例定制": ("改造", "L2 P&ID 语义"),
        "项目设置": ("新增", "L2 P&ID 语义"),
        "用户设置": ("改造", "L1 绘图内核"),
        "图纸权限管理": ("新增", "L5 平台"),
        "2D 绘图": ("复用", "L1 绘图内核"),
        "PID 绘图": ("改造", "L2 P&ID 语义"),
        "数据管理": ("新增", "L3 数据服务"),
        "数据发布": ("改造", "L4 校验交付"),
    }
    lines = ["| 模块 | 处置 | 所属层 | 说明 |", "| --- | --- | --- | --- |"]
    for module in NINE_MODULES:
        disposition, layer = dispositions[module]
        lines.append(
            "| %s | %s | %s | 合成夹具对这一模块的处置说明，长度足以通过下限检查。 |"
            % (module, disposition, layer)
        )
    return "\n".join(lines)


VALID_PRD = """# 合成 PRD 夹具

这是用于校验器正负向对照的合成文档，不是交付物。

## 产品定位与价值主张

本产品面向流程工业，图形是数据的外在表现，交付的是带数据的图纸而不是文档。

| 维度 | 传统 P&ID | 本产品 |
| --- | --- | --- |
| 驱动方式 | 图形库 | 数据库与图形库 |
| 标注来源 | 手工文字 | 属性驱动 |
| 交付物 | 文档 | 带数据的图纸 |
| 校验方式 | 人工核对 | 规则引擎 |

## 目标行业与客户

| 行业 | 典型客户 | 典型规模 | 核心诉求 |
| --- | --- | --- | --- |
| 石油化工 | 设计院 | 800 人规模 | 标准图例与出图 |
| 精细化工 | EPC 总包 | 1200 人规模 | 跨专业协同 |
| 电力核电 | 业主 | 单厂 20000 个位号 | 存量图纸接管 |
| 制药 | 设计院 | 300 人规模 | 合规追溯 |

## 目标用户与角色

| 角色 | 职责 | 关键权限 |
| --- | --- | --- |
| 平台管理员 | 维护账号、项目空间与全局配置 | 平台配置 |
| 系统配置管理员 | 维护类型库与符号库 | 系统配置 |
| 项目管理员 | 初始化项目设置与权限 | 项目配置 |
| 工艺设计人员 | 绘制工艺管线与设备 | 读写 |
| 管道设计人员 | 完成管路与管件建模 | 读写 |
| 校对人员 | 核对位号与规则检查结果 | 只读 |

## 应用场景

| 场景 | 触发条件 | 涉及角色 | 涉及模块 | 期望结果 |
| --- | --- | --- | --- | --- |
| 新项目配置 | 项目立项 | 项目管理员 | 项目设置 | 可绘图环境就绪 |
| 设备建模 | 设备到图 | 工艺设计人员 | PID 绘图 | 位号生成 |
| 管线绘制 | 流程确认 | 管道设计人员 | PID 绘图 | 拓扑连通 |
| 规则检查 | 保存图纸 | 校对人员 | 数据管理 | 违规清单 |
| 材料清单 | 交付准备 | 校对人员 | 数据发布 | 清单导出 |

## 功能架构

五层架构自下而上为 L1 绘图内核、L2 P&ID 语义、L3 数据服务、L4 校验交付、L5 平台。

"""+_nine_module_rows()+"""

## 与参考产品的关键架构差异

| 维度 | 参考产品 | 本产品 | 理由 |
| --- | --- | --- | --- |
| 平台层 | 低代码平台，占手册 75% | 平台层收敛为最小可用 | 降低实现成本 |
| 客户端 | 纯 B/S | 桌面 + Web 双端 | 大图纸性能 |
| 数据存储 | 服务端数据库 | 桌面可脱机 | 现场作业 |
| 增量补强 | 无正文定义 | 规则引擎、材料清单与协同均定义为可执行规格 | 竞品空白 |

## 功能模块清单

| 模块 | 核心能力 | 需求区域 | 覆盖原型屏幕 |
| --- | --- | --- | --- |
| 类型库 | 类型树与属性模型 | TYPE | typelib |
| 符号库 | 多套图例标准 | SYM | symbols |
| 绘图 | 管线与设备 | DRW | editor |

## 路线图

| 阶段 | 阶段目标 | 交付能力 | 退出条件 | 覆盖区域 |
| --- | --- | --- | --- | --- |
| P0 | 固化规格 | 文档与原型 | 20 份交付物全部交付 | 文档 |
| P1 | 单机闭环 | 绘图与出图 | 插入 50 个符号后重复位号为 0 | DRW |
| P2 | 项目数据 | 规则与清单 | 规则 20 条可配置 | DATA |
| P3 | 协同平台 | 权限与审计 | 覆盖 3 个角色 | COLL |
| P4 | 迁移互操作 | 导入与接口 | 解析率 98% | MIG |

## 成功指标

| 指标 | 目标值 | 度量方式 | 关联区域 |
| --- | --- | --- | --- |
| 位号唯一率 | 100% | 重复位号数除以位号总数 | DATA |
| 规则规模 | 20 条 | 统计可解析的规则行数 | DATA |
| 拦截率 | 100% | 被拦截数除以应拦截数 | DATA |
| 清单唯一性 | 每号一行 | 按位号分组统计重复行 | DELV |
| 解析率 | 98% | 解析出位号的实例数除以总数 | MIG |
"""


def _replace_once(text: str, old: str, new: str) -> str:
    assert old in text, "fixture text does not contain %r" % old
    return text.replace(old, new, 1)


def test_prd_structure_fixture_is_valid():
    assert validate_prd(VALID_PRD) == [], validate_prd(VALID_PRD)


def test_prd_structure_rejects_a_missing_chapter():
    broken = _replace_once(VALID_PRD, "## 成功指标", "## 目标与收益")
    errors = validate_prd(broken)
    assert any("成功指标" in error for error in errors), errors


def test_prd_structure_rejects_an_invented_module_name():
    broken = _replace_once(VALID_PRD, "| 标准图例定制 |", "| 符号定制 |")
    errors = validate_prd(broken)
    assert any("标准图例定制" in error or "invents module names" in error for error in errors), errors


def test_prd_structure_rejects_a_vague_exit_condition():
    broken = _replace_once(
        VALID_PRD, "| 20 份交付物全部交付 |", "| 基本完成 |"
    )
    errors = validate_prd(broken)
    assert any("vague exit condition" in error for error in errors), errors


def test_prd_structure_rejects_a_three_row_difference_table():
    broken = _replace_once(
        VALID_PRD,
        "| 数据存储 | 服务端数据库 | 桌面可脱机 | 现场作业 |\n",
        "",
    )
    errors = validate_prd(broken)
    assert any("exactly 4" in error for error in errors), errors


def test_prd_structure_rejects_a_chapter_with_neither_prose_nor_table():
    broken = _replace_once(
        VALID_PRD,
        "| 制药 | 设计院 | 300 人规模 | 合规追溯 |\n",
        "",
    )
    # Removing one industry row keeps a table, so strip the whole chapter body.
    start = broken.index("## 目标行业与客户")
    end = broken.index("## 目标用户与角色")
    broken = broken[:start] + "## 目标行业与客户\n\n太短。\n\n" + broken[end:]
    errors = validate_prd(broken)
    assert any("目标行业与客户" in error for error in errors), errors


def test_prd_structure_rejects_too_few_metrics():
    broken = VALID_PRD
    for name in ("规则规模", "拦截率", "清单唯一性", "解析率"):
        lines = [
            line
            for line in broken.splitlines()
            if not line.startswith("| %s |" % name)
        ]
        broken = "\n".join(lines) + "\n"
    errors = validate_prd(broken)
    assert any("metrics table has" in error for error in errors), errors
