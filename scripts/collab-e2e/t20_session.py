# -*- coding: utf-8 -*-
"""VAL-CLI-001 (welcome and session), VAL-CLI-002 (15 s heartbeat cadence),
VAL-CLI-016 (a clean quit retires the session at once), and on the way
VAL-INFRA-012 (the GUI and the server run side by side, each on its own port).

One GUI instance (u1, 3210) against a fresh server on 3240 with the default
90 s heartbeat timeout. Takes a little over a minute: 002 watches 60 s.
"""
import json
import subprocess
import sys
import time

import m6h


def listeners(*ports):
    """Which process listens on each of these ports, as Get-NetTCPConnection tells it: {port: {pid}} and its lines."""
    command = (f"Get-NetTCPConnection -State Listen -LocalPort {','.join(str(p) for p in ports)} -ErrorAction SilentlyContinue | "
               "ForEach-Object { '{0} {1} {2}' -f $_.LocalAddress, $_.LocalPort, $_.OwningProcess }")
    out = subprocess.run(["powershell", "-NoProfile", "-Command", command], capture_output=True, text=True, errors="replace")
    lines = [line.strip() for line in out.stdout.splitlines() if line.strip()]
    owners = {}
    for line in lines:
        parts = line.split()
        if len(parts) == 3 and parts[1].isdigit() and parts[2].isdigit():
            owners.setdefault(int(parts[1]), set()).add(int(parts[2]))
    return owners, lines


r = m6h.Results("t20_session")
try:
    dwg = m6h.fresh_copy("PID-1101-a.dwg", m6h.SEED_DWG)
    srv = m6h.Server(3240, tag="t20-srv").start(fresh=True)
    r.check("setup", srv.wait_health(), f"服务器 /health = {srv.health()}")
    r.note(f"连接前 sessions 行数: {srv.sql('select count(*) n from sessions')[0]['n']}")

    # ── VAL-CLI-001 ───────────────────────────────────────────────────────
    g = m6h.Gui("t20-u1", 3210, dwg=dwg, user="u1").start()
    r.check("setup", g.wait_ready(180) is not None, "u1 GUI 就绪")
    # VAL-INFRA-012: both programs up at the same moment, each answering on its own port.
    gui_status, gui_body = g.ready()
    srv_status, srv_body = srv.health()
    owners, listening = listeners(g.port, srv.port)
    r.check("VAL-INFRA-012", gui_status == 200 and (gui_body or {}).get("ready") is True and srv_status == 200
            and g.running() and srv.running() and g.port != srv.port
            and owners.get(g.port) == {g.pid} and owners.get(srv.port) == {srv.proc.pid},
            f"GUI（--http {g.port}）和协同服务器（--port {srv.port}）同时在跑、各用各的端口：GET /api/v1/ready → {gui_status}，"
            f"GET /health → {srv_status} {json.dumps(srv_body, ensure_ascii=False)[:120]}；两个端口的监听方"
            f"（Get-NetTCPConnection，地址 端口 进程号）{listening}，正是 GUI 进程 {g.pid} 和服务器进程 {srv.proc.pid}")
    st = g.wait_status(lambda s: s.get("connected") and s.get("session_id"), 30)
    t0 = time.time()
    r.check("setup", st is not None, "u1 已连接")
    sid = st["session_id"]
    hello = m6h.lines_with(srv.log_text(), "入站 hello")
    r.note(f"服务器 stdout: {hello}")
    # A hello that reports tags its drawing already holds is logged twice: what
    # became of the report, then the answer. The handshake is the line that
    # ends in the welcome.
    welcomed = [line for line in hello if "-> welcome" in line]
    r.check("VAL-CLI-001", len(welcomed) == 1 and "user=u1" in welcomed[0] and "drawing=PID-1101" in welcomed[0],
            f"服务器日志出现一条以 welcome 作答的入站 hello（{len(welcomed)} 条），user_id=u1、drawing_id=PID-1101")
    # The seed drawing holds one tag (P-1001). The hello says so, and the
    # server says what it did with it before it answers.
    said = m6h.frames(g.err_text(), "->", "hello")
    reported = [line for line in hello if "上报本机已有位号" in line]
    st_objects, objects = srv.http("GET", "/drawing/PID-1101/objects")
    r.dump("t20-001-hello-frame.json", said)
    r.check("T2.4-existing-tags", len(said) == 1 and said[0].get("existing_tags") == ["P-1001"]
            and len(reported) == 1 and "user=u1" in reported[0] and "drawing=PID-1101" in reported[0]
            and "上报本机已有位号 1 个 -> 新登记 1 个（图面修订 0）" in reported[0]
            and len(welcomed) == 1 and hello.index(reported[0]) < hello.index(welcomed[0])
            and st_objects == 200 and (objects or {}).get("reported_tags") == ["P-1001"] and (objects or {}).get("total") == 0,
            f"加入时上报图里已有的位号：客户端发出的 hello 帧 {len(said)} 条，existing_tags={[f.get('existing_tags') for f in said]}；"
            f"服务器在 welcome 那一行之前记了一行上报结果（{len(reported)} 行）："
            f"{[line.split(' drawing=PID-1101 ')[-1] for line in reported]}；GET /drawing/PID-1101/objects → {st_objects}，"
            f"reported_tags={(objects or {}).get('reported_tags')}、total={(objects or {}).get('total')}（上报的位号不算对象）")
    welcome = m6h.lines_with(g.err_text(), "收到 welcome")
    r.note(f"u1 客户端日志: {welcome}")
    r.dump("t20-001-status.json", st)
    # 2026-11-11（审查 N9）：此前「peers 为数组」读的是状态接口，「claims 为数组」
    # 只看摘要行里有没有 claims=。现在读 GUI 收到的 welcome 帧原文。
    wf = m6h.frames(g.err_text(), "<-", "welcome")
    r.dump("t20-001-welcome-frame.json", wf)
    w = wf[-1] if wf else {}
    r.check("VAL-CLI-001", bool(wf) and isinstance(w.get("session_id"), str) and bool(w.get("session_id"))
            and w.get("session_id") == sid and type(w.get("base_revision")) is int and w.get("base_revision") == 0
            and isinstance(w.get("peers"), list) and isinstance(w.get("claims"), list),
            f"客户端收到的 welcome 帧原文：session_id={w.get('session_id')!r}（非空字符串，与状态接口一致）、"
            f"base_revision={w.get('base_revision')!r}（整数 0）、peers 为数组（{len(w.get('peers') or [])} 项）、"
            f"claims 为数组（{len(w.get('claims') or [])} 项）")
    rows = srv.sql("select session_id,user_id,drawing_id,status,base_revision from sessions")
    r.dump("t20-001-sessions.json", rows)
    r.note(f"sessions: {json.dumps(rows, ensure_ascii=False)}")
    r.check("VAL-CLI-001", len(rows) == 1 and rows[0]["status"] == "活跃" and rows[0]["base_revision"] == 0
            and rows[0]["session_id"] == sid and rows[0]["user_id"] == "u1" and rows[0]["drawing_id"] == "PID-1101",
            "sessions 表新增 1 行，status=活跃、base_revision=0，session_id 与客户端的一致")
    r.check("VAL-CLI-001", st.get("connected") is True and st.get("session_id") == sid,
            f"GET /api/v1/collab/status: connected={st.get('connected')} session_id 与本会话一致")

    # ── VAL-CLI-002: 60 s of heartbeats ───────────────────────────────────
    samples = []
    start = srv.sql("select last_heartbeat from sessions where session_id=?", (sid,))[0]["last_heartbeat"]
    samples.append((0.0, start))
    r.note(f"t=0 last_heartbeat={start}")
    while time.time() - t0 < 61.5:
        time.sleep(1.0)
        value = srv.sql("select last_heartbeat from sessions where session_id=?", (sid,))[0]["last_heartbeat"]
        if value != samples[-1][1]:
            samples.append((round(time.time() - t0, 1), value))
    end = srv.sql("select last_heartbeat from sessions where session_id=?", (sid,))[0]["last_heartbeat"]
    r.note(f"t=60 last_heartbeat={end}")
    values = [v for _, v in samples]
    gaps = [b - a for a, b in zip(values[1:], values[2:])]  # between heartbeats, not from the hello
    first_gap = values[1] - values[0] if len(values) > 1 else None
    r.note(f"last_heartbeat 的取值序列（相对秒, unix 秒）: {samples}")
    r.note(f"hello 到第一次心跳 {first_gap} 秒；相邻两次心跳的差: {gaps}")
    beats = m6h.lines_with(srv.log_text(), "入站 heartbeat", sid)
    stamps = [int(line.split("at=")[1]) for line in beats]
    r.note(f"服务器日志 heartbeat 行的时间戳: {stamps}")
    r.dump("t20-002-heartbeat-lines.txt", "\n".join(beats))
    r.check("VAL-CLI-002", 3 <= len(beats) <= 5, f"60 秒窗口内入站 heartbeat {len(beats)} 条（应在 3 至 5 之间）")
    increases = len(values) - 1
    r.check("VAL-CLI-002", increases >= 3 and all(b > a for a, b in zip(values, values[1:])),
            f"sessions.last_heartbeat 单调递增 {increases} 次")
    all_gaps = [b - a for a, b in zip(values, values[1:])]
    r.check("VAL-CLI-002", bool(all_gaps) and all(13 <= gap <= 17 for gap in all_gaps),
            f"相邻两次的差值都落在 13 至 17 秒: {all_gaps}")

    # The raw welcome frame, from a plain WebSocket client on another drawing
    # so the PID-1101 room is not disturbed: the shape 001 describes.
    obs = m6h.Ws(f"ws://127.0.0.1:{srv.port}/ws", "t20-shape")
    frame = obs.hello("u2", drawing="PID-SHAPE")
    obs.close()
    r.dump("t20-001-welcome-frame.json", frame)
    r.note(f"welcome 帧原文（另一张图上的普通 WS 客户端）: {json.dumps(frame, ensure_ascii=False)}")
    r.check("VAL-CLI-001", isinstance(frame, dict) and frame.get("type") == "welcome" and isinstance(frame.get("session_id"), str)
            and isinstance(frame.get("base_revision"), int) and isinstance(frame.get("peers"), list)
            and isinstance(frame.get("claims"), list),
            "welcome 帧的形状：session_id 字符串、base_revision 整数、peers 数组、claims 数组")

    # ── VAL-CLI-016: a clean quit ─────────────────────────────────────────
    quit_at = time.time()
    try:
        status, body = g.run("QUIT")
        r.note(f"POST /api/v1/run QUIT -> {status} {json.dumps(body, ensure_ascii=False)[:200]}")
    except Exception as e:  # noqa: BLE001 - the process may be gone before it answers
        r.note(f"QUIT 的响应没有读到（进程已退出）: {type(e).__name__}")
    gone = g.wait_exit(10)
    took = time.time() - quit_at
    r.check("VAL-CLI-016", gone, f"进程在 {took:.1f} 秒内退出（上限 10 秒），退出码 {g.exit_code() if gone else '?'}")
    row = m6h.wait_until(lambda: [x for x in srv.sql("select session_id,status from sessions where session_id=?", (sid,))
                                 if x["status"] == "失效"], 5)
    marked = time.time() - quit_at
    close_lines = m6h.lines_with(srv.log_text(), "入站 close 帧", sid)
    r.note(f"服务器日志: {close_lines}")
    r.note(f"u1 客户端日志: {m6h.lines_with(g.err_text(), '退出')}")
    r.check("VAL-CLI-016", bool(close_lines), "服务器记录到该会话的 Close 帧")
    r.check("VAL-CLI-016", bool(row) and marked <= 5 + took,
            f"sessions.status = {row[0]['status'] if row else '（仍是活跃）'}，退出命令发出后 {marked:.1f} 秒查到")
    time.sleep(1.0)
    r.check("VAL-CLI-016", not g.running(), f"进程 {g.pid} 已不在（进程列表的核对见运行脚本的输出）")
    r.dump("t20-server-log.txt", m6h.this_run(srv.log_text()))
    r.dump("t20-u1-collab-log.txt", "\n".join(m6h.lines_with(g.err_text(), "[collab]")))
    print(f"PID_TO_CHECK={g.pid}")
finally:
    m6h.cleanup()
    ok = r.summary()
sys.exit(0 if ok else 1)
