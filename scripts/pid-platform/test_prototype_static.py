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


# ---------------------------------------------------------------------------
# Screen-level static contract (VAL-PROTO-003 / VAL-PROTO-004)
#
# These read the shipped sources, so a screen that loses a required block or
# renames a contractual column fails here without a browser. The rendered
# behaviour itself is proven with browser.mjs; this pins the strings.
# ---------------------------------------------------------------------------

SCREENS_JS = ASSETS / "screens.js"

# The exact BOM-independent column set the drawings list must carry, in order.
DRAWING_COLUMNS = ["文件名", "状态", "版本", "修改人", "修改时间"]
# The exact toolbar labels, in order.
DRAWINGS_TOOLBAR = ["新建图纸", "导入", "复制", "移动", "权限", "版本比对"]
# Blocks the workbench must expose.
WORKBENCH_TESTIDS = [
    "overview-stats",
    "recent-drawings",
    "activity",
    "quick-actions",
]
DRAWINGS_TESTIDS = [
    "pbs-tree",
    "drawing-list",
    "drawing-preview",
    "drawings-toolbar",
    "pbs-node",
]


def _screens_text() -> str:
    assert SCREENS_JS.is_file(), "missing screen bundle: " + str(SCREENS_JS)
    return SCREENS_JS.read_text(encoding="utf-8")


def test_syntax_workbench_exposes_its_four_blocks():
    text = _screens_text()
    missing = [t for t in WORKBENCH_TESTIDS if t not in text]
    assert missing == [], "workbench is missing blocks: %s" % missing


def test_syntax_drawings_exposes_tree_list_preview_and_toolbar():
    text = _screens_text()
    missing = [t for t in DRAWINGS_TESTIDS if t not in text]
    assert missing == [], "drawings screen is missing blocks: %s" % missing


def test_syntax_drawings_column_labels_are_exact_and_ordered():
    text = _screens_text()
    # The header row is emitted as one contiguous run of <th> cells, so matching
    # the run proves both membership and order in one step.
    expected = "".join("<th>%s</th>" % name for name in DRAWING_COLUMNS)
    assert expected in text, (
        "the drawing-list header is not the ordered run %r; the bundle has %r"
        % (expected, re.findall(r"<th>[^<]*</th>", text))
    )


def test_syntax_drawings_toolbar_labels_are_exact_and_ordered():
    text = _screens_text()
    labels = re.findall(
        r"<button[^>]*>([^<]+)</button>", text
    )
    # Restrict to the drawings toolbar block so unrelated buttons cannot satisfy
    # the check.
    start = text.find('data-testid="drawings-toolbar"')
    assert start >= 0, "the drawings toolbar block is missing"
    block = text[start: text.find("</div>", start)]
    in_block = re.findall(r"<button[^>]*>([^<]+)</button>", block)
    assert in_block == DRAWINGS_TOOLBAR, (
        "the toolbar labels are %r; expected %r (all buttons in bundle: %r)"
        % (in_block, DRAWINGS_TOOLBAR, labels)
    )


def test_syntax_seed_data_contains_the_p1001_equipment_tag():
    """VAL-PROTO-004 and later cross-screen flows depend on this seed tag."""
    text = _screens_text()
    assert "P-1001" in text, "the seed data no longer carries the tag P-1001"
    # It must be a 设备 symbol, not a bare string in a comment.
    match = re.search(r"tag:\s*[\"']P-1001[\"']", text)
    assert match, "P-1001 is not attached to a symbol object"
    window = text[max(0, match.start() - 200): match.end() + 200]
    assert "设备" in window, (
        "P-1001 is not seeded as a 设备 symbol; nearby text was: %r" % window
    )


def test_syntax_no_hard_coded_hex_outside_tokens():
    offenders = []
    for path in [INDEX, SCREENS_JS] + sorted(ASSETS.glob("*.css")):
        if path.name == "tokens.css":
            continue
        for lineno, line in enumerate(
            path.read_text(encoding="utf-8").splitlines(), start=1
        ):
            if re.search(r"#[0-9a-fA-F]{3,8}\b", line):
                offenders.append("%s:%d" % (path.name, lineno))
    assert offenders == [], (
        "hard-coded hex colours outside tokens.css: %s" % offenders
    )


# ---------------------------------------------------------------------------
# Enum-column contract (REQ-TYPE-008 / REQ-SYM-005 / REQ-SYM-006 / REQ-SYM-007)
#
# These pin the values, not just the headers. M6 scrutiny round 1 failed on
# three tables whose headers were right and whose cells carried invented
# enumerations, so each owning Enum(...) is now checked against the seed data
# in both directions: nothing outside the allowed set is emitted, and every
# allowed value appears.
# ---------------------------------------------------------------------------

TL_ATTRS_BLOCK_RE = re.compile(
    r"var TL_ATTRS = \{(.*?)\n  \};", re.DOTALL
)
TL_TYPE_RE = re.compile(r'"([a-z-]+)": \[(.*?)\n    \]', re.DOTALL)

TYPELIB_LEVEL_VALUES = ["可选信息", "必选信息"]
# Each type's most characteristic element must lead its attribute table.
TYPELIB_FIRST_ROWS = {
    "process-equipment": "类别编码",
    "rotating": "额定功率",
    "pump": "额定流量",
    "compressor": "排气量",
    "static": "材质",
    "vessel": "容积",
    "tower": "塔板数",
    "piping": "连接形式",
    "valve": "公称直径",
    "fitting": "规格",
    "instrument": "量程",
    "field-instrument": "信号类型",
    "instrument-loop": "回路功能",
}
CONNECTION_TYPES = ["等径", "异径", "信号线"]
CONNECTION_DIRECTIONS = ["双向点", "流入点", "流出点"]
LABEL_POSITIONS = ["上", "下", "左", "右"]


def _tl_attrs() -> dict:
    text = _screens_text()
    block = TL_ATTRS_BLOCK_RE.search(text)
    assert block, "the TL_ATTRS seed block is missing from the bundle"
    out = {}
    for type_id, body in TL_TYPE_RE.findall(block.group(1)):
        rows = re.findall(r'\[([^\]]*)\]', body)
        out[type_id] = [
            [cell.strip().strip('"') for cell in row.split(",")]
            for row in rows
        ]
    assert out, "TL_ATTRS parsed to no types"
    return out


def test_syntax_typelib_level_column_uses_the_requirement_enum():
    """REQ-TYPE-008: 级别 is Enum(可选信息/必选信息), default 可选信息.

    Both directions are checked: every seeded cell is inside the allowed set,
    and the control offers every allowed value, so neither an invented value
    nor a dropped option can pass.
    """
    offenders = []
    seen = set()
    for type_id, rows in _tl_attrs().items():
        for row in rows:
            level = row[4]
            seen.add(level)
            if level not in TYPELIB_LEVEL_VALUES:
                offenders.append("%s/%s -> %r" % (type_id, row[0], level))
    assert offenders == [], (
        "typelib 级别 carries values outside %r: %s"
        % (TYPELIB_LEVEL_VALUES, offenders)
    )
    assert seen, "TL_ATTRS carries no 级别 values at all"
    assert seen == {"可选信息"}, (
        "seeded 级别 values must all default to 可选信息; saw %r"
        % (sorted(seen),)
    )
    text = _screens_text()
    at = text.index('data-testid="attribute-level"')
    offered = re.findall(r">([^<>]+)</option>", text[at: at + 700])[:2]
    assert offered == TYPELIB_LEVEL_VALUES, (
        "the 级别 select offers %r; the Enum requires %r"
        % (offered, TYPELIB_LEVEL_VALUES)
    )


def test_syntax_typelib_level_cell_is_a_select_with_two_options():
    """The 级别 cell must be editable, not a static string."""
    text = _screens_text()
    assert 'data-testid="attribute-level"' in text, (
        "the 级别 cell has no data-testid=attribute-level select"
    )
    select_at = text.index('data-testid="attribute-level"')
    window = text[select_at: select_at + 700]
    options = re.findall(r">([^<>]+)</option>", window)
    assert options[:2] == TYPELIB_LEVEL_VALUES, (
        "the 级别 select options are %r; expected %r"
        % (options[:2], TYPELIB_LEVEL_VALUES)
    )
    # Default is 可选信息 per REQ-TYPE-008: the seed data must say so.
    assert '["位号", "TAG_NO", "字符串", "—", "可选信息"]' in text, (
        "the 位号 row does not default to 可选信息"
    )


def test_syntax_typelib_first_row_is_the_types_characteristic_element():
    """VAL-PROTO-009 accepts a first-row change; make the change unambiguous."""
    attrs = _tl_attrs()
    assert set(attrs) == set(TYPELIB_FIRST_ROWS), (
        "TL_ATTRS covers %r; expected %r"
        % (sorted(attrs), sorted(TYPELIB_FIRST_ROWS))
    )
    wrong = {
        type_id: attrs[type_id][0][0]
        for type_id in attrs
        if attrs[type_id][0][0] != TYPELIB_FIRST_ROWS[type_id]
    }
    assert wrong == {}, "types whose first row is not the characteristic one: %s" % wrong
    # The 位号 row must survive the reorder.
    missing = [t for t, rows in attrs.items() if "位号" not in [r[0] for r in rows]]
    assert missing == [], "types that lost their 位号 row: %s" % missing
    # Row counts must still differ between types so switching always redraws.
    assert len({len(rows) for rows in attrs.values()}) > 1, (
        "every type has the same row count, so switching nodes cannot change the table"
    )


def _seed_block(start_marker: str, end_marker: str) -> str:
    text = _screens_text()
    start = text.index(start_marker)
    return text[start: text.index(end_marker, start)]


def test_syntax_symbol_editor_connection_enum_matches_req_sym_005():
    """REQ-SYM-005: 类型 Enum(等径/异径/信号线), 方向 Enum(双向点/流入点/流出点)."""
    block = _seed_block("var SE_CONNECTIONS", "var SE_ATTRIBUTES")
    rows = re.findall(r'\["([^"]+)", "([^"]+)", "([^"]+)"\]', block)
    assert rows, "SE_CONNECTIONS has no rows"
    bad_types = [r[0] for r in rows if r[0] not in CONNECTION_TYPES]
    bad_dirs = [r[1] for r in rows if r[1] not in CONNECTION_DIRECTIONS]
    assert bad_types == [], (
        "connection 类型 carries values outside %r: %s"
        % (CONNECTION_TYPES, bad_types)
    )
    assert bad_dirs == [], (
        "connection 方向 carries values outside %r: %s"
        % (CONNECTION_DIRECTIONS, bad_dirs)
    )


def test_syntax_symbol_editor_label_position_enum_matches_req_sym_006():
    """REQ-SYM-006: 位置 Enum(上/下/左/右)."""
    block = _seed_block("var SE_ATTRIBUTES", "var SE_GLYPH")
    rows = re.findall(r'\["([^"]+)", "([^"]+)", "([^"]+)", "([^"]+)"\]', block)
    assert rows, "SE_ATTRIBUTES has no rows"
    bad = [r[3] for r in rows if r[3] not in LABEL_POSITIONS]
    assert bad == [], (
        "label 位置 carries values outside %r: %s" % (LABEL_POSITIONS, bad)
    )


def test_syntax_tag_rule_editor_declares_the_hierarchy_select():
    """REQ-SYM-007: 层级选择 Enum(当前类型依次递增/父类型递增)."""
    text = _screens_text()
    assert 'data-testid="rule-hierarchy-select"' in text, (
        "the tag-rule editor has no 层级选择 select"
    )
    at = text.index('data-testid="rule-hierarchy-select"')
    window = text[at: at + 700]
    options = re.findall(r">([^<>]+)</option>", window)
    assert options[:2] == ["当前类型依次递增", "父类型递增"], (
        "the 层级选择 options are %r" % (options[:2],)
    )
    # VAL-PROTO-012 still requires the auto-code switch and the prefix source.
    for testid in ("rule-auto-code", "rule-prefix", "rule-step"):
        assert 'data-testid="%s"' % testid in text, (
            "the tag-rule editor lost data-testid=%s" % testid
        )


def test_syntax_tag_prefix_is_escaped_before_regex_construction():
    """A prefix such as `E(` must not reach RegExp unescaped (VAL-PROTO-020)."""
    text = _screens_text()
    assert "function escapeRegExp(text)" in text, (
        "the bundle has no escapeRegExp helper"
    )
    assert "escapeRegExp(prefix)" in text, (
        "nextTag does not escape the prefix before building its RegExp"
    )
    assert 'new RegExp("^" + prefix' not in text, (
        "nextTag still concatenates the raw prefix into a RegExp"
    )
    assert "function sanitizeTagPrefix(text)" in text, (
        "the tag-rule prefix input is not sanitised"
    )
