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
VAL-INFRA-009  `capture` writes a PNG file and says where, and how large the
               picture is.
VAL-INFRA-010  the same answer carries the matrix from the drawing to the
               picture and a screen coordinate for every entity it marks;
               for an entity whose coordinates are known the two agree, and
               the picture shows it where the matrix says.

Also the first look at the seed-drawing question: which tab is active once the
file is open, what `query Insert` returns, and what `xdata` reads back.
"""
import json
import os
import shutil
import sys
import time

import gh
import m6h
import shot

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

    # ── VAL-INFRA-009 / 010: what `capture` answers ─────────────────────────────────────
    # The entity whose coordinates are known is a line the scenario draws
    # itself, between two points of the view as it is (through the matrix of a
    # first picture), so that it is in the picture whatever drawing was given.
    def lines():
        listed = (g.op("query", doc, type="Line", detail="full")[1] or {}).get("entities") or []
        return [e.get("handle") for e in listed]

    def near(values, digits=2):
        return [round(v, digits) for v in values]

    view = gh.take(g, m6h.SHOTS / "t00-view.png", document_id=doc)
    ends, mine = [], []
    if view.good:
        wide, high = view.picture.width, view.picture.height
        ends = [view.world_of(0.35 * wide, 0.30 * high), view.world_of(0.65 * wide, 0.45 * high)]
        known = set(lines())
        g.run("LINE", doc)
        for x, y in ends:
            g.op("input", doc, kind="point", point=[x, y, 0.0], space="wcs")
        g.op("input", doc, kind="enter")
        mine = [h for h in lines() if h not in known]
    literal = g.api("POST", "/capture", {"document_id": doc, "annotate": True})
    refusal = literal[1].get("error") if isinstance(literal[1], dict) else str(literal[1])[:80]
    target = m6h.SHOTS / "t00-annotated.png"
    taken = gh.take(g, target, document_id=doc, annotate=True)
    answer = taken.answer
    r.dump("t00-annotated.json", taken.kept())

    named = answer.get("path")
    there = isinstance(named, str) and os.path.isfile(named)
    asked = there and os.path.samefile(named, target)
    head = shot.signature(named) if there else b""
    sizes = [answer.get("width"), answer.get("height")]
    r.check("VAL-INFRA-009",
            taken.good and asked and head == shot.PNG_MAGIC and all(type(v) is int and v > 0 for v in sizes)
            and sizes == [taken.picture.width, taken.picture.height],
            f"按契约原样（请求体只有 document_id 和 annotate）发 capture：HTTP {literal[0]}"
            f"{f'，{refusal!r}：程序要求请求里用 path 写明存到哪里' if literal[0] != 200 else ''}。带上 path 再发：{taken.told()}；"
            f"响应 ok={answer.get('ok')!r}，path 字段指向的文件{'存在' if there else '不存在'}"
            f"（{os.path.getsize(named) if there else '—'} 字节，{'就是' if asked else '不是'}请求里给的那个），"
            f"前 8 字节 {head.hex(' ') or '—'}（PNG 魔数 {shot.PNG_MAGIC.hex(' ')}），width={sizes[0]!r}、height={sizes[1]!r}"
            f"（要求是正整数；解码出的图 "
            f"{f'{taken.picture.width}x{taken.picture.height}' if taken.data is not None else '—'}）")

    matrix, notes = answer.get("world_to_pixel_matrix"), answer.get("annotations")
    six = (isinstance(matrix, list) and len(matrix) == 6
           and all(isinstance(v, (int, float)) and not isinstance(v, bool) for v in matrix))
    placed = isinstance(notes, list) and bool(notes) and all(
        isinstance(n.get("screen_pixel"), list) and len(n["screen_pixel"]) == 2 for n in notes)
    entry = taken.entity(mine[0], "annotations") if len(mine) == 1 else None
    middle = at = off = None
    if six and placed and entry:
        middle = [(ends[0][0] + ends[1][0]) / 2, (ends[0][1] + ends[1][1]) / 2]
        at = taken.pixel_of(*middle)
        off = [abs(at[0] - entry["screen_pixel"][0]), abs(at[1] - entry["screen_pixel"][1])]
    r.check("VAL-INFRA-010", off is not None and max(off) <= 20,
            f"同一个应答里 world_to_pixel_matrix={matrix}（{'是' if six else '不是'} 6 个数），annotations "
            f"{len(notes) if isinstance(notes, list) else '—'} 条，{'每条都有' if placed else '不是每条都有'} screen_pixel。"
            f"已知坐标的实体是本场景画的一条线 {[near(p, 4) for p in ends]}（句柄 {mine}，annotations 里{'有' if entry else '没有'}它）："
            f"中点 {near(middle, 4) if middle else '—'} 经矩阵换算为像素 {near(at) if at else '—'}，"
            f"annotations 给的 screen_pixel={entry.get('screen_pixel') if entry else '—'}，两个方向相差 {near(off) if off else '—'}（上限 20 像素）")

    after = gh.take(g, m6h.SHOTS / "t00-line.png", document_id=doc)
    drawn = want = None
    fits = False
    if (view.good and after.good and len(ends) == 2 and gh.same_camera(view, after)
            and (view.picture.width, view.picture.height) == (after.picture.width, after.picture.height)):
        a, b = after.pixel_of(*ends[0]), after.pixel_of(*ends[1])
        want = (min(a[0], b[0]), min(a[1], b[1]), max(a[0], b[0]), max(a[1], b[1]))
        drawn = shot.changed(view.picture, after.picture, view.clear())
        if drawn.count:
            sides = (drawn.left, drawn.top, drawn.right - 1, drawn.bottom - 1)
            fits = all(abs(p - q) <= 3 for p, q in zip(sides, want))
    r.check("VAL-INFRA-010", fits,
            f"矩阵对得上图：画线前后各取一张不带标记的图（{view.told()}；{after.told()}），命令行面板以上两张图之差是 "
            f"{drawn if drawn is not None else '—（两张图不能比：没有取到，或镜头、大小不同）'}；按矩阵算，线的两端所围的框是 "
            f"{near(want, 1) if want else '—'}（左、上、右、下）；差的外接框与它各边相差不超过 3 像素：{'是' if fits else '否'}")
finally:
    m6h.cleanup()
    ok = r.summary()
sys.exit(0 if ok else 1)
