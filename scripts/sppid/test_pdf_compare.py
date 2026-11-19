"""Tests of pdf_compare.py: the verdicts on what pdf-compare.ps1 leaves behind."""

import json
from pathlib import Path
import sys

import pytest

sys.path.insert(0, str(Path(__file__).resolve().parent))

from pdf_compare import main  # noqa: E402
from walk_summary import HOME  # noqa: E402

# What the test says a folder held: so many files, so many bytes, one checksum.
SUMS = {"files": 16, "bytes": 1000, "sha256": "ab" * 32}


def pdf(text):
    """Bytes that stand for the PDF of a drawing."""
    return b"%PDF-1.7\n" + text.encode("utf-8") + b"\n%%EOF\n"


def put(path, value):
    """Write a JSON value, its folder made first."""
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(json.dumps(value, ensure_ascii=False, indent=2) + "\n", encoding="utf-8")


def change(path, how):
    """Rewrite a JSON file as `how` leaves its value."""
    value = json.loads(Path(path).read_text(encoding="utf-8"))
    how(value)
    put(Path(path), value)


def export(tmp_path, package="small", form="folder", drawings=("A", "B"), *, expect=None, limit=None, beyond=1):
    """What one export and one `pid_plot --batch` leave behind, as the entry of runs.json for them.

    A drawing `A` lies at `u1/A.pid` in the restored backup, both sides write the same bytes
    for it, and `pid_plot --batch` plots `beyond` border templates on top.
    """
    label = f"{package}-{form}"
    folder, batch = tmp_path / label, tmp_path / f"{label}-batch"
    source, restored = tmp_path / "samples" / label, folder / "restores" / "r1"
    index, measured, listed = [], [], []
    for name in drawings:
        data = pdf(name)
        for into in (folder / "run-1", folder / "run-2", batch / "u1"):
            into.mkdir(parents=True, exist_ok=True)
            (into / f"{name}.pdf").write_bytes(data)
        page = {"name": name, "pid": f"u1/{name}.pid", "pdf": f"{name}.pdf", "error": None}
        index.append({**page, "bytes": len(data), "page_mm": [594.0, 420.0]})
        measured.append(
            {
                **page,
                "page_mm": [594.0, 420.0],
                "bytes": len(data),
                "starts_as_pdf": True,
                "pages": 1,
                "media_mm": [594.0000047, 419.9999881],
                "unreadable": None,
            }
        )
        listed.append({"drawing": f"u1\\{name}.pid", "pdf": f"u1\\{name}.pdf", "error": None})
    for at in range(beyond):
        (batch / "Ref").mkdir(parents=True, exist_ok=True)
        (batch / "Ref" / f"T{at}.pdf").write_bytes(pdf(f"template {at}"))
        listed.append({"drawing": f"Ref\\T{at}.pid", "pdf": f"Ref\\T{at}.pdf", "error": None})
    for run in ("run-1", "run-2"):
        put(
            folder / run / "_PdfIndex.json",
            {"package": str(source), "drawings": len(index), "written": len(index), "pages": index},
        )
    put(
        folder / "report.json",
        {
            "source": str(source),
            "package": str(source),
            "restored": str(restored),
            "debug_assertions": True,
            "exports": [{"seconds": 1.2}, {"seconds": 1.1}],
            "pages": measured,
            "kept": [
                {"what": "source", "before": dict(SUMS), "after": dict(SUMS), "changed": []},
                {
                    "what": "restored",
                    "before": {**SUMS, "files": 686},
                    "after": {**SUMS, "files": 686},
                    "changed": [],
                },
            ],
            "unmet": [],
        },
    )
    put(batch / "pid_plot-batch.json", {"directory": str(restored), "out_directory": str(batch), "pages": listed})
    return {
        "package": package,
        "form": form,
        "source": str(source),
        "expect": expect or f"{len(drawings)},{len(drawings)}",
        "folder": str(folder),
        "exit_code": 0,
        "seconds": 9.5,
        "limit_seconds": limit,
        "note": "",
        "batch": {"folder": str(batch), "exit_code": 0, "seconds": 2.5},
    }


def report_of(entry):
    return Path(entry["folder"]) / "report.json"


def batch_of(entry):
    return Path(entry["batch"]["folder"])


def measure(entry, name, **what):
    """Have the test's report say `what` of the PDF of one drawing."""

    def how(report):
        for page in report["pages"]:
            if page["name"] == name:
                page.update(what)

    change(report_of(entry), how)


def unplot(entry, name, why="no template"):
    """Leave one drawing of an export without its PDF: an index entry with a reason."""

    def how(index):
        for page in index["pages"]:
            if page["name"] == name:
                page.update(pdf=None, error=why)

    for run in ("run-1", "run-2"):
        (Path(entry["folder"]) / run / f"{name}.pdf").unlink()
        change(Path(entry["folder"]) / run / "_PdfIndex.json", how)
    change(
        report_of(entry),
        lambda report: report.update(pages=[page for page in report["pages"] if page["name"] != name]),
    )


def numbers(coverage=1.0, extra=0.0, matched=10, reference=10, fit=True):
    """What `pid_plot_compare` writes of one sheet, and what a record holds."""
    return {
        "coverage": coverage,
        "extra_length_mm": extra,
        "text_matched": matched,
        "text_reference": reference,
        "fit": {"within_threshold": fit},
    }


def vendor(tmp_path, sheets=None, *, expect=None):
    """What plotting and measuring the vendor's sheets leave behind, as the `vendor` of runs.json.

    `sheets` maps the name of a sheet to (its reading, its record); one that is None is left out.
    """
    if sheets is None:
        sheets = {"S 1": (numbers(), numbers()), "S 2": (numbers(0.98, 187.1, 8), numbers(0.98, 187.1, 8))}
    folder, plots, readings = tmp_path / "vendor", tmp_path / "vendor-plots", tmp_path / "vendor-readings"
    listed, compares = [], []
    for name, (reading, record) in sheets.items():
        plots.mkdir(parents=True, exist_ok=True)
        (plots / f"{name}.pdf").write_bytes(pdf(name))
        listed.append({"name": name, "pdf": str(plots / f"{name}.pdf"), "seconds": 0.5})
        compares.append({"name": name, "exit_code": 0})
        if reading is not None:
            put(readings / f"{name}.json", reading)
        if record is not None:
            put(folder / "plot" / "acceptance" / f"{name}.json", record)
    put(plots / "report.json", {"folder": str(folder), "debug_assertions": True, "sheets": listed})
    return {
        "folder": str(folder),
        "plots": str(plots),
        "readings": str(readings),
        "expect": len(sheets) if expect is None else expect,
        "exit_code": 0,
        "seconds": 19.5,
        "compare_seconds": 22.9,
        "note": "",
        "compares": compares,
    }


def summed(tmp_path, capsys, packages, sheets=None, *options, **manifest):
    """Run the tool on a runs.json of these steps: (exit code, what it printed, what it complained of)."""
    runs = tmp_path / "runs.json"
    put(runs, {"partial": False, "packages": packages, "vendor": sheets, **manifest})
    code = main([str(runs), *options])
    said = capsys.readouterr()
    return code, said.out, said.err


def row_of(out, *first):
    """The cells of the table row that starts with these cells."""
    for line in out.splitlines():
        cells = [cell.strip() for cell in line.strip().strip("|").split("|")]
        if cells[: len(first)] == list(first):
            return cells
    raise AssertionError(f"no row starts with {first}:\n{out}")


# ── Everything as it should be ───────────────────────────────────────────────────────────────────


def test_exports_and_sheets_that_are_as_they_should_be_pass(tmp_path, capsys):
    packages = [export(tmp_path, "plant", limit=600), export(tmp_path), export(tmp_path, form="zip")]
    code, out, err = summed(tmp_path, capsys, packages, vendor(tmp_path))
    assert (code, err) == (0, "")
    assert row_of(out, "全厂包", "目录") == [
        "全厂包",
        "目录",
        "2",
        "2",
        "2（另 1 份不在图纸清单里）",
        "2 / 0",
        "2 / 2",
        "相同（3 个文件）",
        "不变（包 16 个文件；还原目录 686 个文件）",
        "1.2 s / 1.1 s",
        "`2,2`",
        "过",
    ]
    assert row_of(out, "小包", "zip")[-1] == "过"
    assert "- 小包：相同。2 份 PDF，名单相同，逐字节相同。" in out
    assert "全厂包（目录）的一次导出不能超过 600 秒。" in out
    assert (
        "连同打开、导两遍、解析、算校验和以及 cargo 自己花的时间，3 次共用 28 秒；`pid_plot --batch` 3 次共用 8 秒。"
    ) in out
    assert row_of(out, "`S 1`") == ["`S 1`", "1.000000", "1.000000", "0.000", "0.000", "10 / 10", "10 / 10", "过"]
    assert row_of(out, "`S 2`")[1:] == ["0.980000", "0.980000", "187.100", "187.100", "8 / 10", "8 / 10", "过"]
    assert (
        "共 2 张，期望 2 张；出图连同 cargo 自己花的时间用了 20 秒；`pid_plot_compare` 2 次共用 23 秒。"
    ) in out
    assert out.rstrip().endswith("结论：3 次整包导出、2 张厂商的图全部通过，1 项目录和 zip 的比较相同。")


def test_the_build_the_fonts_the_reference_folder_and_the_tools_are_named(tmp_path, capsys):
    said = {
        "build": "Finished `test` profile [optimized] target(s) in 0.35s",
        "fonts": "D:/f",
        "reference": "D:/r",
        "tools": "D:/t",
    }
    code, out, _ = summed(tmp_path, capsys, [export(tmp_path)], **said)
    assert code == 0
    assert (
        "构建：Finished `test` profile [optimized] target(s) in 0.35s（cargo 的原话），debug_assertions 开。" in out
    )
    assert (
        "两边用的字体（`PID_PLOT_FONTS`）：`D:/f`。"
        "没有图框模板时退到的参考目录（`PID_PLOT_CORPUS`）：`D:/r`。"
        "命令行工具：`D:/t`。"
    ) in out


def test_the_sql_server_backup_is_said_not_to_be_accepted(tmp_path, capsys):
    code, out, _ = summed(tmp_path, capsys, [export(tmp_path, "sqlserver")])
    assert code == 0
    assert row_of(out, "SQL Server 包（没有验收过）", "目录")[-1] == "过"
    assert "SQL Server 包没有验收过" in out


def test_a_part_of_the_steps_is_said_to_be_a_part(tmp_path, capsys):
    code, out, _ = summed(tmp_path, capsys, [export(tmp_path)], partial=True)
    assert code == 0
    assert "这次只跑了一部分，不是完整的验收。" in out


def test_out_also_writes_the_markdown(tmp_path, capsys):
    target = tmp_path / "table.md"
    code, out, _ = summed(tmp_path, capsys, [export(tmp_path)], None, "--out", str(target))
    assert code == 0
    assert target.read_text(encoding="utf-8") == out


def test_a_home_folder_is_not_spelled_out(tmp_path, capsys):
    code, out, _ = summed(tmp_path, capsys, [export(tmp_path)], fonts=r"C:\Users\someone\fonts")
    assert code == 0
    assert "`%USERPROFILE%\\fonts`" in out
    assert "someone" not in out


def test_the_summary_says_whether_debug_assertions_were_on(tmp_path, capsys):
    off = export(tmp_path)
    change(report_of(off), lambda report: report.update(debug_assertions=False))
    code, out, _ = summed(tmp_path, capsys, [off])
    assert code == 0
    assert "构建：debug_assertions 关。" in out

    code, out, _ = summed(tmp_path, capsys, [off, export(tmp_path, "plant")])
    assert code == 0
    assert "构建：debug_assertions 有开有关。" in out


# ── An export held to its expectation and to its own index ──────────────────────────────────────


def test_an_export_is_held_to_its_expectation(tmp_path, capsys):
    code, out, _ = summed(tmp_path, capsys, [export(tmp_path, expect="3,2")])
    assert code == 1
    assert row_of(out, "小包", "目录")[-2:] == ["`3,2`", "不过：图纸 2 张，期望 3 张"]
    assert out.rstrip().endswith("结论：1 处不过。\n- 小包（目录）：图纸 2 张，期望 3 张")

    code, out, _ = summed(tmp_path, capsys, [export(tmp_path, "plant", expect="2,1")])
    assert code == 1
    assert row_of(out, "全厂包", "目录")[-1] == "不过：写出 2 份，期望 1 份"


def test_the_folder_must_hold_the_pdfs_the_index_names_and_nothing_else(tmp_path, capsys):
    entry = export(tmp_path)
    for run in ("run-1", "run-2"):
        (Path(entry["folder"]) / run / "stray.txt").write_text("x", encoding="utf-8")
    code, out, _ = summed(tmp_path, capsys, [entry])
    assert code == 1
    assert row_of(out, "小包", "目录")[-1] == "不过：目录里多出的文件：`stray.txt`"

    entry = export(tmp_path, "plant")
    for run in ("run-1", "run-2"):
        (Path(entry["folder"]) / run / "B.pdf").unlink()
    code, out, _ = summed(tmp_path, capsys, [entry])
    assert code == 1
    assert "清单里有而目录里没有的 PDF：`B.pdf`" in row_of(out, "全厂包", "目录")[-1]


def test_an_entry_of_the_index_has_either_a_pdf_or_a_reason(tmp_path, capsys):
    entry = export(tmp_path, expect="2,1")
    unplot(entry, "B", why=None)
    (batch_of(entry) / "u1" / "B.pdf").unlink()
    change(
        batch_of(entry) / "pid_plot-batch.json",
        lambda summary: summary["pages"][1].update(pdf=None, error="no template"),
    )
    code, out, _ = summed(tmp_path, capsys, [entry])
    assert code == 1
    assert row_of(out, "小包", "目录")[-1] == "不过：清单里 1 条不是「要么有 PDF，要么有原因」（`B`）"
    assert "- 小包（目录） `B`：（没有写原因）" in out


def test_two_drawings_written_to_one_pdf_fail(tmp_path, capsys):
    entry = export(tmp_path)
    for run in ("run-1", "run-2"):
        change(Path(entry["folder"]) / run / "_PdfIndex.json", lambda index: index["pages"][1].update(pdf="A.pdf"))
    code, out, _ = summed(tmp_path, capsys, [entry])
    assert code == 1
    assert row_of(out, "小包", "目录")[-1].startswith("不过：清单里不止一张图写到同一个 PDF：`A.pdf`；")


def test_an_export_without_its_index_fails(tmp_path, capsys):
    entry = export(tmp_path)
    (Path(entry["folder"]) / "run-1" / "_PdfIndex.json").unlink()
    code, out, _ = summed(tmp_path, capsys, [entry])
    assert code == 1
    assert row_of(out, "小包", "目录")[-1] == "不过：第一遍的清单 run-1/_PdfIndex.json：没有这个文件"


def test_a_failed_test_fails_its_export_whatever_the_files_show(tmp_path, capsys):
    entry = export(tmp_path)
    entry["exit_code"] = 101
    change(report_of(entry), lambda report: report.update(unmet=["the command line said nothing"]))
    code, out, _ = summed(tmp_path, capsys, [entry])
    assert code == 1
    assert (
        row_of(out, "小包", "目录")[-1]
        == "不过：导出的测试失败（退出码 101）；测试说：the command line said nothing"
    )


def test_a_step_that_was_not_run_fails(tmp_path, capsys):
    entry = export(tmp_path)
    entry.update(exit_code=None, note="no backup at D:/x")
    sheets = vendor(tmp_path)
    sheets.update(exit_code=None, note="no pid_plot_compare.exe")
    code, out, _ = summed(tmp_path, capsys, [entry], sheets)
    assert code == 1
    assert row_of(out, "小包", "目录") == [
        "小包",
        "目录",
        *["—"] * 8,
        "`2,2`",
        "不过：没有导出（no backup at D:/x）",
    ]
    assert "- 厂商的图：没有出图（no pid_plot_compare.exe）" in out


def test_the_report_of_another_backup_fails(tmp_path, capsys):
    entry = export(tmp_path)
    change(report_of(entry), lambda report: report.update(source="D:/elsewhere"))
    code, out, _ = summed(tmp_path, capsys, [entry])
    assert code == 1
    # The folder of the test may lie under a home folder, which the summary does not spell out.
    source = HOME.sub("%USERPROFILE%", entry["source"])
    assert row_of(out, "小包", "目录")[-1] == f"不过：报告的来源是 `D:/elsewhere`，这次导的是 `{source}`"


def test_an_export_without_its_report_fails(tmp_path, capsys):
    entry = export(tmp_path)
    report_of(entry).unlink()
    code, out, _ = summed(tmp_path, capsys, [entry])
    assert code == 1
    assert row_of(out, "小包", "目录")[-1] == "不过：测试的报告 report.json：没有这个文件"


def test_nothing_run_at_all_fails(tmp_path, capsys):
    code, out, _ = summed(tmp_path, capsys, [])
    assert code == 1
    assert "结论：什么也没有跑。" in out


# ── Against `pid_plot --batch` ─────────────────────────────────────────────────────────────────


def test_a_pdf_that_differs_from_the_one_of_pid_plot_is_named(tmp_path, capsys):
    entry = export(tmp_path)
    theirs = batch_of(entry) / "u1" / "B.pdf"
    theirs.write_bytes(theirs.read_bytes().replace(b"B", b"b") + b"more")
    code, out, _ = summed(tmp_path, capsys, [entry])
    assert code == 1
    row = row_of(out, "小包", "目录")
    assert row[5] == "2 / 1"
    assert row[-1] == "不过：和 pid_plot --batch 不同的图 1 张（`B`）"
    assert "- 小包（目录） `B`：本程序 17 字节，`pid_plot --batch` 21 字节，第 10 个字节起不同。" in out


def test_what_pid_plot_plots_beyond_the_drawings_of_the_backup_is_counted_apart(tmp_path, capsys):
    code, out, _ = summed(tmp_path, capsys, [export(tmp_path, beyond=3)])
    assert code == 0
    assert row_of(out, "小包", "目录")[4] == "2（另 3 份不在图纸清单里）"

    code, out, _ = summed(tmp_path, capsys, [export(tmp_path, "plant", beyond=0)])
    assert code == 0
    assert row_of(out, "全厂包", "目录")[4] == "2"


def test_a_drawing_only_pid_plot_wrote_fails_the_export(tmp_path, capsys):
    entry = export(tmp_path, expect="2,1")
    unplot(entry, "B")
    code, out, _ = summed(tmp_path, capsys, [entry])
    assert code == 1
    row = row_of(out, "小包", "目录")
    assert row[2:6] == ["2", "1", "2（另 1 份不在图纸清单里）", "1 / 0"]
    assert row[-1] == "不过：pid_plot --batch 写出而本程序没有写出的图 1 张（`B`）"
    assert "- 小包（目录） `B`：no template" in out


def test_a_drawing_neither_side_wrote_passes_when_the_expectation_says_so(tmp_path, capsys):
    entry = export(tmp_path, expect="2,1")
    unplot(entry, "B")
    (batch_of(entry) / "u1" / "B.pdf").unlink()
    change(
        batch_of(entry) / "pid_plot-batch.json",
        lambda summary: summary["pages"][1].update(pdf=None, error="no template"),
    )
    code, out, _ = summed(tmp_path, capsys, [entry])
    assert code == 0
    assert row_of(out, "小包", "目录")[2:6] == ["2", "1", "1（另 1 份不在图纸清单里）", "1 / 0"]


def test_a_pdf_pid_plot_does_not_list_cannot_be_compared_and_fails(tmp_path, capsys):
    entry = export(tmp_path)
    change(batch_of(entry) / "pid_plot-batch.json", lambda summary: summary["pages"].pop(0))
    code, out, _ = summed(tmp_path, capsys, [entry])
    assert code == 1
    assert row_of(out, "小包", "目录")[-1] == "不过：pid_plot --batch 没有列出、没法比的图 1 张（`A`）"


def test_a_drawing_pid_plot_failed_on_is_not_counted_as_written_by_it(tmp_path, capsys):
    entry = export(tmp_path)
    (batch_of(entry) / "u1" / "B.pdf").unlink()
    change(batch_of(entry) / "pid_plot-batch.json", lambda summary: summary["pages"][1].update(error="no template"))
    code, out, _ = summed(tmp_path, capsys, [entry])
    assert code == 0
    assert row_of(out, "小包", "目录")[3:6] == ["2", "1（另 1 份不在图纸清单里）", "1 / 0"]


def test_a_pdf_pid_plot_lists_and_did_not_leave_cannot_be_compared_and_fails(tmp_path, capsys):
    entry = export(tmp_path)
    (batch_of(entry) / "u1" / "B.pdf").unlink()
    code, out, _ = summed(tmp_path, capsys, [entry])
    assert code == 1
    row = row_of(out, "小包", "目录")
    assert row[4:6] == ["2（另 1 份不在图纸清单里）", "2 / 0"]
    assert row[-1] == "不过：两边的 PDF 读不了、没法比的图 1 张（`B`）"


def test_drawings_are_paired_whatever_the_case_and_the_slashes(tmp_path, capsys):
    entry = export(tmp_path)
    change(batch_of(entry) / "pid_plot-batch.json", lambda summary: summary["pages"][0].update(drawing="U1/a.PID"))
    code, out, _ = summed(tmp_path, capsys, [entry])
    assert code == 0
    assert row_of(out, "小包", "目录")[4:6] == ["2（另 1 份不在图纸清单里）", "2 / 0"]


def test_a_long_list_of_drawings_is_cut_short(tmp_path, capsys):
    names = tuple("ABCDEFGHI")
    entry = export(tmp_path, drawings=names)
    for name in names:
        (batch_of(entry) / "u1" / f"{name}.pdf").write_bytes(pdf(name.lower()))
    code, out, _ = summed(tmp_path, capsys, [entry])
    assert code == 1
    shown = "、".join(f"`{name}`" for name in names[:8])
    assert row_of(out, "小包", "目录")[-1] == f"不过：和 pid_plot --batch 不同的图 9 张（{shown} 等 9 张）"
    assert "- 小包（目录） `I`：本程序 17 字节，`pid_plot --batch` 17 字节，第 10 个字节起不同。" in out


def test_a_batch_without_its_summary_fails(tmp_path, capsys):
    entry = export(tmp_path)
    (batch_of(entry) / "pid_plot-batch.json").unlink()
    code, out, _ = summed(tmp_path, capsys, [entry])
    assert code == 1
    row = row_of(out, "小包", "目录")
    assert row[4:6] == ["—", "—"]
    assert row[-1] == "不过：pid_plot --batch 的汇总 pid_plot-batch.json：没有这个文件"


def test_pid_plot_must_have_run_on_the_restore_of_this_export(tmp_path, capsys):
    elsewhere = export(tmp_path)
    change(
        batch_of(elsewhere) / "pid_plot-batch.json",
        lambda summary: summary.update(directory=str(tmp_path / "elsewhere")),
    )
    failed = export(tmp_path, "plant")
    failed["batch"]["exit_code"] = 3
    without = export(tmp_path, "other")
    without["batch"] = None
    code, out, _ = summed(tmp_path, capsys, [elsewhere, failed, without])
    assert code == 1
    assert row_of(out, "小包", "目录")[-1] == "不过：pid_plot --batch 出的不是这次导出的还原目录"
    assert row_of(out, "全厂包", "目录")[-1] == "不过：pid_plot --batch 失败（退出码 3）"
    assert row_of(out, "other", "目录")[-1] == "不过：没有跑 pid_plot --batch"


# ── One page of the stated size, the same twice, nothing touched, in time ────────────────────────


def test_every_pdf_must_be_one_page_of_the_size_its_entry_states(tmp_path, capsys):
    entry = export(tmp_path)
    measure(entry, "A", media_mm=[594.005, 419.995])
    code, out, _ = summed(tmp_path, capsys, [entry])
    assert code == 0
    assert row_of(out, "小包", "目录")[6] == "2 / 2"

    measure(entry, "A", media_mm=[594.02, 420.0])
    measure(entry, "B", pages=2)
    code, out, _ = summed(tmp_path, capsys, [entry])
    assert code == 1
    row = row_of(out, "小包", "目录")
    assert row[6] == "0 / 2"
    assert row[-1] == (
        "不过：2 份 PDF 不是恰好一页或页面大小对不上："
        "`A` 页面 594.020 × 420.000 毫米，清单里是 594.000 × 420.000；`B` 有 2 页"
    )


def test_a_pdf_the_test_did_not_measure_fails(tmp_path, capsys):
    entry = export(tmp_path)
    measure(entry, "A", bytes=1)
    change(report_of(entry), lambda report: report["pages"].pop())
    code, out, _ = summed(tmp_path, capsys, [entry])
    assert code == 1
    assert row_of(out, "小包", "目录")[-1] == (
        "不过：2 份 PDF 不是恰好一页或页面大小对不上：`A` 测试量的不是这一份文件；`B` 测试没有量过"
    )


def test_a_pdf_that_cannot_be_parsed_or_does_not_start_as_one_fails(tmp_path, capsys):
    entry = export(tmp_path)
    measure(entry, "A", unreadable="invalid file trailer", pages=0)
    measure(entry, "B", starts_as_pdf=False)
    code, out, _ = summed(tmp_path, capsys, [entry])
    assert code == 1
    row = row_of(out, "小包", "目录")
    assert row[6] == "0 / 2"
    assert row[-1] == (
        "不过：2 份 PDF 不是恰好一页或页面大小对不上：`A` 解析不了（invalid file trailer）；`B` 不以 %PDF- 开头"
    )


def test_a_file_only_one_export_left_fails(tmp_path, capsys):
    entry = export(tmp_path)
    (Path(entry["folder"]) / "run-2" / "left.tmp").write_bytes(b"x")
    code, out, _ = summed(tmp_path, capsys, [entry])
    assert code == 1
    row = row_of(out, "小包", "目录")
    assert row[7] == "1 个文件不同"
    assert row[-1] == "不过：两次导出不同：`left.tmp`"


def test_a_second_export_that_differs_from_the_first_fails(tmp_path, capsys):
    entry = export(tmp_path)
    change(Path(entry["folder"]) / "run-2" / "_PdfIndex.json", lambda index: index.update(package="elsewhere"))
    code, out, _ = summed(tmp_path, capsys, [entry])
    assert code == 1
    row = row_of(out, "小包", "目录")
    assert row[7] == "1 个文件不同"
    assert row[-1] == "不过：两次导出不同：`_PdfIndex.json`"


def test_a_backup_that_did_not_stay_as_it_was_fails(tmp_path, capsys):
    entry = export(tmp_path)

    def touched(report):
        report["kept"][1]["after"] = {**SUMS, "files": 687, "sha256": "cd" * 32}
        report["kept"][1]["changed"] = ["u1/A.pid"]

    change(report_of(entry), touched)
    code, out, _ = summed(tmp_path, capsys, [entry])
    assert code == 1
    row = row_of(out, "小包", "目录")
    assert row[8] == "变了"
    assert row[-1] == "不过：还原目录变了（文件 686 → 687，1 个文件不同）"

    unwatched = export(tmp_path, "plant")
    change(report_of(unwatched), lambda report: report.update(kept=[]))
    code, out, _ = summed(tmp_path, capsys, [unwatched])
    assert code == 1
    assert row_of(out, "全厂包", "目录")[-1] == "不过：报告里没有包在导出前后的校验和"


def test_a_folder_without_its_checksums_or_with_changed_files_fails(tmp_path, capsys):
    unsummed = export(tmp_path)
    change(report_of(unsummed), lambda report: report["kept"][1].update(after=None))
    listed = export(tmp_path, "plant")
    change(report_of(listed), lambda report: report["kept"][0].update(changed=["u1/A.pid"]))
    code, out, _ = summed(tmp_path, capsys, [unsummed, listed])
    assert code == 1
    row = row_of(out, "小包", "目录")
    assert row[8] == "变了"
    assert row[-1] == "不过：还原目录没有导出前后的校验和"
    assert row_of(out, "全厂包", "目录")[-1] == "不过：包变了（文件 16 → 16，1 个文件不同）"


def test_both_exports_must_be_timed(tmp_path, capsys):
    entry = export(tmp_path)
    change(report_of(entry), lambda report: report.update(exports=[{"seconds": 1.2}]))
    code, out, _ = summed(tmp_path, capsys, [entry])
    assert code == 1
    row = row_of(out, "小包", "目录")
    assert row[9] == "1.2 s"
    assert row[-1] == "不过：报告里有用时的导出是 1 次，应当是 2 次"


def test_an_export_over_its_time_limit_fails(tmp_path, capsys):
    slow = [{"seconds": 599.6}, {"seconds": 601.2}]
    held = export(tmp_path, "plant", limit=600)
    change(report_of(held), lambda report: report.update(exports=slow))
    free = export(tmp_path)
    change(report_of(free), lambda report: report.update(exports=slow))
    code, out, _ = summed(tmp_path, capsys, [held, free])
    assert code == 1
    row = row_of(out, "全厂包", "目录")
    assert row[9] == "599.6 s / 601.2 s"
    assert row[-1] == "不过：导出用了 601 秒，超过 600 秒，要先报告"
    assert row_of(out, "小包", "目录")[-1] == "过"


# ── From the folder and from the zip ────────────────────────────────────────────────────────────


def test_folder_and_zip_must_write_the_same_bytes(tmp_path, capsys):
    folder, zipped = export(tmp_path), export(tmp_path, form="zip")
    other = pdf("B out of the zip")
    for into in (Path(zipped["folder"]) / "run-1", Path(zipped["folder"]) / "run-2", batch_of(zipped) / "u1"):
        (into / "B.pdf").write_bytes(other)
    measure(zipped, "B", bytes=len(other))
    code, out, _ = summed(tmp_path, capsys, [folder, zipped])
    assert code == 1
    assert row_of(out, "小包", "目录")[-1] == "过"
    assert row_of(out, "小包", "zip")[-1] == "过"
    assert "- 小包：不同。字节不同的 `B.pdf`。" in out
    assert out.rstrip().endswith("结论：1 处不过。\n- 目录和 zip 的比较：小包")


def test_folder_and_zip_must_name_the_same_pdfs(tmp_path, capsys):
    packages = [export(tmp_path), export(tmp_path, form="zip", drawings=("A", "C"))]
    code, out, _ = summed(tmp_path, capsys, packages)
    assert code == 1
    assert "- 小包：不同。只在目录里的 `B.pdf`；只在 zip 里的 `C.pdf`。" in out


def test_folder_and_zip_cannot_be_compared_without_both_exports(tmp_path, capsys):
    folder, zipped = export(tmp_path), export(tmp_path, form="zip")
    report_of(zipped).unlink()
    code, out, _ = summed(tmp_path, capsys, [folder, zipped])
    assert code == 1
    assert "- 小包：没法比，zip 那一次没有读到导出的 PDF。" in out
    assert "结论：2 处不过。" in out
    assert out.rstrip().endswith("- 目录和 zip 的比较：小包")


# ── The vendor's sheets ─────────────────────────────────────────────────────────────────────────


def test_a_reading_within_the_tolerances_of_its_record_passes(tmp_path, capsys):
    sheets = vendor(tmp_path, {"S": (numbers(0.9795, 187.6, 9), numbers(0.98, 187.1, 8))})
    code, out, _ = summed(tmp_path, capsys, [], sheets)
    assert code == 0
    assert row_of(out, "`S`") == ["`S`", "0.979500", "0.980000", "187.600", "187.100", "9 / 10", "8 / 10", "过"]
    assert out.rstrip().endswith("结论：1 张厂商的图全部通过。")


@pytest.mark.parametrize(
    ("reading", "why"),
    [
        (numbers(0.978, 187.1, 8), "覆盖率 0.978000，低于记录的 0.980000"),
        (numbers(0.98, 188.6, 8), "多画 188.600 毫米，高于记录的 187.100 毫米"),
        (numbers(0.98, 187.1, 7), "文字对上 7 条，少于记录的 8 条"),
        (numbers(0.98, 187.1, 8, fit=False), "对位不在阈值内，记录里是在的"),
    ],
)
def test_a_reading_worse_than_its_record_fails(tmp_path, capsys, reading, why):
    sheets = vendor(tmp_path, {"S": (reading, numbers(0.98, 187.1, 8))})
    code, out, _ = summed(tmp_path, capsys, [], sheets)
    assert code == 1
    assert row_of(out, "`S`")[-1] == f"不过：{why}"
    assert out.rstrip().endswith(f"结论：1 处不过。\n- 厂商的图 `S`：{why}")


def test_a_sheet_without_reading_or_record_fails(tmp_path, capsys):
    sheets = vendor(tmp_path, {"S 1": (None, numbers()), "S 2": (numbers(), None), "S 3": (numbers(), numbers())})
    sheets["compares"][2]["exit_code"] = 1
    code, out, _ = summed(tmp_path, capsys, [], sheets)
    assert code == 1
    assert row_of(out, "`S 1`") == [
        "`S 1`",
        "—",
        "1.000000",
        "—",
        "0.000",
        "—",
        "10 / 10",
        "不过：读数：没有这个文件",
    ]
    assert row_of(out, "`S 2`")[-1] == "不过：记录：没有这个文件"
    assert row_of(out, "`S 3`")[-1] == "不过：pid_plot_compare 失败（退出码 1）"


def test_a_sheet_that_was_not_plotted_fails(tmp_path, capsys):
    sheets = vendor(tmp_path, {"S": (None, numbers())})
    change(
        Path(sheets["plots"]) / "report.json",
        lambda report: report.update(sheets=[{"name": "S", "error": "no border template", "seconds": 0.1}]),
    )
    code, out, _ = summed(tmp_path, capsys, [], sheets)
    assert code == 1
    assert row_of(out, "`S`")[-1] == "不过：没有出图：no border template"


def test_a_sheet_that_was_not_compared_fails(tmp_path, capsys):
    sheets = vendor(tmp_path)
    sheets["compares"].pop()
    code, out, _ = summed(tmp_path, capsys, [], sheets)
    assert code == 1
    assert row_of(out, "`S 1`")[-1] == "过"
    assert row_of(out, "`S 2`")[-1] == "不过：没有跑 pid_plot_compare"


def test_a_failed_plotting_test_or_a_missing_report_fails_the_sheets(tmp_path, capsys):
    sheets = vendor(tmp_path)
    sheets["exit_code"] = 101
    code, out, _ = summed(tmp_path, capsys, [], sheets)
    assert code == 1
    assert row_of(out, "`S 1`")[-1] == "过"
    assert out.rstrip().endswith("结论：1 处不过。\n- 厂商的图：出图的测试失败（退出码 101）")

    sheets["exit_code"] = 0
    (Path(sheets["plots"]) / "report.json").unlink()
    code, out, _ = summed(tmp_path, capsys, [], sheets)
    assert code == 1
    assert out.rstrip().endswith("结论：1 处不过。\n- 厂商的图：出图的报告 report.json：没有这个文件")


def test_the_number_of_sheets_is_held_to_the_expectation(tmp_path, capsys):
    code, out, _ = summed(tmp_path, capsys, [], vendor(tmp_path, expect=11))
    assert code == 1
    assert row_of(out, "`S 1`")[-1] == "过"
    assert out.rstrip().endswith("结论：1 处不过。\n- 厂商的图：出了 2 张，期望 11 张")


# ── A runs.json that cannot be used ─────────────────────────────────────────────────────────────


@pytest.mark.parametrize(
    "text",
    [
        "",
        "[]",
        '{"packages": 1}',
        '{"packages": [1]}',
        '{"packages": [{"package": "small", "form": "folder", "source": "s", "expect": "7,7"}]}',
        '{"packages": [{"package": "small", "form": "tar", "source": "s", "expect": "7,7", "folder": "f"}]}',
        '{"packages": [{"package": "small", "form": "zip", "source": "s", "expect": "7,1,1", "folder": "f"}]}',
        '{"packages": [{"package": "small", "form": "zip", "source": "s", "expect": "7,7", "folder": "f",'
        ' "exit_code": "0"}]}',
        '{"packages": [{"package": "small", "form": "zip", "source": "s", "expect": "7,7", "folder": "f",'
        ' "batch": {}}]}',
        '{"packages": [], "vendor": {"folder": "v", "plots": "p", "readings": "r"}}',
        '{"packages": [], "vendor": {"folder": "v", "plots": "p", "expect": 11}}',
        '{"packages": [], "vendor": {"folder": "v", "plots": "p", "readings": "r", "expect": 11,'
        ' "compares": [{}]}}',
    ],
)
def test_a_runs_json_that_cannot_be_used_is_exit_code_2(tmp_path, capsys, text):
    runs = tmp_path / "runs.json"
    runs.write_text(text, encoding="utf-8")
    assert main([str(runs)]) == 2
    said = capsys.readouterr()
    assert said.out == ""
    assert said.err.startswith("pdf_compare: ")


def test_the_same_export_named_twice_is_exit_code_2(tmp_path, capsys):
    runs = tmp_path / "runs.json"
    put(runs, {"packages": [export(tmp_path), export(tmp_path)]})
    assert main([str(runs)]) == 2
    assert "和第 1 次是同一个（small / folder）" in capsys.readouterr().err


def test_no_runs_json_or_no_place_for_out_is_exit_code_2(tmp_path, capsys):
    assert main([str(tmp_path / "absent.json")]) == 2
    assert main([]) == 2
    capsys.readouterr()
    code, out, err = summed(tmp_path, capsys, [export(tmp_path)], None, "--out", str(tmp_path / "no" / "t.md"))
    assert code == 2
    assert err.startswith("pdf_compare: ")
