# -*- coding: utf-8 -*-
"""The rules a whole run has to keep, checked when the run is over.

run19.ps1 starts this file twice: with --begin before anything else (it writes
down when the run began and what the repository looked like), and as the last
scenario, which checks:

VAL-INFRA-013  what the run produced is in the evidence directory, outside the
               repository: `git status --porcelain` shows nothing after the
               run that it did not show before it, and no bytecode cache was
               written beside the scripts (git ignores those, so its status
               would not tell).
VAL-INFRA-007  what the entries did with the scenarios that lost an instance
               (exit code 75 in <name>.runs.txt): each was run once more,
               only once more, and its first attempt was kept.
VAL-INFRA-008  every GUI start of the run (gui-starts.jsonl of both suites):
               the port inside 3210-3239 and found free right before.
VAL-CLI-019    the dependency tree of the desktop program
               (`cargo tree -p OpenCADStudio --edges normal`): no tokio, no
               async-std, tungstenite present.
harness        the two copies of the harness (m6h.py, m7h.py) carry the same
               code for the rules the scenarios rely on.

By hand: `python r01_entry_rules.py --begin`, run what is to be judged, then
`python r01_entry_rules.py`. Without the first step there is nothing to compare
with, and the scenario says so.

The git command only reads (`--no-optional-locks`: not even the index is
refreshed). OCS_E2E_TREE_PACKAGE (for a control run only) names another
package whose tree is read instead.
"""
import inspect
import json
import os
import re
import subprocess
import sys
import time
import traceback

import m6h
import m7h

EVIDENCE = m7h.ROOT.parent
BEGIN = EVIDENCE / "run-begin.json"
PACKAGE = os.environ.get("OCS_E2E_TREE_PACKAGE") or "OpenCADStudio"
CACHE_ROOTS = (m7h.HERE, m7h.REPO / "docs" / "automation")     # where this run's Python code is imported from
SAME = ("gui_port_range", "InstanceVanished", "log_tail", "port_listening", "Gui.pick_port", "Gui.record_start",
        "Gui.api", "Gui.ready_refused", "Gui.check_vanished", "Gui.ready", "Gui.wait_ready", "Gui._wait_ready",
        "Gui.restart_elsewhere", "Gui.run", "Results.summary")
CONSTANTS = ("GUI_PORTS", "READY_BUDGET", "VANISHED_EXIT")


def git_status():
    """The lines of `git status --porcelain`, every untracked file on its own line."""
    out = subprocess.run(["git", "--no-optional-locks", "-c", "core.quotepath=false", "status", "--porcelain",
                          "--untracked-files=all"], cwd=str(m7h.REPO), capture_output=True, text=True,
                         encoding="utf-8", errors="replace")
    if out.returncode != 0:
        raise RuntimeError(f"git status ended with {out.returncode}: {out.stderr.strip()[:300]}")
    return [line for line in out.stdout.splitlines() if line.strip()]


def caches():
    """The bytecode cache files beside the scripts and when each was written."""
    found = {}
    for root in CACHE_ROOTS:
        for folder in root.rglob("__pycache__"):
            for path in folder.iterdir():
                if path.is_file():
                    found[path.relative_to(m7h.REPO).as_posix()] = path.stat().st_mtime_ns
    return found


def written_since(epoch):
    """The files under the evidence directory written since then, relative to it."""
    found = []
    for path in EVIDENCE.rglob("*"):
        try:
            if path.is_file() and path.stat().st_mtime >= epoch:
                found.append(path.relative_to(EVIDENCE).as_posix())
        except OSError:
            pass
    return sorted(found)


def source(module, dotted):
    obj = module
    for name in dotted.split("."):
        obj = getattr(obj, name)
    return inspect.getsource(obj)


def http_port(args):
    return args[args.index("--http") + 1] if "--http" in args[:-1] else None


if "--begin" in sys.argv[1:]:
    BEGIN.write_text(json.dumps({"epoch": time.time(), "at": time.strftime("%Y-%m-%dT%H:%M:%S"),
                                 "status": git_status(), "caches": caches()}, ensure_ascii=False, indent=1), encoding="utf-8")
    print(f"{m7h.now()} run rules: what the repository looked like before the run is written to {BEGIN.name}", flush=True)
    sys.exit(0)

R = m7h.Results("r01_entry_rules")
try:
    begin = json.loads(BEGIN.read_text(encoding="utf-8")) if BEGIN.exists() else None
    low, high = m7h.gui_port_range()
    if begin is None:
        R.check("VAL-INFRA-013", False,
                f"没有这一轮开始时的记录（证据目录下的 {BEGIN.name}），跑完以后和什么比都无从谈起：本场景由 run19.ps1 在一轮的最后跑；"
                f"单独跑之前先执行 python r01_entry_rules.py --begin")
    else:
        # ── VAL-INFRA-013: nothing of the run in the repository ─────────────────────
        after = git_status()
        new = [line for line in after if line not in begin["status"]]
        grown = sorted(name for name, stamp in caches().items() if begin["caches"].get(name) != stamp)
        written = written_since(begin["epoch"])
        outside = EVIDENCE != m7h.REPO and m7h.REPO not in EVIDENCE.parents
        R.dump("r01-git-status.txt", "\n".join(after) + "\n")
        R.dump("r01-files-written.txt", "\n".join(written) + "\n")
        R.check("VAL-INFRA-013", outside and bool(written) and not new and not grown,
                f"这一轮（{begin['at']} 开始）的产物都在仓库之外的证据目录里（开始以后写下的文件 {len(written)} 个，清单在 r01-files-written.txt）；"
                f"跑完后 git status --porcelain {len(after)} 行（全文在 r01-git-status.txt），开始时没有的 {len(new)} 行（开始时 {len(begin['status'])} 行）；"
                f"脚本旁边（scripts/collab-e2e、docs/automation）这一轮新写的字节码缓存文件 {len(grown)} 个")
        for line in new[:20]:
            R.note(f"git status 里新出现的一行：{line}")
        for cache in grown:
            R.note(f"这一轮新写的字节码缓存文件：{cache}")

        # ── VAL-INFRA-007: what the entries did after a lost instance ───────────────
        ran, lost, wrong = 0, [], []
        for suite in ("m6", "m7"):
            logs = EVIDENCE / suite / "logs"
            for path in sorted(logs.glob("*.runs.txt")):
                if path.stat().st_mtime < begin["epoch"]:
                    continue
                ran += 1
                name = path.name[:-len(".runs.txt")]
                codes = [int(found.group(1)) for found in
                         re.finditer(r"^attempt=\d+ exit=(-?\d+)", path.read_text(encoding="ascii", errors="replace"), re.M)]
                kept = all((logs / f"{name}.attempt1.{kind}.txt").exists() for kind in ("console", "result"))
                if codes[:1] == [m7h.VANISHED_EXIT]:
                    lost.append(f"{suite}/{name}")
                    if len(codes) != 2 or not kept:
                        wrong.append(f"{suite}/{name}：各次尝试的退出码 {codes}，第一次的控制台和结果{'留着' if kept else '没有留'}")
                elif len(codes) != 1:
                    wrong.append(f"{suite}/{name}：各次尝试的退出码 {codes}，第一次不是 {m7h.VANISHED_EXIT}，应当正好跑一次")
        if not ran:
            R.note("这一轮没有经入口跑过场景（没有 *.runs.txt），VAL-INFRA-007 的入口规则这一轮不判")
        else:
            R.check("VAL-INFRA-007", not wrong,
                    f"这一轮经入口跑的脚本 {ran} 个，第一次尝试丢了实例（退出码 {m7h.VANISHED_EXIT}）的 {len(lost)} 个{lost if lost else ''}："
                    f"丢了的都重跑了一次、只重跑了一次，第一次的控制台和结果都留着；没丢的都只跑了一次；不合规的 {len(wrong)} 个"
                    + (f"：{wrong[:5]}" if wrong else ""))

        # ── VAL-INFRA-008: every GUI start of the run ───────────────────────────────
        starts = []
        for suite in ("m6", "m7"):
            path = EVIDENCE / suite / "logs" / "gui-starts.jsonl"
            for line in path.read_text(encoding="utf-8").splitlines() if path.exists() else []:
                if line.strip():
                    rec = json.loads(line)
                    if rec.get("epoch", 0) >= begin["epoch"]:
                        starts.append(dict(rec, suite=suite))
        bad = [rec for rec in starts
               if not (type(rec.get("port")) is int and low <= rec["port"] <= high and rec.get("range") == [low, high]
                       and bool(rec.get("checked")) and rec["checked"][-1] == [rec["port"], "free"]
                       and http_port(rec.get("args") or []) == str(rec["port"]))]
        moved = [rec for rec in starts if rec.get("asked") != rec.get("port")]
        if not starts:
            R.note("这一轮没有启动过 GUI（只跑了不带 GUI 的场景），VAL-INFRA-008 没有启动记录可查，这一轮不判")
        else:
            per_suite = {suite: sum(1 for rec in starts if rec["suite"] == suite) for suite in ("m6", "m7")}
            R.check("VAL-INFRA-008", not bad,
                    f"这一轮启动 GUI {len(starts)} 次（{per_suite}，{len({rec.get('scenario') for rec in starts})} 个脚本）：每次的 --http 端口都在 "
                    f"{low}–{high} 之内，启动前都查过没有人在监听（用到的端口 {sorted({rec.get('port') for rec in starts})}）；"
                    f"在范围内换过端口的 {len(moved)} 次（要的端口被占，或者就绪超时后换端口重试）；不合规的 {len(bad)} 次"
                    + (f"：{[(rec.get('scenario'), rec.get('gui'), rec.get('port'), rec.get('checked')) for rec in bad[:5]]}" if bad else ""))

    # ── VAL-CLI-019: the dependency tree of the desktop program ─────────────────────
    command = ["cargo", "tree", "--offline", "--locked", "-p", PACKAGE, "--edges", "normal"]
    tree = subprocess.run(command, cwd=str(m7h.REPO), capture_output=True, text=True, encoding="utf-8", errors="replace")
    R.dump("r01-cargo-tree.txt", tree.stdout if tree.returncode == 0 else tree.stdout + "\n" + tree.stderr)
    crates = {}
    for line in tree.stdout.splitlines():
        found = re.search(r"([A-Za-z0-9_\-]+) v(\d\S*)", line)
        if found:
            crates.setdefault(found.group(1), set()).add(found.group(2))
    banned = [name for name in ("tokio", "async-std") if name in crates]
    R.check("VAL-CLI-019", tree.returncode == 0 and not banned and "tungstenite" in crates,
            f"{' '.join(command)}：退出码 {tree.returncode}，{len(tree.stdout.splitlines())} 行、{len(crates)} 个不同的包；"
            f"tokio 和 async-std {'都不在里面' if not banned else '在里面的有 ' + '、'.join(banned)}，"
            f"tungstenite {'在里面（' + '、'.join(sorted(crates['tungstenite'])) + '）' if 'tungstenite' in crates else '不在里面'}")
    others = sorted(name for name in crates if name == "smol" or name.startswith(("tokio-", "async-std-")))
    if others:
        R.note(f"树里另有 {others}：契约点名的是 tokio 和 async-std 两个包，这几个不在点名之列，只记下来")

    # ── the two copies of the harness ───────────────────────────────────────────
    differing = [name for name in SAME if source(m6h, name) != source(m7h, name)]
    differing += [name for name in CONSTANTS if getattr(m6h, name) != getattr(m7h, name)]
    R.check("harness", not differing,
            f"两份脚手架（m6h.py、m7h.py）里端口、就绪重试、实例消失这三条规则的代码一字不差：比了 {len(SAME)} 段函数和 "
            f"{len(CONSTANTS)} 个常量，不一样的 {differing if differing else '没有'}（t02_harness_rules.py 试的是 m6h.py 那一份）")

    if begin is not None:
        used = BEGIN.with_name("run-begin.used.json")
        if used.exists():
            used.unlink()
        BEGIN.rename(used)      # one run, one comparison: a later run has to write its own
except SystemExit:
    raise
except Exception as error:  # noqa: BLE001
    traceback.print_exc()
    R.check("script", False, f"场景没有跑完：{type(error).__name__}: {error}")
finally:
    m7h.cleanup()
    ok = R.summary()
sys.exit(0 if ok else 1)
