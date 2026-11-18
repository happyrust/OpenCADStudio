"""Tests for scripts/sppid/compare_walks.py, run on made-up reports that keep
the layout of the walk's real report.txt."""

from pathlib import Path
import sys

import pytest

sys.path.insert(0, str(Path(__file__).resolve().parent))
import compare_walks  # noqa: E402


def report(rows, unread=(), unopened=(), count=None, total_ms=None, skip_import=False):
    """The text of a walk's report: the sections the tool reads, in the real
    wording, among look-alike lines of other sections that it must pass over.

    rows: (name, entities, decoded, not drawn, sheet layers, ms) per drawing
    that imported; unread, unopened: (name, reason) per listed drawing.
    """
    imported = [row[0] for row in rows]
    names = imported + [name for name, _ in unopened if name not in imported]
    count = len(names) if count is None else count
    total_ms = sum(row[5] for row in rows) if total_ms is None else total_ms
    lines = [
        "source: D:\\samples\\made-up",
        "",
        f"loaded in 12.3 ms: 3 backup files, {count} drawings, 0 reference files, 0 warnings; "
        "restored to D:\\made-up\\cache (source read in place)",
        "== the backup (root node) ==",
        "  名称 = made-up",
        f"== drawings ({count}; 0 without a file) ==",
        "  drawing-table columns filled, of all drawings:",
        f"    NAME: {count}/{count}",
        "  items per drawing (name | items | types | attributes | read):",
    ]
    # Three numbers and a time: an import row but one field short.
    lines += [f"    {name} | 37 | 8 | 27 | 18.9 ms" for name in imported]
    lines += [
        "  152 items, 70 attributes (116 items carry none); all read in 85.7 ms, slowest X (18.9 ms)",
        '== open "X": the open ran to its end in 14.7 ms; tabs 1 -> 2; active tab shows X with 330 entities ==',
    ]
    if skip_import:
        lines += [
            "== import of every drawing: skipped (OCS_PID_BACKUP_AUDIT_SKIP_IMPORT) ==",
            "  0 entities drawn, 0 source records not drawn; all imported in 0.0 ms, slowest  (0.0 ms)",
        ]
    else:
        lines.append(
            "== import of every drawing (name | entities | decoded | not drawn | sheet layers | time) =="
        )
        lines += [
            f"    {name} | {drawn} | {decoded} | {missing} | {layers} | {ms:.1f} ms"
            for name, drawn, decoded, missing, layers, ms in rows
        ]
        slowest = max(rows, key=lambda row: row[5]) if rows else ("", 0, 0, 0, 0, 0.0)
        lines += [
            f"  {sum(row[1] for row in rows)} entities drawn, {sum(row[3] for row in rows)} source records "
            f"not drawn; all imported in {total_ms:.1f} ms, slowest {slowest[0]} ({slowest[5]:.1f} ms)",
            "  reference data in the import: 0 symbol bodies from the library, 92 from the drawings' own "
            "caches; 4 drawing(s) with their border template (309 entities); symbol library: none found",
        ]
    lines += [
        "== check of the drawings (the button in the backup's details; 753.2 ms) ==",
        f"  检查的图纸 = {count} 张",
    ]
    # The check names the same drawings with the same reasons, indented deeper.
    lines += [f"    {name}: {reason}" for name, reason in unread]
    lines += [
        "== credentials: 0 value(s) of Manifest.txt looked for in the details of 10 node(s), the report, "
        "the command line, the index and the panel's state: 0 found ==",
        "== 0 node(s) without details ==",
        f"== {len(unread)} drawing(s) whose items did not read (0 of them silently) ==",
    ]
    lines += [f"  {name}: {reason}" for name, reason in unread]
    lines.append(f"== {len(unopened)} drawing(s) that did not import ==")
    lines += [f"  {name}: {reason}" for name, reason in unopened]
    return "\n".join(lines) + "\n"


A = ("A-001", 330, 209, 0, 4, 166.2)
B = ("B 02", 765, 364, 0, 3, 228.2)
C = ("井场 P&ID", 133, 129, 2, 2, 16.4)
NO_NUMBER = "无法读取 001.pid：图纸里没有图号"
NO_GEOMETRY = "001.pid 里没有可画的图形"


def run(tmp_path, capsys, before, after, *options):
    """Compare two report texts; the exit code, what was printed, the errors."""
    one, two = tmp_path / "before.txt", tmp_path / "after.txt"
    one.write_text(before, encoding="utf-8")
    two.write_text(after, encoding="utf-8")
    code = compare_walks.main([str(one), str(two), *options])
    captured = capsys.readouterr()
    return code, captured.out, captured.err


# ── Reading a report ─────────────────────────────────────────────────────────


def test_the_walks_layout_is_read():
    walk = compare_walks.parse_report(
        report([A, B, C], unread=[("001", NO_NUMBER)], unopened=[("001", NO_GEOMETRY)])
    )
    assert walk.source == "D:\\samples\\made-up"
    assert walk.drawings == 4
    assert list(walk.imports) == ["A-001", "B 02", "井场 P&ID"]
    assert walk.imports["B 02"] == compare_walks.Import(765, 364, 0, 3, 228.2)
    assert (walk.entities, walk.undrawn) == (1228, 2)
    assert walk.import_millis == pytest.approx(410.8)
    assert walk.unread == {"001": NO_NUMBER}
    assert walk.unopened == {"001": NO_GEOMETRY}


def test_a_name_with_a_bar_in_it_is_read_whole():
    odd = ("P | ID 图", 5, 4, 0, 1, 2.0)
    assert list(compare_walks.parse_report(report([odd])).imports) == ["P | ID 图"]


def test_two_drawings_of_one_name_are_told_apart():
    twin = ("A-001", 9, 5, 0, 1, 1.0)
    walk = compare_walks.parse_report(report([A, twin]))
    assert list(walk.imports) == ["A-001", "A-001 [2]"]
    assert walk.imports["A-001 [2]"].drawn == 9


# ── What changed ─────────────────────────────────────────────────────────────


def test_changed_numbers_are_listed_with_both_values(tmp_path, capsys):
    grown = ("A-001", 340, 215, 0, 4, 170.0)
    code, out, _ = run(tmp_path, capsys, report([A, B, C]), report([grown, B, C]))
    assert code == 0
    assert "| `A-001` | 330 → 340 | 209 → 215 | 0 | 4 |  |" in out
    assert "`B 02`" not in out
    assert "| 画出的图元 | 1228 | 1238 | +10 |" in out
    assert "1 张有变化（共 3 张）" in out
    assert "结论：没有变差。" in out


def test_fewer_entities_and_more_records_not_drawn_are_marked(tmp_path, capsys):
    poorer = ("井场 P&ID", 120, 129, 5, 2, 16.0)
    code, out, _ = run(tmp_path, capsys, report([A, C]), report([A, poorer]))
    # A mark asks for a written reason: it does not fail the comparison, and
    # the comparison does not call the walk "no worse" either.
    assert code == 0
    assert "| `井场 P&ID` | 133 → 120 | 129 | 2 → 5 | 2 | 图元变少；没画出的变多 |" in out
    assert "「要写原因」一栏有字的图" in out
    assert "1 张图标了「要写原因」" in out
    assert "结论：没有变差。" not in out


def test_a_newly_unreadable_drawing_fails(tmp_path, capsys):
    code, out, _ = run(
        tmp_path, capsys, report([A, B]), report([A, B], unread=[("B 02", NO_NUMBER)])
    )
    assert code == 1
    assert f"读不出图内对象的名单：新增 `B 02`（{NO_NUMBER}）" in out
    assert "结论：变差 1 处。" in out
    assert "新增读不出图内对象的图：`B 02`" in out


def test_a_newly_unopenable_drawing_fails(tmp_path, capsys):
    # B no longer imports: it leaves the import rows and joins the list.
    code, out, _ = run(
        tmp_path, capsys, report([A, B]), report([A], unopened=[("B 02", NO_GEOMETRY)])
    )
    assert code == 1
    assert f"打不开的名单：新增 `B 02`（{NO_GEOMETRY}）" in out
    assert "| `B 02` | 765 → 没导入 |" in out
    assert "新增打不开的图：`B 02`" in out


def test_drawings_that_leave_the_lists_are_named_and_pass(tmp_path, capsys):
    before = report([A], unread=[("001", NO_NUMBER)], unopened=[("001", NO_GEOMETRY)])
    mended = ("001", 12, 9, 0, 1, 3.0)
    code, out, _ = run(tmp_path, capsys, before, report([A, mended]))
    assert code == 0
    assert "读不出图内对象的名单：减少 `001`" in out
    assert "打不开的名单：减少 `001`" in out
    assert "| `001` | 没导入 → 12 |" in out
    assert "结论：没有变差。" in out


def test_a_drawing_in_only_one_report_fails(tmp_path, capsys):
    code, out, _ = run(tmp_path, capsys, report([A, B]), report([A, C]))
    assert code == 1
    assert "只在「前」里的图：`B 02`" in out
    assert "只在「后」里的图：`井场 P&ID`" in out
    assert "结论：变差 2 处。" in out


def test_a_different_number_of_drawings_fails(tmp_path, capsys):
    code, out, _ = run(tmp_path, capsys, report([A, B], count=2), report([A, B], count=3))
    assert code == 1
    assert "| 图纸数 | 2 | 3 | +1 |" in out
    assert "图纸数从 2 变成 3" in out


def test_imports_slower_than_the_limit_fail(tmp_path, capsys):
    before, after = report([A, B], total_ms=10000.0), report([A, B], total_ms=16000.0)
    code, out, _ = run(tmp_path, capsys, before, after)
    assert code == 1
    assert "| 全部导入用时 | 10.0 s | 16.0 s | 1.60 倍，超过上限 1.5 倍 |" in out
    code, out, _ = run(tmp_path, capsys, before, after, "--max-time-ratio", "2")
    assert code == 0
    assert "| 全部导入用时 | 10.0 s | 16.0 s | 1.60 倍 |" in out


def test_short_runs_report_the_ratio_without_judging_it(tmp_path, capsys):
    # Half a second against nine tenths is the disk, not the parser.
    code, out, _ = run(
        tmp_path, capsys, report([A, B], total_ms=500.0), report([A, B], total_ms=900.0)
    )
    assert code == 0
    assert "| 全部导入用时 | 500.0 ms | 900.0 ms | 1.80 倍（不足 5.0 s，只报不判） |" in out


def test_identical_reports_say_no_change(tmp_path, capsys):
    text = report([A, B, C], unread=[("001", NO_NUMBER)], unopened=[("001", NO_GEOMETRY)])
    code, out, _ = run(tmp_path, capsys, text, text)
    assert code == 0
    assert "两份报告没有变化" in out
    assert "结论：没有变差。" in out


def test_reports_of_different_sources_are_noted(tmp_path, capsys):
    text = report([A])
    code, out, _ = run(
        tmp_path, capsys, text, text.replace("D:\\samples\\made-up", "D:\\samples\\other")
    )
    assert code == 0
    assert "来源不同" in out and "D:\\samples\\other" in out


# ── Reports that cannot be used ──────────────────────────────────────────────


@pytest.mark.parametrize(
    "old, new, named",
    [
        ("== import of every drawing (", "== imports (", "import of every drawing"),
        ("== drawings (", "== sheets (", "drawings ("),
        ("drawing(s) whose items did not read", "unread", "whose items did not read"),
        ("drawing(s) that did not import", "not imported", "that did not import"),
        (" entities drawn, ", " drawn, ", "entities drawn"),
    ],
)
def test_a_report_in_an_unknown_layout_is_an_error_not_no_change(
    tmp_path, capsys, old, new, named
):
    good = report([A, B])
    broken = good.replace(old, new)
    assert broken != good
    code, out, err = run(tmp_path, capsys, good, broken)
    assert code == 2
    assert named in err and "after.txt" in err
    assert "没有变化" not in out and "没有变差" not in out


def test_a_walk_that_skipped_the_imports_cannot_be_compared(tmp_path, capsys):
    code, out, err = run(tmp_path, capsys, report([A]), report([A], skip_import=True))
    assert code == 2
    assert "OCS_PID_BACKUP_AUDIT_SKIP_IMPORT" in err
    assert out == ""


def test_a_list_shorter_than_its_heading_is_an_error(tmp_path, capsys):
    good = report([A], unread=[("001", NO_NUMBER)])
    cut = good.replace(
        f"  001: {NO_NUMBER}\n== 0 drawing(s) that did not import",
        "== 0 drawing(s) that did not import",
    )
    assert cut != good
    code, _, err = run(tmp_path, capsys, good, cut)
    assert code == 2
    assert "whose items did not read" in err and "1 张" in err and "0 张" in err


def test_rows_that_do_not_add_up_to_the_totals_are_an_error(tmp_path, capsys):
    good = report([A, B])
    odd = good.replace("  1095 entities drawn", "  1000 entities drawn")
    assert odd != good
    code, _, err = run(tmp_path, capsys, good, odd)
    assert code == 2
    assert "1095" in err and "1000" in err


def test_a_missing_report_is_an_error(tmp_path, capsys):
    (tmp_path / "a.txt").write_text(report([A]), encoding="utf-8")
    code = compare_walks.main([str(tmp_path / "a.txt"), str(tmp_path / "nowhere.txt")])
    assert code == 2
    assert "nowhere.txt" in capsys.readouterr().err


# ── The command line ─────────────────────────────────────────────────────────


def test_a_folder_stands_for_its_report_and_the_table_can_go_to_a_file(tmp_path, capsys):
    for name in ("before", "after"):
        (tmp_path / name).mkdir()
        (tmp_path / name / "report.txt").write_text(report([A, B]), encoding="utf-8")
    table = tmp_path / "table.md"
    code = compare_walks.main(
        [str(tmp_path / "before"), str(tmp_path / "after"), "--label", "全厂包", "--out", str(table)]
    )
    assert code == 0
    written = table.read_bytes().decode("utf-8")
    assert written == capsys.readouterr().out
    assert written.startswith("**走查对比：全厂包**\n")
    assert "\r" not in written
