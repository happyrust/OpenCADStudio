"""Citation-title contract: a section number must be pinned to its manual heading.

The defect this module guards against is an off-by-one that survived a fully
green suite. ``source-index.md`` cited 符号编辑器绘图能力 (drawing) to
§5.1.4.3.2.2.3.2 and 连接点类型与方向 (connection points) to §5.1.4.3.2.2.3.3;
the manual's headings there are 符号编辑 and 符号外形绘制, so the drawing
capability was one section early and the connection-point definition two
sections early. Both numbers exist in the analysis extract, so existence checks
could not see it.

The checks below therefore work in both directions:

* every section cited by either reference document must publish its manual
  heading title in the appendix, and the title must come from the fixture;
* every fixture title must be a real heading line in the raw manual, so the
  fixture cannot certify itself.

Each validator has a negative control that mutates the committed text and
asserts the failure is reported.
"""
from __future__ import annotations

import json

import pytest

from conftest import DOCS
from docs_contract import (
    CITATION_APPENDIX_COLUMNS,
    CITATION_APPENDIX_HEADING,
    FIXTURES,
    MANUAL_MIN_SECTION_TITLES,
    analysis_dir,
    analysis_section_tokens,
    citation_appendix,
    document_cited_sections,
    find_table,
    manual_text_path,
    read_manual_text,
    section_body,
    section_titles,
    sections_at,
    source_index_rows,
    validate_citation_appendix,
    validate_section_fixture,
    validate_titles_against_manual,
)

REFERENCE = DOCS / "reference"
SUMMARY = REFERENCE / "reference-product-summary.md"
INDEX = REFERENCE / "source-index.md"

# The two rows that were wrong, and the manual heading each must point at.
SYMBOL_EDITOR_ROWS = {
    "符号编辑器绘图能力": ("§5.1.4.3.2.2.3.3", "外形"),
    "连接点类型与方向": ("§5.1.4.3.2.2.3.4", "连接点"),
}

# The analysis extract writes the symbol-editor block as this one interval
# instead of six headings, so the sections below never appear there as tokens:
# .4 and .5 are inside the interval, .7 is not covered by it at all.
SYMBOL_EDITOR_INTERVAL = "5.1.4.3.2.2.3.2–5"
INTERVAL_ONLY_SECTIONS = (
    "5.1.4.3.2.2.3.4",
    "5.1.4.3.2.2.3.5",
    "5.1.4.3.2.2.3.7",
)


def _text(path) -> str:
    assert path.is_file(), "missing deliverable: " + str(path)
    return path.read_text(encoding="utf-8")


def _source_index_sources(index_text: str) -> dict:
    rows, errors = source_index_rows(index_text)
    assert errors == [], "; ".join(errors)
    return {row["主题"]: row["来源"] for row in rows}


def _title_mismatches(index_text: str, expectations: dict) -> list:
    """Topics whose 来源 section title does not contain the expected keyword."""
    sources = _source_index_sources(index_text)
    titles = section_titles()
    bad = []
    for topic, keyword in expectations.items():
        source = sources.get(topic)
        if source is None:
            bad.append((topic, "<row missing>", keyword))
            continue
        if not source.startswith("§"):
            bad.append((topic, source, keyword))
            continue
        title = titles.get(source.lstrip("§"), "<not in fixture>")
        if keyword not in title:
            bad.append((topic, source, title))
    return bad


# ---------------------------------------------------------------------------
# fixtures/manual-sections.json
# ---------------------------------------------------------------------------


def test_structure_manual_section_fixture_titles_every_section():
    titles = section_titles()
    assert len(titles) >= MANUAL_MIN_SECTION_TITLES, (
        "the fixture lists %d sections, the contract requires at least %d"
        % (len(titles), MANUAL_MIN_SECTION_TITLES)
    )
    errors = validate_section_fixture()
    assert errors == [], "; ".join(errors)


def test_structure_manual_section_fixture_covers_the_symbol_editor_family():
    titles = section_titles()
    family = {
        "5.1.4.3.2.2.3.1": "符号新建",
        "5.1.4.3.2.2.3.2": "符号编辑",
        "5.1.4.3.2.2.3.3": "符号外形绘制",
        "5.1.4.3.2.2.3.4": "符号连接点定义",
        "5.1.4.3.2.2.3.5": "符号标签的添加",
        "5.1.4.3.2.2.3.6": "符号的更新",
        "5.1.4.3.2.2.3.7": "符号属性的修改",
    }
    missing = [s for s in family if s not in titles]
    assert missing == [], "fixture is missing symbol-editor sections: %s" % missing
    wrong = {
        s: (titles[s], expected)
        for s, expected in family.items()
        if titles[s] != expected
    }
    assert wrong == {}, (
        "fixture titles disagree with the manual headings (actual, expected): %s"
        % wrong
    )


def _fixture_comment() -> str:
    path = FIXTURES / "manual-sections.json"
    with path.open(encoding="utf-8") as fh:
        return json.load(fh)["comment"]


def test_structure_fixture_comment_states_the_interval_collapse():
    """The comment must tell the truth about which sections lack a token."""
    comment = _fixture_comment()
    missing = [s for s in INTERVAL_ONLY_SECTIONS if s not in comment]
    assert missing == [], (
        "the fixture comment does not name the sections it says have no "
        "analysis token: %s" % missing
    )
    assert SYMBOL_EDITOR_INTERVAL in comment, (
        "the fixture comment does not quote the collapsed analysis interval %s"
        % SYMBOL_EDITOR_INTERVAL
    )


def test_structure_fixture_comment_matches_the_analysis_extract():
    """The comment's claim is checked against the extract, not just its wording."""
    root = analysis_dir()
    if not root.is_dir():
        pytest.skip(
            "analysis extract not available at %s; the token claim cannot be "
            "checked" % root
        )
    tokens = analysis_section_tokens()
    tokened = [s for s in INTERVAL_ONLY_SECTIONS if s in tokens]
    assert tokened == [], (
        "the analysis now carries these sections as standalone § tokens, but "
        "the fixture comment says they occur only inside §%s: %s"
        % (SYMBOL_EDITOR_INTERVAL, tokened)
    )
    text = "\n".join(
        path.read_text(encoding="utf-8") for path in sorted(root.glob("*.md"))
    )
    assert SYMBOL_EDITOR_INTERVAL in text, (
        "the analysis no longer contains the collapsed interval §%s that the "
        "fixture comment relies on" % SYMBOL_EDITOR_INTERVAL
    )


def test_structure_manual_section_fixture_rejects_untitled_entries(tmp_path):
    legacy = tmp_path / "manual-sections.json"
    legacy.write_text(
        json.dumps({"sections": ["6.2.2.3.4", "6.2.2.3.5"]}, ensure_ascii=False),
        encoding="utf-8",
    )
    errors = validate_section_fixture(legacy)
    assert any("no manual heading title" in e for e in errors), (
        "validator accepted a fixture of bare section numbers: %s" % errors
    )


# ---------------------------------------------------------------------------
# source-index.md appendix
# ---------------------------------------------------------------------------


def test_structure_source_index_has_citation_title_appendix():
    text = _text(INDEX)
    body = section_body(text, 2, CITATION_APPENDIX_HEADING)
    assert body is not None, "missing H2 section " + CITATION_APPENDIX_HEADING
    table = find_table(body, CITATION_APPENDIX_COLUMNS)
    assert table is not None, (
        "the appendix has no table with header "
        + " | ".join(CITATION_APPENDIX_COLUMNS)
    )
    assert len(table["rows"]) >= 30, (
        "the appendix lists only %d sections; the mapping table alone cites "
        "more than that" % len(table["rows"])
    )
    headings = [name for name, _ in sections_at(text, 2)]
    assert headings and headings[-1] == CITATION_APPENDIX_HEADING, (
        "the appendix must be the last H2 in source-index.md, found order: %s"
        % headings
    )


def test_structure_citation_appendix_covers_exactly_the_cited_sections():
    index_text = _text(INDEX)
    summary_text = _text(SUMMARY)
    errors = validate_citation_appendix(index_text, summary_text)
    assert errors == [], "; ".join(errors)
    rows, row_errors = citation_appendix(index_text)
    assert row_errors == [], "; ".join(row_errors)
    cited = document_cited_sections(index_text) | document_cited_sections(summary_text)
    assert set(rows) == cited, (
        "appendix sections differ from the cited set; missing=%s extra=%s"
        % (sorted(cited - set(rows)), sorted(set(rows) - cited))
    )


def test_structure_citation_appendix_titles_are_the_fixture_titles():
    rows, errors = citation_appendix(_text(INDEX))
    assert errors == [], "; ".join(errors)
    titles = section_titles()
    wrong = {
        section: (title, titles.get(section))
        for section, title in rows.items()
        if title != titles.get(section)
    }
    assert wrong == {}, (
        "appendix titles differ from the fixture (appendix, fixture): %s" % wrong
    )


def test_structure_source_index_cites_the_drawing_and_connection_point_sections():
    sources = _source_index_sources(_text(INDEX))
    for topic, (expected, _keyword) in SYMBOL_EDITOR_ROWS.items():
        assert sources.get(topic) == expected, (
            "%s cites %r; the manual documents it at %s"
            % (topic, sources.get(topic), expected)
        )
    mismatches = _title_mismatches(
        _text(INDEX), {topic: keyword for topic, (_s, keyword) in SYMBOL_EDITOR_ROWS.items()}
    )
    assert mismatches == [], (
        "these rows cite a section whose manual title is about something else "
        "(topic, citation, actual title): %s" % mismatches
    )


def test_structure_summary_cites_connection_point_and_label_sections():
    body = section_body(_text(SUMMARY), 3, "标准图例定制")
    assert body is not None, "missing H3 section 标准图例定制"
    for section in ("5.1.4.3.2.2.3.4", "5.1.4.3.2.2.3.5"):
        assert "§" + section in body, (
            "标准图例定制 does not cite §%s; the connection-point and "
            "symbol-label statements would then have no provenance" % section
        )


# ---------------------------------------------------------------------------
# Negative controls
# ---------------------------------------------------------------------------


def test_structure_citation_appendix_rejects_a_wrong_title():
    text = _text(INDEX)
    row = "| §5.1.4.3.2.2.3.3 | 符号外形绘制 |"
    assert row in text, "the appendix does not list §5.1.4.3.2.2.3.3 with its title"
    broken = text.replace(row, "| §5.1.4.3.2.2.3.3 | 符号连接点定义 |")
    errors = validate_citation_appendix(broken, _text(SUMMARY))
    assert errors, "validator accepted an appendix row with the wrong manual title"
    assert any("符号外形绘制" in e for e in errors), (
        "the error does not name the correct manual title: %s" % errors
    )


def test_structure_citation_appendix_rejects_a_misnumbered_row():
    text = _text(INDEX)
    row = "| §5.1.4.3.2.2.3.4 | 符号连接点定义 |"
    assert row in text, "the appendix does not list §5.1.4.3.2.2.3.4"
    # Re-point it at the neighbouring section, which is the original off-by-one.
    broken = text.replace(row, "| §5.1.4.3.2.2.3.3 | 符号连接点定义 |")
    errors = validate_citation_appendix(broken, _text(SUMMARY))
    assert errors, "validator accepted an appendix that lost a cited section"
    assert any("5.1.4.3.2.2.3.4" in e for e in errors), (
        "the missing section is not reported: %s" % errors
    )


def test_structure_citation_appendix_rejects_missing_and_extra_rows():
    text = _text(INDEX)
    dropped = text.replace("| §6.2.2.3.4.2.1.12 | 插入图片 |\n", "")
    assert dropped != text, "the appendix does not list §6.2.2.3.4.2.1.12"
    errors = validate_citation_appendix(dropped, _text(SUMMARY))
    assert any("6.2.2.3.4.2.1.12" in e for e in errors), (
        "dropping a cited section was not reported: %s" % errors
    )

    extra_row = "| §6.2.2.3.4.2.1.9 | 不存在的章节 |\n"
    padded = text.replace("| §6.2.2.4 | P&ID 成果输出 |\n", extra_row + "| §6.2.2.4 | P&ID 成果输出 |\n")
    assert padded != text, "the appendix anchor row is missing"
    errors = validate_citation_appendix(padded, _text(SUMMARY))
    assert any("6.2.2.3.4.2.1.9" in e for e in errors), (
        "an appendix row no document cites was not reported: %s" % errors
    )


def test_structure_off_by_one_reversion_is_detected():
    text = _text(INDEX)
    fixed = "| SYM | 符号编辑器绘图能力 | §5.1.4.3.2.2.3.3 |"
    assert fixed in text, "the drawing row is not on its corrected section"
    reverted = text.replace(
        fixed, "| SYM | 符号编辑器绘图能力 | §5.1.4.3.2.2.3.2 |"
    )
    mismatches = _title_mismatches(reverted, {"符号编辑器绘图能力": "外形"})
    assert mismatches, (
        "reverting the row to the old §5.1.4.3.2.2.3.2 was not detected"
    )
    assert mismatches[0][2] == "符号编辑", (
        "expected the section title 符号编辑 to be reported, got %r"
        % (mismatches[0],)
    )


def test_structure_manual_title_check_rejects_a_swapped_title():
    corpus = (
        "5.1.4.3.2.2.3.3 符号外形绘制\n"
        "\n"
        "    打开符号编辑器，顶部菜单栏切换至“绘图”选项卡。\n"
        "\n"
        "5.1.4.3.2.2.3.4 符号连接点定义\n"
        "\n"
        "    单击“连接点”，选择连接点类型。\n"
    )
    good = {
        "5.1.4.3.2.2.3.3": "符号外形绘制",
        "5.1.4.3.2.2.3.4": "符号连接点定义",
    }
    assert validate_titles_against_manual(good, corpus) == []
    swapped = {
        "5.1.4.3.2.2.3.3": "符号连接点定义",
        "5.1.4.3.2.2.3.4": "符号外形绘制",
    }
    errors = validate_titles_against_manual(swapped, corpus)
    assert errors, "title check accepted the original off-by-one mapping"
    assert any("5.1.4.3.2.2.3.3" in e for e in errors), (
        "the wrong section number is not named: %s" % errors
    )


# ---------------------------------------------------------------------------
# The raw manual is the only external witness
# ---------------------------------------------------------------------------


def test_structure_fixture_titles_are_manual_heading_lines():
    corpus = read_manual_text()
    if corpus is None:
        pytest.skip(
            "raw manual text not available at %s; heading cross-check skipped"
            % manual_text_path()
        )
    errors = validate_titles_against_manual(section_titles(), corpus)
    assert errors == [], "; ".join(errors)
