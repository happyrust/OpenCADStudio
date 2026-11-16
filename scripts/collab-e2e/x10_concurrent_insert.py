# -*- coding: utf-8 -*-
"""VAL-CROSS-010：两个实例并发插入，位号的唯一性由服务器保证。

u1（3210）与 u2（3211）各开一份种子图，连同一个服务器。两个线程同时向两个
GUI 发 PIDINSERT VALVE（间隔小于 1 秒）；两边本地都算出同一个位号。

证据：logs/x10-*.json（两条 op 与两条 op_applied 的帧原文、两边的位号集合、
audit_log、conflicts）、logs/gui-x10-*.err.log、shots/x10-u1.png、x10-u2.png。
"""
import json
import sys
import threading
import time

import m7h

R = m7h.Results("x10_concurrent_insert")


def valve_tags(gui):
    out = []
    for e in gui.inserts():
        a = gui.author(e.get("handle"))
        if a.get("type") == "VALVE":
            out.append(a.get("tag"))
    return sorted(out)


try:
    if not m7h.SEED_DWG.exists():
        R.check("setup", False, f"种子图不存在：{m7h.SEED_DWG}")
        raise SystemExit(1)
    dwg_a = m7h.fresh_copy("x10-PID-1101-a.dwg", m7h.SEED_DWG)
    dwg_b = m7h.fresh_copy("x10-PID-1101-b.dwg", m7h.SEED_DWG)
    srv = m7h.Server(3240, tag="x10-srv").start(fresh=True)
    R.check("setup", srv.wait_health(), f"服务器 /health = {srv.health()}")
    u1 = m7h.Gui("x10-u1", 3210, dwg=dwg_a, user="u1").start()
    u2 = m7h.Gui("x10-u2", 3211, dwg=dwg_b, user="u2").start()
    R.check("setup", u1.wait_ready(180) is not None and u2.wait_ready(180) is not None, "两个 GUI 就绪")
    s1 = u1.wait_status(lambda s: s.get("connected") and s.get("session_id"), 30)
    s2 = u2.wait_status(lambda s: s.get("connected") and s.get("session_id"), 30)
    R.check("setup", s1 is not None and s2 is not None, "u1、u2 均已连接")
    doc1, doc2 = u1.doc_id(), u2.doc_id()
    for g in (u1, u2):
        g.run("ZOOM EXTENTS")
    R.check("setup", valve_tags(u1) == [] and valve_tags(u2) == [], "开始时两边都没有阀门")

    # ── 同时各插一个 VALVE ───────────────────────────────────────────────
    results, stamps = {}, {}
    gate = threading.Barrier(2)

    def insert(name, gui, doc, x):
        gate.wait()
        stamps[name] = time.time()
        results[name] = gui.run(f"PIDINSERT VALVE {x},200", doc)

    threads = [threading.Thread(target=insert, args=("u1", u1, doc1, 100)),
               threading.Thread(target=insert, args=("u2", u2, doc2, 300))]
    for t in threads:
        t.start()
    for t in threads:
        t.join()
    gap = abs(stamps["u1"] - stamps["u2"])
    R.dump("x10-insert-responses.json", {k: {"http_status": v[0], "body": v[1]} for k, v in results.items()})
    R.check("VAL-CROSS-010", all(v[0] == 200 and (v[1] or {}).get("ok") is True for v in results.values()) and gap < 1.0,
            f"两个 GUI 各插入一个 VALVE，发出的间隔 {gap * 1000:.0f} 毫秒: "
            f"{[(k, v[0], (v[1] or {}).get('ok')) for k, v in results.items()]}")

    for g in (u1, u2):
        g.wait_status(lambda s: s.get("base_revision") == 2, 15)
    m7h.wait_until(lambda: len(valve_tags(u1)) == 2 and len(valve_tags(u2)) == 2
                   and valve_tags(u1) == valve_tags(u2) and len(set(valve_tags(u1))) == 2, 15)

    sent1, sent2 = m7h.frames(u1.err_text(), "->", "op"), m7h.frames(u2.err_text(), "->", "op")
    app1, app2 = m7h.frames(u1.err_text(), "<-", "op_applied"), m7h.frames(u2.err_text(), "<-", "op_applied")
    R.dump("x10-op-frames.json", {"u1_sent": sent1, "u2_sent": sent2, "u1_applied": app1, "u2_applied": app2})
    local1 = [f["payload"].get("tag") for f in sent1 if f.get("payload", {}).get("kind") == "insert_symbol"]
    local2 = [f["payload"].get("tag") for f in sent2 if f.get("payload", {}).get("kind") == "insert_symbol"]
    R.check("VAL-CROSS-010", len(local1) == 1 and local1 == local2 and bool(local1[0]),
            f"两边本地都算出同一个位号，两条 insert_symbol op 都带着它: u1 {local1}，u2 {local2}")
    changes = [c for f in app1 for c in f.get("changes") or [] if c.get("kind") == "insert_symbol"]
    taken = sorted(c.get("object_ref") for c in changes)
    renumbered = [c for c in changes if c.get("reassigned_from")]
    R.note("两条 op_applied 里的 insert_symbol: " + json.dumps(changes, ensure_ascii=False))
    R.check("VAL-CROSS-010", [f.get("revision") for f in app1] == [1, 2] and [f.get("revision") for f in app2] == [1, 2]
            and len(changes) == 2 and len(set(taken)) == 2 and local1[0] in taken,
            f"服务器让两条都生效（修订 1、2），生效的位号是 {taken}")
    R.check("VAL-CROSS-010", len(renumbered) == 1 and renumbered[0].get("reassigned_from") == local1[0]
            and renumbered[0].get("object_ref") != local1[0],
            f"只有一条以 {local1[0]} 生效；另一条被改派: {renumbered[0].get('reassigned_from') if renumbered else None} -> "
            f"{renumbered[0].get('object_ref') if renumbered else None}")

    t1, t2 = valve_tags(u1), valve_tags(u2)
    R.dump("x10-tags.json", {"u1": t1, "u2": t2})
    for name, g in (("u1", u1), ("u2", u2)):
        st, ents = g.api("GET", "/entities?xdata_app=PID_AUTHOR")
        R.dump(f"x10-{name}-entities.json", {"http_status": st, "body": ents})
    R.check("VAL-CROSS-010", t1 == t2 == taken and len(set(t1)) == 2,
            f"两实例最终读回的位号集合: u1 {t1}，u2 {t2}（无重复，与服务器生效的一致）")
    st1, st2 = u1.status(), u2.status()
    R.check("VAL-CROSS-010", not st1.get("diverged") and not st2.get("diverged")
            and not st1.get("last_apply_error") and not st2.get("last_apply_error"),
            f"两边都没有与房间不一致: diverged={st1.get('diverged')}/{st2.get('diverged')}，"
            f"last_apply_error={st1.get('last_apply_error')}/{st2.get('last_apply_error')}")

    st, objects = srv.http("GET", "/drawing/PID-1101/objects")
    R.dump("x10-server-objects.json", objects)
    names = sorted(i.get("object_ref") for i in (objects or {}).get("items") or [])
    R.check("VAL-CROSS-010", names == taken, f"服务器侧的对象: {names}（位号唯一）")
    conflicts = srv.sql("select conflict_id, object_ref, field, status from conflicts")
    R.check("VAL-CROSS-010", conflicts == [], f"conflicts 表没有重复位号条目（{len(conflicts)} 行）：唯一性由分配保证")
    rows = srv.sql("select seq, action, object_ref, user_id, change_text from audit_log where action='insert_symbol' order by seq")
    R.dump("x10-audit.json", rows)
    R.check("VAL-CROSS-010", len(rows) == 2 and sorted(r["object_ref"] for r in rows) == taken
            and sorted(r["user_id"] for r in rows) == ["u1", "u2"],
            f"audit_log 两条插入记录: {json.dumps(rows, ensure_ascii=False)}")
    time.sleep(0.5)
    for name, g in (("u1", u1), ("u2", u2)):
        st, resp, shot = g.capture(m7h.SHOTS / f"x10-{name}.png")
        R.check("VAL-CROSS-010", st == 200 and shot, f"{name} 截图 shots/x10-{name}.png")
except SystemExit:
    raise
except Exception as error:  # noqa: BLE001
    import traceback
    R.check("script", False, f"脚本异常: {error!r}\n{traceback.format_exc()}")
finally:
    m7h.cleanup()
    ok = R.summary()
sys.exit(0 if ok else 1)
