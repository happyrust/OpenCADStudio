"""Guards for the notes that live under ``docs/pid-platform/prototype/``.

``prototype/`` is exempt from the 20-deliverable orphan scan
(``manifest_contract.UNREGISTERED_DOCS_SUBTREES``) because VAL-DOC-008
promises exactly 20 markdown deliverables, and registering a prototype note
would break that set. The exemption is about deliverable cardinality only:
a prototype note that an assertion or a feature's expected behaviour names
still needs a real check here, one that fails when the file is deleted.

Measured consequence before this module existed: deleting
``prototype/specs/test-surface.md`` left the whole gate at 519 passed / exit 0
while VAL-PROTO-019's Evidence claimed "契约文档存在性用例全绿".
"""
from __future__ import annotations

import re
from pathlib import Path

# Repo-relative to ``docs/pid-platform``.
TEST_SURFACE_PATH = "prototype/specs/test-surface.md"

# VAL-PROTO-019 fixes both the key set and its order. ``window.__protoErrors``
# is a sibling global, not a member of this set, so it is excluded.
PROTO_FIELD_ORDER = [
    "screen",
    "symbolCount",
    "selectedTag",
    "selectedProps",
    "violations",
    "bomRows",
    "paletteItemCount",
    "tagRulePreview",
]

_H1_RE = re.compile(r"^#\s+\S")
_SECTION_TWO_RE = re.compile(r"^##\s+2\.\s")


def section_two(text: str) -> str:
    """Body of the ``## 2.`` section, without its heading."""
    lines = text.splitlines()
    start = None
    for index, line in enumerate(lines):
        if _SECTION_TWO_RE.match(line):
            start = index
            break
    if start is None:
        return ""
    end = len(lines)
    for index in range(start + 1, len(lines)):
        if lines[index].startswith("## "):
            end = index
            break
    return "\n".join(lines[start + 1:end])


def table_field_names(section: str) -> list:
    """First-column cells of the field table that name a ``__proto`` field.

    Header cells (``字段``) and separator cells (``---``) are not in
    ``PROTO_FIELD_ORDER``, so they are skipped by the membership filter. A
    duplicated field therefore shows up as a longer list rather than being
    silently deduplicated.
    """
    names = []
    for line in section.splitlines():
        stripped = line.strip()
        if not stripped.startswith("|"):
            continue
        cells = [cell.strip() for cell in stripped.strip("|").split("|")]
        if not cells:
            continue
        name = cells[0].strip("`").strip()
        if name in PROTO_FIELD_ORDER:
            names.append(name)
    return names


def check_test_surface(docs_root) -> list:
    """Errors for the prototype test-surface contract document.

    Returns an empty list only when the file exists, carries exactly one H1,
    and its section 2 field table lists the eight ``window.__proto`` fields in
    contract order.
    """
    path = Path(docs_root) / TEST_SURFACE_PATH
    if not path.is_file():
        return ["%s is missing" % TEST_SURFACE_PATH]

    text = path.read_text(encoding="utf-8")
    errors = []

    headings = [line for line in text.splitlines() if _H1_RE.match(line)]
    if len(headings) != 1:
        errors.append(
            "%s must carry exactly one H1, found %d: %r"
            % (TEST_SURFACE_PATH, len(headings), headings)
        )

    section = section_two(text)
    if not section:
        errors.append("%s has no section 2 (`## 2. ...`)" % TEST_SURFACE_PATH)
        return errors

    names = table_field_names(section)
    if names != PROTO_FIELD_ORDER:
        errors.append(
            "%s section 2 table carries %r, expected the eight __proto fields "
            "in contract order %r" % (TEST_SURFACE_PATH, names, PROTO_FIELD_ORDER)
        )

    return errors
