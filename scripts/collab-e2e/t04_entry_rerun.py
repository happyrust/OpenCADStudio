# -*- coding: utf-8 -*-
"""The rerun rule of the entries, tried on stand-in scenarios.

VAL-INFRA-007  what an entry does with a scenario by its exit code
               (Invoke-E2eScenario in e2e_env.ps1): exit code 75, a lost
               instance, gets one more attempt and the first one is kept; a
               second 75 is the end, there is no third attempt; any other
               exit code, a failure included, is one attempt.

No application instance is started here. Four stand-ins are written to the
evidence directory (a suite of their own, "t04", beside m6 and m7); each only
writes a result file, says which attempt it is and ends with the exit code its
name stands for. The entry's own function runs them, in one PowerShell.

What a real instance that vanishes looks like, and the rerun on the same port
and the same drawing, is t03_vanish_retry.py.
"""
import re
import shutil
import subprocess
import sys
import traceback

import m6h

r = m6h.Results("t04_entry_rerun")
ROOT = m6h.ROOT.parent / "t04"
LOGS = ROOT / "logs"
WORK = ROOT / "work"
SUMMARY = LOGS / "t04-summary.txt"
LOST = m6h.VANISHED_EXIT
CASES = {                               # stand-in -> the exit code each attempt would end with
    "t04s_lost_once": [LOST, 0],
    "t04s_lost_twice": [LOST, LOST, 0],
    "t04s_failed": [1, 0],
    "t04s_passed": [0],
}
STAND_IN = '''# -*- coding: utf-8 -*-
"""A stand-in scenario written by t04_entry_rerun.py: no instance, one exit code per attempt."""
import os
import sys
import time
from pathlib import Path

NAME = Path(sys.argv[0]).stem
CODES = __CODES__
LOGS = Path(os.environ["OCS_E2E_EVIDENCE_DIR"]) / "t04" / "logs"
attempt = int(os.environ.get("OCS_E2E_ATTEMPT") or 0)
code = CODES[attempt - 1] if 1 <= attempt <= len(CODES) else 99
with open(LOGS / f"{NAME}.calls.txt", "a", encoding="ascii") as f:
    f.write(f"attempt={attempt} exit={code}\\n")
(LOGS / f"{NAME}.result.txt").write_text(f"written by attempt {attempt}\\n", encoding="ascii")
print(f"{time.strftime('%H:%M:%S')} console of attempt {attempt}", flush=True)
print(f"{time.strftime('%H:%M:%S')} === {NAME}: a stand-in, attempt {attempt}, exit code {code} ===", flush=True)
sys.exit(code)
'''


def quoted(path):
    return "'" + str(path).replace("'", "''") + "'"


def text(path):
    """What a file says, whichever way PowerShell encoded it; None when it is not there."""
    if not path.exists():
        return None
    raw = path.read_bytes()
    if raw[:2] in (b"\xff\xfe", b"\xfe\xff"):
        return raw.decode("utf-16", "replace")
    return raw.decode("utf-8-sig", "replace")


def has(path, needle):
    return needle in (text(path) or "")


def exit_codes(name):
    """The exit code of each attempt, as the entry wrote them down."""
    return [int(found.group(1)) for found in re.finditer(r"^attempt=\d+ exit=(-?\d+)", text(LOGS / f"{name}.runs.txt") or "", re.M)]


def calls(name):
    """The attempts the stand-in itself saw: one entry each time it was started."""
    return [int(found.group(1)) for found in re.finditer(r"^attempt=(\d+)", text(LOGS / f"{name}.calls.txt") or "", re.M)]


def summary_lines(name):
    return [line for line in (text(SUMMARY) or "").splitlines() if f" {name} exit=" in line]


def kept(name):
    """Which of the two files of a first attempt are there."""
    return [kind for kind in ("console", "result") if (LOGS / f"{name}.attempt1.{kind}.txt").exists()]


try:
    shutil.rmtree(ROOT, ignore_errors=True)
    LOGS.mkdir(parents=True)
    WORK.mkdir(parents=True)
    for name, codes in CASES.items():
        (WORK / f"{name}.py").write_text(STAND_IN.replace("__CODES__", repr(codes)), encoding="utf-8")
    # What an earlier run of the same name left behind must not be taken for this run's.
    stale = "t04s_passed"
    for kind in ("console", "result"):
        (LOGS / f"{stale}.attempt1.{kind}.txt").write_text("left by an earlier run\n", encoding="ascii")
    (LOGS / f"{stale}.runs.txt").write_text(f"attempt=1 exit={LOST} seconds=1 leftover=0\nattempt=2 exit=0 seconds=1 leftover=0\n",
                                           encoding="ascii")

    script = [f". {quoted(m6h.HERE / 'e2e_env.ps1')}", f"$e2e = Resolve-E2eEnv -Here {quoted(m6h.HERE)}", f"Set-Location {quoted(WORK)}"]
    script += [f"Invoke-E2eScenario -E2e $e2e -Suite 't04' -Name '{name}' -Summary {quoted(SUMMARY)}" for name in CASES]
    ran = subprocess.run(["powershell", "-NoProfile", "-ExecutionPolicy", "Bypass", "-Command", "; ".join(script)],
                         capture_output=True, text=True, errors="replace")
    r.dump("t04-entry-console.txt", ran.stdout + ("\n--- stderr ---\n" + ran.stderr if ran.stderr.strip() else ""))
    r.check("setup", ran.returncode == 0 and not ran.stderr.strip(),
            f"入口自己的 Invoke-E2eScenario（e2e_env.ps1）把 {len(CASES)} 个替身场景各跑了一遍：PowerShell 退出码 {ran.returncode}，"
            f"错误输出 {len(ran.stderr.strip())} 字符")

    # ── exit code 75 once: one more attempt, the first one kept ─────────────────
    name = "t04s_lost_once"
    lines = summary_lines(name)
    r.check("VAL-INFRA-007", calls(name) == [1, 2] and exit_codes(name) == [LOST, 0] and kept(name) == ["console", "result"]
            and has(LOGS / f"{name}.attempt1.console.txt", "console of attempt 1")
            and has(LOGS / f"{name}.attempt1.result.txt", "written by attempt 1")
            and has(LOGS / f"{name}.console.txt", "console of attempt 2") and has(LOGS / f"{name}.result.txt", "written by attempt 2")
            and len(lines) == 2 and f" exit={LOST} " in lines[0] and "attempt=1 (lost an instance: kept as attempt1, run once more)" in lines[0]
            and " exit=0 " in lines[1] and " attempt=2 ::" in lines[1],
            f"第一次以退出码 {LOST}（丢了实例）结束的场景，入口整个重跑一次：替身被启动 {len(calls(name))} 次（它看到的尝试序号 {calls(name)}），"
            f"入口记下的退出码 {exit_codes(name)}；第一次的控制台和结果留成 {name}.attempt1.*（留下的 {kept(name)}，内容是第一次写的），"
            f"当前的两个文件是第二次写的；汇总里 {len(lines)} 行，各标明是第几次")

    # ── exit code 75 twice: the end, no third attempt ───────────────────────────
    name = "t04s_lost_twice"
    lines = summary_lines(name)
    r.check("VAL-INFRA-007", calls(name) == [1, 2] and exit_codes(name) == [LOST, LOST] and kept(name) == ["console", "result"]
            and has(LOGS / f"{name}.attempt1.result.txt", "written by attempt 1") and has(LOGS / f"{name}.result.txt", "written by attempt 2")
            and len(lines) == 2 and "attempt=2 (lost an instance again: not run a third time)" in lines[1],
            f"重跑时又以 {LOST} 结束的场景到此为止，没有第三次：替身被启动 {len(calls(name))} 次（{calls(name)}；第三次本来会以 0 结束），"
            f"入口记下的退出码 {exit_codes(name)}，汇总第二行写明「not run a third time」，第一次的两个文件照样留着（{kept(name)}）")

    # ── any other exit code: one attempt ────────────────────────────────────────
    name = "t04s_failed"
    lines = summary_lines(name)
    passed = summary_lines("t04s_passed")
    r.check("VAL-INFRA-007", calls(name) == [1] and exit_codes(name) == [1] and not kept(name) and len(lines) == 1
            and "attempt=" not in lines[0] and calls("t04s_passed") == [1] and exit_codes("t04s_passed") == [0]
            and len(passed) == 1 and "attempt=" not in passed[0],
            f"别的退出码只跑一次，失败也不重跑：以 1 结束的替身被启动 {len(calls(name))} 次（第二次本来会以 0 结束），入口记下的退出码 "
            f"{exit_codes(name)}，没有 attempt1 文件；以 0 结束的被启动 {len(calls('t04s_passed'))} 次，退出码 {exit_codes('t04s_passed')}")
    r.check("VAL-INFRA-007", not kept(stale) and exit_codes(stale) == [0],
            f"同名场景上一轮留下的 attempt1 文件和尝试记录（本场景事先放的）在开跑前被清掉，不会被当成这一轮的："
            f"跑完后 attempt1 文件 {kept(stale) or '没有'}，尝试记录里的退出码 {exit_codes(stale)}")
except SystemExit:
    raise
except Exception as error:  # noqa: BLE001
    traceback.print_exc()
    r.check("script", False, f"场景没有跑完：{type(error).__name__}: {error}")
finally:
    m6h.cleanup()
    ok = r.summary()
sys.exit(0 if ok else 1)
