"""Machine-checkable contract for docs/pid-platform/manifest.json.

Pure stdlib module: it must stay importable without pytest. Every
public validator returns a list of English error strings; an empty
list means the manifest is valid. All functions are pure with respect
to their arguments so tests can point them at a temporary directory.
"""
from __future__ import annotations

import json
import re
from collections import Counter
from pathlib import Path
from typing import Sequence

HEADING_RE = re.compile(r"^#{1,6}[ \t]+(.+?)[ \t]*$", re.MULTILINE)
# Section numbering is stripped only when it really is numbering: digits
# (optionally dotted, optionally ending in a dot) or Chinese numerals
# followed by 、 or ., and the token must not run straight into an ASCII
# letter or digit. Without that guard "2D 绘图" would normalize to "D 绘图"
# and could never match the required section name "2D 绘图" in the manifest.
# The atomic group stops the engine from backtracking to a shorter numbering
# prefix, so "2.5D 视图" is left untouched instead of becoming ".5D 视图".
_NUMBERING_RE = re.compile(
    r"^(?>\d+(?:\.\d+)*\.?|[一二三四五六七八九十]+[、.])"
    r"(?![0-9A-Za-z])[ \t]*"
)
_ID_RE = re.compile(r"^[a-z0-9]+(-[a-z0-9]+)*$")

KINDS = frozenset(
    {
        "readme",
        "prd",
        "glossary",
        "gap-analysis",
        "reference",
        "architecture",
        "requirements",
        "prototype",
    }
)
STATUSES = frozenset({"planned", "delivered"})
REQUIRED_FIELDS = ("id", "path", "kind", "requiredSections", "status")
DOCS_PREFIX = "docs/pid-platform/"

# The prototype subtree is deliberately outside the deliverable set: VAL-DOC-008
# promises exactly 20 markdown deliverables, and the prototype's own notes
# (``prototype/specs/test-surface.md``, and any later screen notes) are checked by
# the prototype assertions instead. Without this exemption the orphan walk below
# reports the first prototype note as an unregistered deliverable.
UNREGISTERED_DOCS_SUBTREES = ("prototype/",)

PRD_REQUIRED_SECTIONS = [
    "产品定位与价值主张",
    "目标行业与客户",
    "目标用户与角色",
    "应用场景",
    "功能架构",
    "功能模块清单",
    "路线图",
    "成功指标",
]


def normalize_heading(text: str) -> str:
    """Strip markup and numbering so headings compare exactly."""
    text = text.strip()
    text = re.sub(r"^#+[ \t]*", "", text)
    text = text.replace("**", "").replace("`", "").strip()
    text = _NUMBERING_RE.sub("", text)
    return text.strip().rstrip("：:").strip()


def headings(path: Path) -> list:
    """Return the normalized ATX headings of a markdown file."""
    path = Path(path)
    if not path.exists():
        return []
    return [
        normalize_heading(h)
        for h in HEADING_RE.findall(path.read_text(encoding="utf-8"))
    ]


def load_manifest_file(path: Path) -> dict:
    """Load a manifest JSON file as UTF-8; JSONDecodeError propagates."""
    with Path(path).open(encoding="utf-8") as fh:
        return json.load(fh)


def docs_relative_path(path_str, docs_root: Path) -> str:
    """Normalize a manifest or filesystem path to a docs-relative posix path."""
    s = str(path_str).replace("\\", "/")
    while s.startswith("./"):
        s = s[2:]
    root = str(docs_root).replace("\\", "/").rstrip("/")
    if root and (s == root or s.startswith(root + "/")):
        s = s[len(root):].lstrip("/")
    elif s.startswith(DOCS_PREFIX):
        s = s[len(DOCS_PREFIX):]
    return s


def validate_schema(manifest: dict) -> list:
    """Check the manifest object shape; entries are reported by index."""
    errors = []
    if not isinstance(manifest, dict):
        return ["manifest is not a JSON object"]
    version = manifest.get("version")
    if not isinstance(version, int) or isinstance(version, bool) or version <= 0:
        errors.append(
            "manifest 'version' must be a positive integer, got "
            + repr(version)
        )
    deliverables = manifest.get("deliverables")
    if not isinstance(deliverables, list):
        return errors + ["manifest 'deliverables' must be a list"]
    if not deliverables:
        errors.append("manifest 'deliverables' must not be empty")
    seen_ids = {}
    for i, entry in enumerate(deliverables):
        where = "deliverables[%d]" % i
        if not isinstance(entry, dict):
            errors.append(where + " is not an object")
            continue
        missing = [f for f in REQUIRED_FIELDS if f not in entry]
        if missing:
            errors.append(
                where + " is missing required fields: " + ", ".join(missing)
            )
            continue
        entry_id = entry["id"]
        if not isinstance(entry_id, str) or not _ID_RE.match(entry_id):
            errors.append(
                where + " has an invalid id " + repr(entry_id)
                + " (must match " + _ID_RE.pattern + ")"
            )
        elif entry_id in seen_ids:
            errors.append(
                where + " has duplicate id " + repr(entry_id)
                + " (first registered at deliverables[%d])" % seen_ids[entry_id]
            )
        else:
            seen_ids[entry_id] = i
        path = entry["path"]
        if not isinstance(path, str) or not path:
            errors.append(where + " has an empty or non-string path")
        elif "\\" in path:
            errors.append(where + " path contains a backslash: " + repr(path))
        kind = entry["kind"]
        if not isinstance(kind, str) or kind not in KINDS:
            errors.append(where + " has an unknown kind " + repr(kind))
        sections = entry["requiredSections"]
        if not isinstance(sections, list) or any(
            not isinstance(s, str) or not s for s in sections
        ):
            errors.append(
                where + " requiredSections must be a list of non-empty strings"
            )
        elif isinstance(path, str) and path.endswith(".md") and not sections:
            errors.append(
                where + " (" + path + ") is a markdown deliverable "
                "but requiredSections is empty"
            )
        status = entry["status"]
        if not isinstance(status, str) or status not in STATUSES:
            errors.append(where + " has an invalid status " + repr(status))
    return errors


def check_delivered(manifest: dict, docs_root: Path) -> list:
    """Check that delivered entries exist on disk and carry their sections."""
    errors = []
    if not isinstance(manifest, dict):
        return errors
    for entry in manifest.get("deliverables", []):
        if not isinstance(entry, dict):
            continue
        if entry.get("status") != "delivered":
            continue
        entry_id = entry.get("id")
        rel = entry.get("path", "")
        target = Path(docs_root) / docs_relative_path(rel, docs_root)
        if not target.is_file():
            errors.append(
                "delivered entry '%s' points to a missing file: %s"
                % (entry_id, rel)
            )
            continue
        if isinstance(rel, str) and rel.endswith(".md"):
            found = set(headings(target))
            for name in entry.get("requiredSections", []):
                if name not in found:
                    errors.append(
                        "delivered entry '%s' (%s) is missing required section: %s"
                        % (entry_id, rel, name)
                    )
    return errors


def registered_markdown_paths(manifest: dict, docs_root: Path) -> list:
    """Return every markdown path the manifest registers, docs-relative."""
    registered = []
    if isinstance(manifest, dict):
        for entry in manifest.get("deliverables", []):
            if not isinstance(entry, dict):
                continue
            path = entry.get("path")
            if isinstance(path, str) and path.endswith(".md"):
                registered.append(docs_relative_path(path, docs_root))
    return registered


def registered_paths(manifest: dict, docs_root: Path) -> list:
    """Return every deliverable path the manifest registers, docs-relative.

    Unlike :func:`registered_markdown_paths` this keeps non-markdown entries
    (prototype assets, data files) so the cardinality check cannot be
    bypassed by registering a path with another extension.
    """
    registered = []
    if isinstance(manifest, dict):
        for entry in manifest.get("deliverables", []):
            if not isinstance(entry, dict):
                continue
            path = entry.get("path")
            if isinstance(path, str) and path:
                registered.append(docs_relative_path(path, docs_root))
    return registered


def check_closure(
    manifest: dict, docs_root: Path, expected_paths: Sequence[str]
) -> list:
    """Check the registered markdown set against the promised set and disk."""
    errors = []
    registered = registered_markdown_paths(manifest, docs_root)
    counts = Counter(registered)
    for rel in sorted(counts):
        if counts[rel] > 1:
            errors.append(
                "path '%s' is registered %d times in the manifest"
                % (rel, counts[rel])
            )
    for expected in expected_paths:
        rel = docs_relative_path(expected, docs_root)
        if counts.get(rel, 0) == 0:
            errors.append("missing promised deliverable: " + rel)
    docs_root = Path(docs_root)
    if docs_root.is_dir():
        for file in sorted(docs_root.rglob("*.md")):
            rel = docs_relative_path(file, docs_root)
            if rel.replace("\\", "/").startswith(UNREGISTERED_DOCS_SUBTREES):
                continue
            n = counts.get(rel, 0)
            if n == 0:
                errors.append(
                    "orphan markdown file not registered in manifest: " + rel
                )
            elif n > 1:
                errors.append(
                    "markdown file '%s' is registered %d times in the manifest"
                    % (rel, n)
                )
    return errors


def check_cardinality(
    manifest: dict, docs_root: Path, expected_paths: Sequence[str]
) -> list:
    """VAL-DOC-008: the registered set must equal the promised set exactly.

    ``check_closure`` walks the promised paths and the files on disk, so an
    entry added to the manifest for a path that is neither promised nor
    present (a 21st deliverable, typically still ``planned``) slips through
    both directions. This check compares the two sets directly and reports
    entries in either difference, naming the offending path. Every
    deliverable path counts, not only markdown, so registering a prototype
    asset cannot hide behind the extension filter.
    """
    errors = []
    registered = set(registered_paths(manifest, docs_root))
    promised = {docs_relative_path(p, docs_root) for p in expected_paths}
    for rel in sorted(registered - promised):
        errors.append(
            "manifest registers a deliverable outside the promised set of "
            "%d: %s" % (len(promised), rel)
        )
    for rel in sorted(promised - registered):
        errors.append("manifest is missing a promised deliverable: " + rel)
    return errors


def validate_manifest(
    manifest_path: Path, docs_root: Path, expected_paths: Sequence[str]
) -> list:
    """Full contract check: load, schema, delivered entries, closure, count."""
    try:
        manifest = load_manifest_file(manifest_path)
    except json.JSONDecodeError as exc:
        return [
            "manifest file %s is not valid JSON: %s" % (manifest_path, exc)
        ]
    except OSError as exc:
        return ["manifest file %s cannot be read: %s" % (manifest_path, exc)]
    errors = validate_schema(manifest)
    errors += check_delivered(manifest, docs_root)
    errors += check_closure(manifest, docs_root, expected_paths)
    errors += check_cardinality(manifest, docs_root, expected_paths)
    return errors
