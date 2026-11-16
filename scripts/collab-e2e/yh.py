# -*- coding: utf-8 -*-
"""Helpers shared by this line's own scenarios (y03 / y08 / y09), on top of m7h.

Everything here reads a GUI's real state over its REST channel: the entities
that carry PID_AUTHOR, each one's author record, and the topology table as
`PIDTOPOLOGY` returns it. Handles are local to a drawing (an entity a peer
made has whatever handle the peer's drawing gave it), so the comparisons
replace every handle by what it stands for: a tag, or a kind and a place.
"""
import json
import time

import m7h


def body_of(resp):
    """The JSON body of an (http_status, body) pair."""
    return (resp[1] if isinstance(resp, tuple) else resp) or {}


def ok(resp):
    return resp[0] == 200 and body_of(resp).get("ok") is True


def draw(gui, cmd, points, doc=None):
    """Start `cmd`, feed it `points`, press Enter. Returns the last response."""
    started = gui.run(cmd, doc)
    if not ok(started):
        return started
    for x, y in points:
        fed = gui.op("input", doc, kind="point", point=[x, y, 0.0], space="wcs")
        if not ok(fed):
            return fed
    return gui.op("input", doc, kind="enter")


def look_at(gui, low, high, doc=None):
    """Zoom the view to the window `low`..`high` (two corners), so that a
    picture taken next shows what the scenario is about rather than the
    extents of everything. Nothing is drawn or changed by it."""
    started = gui.run("ZOOM WINDOW", doc)
    if not ok(started):
        return started
    last = started
    for x, y in (low, high):
        last = gui.op("input", doc, kind="point", point=[x, y, 0.0], space="wcs")
    return last


def focus_on(gui, keep=None, doc=None):
    """Fit the view to the entities that carry a PID_AUTHOR record with a
    topology role (the pipe's parts, connectors) — or to those `keep(author)`
    picks — so that a picture taken next shows what the scenario is about.
    Uses the automation channel's `view_focus`; nothing is drawn or changed.
    (`look_at` above does not work: ZOOM takes its window another way.)"""
    keep = keep or (lambda author: bool(author.get("topology")))
    handles = [h for h, (_where, author, _entity) in entities(gui).items() if keep(author)]
    if not handles:
        return None
    return gui.op("view_focus", doc, handles=handles)


def erase(gui, handles, doc=None):
    """Select `handles` and run ERASE on them."""
    selected = gui.op("select", doc, handles=list(handles))
    if not ok(selected):
        return selected
    return gui.run("ERASE", doc)


def num(value):
    value = float(value)
    return 0.0 if abs(value) < 1e-9 else round(value, 4)


def place(entity):
    """Where an entity is, from whichever of these the REST listing gives."""
    parts = [entity.get("type") or "?"]
    if entity.get("block"):
        parts.append(entity["block"])
    for key in ("start", "end", "position", "center", "location", "point"):
        value = entity.get(key)
        if isinstance(value, (list, tuple)) and len(value) >= 2:
            parts.append(f"{key}={num(value[0])},{num(value[1])}")
    return " ".join(parts)


def entities(gui):
    """{handle: (place, author dict)} for every entity carrying PID_AUTHOR."""
    status, listing = gui.api("GET", "/entities?xdata_app=PID_AUTHOR&limit=5000")
    out = {}
    for entity in (listing or {}).get("entities") or []:
        handle = entity.get("handle")
        out[handle] = (place(entity), gui.author(handle), entity)
    return out


def props_of(author):
    try:
        return json.loads(author.get("props") or "{}")
    except ValueError:
        return {"<unparsed>": author.get("props")}


def authored(gui, listing=None):
    """Sorted rows — what each PID_AUTHOR entity is, where, and what it says —
    with the owner named by what it is instead of by its handle."""
    listing = listing if listing is not None else entities(gui)

    def name(handle):
        if not handle:
            return ""
        known = listing.get(handle.upper()) or listing.get(handle)
        if not known:
            return "(gone)"
        return known[1].get("tag") or known[0]

    rows = []
    for handle, (where, author, _entity) in listing.items():
        rows.append(" | ".join([
            where,
            f"type={author.get('type', '')}",
            f"tag={author.get('tag', '')}",
            f"topology={author.get('topology', '')}",
            f"owner={name(author.get('owner', ''))}",
            "props=" + json.dumps(props_of(author), ensure_ascii=False, sort_keys=True),
        ]))
    return sorted(rows)


def topology(gui, doc=None):
    """The topology table as `PIDTOPOLOGY` returns it (result.topology)."""
    status, body = gui.run("PIDTOPOLOGY", doc)
    result = (body or {}).get("result") or {}
    return result.get("topology") or {}


def effective(table, listing):
    """The topology as far as it speaks of entities that are there, with every
    handle replaced by what it stands for. Two drawings that agree give the
    same value here whatever handles their entities have."""
    live = {h.upper() for h in listing}
    alive = lambda handle: bool(handle) and handle.upper() in live  # noqa: E731
    fittings = [f for f in table.get("fittings") or [] if alive(f.get("handle"))]
    segments = {s["handle"].upper(): s for s in table.get("pipe_segments") or [] if alive(s.get("handle"))}
    names = {}
    for fitting in fittings:
        where = (listing.get(fitting["handle"].upper()) or listing.get(fitting["handle"]) or ("?",))[0]
        names[fitting["handle"].upper()] = fitting.get("tag") or f"{fitting.get('fitting_type')} {where}"

    def peer(text):
        if not text or ":" not in text:
            return None
        handle, port = text.split(":", 1)
        return f"{names[handle.upper()]}:{port}" if handle.upper() in names else None

    def segment(handle):
        record = segments.get((handle or "").upper())
        if not record:
            return None
        return {
            "start": [num(v) for v in record["start"]["point"]], "end": [num(v) for v in record["end"]["point"]],
            "start_peer": peer(record["start"].get("peer")), "end_peer": peer(record["end"].get("peer")),
        }

    lines = sorted(({"tag": line["tag"], "runs": [r for r in line.get("runs") or []]}
                    for line in table.get("pipe_lines") or [] if alive(line.get("handle"))), key=lambda l: l["tag"])
    runs = sorted(({"tag": run["tag"], "line_tag": run.get("line_tag"),
                    "segments": [s for s in (segment(h) for h in run.get("segments") or []) if s]}
                   for run in table.get("pipe_runs") or [] if alive(run.get("handle"))), key=lambda r: r["tag"])
    for line in lines:
        line["runs"] = [tag for tag in line["runs"] if any(run["tag"] == tag for run in runs)]
    fits = sorted(({"name": names[f["handle"].upper()], "run_tag": f.get("run_tag"), "fitting_type": f.get("fitting_type"),
                    "tag": f.get("tag"),
                    "ports": {port: segment(bound) for port, bound in sorted((f.get("ports") or {}).items())}}
                   for f in fittings), key=lambda f: json.dumps(f, ensure_ascii=False, sort_keys=True))
    connectors = sorted(({"kind": c.get("kind"), "line_tag": c.get("line_tag"), "peer_drawing": c.get("peer_drawing"),
                          "paired": bool(c.get("peer")), "upstream": c.get("upstream"), "downstream": c.get("downstream")}
                         for c in table.get("connectors") or [] if alive(c.get("handle"))),
                        key=lambda c: json.dumps(c, ensure_ascii=False, sort_keys=True))
    return {"pipe_lines": lines, "pipe_runs": runs, "fittings": fits, "connectors": connectors}


def snapshot(gui, doc=None):
    """One reading of a drawing: its authored rows and its effective topology."""
    listing = entities(gui)
    table = topology(gui, doc)
    return {"authored": authored(gui, listing), "topology": effective(table, listing), "raw_topology": table,
            "handles": {h: (where, author) for h, (where, author, _e) in listing.items()}}


def wait_same(u1, u2, seconds=15, also=None):
    """Poll until the two drawings read the same (and `also(s1, s2)` holds);
    returns the last pair of readings either way."""
    deadline = time.time() + seconds
    while True:
        s1, s2 = snapshot(u1), snapshot(u2)
        same = s1["authored"] == s2["authored"] and s1["topology"] == s2["topology"]
        if same and (also is None or also(s1, s2)):
            return s1, s2, True
        if time.time() >= deadline:
            return s1, s2, False
        time.sleep(0.3)


def handles_where(reading, keep):
    return [h for h, (_where, author) in reading["handles"].items() if keep(author)]


def op_kinds(frames):
    """The kinds an op (sent) or an op_applied (received) carries, in order."""
    out = []
    for frame in frames:
        payload = frame.get("payload")
        changes = [payload] + list((payload or {}).get("changes") or []) if payload is not None else []
        for change in frame.get("changes") or []:
            changes.append(change)
            changes.extend(change.get("changes") or [])
        out.append([c.get("kind") for c in changes if isinstance(c, dict)])
    return out


def not_diverged(*guis):
    states = [g.status() for g in guis]
    fine = all(not s.get("diverged") and not s.get("last_apply_error") for s in states)
    return fine, [(s.get("diverged"), s.get("last_apply_error")) for s in states]
