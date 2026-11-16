# -*- coding: utf-8 -*-
"""VAL-CONF-001 … 005：属性级冲突的三段判定（conflict-field-merge）。

只起协同服务器，两个 WS 会话直连（契约写的工具是 Node 26 的 WebSocket，这里用
m7h.Ws，帧原文逐条落在 logs/ws-*.jsonl），SQLite 只读查询，外加服务器的语义
读回 `GET /drawing/PID-1101/objects`。不起 GUI，不涉及指针与焦点。

三台服务器，各用自己的库：
  A  默认参数            001
  B  默认参数            002、003
  C  --tie-window-ms     004、005（005 要求冲突条数保持 004 之后的计数）
"""
import json
import sys
import time

import m7h

R = m7h.Results("c01_field_merge")
DRAWING = "PID-1101"
WINDOW_MS = 1500


def op(ws, base, op_id, field, value, obj="P-1001"):
    ws.send({"type": "op", "session_id": ws.session_id, "base_revision": base, "op_id": op_id,
             "payload": {"kind": "set_property", "object_ref": obj, "field": field, "value": value}})


def applied(ws, op_id, seconds=10):
    return ws.wait_for(lambda m: m.get("type") == "op_applied" and m.get("op_id") == op_id, seconds)


def field_value(srv, field, obj="P-1001"):
    status, body = srv.http("GET", f"/drawing/{DRAWING}/objects?object_ref={obj}")
    items = (body or {}).get("items") or []
    value = ((items[0].get("fields") or {}).get(field) or {}).get("value") if items else None
    return status, body, value


def two_sessions(srv, tag):
    url = f"ws://127.0.0.1:{srv.port}/ws"
    s1, s2 = m7h.Ws(url, f"{tag}-s1"), m7h.Ws(url, f"{tag}-s2")
    w1, w2 = s1.hello("u1", DRAWING), s2.hello("u2", DRAWING)
    return s1, s2, w1, w2


def conflicts(srv):
    return srv.sql("select * from conflicts order by conflict_id")


try:
    # ── A：VAL-CONF-001 ────────────────────────────────────────────────
    a = m7h.Server(port=3240, tag="c01a-srv").start(fresh=True)
    assert a.wait_health(), "server A did not come up"
    s1, s2, w1, w2 = two_sessions(a, "c01a")
    R.check("VAL-CONF-001", (w1 or {}).get("base_revision") == 0 and (w2 or {}).get("base_revision") == 0,
            f"两个会话同图 base_revision=0：s1 {w1 and w1.get('base_revision')}，s2 {w2 and w2.get('base_revision')}")
    op(s1, 0, "op-001-s1", "设计压力", "1.6MPa")
    a1 = applied(s1, "op-001-s1")
    R.check("VAL-CONF-001", bool(a1) and a1.get("revision") == 1, f"s1 改 设计压力 → op_applied revision=1：{a1}")
    # s2 没有刷新：仍带 0。
    op(s2, 0, "op-001-s2", "公称直径", "DN50")
    a2 = applied(s2, "op-001-s2")
    R.check("VAL-CONF-001", bool(a2) and a2.get("revision") == 2,
            f"s2 带 base_revision=0 改 公称直径 → op_applied revision=2：{a2}")
    time.sleep(0.5)
    got_conflict = s2.incoming("conflict") + s1.incoming("conflict")
    got_error = s2.incoming("error")
    R.check("VAL-CONF-001", not got_conflict and not got_error,
            f"s2 未收到 conflict（也没有 error）：conflict={got_conflict} error={got_error}")
    rows = conflicts(a)
    R.check("VAL-CONF-001", len(rows) == 0, f"select count(*) from conflicts = {len(rows)}")
    st, body, pressure = field_value(a, "设计压力")
    _, _, diameter = field_value(a, "公称直径")
    R.dump("c01-001-objects.json", body)
    R.check("VAL-CONF-001", pressure == "1.6MPa" and diameter == "DN50",
            f"服务器语义读回（HTTP {st}）：设计压力={pressure}，公称直径={diameter}")
    s1.close(); s2.close(); a.kill()

    # ── B：VAL-CONF-002、003 ───────────────────────────────────────────
    b = m7h.Server(port=3240, tag="c01b-srv").start(fresh=True)
    assert b.wait_health(), "server B did not come up"
    s1, s2, w1, w2 = two_sessions(b, "c01b")
    op(s1, 0, "op-002-s1", "设计压力", "1.6MPa")
    a1 = applied(s1, "op-002-s1")
    R.check("VAL-CONF-002", bool(a1) and a1.get("revision") == 1, f"s1 设计压力=1.6MPa → revision=1：{a1}")
    op(s2, 0, "op-002-s2", "设计压力", "2.5MPa")
    refused = s2.wait_for(lambda m: m.get("type") == "error", 10)
    R.check("VAL-CONF-002", bool(refused) and refused.get("code") == "OP_SUPERSEDED"
            and "基线" in refused.get("message", "") and "落后" in refused.get("message", ""),
            f"s2 带 base_revision=0 改同一字段 → error，表示基线落后：{refused}")
    time.sleep(0.5)
    R.check("VAL-CONF-002", not [m for m in s2.incoming("op_applied") if m.get("op_id") == "op-002-s2"],
            "s2 的 op 没有 op_applied")
    rows = conflicts(b)
    R.check("VAL-CONF-002", len(rows) == 0, f"select count(*) from conflicts = {len(rows)}")
    st, body, pressure = field_value(b, "设计压力")
    R.dump("c01-002-objects.json", body)
    R.check("VAL-CONF-002", pressure == "1.6MPa", f"字段最终值（先到者生效）：设计压力={pressure}")

    st, audit = b.http("GET", "/audit?tag=P-1001")
    R.dump("c01-003-audit-http.json", audit)
    last = b.sql("select * from audit_log where object_ref='P-1001' order by seq desc limit 1")
    R.dump("c01-003-audit-row.json", last)
    row = last[0] if last else {}
    R.check("VAL-CONF-003", row.get("object_ref") == "P-1001" and row.get("action") == "property_superseded"
            and row.get("user_id") == "u2",
            f"最新一行：object_ref={row.get('object_ref')} action={row.get('action')} user_id={row.get('user_id')}")
    text = row.get("change_text") or ""
    R.check("VAL-CONF-003", "2.5MPa" in text and "1.6MPa" in text, f"change_text 同时含被覆盖值与生效值：{text}")
    nine = ["module", "action", "at", "object_ref", "user_id", "company", "ip", "workspace", "change_text"]
    empty = [k for k in nine if row.get(k) in (None, "")]
    R.check("VAL-CONF-003", not empty, f"九个字段均非空（空的：{empty}）：{ {k: row.get(k) for k in nine} }")
    http_rows = (audit or {}).get("items") or []
    R.check("VAL-CONF-003", st == 200 and any(r.get("action") == "property_superseded" for r in http_rows),
            f"GET /audit?tag=P-1001 → HTTP {st}，{len(http_rows)} 行，含 property_superseded")
    s1.close(); s2.close(); b.kill()

    # ── C：VAL-CONF-004、005（并列窗口）────────────────────────────────
    c = m7h.Server(port=3240, tag="c01c-srv", extra=["--tie-window-ms", WINDOW_MS]).start(fresh=True)
    assert c.wait_health(), "server C did not come up"
    R.note(f"服务器 C 带 --tie-window-ms {WINDOW_MS}：{m7h.lines_with(c.log_text(), '监听')}")
    s1, s2, w1, w2 = two_sessions(c, "c01c")
    R.check("VAL-CONF-004", (w1 or {}).get("base_revision") == 0 and (w2 or {}).get("base_revision") == 0,
            "两个会话 base_revision 均为 0")
    op(s1, 0, "op-004-s1", "设计压力", "1.6MPa")
    op(s2, 0, "op-004-s2", "设计压力", "2.5MPa")
    c1 = s1.wait_for(lambda m: m.get("type") == "conflict", 10)
    c2 = s2.wait_for(lambda m: m.get("type") == "conflict", 10)
    rows = conflicts(c)
    R.dump("c01-004-conflicts.json", rows)
    row = rows[0] if rows else {}
    R.check("VAL-CONF-004", len(rows) == 1, f"conflicts 表新增 1 行：{len(rows)} 行")
    want = {"drawing_id": DRAWING, "object_ref": "P-1001", "field": "设计压力", "value_a": "1.6MPa",
            "value_b": "2.5MPa", "base_revision": 0, "status": "待裁决", "resolution": None, "resolved_by": None}
    wrong = {k: row.get(k) for k, v in want.items() if row.get(k) != v}
    R.check("VAL-CONF-004", not wrong, f"各列与契约一致（不一致的：{wrong}）：{row}")
    R.check("VAL-CONF-004", bool(c1) and bool(c2) and c1.get("conflict_id") == row.get("conflict_id")
            == c2.get("conflict_id"),
            f"双方各收到 conflict，conflict_id 与表中一致：s1={c1} s2={c2}")
    time.sleep(WINDOW_MS / 1000 + 0.7)
    late = [m for m in s1.incoming("op_applied") + s2.incoming("op_applied")
            if m.get("op_id") in ("op-004-s1", "op-004-s2")]
    st, body, pressure = field_value(c, "设计压力")
    R.check("VAL-CONF-004", not late and pressure is None and (body or {}).get("revision") == 0,
            f"窗口过后两个 op 都没有生效：op_applied={late}，设计压力={pressure}，revision={(body or {}).get('revision')}")

    # 005：s1 一路写到修订 3（公称直径=DN50 生效于修订 3），s2 带基线 1。
    base = 0
    for op_id, field, value in (("op-005-a", "绝热等级", "H"), ("op-005-b", "流体介质", "工艺水"),
                                ("op-005-c", "公称直径", "DN50")):
        op(s1, base, op_id, field, value)
        got = applied(s1, op_id, 10)
        base = (got or {}).get("revision", -1)
    R.check("VAL-CONF-005", base == 3, f"s1 在修订 3 写入 公称直径=DN50：最后一次 op_applied revision={base}")
    before = len(conflicts(c))
    op(s2, 1, "op-005-s2", "公称直径", "DN80")
    told = s2.wait_for(lambda m: m.get("type") == "error" and "op-005-s2" in m.get("message", ""), 10)
    R.check("VAL-CONF-005", bool(told) and told.get("code") == "OP_SUPERSEDED",
            f"s2 带 base_revision=1 写 DN80 → 覆盖通知（error，不是 conflict）：{told}")
    time.sleep(0.5)
    after = len(conflicts(c))
    R.check("VAL-CONF-005", before == after == 1, f"conflicts 行数不增：之前 {before}，之后 {after}")
    st, body, diameter = field_value(c, "公称直径")
    R.dump("c01-005-objects.json", body)
    R.check("VAL-CONF-005", diameter == "DN50", f"字段最终值：公称直径={diameter}")
    R.dump("c01-server-c-log.txt", m7h.this_run(c.log_text()))
    s1.close(); s2.close(); c.kill()
finally:
    m7h.cleanup()
    ok = R.summary()

sys.exit(0 if ok else 1)
