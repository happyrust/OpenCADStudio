# -*- coding: utf-8 -*-
"""VAL-CROSS-001：设计师从打开到保存的完整协同旅程。

u1 的 GUI（3210）连服务器打开一份图；PIDINSERT VALVE 100,200 插入阀门，位号
自动生成；PIDSETPROP 公称直径 DN50 设属性；POST /save 带 path 与 document_id；
另起一个实例重开保存的文件读回；查 audit_log 与会话状态。

证据：logs/x01-*.json、logs/gui-x01-*.err.log（含帧原文）、
logs/x01-srv.out.log、shots/x01-u1.png。
"""
import json
import re
import sys
import time

import m7h

R = m7h.Results("x01_journey")


def valves(gui):
    """{tag: (handle, author)} of every symbol whose author type is VALVE."""
    out = {}
    for e in gui.inserts():
        a = gui.author(e.get("handle"))
        if a.get("type") == "VALVE" and a.get("tag"):
            out[a["tag"]] = (e.get("handle"), a)
    return out


try:
    if not m7h.SEED_DWG.exists():
        R.check("setup", False, f"种子图不存在：{m7h.SEED_DWG}")
        raise SystemExit(1)
    dwg = m7h.fresh_copy("x01-PID-1101.dwg", m7h.SEED_DWG)
    saved = m7h.WORK / "x01-PID-1101-saved.dwg"
    for stale in list(m7h.WORK.glob("x01-PID-1101-saved.dwg*")) + list(m7h.WORK.glob(".x01-PID-1101-saved.dwg*")):
        stale.unlink()
    srv = m7h.Server(3240, tag="x01-srv").start(fresh=True)
    R.check("setup", srv.wait_health(), f"服务器 /health = {srv.health()}")

    u1 = m7h.Gui("x01-u1", 3210, dwg=dwg, user="u1").start()
    R.check("setup", u1.wait_ready(180) is not None, "u1 GUI 就绪")
    s1 = u1.wait_status(lambda s: s.get("connected") and s.get("session_id"), 30)
    R.check("VAL-CROSS-001", s1 is not None, f"u1 连上服务器并打开图纸: session={s1 and s1.get('session_id')}")
    sid1 = (s1 or {}).get("session_id")
    doc = u1.doc_id()
    u1.run("ZOOM EXTENTS")

    # ── 插入阀门，位号自动生成 ───────────────────────────────────────────
    st, ins = u1.run("PIDINSERT VALVE 100,200", doc)
    R.dump("x01-insert-response.json", {"http_status": st, "body": ins})
    R.check("VAL-CROSS-001", st == 200 and (ins or {}).get("ok") is True, f"PIDINSERT VALVE 100,200 -> {st} ok={(ins or {}).get('ok')}")
    vs = valves(u1)
    tag = next(iter(vs), None)
    handle, author = vs.get(tag, (None, {}))
    R.check("VAL-CROSS-001", len(vs) == 1 and bool(tag) and re.fullmatch(r"V-\d+", tag or "") is not None
            and author.get("tag_source") == "auto",
            f"位号自动生成: tag={tag!r}（形如 V-1001），tag_source={author.get('tag_source')!r}，句柄 {handle}")
    u1.wait_status(lambda s: s.get("base_revision") == 1, 10)

    # ── 设属性 ───────────────────────────────────────────────────────────
    st, setp = u1.run("PIDSETPROP 公称直径 DN50", doc)
    R.dump("x01-setprop-response.json", {"http_status": st, "body": setp})
    R.check("VAL-CROSS-001", st == 200 and (setp or {}).get("ok") is True and "DN50" in (u1.author(handle).get("props") or ""),
            f"PIDSETPROP 公称直径 DN50 -> {st} ok={(setp or {}).get('ok')}，props={u1.author(handle).get('props')!r}")
    s1b = u1.wait_status(lambda s: s.get("base_revision") == 2, 10) or u1.status()
    sent = m7h.frames(u1.err_text(), "->", "op")
    applied = m7h.frames(u1.err_text(), "<-", "op_applied")
    R.dump("x01-op-frames.json", {"sent": sent, "applied": applied})
    kinds = [f.get("payload", {}).get("kind") for f in sent]
    R.check("VAL-CROSS-001", kinds == ["insert_symbol", "set_property"] and [f.get("revision") for f in applied] == [1, 2],
            f"u1 发出的 op: {kinds}；收到的 op_applied 修订 {[f.get('revision') for f in applied]}")
    time.sleep(0.5)
    st, resp, shot = u1.capture(m7h.SHOTS / "x01-u1.png")
    R.check("VAL-CROSS-001", st == 200 and shot, "u1 截图 shots/x01-u1.png")

    # ── 保存 ─────────────────────────────────────────────────────────────
    st, sv = u1.api("POST", "/save", {"document_id": doc, "path": str(saved)})
    R.dump("x01-save-response.json", {"http_status": st, "body": sv})
    R.check("VAL-CROSS-001", st == 200 and (sv or {}).get("ok") is True and saved.exists(),
            f"POST /api/v1/save（带 path 与 document_id）-> {st} ok={(sv or {}).get('ok')}，文件 {saved.stat().st_size if saved.exists() else 0} 字节")

    # ── 重新打开该文件（同一个实例里关掉再打开，新文档 id）──────────────
    st, closed = u1.api("DELETE", f"/documents/{doc}")
    R.dump("x01-close-response.json", {"http_status": st, "body": closed})
    st, opened = u1.api("POST", "/documents", {"path": str(saved)})
    R.dump("x01-reopen-response.json", {"http_status": st, "body": opened})
    m7h.wait_until(lambda: u1.doc_id() not in (None, doc) and valves(u1), 30)
    doc2 = u1.doc_id()
    st, ents = u1.api("GET", "/entities?xdata_app=PID_AUTHOR")
    R.dump("x01-reopen-entities.json", {"http_status": st, "body": ents})
    R.check("VAL-CROSS-001", st == 200 and doc2 is not None and doc2 != doc,
            f"重新打开该文件: GET /entities?xdata_app=PID_AUTHOR -> {st}，新文档 id {doc2}（保存时是 {doc}）")
    vs2 = valves(u1)
    h2, a2 = vs2.get(tag, (None, {}))
    R.dump("x01-reopen-xdata.json", u1.xdata(h2)[1] if h2 else None)
    props2 = json.loads(a2.get("props") or "{}") if a2 else {}
    R.check("VAL-CROSS-001", a2.get("tag") == tag and props2.get("公称直径") == "DN50",
            f"重开后该阀门: tag={a2.get('tag')!r}，props={a2.get('props')!r}")

    # ── 审计与会话 ───────────────────────────────────────────────────────
    rows = srv.sql("select seq, action, object_ref, user_id, change_text from audit_log order by seq")
    R.dump("x01-audit.json", rows)
    R.note("audit_log: " + json.dumps(rows, ensure_ascii=False))
    inserted = [r for r in rows if r["action"] == "insert_symbol" and r["user_id"] == "u1" and r["object_ref"] == tag]
    setprop = [r for r in rows if r["action"] == "set_property" and r["user_id"] == "u1" and r["object_ref"] == tag]
    R.check("VAL-CROSS-001", len(inserted) == 1 and len(setprop) == 1,
            f"audit_log 有 insert_symbol 与 set_property（user_id=u1，object_ref={tag}）: {len(inserted)} 行 / {len(setprop)} 行")
    sess = srv.sql("select session_id, user_id, status from sessions")
    R.dump("x01-sessions.json", sess)
    s1c = u1.status()
    reconnects = m7h.lines_with(u1.err_text(), "断开")
    R.check("VAL-CROSS-001", len(sess) == 1 and sess[0]["session_id"] == sid1 and sess[0]["status"] == "活跃"
            and s1c.get("connected") is True and s1c.get("session_id") == sid1 and not reconnects,
            f"协同会话全程活跃: sessions={json.dumps(sess, ensure_ascii=False)}，客户端仍是会话 {s1c.get('session_id')}，断开记录 {len(reconnects)} 条")
except SystemExit:
    raise
except Exception as error:  # noqa: BLE001
    import traceback
    R.check("script", False, f"脚本异常: {error!r}\n{traceback.format_exc()}")
finally:
    m7h.cleanup()
    ok = R.summary()
sys.exit(0 if ok else 1)
