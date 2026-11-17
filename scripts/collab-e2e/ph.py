# -*- coding: utf-8 -*-
"""What the timing scenarios (p01, p02) share, on top of m7h.

`timed_post()` is the contract's way of timing one request: curl.exe sends it
and reports its own `time_total`, the wall clock of that one HTTP round trip,
with nothing of the start of a process in it. What curl is to send (the
address, the body, the token of the instance) is given to it as a config file
on its standard input, so none of it is on a command line or in a file.

`surroundings()` is what the contract wants looked at when a timing is taken:
how many compilers are running, and how many instances of the two programs
other than the scenario's own. It reads names and ids off the process list,
nothing else.
"""
import ctypes
import ctypes.wintypes as wt
import json
import shutil
import subprocess

import m7h
import yh

CURL = shutil.which("curl.exe")
MARK = "\n#curl# "       # what curl writes between the answer and its own figures


# ── one request, timed by curl ───────────────────────────────────────────
def curl_version():
    """The first line of `curl --version`, or why there is none."""
    if not CURL:
        return "curl.exe: not on the PATH"
    said = subprocess.run([CURL, "--version"], capture_output=True, timeout=30).stdout.decode("utf-8", "replace")
    return (said.splitlines() or ["curl.exe: said nothing"])[0].strip()


def _quoted(text):
    """A value of a curl config file, in double quotes."""
    return '"' + text.replace("\\", "\\\\").replace('"', '\\"') + '"'


def timed_post(gui, body, limit=60):
    """POST /api/v1/run through curl.exe. Returns (seconds, HTTP status, answer);
    the seconds are curl's own `time_total` of this one request."""
    config = "\n".join([
        "url = " + _quoted(f"http://127.0.0.1:{gui.port}/api/v1/run"),
        "header = " + _quoted("Content-Type: application/json"),
        "header = " + _quoted(f"Authorization: Bearer {gui.token}"),
        "data = " + _quoted(json.dumps(body)),
        "silent",
        "show-error",
        f"max-time = {limit}",
        'write-out = "\\n#curl# %{time_total} %{http_code}"',
        ""])
    done = subprocess.run([CURL, "--config", "-"], input=config.encode("utf-8"), capture_output=True, timeout=limit + 30)
    raw, mark, figures = done.stdout.decode("utf-8", "replace").rpartition(MARK)
    parts = figures.split()
    if done.returncode != 0 or not mark or len(parts) != 2:
        gui.api("GET", "/ready", timeout=5)     # an instance that is gone is the harness's to tell (VAL-INFRA-007)
        why = done.stderr.decode("utf-8", "replace").strip()[:200] or "nothing on its error output"
        raise RuntimeError(f"curl ended with code {done.returncode}: {why}")
    try:
        answer = json.loads(raw) if raw.strip() else None
    except ValueError:
        answer = {"raw": raw[:400]}
    return float(parts[0]), int(parts[1]), answer


# ── what runs beside a timing ────────────────────────────────────────────
class _Entry(ctypes.Structure):     # PROCESSENTRY32W
    _fields_ = [("dwSize", wt.DWORD), ("cntUsage", wt.DWORD), ("th32ProcessID", wt.DWORD),
                ("th32DefaultHeapID", ctypes.c_size_t), ("th32ModuleID", wt.DWORD), ("cntThreads", wt.DWORD),
                ("th32ParentProcessID", wt.DWORD), ("pcPriClassBase", wt.LONG), ("dwFlags", wt.DWORD),
                ("szExeFile", wt.WCHAR * 260)]


_k32 = ctypes.WinDLL("kernel32", use_last_error=True)
_k32.CreateToolhelp32Snapshot.restype = wt.HANDLE
_k32.CreateToolhelp32Snapshot.argtypes = [wt.DWORD, wt.DWORD]
_k32.Process32FirstW.argtypes = [wt.HANDLE, ctypes.POINTER(_Entry)]
_k32.Process32NextW.argtypes = [wt.HANDLE, ctypes.POINTER(_Entry)]
_k32.CloseHandle.argtypes = [wt.HANDLE]


def processes():
    """(id, name of the executable in lower case) of every process."""
    snapshot = _k32.CreateToolhelp32Snapshot(0x00000002, 0)         # TH32CS_SNAPPROCESS
    if snapshot is None or snapshot == ctypes.c_void_p(-1).value:
        raise OSError(f"CreateToolhelp32Snapshot failed: {ctypes.get_last_error()}")
    entry = _Entry()
    entry.dwSize = ctypes.sizeof(_Entry)
    out = []
    try:
        more = _k32.Process32FirstW(snapshot, ctypes.byref(entry))
        while more:
            out.append((entry.th32ProcessID, entry.szExeFile.lower()))
            more = _k32.Process32NextW(snapshot, ctypes.byref(entry))
    finally:
        _k32.CloseHandle(snapshot)
    return out


def surroundings(own=()):
    """What runs beside a timing: compilers, and instances of the two programs
    that are not `own` (the process ids of the scenario's own instances)."""
    own = set(own)
    gui, server = m7h.EXE_GUI.name.lower(), m7h.EXE_SRV.name.lower()
    listed = processes()
    return {"rustc": sum(1 for _, name in listed if name == "rustc.exe"),
            "gui": sum(1 for pid, name in listed if name == gui and pid not in own),
            "server": sum(1 for pid, name in listed if name == server and pid not in own)}


def quiet(*readings):
    """Nothing of what the contract wants absent was there, in any of the readings."""
    return all(not any(reading.values()) for reading in readings)


def told(reading):
    return f"rustc {reading['rustc']} 个、别的 GUI 实例 {reading['gui']} 个、别的服务器实例 {reading['server']} 个"


def beside(before, after):
    """The two readings around a timing, in the words of a check."""
    state = "契约要的干净环境" if quiet(before, after) else "不是契约要的干净环境"
    return f"量之前 {told(before)}，量之后 {told(after)}（{state}：三样都要是 0）"


# ── a drawing to time in ─────────────────────────────────────────────────
def new_document(gui):
    """A new, empty drawing, made the active one. Returns its id."""
    def listed():
        return [d.get("id") for d in (gui.api("GET", "/documents")[1] or {}).get("documents") or []]
    known = set(listed())
    made = gui.api("POST", "/documents", {})
    added = [d for d in listed() if d not in known]
    if made[0] not in (200, 201) or len(added) != 1:
        raise RuntimeError(f"POST /documents answered {made[0]} and the list has {len(added)} more drawing(s), not 1")
    went = gui.op("activate", added[0])
    if not yh.ok(went) or gui.state().get("document_id") != added[0]:
        raise RuntimeError(f"the new drawing did not become the active one: activate answered {went[0]}")
    return added[0]
