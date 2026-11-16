# -*- coding: utf-8 -*-
"""VAL-CLI-013, 009, 010, 011, 012 in one two-instance session.

u1 (3210) and u2 (3211) each open their own copy of the seed drawing (one
vessel, P-1001, handle the same in both) against one server on 3240.
No third observer joins the room: the `sessions` table and the roster must
show exactly the two instances.
"""
import json
import sys
import time

import m6h

r = m6h.Results("t10_broadcast")
srv = None
try:
    if not m6h.SEED_DWG.exists():
        r.check("setup", False, "种子图不存在，先跑 t01_seed.py")
        raise SystemExit(1)
    dwg_a = m6h.fresh_copy("PID-1101-a.dwg", m6h.SEED_DWG)
    dwg_b = m6h.fresh_copy("PID-1101-b.dwg", m6h.SEED_DWG)

    srv = m6h.Server(3240, tag="t10-srv").start(fresh=True)
    r.check("setup", srv.wait_health(), f"服务器 /health = {srv.health()}")
    r.note(f"开始前 sessions 行数: {srv.sql('select count(*) n from sessions')[0]['n']}")

    # ── VAL-CLI-013 (first half): u1 joins alone ──────────────────────────
    u1 = m6h.Gui("t10-u1", 3210, dwg=dwg_a, user="u1").start()
    r.check("setup", u1.wait_ready(180) is not None, "u1 GUI 就绪")
    s1 = u1.wait_status(lambda s: s.get("connected") and s.get("session_id"), 30)
    r.check("setup", s1 is not None, f"u1 已连接: session={s1 and s1.get('session_id')}")
    sid1 = s1["session_id"]
    hello1 = m6h.lines_with(srv.log_text(), "入站 hello", "user=u1")
    welcome1 = m6h.lines_with(u1.err_text(), "收到 welcome")
    r.note(f"服务器日志 u1 hello: {hello1}")
    r.note(f"u1 客户端日志 welcome: {welcome1}")
    r.dump("t10-u1-status-alone.json", s1)
    peers_alone = s1.get("peers") or []
    r.note(f"u1 独自在线时 /collab/status.peers = {json.dumps(peers_alone, ensure_ascii=False)}")
    others_alone = [p for p in peers_alone if p.get("session_id") != sid1]
    r.check("VAL-CLI-013", len(others_alone) == 0, f"u1 首次连入时没有其他在线用户（others={len(others_alone)}）")
    # 2026-11-11：welcome.peers 不含连入者自己（a0f33b354）。此前这里只是一条
    # 注记（「契约字面：False」），现在是一条检查。
    r.check("VAL-CLI-013", len(peers_alone) == 0 and s1.get("online") == 1,
            f"u1 首次连入：welcome.peers 为空数组（长度 {len(peers_alone)}），在线面板人数 online={s1.get('online')}")
    st, _, shot = u1.capture(m6h.SHOTS / "t10-013-u1-alone.png")
    r.note(f"u1 独自在线截图: {st} {shot} shots/t10-013-u1-alone.png")

    # ── VAL-CLI-013 (second half): u2 joins ───────────────────────────────
    u2 = m6h.Gui("t10-u2", 3211, dwg=dwg_b, user="u2").start()
    r.check("setup", u2.wait_ready(180) is not None, "u2 GUI 就绪")
    s2 = u2.wait_status(lambda s: s.get("connected") and s.get("session_id"), 30)
    r.check("setup", s2 is not None, f"u2 已连接: session={s2 and s2.get('session_id')}")
    sid2 = s2["session_id"]

    s1b = u1.wait_status(lambda s: any(p.get("user_id") == "u2" for p in s.get("peers") or []), 15)
    joined = m6h.lines_with(u1.err_text(), "收到 peer_joined", "user=u2")
    r.note(f"u1 客户端日志 peer_joined: {joined}")
    peer_u2 = next((p for p in (s1b or {}).get("peers") or [] if p.get("user_id") == "u2"), None)
    # 2026-11-11（审查 N9）：此前「display_name 非空」读的是状态接口，而客户端在帧里
    # 没有名字时会自己填 user_id，那一句恒为真。现在读 u1 收到的 peer_joined 帧原文。
    pj = [f for f in m6h.frames(u1.err_text(), "<-", "peer_joined") if f.get("user_id") == "u2"]
    r.dump("t10-013-peer-joined-frame.json", pj)
    pjf = pj[-1] if pj else {}
    r.check("VAL-CLI-013", bool(joined) and bool(pj) and pjf.get("session_id") == sid2
            and isinstance(pjf.get("display_name"), str) and bool(pjf.get("display_name"))
            and peer_u2 is not None and peer_u2.get("session_id") == sid2,
            f"u1 收到的 peer_joined 帧原文：{json.dumps(pjf, ensure_ascii=False)}"
            f"（session_id 是 u2 的会话，display_name 非空）")
    # 2026-11-11：此前检查的是 peers 长度 1 -> 2（peers 里含自己）。现在 peers 是
    # 别人，面板人数读 online（含本会话）。
    r.check("VAL-CLI-013", (s1b or {}).get("online") == 2 and len((s1b or {}).get("peers") or []) == 1,
            f"u1 在线面板人数 {s1.get('online')} -> {(s1b or {}).get('online')}；"
            f"其他在线用户 {len(peers_alone)} -> {len((s1b or {}).get('peers') or [])}")
    r.dump("t10-u1-status-two.json", s1b)
    s2 = u2.status()
    r.dump("t10-u2-status-two.json", s2)
    welcome2 = m6h.lines_with(u2.err_text(), "收到 welcome")
    hello2 = m6h.lines_with(srv.log_text(), "入站 hello", "user=u2")
    r.note(f"u2 客户端日志 welcome: {welcome2}")
    r.note(f"服务器日志 u2 hello: {hello2}")
    peers2 = s2.get("peers") or []
    r.check("VAL-CLI-013", any(p.get("user_id") == "u1" and p.get("session_id") == sid1 for p in peers2),
            f"u2 的 welcome.peers 含 u1（peers={[(p.get('user_id'), p.get('session_id')[:8]) for p in peers2]}）")
    # 2026-11-11：此前是一条注记（「契约字面：False，含 u2 自己」），现在是检查。
    r.check("VAL-CLI-013", len(peers2) == 1, f"u2 的 welcome.peers 长度为 1（实得 {len(peers2)}）")
    st, resp, shot = u1.capture(m6h.SHOTS / "t10-013-u1-two.png")
    r.check("VAL-CLI-013", st == 200 and shot, "u1 截图（在线面板 2 行）shots/t10-013-u1-two.png")

    # ── find the seed symbol in both drawings ─────────────────────────────
    doc1, doc2 = u1.doc_id(), u2.doc_id()
    t1, t2 = u1.tagged(), u2.tagged()
    h1, h2 = t1.get("P-1001"), t2.get("P-1001")
    r.check("setup", bool(h1) and h1 == h2, f"两份图上 P-1001 的句柄: u1={h1} u2={h2}")
    for g in (u1, u2):
        g.run("ZOOM EXTENTS")
    time.sleep(1.0)

    # ── VAL-CLI-009 ───────────────────────────────────────────────────────
    rev1_before = u1.status().get("base_revision")
    st, run1 = u1.run("PIDTAG P-1001 P-1009", doc1)
    r.dump("t10-009-u1-run.json", run1)
    r.check("VAL-CLI-009", st == 200 and (run1 or {}).get("ok") is True,
            f"u1 run PIDTAG P-1001 P-1009 -> {st} ok={(run1 or {}).get('ok')} status={(run1 or {}).get('status')}")
    got2 = m6h.wait_until(lambda: u2.author(h2).get("tag") == "P-1009", 20)
    s1c = u1.wait_status(lambda s: s.get("base_revision") == 1, 10) or u1.status()
    s2c = u2.wait_status(lambda s: s.get("base_revision") == 1, 10) or u2.status()
    sent1 = m6h.lines_with(u1.err_text(), "发送 op")
    applied1 = m6h.lines_with(u1.err_text(), "收到 op_applied")
    applied2 = m6h.lines_with(u2.err_text(), "收到 op_applied")
    srv_ops = m6h.lines_with(srv.log_text(), "入站 op")
    r.note(f"u1 客户端日志 发送 op: {sent1}")
    r.note(f"u1 客户端日志 op_applied: {applied1}")
    r.note(f"u2 客户端日志 op_applied: {applied2}")
    r.note(f"服务器日志 op: {srv_ops}")
    op_id = ""
    if applied1:
        op_id = applied1[-1].split("op_id=")[1].split()[0]
    r.check("VAL-CLI-009", bool(applied1) and bool(op_id) and rev1_before == 0 and s1c.get("base_revision") == 1,
            f"u1 收到 op_applied：op_id={op_id!r}，revision {rev1_before} -> {s1c.get('base_revision')}")
    same = bool(applied2) and f"op_id={op_id} " in applied2[-1] + " " and "revision=1" in applied2[-1]
    r.check("VAL-CLI-009", same, f"u2 收到同一条 op_applied（op_id 相同、revision=1）: {applied2[-1] if applied2 else None}")
    st, xd2 = u2.xdata(h2)
    r.dump("t10-009-u2-xdata.json", xd2)
    r.check("VAL-CLI-009", bool(got2) and u2.author(h2).get("tag") == "P-1009",
            f"u2 GET /entities/{h2}/xdata?app=PID_AUTHOR -> {json.dumps(xd2, ensure_ascii=False)}")
    r.note(f"u2 /collab/status: base_revision={s2c.get('base_revision')} applied_ops={s2c.get('applied_ops')} "
           f"last_apply_error={s2c.get('last_apply_error')}")
    for name, g in (("u1", u1), ("u2", u2)):
        st, resp, shot = g.capture(m6h.SHOTS / f"t10-009-{name}.png")
        r.check("VAL-CLI-009", st == 200 and shot, f"{name} 截图 shots/t10-009-{name}.png")

    # ── VAL-CLI-010 ───────────────────────────────────────────────────────
    st, put2 = u2.api("PUT", f"/entities/{h2}/xdata/PID_AUTHOR",
                      [{"code": 1000, "value": 'props={"设计压力":"2.5MPa"}'}])
    r.dump("t10-010-u2-put.json", put2)
    r.note(f"u2 PUT /entities/{h2}/xdata/PID_AUTHOR props 设计压力=2.5MPa -> {st} ok={(put2 or {}).get('ok')}")
    s1d = u1.wait_status(lambda s: s.get("base_revision") == 2, 15) or u1.status()
    s2d = u2.wait_status(lambda s: s.get("base_revision") == 2, 15) or u2.status()
    sent2 = m6h.lines_with(u2.err_text(), "发送 op")
    srv_ops = m6h.lines_with(srv.log_text(), "入站 op")
    errors2 = m6h.lines_with(u2.err_text(), "收到 error")
    r.note(f"u2 客户端日志 发送 op: {sent2}")
    r.note(f"服务器日志 op: {srv_ops}")
    r.check("VAL-CLI-010", bool(sent2) and "base_revision=1" in sent2[-1],
            f"u2 出站 op 的 base_revision = 1: {sent2[-1] if sent2 else None}")
    srv_second = [line for line in srv_ops if "user=u2" in line]
    r.check("VAL-CLI-010", bool(srv_second) and "base_revision=1 " in srv_second[-1] and "kind=set_property" in srv_second[-1]
            and "revision=2" in srv_second[-1] and not errors2,
            f"服务器接受（op_applied revision=2，未回 error）: {srv_second[-1] if srv_second else None}")
    applied1 = m6h.lines_with(u1.err_text(), "收到 op_applied")
    r.check("VAL-CLI-010", bool(applied1) and "revision=2" in applied1[-1] and s1d.get("base_revision") == 2,
            f"u1 随后收到 op_applied revision=2: {applied1[-1] if applied1 else None}")
    rows = srv.sql("select session_id,user_id,status,base_revision from sessions where drawing_id='PID-1101' order by created_at")
    r.dump("t10-010-sessions.json", rows)
    r.check("VAL-CLI-010", len(rows) == 2 and all(row["base_revision"] == 2 for row in rows),
            f"sessions 表两行的 base_revision: {[(row['user_id'], row['base_revision']) for row in rows]}")
    a1 = m6h.wait_until(lambda: "2.5MPa" in u1.author(h1).get("props", ""), 10)
    r.note(f"u1 本地该符号的 props: {u1.author(h1).get('props')}（u2 的属性修改已到 u1: {bool(a1)}）")

    # ── VAL-CLI-011 ───────────────────────────────────────────────────────
    st, body = srv.http("POST", "/drawing/PID-1101/state", {"user_id": "u4", "state": "只读"})
    r.note(f"POST /drawing/PID-1101/state {{user_id:u4,state:只读}} -> {st} {json.dumps(body, ensure_ascii=False)}")
    state_row = srv.sql("select drawing_id,state from drawings where drawing_id='PID-1101'")
    r.note(f"drawings 表: {state_row}")
    s2e = u2.wait_status(lambda s: s.get("readonly") is True, 15) or u2.status()
    down2 = m6h.lines_with(u2.err_text(), "收到 readonly_downgrade")
    down1 = m6h.lines_with(u1.err_text(), "收到 readonly_downgrade")
    srv_down = m6h.lines_with(srv.log_text(), "readonly_downgrade") + m6h.lines_with(srv.log_text(), "图纸切只读")
    r.note(f"服务器日志: {srv_down}")
    r.note(f"u2 客户端日志: {down2}")
    r.dump("t10-011-u2-status.json", s2e)
    r.check("VAL-CLI-011", st == 200 and bool(down2) and "reason=" in down2[-1] and f"holder={sid1}" in down2[-1]
            and s2e.get("readonly") is True and s2e.get("readonly_holder") == sid1,
            f"u2 收到 readonly_downgrade（含 reason 与 holder）；status.readonly={s2e.get('readonly')} "
            f"readonly_holder={s2e.get('readonly_holder')} readonly_holder_user={s2e.get('readonly_holder_user')}")
    st, run2 = u2.run("PIDTAG P-1009 P-1010", doc2)
    r.dump("t10-011-u2-run.json", run2)
    error2 = (run2 or {}).get("error")
    code2 = (run2 or {}).get("code") or (error2.get("code") if isinstance(error2, dict) else None)
    r.note(f"u2 写命令完整响应: {json.dumps(run2, ensure_ascii=False)[:700]}")
    r.check("VAL-CLI-011", (run2 or {}).get("ok") is False and "COLLAB_READONLY" in json.dumps(run2, ensure_ascii=False),
            f"u2 run PIDTAG P-1009 P-1010 -> HTTP {st} ok={(run2 or {}).get('ok')} code={code2}")
    tag2 = u2.author(h2).get("tag")
    r.check("VAL-CLI-011", tag2 == "P-1009", f"u2 的 PID_AUTHOR.tag 读回仍为 {tag2!r}")
    s1e = u1.status()
    # 2026-11-11（审查 S8，提交 97ff6d5de）：图纸切只读时，持有者也收到一条 readonly_downgrade，
    # 点名的 holder 是它自己——告诉它编辑权在它手里；它不因此变成只读。旧检查要求持有者一条
    # 都收不到。现在查：status.readonly 为 False，并且收到的每一条都点名它自己。
    r.check("VAL-CLI-011", s1e.get("readonly") is False and bool(down1) and all(f"holder={sid1}" in line for line in down1),
            f"u1（编辑权持有者）没有被降级：status.readonly={s1e.get('readonly')}，收到 {len(down1)} 条 readonly_downgrade，"
            f"每一条点名的持有者都是它自己：{down1}")
    st, run1b = u1.run("PIDTAG P-1009 P-1010", doc1)
    r.dump("t10-011-u1-run.json", run1b)
    r.check("VAL-CLI-011", st == 200 and (run1b or {}).get("ok") is True and u1.author(h1).get("tag") == "P-1010",
            f"u1 同类写命令 -> {st} ok={(run1b or {}).get('ok')}，u1 的 tag = {u1.author(h1).get('tag')!r}")
    followed = m6h.wait_until(lambda: u2.author(h2).get("tag") == "P-1010", 15)
    r.note(f"只读的 u2 仍接收广播：u1 改名后 u2 的 tag = {u2.author(h2).get('tag')!r}（{bool(followed)}）")

    # ── VAL-CLI-012 ───────────────────────────────────────────────────────
    time.sleep(0.5)
    st, cap, shot = u2.capture(m6h.SHOTS / "t10-012-u2-readonly.png")
    r.dump("t10-012-u2-capture.json", cap)
    bounds = u2.ui_bounds("pid-collab")
    r.dump("t10-012-u2-ui-bounds.json", bounds)
    r.note(f"u2 ui_bounds prefix=pid-collab: {json.dumps(bounds, ensure_ascii=False)}")
    r.check("VAL-CLI-012", st == 200 and shot, f"u2 截图 shots/t10-012-u2-readonly.png（scale_factor={(cap or {}).get('scale_factor')}）")
    r.check("VAL-CLI-012", "pid-collab-readonly" in bounds,
            f"协同面板画出了只读标识（pid-collab-readonly = {bounds.get('pid-collab-readonly')}）")
    r.check("VAL-CLI-012", "pid-collab-holder-u1" in bounds and not any(k.startswith("pid-collab-holder-") and k != "pid-collab-holder-u1" for k in bounds),
            f"在线面板把编辑权持有者标在 u1 那一行（pid-collab-holder-u1 = {bounds.get('pid-collab-holder-u1')}）")
    bounds1 = u1.ui_bounds("pid-collab")
    st, cap1, shot1 = u1.capture(m6h.SHOTS / "t10-012-u1-editable.png")
    r.check("VAL-CLI-012", "pid-collab-readonly" not in bounds1 and not any(k.startswith("pid-collab-holder-") for k in bounds1),
            f"可编辑的 u1 面板上没有这两样（对照）：{sorted(bounds1)}；截图 shots/t10-012-u1-editable.png")

    # ── what the server and both clients said, for the record ─────────────
    r.dump("t10-server-log.txt", m6h.this_run(srv.log_text()))
    r.dump("t10-u1-collab-log.txt", "\n".join(m6h.lines_with(u1.err_text(), "[collab]")))
    r.dump("t10-u2-collab-log.txt", "\n".join(m6h.lines_with(u2.err_text(), "[collab]")))
    audit = srv.sql("select seq,action,object_ref,user_id,change_text from audit_log order by seq")
    r.dump("t10-audit.json", audit)
    r.note(f"audit_log: {json.dumps(audit, ensure_ascii=False)}")
finally:
    m6h.cleanup()
    ok = r.summary()
sys.exit(0 if ok else 1)
