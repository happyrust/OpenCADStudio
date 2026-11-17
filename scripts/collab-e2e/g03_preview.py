# -*- coding: utf-8 -*-
"""VAL-DRAW-014  while a symbol is being placed, its preview follows the point.
VAL-DRAW-015  SPACE turns the preview a quarter turn about the point.
VAL-DRAW-016  CTRL+SPACE moves the preview to its next anchor.

In all three the drawing does not change: the entity count stays what it was.

One GUI without collaboration on a fresh copy of the base drawing. The camera
is set once, before the command starts, by a capture with `bounds`; every
picture after that is a plain capture of the viewport, with no field that
would move the camera (the ruling of 2026-11-07 asks for that).

The preview is what a picture has that the picture from before the command
lacks. It is looked for around the point the command was given, in a square
of the drawing: the command line panel, the coordinate boxes at the top left
and the coordinate icon at the origin also change while a command runs, and
lie outside these squares.
"""
import sys

import gh
import m7h
import shot
import yh

R = m7h.Results("g03_preview")
FOLLOW, TURN, ANCHOR = "VAL-DRAW-014", "VAL-DRAW-015", "VAL-DRAW-016"
CAMERA = [0, -100, 800, 500]
FIRST, SECOND = (200.0, 150.0), (600.0, 400.0)
REACH = 40                  # drawing units around a point in which its preview is looked for


def square(view, point):
    """The pixels of the square of the drawing around a point, above the command line panel."""
    return gh.common(view.rect_of(point[0] - REACH, point[1] - REACH, point[0] + REACH, point[1] + REACH), view.clear())


def told(blob):
    if not blob.count:
        return "没有像素"
    return f"{blob.count} 像素、范围 {blob.rect}（宽 {blob.size[0]}、高 {blob.size[1]}）、中心 ({blob.mid[0]:.1f}, {blob.mid[1]:.1f})"


try:
    vessel = gh.symbol(gh.library(m7h.REPO), "vessel")
    outline = next(part["points"] for part in vessel["geometry"] if part["kind"] == "polyline" and part.get("closed"))
    body = (max(x for x, _y in outline) - min(x for x, _y in outline),
            max(y for _x, y in outline) - min(y for _x, y in outline))          # width and height of the body, drawing units

    dwg = m7h.fresh_copy("g03-preview.dwg")
    g = gh.fresh_settings(m7h.Gui("g03", 3210, dwg=dwg, collab=False)).start()
    took = g.wait_ready(m7h.READY_BUDGET)
    R.check("setup", took is not None,
            f"一个不连协同的实例，启动后 {'%.1f' % took if took is not None else '—'} 秒就绪（基准图的副本 {dwg.name}；本实例的设置目录先清掉，程序从默认布局起步）")
    if took is None:
        raise SystemExit(1)
    woke = gh.wake(g, m7h.SHOTS / "g03-wake.png")
    R.note(f"画面跟得上截图（capture 应答 waited_for_frame=true）：就绪后 "
           f"{'%.1f 秒' % woke if woke is not None else f'{gh.WAKE:g} 秒内没有等到，下面每张图靠重发来确认'}")
    doc = g.doc_id()
    before = gh.entity_count(g, doc)

    base = gh.take(g, m7h.SHOTS / "g03-before.png", bounds=CAMERA, max_dimension=4096)
    R.dump("g03-before.json", base.kept())
    started = g.run("PIDINSERT vessel", doc)
    R.check("setup", base.good and yh.ok(started) and before is not None,
            f"镜头用 bounds={CAMERA} 定下（{base.told()}），此时实体数 {before}；然后启动 PIDINSERT vessel（命令等待给点）")
    if not (base.good and yh.ok(started)):
        raise SystemExit(1)
    scale = base.per_unit()

    def step(name, **feed):
        """Feed the command, take a plain picture, count the entities."""
        fed = g.op("input", doc, **feed)
        view = gh.take(g, m7h.SHOTS / f"g03-{name}.png", max_dimension=4096)
        usable = yh.ok(fed) and view.good and gh.same_camera(base, view)
        return view, usable, gh.entity_count(g, doc)

    # ── VAL-DRAW-014: the preview is where the point is, and goes with it ──
    one, ok1, n1 = step("at-first", kind="point", point=[FIRST[0], FIRST[1], 0.0], space="wcs")
    two, ok2, n2 = step("at-second", kind="point", point=[SECOND[0], SECOND[1], 0.0], space="wcs")
    if ok1 and ok2:
        a = shot.changed(base.picture, one.picture, square(base, FIRST))
        b = shot.changed(base.picture, two.picture, square(base, SECOND))
        left_behind = shot.changed(base.picture, two.picture, square(base, FIRST))
        want = [base.pixel_of(*SECOND)[i] - base.pixel_of(*FIRST)[i] for i in (0, 1)]
        went = [b.mid[i] - a.mid[i] for i in (0, 1)] if a.count and b.count else None
        sized = all(a.count and b.count and abs(blob.size[i] - body[i] * scale) <= 6 for blob in (a, b) for i in (0, 1))
        R.check(FOLLOW, went is not None and max(abs(went[i] - want[i]) for i in (0, 1)) <= 8 and sized and left_behind.count == 0
                and n1 == before and n2 == before,
                f"给点 {FIRST} 后的图（{one.told()}）比命令前多出的像素：{told(a)}；再给点 {SECOND} 后（{two.told()}）：{told(b)}。"
                f"预览中心移动了 {[round(v, 1) for v in went] if went else '—'} 像素，按 world_to_pixel_matrix 两点相距 "
                f"{[round(v, 1) for v in want]} 像素（每轴容差 8）；预览的宽高与符号本体 {body[0]:g}×{body[1]:g} 个单位换成的 "
                f"{body[0] * scale:.1f}×{body[1] * scale:.1f} 像素相差不过 6；第一个点周围此时还剩 {left_behind.count} 个变了的像素（预览是跟着走，"
                f"不是留下一份）；实体数两步后依次 {n1}、{n2}（命令前 {before}）")
    else:
        b = shot.Blob()
        R.check(FOLLOW, False, f"没有可用来比较的图：{one.told()}；{two.told()}")

    # ── VAL-DRAW-015: a quarter turn about the point ──
    three, ok3, n3 = step("turned", kind="token", text="SPACE")
    c = shot.changed(base.picture, three.picture, square(base, SECOND)) if ok3 else shot.Blob()
    if ok3 and b.count and c.count:
        swapped = abs(c.size[0] - b.size[1]) <= 3 and abs(c.size[1] - b.size[0]) <= 3 and abs(b.size[0] - b.size[1]) > 6
        drift = [c.mid[i] - b.mid[i] for i in (0, 1)]
        R.check(TURN, swapped and max(abs(v) for v in drift) <= 8 and n3 == before,
                f"输入 SPACE 后的图（{three.told()}）里预览：{told(c)}；之前宽 {b.size[0]}、高 {b.size[1]}，现在宽高互换"
                f"（各容差 3）{'成立' if swapped else '不成立'}，中心移动 {[round(v, 1) for v in drift]} 像素（每轴容差 8）；实体数 {n3}（命令前 {before}）")
    else:
        R.check(TURN, False, f"没有可用来比较的图或预览：{three.told()}；转之前 {told(b)}，转之后 {told(c)}")

    # ── VAL-DRAW-016: the next anchor ──
    four, ok4, n4 = step("next-anchor", kind="token", text="CTRL+SPACE")
    d = shot.changed(base.picture, four.picture, square(base, SECOND)) if ok4 else shot.Blob()
    if ok4 and c.count and d.count:
        shift = [d.mid[i] - c.mid[i] for i in (0, 1)]
        same_size = abs(d.size[0] - c.size[0]) <= 3 and abs(d.size[1] - c.size[1]) <= 3
        R.check(ANCHOR, max(abs(v) for v in shift) >= 3 and same_size and n4 == before,
                f"输入 CTRL+SPACE 后的图（{four.told()}）里预览：{told(d)}；给的点没有变，预览相对它移动了 "
                f"{[round(v, 1) for v in shift]} 像素（至少要 3），宽高没变（各容差 3）{'成立' if same_size else '不成立'}；"
                f"实体数 {n4}（命令前 {before}）")
    else:
        R.check(ANCHOR, False, f"没有可用来比较的图或预览：{four.told()}；换锚点之前 {told(c)}，之后 {told(d)}")

    # ── the preview is no entity: it goes when the command is cancelled ──
    cancelled = g.op("cancel", doc)
    after = gh.take(g, m7h.SHOTS / "g03-cancelled.png", max_dimension=4096)
    n5 = gh.entity_count(g, doc)
    if after.good and gh.same_camera(base, after):
        rest = [shot.changed(base.picture, after.picture, square(base, p)).count for p in (FIRST, SECOND)]
        R.check(FOLLOW, yh.body_of(cancelled).get("ok") is True and rest == [0, 0] and n5 == before,
                f"取消命令后的图（{after.told()}）：两个点周围与命令前相差 {rest} 个像素，实体数 {n5}（命令前 {before}）："
                f"上面看到的是预览，没有画进图里")
    else:
        R.check(FOLLOW, False, f"取消命令后没有可用来比较的图：{after.told()}")
except SystemExit:
    raise
except Exception as error:  # noqa: BLE001
    import traceback
    R.check("script", False, f"脚本异常: {error!r}\n{traceback.format_exc()}")
finally:
    m7h.cleanup()
    ok = R.summary()
sys.exit(0 if ok else 1)
