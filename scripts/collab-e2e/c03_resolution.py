# -*- coding: utf-8 -*-
"""VAL-CONF-010 … 016：冲突裁决（conflict-resolution）。

只起协同服务器（带 --tie-window-ms，并列条目是这样造出来的），两个 WS 会话
造冲突，裁决与检索走 HTTP（契约写的工具是 curl，这里用 m7h.Server.http，
请求与响应原文落在 logs/），SQLite 只读查询。不起 GUI，不涉及指针与焦点。
"""
import sys
import time
import urllib.parse

import m7h

R = m7h.Results("c03_resolution")
DRAWING = "PID-1101"
WINDOW_MS = 1200
FIELD = "设计压力"


def send_op(ws, base, op_id, value):
    ws.send({"type": "op", "session_id": ws.session_id, "base_revision": base, "op_id": op_id,
             "payload": {"kind": "set_property", "object_ref": "P-1001", "field": FIELD, "value": value}})


def revision(srv):
    return (srv.http("GET", f"/drawing/{DRAWING}/objects")[1] or {}).get("revision")


def field_value(srv):
    body = srv.http("GET", f"/drawing/{DRAWING}/objects?object_ref=P-1001")[1] or {}
    items = body.get("items") or []
    return ((items[0].get("fields") or {}).get(FIELD) or {}).get("value") if items else None


def make_conflict(srv, s1, s2, tag):
    """s1 提交 1.6MPa、s2 提交 2.5MPa，同一基线。返回新条目的行。"""
    known = {r["conflict_id"] for r in srv.sql("select conflict_id from conflicts")}
    base = revision(srv)
    send_op(s1, base, f"op-{tag}-s1", "1.6MPa")
    send_op(s2, base, f"op-{tag}-s2", "2.5MPa")
    m7h.wait_until(lambda: len(srv.sql("select conflict_id from conflicts")) > len(known), 10)
    rows = [r for r in srv.sql("select * from conflicts") if r["conflict_id"] not in known]
    return rows[0] if rows else {}


def resolve(srv, conflict_id, body, name):
    status, response = srv.http("POST", f"/conflicts/{conflict_id}/resolve", body)
    R.dump(f"c03-{name}-request.json", {"method": "POST", "path": f"/conflicts/{conflict_id}/resolve", "body": body})
    R.dump(f"c03-{name}-response.json", {"status": status, "body": response})
    return status, response or {}


def row_of(srv, conflict_id):
    rows = srv.sql("select * from conflicts where conflict_id=?", (conflict_id,))
    return rows[0] if rows else {}


try:
    srv = m7h.Server(port=3240, tag="c03-srv", extra=["--tie-window-ms", WINDOW_MS]).start(fresh=True)
    assert srv.wait_health(), "server did not come up"
    url = f"ws://127.0.0.1:{srv.port}/ws"
    s1, s2 = m7h.Ws(url, "c03-s1"), m7h.Ws(url, "c03-s2")
    s1.hello("u1", DRAWING)
    s2.hello("u2", DRAWING)

    # ── 010：采用甲方 ──────────────────────────────────────────────────
    c = make_conflict(srv, s1, s2, "010")
    C = c.get("conflict_id")
    R.check("VAL-CONF-010", bool(C) and c.get("value_a") == "1.6MPa" and c.get("value_b") == "2.5MPa" and c.get("status") == "待裁决",
            f"前置（同 VAL-CONF-004）：待裁决条目 {C}，value_a={c.get('value_a')}，value_b={c.get('value_b')}")
    status, body = resolve(srv, C, {"resolution": "采用甲方", "resolved_by": "u4"}, "010")
    R.check("VAL-CONF-010", status == 200, f"u4 裁决 采用甲方 → HTTP {status}：{body}")
    row = row_of(srv, C)
    R.dump("c03-010-row.json", row)
    R.check("VAL-CONF-010", row.get("status") == "已裁决" and row.get("resolution") == "采用甲方" and row.get("resolved_by") == "u4",
            f"conflicts 行：status={row.get('status')} resolution={row.get('resolution')} resolved_by={row.get('resolved_by')}")
    R.check("VAL-CONF-010", field_value(srv) == "1.6MPa", f"对象字段最终值为 value_a：{FIELD}={field_value(srv)}")
    pushed = s1.wait_for(lambda m: m.get("type") == "op_applied" and m.get("op_id") == f"resolve-{C}", 5)
    R.note(f"裁决结果作为 op_applied 推给在线会话：{pushed}")

    # ── 011：采用乙方 ──────────────────────────────────────────────────
    c2 = make_conflict(srv, s1, s2, "011")
    C2 = c2.get("conflict_id")
    R.check("VAL-CONF-011", bool(C2) and c2.get("value_a") == "1.6MPa" and c2.get("value_b") == "2.5MPa",
            f"新的同字段冲突 {C2}：value_a={c2.get('value_a')}，value_b={c2.get('value_b')}")
    status, body = resolve(srv, C2, {"resolution": "采用乙方", "resolved_by": "u4"}, "011")
    row = row_of(srv, C2)
    R.dump("c03-011-row.json", row)
    R.check("VAL-CONF-011", status == 200 and row.get("status") == "已裁决" and row.get("resolution") == "采用乙方"
            and row.get("resolved_by") == "u4",
            f"HTTP {status}；行：status={row.get('status')} resolution={row.get('resolution')} resolved_by={row.get('resolved_by')}")
    R.check("VAL-CONF-011", field_value(srv) == "2.5MPa", f"字段最终值为 value_b：{FIELD}={field_value(srv)}")

    # ── 012：人工改值 ──────────────────────────────────────────────────
    c3 = make_conflict(srv, s1, s2, "012")
    C3 = c3.get("conflict_id")
    status, body = resolve(srv, C3, {"resolution": "人工改值", "value": "2.0MPa", "resolved_by": "u4"}, "012")
    row = row_of(srv, C3)
    R.dump("c03-012-row.json", row)
    R.check("VAL-CONF-012", status == 200 and row.get("status") == "已裁决" and row.get("resolution") == "人工改值"
            and row.get("resolved_by") == "u4",
            f"HTTP {status}；行：status={row.get('status')} resolution={row.get('resolution')} resolved_by={row.get('resolved_by')}")
    final = field_value(srv)
    R.check("VAL-CONF-012", final == "2.0MPa" and final not in (c3.get("value_a"), c3.get("value_b")),
            f"字段最终值 {final}，既不是 value_a（{c3.get('value_a')}）也不是 value_b（{c3.get('value_b')}）")
    R.check("VAL-CONF-012", row.get("resolved_value") == "2.0MPa" and body.get("resolved_value") == "2.0MPa",
            f"自定义值可读回：conflicts.resolved_value={row.get('resolved_value')}，响应里 resolved_value={body.get('resolved_value')}")

    # ── 013：审计 ──────────────────────────────────────────────────────
    st, audit = srv.http("GET", "/audit?tag=P-1001&limit=200")
    R.dump("c03-013-audit-http.json", audit)
    rulings = srv.sql("select seq, object_ref, user_id, action, change_text from audit_log where user_id='u4' order by seq")
    R.dump("c03-013-audit-u4.json", rulings)
    finals = {C: "1.6MPa", C2: "2.5MPa", C3: "2.0MPa"}
    for cid, final in finals.items():
        mine = [r for r in rulings if cid in r["change_text"]]
        ok = (len(mine) >= 1 and all(r["object_ref"] == "P-1001" for r in mine)
              and all("1.6MPa" in r["change_text"] and "2.5MPa" in r["change_text"] and final in r["change_text"] for r in mine))
        R.check("VAL-CONF-013", ok, f"{cid}：{len(mine)} 行裁决记录，user_id=u4、object_ref=P-1001，change_text 含 value_a、value_b 与最终值 {final}："
                                    f"{mine[0]['change_text'] if mine else None}")
    R.check("VAL-CONF-013", st == 200 and sum(1 for r in (audit or {}).get("items", []) if r.get("user_id") == "u4") >= 3,
            f"GET /audit?tag=P-1001 → HTTP {st}，其中 u4 的行 {sum(1 for r in (audit or {}).get('items', []) if r.get('user_id') == 'u4')} 条")

    # ── 014：不可重复裁决 ──────────────────────────────────────────────
    before_rows = len([r for r in srv.sql("select change_text from audit_log where user_id='u4'") if C in r["change_text"]])
    status, body = resolve(srv, C, {"resolution": "采用乙方", "resolved_by": "u4"}, "014")
    R.check("VAL-CONF-014", status == 409 and body.get("ok") is False and body.get("code") == "CONFLICT_ALREADY_RESOLVED",
            f"对已裁决的 {C} 再裁决 → HTTP {status}，ok={body.get('ok')}，code={body.get('code')}")
    row = row_of(srv, C)
    after_rows = len([r for r in srv.sql("select change_text from audit_log where user_id='u4'") if C in r["change_text"]])
    R.check("VAL-CONF-014", row.get("resolution") == "采用甲方" and row.get("resolved_by") == "u4" and before_rows == after_rows,
            f"行不变：resolution={row.get('resolution')} resolved_by={row.get('resolved_by')}；该条目的裁决审计行 {before_rows} → {after_rows}")

    # ── 015：非管理员 ──────────────────────────────────────────────────
    c4 = make_conflict(srv, s1, s2, "015")
    C4 = c4.get("conflict_id")
    status, body = resolve(srv, C4, {"resolution": "采用甲方", "resolved_by": "u3"}, "015")
    R.check("VAL-CONF-015", status == 403 and body.get("code") == "PERM_DENIED_ROLE" and body.get("level") == "role",
            f"u3（校对人员）裁决 → HTTP {status}，code={body.get('code')}，level={body.get('level')}")
    row = row_of(srv, C4)
    R.check("VAL-CONF-015", row.get("status") == "待裁决" and row.get("resolution") is None,
            f"条目仍为 {row.get('status')}，resolution={row.get('resolution')}")

    # ── 016：检索 ──────────────────────────────────────────────────────
    counts = srv.sql("select status, count(*) n from conflicts group by status order by status")
    R.dump("c03-016-counts.json", counts)

    def listing(status_text, name):
        q = urllib.parse.urlencode({"drawing_id": DRAWING, "status": status_text})
        st, body = srv.http("GET", f"/conflicts?{q}")
        R.dump(f"c03-016-{name}.json", {"status": st, "query": q, "body": body})
        return st, body or {}

    st, waiting = listing("待裁决", "pending")
    items = waiting.get("items") or []
    filled = all(all(i.get(k) for k in ("conflict_id", "object_ref", "field", "status")) for i in items)
    R.check("VAL-CONF-016", st == 200 and len(items) == 1 and waiting.get("total") == 1 and filled,
            f"status=待裁决 → HTTP {st}，items {len(items)} 条（表里 {counts}），四个字段均非空：{filled}")
    st, done = listing("已裁决", "resolved")
    R.check("VAL-CONF-016", st == 200 and len(done.get("items") or []) == 3,
            f"status=已裁决 → {len(done.get('items') or [])} 条")
    R.dump("c03-server-log.txt", m7h.this_run(srv.log_text()))
    s1.close(); s2.close(); srv.kill()
finally:
    m7h.cleanup()
    ok = R.summary()

sys.exit(0 if ok else 1)
