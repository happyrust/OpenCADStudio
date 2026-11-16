# -*- coding: utf-8 -*-
"""VAL-SRV-026、028：端口已被占用时再起一个服务器（port-in-use）。

只起协同服务器和几个 WS 会话，SQLite 只读查询。不起 GUI，不涉及指针与焦点。

  026  实例甲按 --port 启动：该端口可连接，/health 200，日志里监听的就是这个端口。
       实例乙用同一端口再起：进程以非零码退出，stderr 报「无法监听 <地址>」，没有改到
       别的端口上监听。
  028  甲的库里已经有会话（A 已握手）和占用（A 占着 V-1001）。乙用同一端口、同一个
       --db 启动并失败之后，sessions / claims 逐行与乙启动前相同，A 的会话仍是「活跃」；
       乙没有打开过数据库（先绑端口、后碰库）；甲照常服务，A 的连接不受影响；
       B 随即 POST /session（同图、协同编辑）后，A 的会话与占用还在。
"""
import subprocess
import sys

import m7h

R = m7h.Results("s02_port_in_use")
DRAWING = "PID-1101"


def connect(srv, name):
    return m7h.Ws(f"ws://127.0.0.1:{srv.port}/ws", name)


def claim(ws, tag, seconds=10):
    ws.send({"type": "claim", "session_id": ws.session_id, "tag": tag})
    return ws.wait_for(lambda m: m.get("type") in ("claim_granted", "claim_denied") and m.get("tag") == tag, seconds)


def rows(srv):
    """The two tables a failed second start must leave alone, row for row."""
    return {
        "sessions": srv.sql("select session_id, drawing_id, user_id, mode, status from sessions order by session_id"),
        "claims": srv.sql("select tag, drawing_id, session_id, user_id from claims order by tag"),
    }


def this_start(path):
    """What one server wrote to a log file since its last start, without the marker line."""
    text = path.read_text(encoding="utf-8", errors="replace") if path.exists() else ""
    return [line for line in m7h.this_run(text).splitlines() if line.strip() and not line.startswith("### ")]


try:
    first = m7h.Server(port=3240, tag="s02-first")
    first.start(fresh=True)
    up = first.wait_health()
    R.check("VAL-SRV-026", up and m7h.port_listening(first.port) and first.health()[0] == 200,
            f"实例甲以 --port {first.port} 启动：127.0.0.1:{first.port} 可连接，/health = {first.health()}")
    listening = [line for line in this_start(first.out) if "监听 http://" in line]
    R.check("VAL-SRV-026", len(listening) == 1 and f"监听 http://127.0.0.1:{first.port}（" in listening[0],
            f"甲的日志里只有一行监听，地址就是参数给的端口：{listening}")

    a = connect(first, "s02-a")
    a.hello("u1", DRAWING)
    granted = claim(a, "V-1001")
    before = rows(first)
    R.dump("s02-before.json", before)
    R.check("setup", (granted or {}).get("type") == "claim_granted"
            and [(s["session_id"], s["status"]) for s in before["sessions"]] == [(a.session_id, "活跃")]
            and [(c["tag"], c["session_id"]) for c in before["claims"]] == [("V-1001", a.session_id)],
            f"A（u1）已握手并占用 V-1001；乙启动前库里 sessions {len(before['sessions'])} 行、"
            f"claims {len(before['claims'])} 行：{before}")

    # ── 实例乙：同一端口、同一个库 ─────────────────────────────────────────
    second = m7h.Server(port=3240, db=first.db, tag="s02-second")
    second.start(fresh=False)
    try:
        code = second.proc.wait(20)
    except subprocess.TimeoutExpired:
        code = None
    stderr, stdout = this_start(second.err), this_start(second.out)
    R.dump("s02-second-output.json", {"exit_code": code, "stderr": stderr, "stdout": stdout})
    R.check("VAL-SRV-026", code is not None and code != 0,
            f"实例乙用同一端口 {second.port} 启动：进程退出码 {code}（None 表示 20 秒内没有退出）")
    refusal = [line for line in stderr if "无法监听" in line and f"127.0.0.1:{second.port}" in line]
    R.check("VAL-SRV-026", len(refusal) == 1, f"乙的 stderr 报告了这个地址无法监听：{stderr}")
    R.check("VAL-SRV-026", not [line for line in stdout if "监听 http://" in line],
            f"乙没有改到别的端口上监听（stdout 里没有监听行）：stdout = {stdout}")

    after = rows(first)
    R.dump("s02-after.json", after)
    R.check("VAL-SRV-028", code is not None and code != 0 and after == before,
            f"乙以退出码 {code} 结束后，库里 sessions {len(after['sessions'])} 行、claims {len(after['claims'])} 行，"
            f"逐行与乙启动前相同：{after}")
    R.check("VAL-SRV-028", [s["status"] for s in after["sessions"] if s["session_id"] == a.session_id] == ["活跃"],
            f"A 的会话行还在，状态仍是「活跃」：{after['sessions']}")
    touched = [line for line in stdout if "journal_mode" in line or "数据库" in line or "播种" in line]
    R.check("VAL-SRV-028", not touched, f"乙没有打开过数据库（stdout 里没有开库、播种的行）：{touched}")
    R.check("VAL-SRV-028", first.running() and first.health()[0] == 200 and not a.closed and not a.incoming("error"),
            f"甲照常服务（/health = {first.health()}），A 的连接还开着、没有收到 error")

    # ── B 随即开会话：A 不能被当成已失效的会话 ────────────────────────────
    status, opened = first.http("POST", "/session", {"user_id": "u2", "drawing_id": DRAWING, "mode": "协同编辑",
                                                     "workspace": "ws-b"})
    later = rows(first)
    R.dump("s02-after-post-session.json", {"status": status, "body": opened, "rows": later})
    R.check("VAL-SRV-028", status == 201
            and [s["status"] for s in later["sessions"] if s["session_id"] == a.session_id] == ["活跃"]
            and later["claims"] == before["claims"],
            f"B（u2）POST /session（同图、协同编辑）→ HTTP {status} {opened}；此后 A 的会话仍「活跃」、"
            f"V-1001 仍归 A：{later}")
    watcher = connect(first, "s02-watcher")
    welcome = watcher.hello("u4", DRAWING)
    held = [c for c in (welcome or {}).get("claims") or [] if c.get("tag") == "V-1001"]
    R.check("VAL-SRV-028", len(held) == 1 and held[0].get("session_id") == a.session_id,
            f"新握手的会话在 welcome.claims 里看到 V-1001 仍由 A 的会话持有：{held}")
    watcher.close()
    a.close()
finally:
    m7h.cleanup()
    ok = R.summary()
sys.exit(0 if ok else 1)
