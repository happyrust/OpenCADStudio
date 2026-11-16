# -*- coding: utf-8 -*-
"""VAL-CLI-020 (a killed server is detected on the read-error branch) and
VAL-CLI-014 (the client survives, backs off, and rebuilds its session when the
server is back on the same port and database).

One GUI instance (u1, 3210); the server on 3240 is killed hard — no close
frames go out — left down for about 30 s, then started again. About 70 s.
"""
import json
import re
import sys
import time

import m6h

r = m6h.Results("t24_reconnect")


def stamp(line):
    """The millisecond wall-clock stamp of a `[collab] t=…` line."""
    found = re.search(r"\bt=(\d+)", line)
    return int(found.group(1)) if found else None


try:
    dwg = m6h.fresh_copy("PID-1101-a.dwg", m6h.SEED_DWG)
    srv = m6h.Server(3240, tag="t24-srv").start(fresh=True)
    r.check("setup", srv.wait_health(), f"服务器 /health = {srv.health()}")
    g = m6h.Gui("t24-u1", 3210, dwg=dwg, user="u1").start()
    r.check("setup", g.wait_ready(180) is not None, "u1 GUI 就绪")
    s1 = g.wait_status(lambda s: s.get("connected") and s.get("session_id"), 30)
    sid_old = s1["session_id"]
    before = srv.sql("select session_id,user_id,status from sessions")
    r.dump("t24-sessions-before.json", before)
    r.note(f"重启前 sessions: {json.dumps(before, ensure_ascii=False)}")
    time.sleep(2.0)

    # ── VAL-CLI-020: kill the server, no close frame ──────────────────────
    killed_ms = int(time.time() * 1000)
    srv.kill()
    lost = g.wait_status(lambda s: s.get("connected") is False, 5, step=0.05)
    seen_ms = int(time.time() * 1000)
    drops = m6h.wait_until(lambda: m6h.lines_with(g.err_text(), "断开："), 5) or []
    r.note(f"服务器在 t={killed_ms} 被强杀；/collab/status 在 {seen_ms - killed_ms} 毫秒后报 connected=false")
    r.note(f"u1 客户端日志: {drops[:1]}")
    first = drops[0] if drops else ""
    detected = stamp(first)
    r.check("VAL-CLI-020", bool(first) and detected is not None and 0 <= detected - killed_ms <= 3000,
            f"客户端在 {None if detected is None else detected - killed_ms} 毫秒内检测到断开（上限 3 秒）")
    r.check("VAL-CLI-020", "read error" in first and "ConnectionReset" in first and "close frame" not in first,
            f"走的是读错误分支而非正常关闭分支: {first.split('断开：')[-1] if first else None}")
    r.check("VAL-CLI-020", lost is not None and "ConnectionReset" in (lost.get("last_error") or ""),
            f"/collab/status.last_error = {lost and lost.get('last_error')!r}")

    # ── VAL-CLI-014: 30 s with the server down ────────────────────────────
    time.sleep(30.0)
    status, body = g.ready()
    r.check("VAL-CLI-014", g.running() and status == 200,
            f"服务器停了 30 秒后客户端仍存活：进程在，GET /api/v1/ready -> {status}")
    down = g.status()
    r.note(f"停机期间 /collab/status: connected={down.get('connected')} reconnect_attempts={down.get('reconnect_attempts')} "
           f"last_error={down.get('last_error')!r}")
    attempts = m6h.lines_with(g.err_text(), "连接 ws://")
    after_kill = [line for line in attempts if (stamp(line) or 0) >= killed_ms]
    times = [stamp(line) for line in after_kill]
    gaps = [round((b - a) / 1000.0, 2) for a, b in zip(times, times[1:])]
    waits = [int(found.group(1)) for line in m6h.lines_with(g.err_text(), "断开：")
             if (stamp(line) or 0) >= killed_ms and (found := re.search(r"；(\d+) 毫秒后重连", line))]
    r.note(f"停机期间的连接尝试 {len(after_kill)} 次，相邻两次相隔（秒）: {gaps}")
    r.note(f"每次失败后宣布的等待（毫秒）: {waits}")
    r.dump("t24-014-attempts.txt", "\n".join(m6h.lines_with(g.err_text(), "[collab]")))
    r.check("VAL-CLI-014", len(waits) >= 4 and all(b >= a for a, b in zip(waits, waits[1:])) and waits[-1] > waits[0],
            f"退避单调不减、整体递增：{waits[0] if waits else '?'} 毫秒起，逐次翻倍，到 5000 毫秒封顶")
    r.check("VAL-CLI-014", len(gaps) >= 4 and all(b >= a - 0.3 for a, b in zip(gaps, gaps[1:])) and gaps[-1] > gaps[0],
            f"连接尝试的实际间隔随之拉长（{gaps[0] if gaps else '?'} 秒 → {gaps[-1] if gaps else '?'} 秒；每次被拒本身约 2 秒）")

    # ── back on the same port and database ────────────────────────────────
    restart_at = time.time()
    srv.start(fresh=False)
    r.check("setup", srv.wait_health(), f"服务器以同一 --port / --db 重启: {srv.health()}")
    back = g.wait_status(lambda s: s.get("connected") and s.get("session_id") and s.get("session_id") != sid_old, 60)
    took = time.time() - restart_at
    after = srv.sql("select session_id,user_id,status from sessions")
    r.dump("t24-sessions-after.json", after)
    r.note(f"重启后 sessions: {json.dumps(after, ensure_ascii=False)}")
    sid_new = back and back.get("session_id")
    row = next((x for x in after if x["session_id"] == sid_new), None)
    r.check("VAL-CLI-014", back is not None and row is not None and row["status"] == "活跃" and row["user_id"] == "u1"
            and sid_new != sid_old,
            f"重启后 {took:.1f} 秒重建会话（上限 60 秒）：新 session_id 与原来不同、status=活跃、user_id=u1")
    again = [line for line in m6h.lines_with(g.err_text(), "收到 welcome") if (stamp(line) or 0) >= killed_ms]
    r.note(f"u1 客户端日志（重连成功行）: {again}")
    r.check("VAL-CLI-020", bool(again) and f"session={sid_new}" in again[-1],
            "重启后 60 秒内重建会话成功，客户端日志有重连成功行")
    r.dump("t24-u1-collab-log.txt", "\n".join(m6h.lines_with(g.err_text(), "[collab]")))
    r.dump("t24-server-log.txt", "\n".join(line for line in srv.log_text().splitlines() if "heartbeat" not in line))
finally:
    m6h.cleanup()
    ok = r.summary()
sys.exit(0 if ok else 1)
