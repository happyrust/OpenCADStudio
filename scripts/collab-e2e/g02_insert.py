# -*- coding: utf-8 -*-
"""VAL-DRAW-009  a symbol is where it was put: by its record, and by the
              picture taken with `focus_handles`.
VAL-DRAW-010  it is drawn, and a capture with `highlight_handles` draws it
              selected.

One GUI without collaboration on a fresh copy of the base drawing. A vessel
is inserted at (200, 150) with `PIDINSERT vessel`. Where its outline has to
be is taken from the symbol library (the closed polyline of the symbol,
around the insertion point), so the pictures are judged at places computed
from the drawing, through the matrix of the answer.

The pictures the pixels are counted in are taken with a camera the scenario
sets (`bounds`), which keeps the symbol above the command line panel; the
picture with `focus_handles` fills the viewport with the symbol, so its lower
side may lie under that panel, and is judged at the sides that do not.
"""
import math
import sys

import gh
import m7h
import shot
import yh

R = m7h.Results("g02_insert")
PLACE, DRAWN = "VAL-DRAW-009", "VAL-DRAW-010"
AT = (200.0, 150.0)
WINDOW = [AT[0] - 50, AT[1] - 40, AT[0] + 50, AT[1] + 40]      # the camera of the pictures the pixels are counted in
AROUND = 40         # px around the symbol's `screen_bounds` that still belong to it: the tag is drawn a little wider than reported


def rounded(values, digits=2):
    return [round(v, digits) for v in values]


try:
    vessel = gh.symbol(gh.library(m7h.REPO), "vessel")
    outline = next(part["points"] for part in vessel["geometry"] if part["kind"] == "polyline" and part.get("closed"))
    corners = [(AT[0] + x, AT[1] + y) for x, y in outline]
    sides = list(zip(corners, corners[1:] + corners[:1]))
    middles = [((ax + bx) / 2, (ay + by) / 2) for (ax, ay), (bx, by) in sides]     # on the outline
    hollow = [(AT[0] + x / 2, AT[1] + y / 2) for x, y in outline]                  # inside it, where nothing is drawn
    length = sum(math.dist(a, b) for a, b in sides)

    dwg = m7h.fresh_copy("g02-insert.dwg")
    g = gh.fresh_settings(m7h.Gui("g02", 3210, dwg=dwg, collab=False)).start()
    took = g.wait_ready(m7h.READY_BUDGET)
    R.check("setup", took is not None,
            f"一个不连协同的实例，启动后 {'%.1f' % took if took is not None else '—'} 秒就绪（基准图的副本 {dwg.name}；本实例的设置目录先清掉，程序从默认布局起步）")
    if took is None:
        raise SystemExit(1)
    woke = gh.wake(g, m7h.SHOTS / "g02-wake.png")
    R.note(f"画面跟得上截图（capture 应答 waited_for_frame=true）：就绪后 "
           f"{'%.1f 秒' % woke if woke is not None else f'{gh.WAKE:g} 秒内没有等到，下面每张图靠重发来确认'}")
    doc = g.doc_id()
    before = gh.entity_count(g, doc)

    # ── VAL-DRAW-009 (1): the record ──
    placed = yh.draw(g, "PIDINSERT vessel", [AT], doc)
    found = gh.symbols(g)
    R.dump("g02-symbols.json", found)
    one = found[0] if len(found) == 1 else {}
    handle = one.get("handle")
    position = (one.get("position") or [math.inf, math.inf])[:2]
    off = [abs(position[0] - AT[0]), abs(position[1] - AT[1])]
    R.check(PLACE, yh.ok(placed) and len(found) == 1 and one.get("block") == "PID_SYM_equipment_vessel" and max(off) <= 0.5,
            f"PIDINSERT vessel 点在 {AT}：图里多出 {len(found)} 个符号，块名 {one.get('block')}，记录的 position={position}，"
            f"与插入点各坐标相差 {rounded(off, 4)}（上限 0.5）；实体数 {before} → {gh.entity_count(g, doc)}")
    if not handle:
        raise SystemExit(1)

    # ── VAL-DRAW-009 (2): the picture with focus_handles ──
    focus = gh.take(g, m7h.SHOTS / "g02-focus.png", focus_handles=[handle], max_dimension=4096)
    R.dump("g02-focus.json", focus.kept())
    seen = focus.entity(handle) or {}
    gap = None
    if focus.good and seen.get("screen_pixel") and seen.get("world_center"):
        back = focus.world_of(*seen["screen_pixel"][:2])
        gap = math.dist(back, seen["world_center"][:2])
    R.check(PLACE, gap is not None and gap <= 0.5,
            f"capture focus_handles=[该符号]：{focus.told()}；visible_entities 里该符号的 screen_pixel={seen.get('screen_pixel')} "
            f"经 pixel_to_world_matrix 反算得 {rounded(back, 3) if gap is not None else '—'}，与 world_center={seen.get('world_center')} "
            f"相距 {'%.4f' % gap if gap is not None else '—'} 个单位（上限 0.5）")
    if focus.good:
        clear = focus.clear()
        ground, _ = shot.commonest(focus.picture, clear)
        in_view = [p for p in middles if gh.within(focus.pixel_of(*p), clear, 4)]
        missing = [p for p in in_view if not gh.drawn_near(focus.picture, focus.pixel_of(*p), ground)]
        filled = [p for p in hollow if gh.within(focus.pixel_of(*p), clear, 4) and gh.drawn_near(focus.picture, focus.pixel_of(*p), ground)]
        R.check(PLACE, len(in_view) >= 3 and not missing and not filled,
                f"同一张图里，按插入点和符号库算出的外框四边中点 {middles} 经 world_to_pixel_matrix 换成像素："
                f"落在命令行面板以上的 {len(in_view)} 个（至少要 3 个）处都画着线（没画的：{missing or '无'}）；"
                f"框内空白处 {hollow} 都是底色 {ground}（不是的：{filled or '无'}）")
    else:
        R.check(PLACE, False, f"没有可用来数像素的图：{focus.told()}")

    # ── VAL-DRAW-010: without the highlight, with it, without it again ──
    plain = gh.take(g, m7h.SHOTS / "g02-plain.png", bounds=WINDOW, max_dimension=4096)
    lit = gh.take(g, m7h.SHOTS / "g02-highlight.png", bounds=WINDOW, highlight_handles=[handle], max_dimension=4096)
    R.dump("g02-plain.json", plain.kept())
    R.dump("g02-highlight.json", lit.kept())
    was, now = plain.entity(handle) or {}, lit.entity(handle) or {}
    usable = plain.good and lit.good and gh.same_camera(plain, lit) and bool(now.get("screen_bounds"))
    R.check(DRAWN, usable and was.get("selected") is False and now.get("selected") is True,
            f"同一个镜头（bounds={WINDOW}）先后两张：不带 highlight_handles 的 {plain.told()}，带的 {lit.told()}；两次应答的矩阵"
            f"{'相同' if gh.same_camera(plain, lit) else '不同'}；该符号都在 visible_entities 里，selected 依次为 {was.get('selected')!r}、{now.get('selected')!r}")
    if usable:
        clear = plain.clear()
        ground, _ = shot.commonest(plain.picture, clear)
        box = gh.rect_from(now["screen_bounds"], AROUND)
        missing = [p for p in middles if not gh.drawn_near(plain.picture, plain.pixel_of(*p), ground)]
        filled = [p for p in hollow if gh.drawn_near(plain.picture, plain.pixel_of(*p), ground)]
        R.check(DRAWN, gh.within(plain.pixel_of(*AT), clear) and not missing and not filled,
                f"不带高亮的图里符号画在该在的地方：外框四边中点处都有线（没有的：{missing or '无'}），框内空白处都是底色（不是的：{filled or '无'}）；"
                f"每单位 {plain.per_unit():.2f} 像素")
        inside = shot.changed(plain.picture, lit.picture, gh.common(box, clear))
        outside = shot.changed(plain.picture, lit.picture, clear, skip=[box])
        least = int(length * plain.per_unit() / 2)
        untouched = [p for p in middles
                     if shot.changed(plain.picture, lit.picture, gh.around(plain.pixel_of(*p), 3)).count == 0]
        R.check(DRAWN, inside.count >= least and outside.count == 0 and not untouched,
                f"两张图的差别全在该符号的 screen_bounds（放宽 {AROUND} 像素：{box}）以内：框内变了 {inside.count} 个像素"
                f"（下限 {least}，即外框周长 {length:g} 个单位换成像素的一半），框外（命令行面板以上）变了 {outside.count} 个；"
                f"外框四边中点处的线都换了颜色（没换的：{untouched or '无'}）")
        g.op("select", doc, handles=[])
        again = gh.take(g, m7h.SHOTS / "g02-plain-again.png", bounds=WINDOW, max_dimension=4096)
        left = shot.changed(plain.picture, again.picture, clear) if again.good and gh.same_camera(plain, again) else None
        R.check(DRAWN, left is not None and left.count == 0 and (again.entity(handle) or {}).get("selected") is False,
                f"取消选择后再取一张（{again.told()}）：与第一张在命令行面板以上相差 "
                f"{left.count if left is not None else '—'} 个像素，selected={(again.entity(handle) or {}).get('selected')!r}：差别来自高亮，不是别的")
    else:
        R.check(DRAWN, False, f"没有可用来比较的两张图：{plain.told()}；{lit.told()}")
except SystemExit:
    raise
except Exception as error:  # noqa: BLE001
    import traceback
    R.check("script", False, f"脚本异常: {error!r}\n{traceback.format_exc()}")
finally:
    m7h.cleanup()
    ok = R.summary()
sys.exit(0 if ok else 1)
