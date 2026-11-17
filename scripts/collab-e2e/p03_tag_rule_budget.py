# -*- coding: utf-8 -*-
"""VAL-SEM-014  the performance test the contract names is run, and what it
             measured is within the budget.

`tag_rule_bulk_2000_under_three_seconds` is a test of the main library that
`cargo test` leaves out unless it is asked for (`#[ignore]`): 2000 instances
get their tags from the rule `P-{序号}` starting at 1001, three times over,
and the test fails when the median of the three is over 3000 ms, a last tag is
not P-3000, or the 2000 tags are not all different. Nothing ran it by habit.
This scenario does: cargo runs that one test in the repository these scripts
are in (offline, the lock file as it is), and the scenario judges the figures
the test printed, not only that cargo said ok.

No GUI is started. When the test program is not built or not up to date, cargo
builds it first, which takes minutes; it is the program the entry's baseline
drawing step builds.
"""
import re
import subprocess
import sys
import time

import m7h

R = m7h.Results("p03_tag_rule_budget")
ITEM = "VAL-SEM-014"
TEST = "tag_rule_bulk_2000_under_three_seconds"
COMMAND = ["cargo", "test", "--offline", "--locked", "--lib", "--", "--ignored", "--nocapture", TEST]
BUDGET, INSTANCES, RUNS, LAST_TAG = 3000, 2000, 3, "P-3000"     # the contract's: ms, instances; the test's three runs

try:
    began = time.time()
    try:
        done = subprocess.run(COMMAND, cwd=str(m7h.REPO), capture_output=True, timeout=7200)
        code = done.returncode
        said = done.stdout.decode("utf-8", "replace") + "\n" + done.stderr.decode("utf-8", "replace")
    except FileNotFoundError:
        code, said = None, ""
    spent = time.time() - began
    R.dump("p03-cargo.console.txt", said)

    # ── the test was run: that one, and it passed ──
    totals = [(int(p), int(f)) for p, f in re.findall(r"test result: \w+\. (\d+) passed; (\d+) failed", said)]
    passed, failed = sum(t[0] for t in totals), sum(t[1] for t in totals)
    named = re.findall(r"^test (\S*%s) \.\.\." % re.escape(TEST), said, re.M)
    R.check(ITEM, code == 0 and passed == 1 and failed == 0 and len(named) == 1,
            f"{' '.join(COMMAND)}（在脚本所在的仓库里执行，连编带跑 {spent:.0f} 秒）："
            f"{'退出码 %d' % code if code is not None else '这台机器的 PATH 里没有 cargo'}；跑了 {passed + failed} 条测试，"
            f"通过 {passed}、失败 {failed}；点到名的是 {named[0] if named else '—（输出里没有这条测试）'}")

    # ── what it measured ──
    runs = [(int(ms), tag, int(unique)) for ms, tag, unique in
            re.findall(r"VAL-SEM-014 run \d+/%d: (\d+) ms, 末个位号 ([^,\s]+), 去重后 (\d+)" % RUNS, said)]
    printed = re.search(r"VAL-SEM-014 实测（中位数 (\d+) ms）", said)
    median = sorted(run[0] for run in runs)[RUNS // 2] if len(runs) == RUNS else None
    R.check(ITEM, median is not None and median <= BUDGET and printed is not None and int(printed.group(1)) == median
            and all(run[1] == LAST_TAG and run[2] == INSTANCES for run in runs),
            f"测试打印的 {len(runs)} 轮（要 {RUNS} 轮）：每轮 {INSTANCES} 个实例，耗时 {[run[0] for run in runs]} 毫秒，"
            f"末个位号 {[run[1] for run in runs]}（要 {LAST_TAG}），去重后的位号数 {[run[2] for run in runs]}（要 {INSTANCES}）；"
            f"中位数 {median if median is not None else '—'} 毫秒（预算 {BUDGET}；测试自己打印的中位数 "
            f"{printed.group(1) if printed else '—'}）")
except SystemExit:
    raise
except Exception as error:  # noqa: BLE001
    import traceback
    R.check("script", False, f"脚本异常: {error!r}\n{traceback.format_exc()}")
finally:
    m7h.cleanup()
    ok = R.summary()
sys.exit(0 if ok else 1)
