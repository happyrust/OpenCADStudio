"""Structural contract tests for the reference-product baseline documents.

Covers VAL-DOC-003 (reference-product-summary.md) and VAL-DOC-004
(source-index.md). These tests read the committed files on disk; the
negative controls build scratch documents so a passing run proves the
validators can fail.
"""
from __future__ import annotations

import pytest

from conftest import DOCS
from docs_contract import (
    MODULE_FIELDS,
    NEW_DESIGN,
    NINE_MODULES,
    SOURCE_AREAS,
    UNDOCUMENTED_CAPABILITIES,
    allowed_sections,
    analysis_dir,
    analysis_section_tokens,
    area_counts,
    field_present,
    find_table,
    manual_heading_numbers,
    manual_text_path,
    module_sections,
    raw_sections_at,
    read_manual_text,
    section_body,
    sections_at,
    source_index_rows,
    validate_reference_summary,
    validate_source_index,
)

REFERENCE = DOCS / "reference"
SUMMARY = REFERENCE / "reference-product-summary.md"
INDEX = REFERENCE / "source-index.md"


def _text(path) -> str:
    assert path.is_file(), "missing deliverable: " + str(path)
    return path.read_text(encoding="utf-8")


def _synthetic_summary() -> str:
    lines = [
        "# 参考产品能力基线",
        "",
        "## 模块能力基线",
        "",
    ]
    for module in NINE_MODULES:
        lines += [
            "### " + module,
            "",
            "- **能力描述**：测试合成的能力描述。",
            "- **关键对象**：测试对象。",
            "- **实现方式**：测试实现方式。",
            "- **手册依据**：§5.1.4.3.1",
            "",
        ]
    lines += ["## 无文档支撑的能力", ""]
    for capability in UNDOCUMENTED_CAPABILITIES:
        lines += [
            "### " + capability,
            "",
            "手册仅有标题或一句话提及，无数据模型、无界面、无规则定义；"
            "本规格将其定义为%s，并在需求文档中给出可判定验收标准。" % NEW_DESIGN,
            "",
        ]
    return "\n".join(lines) + "\n"


def _synthetic_index(areas=None, rows_per_area=4, source="§5.1.4.3.1") -> str:
    areas = list(areas if areas is not None else SOURCE_AREAS)
    lines = [
        "# 需求来源索引",
        "",
        "## 区域代码",
        "",
        "| 区域代码 | 需求文档 | 范围 |",
        "| --- | --- | --- |",
    ]
    for code in areas:
        lines.append("| %s | requirements/x.md | 范围说明 |" % code)
    lines += [
        "",
        "## 来源映射表",
        "",
        "| 区域 | 主题 | 来源 | 说明 |",
        "| --- | --- | --- | --- |",
    ]
    for code in areas:
        for n in range(rows_per_area):
            lines.append(
                "| %s | 主题%d | %s | 该章节记录了测试用的说明文本，长度足够。 |"
                % (code, n, source)
            )
    return "\n".join(lines) + "\n"


# ---------------------------------------------------------------------------
# reference-product-summary.md
# ---------------------------------------------------------------------------


def test_structure_section_parser_tracks_level3_bodies():
    text = "## A\n\n### one\n\nbody one\n\n### two\n\nbody two\n"
    parsed = dict(sections_at(text, 3))
    assert parsed == {"one": "\nbody one\n", "two": "\nbody two"}, parsed


def test_structure_reference_summary_has_nine_module_sections():
    text = _text(SUMMARY)
    modules = module_sections(text)
    missing = [m for m in NINE_MODULES if m not in modules]
    assert missing == [], "missing module sections: %s" % missing
    assert len(modules) == len(NINE_MODULES), (
        "expected %d module sections, matched %d: %s"
        % (len(NINE_MODULES), len(modules), sorted(modules))
    )


@pytest.mark.parametrize("module", NINE_MODULES)
def test_structure_module_section_has_four_fields(module):
    text = _text(SUMMARY)
    modules = module_sections(text)
    assert module in modules, "no level-3 section for module: " + module
    body = modules[module]
    for field in MODULE_FIELDS:
        assert field_present(body, field), (
            "module section %s is missing the field %s" % (module, field)
        )
    cited = body.split("手册依据", 1)[1]
    assert "§" in cited or NEW_DESIGN in cited, (
        "module section %s cites neither a manual section nor %s"
        % (module, NEW_DESIGN)
    )


def test_structure_module_names_are_verbatim():
    text = _text(SUMMARY)
    headings = [name for name, _ in raw_sections_at(text, 3)]
    for module in NINE_MODULES:
        matches = [h for h in headings if module in h]
        assert matches, (
            "no heading contains the normative module name verbatim: " + module
        )
    assert not any("2D绘图" in h for h in headings), (
        "headings use 2D绘图 without the required space"
    )
    assert not any("PID绘图" in h for h in headings), (
        "headings use PID绘图 without the required space"
    )


def test_structure_reference_summary_lists_undocumented_capabilities():
    text = _text(SUMMARY)
    section = section_body(text, 2, "无文档支撑的能力")
    assert section is not None, "missing H2 section 无文档支撑的能力"
    missing = [c for c in UNDOCUMENTED_CAPABILITIES if c not in section]
    assert missing == [], (
        "无文档支撑的能力 does not name: %s" % missing
    )
    for phrase in ("无数据模型", "无界面", "无规则定义"):
        assert phrase in section, "the section never states " + phrase


def test_structure_reference_summary_cites_only_verified_sections():
    text = _text(SUMMARY)
    allowed = set(allowed_sections())
    errors = validate_reference_summary(text, allowed)
    assert errors == [], "; ".join(errors)


def test_structure_reference_validator_rejects_missing_module():
    text = _synthetic_summary()
    broken = text.replace("### 数据发布", "### 其他模块")
    errors = validate_reference_summary(broken)
    assert any("数据发布" in e for e in errors), (
        "validator accepted a summary without the 数据发布 module: %s" % errors
    )


def test_structure_reference_validator_rejects_missing_field():
    text = _synthetic_summary().replace(
        "- **实现方式**：测试实现方式。", "- 实现方式：测试实现方式。", 1
    )
    errors = validate_reference_summary(text)
    assert any("实现方式" in e for e in errors), (
        "validator accepted a module section without the 实现方式 field: %s"
        % errors
    )


def test_structure_reference_validator_rejects_fabricated_section():
    text = _synthetic_summary().replace("§5.1.4.3.1", "§9.9.9.9")
    errors = validate_reference_summary(text)
    assert any("9.9.9.9" in e for e in errors), (
        "validator accepted a fabricated manual section: %s" % errors
    )


def test_structure_reference_validator_rejects_missing_gap_phrases():
    text = _synthetic_summary().replace("无数据模型、无界面、无规则定义", "没有任何说明")
    errors = validate_reference_summary(text)
    assert any("无数据模型" in e for e in errors), (
        "validator accepted a gap entry without the required phrases: %s" % errors
    )


# ---------------------------------------------------------------------------
# source-index.md
# ---------------------------------------------------------------------------


def test_structure_source_index_has_required_columns_and_rows():
    text = _text(INDEX)
    table = find_table(text, ["区域", "主题", "来源", "说明"])
    assert table is not None, (
        "source-index.md has no table with header 区域 | 主题 | 来源 | 说明"
    )
    rows = table["rows"]
    assert len(rows) >= 30, (
        "source index has %d mapping rows, the contract requires at least 30"
        % len(rows)
    )
    assert {len(r) for r in rows} == {4}, (
        "source index rows do not all have four cells"
    )


def test_structure_source_index_area_floors():
    text = _text(INDEX)
    counts = area_counts(text)
    assert set(counts) == set(SOURCE_AREAS), "area enum drifted: %s" % counts
    thin = {code: n for code, n in counts.items() if n < 3}
    assert thin == {}, "areas below the three-row floor: %s" % thin
    total = sum(counts.values())
    assert total >= 30, "only %d rows counted across the nine areas" % total


def test_structure_source_index_sources_are_manual_sections_or_new_design():
    text = _text(INDEX)
    allowed = set(allowed_sections())
    rows, parse_errors = source_index_rows(text)
    assert parse_errors == [], "; ".join(parse_errors)
    bad = []
    for row in rows:
        source = row["来源"]
        if source == NEW_DESIGN:
            continue
        if not source.startswith("§"):
            bad.append(source)
            continue
        if source.lstrip("§") not in allowed:
            bad.append(source)
    assert bad == [], "来源 values outside the verified whitelist: %s" % bad
    new_design_rows = [r for r in rows if r["来源"] == NEW_DESIGN]
    assert new_design_rows, (
        "no row is labelled %s; the reference product cannot cover every area"
        % NEW_DESIGN
    )


def test_structure_source_index_area_codes_table_matches_enum():
    text = _text(INDEX)
    table = find_table(text, ["区域代码", "需求文档", "范围"])
    assert table is not None, "missing the 区域代码 table"
    codes = [row[0] for row in table["rows"]]
    assert codes == SOURCE_AREAS, (
        "区域代码 rows differ from the fixed enum: %s" % codes
    )


def test_structure_manual_section_whitelist_exists_in_analysis():
    """Every whitelisted section must be traceable to the manual.

    The analysis extract collapses the symbol-editor block into the range
    "§5.1.4.3.2.2.3.2–5", so §5.1.4.3.2.2.3.4 and §5.1.4.3.2.2.3.7 appear
    there only as an interval and never as tokens. A body heading line in the
    raw manual is strictly stronger evidence than a token, so a section
    qualifies on either source.
    """
    derived = analysis_section_tokens()
    corpus = read_manual_text()
    headings = manual_heading_numbers(corpus) if corpus is not None else set()
    if not derived and not headings:
        pytest.skip(
            "neither the analysis extract (%s) nor the raw manual (%s) is "
            "available; whitelist cross-check skipped"
            % (analysis_dir(), manual_text_path())
        )
    unknown = [
        s for s in allowed_sections() if s not in derived and s not in headings
    ]
    assert unknown == [], (
        "whitelist entries found neither in the analysis text nor as a manual "
        "heading line (fabricated?): %s" % unknown
    )


def test_structure_source_index_validator_rejects_thin_area():
    text = _synthetic_index(rows_per_area=4)
    broken = text.replace("| SYM |", "| XXX |")
    errors = validate_source_index(broken)
    assert any("SYM" in e for e in errors), (
        "validator accepted an area below the floor: %s" % errors
    )


def test_structure_source_index_validator_rejects_unknown_source():
    text = _synthetic_index(source="§9.9.9.9")
    errors = validate_source_index(text)
    assert any("9.9.9.9" in e for e in errors), (
        "validator accepted a fabricated section reference: %s" % errors
    )
    text = _synthetic_index(source="见手册")
    errors = validate_source_index(text)
    assert any("新增设计" in e for e in errors), (
        "validator accepted a non-§, non-新增设计 source: %s" % errors
    )


def test_structure_source_index_validator_accepts_a_conforming_table():
    errors = validate_source_index(_synthetic_index())
    assert errors == [], "validator rejected a conforming table: %s" % errors
