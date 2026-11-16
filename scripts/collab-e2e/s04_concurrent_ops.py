# -*- coding: utf-8 -*-
"""VAL-SRV-030：并发提交时修订号唯一、不丢帧（concurrent-ops）。

只起协同服务器和 N 个 WS 会话（每个会话的请求与回执逐条落在 logs/ws-s04-cNN.jsonl），SQLite
只读查询。不起 GUI，不涉及指针与焦点。

同一张图上 N 个会话（各是一个用户）同时提交，分两段，每段每个会话 M 次：

  甲  抢同一修订：提交的是服务器不认识的 op 种类，只有基于当前修订才生效，否则回
      STALE_BASE_REVISION。各会话的第一条基于同一个修订同时发出（只能有一条生效），之后
      每个会话收到自己的回执就基于听到的最新修订发下一条。
  乙  各写各的：每个会话对自己的对象连续写 M 次同一个字段，不等回执；这些写入互不相干，
      必须全部生效。

全部回执收齐后：每条提交恰有一条回执；生效的那些修订号无重复、正好是 1..K，K 等于总提交数
减去被 STALE_BASE_REVISION 拒掉的条数；每个 op_id 在每条连接上至多一条 op_applied，每条连接
听到的序列都完整、顺序相同；drawings.revision 等于 K；审计表里的 op 行与生效的 op 一一对应。

规模取契约的下限（N = 8，M = 30），是有意的：服务器给每张图的广播队列只留最近 256 条
（bus.rs 的 DEFAULT_CAPACITY），某条连接落后超过这个数就会被丢广播（stderr 记一行 subscriber
lagged，客户端靠重新握手对齐）。每段最多 N×M = 240 条广播，在这个数以内，所以「每条连接都
听全」在这里是必须成立的；把 N 改成 10（每段 300 条）就能看到服务器丢广播。
"""
import collections
import json
import sys
import threading
import time

import m7h

R = m7h.Results("s04_concurrent_ops")
DRAWING = "PID-1101"
N = 8                   # 并发会话数（契约要求不少于 8）
M = 30                  # 每段每个会话提交的 op 数（契约要求不少于 30）
BUS_CAPACITY = 256      # 服务器每张图的广播队列容量：每段的广播数不超过它，才谈得上「听全」
PROBE = "e2e_probe"     # 服务器不认识的 op 种类
FIELD = "备注"
ANSWER_SECONDS = 30
SETTLE_SECONDS = 15


def seed_for(count):
    """The stock seed plus `count` editors, one per concurrent session."""
    seed = json.loads(m7h.SEED_JSON.read_text(encoding="utf-8"))
    user = next(u for u in seed["users"] if u["user_id"] == "u1")
    grant = next(g for g in seed["pbs_acl"] if g["subject"] == "u1")
    for index in range(1, count + 1):
        seed["users"].append({**user, "user_id": f"u-c{index:02d}", "display_name": f"并发{index:02d}"})
        seed["pbs_acl"].append({**grant, "subject": f"u-c{index:02d}"})
    path = m7h.WORK / "s04-seed.json"
    path.write_text(json.dumps(seed, ensure_ascii=False, indent=1), encoding="utf-8")
    return path


class Client:
    """One session, and what it has sent and heard so far."""

    def __init__(self, srv, index):
        self.index = index
        self.user = f"u-c{index:02d}"
        self.object = f"V-90{index:02d}"
        self.ws = m7h.Ws(f"ws://127.0.0.1:{srv.port}/ws", f"s04-c{index:02d}")
        self.welcome = self.ws.hello(self.user, DRAWING)
        self.revision = (self.welcome or {}).get("base_revision") or 0     # newest revision heard
        self.cursor = 0          # how far into ws.frames this client has looked
        self.sent = []           # (phase, op_id, base_revision), in the order sent
        self.answers = {}        # own op_id -> the frames that answer it
        self.heard = []          # (revision, op_id) of every op_applied, in the order heard

    def pump(self):
        """Take in the frames that arrived since the last look."""
        with self.ws.lock:
            fresh = self.ws.frames[self.cursor:]
            self.cursor = len(self.ws.frames)
        for frame in fresh:
            msg = frame["msg"]
            if frame["dir"] != "in" or msg.get("type") not in ("op_applied", "error"):
                continue
            if msg["type"] == "op_applied":
                self.heard.append((msg.get("revision"), msg.get("op_id")))
                if isinstance(msg.get("revision"), int):
                    self.revision = max(self.revision, msg["revision"])
            if msg.get("op_id") in self.answers:
                self.answers[msg["op_id"]].append(msg)

    def submit(self, phase, op_id, payload, base=None):
        base = self.revision if base is None else base
        self.answers[op_id] = []
        self.sent.append((phase, op_id, base))
        self.ws.send({"type": "op", "session_id": self.ws.session_id, "base_revision": base, "op_id": op_id,
                      "payload": payload})
        return base

    def wait(self, done, seconds):
        deadline = time.time() + seconds
        while True:
            self.pump()
            if done():
                return True
            if self.ws.closed or time.time() >= deadline:
                return False
            time.sleep(0.002)


def contend(client, gate, base):
    """Phase 甲: one op at a time, each against the newest revision heard."""
    gate.wait()
    for turn in range(M):
        op_id = f"s04-a-c{client.index:02d}-{turn:02d}"
        # The first op of every session is made against the same revision.
        used = client.submit("甲", op_id, {"kind": PROBE, "text": op_id}, base if turn == 0 else None)
        if not client.wait(lambda: client.answers[op_id], ANSWER_SECONDS):
            return      # no answer: the checks below say which op
        if client.answers[op_id][0].get("type") == "error":
            # Turned away as stale: look at the drawing again before asking again.
            client.wait(lambda: client.revision > used, 0.5)


def burst(client, gate):
    """Phase 乙: M writes to the session's own object, back to back."""
    gate.wait()
    for turn in range(M):
        op_id = f"s04-b-c{client.index:02d}-{turn:02d}"
        client.pump()
        client.submit("乙", op_id, {"kind": "set_property", "object_ref": client.object, "field": FIELD, "value": op_id})


def run_phase(clients, work, *more):
    """Release every session's thread at once; seconds until every op of the phase is answered."""
    gate = threading.Barrier(len(clients))
    threads = [threading.Thread(target=work, args=(client, gate) + more, daemon=True) for client in clients]
    started = time.time()
    for thread in threads:
        thread.start()
    for thread in threads:
        thread.join(4 * ANSWER_SECONDS)
    deadline = time.time() + ANSWER_SECONDS
    for client in clients:
        client.wait(lambda: all(client.answers.values()), deadline - time.time())
    return time.time() - started


def settle(clients, count):
    """Let every socket catch up with `count` applied ops, then a little longer for anything extra."""
    deadline = time.time() + SETTLE_SECONDS
    for client in clients:
        client.wait(lambda: len(client.heard) >= count, deadline - time.time())
    time.sleep(0.5)
    for client in clients:
        client.pump()


def switches(order, phase):
    """How often two neighbours in commit order come from different sessions."""
    who = [op_id.split("-")[2] for _, op_id in order if op_id.startswith(f"s04-{phase}-")]
    return sum(1 for one, other in zip(who, who[1:]) if one != other)


def answered(clients, phase):
    """(applied {op_id: [revisions]}, stale [op_id], other [(op_id, frame)]) of one phase, as its senders heard."""
    applied, stale, other = {}, [], []
    for client in clients:
        for sent_in, op_id, _ in client.sent:
            if sent_in != phase:
                continue
            for msg in client.answers[op_id]:
                if msg.get("type") == "op_applied":
                    applied.setdefault(op_id, []).append(msg.get("revision"))
                elif msg.get("code") == "STALE_BASE_REVISION" and msg.get("rejected") is True:
                    stale.append(op_id)
                else:
                    other.append((op_id, msg))
    return applied, stale, other


try:
    srv = m7h.Server(port=3240, seed=seed_for(N), heartbeat_timeout=600, tag="s04-srv")
    srv.start(fresh=True)
    R.check("setup", srv.wait_health(), f"服务器已启动，/health = {srv.health()}")
    R.check("setup", N >= 8 and M >= 30 and N * M <= BUS_CAPACITY,
            f"规模：{N} 个会话、每段每个会话 {M} 条（契约下限 8、30）；每段最多 {N * M} 条广播，"
            f"不超过服务器每张图的广播队列容量 {BUS_CAPACITY}")
    clients = [Client(srv, index) for index in range(1, N + 1)]
    sessions = {client.ws.session_id for client in clients}
    start = {client.revision for client in clients}
    R.check("setup", None not in sessions and len(sessions) == N and start == {0},
            f"{N} 个会话（各是一个用户）握手同一张图 {DRAWING}，会话号互不相同，起始修订 {sorted(start)}")

    took_a = run_phase(clients, contend, 0)
    a_applied, a_stale, a_other = answered(clients, "甲")
    settle(clients, len(a_applied))
    took_b = run_phase(clients, burst)
    b_applied, b_stale, b_other = answered(clients, "乙")
    settle(clients, len(a_applied) + len(b_applied))
    a_applied, a_stale, a_other = answered(clients, "甲")
    b_applied, b_stale, b_other = answered(clients, "乙")
    R.note(f"甲段 {took_a:.2f} 秒，乙段 {took_b:.2f} 秒")

    # ── 每条提交恰有一条回执 ────────────────────────────────────────────
    submitted = [(client, phase, op_id, base) for client in clients for phase, op_id, base in client.sent]
    total = len(submitted)
    unanswered = [op_id for client, _, op_id, _ in submitted if not client.answers[op_id]]
    twice = [op_id for client, _, op_id, _ in submitted if len(client.answers[op_id]) > 1]
    R.check("VAL-SRV-030", total == 2 * N * M and not unanswered and not twice,
            f"{N} 个会话共提交 {total} 条 op（两段各 {N}×{M}），每条在自己的连接上恰有一条回执："
            f"没有回执 {len(unanswered)} 条 {unanswered[:5]}，多于一条 {len(twice)} 条 {twice[:5]}")
    R.check("VAL-SRV-030", bool(a_applied) and bool(a_stale) and not a_other and len(a_applied) + len(a_stale) == N * M,
            f"甲段（抢同一修订）{N * M} 条：生效 {len(a_applied)} 条，因 STALE_BASE_REVISION 被拒 {len(a_stale)} 条"
            f"（都带 op_id 与 rejected），其他回执 {len(a_other)} 条 {a_other[:3]}")
    first = sorted(op_id for op_id in a_applied if op_id.endswith("-00"))
    R.check("VAL-SRV-030", len(first) == 1,
            f"甲段各会话的第一条都基于修订 0 同时发出：{N} 条里恰有一条生效（{first}），其余被拒")
    R.check("VAL-SRV-030", len(b_applied) == N * M and not b_stale and not b_other,
            f"乙段（各写各的）{N * M} 条：生效 {len(b_applied)} 条，被拒 {len(b_stale)} 条，其他回执 {len(b_other)} 条 {b_other[:3]}")

    # ── 修订号 ──────────────────────────────────────────────────────────
    applied = {**a_applied, **b_applied}
    stale = len(a_stale) + len(b_stale)
    revisions = [revision for heard in applied.values() for revision in heard]
    repeated = sorted(revision for revision, times in collections.Counter(revisions).items() if times > 1)
    top = max(revisions) if revisions else None
    R.check("VAL-SRV-030", bool(revisions) and not repeated and sorted(revisions) == list(range(1, len(applied) + 1)),
            f"{len(revisions)} 条 op_applied 的修订号无重复，正好是 1..{len(applied)}：重复的 {repeated[:10]}")
    R.check("VAL-SRV-030", top == total - stale,
            f"最大修订号 {top} = 总提交数 {total} − 因 STALE_BASE_REVISION 被拒的 {stale} 条")
    order = sorted((heard[0], op_id) for op_id, heard in applied.items())
    doubled = sorted((client.ws.name, op_id) for client in clients
                     for op_id, times in collections.Counter(op for _, op in client.heard).items() if times > 1)
    differs = [client.ws.name for client in clients if client.heard != order]
    mixed = {phase: switches(order, phase) for phase in ("a", "b")}
    R.check("setup", mixed["b"] >= N,
            f"各会话的提交确实是交错到达的：按生效顺序，相邻两条来自不同会话的，甲段 {mixed['a']} 处、乙段 {mixed['b']} 处"
            f"（一个会话发完才轮到下一个的话，乙段只会有 {N - 1} 处）")
    R.check("VAL-SRV-030", not doubled and not differs,
            f"每个 op_id 在每条连接上至多一条 op_applied（重复 {len(doubled)} 处 {doubled[:3]}）；{N} 条连接听到的"
            f"序列都是完整的 1..{len(order)}、顺序相同（不同的连接：{differs}）")
    lagged = [line for line in m7h.this_run(srv.err.read_text(encoding="utf-8", errors="replace")).splitlines()
              if "subscriber lagged" in line]
    R.check("VAL-SRV-030", not lagged, f"服务器没有因订阅方落后而丢广播（stderr 里没有 subscriber lagged）：{lagged[:3]}")

    # ── 库里 ────────────────────────────────────────────────────────────
    drawing = srv.sql("select revision from drawings where drawing_id = ?", (DRAWING,))
    R.check("VAL-SRV-030", [row["revision"] for row in drawing] == [top],
            f"select revision from drawings where drawing_id = '{DRAWING}' → {drawing}，等于最大修订号 {top}")
    probes = srv.sql("select object_ref from audit_log where action = ? order by seq", (PROBE,))
    writes = srv.sql("select change_text from audit_log where action = 'set_property' order by seq")
    audited = [row["object_ref"] for row in probes] + [row["change_text"].rsplit(" -> ", 1)[-1] for row in writes]
    R.check("VAL-SRV-030", len(audited) == len(applied) and sorted(audited) == sorted(applied),
            f"audit_log 里的 op 行 {len(audited)} 条（{PROBE} {len(probes)} + set_property {len(writes)}），"
            f"与 {len(applied)} 条生效的 op 一一对应")
    logged = srv.sql("select revision, op_id from ops where drawing_id = ? order by revision", (DRAWING,))
    R.check("VAL-SRV-030", [(row["revision"], row["op_id"]) for row in logged] == order,
            f"操作日志（ops 表）{len(logged)} 行，修订号与 op_id 的对应和各连接听到的一致")

    # ── 乙段：同一会话的写入按发出顺序生效 ───────────────────────────────────
    status, page = srv.http("GET", f"/drawing/{DRAWING}/objects")
    values = {item.get("object_ref"): ((item.get("fields") or {}).get(FIELD) or {}).get("value")
              for item in (page or {}).get("items") or []}
    out_of_order, wrong_last = [], []
    for client in clients:
        mine = [applied.get(op_id, [None])[0] for phase, op_id, _ in client.sent if phase == "乙"]
        if None in mine or mine != sorted(mine):
            out_of_order.append(client.ws.name)
        last = [op_id for phase, op_id, _ in client.sent if phase == "乙"][-1]
        if values.get(client.object) != last:
            wrong_last.append((client.object, values.get(client.object), last))
    R.check("VAL-SRV-030", status == 200 and (page or {}).get("revision") == top and not out_of_order and not wrong_last,
            f"乙段每个会话的 {M} 次写入按发出顺序拿到递增的修订号（乱序的会话：{out_of_order}）；服务器图面"
            f"（修订 {(page or {}).get('revision')}）上 {N} 个对象的{FIELD}都是各自最后一次写入的值（不符：{wrong_last[:3]}）")

    R.dump("s04-submissions.json", [
        {"session": client.ws.name, "phase": phase, "op_id": op_id, "base_revision": base,
         "answers": [{key: msg.get(key) for key in ("type", "code", "revision", "rejected") if key in msg}
                     for msg in client.answers[op_id]]}
        for client, phase, op_id, base in submitted])
    R.dump("s04-counts.json", {
        "sessions": N, "ops_per_session_per_phase": M, "submitted": total,
        "phase_a": {"applied": len(a_applied), "stale": len(a_stale), "other": len(a_other), "seconds": round(took_a, 3)},
        "phase_b": {"applied": len(b_applied), "stale": len(b_stale), "other": len(b_other), "seconds": round(took_b, 3)},
        "switches_between_sessions": mixed, "max_revision": top, "drawings": drawing, "audit_rows": {PROBE: len(probes), "set_property": len(writes)},
        "ops_rows": len(logged), "heard_per_socket": {client.ws.name: len(client.heard) for client in clients},
        "revision_to_op_id": order})
    for client in clients:
        client.ws.close()
finally:
    m7h.cleanup()
    ok = R.summary()
sys.exit(0 if ok else 1)
