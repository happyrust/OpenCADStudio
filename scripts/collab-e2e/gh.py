# -*- coding: utf-8 -*-
"""What the scenarios that judge by the picture share (g01 … g05, t00_smoke).

A picture says something about the answer of `capture` only when it shows the
frame that answer describes. `capture` first changes the selection and the
camera as asked, then takes the picture at the next redraw of the window or
after 50 ms, whichever comes first. When the 50 ms come first, the picture can
still show the frame from before the request, while the matrices and the
`screen_bounds` of the answer already describe the new camera. The answer says
which of the two it was: `waited_for_frame`. It is false in two cases that the
answer does not tell apart: the window was redrawn too late (common in the
first seconds of an instance), or nothing asked for a redraw because the
request changed nothing (common whenever the instance is idle).

So a scenario first waits until the instance keeps up (`wake`), and `take`
sends a request again until the picture is known to be current. Sending it
again changes nothing in the drawing: a request that has been obeyed once
selects the same entities and puts the camera where it already is.

A picture also depends on how the window is laid out, and the program keeps
part of that from one run to the next: its settings file (`settings.json`,
block `dock`) says on which side each panel is docked. Every instance of a
scenario has a settings folder of its own inside the evidence directory, and a
later run of the scenario in the same evidence directory finds it as the
earlier run left it. A scenario that judges by where things are in the window
therefore removes that folder before it starts its instance (`fresh_settings`)
and so starts from the program's own defaults.

The module imports neither harness; it works on the `Gui` object of either.
"""
import json
import math
import shutil
import time
from pathlib import Path

import shot

TRIES = 8               # how often one request is sent at most
PAUSE = 0.4             # seconds between two of them
WAKE = 30.0             # seconds an instance is given to start keeping up
COMMAND_LINE = 96       # logical px at the bottom of the viewport that the command line panel may cover
GRID = 12               # the grid lines of the viewport are within this of its background, per channel


class Shot:
    """One `capture`: the HTTP status, the answer, the bytes of the file, what
    `waited_for_frame` was at every attempt, and `proof`: why the picture is
    taken to show the frame the answer describes (None when nothing says so).
    `picture` is decoded when it is first asked for."""

    def __init__(self, path, status, answer, data, waited, proof):
        self.path, self.status, self.answer, self.data = Path(path), status, answer or {}, data
        self.waited, self.proof = waited, proof
        self._picture = None

    @property
    def picture(self):
        if self._picture is None and self.data is not None:
            self._picture = shot.decode(self.data, self.path)
        return self._picture

    @property
    def good(self):
        return (self.status == 200 and self.answer.get("ok") is True
                and self.data is not None and self.proof is not None)

    def told(self):
        """For the text of a check: the file, its size, how the picture was settled."""
        if self.data is None:
            return f"shots/{self.path.name}：没有得到图（HTTP {self.status}，error={self.answer.get('error')!r}）"
        return (f"shots/{self.path.name} {self.picture.width}x{self.picture.height}"
                f"（{self.proof or '没有一张能确认是应答所说的那一帧'}；各次的 waited_for_frame {self.waited}）")

    def kept(self):
        """The answer as it is filed with the evidence: without its `state`
        block, which describes the instance (its session id included) and
        says nothing about the picture."""
        return {key: value for key, value in self.answer.items() if key != "state"}

    # ── the drawing and the picture ──
    def pixel_of(self, x, y):
        """Where a point of the drawing is in the picture."""
        return shot.apply(self.answer["world_to_pixel_matrix"], x, y)

    def world_of(self, x, y):
        """Which point of the drawing a pixel of the picture shows."""
        return shot.apply(self.answer["pixel_to_world_matrix"], x, y)

    def per_unit(self):
        """Pixels per drawing unit."""
        m = self.answer["world_to_pixel_matrix"]
        return math.hypot(m[0], m[2])

    def rect_of(self, x0, y0, x1, y1, grow=0):
        """The pixels a rectangle of the drawing covers, and `grow` more on every side."""
        (ax, ay), (bx, by) = self.pixel_of(x0, y0), self.pixel_of(x1, y1)
        return (math.floor(min(ax, bx)) - grow, math.floor(min(ay, by)) - grow,
                math.ceil(max(ax, bx)) + 1 + grow, math.ceil(max(ay, by)) + 1 + grow)

    def clear(self):
        """The part of a viewport picture that shows the drawing whatever the
        command line says: all of it but the strip at the bottom where the
        command line panel lies over it. (For a picture that was not made
        smaller: ask for `max_dimension` large enough.)"""
        strip = math.ceil(COMMAND_LINE * float(self.answer.get("scale_factor") or 1.0))
        return 0, 0, self.picture.width, max(0, self.picture.height - strip)

    def entity(self, handle, among="visible_entities"):
        """What the answer says of one entity (`visible_entities` or `annotations`), or None."""
        want = str(handle).upper()
        for item in self.answer.get(among) or []:
            if str(item.get("handle")).upper() == want:
                return item
        return None


def take(gui, path, tries=TRIES, pause=PAUSE, **request):
    """`capture` with the fields of `request`, sent again until the picture is
    known to show the frame the answer describes.

    Only what `request` names is sent besides the document and the path, so a
    scenario can spell a request exactly as the contract does.

    The picture counts as settled when the answer says `waited_for_frame`
    true. Failing that, it counts when it is the picture of a repeated
    request and equal to the picture of the repeat before it, with the same
    matrix: the request had been obeyed before either was taken, and the frame
    did not change between them. Equal means every pixel for a picture of the
    window, and every pixel above the command line panel (whose text changes
    by itself) for a picture of the viewport."""
    path = Path(path)
    body = {"document_id": request.pop("document_id", None) or gui.doc_id(), "path": str(path)}
    body.update(request)
    waited, earlier, last = [], None, None
    for attempt in range(tries):
        if attempt:
            time.sleep(pause)
        if path.exists():
            path.unlink()
        status, answer = gui.api("POST", "/capture", body, timeout=120)
        answer = answer if isinstance(answer, dict) else {}
        if status != 200 or not path.exists():
            return Shot(path, status, answer, None, waited, None)
        waited.append(answer.get("waited_for_frame"))
        last = Shot(path, status, answer, path.read_bytes(), waited, None)
        if answer.get("waited_for_frame") is True:
            last.proof = "应答 waited_for_frame=true"
            return last
        if attempt == 0:
            continue                    # this one may show the frame from before the request
        if earlier is not None and same_camera(earlier, last) and _steady(earlier, last):
            last.proof = f"没有一次 waited_for_frame=true，同一请求第 {attempt} 次和第 {attempt + 1} 次得到的画面相同、矩阵相同"
            return last
        earlier = last
    return last


def _steady(a, b):
    """Two pictures of one request show one frame."""
    if a.data == b.data:
        return True
    if b.answer.get("scope") == "window":
        return False
    if (a.picture.width, a.picture.height) != (b.picture.width, b.picture.height):
        return False
    return shot.changed(a.picture, b.picture, b.clear()).count == 0


def fresh_settings(gui):
    """Make an instance that has not been started yet start from the
    program's own defaults: its settings folder (the APPDATA of this one
    instance, inside the evidence directory) is removed, with whatever an
    earlier run of the scenario left in it. Returns the instance."""
    if gui.appdata.exists():
        shutil.rmtree(gui.appdata)
    return gui


def wake(gui, path, seconds=WAKE, pause=0.25):
    """Wait until the instance redraws its window within the 50 ms `capture`
    gives it: small pictures of the viewport are asked for until an answer
    says `waited_for_frame` true. Returns how many seconds that took, or None
    when it did not happen in `seconds`."""
    body = {"document_id": gui.doc_id(), "path": str(path), "max_dimension": 256}
    began = time.time()
    while time.time() - began < seconds:
        status, answer = gui.api("POST", "/capture", body, timeout=120)
        if status == 200 and isinstance(answer, dict) and answer.get("waited_for_frame") is True:
            return time.time() - began
        time.sleep(pause)
    return None


def same_camera(a, b, tolerance=1e-6):
    """Two captures were taken with one camera: their matrices agree."""
    ma, mb = a.answer.get("world_to_pixel_matrix") or [], b.answer.get("world_to_pixel_matrix") or []
    return len(ma) == 6 and len(mb) == 6 and all(abs(p - q) <= tolerance for p, q in zip(ma, mb))


def rect_from(bounds, grow=0):
    """A `screen_bounds` ([min x, min y, max x, max y], fractions of pixels)
    as a rectangle of whole pixels, `grow` more on every side."""
    return (math.floor(bounds[0]) - grow, math.floor(bounds[1]) - grow,
            math.ceil(bounds[2]) + 1 + grow, math.ceil(bounds[3]) + 1 + grow)


def scaled(bounds, factor):
    """A widget's bounds as `ui_bounds` gives them ([x, y, width, height] in
    logical px) as a rectangle of the window picture."""
    x, y, width, height = bounds
    return (int(round(x * factor)), int(round(y * factor)),
            int(round((x + width) * factor)), int(round((y + height) * factor)))


def common(a, b):
    """The rectangle two rectangles share, or None."""
    left, top, right, bottom = max(a[0], b[0]), max(a[1], b[1]), min(a[2], b[2]), min(a[3], b[3])
    return (left, top, right, bottom) if left < right and top < bottom else None


def around(point, radius):
    """The square of pixels within `radius` of a pixel."""
    x, y = int(round(point[0])), int(round(point[1]))
    return x - radius, y - radius, x + radius + 1, y + radius + 1


def within(point, rect, margin=0):
    """A pixel lies inside a rectangle, at least `margin` from its sides."""
    return (rect[0] + margin <= point[0] < rect[2] - margin
            and rect[1] + margin <= point[1] < rect[3] - margin)


def drawn_near(picture, point, ground, radius=3, tolerance=GRID):
    """Something is drawn at a pixel or within `radius` of it: a pixel there
    is not the ground colour (the grid counts as ground)."""
    return shot.unlike(picture, ground, around(point, radius), tolerance).count > 0


def library(repo):
    """The symbol library the program is built with."""
    return json.loads((Path(repo) / "assets" / "pid-symbols.json").read_text(encoding="utf-8"))


def symbol(lib, name):
    """One symbol of the library by its id."""
    for category in lib["categories"]:
        for item in category["symbols"]:
            if item["id"] == name:
                return item
    raise KeyError(name)


def symbols(gui):
    """The P&ID symbols of the drawing as `query` lists them, left to right."""
    found = [e for e in gui.inserts() if str(e.get("block") or "").startswith("PID_SYM_")]
    return sorted(found, key=lambda e: (e.get("position") or [0, 0])[0])


def entity_count(gui, doc=None, **only):
    """`count` of a `query` (of everything, or of what `only` narrows it to)."""
    status, body = gui.op("query", doc, **only)
    return (body or {}).get("count") if status == 200 else None
