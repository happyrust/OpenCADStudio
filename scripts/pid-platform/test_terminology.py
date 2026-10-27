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
from requirements_contract import (
    TAG_RULE_FIELDS_IN_DATA_DOC,
    load_documents,
    validate_field_group_consistency,
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


@pytest.mark.parametrize(
    "word",
    ["WBS", "数据项"],
    ids=["different-concept", "another-object-name"],
)
def test_terminology_alias_policy_requires_a_reason_for_every_retired_word(word):
    text = _drop_row(_text(GLOSSARY), word)
    errors = validate_alias_policy(text)
    assert any(word in error for error in errors), (
        "dropping the %s judgement from the retired table passed: %s"
        % (word, errors)
    )


def test_terminology_retired_table_matches_the_disqualified_word_set():
    rows, errors = retired_alias_rows(_text(GLOSSARY))
    assert errors == [], errors
    listed = {row["词"] for row in rows}
    assert listed == set(NOT_AN_ALIAS), (
        "the retired-alias table and NOT_AN_ALIAS have drifted apart; only in "
        "the table: %s; only in the constant: %s"
        % (
            sorted(listed - set(NOT_AN_ALIAS)),
            sorted(set(NOT_AN_ALIAS) - listed),
        )
    )
    assert len(rows) == 24, (
        "the retired-alias table carries %d rows; the round-2 revision leaves "
        "24 once 数据项 joins 数据字段" % len(rows)
    )


def test_terminology_data_item_is_retired_under_data_element():
    text = _text(GLOSSARY)
    rows, errors = retired_alias_rows(text)
    assert errors == [], errors
    by_word = {row["词"]: row for row in rows}
    assert "数据项" in by_word, (
        "数据项 is absent from the retired table; the glossary's own rule "
        "requires a stated judgement for every removed word"
    )
    assert by_word["数据项"]["曾被归到"] == "数据要素", (
        "the 数据项 entry must record that it was formerly grouped under "
        "数据要素, got %r" % by_word["数据项"]["曾被归到"]
    )
    assert by_word["数据项"]["理由"].strip(), "the 数据项 judgement has no reason"
    assert "数据项" not in banned_alias_map(text), (
        "数据项 is still registered as a banned alias"
    )
    # Negative control: the NOT_AN_ALIAS entry is what rejects a relapse.
    relapsed = _set_alias(text, "数据要素", "数据项")
    errors = validate_alias_policy(relapsed)
    assert any("数据项" in error for error in errors), (
        "re-registering 数据项 as a banned alias of 数据要素 was accepted: %s"
        % errors
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


# ---------------------------------------------------------------------------
# VAL-DOC-009, document half: the reference documents reuse the defining
# document's field names verbatim. The prototype halves (DOM labels and
# screen-catalog.json) belong to the prototype milestones.
# ---------------------------------------------------------------------------

DATA_DOC_PATH = "requirements/50-data-and-rule-engine.md"
DELIVERABLE_DOC_PATH = "requirements/60-deliverables-and-publish.md"


def _field_group_documents() -> dict:
    """Minimal pair: a data document reusing the tag fields, a BOM header."""
    data = "# 数据\n\n位号生成沿用位号规则定义的 " + "、".join(
        TAG_RULE_FIELDS_IN_DATA_DOC
    ) + "。\n"
    bom = (
        "| 序号 | 类型 | 位号 | 名称 | 数量 | 单位 |\n"
        "| --- | --- | --- | --- | --- | --- |\n"
        "| 1 | 设备 | P-1001 | 离心泵 | 1 | 台 |\n"
    )
    return {DATA_DOC_PATH: data, DELIVERABLE_DOC_PATH: "# 交付\n\n" + bom}


def test_terminology_field_groups_agree_across_the_delivered_documents():
    """VAL-DOC-009: tag-rule and BOM field names match their defining documents."""
    errors = validate_field_group_consistency(load_documents(DOCS))
    assert errors == [], "field-name drift: %s" % errors


def test_terminology_field_group_control_accepts_the_synthetic_pair():
    """Positive control: the synthetic pair reuses every name verbatim."""
    assert validate_field_group_consistency(_field_group_documents()) == []


def test_terminology_validator_rejects_a_tag_rule_field_synonym_in_the_data_document():
    """Negative control: the rewritten name is drift even in a referencing document."""
    documents = _field_group_documents()
    documents[DATA_DOC_PATH] = documents[DATA_DOC_PATH].replace(
        "位号唯一校验", "位号唯一性校验"
    )
    errors = validate_field_group_consistency(documents)
    assert any("uses the synonym" in error for error in errors), errors


def test_terminology_validator_rejects_a_dropped_tag_rule_field():
    """Negative control: each of the six names must appear in the data document."""
    documents = _field_group_documents()
    documents[DATA_DOC_PATH] = documents[DATA_DOC_PATH].replace("自动弹窗", "自动弹出")
    errors = validate_field_group_consistency(documents)
    assert any("tag-rule field name '自动弹窗'" in error for error in errors), errors


def test_terminology_validator_rejects_a_renamed_bom_column():
    """Negative control: 数量 renamed in every occurrence must still be caught."""
    documents = _field_group_documents()
    documents[DELIVERABLE_DOC_PATH] = (
        documents[DELIVERABLE_DOC_PATH].replace("数量", "件数")
    )
    errors = validate_field_group_consistency(documents)
    assert any("material-list column name '数量'" in error for error in errors), errors


def test_terminology_validator_rejects_bom_names_that_never_form_a_header():
    """Negative control: the six words scattered in prose are not the table header."""
    documents = _field_group_documents()
    documents[DELIVERABLE_DOC_PATH] = (
        "# 交付\n\n材料清单列有 序号、类型、位号、名称、数量、单位 六项。\n"
    )
    errors = validate_field_group_consistency(documents)
    assert any("never appear together as the header" in error for error in errors), errors


# ---------------------------------------------------------------------------
# VAL-PROTO-020 / VAL-CROSS-002, prototype half: the rendered prototype must use
# the same vocabulary as the glossary. The alias list is never duplicated here —
# it comes from ``banned_alias_map`` over the committed glossary, so the glossary
# stays the single source of truth.
# ---------------------------------------------------------------------------

PROTO = DOCS / "prototype"
PROTO_SOURCES = (".html", ".js")


def _proto_rendered_strings() -> dict:
    """Every prototype text source as {docs-relative path: text}.

    CSS is excluded on purpose: it carries no user-visible Chinese copy, and the
    colour tokens there are machine values rather than terminology.
    """
    documents = {}
    for path in sorted(PROTO.rglob("*")):
        if path.is_file() and path.suffix in PROTO_SOURCES:
            rel = str(path.relative_to(DOCS)).replace("\\", "/")
            documents[rel] = path.read_text(encoding="utf-8")
    return documents


def test_terminology_prototype_sources_exist_and_cover_the_shell():
    """The scan must have something real to scan, or the check below is vacuous."""
    documents = _proto_rendered_strings()
    assert documents, "no prototype .html/.js sources found under " + str(PROTO)
    joined = "\n".join(documents.values())
    for testid in ("app-shell", "main-nav", "module-nav", "screen-body"):
        assert testid in joined, "prototype sources never mention data-testid " + testid
    assert "位号" in joined, "the prototype copy does not use the term 位号"


def test_terminology_prototype_uses_no_banned_alias():
    """VAL-CROSS-002: no banned alias reaches the prototype's rendered strings."""
    glossary = GLOSSARY.read_text(encoding="utf-8")
    aliases = banned_alias_map(glossary)
    assert aliases, "the glossary registers no banned alias; the scan would be vacuous"
    errors = scan_alias_violations(glossary, _proto_rendered_strings())
    assert errors == [], "banned alias in the prototype: %s" % errors


def test_terminology_prototype_alias_check_rejects_a_planted_alias():
    """Negative control: planting one alias must be caught, with file and word."""
    glossary = GLOSSARY.read_text(encoding="utf-8")
    aliases = banned_alias_map(glossary)
    planted_word = "物料清单"
    assert planted_word in aliases, (
        "the control needs %r to be a registered alias" % planted_word
    )
    documents = _proto_rendered_strings()
    target = sorted(documents)[0]
    documents[target] = documents[target] + "\n<!-- " + planted_word + " -->\n"
    errors = scan_alias_violations(glossary, documents)
    assert any(
        target in error and planted_word in error for error in errors
    ), "planted alias not reported for %s: %s" % (target, errors)
