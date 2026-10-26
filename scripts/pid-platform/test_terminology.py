"""Terminology contract tests for docs/pid-platform/glossary.md.

The glossary is the single source of vocabulary for every deliverable, so
these tests assert its shape against the committed file, not against a
hand-written expectation that could drift from disk.
"""
from __future__ import annotations

import pytest

from conftest import DOCS
from docs_contract import (
    GLOSSARY_COLUMNS,
    GLOSSARY_TERMS,
    banned_alias_map,
    find_table,
    glossary_rows,
    section_body,
    split_aliases,
    validate_glossary,
)

GLOSSARY = DOCS / "glossary.md"
REFERENCE = DOCS / "reference"


def _text(path) -> str:
    assert path.is_file(), "missing deliverable: " + str(path)
    return path.read_text(encoding="utf-8")


def _synthetic_glossary(terms) -> str:
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
            "| %s | %s | 由测试合成的定义文本，用于验证契约解析器 | 旧称 |"
            % (term, "Term")
        )
    return "\n".join(lines) + "\n"


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


def test_terminology_banned_aliases_absent_from_reference_baseline():
    """The deliverables this feature owns must not use a banned alias.

    The scan is scoped to the reference baseline because it is the only
    prose this feature writes; widening it to every deliverable is the
    closing audit's job.
    """
    text = _text(GLOSSARY)
    aliases = banned_alias_map(text)
    assert aliases, "glossary registers no banned aliases at all"
    documents = sorted(REFERENCE.glob("*.md"))
    assert documents, "no reference documents found under " + str(REFERENCE)
    offences = []
    for document in documents:
        body = document.read_text(encoding="utf-8")
        for alias, canonical in aliases.items():
            if alias and alias in body:
                offences.append("%s uses %r (banned alias of %s)" % (
                    document.name, alias, canonical,
                ))
    assert offences == [], "; ".join(offences)


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
