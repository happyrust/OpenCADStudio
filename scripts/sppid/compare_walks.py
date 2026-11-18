#!/usr/bin/env python3
"""Compare two reports of the SmartPlant backup walk: before and after a change.

The walk is the ignored test `app::commands::pidbackup::audit`; with
OCS_PID_BACKUP_AUDIT_OUT set it leaves a `report.txt`. This tool reads two of
them and prints, as Markdown ready for readings.md:

- the totals side by side: drawings, drawings whose items did not read,
  drawings that did not import, entities drawn, source records not drawn,
  the time of all imports;
- which drawings joined or left the two lists of failures;
- every drawing whose four import numbers changed, with both values.

    python scripts/sppid/compare_walks.py BEFORE AFTER [--label NAME]
        [--max-time-ratio 1.5] [--time-floor-ms 5000] [--out FILE]

BEFORE and AFTER are report.txt files, or the folders that hold one.

Exit code:

- 0: nothing got worse;
- 1: something got worse: a drawing newly unreadable or unopenable, a drawing
  in only one of the reports, another number of drawings, or the imports
  slower than the limit allows;
- 2: the arguments or a report cannot be used. A report that lacks one of the
  sections read here is an error, never "no change".

A drawing whose numbers changed does not fail the comparison by itself: a
newer parser may rightly draw more. Fewer entities and more records not drawn
are marked, and each mark is owed a written reason in readings.md.

The time of all imports is judged only when one of the two runs took at least
--time-floor-ms: under that the ratio measures the disk, not the parser, and
it is printed without a verdict.

The Markdown is Chinese, as readings.md is.
"""

import argparse
from dataclasses import dataclass, field
from pathlib import Path
import re
import sys

SOURCE = re.compile(r"^source: (.*)$")
DRAWINGS = re.compile(r"^== drawings \((\d+); \d+ without a file\) ==$")
IMPORT_HEADING = (
    "== import of every drawing (name | entities | decoded | not drawn | sheet layers | time) =="
)
IMPORT_SKIPPED = "== import of every drawing: skipped"
# The name is whatever is left of the five fields: it may hold a bar itself.
IMPORT_ROW = re.compile(r"^    (.+) \| (\d+) \| (\d+) \| (\d+) \| (\d+) \| (\d+(?:\.\d+)?) ms$")
IMPORT_TOTALS = re.compile(
    r"^  (\d+) entities drawn, (\d+) source records not drawn; "
    r"all imported in (\d+(?:\.\d+)?) ms, slowest "
)
UNREAD = re.compile(r"^== (\d+) drawing\(s\) whose items did not read \(\d+ of them silently\) ==$")
UNOPENED = re.compile(r"^== (\d+) drawing\(s\) that did not import ==$")

NOT_IMPORTED = "没导入"


class ReportError(Exception):
    """A report that cannot be read as the report of a walk."""


@dataclass
class Import:
    """What the import of one drawing made of its file."""

    drawn: int
    decoded: int
    missing: int
    sheet_layers: int
    millis: float

    def numbers(self):
        return (self.drawn, self.decoded, self.missing, self.sheet_layers)


@dataclass
class Walk:
    """The parts of one report that a change of the parser can move."""

    source: str = ""
    drawings: int = 0
    # Drawing name -> Import, in the report's order.
    imports: dict = field(default_factory=dict)
    entities: int = 0
    undrawn: int = 0
    import_millis: float = 0.0
    # Drawing name -> the reason the report gives.
    unread: dict = field(default_factory=dict)
    unopened: dict = field(default_factory=dict)

    def names(self):
        """Every drawing the report names, in the order it meets them."""
        return list(dict.fromkeys([*self.imports, *self.unopened, *self.unread]))


def _keyed(pairs):
    """name -> value in order. Two drawings of one name are told apart by
    the order the report lists them in: `name`, `name [2]`, ..."""
    keyed, met = {}, {}
    for name, value in pairs:
        met[name] = met.get(name, 0) + 1
        keyed[name if met[name] == 1 else f"{name} [{met[name]}]"] = value
    return keyed


def _find(lines, pattern, wanted):
    """The first line a pattern matches: (its index, the match)."""
    for at, line in enumerate(lines):
        found = pattern.match(line)
        if found:
            return at, found
    raise ReportError(f"报告里没有「{wanted}」这一行")


def _failures(lines, pattern, wanted):
    """The `  name: reason` entries under one of the two closing headings."""
    at, found = _find(lines, pattern, wanted)
    count = int(found.group(1))
    entries = []
    for line in lines[at + 1 : at + 1 + count]:
        name, colon, reason = line[2:].partition(": ")
        if not line.startswith("  ") or line.startswith("== ") or not colon:
            break
        entries.append((name, reason))
    if len(entries) != count:
        raise ReportError(f"「{wanted}」的标题说有 {count} 张，下面列了 {len(entries)} 张")
    return _keyed(entries)


def parse_report(text):
    """Read a walk's report.txt. Raises ReportError naming what is missing."""
    lines = text.splitlines()
    walk = Walk()
    for line in lines:
        found = SOURCE.match(line)
        if found:
            walk.source = found.group(1)
            break
    walk.drawings = int(_find(lines, DRAWINGS, "== drawings (N; M without a file) ==")[1].group(1))

    if any(line.startswith(IMPORT_SKIPPED) for line in lines):
        raise ReportError("这次走查跳过了导入（OCS_PID_BACKUP_AUDIT_SKIP_IMPORT），没有可比的数")
    if IMPORT_HEADING not in lines:
        raise ReportError(f"报告里没有导入一节「{IMPORT_HEADING}」")
    rows, totals = [], None
    for line in lines[lines.index(IMPORT_HEADING) + 1 :]:
        if line.startswith("== "):
            break
        row = IMPORT_ROW.match(line)
        if row:
            numbers = [int(row.group(at)) for at in (2, 3, 4, 5)]
            rows.append((row.group(1), Import(*numbers, float(row.group(6)))))
            continue
        totals = IMPORT_TOTALS.match(line)
        if totals:
            break
    if totals is None:
        raise ReportError(
            "导入一节没有合计行「N entities drawn, M source records not drawn; all imported in ...」"
        )
    walk.imports = _keyed(rows)
    walk.entities, walk.undrawn = int(totals.group(1)), int(totals.group(2))
    walk.import_millis = float(totals.group(3))
    # A row read wrong would move every number after it: the report's own
    # totals are the check that the rows were read as written.
    drawn = sum(found.drawn for _, found in rows)
    missing = sum(found.missing for _, found in rows)
    if (drawn, missing) != (walk.entities, walk.undrawn):
        raise ReportError(
            f"导入一节各行加起来是 {drawn} 个图元、{missing} 条没画出，"
            f"合计行写的是 {walk.entities}、{walk.undrawn}"
        )

    walk.unread = _failures(lines, UNREAD, "== N drawing(s) whose items did not read (...) ==")
    walk.unopened = _failures(lines, UNOPENED, "== N drawing(s) that did not import ==")
    return walk


def load(path):
    """The walk a report.txt, or the folder holding one, describes."""
    path = Path(path)
    if path.is_dir():
        path = path / "report.txt"
    try:
        return parse_report(path.read_text(encoding="utf-8"))
    except OSError as error:
        raise ReportError(f"{path}: 读不了（{error.strerror or error}）") from None
    except UnicodeDecodeError:
        raise ReportError(f"{path}: 不是 UTF-8 文本") from None
    except ReportError as error:
        raise ReportError(f"{path}: {error}") from None


def _duration(millis):
    return f"{millis:.1f} ms" if millis < 1000 else f"{millis / 1000:.1f} s"


def _delta(before, after):
    return "不变" if after == before else f"{after - before:+d}"


def _cell(text):
    """Text that stays inside one cell of a Markdown table."""
    return text.replace("|", "\\|")


def _name(name):
    name = _cell(name)
    return name if "`" in name else f"`{name}`"


def _reason(reason, limit=120):
    reason = _cell(reason)
    return reason if len(reason) <= limit else reason[: limit - 1] + "…"


def compare(before, after, label="", max_time_ratio=1.5, time_floor_ms=5000.0):
    """The comparison of two walks: (its Markdown lines, what got worse).

    `worse` holds one sentence per thing that got worse and is empty when
    nothing did.
    """
    out, worse = [], []
    out += [f"**走查对比：{label}**" if label else "**走查对比**", ""]
    if before.source != after.source:
        out += [f"注意：两份报告的来源不同（前 `{before.source}`，后 `{after.source}`）。", ""]

    # ── The totals ─────────────────────────────────────────────────────────────
    out += ["| 项 | 前 | 后 | 变化 |", "|---|---:|---:|---|"]
    for title, old, new in (
        ("图纸数", before.drawings, after.drawings),
        ("读不出图内对象的图", len(before.unread), len(after.unread)),
        ("打不开的图", len(before.unopened), len(after.unopened)),
        ("画出的图元", before.entities, after.entities),
        ("没画出的记录", before.undrawn, after.undrawn),
    ):
        out.append(f"| {title} | {old} | {new} | {_delta(old, new)} |")
    if before.drawings != after.drawings:
        worse.append(f"图纸数从 {before.drawings} 变成 {after.drawings}")
    ratio_shown = "—"
    if before.import_millis > 0:
        ratio = after.import_millis / before.import_millis
        ratio_shown = f"{ratio:.2f} 倍"
        if max_time_ratio > 0 and ratio > max_time_ratio:
            if max(before.import_millis, after.import_millis) >= time_floor_ms:
                over = f"超过上限 {max_time_ratio:g} 倍"
                ratio_shown += f"，{over}"
                worse.append(f"全部导入用时是原来的 {ratio:.2f} 倍，{over}")
            else:
                ratio_shown += f"（不足 {_duration(time_floor_ms)}，只报不判）"
    out.append(
        f"| 全部导入用时 | {_duration(before.import_millis)} | {_duration(after.import_millis)} "
        f"| {ratio_shown} |"
    )
    out.append("")

    # ── The two lists of failures, and drawings in one report only ────────────
    moved = False
    for title, kind, old, new in (
        ("读不出图内对象的名单", "读不出图内对象的图", before.unread, after.unread),
        ("打不开的名单", "打不开的图", before.unopened, after.unopened),
    ):
        joined = [name for name in new if name not in old]
        left = [name for name in old if name not in new]
        if not joined and not left:
            out.append(f"- {title}：没有变化（{len(new)} 张）。")
            continue
        moved = True
        parts = []
        if joined:
            parts.append(
                "新增 " + "、".join(f"{_name(name)}（{_reason(new[name])}）" for name in joined)
            )
            worse += [f"新增{kind}：{_name(name)}" for name in joined]
        if left:
            parts.append("减少 " + "、".join(_name(name) for name in left))
        out.append(f"- {title}：" + "；".join(parts) + "。")
    names_before, names_after = before.names(), after.names()
    for side, mine, theirs in (("前", names_before, names_after), ("后", names_after, names_before)):
        alone = [name for name in mine if name not in theirs]
        if alone:
            moved = True
            out.append(f"- 只在「{side}」里的图：" + "、".join(_name(name) for name in alone) + "。")
            worse += [f"只在「{side}」的报告里的图：{_name(name)}" for name in alone]
    out.append("")

    # ── The four numbers of each drawing ───────────────────────────────────────
    rows, compared, marked = [], 0, 0
    for name in dict.fromkeys([*names_before, *names_after]):
        old, new = before.imports.get(name), after.imports.get(name)
        if old is None and new is None:
            continue
        compared += 1
        old_numbers = old.numbers() if old else (NOT_IMPORTED,) * 4
        new_numbers = new.numbers() if new else (NOT_IMPORTED,) * 4
        if old_numbers == new_numbers:
            continue
        marks = []
        if old and new:
            if new.drawn < old.drawn:
                marks.append("图元变少")
            if new.missing > old.missing:
                marks.append("没画出的变多")
        elif old:
            marks.append("后来没导入")
        marked += bool(marks)
        cells = [
            str(was) if was == now else f"{was} → {now}"
            for was, now in zip(old_numbers, new_numbers)
        ]
        rows.append(f"| {_name(name)} | " + " | ".join(cells) + f" | {'；'.join(marks)} |")
    if rows:
        out += [
            f"逐张的四个数：{len(rows)} 张有变化（共 {compared} 张）。",
            "",
            "| 图纸 | 画出的图元 | 解出的记录 | 没画出的记录 | 图纸层 | 要写原因 |",
            "|---|---|---|---|---|---|",
            *rows,
            "",
        ]
        if marked:
            out += ["「要写原因」一栏有字的图，逐张写明原因，指到解析库的提交或它的说明。", ""]
    else:
        out += [f"逐张的四个数：没有变化（共 {compared} 张）。", ""]

    if not rows and not moved and before.drawings == after.drawings:
        out += ["两份报告没有变化（用时不算）。", ""]
    # A marked drawing is worse unless its change has a reason, and a reason
    # is not something this tool can read: it leaves the verdict open.
    owed = f"{marked} 张图标了「要写原因」，说得出原因才算没有变差"
    if worse:
        out.append(f"结论：变差 {len(worse)} 处。")
        out += [f"- {what}" for what in worse]
        if marked:
            out.append(f"- 另有 {owed}")
    elif marked:
        out.append(f"结论：名单、图纸数和用时没有变差；{owed}。")
    else:
        out.append("结论：没有变差。")
    return out, worse


def main(argv=None):
    parser = argparse.ArgumentParser(
        prog="compare_walks.py",
        description="Compare two reports of the SmartPlant backup walk, before and after a change.",
    )
    parser.add_argument("before", help="report.txt of the earlier walk, or the folder holding it")
    parser.add_argument("after", help="report.txt of the later walk, or the folder holding it")
    parser.add_argument("--label", default="", help="what was walked, for the heading")
    parser.add_argument(
        "--max-time-ratio",
        type=float,
        default=1.5,
        help="how many times slower all imports may get (default 1.5; 0 judges no time)",
    )
    parser.add_argument(
        "--time-floor-ms",
        type=float,
        default=5000.0,
        help="the time is judged only when a run took at least this long (default 5000)",
    )
    parser.add_argument("--out", help="also write the Markdown to this file, as UTF-8")
    try:
        args = parser.parse_args(argv)
    except SystemExit as stop:
        return 2 if stop.code else 0

    # A console that cannot show Chinese must not turn a verdict into a crash.
    for stream in (sys.stdout, sys.stderr):
        if hasattr(stream, "reconfigure"):
            stream.reconfigure(errors="backslashreplace")
    try:
        before, after = load(args.before), load(args.after)
    except ReportError as error:
        print(f"compare_walks: {error}", file=sys.stderr)
        return 2
    lines, worse = compare(before, after, args.label, args.max_time_ratio, args.time_floor_ms)
    text = "\n".join(lines) + "\n"
    if args.out:
        Path(args.out).write_text(text, encoding="utf-8", newline="\n")
    sys.stdout.write(text)
    return 1 if worse else 0


if __name__ == "__main__":
    sys.exit(main())
