# -*- coding: utf-8 -*-
"""VAL-CROSS-006：首次访问，从空列表到两人。

u1 的 GUI（3210）启动时不带任何协同参数，APPDATA 是一个刚建的空目录：没有
settings.json，没有协同配置。在程序里用命令把用户、图号、服务器地址配上
（PIDCOLLAB USER / DRAWING / SERVER，经 --http 的 REST 通道发，不动指针），
配置随即落盘，这次运行当场连上。随后 u2（3211，带 --collab 启动）连入同一
张图。

证据：logs/x06-*.json（settings.json 的 collab 段、两端的状态、帧原文、
sessions 表）、logs/gui-x06-*.err.log、shots/cross-first-0.png（0 名其他
用户）、shots/cross-first-2.png（共 2 人）。
"""
import json
import shutil
import sys
import time

import m7h

R = m7h.Results("x06_first_visit")
URL = f"ws://127.0.0.1:{m7h.shifted(3240)}/ws"

try:
    if not m7h.SEED_DWG.exists():
        R.check("setup", False, f"种子图不存在：{m7h.SEED_DWG}")
        raise SystemExit(1)
    dwg_a = m7h.fresh_copy("x06-PID-1101-a.dwg", m7h.SEED_DWG)
    dwg_b = m7h.fresh_copy("x06-PID-1101-b.dwg", m7h.SEED_DWG)

    srv = m7h.Server(3240, tag="x06-srv").start(fresh=True)
    R.check("setup", srv.wait_health(), f"服务器 /health = {srv.health()}")
    empty = (srv.sql("select count(*) n from sessions")[0]["n"], srv.sql("select count(*) n from claims")[0]["n"])
    R.check("VAL-CROSS-006", empty == (0, 0), f"开始前 sessions 与 claims 表为空（行数 {empty}）")

    # 全新用户：APPDATA 目录先清掉，启动时里面什么都没有。
    u1 = m7h.Gui("x06-u1", 3210, dwg=dwg_a, collab=False)
    if u1.appdata.exists():
        shutil.rmtree(u1.appdata)
    settings = u1.appdata / "OpenCADStudio" / "settings.json"
    u1.start()
    R.check("setup", u1.wait_ready(180) is not None, "u1 GUI 就绪（启动参数里没有 --collab / --user / --drawing）")
    R.note(f"u1 启动参数: {' '.join(u1.args())}")
    doc1 = u1.doc_id()
    before = u1.status()
    R.dump("x06-u1-status-before.json", before)
    saved_before = json.loads(settings.read_text(encoding="utf-8")).get("collab") if settings.exists() else None
    R.check("VAL-CROSS-006", before.get("configured") is False and before.get("connected") is False
            and not (saved_before or {}).get("server_url"),
            f"配置之前：configured={before.get('configured')} connected={before.get('connected')}，"
            f"settings.json 的 collab 段 = {json.dumps(saved_before, ensure_ascii=False)}")
    time.sleep(3.0)
    R.check("VAL-CROSS-006", srv.sql("select count(*) n from sessions")[0]["n"] == 0,
            "配置之前服务器没有收到任何连接（sessions 仍为 0 行）")

    # 在程序里配置并保存。
    for cmd in ("PIDCOLLAB USER u1", "PIDCOLLAB DRAWING PID-1101", f"PIDCOLLAB SERVER {URL}"):
        st, body = u1.run(cmd, doc1)
        R.dump(f"x06-u1-run-{cmd.split()[1].lower()}.json", {"http_status": st, "body": body})
        R.check("VAL-CROSS-006", st == 200 and (body or {}).get("ok") is True, f"u1 run {cmd} -> {st} ok={(body or {}).get('ok')}")
    saved = m7h.wait_until(lambda: settings.exists() and json.loads(settings.read_text(encoding="utf-8")).get("collab"), 10) or {}
    R.dump("x06-u1-settings-collab.json", saved)
    R.check("VAL-CROSS-006", saved.get("server_url") == URL and saved.get("user_id") == "u1" and saved.get("drawing_id") == "PID-1101",
            f"{settings} 的 collab 段 = {json.dumps(saved, ensure_ascii=False)}")

    # 随后连接：不重启，这次运行当场连上。
    s1 = u1.wait_status(lambda s: s.get("connected") and s.get("session_id"), 30)
    R.check("VAL-CROSS-006", s1 is not None, f"u1 随后连上: session={s1 and s1.get('session_id')}")
    s1 = s1 or u1.status()
    sid1 = s1.get("session_id")
    R.dump("x06-u1-status-alone.json", s1)
    w1 = m7h.frames(u1.err_text(), "<-", "welcome")
    hello1 = m7h.frames(u1.err_text(), "->", "hello")
    R.dump("x06-u1-hello-welcome-frames.json", {"hello": hello1, "welcome": w1})
    wf1 = w1[-1] if w1 else {}
    R.check("VAL-CROSS-006", bool(hello1) and hello1[-1].get("user_id") == "u1" and hello1[-1].get("drawing_id") == "PID-1101",
            f"u1 发出的 hello 帧原文: {json.dumps(hello1[-1] if hello1 else None, ensure_ascii=False)}")
    R.check("VAL-CROSS-006", bool(w1) and wf1.get("peers") == [] and s1.get("peers") == [] and s1.get("online") == 1,
            f"u1 的 welcome.peers 为空（帧原文 peers={wf1.get('peers')!r}），在线面板 0 名其他用户、共 {s1.get('online')} 人")
    time.sleep(1.0)
    bounds = u1.ui_bounds("pid-collab")
    st, resp, shot = u1.capture(m7h.SHOTS / "cross-first-0.png")
    R.check("VAL-CROSS-006", st == 200 and shot and "pid-collab-panel" in bounds,
            f"u1 截图 shots/cross-first-0.png（协同面板在：{bounds.get('pid-collab-panel')}）")

    u2 = m7h.Gui("x06-u2", 3211, dwg=dwg_b, user="u2").start()
    R.check("setup", u2.wait_ready(180) is not None, "u2 GUI 就绪")
    s2 = u2.wait_status(lambda s: s.get("connected") and s.get("session_id"), 30)
    R.check("setup", s2 is not None, f"u2 已连接: session={s2 and s2.get('session_id')}")
    s2 = s2 or u2.status()
    sid2 = s2.get("session_id")

    s1b = u1.wait_status(lambda s: any(p.get("user_id") == "u2" for p in s.get("peers") or []), 15) or u1.status()
    R.dump("x06-u1-status-two.json", s1b)
    pj = [f for f in m7h.frames(u1.err_text(), "<-", "peer_joined") if f.get("user_id") == "u2"]
    R.dump("x06-u1-peer-joined-frames.json", pj)
    R.check("VAL-CROSS-006", bool(pj) and pj[-1].get("user_id") == "u2" and pj[-1].get("session_id") == sid2,
            f"u1 收到的 peer_joined 帧原文: {json.dumps(pj[-1] if pj else None, ensure_ascii=False)}")
    R.check("VAL-CROSS-006", len(s1b.get("peers") or []) == 1 and s1b.get("online") == 2,
            f"u1 面板：其他用户 {len(s1b.get('peers') or [])} 名、共 {s1b.get('online')} 人")
    w2 = m7h.frames(u2.err_text(), "<-", "welcome")
    R.dump("x06-u2-welcome-frames.json", w2)
    wf2 = w2[-1] if w2 else {}
    R.check("VAL-CROSS-006", any(p.get("user_id") == "u1" and p.get("session_id") == sid1 for p in wf2.get("peers") or []),
            f"u2 的 welcome.peers 含 u1（帧原文 peers={json.dumps(wf2.get('peers'), ensure_ascii=False)}）")
    R.check("VAL-CROSS-006", len(wf2.get("peers") or []) == 1, f"u2 的 welcome.peers 只有 u1 一项（{len(wf2.get('peers') or [])} 项）")
    time.sleep(1.0)
    st, resp, shot = u1.capture(m7h.SHOTS / "cross-first-2.png")
    R.check("VAL-CROSS-006", st == 200 and shot, "u1 截图 shots/cross-first-2.png（共 2 人）")
    rows = srv.sql("select session_id,user_id,drawing_id,status from sessions order by created_at")
    R.dump("x06-sessions.json", rows)
    R.check("VAL-CROSS-006", len(rows) == 2 and [r["user_id"] for r in rows] == ["u1", "u2"]
            and all(r["status"] == "活跃" and r["drawing_id"] == "PID-1101" for r in rows),
            f"sessions 表 2 行: {json.dumps(rows, ensure_ascii=False)}")
except SystemExit:
    raise
except Exception as error:  # noqa: BLE001
    import traceback
    R.check("script", False, f"脚本异常: {error!r}\n{traceback.format_exc()}")
finally:
    m7h.cleanup()
    ok = R.summary()
sys.exit(0 if ok else 1)
