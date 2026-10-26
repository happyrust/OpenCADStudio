"""Machine-checkable contract for ``docs/pid-platform/PRD.md``.

Covers the three PRD assertions:

* ``VAL-PRD-001`` — the eight mandated chapters exist and each one carries
  either at least 150 characters of prose or at least one Markdown table.
* ``VAL-PRD-002`` — the 功能架构 chapter carries the nine-module table with
  the normative module names, a legal 处置 and a legal 所属层, with at least
  one 复用 and at least three 新增.
* ``VAL-PRD-003`` — the 路线图 table's exit conditions are decidable, and the
  architecture-difference section has the four mandated rows.

Pure stdlib, like the other ``*_contract`` modules: every validator is a pure
function of the document text so the tests can point it at scratch content.
"""
from __future__ import annotations

import re

from docs_contract import (
    NINE_MODULES,
    SOURCE_AREAS,
    h1_count,
    parse_tables,
    section_body,
)

PRD_CHAPTERS = [
    "产品定位与价值主张",
    "目标行业与客户",
    "目标用户与角色",
    "应用场景",
    "功能架构",
    "功能模块清单",
    "路线图",
    "成功指标",
]

PRD_DIFF_SECTION = "与参考产品的关键架构差异"

MODULE_COLUMNS = ["模块", "处置", "所属层", "说明"]
ROADMAP_COLUMNS = ["阶段", "阶段目标", "交付能力", "退出条件", "覆盖区域"]
DIFF_COLUMNS = ["维度", "参考产品", "本产品", "理由"]
METRIC_COLUMNS = ["指标", "目标值", "度量方式"]

DISPOSITIONS = ("复用", "改造", "新增")
# The five layer names are pinned by VAL-PRD-002 and reused verbatim by the
# architecture and requirements documents.
LAYERS = (
    "L1 绘图内核",
    "L2 P&ID 语义",
    "L3 数据服务",
    "L4 校验交付",
    "L5 平台",
)

MIN_PROSE_CHARS = 150
MIN_MODULE_ROWS = 9
MIN_REUSE_ROWS = 1
MIN_NEW_ROWS = 3
MIN_DIFF_ROWS = 4
MIN_ROADMAP_ROWS = 5
MIN_METRIC_ROWS = 5
MIN_INDUSTRIES = 4
MIN_ROLES = 6
MIN_SCENARIOS = 5

# "基本完成" and "大致可用" are the two vague exit conditions the contract
# names. Keep the list closed so a legitimate phase name is never rejected.
VAGUE_EXIT_PHRASES = ("基本完成", "大致可用")

# A decidable exit condition states a number or an observable end state.
# Digits cover counts and thresholds; the words cover states such as
# "三个通道各有 1 条用例通过" or "判定结果与权限矩阵一致".
OBSERVABLE_STATE_WORDS = (
    "通过",
    "一致",
    "不为空",
    "不再出现",
    "存在",
    "出现",
    "消失",
    "退出码",
)

# The four decisions VAL-PRD-003 requires the difference table to cover.
DIFF_DECISIONS = [
    ("平台层收敛为最小可用", ("平台层", "最小可用")),
    ("桌面 + Web 双端", ("桌面", "web")),
    ("桌面可脱机", ("脱机",)),
    ("规则引擎、材料清单、协同为增量补强", ("规则引擎", "材料清单", "协同")),
]

_MARKUP_RE = re.compile(r"[`*_\u3000\s]")
_DIGIT_RE = re.compile(r"\d")


def _clean_header_cell(cell: str) -> str:
    """Strip markdown decoration and spaces from a header cell for matching."""
    return _MARKUP_RE.sub("", cell)


def _header(table: dict) -> list:
    return [_clean_header_cell(c) for c in table["header"]]


def _find_table(body: str, required: list):
    """First table whose header carries every required column, plus header."""
    for table in parse_tables(body):
        header = _header(table)
        if all(column in header for column in required):
            return table, header
    return None, None


def _row_dicts(table: dict, header: list, label: str):
    rows = []
    errors = []
    for index, cells in enumerate(table["rows"], start=2):
        if len(cells) != len(header):
            errors.append(
                "%s row %d has %d cells, expected %d: %s"
                % (label, index, len(cells), len(header), " | ".join(cells))
            )
            continue
        rows.append(dict(zip(header, cells)))
    return rows, errors


def chapter_stats(body: str) -> dict:
    """Return {'chars': prose character count, 'tables': table count}."""
    prose = []
    for line in body.splitlines():
        stripped = line.strip()
        if not stripped or stripped.startswith("|"):
            continue
        prose.append(stripped)
    text = "".join(prose)
    text = re.sub(r"[`*_#>]", "", text)
    return {
        "chars": len(re.sub(r"\s", "", text)),
        "tables": len(parse_tables(body)),
    }


def prd_chapter_stats(text: str) -> dict:
    """Per-chapter {'chars', 'tables'} for the eight mandated chapters."""
    stats = {}
    for chapter in PRD_CHAPTERS:
        body = section_body(text, 2, chapter)
        if body is not None:
            stats[chapter] = chapter_stats(body)
    return stats


def _has_number_or_state(cell: str) -> bool:
    if _DIGIT_RE.search(cell):
        return True
    return any(word in cell for word in OBSERVABLE_STATE_WORDS)


def _validate_chapters(text: str, errors: list) -> dict:
    bodies = {}
    for chapter in PRD_CHAPTERS:
        body = section_body(text, 2, chapter)
        if body is None:
            errors.append("missing H2 chapter " + chapter)
            continue
        bodies[chapter] = body
        stats = chapter_stats(body)
        if stats["chars"] < MIN_PROSE_CHARS and stats["tables"] < 1:
            errors.append(
                "chapter %s has %d prose characters and %d tables; the "
                "contract needs at least %d characters or at least one table"
                % (chapter, stats["chars"], stats["tables"], MIN_PROSE_CHARS)
            )
    return bodies


def _validate_modules(body: str, errors: list) -> list:
    table, header = _find_table(body, MODULE_COLUMNS)
    if table is None:
        errors.append(
            "功能架构 chapter has no table with columns 模块 | 处置 | 所属层 | 说明"
        )
        return []
    rows, row_errors = _row_dicts(table, header, "nine-module table")
    errors.extend(row_errors)
    if len(rows) != MIN_MODULE_ROWS:
        errors.append(
            "nine-module table has %d data rows, expected exactly %d"
            % (len(rows), MIN_MODULE_ROWS)
        )
    names = [row["模块"].strip() for row in rows]
    duplicates = sorted({name for name in names if names.count(name) > 1})
    if duplicates:
        errors.append("nine-module table repeats modules: " + "、".join(duplicates))
    missing = [name for name in NINE_MODULES if name not in names]
    if missing:
        errors.append(
            "nine-module table is missing normative module names: " + "、".join(missing)
        )
    unknown = [name for name in names if name not in NINE_MODULES]
    if unknown:
        errors.append(
            "nine-module table invents module names outside VAL-DOC-003: "
            + "、".join(unknown)
        )
    for index, row in enumerate(rows, start=2):
        module = row["模块"].strip()
        disposition = row["处置"].strip()
        layer = row["所属层"].strip()
        description = row["说明"].strip()
        if disposition not in DISPOSITIONS:
            errors.append(
                "nine-module table row %d (%s) has 处置 %r outside 复用/改造/新增"
                % (index, module, disposition)
            )
        if layer not in LAYERS:
            errors.append(
                "nine-module table row %d (%s) has 所属层 %r outside the five "
                "layer names" % (index, module, layer)
            )
        if len(description) < 10:
            errors.append(
                "nine-module table row %d (%s) has a 说明 shorter than 10 "
                "characters" % (index, module)
            )
    reuse = sum(1 for row in rows if row["处置"].strip() == "复用")
    new = sum(1 for row in rows if row["处置"].strip() == "新增")
    if reuse < MIN_REUSE_ROWS:
        errors.append(
            "nine-module table marks %d modules 复用, the contract requires at "
            "least %d" % (reuse, MIN_REUSE_ROWS)
        )
    if new < MIN_NEW_ROWS:
        errors.append(
            "nine-module table marks %d modules 新增, the contract requires at "
            "least %d" % (new, MIN_NEW_ROWS)
        )
    return rows


def _validate_diff(text: str, errors: list) -> list:
    body = section_body(text, 2, PRD_DIFF_SECTION)
    if body is None:
        errors.append("missing H2 section " + PRD_DIFF_SECTION)
        return []
    table, header = _find_table(body, DIFF_COLUMNS)
    if table is None:
        errors.append(
            "architecture-difference section has no table with columns "
            "维度 | 参考产品 | 本产品 | 理由"
        )
        return []
    rows, row_errors = _row_dicts(table, header, "architecture-difference table")
    errors.extend(row_errors)
    if len(rows) != MIN_DIFF_ROWS:
        errors.append(
            "architecture-difference table has %d data rows, expected exactly %d"
            % (len(rows), MIN_DIFF_ROWS)
        )
    for index, row in enumerate(rows, start=2):
        empty = [column for column in DIFF_COLUMNS if not row[column].strip()]
        if empty:
            errors.append(
                "architecture-difference row %d leaves these columns empty: %s"
                % (index, "、".join(empty))
            )
    haystack = " ".join(
        row[column].strip().lower() for row in rows for column in DIFF_COLUMNS
    )
    for label, keywords in DIFF_DECISIONS:
        missing = [keyword for keyword in keywords if keyword.lower() not in haystack]
        if missing:
            errors.append(
                "architecture-difference table does not state the decision "
                "'%s'; missing: %s" % (label, "、".join(missing))
            )
    return rows


def _validate_roadmap(body: str, errors: list) -> list:
    table, header = _find_table(body, ROADMAP_COLUMNS)
    if table is None:
        errors.append(
            "路线图 chapter has no table with columns 阶段 | 阶段目标 | "
            "交付能力 | 退出条件 | 覆盖区域"
        )
        return []
    rows, row_errors = _row_dicts(table, header, "roadmap table")
    errors.extend(row_errors)
    if len(rows) < MIN_ROADMAP_ROWS:
        errors.append(
            "roadmap table has %d data rows, the contract requires at least %d"
            % (len(rows), MIN_ROADMAP_ROWS)
        )
    for index, row in enumerate(rows, start=2):
        phase = row["阶段"].strip()
        exit_condition = row["退出条件"].strip()
        coverage = row["覆盖区域"].strip()
        if not exit_condition:
            errors.append("roadmap row %d (%s) has an empty 退出条件" % (index, phase))
        else:
            vague = [p for p in VAGUE_EXIT_PHRASES if p in exit_condition]
            if vague:
                errors.append(
                    "roadmap row %d (%s) uses a vague exit condition: %s"
                    % (index, phase, "、".join(vague))
                )
            elif not _has_number_or_state(exit_condition):
                errors.append(
                    "roadmap row %d (%s) has a 退出条件 with neither a number "
                    "nor an observable state: %s" % (index, phase, exit_condition)
                )
        covered = list(SOURCE_AREAS) + ["文档", "原型"]
        if not any(token in coverage for token in covered):
            errors.append(
                "roadmap row %d (%s) has 覆盖区域 %r without an area code or "
                "文档/原型" % (index, phase, coverage)
            )
        if len(row["阶段目标"].strip()) < 4:
            errors.append("roadmap row %d (%s) has too short a 阶段目标" % (index, phase))
        if len(row["交付能力"].strip()) < 4:
            errors.append("roadmap row %d (%s) has too short a 交付能力" % (index, phase))
    return rows


def _validate_metrics(body: str, errors: list) -> list:
    table, header = _find_table(body, METRIC_COLUMNS)
    if table is None:
        errors.append(
            "成功指标 chapter has no table with columns 指标 | 目标值 | 度量方式"
        )
        return []
    rows, row_errors = _row_dicts(table, header, "metrics table")
    errors.extend(row_errors)
    if len(rows) < MIN_METRIC_ROWS:
        errors.append(
            "metrics table has %d data rows, the contract requires at least %d"
            % (len(rows), MIN_METRIC_ROWS)
        )
    for index, row in enumerate(rows, start=2):
        name = row["指标"].strip()
        target = row["目标值"].strip()
        method = row["度量方式"].strip()
        if not name:
            errors.append("metrics row %d has an empty 指标" % index)
        if not target:
            errors.append("metrics row %d (%s) has an empty 目标值" % (index, name))
        if len(method) < 6:
            errors.append(
                "metrics row %d (%s) has a 度量方式 shorter than 6 characters"
                % (index, name)
            )
    return rows


def _validate_industries(body: str, errors: list) -> list:
    table, header = _find_table(body, ["行业", "典型规模"])
    if table is None:
        errors.append("目标行业与客户 chapter has no table with 行业 and 典型规模")
        return []
    rows, row_errors = _row_dicts(table, header, "industry table")
    errors.extend(row_errors)
    if len(rows) < MIN_INDUSTRIES:
        errors.append(
            "industry table has %d data rows, the contract requires at least %d"
            % (len(rows), MIN_INDUSTRIES)
        )
    for index, row in enumerate(rows, start=2):
        scale = row["典型规模"].strip()
        if not _DIGIT_RE.search(scale):
            errors.append(
                "industry row %d (%s) has a 典型规模 without a number: %r"
                % (index, row.get("行业", "").strip(), scale)
            )
    return rows


def _validate_roles(body: str, errors: list) -> list:
    table, header = _find_table(body, ["角色", "职责"])
    if table is None:
        errors.append("目标用户与角色 chapter has no table with 角色 and 职责")
        return []
    rows, row_errors = _row_dicts(table, header, "role table")
    errors.extend(row_errors)
    if len(rows) < MIN_ROLES:
        errors.append(
            "role table has %d data rows, the contract requires at least %d"
            % (len(rows), MIN_ROLES)
        )
    for index, row in enumerate(rows, start=2):
        duty = row["职责"].strip()
        if len(duty) < 8:
            errors.append(
                "role row %d (%s) has a 职责 shorter than 8 characters"
                % (index, row["角色"].strip())
            )
    return rows


def _validate_scenarios(body: str, errors: list) -> list:
    table, header = _find_table(body, ["场景", "涉及角色", "涉及模块"])
    if table is None:
        errors.append(
            "应用场景 chapter has no table with 场景, 涉及角色 and 涉及模块"
        )
        return []
    rows, row_errors = _row_dicts(table, header, "scenario table")
    errors.extend(row_errors)
    if len(rows) < MIN_SCENARIOS:
        errors.append(
            "scenario table has %d data rows, the contract requires at least %d"
            % (len(rows), MIN_SCENARIOS)
        )
    for index, row in enumerate(rows, start=2):
        name = row["场景"].strip()
        for column in ("涉及角色", "涉及模块"):
            if len(row[column].strip()) < 2:
                errors.append(
                    "scenario row %d (%s) has an empty %s" % (index, name, column)
                )
    return rows


def validate_prd(text: str) -> list:
    """Return the contract violations of a PRD document (empty when valid)."""
    errors = []
    if h1_count(text) != 1:
        errors.append("PRD.md must contain exactly one H1, found %d" % h1_count(text))
    bodies = _validate_chapters(text, errors)
    if "功能架构" in bodies:
        _validate_modules(bodies["功能架构"], errors)
    _validate_diff(text, errors)
    if "路线图" in bodies:
        _validate_roadmap(bodies["路线图"], errors)
    if "成功指标" in bodies:
        _validate_metrics(bodies["成功指标"], errors)
    if "目标行业与客户" in bodies:
        _validate_industries(bodies["目标行业与客户"], errors)
    if "目标用户与角色" in bodies:
        _validate_roles(bodies["目标用户与角色"], errors)
    if "应用场景" in bodies:
        _validate_scenarios(bodies["应用场景"], errors)
    return errors
