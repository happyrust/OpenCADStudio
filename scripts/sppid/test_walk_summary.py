"""Tests for scripts/sppid/walk_summary.py, run on made-up walks: a runs.json
as walk-samples.ps1 leaves it, and reports that keep the layout of the walk's
real report.txt."""

import json
from pathlib import Path
import sys

import pytest

sys.path.insert(0, str(Path(__file__).resolve().parent))
import walk_summary  # noqa: E402
from test_compare_walks import A, B, C, NO_GEOMETRY, NO_NUMBER, report  # noqa: E402

# What `report` writes on the report's first line.
SOURCE = "D:\\samples\\made-up"
INTERFACE = (
    "interface: the P&ID module, as a launch without --classic starts it "
    "(OCS_PID_BACKUP_AUDIT_SHELL=1)"
)


def walked(
    rows=(A, B, C), unread=(), unopened=(), expect="3,0,0", met=True, unmet=(), module=False, off=0
):
    """The report of a walk run the way walk-samples.ps1 runs it: its
    expectation closes it, and a walk in the module says so on the second
    line and lists the drawings it opened from the panel."""
    lines = report(list(rows), unread=unread, unopened=unopened).splitlines()
    if module:
        lines.insert(1, INTERFACE)
        table = [
            "== every drawing opened from the panel in the module "
            "(name | tabs | entities in its tab | time) =="
        ]
        table += [
            f"    {name} | 1 -> 1, not opened | 打开失败 : {reason} | 11.9 ms"
            for name, reason in unopened
        ]
        table += [f"    {row[0]} | 1 -> 2, a tab of its own | {row[1] + 70} | 250.0 ms" for row in rows]
        table.append(
            f"  {len(rows)} opened: {len(rows)} in a tab of their own, 0 in the blank tab in front, "
            f"0 in the tab that showed them already; {len(unopened)} not opened, their import fails; "
            f"0 imported with no entity and left out; {off} not as an open owes; all in 761.9 ms"
        )
        table += [f"  not as an open owes: {rows[0][0]}: the tab in front has no entity"] * off
        at = next(at for at, line in enumerate(lines) if line.startswith("== check of the drawings"))
        lines[at:at] = table
    if expect is not None:
        lines.append(
            f"== expectation {expect} (drawings, items unread at most, not imported at most): "
            f"{'met' if met else 'not met'} =="
        )
        lines += [f"  {line}" for line in unmet]
    return "\n".join(lines) + "\n"


def a_walk(
    tmp_path,
    text,
    package="small",
    form="folder",
    interface="classic",
    expect="3,0,0",
    exit_code=0,
    source=SOURCE,
    note="",
):
    """One entry of runs.json, with what the walk left in its folder: the
    report (none when `text` is None) and the output of its test (none when
    the walk was not run)."""
    folder = tmp_path / f"{package}-{form}-{interface}"
    folder.mkdir()
    if text is not None:
        (folder / "report.txt").write_text(text, encoding="utf-8")
    if exit_code is not None:
        ended = "ok. 1 passed; 0 failed" if exit_code == 0 else "FAILED. 0 passed; 1 failed"
        (folder / "cargo-test.log").write_text(
            "running 1 test\n"
            f"test result: {ended}; 0 ignored; 0 measured; 6972 filtered out; finished in 16.02s\n",
            encoding="utf-8",
        )
    return {
        "package": package,
        "form": form,
        "interface": interface,
        "source": source,
        "expect": expect,
        "folder": str(folder),
        "exit_code": exit_code,
        "seconds": 84.2,
        "note": note,
    }


def both(tmp_path, classic=None, module=None, **common):
    """The two walks of one backup, one per interface."""
    classic = walked() if classic is None else classic
    module = walked(module=True) if module is None else module
    return [
        a_walk(tmp_path, classic, interface="classic", **common),
        a_walk(tmp_path, module, interface="module", **common),
    ]


def summed(tmp_path, capsys, runs, *options, **manifest):
    """Sum up the walks of a runs.json: the exit code, what was printed, the errors."""
    path = tmp_path / "runs.json"
    path.write_text(json.dumps({"runs": runs, **manifest}, ensure_ascii=False), encoding="utf-8")
    code = walk_summary.main([str(path), *options])
    captured = capsys.readouterr()
    return code, captured.out, captured.err


def row_of(out, *cells):
    """The table row that starts with these cells."""
    start = "| " + " | ".join(cells) + " |"
    rows = [line for line in out.splitlines() if line.startswith(start)]
    assert len(rows) == 1, f"{start!r} starts {len(rows)} row(s) of:\n{out}"
    return rows[0]


# ── The rows ───────────────────────────────────────────────────────────────────


def test_walks_within_their_expectation_pass(tmp_path, capsys):
    code, out, _ = summed(tmp_path, capsys, both(tmp_path))
    assert code == 0
    assert (
        row_of(out, "小包", "目录", "经典")
        == "| 小包 | 目录 | 经典 | 3 | 0 | 0 | 1228 | — | 410.8 ms | 16 s | `3,0,0` | 过 |"
    )
    assert (
        row_of(out, "小包", "目录", "新界面")
        == "| 小包 | 目录 | 新界面 | 3 | 0 | 0 | 1228 | 3 张，0 处不符，761.9 ms | 410.8 ms | 16 s "
        "| `3,0,0` | 过 |"
    )
    # What the two runs of cargo took, the walks and the start of the tests.
    assert "2 次走查共用 2 分 48 秒" in out
    assert "结论：2 次走查全部通过，1 项比较全部相同。" in out


def test_failures_within_the_limits_pass_and_are_listed_with_their_reasons(tmp_path, capsys):
    failing = dict(unread=[("001", NO_NUMBER)], unopened=[("001", NO_GEOMETRY)], expect="4,1,1")
    runs = both(
        tmp_path, walked(**failing), walked(module=True, **failing), package="plant", expect="4,1,1"
    )
    code, out, _ = summed(tmp_path, capsys, runs)
    assert code == 0
    assert row_of(out, "全厂包", "目录", "新界面").startswith(
        "| 全厂包 | 目录 | 新界面 | 4 | 1 | 1 | 1228 | 3 张，0 处不符，761.9 ms |"
    )
    assert f"  - 读不出 `001`：{NO_NUMBER}" in out
    assert f"  - 打不开 `001`：{NO_GEOMETRY}" in out


def test_a_walk_whose_test_failed_does_not_pass(tmp_path, capsys):
    short = walked(expect="4,0,0", met=False, unmet=["4 drawing(s) expected, the backup lists 3"])
    runs = [a_walk(tmp_path, short, expect="4,0,0", exit_code=101)]
    code, out, _ = summed(tmp_path, capsys, runs)
    assert code == 1
    row = row_of(out, "小包", "目录", "经典")
    assert row.endswith("| `4,0,0` | 不过：走查的测试失败（退出码 101）；图纸 3 张，期望 4 张 |")
    assert "结论：1 处不过。" in out


def test_a_walk_without_a_report_does_not_pass(tmp_path, capsys):
    code, out, _ = summed(tmp_path, capsys, [a_walk(tmp_path, None, exit_code=101)])
    assert code == 1
    row = row_of(out, "小包", "目录", "经典")
    assert row.startswith("| 小包 | 目录 | 经典 | — | — | — | — | — | — | 16 s |")
    assert "没有报告" in row


def test_a_report_that_is_not_a_walks_does_not_pass(tmp_path, capsys):
    code, out, _ = summed(tmp_path, capsys, [a_walk(tmp_path, "source: somewhere\n")])
    assert code == 1
    assert "报告不能用：报告里没有「== drawings (N; M without a file) ==」这一行" in out


def test_a_walk_that_was_not_run_does_not_pass(tmp_path, capsys):
    runs = [a_walk(tmp_path, None, exit_code=None, note="the source does not exist")]
    code, out, _ = summed(tmp_path, capsys, runs)
    assert code == 1
    row = row_of(out, "小包", "目录", "经典")
    assert row.endswith("| — | `3,0,0` | 不过：没有走查（the source does not exist） |")


@pytest.mark.parametrize(
    ("text", "entry", "said"),
    [
        # Left by a walk of another backup.
        (
            walked(),
            {"source": "D:\\samples\\another"},
            "报告的来源是 `D:\\samples\\made-up`，这次走查的是 `D:\\samples\\another`",
        ),
        # Left by a walk held to something else.
        (walked(expect="3,1,1"), {}, "报告里的期望值是 `3,1,1`，这次要的是 `3,0,0`"),
        # Left by a walk held to nothing.
        (walked(expect=None), {}, "报告里没有期望值一节"),
        # Left by a walk in the other interface.
        (walked(module=True), {}, "报告是新界面下的，这次走的是经典界面"),
        (walked(), {"interface": "module"}, "报告是经典界面下的，这次走的是新界面"),
    ],
    ids=["source", "expectation", "no-expectation", "module-report", "classic-report"],
)
def test_the_report_of_another_walk_does_not_pass(tmp_path, capsys, text, entry, said):
    code, out, _ = summed(tmp_path, capsys, [a_walk(tmp_path, text, **entry)])
    assert code == 1
    assert said in out


def test_the_source_is_the_same_however_it_is_written(tmp_path, capsys):
    runs = [a_walk(tmp_path, walked(), source="d:/samples/Made-Up/")]
    code, out, _ = summed(tmp_path, capsys, runs)
    assert code == 0
    assert row_of(out, "小包", "目录", "经典").endswith("| 过 |")


def test_numbers_beyond_the_expectation_do_not_pass_whatever_the_report_says(tmp_path, capsys):
    # The report claims `met`; its own lists say otherwise.
    text = walked(unread=[("001", NO_NUMBER)], unopened=[("001", NO_GEOMETRY)], expect="4,0,1")
    code, out, _ = summed(tmp_path, capsys, [a_walk(tmp_path, text, expect="4,0,1")])
    assert code == 1
    row = row_of(out, "小包", "目录", "经典")
    assert row.endswith("| `4,0,1` | 不过：读不出 1 张，上限 0 张（`001`） |")


def test_a_report_that_says_its_expectation_is_not_met_does_not_pass(tmp_path, capsys):
    # Nothing the lists show: the report's own word is passed on.
    text = walked(met=False, unmet=["something the lists do not show"])
    code, out, _ = summed(tmp_path, capsys, [a_walk(tmp_path, text, exit_code=101)])
    assert code == 1
    assert "报告说期望值没有达到（something the lists do not show）" in row_of(
        out, "小包", "目录", "经典"
    )


def test_a_listed_drawing_without_a_reason_does_not_pass(tmp_path, capsys):
    text = walked(unread=[("001", "")], unopened=[("001", NO_GEOMETRY)], expect="4,1,1")
    code, out, _ = summed(tmp_path, capsys, [a_walk(tmp_path, text, expect="4,1,1")])
    assert code == 1
    assert row_of(out, "小包", "目录", "经典").endswith("| 不过：没有写原因：读不出的 `001` |")


def test_a_drawing_opened_wrong_in_the_module_does_not_pass(tmp_path, capsys):
    runs = [a_walk(tmp_path, walked(module=True, off=1), interface="module", exit_code=101)]
    code, out, _ = summed(tmp_path, capsys, runs)
    assert code == 1
    row = row_of(out, "小包", "目录", "新界面")
    assert "| 3 张，1 处不符，761.9 ms |" in row
    assert "从面板打开的图有 1 处不符（A-001: the tab in front has no entity）" in row


def test_a_module_walk_without_its_table_of_opened_drawings_does_not_pass(tmp_path, capsys):
    # The interface line alone: the drawings were never opened from the panel.
    lines = walked().splitlines()
    lines.insert(1, INTERFACE)
    runs = [a_walk(tmp_path, "\n".join(lines) + "\n", interface="module")]
    code, out, _ = summed(tmp_path, capsys, runs)
    assert code == 1
    row = row_of(out, "小包", "目录", "新界面")
    assert row.endswith("| 不过：报告里没有从面板开图的合计行 |")


# ── The comparisons ────────────────────────────────────────────────────────────


def test_the_two_interfaces_must_draw_the_same(tmp_path, capsys):
    more = ("B 02", 766, 364, 0, 3, 228.2)
    runs = both(tmp_path, module=walked(rows=(A, more, C), module=True))
    code, out, _ = summed(tmp_path, capsys, runs)
    assert code == 1
    # Each walk passes by itself: it is the pair that does not agree.
    assert row_of(out, "小包", "目录", "经典").endswith("| 过 |")
    assert row_of(out, "小包", "目录", "新界面").endswith("| 过 |")
    assert "- 小包（目录）：不同。`B 02` 的四个数：经典 765/364/0/3，新界面 766/364/0/3。" in out
    assert "结论：1 处不过。" in out


def test_the_two_interfaces_must_list_the_same_failures(tmp_path, capsys):
    one = walked(unread=[("001", NO_NUMBER)], unopened=[("001", NO_GEOMETRY)], expect="4,1,1")
    other = walked(unopened=[("001", NO_GEOMETRY)], expect="4,1,1", module=True)
    code, out, _ = summed(tmp_path, capsys, both(tmp_path, one, other, expect="4,1,1"))
    assert code == 1
    assert "- 小包（目录）：不同。读不出的名单：只在经典里的 `001`。" in out


def test_a_zip_and_its_folder_must_hold_the_same_drawings(tmp_path, capsys):
    runs = [
        a_walk(tmp_path, walked(), form="folder"),
        a_walk(tmp_path, walked(rows=(A, B), expect="2,0,0"), form="zip", expect="2,0,0"),
    ]
    code, out, _ = summed(tmp_path, capsys, runs)
    assert code == 1
    assert (
        "- 小包，经典：不同。图纸数：目录 3，zip 2；图纸名单：只在目录里的 `井场 P&ID`。" in out
    )


def test_a_zip_that_agrees_with_its_folder_passes(tmp_path, capsys):
    runs = [
        a_walk(tmp_path, walked(), form="folder"),
        a_walk(tmp_path, walked(), form="zip"),
    ]
    code, out, _ = summed(tmp_path, capsys, runs)
    assert code == 0
    assert "- 小包，经典：相同。" in out
    assert row_of(out, "小包", "zip", "经典").endswith("| 过 |")


def test_a_comparison_with_a_side_missing_is_not_a_pass(tmp_path, capsys):
    runs = [
        a_walk(tmp_path, walked(), interface="classic"),
        a_walk(tmp_path, None, interface="module", exit_code=101),
    ]
    code, out, _ = summed(tmp_path, capsys, runs)
    assert code == 1
    assert "- 小包（目录）：没法比，新界面那一次没有可用的报告。" in out
    assert "结论：2 处不过。" in out


# ── The whole ──────────────────────────────────────────────────────────────────


def test_a_part_of_the_walks_is_said_to_be_a_part(tmp_path, capsys):
    code, out, _ = summed(tmp_path, capsys, both(tmp_path), partial=True)
    assert code == 0
    assert "这次只走了一部分样本包，不是完整的验收" in out
    code, out, _ = summed(tmp_path, capsys, [], partial=False)
    assert "不是完整的验收" not in out


def test_no_walk_at_all_is_not_a_pass(tmp_path, capsys):
    code, out, _ = summed(tmp_path, capsys, [])
    assert code == 1
    assert "结论：一次走查也没有。" in out


def test_the_summary_is_written_to_a_file_as_well(tmp_path, capsys):
    kept = tmp_path / "summary.md"
    code, out, _ = summed(tmp_path, capsys, both(tmp_path), "--out", str(kept))
    assert code == 0
    assert out.startswith("**样本包走查汇总**\n")
    assert kept.read_bytes() == out.encode("utf-8")
    assert b"\r" not in kept.read_bytes()


def test_a_summary_file_that_cannot_be_written_is_an_error(tmp_path, capsys):
    nowhere = tmp_path / "no-such-folder" / "summary.md"
    code, out, err = summed(tmp_path, capsys, both(tmp_path), "--out", str(nowhere))
    assert code == 2
    assert "summary.md: 写不了" in err
    assert out == ""


def test_home_folders_are_not_printed(tmp_path, capsys):
    # A zip is restored under the temporary folder of whoever runs the walk.
    where = "C:\\Users\\someone\\AppData\\Local\\Temp\\ocs-pid-backups\\1f\\001.pid"
    text = walked(unopened=[("001", f"无法读取 {where}")], expect="4,0,1")
    code, out, _ = summed(tmp_path, capsys, [a_walk(tmp_path, text, expect="4,0,1")])
    assert code == 0
    assert (
        "  - 打不开 `001`：无法读取 %USERPROFILE%\\AppData\\Local\\Temp\\ocs-pid-backups\\1f\\001.pid"
        in out
    )
    assert "someone" not in out


@pytest.mark.parametrize(
    "where",
    [
        # As the walk prints a path it holds as Some("..."): every separator twice.
        r"C:\\Users\\someone\\AppData\\Local\\Temp\\x.pid",
        "C:/Users/someone/AppData/Local/Temp/x.pid",
    ],
    ids=["doubled", "slashes"],
)
def test_home_folders_are_not_printed_however_they_are_written(tmp_path, capsys, where):
    text = walked(module=True, off=1).replace(
        "the tab in front has no entity", f'the tab in front shows Some("{where}")'
    )
    runs = [a_walk(tmp_path, text, interface="module", exit_code=101)]
    code, out, _ = summed(tmp_path, capsys, runs)
    assert code == 1
    assert 'the tab in front shows Some("%USERPROFILE%' in out
    assert "someone" not in out


ONE = {
    "package": "small",
    "form": "folder",
    "interface": "classic",
    "source": "D:\\samples\\made-up",
    "expect": "3,0,0",
    "folder": "D:\\nowhere",
}


@pytest.mark.parametrize(
    ("manifest", "said"),
    [
        (None, "读不了"),
        ("not json", "不是 JSON"),
        (json.dumps({"walks": []}), "没有 runs 一项"),
        (json.dumps({"runs": [{"package": "small"}]}), "第 1 次走查缺 form"),
        (json.dumps({"runs": [{**ONE, "form": "dir"}]}), "第 1 次走查的 form 是「dir」"),
        (json.dumps({"runs": [{**ONE, "interface": "new"}]}), "第 1 次走查的 interface 是「new」"),
        (json.dumps({"runs": [{**ONE, "exit_code": "0"}]}), "第 1 次走查的 exit_code 不是整数"),
        (json.dumps({"runs": [ONE, ONE]}), "第 2 次走查和第 1 次是同一个"),
    ],
    ids=["missing", "not-json", "no-runs", "no-form", "form", "interface", "exit-code", "twice"],
)
def test_a_manifest_that_cannot_be_used_is_an_error(tmp_path, capsys, manifest, said):
    path = tmp_path / "runs.json"
    if manifest is not None:
        path.write_text(manifest, encoding="utf-8")
    assert walk_summary.main([str(path)]) == 2
    captured = capsys.readouterr()
    assert said in captured.err
    assert captured.out == ""


@pytest.mark.parametrize("expect", ["7,1", "7,x,1", "7,1,1,0", ""])
def test_an_expectation_that_is_not_three_numbers_is_an_error(tmp_path, capsys, expect):
    code, _, err = summed(tmp_path, capsys, [a_walk(tmp_path, walked(), expect=expect)])
    assert code == 2
    assert f"第 1 次走查的期望值「{expect}」不是三个数" in err


def test_a_manifest_with_a_byte_order_mark_is_read(tmp_path, capsys):
    # What Windows PowerShell 5.1 writes when asked for UTF-8.
    path = tmp_path / "runs.json"
    path.write_text(json.dumps({"runs": both(tmp_path)}), encoding="utf-8-sig")
    assert walk_summary.main([str(path)]) == 0
    assert "结论：2 次走查全部通过" in capsys.readouterr().out
