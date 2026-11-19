#!/usr/bin/env python3
"""Sum up the PDF exports that pdf-compare.ps1 ran on the sample backups.

pdf-compare.ps1 has the program export every sample backup twice (the test
`a_real_backup_exports_the_same_pages_twice_and_stays_as_it_was`), has
`pid_plot --batch` plot the restored backup the program exported, has the
program plot the vendor's sheets and `pid_plot_compare` measure each against
the vendor's PDF, and leaves a `runs.json` naming what it ran and the folder
each step wrote to. This tool reads those folders and prints, as Markdown
ready for readings.md:

- one row per export: the drawings, the PDFs the program wrote, the PDFs
  `pid_plot --batch` wrote of the same drawings and how many differ, the PDFs
  that are one page of the size the index states, whether the second export
  equals the first, whether the backup and its restore stayed as they were,
  the times, the expectation, and whether the export passes;
- for every backup exported both from its folder and from its zip: whether
  the two exports wrote the same PDFs;
- one row per sheet of the vendor: coverage, extra length and matched text as
  `pid_plot_compare` measured them, each beside what pid-plot's own acceptance
  recorded, and whether the sheet is no worse than its record;
- the drawings whose PDF differs from the one of `pid_plot --batch`, and the
  drawings without a PDF, each with the reason the index gives.

    python scripts/sppid/pdf_compare.py RUNS_JSON [--out FILE]

An export passes when its test passed, its report is the report of that very
backup, every entry of the first export's index has either a PDF or a reason,
the folder holds the PDFs the index names and nothing else, the numbers of
drawings and of PDFs are the expected ones, every PDF is one page within
0.01 mm of the size its entry states, the two exports hold the same files
with the same bytes, the backup and its restore have the same files with the
same checksums as before, `pid_plot --batch` lists every drawing the program
wrote and writes none the program did not, the PDFs both wrote are the same
bytes, and the export stayed within its time limit when it has one.

The verdicts are made here: the files of the exports and of `pid_plot
--batch` are read and compared, and the numbers of the report are held to the
tolerances. What the report says of itself is shown when the test failed and
is not taken for the verdict. Two things are measured where the export runs
and taken from the report: the pages and their sizes, which the test reads
with lopdf, and the checksums of the backup before and after.

`pid_plot --batch` plots every `.pid` under the restored backup, also the
border templates and assemblies of the reference data, which are no drawings
of the backup. Those PDFs are counted apart and compared with nothing.

A sheet of the vendor passes when it was plotted and measured, has a record,
its coverage is at most 0.001 below the record, its extra length at most 1 mm
above, its matched text not less, and its fit within the threshold when the
record's is.

Exit code:

- 0: every export and every sheet passes and every comparison agrees;
- 1: something does not pass, or nothing was run at all;
- 2: the arguments or runs.json cannot be used, or the file of --out cannot
  be written.

runs.json:

    {"partial": false, "build": "Finished `test` profile [optimized] ...",
     "fonts": "D:/samples/plant/fonts", "reference": "D:/samples/plant",
     "tools": "D:/tools/96ee00c",
     "packages": [{"package": "small", "form": "folder",
                   "source": "D:/samples/small", "expect": "7,7",
                   "folder": "D:/out/small", "exit_code": 0, "seconds": 11.2,
                   "limit_seconds": null, "note": "",
                   "batch": {"folder": "D:/out/small-batch", "exit_code": 0,
                             "seconds": 2.5}}],
     "vendor": {"folder": "D:/samples/vendor", "expect": 11,
                "plots": "D:/out/vendor", "readings": "D:/out/vendor-compare",
                "exit_code": 0, "seconds": 19.5, "compare_seconds": 22.9,
                "note": "",
                "compares": [{"name": "Sheet 1", "exit_code": 0}]}}

`package` is plant, small, sqlserver, or any other name; `form` is folder or
zip; `expect` is the number of drawings and the number of PDFs; `folder`
holds the test's report.json and the folders run-1 and run-2; `exit_code` is
what cargo returned, null when the step was not run, and then `note` says
why; `limit_seconds` is how long one export may take, null for any time; the
folder of `batch` holds what `pid_plot --batch` wrote, pid_plot-batch.json
among it. `vendor` is null when the vendor's sheets were not asked for: its
`folder` is the vendor's, with the records in plot/acceptance, `plots` holds
the test's report.json, `readings` what `pid_plot_compare` wrote, `expect` is
the number of sheets, `compares` what `pid_plot_compare` returned for each
sheet, and `compare_seconds` how long all of its runs took together.
`partial` is true when only a part of the steps was run. `build`
is what cargo said of the test program, `fonts` the PID_PLOT_FONTS both sides
plotted with, `reference` the PID_PLOT_CORPUS, `tools` the folder of pid_plot
and pid_plot_compare: the four are named in the summary as they stand.

The Markdown is Chinese, as readings.md is. A home folder in it is written
%USERPROFILE%: the summary goes into a document that is committed.
"""

import argparse
from dataclasses import dataclass, field
import hashlib
import json
from pathlib import Path
import re
import sys

from compare_walks import _cell, _name, _reason
from walk_summary import FORMS, HOME, ManifestError, _package, _same_path, _span

REPORT = "report.json"
INDEX = "_PdfIndex.json"
BATCH = "pid_plot-batch.json"
# How far a page may be from the size its index entry states, in millimetres
# (SPEC-sppid-pdf §6, criterion 6).
PAGE_WITHIN_MM = 0.01
# How far below its record a coverage may lie, and how far above its record an
# extra length, in millimetres (criterion 8).
COVERAGE_WITHIN = 0.001
LENGTH_WITHIN_MM = 1.0
# Of the drawings a reason is about, how many it names.
SHOWN = 8
# What the test watched for change, as the table names it.
KEPT = {"source": "包", "package": "包根目录", "restored": "还原目录"}
# The backup nothing was accepted on (SPEC-sppid-pdf §6, criterion 9).
UNACCEPTED = "sqlserver"


@dataclass
class Batch:
    """One run of `pid_plot --batch` on a restored backup."""

    folder: Path
    exit_code: object = None
    seconds: object = None


@dataclass
class Differing:
    """A drawing whose PDF is not the one `pid_plot --batch` wrote."""

    name: str
    ours: int
    theirs: int
    # The first byte that differs, counted from 1: one past the shorter file
    # when that is the start of the longer.
    at: int


@dataclass
class Export:
    """One export: what runs.json says of it, then what its folders hold."""

    package: str
    form: str
    source: str
    # Drawings, PDFs written.
    expect: tuple
    folder: Path
    exit_code: object = None
    seconds: object = None
    limit: object = None
    note: str = ""
    batch: object = None
    # The cells of the table row that could be read, each a tuple.
    cells: dict = field(default_factory=dict)
    # The PDFs of the first export: file name -> (length, SHA-256).
    pdfs: object = None
    differing: list = field(default_factory=list)
    # The drawings without a PDF: (name, the reason the index gives).
    unplotted: list = field(default_factory=list)
    # Whether the test ran with debug assertions.
    debug: object = None
    # Why the export does not pass, as Markdown fit for a table cell.
    reasons: list = field(default_factory=list)


@dataclass
class Numbers:
    """What one comparison with a PDF of the vendor measured."""

    coverage: float
    extra: float
    matched: int
    reference: int
    # Whether the two pages fit within the threshold; None when it is not said.
    fit: object = None


@dataclass
class Sheet:
    """One sheet of the vendor: its reading and its record."""

    name: str
    reading: object = None
    record: object = None
    reasons: list = field(default_factory=list)


@dataclass
class Vendor:
    """The vendor's sheets: what runs.json says of them, then what was measured."""

    folder: Path
    plots: Path
    readings: Path
    expect: int
    exit_code: object = None
    seconds: object = None
    note: str = ""
    # What `pid_plot_compare` returned for each sheet, by its name, and how
    # long all of its runs took together.
    compares: dict = field(default_factory=dict)
    compare_seconds: object = None
    sheets: list = field(default_factory=list)
    debug: object = None
    # Why the sheets as a whole do not pass.
    reasons: list = field(default_factory=list)


def _number(value):
    return isinstance(value, (int, float)) and not isinstance(value, bool)


def _whole(value):
    return isinstance(value, int) and not isinstance(value, bool)


def _size(value):
    """Whether a JSON value is a width and a height."""
    return isinstance(value, list) and len(value) == 2 and all(_number(side) for side in value)


def _load(path):
    """The JSON a file holds: (the value, ""), or (None, why it cannot be had)."""
    try:
        return json.loads(Path(path).read_text(encoding="utf-8-sig")), ""
    except FileNotFoundError:
        return None, "没有这个文件"
    except OSError as error:
        return None, f"读不了（{error.strerror or error}）"
    except UnicodeDecodeError:
        return None, "不是 UTF-8 文本"
    except json.JSONDecodeError as error:
        return None, f"不是 JSON（{error}）"


def _held(folder):
    """The files right in a folder: name -> (length, SHA-256 of the bytes)."""
    held = {}
    for entry in sorted(Path(folder).iterdir()):
        if entry.is_file():
            data = entry.read_bytes()
            held[entry.name] = (len(data), hashlib.sha256(data).hexdigest())
    return held


def _parts(path):
    return [part for part in re.split(r"[\\/]+", path) if part]


def _key(path):
    """A path inside a backup, the same whether the index or `pid_plot --batch` wrote it."""
    return "/".join(_parts(path)).casefold()


def _listing(names, unit="张"):
    """Names for a reason: the first few, then how many there are."""
    said = "、".join(_name(name) for name in names[:SHOWN])
    return said + (f" 等 {len(names)} {unit}" if len(names) > SHOWN else "")


def _first_difference(one, other):
    """The first byte at which two files differ, counted from 1."""
    shared = min(len(one), len(other))
    return next((at + 1 for at in range(shared) if one[at] != other[at]), shared + 1)


def _label(run):
    return f"{_package(run.package)}（{FORMS[run.form]}）"


def _code(entry, which):
    """The exit code an entry of runs.json states: a whole number, or None."""
    code = entry.get("exit_code")
    if code is not None and not _whole(code):
        raise ManifestError(f"{which}的 exit_code 不是整数")
    return code


def _seconds(entry, key="seconds"):
    return entry[key] if _number(entry.get(key)) else None


def load_runs(path):
    """What a runs.json names: (the exports, the vendor's sheets or None,
    whether they are only a part, what it says of the build, the fonts, the
    reference folder and the tools)."""
    manifest, why = _load(path)
    if manifest is None:
        raise ManifestError(f"{path}: {why or '是空的'}")
    if not isinstance(manifest, dict) or not isinstance(manifest.get("packages"), list):
        raise ManifestError(f"{path}: 没有 packages 一项")
    exports, met = [], {}
    for at, entry in enumerate(manifest["packages"], 1):
        which = f"{path}: 第 {at} 次导出"
        if not isinstance(entry, dict):
            raise ManifestError(f"{which}不是一个对象")
        for key in ("package", "form", "source", "expect", "folder"):
            if not isinstance(entry.get(key), str) or not entry[key]:
                raise ManifestError(f"{which}缺 {key}")
        if entry["form"] not in FORMS:
            raise ManifestError(f"{which}的 form 是「{entry['form']}」，只能是 {' 或 '.join(FORMS)}")
        expect = re.fullmatch(r"(\d+),(\d+)", entry["expect"])
        if not expect:
            raise ManifestError(f"{which}的期望值「{entry['expect']}」不是两个数（图纸数,写出的 PDF 数）")
        key = (entry["package"], entry["form"])
        if key in met:
            raise ManifestError(f"{which}和第 {met[key]} 次是同一个（{' / '.join(key)}）")
        met[key] = at
        batch = entry.get("batch")
        if batch is not None:
            if not isinstance(batch, dict) or not isinstance(batch.get("folder"), str) or not batch["folder"]:
                raise ManifestError(f"{which}的 batch 缺 folder")
            batch = Batch(Path(batch["folder"]), _code(batch, f"{which}的 batch "), _seconds(batch))
        exports.append(
            Export(
                *key,
                source=entry["source"],
                expect=tuple(int(part) for part in expect.groups()),
                folder=Path(entry["folder"]),
                exit_code=_code(entry, which),
                seconds=_seconds(entry),
                limit=_seconds(entry, "limit_seconds"),
                note=str(entry.get("note") or ""),
                batch=batch,
            )
        )
    vendor = manifest.get("vendor")
    if vendor is not None:
        which = f"{path}: vendor "
        if not isinstance(vendor, dict):
            raise ManifestError(f"{which}不是一个对象")
        for key in ("folder", "plots", "readings"):
            if not isinstance(vendor.get(key), str) or not vendor[key]:
                raise ManifestError(f"{which}缺 {key}")
        if not _whole(vendor.get("expect")):
            raise ManifestError(f"{which}的 expect 不是整数")
        listed, compares = vendor.get("compares", []), {}
        if not isinstance(listed, list):
            raise ManifestError(f"{which}的 compares 不是一个列表")
        for compare in listed:
            if not isinstance(compare, dict) or not isinstance(compare.get("name"), str):
                raise ManifestError(f"{which}的 compares 里有一项没有 name")
            compares[compare["name"]] = _code(compare, f"{which}的 compares ")
        vendor = Vendor(
            Path(vendor["folder"]),
            Path(vendor["plots"]),
            Path(vendor["readings"]),
            vendor["expect"],
            exit_code=_code(vendor, which),
            seconds=_seconds(vendor),
            note=str(vendor.get("note") or ""),
            compares=compares,
            compare_seconds=_seconds(vendor, "compare_seconds"),
        )
    said = {key: str(manifest.get(key) or "") for key in ("build", "fonts", "reference", "tools")}
    return exports, vendor, bool(manifest.get("partial")), said


# ── One export ──────────────────────────────────────────────────────────────────────────────────


def read_export(run):
    """Read what an export left in its folders, and note why it does not pass."""
    if run.exit_code is None:
        run.reasons.append(f"没有导出（{_cell(run.note)}）" if run.note else "没有导出")
        return
    if run.exit_code != 0:
        run.reasons.append(f"导出的测试失败（退出码 {run.exit_code}）")
    report, why = _load(run.folder / REPORT)
    if not isinstance(report, dict):
        run.reasons.append(f"测试的报告 {REPORT}：{_cell(why or '不是一个对象')}")
        return
    source = str(report.get("source") or "")
    if not _same_path(source, run.source):
        run.reasons.append(f"报告的来源是 `{_cell(source)}`，这次导的是 `{_cell(run.source)}`")
    unmet = report.get("unmet")
    if run.exit_code != 0 and isinstance(unmet, list) and unmet:
        said = "；".join(_reason(str(what)) for what in unmet[:3])
        run.reasons.append(f"测试说：{said}" + (f" 等 {len(unmet)} 条" if len(unmet) > 3 else ""))
    if isinstance(report.get("debug_assertions"), bool):
        run.debug = report["debug_assertions"]

    index, why = _load(run.folder / "run-1" / INDEX)
    pages = index.get("pages") if isinstance(index, dict) else None
    if not isinstance(pages, list) or not all(isinstance(page, dict) for page in pages):
        run.reasons.append(f"第一遍的清单 run-1/{INDEX}：{_cell(why or '没有 pages 一项')}")
        return
    try:
        first, second = _held(run.folder / "run-1"), _held(run.folder / "run-2")
    except OSError as error:
        run.reasons.append(_cell(f"导出目录读不了（{error.strerror or error}）"))
        return
    _listed(run, pages, first)
    _measured(run, pages, report, first)
    _rerun(run, first, second)
    _kept(run, report)
    _timed(run, report)
    _against_batch(run, pages, report, first)


def _listed(run, pages, first):
    """The drawings and the PDFs of the first export, held to its index and to the expectation."""
    names = [page["pdf"] for page in pages if page.get("pdf")]
    run.cells["drawings"] = (len(pages),)
    run.cells["written"] = (len(names),)
    run.unplotted = [(str(page.get("name")), str(page.get("error") or "")) for page in pages if not page.get("pdf")]
    odd = [str(page.get("name")) for page in pages if bool(page.get("pdf")) == bool(page.get("error"))]
    if odd:
        run.reasons.append(f"清单里 {len(odd)} 条不是「要么有 PDF，要么有原因」（{_listing(odd, '条')}）")
    twice = sorted({name for name in names if names.count(name) > 1})
    if twice:
        run.reasons.append(f"清单里不止一张图写到同一个 PDF：{_listing(twice, '个')}")
    in_folder = set(first) - {INDEX}
    lacking, stray = sorted(set(names) - in_folder), sorted(in_folder - set(names))
    if lacking:
        run.reasons.append(f"清单里有而目录里没有的 PDF：{_listing(lacking, '个')}")
    if stray:
        run.reasons.append(f"目录里多出的文件：{_listing(stray, '个')}")
    run.pdfs = {name: first[name] for name in names if name in first}
    drawings, written = run.expect
    if len(pages) != drawings:
        run.reasons.append(f"图纸 {len(pages)} 张，期望 {drawings} 张")
    if len(names) != written:
        run.reasons.append(f"写出 {len(names)} 份，期望 {written} 份")


def _page_off(measured, stated, held):
    """Why a PDF is not one page of the size its index entry states; empty when it is.

    `measured` is what the test read of the PDF, `stated` its entry of the
    index, `held` the length and checksum of the file as it lies there now.
    """
    if measured is None:
        return "测试没有量过"
    if held is None or measured.get("bytes") != held[0]:
        return "测试量的不是这一份文件"
    if measured.get("unreadable"):
        return _reason(f"解析不了（{measured['unreadable']}）")
    if measured.get("starts_as_pdf") is not True:
        return "不以 %PDF- 开头"
    if measured.get("pages") != 1:
        return f"有 {measured.get('pages')} 页"
    media, page = measured.get("media_mm"), stated.get("page_mm")
    if not _size(media):
        return "量不出页面大小"
    if not _size(page):
        return "清单里没有 page_mm"
    if any(abs(got - said) > PAGE_WITHIN_MM for got, said in zip(media, page)):
        return f"页面 {media[0]:.3f} × {media[1]:.3f} 毫米，清单里是 {page[0]:.3f} × {page[1]:.3f}"
    return ""


def _measured(run, pages, report, first):
    """Every PDF one page of the size its entry states, as the test read it."""
    listed = report.get("pages") if isinstance(report.get("pages"), list) else []
    measured = {page["pdf"]: page for page in listed if isinstance(page, dict) and isinstance(page.get("pdf"), str)}
    good, off = 0, []
    for page in pages:
        pdf = page.get("pdf")
        if not pdf:
            continue
        why = _page_off(measured.get(pdf), page, first.get(pdf))
        if why:
            off.append(f"{_name(str(page.get('name')))} {why}")
        else:
            good += 1
    run.cells["pages"] = (good, len(off))
    if off:
        more = f" 等 {len(off)} 份" if len(off) > SHOWN else ""
        run.reasons.append(f"{len(off)} 份 PDF 不是恰好一页或页面大小对不上：" + "；".join(off[:SHOWN]) + more)


def _rerun(run, first, second):
    """The second export against the first: every file, the index among them."""
    differ = sorted(name for name in first.keys() | second.keys() if first.get(name) != second.get(name))
    run.cells["rerun"] = (len(first), differ)
    if differ:
        run.reasons.append(f"两次导出不同：{_listing(differ, '个文件')}")


def _sums(value):
    """Whether a JSON value is what the test says a folder held."""
    return isinstance(value, dict) and _whole(value.get("files")) and bool(value.get("sha256"))


def _kept(run, report):
    """The backup and its restore as they were: the files and checksums the test took."""
    listed = report.get("kept") if isinstance(report.get("kept"), list) else []
    kept = [entry for entry in listed if isinstance(entry, dict)]
    if not any(entry.get("what") == "source" for entry in kept):
        run.reasons.append("报告里没有包在导出前后的校验和")
    same, changed = [], []
    for entry in kept:
        what = KEPT.get(entry.get("what"), _cell(str(entry.get("what"))))
        before, after = entry.get("before"), entry.get("after")
        if not _sums(before) or not _sums(after):
            changed.append(f"{what}没有导出前后的校验和")
        elif (before["files"], before["sha256"]) != (after["files"], after["sha256"]) or entry.get("changed"):
            files = entry.get("changed") if isinstance(entry.get("changed"), list) else []
            said = f"，{len(files)} 个文件不同" if files else ""
            changed.append(f"{what}变了（文件 {before['files']} → {after['files']}{said}）")
        else:
            same.append(f"{what} {before['files']:,} 个文件")
    if kept:
        run.cells["kept"] = (same, changed)
    run.reasons += changed


def _timed(run, report):
    """How long the two exports took, held to the limit when there is one."""
    listed = report.get("exports") if isinstance(report.get("exports"), list) else []
    seconds = [entry["seconds"] for entry in listed if isinstance(entry, dict) and _number(entry.get("seconds"))]
    if seconds:
        run.cells["seconds"] = tuple(seconds)
    if len(seconds) != 2:
        run.reasons.append(f"报告里有用时的导出是 {len(seconds)} 次，应当是 2 次")
    if run.limit is not None and seconds and max(seconds) > run.limit:
        run.reasons.append(f"导出用了 {max(seconds):.0f} 秒，超过 {run.limit:.0f} 秒，要先报告")


def _against_batch(run, pages, report, first):
    """The PDFs of the first export against the ones `pid_plot --batch` wrote of the same drawings."""
    batch = run.batch
    if batch is None or batch.exit_code is None:
        run.reasons.append("没有跑 pid_plot --batch")
        return
    if batch.exit_code != 0:
        run.reasons.append(f"pid_plot --batch 失败（退出码 {batch.exit_code}）")
    summary, why = _load(batch.folder / BATCH)
    listed = summary.get("pages") if isinstance(summary, dict) else None
    if not isinstance(listed, list):
        run.reasons.append(f"pid_plot --batch 的汇总 {BATCH}：{_cell(why or '没有 pages 一项')}")
        return
    if not _same_path(str(summary.get("directory") or ""), str(report.get("restored") or "")):
        run.reasons.append("pid_plot --batch 出的不是这次导出的还原目录")
    theirs = {}
    for page in listed:
        if isinstance(page, dict) and isinstance(page.get("drawing"), str):
            theirs[_key(page["drawing"])] = page

    wrote = both = 0
    unlisted, alone, unread = [], [], []
    for page in pages:
        name, pid, pdf = str(page.get("name")), page.get("pid"), page.get("pdf")
        other = theirs.pop(_key(pid), None) if isinstance(pid, str) else None
        plotted = other is not None and not other.get("error") and isinstance(other.get("pdf"), str)
        if plotted:
            wrote += 1
        if pdf and other is None:
            unlisted.append(name)
        elif plotted and not pdf:
            alone.append(name)
        elif plotted and pdf in first:
            both += 1
            try:
                data = batch.folder.joinpath(*_parts(other["pdf"])).read_bytes()
                ours = (run.folder / "run-1" / pdf).read_bytes()
            except OSError:
                unread.append(name)
                continue
            if ours != data:
                run.differing.append(Differing(name, len(ours), len(data), _first_difference(ours, data)))
    # What is left was plotted on top of the drawings of the backup.
    run.cells["batch"] = (wrote, len(theirs))
    run.cells["both"] = (both, len(run.differing))
    if run.differing:
        names = _listing([found.name for found in run.differing])
        run.reasons.append(f"和 pid_plot --batch 不同的图 {len(run.differing)} 张（{names}）")
    if unread:
        run.reasons.append(f"两边的 PDF 读不了、没法比的图 {len(unread)} 张（{_listing(unread)}）")
    if unlisted:
        run.reasons.append(f"pid_plot --batch 没有列出、没法比的图 {len(unlisted)} 张（{_listing(unlisted)}）")
    if alone:
        run.reasons.append(f"pid_plot --batch 写出而本程序没有写出的图 {len(alone)} 张（{_listing(alone)}）")


def _row(run):
    """The table row of one export."""

    def cell(key, shown):
        return shown(*run.cells[key]) if key in run.cells else "—"

    cells = [
        _package(run.package) + ("（没有验收过）" if run.package == UNACCEPTED else ""),
        FORMS[run.form],
        cell("drawings", str),
        cell("written", str),
        cell("batch", lambda wrote, beyond: str(wrote) + (f"（另 {beyond} 份不在图纸清单里）" if beyond else "")),
        cell("both", lambda both, differ: f"{both} / {differ}"),
        cell("pages", lambda good, off: f"{good} / {good + off}"),
        cell("rerun", lambda files, differ: f"{len(differ)} 个文件不同" if differ else f"相同（{files} 个文件）"),
        cell("kept", lambda same, changed: "变了" if changed else "不变（" + "；".join(same) + "）"),
        cell("seconds", lambda *seconds: " / ".join(f"{second:.1f} s" for second in seconds)),
        "`" + ",".join(str(part) for part in run.expect) + "`",
        "不过：" + "；".join(run.reasons) if run.reasons else "过",
    ]
    return "| " + " | ".join(cells) + " |"


def _forms(run, other):
    """The line of one backup exported from its folder and from its zip, and whether the two agree."""
    label = _package(run.package)
    lacking = [FORMS[side.form] for side in (run, other) if side.pdfs is None]
    if lacking:
        which = "和".join(lacking)
        gap = " " if which[-1].isascii() else ""
        return f"- {label}：没法比，{which}{gap}那一次没有读到导出的 PDF。", False
    found = []
    for side, against in ((run, other), (other, run)):
        alone = sorted(name for name in side.pdfs if name not in against.pdfs)
        if alone:
            form = FORMS[side.form]
            where = f" {form} " if form.isascii() else form
            found.append(f"只在{where}里的 {_listing(alone, '个')}")
    changed = sorted(name for name in run.pdfs if name in other.pdfs and run.pdfs[name] != other.pdfs[name])
    if changed:
        found.append(f"字节不同的 {_listing(changed, '个')}")
    if found:
        return f"- {label}：不同。" + "；".join(found) + "。", False
    return f"- {label}：相同。{len(run.pdfs)} 份 PDF，名单相同，逐字节相同。", True


# ── The vendor's sheets ─────────────────────────────────────────────────────────────────────────


def _numbers(path):
    """The numbers of the comparison a file holds: (them, ""), or (None, why not)."""
    value, why = _load(path)
    if not isinstance(value, dict):
        return None, why or "不是一个对象"
    coverage, extra = value.get("coverage"), value.get("extra_length_mm")
    matched, reference = value.get("text_matched"), value.get("text_reference")
    if not (_number(coverage) and _number(extra) and _whole(matched) and _whole(reference)):
        return None, "缺覆盖率、多画的长度或文字条数"
    fit = value.get("fit")
    fit = fit.get("within_threshold") if isinstance(fit, dict) else None
    return Numbers(coverage, extra, matched, reference, fit if isinstance(fit, bool) else None), ""


def _worse(reading, record):
    """Where a reading is worse than its record by more than the tolerances."""
    found = []
    if reading.coverage < record.coverage - COVERAGE_WITHIN:
        found.append(f"覆盖率 {reading.coverage:.6f}，低于记录的 {record.coverage:.6f}")
    if reading.extra > record.extra + LENGTH_WITHIN_MM:
        found.append(f"多画 {reading.extra:.3f} 毫米，高于记录的 {record.extra:.3f} 毫米")
    if reading.matched < record.matched:
        found.append(f"文字对上 {reading.matched} 条，少于记录的 {record.matched} 条")
    if record.fit is True and reading.fit is not True:
        found.append("对位不在阈值内，记录里是在的")
    return found


def read_vendor(vendor):
    """Read what plotting and measuring the vendor's sheets left, and note what does not pass."""
    if vendor.exit_code is None:
        vendor.reasons.append(f"没有出图（{_cell(vendor.note)}）" if vendor.note else "没有出图")
        return
    if vendor.exit_code != 0:
        vendor.reasons.append(f"出图的测试失败（退出码 {vendor.exit_code}）")
    report, why = _load(vendor.plots / REPORT)
    listed = report.get("sheets") if isinstance(report, dict) else None
    if not isinstance(listed, list):
        vendor.reasons.append(f"出图的报告 {REPORT}：{_cell(why or '没有 sheets 一项')}")
        return
    if isinstance(report.get("debug_assertions"), bool):
        vendor.debug = report["debug_assertions"]
    for entry in listed:
        if not isinstance(entry, dict) or not isinstance(entry.get("name"), str):
            continue
        sheet = Sheet(entry["name"])
        vendor.sheets.append(sheet)
        sheet.record, without = _numbers(vendor.folder / "plot" / "acceptance" / f"{sheet.name}.json")
        if entry.get("pdf"):
            code = vendor.compares.get(sheet.name)
            if code is None:
                sheet.reasons.append("没有跑 pid_plot_compare")
            elif code != 0:
                sheet.reasons.append(f"pid_plot_compare 失败（退出码 {code}）")
            sheet.reading, unread = _numbers(vendor.readings / f"{sheet.name}.json")
            if sheet.reading is None and code == 0:
                sheet.reasons.append(_cell(f"读数：{unread}"))
        else:
            sheet.reasons.append("没有出图：" + _reason(str(entry.get("error") or "（没有写原因）")))
        if sheet.record is None:
            sheet.reasons.append(_cell(f"记录：{without}"))
        if sheet.reading is not None and sheet.record is not None:
            sheet.reasons += _worse(sheet.reading, sheet.record)
    if len(vendor.sheets) != vendor.expect:
        vendor.reasons.append(f"出了 {len(vendor.sheets)} 张，期望 {vendor.expect} 张")


def _sheet_row(sheet):
    """The table row of one sheet: each number of the reading beside the one of the record."""

    def shown(numbers):
        if numbers is None:
            return ["—"] * 3
        return [f"{numbers.coverage:.6f}", f"{numbers.extra:.3f}", f"{numbers.matched} / {numbers.reference}"]

    cells = [_name(sheet.name)]
    for reading, record in zip(shown(sheet.reading), shown(sheet.record)):
        cells += [reading, record]
    cells.append("不过：" + "；".join(sheet.reasons) if sheet.reasons else "过")
    return "| " + " | ".join(cells) + " |"


# ── The summary ─────────────────────────────────────────────────────────────────────────────────


def _built(exports, vendor):
    """Whether the tests ran with debug assertions, as far as their reports say."""
    seen = {run.debug for run in exports} | ({vendor.debug} if vendor else set())
    seen.discard(None)
    if not seen:
        return ""
    return "debug_assertions " + ("开" if seen == {True} else "关" if seen == {False} else "有开有关")


def summary(exports, vendor=None, partial=False, said=None):
    """The Markdown of the exports and the sheets: (its lines, what does not pass).

    `failed` holds one sentence per export or sheet that does not pass and per
    comparison that does not agree, and is empty when everything passes.
    """
    said = said or {}
    out, failed = ["**PDF 比对汇总**", ""], []
    if partial:
        out += ["这次只跑了一部分，不是完整的验收。", ""]
    if not exports and vendor is None:
        return [*out, "结论：什么也没有跑。"], ["什么也没有跑"]

    built = "，".join(part for part in (f"{said['build']}（cargo 的原话）" if said.get("build") else "", _built(exports, vendor)) if part)
    if built:
        out += [f"构建：{built}。", ""]
    used = []
    if said.get("fonts"):
        used.append(f"两边用的字体（`PID_PLOT_FONTS`）：`{said['fonts']}`。")
    if said.get("reference"):
        used.append(f"没有图框模板时退到的参考目录（`PID_PLOT_CORPUS`）：`{said['reference']}`。")
    if said.get("tools"):
        used.append(f"命令行工具：`{said['tools']}`。")
    if used:
        out += ["".join(used), ""]

    # ── One row per export ───────────────────────────────────────────────────────────────────────
    pairs = []
    if exports:
        out += [
            "| 包 | 目录或 zip | 图纸 | 本程序写出 | `pid_plot --batch` 写出 | 两边都有 / 其中不同 "
            "| 恰好一页且页面对得上 / 写出 | 重跑两次 | 包和还原目录 | 两次导出用时 | 期望值 | 结果 |",
            "|---|---|---:|---:|---|---|---|---|---|---|---|---|",
        ]
        for run in exports:
            out.append(_row(run))
            if run.reasons:
                failed.append(f"{_label(run)}：" + "；".join(run.reasons))
        notes = (
            "期望值的两个数：图纸数、写出的 PDF 数。"
            "「`pid_plot --batch` 写出」只数包里的图纸：它对整个还原目录出图，"
            "不在包的图纸清单里的 `.pid`（参考数据里的图框模板、组件）也会出，这些另计，不拿来比。"
            "两边都写出的逐字节比。"
            f"页数和页面大小是测试里用 `lopdf` 解析每份 PDF 量的，和清单的 `page_mm` 相差不超过 {PAGE_WITHIN_MM} 毫米算对得上。"
            f"「重跑两次」比两个导出目录里的全部文件，含 `{INDEX}`。"
            "「包和还原目录」比导出前后的文件数和校验和。"
        )
        for run in exports:
            if run.limit is not None:
                notes += f"{_label(run)}的一次导出不能超过 {run.limit:.0f} 秒。"
        timed = [run.seconds for run in exports if run.seconds is not None]
        if timed:
            notes += f"连同打开、导两遍、解析、算校验和以及 cargo 自己花的时间，{len(timed)} 次共用 {_span(sum(timed))}"
            plotted = [run.batch.seconds for run in exports if run.batch and run.batch.seconds is not None]
            if plotted:
                notes += f"；`pid_plot --batch` {len(plotted)} 次共用 {_span(sum(plotted))}"
            notes += "。"
        out += ["", notes, ""]
        if any(run.package == UNACCEPTED for run in exports):
            out += [
                "SQL Server 包没有验收过：这一行只说明它导出了多少、两次是否相同、和命令行工具是否一致，"
                "图面没有和任何参考比过。",
                "",
            ]

        # ── From the folder and from the zip ───────────────────────────────────────────────────────
        by = {(run.package, run.form): run for run in exports}
        pairs = [(run, by[run.package, "zip"]) for run in exports if run.form == "folder" and (run.package, "zip") in by]
        if pairs:
            out += [
                f"同一个包从目录打开和从 zip 打开（PDF 的名单和每份的字节都要相同；`{INDEX}` 里记着包的路径，不比）：",
                "",
            ]
            for run, other in pairs:
                line, same = _forms(run, other)
                out.append(line)
                if not same:
                    failed.append(f"目录和 zip 的比较：{_package(run.package)}")
            out.append("")

    # ── One row per sheet of the vendor ───────────────────────────────────────────────────────────
    if vendor is not None:
        out += [
            "厂商的图：本程序用厂商文件夹里的 `.pid` 出图，`pid_plot_compare` 拿它和厂商的 PDF 比；"
            "每个读数的右边是 `pid-plot` 自己验收时留的记录。"
            f"覆盖率最多比记录低 {COVERAGE_WITHIN}，多画的长度最多比记录高 {LENGTH_WITHIN_MM:g} 毫米，"
            "文字对上的条数不能少。",
            "",
        ]
        if vendor.sheets:
            out += [
                "| 图 | 覆盖率 | 记录 | 多画的长度（毫米） | 记录 | 文字对上 / 参考里的 | 记录 | 结果 |",
                "|---|---:|---:|---:|---:|---:|---:|---|",
            ]
            for sheet in vendor.sheets:
                out.append(_sheet_row(sheet))
                if sheet.reasons:
                    failed.append(f"厂商的图 {_name(sheet.name)}：" + "；".join(sheet.reasons))
            out.append("")
        failed += [f"厂商的图：{reason}" for reason in vendor.reasons]
        counted = f"共 {len(vendor.sheets)} 张，期望 {vendor.expect} 张"
        if vendor.seconds is not None:
            counted += f"；出图连同 cargo 自己花的时间用了 {_span(vendor.seconds)}"
        if vendor.compare_seconds is not None:
            counted += f"；`pid_plot_compare` {len(vendor.compares)} 次共用 {_span(vendor.compare_seconds)}"
        out += [counted + "。", ""]

    # ── The drawings that differ, and the drawings without a PDF ──────────────────────────────────
    if exports:
        out += ["和 `pid_plot --batch` 不同的图：", ""]
        differing = [
            f"- {_label(run)} {_name(found.name)}：本程序 {found.ours:,} 字节，`pid_plot --batch` {found.theirs:,} 字节，"
            f"第 {found.at:,} 个字节起不同。"
            for run in exports
            for found in run.differing
        ]
        out += differing or ["- 没有。"]
        out += ["", "没有写出 PDF 的图和清单里写的原因：", ""]
        unplotted = [
            f"- {_label(run)} {_name(name)}：{_reason(why) if why.strip() else '（没有写原因）'}"
            for run in exports
            for name, why in run.unplotted
        ]
        out += unplotted or ["- 没有。"]
        out.append("")

    if failed:
        out.append(f"结论：{len(failed)} 处不过。")
        out += [f"- {what}" for what in failed]
    else:
        passed = [f"{len(exports)} 次整包导出"] if exports else []
        if vendor is not None:
            passed.append(f"{len(vendor.sheets)} 张厂商的图")
        agreed = f"，{len(pairs)} 项目录和 zip 的比较相同" if pairs else ""
        out.append(f"结论：{'、'.join(passed)}全部通过{agreed}。")
    return out, failed


def main(argv=None):
    parser = argparse.ArgumentParser(
        prog="pdf_compare.py",
        description="Sum up the PDF exports that pdf-compare.ps1 ran on the sample backups.",
    )
    parser.add_argument("runs", help="the runs.json that pdf-compare.ps1 wrote")
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
        exports, vendor, partial, said = load_runs(args.runs)
    except ManifestError as error:
        print(f"pdf_compare: {error}", file=sys.stderr)
        return 2
    for run in exports:
        read_export(run)
    if vendor is not None:
        read_vendor(vendor)
    lines, failed = summary(exports, vendor, partial, said)
    text = HOME.sub("%USERPROFILE%", "\n".join(lines) + "\n")
    if args.out:
        try:
            Path(args.out).write_text(text, encoding="utf-8", newline="\n")
        except OSError as error:
            print(f"pdf_compare: {args.out}: 写不了（{error.strerror or error}）", file=sys.stderr)
            return 2
    sys.stdout.write(text)
    return 1 if failed else 0


if __name__ == "__main__":
    sys.exit(main())
