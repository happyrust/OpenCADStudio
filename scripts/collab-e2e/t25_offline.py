# -*- coding: utf-8 -*-
"""VAL-CLI-015 (server unreachable: the client is offline and the UI is not
blocked) and VAL-CLI-017 (no `--collab` and no saved address: nothing dials
out at all).

015 runs with no server on 3240. 017 then starts a server just to have
something that would notice a connection, and watches it for 60 s.
About 80 s in all.
"""
import json
import re
import sys
import time

import m6h

r = m6h.Results("t25_offline")
try:
    # ── VAL-CLI-015 ───────────────────────────────────────────────────────
    r.check("setup", not m6h.port_listening(m6h.shifted(3240)), "3240 上没有服务器在听")
    dwg = m6h.fresh_copy("PID-1101-a.dwg", m6h.SEED_DWG)
    g = m6h.Gui("t25-offline", 3210, dwg=dwg, user="u1").start()
    took = g.wait_ready(60)
    status, body = g.ready()
    r.check("VAL-CLI-015", took is not None and took <= 5.0 and status == 200,
            f"GUI 启动 {None if took is None else round(took, 2)} 秒后 GET /api/v1/ready -> {status}（上限 5 秒）")
    st = g.status()
    r.dump("t25-015-status.json", st)
    r.check("VAL-CLI-015", st.get("configured") is True and st.get("connected") is False,
            f"/collab/status: configured={st.get('configured')} connected={st.get('connected')} "
            f"reconnect_attempts={st.get('reconnect_attempts')} last_error={st.get('last_error')!r}")
    doc = g.doc_id()
    code, circle = g.run("CIRCLE 0,0 10", doc)
    r.dump("t25-015-circle.json", circle)
    circles = (g.op("query", type="Circle")[1] or {}).get("entities") or []
    r.check("VAL-CLI-015", code == 200 and (circle or {}).get("ok") is True and len(circles) >= 1,
            f"本地编辑仍可用：POST /api/v1/run CIRCLE 0,0 10 -> {code} ok={(circle or {}).get('ok')}，图上圆 {len(circles)} 个")
    g.run("PIDCOLLAB ON", doc)
    time.sleep(1.0)
    code, cap, shot = g.capture(m6h.SHOTS / "t25-015-offline.png")
    r.check("VAL-CLI-015", code == 200 and shot, "截图 shots/t25-015-offline.png（协同面板显示未连接）")
    time.sleep(12.0)
    tries = m6h.lines_with(g.err_text(), "连接 ws://")
    fails = m6h.lines_with(g.err_text(), "断开：")
    stamps = [int(re.search(r"\bt=(\d+)", line).group(1)) for line in tries]
    gaps = [round((b - a) / 1000.0, 2) for a, b in zip(stamps, stamps[1:])]
    r.note(f"重试日志：{len(tries)} 次连接尝试，相邻两次相隔（秒）{gaps}")
    r.note(f"最近一次失败: {fails[-1] if fails else None}")
    r.dump("t25-015-collab-log.txt", "\n".join(m6h.lines_with(g.err_text(), "[collab]")))
    r.check("VAL-CLI-015", len(tries) >= 4 and len(fails) >= 3,
            f"重试日志显示周期重连尝试（约 {round(time.time() - g.launched)} 秒内 {len(tries)} 次）")
    still = g.ready()[0]
    r.check("VAL-CLI-015", still == 200 and g.running(), f"重试期间 UI 一直可用：GET /api/v1/ready -> {still}")
    g.kill()

    # ── VAL-CLI-017 ───────────────────────────────────────────────────────
    srv = m6h.Server(3240, tag="t25-srv").start(fresh=True)
    r.check("setup", srv.wait_health(), f"服务器 /health = {srv.health()}")
    n0 = srv.sql("select count(*) n from sessions")[0]["n"]
    dwg2 = m6h.fresh_copy("PID-1101-b.dwg", m6h.SEED_DWG)
    plain = m6h.Gui("t25-plain", 3211, dwg=dwg2, collab=False).start()
    r.check("setup", plain.wait_ready(60) is not None, "不带 --collab 的 GUI 就绪")
    settings = plain.appdata / "OpenCADStudio" / "settings.json"
    saved = ""
    if settings.exists():
        try:
            saved = (json.loads(settings.read_text(encoding="utf-8")).get("collab") or {}).get("server_url", "")
        except ValueError:
            saved = "(settings.json 读不出)"
    r.note(f"这个实例的 settings.json（{settings}）存在: {settings.exists()}；collab.server_url = {saved!r}")
    st = plain.status()
    r.dump("t25-017-status.json", st)
    plain.run("PIDCOLLAB ON")
    time.sleep(1.0)
    code, cap, shot = plain.capture(m6h.SHOTS / "t25-017-unconfigured.png")
    started = time.time()
    while time.time() - started < 60:
        time.sleep(5.0)
    n1 = srv.sql("select count(*) n from sessions")[0]["n"]
    inbound = m6h.lines_with(srv.log_text(), "入站")
    r.note(f"60 秒里服务器日志的入站行: {inbound}")
    r.dump("t25-017-server-log.txt", m6h.this_run(srv.log_text()))
    r.check("VAL-CLI-017", n0 == n1 == 0 and not inbound,
            f"60 秒内服务器没有任何入站连接：sessions 行数 {n0} -> {n1}，日志入站行 {len(inbound)} 条")
    r.check("VAL-CLI-017", st.get("configured") is False and st.get("connected") is False and not st.get("server_url"),
            f"/collab/status: configured={st.get('configured')} connected={st.get('connected')} server_url={st.get('server_url')!r}")
    r.check("VAL-CLI-017", code == 200 and shot, "截图 shots/t25-017-unconfigured.png（协同面板显示未配置）")
    code, _ = plain.ready()
    r.check("VAL-CLI-017", code == 200, f"GET /api/v1/ready -> {code}")
    quiet = [line for line in m6h.this_run(plain.err_text()).splitlines() if "[collab]" in line]
    r.note(f"这个实例的客户端日志里的 [collab] 行: {quiet}")
finally:
    m6h.cleanup()
    ok = r.summary()
sys.exit(0 if ok else 1)
