# -*- coding: utf-8 -*-
"""VAL-CLI-006: a claim left alone is released when its 300 s TTL runs out, and
every session on the drawing is told.

u1 (3210) claims P-1001 and does nothing more; u2 (3211) is on the same
drawing. The server runs with its default claim TTL (300 s), so this takes a
little over five minutes.
"""
import json
import sys
import time

import m6h

r = m6h.Results("t22_ttl")
try:
    dwg_a = m6h.fresh_copy("PID-1101-a.dwg", m6h.SEED_DWG)
    dwg_b = m6h.fresh_copy("PID-1101-b.dwg", m6h.SEED_DWG)
    srv = m6h.Server(3240, tag="t22-srv").start(fresh=True)
    r.check("setup", srv.wait_health(), f"服务器 /health = {srv.health()}（默认 claim TTL，未传 --claim-ttl）")
    u1 = m6h.Gui("t22-u1", 3210, dwg=dwg_a, user="u1").start()
    r.check("setup", u1.wait_ready(180) is not None, "u1 GUI 就绪")
    u1.wait_status(lambda s: s.get("connected") and s.get("session_id"), 30)
    u2 = m6h.Gui("t22-u2", 3211, dwg=dwg_b, user="u2").start()
    r.check("setup", u2.wait_ready(180) is not None, "u2 GUI 就绪")
    u2.wait_status(lambda s: s.get("connected") and s.get("session_id"), 30)

    st, body = u1.run("PIDCLAIM P-1001")
    m6h.wait_until(lambda: srv.sql("select count(*) n from claims where tag='P-1001'")[0]["n"] == 1, 10)
    rows = srv.sql("select tag,user_id,expires_at from claims where tag='P-1001'")
    r.check("setup", len(rows) == 1, f"u1 占用 P-1001: {rows}")
    expires = rows[0]["expires_at"]
    before = srv.sql("select count(*) n from claims where tag='P-1001'")[0]["n"]
    r.note(f"到期前 select count(*) from claims where tag='P-1001' = {before}；expires_at={expires}，"
           f"距现在 {expires - int(time.time())} 秒")

    # Nothing refreshes the claim. Wait until 10 s past its expiry, looking in
    # now and then so the log shows the row was there until the end.
    while time.time() < expires + 10:
        left = expires + 10 - time.time()
        time.sleep(min(30, max(0.5, left)))
        n = srv.sql("select count(*) n from claims where tag='P-1001'")[0]["n"]
        r.note(f"t={int(time.time()) - expires:+d}s（相对到期时刻） claims 行数 {n}")
    after = srv.sql("select count(*) n from claims where tag='P-1001'")[0]["n"]
    r.check("VAL-CLI-006", before == 1 and after == 0,
            f"claims 表中 tag=P-1001 的行在到期后被删除：到期前 {before} 行，到期后 10 秒 {after} 行")
    rel1 = m6h.lines_with(u1.err_text(), "收到 claim_released")
    rel2 = m6h.lines_with(u2.err_text(), "收到 claim_released")
    r.note(f"u1 客户端日志: {rel1}")
    r.note(f"u2 客户端日志: {rel2}")
    for name, lines in (("u1", rel1), ("u2", rel2)):
        r.check("VAL-CLI-006", len(lines) == 1 and "tag=P-1001" in lines[0] and "reason=expired" in lines[0],
                f"{name} 收到一条 claim_released，tag=P-1001，reason=expired（表示超时）")
    ttl = m6h.lines_with(srv.log_text(), "占用到期回收", "tag=P-1001")
    r.note(f"服务器日志 TTL 回收行: {ttl}")
    at = int(ttl[-1].split(" at=")[1]) if ttl else None
    r.check("VAL-CLI-006", len(ttl) == 1 and at is not None and expires <= at <= expires + 3,
            f"服务器日志有一条 TTL 回收行，回收时刻 at={at}，在 expires_at={expires} 之后 {None if at is None else at - expires} 秒")
    s1 = u1.status()
    r.note(f"u1 /collab/status.claims = {json.dumps(s1.get('claims'), ensure_ascii=False)} "
           f"last_claim_message={s1.get('last_claim_message')!r}")
    r.dump("t22-server-log.txt", "\n".join(line for line in m6h.this_run(srv.log_text()).splitlines() if "heartbeat" not in line))
    r.dump("t22-u1-collab-log.txt", "\n".join(m6h.lines_with(u1.err_text(), "[collab]")))
    r.dump("t22-u2-collab-log.txt", "\n".join(m6h.lines_with(u2.err_text(), "[collab]")))
finally:
    m6h.cleanup()
    ok = r.summary()
sys.exit(0 if ok else 1)
