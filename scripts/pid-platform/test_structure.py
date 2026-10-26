"""Structural contract tests for docs/pid-platform/manifest.json.

Every test asserts real filesystem state: the committed manifest on disk,
the markdown tree under docs/pid-platform, or scratch trees under tmp_path.
"""
from __future__ import annotations

import json
from collections import Counter

from conftest import DOCS, EXPECTED_DELIVERABLES, MANIFEST, delivered_entries
from manifest_contract import (
    PRD_REQUIRED_SECTIONS,
    check_cardinality,
    check_closure,
    check_delivered,
    docs_relative_path,
    load_manifest_file,
    registered_markdown_paths,
    validate_manifest,
    validate_schema,
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
    manifest = load_manifest_file(MANIFEST)
    counts = Counter(
        docs_relative_path(d.get("path", ""), DOCS)
        for d in _deliverables(manifest)
        if isinstance(d, dict) and str(d.get("path", "")).endswith(".md")
    )
    orphans = [
        docs_relative_path(f, DOCS)
        for f in sorted(DOCS.rglob("*.md"))
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


def test_structure_delivered_entry_uses_repo_relative_path(tmp_path):
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
                "path": "docs/pid-platform/PRD.md",
                "kind": "prd",
                "requiredSections": ["产品定位与价值主张"],
                "status": "delivered",
            }
        ],
    }
    assert check_delivered(manifest, docs_root) == []
    target.unlink()
    errors = check_delivered(manifest, docs_root)
    assert errors, "deleted file must be reported as missing"
    assert any("docs/pid-platform/PRD.md" in e for e in errors), (
        "error should mention the repo-relative path: %s" % errors
    )
