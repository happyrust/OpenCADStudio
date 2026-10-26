"""Regression tests for heading numbering normalization.

``check_delivered`` compares a manifest ``requiredSections`` string against
the normalized headings of a document, so a heading that merely *starts*
with a digit must survive normalization untouched. Before the guard in
``manifest_contract._NUMBERING_RE`` the expression stripped any leading
digits, turning "2D 绘图" into "D 绘图" and making such a required section
impossible to satisfy.
"""
from __future__ import annotations

import re

import pytest

import conftest
import manifest_contract
from manifest_contract import check_delivered, normalize_heading

# The expression that shipped before the guard. Keeping it here lets the
# regression below prove that the fix is not a no-op.
_OLD_NUMBERING_RE = re.compile(
    r"^(?:\d+(?:\.\d+)*\.?|[一二三四五六七八九十]+[、.])[ \t]*"
)

DIGIT_LETTER_NAMES = ["2D 绘图", "3D 视图", "4K 标注"]

NUMBERED_HEADINGS = [
    ("1. 产品定位", "产品定位"),
    ("2.3 目标用户与角色", "目标用户与角色"),
    ("一、总体要求", "总体要求"),
    ("1.2.3 详细设计", "详细设计"),
]


@pytest.mark.parametrize("name", DIGIT_LETTER_NAMES)
def test_normalization_keeps_digit_letter_names(name):
    assert normalize_heading(name) == name, (
        "a name starting with a digit must not be read as numbering: "
        "%r normalized to %r" % (name, normalize_heading(name))
    )
    assert normalize_heading("## " + name) == name, (
        "a markdown heading prefix must not make the name look numbered: "
        "%r normalized to %r" % ("## " + name, normalize_heading("## " + name))
    )


@pytest.mark.parametrize("raw, expected", NUMBERED_HEADINGS)
def test_normalization_strips_section_numbering(raw, expected):
    assert normalize_heading(raw) == expected, (
        "numbering in %r was not stripped: got %r"
        % (raw, normalize_heading(raw))
    )
    assert normalize_heading("## " + raw) == expected, (
        "numbering in %r was not stripped: got %r"
        % ("## " + raw, normalize_heading("## " + raw))
    )


@pytest.mark.parametrize("name", ["2.5D 视图", "1.2.3D 动画"])
def test_normalization_keeps_multi_level_prefix_before_letter(name):
    assert normalize_heading(name) == name, (
        "a partial numbering match must not eat a fragment of the name: "
        "%r normalized to %r" % (name, normalize_heading(name))
    )


@pytest.mark.parametrize("heading", ["路线图", "PID 绘图", "一般要求"])
def test_normalization_leaves_unnumbered_headings_alone(heading):
    assert normalize_heading(heading) == heading


def test_normalization_strips_number_without_separator():
    assert normalize_heading("1产品定位") == "产品定位"


def test_old_expression_would_mangle_digit_letter_names():
    """Non-vacuity: reverting the guard must break the tests above."""
    for name in DIGIT_LETTER_NAMES:
        mangled = _OLD_NUMBERING_RE.sub("", name).strip()
        assert mangled != name, (
            "the pre-fix expression already left %r alone; the regression "
            "tests would not catch a revert" % name
        )
        assert normalize_heading(name) == name


def test_normalization_is_reexported_from_conftest():
    assert conftest.normalize_heading is manifest_contract.normalize_heading, (
        "conftest must re-export the single normalization implementation"
    )


def test_delivered_entry_accepts_digit_letter_required_section(tmp_path):
    (tmp_path / "doc.md").write_text(
        "# 参考产品\n\n## 1. 概述\n\n正文。\n\n"
        "## 2D 绘图\n\n正文。\n\n## PID 绘图\n\n正文。\n",
        encoding="utf-8",
    )
    manifest = {
        "version": 1,
        "deliverables": [
            {
                "id": "reference-product-summary",
                "path": "doc.md",
                "kind": "reference",
                "requiredSections": ["2D 绘图", "PID 绘图"],
                "status": "delivered",
            }
        ],
    }
    assert check_delivered(manifest, tmp_path) == [], (
        "headings '2D 绘图' and 'PID 绘图' must match their manifest names"
    )


def test_delivered_entry_rejects_missing_digit_letter_section(tmp_path):
    (tmp_path / "doc.md").write_text(
        "# 参考产品\n\n## 3D 视图\n\n正文。\n", encoding="utf-8"
    )
    manifest = {
        "version": 1,
        "deliverables": [
            {
                "id": "reference-product-summary",
                "path": "doc.md",
                "kind": "reference",
                "requiredSections": ["2D 绘图"],
                "status": "delivered",
            }
        ],
    }
    errors = check_delivered(manifest, tmp_path)
    assert errors, "a missing required section must be reported"
    assert any("2D 绘图" in e for e in errors), (
        "the error must name the missing section: %s" % errors
    )
