# -*- coding: utf-8 -*-
"""VAL-INFRA-007: an instance that vanishes in the middle of a flow.

The flow is the one of VAL-INFRA-004: count the entities, run `CIRCLE 0,0 5`,
count again. In the first attempt the instance is ended from outside right
before the flow, the way a crashed instance goes; the harness is not told.
What has to happen then:

* the harness notices at the first request, writes down the exit code, the
  tail of the log and the time (logs/vanished-t03_vanish_retry-*.json), and
  the scenario ends with exit code 75;
* the entry (run_all.ps1, so run19.ps1 as well) keeps the console and the
  result file of that attempt as t03_vanish_retry.attempt1.* and runs the
  whole scenario once more: the same drawing on the same port;
* in the second attempt nothing is ended, the flow goes through and the
  entity count goes up by one. A scenario is never run a third time.

So this scenario is meant to be run through an entry. By hand, the first run
ends with exit code 75; a second run with OCS_E2E_ATTEMPT=2 finds the record
but not the kept files, which only an entry makes.

OCS_E2E_VANISH_ALWAYS=1 (for a control run only) ends the instance in every
attempt: the entry then has to stop after the second one and report the
scenario as not clean.
"""
import hashlib
import json
import os
import sys
import time
import traceback

import m6h

r = m6h.Results("t03_vanish_retry")
ALWAYS = os.environ.get("OCS_E2E_VANISH_ALWAYS") == "1"
FIRST = m6h.LOGS / "t03-first-attempt.json"     # what the first attempt leaves for the second
NAME = "t03-vanish.dwg"
KEPT = ("t03_vanish_retry.attempt1.console.txt", "t03_vanish_retry.attempt1.result.txt")


def digest(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def total(g):
    status, body = g.api("GET", "/entities")
    return (body or {}).get("total") if status == 200 else None


try:
    dwg = m6h.WORK / NAME
    if m6h.ATTEMPT == 1:
        for old in m6h.LOGS.glob("vanished-t03_vanish_retry-*.json"):
            old.unlink()
        if FIRST.exists():
            FIRST.unlink()
        m6h.fresh_copy(NAME)
    else:
        # The drawing of the first attempt, not a new copy: only what the ended
        # instance left beside it (lock and recovery files) is removed.
        stale = list(m6h.WORK.glob(f"{NAME}.ocs*")) + list(m6h.WORK.glob(f".{NAME}*"))
        for path in stale:
            path.unlink()
        r.note(f"第一次尝试的实例在图纸旁边留下的锁/恢复文件 {len(stale)} 个，已删；图纸本身没有重新复制")
    opened = digest(dwg) if dwg.exists() else None

    g = m6h.Gui("t03", 3210, dwg=dwg, collab=False).start()
    r.check("setup", g.wait_ready(180) is not None, f"GUI 就绪（第 {m6h.ATTEMPT} 次尝试，端口 {g.port}）")
    doc = g.doc_id()
    n0 = total(g)
    if m6h.ATTEMPT == 1 or ALWAYS:
        if m6h.ATTEMPT == 1:
            FIRST.write_text(json.dumps({"at": time.strftime("%Y-%m-%dT%H:%M:%S"), "port": g.port, "pid": g.pid,
                                         "dwg": str(dwg), "sha256": opened, "total": n0}), encoding="utf-8")
        m6h.k32.TerminateProcess(g.process, 9)
        m6h.k32.WaitForSingleObject(g.process, 10000)
        r.note(f"第 {m6h.ATTEMPT} 次尝试：实例 {g.pid} 被从外面结束（退出码 9），脚手架没有被告知；接着做流程")

    # ── the flow of VAL-INFRA-004 ──────────────────────────────────────────────
    status, body = g.run("CIRCLE 0,0 5", doc=doc)
    n1 = total(g)

    if m6h.ATTEMPT == 1 or ALWAYS:
        r.check("VAL-INFRA-007", False,
                f"实例已经被结束，脚手架却没有发现：POST /api/v1/run 返回 {status}，流程照常往下走了")
    else:
        first = json.loads(FIRST.read_text(encoding="utf-8")) if FIRST.exists() else {}
        files = sorted(m6h.LOGS.glob("vanished-t03_vanish_retry-*.json"))
        rec = json.loads(files[-1].read_text(encoding="utf-8")) if files else {}
        tail = rec.get("log_tail") or []
        r.check("VAL-INFRA-007", len(files) == 1 and rec.get("how") == "process exited" and rec.get("exit_code") == 9
                and rec.get("attempt") == 1 and bool(first) and rec.get("pid") == first.get("pid") and bool(rec.get("at")) and bool(tail),
                f"第一次尝试里实例中途消失，现象被记了下来（{files[-1].name if files else '没有记录文件'}）：{rec.get('how')}、"
                f"退出码 {rec.get('exit_code')}、时刻 {rec.get('at')}、日志尾部 {len(tail)} 行、发现时在做的请求 {rec.get('request')}")
        r.check("VAL-INFRA-007", bool(first) and g.port == first.get("port") == m6h.shifted(3210) and str(dwg) == first.get("dwg")
                and opened is not None and opened == first.get("sha256") and n0 == first.get("total"),
                f"重跑用的是同一个端口（{g.port}，第一次是 {first.get('port')}）和同一张图（路径相同，文件内容和第一次打开时逐字节相同，"
                f"流程开始时的实体数两次都是 {n0}）")
        r.check("VAL-INFRA-007", status == 200 and (body or {}).get("ok") is True and isinstance(n0, int) and n1 == n0 + 1,
                f"整个流程重跑了一遍并且走通：GET /api/v1/entities 的 total {n0} → POST /api/v1/run CIRCLE 0,0 5"
                f"（HTTP {status}，ok={(body or {}).get('ok')}）→ total {n1}，多了一个")
        kept = [name for name in KEPT if (m6h.LOGS / name).exists()]
        r.check("VAL-INFRA-007", m6h.ATTEMPT == 2 and len(files) == 1 and len(kept) == len(KEPT),
                f"只重跑一次，两次的结果都留着：这是第 {m6h.ATTEMPT} 次尝试，消失记录 {len(files)} 份；第一次尝试的控制台和结果由入口留成了 {kept}")
except SystemExit:
    raise
except Exception as error:  # noqa: BLE001
    traceback.print_exc()
    r.check("script", False, f"场景没有跑完：{type(error).__name__}: {error}")
finally:
    m6h.cleanup()
    ok = r.summary()
sys.exit(0 if ok else 1)
