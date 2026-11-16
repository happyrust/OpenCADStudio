# -*- coding: utf-8 -*-
"""VAL-CROSS-008：离线编辑排队，重连后对账；被服务器拒绝的修改在本地撤回并有提示。

u1（3210）与 u2（3211）各开一份种子图（一个容器 P-1001），连同一个服务器（3240）。

第一轮（不同字段）：杀掉服务器；u1 改 P-1001 的设计压力、u2 改它的设计温度；用同一个库
把服务器起回来。两边 60 秒内各以新会话连上，把排队的修改提交，两边的图一致，服务器侧
读回两个值，审计里各有一条。

第二轮（同一字段）：再杀一次；两边都改设计压力，值不同；起回来。先到的一方生效，后到的
一方落后让位（OP_SUPERSEDED）：它的那一步在本地撤回，字段是胜出的值，状态里有说明，
审计里有让位记录。谁先到不由脚本定，检查对两种次序都成立。

第三轮（对方不知情的占用）：u1 占着 P-1001，u2 没有申请过、不知道；u2 改它的属性，
服务器以 CLAIM_HELD 拒绝，u2 那一步撤回并有说明。

检查读的是两个 GUI 的真实状态（带 PID_AUTHOR 的实体与作者记录、/collab/status）、
服务器的 /drawing/PID-1101/objects 与库里的 ops、audit_log、claims 表，以及两个 GUI
收发的帧原文。

证据：logs/y08-*.json、logs/gui-y08-*.err.log、logs/y08-srv.out.log、
shots/cross-offline-u1.png、shots/cross-offline-u2.png。
"""
import json
import sys
import time

import m7h
import yh

R = m7h.Results("y08_offline_reconcile")
A = "VAL-CROSS-008"
TAKEN_BACK = "CT-14-rejected"
OBJ = "P-1001"
PRESSURE, TEMPERATURE = "设计压力", "设计温度"


def props(gui, handle):
    """The properties the drawing has for one entity (its author record)."""
    try:
        return json.loads(gui.author(handle).get("props") or "{}")
    except ValueError:
        return {}


def value(gui, handle, field):
    return props(gui, handle).get(field)


def server_value(srv, field):
    status, body = srv.http("GET", f"/drawing/PID-1101/objects?object_ref={OBJ}")
    items = (body or {}).get("items") or []
    return ((items[0].get("fields") or {}).get(field) or {}).get("value") if items else None


def away(*guis, seconds=20):
    return all(g.wait_status(lambda s: s.get("connected") is False and not s.get("session_id"), seconds) is not None
               for g in guis)


def back(gui, old_session, seconds=60):
    """Wait until `gui` is in a new session with nothing left to send."""
    return gui.wait_status(
        lambda s: s.get("connected") and s.get("session_id") and s.get("session_id") != old_session
        and s.get("queued_ops") == 0 and s.get("catching_up") is None, seconds)


def set_prop(gui, field, text):
    return gui.run(f"PIDSETPROP {OBJ} {field} {text}")


def ops_rows(srv):
    return srv.sql("select revision, op_id, user_id, base_revision, changes from ops where drawing_id='PID-1101' order by revision")


def audit_rows(srv):
    return srv.sql("select seq, action, object_ref, user_id, change_text from audit_log order by seq")


try:
    if not m7h.SEED_DWG.exists():
        R.check("setup", False, f"种子图不存在：{m7h.SEED_DWG}")
        raise SystemExit(1)
    dwg_a = m7h.fresh_copy("y08-PID-1101-a.dwg", m7h.SEED_DWG)
    dwg_b = m7h.fresh_copy("y08-PID-1101-b.dwg", m7h.SEED_DWG)
    srv = m7h.Server(3240, tag="y08-srv").start(fresh=True)
    R.check("setup", srv.wait_health(), f"服务器 /health = {srv.health()}")
    u1 = m7h.Gui("y08-u1", 3210, dwg=dwg_a, user="u1").start()
    u2 = m7h.Gui("y08-u2", 3211, dwg=dwg_b, user="u2").start()
    R.check("setup", u1.wait_ready(180) is not None and u2.wait_ready(180) is not None, "两个 GUI 就绪")
    s1 = u1.wait_status(lambda s: s.get("connected") and s.get("session_id"), 30)
    s2 = u2.wait_status(lambda s: s.get("connected") and s.get("session_id"), 30)
    R.check("setup", s1 is not None and s2 is not None, "u1、u2 均已连接")
    for g in (u1, u2):
        g.run("ZOOM EXTENTS")
    h1, h2 = u1.tagged().get(OBJ), u2.tagged().get(OBJ)
    R.check("setup", bool(h1 and h2), f"两边都有 {OBJ}（句柄 {h1} / {h2}）")

    # 在线先改一次：队列在线上也是它，这一步应当照常生效。
    resp = set_prop(u1, PRESSURE, "1.0MPa")
    at1 = all(g.wait_status(lambda s: s.get("base_revision") == 1 and s.get("queued_ops") == 0, 15) is not None
              for g in (u1, u2))
    R.check("setup", yh.ok(resp) and at1 and value(u2, h2, PRESSURE) == "1.0MPa",
            f"在线时 u1 改设计压力为 1.0MPa：两边到修订 1，u2 读回 {value(u2, h2, PRESSURE)}，队列为空")
    old1, old2 = u1.status().get("session_id"), u2.status().get("session_id")

    # ── 第一轮：离线各改一处（不同字段） ─────────────────────────────────
    srv.kill()
    R.check(A, away(u1, u2), "服务器被杀掉后，两边都报告未连接、没有会话")
    r1 = set_prop(u1, PRESSURE, "1.6MPa")
    r2 = set_prop(u2, TEMPERATURE, "120℃")
    q1, q2 = u1.status(), u2.status()
    R.dump("y08-round1-offline-status.json", {"u1": q1, "u2": q2})
    R.check(A, yh.ok(r1) and yh.ok(r2) and q1.get("queued_ops") == 1 and q2.get("queued_ops") == 1
            and value(u1, h1, PRESSURE) == "1.6MPa" and value(u2, h2, TEMPERATURE) == "120℃"
            and value(u2, h2, PRESSURE) == "1.0MPa",
            f"离线期间 u1 把设计压力改成 1.6MPa、u2 把设计温度改成 120℃：两条命令都成功，"
            f"各自排队 {q1.get('queued_ops')} / {q2.get('queued_ops')} 个 op，修改只在各自的图上")
    restarted = time.time()
    srv.start(fresh=False)
    R.check(A, srv.wait_health(), f"服务器用同一个库重新起来：/health = {srv.health()}")
    b1, b2 = back(u1, old1), back(u2, old2)
    took = time.time() - restarted
    R.dump("y08-round1-back-status.json", {"u1": b1, "u2": b2})
    R.check(A, b1 is not None and b2 is not None and took <= 60,
            f"两边在 {took:.1f} 秒内各以新会话连上（≤ 60 秒）：u1 {old1} → {(b1 or {}).get('session_id')}，"
            f"u2 {old2} → {(b2 or {}).get('session_id')}，排队的 op 都已得到答复")
    hello1 = m7h.frames(u1.err_text(), "->", "hello")
    hello2 = m7h.frames(u2.err_text(), "->", "hello")
    welcome1 = m7h.frames(u1.err_text(), "<-", "welcome")
    welcome2 = m7h.frames(u2.err_text(), "<-", "welcome")
    R.dump("y08-round1-handshakes.json", {"u1_hello": hello1, "u1_welcome": welcome1,
                                           "u2_hello": hello2, "u2_welcome": welcome2})
    R.check(A, len(hello1) >= 2 and len(hello2) >= 2 and "since" not in hello1[0] and "since" not in hello2[0]
            and hello1[-1].get("since") == 1 and hello2[-1].get("since") == 1
            and welcome1[-1].get("replay_from") == 1 and welcome2[-1].get("replay_from") == 1,
            f"重连的 hello 说出本图所在的修订（since={hello1[-1].get('since') if hello1 else None} / "
            f"{hello2[-1].get('since') if hello2 else None}，第一次加入时没有这个字段），"
            f"welcome 答以 replay_from={welcome1[-1].get('replay_from') if welcome1 else None} / "
            f"{welcome2[-1].get('replay_from') if welcome2 else None}")
    both3 = all(g.wait_status(lambda s: s.get("base_revision") == 3, 15) is not None for g in (u1, u2))
    a1, a2, same = yh.wait_same(u1, u2, 15, also=lambda a, b: True)
    R.dump("y08-round1-drawings.json", {"u1": a1["authored"], "u2": a2["authored"]})
    R.check(A, both3 and same
            and value(u1, h1, PRESSURE) == "1.6MPa" and value(u2, h2, PRESSURE) == "1.6MPa"
            and value(u1, h1, TEMPERATURE) == "120℃" and value(u2, h2, TEMPERATURE) == "120℃",
            f"两边的图一致，且都有两处修改：设计压力 {value(u1, h1, PRESSURE)} / {value(u2, h2, PRESSURE)}，"
            f"设计温度 {value(u1, h1, TEMPERATURE)} / {value(u2, h2, TEMPERATURE)}；基线都到修订 3")
    R.check(A, server_value(srv, PRESSURE) == "1.6MPa" and server_value(srv, TEMPERATURE) == "120℃",
            f"服务器侧读回：设计压力 {server_value(srv, PRESSURE)}，设计温度 {server_value(srv, TEMPERATURE)}")
    rows = ops_rows(srv)
    R.dump("y08-round1-ops.json", rows)
    offline = [r for r in rows if r["revision"] in (2, 3)]
    R.check(A, len(rows) == 3 and sorted(r["user_id"] for r in offline) == ["u1", "u2"]
            and all(r["base_revision"] == 1 for r in offline),
            f"两条离线修改各提交为一个 op：修订 {[r['revision'] for r in offline]}，"
            f"用户 {[r['user_id'] for r in offline]}，都带着断线时的基线 {[r['base_revision'] for r in offline]}"
            f"（后提交的那条落后一个修订，改的是另一个字段，自动合并）")
    audit = audit_rows(srv)
    R.dump("y08-round1-audit.json", audit)
    edits = [r for r in audit if r["object_ref"] == OBJ and r["action"] == "set_property"]
    R.check(A, sorted(r["user_id"] for r in edits) == ["u1", "u1", "u2"],
            f"audit_log 里 {OBJ} 的 set_property 共 {len(edits)} 条：在线一条（u1）加离线两条（u1、u2）")
    fine, why = yh.not_diverged(u1, u2)
    st1, st2 = u1.status(), u2.status()
    R.check(A, fine and not st1.get("rejections") and not st2.get("rejections"),
            f"没有被拒的 op，两边都没有与房间不一致：{why}")

    # ── 第二轮：离线改同一个字段 ─────────────────────────────────────────
    old1, old2 = st1.get("session_id"), st2.get("session_id")
    srv.kill()
    R.check(TAKEN_BACK, away(u1, u2), "第二次杀掉服务器，两边都报告未连接")
    r1 = set_prop(u1, PRESSURE, "2.5MPa")
    r2 = set_prop(u2, PRESSURE, "4.0MPa")
    R.check(TAKEN_BACK, yh.ok(r1) and yh.ok(r2) and value(u1, h1, PRESSURE) == "2.5MPa"
            and value(u2, h2, PRESSURE) == "4.0MPa",
            "离线期间两边都改设计压力：u1 改成 2.5MPa，u2 改成 4.0MPa，各在各的图上")
    srv.start(fresh=False)
    R.check(TAKEN_BACK, srv.wait_health(), "服务器再次用同一个库起来")
    b1, b2 = back(u1, old1), back(u2, old2)
    both4 = all(g.wait_status(lambda s: s.get("base_revision") == 4, 15) is not None for g in (u1, u2))
    st1, st2 = u1.status(), u2.status()
    R.dump("y08-round2-status.json", {"u1": st1, "u2": st2})
    rej1, rej2 = st1.get("rejections") or [], st2.get("rejections") or []
    winner = server_value(srv, PRESSURE)
    loser, loser_status, loser_rej = ("u2", st2, rej2) if rej2 else ("u1", st1, rej1)
    R.check(TAKEN_BACK, b1 is not None and b2 is not None and both4 and winner in ("2.5MPa", "4.0MPa")
            and (len(rej1), len(rej2)) in ((1, 0), (0, 1)),
            f"两边重新连上，服务器只收下先到的一条（修订 4，设计压力 {winner}）；另一条被拒："
            f"u1 被拒 {len(rej1)} 次，u2 被拒 {len(rej2)} 次")
    said = (loser_rej[0] if loser_rej else {})
    R.check(TAKEN_BACK, said.get("code") == "OP_SUPERSEDED" and said.get("outcome") == "已撤回"
            and said.get("steps_moved") == 1,
            f"{loser} 的那一步落后让位并在本地撤回：{json.dumps(said, ensure_ascii=False)}")
    a1, a2, same = yh.wait_same(u1, u2, 15, also=lambda a, b: True)
    R.dump("y08-round2-drawings.json", {"u1": a1["authored"], "u2": a2["authored"]})
    R.check(TAKEN_BACK, same and value(u1, h1, PRESSURE) == winner and value(u2, h2, PRESSURE) == winner,
            f"两边的图一致，设计压力都是胜出的值 {winner}（u1 读回 {value(u1, h1, PRESSURE)}，u2 读回 {value(u2, h2, PRESSURE)}）"
            f"——被拒的一方不是退回它改之前的 1.6MPa")
    hint = loser_status.get("last_error") or ""
    R.check(TAKEN_BACK, "OP_SUPERSEDED" in hint and "已撤回" in hint,
            f"{loser} 有提示（/collab/status 的 last_error，也是命令行上那句）：{hint}")
    loser_gui = u2 if loser == "u2" else u1
    errors = [f for f in m7h.frames(loser_gui.err_text(), "<-", "error") if f.get("code") == "OP_SUPERSEDED"]
    R.dump("y08-round2-error-frames.json", errors)
    R.check(TAKEN_BACK, len(errors) == 1 and errors[0].get("op_id") == said.get("op_id") and errors[0].get("rejected") is True,
            f"服务器的 error 帧点名了被拒的 op 并标明整个没有生效：op_id={errors[0].get('op_id') if errors else None}，"
            f"rejected={errors[0].get('rejected') if errors else None}")
    audit = audit_rows(srv)
    R.dump("y08-round2-audit.json", audit)
    yielded = [r for r in audit if r["action"] == "property_superseded" and r["object_ref"] == OBJ]
    R.check(TAKEN_BACK, len(yielded) == 1 and yielded[0]["user_id"] == loser and len(ops_rows(srv)) == 4,
            f"audit_log 留下让位记录：{[(r['action'], r['user_id'], r['change_text']) for r in yielded]}；"
            f"ops 表共 {len(ops_rows(srv))} 行（被拒的那条没有进）")
    fine, why = yh.not_diverged(u1, u2)
    R.check(TAKEN_BACK, fine, f"两边都没有与房间不一致：{why}")

    # ── 第三轮：对方不知情的占用 ─────────────────────────────────────────
    resp = u1.run(f"PIDCLAIM {OBJ}")
    held = u1.wait_status(lambda s: (s.get("claims") or {}).get(OBJ) == "u1", 10)
    unaware = OBJ not in (u2.status().get("claims") or {})
    R.check(TAKEN_BACK, held is not None and unaware,
            f"u1 占用 {OBJ}；u2 没有申请过，它的状态里没有这条占用（{u2.status().get('claims')}）")
    before_temp = value(u2, h2, TEMPERATURE)
    resp = set_prop(u2, TEMPERATURE, "150℃")
    refused = u2.wait_status(lambda s: any(r.get("code") == "CLAIM_HELD" for r in (s.get("rejections") or [])), 15)
    R.dump("y08-round3-status.json", {"u2": refused or u2.status()})
    # 命令行上的那句提示，趁它还是最新的一行。
    st, shot_resp, shot = u2.capture(m7h.SHOTS / "cross-offline-u2-refused.png")
    R.note(f"u2 被拒后的截图 shots/cross-offline-u2-refused.png: HTTP {st}，文件 {'在' if shot else '不在'}")
    held_rej = [r for r in ((refused or {}).get("rejections") or []) if r.get("code") == "CLAIM_HELD"]
    R.check(TAKEN_BACK, refused is not None and len(held_rej) == 1 and held_rej[0].get("outcome") == "已撤回",
            f"u2 改 {OBJ} 的设计温度：服务器以 CLAIM_HELD 拒绝，u2 那一步撤回：{json.dumps(held_rej[:1], ensure_ascii=False)}")
    a1, a2, same = yh.wait_same(u1, u2, 15, also=lambda a, b: True)
    hint = (u2.status().get("last_error") or "")
    R.check(TAKEN_BACK, same and value(u2, h2, TEMPERATURE) == before_temp == "120℃"
            and value(u1, h1, TEMPERATURE) == "120℃" and server_value(srv, TEMPERATURE) == "120℃"
            and "CLAIM_HELD" in hint,
            f"u2 的设计温度回到 {value(u2, h2, TEMPERATURE)}，u1 与服务器一直是 120℃，两边的图一致；提示：{hint}")
    claim_errors = [f for f in m7h.frames(u2.err_text(), "<-", "error") if f.get("code") == "CLAIM_HELD"]
    R.check(TAKEN_BACK, len(claim_errors) == 1 and claim_errors[0].get("rejected") is True and claim_errors[0].get("op_id"),
            f"error 帧：{json.dumps(claim_errors[:1], ensure_ascii=False)}")
    st1, st2 = u1.status(), u2.status()
    R.check(TAKEN_BACK, st1.get("base_revision") == 4 and st2.get("base_revision") == 4 and st2.get("queued_ops") == 0
            and len(ops_rows(srv)) == 4,
            f"房间的修订没有动（{st1.get('base_revision')} / {st2.get('base_revision')}），u2 的队列为空，ops 表仍是 4 行")
    # 占用放开之后，u2 重新改一次，照常生效：被拒不是把会话弄坏了。
    u1.run(f"PIDRELEASE {OBJ}")
    u1.wait_status(lambda s: OBJ not in (s.get("claims") or {}), 10)
    resp = set_prop(u2, TEMPERATURE, "150℃")
    both5 = all(g.wait_status(lambda s: s.get("base_revision") == 5, 15) is not None for g in (u1, u2))
    a1, a2, same = yh.wait_same(u1, u2, 15, also=lambda a, b: True)
    fine, why = yh.not_diverged(u1, u2)
    R.check(TAKEN_BACK, yh.ok(resp) and both5 and same and value(u1, h1, TEMPERATURE) == "150℃" and fine,
            f"u1 放开占用后 u2 再改一次：两边到修订 5，u1 读回设计温度 {value(u1, h1, TEMPERATURE)}，两边的图一致：{why}")
    time.sleep(0.5)
    for name, g in (("u1", u1), ("u2", u2)):
        st, resp, shot = g.capture(m7h.SHOTS / f"cross-offline-{name}.png")
        R.check(TAKEN_BACK, st == 200 and shot, f"{name} 截图 shots/cross-offline-{name}.png")
except SystemExit:
    raise
except Exception as error:  # noqa: BLE001
    import traceback
    R.check("script", False, f"脚本异常: {error!r}\n{traceback.format_exc()}")
finally:
    m7h.cleanup()
    ok = R.summary()
sys.exit(0 if ok else 1)
