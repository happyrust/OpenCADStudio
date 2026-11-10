#!/usr/bin/env python3
"""Run the real editor where nobody is working: on a Windows desktop of its
own.

A window on another desktop is never shown on the one a person is using. It
cannot take the foreground there, and nothing that happens to it moves the
person's pointer. The editor still draws with the GPU exactly as it would on
screen, so what `capture` returns is what a user would see.

The editor is driven through its loopback REST channel (`--http <port>`):
command lines, state, widget bounds and screenshots. There is no pointer on
the hidden desktop to click with, and mouse messages posted to the window do
not reach the widgets, so every step has to be something a command can do.

    from hidden_desktop import HiddenEditor, desktop_state, same_desktop, start_drawing

    out = "target/ui-audit/run"
    before = desktop_state()
    with HiddenEditor("target/debug/OpenCADStudio.exe", 8231, out, [start_drawing(out)]) as editor:
        editor.wait_ready()
        editor.find_window()
        editor.command("LINE 0,0 100,50")
        editor.capture("line")
    print(same_desktop(before, desktop_state()))

Several editors run side by side: give each its own port (and with it its own
desktop, settings folder and output folder). `hidden_steps.py` drives one or
more of them from a list of steps without any code.

Windows only; the standard library is enough (Pillow, when installed, lets
`crop` cut a capture to one widget). See `guides/hidden-desktop.md`.
"""

import ctypes
import ctypes.wintypes as wt
import json
import os
import queue
import secrets
import threading
import time
import urllib.error
import urllib.request
from pathlib import Path

k32 = ctypes.WinDLL("kernel32", use_last_error=True)
u32 = ctypes.WinDLL("user32", use_last_error=True)

GENERIC_ALL = 0x10000000
GENERIC_WRITE = 0x40000000
CREATE_SUSPENDED = 0x00000004
CREATE_UNICODE_ENVIRONMENT = 0x00000400
STARTF_USESTDHANDLES = 0x00000100
WM_CLOSE = 0x0010
JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE = 0x2000
JOB_OBJECT_EXTENDED_LIMIT_INFORMATION = 9


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


class JOBOBJECT_BASIC_LIMIT_INFORMATION(ctypes.Structure):
    _fields_ = [
        ("PerProcessUserTimeLimit", ctypes.c_int64), ("PerJobUserTimeLimit", ctypes.c_int64),
        ("LimitFlags", wt.DWORD), ("MinimumWorkingSetSize", ctypes.c_size_t),
        ("MaximumWorkingSetSize", ctypes.c_size_t), ("ActiveProcessLimit", wt.DWORD),
        ("Affinity", ctypes.c_size_t), ("PriorityClass", wt.DWORD), ("SchedulingClass", wt.DWORD),
    ]


class IO_COUNTERS(ctypes.Structure):
    _fields_ = [(name, ctypes.c_uint64) for name in (
        "ReadOperationCount", "WriteOperationCount", "OtherOperationCount",
        "ReadTransferCount", "WriteTransferCount", "OtherTransferCount")]


class JOBOBJECT_EXTENDED_LIMIT_INFORMATION(ctypes.Structure):
    _fields_ = [
        ("BasicLimitInformation", JOBOBJECT_BASIC_LIMIT_INFORMATION), ("IoInfo", IO_COUNTERS),
        ("ProcessMemoryLimit", ctypes.c_size_t), ("JobMemoryLimit", ctypes.c_size_t),
        ("PeakProcessMemoryUsed", ctypes.c_size_t), ("PeakJobMemoryUsed", ctypes.c_size_t),
    ]


class LASTINPUTINFO(ctypes.Structure):
    _fields_ = [("cbSize", wt.UINT), ("dwTime", wt.DWORD)]


u32.CreateDesktopW.restype = wt.HANDLE
u32.CreateDesktopW.argtypes = [wt.LPCWSTR, wt.LPCWSTR, ctypes.c_void_p, wt.DWORD, wt.DWORD, ctypes.c_void_p]
u32.CloseDesktop.argtypes = [wt.HANDLE]
u32.SetThreadDesktop.argtypes = [wt.HANDLE]
u32.SetThreadDesktop.restype = wt.BOOL
u32.PostMessageW.argtypes = [wt.HWND, wt.UINT, wt.WPARAM, wt.LPARAM]
u32.PostMessageW.restype = wt.BOOL
u32.GetForegroundWindow.restype = wt.HWND
u32.GetWindowTextW.argtypes = [wt.HWND, wt.LPWSTR, ctypes.c_int]
u32.GetWindowThreadProcessId.argtypes = [wt.HWND, ctypes.POINTER(wt.DWORD)]
u32.GetClientRect.argtypes = [wt.HWND, ctypes.POINTER(wt.RECT)]
u32.IsWindowVisible.argtypes = [wt.HWND]
u32.GetDpiForWindow.argtypes = [wt.HWND]
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
k32.ResumeThread.argtypes = [wt.HANDLE]
k32.CreateJobObjectW.restype = wt.HANDLE
k32.CreateJobObjectW.argtypes = [ctypes.c_void_p, wt.LPCWSTR]
k32.SetInformationJobObject.argtypes = [wt.HANDLE, ctypes.c_int, ctypes.c_void_p, wt.DWORD]
k32.AssignProcessToJobObject.argtypes = [wt.HANDLE, wt.HANDLE]
k32.GetTickCount.restype = wt.DWORD

ENUM_PROC = ctypes.WINFUNCTYPE(wt.BOOL, wt.HWND, wt.LPARAM)
u32.EnumDesktopWindows.argtypes = [wt.HANDLE, ENUM_PROC, wt.LPARAM]


def desktop_state():
    """What the desktop a person is using shows right now: where the pointer
    is, which window has the foreground, and when a key or the mouse was last
    touched. Taken before and after a run, the two say whether the run reached
    that desktop (see `same_desktop`). Read-only."""
    point = wt.POINT()
    u32.GetCursorPos(ctypes.byref(point))
    foreground = u32.GetForegroundWindow()
    title = ctypes.create_unicode_buffer(200)
    u32.GetWindowTextW(foreground, title, 200)
    owner = wt.DWORD()
    u32.GetWindowThreadProcessId(foreground, ctypes.byref(owner))
    info = LASTINPUTINFO()
    info.cbSize = ctypes.sizeof(info)
    u32.GetLastInputInfo(ctypes.byref(info))
    return {
        "pointer": (point.x, point.y),
        "foreground": int(foreground or 0),
        "foreground_title": title.value,
        "foreground_pid": owner.value,
        "last_input_tick": info.dwTime,
        "seconds_since_input": round(((k32.GetTickCount() - info.dwTime) & 0xFFFFFFFF) / 1000.0, 1),
    }


def same_desktop(before, after):
    """One line saying whether the pointer and the foreground window are where
    they were. When they are not, the last-input time tells a person at the
    machine (it moved on) from something the run did (it did not)."""
    moved = before["pointer"] != after["pointer"]
    switched = before["foreground"] != after["foreground"]
    if not moved and not switched:
        return (f"desktop untouched: pointer still at {after['pointer']}, foreground still "
                f"{after['foreground']:#x} \"{after['foreground_title'][:40]}\"")
    what = " and ".join(part for part, changed in (
        (f"pointer {before['pointer']} -> {after['pointer']}", moved),
        (f"foreground \"{before['foreground_title'][:30]}\" -> \"{after['foreground_title'][:30]}\"", switched),
    ) if changed)
    who = ("a person used the machine meanwhile (input was registered)"
           if before["last_input_tick"] != after["last_input_tick"]
           else "NO input was registered meanwhile: the run itself did this")
    return f"desktop CHANGED: {what}; {who}"


def start_drawing(folder):
    """A drawing to start the editor on, for a run that has none of its own:
    a copy of a small fixture of this repository, in `folder`, so that
    nothing is ever written beside the fixture itself."""
    import shutil
    source = Path(__file__).resolve().parents[2] / "tests" / "fixtures" / "plot" / "page-setups-metric.dxf"
    folder = Path(folder).resolve()
    folder.mkdir(parents=True, exist_ok=True)
    return Path(shutil.copyfile(source, folder / "start.dxf"))


def _quote(arg):
    arg = str(arg)
    if arg and not any(ch in arg for ch in ' \t"'):
        return arg
    return '"' + arg.replace('"', '\\"') + '"'


class _DesktopThread:
    """One thread attached to the hidden desktop: calls about the editor's
    window (finding it, closing it) have to come from there."""

    def __init__(self, hdesk):
        self.jobs = queue.Queue()
        self.attached = None
        self.thread = threading.Thread(target=self._run, args=(hdesk,), daemon=True)
        self.thread.start()
        self.call(lambda: None)

    def _run(self, hdesk):
        self.attached = bool(u32.SetThreadDesktop(hdesk))
        while True:
            job = self.jobs.get()
            if job is None:
                return
            function, done = job
            try:
                done.put((True, function()))
            except BaseException as error:  # noqa: BLE001 - handed to the caller
                done.put((False, error))

    def call(self, function):
        done = queue.Queue()
        self.jobs.put((function, done))
        ok, value = done.get()
        if not ok:
            raise value
        return value

    def stop(self):
        self.jobs.put(None)


class HiddenEditor:
    """One editor process on a desktop of its own.

    `exe`      the editor (a debug build draws the same and starts faster)
    `port`     its REST port; also what tells several editors apart
    `out`      receives the captures, `editor-stderr.log` and `report.txt`
    `args`     the rest of the command line. It has to name a drawing:
               `--http` without one starts the headless server, which has no
               window. `--backend vulkan`, `--collab ...` and the like go
               here too
    `appdata`  the editor's settings folder (`%APPDATA%`); by default one
               under `out`, so the person's own settings, recent files and
               session list are neither read nor written
    `temp`     `%TEMP%` for the editor, when the default drive is short
    `env`      more environment variables
    """

    def __init__(self, exe, port, out, args=(), appdata=None, temp=None, env=None, desktop=None):
        self.exe = Path(exe).resolve()
        self.port = int(port)
        self.out = Path(out).resolve()
        self.out.mkdir(parents=True, exist_ok=True)
        # The editor runs in `out`: a drawing named relative to here is not
        # there, and an editor told to open a missing file never gets ready.
        self.args = [
            str(Path(arg).resolve()) if not str(arg).startswith("-") and Path(arg).is_file() else str(arg)
            for arg in args
        ] + ["--http", str(self.port)]
        self.appdata = Path(appdata).resolve() if appdata else self.out / "appdata"
        self.temp = Path(temp).resolve() if temp else None
        self.env = dict(env or {})
        self.token = secrets.token_hex(16)
        self.desktop_name = desktop or f"ocs-hidden-{os.getpid()}-{self.port}"
        self.hdesk = self.job = self.process = self.window_thread = None
        self.pid = 0
        self.hwnd = None
        self.scale = 1.0
        self.log = []
        self.shots = 0

    def __enter__(self):
        self.start()
        return self

    def __exit__(self, *_):
        self.close()

    def say(self, text):
        print(text, flush=True)
        self.log.append(text)

    # ── the process ──────────────────────────────────────────────────────
    def start(self):
        self.hdesk = u32.CreateDesktopW(self.desktop_name, None, None, 0, GENERIC_ALL, None)
        if not self.hdesk:
            raise OSError(f"CreateDesktopW failed: {ctypes.get_last_error()}")
        env = dict(os.environ)
        env.update(self.env)
        env["OCS_API_TOKEN"] = self.token
        self.appdata.mkdir(parents=True, exist_ok=True)
        env["APPDATA"] = str(self.appdata)
        if self.temp:
            self.temp.mkdir(parents=True, exist_ok=True)
            env["TEMP"] = env["TMP"] = str(self.temp)
        block = "".join(f"{key}={value}\0" for key, value in sorted(env.items())) + "\0"
        inherit = SECURITY_ATTRIBUTES(ctypes.sizeof(SECURITY_ATTRIBUTES), None, True)
        self.log_path = self.out / "editor-stderr.log"
        log_handle = k32.CreateFileW(str(self.log_path), GENERIC_WRITE, 0x3, ctypes.byref(inherit), 2, 0x80, None)
        startup = STARTUPINFOW()
        startup.cb = ctypes.sizeof(STARTUPINFOW)
        startup.lpDesktop = self.desktop_name
        startup.dwFlags = STARTF_USESTDHANDLES
        startup.hStdOutput = startup.hStdError = log_handle
        info = PROCESS_INFORMATION()
        line = ctypes.create_unicode_buffer(" ".join(_quote(part) for part in [self.exe] + self.args))
        self.launched = time.time()
        ok = k32.CreateProcessW(
            str(self.exe), line, None, None, True, CREATE_UNICODE_ENVIRONMENT | CREATE_SUSPENDED,
            ctypes.c_wchar_p(block), str(self.out), ctypes.byref(startup), ctypes.byref(info),
        )
        k32.CloseHandle(log_handle)
        if not ok:
            error = ctypes.get_last_error()
            u32.CloseDesktop(self.hdesk)
            self.hdesk = None
            raise OSError(f"CreateProcessW failed for {self.exe}: {error}")
        self.process = info.hProcess
        self.pid = info.dwProcessId
        # The editor and whatever it starts die with this script, however the
        # script ends: nobody could see a leftover window to close it.
        self.job = k32.CreateJobObjectW(None, None)
        limits = JOBOBJECT_EXTENDED_LIMIT_INFORMATION()
        limits.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE
        tied = bool(
            self.job
            and k32.SetInformationJobObject(
                self.job, JOB_OBJECT_EXTENDED_LIMIT_INFORMATION, ctypes.byref(limits), ctypes.sizeof(limits))
            and k32.AssignProcessToJobObject(self.job, self.process)
        )
        k32.ResumeThread(info.hThread)
        k32.CloseHandle(info.hThread)
        self.window_thread = _DesktopThread(self.hdesk)
        self.say(
            f"editor pid {self.pid} on desktop \"{self.desktop_name}\", REST port {self.port} "
            f"(ends with this script: {tied}; window thread attached: {self.window_thread.attached})"
        )

    def running(self):
        return self.process is not None and k32.WaitForSingleObject(self.process, 0) != 0

    def exit_code(self):
        code = wt.DWORD()
        k32.GetExitCodeProcess(self.process, ctypes.byref(code))
        return code.value

    def close(self, wait=6.0):
        """Close the window as its close button would; end the process if it
        does not go (it asks whether to save when a drawing was changed, and
        nobody is there to answer)."""
        how = "was gone already"
        if self.running():
            if self.hwnd:
                self.window_thread.call(lambda: u32.PostMessageW(self.hwnd, WM_CLOSE, 0, 0))
                if k32.WaitForSingleObject(self.process, int(wait * 1000)) == 0:
                    how = "closed by WM_CLOSE"
            if self.running():
                k32.TerminateProcess(self.process, 1)
                k32.WaitForSingleObject(self.process, 20000)
                how = "did not close (an unsaved drawing asks to be saved) and was ended"
        if self.process:
            self.say(f"editor pid {self.pid} {how}, exit code {self.exit_code()}")
            k32.CloseHandle(self.process)
            self.process = None
        if self.job:
            k32.CloseHandle(self.job)
            self.job = None
        if self.window_thread:
            self.window_thread.stop()
            self.window_thread = None
        if self.hdesk:
            u32.CloseDesktop(self.hdesk)
            self.hdesk = None
        (self.out / "report.txt").write_text("\n".join(self.log) + "\n", encoding="utf-8")

    # ── the REST channel ─────────────────────────────────────────────────
    def call(self, method, path, body=None, timeout=120):
        data = json.dumps(body).encode() if body is not None else None
        request = urllib.request.Request(
            f"http://127.0.0.1:{self.port}/api/v1{path}", data=data, method=method,
            headers={"Content-Type": "application/json", "Authorization": f"Bearer {self.token}"},
        )
        try:
            with urllib.request.urlopen(request, timeout=timeout) as response:
                raw = response.read()
                return response.status, (json.loads(raw) if raw else None)
        except urllib.error.HTTPError as error:
            raw = error.read()
            try:
                return error.code, json.loads(raw)
            except ValueError:
                return error.code, {"raw": raw.decode("utf-8", "replace")}

    def wait_ready(self, timeout=180):
        deadline = time.time() + timeout
        while time.time() < deadline:
            if not self.running():
                raise SystemExit(f"the editor exited early with {self.exit_code()}; see {self.log_path}")
            try:
                status, body = self.call("GET", "/ready", timeout=5)
                if status == 200:
                    self.say(f"editor ready in {time.time() - self.launched:.1f}s: version {body.get('version')}")
                    return body
            except (urllib.error.URLError, ConnectionError, OSError):
                pass
            time.sleep(0.5)
        raise SystemExit("the editor never became ready")

    def state(self):
        return self.call("GET", "/state")[1] or {}

    def document(self):
        state = self.state()
        return (state.get("result") or state).get("document_id")

    def documents(self):
        state = self.state()
        return state.get("documents") or (state.get("result") or {}).get("documents") or []

    def op(self, name, body=None):
        """One operation of the automation API, on the active document unless
        `body` names another."""
        body = dict(body or {})
        body.setdefault("document_id", self.document())
        return self.call("POST", f"/{name}", body)

    def command(self, line):
        """A command line, as typed: `LINE 0,0 10,0`, `PIDBACKUPOPEN D:\\backup`."""
        status, body = self.op("commands", {"cmd": line})
        self.say(f"command {line!r}: HTTP {status} {(body or {}).get('status')} {(body or {}).get('error')}")
        return status, body

    def capture(self, name, scope="window"):
        """What the editor draws, as `<out>/<nn>-<name>.png`: the whole window,
        or the drawing area alone with `scope="viewport"`."""
        self.shots += 1
        path = self.out / f"{self.shots:02d}-{name}.png"
        if path.exists():
            path.unlink()
        status, body = self.op("capture", {"path": str(path), "scope": scope, "max_dimension": 4096})
        ok = status == 200 and path.exists()
        self.say(f"  [capture] {path.name} {'ok' if ok else f'FAILED {status} {body}'}")
        return path if ok else None

    def bounds(self, prefix):
        """The widgets that report where they are, by id, as `[x, y, w, h]` in
        logical pixels of the window."""
        _, body = self.op("ui_bounds", {"prefix": prefix})
        widgets = (body or {}).get("widgets") or ((body or {}).get("result") or {}).get("widgets") or []
        return {widget["id"]: widget["bounds"] for widget in widgets}

    def crop(self, name, widget, margin=(4, 30, 4, 4)):
        """`capture`, cut to the widget with this id (left, top, right and
        bottom margins in logical pixels; the top default leaves room for a
        panel's title bar). Needs Pillow; without it the whole window stays."""
        path = self.capture(name)
        box = self.bounds(widget).get(widget)
        if not path or not box:
            self.say(f"  [crop] {name}: no bounds for \"{widget}\"")
            return path
        try:
            from PIL import Image
        except ImportError:
            self.say("  [crop] Pillow is not installed: the capture is the whole window")
            return path
        image = Image.open(path)
        # The capture is in physical pixels, the bounds in logical ones.
        left = max(int((box[0] - margin[0]) * self.scale), 0)
        top = max(int((box[1] - margin[1]) * self.scale), 0)
        right = min(int((box[0] + box[2] + margin[2]) * self.scale), image.width)
        bottom = min(int((box[1] + box[3] + margin[3]) * self.scale), image.height)
        cut = path.with_name(f"{path.stem}-{widget}.png")
        image.crop((left, top, right, bottom)).save(cut)
        self.say(f"  [crop] {cut.name} {right - left}x{bottom - top}")
        return cut

    # ── the window, seen from the thread on its desktop ──────────────────
    def find_window(self, timeout=90):
        """The editor's main window: needed to close it, and for the scale
        between the logical pixels of `bounds` and the pixels of a capture."""
        def look():
            found = []

            @ENUM_PROC
            def each(hwnd, _):
                owner = wt.DWORD()
                u32.GetWindowThreadProcessId(hwnd, ctypes.byref(owner))
                if owner.value == self.pid and u32.IsWindowVisible(hwnd):
                    rect = wt.RECT()
                    u32.GetClientRect(hwnd, ctypes.byref(rect))
                    title = ctypes.create_unicode_buffer(200)
                    u32.GetWindowTextW(hwnd, title, 200)
                    found.append((rect.right * rect.bottom, hwnd, rect.right, rect.bottom, title.value))
                return True

            u32.EnumDesktopWindows(self.hdesk, each, 0)
            return found

        deadline = time.time() + timeout
        while time.time() < deadline:
            found = self.window_thread.call(look)
            large = [item for item in found if item[2] > 200 and item[3] > 200]
            if large:
                _, hwnd, width, height, title = max(large)
                self.hwnd = hwnd
                self.scale = (self.window_thread.call(lambda: u32.GetDpiForWindow(hwnd)) or 96) / 96.0
                self.say(f"window {hwnd:#x} \"{title}\" client {width}x{height} px, scale {self.scale}")
                return hwnd
            if not self.running():
                raise SystemExit(f"the editor exited with {self.exit_code()}; see {self.log_path}")
            time.sleep(0.5)
        raise SystemExit("the editor window never appeared on its desktop")
