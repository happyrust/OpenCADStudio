"""Smoke tests for scripts/pid-platform/browser.mjs.

These tests launch a real headless Chrome via the zero-dependency CDP
harness in browser.mjs. They skip (never fail) when node or Chrome is
absent from the environment.
"""
from __future__ import annotations

import json
import os
import shutil
import subprocess
from pathlib import Path

import pytest

from conftest import PROTO, REPO_ROOT

BROWSER = REPO_ROOT / "scripts" / "pid-platform" / "browser.mjs"
PROTO_INDEX = PROTO / "index.html"

_NODE = shutil.which("node")


def _chrome_path() -> Path | None:
    candidates = []
    env = os.environ.get("CHROME_PATH")
    if env:
        candidates.append(Path(env))
    candidates.append(
        Path(r"C:\Program Files\Google\Chrome\Application\chrome.exe")
    )
    candidates.append(
        Path(r"C:\Program Files (x86)\Google\Chrome\Application\chrome.exe")
    )
    local = os.environ.get("LOCALAPPDATA")
    if local:
        candidates.append(
            Path(local) / "Google" / "Chrome" / "Application" / "chrome.exe"
        )
    for candidate in candidates:
        if candidate.is_file():
            return candidate
    return None


_CHROME = _chrome_path()


def _missing_prereq():
    if _NODE is None:
        return "node executable not found on PATH"
    if not BROWSER.is_file():
        return "browser.mjs not found at " + str(BROWSER)
    return None


_PREREQ_PROBLEM = _missing_prereq()

pytestmark = pytest.mark.skipif(
    _PREREQ_PROBLEM is not None, reason=_PREREQ_PROBLEM or ""
)

requires_chrome = pytest.mark.skipif(
    _CHROME is None,
    reason=(
        "chrome executable not found (checked CHROME_PATH, Program Files, "
        "Program Files (x86), LOCALAPPDATA)"
    ),
)


def _run(steps, out_dir, url, extra=()):
    out_dir = Path(out_dir)
    out_dir.mkdir(parents=True, exist_ok=True)
    steps_file = out_dir / "steps.json"
    steps_file.write_text(json.dumps(steps, ensure_ascii=False), encoding="utf-8")
    cmd_extra = list(extra)
    if _CHROME is not None:
        cmd_extra = ["--chrome", str(_CHROME)] + cmd_extra
    proc = subprocess.run(
        [
            _NODE or "node",
            str(BROWSER),
            "--url",
            url,
            "--out",
            str(out_dir),
            "--steps",
            str(steps_file),
            *cmd_extra,
        ],
        capture_output=True,
        text=True,
        timeout=60,
        encoding="utf-8",
        errors="replace",
    )
    lines = [json.loads(l) for l in proc.stdout.splitlines() if l.strip()]
    return proc, lines


def test_browser_tool_help():
    proc = subprocess.run(
        [_NODE or "node", str(BROWSER), "--help"],
        capture_output=True,
        text=True,
        timeout=60,
        encoding="utf-8",
        errors="replace",
    )
    assert proc.returncode == 0, proc.stderr
    assert "--url" in proc.stdout
    assert "--steps" in proc.stdout


@requires_chrome
def test_browser_tool_reads_title(tmp_path):
    steps = [
        {"kind": "eval", "label": "title", "expression": "document.title"},
    ]
    proc, lines = _run(steps, tmp_path, PROTO_INDEX.as_uri())
    assert proc.returncode == 0, proc.stderr
    assert len(lines) == 1
    line = lines[0]
    assert line["step"] == 1
    assert line["kind"] == "eval"
    assert line["error"] is None
    assert line["value"] == "智能 P&ID 平台原型"


@requires_chrome
def test_browser_tool_click_changes_hash(tmp_path):
    steps = [
        {"kind": "eval", "label": "hash-before", "expression": "location.hash"},
        {
            "kind": "click",
            "label": "nav",
            "selector": "[data-testid='main-nav'] a",
        },
        {"kind": "wait", "label": "settle", "ms": 300},
        {"kind": "eval", "label": "hash-after", "expression": "location.hash"},
    ]
    proc, lines = _run(steps, tmp_path, PROTO_INDEX.as_uri())
    assert proc.returncode == 0, proc.stderr
    assert len(lines) == len(steps)
    assert lines[0]["error"] is None
    assert lines[0]["value"] == ""
    assert lines[-1]["error"] is None
    assert lines[-1]["value"] == "#/screen/workbench"
    assert lines[0]["value"] != lines[-1]["value"]


@requires_chrome
def test_browser_tool_screenshot_is_non_trivial(tmp_path):
    steps = [
        {"kind": "screenshot", "label": "shot", "file": "shot.png"},
    ]
    proc, lines = _run(steps, tmp_path, PROTO_INDEX.as_uri())
    assert proc.returncode == 0, proc.stderr
    shot = tmp_path / "shot.png"
    assert shot.is_file(), "screenshot not written: " + proc.stderr
    assert shot.stat().st_size > 10240


@requires_chrome
def test_browser_tool_fails_cleanly_on_missing_page(tmp_path):
    steps = [
        {"kind": "eval", "label": "title", "expression": "document.title"},
    ]
    proc, lines = _run(
        steps, tmp_path, "file:///D:/no/such/ocs-browser-tool-missing.html"
    )
    assert proc.returncode != 0
    assert proc.stderr.strip(), "expected a diagnostic on stderr"
    assert lines == []
    assert "Traceback" not in proc.stderr
    assert "node:internal" not in proc.stderr


@requires_chrome
def test_browser_tool_console_reports_zero_errors_on_clean_page(tmp_path):
    steps = [
        {"kind": "console"},
    ]
    proc, lines = _run(steps, tmp_path, PROTO_INDEX.as_uri())
    assert proc.returncode == 0, proc.stderr
    assert len(lines) == 1
    line = lines[0]
    assert line["kind"] == "console"
    assert line["error"] is None
    assert line["value"]["errorCount"] == 0
    assert line["value"]["messages"] == []


@requires_chrome
def test_browser_tool_console_captures_console_error(tmp_path):
    page = tmp_path / "console-error.html"
    page.write_text(
        '<!doctype html><html><head><meta charset="utf-8">'
        "<title>console error</title></head><body>"
        "<script>console.error('ocs-smoke-console-error');</script>"
        "</body></html>",
        encoding="utf-8",
    )
    steps = [
        {"kind": "console"},
    ]
    proc, lines = _run(steps, tmp_path, page.as_uri())
    assert proc.returncode == 0, proc.stderr
    assert len(lines) == 1
    line = lines[0]
    assert line["error"] is None
    assert line["value"]["errorCount"] >= 1
    assert any(
        "ocs-smoke-console-error" in m["text"]
        for m in line["value"]["messages"]
    )


@requires_chrome
def test_browser_tool_console_captures_load_time_exception(tmp_path):
    page = tmp_path / "load-exception.html"
    page.write_text(
        '<!doctype html><html><head><meta charset="utf-8">'
        "<title>load exception</title></head><body>"
        "<script>throw new Error('ocs-smoke-load-exception');</script>"
        "</body></html>",
        encoding="utf-8",
    )
    steps = [
        {"kind": "console"},
    ]
    proc, lines = _run(steps, tmp_path, page.as_uri())
    assert proc.returncode == 0, proc.stderr
    assert len(lines) == 1
    line = lines[0]
    assert line["error"] is None
    assert line["value"]["errorCount"] >= 1
    assert any(
        "ocs-smoke-load-exception" in m["text"]
        for m in line["value"]["messages"]
    )


@requires_chrome
def test_browser_tool_snapshot_lists_navigation(tmp_path):
    steps = [
        {"kind": "snapshot"},
    ]
    proc, lines = _run(steps, tmp_path, PROTO_INDEX.as_uri())
    assert proc.returncode == 0, proc.stderr
    assert len(lines) == 1
    line = lines[0]
    assert line["kind"] == "snapshot"
    assert line["error"] is None
    value = line["value"]
    assert value["title"] == "智能 P&ID 平台原型"
    assert value["hash"] == ""
    assert value["navCount"] == 3
    assert [a["text"] for a in value["nav"]] == [
        "工作台",
        "图纸管理",
        "P&ID 编辑器",
    ]
    assert [a["href"] for a in value["nav"]] == [
        "#/screen/workbench",
        "#/screen/drawings",
        "#/screen/editor",
    ]


@requires_chrome
def test_browser_tool_snapshot_reflects_hash_after_click(tmp_path):
    steps = [
        {
            "kind": "click",
            "label": "nav",
            "selector": "[data-testid='main-nav'] a",
        },
        {"kind": "wait", "label": "settle", "ms": 200},
        {"kind": "snapshot"},
    ]
    proc, lines = _run(steps, tmp_path, PROTO_INDEX.as_uri())
    assert proc.returncode == 0, proc.stderr
    assert len(lines) == len(steps)
    line = lines[-1]
    assert line["error"] is None
    assert line["value"]["hash"] == "#/screen/workbench"
    assert line["value"]["navCount"] == 3
