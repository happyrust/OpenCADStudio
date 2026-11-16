# -*- coding: utf-8 -*-
"""Smoke test of the harness: one GUI on the hidden desktop, no collaboration.

VAL-INFRA-001  started with a real drawing, the instance answers /api/v1/ready
               with 200 within 60 seconds, and the five fields of the answer
               have the values and types the contract names.
VAL-INFRA-014  from the start of the process to the first 200: 60 seconds at
               most. (What the harness does with an instance that takes
               longer is tried in t02_harness_rules.py.)
VAL-INFRA-008  the port is inside the GUI range and nothing listened on it
               before the start.

Also the first look at the seed-drawing question: which tab is active once the
file is open, what `query Insert` returns, and what `xdata` reads back.
"""
import json
import os
import shutil
import sys
import time

import m6h

src = sys.argv[1] if len(sys.argv) > 1 else str(m6h.BASE_DWG)
r = m6h.Results("t00_smoke")


def clock(epoch):
    return time.strftime("%H:%M:%S", time.localtime(epoch)) if epoch else "—"


try:
    dwg = m6h.WORK / "smoke.dwg"
    for stale in m6h.WORK.glob("smoke.dwg*"):
        stale.unlink()
    for stale in m6h.WORK.glob(".smoke.dwg*"):
        stale.unlink()
    shutil.copyfile(src, dwg)
    g = m6h.Gui("smoke", 3210, dwg=dwg, collab=False).start()
    took = g.wait_ready(m6h.READY_BUDGET)
    r.check("harness", took is not None, f"GUI ready after {took}s (source {src})")
    in_time = took is not None and took <= m6h.READY_BUDGET
    again = (f"；第一次启动（端口 {g.retried['port']}）到上限没有就绪，按规则结束、留日志、换端口重试过一次：{g.retried['retry_result']}"
             if g.retried else "；没有用到重试")
    r.check("VAL-INFRA-014", in_time,
            f"从进程启动（{clock(g.launched)}）到 /api/v1/ready 首次返回 200（{clock(g.ready_at) if in_time else '没有等到'}）："
            f"{'%.1f' % took if took is not None else '—'} 秒，上限 {m6h.READY_BUDGET} 秒{again}")
    r.check("VAL-INFRA-001", in_time,
            f"以 {m6h.EXE_GUI.name} {' '.join(g.args())} 启动，轮询 GET /api/v1/ready {g.polls} 次，"
            f"启动后 {'%.1f' % took if took is not None else '—'} 秒首次得到 HTTP 200（上限 {m6h.READY_BUDGET} 秒）")
    if took is None:
        r.note(g.err_text()[-3000:])
        raise SystemExit(1)

    # ── VAL-INFRA-001: the five fields of the answer ────────────────────────────
    status, body = g.ready()
    body = body if isinstance(body, dict) else {}
    r.dump("t00-ready.json", body)
    fields = {
        "ok 为 true": body.get("ok") is True,
        "ready 为 true": body.get("ready") is True,
        "version 是非空字符串": isinstance(body.get("version"), str) and bool(body.get("version")),
        "session_id 是非空字符串": isinstance(body.get("session_id"), str) and bool(body.get("session_id")),
        "document_id 是整数": type(body.get("document_id")) is int,
    }
    wrong = [name for name, good in fields.items() if not good]
    r.check("VAL-INFRA-001", status == 200 and not wrong,
            f"GET /api/v1/ready → HTTP {status}，五个字段的值和类型（{'、'.join(fields)}）："
            f"{'都对' if not wrong else '不对的有 ' + '、'.join(wrong)}；响应体 {json.dumps(body, ensure_ascii=False)}")
    docs = (g.api("GET", "/documents")[1] or {}).get("documents") or []
    mine = [d for d in docs if d.get("path") and os.path.normcase(d["path"]) == os.path.normcase(str(dwg))]
    r.check("VAL-INFRA-001", len(mine) == 1 and type(body.get("document_id")) is int
            and mine[0].get("id") == body.get("document_id"),
            f"/api/v1/ready 给的 document_id（{body.get('document_id')!r}）就是命令行上那张图所在的文档：/api/v1/documents 里的文档 "
            f"{[(d.get('id'), d.get('title') or d.get('name'), bool(d.get('path'))) for d in docs]}（编号、标题、有没有路径）")

    # ── VAL-INFRA-008: the port ─────────────────────────────────────────────────
    low, high = m6h.gui_port_range()
    r.check("VAL-INFRA-008", low <= g.port <= high and bool(g.port_check) and g.port_check[-1] == [g.port, "free"],
            f"这个实例的 --http 端口 {g.port} 在 {low}–{high} 之内，启动前查过没有在监听（查的结果：{g.port_check}）")

    # Poll the state for a while: is the drawing the active tab right away?
    seen = []
    for _ in range(40):
        st = g.state()
        docs = st.get("documents") or (st.get("result") or {}).get("documents") or []
        snap = [(d.get("id"), d.get("title") or d.get("name"), bool(d.get("path")), d.get("active")) for d in docs]
        if not seen or seen[-1][1] != snap:
            seen.append((round(time.time() - g.launched, 1), snap))
        time.sleep(0.25)
    for at, snap in seen:
        r.note(f"t+{at}s documents: {snap}")
    st = g.state()
    r.dump("t00-state.json", st)
    r.note("state keys: " + ", ".join(sorted(st.keys()))[:400])

    doc = g.doc_id()
    r.note(f"document id with a path: {doc}")
    inserts = g.inserts()
    r.note(f"query Insert -> {len(inserts)} entities")
    r.dump("t00-inserts.json", inserts)
    for e in inserts[:12]:
        h = e.get("handle")
        status, body = g.xdata(h)
        r.note(f"insert handle={h} block={e.get('block') or e.get('name')} xdata status={status} body={json.dumps(body, ensure_ascii=False)[:300]}")
    status, resp, exists = g.capture(m6h.SHOTS / "t00-smoke.png")
    r.check("harness", status == 200 and exists, f"capture status={status} file={exists} keys={sorted((resp or {}).keys())[:12]}")
    r.dump("t00-capture.json", resp)
finally:
    m6h.cleanup()
    ok = r.summary()
sys.exit(0 if ok else 1)
