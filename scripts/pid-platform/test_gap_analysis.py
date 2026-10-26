"""Structural contract tests for docs/pid-platform/gap-analysis.md.

Covers VAL-DOC-005: five-column capability matrix, at least twenty data
rows, 处置 restricted to 复用/改造/新增, a real repository path for every
复用/改造 row, —— for every 新增 row, and the ten confirmed-absent
capabilities.

The positive tests read the committed document on disk. The negative
controls build scratch documents under tmp_path so a green run proves the
validator can actually fail.
"""
from __future__ import annotations

from pathlib import Path

from conftest import DOCS, REPO_ROOT
from docs_contract import (
    GAP_ABSENT_SECTION,
    GAP_COLUMNS,
    GAP_CONFIRMED_ABSENT,
    GAP_DISPOSITIONS,
    GAP_EVIDENCE_PREFIXES,
    GAP_MATRIX_SECTION,
    GAP_MIN_ROWS,
    GAP_NO_EVIDENCE,
    GAP_REQUIRED_TOKENS,
    gap_banned_identifier_hits,
    gap_evidence_paths,
    gap_matrix_rows,
    h1_count,
    section_body,
    sections_at,
    validate_gap_analysis,
)

GAP = DOCS / "gap-analysis.md"
HARNESS = Path(__file__).resolve().parent


def _text(path) -> str:
    assert path.is_file(), "missing deliverable: " + str(path)
    return path.read_text(encoding="utf-8")


def _default_rows():
    rows = []
    for i in range(GAP_MIN_ROWS):
        token = GAP_REQUIRED_TOKENS[i] if i < len(GAP_REQUIRED_TOKENS) else "既有实现"
        rows.append(
            {
                "能力": "测试能力 %02d" % (i + 1),
                "现状": "现状说明条目 %02d 引用了 %s" % (i + 1, token),
                "目标状态": "目标状态说明条目 %02d" % (i + 1),
                "处置": "复用" if i % 2 == 0 else "新增",
                "证据路径": "src/entities/" if i % 2 == 0 else GAP_NO_EVIDENCE,
            }
        )
    return rows


def _synthetic_gap(rows=None, absent=None) -> str:
    rows = list(rows) if rows is not None else _default_rows()
    absent = list(absent) if absent is not None else list(GAP_CONFIRMED_ABSENT)
    lines = [
        "# 现状差距与复用映射",
        "",
        "本文件由测试合成，用于验证 VAL-DOC-005 的解析与判定。",
        "",
        "## " + GAP_MATRIX_SECTION,
        "",
        "| " + " | ".join(GAP_COLUMNS) + " |",
        "| " + " | ".join(["---"] * len(GAP_COLUMNS)) + " |",
    ]
    for row in rows:
        lines.append(
            "| %s | %s | %s | %s | %s |"
            % (
                row["能力"],
                row["现状"],
                row["目标状态"],
                row["处置"],
                row["证据路径"],
            )
        )
    lines += ["", "## " + GAP_ABSENT_SECTION, ""]
    for item in absent:
        lines += [
            "### " + item,
            "",
            "该能力在仓库中没有任何实现，现有近似实现不足以承担目标职责，"
            "因此处置为 新增，并在需求文档中给出可判定的验收标准与数据契约。",
            "",
        ]
    return "\n".join(lines) + "\n"


# ---------------------------------------------------------------------------
# The committed document
# ---------------------------------------------------------------------------


def test_structure_gap_analysis_satisfies_contract():
    text = _text(GAP)
    errors = validate_gap_analysis(text, REPO_ROOT)
    assert errors == [], "gap-analysis.md failed VAL-DOC-005: " + "; ".join(
        errors
    )


def test_structure_gap_analysis_has_one_h1_and_both_required_sections():
    text = _text(GAP)
    assert h1_count(text) == 1, "expected exactly one H1, found %d" % h1_count(
        text
    )
    assert section_body(text, 2, GAP_MATRIX_SECTION) is not None, (
        "missing H2 section " + GAP_MATRIX_SECTION
    )
    assert section_body(text, 2, GAP_ABSENT_SECTION) is not None, (
        "missing H2 section " + GAP_ABSENT_SECTION
    )


def test_structure_gap_analysis_matrix_has_five_columns_and_twenty_rows():
    text = _text(GAP)
    rows, errors = gap_matrix_rows(text)
    assert errors == [], "matrix parsing failed: " + "; ".join(errors)
    assert len(rows) >= GAP_MIN_ROWS, (
        "capability matrix has %d data rows, expected at least %d"
        % (len(rows), GAP_MIN_ROWS)
    )
    assert list(rows[0].keys()) == GAP_COLUMNS, (
        "capability matrix columns are %s, expected %s"
        % (list(rows[0].keys()), GAP_COLUMNS)
    )


def test_structure_gap_analysis_covers_required_existing_capabilities():
    text = _text(GAP)
    matrix = section_body(text, 2, GAP_MATRIX_SECTION) or ""
    missing = [t for t in GAP_REQUIRED_TOKENS if t not in matrix]
    assert missing == [], (
        "capability matrix does not cover: %s" % "、".join(missing)
    )


def test_structure_gap_analysis_dispositions_are_restricted_to_the_enum():
    text = _text(GAP)
    rows, errors = gap_matrix_rows(text)
    assert errors == [], "matrix parsing failed: " + "; ".join(errors)
    observed = sorted({row["处置"] for row in rows})
    unknown = [d for d in observed if d not in GAP_DISPOSITIONS]
    assert unknown == [], "处置 values outside the enum: %s" % unknown
    counts = {d: sum(1 for r in rows if r["处置"] == d) for d in GAP_DISPOSITIONS}
    print("disposition counts:", counts)
    assert counts["复用"] + counts["改造"] >= 10, (
        "expected at least ten 复用/改造 rows, got %s" % counts
    )
    assert counts["新增"] >= 10, (
        "expected at least ten 新增 rows (one per confirmed-absent "
        "capability), got %s" % counts
    )


def test_structure_gap_analysis_evidence_paths_exist_in_the_repository():
    text = _text(GAP)
    rows, errors = gap_matrix_rows(text)
    assert errors == [], "matrix parsing failed: " + "; ".join(errors)
    checked = []
    for index, row in enumerate(rows, start=2):
        disposition = row["处置"]
        evidence = row["证据路径"].strip()
        if disposition not in ("复用", "改造"):
            continue
        paths = gap_evidence_paths(evidence)
        assert paths, (
            "row %d (%s) is %s but has no evidence path"
            % (index, row["能力"], disposition)
        )
        for path in paths:
            assert path.startswith(GAP_EVIDENCE_PREFIXES), (
                "row %d (%s) cites %r outside src/, crates/, assets/, docs/"
                % (index, row["能力"], path)
            )
            assert (REPO_ROOT / path).exists(), (
                "row %d (%s) cites a path that does not exist: %s"
                % (index, row["能力"], path)
            )
            checked.append(path)
    print("verified evidence paths:", len(checked))
    assert len(checked) >= 10, (
        "expected at least ten verified evidence paths, got %d" % len(checked)
    )


def test_structure_gap_analysis_new_rows_carry_no_evidence_path():
    text = _text(GAP)
    rows, errors = gap_matrix_rows(text)
    assert errors == [], "matrix parsing failed: " + "; ".join(errors)
    offenders = [
        row["能力"]
        for row in rows
        if row["处置"] == "新增" and row["证据路径"].strip() != GAP_NO_EVIDENCE
    ]
    assert offenders == [], (
        "新增 rows must use %s as their evidence path: %s"
        % (GAP_NO_EVIDENCE, offenders)
    )


def test_structure_gap_analysis_confirmed_absent_lists_ten_items():
    text = _text(GAP)
    body = section_body(text, 2, GAP_ABSENT_SECTION)
    assert body is not None, "missing H2 section " + GAP_ABSENT_SECTION
    subsections = dict(sections_at(body, 3))
    assert len(subsections) == len(GAP_CONFIRMED_ABSENT), (
        "expected exactly %d subsections under %s, found %d: %s"
        % (
            len(GAP_CONFIRMED_ABSENT),
            GAP_ABSENT_SECTION,
            len(subsections),
            list(subsections),
        )
    )
    missing = [item for item in GAP_CONFIRMED_ABSENT if item not in subsections]
    assert missing == [], "confirmed-absent items not listed: %s" % missing


def test_structure_gap_analysis_does_not_name_a_nonexistent_mcp_operation():
    """The automation-API row may only claim what src/mcp.rs contains.

    Its evidence path is src/mcp.rs; every operation name in that row must
    therefore be one of READ_OPS/EXECUTE_OPS, and the P&ID group capability
    must be described as the PIDGROUP command from
    src/app/commands/pidlegend.rs, not as an MCP operation.
    """
    text = _text(GAP)
    hits = gap_banned_identifier_hits(text)
    assert hits == [], (
        "gap-analysis.md claims an MCP operation that src/mcp.rs does not "
        "define; report PIDGROUP as a command instead: %s" % hits
    )
    rows, errors = gap_matrix_rows(text)
    assert errors == [], "matrix parsing failed: " + "; ".join(errors)
    row = next(r for r in rows if r["能力"].strip() == "自动化 API")
    assert "PIDGROUP" in row["现状"], (
        "the automation-API row must name the PIDGROUP command: %s" % row["现状"]
    )
    assert "`pid_legend`" in row["现状"], row["现状"]
    paths = gap_evidence_paths(row["证据路径"])
    assert paths == ["src/mcp.rs", "src/app/commands/pidlegend.rs"], (
        "unexpected evidence paths: %s" % paths
    )


def test_structure_no_retired_group_identifier_in_documents_or_harness():
    """Neither the deliverables nor the test harness may use that name.

    The harness bans the identifier but must not carry it, so the pattern
    lives in docs_contract as a character class and this sweep reads files
    from disk rather than comparing constants.
    """
    offenders = []
    for path in sorted(DOCS.rglob("*.md")):
        text = path.read_text(encoding="utf-8")
        for number, line in gap_banned_identifier_hits(text):
            offenders.append(
                "%s:%d: %s"
                % (path.relative_to(REPO_ROOT).as_posix(), number, line)
            )
    for path in sorted(HARNESS.rglob("*.py")):
        text = path.read_text(encoding="utf-8")
        for number, line in gap_banned_identifier_hits(text):
            offenders.append(
                "%s:%d: %s"
                % (path.relative_to(REPO_ROOT).as_posix(), number, line)
            )
    print("scanned", len(list(DOCS.rglob("*.md"))), "documents and",
          len(list(HARNESS.rglob("*.py"))), "harness modules")
    assert offenders == [], (
        "the retired identifier reappeared (write PIDGROUP instead): %s"
        % offenders
    )


# ---------------------------------------------------------------------------
# Negative controls: the validator must reject broken documents
# ---------------------------------------------------------------------------


def test_structure_gap_validator_accepts_a_conforming_document():
    errors = validate_gap_analysis(_synthetic_gap(), None)
    assert errors == [], "validator rejected a conforming document: %s" % errors


def test_structure_gap_validator_rejects_a_thin_matrix():
    text = _synthetic_gap(rows=_default_rows()[:5])
    errors = validate_gap_analysis(text, None)
    assert any("at least %d" % GAP_MIN_ROWS in e for e in errors), (
        "row floor not reported: %s" % errors
    )


def test_structure_gap_validator_rejects_a_bad_disposition():
    rows = _default_rows()
    rows[0]["处置"] = "重写"
    errors = validate_gap_analysis(_synthetic_gap(rows=rows), None)
    assert any("重写" in e for e in errors), (
        "unknown disposition not reported: %s" % errors
    )


def test_structure_gap_validator_rejects_reuse_row_without_path():
    rows = _default_rows()
    rows[0]["证据路径"] = GAP_NO_EVIDENCE
    errors = validate_gap_analysis(_synthetic_gap(rows=rows), None)
    assert any("no 证据路径" in e for e in errors), (
        "missing evidence path not reported: %s" % errors
    )


def test_structure_gap_validator_rejects_path_outside_the_repository():
    rows = _default_rows()
    rows[0]["证据路径"] = "Cargo.toml"
    errors = validate_gap_analysis(_synthetic_gap(rows=rows), None)
    assert any("outside src/" in e for e in errors), (
        "out-of-tree evidence path not reported: %s" % errors
    )


def test_structure_gap_validator_rejects_a_nonexistent_path(tmp_path):
    (tmp_path / "src").mkdir()
    rows = _default_rows()
    rows[0]["证据路径"] = "src/does-not-exist.rs"
    errors = validate_gap_analysis(_synthetic_gap(rows=rows), tmp_path)
    assert any("does not exist" in e for e in errors), (
        "nonexistent evidence path not reported: %s" % errors
    )


def test_structure_gap_validator_rejects_new_row_with_a_path():
    rows = _default_rows()
    rows[1]["处置"] = "新增"
    rows[1]["证据路径"] = "src/entities/"
    errors = validate_gap_analysis(_synthetic_gap(rows=rows), None)
    assert any("is 新增 but its 证据路径" in e for e in errors), (
        "新增 row with a path not reported: %s" % errors
    )


def test_structure_gap_validator_rejects_missing_absent_section():
    text = _synthetic_gap().replace("## " + GAP_ABSENT_SECTION, "## 其他缺口")
    errors = validate_gap_analysis(text, None)
    assert any(GAP_ABSENT_SECTION in e for e in errors), (
        "missing required section not reported: %s" % errors
    )


def test_structure_gap_validator_rejects_missing_absent_item():
    text = _synthetic_gap(absent=GAP_CONFIRMED_ABSENT[:-1])
    errors = validate_gap_analysis(text, None)
    assert any(GAP_CONFIRMED_ABSENT[-1] in e for e in errors), (
        "missing confirmed-absent item not reported: %s" % errors
    )


def test_structure_gap_validator_rejects_thin_absent_entry():
    text = _synthetic_gap()
    text = text.replace(
        "该能力在仓库中没有任何实现，现有近似实现不足以承担目标职责，"
        "因此处置为 新增，并在需求文档中给出可判定的验收标准与数据契约。",
        "缺少。",
    )
    errors = validate_gap_analysis(text, None)
    assert any("too thin" in e for e in errors), (
        "thin confirmed-absent entry not reported: %s" % errors
    )


def test_structure_gap_validator_rejects_wrong_table_header():
    text = _synthetic_gap().replace(
        "| " + " | ".join(GAP_COLUMNS) + " |",
        "| 能力 | 现状 | 目标状态 | 处置 | 证据 |",
    )
    errors = validate_gap_analysis(text, None)
    assert any("table not found" in e for e in errors), (
        "wrong header not reported: %s" % errors
    )


def test_structure_gap_validator_rejects_the_retired_group_identifier(tmp_path):
    """Writing the old name back into the document must fail the gate.

    The token is assembled here from fragments so the harness itself stays
    clean: a grep for the identifier across docs/pid-platform and
    scripts/pid-platform must return nothing, while the guard still rejects
    a document that reintroduces it.
    """
    retired = "pid" + "_group"
    clean = _text(GAP)
    assert validate_gap_analysis(clean, REPO_ROOT) == [], (
        "the committed document must satisfy VAL-DOC-005"
    )
    assert gap_banned_identifier_hits(clean) == []
    dirty = clean.replace(
        "写侧不占用独立的 MCP 操作位",
        "写侧不占用独立的 MCP 操作位（另有 `" + retired + "` 操作）",
        1,
    )
    assert dirty != clean, "the injection anchor no longer exists"
    copy = tmp_path / "gap-analysis.md"
    copy.write_text(dirty, encoding="utf-8")
    errors = validate_gap_analysis(copy.read_text(encoding="utf-8"), REPO_ROOT)
    print("observed errors:", errors)
    assert errors, "validator accepted a document naming the retired identifier"
    assert any("PIDGROUP command instead" in e for e in errors), (
        "no error points at PIDGROUP as the correct name: %s" % errors
    )
    assert any("line " in e for e in errors), (
        "no error gives the offending line number: %s" % errors
    )
    assert gap_banned_identifier_hits(dirty) != [], (
        "the scan predicate must see the injected identifier"
    )
    assert _text(GAP) == clean, "the committed document must not be modified"

