# -*- coding: utf-8 -*-
"""VAL-CLI-003: 90 s without a heartbeat makes the session stale and the room
is told `peer_left`.

u1 is a GUI instance (3210). The second observer is a plain WebSocket client
as u2, as the 2026-11-09 ruling on this assertion allows (two GUI instances
are not needed to watch a roster). u1's process is then suspended — every
thread frozen, the socket left open — and nothing more is sent for it.
Default heartbeat timeout (90 s); takes about 100 s.
"""
import json
import sys
import time

import m6h

r = m6h.Results("t23_stale")
try:
    dwg = m6h.fresh_copy("PID-1101-a.dwg", m6h.SEED_DWG)
    srv = m6h.Server(3240, tag="t23-srv").start(fresh=True)
    r.check("setup", srv.wait_health(), f"服务器 /health = {srv.health()}（默认 heartbeat 超时 90 秒）")
    g = m6h.Gui("t23-u1", 3210, dwg=dwg, user="u1").start()
    r.check("setup", g.wait_ready(180) is not None, "u1 GUI 就绪")
    s1 = g.wait_status(lambda s: s.get("connected") and s.get("session_id"), 30)
    sid1 = s1["session_id"]

    # The second observer joins the same drawing.
    obs = m6h.Ws(f"ws://127.0.0.1:{srv.port}/ws", "t23-u2")
    welcome = obs.hello("u2")
    obs.keep_alive(10.0)
    r.dump("t23-u2-welcome.json", welcome)
    peers = (welcome or {}).get("peers") or []
    r.note(f"u2 收到的 welcome: {json.dumps(welcome, ensure_ascii=False)}")
    # 2026-11-11：welcome.peers 不含连入者自己（a0f33b354）。裁决条目 1 原文是
    # 「peers 长度为 2」，按用户的决定改成 1；这一句检查跟着改。
    r.check("VAL-CLI-003", (welcome or {}).get("type") == "welcome" and len(peers) == 1
            and any(p.get("user_id") == "u1" and p.get("session_id") == sid1 for p in peers),
            f"裁决条目 1（按 2026-11-11 的决定）：u2 侧收到 welcome 且 peers 长度为 1、包含 u1（实得 {len(peers)}）")
    # u2 自己维护的在线列表：自己加上 welcome 里的别人。
    roster = {p["session_id"]: p["user_id"] for p in peers}
    roster[(welcome or {}).get("session_id")] = "u2"
    peers_before = len(roster)
    joined_at_u1 = m6h.wait_until(lambda: m6h.lines_with(g.err_text(), "收到 peer_joined", "user=u2"), 10) or []
    r.note(f"u1 客户端日志: {joined_at_u1}")
    r.check("VAL-CLI-003", bool(joined_at_u1), "第二个观测端连入后，先在线的 u1 收到 peer_joined（user=u2）")
    literal = obs.incoming("peer_joined")
    r.note(f"【裁决条目 2 的前半句按字面】u2 侧收到 peer_joined(u1): {bool(literal)}"
           f"（u1 先于 u2 在线，u2 是从 welcome.peers 得知 u1 的）")

    before = srv.sql("select session_id,user_id,status,last_heartbeat from sessions order by created_at")
    r.dump("t23-sessions-before.json", before)
    r.note(f"挂起前 sessions: {json.dumps(before, ensure_ascii=False)}")

    # Freeze u1: no more heartbeats, no close frame.
    frozen = g.suspend()
    frozen_at = time.time()
    r.check("setup", frozen, f"u1 进程 {g.pid} 已挂起（全部线程冻结，socket 不关），之后静置 95 秒")
    left = obs.wait_for(lambda m: m.get("type") == "peer_left" and m.get("session_id") == sid1, 110)
    waited = time.time() - frozen_at
    if left:
        roster.pop(left.get("session_id"), None)
    time.sleep(max(0.0, 95 - waited))
    after = srv.sql("select session_id,user_id,status,last_heartbeat from sessions order by created_at")
    r.dump("t23-sessions-after.json", after)
    r.note(f"静置后 sessions: {json.dumps(after, ensure_ascii=False)}")
    row = next((x for x in after if x["session_id"] == sid1), None)
    r.check("VAL-CLI-003", row is not None and row["status"] == "失效",
            f"sessions 表中 u1 的会话 status = {row and row['status']}（挂起后 {time.time() - frozen_at:.0f} 秒）")
    r.note(f"u2 收到的 peer_left 帧原文: {json.dumps(left, ensure_ascii=False)}（挂起后 {waited:.0f} 秒到达）")
    r.check("VAL-CLI-003", bool(left) and left.get("user_id") == "u1" and left.get("session_id") == sid1,
            "裁决条目 2 的后半句：u2 侧收到 peer_left，session_id 与 u1 匹配、user_id=u1")
    r.check("VAL-CLI-003", peers_before == 2 and len(roster) == 1,
            f"裁决条目 3：u2 自己维护的在线列表 peersBefore={peers_before} peersAfter={len(roster)}")
    srv_left = m6h.lines_with(srv.log_text(), "广播 peer_left", sid1)
    r.note(f"服务器日志: {srv_left}")
    r.check("VAL-CLI-003", bool(srv_left) and "reason=heartbeat-timeout" in srv_left[-1],
            "服务器日志：因心跳超时广播 peer_left")
    beats = [int(line.split("at=")[1]) for line in m6h.lines_with(srv.log_text(), "入站 heartbeat", sid1)]
    last = max(beats) if beats else before[0]["last_heartbeat"]
    stale_at = int(time.time() - (time.time() - frozen_at - waited)) if left else None
    r.note(f"u1 最后一次心跳（或建会话）在 {last}；peer_left 约在 {stale_at} 到达，相隔 {None if stale_at is None else stale_at - last} 秒（超时 90 秒）")
    obs.close()
    r.dump("t23-u2-frames.txt", (m6h.LOGS / "ws-t23-u2.jsonl").read_text(encoding="utf-8"))
    r.dump("t23-server-log.txt", "\n".join(line for line in m6h.this_run(srv.log_text()).splitlines() if "heartbeat" not in line or sid1 in line))
finally:
    m6h.cleanup()
    ok = r.summary()
sys.exit(0 if ok else 1)
