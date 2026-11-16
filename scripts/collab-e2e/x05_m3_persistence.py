# -*- coding: utf-8 -*-
"""VAL-CROSS-005：M3 的持久化数据穿越协同会话后完整。

基线图是 M3 往返测试的那份内容，由默认不跑的测试 write_the_m3_baseline_drawing
写出，放在证据目录的 m7/work/PID-1101-m3.dwg（契约写的那个路径上没有这个文件；
总入口在它不存在时先生成）。先不连服务器读基线；
u1、u2 连服务器各开一份，做三次协同编辑（改属性、插入阀门、改位号），都
收到 op_applied；u1 保存回它打开的那个路径；再不连服务器重开比对。

证据：logs/x05-*.json、logs/cross-m3-m6.json（基线与保存后的逐项对比）、
logs/gui-x05-*.err.log。
"""
import json
import sys
import time

import m7h

R = m7h.Results("x05_m3_persistence")
BASELINE = m7h.WORK / "PID-1101-m3.dwg"
KEYS = ("PID_AUTHOR_TYPE_LIB", "PID_AUTHOR_TAG_RULES", "PID_AUTHOR_TOPOLOGY")


def read_offline(name, port, dwg):
    """{handle: author dict} of every entity carrying PID_AUTHOR, and the graph XRecords."""
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
    g.kill()
    return authors, records


def props_of(author):
    try:
        return json.loads(author.get("props") or "{}")
    except ValueError:
        return {}


try:
    if not BASELINE.exists():
        R.check("setup", False, f"M3 基线图不存在：{BASELINE}")
        raise SystemExit(1)
    dwg_a = m7h.fresh_copy("x05-PID-1101-a.dwg", BASELINE)
    dwg_b = m7h.fresh_copy("x05-PID-1101-b.dwg", BASELINE)

    # ── 基线：不连服务器 ─────────────────────────────────────────────────
    base_authors, base_records = read_offline("x05-baseline", 3214, dwg_a)
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

    # ── u1 保存回原路径 ──────────────────────────────────────────────────
    st, sv = u1.api("POST", "/save", {"document_id": doc1})
    R.dump("x05-save-response.json", {"http_status": st, "body": sv})
    R.check("VAL-CROSS-005", st == 200 and (sv or {}).get("ok") is True, f"u1 保存回原路径: POST /save -> {st} ok={(sv or {}).get('ok')}")
    u1.kill()
    u2.kill()
    time.sleep(0.5)

    # ── 不连服务器重开，逐项比对 ─────────────────────────────────────────
    after_authors, after_records = read_offline("x05-after", 3215, dwg_a)
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
