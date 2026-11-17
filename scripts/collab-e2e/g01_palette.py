# -*- coding: utf-8 -*-
"""VAL-DRAW-001  PIDPALETTE opens a docked panel with one tab for every
              category of the symbol library; the drawing stays as it was.
VAL-DRAW-002  PIDPALETTE closes the panel; opening it again shows it once.
VAL-DRAW-003  the grid shows the symbols of the tab that is chosen.
VAL-DRAW-004  every cell of the equipment tab shows an icon.
VAL-DRAW-006  the panel stays open on its tab while the document changes.

One GUI without collaboration on a fresh copy of the base drawing. Every
picture is one of the whole window (`view: "window"`, not made smaller) and is
read at the places the program reports through `ui_bounds`: the panel, its
cells and the viewport, in logical px, times the scale factor of the answer.

What a picture is asked: is the panel's rectangle filled with one colour, and
was that place different before; how many filled boxes stand in the row of
tabs at its top, and which of them has a colour of its own (the tab that is
chosen); is something written on each of those boxes, and something else on
each; is something drawn where the icon of a cell belongs; is nothing drawn
in the grid outside the cells that are reported.

Which characters a tab shows is not read: nothing here recognises text in a
picture. What is established in its place: the row holds as many tabs as the
library has categories, each with something written on it that is on no
other, and the label of every category, given to the control channel, lights
the tab at that category's place and brings up that category's symbols. In
the source the text of a tab is the label of its category
(src/ui/window/pid_palette.rs).

A tab is chosen through the control channel (`action pid_palette
tab=<label>`), which sends the message a click on the tab sends
(src/app/control/actions.rs); the pointer is not moved.

The command line panel lies over the bottom of the viewport and its text
changes by itself, so that strip is left out wherever pictures are compared
or searched. It is taken from the viewport as it is while the palette is
closed: the command line panel keeps its place when the palette opens.
"""
import math
import sys

import gh
import m7h
import shot
import yh

R = m7h.Results("g01_palette")
OPEN, CLOSE, TABS, ICONS, DOCS = "VAL-DRAW-001", "VAL-DRAW-002", "VAL-DRAW-003", "VAL-DRAW-004", "VAL-DRAW-006"
PANEL = "pid-palette-panel"             # the id the panel reports its rectangle under
CELL = "pid-palette-cell-"              # the same for a cell, followed by the name of the symbol's block
VIEWPORT = "viewport-capture-bounds"    # the same for the viewport
TAB_ROW = 28        # logical px: how high the row of tabs at the top of the panel is
ICON = 46           # logical px: the side of the square canvas an icon is drawn in
EDGE = 2            # logical px between the side of a cell and that canvas
FILLED = 0.8        # this share of the panel's rectangle has the panel's colour, at least
FRESH = 0.9         # this share of that place is not as it was before the panel opened, at least
INSET = 2           # logical px left out along each side of a tab's box where its text is looked for: its corners are round
WRITTEN = 0.05      # this share of the rest of the box is not of the tab's own colour, at least: something is written there
APART = 0.3         # this share of what is written on two tabs is on one of them only, at least: they do not read the same
SLIDE = 2           # px: two tabs are laid over each other at every offset up to this far each way, and the best fit counts


def reported(gui, doc, prefix):
    """What `ui_bounds` lists under a prefix, in its order: (id, [x, y, width, height] in logical px)."""
    _, body = gui.op("ui_bounds", doc, prefix=prefix)
    return [(w.get("id"), w.get("bounds")) for w in (body or {}).get("widgets") or []]


def size_of(rect):
    return max(0, rect[2] - rect[0]) * max(0, rect[3] - rect[1]) if rect else 0


def same_size(a, b):
    return (a.picture.width, a.picture.height) == (b.picture.width, b.picture.height)


def lone(colours):
    """Which of the colours no other one is like: its index, or None when
    that is true of none of them or of several."""
    alone = [i for i, colour in enumerate(colours)
             if all(shot.far(colour, other, gh.GRID) for j, other in enumerate(colours) if j != i)]
    return alone[0] if len(alone) == 1 else None


def written(text):
    """The share of a tab's box, without its rim, that is not of the tab's own colour."""
    area = sum(len(row) for row in text)
    return sum(sum(row) for row in text) / area if area else 0.0


def apart(a, b):
    """How unlike what is written on two tabs is: of the places written on in
    either, the share written on in one of them only, at the offset where the
    two fit best. 0 when nothing is written on either.

    At a scale factor of 1.5 the three labels of the library as it is are
    50 % to 69 % apart; one label on a tab that is chosen and on one that is
    not, 5 % to 8 % (the edges of the strokes come out a little differently
    on another colour)."""
    p, q = ({(x, y) for y, row in enumerate(text) for x, value in enumerate(row) if value} for text in (a, b))
    if not p and not q:
        return 0.0
    best = 1.0
    for dx in range(-SLIDE, SLIDE + 1):
        for dy in range(-SLIDE, SLIDE + 1):
            moved = {(x + dx, y + dy) for x, y in q}
            best = min(best, len(p ^ moved) / len(p | moved))
    return best


def said(resp):
    """How a request was answered, for the text of a check."""
    body = yh.body_of(resp)
    return f"HTTP {resp[0]}，ok={body.get('ok')!r}" + (f"，error={body.get('error')!r}" if body.get("error") else "")


def blocks_of(category):
    """The names of the blocks of a category's symbols, which its cells are reported under."""
    return sorted(f"PID_SYM_{category['id']}_{item['id']}" for item in category["symbols"])


class Look:
    """A picture of the whole window, with what the program reports of the
    panel, its cells and the viewport right after it was taken."""

    def __init__(self, gui, name, doc):
        self.view = gh.take(gui, m7h.SHOTS / f"g01-{name}.png", document_id=doc, view="window", max_dimension=4096)
        R.dump(f"g01-{name}.json", self.view.kept())
        self.scale = float(self.view.answer.get("scale_factor") or 1.0)
        listed = reported(gui, doc, "pid-palette")
        self.panels = [bounds for key, bounds in listed if key == PANEL]
        self.cells = {key[len(CELL):]: bounds for key, bounds in listed if str(key).startswith(CELL)}
        self.viewport = dict(reported(gui, doc, VIEWPORT)).get(VIEWPORT)
        self.good = self.view.good and self.view.answer.get("scope") == "window" and self.viewport is not None
        self.panel = self._ground = None
        if self.good and len(self.panels) == 1:
            self.panel = self.picture.box(gh.scaled(self.panels[0], self.scale))

    @property
    def picture(self):
        return self.view.picture

    def bottom(self):
        """The bottom of the viewport, as high as the command line panel may be."""
        left, _, right, bottom = gh.scaled(self.viewport, self.scale)
        return left - 2, bottom - math.ceil(gh.COMMAND_LINE * self.scale), right + 2, bottom + 2

    def ground(self):
        """The colour most of the panel's rectangle has, and the share of the rectangle that has it."""
        if self._ground is None:
            colour, n = shot.commonest(self.picture, self.panel)
            self._ground = colour, n / max(1, size_of(self.panel))
        return self._ground

    def tabs(self):
        """The tabs the picture shows: the filled boxes in the row at the top
        of the panel, left to right, each as (rectangle, the colour of most
        of it, what is written on it). The last is the box without a rim of
        INSET logical px, as rows of 0 and 1: 1 where a pixel is not of that
        colour."""
        colour = self.ground()[0]
        row = (self.panel[0], self.panel[1], self.panel[2], self.panel[1] + int(round(TAB_ROW * self.scale)))
        rim = math.ceil(INSET * self.scale)
        found = []
        for left, right in shot.stretches(self.picture, row, colour, gh.GRID):
            box = shot.unlike(self.picture, colour, (left, row[1], right, row[3]), gh.GRID).rect
            fill = shot.commonest(self.picture, box)[0]
            inner = (box[0] + rim, box[1] + rim, box[2] - rim, box[3] - rim)
            text = shot.mask(self.picture, inner, fill, gh.GRID) if inner[0] < inner[2] and inner[1] < inner[3] else []
            found.append((box, fill, text))
        return found

    def icon(self, block):
        """Where the icon of a cell belongs (the square canvas inside the
        cell, one logical px in from its sides), and what is drawn there."""
        x, y = self.cells[block][:2]
        square = gh.scaled([x + EDGE + 1, y + EDGE + 1, ICON - 2, ICON - 2], self.scale)
        return square, shot.unlike(self.picture, self.ground()[0], square, gh.GRID)

    def first(self):
        """The cell at the top left of the grid."""
        return min(self.cells, key=lambda block: (self.cells[block][1], self.cells[block][0]))

    def stray(self, bar):
        """What is drawn in the grid outside the reported cells: from the top
        of the highest cell down to the command line strip `bar`, all the
        width of the panel."""
        cells = [gh.scaled(bounds, self.scale) for bounds in self.cells.values()]
        cells = [(left - 2, top - 2, right + 2, bottom + 2) for left, top, right, bottom in cells]
        grid = (self.panel[0], min(top for _, top, _, _ in cells), self.panel[2], min(self.panel[3], bar[1]))
        return shot.unlike(self.picture, self.ground()[0], grid, gh.GRID, skip=cells)


def shows(look, category, index, tabs, bar):
    """Does a picture show the panel open on the tab of a category: yes or
    no, and the text that says what was seen."""
    want = blocks_of(category)
    if not look.good or look.panel is None:
        return False, f"{look.view.told()}；程序报的面板 {len(look.panels)} 个，没有可读的面板"
    colour, share = look.ground()
    seen = look.tabs()
    chosen = lone([fill for _, fill, _ in seen])
    icons = {block: look.icon(block)[1].count for block in sorted(look.cells)}
    stray = look.stray(bar) if look.cells else None
    good = (share >= FILLED and len(seen) == tabs and chosen == index and sorted(look.cells) == want
            and all(icons.values()) and stray is not None and stray.count == 0)
    return good, (f"{look.view.told()}；面板矩形 {look.panel} 里 {share:.1%} 是面板底色 {colour}；页签条里 {len(seen)} 个色块，"
                  f"颜色独一份的（选中的页签）是第 {chosen + 1 if chosen is not None else '—'} 个，应是第 {index + 1} 个；"
                  f"程序报的格子 {sorted(look.cells)}，该类在符号库里的符号 {want}；各格放图标的方块里不是底色的像素数 {icons}；"
                  f"这些格子以外的网格区域（到命令行条为止）里画着 {stray.count if stray is not None else '—'} 个像素")


try:
    categories = gh.library(m7h.REPO)["categories"]
    labels = [category["label"] for category in categories]
    equipment = next(category for category in categories if category["id"] == "equipment")
    valve = next(category for category in categories if category["id"] == "valve")

    dwg = m7h.fresh_copy("g01-palette.dwg")
    g = gh.fresh_settings(m7h.Gui("g01", 3210, dwg=dwg, collab=False)).start()
    took = g.wait_ready(m7h.READY_BUDGET)
    R.check("setup", took is not None,
            f"一个不连协同的实例，启动后 {'%.1f' % took if took is not None else '—'} 秒就绪（基准图的副本 {dwg.name}；本实例的设置目录先清掉，程序从默认布局起步）")
    if took is None:
        raise SystemExit(1)
    woke = gh.wake(g, m7h.SHOTS / "g01-wake.png")
    R.note(f"画面跟得上截图（capture 应答 waited_for_frame=true）：就绪后 "
           f"{'%.1f 秒' % woke if woke is not None else f'{gh.WAKE:g} 秒内没有等到，下面每张图靠重发来确认'}")
    doc = g.doc_id()
    counts = [gh.entity_count(g, doc)]

    def choose(category):
        return g.api("POST", "/run", {"document_id": doc, "op": "action", "name": "pid_palette",
                                      "value": f"tab={category['label']}"})

    closed = Look(g, "closed", doc)
    R.check("setup", closed.good and not closed.panels,
            f"打开面板之前的整窗图（view=window；另加 max_dimension=4096，让图不被缩小）：{closed.view.told()}，"
            f"应答的 scope={closed.view.answer.get('scope')!r}；程序报的视口 {closed.viewport}（逻辑像素 x、y、宽、高），"
            f"报的面板 {len(closed.panels)} 个")
    if not closed.good:
        raise SystemExit(1)
    bar = closed.bottom()       # where the command line panel is, with the palette open as well

    # ── VAL-DRAW-001: open ──
    turned = g.run("PIDPALETTE", doc)
    opened = Look(g, "open", doc)
    counts.append(gh.entity_count(g, doc))
    usable = opened.good and opened.panel is not None and same_size(closed, opened)
    docked = False
    if usable:
        v0, v1, p = closed.viewport, opened.viewport, opened.panels[0]
        docked = (v1[2] < v0[2] and v1[0] + v1[2] <= p[0] + 0.5 and p[0] + p[2] <= v0[0] + v0[2] + 0.5
                  and v0[1] - 0.5 <= p[1] and p[1] + p[3] <= v0[1] + v0[3] + 0.5)
    R.check(OPEN, yh.ok(turned) and len(opened.panels) == 1 and docked,
            f"PIDPALETTE（{said(turned)}）之后程序报 {len(opened.panels)} 个 {PANEL}：{opened.panels[:1]}；"
            f"视口由 {closed.viewport} 变为 {opened.viewport}；视口变窄、面板落在视口右边让出来的那一条里"
            f"（停靠在右侧，不是浮在图上）：{'是' if docked else '否'}")
    if usable:
        colour, share = opened.ground()
        above = size_of(opened.panel) - size_of(gh.common(opened.panel, bar))
        fresh = shot.changed(closed.picture, opened.picture, opened.panel, gh.GRID, skip=[bar]).count / max(1, above)
        R.check(OPEN, share >= FILLED and fresh >= FRESH,
                f"打开后的整窗图 {opened.view.told()}：面板矩形 {opened.panel}（ui_bounds 乘缩放 {opened.scale:g}）里 "
                f"{share:.1%} 是同一种颜色 {colour}（下限 {FILLED:.0%}）；与打开前的图相比，这块地方（命令行条 {bar} 除外）"
                f"有 {fresh:.1%} 的像素变了（下限 {FRESH:.0%}）")
        seen = opened.tabs()
        fills = [fill for _, fill, _ in seen]
        chosen = lone(fills)
        inked = [written(text) for _, _, text in seen]
        pairs = {f"{i + 1} 与 {j + 1}": apart(seen[i][2], seen[j][2])
                 for i in range(len(seen)) for j in range(i + 1, len(seen))}
        inked_said = "、".join(f"{part:.1%}" for part in inked)
        pairs_said = "、".join(f"第 {pair} 个 {part:.1%}" for pair, part in pairs.items())
        R.check(OPEN, len(seen) == len(categories) and chosen == 0 and sorted(opened.cells) == blocks_of(categories[0])
                and all(part >= WRITTEN for part in inked) and all(part >= APART for part in pairs.values()),
                f"面板顶上的页签条（高 {TAB_ROW} 逻辑像素）里从左到右 {len(seen)} 个色块 {[box for box, _, _ in seen]}，颜色依次 {fills}；"
                f"符号库 categories 有 {len(categories)} 类：{labels}。颜色独一份的（选中的页签）是第 {chosen + 1 if chosen is not None else '—'} 个，"
                f"应是第 1 个（{labels[0]}）；程序报的格子 {sorted(opened.cells)}，第 1 类的符号 {blocks_of(categories[0])}。"
                f"每个色块上都写着东西：去掉四周 {INSET} 逻辑像素的边，不是色块自己底色的像素依次占 {inked_said}（下限 {WRITTEN:.0%}）；"
                f"写的东西两两不同：两个色块叠在一起（上下左右各挪 {SLIDE} 像素以内，取最合得上的一处），写了东西的位置里"
                f"只有一个色块写了的占：{pairs_said}（下限 {APART:.0%}）。"
                f"写的是哪几个字不从图上认：源码里页签的字就是分类的 label，下面按每一类的 label 选页签")
        over = shot.unlike(opened.picture, colour, gh.common(opened.panel, bar), gh.GRID) if gh.common(opened.panel, bar) else None
        if over is not None and over.count:
            R.note(f"记一笔（不计入结果）：符号面板矩形底部落在命令行条里的那一段 {gh.common(opened.panel, bar)} 有 {over.count} 个像素不是面板底色，"
                   f"在 {over.rect} 之内：面板打开后视口变窄了，命令行面板没有跟着变，右端压在符号面板的左下角上（见 shots/{opened.view.path.name}）")
    else:
        R.check(OPEN, False, f"没有可读的整窗图：{opened.view.told()}；程序报的面板 {len(opened.panels)} 个")

    # ── VAL-DRAW-002: closed again, then open again ──
    turned = g.run("PIDPALETTE", doc)
    shut = Look(g, "closed-again", doc)
    counts.append(gh.entity_count(g, doc))
    gone = shot.changed(closed.picture, shut.picture, None, skip=[bar]) if shut.good and same_size(closed, shut) else None
    R.check(CLOSE, yh.ok(turned) and not shut.panels and not shut.cells and gone is not None and gone.count == 0,
            f"再执行一次 PIDPALETTE（{said(turned)}）：程序报的面板 {len(shut.panels)} 个、格子 {len(shut.cells)} 个，视口回到 {shut.viewport}；"
            f"整窗图 {shut.view.told()} 与打开之前那张相比，除命令行条 {bar} 外相差 "
            f"{gone.count if gone is not None else '—'} 个像素：面板和页签原来所在的地方又是打开前的样子")
    R.check(OPEN, counts[0] is not None and counts[0] == counts[1] == counts[2],
            f"面板不改图：query 的 count 打开前 {counts[0]}、打开后 {counts[1]}、关闭后 {counts[2]}")

    turned = g.run("PIDPALETTE", doc)
    again = Look(g, "open-again", doc)
    counts.append(gh.entity_count(g, doc))
    twice = (shot.changed(opened.picture, again.picture, None, skip=[bar])
             if opened.good and again.good and same_size(opened, again) else None)
    R.check(CLOSE, yh.ok(turned) and len(again.panels) == 1 and again.panels == opened.panels and twice is not None and twice.count == 0,
            f"第三次执行 PIDPALETTE（{said(turned)}）：程序报的 {PANEL} 恰好 {len(again.panels)} 个，位置 {again.panels[:1]} 与第一次打开时"
            f"{'相同' if again.panels == opened.panels else '不同'}；整窗图 {again.view.told()} 与第一次打开的那张相比，除命令行条外相差 "
            f"{twice.count if twice is not None else '—'} 个像素：面板还是那一个，别处没有多出第二个")
    R.check(CLOSE, counts[0] is not None and counts[2] == counts[0] and counts[3] == counts[0],
            f"query 的 count 始终是 {counts[0]}：打开前、打开后、关闭后、再打开后依次 {counts}")

    # ── VAL-DRAW-003 and 004: the equipment tab, every other tab, the valve tab last ──
    picked = choose(equipment)
    first = Look(g, "tab-equipment", doc)
    good, text = shows(first, equipment, categories.index(equipment), len(categories), bar)
    R.check(TABS, yh.ok(picked) and good,
            f"选「{equipment['label']}」页签（action pid_palette tab={equipment['label']}，{said(picked)}）后：{text}")
    if first.good and first.panel is not None and first.cells:
        marks = {}
        for block in sorted(first.cells):
            drawn = first.icon(block)[1]
            marks[block] = (drawn.count, drawn.size)
        R.check(ICONS, sorted(first.cells) == blocks_of(equipment) and all(n for n, _ in marks.values())
                and len(set(marks.values())) == len(marks),
                f"「{equipment['label']}」页签选中时的整窗图（shots/{first.view.path.name}）：程序报 {len(first.cells)} 个格子，"
                f"equipment.symbols 有 {len(equipment['symbols'])} 个；每个格子放图标的方块（格子里 {ICON} 逻辑像素见方的画布，各边再收 1）里"
                f"不是面板底色的像素数和它们的外接框大小：{marks}：没有空格子，各格画的不一样")
    else:
        R.check(ICONS, False, f"没有可读的图或程序没有报格子：{first.view.told()}；格子 {sorted(first.cells)}")

    for other in categories:
        if other is equipment or other is valve:
            continue
        picked = choose(other)
        between = Look(g, f"tab-{other['id']}", doc)
        good, text = shows(between, other, categories.index(other), len(categories), bar)
        R.check(TABS, yh.ok(picked) and good,
                f"选「{other['label']}」页签（action pid_palette tab={other['label']}，{said(picked)}）后：{text}")

    picked = choose(valve)
    second = Look(g, "tab-valve", doc)
    good, text = shows(second, valve, categories.index(valve), len(categories), bar)
    R.check(TABS, yh.ok(picked) and good,
            f"选「{valve['label']}」页签（action pid_palette tab={valve['label']}，{said(picked)}）后：{text}")
    if first.good and second.good and first.panel is not None and second.panel is not None and first.cells and second.cells:
        (here, was), (there, now) = first.icon(first.first()), second.icon(second.first())
        moved = here != there
        differ = None if moved else shot.changed(first.picture, second.picture, here).count
        R.check(TABS, moved or differ > 0,
                f"两张图内容不同：第一个格位放图标的方块在两张图里依次是 {here}、{there}（"
                f"{'位置不同' if moved else f'位置相同，里面有 {differ} 个像素不一样'}）；"
                f"前一张是 {first.first()}（{was.count} 个像素），后一张是 {second.first()}（{now.count} 个像素）")
    else:
        R.check(TABS, False, "没有可用来比较的两张图")

    # ── VAL-DRAW-006: a second document, and back ──
    def documents():
        status, body = g.api("GET", "/documents")
        return list((body or {}).get("documents") or []) if status == 200 else []

    def brief(docs):
        return [{"id": d.get("id"), "start": d.get("start"), "has_path": bool(d.get("path"))} for d in docs]

    def active():
        return g.state().get("document_id")

    before = documents()
    made = g.api("POST", "/documents", {})
    after = documents()
    R.dump("g01-documents.json", {"before": brief(before), "after": brief(after)})
    known = {d.get("id") for d in before}
    added = [d.get("id") for d in after if d.get("id") not in known]
    new = added[0] if len(added) == 1 else None
    drawings = [sum(1 for d in docs if not d.get("start")) for docs in (before, after)]
    R.check(DOCS, made[0] == 201 and new is not None and drawings == [1, 2],
            f"POST /documents {{}} → HTTP {made[0]}；GET /documents 由 {len(before)} 条变为 {len(after)} 条，其中图纸"
            f"（不算 start=true 的起始页）由 {drawings[0]} 张变为 {drawings[1]} 张，新文档的 id={new}（原来的图 id={doc}）。"
            f"契约写「返回 2 个文档」，实际列表里还有起始页那一条")
    if new is not None:
        went = g.op("activate", new)
        on = active()
        there = Look(g, "doc-new", new)
        good, text = shows(there, valve, categories.index(valve), len(categories), bar)
        R.check(DOCS, yh.ok(went) and on == new and active() == new and good,
                f"切到新文档（op activate，{said(went)}；state 的 document_id={on}，取图之后 {active()}），面板仍开着、仍在"
                f"「{valve['label']}」页签：{text}")
        went = g.op("activate", doc)
        on = active()
        back = Look(g, "doc-back", doc)
        good, text = shows(back, valve, categories.index(valve), len(categories), bar)
        R.check(DOCS, yh.ok(went) and on == doc and active() == doc and good,
                f"切回原来的图（op activate，{said(went)}；state 的 document_id={on}，取图之后 {active()}），面板同样开着、在"
                f"「{valve['label']}」页签：{text}")
    else:
        R.check(DOCS, False, "没有得到新文档，切换文档的两步没有做")
except SystemExit:
    raise
except Exception as error:  # noqa: BLE001
    import traceback
    R.check("script", False, f"脚本异常: {error!r}\n{traceback.format_exc()}")
finally:
    m7h.cleanup()
    ok = R.summary()
sys.exit(0 if ok else 1)
