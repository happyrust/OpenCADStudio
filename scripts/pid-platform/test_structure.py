"""Structural contract tests for docs/pid-platform/manifest.json.

Every test asserts real filesystem state: the committed manifest on disk,
the markdown tree under docs/pid-platform, or scratch trees under tmp_path.
"""
from __future__ import annotations

import json
from collections import Counter
from pathlib import Path

import pytest

from conftest import DOCS, EXPECTED_DELIVERABLES, MANIFEST, delivered_entries
from manifest_contract import (
    PRD_REQUIRED_SECTIONS,
    UNREGISTERED_DOCS_SUBTREES,
    check_cardinality,
    check_closure,
    check_delivered,
    docs_relative_path,
    load_manifest_file,
    registered_markdown_paths,
    validate_manifest,
    validate_schema,
)
from prototype_notes import (
    PROTO_FIELD_ORDER,
    TEST_SURFACE_PATH,
    check_test_surface,
    section_two,
    table_field_names,
)


def _deliverables(manifest: dict) -> list:
    return manifest.get("deliverables", [])


def test_structure_manifest_is_valid_json_and_unique_ids():
    manifest = load_manifest_file(MANIFEST)
    errors = validate_schema(manifest)
    assert errors == [], "schema errors: " + "; ".join(errors)
    deliverables = _deliverables(manifest)
    assert len(EXPECTED_DELIVERABLES) == 20, (
        "VAL-DOC-008 promises exactly 20 deliverables; the fixture lists %d"
        % len(EXPECTED_DELIVERABLES)
    )
    assert len(deliverables) == len(EXPECTED_DELIVERABLES), (
        "expected exactly %d deliverables, got %d (VAL-DOC-008 forbids a "
        "21st, not just a shortfall)"
        % (len(EXPECTED_DELIVERABLES), len(deliverables))
    )
    ids = [d["id"] for d in deliverables]
    dupes = sorted({i for i in ids if ids.count(i) > 1})
    assert len(ids) == len(set(ids)), "duplicate deliverable ids: %s" % dupes
    prd = [d["id"] for d in deliverables if d.get("kind") == "prd"]
    assert len(prd) == 1, "expected exactly one prd entry, got: %s" % prd


def test_structure_deliverable_set_equals_the_promised_twenty():
    manifest = load_manifest_file(MANIFEST)
    registered = set(registered_markdown_paths(manifest, DOCS))
    promised = set(EXPECTED_DELIVERABLES)
    extra = sorted(registered - promised)
    missing = sorted(promised - registered)
    assert extra == [], (
        "manifest registers paths VAL-DOC-008 does not promise: %s" % extra
    )
    assert missing == [], "manifest omits promised paths: %s" % missing
    assert registered == promised, (
        "manifest deliverable set (%d) must equal the promised set (%d)"
        % (len(registered), len(promised))
    )
    assert check_cardinality(manifest, DOCS, EXPECTED_DELIVERABLES) == [], (
        "cardinality check must accept the committed manifest"
    )
    # Every entry carries a legal status. The all-delivered clause of the
    # contract belongs to VAL-DOC-007 and to the closeout feature; 16 entries
    # are legitimately still planned while their documents are unauthored.
    statuses = sorted({d.get("status") for d in _deliverables(manifest)})
    assert statuses <= ["delivered", "planned"], (
        "unexpected deliverable statuses: %s" % statuses
    )


def test_structure_every_expected_deliverable_is_registered():
    manifest = load_manifest_file(MANIFEST)
    registered = {
        docs_relative_path(d.get("path", ""), DOCS)
        for d in _deliverables(manifest)
        if isinstance(d, dict)
    }
    missing = [p for p in EXPECTED_DELIVERABLES if p not in registered]
    assert missing == [], "expected deliverables not registered: %s" % missing


def test_structure_no_orphan_markdown_files():
    """Every deliverable markdown is registered exactly once.

    The ``prototype/`` subtree is exempt (single source: ``UNREGISTERED_DOCS_SUBTREES``
    in ``manifest_contract``): its notes are checked by the prototype assertions,
    not by the 20-deliverable set. The exemption is asserted to be non-empty so
    this test cannot silently skip everything.
    """
    assert UNREGISTERED_DOCS_SUBTREES, "the exemption list must not be empty"
    manifest = load_manifest_file(MANIFEST)
    counts = Counter(
        docs_relative_path(d.get("path", ""), DOCS)
        for d in _deliverables(manifest)
        if isinstance(d, dict) and str(d.get("path", "")).endswith(".md")
    )
    scanned = [
        f
        for f in sorted(DOCS.rglob("*.md"))
        if not docs_relative_path(f, DOCS)
        .replace("\\", "/")
        .startswith(UNREGISTERED_DOCS_SUBTREES)
    ]
    assert len(scanned) >= 20, (
        "the orphan scan must still cover the promised deliverables, saw %d "
        "(all 20 promised deliverables are now delivered and registered)"
        % len(scanned)
    )
    orphans = [
        docs_relative_path(f, DOCS)
        for f in scanned
        if counts.get(docs_relative_path(f, DOCS), 0) != 1
    ]
    assert orphans == [], (
        "markdown files not registered exactly once in manifest: %s" % orphans
    )


def test_structure_delivered_entries_have_file_and_sections(manifest):
    errors = check_delivered(manifest, DOCS)
    assert errors == [], "delivered entries failed: " + "; ".join(errors)
    delivered = delivered_entries(manifest)
    checked = [d for d in delivered if isinstance(d.get("path"), str)]
    assert len(delivered) == len(checked), (
        "delivered entries (%d) and checked entries (%d) diverged"
        % (len(delivered), len(checked))
    )
    bad = [
        d.get("id")
        for d in delivered
        if not all(
            isinstance(s, str) and s for s in d.get("requiredSections", [])
        )
    ]
    assert bad == [], "delivered entries with invalid requiredSections: %s" % bad


def test_structure_prd_required_sections_cover_all_eight():
    manifest = load_manifest_file(MANIFEST)
    assert len(PRD_REQUIRED_SECTIONS) == 8
    prd = [d for d in _deliverables(manifest) if d.get("kind") == "prd"]
    assert len(prd) == 1, "expected exactly one prd entry, got: %s" % [
        d.get("id") for d in prd
    ]
    sections = prd[0]["requiredSections"]
    missing = [s for s in PRD_REQUIRED_SECTIONS if s not in sections]
    assert missing == [], "PRD requiredSections missing: %s" % missing


def test_structure_validator_fails_when_a_deliverable_is_missing(tmp_path):
    """VAL-DOC-008's mandatory negative control: delete one deliverable."""
    baseline = validate_manifest(MANIFEST, DOCS, EXPECTED_DELIVERABLES)
    assert baseline == [], "baseline manifest invalid: %s" % baseline
    manifest = load_manifest_file(MANIFEST)
    deliverables = _deliverables(manifest)
    index = next(
        i for i, d in enumerate(deliverables) if d.get("kind") == "prd"
    )
    removed = deliverables[index]
    manifest["deliverables"] = [
        d for i, d in enumerate(deliverables) if i != index
    ]
    copy = tmp_path / "manifest.json"
    copy.write_text(
        json.dumps(manifest, ensure_ascii=False, indent=2), encoding="utf-8"
    )
    errors = validate_manifest(copy, DOCS, EXPECTED_DELIVERABLES)
    print("observed errors:", errors)
    assert errors, "validator accepted a manifest missing the prd deliverable"
    removed_rel = docs_relative_path(removed["path"], DOCS)
    assert any(
        removed["id"] in e or removed["path"] in e or removed_rel in e
        for e in errors
    ), "no error mentions the removed entry %s (%s): %s" % (
        removed["id"], removed_rel, errors,
    )


def test_structure_cardinality_rejects_a_21st_deliverable(tmp_path):
    """A 21st manifest entry must fail the set-equality check.

    The entry is deliberately ``planned`` and points at a path that exists
    neither in the promised set nor on disk: that is the shape that slipped
    past ``check_closure``, which only walks promised paths and on-disk
    files. The assertion on check_closure documents that the old coverage
    accepts this input, so the new failure is not vacuous.
    """
    manifest = load_manifest_file(MANIFEST)
    assert check_cardinality(manifest, DOCS, EXPECTED_DELIVERABLES) == []
    extra_path = "docs/pid-platform/reference/not-promised.md"
    manifest["deliverables"] = list(_deliverables(manifest)) + [
        {
            "id": "reference-extra",
            "path": extra_path,
            "kind": "reference",
            "requiredSections": ["多余章节"],
            "status": "planned",
        }
    ]
    copy = tmp_path / "manifest.json"
    copy.write_text(
        json.dumps(manifest, ensure_ascii=False, indent=2), encoding="utf-8"
    )
    extra_rel = docs_relative_path(extra_path, DOCS)
    assert check_closure(manifest, DOCS, EXPECTED_DELIVERABLES) == [], (
        "check_closure is expected to miss a 21st planned entry; if this "
        "fails the scenario no longer reproduces the gap"
    )
    errors = validate_manifest(copy, DOCS, EXPECTED_DELIVERABLES)
    print("observed errors:", errors)
    assert errors, "validator accepted a manifest with 21 deliverables"
    assert any(extra_rel in e for e in errors), (
        "no error names the extra deliverable %s: %s" % (extra_rel, errors)
    )
    assert not any("missing a promised deliverable" in e for e in errors), (
        "the extra-entry case must not report a missing deliverable: %s" % errors
    )
    assert len(_deliverables(load_manifest_file(MANIFEST))) == 20, (
        "the committed manifest must stay at 20 entries"
    )


def test_structure_cardinality_rejects_a_deleted_deliverable(tmp_path):
    """Deleting one entry must fail the set-equality check and name it."""
    manifest = load_manifest_file(MANIFEST)
    deliverables = list(_deliverables(manifest))
    removed = next(
        d for d in deliverables if d.get("kind") == "architecture"
    )
    manifest["deliverables"] = [d for d in deliverables if d is not removed]
    copy = tmp_path / "manifest.json"
    copy.write_text(
        json.dumps(manifest, ensure_ascii=False, indent=2), encoding="utf-8"
    )
    errors = validate_manifest(copy, DOCS, EXPECTED_DELIVERABLES)
    print("observed errors:", errors)
    missing_rel = docs_relative_path(removed["path"], DOCS)
    assert any(
        "missing a promised deliverable" in e and missing_rel in e
        for e in errors
    ), "cardinality check did not report the missing path %s: %s" % (
        missing_rel, errors,
    )
    assert len(_deliverables(load_manifest_file(MANIFEST))) == 20, (
        "the committed manifest must stay at 20 entries"
    )


def test_structure_cardinality_rejects_a_prototype_deliverable():
    """A registered prototype asset must fail the set-equality check.

    ``check_closure`` walks only the promised paths and the markdown files
    on disk, so an extra entry whose path is neither promised nor markdown
    is invisible in both directions: ``prototype/index.html`` really exists
    on disk (``check_delivered`` finds the file), the promised-path walk
    never names it, and the orphan walk only globs ``*.md``. That is the
    shape ``check_cardinality`` must catch, which is why it counts every
    registered path rather than only markdown.
    """
    manifest = load_manifest_file(MANIFEST)
    assert check_cardinality(manifest, DOCS, EXPECTED_DELIVERABLES) == []
    manifest["deliverables"] = list(_deliverables(manifest)) + [
        {
            "id": "prototype-shell",
            "path": "docs/pid-platform/prototype/index.html",
            "kind": "prototype",
            "requiredSections": [],
            "status": "delivered",
        }
    ]
    assert check_closure(manifest, DOCS, EXPECTED_DELIVERABLES) == [], (
        "check_closure is expected to miss a registered non-markdown path; "
        "if this fails the scenario no longer reproduces the gap"
    )
    errors = check_cardinality(manifest, DOCS, EXPECTED_DELIVERABLES)
    assert errors, (
        "check_cardinality accepted a 21st deliverable pointing at a "
        "prototype asset"
    )
    extra_rel = docs_relative_path(
        "docs/pid-platform/prototype/index.html", DOCS
    )
    assert any(extra_rel in e for e in errors), (
        "no error names the extra deliverable %s: %s" % (extra_rel, errors)
    )
    assert not any("missing a promised deliverable" in e for e in errors), (
        "the extra-entry case must not report a missing deliverable: %s" % errors
    )
    assert len(_deliverables(load_manifest_file(MANIFEST))) == 20, (
        "the committed manifest must stay at 20 entries"
    )


def test_structure_delivered_entry_without_file_is_rejected(tmp_path):
    manifest = {
        "version": 1,
        "deliverables": [
            {
                "id": "prd",
                "path": "PRD.md",
                "kind": "prd",
                "requiredSections": ["产品定位与价值主张"],
                "status": "delivered",
            }
        ],
    }
    errors = check_delivered(manifest, tmp_path)
    assert any("PRD.md" in e for e in errors), (
        "missing file not reported: %s" % errors
    )


def test_structure_delivered_entry_without_required_section_is_rejected(tmp_path):
    (tmp_path / "PRD.md").write_text(
        "# 产品定位与价值主张\n\n占位内容。\n", encoding="utf-8"
    )
    manifest = {
        "version": 1,
        "deliverables": [
            {
                "id": "prd",
                "path": "PRD.md",
                "kind": "prd",
                "requiredSections": ["产品定位与价值主张", "路线图"],
                "status": "delivered",
            }
        ],
    }
    errors = check_delivered(manifest, tmp_path)
    assert any("路线图" in e for e in errors), (
        "missing section not reported: %s" % errors
    )
    assert not any(
        "missing required section: 产品定位与价值主张" in e for e in errors
    ), "present section wrongly reported missing: %s" % errors


def test_structure_planned_entry_without_file_is_accepted(tmp_path):
    manifest = {
        "version": 1,
        "deliverables": [
            {
                "id": "prd",
                "path": "PRD.md",
                "kind": "prd",
                "requiredSections": ["产品定位与价值主张"],
                "status": "planned",
            }
        ],
    }
    assert check_delivered(manifest, tmp_path) == []


def test_structure_orphan_markdown_is_rejected(tmp_path):
    (tmp_path / "extra.md").write_text("# 未登记文档\n", encoding="utf-8")
    manifest = {"version": 1, "deliverables": []}
    errors = check_closure(manifest, tmp_path, [])
    assert any("extra.md" in e for e in errors), (
        "orphan markdown not reported: %s" % errors
    )


def test_structure_section_match_is_exact_after_normalization(tmp_path):
    manifest = {
        "version": 1,
        "deliverables": [
            {
                "id": "prd",
                "path": "doc.md",
                "kind": "prd",
                "requiredSections": ["范围"],
                "status": "delivered",
            }
        ],
    }
    rejected = tmp_path / "rejected"
    rejected.mkdir()
    (rejected / "doc.md").write_text("## 1. 范围与目标\n", encoding="utf-8")
    assert check_delivered(manifest, rejected) != [], (
        "section '范围' must not substring-match heading '范围与目标'"
    )
    accepted = tmp_path / "accepted"
    accepted.mkdir()
    (accepted / "doc.md").write_text("## 2. 范围\n", encoding="utf-8")
    assert check_delivered(manifest, accepted) == [], (
        "section '范围' must exact-match normalized heading '范围'"
    )


# ``check_delivered`` echoes the manifest path verbatim in its missing-file
# message, so the message proves the manifest's own form, not a normalised
# one. ``docs_relative_path`` is what makes an absolute manifest path still
# resolve; the message must nevertheless end in the repo-relative form, or a
# reader (and any consumer that prints the message) sees a machine-specific
# absolute path. Commit 4d88395aa dropped the assertion that pinned this and
# left only ``assert errors``, so the case name promised more than it checked.
MISSING_FILE_SUFFIX = ": docs/pid-platform/PRD.md"


def _assert_missing_file_error_is_repo_relative(errors, docs_root):
    """The missing-file error names the repo-relative path, not an absolute one."""
    assert errors, "deleted file must be reported as missing"
    assert any(e.endswith(MISSING_FILE_SUFFIX) for e in errors), (
        "the missing-file error must end with the repo-relative path %r, got %r"
        % (MISSING_FILE_SUFFIX, errors)
    )
    root = Path(docs_root)
    if root.is_absolute():
        assert not any(str(root) in e for e in errors), (
            "the missing-file error leaked the absolute docs root %r: %r"
            % (str(root), errors)
        )


def _relative_path_fixture(tmp_path, manifest_path="docs/pid-platform/PRD.md"):
    """A scratch docs root holding PRD.md, plus a one-entry delivered manifest."""
    docs_root = tmp_path / "docs" / "pid-platform"
    docs_root.mkdir(parents=True)
    target = docs_root / "PRD.md"
    target.write_text(
        "# 主 PRD\n\n## 1. 产品定位与价值主张\n\n占位内容。\n",
        encoding="utf-8",
    )
    manifest = {
        "version": 1,
        "deliverables": [
            {
                "id": "prd",
                "path": manifest_path,
                "kind": "prd",
                "requiredSections": ["产品定位与价值主张"],
                "status": "delivered",
            }
        ],
    }
    return docs_root, target, manifest


def test_structure_delivered_entry_uses_repo_relative_path(tmp_path):
    """The missing-file error must name the repo-relative path."""
    docs_root, target, manifest = _relative_path_fixture(tmp_path)
    assert check_delivered(manifest, docs_root) == [], (
        "the fixture must start green, otherwise the mutation below proves nothing"
    )
    target.unlink()
    errors = check_delivered(manifest, docs_root)
    _assert_missing_file_error_is_repo_relative(errors, docs_root)


def test_structure_repo_relative_path_guard_rejects_an_absolute_message(tmp_path):
    """Negative control: an absolute manifest path must fail the same guard.

    ``docs_relative_path`` still resolves an absolute path, so this manifest
    is accepted while the file exists; the missing-file message then carries
    the absolute form, which is exactly what the case above forbids.
    """
    docs_root, target, manifest = _relative_path_fixture(tmp_path)
    manifest["deliverables"][0]["path"] = str(docs_root / "PRD.md")
    assert check_delivered(manifest, docs_root) == [], (
        "the absolute-path manifest must still resolve the existing file, "
        "otherwise this control mutates two things at once"
    )
    target.unlink()
    errors = check_delivered(manifest, docs_root)
    assert errors, "the control must still report the missing file"
    with pytest.raises(AssertionError):
        _assert_missing_file_error_is_repo_relative(errors, docs_root)


# ---------------------------------------------------------------------------
# Prototype notes are outside the 20-deliverable set but not outside the gate.
#
# VAL-PROTO-019's Evidence says "契约文档存在性用例全绿" and the shell
# feature's expectedBehaviour names prototype/specs/test-surface.md, yet
# manifest_contract.UNREGISTERED_DOCS_SUBTREES exempts prototype/ from the
# orphan scan, so before these cases the file could be deleted with the whole
# suite still green (measured: 519 passed / exit 0).
# ---------------------------------------------------------------------------

TEST_SURFACE_DOC = """# 原型可编程测试面说明

## 1. 总体约定

正文。

## 2. `window.__proto` 字段契约

| 字段 | 类型 | 含义 | 更新时机 |
| --- | --- | --- | --- |
| `screen` | string | 当前路由界面 id | 每次界面切换 |
| `symbolCount` | number | 符号实例数量 | 编辑器发布后 |
| `selectedTag` | string | 当前选中位号 | 选中变化时 |
| `selectedProps` | array | 属性列表 | 选中变化时 |
| `violations` | array | 违规集合 | 检查执行时 |
| `bomRows` | array | 材料清单行 | 统计发布时 |
| `paletteItemCount` | number | 调色板条目数 | 数据发布后 |
| `tagRulePreview` | string | 位号预览文本 | 规则编辑后 |
| `window.__protoErrors` | string[] | 页面级错误收集数组 | error 事件触发时 |

## 3. 后续章节

正文。
"""


def _synthetic_docs_root(tmp_path, text=TEST_SURFACE_DOC):
    """A scratch tree carrying only the prototype note under test."""
    docs_root = tmp_path / "docs" / "pid-platform"
    target = docs_root / TEST_SURFACE_PATH
    target.parent.mkdir(parents=True, exist_ok=True)
    target.write_text(text, encoding="utf-8")
    return docs_root


def test_structure_test_surface_contract_document_is_guarded():
    """The committed prototype test-surface note exists and lists the contract."""
    errors = check_test_surface(DOCS)
    assert errors == [], "; ".join(errors)


def test_structure_test_surface_fields_are_the_eight_in_contract_order():
    """Section 2 must list exactly the eight __proto fields, in contract order."""
    path = DOCS / TEST_SURFACE_PATH
    assert path.is_file(), "missing " + str(path)
    text = path.read_text(encoding="utf-8")
    names = table_field_names(section_two(text))
    assert names == PROTO_FIELD_ORDER, (
        "%s section 2 lists %r, expected %r"
        % (TEST_SURFACE_PATH, names, PROTO_FIELD_ORDER)
    )
    assert len(PROTO_FIELD_ORDER) == 8, (
        "VAL-PROTO-019 fixes window.__proto at 8 keys; the fixture lists %d"
        % len(PROTO_FIELD_ORDER)
    )
    assert "window.__protoErrors" not in PROTO_FIELD_ORDER, (
        "__protoErrors is a sibling global, not a ninth key of window.__proto"
    )


def test_structure_test_surface_guard_rejects_a_deleted_document(tmp_path):
    """Negative control: the same check must fail when the file is gone.

    The assertion is on the check function's verdict, not on the absence of
    the file, so a check that merely returns [] for everything is caught.
    """
    docs_root = _synthetic_docs_root(tmp_path)
    assert check_test_surface(docs_root) == [], (
        "the synthetic control tree must start green, otherwise the mutation "
        "below proves nothing"
    )
    (docs_root / TEST_SURFACE_PATH).unlink()
    errors = check_test_surface(docs_root)
    assert errors, "deleting %s must be reported" % TEST_SURFACE_PATH
    assert any("missing" in e for e in errors), (
        "the error does not name the missing document: %s" % errors
    )


def test_structure_test_surface_guard_rejects_a_wrong_field_order(tmp_path):
    """Negative control: reordering two fields must fail the same check."""
    reordered = TEST_SURFACE_DOC.replace(
        "| `symbolCount` | number | 符号实例数量 | 编辑器发布后 |\n"
        "| `selectedTag` | string | 当前选中位号 | 选中变化时 |",
        "| `selectedTag` | string | 当前选中位号 | 选中变化时 |\n"
        "| `symbolCount` | number | 符号实例数量 | 编辑器发布后 |",
    )
    assert reordered != TEST_SURFACE_DOC, "the reorder mutation changed nothing"
    docs_root = _synthetic_docs_root(tmp_path, reordered)
    errors = check_test_surface(docs_root)
    assert errors, "a swapped field order must be reported"
    assert any("contract order" in e for e in errors), (
        "the error does not mention the ordering contract: %s" % errors
    )


def test_structure_test_surface_guard_rejects_a_dropped_field(tmp_path):
    """Negative control: dropping one field must fail the same check."""
    dropped = TEST_SURFACE_DOC.replace(
        "| `paletteItemCount` | number | 调色板条目数 | 数据发布后 |\n", ""
    )
    assert dropped != TEST_SURFACE_DOC, "the drop mutation changed nothing"
    errors = check_test_surface(_synthetic_docs_root(tmp_path, dropped))
    assert errors, "a missing __proto field must be reported"


def test_structure_test_surface_guard_rejects_a_second_h1(tmp_path):
    """Negative control: the one-H1 rule must be enforced on this document too."""
    extra = TEST_SURFACE_DOC + "\n# 另一个一级标题\n"
    errors = check_test_surface(_synthetic_docs_root(tmp_path, extra))
    assert errors, "a second H1 must be reported"
    assert any("one H1" in e for e in errors), (
        "the error does not mention the H1 rule: %s" % errors
    )
    assert any(TEST_SURFACE_PATH in e for e in errors), (
        "the error should name the document path: %s" % errors
    )


# A row that names a field but leaves 类型 / 含义 / 更新时机 blank records
# nothing, yet the name-only guard stayed green on it. VAL-PROTO-019 asks for
# all four columns, so each one gets its own negative control.
@pytest.mark.parametrize(
    "row, blanked",
    [
        (
            "| `symbolCount` | number | 符号实例数量 | 编辑器发布后 |",
            "| `symbolCount` |  | 符号实例数量 | 编辑器发布后 |",
        ),
        (
            "| `symbolCount` | number | 符号实例数量 | 编辑器发布后 |",
            "| `symbolCount` | number |  | 编辑器发布后 |",
        ),
        (
            "| `symbolCount` | number | 符号实例数量 | 编辑器发布后 |",
            "| `symbolCount` | number | 符号实例数量 |  |",
        ),
    ],
)
def test_structure_test_surface_guard_rejects_a_blank_column(
    tmp_path, row, blanked
):
    """Negative control: emptying any of the three descriptive columns fails."""
    mutated = TEST_SURFACE_DOC.replace(row, blanked)
    assert mutated != TEST_SURFACE_DOC, "the blanking mutation changed nothing"
    docs_root = _synthetic_docs_root(tmp_path, mutated)
    errors = check_test_surface(docs_root)
    assert errors, "a blank column in a field row must be reported"
    assert any("symbolCount" in e for e in errors), (
        "the error should name the offending field row: %s" % errors
    )


def test_structure_test_surface_guard_rejects_a_fenced_table(tmp_path):
    """Negative control: a table quoted inside a fence is not the contract.

    Moving section 2's body into a ``` fence makes it documentation *about*
    the contract. Before the fence was stripped, the guard still read the
    rows out of the fence and stayed green.
    """
    head, sep, rest = TEST_SURFACE_DOC.partition("## 2. `window.__proto` 字段契约\n")
    assert sep, "the fixture lost its section 2 heading"
    body, sep2, tail = rest.partition("\n## 3. 后续章节")
    assert sep2, "the fixture lost its section 3 heading"
    fenced = (
        head
        + sep
        + "\n```markdown\n"
        + body.strip("\n")
        + "\n```\n"
        + sep2
        + tail
    )
    assert "```markdown" in fenced, "the fence mutation changed nothing"
    docs_root = _synthetic_docs_root(tmp_path, fenced)
    errors = check_test_surface(docs_root)
    assert errors, "a fenced section 2 table must not satisfy the guard"
    assert any("section 2 table carries" in e for e in errors), (
        "the error should say the section 2 table no longer lists the fields: %s"
        % errors
    )
