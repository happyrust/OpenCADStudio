# -*- coding: utf-8 -*-
"""VAL-CROSS-005：M3 的持久化数据穿越协同会话后完整。

基线图是 M3 往返测试的那份内容，由默认不跑的测试 write_the_m3_baseline_drawing
写出，放在证据目录的 m7/work/PID-1101-m3.dwg（契约写的那个路径上没有这个文件；
总入口在它不存在时先生成）。先不连服务器读基线；
u1、u2 连服务器各开一份，做三次协同编辑（改属性、插入阀门、改位号），都
收到 op_applied；u1 保存回它打开的那个路径；再不连服务器重开比对。

三次编辑之后、保存之前，同一个会话里再走一遍属性超过 2 KB 的对象（这类对象的属性不在
实体自己的记录里，另存一条记录）：写到超过、改其中一项、撤销、重做、隔着对方的改动撤销，
两端读回的属性逐项相同（标签 CC-2-large-props）；删掉再撤销，对方那里放回来的对象属性
是全的，保存重开之后还在（标签 CC-1-large-props）。只走了符号；管线、管路、管件的同一类
路径由单元测试 collab_large_props 覆盖。

证据：logs/x05-*.json、logs/cross-m3-m6.json（基线与保存后的逐项对比）、
logs/x05-large-props-frames.json（这一段两端发出的变更）、logs/gui-x05-*.err.log。
"""
import json
import sys
import time

import m7h

R = m7h.Results("x05_m3_persistence")
BASELINE = m7h.WORK / "PID-1101-m3.dwg"
KEYS = ("PID_AUTHOR_TYPE_LIB", "PID_AUTHOR_TAG_RULES", "PID_AUTHOR_TOPOLOGY")


def read_offline(name, port, dwg, tags=()):
    """{handle: author dict} of every entity carrying PID_AUTHOR, the graph
    XRecords, and what the drawing holds for each of `tags` (see `held`)."""
    g = m7h.Gui(name, port, dwg=dwg, collab=False).start()
    if g.wait_ready(180) is None:
        raise RuntimeError(f"{name} 没有就绪")
    st, body = g.api("GET", "/entities?xdata_app=PID_AUTHOR")
    authors = {}
    for e in (body or {}).get("entities") or ((body or {}).get("result") or {}).get("entities") or []:
        a = g.author(e.get("handle"))
        if a:
            authors[e.get("handle")] = a
    st, body = g.api("GET", "/records?collection=objects&limit=10000")
    records = {}
    for rec in (body or {}).get("records") or []:
        props = rec.get("properties") or {}
        if rec.get("type") != "XRecord" or props.get("name") not in KEYS:
            continue
        strings = [e["value"]["String"] for e in props.get("entries") or []
                   if isinstance(e.get("value"), dict) and "String" in e["value"]]
        data = [s for s in strings if s != "kind=graph_data" and not s.startswith("key=")]
        records[props["name"]] = {"handle": rec.get("handle"), "strings": data,
                                  "bytes": sum(len(s.encode("utf-8")) for s in data)}
    kept = {tag: held(g, tag) for tag in tags}
    g.kill()
    return authors, records, kept


def props_of(author):
    try:
        return json.loads(author.get("props") or "{}")
    except ValueError:
        return {}


def went(resp):
    return resp[0] == 200 and (resp[1] or {}).get("ok") is True


def held(gui, tag):
    """(props, where) of the object tagged `tag`, as `gui` holds it. Props that
    outgrew the entity's own record are kept in a record of their own, which
    the entity names: 'overflow' then, 'inline' otherwise; 'dangling' when the
    record it names is not there, 'absent' when the object is not."""
    handle = gui.tagged().get(tag)
    author = gui.author(handle) if handle else None
    if not author:
        return None, "absent"
    ref = author.get("props_ref")
    if not ref:
        return props_of(author), "inline"
    st, body = gui.api("GET", "/records?collection=objects&limit=10000")
    for rec in (body or {}).get("records") or []:
        props = rec.get("properties") or {}
        if rec.get("type") != "XRecord" or props.get("name") != ref:
            continue
        data = bytearray()
        for entry in props.get("entries") or []:
            value = entry.get("value") or {}
            if entry.get("code") == 310 and isinstance(value, dict) and "Chunk" in value:
                data.extend(value["Chunk"])
        return json.loads(data.decode("utf-8")), "overflow"
    return None, "dangling"


def brief(props):
    """Props for a line of the report: a long value is cut to how it starts and how long it is."""
    if props is None:
        return None
    return {k: (v if len(str(v)) <= 16 else f"{str(v)[:8]}…（{len(str(v).encode('utf-8'))} 字节）") for k, v in props.items()}


def server_notes(srv, tag):
    """The 备注… fields the server holds for `tag`; None when it holds no such object."""
    st, objects = srv.http("GET", "/drawing/PID-1101/objects")
    for item in (objects or {}).get("items") or []:
        if item.get("object_ref") == tag:
            return {k: v.get("value") for k, v in (item.get("fields") or {}).items() if k.startswith("备注")}
    return None


def last_sent(gui):
    """The payload of the op `gui` sent last."""
    sent = m7h.frames(gui.err_text(), "->", "op")
    return (sent[-1].get("payload") or {}) if sent else {}


try:
    if not BASELINE.exists():
        R.check("setup", False, f"M3 基线图不存在：{BASELINE}")
        raise SystemExit(1)
    dwg_a = m7h.fresh_copy("x05-PID-1101-a.dwg", BASELINE)
    dwg_b = m7h.fresh_copy("x05-PID-1101-b.dwg", BASELINE)

    # ── 基线：不连服务器 ─────────────────────────────────────────────────
    base_authors, base_records, _ = read_offline("x05-baseline", 3214, dwg_a)
    R.dump("x05-baseline.json", {"authors": base_authors, "records": base_records})
    R.check("VAL-CROSS-005", len(base_authors) >= 5 and set(base_records) == set(KEYS),
            f"基线: 带 PID_AUTHOR 的实体 {len(base_authors)} 个（位号 {sorted(a.get('tag') for a in base_authors.values())}），"
            f"三个 XRecord 的字节长度 {dict((k, v['bytes']) for k, v in base_records.items())}")

    srv = m7h.Server(3240, tag="x05-srv").start(fresh=True)
    R.check("setup", srv.wait_health(), f"服务器 /health = {srv.health()}")
    u1 = m7h.Gui("x05-u1", 3210, dwg=dwg_a, user="u1").start()
    u2 = m7h.Gui("x05-u2", 3211, dwg=dwg_b, user="u2").start()
    R.check("setup", u1.wait_ready(180) is not None and u2.wait_ready(180) is not None, "两个 GUI 就绪")
    s1 = u1.wait_status(lambda s: s.get("connected") and s.get("session_id"), 30)
    s2 = u2.wait_status(lambda s: s.get("connected") and s.get("session_id"), 30)
    R.check("setup", s1 is not None and s2 is not None, "u1、u2 均已连接")
    doc1, doc2 = u1.doc_id(), u2.doc_id()

    # ── 三次协同编辑 ─────────────────────────────────────────────────────
    st, e1 = u1.run("PIDSETPROP P-1001 设计压力 2.5MPa", doc1)
    R.check("VAL-CROSS-005", st == 200 and (e1 or {}).get("ok") is True, f"① u1 改属性 PIDSETPROP P-1001 设计压力 2.5MPa -> {st} ok={(e1 or {}).get('ok')}")
    for g in (u1, u2):
        g.wait_status(lambda s: s.get("base_revision") == 1, 15)
    st, e2 = u2.run("PIDINSERT VALVE 150,80", doc2)
    R.check("VAL-CROSS-005", st == 200 and (e2 or {}).get("ok") is True, f"② u2 插入阀门 PIDINSERT VALVE 150,80 -> {st} ok={(e2 or {}).get('ok')}")
    for g in (u1, u2):
        g.wait_status(lambda s: s.get("base_revision") == 2, 15)
    st, e3 = u1.run("PIDTAG TI-1001 TI-1009", doc1)
    R.check("VAL-CROSS-005", st == 200 and (e3 or {}).get("ok") is True, f"③ u1 改位号 PIDTAG TI-1001 TI-1009 -> {st} ok={(e3 or {}).get('ok')}")
    for g in (u1, u2):
        g.wait_status(lambda s: s.get("base_revision") == 3, 15)
    a1, a2 = m7h.frames(u1.err_text(), "<-", "op_applied"), m7h.frames(u2.err_text(), "<-", "op_applied")
    R.dump("x05-op-applied-frames.json", {"u1": a1, "u2": a2})
    kinds1 = [[c.get("kind") for c in f.get("changes") or []] for f in a1]
    kinds2 = [[c.get("kind") for c in f.get("changes") or []] for f in a2]
    R.check("VAL-CROSS-005", [f.get("revision") for f in a1] == [1, 2, 3] and [f.get("revision") for f in a2] == [1, 2, 3]
            and kinds1 == kinds2 == [["set_property"], ["insert_symbol"], ["set_tag"]],
            f"双方均收到三条 op_applied（修订 1、2、3）: {kinds1}")
    t1 = sorted(a.get("tag") for a in (u1.author(e.get("handle")) for e in u1.inserts()) if a.get("tag"))
    t2 = sorted(a.get("tag") for a in (u2.author(e.get("handle")) for e in u2.inserts()) if a.get("tag"))
    st1, st2 = u1.status(), u2.status()
    R.check("VAL-CROSS-005", t1 == t2 and not st1.get("diverged") and not st2.get("diverged"),
            f"三次编辑之后两边的位号集合相同: {t1}；diverged={st1.get('diverged')}/{st2.get('diverged')}")

    # ── 属性超过 2 KB 的对象 ─────────────────────────────────────────────
    # 第 ② 次编辑放的阀门由 u2 写；基线里的 P-1001 由 u1 写。每做一件事，两端的修订号加一。
    valve = ((a2[1].get("changes") or [{}])[0].get("object_ref") if len(a2) > 1 else None) or ""
    rev = 3
    sent1, sent2 = len(m7h.frames(u1.err_text(), "->", "op")), len(m7h.frames(u2.err_text(), "->", "op"))

    def settled(revision):
        return all(g.wait_status(lambda s: s.get("base_revision") == revision, 15) is not None for g in (u1, u2))

    def both_hold(tag, want, where):
        """Wait until both ends hold `want` for `tag`, kept `where`; what each holds, u1's then u2's."""
        m7h.wait_until(lambda: held(u1, tag) == (want, where) and held(u2, tag) == (want, where), 15)
        return held(u1, tag), held(u2, tag)

    def told(seen):
        return "；".join(f"{name} 读到（{where}）{json.dumps(brief(props), ensure_ascii=False)}"
                         for name, (props, where) in zip(("u1", "u2"), seen))

    def twice(props, where):
        return (props, where), (props, where)

    # 写到超过：九项，逐项写。
    notes = {f"备注{i:02d}": f"第{i:02d}条-" + "检修记录" * 20 for i in range(1, 10)}
    wrote = []
    for field, value in notes.items():
        wrote.append(went(u2.run(f"PIDSETPROP {valve} {field} {value}", doc2)))
        rev += 1
        settled(rev)
    seen = both_hold(valve, notes, "overflow")
    server = server_notes(srv, valve)
    size = sum(len(k.encode("utf-8")) + len(v.encode("utf-8")) for k, v in notes.items())
    R.check("CC-2-large-props", bool(valve) and all(wrote) and seen == twice(notes, "overflow") and server == notes,
            f"u2 给 {valve} 逐项写 9 项属性（键值合计 {size} 字节，过了 2 KB 之后写的几项也要到对方）：两端读回的 9 项逐项相同、"
            f"都另存在溢出记录里 —— {told(seen)}；服务器记着的 9 项与之相同: {server == notes}")
    said = [f.get("payload") or {} for f in m7h.frames(u2.err_text(), "->", "op")[sent2:]]
    R.check("CC-2-large-props", [(p.get("kind"), p.get("object_ref"), p.get("field")) for p in said]
            == [("set_property", valve, field) for field in notes],
            f"每写一项发出一条 set_property，共 {len(said)} 条，字段依次是 {[p.get('field') for p in said]}")

    # 改其中一项，撤销，重做。
    changed = {**notes, "备注01": "改过的值"}
    modified = u2.run(f"PIDSETPROP {valve} 备注01 改过的值", doc2)
    rev += 1
    settled(rev)
    seen = both_hold(valve, changed, "overflow")
    server = server_notes(srv, valve)
    sent = last_sent(u2)
    R.check("CC-2-large-props", went(modified) and seen == twice(changed, "overflow") and server == changed
            and (sent.get("kind"), sent.get("field"), sent.get("value"), sent.get("old_value"))
            == ("set_property", "备注01", "改过的值", notes["备注01"]),
            f"u2 改其中一项（备注01）：发出一条 {sent.get('kind')}，带着改前的值（{len(str(sent.get('old_value') or '').encode('utf-8'))} 字节）；"
            f"两端读回 —— {told(seen)}；服务器记着的 9 项与之相同: {server == changed}")
    undone = u2.op("undo", doc2)
    rev += 1
    settled(rev)
    seen_undone = both_hold(valve, notes, "overflow")
    redone = u2.op("redo", doc2)
    rev += 1
    settled(rev)
    seen_redone = both_hold(valve, changed, "overflow")
    R.check("CC-2-large-props", went(undone) and seen_undone == twice(notes, "overflow")
            and went(redone) and seen_redone == twice(changed, "overflow"),
            f"u2 撤销、再重做这一改，两端的 9 项每次都逐项相同：撤销后备注01 = "
            f"{[brief(p or {}).get('备注01') for p, _w in seen_undone]}，重做后 = {[(p or {}).get('备注01') for p, _w in seen_redone]}；"
            f"存放处 {[w for _p, w in seen_undone + seen_redone]}")

    # 隔着对方的改动撤销：u1 一步把 P-1001 写过 2 KB，然后两端各撤各的。
    before = held(u1, "P-1001"), held(u2, "P-1001")
    plain = before[0][0] or {}
    big = "一次写入-" + "设备说明" * 180
    grown = {**plain, "备注": big}
    wrote_b = u1.run(f"PIDSETPROP P-1001 备注 {big}", doc1)
    rev += 1
    settled(rev)
    seen_b = both_hold("P-1001", grown, "overflow")
    server = server_notes(srv, "P-1001")
    R.check("CC-2-large-props", bool(plain) and before == twice(plain, "inline") and went(wrote_b)
            and seen_b == twice(grown, "overflow") and server == {"备注": big},
            f"u1 给 P-1001 写一项 {len(big.encode('utf-8'))} 字节的属性（一步就过 2 KB）：写之前两端都是实体自己记录里的 {len(plain)} 项，"
            f"写之后 —— {told(seen_b)}；服务器记着这一项: {server == {'备注': big}}")
    crossed = u2.op("undo", doc2)
    rev += 1
    settled(rev)
    seen_a, seen_b = both_hold(valve, notes, "overflow"), both_hold("P-1001", grown, "overflow")
    R.check("CC-2-large-props", went(crossed) and seen_a == twice(notes, "overflow") and seen_b == twice(grown, "overflow"),
            f"u2 隔着 u1 的这次改动撤销自己那一改：{valve} 两端都回到改之前的 9 项（备注01 = "
            f"{[brief(p or {}).get('备注01') for p, _w in seen_a]}）；u1 写进 P-1001 的属性两端都还在、一项不少 —— {told(seen_b)}")
    back = u1.op("undo", doc1)
    rev += 1
    settled(rev)
    seen_b, seen_a = both_hold("P-1001", plain, "inline"), both_hold(valve, notes, "overflow")
    R.check("CC-2-large-props", went(back) and seen_b == twice(plain, "inline") and seen_a == twice(notes, "overflow"),
            f"u1 隔着 u2 的撤销，撤销自己写 P-1001 的那一步：P-1001 两端都回到写之前的 {len(plain)} 项、回到实体自己的记录里 —— "
            f"{told(seen_b)}；{valve} 的 9 项两端不变: {seen_a == twice(notes, 'overflow')}")
    again = u2.op("redo", doc2)
    rev += 1
    settled(rev)
    seen_a, seen_b = both_hold(valve, changed, "overflow"), both_hold("P-1001", plain, "inline")
    R.check("CC-2-large-props", went(again) and seen_a == twice(changed, "overflow") and seen_b == twice(plain, "inline"),
            f"u2 再重做：{valve} 的备注01 两端 = {[(p or {}).get('备注01') for p, _w in seen_a]}，9 项逐项相同: "
            f"{seen_a == twice(changed, 'overflow')}；P-1001 两端不变: {seen_b == twice(plain, 'inline')}")

    # 删掉再撤销：放回去的对象在对方那里是新建的，属性要是全的。
    handle1, handle2 = u1.tagged().get(valve), u2.tagged().get(valve)
    picked = u2.op("select", doc2, handles=[handle2])
    erased = u2.run("ERASE", doc2)
    rev += 1
    settled(rev)
    gone = both_hold(valve, None, "absent")
    server = server_notes(srv, valve)
    R.check("setup", went(picked) and went(erased) and gone == twice(None, "absent") and server is None,
            f"u2 删掉 {valve}：两端都没有它了（{[w for _p, w in gone]}），服务器也没有（{server}）")
    restored = u2.op("undo", doc2)
    rev += 1
    settled(rev)
    seen = both_hold(valve, changed, "overflow")
    server = server_notes(srv, valve)
    sent = last_sent(u2)
    carried = sent.get("props") if isinstance(sent.get("props"), dict) else {}
    R.check("CC-1-large-props", went(restored) and sent.get("kind") == "insert_symbol" and carried == changed
            and seen == twice(changed, "overflow") and server == changed,
            f"u2 撤销删除：放回去用的 {sent.get('kind')} 带着全部 {len(carried)} 项属性；u1 那边放回来的对象 9 项都在、与 u2 的逐项相同 —— "
            f"{told(seen)}；服务器记着的 9 项与之相同: {server == changed}")
    R.note(f"{valve} 的句柄：u1 {handle1} -> {u1.tagged().get(valve)}，u2 {handle2} -> {u2.tagged().get(valve)}")

    # 这一段一件事一条变更：没有多发，也没有漏发。
    said1 = m7h.frames(u1.err_text(), "->", "op")[sent1:]
    said2 = m7h.frames(u2.err_text(), "->", "op")[sent2:]
    ends = [u1.status(), u2.status()]
    R.dump("x05-large-props-frames.json", {"u1": said1, "u2": said2})
    R.check("CC-2-large-props", (len(said1), len(said2)) == (2, 16) and rev == 21
            and [s.get("base_revision") for s in ends] == [rev, rev]
            and not any(s.get("diverged") or s.get("last_apply_error") for s in ends),
            f"这一段 u1 做了 2 件事、u2 做了 16 件，各发出 {len(said1)} 条、{len(said2)} 条变更；两端修订号 3 -> "
            f"{[s.get('base_revision') for s in ends]}；diverged={[s.get('diverged') for s in ends]}，"
            f"last_apply_error={[s.get('last_apply_error') for s in ends]}")

    # ── u1 保存回原路径 ──────────────────────────────────────────────────
    st, sv = u1.api("POST", "/save", {"document_id": doc1})
    R.dump("x05-save-response.json", {"http_status": st, "body": sv})
    R.check("VAL-CROSS-005", st == 200 and (sv or {}).get("ok") is True, f"u1 保存回原路径: POST /save -> {st} ok={(sv or {}).get('ok')}")
    u1.kill()
    u2.kill()
    time.sleep(0.5)

    # ── 不连服务器重开，逐项比对 ─────────────────────────────────────────
    after_authors, after_records, kept = read_offline("x05-after", 3215, dwg_a, tags=(valve,))
    R.dump("x05-after.json", {"authors": after_authors, "records": after_records})
    rows = []
    for handle, before in sorted(base_authors.items()):
        after = after_authors.get(handle) or {}
        rows.append({"handle": handle, "tag_before": before.get("tag"), "tag_after": after.get("tag"),
                     "props_before": props_of(before), "props_after": props_of(after),
                     "present": bool(after), "props_not_fewer": len(props_of(after)) >= len(props_of(before))})
    added = sorted(set(after_authors) - set(base_authors))
    parsed = {}
    for key, rec in after_records.items():
        try:
            parsed[key] = [json.loads(s) for s in rec["strings"]]
        except ValueError as error:
            parsed[key] = f"解析失败: {error}"
    diff = {"entities": rows, "added_handles": added,
            "xrecords": {k: {"bytes_before": base_records.get(k, {}).get("bytes"),
                             "bytes_after": after_records.get(k, {}).get("bytes"),
                             "present": k in after_records, "parses": isinstance(parsed.get(k), list),
                             "identical": base_records.get(k, {}).get("strings") == after_records.get(k, {}).get("strings")}
                         for k in KEYS}}
    R.dump("cross-m3-m6.json", diff)
    R.check("VAL-CROSS-005", all(r["present"] and r["props_not_fewer"] for r in rows),
            "基线里每个带 PID_AUTHOR 的实体都还在，props 的键值对数量不少于基线: "
            + json.dumps([(r["handle"], r["tag_before"], r["tag_after"], len(r["props_before"]), len(r["props_after"])) for r in rows], ensure_ascii=False))
    p1001 = next((r for r in rows if r["tag_before"] == "P-1001"), {})
    R.check("VAL-CROSS-005", p1001.get("props_after", {}).get("设计压力") == "2.5MPa" and len(added) == 1
            and any(r["tag_after"] == "TI-1009" for r in rows),
            f"三次编辑都在保存的文件里: P-1001 的 props={json.dumps(p1001.get('props_after'), ensure_ascii=False)}，"
            f"新增实体 {added}，TI-1001 现在是 {[r['tag_after'] for r in rows if r['tag_before'] == 'TI-1001']}")
    saved, saved_where = kept.get(valve) or (None, "absent")
    R.check("CC-1-large-props", (saved, saved_where) == (changed, "overflow"),
            f"u1 是接收的一方，它保存的文件不连服务器重开：{valve} 的 9 项属性都在、与会话里最后的值逐项相同（{saved_where}）"
            f"{json.dumps(brief(saved), ensure_ascii=False)}")
    R.check("VAL-CROSS-005", all(diff["xrecords"][k]["present"] and diff["xrecords"][k]["parses"] for k in KEYS),
            f"三个 XRecord key 均存在且可解析为 JSON: {json.dumps(diff['xrecords'], ensure_ascii=False)}")
    R.check("VAL-CROSS-005", diff["xrecords"]["PID_AUTHOR_TAG_RULES"]["identical"],
            f"PID_AUTHOR_TAG_RULES 与基线逐字节相同（{base_records['PID_AUTHOR_TAG_RULES']['bytes']} 字节）")
except SystemExit:
    raise
except Exception as error:  # noqa: BLE001
    import traceback
    R.check("script", False, f"脚本异常: {error!r}\n{traceback.format_exc()}")
finally:
    m7h.cleanup()
    ok = R.summary()
sys.exit(0 if ok else 1)
