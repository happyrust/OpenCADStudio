# -*- coding: utf-8 -*-
"""VAL-CLI-004 (claim granted), 008 (claim visible in the panel, the drawing
and the status), 005 (claim denied), 007 (voluntary release).

u1 (3210) and u2 (3211) on their own copies of the seed drawing; fresh server
on 3240 with the default 300 s claim TTL.
"""
import json
import sys
import time

import m6h
import shot as reader   # under another name: further down `shot` is whether a capture wrote its file

r = m6h.Results("t21_claims")


def claim_pixels(png, bounds):
    """How many pixels inside `bounds` (physical px) carry the claim colour."""
    picture = reader.load(png)
    # CLAIM_COLOR is [1.0, 0.70, 0.10]: amber.
    n = reader.count(picture, [int(v) for v in bounds],
                     lambda red, green, blue: red > 240 and 165 < green < 195 and blue < 45)
    return n, (picture.width, picture.height)


try:
    dwg_a = m6h.fresh_copy("PID-1101-a.dwg", m6h.SEED_DWG)
    dwg_b = m6h.fresh_copy("PID-1101-b.dwg", m6h.SEED_DWG)
    srv = m6h.Server(3240, tag="t21-srv").start(fresh=True)
    r.check("setup", srv.wait_health(), f"服务器 /health = {srv.health()}")
    u1 = m6h.Gui("t21-u1", 3210, dwg=dwg_a, user="u1").start()
    r.check("setup", u1.wait_ready(180) is not None, "u1 GUI 就绪")
    s1 = u1.wait_status(lambda s: s.get("connected") and s.get("session_id"), 30)
    u2 = m6h.Gui("t21-u2", 3211, dwg=dwg_b, user="u2").start()
    r.check("setup", u2.wait_ready(180) is not None, "u2 GUI 就绪")
    s2 = u2.wait_status(lambda s: s.get("connected") and s.get("session_id"), 30)
    sid1, sid2 = s1["session_id"], s2["session_id"]
    doc1, doc2 = u1.doc_id(), u2.doc_id()
    handle = u1.tagged().get("P-1001")
    r.check("setup", bool(handle), f"u1 图上 P-1001 的句柄 {handle}")

    # An unclaimed baseline for 008: the symbol before anybody holds it.
    #
    # The pixels are measured on a viewport capture. A window capture reports
    # `screen_bounds` as if the viewport filled the whole window image
    # (`control_screenshot` hands `compute_grounding` the window's pixel size),
    # so they do not land on the symbol there; on a viewport capture they do.
    u1.run("ZOOM EXTENTS", doc1)
    u2.run("ZOOM EXTENTS", doc2)
    time.sleep(1.0)
    st, cap0, shot0 = u1.capture(m6h.SHOTS / "t21-008-u1-viewport-before.png", scope="viewport", max_dimension=4096)
    ann0 = next((a for a in (cap0 or {}).get("annotations") or [] if a.get("handle") == handle), None)
    before_px = claim_pixels(m6h.SHOTS / "t21-008-u1-viewport-before.png", ann0["screen_bounds"])[0] if ann0 and shot0 else None
    r.note(f"占用前（视口截图 shots/t21-008-u1-viewport-before.png）该符号 screen_bounds={ann0 and ann0.get('screen_bounds')} "
           f"内的占用色像素: {before_px}")

    # ── VAL-CLI-004 ───────────────────────────────────────────────────────
    st, body = u1.run("PIDCLAIM P-1001", doc1)
    now = int(time.time())
    r.note(f"u1 run PIDCLAIM P-1001 -> {st} ok={(body or {}).get('ok')}")
    s1b = u1.wait_status(lambda s: (s.get("claims") or {}).get("P-1001") == "u1", 10) or u1.status()
    granted = m6h.lines_with(u1.err_text(), "收到 claim_granted")
    sent = m6h.lines_with(u1.err_text(), "发送 claim")
    srv_claim = m6h.lines_with(srv.log_text(), "入站 claim", "user=u1")
    r.note(f"u1 客户端日志: {sent} {granted}")
    r.note(f"服务器日志: {srv_claim}")
    # 2026-11-11（审查 N9）：此前「session_id 等于本会话」读的是服务器日志里的
    # 请求方。现在读 GUI 收到的 claim_granted 帧原文。
    gf = m6h.frames(u1.err_text(), "<-", "claim_granted")
    r.dump("t21-004-claim-granted-frame.json", gf)
    r.check("VAL-CLI-004", bool(gf) and gf[-1].get("tag") == "P-1001" and gf[-1].get("session_id") == sid1
            and bool(srv_claim) and "claim_granted" in srv_claim[-1],
            f"客户端收到的 claim_granted 帧原文：{json.dumps(gf[-1], ensure_ascii=False) if gf else None}"
            f"（tag=P-1001，session_id 等于本会话 {sid1}）")
    rows = srv.sql("select tag,drawing_id,session_id,user_id,expires_at from claims")
    r.dump("t21-004-claims.json", rows)
    r.note(f"claims: {json.dumps(rows, ensure_ascii=False)}")
    remaining = rows[0]["expires_at"] - now if rows else None
    r.check("VAL-CLI-004", len(rows) == 1 and rows[0]["tag"] == "P-1001" and rows[0]["user_id"] == "u1"
            and rows[0]["session_id"] == sid1 and remaining is not None and 295 <= remaining <= 305,
            f"claims 表新增 1 行：tag=P-1001、user_id=u1，expires_at 与当前 unix 秒之差 {remaining}（应在 295 至 305）")

    # ── VAL-CLI-008 ───────────────────────────────────────────────────────
    time.sleep(0.5)
    png = m6h.SHOTS / "t21-008-u1-claim.png"
    st, cap, shot = u1.capture(png, max_dimension=4096)
    r.dump("t21-008-capture-window.json", cap)
    bounds = u1.ui_bounds("pid-collab")
    r.dump("t21-008-ui-bounds.json", bounds)
    r.note(f"窗口截图: {st} scope={(cap or {}).get('scope')} waited_for_frame={(cap or {}).get('waited_for_frame')} "
           f"scale_factor={(cap or {}).get('scale_factor')} size={(cap or {}).get('width')}x{(cap or {}).get('height')}")
    r.note(f"ui_bounds prefix=pid-collab（逻辑像素）: {json.dumps(bounds, ensure_ascii=False)}")
    r.check("VAL-CLI-008", st == 200 and shot and "pid-collab-row-P-1001" in bounds,
            f"窗口截图 shots/t21-008-u1-claim.png；面板占用行 pid-collab-row-P-1001 = {bounds.get('pid-collab-row-P-1001')}")
    vpng = m6h.SHOTS / "t21-008-u1-viewport-claim.png"
    st, vcap, vshot = u1.capture(vpng, scope="viewport", max_dimension=4096)
    r.dump("t21-008-capture-viewport.json", vcap)
    ann = next((a for a in (vcap or {}).get("annotations") or [] if a.get("handle") == handle), None)
    r.note(f"视口截图: {st} scope={(vcap or {}).get('scope')} scale_factor={(vcap or {}).get('scale_factor')} "
           f"size={(vcap or {}).get('width')}x{(vcap or {}).get('height')}；目标实体 {handle} 的 screen_bounds（物理像素）: "
           f"{ann and ann.get('screen_bounds')}")
    after_px, size = claim_pixels(vpng, ann["screen_bounds"]) if ann and vshot else (None, None)
    r.check("VAL-CLI-008", after_px is not None and after_px > 0 and before_px == 0,
            f"该位号实体以占用色描边（视口截图 shots/t21-008-u1-viewport-claim.png）：占用色像素 占用前 {before_px} -> 占用后 {after_px}")
    r.check("VAL-CLI-008", (s1b.get("claims") or {}).get("P-1001") == "u1",
            f"GET /api/v1/collab/status 的 claims: {json.dumps(s1b.get('claims'), ensure_ascii=False)}")

    # ── VAL-CLI-005 ───────────────────────────────────────────────────────
    st, body = u2.run("PIDCLAIM P-1001", doc2)
    r.note(f"u2 run PIDCLAIM P-1001 -> {st} ok={(body or {}).get('ok')}")
    denied = m6h.wait_until(lambda: m6h.lines_with(u2.err_text(), "收到 claim_denied"), 10) or []
    srv_denied = m6h.lines_with(srv.log_text(), "入站 claim", "user=u2")
    r.note(f"u2 客户端日志: {denied}")
    r.note(f"服务器日志: {srv_denied}")
    expires = None
    if denied:
        expires = int(denied[-1].split("expires_at=")[1].split()[0])
    r.check("VAL-CLI-005", bool(denied) and "tag=P-1001" in denied[-1] and f"holder={sid1}" in denied[-1]
            and expires is not None and expires > 0,
            f"u2 收到 claim_denied：tag=P-1001、holder={sid1[:12]}…（u1 的会话）、expires_at={expires}")
    s2b = u2.status()
    r.note(f"u2 /collab/status: claims={json.dumps(s2b.get('claims'), ensure_ascii=False)} last_claim_message={s2b.get('last_claim_message')!r}")
    r.check("VAL-CLI-005", (s2b.get("claims") or {}).get("P-1001") == "u1",
            "u2 一侧把 holder 解析成用户 u1（面板与 status 都按用户显示）")
    rows = srv.sql("select tag,user_id from claims")
    r.check("VAL-CLI-005", len(rows) == 1 and rows[0]["user_id"] == "u1", f"claims 表仍只有 1 行且 user_id=u1: {rows}")
    time.sleep(0.5)
    st, cap2, shot2 = u2.capture(m6h.SHOTS / "t21-005-u2-denied.png")
    r.check("VAL-CLI-005", st == 200 and shot2, "u2 截图（占用失败提示）shots/t21-005-u2-denied.png")

    # ── VAL-CLI-007 ───────────────────────────────────────────────────────
    st, body = u1.run("PIDRELEASE P-1001", doc1)
    r.note(f"u1 run PIDRELEASE P-1001 -> {st} ok={(body or {}).get('ok')}")
    rel1 = m6h.wait_until(lambda: m6h.lines_with(u1.err_text(), "收到 claim_released"), 10) or []
    rel2 = m6h.wait_until(lambda: m6h.lines_with(u2.err_text(), "收到 claim_released"), 10) or []
    srv_rel = m6h.lines_with(srv.log_text(), "入站 release")
    r.note(f"u1 客户端日志: {rel1}")
    r.note(f"u2 客户端日志: {rel2}")
    r.note(f"服务器日志: {srv_rel}")
    ok_reason = all(lines and "tag=P-1001" in lines[-1] and "reason=released" in lines[-1] for lines in (rel1, rel2))
    r.check("VAL-CLI-007", ok_reason, "u1 与 u2 均收到 claim_released，tag=P-1001，reason=released（与超时的 expired 不同）")
    rows = srv.sql("select count(*) n from claims where tag='P-1001'")
    r.check("VAL-CLI-007", rows[0]["n"] == 0, f"claims 表中该行已删除（行数 {rows[0]['n']}）")
    s1c = u1.wait_status(lambda s: "P-1001" not in (s.get("claims") or {}), 5) or u1.status()
    r.note(f"u1 /collab/status.claims = {json.dumps(s1c.get('claims'), ensure_ascii=False)}")

    r.dump("t21-server-log.txt", m6h.this_run(srv.log_text()))
    r.dump("t21-u1-collab-log.txt", "\n".join(m6h.lines_with(u1.err_text(), "[collab]")))
    r.dump("t21-u2-collab-log.txt", "\n".join(m6h.lines_with(u2.err_text(), "[collab]")))
finally:
    m6h.cleanup()
    ok = r.summary()
sys.exit(0 if ok else 1)
