"""The ``-k structure`` gate for Markdown links and anchors.

Every test name starts with ``test_structure_links_`` so the structure
group selects this module. The rules live in ``link_contract.py``; this
module holds one positive control over the real docs tree, one positive
control over the README index table, and the negative controls that prove
the validator can both fail and pass.
"""
from __future__ import annotations

import re

from conftest import DOCS
from link_contract import check_links, github_anchor

INDEX_ROW_RE = re.compile(
    r"^\|\s*[^|]+\|\s*\[(?P<label>docs/pid-platform/[^\]]+)\]\((?P<target>[^)]+)\)\s*\|",
    re.MULTILINE,
)


def test_structure_links_real_docs_tree_has_no_broken_links():
    errors, checked = check_links(DOCS)
    assert errors == [], "docs/pid-platform 存在断链或错锚点：" + "; ".join(errors)
    assert checked > 0, "链接检查器没有检查到任何相对链接，正向控制无效"
    assert checked >= 19, (
        "README 交付物索引表的 19 条链接未被计入，实际检查到 " + str(checked) + " 条"
    )


def test_structure_links_readme_index_table_is_link_backed():
    text = (DOCS / "README.md").read_text(encoding="utf-8")
    rows = [m.groupdict() for m in INDEX_ROW_RE.finditer(text)]
    assert len(rows) == 19, (
        "交付物索引表应有 19 条 Markdown 链接行，实际 " + str(len(rows)) + " 条"
    )
    prefix = "docs/pid-platform/"
    for row in rows:
        label = row["label"]
        target = row["target"]
        assert label.startswith(prefix), "链接文本不是仓库相对路径：" + label
        assert target == label[len(prefix):], (
            "链接文本与链接目标不一致：" + label + " -> " + target
        )
        assert (DOCS / target).exists(), "索引表指向的文件不存在：" + target


def test_structure_links_detects_a_broken_relative_link(tmp_path):
    (tmp_path / "index.md").write_text(
        "# 索引\n\n见 [缺失页面](missing.md)。\n", encoding="utf-8"
    )
    errors, checked = check_links(tmp_path)
    assert checked == 1, "负向控制应恰好检查到 1 条链接，实际 " + str(checked)
    assert len(errors) == 1, "负向控制应恰好报 1 条错误，实际 " + str(len(errors))
    assert "missing.md" in errors[0], "错误未点名断链目标：" + errors[0]
    assert "断链" in errors[0], "错误未标注断链类型：" + errors[0]


def test_structure_links_detects_a_missing_anchor(tmp_path):
    (tmp_path / "target.md").write_text("# 目标页面\n\n正文。\n", encoding="utf-8")
    (tmp_path / "index.md").write_text(
        "# 索引\n\n见 [目标页面](target.md#不存在的标题)。\n", encoding="utf-8"
    )
    errors, checked = check_links(tmp_path)
    assert checked == 1, "负向控制应恰好检查到 1 条链接，实际 " + str(checked)
    assert len(errors) == 1, "负向控制应恰好报 1 条错误，实际 " + str(len(errors))
    assert "不存在的标题" in errors[0], "错误未点名错锚点：" + errors[0]
    assert "锚点缺失" in errors[0], "错误未标注锚点缺失：" + errors[0]


def test_structure_links_accepts_a_real_anchor(tmp_path):
    (tmp_path / "target.md").write_text("# 交付物索引\n\n正文。\n", encoding="utf-8")
    (tmp_path / "index.md").write_text(
        "# 索引\n\n见 [交付物索引](target.md#交付物索引)。\n", encoding="utf-8"
    )
    errors, checked = check_links(tmp_path)
    assert errors == [], "存在的锚点被误判为缺失：" + "; ".join(errors)
    assert checked == 1, "应恰好检查到 1 条链接，实际 " + str(checked)


def test_structure_links_skips_external_links_and_code_fences(tmp_path):
    (tmp_path / "index.md").write_text(
        "# 索引\n\n[官网](https://example.com) 与 [邮件](mailto:a@b.c)\n\n"
        "```\n[示例](nope.md)\n```\n",
        encoding="utf-8",
    )
    errors, checked = check_links(tmp_path)
    assert errors == [], "外链或代码块内的方括号被当成真实链接：" + "; ".join(errors)
    assert checked == 0, "外链不应计入检查条数，实际 " + str(checked)


def test_structure_links_anchor_normalization_matches_github_rules():
    assert github_anchor("交付物索引") == "交付物索引"
    assert github_anchor("原型运行方法") == "原型运行方法"
    assert github_anchor("Step 1: Setup") == "step-1-setup"
    assert github_anchor("`code` and **bold**") == "code-and-bold"
    assert github_anchor("A_B") == "a_b"
