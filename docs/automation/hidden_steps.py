#!/usr/bin/env python3
"""Drive the real editor, on a desktop of its own, through a list of steps,
and keep what its window draws along the way.

    python docs/automation/hidden_steps.py <editor exe> <out dir> <steps.json>
        [--port 8231] [--instances 1] [--open DRAWING] [--backend vulkan]
        [--editor-args "..."] [--editor-args-N "..."]
        [--temp DIR] [--env NAME=VALUE ...]

Without `--open` each editor starts on a copy of a small fixture drawing.
`--editor-args` adds to the command line of every editor, `--editor-args-1`
to that of editor 1 only: two clients of one collaboration session differ
in their `--user`.

Nothing reaches the desktop a person is using: the editor's windows are not
shown there, the pointer is not moved and no focus is taken. The script
checks that and says so in its last line. See `hidden_desktop.py` for how,
and `guides/hidden-desktop.md` for what the method can and cannot show.

The steps are a JSON list. Each is `[kind, argument...]`:

    ["cmd", "LINE 0,0 100,50"]            a command line, as typed
    ["op", "undo", {}]                    any operation of the REST API
    ["wait", 1.5]                         seconds
    ["capture", "name"]                   the window, as <out>/<nn>-<name>.png
    ["capture", "name", "viewport"]       the drawing area alone
    ["crop", "name", "pid-backup-panel"]  the window cut to one widget (Pillow)
    ["state", "why"]                      the documents: title and dirty flag
    ["history", 8]                        the last lines of the command line
    ["say", "text"]                       a line in the report

and, for the P&ID backup panel of this fork:

    ["select", "备份文件 > Export.dmp"]    PIDBACKUPOPEN Select <labels>
    ["panel", "name"]                     ["crop", name, "pid-backup-panel"]

With `--instances N` there are N editors, on ports `--port`, `--port`+1, ...,
each on its own desktop with its own settings folder and its own
`<out>/<n>` folder. A step that starts with a number goes to that editor
(`[1, "cmd", "ZOOM E"]`); the others go to editor 0.
"""

import json
import shlex
import sys
import time
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
from hidden_desktop import HiddenEditor, desktop_state, same_desktop, start_drawing  # noqa: E402


def option(name, default=None):
    return sys.argv[sys.argv.index(name) + 1] if name in sys.argv else default


def options(name):
    return [sys.argv[at + 1] for at, arg in enumerate(sys.argv[:-1]) if arg == name]


def run_step(editor, step):
    kind, rest = step[0], step[1:]
    if kind == "cmd":
        editor.command(rest[0])
        time.sleep(0.8)
    elif kind == "op":
        status, body = editor.op(rest[0], rest[1] if len(rest) > 1 else {})
        text = json.dumps(body, ensure_ascii=False)
        editor.say(f"op {rest[0]}: HTTP {status} {text[:300]}")
    elif kind == "select":
        editor.command(f"PIDBACKUPOPEN Select {rest[0]}")
        time.sleep(0.9)
    elif kind == "wait":
        time.sleep(rest[0])
    elif kind == "capture":
        editor.capture(rest[0], rest[1] if len(rest) > 1 else "window")
    elif kind == "crop":
        editor.crop(rest[0], rest[1])
    elif kind == "panel":
        editor.crop(rest[0], "pid-backup-panel")
    elif kind == "state":
        flags = [(doc.get("title"), doc.get("dirty")) for doc in editor.documents()]
        editor.say(f"documents ({rest[0] if rest else ''}): {flags}")
    elif kind == "history":
        _, body = editor.op("history", {})
        for entry in ((body or {}).get("entries") or [])[-int(rest[0]):]:
            editor.say(f"  command line [{entry.get('kind')}]: {str(entry.get('text'))[:200]}")
    elif kind == "say":
        editor.say(rest[0])
    else:
        raise SystemExit(f"unknown step {step}")


def main():
    if len(sys.argv) < 4:
        raise SystemExit(__doc__)
    exe, out, steps_path = Path(sys.argv[1]), Path(sys.argv[2]).resolve(), Path(sys.argv[3])
    steps = json.loads(steps_path.read_text(encoding="utf-8"))
    port = int(option("--port", "8231"))
    count = int(option("--instances", "1"))
    args = []
    if option("--backend"):
        args += ["--backend", option("--backend")]
    env = dict(pair.split("=", 1) for pair in options("--env"))

    before = desktop_state()
    print(f"before: pointer {before['pointer']}, foreground \"{before['foreground_title'][:50]}\", "
          f"last input {before['seconds_since_input']}s ago", flush=True)
    editors = []
    try:
        for at in range(count):
            folder = out / str(at) if count > 1 else out
            # The editor has a window only when it is given a drawing.
            drawing = Path(option("--open")).resolve() if option("--open") else start_drawing(folder)
            own = shlex.split(option("--editor-args", ""), posix=False)
            own += shlex.split(option(f"--editor-args-{at}", ""), posix=False)
            editor = HiddenEditor(exe, port + at, folder, [drawing] + args + own, temp=option("--temp"), env=env)
            editors.append(editor)
            editor.start()
        for editor in editors:
            editor.wait_ready()
            editor.find_window()
        time.sleep(1.5)
        for step in steps:
            at = 0
            if isinstance(step[0], int):
                at, step = step[0], step[1:]
            run_step(editors[at], step)
    finally:
        for editor in editors:
            editor.close()
    verdict = same_desktop(before, desktop_state())
    print(verdict, flush=True)
    if verdict.startswith("desktop CHANGED") and "NO input" in verdict:
        raise SystemExit(2)


if __name__ == "__main__":
    sys.stdout.reconfigure(encoding="utf-8")
    main()
