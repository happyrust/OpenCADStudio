# -*- coding: utf-8 -*-
"""M6（VAL-CLI）端到端验证的脚手架。

协同服务器、GUI 实例、WS 观察端和 SQLite 读数都从这里起。约束：

* 不动鼠标指针、不抢前台焦点：GUI 起在自己单独的 Windows 桌面上（CreateDesktop），
  它的窗口从不出现在人正在用的桌面上；全程只走 `--http` 的本机 REST 通道。
* GUI 带 `--new-instance`：既不把图纸转交给已经在跑的编辑器，也不去占单实例端口
  （否则别人双击图纸会被转到这个看不见的实例里）。
* 每个实例用自己的 APPDATA / TEMP，不碰用户的 settings.json。
* 端口只用 3210-3259：协同服务器 3240（或给定端口），GUI 3210-3239。要和同一台
  机器上别的在跑的实例错开时，用 OCS_E2E_PORT_SHIFT 把整段平移（`shifted()`）。
* 结束时把自己起的进程全部关掉（`cleanup()`）。

路径不写死，由本文件所在的位置和环境变量决定：

* OCS_E2E_EVIDENCE_DIR  证据目录（日志、截图、数据库、图纸副本）。必须给，而且
                        必须在仓库之外；本脚手架用它下面的 m6 子目录。
* OCS_E2E_BIN_DIR       OpenCADStudio.exe 和 ocs_collab_server.exe 所在的目录，
                        不给时是仓库的 target/debug。
* OCS_E2E_BASE_DWG      场景拿来复制的基础图，不给时是仓库里的
                        tests/fixtures/plot/page-setups-metric.dwg。
* OCS_E2E_PORT_SHIFT    加在每个端口上的平移量，不给时是 0。
* OCS_E2E_EXTRA_ARGS    追加给每个 GUI 实例的命令行参数。

只用标准库。
"""
import base64
import ctypes
import ctypes.wintypes as wt
import json
import os
import secrets
import shutil
import socket
import sqlite3
import struct
import subprocess
import sys
import threading
import time
import urllib.error
import urllib.request
from pathlib import Path

if hasattr(sys.stdout, "reconfigure"):
    sys.stdout.reconfigure(encoding="utf-8")

HERE = Path(__file__).resolve().parent
REPO = HERE.parents[1]


def _evidence_dir():
    """Where logs, screenshots, databases and drawing copies go: never the repository."""
    given = os.environ.get("OCS_E2E_EVIDENCE_DIR")
    if not given:
        raise SystemExit("OCS_E2E_EVIDENCE_DIR is not set: name a directory outside the repository "
                         "for the logs, screenshots and databases (README.md, section 3.2)")
    path = Path(given).resolve()
    if path == REPO or REPO in path.parents:
        raise SystemExit(f"OCS_E2E_EVIDENCE_DIR is inside the repository ({path}); it has to be outside")
    return path


ROOT = _evidence_dir() / "m6"
LOGS = ROOT / "logs"
SHOTS = ROOT / "shots"
WORK = ROOT / "work"
BIN_DIR = Path(os.environ.get("OCS_E2E_BIN_DIR") or REPO / "target" / "debug")
EXE_GUI = BIN_DIR / "OpenCADStudio.exe"
EXE_SRV = BIN_DIR / "ocs_collab_server.exe"
BASE_DWG = Path(os.environ.get("OCS_E2E_BASE_DWG") or REPO / "tests" / "fixtures" / "plot" / "page-setups-metric.dwg")
SEED_JSON = HERE / "seed-val-cli.json"
PORT_SHIFT = int(os.environ.get("OCS_E2E_PORT_SHIFT") or 0)


def shifted(port):
    """A port of the contract's range (3210-3259), moved by OCS_E2E_PORT_SHIFT."""
    return port + PORT_SHIFT

for _d in (ROOT, LOGS, SHOTS, WORK):
    _d.mkdir(parents=True, exist_ok=True)

k32 = ctypes.WinDLL("kernel32", use_last_error=True)
u32 = ctypes.WinDLL("user32", use_last_error=True)

GENERIC_ALL = 0x10000000
CREATE_UNICODE_ENVIRONMENT = 0x00000400
CREATE_NO_WINDOW = 0x08000000
STARTF_USESTDHANDLES = 0x00000100


class STARTUPINFOW(ctypes.Structure):
    _fields_ = [
        ("cb", wt.DWORD), ("lpReserved", wt.LPWSTR), ("lpDesktop", wt.LPWSTR), ("lpTitle", wt.LPWSTR),
        ("dwX", wt.DWORD), ("dwY", wt.DWORD), ("dwXSize", wt.DWORD), ("dwYSize", wt.DWORD),
        ("dwXCountChars", wt.DWORD), ("dwYCountChars", wt.DWORD), ("dwFillAttribute", wt.DWORD),
        ("dwFlags", wt.DWORD), ("wShowWindow", wt.WORD), ("cbReserved2", wt.WORD),
        ("lpReserved2", ctypes.c_void_p),
        ("hStdInput", wt.HANDLE), ("hStdOutput", wt.HANDLE), ("hStdError", wt.HANDLE),
    ]


class PROCESS_INFORMATION(ctypes.Structure):
    _fields_ = [("hProcess", wt.HANDLE), ("hThread", wt.HANDLE), ("dwProcessId", wt.DWORD), ("dwThreadId", wt.DWORD)]


class SECURITY_ATTRIBUTES(ctypes.Structure):
    _fields_ = [("nLength", wt.DWORD), ("lpSecurityDescriptor", ctypes.c_void_p), ("bInheritHandle", wt.BOOL)]


u32.CreateDesktopW.restype = wt.HANDLE
u32.CreateDesktopW.argtypes = [wt.LPCWSTR, wt.LPCWSTR, ctypes.c_void_p, wt.DWORD, wt.DWORD, ctypes.c_void_p]
u32.CloseDesktop.argtypes = [wt.HANDLE]
k32.CreateFileW.restype = wt.HANDLE
k32.CreateFileW.argtypes = [wt.LPCWSTR, wt.DWORD, wt.DWORD, ctypes.c_void_p, wt.DWORD, wt.DWORD, wt.HANDLE]
k32.CreateProcessW.argtypes = [
    wt.LPCWSTR, wt.LPWSTR, ctypes.c_void_p, ctypes.c_void_p, wt.BOOL, wt.DWORD,
    ctypes.c_void_p, wt.LPCWSTR, ctypes.POINTER(STARTUPINFOW), ctypes.POINTER(PROCESS_INFORMATION),
]
k32.CreateProcessW.restype = wt.BOOL
k32.WaitForSingleObject.argtypes = [wt.HANDLE, wt.DWORD]
k32.TerminateProcess.argtypes = [wt.HANDLE, wt.UINT]
k32.GetExitCodeProcess.argtypes = [wt.HANDLE, ctypes.POINTER(wt.DWORD)]
k32.CloseHandle.argtypes = [wt.HANDLE]
k32.OpenThread.restype = wt.HANDLE
k32.OpenThread.argtypes = [wt.DWORD, wt.BOOL, wt.DWORD]
k32.SuspendThread.argtypes = [wt.HANDLE]
k32.ResumeThread.argtypes = [wt.HANDLE]
k32.CreateJobObjectW.restype = wt.HANDLE
k32.CreateJobObjectW.argtypes = [ctypes.c_void_p, wt.LPCWSTR]
k32.SetInformationJobObject.argtypes = [wt.HANDLE, ctypes.c_int, ctypes.c_void_p, wt.DWORD]
k32.AssignProcessToJobObject.argtypes = [wt.HANDLE, wt.HANDLE]
k32.AssignProcessToJobObject.restype = wt.BOOL

_started = []          # everything this process started, for cleanup()
_desktop = {"name": None, "handle": None}

# The repository's own hidden-desktop library (merged from the other line at
# 412b1d3f): its read-only probe of the desktop a person is using tells, before
# and after a run, whether the pointer or the foreground window moved.
REPO_AUTOMATION = REPO / "docs" / "automation"
sys.path.insert(0, str(REPO_AUTOMATION))
import hidden_desktop as hd  # noqa: E402

_job = {"handle": None}


def job():
    """A job object that kills every process in it when this script ends, however
    it ends: a window left behind on a hidden desktop is one nobody can see."""
    if _job["handle"] is None:
        handle = k32.CreateJobObjectW(None, None)
        info = hd.JOBOBJECT_EXTENDED_LIMIT_INFORMATION()
        info.BasicLimitInformation.LimitFlags = 0x2000  # JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE
        # 9 = JobObjectExtendedLimitInformation
        if not k32.SetInformationJobObject(handle, 9, ctypes.byref(info), ctypes.sizeof(info)):
            raise OSError(f"SetInformationJobObject failed: {ctypes.get_last_error()}")
        _job["handle"] = handle
    return _job["handle"]


def now():
    return time.strftime("%H:%M:%S")


def say(text):
    print(f"{now()} {text}", flush=True)


def quote(arg):
    arg = str(arg)
    if arg and not any(ch in arg for ch in ' \t"'):
        return arg
    return '"' + arg.replace('"', '\\"') + '"'


def hidden_desktop():
    """The one desktop every GUI instance of this run lives on."""
    if _desktop["handle"] is None:
        name = f"ocs-m6val-{os.getpid()}"
        handle = u32.CreateDesktopW(name, None, None, 0, GENERIC_ALL, None)
        if not handle:
            raise OSError(f"CreateDesktopW failed: {ctypes.get_last_error()}")
        _desktop["name"], _desktop["handle"] = name, handle
    return _desktop["name"]


# ── collaboration server ─────────────────────────────────────────────────
class Server:
    def __init__(self, port=3240, db=None, seed=SEED_JSON, heartbeat_timeout=None, claim_ttl=None, tag="srv"):
        self.port = shifted(port)
        self.db = Path(db) if db else WORK / f"{tag}.db"
        self.seed = seed
        self.heartbeat_timeout = heartbeat_timeout
        self.claim_ttl = claim_ttl
        self.tag = tag
        self.out = LOGS / f"{tag}.out.log"
        self.err = LOGS / f"{tag}.err.log"
        self.proc = None
        self.starts = 0

    def reset_db(self):
        for suffix in ("", "-shm", "-wal"):
            p = Path(str(self.db) + suffix)
            if p.exists():
                p.unlink()

    def start(self, fresh=False, with_seed=True):
        if fresh:
            self.reset_db()
        args = [str(EXE_SRV), "--port", str(self.port), "--db", str(self.db)]
        if with_seed and self.seed:
            args += ["--seed", str(self.seed)]
        if self.heartbeat_timeout is not None:
            args += ["--heartbeat-timeout", str(self.heartbeat_timeout)]
        if self.claim_ttl is not None:
            args += ["--claim-ttl", str(self.claim_ttl)]
        self.starts += 1
        mode = "ab"
        fo, fe = open(self.out, mode), open(self.err, mode)
        marker = f"\n### {time.strftime('%Y-%m-%dT%H:%M:%S')} start #{self.starts}: {' '.join(args)}\n".encode("utf-8")
        fo.write(marker); fo.flush(); fe.write(marker); fe.flush()
        self.proc = subprocess.Popen(args, stdout=fo, stderr=fe, creationflags=CREATE_NO_WINDOW, cwd=str(WORK))
        fo.close(); fe.close()
        k32.AssignProcessToJobObject(job(), wt.HANDLE(int(self.proc._handle)))
        _started.append(("server", self.proc.pid, self))
        say(f"server pid={self.proc.pid} port={self.port} db={self.db.name}")
        return self

    def health(self, timeout=6):
        try:
            with urllib.request.urlopen(f"http://127.0.0.1:{self.port}/health", timeout=timeout) as r:
                return r.status, json.loads(r.read() or b"{}")
        except urllib.error.HTTPError as e:
            return e.code, {}
        except Exception as e:  # noqa: BLE001
            return 0, {"error": str(e)}

    def wait_health(self, seconds=20):
        deadline = time.time() + seconds
        while time.time() < deadline:
            status, _ = self.health()
            if status == 200:
                return True
            if self.proc.poll() is not None:
                return False
            time.sleep(0.25)
        return False

    def http(self, method, path, body=None, timeout=10):
        data = json.dumps(body).encode("utf-8") if body is not None else None
        req = urllib.request.Request(f"http://127.0.0.1:{self.port}{path}", data=data, method=method,
                                     headers={"Content-Type": "application/json"})
        try:
            with urllib.request.urlopen(req, timeout=timeout) as r:
                raw = r.read()
                return r.status, (json.loads(raw) if raw else None)
        except urllib.error.HTTPError as e:
            raw = e.read()
            try:
                return e.code, json.loads(raw)
            except ValueError:
                return e.code, {"raw": raw.decode("utf-8", "replace")}

    def running(self):
        return self.proc is not None and self.proc.poll() is None

    def kill(self):
        """Hard kill: no close frames go out (`Stop-Process -Force`)."""
        if self.running():
            self.proc.kill()
            self.proc.wait(10)
            say(f"server pid={self.proc.pid} killed")

    def log_text(self):
        return self.out.read_text(encoding="utf-8", errors="replace") if self.out.exists() else ""

    def sql(self, query, params=()):
        return sql(self.db, query, params)


def sql(db, query, params=()):
    """Read-only query against a database the server has open (WAL)."""
    con = sqlite3.connect(f"file:{Path(db).as_posix()}?mode=ro", uri=True, timeout=10)
    try:
        cur = con.execute(query, params)
        cols = [c[0] for c in cur.description] if cur.description else []
        return [dict(zip(cols, row)) for row in cur.fetchall()]
    finally:
        con.close()


# ── GUI instance on the hidden desktop ───────────────────────────────────
class Gui:
    def __init__(self, name, port, dwg=None, user=None, drawing="PID-1101", collab_port=3240,
                 collab=True, mode=None, extra=(), backend="vulkan"):
        self.name = name
        self.port = shifted(port)
        self.dwg = Path(dwg) if dwg else None
        self.user = user
        self.drawing = drawing
        self.collab_port = shifted(collab_port)
        self.collab = collab
        self.mode = mode
        self.extra = list(extra)
        self.backend = backend
        self.token = "m6val-" + secrets.token_hex(8)
        self.appdata = WORK / "appdata" / name
        self.temp = WORK / "temp" / name
        self.err = LOGS / f"gui-{name}.err.log"
        self.process = None
        self.pid = 0
        self.thread_id = 0
        self.launched = 0.0
        self.starts = 0

    def args(self):
        a = []
        if self.dwg:
            a.append(str(self.dwg))
        a += ["--http", str(self.port), "--new-instance"]
        if self.backend:
            a += ["--backend", self.backend]
        if self.collab:
            a += ["--collab", f"ws://127.0.0.1:{self.collab_port}/ws"]
            if self.user:
                a += ["--user", self.user]
            if self.drawing:
                a += ["--drawing", self.drawing]
            if self.mode:
                a += ["--collab-mode", self.mode]
        return a + self.extra + os.environ.get("OCS_E2E_EXTRA_ARGS", "").split()

    def descriptors(self):
        """The automation-channel descriptor files under this instance's APPDATA."""
        return sorted((self.appdata / "OpenCADStudio" / "automation").glob("*.json"))

    def drop_descriptors(self):
        """Delete them (2026-11-11, review N8): an instance that quits removes its
        own, one that cleanup() kills leaves it behind. Paths only, never contents."""
        for path in self.descriptors():
            try:
                path.unlink()
            except OSError:
                pass

    def start(self):
        desktop = hidden_desktop()
        self.appdata.mkdir(parents=True, exist_ok=True)
        self.drop_descriptors()
        self.temp.mkdir(parents=True, exist_ok=True)
        env = dict(os.environ)
        env["OCS_API_TOKEN"] = self.token
        env["APPDATA"] = str(self.appdata)
        # 2026-11-11：协同客户端把每一帧收发的原文写进 stderr（c1db1a03d，审查 N9）。
        env["OCS_COLLAB_LOG"] = "frames"
        env["TEMP"] = env["TMP"] = str(self.temp)
        block = "".join(f"{k}={v}\0" for k, v in sorted(env.items())) + "\0"
        attributes = SECURITY_ATTRIBUTES(ctypes.sizeof(SECURITY_ATTRIBUTES), None, True)
        self.starts += 1
        with open(self.err, "ab") as f:
            f.write(f"\n### {time.strftime('%Y-%m-%dT%H:%M:%S')} start #{self.starts}: {' '.join(self.args())}\n".encode("utf-8"))
        # FILE_APPEND_DATA, share read/write, OPEN_ALWAYS
        handle = k32.CreateFileW(str(self.err), 0x0004, 0x3, ctypes.byref(attributes), 4, 0x80, None)
        startup = STARTUPINFOW()
        startup.cb = ctypes.sizeof(STARTUPINFOW)
        startup.lpDesktop = desktop
        startup.dwFlags = STARTF_USESTDHANDLES
        startup.hStdOutput = handle
        startup.hStdError = handle
        info = PROCESS_INFORMATION()
        line = ctypes.create_unicode_buffer(" ".join(quote(p) for p in [EXE_GUI] + self.args()))
        self.launched = time.time()
        ok = k32.CreateProcessW(str(EXE_GUI), line, None, None, True, CREATE_UNICODE_ENVIRONMENT,
                                ctypes.c_wchar_p(block), str(WORK), ctypes.byref(startup), ctypes.byref(info))
        k32.CloseHandle(handle)
        if not ok:
            raise OSError(f"CreateProcessW failed: {ctypes.get_last_error()}")
        self.process = info.hProcess
        self.pid = info.dwProcessId
        self.thread_id = info.dwThreadId
        k32.CloseHandle(info.hThread)
        k32.AssignProcessToJobObject(job(), self.process)
        _started.append(("gui", self.pid, self))
        say(f"gui {self.name} pid={self.pid} port={self.port} desktop={desktop} args={' '.join(self.args())}")
        return self

    def running(self):
        return self.process is not None and k32.WaitForSingleObject(self.process, 0) != 0

    def exit_code(self):
        code = wt.DWORD()
        k32.GetExitCodeProcess(self.process, ctypes.byref(code))
        return code.value

    def wait_exit(self, seconds):
        if self.process is None:
            return True
        return k32.WaitForSingleObject(self.process, int(seconds * 1000)) == 0

    def kill(self):
        if self.running():
            k32.TerminateProcess(self.process, 1)
            k32.WaitForSingleObject(self.process, 20000)
            say(f"gui {self.name} pid={self.pid} terminated")
        if self.process:
            k32.CloseHandle(self.process)
            self.process = None

    def suspend(self):
        """Freeze every thread of the process (the contract's `Suspend()`).

        The socket stays open and nothing more is sent on it: no heartbeat, no
        close frame. `TerminateProcess` still ends a suspended process, so
        `kill()` and the job object clean it up like any other.
        """
        ntdll = ctypes.WinDLL("ntdll")
        ntdll.NtSuspendProcess.argtypes = [wt.HANDLE]
        ntdll.NtSuspendProcess.restype = ctypes.c_long
        status = ntdll.NtSuspendProcess(self.process)
        say(f"gui {self.name} pid={self.pid}: NtSuspendProcess -> {status:#x}")
        return status == 0
    # ── REST ──
    def api(self, method, path, body=None, timeout=60):
        data = json.dumps(body).encode("utf-8") if body is not None else None
        req = urllib.request.Request(
            f"http://127.0.0.1:{self.port}/api/v1{path}", data=data, method=method,
            headers={"Content-Type": "application/json", "Authorization": f"Bearer {self.token}"})
        try:
            with urllib.request.urlopen(req, timeout=timeout) as r:
                raw = r.read()
                return r.status, (json.loads(raw) if raw else None)
        except urllib.error.HTTPError as e:
            raw = e.read()
            try:
                return e.code, json.loads(raw)
            except ValueError:
                return e.code, {"raw": raw.decode("utf-8", "replace")}

    def ready(self, timeout=5):
        try:
            return self.api("GET", "/ready", timeout=timeout)
        except Exception as e:  # noqa: BLE001
            return 0, {"error": str(e)}

    def wait_ready(self, seconds=120):
        deadline = time.time() + seconds
        while time.time() < deadline:
            if not self.running():
                say(f"gui {self.name} exited early: code {self.exit_code()}")
                return None
            status, body = self.ready()
            if status == 200:
                elapsed = time.time() - self.launched
                say(f"gui {self.name} ready in {elapsed:.1f}s")
                return elapsed
            time.sleep(0.4)
        return None

    def state(self):
        return self.api("GET", "/state")[1] or {}

    def doc_id(self):
        """The document that has a path (the drawing), else the active one."""
        st = self.state()
        docs = st.get("documents") or (st.get("result") or {}).get("documents") or []
        for d in docs:
            if d.get("path"):
                return d["id"]
        return st.get("document_id") or (st.get("result") or {}).get("document_id")

    def run(self, cmd, doc=None, **more):
        body = {"document_id": doc if doc is not None else self.doc_id(), "cmd": cmd}
        body.update(more)
        return self.api("POST", "/run", body)

    def op(self, name, doc=None, **fields):
        body = {"document_id": doc if doc is not None else self.doc_id(), "op": name}
        body.update(fields)
        return self.api("POST", "/run", body)

    def status(self):
        return self.api("GET", "/collab/status")[1] or {}

    def wait_status(self, pred, seconds=20, step=0.2):
        deadline = time.time() + seconds
        last = {}
        while time.time() < deadline:
            last = self.status()
            try:
                if pred(last):
                    return last
            except Exception:  # noqa: BLE001
                pass
            time.sleep(step)
        return None

    def xdata(self, handle, app="PID_AUTHOR"):
        return self.api("GET", f"/entities/{handle}/xdata?app={app}")

    def author(self, handle):
        """PID_AUTHOR of one entity as a dict of its key=value strings."""
        status, body = self.xdata(handle)
        out = {}
        for item in (body or {}).get("items") or []:
            for value in (item.get("xdata") or {}).get("PID_AUTHOR") or []:
                text = value.get("value") if isinstance(value, dict) else value
                if isinstance(text, str) and "=" in text:
                    k, v = text.split("=", 1)
                    out[k] = v
        return out

    def inserts(self):
        status, body = self.op("query", type="Insert", detail="full")
        return (body or {}).get("entities") or ((body or {}).get("result") or {}).get("entities") or []

    def tagged(self):
        """{tag: handle} for every symbol that carries a PID_AUTHOR tag."""
        out = {}
        for e in self.inserts():
            h = e.get("handle")
            a = self.author(h)
            if a.get("tag"):
                out[a["tag"]] = h
        return out

    def capture(self, path, scope="window", wait=True, **more):
        path = Path(path)
        if path.exists():
            path.unlink()
        body = {"document_id": self.doc_id(), "path": str(path), "scope": scope, "wait_for_frame": wait}
        body.update(more)
        status, resp = self.api("POST", "/capture", body, timeout=120)
        return status, resp, path.exists()

    def ui_bounds(self, prefix):
        status, body = self.op("ui_bounds", prefix=prefix)
        widgets = (body or {}).get("widgets") or ((body or {}).get("result") or {}).get("widgets") or []
        return {w["id"]: w["bounds"] for w in widgets}

    def err_text(self):
        return self.err.read_text(encoding="utf-8", errors="replace") if self.err.exists() else ""


# ── minimal RFC 6455 client: the third observer on the wire ──────────────
class Ws:
    def __init__(self, url, name):
        assert url.startswith("ws://")
        rest = url[5:]
        hostport, _, path = rest.partition("/")
        host, _, port = hostport.partition(":")
        self.name = name
        self.path = "/" + path
        self.sock = socket.create_connection((host, int(port or 80)), timeout=10)
        key = base64.b64encode(os.urandom(16)).decode()
        req = (f"GET {self.path} HTTP/1.1\r\nHost: {hostport}\r\nUpgrade: websocket\r\nConnection: Upgrade\r\n"
               f"Sec-WebSocket-Key: {key}\r\nSec-WebSocket-Version: 13\r\n\r\n")
        self.sock.sendall(req.encode())
        head = b""
        while b"\r\n\r\n" not in head:
            chunk = self.sock.recv(4096)
            if not chunk:
                raise ConnectionError("handshake closed")
            head += chunk
        if b" 101 " not in head.split(b"\r\n", 1)[0]:
            raise ConnectionError(f"handshake refused: {head[:120]!r}")
        self.buf = head.split(b"\r\n\r\n", 1)[1]
        self.frames = []       # {"t": epoch, "dir": "in"/"out", "msg": obj}
        self.closed = False
        self.close_reason = None
        self.lock = threading.Lock()
        self.log = LOGS / f"ws-{name}.jsonl"
        self.log.write_text("", encoding="utf-8")
        self.sock.settimeout(0.5)
        self.thread = threading.Thread(target=self._reader, daemon=True)
        self.thread.start()

    def _record(self, direction, msg):
        entry = {"t": round(time.time(), 3), "at": now(), "dir": direction, "msg": msg}
        with self.lock:
            self.frames.append(entry)
            with open(self.log, "a", encoding="utf-8") as f:
                f.write(json.dumps(entry, ensure_ascii=False) + "\n")

    def _need(self, n):
        while len(self.buf) < n:
            try:
                chunk = self.sock.recv(65536)
            except socket.timeout:
                if self.closed:
                    raise ConnectionError("closed")
                continue
            if not chunk:
                raise ConnectionError("peer closed")
            self.buf += chunk
        out, self.buf = self.buf[:n], self.buf[n:]
        return out

    def _reader(self):
        try:
            while not self.closed:
                b0, b1 = self._need(2)
                opcode = b0 & 0x0F
                length = b1 & 0x7F
                if length == 126:
                    length = struct.unpack(">H", self._need(2))[0]
                elif length == 127:
                    length = struct.unpack(">Q", self._need(8))[0]
                mask = self._need(4) if b1 & 0x80 else None
                payload = self._need(length)
                if mask:
                    payload = bytes(c ^ mask[i % 4] for i, c in enumerate(payload))
                if opcode == 0x1:
                    try:
                        self._record("in", json.loads(payload.decode("utf-8")))
                    except ValueError:
                        self._record("in", {"raw": payload.decode("utf-8", "replace")})
                elif opcode == 0x8:
                    self._record("in", {"type": "<close>"})
                    self.close_reason = "close frame"
                    self.closed = True
                elif opcode == 0x9:
                    self._send_frame(0xA, payload)
        except Exception as e:  # noqa: BLE001
            if not self.closed:
                self.close_reason = f"{type(e).__name__}: {e}"
                self._record("in", {"type": "<socket-ended>", "why": self.close_reason})
            self.closed = True

    def _send_frame(self, opcode, payload):
        mask = os.urandom(4)
        head = bytes([0x80 | opcode])
        n = len(payload)
        if n < 126:
            head += bytes([0x80 | n])
        elif n < 65536:
            head += bytes([0x80 | 126]) + struct.pack(">H", n)
        else:
            head += bytes([0x80 | 127]) + struct.pack(">Q", n)
        self.sock.sendall(head + mask + bytes(c ^ mask[i % 4] for i, c in enumerate(payload)))

    def send(self, obj):
        self._record("out", obj)
        self._send_frame(0x1, json.dumps(obj, ensure_ascii=False).encode("utf-8"))

    def incoming(self, type_=None):
        with self.lock:
            return [f["msg"] for f in self.frames
                    if f["dir"] == "in" and (type_ is None or f["msg"].get("type") == type_)]

    def wait_for(self, pred, seconds=10):
        deadline = time.time() + seconds
        while time.time() < deadline:
            with self.lock:
                for f in self.frames:
                    if f["dir"] == "in":
                        try:
                            if pred(f["msg"]):
                                return f["msg"]
                        except Exception:  # noqa: BLE001
                            pass
            if self.closed:
                break
            time.sleep(0.05)
        return None

    def hello(self, user, drawing="PID-1101", mode="协同编辑", version="m6val-1"):
        self.send({"type": "hello", "user_id": user, "drawing_id": drawing, "mode": mode, "client_version": version})
        welcome = self.wait_for(lambda m: m.get("type") in ("welcome", "error"), 10)
        self.session_id = (welcome or {}).get("session_id")
        return welcome

    def heartbeat(self):
        self.send({"type": "heartbeat", "session_id": self.session_id})

    def keep_alive(self, every=10.0):
        """Send heartbeats in the background so the observer stays a live session."""
        def beat():
            while not self.closed:
                time.sleep(every)
                if self.closed:
                    return
                try:
                    self.heartbeat()
                except Exception:  # noqa: BLE001
                    return
        threading.Thread(target=beat, daemon=True).start()

    def close(self):
        if not self.closed:
            try:
                self._send_frame(0x8, b"")
            except Exception:  # noqa: BLE001
                pass
        self.closed = True
        try:
            self.sock.close()
        except Exception:  # noqa: BLE001
            pass


# ── reading logs ─────────────────────────────────────────────────────────
def this_run(text):
    """The part of an appended log written since the last `### … start` marker."""
    cut = text.rfind("\n### ")
    return text[cut + 1:] if cut >= 0 else text


def lines_with(text, *needles):
    """Lines of this run's log that contain every needle."""
    return [line for line in this_run(text).splitlines() if all(n in line for n in needles)]


def frames(text, direction="<-", type_=None):
    """The frames a GUI wrote to its log since its last start, as they were on
    the wire (`OCS_COLLAB_LOG=frames`): `<-` received, `->` sent."""
    out = []
    marker = f"帧 {direction} "
    for line in this_run(text).splitlines():
        at = line.find(marker)
        if at < 0:
            continue
        try:
            frame = json.loads(line[at + len(marker):])
        except ValueError:
            continue
        if type_ is None or frame.get("type") == type_:
            out.append(frame)
    return out


def wait_until(pred, seconds=15, step=0.2):
    """Poll `pred` until it returns something truthy; that value, or None on timeout."""
    deadline = time.time() + seconds
    while time.time() < deadline:
        try:
            value = pred()
        except Exception:  # noqa: BLE001
            value = None
        if value:
            return value
        time.sleep(step)
    return None


# ── bookkeeping ──────────────────────────────────────────────────────────
def fresh_copy(name, source=None):
    """A fresh copy of a drawing under WORK, with stale lock/recovery files gone.

    `source` defaults to the base drawing (no symbols); the two-instance
    scenarios pass the seed built by `t01_seed.py` (one vessel, P-1001).
    """
    target = WORK / name
    for stale in list(WORK.glob(f"{target.name}.ocs*")) + list(WORK.glob(f".{target.name}*")):
        stale.unlink()
    shutil.copyfile(source or BASE_DWG, target)
    return target


SEED_DWG = WORK / "PID-1101-seed.dwg"


def port_listening(port):
    s = socket.socket()
    s.settimeout(0.3)
    try:
        s.connect(("127.0.0.1", port))
        return True
    except OSError:
        return False
    finally:
        s.close()


def cleanup():
    """Stop everything this process started, newest first, and drop the desktop."""
    for kind, pid, obj in reversed(_started):
        try:
            obj.kill()
            if kind == "gui":
                obj.drop_descriptors()
        except Exception as e:  # noqa: BLE001
            say(f"cleanup: {kind} pid={pid}: {e}")
    _started.clear()
    if _desktop["handle"]:
        u32.CloseDesktop(_desktop["handle"])
        _desktop["handle"] = None


class Results:
    """PASS/FAIL lines of one scenario, appended to its own log and to the shared results file."""

    def __init__(self, scenario):
        self.scenario = scenario
        self.path = LOGS / f"{scenario}.result.txt"
        self.path.write_text(f"=== {scenario} {time.strftime('%Y-%m-%dT%H:%M:%S')} ===\n", encoding="utf-8")
        self.rows = []
        # Where the pointer and the foreground window were before anything ran.
        self.desktop_before = hd.desktop_state()

    def note(self, text):
        say(f"      {text}")
        with open(self.path, "a", encoding="utf-8") as f:
            f.write(f"      {text}\n")

    def check(self, assertion, ok, detail):
        verdict = "PASS" if ok else "FAIL"
        line = f"{verdict} {assertion} {detail}"
        say(line)
        self.rows.append((assertion, bool(ok), detail))
        with open(self.path, "a", encoding="utf-8") as f:
            f.write(line + "\n")
        with open(ROOT / "results.jsonl", "a", encoding="utf-8") as f:
            f.write(json.dumps({"at": time.strftime("%Y-%m-%dT%H:%M:%S"), "scenario": self.scenario,
                                "assertion": assertion, "ok": bool(ok), "detail": detail}, ensure_ascii=False) + "\n")
        return bool(ok)

    def dump(self, name, obj):
        """Write one piece of evidence under LOGS and return its path."""
        p = LOGS / name
        if isinstance(obj, (dict, list)):
            p.write_text(json.dumps(obj, ensure_ascii=False, indent=1), encoding="utf-8")
        else:
            p.write_text(str(obj), encoding="utf-8")
        return p

    def summary(self):
        passed = sum(1 for _, ok, _ in self.rows if ok)
        failed = sum(1 for _, ok, _ in self.rows if not ok)
        # Whether the run reached the desktop a person is using: the pointer
        # and the foreground window, before and after.
        self.note(hd.same_desktop(self.desktop_before, hd.desktop_state()))
        line = f"=== {self.scenario}: {passed} PASS / {failed} FAIL ==="
        say(line)
        with open(self.path, "a", encoding="utf-8") as f:
            f.write(line + "\n")
        return failed == 0
