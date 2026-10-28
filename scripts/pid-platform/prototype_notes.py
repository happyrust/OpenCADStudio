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

# VAL-PROTO-019 requires the note to record 字段名、类型、含义、更新时机 for each
# field. A row that names the field but leaves 类型 / 含义 / 更新时机 empty
# records nothing, so the guard counts cells rather than just names.
PROTO_FIELD_COLUMNS = ["字段", "类型", "含义", "更新时机"]
PROTO_FIELD_CELL_COUNT = len(PROTO_FIELD_COLUMNS)


def strip_fenced_blocks(text: str) -> str:
    """Drop fenced code blocks, keeping every other line in place.

    A table that has been moved inside a ``` fence is documentation *about*
    the contract, not the contract, so it must not satisfy the guard. Fences
    are recognised at the start of a line after optional indentation, for both
    ``` and ~~~.

    Limits: a fence indented inside a list item is stripped like any other
    (CommonMark would treat four-space indentation as a nested fence), a
    ```-fence is not closed by a ~~~ line, and an unterminated fence swallows
    the rest of the document — which is also how CommonMark reads it, and
    which correctly makes section 2 disappear.
    """
    kept = []
    fence = None
    for line in text.splitlines():
        stripped = line.lstrip()
        marker = stripped[:3]
        if fence is None:
            if marker in ("```", "~~~"):
                fence = marker
                continue
            kept.append(line)
        elif stripped.startswith(fence):
            fence = None
    return "\n".join(kept)


def section_two(text: str) -> str:
    """Body of the ``## 2.`` section, without its heading.

    Fenced blocks are removed first: a ``## 2.`` heading or table quoted
    inside a fence is an example, not the documented contract.
    """
    lines = strip_fenced_blocks(text).splitlines()
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


def _table_cells(line: str):
    """Cells of one pipe-table line, or ``None`` when the line is not a row."""
    stripped = line.strip()
    if not stripped.startswith("|"):
        return None
    return [cell.strip() for cell in stripped.strip("|").split("|")]


def table_field_names(section: str) -> list:
    """First-column cells of the field table that name a ``__proto`` field.

    Header cells (``字段``) and separator cells (``---``) are not in
    ``PROTO_FIELD_ORDER``, so they are skipped by the membership filter. A
    duplicated field therefore shows up as a longer list rather than being
    silently deduplicated.
    """
    names = []
    for line in section.splitlines():
        cells = _table_cells(line)
        if not cells:
            continue
        name = cells[0].strip("`").strip()
        if name in PROTO_FIELD_ORDER:
            names.append(name)
    return names


def table_field_rows(section: str) -> list:
    """``(name, cells)`` for every table row whose first cell is a field name."""
    rows = []
    for line in section.splitlines():
        cells = _table_cells(line)
        if not cells:
            continue
        name = cells[0].strip("`").strip()
        if name in PROTO_FIELD_ORDER:
            rows.append((name, cells))
    return rows


def empty_cell_errors(section: str) -> list:
    """Errors for field rows that do not carry four non-empty cells.

    Limits: a ``|`` inside a cell's inline code (for example a union type
    written as ``string|number``) would be split into two cells here; the
    document does not use that form.
    """
    errors = []
    for name, cells in table_field_rows(section):
        if len(cells) != PROTO_FIELD_CELL_COUNT:
            errors.append(
                "%s row '%s' has %d cells, expected %d (%s)"
                % (
                    TEST_SURFACE_PATH,
                    name,
                    len(cells),
                    PROTO_FIELD_CELL_COUNT,
                    " / ".join(PROTO_FIELD_COLUMNS),
                )
            )
            continue
        empty = [
            column
            for column, cell in zip(PROTO_FIELD_COLUMNS, cells)
            if not cell.strip()
        ]
        if empty:
            errors.append(
                "%s row '%s' leaves %s empty; VAL-PROTO-019 requires 字段名、"
                "类型、含义、更新时机 to all be recorded"
                % (TEST_SURFACE_PATH, name, "、".join(empty))
            )
    return errors


def check_test_surface(docs_root) -> list:
    """Errors for the prototype test-surface contract document.

    Returns an empty list only when the file exists, carries exactly one H1,
    and its section 2 field table lists the eight ``window.__proto`` fields in
    contract order with all four columns filled in. Fenced code blocks are
    stripped before section 2 is located, so a table quoted inside a fence
    does not count as the contract.
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

    errors.extend(empty_cell_errors(section))

    return errors
