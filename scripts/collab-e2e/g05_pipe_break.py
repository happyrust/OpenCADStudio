# -*- coding: utf-8 -*-
"""VAL-PIPE-017  a valve put on a run breaks it, and the picture shows the
              break: the two segments and the valve follow one another in the
              answer of `capture`, and between each segment and the valve the
              picture has pixels with nothing drawn.

One GUI without collaboration on a fresh copy of the base drawing. A run
`L-1001-01` from (0, 0) to (200, 0) gets a gate valve at (100, 0), as the
contract says.

The pipe line the run belongs to is drawn elsewhere, from (0, 60) to (200, 60).
A run hangs on its line by tag, not by place, and the line's own entity is
not split by the valve: drawn where the run is (as y03_pipe_topology.py does,
which judges the topology and not the picture), it lies under the run from end
to end and shows through the two gaps. What the picture looks like then is
written down at the end as a note, not as a check.

The list of entities in the answer (`annotations`, the same as
`visible_entities`) holds 64 at most: the selected ones first, then by the area
of their `screen_bounds`, largest first. A horizontal segment has no area, so
with the base drawing in view it can fall off the end of the list. The request
as the contract spells it (only the valve highlighted) is therefore judged by
the valve alone, and the order of the three is read from a second picture of
the same view in which the two segments are highlighted as well.
"""
import math
import sys

import gh
import m7h
import shot
import yh

R = m7h.Results("g05_pipe_break")
BREAK = "VAL-PIPE-017"
LINE = [(0, 60), (200, 60)]
RUN = [(0, 0), (200, 0)]
VALVE = (100, 0)
UNDER = -80                 # where the second run is drawn, on top of its own line, for the note


def parts(gui):
    """The topology table: lines, runs, segments (left to right), fittings."""
    raw = yh.snapshot(gui).get("raw_topology") or {}
    segments = sorted(raw.get("pipe_segments") or [], key=lambda s: s["start"]["point"][0])
    return raw.get("pipe_lines") or [], raw.get("pipe_runs") or [], segments, raw.get("fittings") or []


def row_of(view, y, x0, x1, ground):
    """The stretches with something drawn along the drawing's horizontal `y`,
    between `x0` and `x1`: the row of pixels the line is on and the one above
    and below it (a line one pixel wide is spread over two rows)."""
    left, row = view.pixel_of(x0, y)
    right, _ = view.pixel_of(x1, y)
    row = int(round(row))
    band = (math.floor(left), row - 1, math.ceil(right) + 1, row + 2)
    return band, shot.stretches(view.picture, band, ground, gh.GRID)


try:
    dwg = m7h.fresh_copy("g05-pipe-break.dwg")
    g = gh.fresh_settings(m7h.Gui("g05", 3210, dwg=dwg, collab=False)).start()
    took = g.wait_ready(m7h.READY_BUDGET)
    R.check("setup", took is not None,
            f"一个不连协同的实例，启动后 {'%.1f' % took if took is not None else '—'} 秒就绪（基准图的副本 {dwg.name}；本实例的设置目录先清掉，程序从默认布局起步）")
    if took is None:
        raise SystemExit(1)
    woke = gh.wake(g, m7h.SHOTS / "g05-wake.png")
    R.note(f"画面跟得上截图（capture 应答 waited_for_frame=true）：就绪后 "
           f"{'%.1f 秒' % woke if woke is not None else f'{gh.WAKE:g} 秒内没有等到，下面每张图靠重发来确认'}")
    doc = g.doc_id()

    # ── the run and its valve ──
    done = [yh.ok(yh.draw(g, cmd, points, doc))
            for cmd, points in (("PIDPIPE", LINE), ("PIDPIPE run", RUN), ("PIDFITTING gate_valve", [VALVE]))]
    lines, runs, segments, fittings = parts(g)
    R.dump("g05-topology.json", {"pipe_lines": lines, "pipe_runs": runs, "pipe_segments": segments, "fittings": fittings})
    made = (all(done) and len(lines) == 1 and len(runs) == 1 and len(segments) == 2 and len(fittings) == 1
            and sorted(runs[0].get("segments") or []) == sorted(s["handle"] for s in segments)
            and fittings[0].get("fitting_type") == "gate_valve")
    ends = [(s["start"]["point"], s["end"]["point"]) for s in segments]
    R.check("setup", made,
            f"PIDPIPE {LINE}、PIDPIPE run {RUN}、PIDFITTING gate_valve {VALVE} 三步{'都成了' if all(done) else f'的结果 {done}'}："
            f"管线 {[x.get('tag') for x in lines]}，管路 {[x.get('tag') for x in runs]}，管段端点 {ends}，"
            f"阀门 {[(x.get('tag'), x.get('fitting_type')) for x in fittings]}")
    if not made:
        raise SystemExit(1)
    first, second = segments
    valve = fittings[0]["handle"]
    a_end, b_start = first["end"]["point"], second["start"]["point"]
    a_mid = (first["start"]["point"][0] + a_end[0]) / 2
    b_mid = (b_start[0] + second["end"]["point"][0]) / 2

    # ── the request as the contract spells it ──
    three = (first["handle"], valve, second["handle"])
    spelled = gh.take(g, m7h.SHOTS / "g05-annotated.png", view="extents", highlight_handles=[valve], annotate=True,
                      max_dimension=4096)
    R.dump("g05-annotated.json", spelled.kept())
    listed = [spelled.entity(h, "annotations") for h in three]
    total = len(spelled.answer.get("annotations") or [])
    R.check(BREAK, spelled.good and spelled.answer.get("annotated") is True and bool(listed[1]) and listed[1].get("selected") is True,
            f"capture view=extents、highlight_handles=[阀门]、annotate=true（另加 max_dimension=4096，图不缩小）：{spelled.told()}；"
            f"annotated={spelled.answer.get('annotated')!r}，阀门在 annotations 里且 selected="
            f"{listed[1].get('selected') if listed[1] else None!r}；annotations 共 {total} 条（程序最多给 64 条：选中的在前，"
            f"其余按屏幕面积从大到小），段 A {'在' if listed[0] else '不在'}其中、段 B {'在' if listed[2] else '不在'}其中")

    # ── the answer: the three follow one another ──
    marked = gh.take(g, m7h.SHOTS / "g05-annotated-three.png", view="extents", highlight_handles=list(three), annotate=True,
                     max_dimension=4096)
    R.dump("g05-annotated-three.json", marked.kept())
    noted = [marked.entity(h, "annotations") for h in three]
    boxes = [n.get("screen_bounds") if n else None for n in noted]
    in_order = (all(boxes) and boxes[0][2] <= boxes[1][0] + 2 and boxes[1][2] <= boxes[2][0] + 2)
    R.check(BREAK, marked.good and gh.same_camera(spelled, marked) and in_order,
            f"同一个镜头再取一张，把两个管段也放进 highlight_handles（选中的排在列表最前，不会被 64 条的上限截掉）：{marked.told()}；"
            f"annotations 里段 A、阀门、段 B 的 screen_bounds 依次为 {boxes}：段 A 的右缘 ≤ 阀门的左缘、阀门的右缘 ≤ 段 B 的左缘"
            f"（各容 2 像素）{'成立' if in_order else '不成立'}")

    # ── the picture: nothing is drawn between a segment and the valve ──
    plain = gh.take(g, m7h.SHOTS / "g05-plain.png", view="extents", highlight_handles=[valve], max_dimension=4096)
    R.dump("g05-plain.json", plain.kept())
    if plain.good and marked.good and gh.same_camera(marked, plain) and in_order:
        clear = plain.clear()
        ground, _ = shot.commonest(plain.picture, clear)
        band, found = row_of(plain, RUN[0][1], a_mid, b_mid, ground)
        gaps = [(found[i][1], found[i + 1][0]) for i in range(len(found) - 1)]
        x_a, x_b = plain.pixel_of(*a_end)[0], plain.pixel_of(*b_start)[0]
        broken = (len(found) == 3 and all(right - left >= 1 for left, right in gaps)
                  and abs(gaps[0][0] - x_a) <= 3 and abs(gaps[1][1] - x_b) <= 3
                  and boxes[1][0] - 2 <= found[1][0] and found[1][1] <= boxes[1][2] + 3)
        R.check(BREAK, broken and gh.within((band[0], band[1]), clear) and gh.within((band[2] - 1, band[3] - 1), clear),
                f"同一个镜头、只高亮阀门、不带编号标记的图（{plain.told()}）里，沿管路所在的横线 y={RUN[0][1]}（第 {band[1] + 1} 行及上下各一行）"
                f"从段 A 中点到段 B 中点（第 {band[0]}–{band[2] - 1} 列），画了东西的列段为 {found}：恰好三段（段 A、阀门自带的横线、段 B），"
                f"中间两道缝 {gaps} 宽 {[right - left for left, right in gaps]} 像素，都是底色 {ground}；第一道缝从段 A 的终点"
                f"（{a_end} → 第 {x_a:.1f} 列）起，第二道缝到段 B 的起点（{b_start} → 第 {x_b:.1f} 列）止，各容 3 像素；"
                f"按 screen_bounds 算的两道缝宽 {[boxes[1][0] - boxes[0][2], boxes[2][0] - boxes[1][2]]} 像素；每单位 {plain.per_unit():.2f} 像素")
    else:
        R.check(BREAK, False, f"没有可用来数像素的图：{plain.told()}；与带标记的那张矩阵"
                f"{'相同' if marked.good and plain.good and gh.same_camera(marked, plain) else '不同或缺图'}")

    # ── a note: the same with the line under its run ──
    g.op("select", doc, handles=[])
    under = [(0, UNDER), (200, UNDER)]
    again = [yh.ok(yh.draw(g, cmd, points, doc))
             for cmd, points in (("PIDPIPE", under), ("PIDPIPE run", under), ("PIDFITTING gate_valve", [(VALVE[0], UNDER)]))]
    lines2, runs2, segments2, fittings2 = parts(g)
    lower = [s for s in segments2 if abs(s["start"]["point"][1] - UNDER) < 1e-6]
    valve2 = next((f["handle"] for f in fittings2 if f["handle"] != valve), None)
    if all(again) and len(lower) == 2 and valve2:
        near = gh.take(g, m7h.SHOTS / "g05-line-under-run.png", highlight_handles=[valve2], max_dimension=4096,
                       bounds=[VALVE[0] - 60, UNDER - 40, VALVE[0] + 60, UNDER + 40])
        if near.good:
            ground2, _ = shot.commonest(near.picture, near.clear())
            x0 = (lower[0]["start"]["point"][0] + lower[0]["end"]["point"][0]) / 2
            x1 = (lower[1]["start"]["point"][0] + lower[1]["end"]["point"][0]) / 2
            band2, found2 = row_of(near, UNDER, max(x0, VALVE[0] - 55), min(x1, VALVE[0] + 55), ground2)
            reading = ("从头到尾连成一段，看不出断开：管线自己的那根线不随阀门拆开，压在管路下面时把两道缝补上了"
                       if len(found2) == 1 else f"分成 {len(found2)} 段")
            R.note(f"备注（不算检查）：另画一组，管线与管路都在 {under}、阀门在 {(VALVE[0], UNDER)}，拓扑里同样是两段 "
                   f"{[(s['start']['point'], s['end']['point']) for s in lower]}；图 shots/{near.path.name} 里沿 y={UNDER} 画了东西的列段为 "
                   f"{found2}（第 {band2[0]}–{band2[2] - 1} 列）：{reading}")
        else:
            R.note(f"备注（不算检查）：管线压在管路下面的那一组没有取到可用的图：{near.told()}")
    else:
        R.note(f"备注（不算检查）：管线压在管路下面的那一组没有画成（三步结果 {again}，该处管段 {len(lower)} 个），这条备注从缺")
except SystemExit:
    raise
except Exception as error:  # noqa: BLE001
    import traceback
    R.check("script", False, f"脚本异常: {error!r}\n{traceback.format_exc()}")
finally:
    m7h.cleanup()
    ok = R.summary()
sys.exit(0 if ok else 1)
