"""Terminology contract tests for docs/pid-platform/glossary.md.

The glossary is the single source of vocabulary for every deliverable, so
these tests assert its shape against the committed file, not against a
hand-written expectation that could drift from disk.
"""
from __future__ import annotations

import pytest

from conftest import DOCS
from docs_contract import (
    ALIAS_CRITERIA_HEADING,
    GLOSSARY_COLUMNS,
    GLOSSARY_TERMS,
    NOT_AN_ALIAS,
    alias_hygiene_errors,
    alias_scan_documents,
    banned_alias_map,
    find_table,
    glossary_rows,
    retired_alias_rows,
    scan_alias_violations,
    section_body,
    split_aliases,
    validate_alias_policy,
    validate_glossary,
)

GLOSSARY = DOCS / "glossary.md"

# Wrong names the alias criteria keep: each names the row's own concept, is not
# ordinary requirement vocabulary, and has no other meaning in the manual.
KEPT_WRONG_NAMES = {
    "页间连接符": ["跨页符", "跨页连接符"],
    "打断符": ["断开符", "跳线"],
    "符号": ["图符", "图形记号"],
    "连接点": ["挂点", "连接锚点"],
    "类型库": ["业务类库", "类型表"],
    "设计规则检查": ["设计校核", "错误校验"],
    "图纸版本快照": ["版本存档", "快照版本"],
}

# Names of genuinely different objects: a valve body is part of a valve, WBS is
# another breakdown standard, a control loop belongs to the control scheme, and
# a line segment is a geometric description.
DIFFERENT_CONCEPTS = ["阀体", "WBS", "控制回路", "线段"]


def _text(path) -> str:
    assert path.is_file(), "missing deliverable: " + str(path)
    return path.read_text(encoding="utf-8")


def _synthetic_glossary(terms, alias: str = "旧称") -> str:
    """A minimal glossary that satisfies the shape rules for ``terms``."""
    lines = [
        "# 测试术语表",
        "",
        "## 唯一词源声明",
        "",
        "本术语表是全部交付物的唯一词源，任何其他文档使用术语前必须先在本表登记；"
        "若其他文档与本表冲突，以本表为准。禁用别名列具有约束力，不得出现在标题、表格与界面标签中。",
        "",
        "## 术语表",
        "",
        "| 中文名 | 英文名 | 定义 | 禁用别名 |",
        "| --- | --- | --- | --- |",
    ]
    for term in terms:
        lines.append(
            "| %s | %s | 由测试合成的定义文本，用于验证契约解析器 | %s |"
            % (term, "Term", alias)
        )
    return "\n".join(lines) + "\n"


def _set_alias(text: str, term: str, alias: str) -> str:
    """Rewrite the 禁用别名 cell of ``term``'s row, leaving everything else."""
    lines = text.splitlines()
    for index, line in enumerate(lines):
        if not line.startswith("| %s |" % term):
            continue
        cells = [cell.strip() for cell in line.strip().strip("|").split("|")]
        assert len(cells) == len(GLOSSARY_COLUMNS), line
        cells[-1] = alias
        lines[index] = "| " + " | ".join(cells) + " |"
        return "\n".join(lines) + "\n"
    raise AssertionError("glossary row not found: " + term)


def _drop_section(text: str, heading: str) -> str:
    kept = []
    skipping = False
    for line in text.splitlines():
        if line.startswith("## "):
            skipping = line[3:].strip() == heading
        if not skipping:
            kept.append(line)
    return "\n".join(kept) + "\n"


def _drop_row(text: str, word: str) -> str:
    lines = text.splitlines()
    kept = [line for line in lines if not line.startswith("| %s |" % word)]
    assert len(kept) == len(lines) - 1, "retired-alias row not found: " + word
    return "\n".join(kept) + "\n"



def test_terminology_glossary_has_four_columns():
    text = _text(GLOSSARY)
    table = find_table(text, GLOSSARY_COLUMNS)
    assert table is not None, (
        "glossary.md has no table with the required header: "
        + " | ".join(GLOSSARY_COLUMNS)
    )
    widths = {len(row) for row in table["rows"]}
    assert widths == {len(GLOSSARY_COLUMNS)}, (
        "glossary rows have inconsistent cell counts: %s" % sorted(widths)
    )


def test_terminology_glossary_covers_all_24_terms():
    text = _text(GLOSSARY)
    rows, _ = glossary_rows(text)
    missing = [t for t in GLOSSARY_TERMS if t not in rows]
    assert missing == [], (
        "glossary is missing %d core concept(s): %s" % (len(missing), missing)
    )
    assert len(rows) >= len(GLOSSARY_TERMS), (
        "glossary has %d rows, expected at least %d"
        % (len(rows), len(GLOSSARY_TERMS))
    )


@pytest.mark.parametrize("term", GLOSSARY_TERMS)
def test_terminology_glossary_row_is_complete(term):
    text = _text(GLOSSARY)
    rows, _ = glossary_rows(text)
    assert term in rows, "glossary row missing: " + term
    row = rows[term]
    assert row["英文名"].strip(), "row %s has an empty 英文名" % term
    assert len(row["定义"].strip()) >= 10, (
        "row %s definition is too short: %r" % (term, row["定义"])
    )
    assert row["禁用别名"].strip(), (
        "row %s has an empty 禁用别名 cell; use — when there is no alias" % term
    )


def test_terminology_glossary_declares_single_vocabulary_source():
    text = _text(GLOSSARY)
    declaration = section_body(text, 2, "唯一词源声明")
    assert declaration is not None, (
        "glossary.md has no H2 section named 唯一词源声明"
    )
    assert "唯一词源" in declaration, (
        "the declaration never uses the words 唯一词源"
    )
    assert "交付物" in declaration, (
        "the declaration does not state its scope over the deliverables"
    )
    assert len(declaration.strip()) >= 80, (
        "the declaration is only %d characters; it must be a binding statement"
        % len(declaration.strip())
    )


def test_terminology_no_blanket_contradiction_between_terms():
    text = _text(GLOSSARY)
    rows, _ = glossary_rows(text)
    aliases = banned_alias_map(text)
    collisions = sorted(
        alias for alias in aliases if alias in rows
    )
    assert collisions == [], (
        "these strings are both a canonical term and a banned alias: %s"
        % collisions
    )
    # A term must not ban one of its own sanctioned short forms.
    for term, short_forms in {
        "类型库": ["类库"],
        "位号": ["TAG"],
        "材料清单": ["BOM"],
    }.items():
        if term in rows:
            banned = split_aliases(rows[term]["禁用别名"])
            clashes = [s for s in short_forms if s in banned]
            assert clashes == [], (
                "row %s bans a sanctioned short form: %s" % (term, clashes)
            )


def test_terminology_alias_criteria_are_documented():
    errors = validate_alias_policy(_text(GLOSSARY))
    assert errors == [], "alias policy violations: %s" % errors


def test_terminology_alias_criteria_section_states_every_rule():
    body = section_body(_text(GLOSSARY), 2, ALIAS_CRITERIA_HEADING)
    assert body is not None, "glossary.md has no H2 section named " + ALIAS_CRITERIA_HEADING
    for keyword in ("同一概念", "非普通工程用词", "不另有所指", "可机械判定"):
        assert keyword in body, "criteria section never states " + keyword
    assert "docs/pid-platform" in body, (
        "the criteria section never names the scan scope docs/pid-platform"
    )


def test_terminology_retired_suspects_carry_their_reasons():
    rows, errors = retired_alias_rows(_text(GLOSSARY))
    assert errors == [], errors
    listed = {row["词"] for row in rows}
    missing = sorted(word for word in NOT_AN_ALIAS if word not in listed)
    assert missing == [], (
        "the retired-alias table omits %d disqualified word(s): %s"
        % (len(missing), "、".join(missing))
    )
    assert len(rows) >= 15, "only %d retired entries" % len(rows)


def test_terminology_banned_aliases_absent_from_every_deliverable():
    """Every markdown under docs/pid-platform is scanned, not just reference/."""
    documents = alias_scan_documents(DOCS)
    assert "glossary.md" in documents, "the scan did not pick up the glossary"
    assert len(documents) >= 4, (
        "expected the delivered documents, found %s" % sorted(documents)
    )
    assert any(name.startswith("reference/") for name in documents), (
        "the widened scan lost the reference baseline: %s" % sorted(documents)
    )
    errors = scan_alias_violations(_text(GLOSSARY), documents)
    assert errors == [], "; ".join(errors)


def test_terminology_alias_scan_names_the_word_and_the_file():
    glossary = _synthetic_glossary(["位号"], alias="编号")
    documents = {
        "glossary.md": glossary,
        "requirements/50-data-and-rule-engine.md": "每个对象都要有编号。",
    }
    errors = scan_alias_violations(glossary, documents)
    assert len(errors) == 1, errors
    assert "requirements/50-data-and-rule-engine.md" in errors[0], (
        "the error does not name the offending file: %s" % errors[0]
    )
    assert "编号" in errors[0], (
        "the error does not name the offending word: %s" % errors[0]
    )


def test_terminology_alias_scan_passes_a_canonical_only_sample():
    glossary = _synthetic_glossary(["位号"], alias="编号")
    documents = {
        "glossary.md": glossary,
        "requirements/50-data-and-rule-engine.md": "位号由位号规则生成并接受唯一校验。",
    }
    assert scan_alias_violations(glossary, documents) == []


def test_terminology_alias_scan_exempts_only_the_glossary_bookkeeping():
    text = _text(GLOSSARY)
    assert scan_alias_violations(text, {"glossary.md": text}) == [], (
        "the glossary's own alias column must not flag the glossary"
    )
    polluted = text.replace(
        "## 使用与维护规则",
        "## 使用与维护规则\n\n新术语不得写成图符。",
        1,
    )
    errors = scan_alias_violations(text, {"glossary.md": polluted})
    assert errors, "an alias in the glossary's maintenance rules went unreported"
    assert "图符" in errors[0], errors


def test_terminology_alias_scan_ignores_unregistered_words():
    glossary = _synthetic_glossary(["位号"], alias="编号")
    documents = {"gap-analysis.md": "阀体与 WBS 都不是别名。"}
    assert scan_alias_violations(glossary, documents) == []


def test_terminology_alias_policy_rejects_a_different_concept():
    text = _set_alias(_text(GLOSSARY), "阀门", "阀体")
    errors = validate_alias_policy(text)
    assert any("阀体" in error for error in errors), (
        "registering 阀体 as an alias of 阀门 was accepted: %s" % errors
    )


def test_terminology_alias_policy_rejects_an_ordinary_engineering_word():
    text = _set_alias(_text(GLOSSARY), "位号", "编号")
    errors = validate_alias_policy(text)
    assert any("编号" in error for error in errors), (
        "registering 编号 as an alias of 位号 was accepted: %s" % errors
    )


def test_terminology_alias_policy_requires_the_criteria_section():
    text = _drop_section(_text(GLOSSARY), ALIAS_CRITERIA_HEADING)
    errors = validate_alias_policy(text)
    assert any(ALIAS_CRITERIA_HEADING in error for error in errors), (
        "a glossary without the criteria section passed: %s" % errors
    )


def test_terminology_alias_policy_requires_a_reason_for_every_retired_word():
    text = _drop_row(_text(GLOSSARY), "WBS")
    errors = validate_alias_policy(text)
    assert any("WBS" in error for error in errors), (
        "dropping the WBS judgement from the retired table passed: %s" % errors
    )


def test_terminology_alias_hygiene_accepts_the_delivered_glossary():
    assert alias_hygiene_errors(_text(GLOSSARY)) == []


def test_terminology_alias_hygiene_rejects_a_fragment_of_a_canonical_term():
    text = _set_alias(_text(GLOSSARY), "位号", "数据")
    errors = alias_hygiene_errors(text)
    assert any("数据" in error for error in errors), (
        "an alias that is a fragment of 数据要素 was accepted: %s" % errors
    )


def test_terminology_keeps_the_wrong_names_that_are_real_misnomers():
    rows, _ = glossary_rows(_text(GLOSSARY))
    for term, aliases in KEPT_WRONG_NAMES.items():
        assert term in rows, "glossary row missing: " + term
        registered = split_aliases(rows[term]["禁用别名"])
        missing = [alias for alias in aliases if alias not in registered]
        assert missing == [], (
            "row %s dropped a genuine misnomer: %s" % (term, missing)
        )


def test_terminology_never_registers_a_different_concept_as_alias():
    aliases = banned_alias_map(_text(GLOSSARY))
    offenders = sorted(word for word in DIFFERENT_CONCEPTS if word in aliases)
    assert offenders == [], (
        "these name different concepts and must not be aliases: %s" % offenders
    )



def test_terminology_validator_accepts_a_complete_glossary():
    errors = validate_glossary(_synthetic_glossary(GLOSSARY_TERMS))
    assert errors == [], "validator rejected a valid glossary: %s" % errors


def test_terminology_validator_rejects_missing_term():
    incomplete = [t for t in GLOSSARY_TERMS if t != "图纸版本快照"]
    errors = validate_glossary(_synthetic_glossary(incomplete))
    assert errors, "validator accepted a glossary missing a core concept"
    assert any("图纸版本快照" in e for e in errors), (
        "the error does not name the missing term: %s" % errors
    )


def test_terminology_validator_rejects_wrong_columns():
    text = _synthetic_glossary(GLOSSARY_TERMS).replace(
        "| 中文名 | 英文名 | 定义 | 禁用别名 |",
        "| 中文 | 英文 | 含义 | 别名 |",
    )
    errors = validate_glossary(text)
    assert any("header" in e or "table" in e for e in errors), (
        "validator accepted a table with the wrong columns: %s" % errors
    )


def test_terminology_validator_rejects_blank_alias_and_thin_definition():
    text = _synthetic_glossary(["位号"])
    text = text.replace(
        "| 位号 | Term | 由测试合成的定义文本，用于验证契约解析器 | 旧称 |",
        "| 位号 | Term | 短 |  |",
    )
    errors = validate_glossary(text)
    assert any("位号" in e and "禁用别名" in e for e in errors), (
        "blank alias cell not reported: %s" % errors
    )
    assert any("位号" in e and "definition" in e for e in errors), (
        "thin definition not reported: %s" % errors
    )
