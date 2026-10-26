"""Static checks for the prototype shell under docs/pid-platform/prototype.

These tests only inspect the static files; they never start a server.
"""
from __future__ import annotations

import re
import shutil
import subprocess

import pytest

bs4 = pytest.importorskip(
    "bs4", reason="beautifulsoup4 is required for prototype static checks"
)
BeautifulSoup = bs4.BeautifulSoup

from conftest import PROTO  # noqa: E402

INDEX = PROTO / "index.html"
ASSETS = PROTO / "assets"

_IMPORT_RE = re.compile(r"@import\s+(?:url\(\s*)?['\"]([^'\")]+)", re.IGNORECASE)
_FETCH_RE = re.compile(r"fetch\(\s*['\"]([^'\"]+)")
_SCHEME_RE = re.compile(r"^[a-zA-Z][a-zA-Z0-9+.-]*:")


def _index_text() -> str:
    return INDEX.read_text(encoding="utf-8")


def _soup():
    return BeautifulSoup(_index_text(), "html.parser")


def _asset_texts() -> list:
    files = sorted(ASSETS.rglob("*.css")) + sorted(ASSETS.rglob("*.js"))
    return [p.read_text(encoding="utf-8") for p in files]


def _all_reference_strings() -> list:
    """Every src/href attribute value plus @import and fetch() string args."""
    soup = _soup()
    refs = [
        tag[attr]
        for tag in soup.find_all(True)
        for attr in ("src", "href")
        if tag.has_attr(attr) and isinstance(tag[attr], str) and tag[attr]
    ]
    for body in [_index_text()] + _asset_texts():
        refs += _IMPORT_RE.findall(body)
        refs += _FETCH_RE.findall(body)
    return refs


def _is_local(ref: str) -> bool:
    ref = ref.strip()
    return (
        bool(ref)
        and not ref.startswith("#")
        and not ref.startswith("//")
        and not _SCHEME_RE.match(ref)
    )


def _local_resource_refs() -> list:
    """link[href], script[src], img[src] and @import targets -> (ref, base)."""
    soup = _soup()
    refs = []
    for tag_name, attr in (("link", "href"), ("script", "src"), ("img", "src")):
        for el in soup.find_all(tag_name):
            value = el.get(attr)
            if value and _is_local(value):
                refs.append((value, PROTO))
    for el in soup.find_all("style"):
        for value in _IMPORT_RE.findall(el.get_text()):
            if _is_local(value):
                refs.append((value, PROTO))
    for css in sorted(ASSETS.rglob("*.css")):
        for value in _IMPORT_RE.findall(css.read_text(encoding="utf-8")):
            if _is_local(value):
                refs.append((value, css.parent))
    return refs


def test_syntax_index_html_exists():
    assert INDEX.is_file(), "missing prototype entry: " + str(INDEX)
    assert INDEX.stat().st_size > 0, "index.html is empty"


def test_syntax_prototype_html_is_well_formed():
    text = _index_text()
    assert text.lstrip().lower().startswith("<!doctype"), (
        "index.html must start with a doctype declaration"
    )
    soup = _soup()
    assert soup.html is not None, "no <html> element parsed"
    assert soup.html.get("lang") == "zh-CN", (
        "<html> lang must be zh-CN, got %r" % soup.html.get("lang")
    )
    assert soup.find(attrs={"data-testid": "app-shell"}) is not None, (
        "missing element with data-testid='app-shell'"
    )


def test_syntax_no_external_resource_references():
    bad = [
        ref
        for ref in _all_reference_strings()
        if "http://" in ref
        or "https://" in ref
        or ref.strip().startswith("//")
        or "cdn" in ref.lower()
    ]
    assert bad == [], "external resource references found: %s" % bad


def test_syntax_local_resource_count_within_limit():
    refs = _local_resource_refs()
    assert len(refs) <= 4, "too many local resources referenced: %s" % [
        r for r, _ in refs
    ]


def test_syntax_local_resources_exist():
    proto_root = PROTO.resolve()
    missing = []
    for ref, base in _local_resource_refs():
        target = (base / ref).resolve()
        if not target.is_file() or not target.is_relative_to(proto_root):
            missing.append(ref)
    assert missing == [], (
        "local resources missing or outside prototype dir: %s" % missing
    )


def test_syntax_app_js_passes_node_check():
    node = shutil.which("node")
    if node is None:
        pytest.skip("node executable not found on PATH")
    js_files = sorted(ASSETS.glob("*.js"))
    assert js_files, "no .js files found under " + str(ASSETS)
    for js in js_files:
        proc = subprocess.run(
            [node, "--check", str(js)],
            capture_output=True,
            text=True,
            shell=False,
        )
        assert proc.returncode == 0, (
            "node --check failed for %s:\n%s" % (js, proc.stderr)
        )


def test_syntax_tokens_css_defines_at_least_twelve_custom_properties():
    tokens = ASSETS / "tokens.css"
    assert tokens.is_file(), "missing token file: " + str(tokens)
    text = tokens.read_text(encoding="utf-8")
    match = re.search(r":root\s*\{(.*?)\}", text, re.DOTALL)
    assert match, "no :root block found in tokens.css"
    names = set(re.findall(r"--([a-zA-Z0-9_-]+)\s*:", match.group(1)))
    print("custom properties defined:", len(names))
    assert len(names) >= 12, (
        "expected at least 12 custom properties, found %d: %s"
        % (len(names), sorted(names))
    )
