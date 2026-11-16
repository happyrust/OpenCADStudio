# -*- coding: utf-8 -*-
"""VAL-SRV-027：连接被直接销毁时，会话和占用不立即清掉（ws-disconnect）。

只起协同服务器和几个 WS 会话，SQLite 只读查询。不起 GUI，不涉及指针与焦点。

  A 已握手并占着 V-1001。不发 close 帧，直接销毁 A 的 socket（m7h.Ws.abort：连接被重置）。
  服务器察觉了断线（旁观会话收到 A 的 peer_left），但没有把它当成退出：过了几轮每秒一次的
  清扫之后，同一用户在同一张图上重新握手（30 秒以内），welcome.claims 仍含 V-1001，持有者是
  A 原来的会话；库里 A 的会话仍是「活跃」；新会话去占 V-1001 得到 claim_denied，指明持有者。
  对照：另一个会话发 close 帧退出，它占的位号当即释放 —— 可见上面留住占用靠的是「断线不等于
  退出」，而不是占用本来就不会被清。
"""
import sys
import time

import m7h

R = m7h.Results("s03_disconnect")
DRAWING = "PID-1101"
SWEEPS = 3.0     # 服务器每秒清扫一次；销毁连接后至少等这么久再重连


def connect(srv, name):
    return m7h.Ws(f"ws://127.0.0.1:{srv.port}/ws", name)


def claim(ws, tag, seconds=10):
    ws.send({"type": "claim", "session_id": ws.session_id, "tag": tag})
    return ws.wait_for(lambda m: m.get("type") in ("claim_granted", "claim_denied") and m.get("tag") == tag, seconds)


def held(welcome, tag):
    return [c for c in (welcome or {}).get("claims") or [] if c.get("tag") == tag]


try:
    srv = m7h.Server(port=3240, tag="s03-srv")
    srv.start(fresh=True)
    R.check("setup", srv.wait_health(), f"服务器已启动，/health = {srv.health()}")
    watcher, a, c = connect(srv, "s03-watcher"), connect(srv, "s03-a"), connect(srv, "s03-c")
    watcher.hello("u4", DRAWING)
    a.hello("u1", DRAWING)
    c.hello("u2", DRAWING)
    got_a, got_c = claim(a, "V-1001"), claim(c, "V-1002")
    old = a.session_id
    R.check("setup", bool(watcher.session_id) and bool(old) and bool(c.session_id)
            and (got_a or {}).get("type") == "claim_granted" and (got_c or {}).get("type") == "claim_granted",
            f"旁观会话（u4）、A（u1，占 V-1001）、C（u2，占 V-1002）都已握手：{got_a}；{got_c}")

    # ── 直接销毁 A 的 socket ─────────────────────────────────────────────
    destroyed = time.time()
    a.abort()
    left = watcher.wait_for(lambda m: m.get("type") == "peer_left" and m.get("session_id") == old, 10)
    noticed = time.time() - destroyed
    R.check("VAL-SRV-027", bool(left) and not m7h.lines_with(srv.log_text(), "入站 close 帧", f"session={old}"),
            f"A 的 socket 被直接销毁（abort：不发 close 帧）；服务器察觉了断线 —— 旁观会话在 {noticed:.2f} 秒后"
            f"收到 {left}，服务器日志里没有这个会话的「入站 close 帧」")
    time.sleep(max(0.0, SWEEPS - (time.time() - destroyed)))

    again = connect(srv, "s03-a-again")
    welcome = again.hello("u1", DRAWING)
    elapsed = time.time() - destroyed
    R.dump("s03-welcome-after-reconnect.json", {"destroyed_to_welcome_seconds": round(elapsed, 3), "welcome": welcome})
    R.check("VAL-SRV-027", (welcome or {}).get("type") == "welcome" and SWEEPS <= elapsed < 30,
            f"同一用户在同一张图上重新握手，距销毁 {elapsed:.2f} 秒（过了至少 {SWEEPS:.0f} 轮清扫，在 30 秒以内），"
            f"新会话 {again.session_id}")
    kept = held(welcome, "V-1001")
    R.check("VAL-SRV-027", len(kept) == 1 and kept[0].get("session_id") == old and again.session_id != old,
            f"重连的 welcome.claims 仍含 V-1001，持有者是 A 原来的会话 {old}：{kept}")
    released = [m for m in watcher.incoming("claim_released") if m.get("tag") == "V-1001"]
    R.check("VAL-SRV-027", not released, f"旁观会话没有收到 V-1001 的 claim_released：{released}")
    session = srv.sql("select session_id, user_id, status from sessions where session_id = ?", (old,))
    claims = srv.sql("select tag, session_id, user_id from claims order by tag")
    R.check("VAL-SRV-027", [s["status"] for s in session] == ["活跃"]
            and [(k["tag"], k["session_id"]) for k in claims] == [("V-1001", old), ("V-1002", c.session_id)],
            f"库里 A 的会话仍是「活跃」，占用表没有变：sessions = {session}，claims = {claims}")
    denied = claim(again, "V-1001")
    R.check("VAL-SRV-027", (denied or {}).get("type") == "claim_denied" and denied.get("holder") == old,
            f"新会话去占 V-1001 → {denied}（持有者是 A 原来的会话）")

    # ── 对照：发 close 帧退出的会话 ────────────────────────────────────────
    c.close()
    gone = watcher.wait_for(lambda m: m.get("type") == "claim_released" and m.get("tag") == "V-1002", 10)
    c_left = watcher.wait_for(lambda m: m.get("type") == "peer_left" and m.get("session_id") == c.session_id, 10)
    logged = m7h.wait_until(lambda: m7h.lines_with(srv.log_text(), "入站 close 帧", f"session={c.session_id}"), 5)
    late = connect(srv, "s03-late")
    late_welcome = late.hello("u2", DRAWING)
    R.check("VAL-SRV-027", bool(gone) and bool(c_left) and bool(logged)
            and not held(late_welcome, "V-1002") and len(held(late_welcome, "V-1001")) == 1,
            f"对照：C 发 close 帧退出 → 旁观会话收到 {gone} 和 C 的 peer_left，服务器日志 {logged}；"
            f"此后握手的 welcome.claims 只剩 {(late_welcome or {}).get('claims')}")
    late.close()
    again.close()
    watcher.close()
    R.dump("s03-server-log.txt", m7h.this_run(srv.log_text()))
finally:
    m7h.cleanup()
    ok = R.summary()
sys.exit(0 if ok else 1)
