# -*- coding: utf-8 -*-
"""VAL-CONF-006 … 009、017：对象级与拓扑级冲突（conflict-object-topology）。

只起协同服务器，WS 会话直连（m7h.Ws，帧原文在 logs/ws-*.jsonl），SQLite 只读
查询，服务器的语义读回 `GET /drawing/PID-1101/objects` 与拓扑读回
`GET /drawing/PID-1101/topology`。不起 GUI，不涉及指针与焦点。

两台服务器，各用自己的库：
  A  默认参数            006、007、017
  B  --tie-window-ms     008、009
"""
import sys
import time

import m7h

R = m7h.Results("c02_object_topology")
DRAWING = "PID-1101"
WINDOW_MS = 1500


def send_op(ws, base, op_id, payload):
    ws.send({"type": "op", "session_id": ws.session_id, "base_revision": base, "op_id": op_id,
             "payload": payload})


def applied(ws, op_id, seconds=10):
    return ws.wait_for(lambda m: m.get("type") == "op_applied" and m.get("op_id") == op_id, seconds)


def error_for(ws, op_id, seconds=10):
    return ws.wait_for(lambda m: m.get("type") == "error" and op_id in m.get("message", ""), seconds)


def objects(srv, ref=None):
    status, body = srv.http("GET", f"/drawing/{DRAWING}/objects" + (f"?object_ref={ref}" if ref else ""))
    return status, body, [i["object_ref"] for i in (body or {}).get("items") or []]


def two_sessions(srv, tag):
    url = f"ws://127.0.0.1:{srv.port}/ws"
    s1, s2 = m7h.Ws(url, f"{tag}-s1"), m7h.Ws(url, f"{tag}-s2")
    s1.hello("u1", DRAWING)
    s2.hello("u2", DRAWING)
    return s1, s2


try:
    # ── A：VAL-CONF-006、007、017 ──────────────────────────────────────
    a = m7h.Server(port=3240, tag="c02a-srv").start(fresh=True)
    assert a.wait_health(), "server A did not come up"
    s1, s2 = two_sessions(a, "c02a")
    # P-1002 已存在于图上，带一个已知的设计压力。
    send_op(s1, 0, "op-006-seed", {"kind": "set_property", "object_ref": "P-1002", "field": "设计压力",
                                   "value": "1.0MPa"})
    seeded = applied(s1, "op-006-seed")
    _, _, before = objects(a)
    R.check("VAL-CONF-006", bool(seeded) and "P-1002" in before, f"前置：P-1002 在语义查询里（{before}），修订 {seeded and seeded.get('revision')}")
    audit_before = a.sql("select count(*) n from audit_log where object_ref='P-1002'")[0]["n"]

    send_op(s1, 1, "op-006-delete", {"kind": "delete_object", "object_ref": "P-1002"})
    deleted = applied(s1, "op-006-delete")
    R.check("VAL-CONF-006", bool(deleted) and deleted.get("revision") == 2, f"s1 删除 P-1002 → op_applied：{deleted}")
    # s2 没有看到这次删除：仍带基线 1。
    send_op(s2, 1, "op-006-s2", {"kind": "set_property", "object_ref": "P-1002", "field": "设计压力",
                                 "value": "2.5MPa"})
    told = error_for(s2, "op-006-s2")
    R.check("VAL-CONF-006", bool(told) and told.get("code") == "OBJECT_DELETED" and "1.0MPa" in told.get("message", ""),
            f"s2 收到通知，含删除前的旧值 1.0MPa：{told}")
    time.sleep(0.4)
    st, body, after = objects(a)
    R.dump("c02-006-objects-after.json", body)
    R.check("VAL-CONF-006", "P-1002" not in after and (body or {}).get("revision") == 2,
            f"语义查询里不再有 P-1002（{after}），修订停在 2：删除没有被回滚，s2 的修改没有生效")
    rows = a.sql("select seq, action, user_id, change_text from audit_log where object_ref='P-1002' order by seq")
    R.dump("c02-006-audit.json", rows)

    n_conf = a.sql("select count(*) n from conflicts where object_ref='P-1002'")[0]["n"]
    R.check("VAL-CONF-007", n_conf == 0, f"select count(*) from conflicts where object_ref='P-1002' = {n_conf}")
    new_rows = rows[audit_before:]
    actions = [r["action"] for r in new_rows]
    R.check("VAL-CONF-007", len(new_rows) >= 2 and "delete_object" in actions and "change_rejected_deleted" in actions,
            f"audit_log 新增 {len(new_rows)} 行：{actions}")
    refusal = next((r for r in new_rows if r["action"] == "change_rejected_deleted"), {})
    deletion = next((r for r in new_rows if r["action"] == "delete_object"), {})
    R.check("VAL-CONF-007", "1.0MPa" in (refusal.get("change_text") or "") and "1.0MPa" in (deletion.get("change_text") or ""),
            f"change_text 含旧值：删除行「{deletion.get('change_text')}」；被拒的修改「{refusal.get('change_text')}」")
    st, http_audit = a.http("GET", "/audit?tag=P-1002")
    R.dump("c02-007-audit-http.json", http_audit)
    R.check("VAL-CONF-007", st == 200 and (http_audit or {}).get("total", 0) >= 3,
            f"GET /audit?tag=P-1002 → HTTP {st}，total={(http_audit or {}).get('total')}")

    # 017：P-1003 是 L-1001 一条管路的端点。
    send_op(s1, 2, "op-017-seed", {"kind": "set_property", "object_ref": "P-1003", "field": "设计压力",
                                   "value": "0.6MPa"})
    applied(s1, "op-017-seed")
    send_op(s1, 3, "op-017-run", {"kind": "add_pipe_run", "line_tag": "L-1001", "points": [[0, 0], [100, 0]],
                                  "from": "P-1003"})
    run = applied(s1, "op-017-run")
    st, topo = a.http("GET", f"/drawing/{DRAWING}/topology")
    R.dump("c02-017-topology.json", topo)
    runs = (topo or {}).get("pipe_runs") or []
    R.check("VAL-CONF-017", bool(run) and any(r.get("from") == "P-1003" and r.get("line_tag") == "L-1001" for r in runs),
            f"前置：P-1003 被 L-1001 的拓扑引用为端点：{runs}")
    audit_before = a.sql("select count(*) n from audit_log where object_ref='P-1003'")[0]["n"]
    send_op(s2, 4, "op-017-delete", {"kind": "delete_object", "object_ref": "P-1003"})
    refused = error_for(s2, "op-017-delete")
    R.check("VAL-CONF-017", bool(refused) and refused.get("code") == "OBJECT_REFERENCED" and "L-1001" in refused.get("message", ""),
            f"删除被拒，code 表示被引用，message 列出引用方 L-1001：{refused}")
    time.sleep(0.4)
    no_applied = not [m for m in s2.incoming("op_applied") if m.get("op_id") == "op-017-delete"]
    _, _, still = objects(a)
    R.check("VAL-CONF-017", no_applied and "P-1003" in still, f"对象仍在语义查询里：{still}")
    rows = a.sql("select seq, action, user_id, change_text from audit_log where object_ref='P-1003' order by seq")
    R.dump("c02-017-audit.json", rows)
    attempt = [r for r in rows[audit_before:] if r["action"] == "delete_refused"]
    R.check("VAL-CONF-017", len(attempt) == 1 and attempt[0]["user_id"] == "u2",
            f"audit_log 记下被拒的尝试：{attempt}")
    R.dump("c02-server-a-log.txt", m7h.this_run(a.log_text()))
    s1.close(); s2.close(); a.kill()

    # ── B：VAL-CONF-008、009（并列窗口）────────────────────────────────
    b = m7h.Server(port=3240, tag="c02b-srv", extra=["--tie-window-ms", WINDOW_MS]).start(fresh=True)
    assert b.wait_health(), "server B did not come up"
    s1, s2 = two_sessions(b, "c02b")
    # L-1001 在图上。
    send_op(s1, 0, "op-008-line", {"kind": "create_pipe_line", "tag": "L-1001", "props": {"管道等级": "A1A"}})
    line = applied(s1, "op-008-line")
    st, topo_before = b.http("GET", f"/drawing/{DRAWING}/topology")
    R.dump("c02-009-topology-before.json", topo_before)
    R.check("VAL-CONF-008", bool(line) and (topo_before or {}).get("pipe_lines") == [{"tag": "L-1001", "runs": []}],
            f"前置：L-1001 在 PID-1101 上，修订 {(topo_before or {}).get('revision')}：{(topo_before or {}).get('pipe_lines')}")
    base = (line or {}).get("revision", 1)
    send_op(s1, base, "op-008-s1", {"kind": "add_pipe_run", "line_tag": "L-1001", "points": [[0, 0], [100, 0]]})
    send_op(s2, base, "op-008-s2", {"kind": "insert_fitting", "run_tag": "L-1001-01", "fitting_type": "gate_valve",
                                    "at": [50, 0]})
    c1 = s1.wait_for(lambda m: m.get("type") == "conflict", 10)
    c2 = s2.wait_for(lambda m: m.get("type") == "conflict", 10)
    rows = b.sql("select * from conflicts where object_ref='L-1001'")
    R.dump("c02-008-conflicts.json", rows)
    row = rows[0] if rows else {}
    R.check("VAL-CONF-008", len(rows) == 1 and row.get("field") == "topology" and row.get("status") == "待裁决"
            and bool(row.get("value_a")) and bool(row.get("value_b")),
            f"conflicts 新增 1 行，object_ref=L-1001、field={row.get('field')}、status={row.get('status')}，"
            f"value_a={row.get('value_a')}，value_b={row.get('value_b')}")
    R.check("VAL-CONF-008", bool(c1) and bool(c2) and c1.get("conflict_id") == row.get("conflict_id") == c2.get("conflict_id"),
            f"双方各收到 conflict：s1={c1}；s2={c2}")

    time.sleep(WINDOW_MS / 1000 + 0.7)
    st, topo_after = b.http("GET", f"/drawing/{DRAWING}/topology")
    R.dump("c02-009-topology-after.json", topo_after)
    R.check("VAL-CONF-009", topo_after == topo_before,
            f"裁决前拓扑与冲突前逐字相同（既不是 s1 的也不是 s2 的版本）：pipe_runs={(topo_after or {}).get('pipe_runs')}，revision={(topo_after or {}).get('revision')}")
    late = [m for m in s1.incoming("op_applied") + s2.incoming("op_applied") if m.get("op_id") in ("op-008-s1", "op-008-s2")]
    status = b.sql("select status from conflicts where object_ref='L-1001'")
    R.check("VAL-CONF-009", not late and [r["status"] for r in status] == ["待裁决"],
            f"两个 op 都没有 op_applied（{late}）；conflicts.status 仍为 {[r['status'] for r in status]}")
    R.dump("c02-server-b-log.txt", m7h.this_run(b.log_text()))
    s1.close(); s2.close(); b.kill()
finally:
    m7h.cleanup()
    ok = R.summary()

sys.exit(0 if ok else 1)
