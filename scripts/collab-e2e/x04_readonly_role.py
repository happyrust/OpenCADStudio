# -*- coding: utf-8 -*-
"""VAL-CROSS-004 与 VAL-CROSS-011：没有编辑权限的角色。

u1（工艺设计人员，3210）与 u3（校对人员，3212）各开一份种子图（一个容器
P-1001，两份图里句柄相同），连同一个服务器（3240）。两个 GUI 都起在隐藏
桌面上，只走 --http 的本机 REST 通道；场景前后各记一次指针位置与前台窗口。

004：u3 连上即被降级（readonly_downgrade，角色级）；面板画出只读标识；
     写命令 403、ok=false、code=PERM_DENIED_ROLE、level=role；audit_log 有
     一行拒绝；读照常 200。
011：u1 改 P-1001 的属性并广播；u3 收到 op_applied、读回新值；u3 的属性
     面板显示新值、每一行都是只读值；u3 的写入被拒并留痕；audit_log 里
     u1 的成功行与 u3 的被拒行 object_ref 相同。

证据：logs/x04-*.json、logs/gui-x04-*.err.log（GUI 的协同日志与帧原文）、
logs/x04-srv.out.log、shots/cross-u3-readonly.png、shots/cross-u3-panel.png。
"""
import json
import sys
import time

import m7h

R = m7h.Results("x04_readonly_role")
FIELD = "设计压力"
NEW_VALUE = "2.5MPa"


def sections(gui):
    """属性面板的各组（控制通道的 properties 操作）。"""
    status, body = gui.op("properties")
    body = body or {}
    return status, body.get("sections") or (body.get("result") or {}).get("sections") or []


def row_by_label(secs, label):
    for sec in secs:
        for row in sec.get("properties") or []:
            if row.get("label") == label:
                return sec.get("title"), row
    return None, None


def code_of(body):
    body = body or {}
    error = body.get("error")
    return body.get("code") or (error.get("code") if isinstance(error, dict) else None)


srv = None
try:
    if not m7h.SEED_DWG.exists():
        R.check("setup", False, f"种子图不存在：{m7h.SEED_DWG}")
        raise SystemExit(1)
    dwg_a = m7h.fresh_copy("x04-PID-1101-a.dwg", m7h.SEED_DWG)
    dwg_c = m7h.fresh_copy("x04-PID-1101-c.dwg", m7h.SEED_DWG)

    srv = m7h.Server(3240, tag="x04-srv").start(fresh=True)
    R.check("setup", srv.wait_health(), f"服务器 /health = {srv.health()}")
    audit_before = srv.sql("select count(*) n from audit_log")[0]["n"]

    u1 = m7h.Gui("x04-u1", 3210, dwg=dwg_a, user="u1").start()
    R.check("setup", u1.wait_ready(180) is not None, "u1 GUI 就绪")
    s1 = u1.wait_status(lambda s: s.get("connected") and s.get("session_id"), 30)
    R.check("setup", s1 is not None, f"u1 已连接: session={s1 and s1.get('session_id')}")

    u3 = m7h.Gui("x04-u3", 3212, dwg=dwg_c, user="u3").start()
    R.check("setup", u3.wait_ready(180) is not None, "u3 GUI 就绪")
    s3 = u3.wait_status(lambda s: s.get("connected") and s.get("session_id") and s.get("readonly"), 30)
    R.check("setup", s3 is not None, f"u3 已连接并已只读: {json.dumps(u3.status(), ensure_ascii=False)[:400]}")
    s3 = s3 or u3.status()
    sid3 = s3.get("session_id")
    R.dump("x04-u3-status.json", s3)

    doc1, doc3 = u1.doc_id(), u3.doc_id()
    h1, h3 = u1.tagged().get("P-1001"), u3.tagged().get("P-1001")
    R.check("setup", bool(h1) and h1 == h3, f"两份图上 P-1001 的句柄: u1={h1} u3={h3}")
    for g in (u1, u3):
        g.run("ZOOM EXTENTS")
    time.sleep(1.0)

    # ── VAL-CROSS-004 ─────────────────────────────────────────────────────
    down3 = m7h.lines_with(u3.err_text(), "收到 readonly_downgrade")
    frames3 = m7h.frames(u3.err_text(), "<-", "readonly_downgrade")
    R.dump("x04-004-u3-readonly-downgrade-frames.json", frames3)
    srv_down = m7h.lines_with(srv.log_text(), "readonly_downgrade", "user=u3")
    R.note(f"u3 客户端日志: {down3}")
    R.note(f"服务器日志: {srv_down}")
    f3 = frames3[-1] if frames3 else {}
    R.check("VAL-CROSS-004", bool(frames3) and bool(f3.get("reason")) and bool(srv_down),
            f"u3 收到 readonly_downgrade，帧原文: {json.dumps(f3, ensure_ascii=False)}")
    R.check("VAL-CROSS-004", f3.get("level") == "role" and f3.get("code") == "PERM_DENIED_ROLE",
            f"帧里 level={f3.get('level')!r}、code={f3.get('code')!r}（角色级的权限拒绝）")
    R.check("VAL-CROSS-004", s3.get("readonly") is True and s3.get("readonly_level") == "role",
            f"u3 /collab/status: readonly={s3.get('readonly')} readonly_level={s3.get('readonly_level')}")
    bounds = u3.ui_bounds("pid-collab")
    R.dump("x04-u3-ui-bounds.json", bounds)
    R.check("VAL-CROSS-004", "pid-collab-readonly" in bounds,
            f"u3 的协同面板画出了只读标识（pid-collab-readonly = {bounds.get('pid-collab-readonly')}）")
    st, resp, shot = u3.capture(m7h.SHOTS / "cross-u3-readonly.png")
    R.check("VAL-CROSS-004", st == 200 and shot, "u3 截图 shots/cross-u3-readonly.png")

    st, run3 = u3.run("PIDTAG P-1001 P-1009", doc3)
    R.dump("x04-004-u3-run.json", {"http_status": st, "body": run3})
    R.check("VAL-CROSS-004", st == 403 and (run3 or {}).get("ok") is False and code_of(run3) == "PERM_DENIED_ROLE"
            and (run3 or {}).get("level") == "role",
            f"u3 POST /api/v1/run PIDTAG P-1001 P-1009 -> HTTP {st} ok={(run3 or {}).get('ok')} "
            f"code={code_of(run3)} level={(run3 or {}).get('level')}")
    R.check("VAL-CROSS-004", u3.author(h3).get("tag") == "P-1001", f"u3 的位号读回仍为 {u3.author(h3).get('tag')!r}")

    def denied_rows():
        return srv.sql("select seq, module, action, at, object_ref, user_id, change_text from audit_log "
                       "where user_id='u3' and action='write_denied' order by seq")
    m7h.wait_until(lambda: len(denied_rows()) >= 1, 10)
    rows = denied_rows()
    R.dump("x04-004-audit-u3.json", rows)
    first = rows[0] if rows else {}
    R.check("VAL-CROSS-004", bool(rows) and first.get("object_ref") == "P-1001" and "PIDTAG" in (first.get("change_text") or ""),
            f"audit_log 新增拒绝行: user_id={first.get('user_id')} action={first.get('action')} "
            f"object_ref={first.get('object_ref')} change_text={first.get('change_text')!r}")

    st, ents = u3.api("GET", "/entities")
    count = len((ents or {}).get("entities") or ((ents or {}).get("result") or {}).get("entities") or [])
    R.check("VAL-CROSS-004", st == 200 and (ents or {}).get("ok") is True and count > 0,
            f"u3 GET /api/v1/entities -> HTTP {st} ok={(ents or {}).get('ok')} 实体 {count} 个")

    # ── VAL-CROSS-011 ─────────────────────────────────────────────────────
    since = time.time()
    st, put1 = u1.api("PUT", f"/entities/{h1}/xdata/PID_AUTHOR",
                      [{"code": 1000, "value": 'props={"%s":"%s"}' % (FIELD, NEW_VALUE)}])
    R.dump("x04-011-u1-put.json", {"http_status": st, "body": put1})
    s1b = u1.wait_status(lambda s: s.get("base_revision") == 1, 15) or u1.status()
    applied1 = m7h.lines_with(u1.err_text(), "收到 op_applied")
    R.check("VAL-CROSS-011", st == 200 and (put1 or {}).get("ok") is True and bool(applied1) and s1b.get("base_revision") == 1,
            f"u1 改 P-1001 的 {FIELD} 为 {NEW_VALUE} 并广播成功: PUT -> {st}，{applied1[-1] if applied1 else None}")

    got3 = m7h.wait_until(lambda: FIELD in (u3.author(h3).get("props") or "") and NEW_VALUE in (u3.author(h3).get("props") or ""), 20)
    applied3 = m7h.lines_with(u3.err_text(), "收到 op_applied")
    frames_op3 = m7h.frames(u3.err_text(), "<-", "op_applied")
    R.dump("x04-011-u3-op-applied-frames.json", frames_op3)
    st, xd3 = u3.xdata(h3)
    R.dump("x04-011-u3-xdata.json", xd3)
    fo3 = frames_op3[-1] if frames_op3 else {}
    R.check("VAL-CROSS-011", bool(got3) and bool(applied3) and fo3.get("revision") == 1
            and any(c.get("object_ref") == "P-1001" and c.get("field") == FIELD and c.get("value") == NEW_VALUE
                    for c in fo3.get("changes") or []),
            f"u3 收到 op_applied，帧原文: {json.dumps(fo3, ensure_ascii=False)}；本地读回 props={u3.author(h3).get('props')!r}")
    s3b = u3.status()
    R.check("VAL-CROSS-011", s3b.get("readonly") is True and s3b.get("base_revision") == 1 and not s3b.get("last_apply_error"),
            f"u3 仍只读且基线跟上: readonly={s3b.get('readonly')} base_revision={s3b.get('base_revision')} "
            f"last_apply_error={s3b.get('last_apply_error')}")

    # 属性面板：选中 P-1001，读面板的各行。
    st, sel = u3.op("select", doc3, handles=[h3])
    R.note(f"u3 select {h3} -> {st} ok={(sel or {}).get('ok')}")
    st, secs = sections(u3)
    R.dump("x04-011-u3-properties.json", secs)
    title, row = row_by_label(secs, FIELD)
    R.check("VAL-CROSS-011", row is not None and row.get("value") == NEW_VALUE,
            f"u3 属性面板「{title}」组里 {FIELD} 一行显示新值: {json.dumps(row, ensure_ascii=False)}")
    kinds = sorted({(row.get("kind") or "?") for sec in secs for row in sec.get("properties") or []})
    total = sum(len(sec.get("properties") or []) for sec in secs)
    R.check("VAL-CROSS-011", total > 0 and kinds == ["readonly"],
            f"u3 属性面板 {len(secs)} 组 {total} 行，行的种类只有 {kinds}（没有输入控件）")
    # 对照：有编辑权的 u1 选中同一个对象，面板里是输入控件。
    u1.op("select", doc1, handles=[h1])
    st, secs1 = sections(u1)
    R.dump("x04-011-u1-properties.json", secs1)
    _, row1 = row_by_label(secs1, FIELD)
    kinds1 = sorted({(row.get("kind") or "?") for sec in secs1 for row in sec.get("properties") or []})
    R.check("VAL-CROSS-011", row1 is not None and row1.get("kind") == "text" and len(kinds1) > 1,
            f"对照：u1 的面板里 {FIELD} 一行是输入控件（kind={row1 and row1.get('kind')}），行的种类 {kinds1}")
    bounds = u3.ui_bounds("pid-collab")
    st, resp, shot = u3.capture(m7h.SHOTS / "cross-u3-panel.png")
    R.check("VAL-CROSS-011", st == 200 and shot and "pid-collab-readonly" in bounds,
            f"u3 截图 shots/cross-u3-panel.png（只读标识在：pid-collab-readonly = {bounds.get('pid-collab-readonly')}）")

    # u3 的写入尝试：与 u1 同一个写入通道、同一个对象。
    st, put3 = u3.api("PUT", f"/entities/{h3}/xdata/PID_AUTHOR",
                      [{"code": 1000, "value": 'props={"%s":"9.9MPa"}' % FIELD}])
    R.dump("x04-011-u3-put.json", {"http_status": st, "body": put3})
    R.check("VAL-CROSS-011", st == 403 and (put3 or {}).get("ok") is False and code_of(put3) == "PERM_DENIED_ROLE"
            and (put3 or {}).get("level") == "role",
            f"u3 PUT 同一属性 -> HTTP {st} ok={(put3 or {}).get('ok')} code={code_of(put3)} level={(put3 or {}).get('level')}")
    R.check("VAL-CROSS-011", NEW_VALUE in (u3.author(h3).get("props") or "") and "9.9MPa" not in (u3.author(h3).get("props") or ""),
            f"u3 的值没有变: props={u3.author(h3).get('props')!r}")
    m7h.wait_until(lambda: len(denied_rows()) >= 2, 10)
    rows = srv.sql("select seq, module, action, at, object_ref, user_id, change_text from audit_log "
                   "where at >= ? order by seq", (int(since) - 1,))
    R.dump("x04-011-audit.json", rows)
    ok_rows = [row for row in rows if row["user_id"] == "u1" and row["action"] != "write_denied"]
    no_rows = [row for row in rows if row["user_id"] == "u3" and row["action"] == "write_denied"]
    R.note(f"同一时间段的审计行: {json.dumps(rows, ensure_ascii=False)}")
    same = bool(ok_rows) and bool(no_rows) and ok_rows[-1]["object_ref"] == no_rows[-1]["object_ref"] == "P-1001"
    R.check("VAL-CROSS-011", same,
            f"u1 的成功行（action={ok_rows[-1]['action'] if ok_rows else None}）与 u3 的被拒行 object_ref 相同"
            f"（{ok_rows[-1]['object_ref'] if ok_rows else None} / {no_rows[-1]['object_ref'] if no_rows else None}）")
    text = no_rows[-1]["change_text"] if no_rows else ""
    R.check("VAL-CROSS-011", "只读" in text, f"u3 行的 change_text 含「只读」: {text!r}")
    R.note(f"audit_log 行数 {audit_before} -> {srv.sql('select count(*) n from audit_log')[0]['n']}")
except SystemExit:
    raise
except Exception as error:  # noqa: BLE001
    import traceback
    R.check("script", False, f"脚本异常: {error!r}\n{traceback.format_exc()}")
finally:
    m7h.cleanup()
    ok = R.summary()
sys.exit(0 if ok else 1)
