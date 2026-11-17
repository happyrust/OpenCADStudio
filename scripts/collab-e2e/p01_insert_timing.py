# -*- coding: utf-8 -*-
"""VAL-DRAW-030  on a drawing of 5000 lines, the median of 20 insertions is at
              most 100 ms.
VAL-DRAW-032  with 200 P&ID symbols in that drawing as well, the median of 20
              more is at most 100 ms, and not more than twice the first.

One GUI without collaboration. In a new, empty drawing 5000 `Line` entities
are made with `entities_create`. An insertion is the contract's three
requests: `PIDINSERT vessel`, a point, Enter. Each of the three is sent by
curl.exe and timed by curl itself (`time_total`), as the contract says; the
sample of an insertion is the sum of its three times. Nothing else is in a
sample: not the start of the program, not the start of curl, not what this
script does between two requests.

Before each of the timed requests the scenario waits PAUSE seconds. After an
answer the program is busy for some tens of milliseconds (about 40 where this
was written, whatever the request and however large the drawing), and a
request sent into that waits for it. The script the contract's own figures
were taken with left such a gap without saying so (PowerShell needed the time
to start curl), and a person inserting a symbol leaves a longer one. What the
three requests take when each is sent right after the answer to the one before
is measured as well, in a third series, and noted, not judged: the contract
sets no limit for that.

Between the two series more symbols are inserted, every symbol of the library
in turn, until the drawing holds the contract's 200 with the 20 of the first
series.

The contract counts a timing when no compiler runs and no other instance of
the two programs does. The scenario reads that off the process list before
and after each series and says it in the check. It does not change the
verdict: a median within the limit on a busy machine is within it on a quiet
one; a median over the limit on a busy machine fails, and the check says that
the timing has to be taken again on a quiet machine before it counts.
"""
import collections
import statistics
import sys
import time

import gh
import m7h
import ph
import yh

R = m7h.Results("p01_insert_timing")
FIRST, SECOND = "VAL-DRAW-030", "VAL-DRAW-032"
LINES, SAMPLES, SYMBOLS = 5000, 20, 200
LIMIT, GROWTH = 100.0, 100.0        # the contract's: ms for the median of a series; percent the second may be over the first
DELIVERED = "28.7 ms / 31.7 ms"     # what the contract records as measured at delivery
PAUSE = 0.2                         # seconds before each timed request: see the top of the file


def timed_after(g, body, pause):
    if pause:
        time.sleep(pause)
    return ph.timed_post(g, body)


def series(g, doc, y, pause=PAUSE):
    """SAMPLES insertions along one row, every request timed by curl. Returns
    what each request gave, and what ran beside the series before and after."""
    before = ph.surroundings([g.pid])
    rows = []
    for k in range(SAMPLES):
        at = [300.0 + 25.0 * k, y, 0.0]
        rows.append([timed_after(g, body, pause) for body in (
            {"document_id": doc, "cmd": "PIDINSERT vessel"},
            {"document_id": doc, "op": "input", "kind": "point", "point": at, "space": "wcs"},
            {"document_id": doc, "op": "input", "kind": "enter"})])
    return rows, before, ph.surroundings([g.pid])


def samples(rows):
    """One figure per insertion: its three `time_total` added up, in ms."""
    return [sum(part[0] for part in row) * 1000.0 for row in rows]


def judged(item, rows, before, after, counts):
    """The two checks of a series: every insertion went through; the median."""
    bad = [k + 1 for k, row in enumerate(rows)
           if not all(part[1] == 200 and (part[2] or {}).get("ok") is True for part in row)]
    (authored0, total0), (authored1, total1) = counts
    R.check(item, not bad and authored1 - authored0 == SAMPLES,
            f"连做 {SAMPLES} 次插入，每次三个请求（PIDINSERT vessel、给点、回车）都由 curl.exe 发："
            f"{'每个请求都答 200 且 ok' if not bad else f'第 {bad} 次有请求没有答 200 且 ok'}；"
            f"带作者记录的实体 {authored0} → {authored1}，实体数 {total0} → {total1}")
    taken = samples(rows)
    median = statistics.median(taken)
    clean = ph.quiet(before, after)
    over = "" if median <= LIMIT else ("；量的时候环境是干净的" if clean else "；环境不干净，按契约要等干净了重量一遍才算数")
    R.check(item, median <= LIMIT,
            f"这 {SAMPLES} 次的中位数 {median:.1f} 毫秒（上限 {LIMIT:g}）：每次的样本是三次往返各自的 time_total 之和"
            f"（curl 自己量的，不含程序和 curl 的启动），每个请求发出前停 {PAUSE:g} 秒（程序答完一个请求后有几十毫秒在忙，等它空下来再发）；"
            f"样本依次 {[round(v, 1) for v in taken]} 毫秒，"
            f"最小 {min(taken):.1f}、最大 {max(taken):.1f}；{ph.beside(before, after)}；契约记的交付基线是 {DELIVERED}{over}")
    return median


def counts(g, doc):
    return gh.entity_count(g, doc, xdata_app="PID_AUTHOR"), gh.entity_count(g, doc)


def kept(rows, before, after):
    """A series as it goes into the evidence file."""
    taken = samples(rows)
    return {"samples_ms": [round(v, 3) for v in taken], "sorted_ms": sorted(round(v, 3) for v in taken),
            "median_ms": round(statistics.median(taken), 3),
            "time_total_ms": [[round(part[0] * 1000.0, 3) for part in row] for row in rows],
            "http": [[part[1] for part in row] for row in rows], "beside": {"before": before, "after": after}}


try:
    if not ph.CURL:
        R.check(FIRST, False, "curl.exe 不在 PATH 里：契约用它的 time_total 计时，没有它这两条量不了")
        raise SystemExit(1)
    library = gh.library(m7h.REPO)
    kinds = [(category["id"], item["id"]) for category in library["categories"] for item in category["symbols"]]
    labels = {category["id"]: category.get("label") or category["id"] for category in library["categories"]}

    dwg = m7h.fresh_copy("p01-timing.dwg")
    g = gh.fresh_settings(m7h.Gui("p01", 3210, dwg=dwg, collab=False)).start()
    took = g.wait_ready(m7h.READY_BUDGET)
    R.check("setup", took is not None,
            f"一个不连协同的实例，启动后 {'%.1f' % took if took is not None else '—'} 秒就绪；计时用的是 {ph.curl_version()}")
    if took is None:
        raise SystemExit(1)

    # ── the base: a new, empty drawing with 5000 lines ──
    doc = ph.new_document(g)
    empty = gh.entity_count(g, doc)
    refused = None
    for first in range(0, LINES, 1000):
        batch = [{"type": "Line", "start": [(i // 100) * 2.0, (i % 100) * 2.0], "end": [(i // 100) * 2.0 + 1.5, (i % 100) * 2.0 + 1.0]}
                 for i in range(first, min(first + 1000, LINES))]
        answer = g.op("entities_create", doc, entities=batch)
        if not yh.ok(answer):
            refused = f"第 {first // 1000 + 1} 批答 {answer[0]}：{str(answer[1])[:200]}"
            break
    lines, total = gh.entity_count(g, doc, type="Line"), gh.entity_count(g, doc)
    R.check(FIRST, empty == 0 and refused is None and lines == LINES and total == LINES,
            f"底图：新建一张空图（实体数 {empty}），用 entities_create 分 {LINES // 1000} 批建出 {LINES} 条线"
            f"（{refused or '每批都成了'}）；query 的 count 是 {total}，其中 Line {lines} 条")
    if lines != LINES or total != LINES:
        raise SystemExit(1)

    # ── VAL-DRAW-030: 20 insertions on it ──
    at_start = counts(g, doc)
    rows1, before1, after1 = series(g, doc, 300.0)
    after_first = counts(g, doc)
    first = judged(FIRST, rows1, before1, after1, (at_start, after_first))

    # ── VAL-DRAW-032: 200 symbols in the drawing, then 20 more insertions ──
    more = SYMBOLS - (after_first[0] or 0)
    failed = None
    for k in range(more):
        name = kinds[k % len(kinds)][1]
        last = yh.draw(g, "PIDINSERT " + name, [(300.0 + 25.0 * (k % 20), 600.0 + 40.0 * (k // 20))], doc)
        if not yh.ok(last):
            failed = f"第 {k + 1} 个（{name}）答 {last[0]}：{str(last[1])[:200]}"
            break
    listing = g.op("query", doc, type="Insert", detail="full")
    inserts = yh.body_of(listing).get("entities") or (yh.body_of(listing).get("result") or {}).get("entities") or []
    per = collections.Counter(str(e.get("block") or "")[len("PID_SYM_"):].split("_", 1)[0]
                              for e in inserts if str(e.get("block") or "").startswith("PID_SYM_"))
    before_second = counts(g, doc)
    R.check(SECOND, failed is None and before_second[0] == SYMBOLS and all(per.get(c, 0) > 0 for c in labels)
            and gh.entity_count(g, doc, type="Line") == LINES,
            f"两组之间再插入 {more} 个符号（符号库的 {len(kinds)} 种轮着来；{failed or '都成了'}），连同第一组的 {SAMPLES} 个，"
            f"图里带作者记录的实体是 {before_second[0]} 个（要 {SYMBOLS}）："
            f"{'、'.join(f'{labels[c]}类 {per.get(c, 0)} 个' for c in labels)}；底图的线仍是 {LINES} 条，实体数 {before_second[1]}")
    rows2, before2, after2 = series(g, doc, 1200.0)
    second = judged(SECOND, rows2, before2, after2, (before_second, counts(g, doc)))
    growth = (second - first) / first * 100.0 if first > 0 else float("inf")
    R.check(SECOND, growth <= GROWTH,
            f"第二组的中位数 {second:.1f} 毫秒，比第一组的 {first:.1f} 毫秒{'增长' if growth >= 0 else '减少'} {abs(growth):.1f}%"
            f"（增长的上限 {GROWTH:g}%）：图里的 P&ID 实体从 {at_start[0]} 个到 {before_second[0]} 个，插入没有跟着成倍变慢")

    # ── noted, not judged: the same without the pause ──
    rows3, before3, after3 = series(g, doc, 1600.0, pause=0)
    close = samples(rows3)
    hurried = statistics.median(close)
    R.note(f"不判，只记：同样 {SAMPLES} 次插入，请求前不停、一个紧接一个发，中位数 {hurried:.1f} 毫秒（停 {PAUSE:g} 秒再发的两组是 {first:.1f} 和 {second:.1f}），"
           f"样本依次 {[round(v, 1) for v in close]} 毫秒；多出来的部分，是请求赶在程序答完上一个请求还没空下来的时候到、等出来的，契约没有给这种发法定上限")
    R.dump("p01-timing.json", {
        "timed_with": ph.curl_version(), "sample": "sum of curl's time_total of the three requests of one insertion",
        "pause_before_each_request_s": PAUSE, "base": {"lines": lines, "count": total},
        FIRST: kept(rows1, before1, after1), SECOND: dict(kept(rows2, before2, after2), symbols_before=before_second[0]),
        "without_pause_not_judged": kept(rows3, before3, after3),
        "growth_percent": round(growth, 2), "limit_ms": LIMIT, "growth_limit_percent": GROWTH})
except SystemExit:
    raise
except Exception as error:  # noqa: BLE001
    import traceback
    R.check("script", False, f"脚本异常: {error!r}\n{traceback.format_exc()}")
finally:
    m7h.cleanup()
    ok = R.summary()
sys.exit(0 if ok else 1)
