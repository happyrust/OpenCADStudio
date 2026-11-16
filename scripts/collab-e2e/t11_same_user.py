# -*- coding: utf-8 -*-
"""VAL-CLI-018: the same user from two instances gets two independent sessions."""
import json
import sys
import time

import m6h

r = m6h.Results("t11_same_user")
try:
    dwg_a = m6h.fresh_copy("PID-1101-a.dwg", m6h.SEED_DWG)
    dwg_b = m6h.fresh_copy("PID-1101-b.dwg", m6h.SEED_DWG)
    srv = m6h.Server(3240, tag="t11-srv").start(fresh=True)
    r.check("setup", srv.wait_health(), f"服务器 /health = {srv.health()}")

    a = m6h.Gui("t11-a", 3210, dwg=dwg_a, user="u1").start()
    r.check("setup", a.wait_ready(180) is not None, "实例 A（3210，u1）就绪")
    sa = a.wait_status(lambda s: s.get("connected") and s.get("session_id"), 30)
    b = m6h.Gui("t11-b", 3211, dwg=dwg_b, user="u1").start()
    r.check("setup", b.wait_ready(180) is not None, "实例 B（3211，u1）就绪")
    sb = b.wait_status(lambda s: s.get("connected") and s.get("session_id"), 30)
    sid_a, sid_b = sa["session_id"], sb["session_id"]

    rows = srv.sql("select session_id,user_id,drawing_id,status from sessions where user_id='u1' order by created_at")
    r.dump("t11-sessions-two.json", rows)
    r.note(f"sessions: {json.dumps(rows, ensure_ascii=False)}")
    r.check("VAL-CLI-018", len(rows) == 2 and all(x["status"] == "活跃" and x["drawing_id"] == "PID-1101" for x in rows)
            and rows[0]["session_id"] != rows[1]["session_id"] and {rows[0]["session_id"], rows[1]["session_id"]} == {sid_a, sid_b},
            f"sessions 表 2 行 user_id=u1、drawing_id=PID-1101、status=活跃，session_id 不同: "
            f"{[(x['session_id'][:10], x['status']) for x in rows]}")

    sa2 = a.wait_status(lambda s: any(p.get("session_id") == sid_b for p in s.get("peers") or []), 15) or a.status()
    joined_a = m6h.lines_with(a.err_text(), "收到 peer_joined")
    joined_b = m6h.lines_with(b.err_text(), "收到 peer_joined")
    welcome_b = m6h.lines_with(b.err_text(), "收到 welcome")
    r.note(f"A 客户端日志 peer_joined: {joined_a}")
    r.note(f"B 客户端日志 peer_joined: {joined_b}")
    r.note(f"B 客户端日志 welcome: {welcome_b}")
    r.check("VAL-CLI-018", bool(joined_a) and f"session={sid_b}" in joined_a[-1] and "user=u1" in joined_a[-1],
            f"先连入的 A 收到后连入的 B 的 peer_joined: {joined_a[-1] if joined_a else None}")
    knows = any(p.get("session_id") == sid_a for p in sb.get("peers") or [])
    r.check("VAL-CLI-018", knows, f"后连入的 B 知道 A 在线（B 的 peers 含 A 的会话: {knows}）")
    r.note(f"【契约字面】B 也收到一条关于 A 的 peer_joined: {bool(joined_b)}"
           f"（A 先于 B 在线，协议里 B 是从 welcome.peers 得知 A 的，不会再有 peer_joined）")
    r.dump("t11-a-status.json", sa2)
    r.dump("t11-b-status.json", b.status())

    # The first instance leaves (a clean quit); the second's session stays live.
    try:
        a.run("QUIT")
    except Exception as e:  # noqa: BLE001 - the process may go before it answers
        r.note(f"A QUIT 的响应没有读到（进程已退出）: {type(e).__name__}")
    gone = a.wait_exit(15)
    r.note(f"A 进程 15 秒内退出: {gone}")
    time.sleep(1.5)
    rows2 = srv.sql("select session_id,user_id,status from sessions where user_id='u1' order by created_at")
    r.dump("t11-sessions-after.json", rows2)
    row_a = next((x for x in rows2 if x["session_id"] == sid_a), None)
    row_b = next((x for x in rows2 if x["session_id"] == sid_b), None)
    r.check("VAL-CLI-018", row_b is not None and row_b["status"] == "活跃",
            f"A 断开后 B 的 sessions 行仍为活跃: A={row_a and row_a['status']} B={row_b and row_b['status']}")
    sb3 = b.status()
    r.note(f"B /collab/status: connected={sb3.get('connected')} peers={[(p.get('user_id'), p.get('session_id')[:10]) for p in sb3.get('peers') or []]}")
    r.note(f"B 客户端日志 peer_left: {m6h.lines_with(b.err_text(), '收到 peer_left')}")
    r.dump("t11-server-log.txt", m6h.this_run(srv.log_text()))
    r.dump("t11-a-collab-log.txt", "\n".join(m6h.lines_with(a.err_text(), "[collab]")))
    r.dump("t11-b-collab-log.txt", "\n".join(m6h.lines_with(b.err_text(), "[collab]")))
finally:
    m6h.cleanup()
    ok = r.summary()
sys.exit(0 if ok else 1)
