#!/usr/bin/env python3
"""Sum up the walks of the sample backups that walk-samples.ps1 ran.

walk-samples.ps1 walks every sample backup twice, once in the classic
interface and once in the P&ID module, each walk held to its expectation, and
leaves a `runs.json` naming what it ran and the folder each walk wrote its
report.txt to. This tool reads those reports and prints, as Markdown ready
for readings.md:

- one row per walk: drawings, drawings whose items did not read, drawings that
  did not import, entities drawn, the drawings opened from the panel (in the
  module), the times, the expectation, and whether the walk passes;
- for every backup walked in both interfaces, and for every backup walked
  both from its folder and from its zip: whether the two walks agree;
- the drawings that did not read or did not import, each with the reason its
  report gives.

    python scripts/sppid/walk_summary.py RUNS_JSON [--out FILE]

A walk passes when its test passed, its report is the report of that very
walk (same source, same interface, same expectation), the report's lists stay
within the expectation, every listed drawing has a reason, and in the module
no drawing opened other than an open owes. The numbers are judged here from
the report's lists: what the report says of its own expectation is not taken
for it.

Two walks agree when they count the same drawings, name the same drawings,
list the same drawings as unread and as not imported, and give every drawing
the same four import numbers.

Exit code:

- 0: every walk passes and every comparison agrees;
- 1: a walk does not pass, a comparison does not agree or cannot be made, or
  there is no walk at all;
- 2: the arguments or runs.json cannot be used, or the file of --out cannot
  be written.

runs.json:

    {"partial": false,
     "runs": [{"package": "small", "form": "folder", "interface": "module",
               "source": "D:/samples/small", "expect": "7,1,1",
               "folder": "D:/out/small-module", "exit_code": 0,
               "seconds": 97.1, "note": ""}]}

`package` is plant, small, sqlserver, or any other name; `form` is folder or
zip; `interface` is classic or module; `expect` is the walk's
OCS_PID_BACKUP_AUDIT_EXPECT; `folder` holds the walk's report.txt and
cargo-test.log; `exit_code` is what cargo returned, null when the walk was not
run, and then `note` says why; `seconds` is how long cargo took; `partial` is
true when only some of the sample backups were walked.

The Markdown is Chinese, as readings.md is. A home folder in it is written
%USERPROFILE%: the summary goes into a document that is committed.
"""

import argparse
from dataclasses import dataclass, field
import json
from pathlib import Path
import re
import sys

import compare_walks
from compare_walks import ReportError, _cell, _duration, _name, _reason

INTERFACE = "interface: the P&ID module"
EXPECTATION = re.compile(
    r"^== expectation (\d+),(\d+),(\d+) \(drawings, items unread at most, not imported at most\): "
    r"(met|not met) ==$"
)
OPENED = re.compile(r"^  (\d+) opened: .*; (\d+) not as an open owes; all in (\d+(?:\.\d+)?) ms$")
NOT_AS_OWED = "  not as an open owes: "
FINISHED = re.compile(r"^test result: .* finished in (\d+(?:\.\d+)?)s$")
# Up to the next separator: a name may hold spaces, and too much hidden is
# better than half a name shown. A separator is either slash, once or twice:
# the walk prints a path it holds as Some("...") with every backslash doubled.
HOME = re.compile(r"(?<![A-Za-z0-9])[A-Za-z]:[\\/]+Users[\\/]+[^\\/\r\n]+", re.IGNORECASE)

PACKAGES = {"plant": "全厂包", "small": "小包", "sqlserver": "SQL Server 包"}
FORMS = {"folder": "目录", "zip": "zip"}
INTERFACES = {"classic": "经典", "module": "新界面"}


class ManifestError(Exception):
    """A runs.json that cannot be read as the list of the walks."""


@dataclass
class Opened:
    """The totals of the drawings a walk in the module opened from the panel."""

    opened: int
    off: int
    millis: float
    # What the report says of each drawing that opened other than an open owes.
    said: list = field(default_factory=list)


@dataclass
class Run:
    """One walk: what runs.json says of it, then what its folder holds."""

    package: str
    form: str
    interface: str
    source: str
    # Drawings, drawings unread at most, drawings not imported at most.
    expect: tuple
    folder: Path
    exit_code: object = None
    seconds: object = None
    note: str = ""
    # A compare_walks.Walk, when the report reads as one.
    walk: object = None
    opened: object = None
    # How long the walk says it took, the start of the tests left out.
    walk_seconds: object = None
    # Why the walk does not pass, as Markdown fit for a table cell; empty when it does.
    reasons: list = field(default_factory=list)


def _package(name):
    return PACKAGES.get(name, _cell(name))


def _expectation(text):
    """`D,U,N` as three numbers; None when it is anything else."""
    found = re.fullmatch(r"(\d+),(\d+),(\d+)", text)
    return tuple(int(part) for part in found.groups()) if found else None


def _shown(expect):
    return "`" + ",".join(str(part) for part in expect) + "`"


def _span(seconds):
    minutes, rest = divmod(round(seconds), 60)
    return f"{minutes} 分 {rest} 秒" if minutes else f"{rest} 秒"


def _same_path(one, other):
    def plain(path):
        return path.replace("/", "\\").rstrip("\\").casefold()

    return plain(one) == plain(other)


def load_runs(path):
    """What a runs.json names: (the walks, whether they are only a part)."""
    try:
        manifest = json.loads(Path(path).read_text(encoding="utf-8-sig"))
    except OSError as error:
        raise ManifestError(f"{path}: 读不了（{error.strerror or error}）") from None
    except UnicodeDecodeError:
        raise ManifestError(f"{path}: 不是 UTF-8 文本") from None
    except json.JSONDecodeError as error:
        raise ManifestError(f"{path}: 不是 JSON（{error}）") from None
    if not isinstance(manifest, dict) or not isinstance(manifest.get("runs"), list):
        raise ManifestError(f"{path}: 没有 runs 一项")
    runs, met = [], {}
    for at, entry in enumerate(manifest["runs"], 1):
        which = f"{path}: 第 {at} 次走查"
        if not isinstance(entry, dict):
            raise ManifestError(f"{which}不是一个对象")
        for key in ("package", "form", "interface", "source", "expect", "folder"):
            if not isinstance(entry.get(key), str) or not (entry[key] or key == "expect"):
                raise ManifestError(f"{which}缺 {key}")
        for key, known in (("form", FORMS), ("interface", INTERFACES)):
            if entry[key] not in known:
                raise ManifestError(
                    f"{which}的 {key} 是「{entry[key]}」，只能是 {' 或 '.join(known)}"
                )
        expect = _expectation(entry["expect"])
        if expect is None:
            raise ManifestError(
                f"{which}的期望值「{entry['expect']}」不是三个数（图纸数,读不出的上限,打不开的上限）"
            )
        exit_code = entry.get("exit_code")
        if exit_code is not None and (isinstance(exit_code, bool) or not isinstance(exit_code, int)):
            raise ManifestError(f"{which}的 exit_code 不是整数")
        seconds = entry.get("seconds")
        if isinstance(seconds, bool) or not isinstance(seconds, (int, float)):
            seconds = None
        key = (entry["package"], entry["form"], entry["interface"])
        if key in met:
            raise ManifestError(f"{which}和第 {met[key]} 次是同一个（{' / '.join(key)}）")
        met[key] = at
        runs.append(
            Run(
                *key,
                source=entry["source"],
                expect=expect,
                folder=Path(entry["folder"]),
                exit_code=exit_code,
                seconds=seconds,
                note=str(entry.get("note") or ""),
            )
        )
    return runs, bool(manifest.get("partial"))


def read(run):
    """Read what a walk left in its folder, and note why the walk does not pass."""
    try:
        log = (run.folder / "cargo-test.log").read_text(encoding="utf-8", errors="replace")
    except OSError:
        log = ""
    for line in log.splitlines():
        found = FINISHED.match(line)
        if found:
            run.walk_seconds = float(found.group(1))
    if run.exit_code is None:
        run.reasons.append(f"没有走查（{_cell(run.note)}）" if run.note else "没有走查")
        return
    if run.exit_code != 0:
        run.reasons.append(f"走查的测试失败（退出码 {run.exit_code}）")
    try:
        text = (run.folder / "report.txt").read_text(encoding="utf-8")
        run.walk = compare_walks.parse_report(text)
    except FileNotFoundError:
        run.reasons.append("没有报告")
    except OSError as error:
        run.reasons.append(_cell(f"报告读不了（{error.strerror or error}）"))
    except UnicodeDecodeError:
        run.reasons.append("报告不是 UTF-8 文本")
    except ReportError as error:
        run.reasons.append(_cell(f"报告不能用：{error}"))
    else:
        run.reasons += _unmet(run, text.splitlines())


def _unmet(run, lines):
    """Why the report a walk left does not let the walk pass."""
    walk, reasons = run.walk, []
    if not _same_path(walk.source, run.source):
        reasons.append(f"报告的来源是 `{_cell(walk.source)}`，这次走查的是 `{_cell(run.source)}`")

    # The interface is named before the first section, or not at all.
    head = next((at for at, line in enumerate(lines) if line.startswith("== ")), len(lines))
    in_module = any(line.startswith(INTERFACE) for line in lines[:head])
    if in_module != (run.interface == "module"):
        left, asked = ("新界面", "经典界面") if in_module else ("经典界面", "新界面")
        reasons.append(f"报告是{left}下的，这次走的是{asked}")

    stated = None
    for at, line in enumerate(lines):
        found = EXPECTATION.match(line)
        if found:
            said = []
            for below in lines[at + 1 :]:
                if not below.startswith("  "):
                    break
                said.append(_cell(below.strip()))
            stated = (tuple(int(part) for part in found.groups()[:3]), found.group(4) == "met", said)
    drawings, unread, unopened = run.expect
    beyond = []
    if walk.drawings != drawings:
        beyond.append(f"图纸 {walk.drawings} 张，期望 {drawings} 张")
    for title, listed, limit in (("读不出", walk.unread, unread), ("打不开", walk.unopened, unopened)):
        if len(listed) > limit:
            names = "、".join(_name(name) for name in listed)
            beyond.append(f"{title} {len(listed)} 张，上限 {limit} 张（{names}）")
    if stated is None:
        reasons.append("报告里没有期望值一节")
    elif stated[0] != run.expect:
        reasons.append(f"报告里的期望值是 {_shown(stated[0])}，这次要的是 {_shown(run.expect)}")
    elif not stated[1] and not beyond:
        # Not met by something the lists do not show: the report's own words.
        reasons.append("报告说期望值没有达到" + (f"（{'；'.join(stated[2])}）" if stated[2] else ""))
    reasons += beyond

    silent = [f"读不出的 {_name(name)}" for name, why in walk.unread.items() if not why.strip()]
    silent += [f"打不开的 {_name(name)}" for name, why in walk.unopened.items() if not why.strip()]
    if silent:
        reasons.append("没有写原因：" + "、".join(silent))

    if in_module:
        for at, line in enumerate(lines):
            found = OPENED.match(line)
            if not found:
                continue
            run.opened = Opened(int(found.group(1)), int(found.group(2)), float(found.group(3)))
            for below in lines[at + 1 :]:
                if not below.startswith(NOT_AS_OWED):
                    break
                run.opened.said.append(_cell(below[len(NOT_AS_OWED) :]))
            break
        if run.opened is None:
            reasons.append("报告里没有从面板开图的合计行")
        elif run.opened.off:
            said = f"（{'；'.join(run.opened.said)}）" if run.opened.said else ""
            reasons.append(f"从面板打开的图有 {run.opened.off} 处不符{said}")
    return reasons


def _row(run):
    """The table row of one walk."""
    walk = run.walk
    if walk:
        numbers = [walk.drawings, len(walk.unread), len(walk.unopened), walk.entities]
        imported = _duration(walk.import_millis)
    else:
        numbers, imported = ["—"] * 4, "—"
    opened = "—"
    if run.opened:
        opened = f"{run.opened.opened} 张，{run.opened.off} 处不符，{_duration(run.opened.millis)}"
    cells = [
        _package(run.package),
        FORMS[run.form],
        INTERFACES[run.interface],
        *(str(number) for number in numbers),
        opened,
        imported,
        "—" if run.walk_seconds is None else f"{run.walk_seconds:.0f} s",
        _shown(run.expect),
        "不过：" + "；".join(run.reasons) if run.reasons else "过",
    ]
    return "| " + " | ".join(cells) + " |"


def _only(one, mine, other, theirs):
    """The names only one of two lists holds, said in words; empty when none."""
    parts = []
    for side, names, others in ((one, mine, theirs), (other, theirs, mine)):
        alone = [name for name in names if name not in others]
        if alone:
            parts.append(f"只在{side}里的 " + "、".join(_name(name) for name in alone))
    return "，".join(parts)


def differences(one, mine, other, theirs, shown=8):
    """Where two walks of one backup disagree: a phrase per thing, none when
    they agree. `one` and `other` name the two sides in the phrases."""
    found = []
    if mine.drawings != theirs.drawings:
        found.append(f"图纸数：{one} {mine.drawings}，{other} {theirs.drawings}")
    for title, names, others in (
        ("图纸名单", mine.names(), theirs.names()),
        ("读不出的名单", mine.unread, theirs.unread),
        ("打不开的名单", mine.unopened, theirs.unopened),
    ):
        only = _only(one, names, other, others)
        if only:
            found.append(f"{title}：{only}")
    changed = [
        name
        for name, numbers in mine.imports.items()
        if name in theirs.imports and numbers.numbers() != theirs.imports[name].numbers()
    ]
    for name in changed[:shown]:
        here = "/".join(str(number) for number in mine.imports[name].numbers())
        there = "/".join(str(number) for number in theirs.imports[name].numbers())
        found.append(f"{_name(name)} 的四个数：{one} {here}，{other} {there}")
    if len(changed) > shown:
        found.append(f"另有 {len(changed) - shown} 张图的四个数不同")
    return found


def comparisons(runs):
    """The pairs of walks that must agree, in the order of the walks:
    (kind, what is compared, name of one side, its run, name of the other, its run)."""
    by = {(run.package, run.form, run.interface): run for run in runs}
    pairs = []
    for run in runs:
        other = by.get((run.package, run.form, "module"))
        if run.interface == "classic" and other:
            label = f"{_package(run.package)}（{FORMS[run.form]}）"
            pairs.append(("interfaces", label, "经典", run, "新界面", other))
    for run in runs:
        other = by.get((run.package, "zip", run.interface))
        if run.form == "folder" and other:
            label = f"{_package(run.package)}，{INTERFACES[run.interface]}"
            pairs.append(("forms", label, "目录", run, "zip", other))
    return pairs


def _compared(label, one, run, other, against):
    """The line of one comparison, and whether the two walks agree."""
    lacking = [side for side, walked in ((one, run), (other, against)) if walked.walk is None]
    if len(lacking) == 2:
        return f"- {label}：没法比，两次都没有可用的报告。", False
    if lacking:
        return f"- {label}：没法比，{lacking[0]}那一次没有可用的报告。", False
    found = differences(one, run.walk, other, against.walk)
    if found:
        return f"- {label}：不同。" + "；".join(found) + "。", False
    walk = run.walk
    return f"- {label}：相同。{walk.drawings} 张图，导入了的 {len(walk.imports)} 张逐张比过。", True


def summary(runs, partial=False):
    """The Markdown of the walks: (its lines, what does not pass).

    `failed` holds one sentence per walk that does not pass and per
    comparison that does not agree, and is empty when everything passes.
    """
    out, failed = ["**样本包走查汇总**", ""], []
    if partial:
        out += ["这次只走了一部分样本包，不是完整的验收。", ""]
    if not runs:
        return [*out, "结论：一次走查也没有。"], ["一次走查也没有"]

    # ── One row per walk ─────────────────────────────────────────────────────────
    out += [
        "| 包 | 目录或 zip | 界面 | 图纸 | 读不出图内对象 | 打不开 | 画出的图元 | 从面板打开（新界面） "
        "| 全部导入用时 | 走查用时 | 期望值 | 结果 |",
        "|---|---|---|---:|---:|---:|---:|---|---:|---:|---|---|",
    ]
    for run in runs:
        out.append(_row(run))
        if run.reasons:
            where = f"{_package(run.package)}（{FORMS[run.form]}），{INTERFACES[run.interface]}"
            failed.append(f"{where}：" + "；".join(run.reasons))
    notes = (
        "期望值的三个数：图纸数、读不出图内对象的图最多几张、打不开的图最多几张。"
        "「从面板打开」是新界面下逐张从备份面板打开的图数、打开后和应有的不符的处数、这些图打开一共用的时间。"
        "走查用时是走查这条测试自己报的"
    )
    timed = [run.seconds for run in runs if run.seconds is not None]
    if timed:
        notes += f"；连同 cargo 自己花的时间，{len(timed)} 次走查共用 {_span(sum(timed))}"
    out += ["", notes + "。", ""]

    # ── The walks that must agree ────────────────────────────────────────────────
    pairs = comparisons(runs)
    for kind, heading, short in (
        (
            "interfaces",
            "同一个包的两种界面（图纸数、图纸名单、读不出的名单、打不开的名单、逐张的四个数都要相同）：",
            "两种界面的比较",
        ),
        ("forms", "同一个包从目录打开和从 zip 打开（要相同的同上）：", "目录和 zip 的比较"),
    ):
        mine = [pair[1:] for pair in pairs if pair[0] == kind]
        if not mine:
            continue
        out += [heading, ""]
        for label, one, run, other, against in mine:
            line, same = _compared(label, one, run, other, against)
            out.append(line)
            if not same:
                failed.append(f"{short}：{label}")
        out.append("")

    # ── The drawings that failed, with their reasons ──────────────────────────────
    out += ["读不出、打不开的图和报告里写的原因（每个包取第一份可用的报告）：", ""]
    told = set()
    for run in runs:
        if run.package in told or run.walk is None:
            continue
        told.add(run.package)
        walk = run.walk
        listed = [("读不出", name, why) for name, why in walk.unread.items()]
        listed += [("打不开", name, why) for name, why in walk.unopened.items()]
        if not listed:
            out.append(f"- {_package(run.package)}：没有。")
            continue
        out.append(
            f"- {_package(run.package)}（读不出 {len(walk.unread)} 张，打不开 {len(walk.unopened)} 张）："
        )
        out += [
            f"  - {kind} {_name(name)}：{_reason(why) if why.strip() else '（没有写原因）'}"
            for kind, name, why in listed
        ]
    for package in dict.fromkeys(run.package for run in runs):
        if package not in told:
            out.append(f"- {_package(package)}：没有可用的报告。")
    out.append("")

    if failed:
        out.append(f"结论：{len(failed)} 处不过。")
        out += [f"- {what}" for what in failed]
    else:
        agreed = f"，{len(pairs)} 项比较全部相同" if pairs else ""
        out.append(f"结论：{len(runs)} 次走查全部通过{agreed}。")
    return out, failed


def main(argv=None):
    parser = argparse.ArgumentParser(
        prog="walk_summary.py",
        description="Sum up the walks of the sample backups that walk-samples.ps1 ran.",
    )
    parser.add_argument("runs", help="the runs.json that walk-samples.ps1 wrote")
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
        runs, partial = load_runs(args.runs)
    except ManifestError as error:
        print(f"walk_summary: {error}", file=sys.stderr)
        return 2
    for run in runs:
        read(run)
    lines, failed = summary(runs, partial)
    text = HOME.sub("%USERPROFILE%", "\n".join(lines) + "\n")
    if args.out:
        try:
            Path(args.out).write_text(text, encoding="utf-8", newline="\n")
        except OSError as error:
            print(f"walk_summary: {args.out}: 写不了（{error.strerror or error}）", file=sys.stderr)
            return 2
    sys.stdout.write(text)
    return 1 if failed else 0


if __name__ == "__main__":
    sys.exit(main())
