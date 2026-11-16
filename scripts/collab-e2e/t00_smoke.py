# -*- coding: utf-8 -*-
"""Smoke test of the harness: one GUI on the hidden desktop, no collaboration.

Also the first look at the seed-drawing question: which tab is active once the
file is open, what `query Insert` returns, and what `xdata` reads back.
"""
import json
import sys
import time

import m6h

src = sys.argv[1] if len(sys.argv) > 1 else str(m6h.BASE_DWG)
r = m6h.Results("t00_smoke")
try:
    dwg = m6h.WORK / "smoke.dwg"
    import shutil
    for stale in m6h.WORK.glob("smoke.dwg*"):
        stale.unlink()
    for stale in m6h.WORK.glob(".smoke.dwg*"):
        stale.unlink()
    shutil.copyfile(src, dwg)
    g = m6h.Gui("smoke", 3230, dwg=dwg, collab=False).start()
    took = g.wait_ready(180)
    r.check("harness", took is not None, f"GUI ready after {took}s (source {src})")
    if took is None:
        r.note(g.err_text()[-3000:])
        raise SystemExit(1)

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
    r.summary()
