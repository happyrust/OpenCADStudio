# -*- coding: utf-8 -*-
"""VAL-SRV-024、025：服务器进程重启前后（server-restart）。

只起协同服务器和几个 WS 会话（m7h.Ws，帧原文逐条落在 logs/ws-*.jsonl），SQLite
只读查询，外加服务器自己的读回接口。不起 GUI，不涉及指针与焦点。

  024  两个会话各提交一次属性修改（留下审计行，把图面修订推到 2），强行结束服务器
       进程，用同一条命令行（同一个 --db）再起。/health 200；GET /audit 的响应、
       图纸登记（drawings 行，含修订）、服务器从操作日志重建出的图面
       （GET /drawing/PID-1101/objects）都与重启前逐项相同。
  025  重启前的会话号不能再用：新开连接、不握手，拿旧会话号发 heartbeat，必须回带
       code 的 error（连接上还没有会话，服务器回的是 HELLO_REQUIRED）；握手之后再拿
       旧会话号发，回 SESSION_UNKNOWN。对照：拿自己的会话号发，服务器记下这次心跳、
       不回 error。库里重启前的会话行和占用行都不在了。
"""
import sys

import m7h

R = m7h.Results("s01_restart")
DRAWING = "PID-1101"


def connect(srv, name):
    return m7h.Ws(f"ws://127.0.0.1:{srv.port}/ws", name)


def claim(ws, tag, seconds=10):
    ws.send({"type": "claim", "session_id": ws.session_id, "tag": tag})
    return ws.wait_for(lambda m: m.get("type") in ("claim_granted", "claim_denied") and m.get("tag") == tag, seconds)


def write(ws, base, op_id, obj, field, value, seconds=10):
    ws.send({"type": "op", "session_id": ws.session_id, "base_revision": base, "op_id": op_id,
             "payload": {"kind": "set_property", "object_ref": obj, "field": field, "value": value}})
    return ws.wait_for(lambda m: m.get("type") in ("op_applied", "error") and m.get("op_id") == op_id, seconds)


def kept(srv):
    """What a restart has to keep, as the server gives it out and as the database holds it."""
    return {
        "audit_of_v1001": srv.http("GET", "/audit?object_ref=V-1001"),
        "audit": srv.http("GET", "/audit"),
        "objects": srv.http("GET", f"/drawing/{DRAWING}/objects"),
        "drawings": srv.sql("select drawing_id, file_name, pbs_node_id, state, lifecycle, revision "
                            "from drawings order by drawing_id"),
        "audit_rows": srv.sql("select count(*) as n, max(seq) as last_seq from audit_log")[0],
        "ops": srv.sql("select revision, op_id, user_id from ops where drawing_id = ? order by revision", (DRAWING,)),
    }


try:
    srv = m7h.Server(port=3240, tag="s01-srv")
    logged = len(srv.log_text())
    srv.start(fresh=True)
    R.check("setup", srv.wait_health(), f"服务器第一次启动，/health = {srv.health()}")
    a, b = connect(srv, "s01-a"), connect(srv, "s01-b")
    wa, wb = a.hello("u1", DRAWING), b.hello("u2", DRAWING)
    old = [a.session_id, b.session_id]
    R.check("setup", all(old) and (wa or {}).get("base_revision") == 0 and (wb or {}).get("base_revision") == 0,
            f"会话 A（u1）、B（u2）握手同一张图，图面修订 {wa and wa.get('base_revision')}")
    granted = claim(a, "V-1001")
    R.check("setup", (granted or {}).get("type") == "claim_granted", f"A 占用 V-1001：{granted}")
    first = write(a, 0, "s01-op-a", "V-1001", "设计压力", "2.5MPa")
    second = write(b, 1, "s01-op-b", "P-1001", "流量", "30m3/h")
    R.check("setup", (first or {}).get("revision") == 1 and (second or {}).get("revision") == 2,
            f"A 改 V-1001.设计压力、B 改 P-1001.流量，各得 op_applied：修订 "
            f"{first and first.get('revision')}、{second and second.get('revision')}")
    before = kept(srv)
    R.dump("s01-before.json", before)
    status, page = before["audit_of_v1001"]
    rows = (page or {}).get("items") or []
    R.check("setup", status == 200 and len(rows) == 1 and "2.5MPa" in (rows[0].get("change_text") or ""),
            f"重启前 GET /audit?object_ref=V-1001 → HTTP {status}，{len(rows)} 行：{rows}")

    # ── 结束进程，同一条命令行再起 ──────────────────────────────────────
    srv.kill()
    R.check("setup", bool(m7h.wait_until(lambda: not m7h.port_listening(srv.port), 10)),
            f"服务器进程已强行结束，端口 {srv.port} 不再监听")
    m7h.wait_until(lambda: a.closed and b.closed, 5)
    R.note(f"重启前的两条连接随进程结束而断开：A {a.close_reason}；B {b.close_reason}")
    srv.start(fresh=False)
    up = srv.wait_health()
    R.check("VAL-SRV-024", up and srv.health()[0] == 200,
            f"用同一条命令行（同一个 --db）再起，/health = {srv.health()}")
    after = kept(srv)
    R.dump("s01-after.json", after)
    R.check("VAL-SRV-024", after["audit_of_v1001"] == before["audit_of_v1001"],
            f"GET /audit?object_ref=V-1001 的完整响应与重启前相同（seq、change_text 等逐项）："
            f"重启后 {after['audit_of_v1001']}")
    R.check("VAL-SRV-024", after["audit"] == before["audit"] and after["audit_rows"] == before["audit_rows"],
            f"全部审计与重启前相同：重启前 {before['audit_rows']}，重启后 {after['audit_rows']}")
    R.check("VAL-SRV-024", after["drawings"] == before["drawings"] and after["ops"] == before["ops"]
            and [d["revision"] for d in after["drawings"] if d["drawing_id"] == DRAWING] == [2],
            f"图纸登记（含修订）和操作日志与重启前相同：{after['drawings']}")
    R.check("VAL-SRV-024", after["objects"] == before["objects"],
            f"服务器从操作日志重建出的图面与重启前相同：{after['objects']}")
    mine = srv.log_text()[logged:]
    starts = [line.split(": ", 1)[-1] for line in mine.splitlines() if line.startswith("### ")]
    listening = [line for line in mine.splitlines() if "监听 http://" in line]
    R.check("VAL-SRV-024", len(starts) == 2 and starts[0] == starts[1] and len(listening) == 2,
            f"服务器日志里有两次启动、命令行相同，各有一行监听：{listening}")

    # ── 重启前的会话号 ─────────────────────────────────────────────────
    stranger = connect(srv, "s01-old-id")
    stranger.send({"type": "heartbeat", "session_id": old[0]})
    refused = stranger.wait_for(lambda m: m.get("type") == "error", 10)
    R.check("VAL-SRV-025", bool(refused) and bool(refused.get("code")),
            f"重启后新开连接、不握手，拿重启前 A 的会话号发 heartbeat → {refused}")
    stranger.close()

    c = connect(srv, "s01-c")
    wc = c.hello("u1", DRAWING)
    R.check("VAL-SRV-024", (wc or {}).get("base_revision") == 2,
            f"重启后握手的新会话：welcome.base_revision = {wc and wc.get('base_revision')}（重启前图面修订是 2）")
    R.check("VAL-SRV-025", bool(c.session_id) and c.session_id not in old
            and not (wc or {}).get("peers") and not (wc or {}).get("claims"),
            f"新会话的会话号不是旧的，welcome 里没有重启前的同伴和占用：{wc}")
    c.send({"type": "heartbeat", "session_id": old[0]})
    unknown = c.wait_for(lambda m: m.get("type") == "error", 10)
    R.check("VAL-SRV-025", (unknown or {}).get("code") == "SESSION_UNKNOWN",
            f"握手之后在这条连接上拿旧会话号发 heartbeat → {unknown}")
    errors = len(c.incoming("error"))
    c.heartbeat()
    beat = m7h.wait_until(lambda: m7h.lines_with(srv.log_text(), "入站 heartbeat", f"session={c.session_id}"), 10)
    R.check("VAL-SRV-025", bool(beat) and len(c.incoming("error")) == errors,
            f"对照：拿自己的会话号发 heartbeat，服务器记下了这次心跳、没有回 error：{beat}")
    sessions = srv.sql("select session_id, user_id, status from sessions order by created_at")
    claims = srv.sql("select tag, session_id from claims order by tag")
    R.check("VAL-SRV-025", not [s for s in sessions if s["session_id"] in old] and not claims,
            f"库里重启前的会话行和占用行都不在了：sessions = {sessions}，claims = {claims}")
    again = claim(c, "V-1001")
    R.check("VAL-SRV-025", (again or {}).get("type") == "claim_granted",
            f"重启前 A 占着的 V-1001，新会话可以占用：{again}")
    c.close()
    R.dump("s01-server-log.txt", srv.log_text()[logged:])
finally:
    m7h.cleanup()
    ok = R.summary()
sys.exit(0 if ok else 1)
