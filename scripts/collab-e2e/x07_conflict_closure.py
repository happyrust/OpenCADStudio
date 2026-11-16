# -*- coding: utf-8 -*-
"""VAL-CROSS-007：冲突从生成到裁决的闭环。

服务器带 --tie-window-ms 起（并列是这样造出来的，见 c01/c03）。u1 是 GUI
（3210），u2 是脚本里的 WS 会话（契约写的是 Node WS，这里用 m7h.Ws，帧原文
落在 logs/ws-x07-u2.jsonl）。两边从同一基线把 P-1001 的 设计压力 写成
1.6MPa 与 2.5MPa。u4 经服务器的 REST 裁决「采用乙方」。

证据：logs/x07-*.json、logs/gui-x07-u1.err.log（含帧原文）、
logs/ws-x07-u2.jsonl、shots/x07-u1-conflict.png（面板上有待裁决条目）、
shots/x07-u1-resolved.png。
"""
import json
import sys
import time

import m7h

R = m7h.Results("x07_conflict_closure")
DRAWING = "PID-1101"
FIELD = "设计压力"
WINDOW_MS = 3000


def sections(gui):
    status, body = gui.op("properties")
    body = body or {}
    return body.get("sections") or (body.get("result") or {}).get("sections") or []


def row_by_label(secs, label):
    for sec in secs:
        for row in sec.get("properties") or []:
            if row.get("label") == label:
                return row
    return None


try:
    if not m7h.SEED_DWG.exists():
        R.check("setup", False, f"种子图不存在：{m7h.SEED_DWG}")
        raise SystemExit(1)
    dwg_a = m7h.fresh_copy("x07-PID-1101-a.dwg", m7h.SEED_DWG)
    srv = m7h.Server(3240, tag="x07-srv", extra=["--tie-window-ms", WINDOW_MS]).start(fresh=True)
    R.check("setup", srv.wait_health(), f"服务器 /health = {srv.health()}（并列窗口 {WINDOW_MS} 毫秒）")

    u1 = m7h.Gui("x07-u1", 3210, dwg=dwg_a, user="u1").start()
    R.check("setup", u1.wait_ready(180) is not None, "u1 GUI 就绪")
    s1 = u1.wait_status(lambda s: s.get("connected") and s.get("session_id"), 30)
    R.check("setup", s1 is not None, f"u1 已连接: session={s1 and s1.get('session_id')}")
    u2 = m7h.Ws(f"ws://127.0.0.1:{srv.port}/ws", "x07-u2")
    welcome2 = u2.hello("u2", DRAWING)
    u2.keep_alive()
    R.check("setup", (welcome2 or {}).get("type") == "welcome", f"u2（WS）已连接: {json.dumps(welcome2, ensure_ascii=False)[:200]}")
    base1, base2 = (s1 or {}).get("base_revision"), (welcome2 or {}).get("base_revision")
    R.check("VAL-CROSS-007", base1 == base2 == 0, f"u1 与 u2 同图同 base_revision（{base1} / {base2}）")

    doc1 = u1.doc_id()
    h1 = u1.tagged().get("P-1001")
    R.check("setup", bool(h1), f"u1 图上 P-1001 的句柄 {h1}")
    u1.run("ZOOM EXTENTS")

    # u1 经 GUI 写 1.6MPa；op 到服务器后在并列窗口里等。
    st, put1 = u1.api("PUT", f"/entities/{h1}/xdata/PID_AUTHOR", [{"code": 1000, "value": 'props={"%s":"1.6MPa"}' % FIELD}])
    R.dump("x07-u1-put.json", {"http_status": st, "body": put1})
    held = m7h.wait_until(lambda: m7h.lines_with(srv.log_text(), "入站 op", "user=u1", "并列窗口内暂存"), 5)
    R.check("VAL-CROSS-007", st == 200 and (put1 or {}).get("ok") is True and bool(held),
            f"u1 经 GUI 改 P-1001 的 {FIELD} 为 1.6MPa: PUT -> {st}；服务器: {held[-1] if held else None}")
    # u2 经 WS 在同一基线上写 2.5MPa。
    u2.send({"type": "op", "session_id": u2.session_id, "base_revision": 0, "op_id": "op-x07-u2",
             "payload": {"kind": "set_property", "object_ref": "P-1001", "field": FIELD, "value": "2.5MPa"}})
    m7h.wait_until(lambda: srv.sql("select conflict_id from conflicts"), 10)
    rows = srv.sql("select * from conflicts")
    R.dump("x07-conflicts-pending.json", rows)
    c = rows[0] if rows else {}
    CX = c.get("conflict_id")
    R.check("VAL-CROSS-007", len(rows) == 1 and c.get("status") == "待裁决" and c.get("object_ref") == "P-1001"
            and c.get("field") == FIELD and {c.get("value_a"), c.get("value_b")} == {"1.6MPa", "2.5MPa"},
            f"conflicts 待裁决行 $CX={CX}: object_ref={c.get('object_ref')} field={c.get('field')} "
            f"value_a={c.get('value_a')} value_b={c.get('value_b')} status={c.get('status')}")

    # u1 的 GUI 收到 conflict，面板显示条目。
    s1c = u1.wait_status(lambda s: any(e.get("conflict_id") == CX for e in s.get("conflicts") or []), 10) or u1.status()
    cf = [f for f in m7h.frames(u1.err_text(), "<-", "conflict") if f.get("conflict_id") == CX]
    R.dump("x07-u1-conflict-frames.json", cf)
    R.dump("x07-u1-status-conflict.json", s1c)
    R.check("VAL-CROSS-007", bool(cf) and cf[-1].get("object_ref") == "P-1001" and cf[-1].get("field") == FIELD,
            f"u1 的 GUI 收到的 conflict 帧原文: {json.dumps(cf[-1] if cf else None, ensure_ascii=False)}")
    R.check("VAL-CROSS-007", any(e.get("conflict_id") == CX for e in s1c.get("conflicts") or []) and s1c.get("base_revision") == 0,
            f"u1 /collab/status.conflicts = {json.dumps(s1c.get('conflicts'), ensure_ascii=False)}（基线仍为 {s1c.get('base_revision')}）")
    time.sleep(1.0)
    bounds = u1.ui_bounds("pid-collab")
    R.dump("x07-u1-ui-bounds-conflict.json", bounds)
    row_id = f"pid-collab-conflict-{CX}"
    R.check("VAL-CROSS-007", row_id in bounds, f"协同面板画出了冲突条目（{row_id} = {bounds.get(row_id)}）")
    st, resp, shot = u1.capture(m7h.SHOTS / "x07-u1-conflict.png")
    R.check("VAL-CROSS-007", st == 200 and shot, "u1 截图 shots/x07-u1-conflict.png")
    u2c = u2.wait_for(lambda m: m.get("type") == "conflict" and m.get("conflict_id") == CX, 5)
    R.note(f"u2（WS）也收到同一条 conflict: {json.dumps(u2c, ensure_ascii=False)}")
    R.note(f"裁决之前 u1 本地的值（它自己写的）: props={u1.author(h1).get('props')!r}")

    # u4 裁决：采用乙方。
    status, body = srv.http("POST", f"/conflicts/{CX}/resolve", {"resolution": "采用乙方", "resolved_by": "u4"})
    R.dump("x07-resolve-response.json", {"http_status": status, "body": body})
    R.check("VAL-CROSS-007", status == 200, f"u4 POST /conflicts/{CX}/resolve 采用乙方 -> HTTP {status}: {json.dumps(body, ensure_ascii=False)[:300]}")

    got = m7h.wait_until(lambda: "2.5MPa" in (u1.author(h1).get("props") or ""), 15)
    pushed = [f for f in m7h.frames(u1.err_text(), "<-", "op_applied") if f.get("op_id") == f"resolve-{CX}"]
    R.dump("x07-u1-ruling-frames.json", pushed)
    st, xd1 = u1.xdata(h1)
    R.dump("x07-u1-xdata-after.json", xd1)
    R.check("VAL-CROSS-007", bool(pushed) and bool(got),
            f"裁决后 u1 收到推送（{json.dumps(pushed[-1] if pushed else None, ensure_ascii=False)}），"
            f"xdata 读回 props={u1.author(h1).get('props')!r}")
    u1.op("select", doc1, handles=[h1])
    secs = sections(u1)
    R.dump("x07-u1-properties-after.json", secs)
    row = row_by_label(secs, FIELD)
    R.check("VAL-CROSS-007", row is not None and row.get("value") == "2.5MPa",
            f"u1 属性面板读回同值: {json.dumps(row, ensure_ascii=False)}")
    s1d = u1.status()
    time.sleep(1.0)
    bounds = u1.ui_bounds("pid-collab")
    R.check("VAL-CROSS-007", not s1d.get("conflicts") and row_id not in bounds and not s1d.get("diverged"),
            f"裁决后 u1 的待裁决条目清空（status.conflicts={s1d.get('conflicts')}，面板不再画 {row_id}），diverged={s1d.get('diverged')}")
    st, resp, shot = u1.capture(m7h.SHOTS / "x07-u1-resolved.png")
    R.check("VAL-CROSS-007", st == 200 and shot, "u1 截图 shots/x07-u1-resolved.png")

    row = (srv.sql("select * from conflicts where conflict_id=?", (CX,)) or [{}])[0]
    R.dump("x07-conflicts-resolved.json", row)
    R.check("VAL-CROSS-007", row.get("status") == "已裁决" and row.get("resolution") == "采用乙方" and row.get("resolved_by") == "u4",
            f"conflicts 中 {CX}: status={row.get('status')} resolution={row.get('resolution')} resolved_by={row.get('resolved_by')}")
    st, audit = srv.http("GET", "/audit?tag=P-1001&limit=200")
    R.dump("x07-audit-http.json", audit)
    items = (audit or {}).get("items") or []
    by_user = {u: [i for i in items if i.get("user_id") == u] for u in ("u1", "u2", "u4")}
    R.note("GET /audit?tag=P-1001: " + json.dumps([(i.get("user_id"), i.get("action"), i.get("object_ref"), i.get("change_text")) for i in items], ensure_ascii=False))
    R.check("VAL-CROSS-007", st == 200 and all(by_user[u] for u in by_user) and all(i.get("object_ref") == "P-1001" for i in items)
            and any("1.6MPa" in (i.get("change_text") or "") for i in by_user["u1"])
            and any("2.5MPa" in (i.get("change_text") or "") for i in by_user["u2"])
            and any(i.get("action") == "conflict_resolved" for i in by_user["u4"]),
            f"audit_log 同时有 u1、u2 的原始写入与 u4 的裁决，object_ref 均为 P-1001"
            f"（u1 {len(by_user['u1'])} 行、u2 {len(by_user['u2'])} 行、u4 {len(by_user['u4'])} 行）")
    u2.close()
except SystemExit:
    raise
except Exception as error:  # noqa: BLE001
    import traceback
    R.check("script", False, f"脚本异常: {error!r}\n{traceback.format_exc()}")
finally:
    m7h.cleanup()
    ok = R.summary()
sys.exit(0 if ok else 1)
