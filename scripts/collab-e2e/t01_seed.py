# -*- coding: utf-8 -*-
"""Build the seed drawing the two-instance scenarios open, and settle the
question the mission's probe stopped on: does a symbol's PID_AUTHOR survive a
save and a reopen?

1. open a copy of the base drawing, insert one vessel (it gets P-1001);
2. read its PID_AUTHOR back, save the drawing under a new name, quit;
3. open the saved file in a fresh instance and read PID_AUTHOR back again.

The seed is `work/PID-1101-seed.dwg`.
"""
import json
import shutil
import sys

import m6h

r = m6h.Results("t01_seed")
ok = False
try:
    src = m6h.WORK / "seed-src.dwg"
    seed = m6h.WORK / "PID-1101-seed.dwg"
    for p in list(m6h.WORK.glob("seed-src.dwg*")) + list(m6h.WORK.glob(".seed-src.dwg*")) + \
            list(m6h.WORK.glob("PID-1101-seed.dwg*")) + list(m6h.WORK.glob(".PID-1101-seed.dwg*")):
        p.unlink()
    shutil.copyfile(m6h.BASE_DWG, src)

    g = m6h.Gui("seed-make", 3230, dwg=src, collab=False).start()
    if g.wait_ready(180) is None:
        r.check("seed", False, "GUI 没有就绪")
        raise SystemExit(1)
    doc = g.doc_id()
    before = g.inserts()
    r.note(f"基础图里原有 Insert {len(before)} 个")
    status, body = g.op("pid_insert_batch", symbol="vessel", count=1, origin=[200, 150, 0], step=[100, 0, 0])
    r.note(f"pid_insert_batch -> {status} {json.dumps(body, ensure_ascii=False)[:300]}")
    tagged = g.tagged()
    r.check("seed", "P-1001" in tagged, f"插入后带位号的符号: {tagged}")
    handle = tagged.get("P-1001")
    status, body = g.xdata(handle)
    r.dump("t01-xdata-before-save.json", body)
    r.note(f"保存前 GET /entities/{handle}/xdata?app=PID_AUTHOR -> {json.dumps(body, ensure_ascii=False)}")

    status, body = g.api("POST", "/save", {"document_id": doc, "path": str(seed)})
    r.check("seed", status == 200 and (body or {}).get("ok") is True and seed.exists(),
            f"save -> {status} ok={(body or {}).get('ok')} file={seed.exists()} size={seed.stat().st_size if seed.exists() else 0}")
    g.kill()

    g2 = m6h.Gui("seed-reopen", 3231, dwg=seed, collab=False).start()
    if g2.wait_ready(180) is None:
        r.check("seed", False, "重开的 GUI 没有就绪")
        raise SystemExit(1)
    tagged2 = g2.tagged()
    status, body = g2.xdata(handle)
    r.dump("t01-xdata-after-reopen.json", body)
    r.note(f"重开后 GET /entities/{handle}/xdata?app=PID_AUTHOR -> {json.dumps(body, ensure_ascii=False)}")
    author = g2.author(handle)
    ok = r.check("seed", tagged2.get("P-1001") == handle and author.get("tag") == "P-1001",
                 f"重开后同一句柄 {handle} 的 PID_AUTHOR.tag = {author.get('tag')!r}（带位号的符号: {tagged2}）")
    g2.kill()
    # The reopened instance leaves a lock beside the seed; the scenarios copy
    # the seed, so only the drawing itself has to stay.
    for p in list(m6h.WORK.glob(".PID-1101-seed.dwg*")) + list(m6h.WORK.glob("PID-1101-seed.dwg.ocs*")):
        p.unlink()
finally:
    m6h.cleanup()
    r.summary()
sys.exit(0 if ok else 1)
