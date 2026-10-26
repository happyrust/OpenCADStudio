"""The ``-k lint`` gate for docs/pid-platform markdown hygiene.

Every test name starts with ``test_lint_`` so ``pytest -k lint`` selects
exactly this module. The rules live in ``lint_rules.py``; this module holds
the real-file assertions over ``docs/pid-platform/**/*.md`` plus the
negative and positive controls that prove each validator can both fail
and pass.
"""
from __future__ import annotations

import json
from pathlib import Path

import pytest

from conftest import DOCS, MANIFEST
from lint_rules import (
    BANNED_PHRASES,
    PLACEHOLDERS,
    banned_phrase_errors,
    h1_errors,
    heading_level_errors,
    markdown_registration_errors,
    placeholder_errors,
    planned_entry_errors,
    table_errors,
)
from manifest_contract import load_manifest_file


def _docs_files() -> list[Path]:
    """All real documents; guards against a vacuously empty docs tree."""
    files = sorted(DOCS.rglob("*.md"))
    assert files, "no markdown files under " + str(DOCS)
    assert len(files) >= 4, (
        "expected at least 4 markdown documents, found %d" % len(files)
    )
    for path in files:
        assert path.stat().st_size > 0, "empty markdown file: " + str(path)
    return files


def _rel(path: Path) -> str:
    """Repo-relative posix path, e.g. ``docs/pid-platform/glossary.md``."""
    return path.relative_to(DOCS.parents[1]).as_posix()


def _scan(scanner) -> dict[str, list[str]]:
    """Run ``scanner`` over every document; {repo-relative path: errors}."""
    report = {}
    for path in _docs_files():
        rel = _rel(path)
        report[rel] = scanner(path.read_text(encoding="utf-8"), rel)
    assert report, "lint scan covered no documents under " + str(DOCS)
    return report


# ---------------------------------------------------------------------------
# Real-file assertions over docs/pid-platform/**/*.md
# ---------------------------------------------------------------------------


def test_lint_document_set_is_not_empty():
    files = _docs_files()
    names = [_rel(path) for path in files]
    assert len(files) >= 4, "document set too small: %s" % names


def test_lint_every_document_has_exactly_one_h1():
    report = _scan(h1_errors)
    bad = {path: errors for path, errors in report.items() if errors}
    assert bad == {}, (
        "H1 violations across %d documents: %s" % (len(report), bad)
    )


def test_lint_headings_do_not_skip_levels():
    report = _scan(heading_level_errors)
    bad = {path: errors for path, errors in report.items() if errors}
    assert bad == {}, (
        "heading violations across %d documents: %s" % (len(report), bad)
    )


def test_lint_tables_have_consistent_column_counts():
    report = _scan(table_errors)
    bad = {path: errors for path, errors in report.items() if errors}
    assert bad == {}, (
        "table violations across %d documents: %s" % (len(report), bad)
    )


def test_lint_documents_contain_no_placeholder_markers():
    report = _scan(placeholder_errors)
    bad = {path: errors for path, errors in report.items() if errors}
    assert bad == {}, (
        "placeholder markers across %d documents: %s" % (len(report), bad)
    )


def test_lint_documents_avoid_banned_vague_phrases():
    report = _scan(banned_phrase_errors)
    bad = {path: errors for path, errors in report.items() if errors}
    assert bad == {}, (
        "banned phrases across %d documents: %s" % (len(report), bad)
    )


def test_lint_every_markdown_is_registered_exactly_once():
    manifest = load_manifest_file(MANIFEST)
    errors = markdown_registration_errors(manifest, DOCS)
    assert errors == [], (
        "registration errors across %d documents: %s"
        % (len(_docs_files()), errors)
    )


# ---------------------------------------------------------------------------
# Negative controls: each validator must reject a broken sample and name
# the sample path plus the offending line.
# ---------------------------------------------------------------------------


def test_lint_h1_check_rejects_two_h1_headings():
    sample = "\n".join(
        ["# 第一标题", "", "## 小节", "", "# 第二标题", ""]
    )
    errors = h1_errors(sample, "sample/two-h1.md")
    expected = (
        "sample/two-h1.md:5: document has 2 level-1 headings; "
        "expected exactly one"
    )
    assert expected in errors, (
        "duplicate H1 not reported at line 5: %s" % errors
    )


def test_lint_h1_check_rejects_document_without_h1():
    sample = "## 小节\n\n正文。\n"
    errors = h1_errors(sample, "sample/no-h1.md")
    expected = (
        "sample/no-h1.md:1: document has no level-1 heading; "
        "expected exactly one"
    )
    assert errors == [expected], (
        "missing-H1 document not reported at line 1: %s" % errors
    )
    assert "no level-1 heading" in errors[0]


def test_lint_heading_check_rejects_skipped_level():
    sample = "# T\n\n### Sub\n"
    errors = heading_level_errors(sample, "sample/jump.md")
    expected = "sample/jump.md:3: heading level jumps from H1 to H3"
    assert expected in errors, (
        "H1-to-H3 jump not reported at line 3: %s" % errors
    )


def test_lint_heading_check_accepts_contiguous_levels():
    sample = "# T\n\n## A\n\n### B\n\n## C\n"
    assert heading_level_errors(sample, "sample/contiguous.md") == []
    assert h1_errors(sample, "sample/contiguous.md") == []


def test_lint_heading_check_ignores_headings_inside_fenced_code():
    sample = "\n".join(
        [
            "# 真实标题",
            "",
            "```markdown",
            "## Not a heading",
            "### also not",
            "```",
            "",
        ]
    )
    assert heading_level_errors(sample, "sample/fenced.md") == []
    assert h1_errors(sample, "sample/fenced.md") == []


def test_lint_table_check_rejects_ragged_row():
    sample = "\n".join(
        [
            "# T",
            "",
            "| 名称 | 类型 | 说明 |",
            "| --- | --- | --- |",
            "| a | b | c |",
            "| d | e |",
            "",
        ]
    )
    errors = table_errors(sample, "sample/ragged.md")
    expected = (
        "sample/ragged.md:6: table (header at line 3, 3 columns) has a "
        "row with 2 cells"
    )
    assert expected in errors, (
        "ragged row not reported at line 6: %s" % errors
    )


def test_lint_table_check_ignores_pipe_inside_code_span():
    sample = "\n".join(
        [
            "# T",
            "",
            "| 规则 | 模式 |",
            "| --- | --- |",
            "| 位号 | `REQ-(PLAT|TYPE)-\\d{3}` |",
            "",
        ]
    )
    assert table_errors(sample, "sample/code-span.md") == []


@pytest.mark.parametrize("marker", PLACEHOLDERS)
def test_lint_placeholder_check_rejects_every_marker(marker):
    sample = "\n".join(
        ["# 标题", "", "正文出现 " + marker + " 标记。", ""]
    )
    errors = placeholder_errors(sample, "sample/placeholder.md")
    expected = (
        "sample/placeholder.md:3: placeholder marker '%s' found" % marker
    )
    assert expected in errors, (
        "marker %r not reported at line 3: %s" % (marker, errors)
    )


@pytest.mark.parametrize("phrase", BANNED_PHRASES)
def test_lint_banned_phrase_check_rejects_every_phrase(phrase):
    sample = "# 标题\n本系统" + phrase + "。\n"
    errors = banned_phrase_errors(sample, "sample/banned.md")
    expected = "sample/banned.md:2: banned vague phrase '%s' found" % phrase
    assert expected in errors, (
        "phrase %r not reported at line 2: %s" % (phrase, errors)
    )


def test_lint_registration_check_rejects_orphan_and_duplicate(tmp_path):
    docs_root = tmp_path / "docs" / "pid-platform"
    docs_root.mkdir(parents=True)
    (docs_root / "a.md").write_text("# A\n", encoding="utf-8")
    (docs_root / "b.md").write_text("# B\n", encoding="utf-8")
    broken = {
        "version": 1,
        "deliverables": [
            {
                "id": "a-first",
                "path": "a.md",
                "kind": "reference",
                "requiredSections": ["A"],
                "status": "delivered",
            },
            {
                "id": "a-second",
                "path": "a.md",
                "kind": "reference",
                "requiredSections": ["A"],
                "status": "delivered",
            },
        ],
    }
    manifest_path = tmp_path / "manifest.json"
    manifest_path.write_text(
        json.dumps(broken, ensure_ascii=False), encoding="utf-8"
    )
    errors = markdown_registration_errors(
        load_manifest_file(manifest_path), docs_root
    )
    assert any("a.md" in e for e in errors), (
        "duplicate registration of a.md not reported: %s" % errors
    )
    assert any("b.md" in e for e in errors), (
        "orphan b.md not reported: %s" % errors
    )
    fixed = {
        "version": 1,
        "deliverables": [
            {
                "id": "a",
                "path": "a.md",
                "kind": "reference",
                "requiredSections": ["A"],
                "status": "delivered",
            },
            {
                "id": "b",
                "path": "b.md",
                "kind": "reference",
                "requiredSections": ["B"],
                "status": "delivered",
            },
        ],
    }
    assert markdown_registration_errors(fixed, docs_root) == []


def test_lint_planned_entry_check_flags_planned_entries():
    manifest = {
        "version": 1,
        "deliverables": [
            {
                "id": "prd",
                "path": "docs/pid-platform/PRD.md",
                "kind": "prd",
                "requiredSections": ["产品定位与价值主张"],
                "status": "planned",
            }
        ],
    }
    errors = planned_entry_errors(manifest)
    expected = "manifest entry 'prd' is still planned: docs/pid-platform/PRD.md"
    assert expected in errors, (
        "planned entry not reported: %s" % errors
    )


def test_lint_planned_entry_check_accepts_delivered_manifest():
    manifest = {
        "version": 1,
        "deliverables": [
            {
                "id": "glossary",
                "path": "docs/pid-platform/glossary.md",
                "kind": "glossary",
                "requiredSections": ["术语表"],
                "status": "delivered",
            },
            {
                "id": "gap-analysis",
                "path": "docs/pid-platform/gap-analysis.md",
                "kind": "gap-analysis",
                "requiredSections": ["能力矩阵"],
                "status": "delivered",
            },
        ],
    }
    assert planned_entry_errors(manifest) == []


# ---------------------------------------------------------------------------
# Positive control: a compliant document must yield zero errors from every
# rule, proving the validators are not always-false.
# ---------------------------------------------------------------------------


def test_lint_compliant_sample_document_passes_all_rules():
    sample = "\n".join(
        [
            "# 平台术语示例",
            "",
            "本段落描述系统的职责边界与约束，作为合规样例使用。",
            "",
            "## 章节一",
            "",
            "### 小节一",
            "",
            "| 名称 | 类型 | 说明 |",
            "| --- | --- | --- |",
            "| 位号 | Tag | 对象身份主键 |",
            "| 管线 | Pipe Line | 介质输送路径 |",
            "",
            "## 章节二",
            "",
            "收尾段落，内容与格式均符合约定。",
            "",
        ]
    )
    path = "sample/compliant.md"
    assert h1_errors(sample, path) == []
    assert heading_level_errors(sample, path) == []
    assert table_errors(sample, path) == []
    assert placeholder_errors(sample, path) == []
    assert banned_phrase_errors(sample, path) == []
