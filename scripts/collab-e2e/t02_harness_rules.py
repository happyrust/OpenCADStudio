# -*- coding: utf-8 -*-
"""The harness's own rules, tried on real instances (no collaboration server).

VAL-INFRA-008  a GUI port outside 3210-3239 is refused before anything is
               started; a port that is taken gives way to another port of the
               range; every start writes down its command line and what the
               port check found.
VAL-INFRA-014  an instance that is not ready within its budget is ended, the
               tail of its log is kept, and one more is started on another
               port. Not ready again: that one is ended too, and there is no
               third. (That a healthy instance is ready within the contract's
               60 seconds is t00_smoke.py.)
VAL-INFRA-007  the second way an instance can be gone: its process is there,
               but /api/v1/ready refuses the connection three times in a row.
               How a scenario that lost an instance ends: the first attempt
               with exit code 75 and no failure, the second with a failure.
               And what is not a loss: a request that times out, an instance
               the harness ended itself, an instance that was told to quit.
               (The first way, an exited process, and the rerun on the same
               port and drawing are t03_vanish_retry.py; what the entry does
               with exit code 75 is t04_entry_rerun.py.)

The instances that must not become ready are real ones with a real reason:
their port is bound by somebody who does not listen on it, their drawing does
not exist, or their command line has an option the program does not know.
"""
import contextlib
import ctypes
import ctypes.wintypes as wt
import io
import json
import socket
import sys
import time
import traceback

import m6h

r = m6h.Results("t02_harness_rules")
LOW, HIGH = m6h.gui_port_range()
STARTS = m6h.LOGS / "gui-starts.jsonl"
RETRIES = m6h.LOGS / "gui-retries.jsonl"
BUDGET = 6      # seconds an instance gets here: the contract's 60 through the same code, only shorter

m6h.k32.OpenProcess.restype = wt.HANDLE
m6h.k32.OpenProcess.argtypes = [wt.DWORD, wt.BOOL, wt.DWORD]
sockets = []


def records(path):
    if not path.exists():
        return []
    return [json.loads(line) for line in path.read_text(encoding="utf-8").splitlines() if line.strip()]


def alive(pid):
    """Whether a process with this id is running."""
    handle = m6h.k32.OpenProcess(0x1000, False, pid)        # PROCESS_QUERY_LIMITED_INFORMATION
    if not handle:
        return False
    code = wt.DWORD()
    m6h.k32.GetExitCodeProcess(handle, ctypes.byref(code))
    m6h.k32.CloseHandle(handle)
    return code.value == 259                                # STILL_ACTIVE


def bound(port, listen):
    """A socket of this script on a GUI port: listening, or bound and nothing more."""
    s = socket.socket()
    s.setsockopt(socket.SOL_SOCKET, socket.SO_EXCLUSIVEADDRUSE, 1)
    s.bind(("127.0.0.1", port))
    if listen:
        s.listen(8)
    sockets.append(s)
    return s


def http_port(args):
    return args[args.index("--http") + 1] if "--http" in args and args.index("--http") + 1 < len(args) else None


def request(g, timeout):
    """What one request to the instance ends in, by name."""
    try:
        return f"answered {g.api('GET', '/ready', timeout=timeout)[0]}", None
    except m6h.InstanceVanished as gone:
        return "judged vanished", gone.record
    except Exception as error:  # noqa: BLE001
        return f"{type(error).__name__}: {error}", None


class Probe(m6h.Results):
    """A results object for trying the harness's summary(): it keeps its rows and
    notes to itself and writes one scratch file, no result file and no shared one."""

    def __init__(self):
        self.scenario = "t02-probe"
        self.path = m6h.WORK / "t02-probe.result.txt"
        self.path.write_text("", encoding="utf-8")
        self.rows = []
        self.notes = []
        self.desktop_before = r.desktop_before

    def note(self, text):
        self.notes.append(text)

    def check(self, assertion, ok, detail):
        self.rows.append((assertion, bool(ok), detail))
        return bool(ok)


def ending(attempt):
    """How summary() ends a scenario on this attempt, with what is on the books right now."""
    probe = Probe()
    here = m6h.ATTEMPT
    m6h.ATTEMPT = attempt
    try:
        with contextlib.redirect_stdout(io.StringIO()):
            returned = probe.summary()
        return f"returned {returned}", probe
    except m6h.InstanceVanished as stop:
        return f"exit code {stop.code}", probe
    finally:
        m6h.ATTEMPT = here


try:
    for old in m6h.LOGS.glob("vanished-t02_harness_rules-*.json"):
        old.unlink()
    dwg = m6h.fresh_copy("t02-rules.dwg")

    # ── VAL-INFRA-008: a port outside the range ─────────────────────────────────
    before = len(records(STARTS))
    outcome = {}
    for port in (m6h.GUI_PORTS[0] - 1, m6h.GUI_PORTS[1] + 1, m6h.GUI_PORTS[1] + 21):
        g = m6h.Gui(f"t02-out-{port}", port, dwg=dwg, collab=False)
        try:
            g.start()
            outcome[g.port] = f"起了进程 {g.pid}"
        except ValueError as error:
            outcome[g.port] = "拒绝" if g.process is None and g.pid == 0 else f"拒绝，但已经起了进程 {g.pid}"
            r.note(str(error))
    r.check("VAL-INFRA-008", all(v == "拒绝" for v in outcome.values()) and len(records(STARTS)) == before,
            f"{LOW}–{HIGH} 之外的端口，脚手架在启动之前就拒绝：{outcome}；没有起进程，启动记录也没有多（前后都是 {before} 行）")

    # ── VAL-INFRA-008: a port somebody listens on ───────────────────────────────
    asked = m6h.shifted(3220)
    busy = bound(asked, listen=True)
    g = m6h.Gui("t02-busy", 3220, dwg=dwg, collab=False).start()
    took = g.wait_ready(m6h.READY_BUDGET)
    serving = m6h.lines_with(g.err_text(), "REST listening", f":{g.port}/")
    r.check("VAL-INFRA-008", g.port != asked and LOW <= g.port <= HIGH and g.port_check[:1] == [[asked, "listening"]]
            and g.port_check[-1] == [g.port, "free"] and took is not None and g.ready()[0] == 200 and bool(serving),
            f"要的端口 {asked} 已经有人在监听（本场景自己占的）：启动前查到了，在范围内换成 {g.port}（查的结果 {g.port_check}），"
            f"实例在新端口上就绪（{'%.1f 秒' % took if took is not None else '没有就绪'}，日志里的监听行 {len(serving)} 条）")
    rec = (records(STARTS) or [{}])[-1]
    r.check("VAL-INFRA-008", rec.get("gui") == "t02-busy" and rec.get("pid") == g.pid and rec.get("asked") == asked
            and rec.get("port") == g.port and rec.get("range") == [LOW, HIGH] and rec.get("checked") == g.port_check
            and http_port(rec.get("args") or []) == str(g.port) and bool(rec.get("at")),
            f"这次启动记在 gui-starts.jsonl 里：命令行（--http {http_port(rec.get('args') or [])}）、要的端口 {rec.get('asked')}、"
            f"用的端口 {rec.get('port')}、范围 {rec.get('range')}、端口检查的结果 {rec.get('checked')}、时刻 {rec.get('at')}")
    g.kill()
    busy.close()

    # ── VAL-INFRA-008: two instances started back to back ───────────────────────
    a = m6h.Gui("t02-pair-a", 3221, dwg=m6h.fresh_copy("t02-pair-a.dwg"), collab=False).start()
    b = m6h.Gui("t02-pair-b", 3221, dwg=m6h.fresh_copy("t02-pair-b.dwg"), collab=False).start()
    ta, tb = a.wait_ready(m6h.READY_BUDGET), b.wait_ready(m6h.READY_BUDGET)
    r.check("VAL-INFRA-008", a.port == m6h.shifted(3221) and b.port != a.port and LOW <= b.port <= HIGH
            and b.port_check[:1] == [[a.port, "held by this run"]] and ta is not None and tb is not None
            and a.ready()[0] == 200 and b.ready()[0] == 200,
            f"两个实例前后脚要同一个端口 {a.port}（第一个还没开始监听）：第二个查到它已经被本轮的实例占用，换成 {b.port}"
            f"（查的结果 {b.port_check}），两个都就绪，各答各的 /api/v1/ready")
    a.kill()
    b.kill()

    # ── VAL-INFRA-014: not ready in time, the second start is ───────────────────
    # The port is bound by somebody who does not listen on it: the check finds
    # nobody listening, and the instance cannot serve there.
    first_port = m6h.shifted(3222)
    held = bound(first_port, listen=False)
    retries_before = len(records(RETRIES))
    g = m6h.Gui("t02-late", 3222, dwg=m6h.fresh_copy("t02-late.dwg"), collab=False).start()
    first_pid = g.pid
    began = time.time()
    took = g.wait_ready(BUDGET)
    spent = time.time() - began
    rec = g.retried or {}
    r.check("VAL-INFRA-014", took is not None and g.starts == 2 and rec.get("port") == first_port and rec.get("pid") == first_pid
            and not alive(first_pid) and g.port != first_port and LOW <= g.port <= HIGH and rec.get("retry_port") == g.port
            and g.ready()[0] == 200,
            f"实例在端口 {first_port} 上 {BUDGET} 秒没有就绪（{rec.get('state')}，轮询 {rec.get('polls')} 次）：进程 {first_pid} 被结束，"
            f"换到端口 {g.port} 再起一次，{'%.1f 秒就绪' % took if took is not None else '仍没有就绪'}；一共启动 {g.starts} 次，前后 {spent:.0f} 秒"
            f"（这里的上限是 {BUDGET} 秒，契约的 {m6h.READY_BUDGET} 秒走的是同一段代码）")
    tail = rec.get("log_tail") or []
    reason = [line for line in tail if "cannot serve" in line]
    new = records(RETRIES)[retries_before:]
    r.check("VAL-INFRA-014", bool(reason) and len(new) == 1 and new[0].get("pid") == first_pid and bool(new[0].get("at"))
            and new[0].get("budget_seconds") == BUDGET and str(new[0].get("retry_result", "")).startswith("ready after"),
            f"这次重试记在 gui-retries.jsonl 里（新增 {len(new)} 行）：时刻 {new[0].get('at') if new else '—'}、启动日志尾部 {len(tail)} 行，"
            f"里面有没能就绪的原因（{reason[0][:160] if reason else '没有找到'}）、重试的结果（{new[0].get('retry_result') if new else '—'}）")
    g.kill()
    held.close()

    # ── VAL-INFRA-014: not ready twice, no third start ──────────────────────────
    missing = m6h.WORK / "t02-no-such-drawing.dwg"
    if missing.exists():
        missing.unlink()
    starts_before, retries_before = len(records(STARTS)), len(records(RETRIES))
    g = m6h.Gui("t02-never", 3223, dwg=missing, collab=False).start()
    took = g.wait_ready(BUDGET)
    rec = g.retried or {}
    mine = [s for s in records(STARTS)[starts_before:] if s.get("gui") == "t02-never"]
    r.check("VAL-INFRA-014", took is None and g.starts == 2 and len(mine) == 2 and mine[0].get("port") != mine[1].get("port")
            and not any(alive(s.get("pid")) for s in mine) and g.process is None
            and str(rec.get("retry_result", "")).startswith("not ready either") and "retry_log_tail" in rec
            and len(records(RETRIES)) == retries_before + 1,
            f"图纸不存在的实例一直不就绪（{rec.get('state')}）：第一次在端口 {[s.get('port') for s in mine][:1]}，到时结束、换端口重试一次"
            f"（端口 {[s.get('port') for s in mine][1:2]}），仍不就绪；没有第三次启动（启动记录 {len(mine)} 行），两个进程都已结束，"
            f"wait_ready 返回 {took!r}，记录里写的是「{rec.get('retry_result')}」")

    # ── VAL-INFRA-014: a process that ends before it is ready ───────────────────
    starts_before = len(records(STARTS))
    g = m6h.Gui("t02-exits", 3224, dwg=dwg, collab=False, extra=["--no-such-option"]).start()
    took = g.wait_ready(BUDGET)
    rec = g.retried or {}
    mine = [s for s in records(STARTS)[starts_before:] if s.get("gui") == "t02-exits"]
    said = [line for line in rec.get("log_tail") or [] if "unexpected argument" in line]
    r.check("VAL-INFRA-014", took is None and g.starts == 2 and len(mine) == 2 and mine[0].get("port") != mine[1].get("port")
            and "had ended with code 2" in str(rec.get("state")) and bool(said) and not any(alive(s.get("pid")) for s in mine)
            and str(rec.get("retry_result", "")).startswith("not ready either"),
            f"还没就绪进程就退出的实例（命令行里有不认识的参数；{rec.get('state')}）按同一条规则处理：留下日志尾部"
            f"（{said[0][:120] if said else '没有找到那一行'}），换端口重试一次（{[s.get('port') for s in mine]}），仍不就绪，不起第三个")

    # ── VAL-INFRA-007: the process is there, /ready is refused three times ──────
    g = m6h.Gui("t02-refused", 3225, dwg=m6h.fresh_copy("t02-refused.dwg"), collab=False).start()
    r.check("setup", g.wait_ready(m6h.READY_BUDGET) is not None, "实例就绪")
    real = g.port
    nobody = next(p for p in range(HIGH, LOW - 1, -1) if p != real and not m6h.port_listening(p))
    g.port = nobody     # from here the harness asks a port nobody listens on: the process lives, /ready is refused
    probes = []
    probe = g.ready_refused
    g.ready_refused = lambda: probes.append(probe()) or probes[-1]
    began = time.time()
    outcome, lost = request(g, 5)
    spent = time.time() - began
    g.port = real
    del g.ready_refused
    lost = lost or {}
    files = sorted(m6h.LOGS.glob("vanished-t02_harness_rules-*.json"))
    on_disk = json.loads(files[-1].read_text(encoding="utf-8")) if files else {}
    r.check("VAL-INFRA-007", outcome == "judged vanished" and str(lost.get("how", "")).startswith("/api/v1/ready refused the connection 3 times")
            and probes == [True, True, True] and lost.get("exit_code") is None and g.running() and len(files) == 1
            and on_disk.get("gui") == "t02-refused"
            and bool(on_disk.get("at")) and bool(on_disk.get("log_tail")) and g.ready()[0] == 200,
            f"进程还在、/api/v1/ready 连续 3 次被拒绝（做法：让脚手架去问本机一个没人监听的端口 {nobody}）：请求的结局是「{outcome}」，"
            f"发现请求失败后探测了 {len(probes)} 次 /api/v1/ready，被拒绝的情况 {probes}，前后 {spent:.0f} 秒；记录文件 {len(files)} 个（{files[-1].name if files else '—'}），写的是「{on_disk.get('how')}」、"
            f"时刻 {on_disk.get('at')}、日志尾部 {len(on_disk.get('log_tail') or [])} 行、退出码 {on_disk.get('exit_code')!r}（进程没有退出）")

    # ── VAL-INFRA-007: how a scenario that lost an instance ends ────────────────
    # The harness's own summary(), with the loss above still on its books.
    try:
        on_first, first = ending(1)
        on_second, second = ending(2)
    finally:
        # The loss was provoked on purpose: not a reason to run this scenario again.
        m6h._vanished.clear()
        g.gone = None
    calm, _ = ending(1)
    leaving = f"exit code {m6h.VANISHED_EXIT}"
    r.check("VAL-INFRA-007", on_first == leaving and not first.rows and any("runs the scenario once more" in n for n in first.notes)
            and on_second == leaving and [row[:2] for row in second.rows] == [("harness", False)]
            and "not run a third time" in second.rows[0][2] and calm == "returned True",
            f"丢了实例的场景怎么收场（拿上一条留下的那条记录，在只写一个草稿文件的结果对象上试脚手架的 summary()）：第一次尝试 → {on_first}，"
            f"不记 FAIL（{len(first.rows)} 条），留一句说明等入口重跑；第二次尝试 → {on_second}，记一条 FAIL"
            f"（{second.rows[0][2][:150] if second.rows else '没有记'}）；账上没有丢实例时 → {calm}")

    # ── VAL-INFRA-007: what is not a loss ───────────────────────────────────────
    frozen = g.suspend()
    timed_out, _ = request(g, 3)
    g.kill()
    ended, _ = request(g, 5)
    q = m6h.Gui("t02-quit", 3226, dwg=m6h.fresh_copy("t02-quit.dwg"), collab=False).start()
    r.check("setup", q.wait_ready(m6h.READY_BUDGET) is not None, "第二个实例就绪")
    try:
        q.run("QUIT")
        quitting = "answered"
    except m6h.InstanceVanished:
        quitting = "judged vanished"
    except Exception as error:  # noqa: BLE001
        quitting = f"响应没有读到（{type(error).__name__}）"
    left = q.wait_exit(15)
    told, _ = request(q, 5)
    files = sorted(m6h.LOGS.glob("vanished-t02_harness_rules-*.json"))
    quiet = ("answered", "judged vanished")
    r.check("VAL-INFRA-007", frozen and not timed_out.startswith(quiet) and not ended.startswith(quiet)
            and left and quitting != "judged vanished" and not told.startswith(quiet)
            and not m6h._vanished and len(files) == 1,
            f"不算消失的三种情形没有被误判：冻结的实例，请求超时（{timed_out[:60]}）；脚手架自己结束的实例，连接被拒绝（{ended[:60]}）；"
            f"被告知 QUIT 的实例（QUIT 请求：{quitting}，进程{'已退出' if left else '没有退出'}），退出后连接被拒绝（{told[:60]}）。"
            f"三次都没有新增记录（记录文件仍是 {len(files)} 个）")
except SystemExit:
    raise
except Exception as error:  # noqa: BLE001
    traceback.print_exc()
    r.check("script", False, f"场景没有跑完：{type(error).__name__}: {error}")
finally:
    for s in sockets:
        s.close()
    m6h.cleanup()
    ok = r.summary()
sys.exit(0 if ok else 1)
