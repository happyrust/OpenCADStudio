# -*- coding: utf-8 -*-
"""VAL-CROSS-003：管线插阀门后双方拓扑一致；之后删除阀门、管路、管线，对端随之一致。

u1（3210）与 u2（3211）各开一份种子图，连同一个服务器（3240）。u1 画管线
L-1001（0,0 → 200,0）、在它上面画一条管路、在管路上 PIDFITTING gate_valve 100,0。
检查读的是两个 GUI 的真实状态：带 PID_AUTHOR 的实体与它们的作者记录、PIDTOPOLOGY
读回的拓扑表、服务器的 /drawing/PID-1101/topology、conflicts 与 audit_log。

契约字面上是「画管线后直接 PIDFITTING」；PIDFITTING 只认管段，管段属于管路，所以这里
多一步 PIDPIPE run，op_applied 是三条（管线、管路、阀门）而不是两条。

证据：logs/y03-*.json、logs/gui-y03-*.err.log（帧原文）、logs/y03-srv.out.log、
shots/cross-pipe-u1.png、shots/cross-pipe-u2.png。
"""
import json
import sys
import time

import m7h
import yh

R = m7h.Results("y03_pipe_topology")
A = "VAL-CROSS-003"
DEL = "CT-13-delete"


def both_at(u1, u2, revision, seconds=15):
    return all(g.wait_status(lambda s: s.get("base_revision") == revision, seconds) is not None for g in (u1, u2))


try:
    if not m7h.SEED_DWG.exists():
        R.check("setup", False, f"种子图不存在：{m7h.SEED_DWG}")
        raise SystemExit(1)
    dwg_a = m7h.fresh_copy("y03-PID-1101-a.dwg", m7h.SEED_DWG)
    dwg_b = m7h.fresh_copy("y03-PID-1101-b.dwg", m7h.SEED_DWG)
    srv = m7h.Server(3240, tag="y03-srv").start(fresh=True)
    R.check("setup", srv.wait_health(), f"服务器 /health = {srv.health()}")
    u1 = m7h.Gui("y03-u1", 3210, dwg=dwg_a, user="u1").start()
    u2 = m7h.Gui("y03-u2", 3211, dwg=dwg_b, user="u2").start()
    R.check("setup", u1.wait_ready(180) is not None and u2.wait_ready(180) is not None, "两个 GUI 就绪")
    s1 = u1.wait_status(lambda s: s.get("connected") and s.get("session_id"), 30)
    s2 = u2.wait_status(lambda s: s.get("connected") and s.get("session_id"), 30)
    R.check("setup", s1 is not None and s2 is not None, "u1、u2 均已连接")
    for g in (u1, u2):
        g.run("ZOOM EXTENTS")
    before1, before2 = yh.snapshot(u1), yh.snapshot(u2)
    R.check("setup", before1["authored"] == before2["authored"] and len(before1["authored"]) == 1
            and before1["topology"]["pipe_lines"] == [],
            f"开始时两边的图相同：{len(before1['authored'])} 个带作者记录的实体，没有管线")

    # ── u1 画管线、管路、插阀门 ──────────────────────────────────────────
    steps = [("PIDPIPE", [(0, 0), (200, 0)], "create_pipe_line"),
             ("PIDPIPE run", [(0, 0), (200, 0)], "add_pipe_run"),
             ("PIDFITTING gate_valve", [(100, 0)], "insert_fitting")]
    responses = {}
    for revision, (cmd, points, kind) in enumerate(steps, start=1):
        resp = yh.draw(u1, cmd, points)
        responses[cmd] = {"http_status": resp[0], "body": resp[1]}
        arrived = both_at(u1, u2, revision)
        R.check(A, yh.ok(resp) and arrived,
                f"u1 执行 {cmd} {points}: ok={yh.body_of(resp).get('ok')}，两边的基线都到修订 {revision}: {arrived}")
    R.dump("y03-draw-responses.json", responses)

    sent1 = m7h.frames(u1.err_text(), "->", "op")
    app1 = m7h.frames(u1.err_text(), "<-", "op_applied")
    app2 = m7h.frames(u2.err_text(), "<-", "op_applied")
    R.dump("y03-op-frames.json", {"u1_sent": sent1, "u1_applied": app1, "u2_applied": app2})
    kinds_sent, kinds_u2 = yh.op_kinds(sent1), yh.op_kinds(app2)
    wanted = [[kind] for _cmd, _points, kind in steps]
    R.check(A, kinds_sent == wanted and sent1[0]["payload"].get("tag") == "L-1001"
            and sent1[1]["payload"].get("line_tag") == "L-1001" and sent1[2]["payload"].get("run_tag") == "L-1001-01",
            f"u1 发出三条 op：{kinds_sent}（管线 {sent1[0]['payload'].get('tag') if sent1 else None}，"
            f"管路 {sent1[1]['payload'].get('tag') if len(sent1) > 1 else None}，"
            f"阀门 {sent1[2]['payload'].get('tag') if len(sent1) > 2 else None} 插在 "
            f"{sent1[2]['payload'].get('at') if len(sent1) > 2 else None}）")
    R.check(A, kinds_u2 == wanted and [f.get("revision") for f in app2] == [1, 2, 3]
            and [f.get("revision") for f in app1] == [1, 2, 3],
            f"服务器向房间广播了对应的 op_applied：u2 收到 {kinds_u2}，修订 {[f.get('revision') for f in app2]}；"
            f"u1 也收到自己的三条（契约写两条：管线与阀门，多的一条是管路）")

    r1, r2, same = yh.wait_same(u1, u2, 15, also=lambda a, b: len(a["authored"]) == 6)
    R.dump("y03-after-valve.json", {"u1": r1, "u2": r2})
    R.check(A, same and len(r2["authored"]) == 6,
            f"两边带作者记录的实体逐个相同（种类、位置、类型、位号、拓扑角色、宿主、属性）：{len(r1['authored'])} 个 / {len(r2['authored'])} 个")

    # u2 自己的拓扑表，照契约读。
    raw2 = r2["raw_topology"]
    runs2 = raw2.get("pipe_runs") or []
    segs2 = {s["handle"]: s for s in raw2.get("pipe_segments") or []}
    fits2 = raw2.get("fittings") or []
    ordered = [segs2.get(h) for h in (runs2[0]["segments"] if runs2 else [])]
    R.check(A, len(runs2) == 1 and runs2[0]["tag"] == "L-1001-01" and len(ordered) == 2 and all(ordered),
            f"u2 的拓扑：管路 {runs2[0]['tag'] if runs2 else None} 被拆为 {len(ordered)} 段（管段数=2）")
    first, second = (ordered + [None, None])[:2]
    valve = fits2[0] if fits2 else {}
    ports = valve.get("ports") or {}
    R.check(A, bool(first and second) and len(fits2) == 1 and valve.get("fitting_type") == "gate_valve"
            and ports.get("连接点1") == first["handle"] and ports.get("连接点2") == second["handle"]
            and first["end"].get("peer") == f"{valve.get('handle')}:连接点1"
            and second["start"].get("peer") == f"{valve.get('handle')}:连接点2",
            f"u2 的阀门 {valve.get('tag')} 两个连接点分别连到两段的端点：ports={json.dumps(ports, ensure_ascii=False)}，"
            f"段一终点 peer={first['end'].get('peer') if first else None}，段二起点 peer={second['start'].get('peer') if second else None}")
    a_end, b_start = (first["end"]["point"], second["start"]["point"]) if first and second else ([0, 0], [0, 0])
    R.check(A, bool(first and second) and first["start"]["point"] == [0.0, 0.0] and second["end"]["point"] == [200.0, 0.0]
            and a_end[0] < 100.0 < b_start[0] and a_end[1] == 0.0 and b_start[1] == 0.0,
            f"无重叠无缺口：段一 {first['start']['point'] if first else None}→{a_end}，段二 {b_start}→{second['end']['point'] if second else None}，"
            f"两个内侧端点之间只有阀门（插在 100,0）")
    ends = lambda reading: [(s["start"], s["end"]) for run in reading["topology"]["pipe_runs"] for s in run["segments"]]  # noqa: E731
    R.check(A, ends(r1) == ends(r2) and len(ends(r1)) == 2 and r1["topology"] == r2["topology"],
            f"两侧同一拓扑查询（PIDTOPOLOGY）读回的段数与端点坐标完全相同：u1 {ends(r1)}，u2 {ends(r2)}；"
            f"去掉句柄后的整张拓扑表也相同")

    st, known = srv.http("GET", "/drawing/PID-1101/topology")
    R.dump("y03-server-topology.json", known)
    topo = (known or {}).get("topology") or known or {}
    run0 = (topo.get("pipe_runs") or [{}])[0]
    R.check(A, st == 200 and [l.get("tag") for l in topo.get("pipe_lines") or []] == ["L-1001"]
            and (topo.get("pipe_lines") or [{}])[0].get("runs") == ["L-1001-01"]
            and len(run0.get("fittings") or []) == 1 and len(run0.get("segments") or []) == 2,
            f"服务器侧的拓扑：管线 {[l.get('tag') for l in topo.get('pipe_lines') or []]}，管路 {run0.get('tag')} "
            f"有 {len(run0.get('fittings') or [])} 个管件、{len(run0.get('segments') or [])} 段")
    conflicts = srv.sql("select conflict_id, object_ref, field, status from conflicts where object_ref='L-1001'")
    R.check(A, conflicts == [], f"conflicts 表里没有 object_ref='L-1001' 的行（{len(conflicts)} 行）：顺序提交不构成冲突")
    audit = srv.sql("select seq, action, object_ref, user_id, change_text from audit_log "
                    "where action in ('create_pipe_line','add_pipe_run','insert_fitting') order by seq")
    R.dump("y03-audit.json", audit)
    R.check(A, [r["action"] for r in audit] == ["create_pipe_line", "add_pipe_run", "insert_fitting"]
            and all(r["user_id"] == "u1" and r["object_ref"] == "L-1001" for r in audit),
            f"audit_log 记了三步，都在管线 L-1001 名下、用户 u1：{[(r['action'], r['object_ref']) for r in audit]}")
    fine, why = yh.not_diverged(u1, u2)
    R.check(A, fine, f"两边都没有与房间不一致：{why}")
    time.sleep(0.5)
    for name, g in (("u1", u1), ("u2", u2)):
        # 管线画在 X 轴上（契约给的坐标），取景对着管线的各个部分，避开命令行。
        yh.focus_on(g)
        time.sleep(0.4)
        st, resp, shot = g.capture(m7h.SHOTS / f"cross-pipe-{name}.png")
        R.check(A, st == 200 and shot, f"{name} 截图 shots/cross-pipe-{name}.png")

    # ── 此后对这条管线的属性修改到了对端不再「对不上对象」 ─────────────
    resp = u1.run("PIDSETPROP L-1001 公称直径 DN80")
    both_at(u1, u2, 4)
    r1, r2, same = yh.wait_same(u1, u2, 15, also=lambda a, b: sum("DN80" in row for row in b["authored"]) >= 4)
    carried = [row for row in r2["authored"] if "DN80" in row]
    fine, why = yh.not_diverged(u1, u2)
    R.check(A, yh.ok(resp) and same and len(carried) == 4 and fine,
            f"u1 把 L-1001 的公称直径改成 DN80：u2 的管线、管路和两段都读回 DN80（{len(carried)} 个实体），两边仍一致，没有被标成不一致：{why}")

    # ── 删除：阀门、管路、管线 ───────────────────────────────────────────
    valve_h = yh.handles_where(r1, lambda a: a.get("type") == "VALVE")
    resp = yh.erase(u1, valve_h)
    both_at(u1, u2, 5)
    r1, r2, same = yh.wait_same(u1, u2, 15, also=lambda a, b: not any("type=VALVE" in row for row in b["authored"]))
    R.dump("y03-after-valve-erased.json", {"u1": r1, "u2": r2})
    st, known = srv.http("GET", "/drawing/PID-1101/topology")
    topo = (known or {}).get("topology") or known or {}
    R.check(DEL, yh.ok(resp) and same and len(valve_h) == 1 and not any("type=VALVE" in row for row in r2["authored"])
            and len(r2["topology"]["pipe_runs"][0]["segments"]) == 2 and r2["topology"]["fittings"] == []
            and ((topo.get("pipe_runs") or [{}])[0].get("fittings")) == [],
            f"u1 删掉阀门：u2 的阀门也没了，两段管段留着（中间是缺口），两边的图与拓扑表相同；服务器的管路上也不再有管件")

    run_h = yh.handles_where(r1, lambda a: a.get("topology") in ("pipe_run", "pipe_segment"))
    resp = yh.erase(u1, run_h)
    both_at(u1, u2, 6)
    r1, r2, same = yh.wait_same(u1, u2, 15, also=lambda a, b: len(b["authored"]) == 2)
    R.dump("y03-after-run-erased.json", {"u1": r1, "u2": r2})
    st, known = srv.http("GET", "/drawing/PID-1101/topology")
    topo = (known or {}).get("topology") or known or {}
    R.check(DEL, yh.ok(resp) and same and len(run_h) == 3 and len(r2["authored"]) == 2
            and r2["topology"]["pipe_runs"] == [] and (topo.get("pipe_runs") or []) == [],
            f"u1 删掉管路（本体与两段）：u2 只剩容器和管线（{len(r2['authored'])} 个实体），拓扑表里没有管路；服务器侧也没有")

    line_h = yh.handles_where(r1, lambda a: a.get("topology") == "pipe_line")
    resp = yh.erase(u1, line_h)
    both_at(u1, u2, 7)
    r1, r2, same = yh.wait_same(u1, u2, 15, also=lambda a, b: len(b["authored"]) == 1)
    R.dump("y03-after-line-erased.json", {"u1": r1, "u2": r2})
    st, known = srv.http("GET", "/drawing/PID-1101/topology")
    topo = (known or {}).get("topology") or known or {}
    R.check(DEL, yh.ok(resp) and same and len(line_h) == 1 and r2["authored"] == before2["authored"]
            and r2["topology"]["pipe_lines"] == [] and (topo.get("pipe_lines") or []) == [],
            f"u1 删掉管线：u2 的图回到开始时的样子（{len(r2['authored'])} 个实体），两边与服务器都没有管线")
    app2 = m7h.frames(u2.err_text(), "<-", "op_applied")
    R.dump("y03-op-frames-all.json", {"u1_sent": m7h.frames(u1.err_text(), "->", "op"), "u2_applied": app2})
    kinds = yh.op_kinds(app2)
    R.check(DEL, kinds[4:] == [["delete_object", "update_pipe_run"], ["delete_object"], ["delete_object"]],
            f"三步删除在线上的说法：{kinds[4:]}（阀门：位号没了加所在管路的现状；管路；管线）")
    fine, why = yh.not_diverged(u1, u2)
    st1, st2 = u1.status(), u2.status()
    R.check(DEL, fine and st1.get("base_revision") == 7 and st2.get("base_revision") == 7,
            f"全程两边都没有被标成不一致：{why}；基线 {st1.get('base_revision')}/{st2.get('base_revision')}")
except SystemExit:
    raise
except Exception as error:  # noqa: BLE001
    import traceback
    R.check("script", False, f"脚本异常: {error!r}\n{traceback.format_exc()}")
finally:
    m7h.cleanup()
    ok = R.summary()
sys.exit(0 if ok else 1)
