# -*- coding: utf-8 -*-
"""VAL-DRAW-022  three symbols show three tags: beside the geometry of each the
              picture has the pixels of its tag, and the three tags do not
              overlap.
VAL-DRAW-021  a tag set by hand to one that is taken: the status is
              `duplicated`, the record keeps the tag, and the properties panel
              and the drawing show `？` in its place.

One GUI without collaboration on a fresh copy of the base drawing; three
vessels in a row, tagged P-1001, P-1002, P-1003 by the program.

Which pixels are a tag is found by taking every view twice with one camera:
with the attributes shown, and after `ATTDISP OFF`. What the second picture
lacks is the tag; what it keeps is the geometry. (The second picture is asked
for without `focus_handles`: with the attributes hidden the symbols are
smaller, and fitting the camera to them again would move it.)

A picture does not say which characters a tag is made of; that is read from
the record. The picture says where the tag is, how wide and how high it is in
text heights, and so whether it is a tag of several characters or the single
`？`.
"""
import sys

import gh
import m7h
import shot
import yh

R = m7h.Results("g04_tag_display")
SHOWN, TAKEN = "VAL-DRAW-022", "VAL-DRAW-021"
PLACES = [(200.0, 150.0), (400.0, 150.0), (600.0, 150.0)]
AROUND = 40             # px around a symbol's `screen_bounds` in which its tag is looked for (the contract's margin)
MARK = "？"              # what is shown in place of a tag that is taken


def pair(gui, doc, name, **camera):
    """One view twice: with the attributes shown, then hidden. Returns the
    two captures and whether both ATTDISP commands were obeyed."""
    shown = gh.take(gui, m7h.SHOTS / f"g04-{name}.png", max_dimension=4096, **camera)
    off = gui.run("ATTDISP OFF", doc)
    try:
        hidden = gh.take(gui, m7h.SHOTS / f"g04-{name}-no-tags.png", max_dimension=4096)
    finally:
        on = gui.run("ATTDISP NORMAL", doc)
    usable = shown.good and hidden.good and yh.ok(off) and yh.ok(on) and gh.same_camera(shown, hidden)
    return shown, hidden, usable


def tag_of(shown, hidden, record, top):
    """What the pictures say of one symbol's tag: the pixels within AROUND of
    the symbol's `screen_bounds` that are gone with the attributes hidden.
    Sizes are given in text heights, the place relative to the symbol."""
    seen = shown.entity(record["handle"]) or {}
    if not seen.get("screen_bounds"):
        return {"area": None, "count": 0, "rect": None, "wide": 0.0, "high": 0.0, "above": False, "centred": False}
    area = gh.common(gh.rect_from(seen["screen_bounds"], AROUND), shown.clear())
    blob = shot.changed(hidden.picture, shown.picture, area) if area else shot.Blob()
    height = record["attribute_children"][0]["height"] * shown.per_unit()         # the text height in pixels
    x, y = record["position"][:2]
    column, row = shown.pixel_of(x, y + top)                                        # the middle of the body's upper side
    return {"area": area, "count": blob.count, "rect": blob.rect,
            "wide": blob.size[0] / height, "high": blob.size[1] / height,
            "above": blob.count > 0 and blob.bottom <= row + 1,
            "centred": blob.count > 0 and abs(blob.mid[0] - column) <= height}


def told(tag):
    return (f"{tag['count']} 像素、范围 {tag['rect']}、宽 {tag['wide']:.2f} 倍字高、高 {tag['high']:.2f} 倍字高、"
            f"{'在' if tag['above'] else '不在'}本体上方、{'对着' if tag['centred'] else '没对着'}符号中线")


def several(tag):
    """A tag of several characters, of the height the record names."""
    return tag["wide"] >= 3 and 0.6 <= tag["high"] <= 1.6 and tag["above"] and tag["centred"]


def single(tag):
    """One character."""
    return tag["count"] > 0 and tag["wide"] <= 1 and tag["high"] <= 1.6 and tag["above"] and tag["centred"]


try:
    vessel = gh.symbol(gh.library(m7h.REPO), "vessel")
    outline = next(part["points"] for part in vessel["geometry"] if part["kind"] == "polyline" and part.get("closed"))
    top = max(y for _x, y in outline)
    sides = list(zip(outline, outline[1:] + outline[:1]))
    middles = [((ax + bx) / 2, (ay + by) / 2) for (ax, ay), (bx, by) in sides]     # of the body's sides, around (0, 0)

    dwg = m7h.fresh_copy("g04-tag-display.dwg")
    g = gh.fresh_settings(m7h.Gui("g04", 3210, dwg=dwg, collab=False)).start()
    took = g.wait_ready(m7h.READY_BUDGET)
    R.check("setup", took is not None,
            f"一个不连协同的实例，启动后 {'%.1f' % took if took is not None else '—'} 秒就绪（基准图的副本 {dwg.name}；本实例的设置目录先清掉，程序从默认布局起步）")
    if took is None:
        raise SystemExit(1)
    woke = gh.wake(g, m7h.SHOTS / "g04-wake.png")
    R.note(f"画面跟得上截图（capture 应答 waited_for_frame=true）：就绪后 "
           f"{'%.1f 秒' % woke if woke is not None else f'{gh.WAKE:g} 秒内没有等到，下面每张图靠重发来确认'}")
    doc = g.doc_id()

    placed = [yh.ok(yh.draw(g, "PIDINSERT vessel", [at], doc)) for at in PLACES]
    records = gh.symbols(g)
    R.dump("g04-symbols.json", records)
    handles = [e["handle"] for e in records]
    tags = [g.author(h).get("tag") for h in handles]
    texts = [(e.get("attributes") or {}).get("PID_TAG") for e in records]
    made = all(placed) and len(records) == 3 and tags == ["P-1001", "P-1002", "P-1003"] and texts == tags
    R.check("setup", made,
            f"PIDINSERT vessel 三次（{PLACES}）：图里 {len(records)} 个符号，记录的位号依次为 {tags}，块属性 PID_TAG 的值依次为 {texts}")
    if not made:
        raise SystemExit(1)

    # ── VAL-DRAW-022 ──
    authored = gh.entity_count(g, doc, xdata_app="PID_AUTHOR")
    R.check(SHOWN, authored == 3, f"query xdata_app=PID_AUTHOR 的 count={authored}（应为 3）")
    shown, hidden, usable = pair(g, doc, "three", focus_handles=handles)
    R.dump("g04-three.json", shown.kept())
    if usable:
        ground, _ = shot.commonest(hidden.picture, hidden.clear())
        found = [tag_of(shown, hidden, e, top) for e in records]
        bare = [[p for p in middles
                 if not gh.drawn_near(hidden.picture, hidden.pixel_of(e["position"][0] + p[0], e["position"][1] + p[1]), ground)]
                for e in records]
        R.check(SHOWN, all(several(t) for t in found) and not any(bare),
                f"capture focus_handles=[三个符号]（{shown.told()}）与同一镜头下关掉属性显示的一张（{hidden.told()}）相比，"
                f"每个符号的 screen_bounds 放宽 {AROUND} 像素以内：关掉属性后外框四边中点处仍有线（没有的：{[b or '无' for b in bare]}），"
                f"多出来的位号像素依次为：{'；'.join(told(t) for t in found)}。判据：宽 ≥ 3 倍字高（多个字符）、高 0.6–1.6 倍字高、"
                f"在本体上方、对着符号中线；每单位 {shown.per_unit():.2f} 像素。是哪几个字符以记录为准（{texts}），图上不认字")
        rects = [t["rect"] for t in found]
        apart = all(rects) and all(gh.common(rects[i], rects[j]) is None for i in range(3) for j in range(i + 1, 3))
        R.check(SHOWN, apart, f"三块位号像素的范围 {rects} 两两不相交")
    else:
        R.check(SHOWN, False, f"没有可用来比较的两张图：{shown.told()}；{hidden.told()}；矩阵"
                f"{'相同' if shown.good and hidden.good and gh.same_camera(shown, hidden) else '不同或缺图'}")

    # ── VAL-DRAW-021 ──
    renamed = g.run("PIDTAG P-1002 P-1001", doc)
    records = gh.symbols(g)
    R.dump("g04-symbols-after.json", records)
    second = records[1]
    author = g.author(second["handle"])
    text = (second.get("attributes") or {}).get("PID_TAG")
    R.check(TAKEN, yh.ok(renamed) and author.get("tag") == "P-1001" and author.get("tag_status") == "duplicated" and text == MARK,
            f"PIDTAG P-1002 P-1001（把第二个符号的位号手工改成已被第一个占用的 P-1001）：记录里 tag={author.get('tag')!r}（保留）、"
            f"tag_source={author.get('tag_source')!r}、tag_status={author.get('tag_status')!r}；块属性 PID_TAG 的值 {text!r}；"
            f"另两个符号的位号 {[g.author(e['handle']).get('tag') for e in (records[0], records[2])]}、"
            f"块属性 {[(e.get('attributes') or {}).get('PID_TAG') for e in (records[0], records[2])]}")

    g.op("select", doc, handles=[second["handle"]])
    status, panel = g.op("properties", doc)
    g.op("select", doc, handles=[])
    panel = panel or {}
    rows = {row.get("label"): row.get("value")
            for section in panel.get("sections") or (panel.get("result") or {}).get("sections") or []
            for row in section.get("properties") or []}
    R.dump("g04-properties.json", rows)
    R.check(TAKEN, status == 200 and rows.get("位号") == MARK,
            f"选中第二个符号后读属性面板（properties）：「位号」行的值 {rows.get('位号')!r}，"
            f"「位号来源」{rows.get('位号来源')!r}，「状态」{rows.get('状态')!r}")

    shown, hidden, usable = pair(g, doc, "three-after", focus_handles=handles)
    R.dump("g04-three-after.json", shown.kept())
    if usable:
        found = [tag_of(shown, hidden, e, top) for e in records]
        R.check(TAKEN, several(found[0]) and single(found[1]) and several(found[2]),
                f"改后再取同样的两张（{shown.told()}；{hidden.told()}），三个符号旁的位号像素依次为：{'；'.join(told(t) for t in found)}。"
                f"第二个只剩一个字符的宽度（≤ 1 倍字高），另两个仍是多个字符（≥ 3 倍字高）")
    else:
        R.check(TAKEN, False, f"没有可用来比较的两张图：{shown.told()}；{hidden.told()}")

    near, bare_near, usable = pair(g, doc, "second", focus_handles=[second["handle"]])
    R.dump("g04-second.json", near.kept())
    if usable:
        tag = tag_of(near, bare_near, second, top)
        R.check(TAKEN, single(tag),
                f"capture focus_handles=[第二个符号]（{near.told()}）与关掉属性显示的一张（{bare_near.told()}）相比：位号像素 {told(tag)}；"
                f"每单位 {near.per_unit():.2f} 像素。是一个字符宽，不是 P-1001 那样的多个字符")
    else:
        R.check(TAKEN, False, f"没有可用来比较的两张图：{near.told()}；{bare_near.told()}")
except SystemExit:
    raise
except Exception as error:  # noqa: BLE001
    import traceback
    R.check("script", False, f"脚本异常: {error!r}\n{traceback.format_exc()}")
finally:
    m7h.cleanup()
    ok = R.summary()
sys.exit(0 if ok else 1)
