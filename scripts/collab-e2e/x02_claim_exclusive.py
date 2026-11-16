# -*- coding: utf-8 -*-
"""VAL-CROSS-002：占用的排他性贯穿 GUI 与服务器。

u1（3210）与 u2（3211）各开一份种子图，连同一个服务器（3240）。u1 占
P-1001 并修改成功；u2 占用被拒、写入被拒、值不变；u1 释放后 u2 占用获批、
写入成功、u1 读回新值。另起一个 WS 会话（u4）直接向服务器提交对 P-1001 的
op，查服务器自己也拒绝（不只是 GUI 自己不发）。

证据：logs/x02-*.json（写操作的响应、claims 表前后、帧原文）、
logs/gui-x02-*.err.log、logs/ws-x02-u4.jsonl、shots/x02-u1.png、
shots/x02-u2-denied.png、shots/x02-u2-granted.png。
"""
import json
import sys
import time

import m7h

R = m7h.Results("x02_claim_exclusive")
FIELD = "设计压力"


def put(gui, handle, value):
    return gui.api("PUT", f"/entities/{handle}/xdata/PID_AUTHOR", [{"code": 1000, "value": 'props={"%s":"%s"}' % (FIELD, value)}])


def code_of(body):
    body = body or {}
    error = body.get("error")
    return body.get("code") or (error.get("code") if isinstance(error, dict) else None)


def claims(srv):
    return srv.sql("select tag, drawing_id, session_id, user_id, expires_at from claims order by tag")


try:
    if not m7h.SEED_DWG.exists():
        R.check("setup", False, f"种子图不存在：{m7h.SEED_DWG}")
        raise SystemExit(1)
    dwg_a = m7h.fresh_copy("x02-PID-1101-a.dwg", m7h.SEED_DWG)
    dwg_b = m7h.fresh_copy("x02-PID-1101-b.dwg", m7h.SEED_DWG)
    srv = m7h.Server(3240, tag="x02-srv").start(fresh=True)
    R.check("setup", srv.wait_health(), f"服务器 /health = {srv.health()}")

    u1 = m7h.Gui("x02-u1", 3210, dwg=dwg_a, user="u1").start()
    u2 = m7h.Gui("x02-u2", 3211, dwg=dwg_b, user="u2").start()
    R.check("setup", u1.wait_ready(180) is not None and u2.wait_ready(180) is not None, "两个 GUI 就绪")
    s1 = u1.wait_status(lambda s: s.get("connected") and s.get("session_id"), 30)
    s2 = u2.wait_status(lambda s: s.get("connected") and s.get("session_id"), 30)
    R.check("setup", s1 is not None and s2 is not None, "u1、u2 均已连接")
    sid1, sid2 = s1["session_id"], s2["session_id"]
    doc1, doc2 = u1.doc_id(), u2.doc_id()
    h1, h2 = u1.tagged().get("P-1001"), u2.tagged().get("P-1001")
    R.check("setup", bool(h1) and h1 == h2, f"两份图上 P-1001 的句柄: u1={h1} u2={h2}")
    for g in (u1, u2):
        g.run("ZOOM EXTENTS")
    R.dump("x02-claims-0-before.json", claims(srv))
    R.note(f"claims 表（开始）: {claims(srv)}")

    # ── u1 占用并修改 ─────────────────────────────────────────────────────
    st, body = u1.run("PIDCLAIM P-1001", doc1)
    u1.wait_status(lambda s: (s.get("claims") or {}).get("P-1001") == "u1", 10)
    g1 = m7h.frames(u1.err_text(), "<-", "claim_granted")
    rows = claims(srv)
    R.dump("x02-claims-1-u1-holds.json", rows)
    R.check("VAL-CROSS-002", bool(g1) and g1[-1].get("tag") == "P-1001" and g1[-1].get("session_id") == sid1
            and len(rows) == 1 and rows[0]["user_id"] == "u1" and rows[0]["session_id"] == sid1,
            f"u1 占 P-1001：claim_granted 帧 {json.dumps(g1[-1] if g1 else None, ensure_ascii=False)}；claims 表 {rows}")
    st, put1 = put(u1, h1, "1.6MPa")
    R.dump("x02-u1-put-while-holding.json", {"http_status": st, "body": put1})
    u1.wait_status(lambda s: s.get("base_revision") == 1, 10)
    m7h.wait_until(lambda: "1.6MPa" in (u2.author(h2).get("props") or ""), 15)
    R.check("VAL-CROSS-002", st == 200 and (put1 or {}).get("ok") is True and "1.6MPa" in (u1.author(h1).get("props") or "")
            and u1.status().get("base_revision") == 1,
            f"u1 在 GUI 里改 P-1001 的 {FIELD} 为 1.6MPa 成功: PUT -> {st} ok={(put1 or {}).get('ok')}，"
            f"修订 {u1.status().get('base_revision')}；u2 也读到 props={u2.author(h2).get('props')!r}")

    # ── u2 占用被拒 ───────────────────────────────────────────────────────
    st, body = u2.run("PIDCLAIM P-1001", doc2)
    u2.wait_status(lambda s: (s.get("claims") or {}).get("P-1001") == "u1", 10)
    d2 = m7h.frames(u2.err_text(), "<-", "claim_denied")
    R.dump("x02-u2-claim-denied-frames.json", d2)
    R.check("VAL-CROSS-002", bool(d2) and d2[-1].get("tag") == "P-1001" and d2[-1].get("holder") == sid1
            and not m7h.frames(u2.err_text(), "<-", "claim_granted"),
            f"u2 对同一位号的占用被拒：claim_denied 帧 {json.dumps(d2[-1] if d2 else None, ensure_ascii=False)}")
    R.check("VAL-CROSS-002", claims(srv) == rows, f"claims 表不变: {claims(srv)}")

    # ── u2 的写操作被拒 ───────────────────────────────────────────────────
    ops_before = len(m7h.lines_with(srv.log_text(), "入站 op", "user=u2"))
    st, put2 = put(u2, h2, "9.9MPa")
    R.dump("x02-u2-put-denied.json", {"http_status": st, "body": put2})
    R.check("VAL-CROSS-002", st == 409 and (put2 or {}).get("ok") is False and code_of(put2) == "CLAIM_HELD",
            f"u2 在 GUI 里写同一位号的属性被拒: PUT -> HTTP {st} ok={(put2 or {}).get('ok')} code={code_of(put2)}")
    st, run2 = u2.run("PIDTAG P-1001 P-1009", doc2)
    R.dump("x02-u2-run-denied.json", {"http_status": st, "body": run2})
    R.check("VAL-CROSS-002", st == 409 and (run2 or {}).get("ok") is False and code_of(run2) == "CLAIM_HELD",
            f"u2 run PIDTAG P-1001 P-1009 -> HTTP {st} ok={(run2 or {}).get('ok')} code={code_of(run2)}")
    a2 = u2.author(h2)
    R.check("VAL-CROSS-002", "1.6MPa" in (a2.get("props") or "") and "9.9MPa" not in (a2.get("props") or "") and a2.get("tag") == "P-1001",
            f"u2 本地该属性值未变: tag={a2.get('tag')!r} props={a2.get('props')!r}")
    time.sleep(1.0)
    ops_after = len(m7h.lines_with(srv.log_text(), "入站 op", "user=u2"))
    denied = srv.sql("select seq, action, object_ref, user_id, change_text from audit_log where user_id='u2' and action='write_denied' order by seq")
    R.dump("x02-audit-u2-denied.json", denied)
    R.check("VAL-CROSS-002", ops_after == ops_before and len(denied) == 2 and all(r["object_ref"] == "P-1001" and "占用" in r["change_text"] for r in denied),
            f"u2 没有把 op 发给服务器（入站 op {ops_before} -> {ops_after}），两次尝试都留痕: {json.dumps(denied, ensure_ascii=False)}")
    st, resp, shot = u2.capture(m7h.SHOTS / "x02-u2-denied.png")
    R.check("VAL-CROSS-002", st == 200 and shot, "u2 截图 shots/x02-u2-denied.png")
    st, resp, shot = u1.capture(m7h.SHOTS / "x02-u1.png")
    R.check("VAL-CROSS-002", st == 200 and shot, "u1 截图 shots/x02-u1.png")

    # ── 服务器自己也拒绝：不经 GUI，直接提交 op ───────────────────────────
    u4 = m7h.Ws(f"ws://127.0.0.1:{srv.port}/ws", "x02-u4")
    w4 = u4.hello("u4", "PID-1101")
    u4.send({"type": "op", "session_id": u4.session_id, "base_revision": (w4 or {}).get("base_revision"), "op_id": "op-x02-u4",
             "payload": {"kind": "set_property", "object_ref": "P-1001", "field": FIELD, "value": "7.7MPa"}})
    e4 = u4.wait_for(lambda m: m.get("type") in ("error", "op_applied") and (m.get("type") == "error" or m.get("op_id") == "op-x02-u4"), 10)
    R.dump("x02-u4-op-refused.json", e4)
    rev = (srv.http("GET", "/drawing/PID-1101/objects")[1] or {}).get("revision")
    R.check("VAL-CROSS-002", (e4 or {}).get("type") == "error" and (e4 or {}).get("code") == "CLAIM_HELD" and (e4 or {}).get("holder") == sid1 and rev == 1,
            f"另一个会话（u4，WS）直接提交对 P-1001 的 op，服务器回: {json.dumps(e4, ensure_ascii=False)}；图纸修订仍为 {rev}")
    u4.close()

    # ── u1 释放，u2 占用获批并写入成功 ───────────────────────────────────
    st, body = u1.run("PIDRELEASE P-1001", doc1)
    u2.wait_status(lambda s: "P-1001" not in (s.get("claims") or {}), 10)
    rel2 = m7h.frames(u2.err_text(), "<-", "claim_released")
    R.dump("x02-claims-2-released.json", claims(srv))
    R.check("VAL-CROSS-002", bool(rel2) and rel2[-1].get("tag") == "P-1001" and claims(srv) == [],
            f"u1 释放：u2 收到 claim_released 帧 {json.dumps(rel2[-1] if rel2 else None, ensure_ascii=False)}；claims 表 {claims(srv)}")
    st, body = u2.run("PIDCLAIM P-1001", doc2)
    u2.wait_status(lambda s: (s.get("claims") or {}).get("P-1001") == "u2", 10)
    g2 = m7h.frames(u2.err_text(), "<-", "claim_granted")
    rows2 = claims(srv)
    R.dump("x02-claims-3-u2-holds.json", rows2)
    R.check("VAL-CROSS-002", bool(g2) and g2[-1].get("tag") == "P-1001" and g2[-1].get("session_id") == sid2
            and len(rows2) == 1 and rows2[0]["user_id"] == "u2",
            f"u2 重新申请获批：claim_granted 帧 {json.dumps(g2[-1] if g2 else None, ensure_ascii=False)}；claims 表 {rows2}")
    st, put2b = put(u2, h2, "2.5MPa")
    R.dump("x02-u2-put-granted.json", {"http_status": st, "body": put2b})
    got1 = m7h.wait_until(lambda: "2.5MPa" in (u1.author(h1).get("props") or ""), 15)
    applied1 = [f for f in m7h.frames(u1.err_text(), "<-", "op_applied") if f.get("revision") == 2]
    R.dump("x02-u1-op-applied-frames.json", applied1)
    R.check("VAL-CROSS-002", st == 200 and (put2b or {}).get("ok") is True,
            f"此后 u2 的同类写操作: PUT -> {st} ok={(put2b or {}).get('ok')}")
    R.check("VAL-CROSS-002", bool(applied1) and bool(got1),
            f"u1 收到 op_applied（{json.dumps(applied1[-1] if applied1 else None, ensure_ascii=False)}）并读回 props={u1.author(h1).get('props')!r}")
    st, resp, shot = u2.capture(m7h.SHOTS / "x02-u2-granted.png")
    R.check("VAL-CROSS-002", st == 200 and shot, "u2 截图 shots/x02-u2-granted.png")
except SystemExit:
    raise
except Exception as error:  # noqa: BLE001
    import traceback
    R.check("script", False, f"脚本异常: {error!r}\n{traceback.format_exc()}")
finally:
    m7h.cleanup()
    ok = R.summary()
sys.exit(0 if ok else 1)
