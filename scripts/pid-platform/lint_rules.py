"""Pure-stdlib Markdown hygiene rules for the ``-k lint`` gate.

Every public validator takes the document text plus a display path and
returns a list of English error strings shaped ``"<path>:<line>: <problem>"``;
an empty list means the document satisfies the rule. The module deliberately
has no ``test_`` prefix so ``pytest -k lint`` selects only the contract
tests in ``test_lint.py``.
"""
from __future__ import annotations

import re
from pathlib import Path
from typing import Iterator

from manifest_contract import check_closure

# The closed list of placeholder/undone markers from the documentation
# contract. Bare "占位" and "待补" are deliberately NOT banned: "占位申请"
# is legitimate domain vocabulary, so only the bracketed marker "<占位>"
# and the full "待补充" count as markers.
PLACEHOLDERS = (
    "TODO",
    "TBD",
    "FIXME",
    "待补充",
    "待定",
    "Lorem ipsum",
    "<占位>",
    "xxx",
)

BANNED_PHRASES = ("性能优秀", "支持良好", "高效", "尽可能", "尽量", "良好的")

_FENCE_OPEN_RE = re.compile(r"^ {0,3}(`{3,}|~{3,})")
_ATX_RE = re.compile(r"^ {0,3}(#{1,6})[ \t]+\S")
_CODE_SPAN_RE = re.compile(r"(`+).*?\1")
_PIPE_SPLIT_RE = re.compile(r"(?<!\\)\|")
_SEPARATOR_CELL_RE = re.compile(r"^:?-{3,}:?$")


def _is_closing_fence(line: str, char: str, length: int) -> bool:
    body = line.lstrip(" ")
    if len(line) - len(body) > 3:
        return False
    run = len(body) - len(body.lstrip(char))
    return run >= length and not body[run:].strip(" \t")


def content_lines(text: str) -> Iterator[tuple[int, str]]:
    """Yield ``(line number, line)`` for lines outside fenced code blocks.

    Line numbers are 1-based. An opening fence is at most three leading
    spaces then three or more backticks or tildes, optionally followed by
    an info string; the matching closing fence uses the same character,
    at least as many of them, and nothing but whitespace afterwards.
    Fence lines and the lines between them are not yielded.
    """
    fence_char = ""
    fence_len = 0
    for lineno, line in enumerate(text.splitlines(), start=1):
        if fence_len:
            if _is_closing_fence(line, fence_char, fence_len):
                fence_len = 0
            continue
        opening = _FENCE_OPEN_RE.match(line)
        if opening:
            run = opening.group(1)
            fence_char, fence_len = run[0], len(run)
            continue
        yield lineno, line


def split_table_cells(line: str) -> list[str]:
    """Split a pipe-table row into stripped cells.

    Inline code spans are masked to 'X' characters first so pipes inside
    spans such as ``REQ-(PLAT|TYPE)-\\d{3}`` do not create columns. One
    leading pipe is dropped, one trailing pipe is dropped unless escaped
    as ``\\|``, and the remainder is split on unescaped pipes.
    """
    masked = _CODE_SPAN_RE.sub(lambda m: "X" * len(m.group(0)), line)
    s = masked.strip()
    if s.startswith("|"):
        s = s[1:]
    if s.endswith("|") and not s.endswith("\\|"):
        s = s[:-1]
    return [cell.strip() for cell in _PIPE_SPLIT_RE.split(s)]


def is_table_separator(line: str) -> bool:
    """True for a pipe-table delimiter row such as ``| --- | :--- | ---: |``.

    The row must itself be a pipe row; a bare ``---`` rule or a setext
    underline never delimits a table.
    """
    if not line.lstrip().startswith("|"):
        return False
    cells = split_table_cells(line)
    return bool(cells) and all(_SEPARATOR_CELL_RE.match(c) for c in cells)


def _atx_level(line: str):
    match = _ATX_RE.match(line)
    return len(match.group(1)) if match else None


def h1_errors(text: str, path: str = "<text>") -> list[str]:
    """Require exactly one ATX level-1 heading outside fenced code blocks."""
    h1_lines = [
        lineno
        for lineno, line in content_lines(text)
        if _atx_level(line) == 1
    ]
    if not h1_lines:
        return [
            "%s:1: document has no level-1 heading; expected exactly one"
            % path
        ]
    count = len(h1_lines)
    return [
        "%s:%d: document has %d level-1 headings; expected exactly one"
        % (path, lineno, count)
        for lineno in h1_lines[1:]
    ]


def heading_level_errors(text: str, path: str = "<text>") -> list[str]:
    """Require the first heading to be H1 and levels to step by one."""
    errors = []
    previous = None
    for lineno, line in content_lines(text):
        level = _atx_level(line)
        if level is None:
            continue
        if previous is None:
            if level > 1:
                errors.append(
                    "%s:%d: document starts at H%d; expected H1"
                    % (path, lineno, level)
                )
        elif level > previous + 1:
            errors.append(
                "%s:%d: heading level jumps from H%d to H%d"
                % (path, lineno, previous, level)
            )
        previous = level
    return errors


def table_errors(text: str, path: str = "<text>") -> list[str]:
    """Require every row of a pipe table to match the header column count.

    A table block is a ``|`` line followed by a separator row; one error
    per table is reported at the first offending row.
    """
    lines = list(content_lines(text))
    errors = []
    i = 0
    while i < len(lines):
        lineno, line = lines[i]
        if (
            line.lstrip().startswith("|")
            and i + 1 < len(lines)
            and is_table_separator(lines[i + 1][1])
        ):
            header_line = lineno
            expected = len(split_table_cells(line))
            j = i + 2
            bad_rows = []
            while j < len(lines) and lines[j][1].lstrip().startswith("|"):
                row_lineno, row = lines[j]
                actual = len(split_table_cells(row))
                if actual != expected:
                    bad_rows.append((row_lineno, actual))
                j += 1
            if bad_rows:
                first_lineno, first_actual = bad_rows[0]
                message = (
                    "%s:%d: table (header at line %d, %d columns) has a "
                    "row with %d %s"
                    % (
                        path,
                        first_lineno,
                        header_line,
                        expected,
                        first_actual,
                        "cell" if first_actual == 1 else "cells",
                    )
                )
                if len(bad_rows) > 1:
                    message += (
                        "; %d mismatched rows in this table" % len(bad_rows)
                    )
                errors.append(message)
            i = j
        else:
            i += 1
    return errors


def placeholder_errors(text: str, path: str = "<text>") -> list[str]:
    """Flag every placeholder marker on every raw line, fences included."""
    errors = []
    for lineno, line in enumerate(text.splitlines(), start=1):
        lowered = line.lower()
        for marker in PLACEHOLDERS:
            if marker.lower() in lowered:
                errors.append(
                    "%s:%d: placeholder marker '%s' found"
                    % (path, lineno, marker)
                )
    return errors


def banned_phrase_errors(text: str, path: str = "<text>") -> list[str]:
    """Flag every banned vague phrase on every raw line, case-sensitive."""
    errors = []
    for lineno, line in enumerate(text.splitlines(), start=1):
        for phrase in BANNED_PHRASES:
            if phrase in line:
                errors.append(
                    "%s:%d: banned vague phrase '%s' found"
                    % (path, lineno, phrase)
                )
    return errors


def markdown_registration_errors(
    manifest: dict, docs_root: Path, expected_paths=()
) -> list[str]:
    """Delegate to the manifest closure check in manifest_contract.

    The structure group owns the full manifest closure; the lint gate only
    asserts that every markdown file under docs/pid-platform is registered
    exactly once, which is why the default expected_paths is empty.
    """
    return check_closure(manifest, Path(docs_root), expected_paths)


def planned_entry_errors(manifest: dict) -> list[str]:
    """Flag manifest deliverables that are still ``planned``.

    This is the second clause of the placeholder/undone-marker contract:
    no deliverable may remain "planned" at closeout. It cannot be asserted
    against the real manifest during the research baseline because 16
    deliverables are legitimately still planned; the synthetic controls
    pin the predicate until the closeout feature points it at the real
    manifest.
    """
    errors = []
    if isinstance(manifest, dict):
        for entry in manifest.get("deliverables", []):
            if isinstance(entry, dict) and entry.get("status") == "planned":
                errors.append(
                    "manifest entry '%s' is still planned: %s"
                    % (entry.get("id"), entry.get("path"))
                )
    return errors
