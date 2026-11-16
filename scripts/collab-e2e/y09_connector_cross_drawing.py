# -*- coding: utf-8 -*-
"""VAL-CROSS-009：页间连接符跨图协同与属性传递。

u1（3210）打开 PID-1101，u2（3211）打开 PID-1102，连同一个服务器（3240）——两张图，
两个房间。两边各在自己的图上画一条管线 L-1001（u2 还在它上面画一条管路，好看见属性
顺着线→管路→管段往下走）。

1. u1 在 PID-1101 的 L-1001 上建管道流出连接符，目标 PID-1102：`PIDCONNECTOR pipe_out PID-1102`。
2. u2 在 PID-1102 的 L-1001 上建对应的流入连接符：`PIDCONNECTOR pipe_in`。
   服务器把两端配上，两个房间各收到一个它发起的 op（pair_connector）。
3. u1 把 L-1001 的管道等级改成 A1A；u2 那边的 L-1001（连接符的对端对象）读回 A1A。
4. u2 在 PID-1102 上对同一线号再建第二个流入 → ok=false、code=DUPLICATE_INFLOW_CONNECTOR。

检查读的是两个 GUI 的真实状态（PIDTOPOLOGY 的连接符表、连接符与线体实体的 PID_AUTHOR、
/collab/status）、两个 GUI 收发的帧原文、服务器的 /drawing/<图>/topology 与 objects、
库里两张图各自的 ops 与 audit_log。

证据：logs/y09-*.json、logs/gui-y09-*.err.log、logs/y09-srv.out.log、
shots/cross-connector-u1.png、shots/cross-connector-u2.png。
"""
import json
import sys
import time

import m7h
import yh

R = m7h.Results("y09_connector_cross_drawing")
A = "VAL-CROSS-009"
FIRST, SECOND = "PID-1101", "PID-1102"
LINE = "L-1001"
GRADE = "管道等级"


def at(gui, revision, seconds=15):
    return gui.wait_status(lambda s: s.get("base_revision") == revision and s.get("queued_ops") == 0, seconds) is not None


def applied(gui):
    """(op_id, revision, kinds) of every op_applied the GUI received."""
    frames = m7h.frames(gui.err_text(), "<-", "op_applied")
    return [(f.get("op_id"), f.get("revision"), [c.get("kind") for c in f.get("changes") or []]) for f in frames]


def connector_rows(gui):
    """Each connector of the drawing: the table's record, and what the entity carries."""
    table = yh.topology(gui)
    rows = []
    for record in table.get("connectors") or []:
        author = gui.author(record.get("handle"))
        rows.append({"kind": record.get("kind"), "line_tag": record.get("line_tag"),
                     "peer_drawing": record.get("peer_drawing"), "upstream": record.get("upstream"),
                     "downstream": record.get("downstream"), "record_peer": record.get("peer"),
                     "type": author.get("type"), "topology": author.get("topology"),
                     "connector_peer": author.get("connector_peer", "")})
    return rows


def by_role(gui, field):
    """{topology role: value of `field`} over the entities that carry one."""
    out = {}
    for _handle, (_where, author, _entity) in yh.entities(gui).items():
        role = author.get("topology") or ""
        if role.startswith("connector"):
            continue
        value = yh.props_of(author).get(field)
        if value is not None:
            out[role] = value
    return out


def server_connectors(srv, drawing):
    status, body = srv.http("GET", f"/drawing/{drawing}/topology")
    return status, ((body or {}).get("connectors") or [])


def server_value(srv, drawing, field):
    status, body = srv.http("GET", f"/drawing/{drawing}/objects?object_ref={LINE}")
    items = (body or {}).get("items") or []
    return ((items[0].get("fields") or {}).get(field) or {}).get("value") if items else None


def ops_rows(srv, drawing):
    return srv.sql("select revision, op_id, session_id, user_id, changes from ops where drawing_id=? order by revision",
                   (drawing,))


try:
    if not m7h.SEED_DWG.exists():
        R.check("setup", False, f"种子图不存在：{m7h.SEED_DWG}")
        raise SystemExit(1)
    # 种子数据里只有 PID-1101；第二张图放在同一个 PBS 节点下，两个人对它都有编辑权。
    seed = json.loads(m7h.SEED_JSON.read_text(encoding="utf-8"))
    seed["drawings"].append({"drawing_id": SECOND, "file_name": "PID-1102.dwg", "pbs_node_id": "P-UNIT",
                             "state": "协同编辑"})
    seed_path = m7h.ROOT / "seed-y09.json"
    seed_path.write_text(json.dumps(seed, ensure_ascii=False, indent=1), encoding="utf-8")
    dwg_a = m7h.fresh_copy("y09-PID-1101.dwg", m7h.SEED_DWG)
    dwg_b = m7h.fresh_copy("y09-PID-1102.dwg", m7h.SEED_DWG)
    srv = m7h.Server(3240, tag="y09-srv", seed=seed_path).start(fresh=True)
    R.check("setup", srv.wait_health(), f"服务器 /health = {srv.health()}")
    u1 = m7h.Gui("y09-u1", 3210, dwg=dwg_a, user="u1", drawing=FIRST).start()
    u2 = m7h.Gui("y09-u2", 3211, dwg=dwg_b, user="u2", drawing=SECOND).start()
    R.check("setup", u1.wait_ready(180) is not None and u2.wait_ready(180) is not None, "两个 GUI 就绪")
    s1 = u1.wait_status(lambda s: s.get("connected") and s.get("session_id"), 30)
    s2 = u2.wait_status(lambda s: s.get("connected") and s.get("session_id"), 30)
    R.check("setup", s1 is not None and s2 is not None
            and (s1 or {}).get("drawing_id") == FIRST and (s2 or {}).get("drawing_id") == SECOND,
            f"u1 连在 {(s1 or {}).get('drawing_id')}，u2 连在 {(s2 or {}).get('drawing_id')}：两张图、两个房间")

    # 两边各画自己的 L-1001；u2 再画一条管路。
    r1 = yh.draw(u1, "PIDPIPE", [(0, 0), (200, 0)])
    r2 = yh.draw(u2, "PIDPIPE", [(0, 0), (200, 0)])
    r3 = yh.draw(u2, "PIDPIPE run", [(0, 0), (200, 0)])
    ready = at(u1, 1) and at(u2, 2)
    R.check("setup", yh.ok(r1) and yh.ok(r2) and yh.ok(r3) and ready,
            f"两边各有管线 {LINE}（u2 另有一条管路）：PID-1101 到修订 1，PID-1102 到修订 2——各走各的修订")

    # ── ① u1 建流出，目标 PID-1102 ──────────────────────────────────────
    made_out = yh.draw(u1, f"PIDCONNECTOR pipe_out {SECOND}", [(120, 0)])
    reached = at(u1, 2)
    sent = [f for f in m7h.frames(u1.err_text(), "->", "op") if (f.get("payload") or {}).get("kind") == "create_connector"]
    R.dump("y09-u1-create-connector.json", {"response": yh.body_of(made_out), "sent": sent})
    said = (sent[-1].get("payload") if sent else {}) or {}
    R.check(A, yh.ok(made_out) and reached and said.get("line_tag") == LINE and said.get("connector") == "pipe_out"
            and said.get("target_drawing") == SECOND and said.get("at") == [120.0, 0.0],
            f"u1 在 {FIRST} 的 {LINE} 上建管道流出连接符（目标 {SECOND}）：命令成功，说给房间的是 "
            f"create_connector line_tag={said.get('line_tag')} connector={said.get('connector')} "
            f"target_drawing={said.get('target_drawing')} at={said.get('at')}")
    rows1 = connector_rows(u1)
    R.check(A, len(rows1) == 1 and rows1[0]["kind"] == "pipe_out" and rows1[0]["peer_drawing"] == SECOND
            and rows1[0]["connector_peer"] == "" and at(u2, 2),
            f"还没有对端：u1 的连接符记着目标图 {rows1[0]['peer_drawing'] if rows1 else None}，connector_peer 为空；"
            f"{SECOND} 的房间没有因此动（u2 仍在修订 2）")

    # ── ② u2 建对应的流入；服务器配对 ────────────────────────────────────
    made_in = yh.draw(u2, "PIDCONNECTOR pipe_in", [(20, 0)])
    paired = at(u2, 4) and at(u1, 3)
    a1, a2 = applied(u1), applied(u2)
    R.dump("y09-op-applied.json", {"u1": a1, "u2": a2})
    R.check(A, yh.ok(made_in) and paired,
            f"u2 在 {SECOND} 的 {LINE} 上建管道流入连接符：命令成功；{SECOND} 到修订 4（它的 op 与配对），"
            f"{FIRST} 到修订 3（配对）")
    R.check(A, [k for _o, _r, k in a1] == [["create_pipe_line"], ["create_connector"], ["pair_connector"]]
            and [k for _o, _r, k in a2][:4] == [["create_pipe_line"], ["add_pipe_run"], ["create_connector"],
                                                ["pair_connector"]]
            and a1[2][0].startswith("pair-") and a2[3][0] == a1[2][0],
            f"双方各收到对应的 op_applied：u1 {[(r, k) for _o, r, k in a1]}；u2 {[(r, k) for _o, r, k in a2]}；"
            f"配对是服务器发起的同一个 op（{a1[2][0] if len(a1) > 2 else None}）在两个房间里各一份")
    rows1, rows2 = connector_rows(u1), connector_rows(u2)
    R.dump("y09-connectors.json", {"u1": rows1, "u2": rows2})
    out, inn = (rows1 or [{}])[0], (rows2 or [{}])[0]
    R.check(A, len(rows1) == 1 and out.get("kind") == "pipe_out" and out.get("peer_drawing") == SECOND
            and out.get("downstream") == LINE and out.get("connector_peer") == f"{SECOND}:{LINE}/pipe_in"
            and out.get("type") == "PIPE_LINE" and out.get("topology") == "connector_out",
            f"u1 的流出连接符知道对端：{json.dumps(out, ensure_ascii=False)}")
    R.check(A, len(rows2) == 1 and inn.get("kind") == "pipe_in" and inn.get("peer_drawing") == FIRST
            and inn.get("upstream") == LINE and inn.get("connector_peer") == f"{FIRST}:{LINE}/pipe_out"
            and inn.get("type") == "PIPE_LINE" and inn.get("topology") == "connector_in",
            f"u2 的流入连接符知道对端：{json.dumps(inn, ensure_ascii=False)}")
    st1, c1 = server_connectors(srv, FIRST)
    st2, c2 = server_connectors(srv, SECOND)
    R.dump("y09-server-connectors.json", {FIRST: c1, SECOND: c2})
    R.check(A, st1 == 200 and st2 == 200 and len(c1) == 1 and len(c2) == 1
            and c1[0].get("peer_drawing") == SECOND and c1[0].get("peer_connector") == "pipe_in"
            and c2[0].get("peer_drawing") == FIRST and c2[0].get("peer_connector") == "pipe_out",
            f"服务器两张图的拓扑各有一个连接符且互为对端：{FIRST} → {c1[0].get('peer_drawing') if c1 else None}，"
            f"{SECOND} → {c2[0].get('peer_drawing') if c2 else None}")
    log1, log2 = ops_rows(srv, FIRST), ops_rows(srv, SECOND)
    R.dump("y09-ops.json", {FIRST: log1, SECOND: log2})
    R.check(A, len(log1) == 3 and len(log2) == 4 and log1[-1]["session_id"] == f"link:{SECOND}"
            and log2[-1]["session_id"] == f"link:{SECOND}" and log1[-1]["user_id"] == "u2",
            f"配对进了两张图各自的 ops 表：{FIRST} 第 {log1[-1]['revision'] if log1 else None} 行、"
            f"{SECOND} 第 {log2[-1]['revision'] if log2 else None} 行，会话名 {log1[-1]['session_id'] if log1 else None}"
            f"（服务器自己发起），记在起因那一步的用户 {log1[-1]['user_id'] if log1 else None} 名下")

    # ── ③ u1 改管道等级；u2 的对端对象读回 ───────────────────────────────
    before = by_role(u2, GRADE)
    changed = u1.run(f"PIDSETPROP {LINE} {GRADE} A1A")
    carried = at(u1, 4) and at(u2, 5)
    values = None
    deadline = time.time() + 15
    while time.time() < deadline:
        values = by_role(u2, GRADE)
        if values.get("pipe_line") == "A1A" and values.get("pipe_segment") == "A1A":
            break
        time.sleep(0.3)
    R.dump("y09-u2-grade.json", {"before": before, "after": values})
    R.check(A, yh.ok(changed) and carried and before.get("pipe_line") in (None, "")
            and (values or {}).get("pipe_line") == "A1A",
            f"u1 把 {FIRST} 的 {LINE} 的{GRADE}改成 A1A：u2 那边连接符的对端对象（{SECOND} 的 {LINE}）读回 "
            f"{(values or {}).get('pipe_line')}（之前是 {before.get('pipe_line')!r}）；{SECOND} 到修订 5")
    R.check(A, (values or {}).get("pipe_run") == "A1A" and (values or {}).get("pipe_segment") == "A1A",
            f"并顺着 u2 图上的下传链到了管路与管段：{json.dumps(values, ensure_ascii=False)}")
    a2 = applied(u2)
    last = a2[-1] if a2 else (None, None, None)
    frame = (m7h.frames(u2.err_text(), "<-", "op_applied") or [{}])[-1]
    change = (frame.get("changes") or [{}])[0]
    R.check(A, str(last[0]).startswith(f"carry-{FIRST}-") and last[2] == ["set_property"]
            and change.get("object_ref") == LINE and change.get("field") == GRADE and change.get("value") == "A1A",
            f"u2 收到的是服务器在 {SECOND} 房间里提交的一个 op：{last[0]} 修订 {last[1]}，"
            f"set_property {change.get('object_ref')}.{change.get('field')}={change.get('value')}")
    R.check(A, server_value(srv, FIRST, GRADE) == "A1A" and server_value(srv, SECOND, GRADE) == "A1A",
            f"服务器侧两张图的 {LINE} 都读回 {GRADE}：{server_value(srv, FIRST, GRADE)} / {server_value(srv, SECOND, GRADE)}")
    log2 = ops_rows(srv, SECOND)
    audit = srv.sql("select seq, action, object_ref, user_id, change_text from audit_log where object_ref=? order by seq", (LINE,))
    R.dump("y09-audit.json", audit)
    along = [r for r in audit if "沿连接符" in (r["change_text"] or "") and "A1A" in (r["change_text"] or "")]
    R.check(A, len(log2) == 5 and log2[-1]["session_id"] == f"link:{FIRST}" and log2[-1]["user_id"] == "u1"
            and len(along) == 1 and along[0]["user_id"] == "u1",
            f"{SECOND} 的 ops 表第 5 行是这次传递（会话名 {log2[-1]['session_id'] if log2 else None}，用户 "
            f"{log2[-1]['user_id'] if log2 else None}）；审计一条：{along[0]['change_text'] if along else None}")
    sent2 = m7h.frames(u2.err_text(), "->", "op")
    fine, why = yh.not_diverged(u1, u2)
    R.check(A, fine and len(sent2) == 3,
            f"u2 没有把到达的值再说回去（它一共发过 {len(sent2)} 个 op：管线、管路、连接符），两边都没有被标成不一致：{why}")

    # ── ④ 同一线号的第二个流入 ───────────────────────────────────────────
    before_rows, before_count = connector_rows(u2), len(yh.entities(u2))
    refused = yh.draw(u2, "PIDCONNECTOR pipe_in", [(60, 0)])
    body = yh.body_of(refused)
    R.dump("y09-duplicate-inflow.json", {"http_status": refused[0], "body": body})
    R.check(A, body.get("ok") is False and body.get("code") == "DUPLICATE_INFLOW_CONNECTOR",
            f"u2 在 {SECOND} 上对 {LINE} 再建第二个流入连接符：ok={body.get('ok')}，code={body.get('code')}")
    time.sleep(1.0)
    R.check(A, connector_rows(u2) == before_rows and len(yh.entities(u2)) == before_count
            and len(m7h.frames(u2.err_text(), "->", "op")) == 3 and len(ops_rows(srv, SECOND)) == 5,
            f"被拒之后什么都没变：u2 仍是 {len(before_rows)} 个连接符、{before_count} 个带作者记录的实体，"
            f"没有发出 op，{SECOND} 的 ops 表仍是 5 行")
    time.sleep(0.5)
    for name, g in (("u1", u1), ("u2", u2)):
        # 取景对着管线与连接符（它们画在 X 轴上，缺省视图里被命令行盖住）。
        yh.focus_on(g)
        time.sleep(0.4)
        st, resp, shot = g.capture(m7h.SHOTS / f"cross-connector-{name}.png")
        R.check(A, st == 200 and shot, f"{name} 截图 shots/cross-connector-{name}.png")
except SystemExit:
    raise
except Exception as error:  # noqa: BLE001
    import traceback
    R.check("script", False, f"脚本异常: {error!r}\n{traceback.format_exc()}")
finally:
    m7h.cleanup()
    ok = R.summary()
sys.exit(0 if ok else 1)
