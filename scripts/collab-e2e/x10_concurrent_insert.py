# -*- coding: utf-8 -*-
"""VAL-CROSS-010：两个实例并发插入，位号的唯一性由服务器保证。

u1（3210）与 u2（3211）各开一份种子图，连同一个服务器。两个线程同时向两个
GUI 发 PIDINSERT VALVE（间隔小于 1 秒）；两边本地都算出同一个位号。

之后是「加入时上报图里已有的位号」一段（标签 T2.4-existing-tags、
T2.4-claim-after-rejoin）：两边的 hello 都带着种子图里原有的位号；第三方 u4
（3212）不带图启动、会话先连上，再打开一份离线画好、带着一个服务器没见过的
位号的图，第一次使用（一次占用）引出上报与重新加入；然后 u1 再放一个阀门，
本地算出的正是那个位号，服务器改派到下一个。

证据：logs/x10-*.json（两条 op 与两条 op_applied 的帧原文、两边的位号集合、
audit_log、conflicts；hello 帧、占用与会话的记录、第三次插入的帧与三方的位号）、
logs/gui-x10-*.err.log、shots/x10-u1.png、x10-u2.png。
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

    # ── 加入时上报图里已有的位号 ────────────────────────────────────────────
    # 种子图里原有一个 P-1001。两边加入时的 hello 都带着它；服务器登记下来，
    # 不算对象，回读得到。
    hello1, hello2 = m7h.frames(u1.err_text(), "->", "hello"), m7h.frames(u2.err_text(), "->", "hello")
    R.dump("x10-hello-frames.json", {"u1": hello1, "u2": hello2})
    said1, said2 = [f.get("existing_tags") for f in hello1], [f.get("existing_tags") for f in hello2]
    st, objects = srv.http("GET", "/drawing/PID-1101/objects")
    st_one, one = srv.http("GET", "/drawing/PID-1101/objects?object_ref=P-1001")
    R.check("T2.4-existing-tags", said1 == [["P-1001"]] and said2 == [["P-1001"]]
            and (objects or {}).get("reported_tags") == ["P-1001"] and (objects or {}).get("total") == 2
            and st_one == 200 and (one or {}).get("reported_tags") == ["P-1001"] and (one or {}).get("items") == [],
            f"两边加入时的 hello 都带着图里原有的位号: u1 {said1}，u2 {said2}；服务器回读 "
            f"reported_tags={(objects or {}).get('reported_tags')}、total={(objects or {}).get('total')}（上报的位号不算对象）；"
            f"按位号查 P-1001 -> {st_one}，reported_tags={(one or {}).get('reported_tags')}、items={(one or {}).get('items')}")
    logged = [line.split(" at=")[0].split("入站 hello ")[-1]
              for line in m7h.lines_with(srv.log_text(), "入站 hello", "上报本机已有位号")]
    rows = srv.sql("select drawing_id, tag, revision from reported_tags order by tag")
    R.check("T2.4-existing-tags", sorted(line.split()[0] for line in logged) == ["user=u1", "user=u2"]
            and sorted(line.split("-> ")[-1] for line in logged) == ["新登记 0 个（图面修订 0）", "新登记 1 个（图面修订 0）"]
            and rows == [{"drawing_id": "PID-1101", "tag": "P-1001", "revision": 0}],
            f"服务器对两次上报各记一行，先到的登记、后到的不重复登记: {logged}；"
            f"reported_tags 表 {json.dumps(rows, ensure_ascii=False)}")

    # 第三份图：不连服务器，在种子图上放一个阀门，位号改成 u1 下一次会在本地算出的
    # 那一个——服务器没见过它。
    numbers = sorted(int(tag.rsplit("-", 1)[1]) for tag in taken)
    clash, moved = f"V-{numbers[-1] + 1}", f"V-{numbers[-1] + 2}"
    dwg_c = m7h.fresh_copy("x10-PID-1101-c.dwg", m7h.SEED_DWG)
    offline = m7h.Gui("x10-offline", 3215, dwg=dwg_c, collab=False).start()
    R.check("setup", offline.wait_ready(180) is not None, "离线实例就绪（不连服务器）")
    doc0 = offline.doc_id()
    placed = offline.run("PIDINSERT VALVE 500,200", doc0)
    first = valve_tags(offline)
    renamed = offline.run(f"PIDTAG {first[0]} {clash}", doc0) if first else (0, None)
    st, saved = offline.api("POST", "/save", {"document_id": doc0})
    R.check("setup", placed[0] == 200 and (placed[1] or {}).get("ok") is True
            and renamed[0] == 200 and (renamed[1] or {}).get("ok") is True
            and valve_tags(offline) == [clash] and st == 200 and (saved or {}).get("ok") is True,
            f"离线在第三份图上放一个阀门（{first}）、位号改成 {clash} 并保存: 图里的阀门 {valve_tags(offline)}，"
            f"POST /save -> {st} ok={(saved or {}).get('ok')}")
    offline.kill()
    time.sleep(0.5)

    # u4 不带图启动：文件参数只为让界面带着 REST 通道起来，--new 让它不打开文件。
    # 会话先于图纸连上，这时没有位号可报。
    u4 = m7h.Gui("x10-u4", 3212, dwg=dwg_c, user="u4", extra=("--new",)).start()
    R.check("setup", u4.wait_ready(180) is not None, "u4 GUI 就绪（不带图启动）")
    s4 = u4.wait_status(lambda s: s.get("connected") and s.get("session_id"), 40) or {}
    before = s4.get("session_id")
    hello4 = m7h.frames(u4.err_text(), "->", "hello")
    st, objects = srv.http("GET", "/drawing/PID-1101/objects")
    R.check("T2.4-existing-tags", bool(before) and s4.get("document_id") is None
            and len(hello4) == 1 and not hello4[0].get("existing_tags")
            and (objects or {}).get("reported_tags") == ["P-1001"],
            f"还没有图的会话先加入: 已连接={bool(before)}，会话的图 document_id={s4.get('document_id')}，"
            f"hello {len(hello4)} 条、existing_tags={[f.get('existing_tags') for f in hello4]}；"
            f"服务器登记的仍是 {(objects or {}).get('reported_tags')}")
    opened = u4.api("POST", "/documents", {"path": str(dwg_c)})
    m7h.wait_until(lambda: sorted(u4.tagged()) == ["P-1001", clash], 20)
    doc4 = u4.doc_id()
    R.check("setup", opened[0] in (200, 201) and sorted(u4.tagged()) == ["P-1001", clash],
            f"u4 打开第三份图: POST /documents -> {opened[0]}，图里的位号 {sorted(u4.tagged())}")

    # 会话第一次被使用是一次占用：界面这才说出图里的位号，传输层离开会话、重新加入，
    # 随 hello 上报。
    claimed = u4.run("PIDCLAIM P-1001", doc4)
    s4 = u4.wait_status(lambda s: s.get("connected") and s.get("session_id") and s.get("session_id") != before, 30) or {}
    after = s4.get("session_id")
    hello4 = m7h.frames(u4.err_text(), "->", "hello")
    R.dump("x10-u4-hello-frames.json", hello4)
    told = m7h.lines_with(u4.err_text(), "断开后重新加入，随 hello 上报")
    logged = [line.split(" at=")[0].split("入站 hello ")[-1]
              for line in m7h.lines_with(srv.log_text(), "入站 hello", "user=u4", "上报本机已有位号")]
    st, objects = srv.http("GET", "/drawing/PID-1101/objects")
    st_one, one = srv.http("GET", f"/drawing/PID-1101/objects?object_ref={clash}")
    R.check("T2.4-existing-tags", claimed[0] == 200 and bool(after) and len(told) == 1
            and len(hello4) == 2 and hello4[1].get("existing_tags") == ["P-1001", clash] and hello4[1].get("since") == 2
            and len(logged) == 1 and logged[0].endswith("上报本机已有位号 2 个 -> 新登记 1 个（图面修订 2）")
            and (objects or {}).get("reported_tags") == ["P-1001", clash] and (objects or {}).get("total") == 2
            and st_one == 200 and (one or {}).get("reported_tags") == [clash] and (one or {}).get("items") == [],
            f"会话有了图之后重新加入去上报: 换了会话={bool(after)}，客户端日志「断开后重新加入，随 hello 上报」{len(told)} 行；"
            f"第二条 hello 的 existing_tags={[f.get('existing_tags') for f in hello4[1:]]}、since={[f.get('since') for f in hello4[1:]]}；"
            f"服务器: {logged}，reported_tags={(objects or {}).get('reported_tags')}、total={(objects or {}).get('total')}；"
            f"按位号查 {clash} -> {st_one}，reported_tags={(one or {}).get('reported_tags')}、items={(one or {}).get('items')}")

    # 那次占用是向离开的会话请求的。离开不是这个人造成的，所以新会话把它要回来：
    # 要得回来，也释放得掉。
    def whose(session_id):
        return "离开的会话" if session_id == before else "新会话" if session_id == after else "别的会话"

    m7h.wait_until(lambda: [r["session_id"] for r in srv.sql("select session_id from claims where tag='P-1001'")] == [after]
                   and (u4.status().get("claims") or {}).get("P-1001") == "u4", 15)
    asked = [whose(f.get("session_id")) for f in m7h.frames(u4.err_text(), "->", "claim")]
    answered = [f.get("type") for f in m7h.frames(u4.err_text(), "<-") if str(f.get("type")).startswith("claim_")]
    claims = srv.sql("select tag, drawing_id, session_id, user_id from claims")
    sessions = srv.sql("select session_id, status from sessions where user_id='u4' order by created_at")
    R.dump("x10-u4-claims.json", {"claims": claims, "sessions": sessions, "status": u4.status(),
                                  "claim_frames_sent": m7h.frames(u4.err_text(), "->", "claim")})
    R.check("T2.4-claim-after-rejoin", asked == ["离开的会话", "新会话"],
            f"占用请求向离开的会话发过一次，重新加入后向新会话再发一次、只一次: {asked}")
    held = (u4.status().get("claims") or {})
    R.check("T2.4-claim-after-rejoin", [(r["tag"], r["user_id"], whose(r["session_id"])) for r in claims] == [("P-1001", "u4", "新会话")]
            and held == {"P-1001": "u4"} and answered[-1:] == ["claim_granted"]
            and [(whose(r["session_id"]), r["status"] == "活跃") for r in sessions] == [("离开的会话", False), ("新会话", True)],
            f"重新加入后占用还在，记在新会话名下: claims 表 {[(r['tag'], r['user_id'], whose(r['session_id'])) for r in claims]}，"
            f"u4 的状态接口 claims={json.dumps(held, ensure_ascii=False)}，收到的占用应答 {answered}；"
            f"sessions 表 {[(whose(r['session_id']), r['status']) for r in sessions]}（离开的会话已结束）")
    released = u4.run("PIDRELEASE P-1001", doc4)
    u4.wait_status(lambda s: not s.get("claims"), 10)
    left = srv.sql("select tag, user_id from claims")
    R.check("T2.4-claim-after-rejoin", released[0] == 200 and not u4.status().get("claims") and left == [],
            f"u4 释放得掉这个占用: PIDRELEASE -> {released[0]}，状态接口 claims={json.dumps(u4.status().get('claims'), ensure_ascii=False)}，"
            f"claims 表 {json.dumps(left, ensure_ascii=False)}；最近一条提示: {u4.status().get('last_claim_message')}")

    # u1 再放一个阀门：本地算出的正是第三份图里原有的那个位号，服务器改派到下一个。
    resp = u1.run("PIDINSERT VALVE 100,400", doc1)
    for g in (u1, u2, u4):
        g.wait_status(lambda s: s.get("base_revision") == 3, 15)
    m7h.wait_until(lambda: all(moved in valve_tags(g) for g in (u1, u2, u4)), 15)
    sent = [f["payload"].get("tag") for f in m7h.frames(u1.err_text(), "->", "op") if f.get("payload", {}).get("kind") == "insert_symbol"]
    third = [f for f in m7h.frames(u1.err_text(), "<-", "op_applied") if f.get("revision") == 3]
    change = ((third[0].get("changes") or [{}])[0]) if third else {}
    t1, t2, t4 = valve_tags(u1), valve_tags(u2), valve_tags(u4)
    R.dump("x10-third-insert.json", {"u1_sent_tags": sent, "op_applied": third, "u1": t1, "u2": t2, "u4": t4})
    R.check("T2.4-existing-tags", resp[0] == 200 and (resp[1] or {}).get("ok") is True and sent[-1:] == [clash]
            and len(third) == 1 and change.get("kind") == "insert_symbol"
            and change.get("reassigned_from") == clash and change.get("object_ref") == moved,
            f"u1 再放一个阀门，本地算出的是 {sent[-1:]}（第三份图里原有的位号）；服务器没有让它以这个位号生效，改派: "
            f"{change.get('reassigned_from')} -> {change.get('object_ref')}（修订 {[f.get('revision') for f in third]}）")
    R.check("T2.4-existing-tags", t1 == t2 == sorted(taken + [moved]) and t4 == [clash, moved],
            f"三方读回的阀门位号: u1 {t1}，u2 {t2}，u4 {t4}（u4 的图里原有的 {clash} 还是它自己的，谁的图里都没有重号）")
    ends = [g.status() for g in (u1, u2, u4)]
    st, objects = srv.http("GET", "/drawing/PID-1101/objects")
    R.dump("x10-server-objects-after.json", objects)
    names = sorted(i.get("object_ref") for i in (objects or {}).get("items") or [])
    R.check("T2.4-existing-tags", not any(s.get("diverged") or s.get("last_apply_error") for s in ends)
            and names == sorted(taken + [moved]) and (objects or {}).get("reported_tags") == ["P-1001", clash],
            f"三方都没有与房间不一致: diverged={[s.get('diverged') for s in ends]}，last_apply_error={[s.get('last_apply_error') for s in ends]}；"
            f"服务器侧的对象 {names}，登记的已有位号 {(objects or {}).get('reported_tags')}")
except SystemExit:
    raise
except Exception as error:  # noqa: BLE001
    import traceback
    R.check("script", False, f"脚本异常: {error!r}\n{traceback.format_exc()}")
finally:
    m7h.cleanup()
    ok = R.summary()
sys.exit(0 if ok else 1)
