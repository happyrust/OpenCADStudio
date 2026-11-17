# -*- coding: utf-8 -*-
"""VAL-DRAW-031  2000 symbols placed in one batch have their tags within
              3000 ms: the median of three complete repetitions.

One GUI without collaboration. Each repetition starts in a new, empty drawing:
`pid_insert_batch` places 2000 vessels in one request (the contract's
clarification of 2026-11-07: one batch request, not 2000 single insertions),
and the clock runs from the moment the last `/ready` before that request was
answered to the moment the author record of the 2000th instance gives a tag
that is not empty. The clock is this script's monotonic one
(`time.perf_counter`), and the requests go through the harness like those of
every other scenario: against 3000 ms the few ms a client adds decide nothing,
and they count against the program, not for it.

When the clock has stopped the drawing is read back: 2000 entities carry a
PID_AUTHOR record, their 2000 tags are all different, the first is P-1001 and
the last P-3000.

What runs beside the timing is read off the process list as in
p01_insert_timing.py and said in the check; it does not change the verdict.
"""
import statistics
import sys
import time

import gh
import m7h
import ph
import yh

R = m7h.Results("p02_bulk_tags")
ITEM = "VAL-DRAW-031"
COUNT, REPEATS, LIMIT = 2000, 3, 3000.0     # the contract's: instances, repetitions, ms for the median
FIRST_TAG, LAST_TAG = "P-1001", "P-3000"
PATIENCE = 60.0                             # seconds a repetition waits for the last tag before it gives up


def tags_of(g, doc, handles):
    """The tag in the author record of each of `handles`, in their order."""
    body = yh.body_of(g.op("xdata_get", doc, handles=handles, app="PID_AUTHOR"))
    found = {}
    for item in body.get("items") or (body.get("result") or {}).get("items") or []:
        for value in (item.get("xdata") or {}).get("PID_AUTHOR") or []:
            text = value.get("value") if isinstance(value, dict) else value
            if isinstance(text, str) and text.startswith("tag="):
                found[item.get("handle")] = text[4:]
    return [found.get(handle) for handle in handles]


def repetition(g):
    """One complete repetition in a new drawing: what was placed, how long the
    last tag took, and what the drawing holds afterwards."""
    doc = ph.new_document(g)
    empty = gh.entity_count(g, doc)
    before = ph.surroundings([g.pid])
    ready = g.api("GET", "/ready")
    began = time.perf_counter()
    placed = g.op("pid_insert_batch", doc, symbol="vessel", count=COUNT, origin=[0, 0, 0], step=[25, 25, 0], columns=50)
    result = yh.body_of(placed).get("result") or {}
    handles = result.get("handles") or []
    tag, reads = None, 0
    while len(handles) == COUNT and not tag and time.perf_counter() - began < PATIENCE:
        reads += 1
        tag = g.author(handles[-1]).get("tag")
    took = (time.perf_counter() - began) * 1000.0 if tag else None
    after = ph.surroundings([g.pid])
    tags = tags_of(g, doc, handles) if handles else []
    return {"empty": empty, "ready": ready[0], "http": placed[0], "ok": yh.body_of(placed).get("ok"),
            "created": result.get("created"), "tagged": result.get("tagged"), "handles": len(handles),
            "reads": reads, "last_tag_read": tag, "ms": took,
            "authored": gh.entity_count(g, doc, xdata_app="PID_AUTHOR"), "distinct": len({t for t in tags if t}),
            "first": tags[0] if tags else None, "last": tags[-1] if tags else None,
            "beside": {"before": before, "after": after}}


try:
    dwg = m7h.fresh_copy("p02-bulk.dwg")
    g = gh.fresh_settings(m7h.Gui("p02", 3210, dwg=dwg, collab=False)).start()
    took = g.wait_ready(m7h.READY_BUDGET)
    R.check("setup", took is not None, f"一个不连协同的实例，启动后 {'%.1f' % took if took is not None else '—'} 秒就绪")
    if took is None:
        raise SystemExit(1)

    rows = [repetition(g) for _ in range(REPEATS)]
    R.dump("p02-timing.json", {"repetitions": rows, "limit_ms": LIMIT,
                               "clock": "time.perf_counter, from the answer of the last /ready before the batch to the first non-empty tag of the 2000th instance"})

    placed = all(row["empty"] == 0 and row["ready"] == 200 and row["http"] == 200 and row["ok"] is True and row["created"] == COUNT
                 and row["tagged"] == COUNT and row["handles"] == COUNT and bool(row["last_tag_read"]) for row in rows)
    R.check(ITEM, placed,
            f"{REPEATS} 次完整重复，各在一张新建的空图里（实体数 {[row['empty'] for row in rows]}）用一个 pid_insert_batch 请求放 {COUNT} 个 vessel："
            f"应答 HTTP {[row['http'] for row in rows]}，created {[row['created'] for row in rows]}、tagged {[row['tagged'] for row in rows]}、"
            f"handles {[row['handles'] for row in rows]} 个；读第 {COUNT} 个实例的作者记录，第 {[row['reads'] for row in rows]} 次读到位号 "
            f"{[row['last_tag_read'] for row in rows]}")

    taken = [row["ms"] for row in rows]
    median = statistics.median(taken) if all(v is not None for v in taken) else None
    readings = [reading for row in rows for reading in row["beside"].values()]
    most = {key: max(reading[key] for reading in readings) for key in readings[0]}
    clean = ph.quiet(*readings)
    over = "" if median is not None and median <= LIMIT else ("；量的时候环境是干净的" if clean else "；环境不干净，要等干净了重量一遍才算数")
    R.check(ITEM, median is not None and median <= LIMIT,
            f"从提交前最后一次 /ready 答回，到读到第 {COUNT} 个实例的非空位号，{REPEATS} 次依次 "
            f"{[round(v, 1) if v is not None else None for v in taken]} 毫秒，中位数 {'%.1f' % median if median is not None else '—'} 毫秒"
            f"（上限 {LIMIT:g}）；用的是脚本的单调时钟，不含程序的启动；每次前后各读一遍进程表，最多的一次是 {ph.told(most)}"
            f"（{'契约要的干净环境' if clean else '不是契约要的干净环境'}）{over}")

    R.check(ITEM, all(row["authored"] == COUNT and row["distinct"] == COUNT and row["first"] == FIRST_TAG and row["last"] == LAST_TAG
                      for row in rows),
            f"停表之后把每张图读回来：query xdata_app=PID_AUTHOR 的 count {[row['authored'] for row in rows]}（要 {COUNT}），"
            f"一次 xdata_get 读回全部 {COUNT} 个位号，去重后 {[row['distinct'] for row in rows]} 个（要 {COUNT}），"
            f"第一个 {[row['first'] for row in rows]}（要 {FIRST_TAG}）、第 {COUNT} 个 {[row['last'] for row in rows]}（要 {LAST_TAG}）")
except SystemExit:
    raise
except Exception as error:  # noqa: BLE001
    import traceback
    R.check("script", False, f"脚本异常: {error!r}\n{traceback.format_exc()}")
finally:
    m7h.cleanup()
    ok = R.summary()
sys.exit(0 if ok else 1)
