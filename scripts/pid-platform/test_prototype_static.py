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

from conftest import DOCS, PROTO  # noqa: E402
from requirements_contract import bom_rows, find_bom_header  # noqa: E402

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


def _mount_count(text: str, testid: str) -> int:
    """How many times ``testid`` is actually mounted as an attribute.

    The leading space matters: a delegated listener writes the selector form
    ``[data-testid="ribbon"]``, which a bare ``count('data-testid="ribbon"')``
    also matches (ribbon and canvas each score 2 that way, pbs-node and
    drawing-list likewise). Counting the mount form is the only way the guard
    can tell a deleted mount point from a surviving CSS selector.
    """
    return text.count(' data-testid="%s"' % testid)


def _assert_mounted_testids(text: str, testids: list, screen: str) -> None:
    """Every named testid must be mounted exactly once."""
    wrong = {
        testid: _mount_count(text, testid)
        for testid in testids
        if _mount_count(text, testid) != 1
    }
    assert wrong == {}, (
        "%s screen mount points are wrong (testid -> mount count, expected 1 "
        "each): %s" % (screen, wrong)
    )


def test_syntax_workbench_exposes_its_four_blocks():
    """VAL-PROTO-003: the four workbench blocks are mounted, one each.

    Counting the mount form rather than `in text` means deleting
    `data-testid="activity"` from the markup fails here even though the
    screen's delegated listener still contains `[data-testid="activity"]`.
    """
    _assert_mounted_testids(_screens_text(), WORKBENCH_TESTIDS, "workbench")


def test_syntax_drawings_exposes_tree_list_preview_and_toolbar():
    """VAL-PROTO-004: the five drawings blocks are mounted, one each."""
    _assert_mounted_testids(_screens_text(), DRAWINGS_TESTIDS, "drawings")


def test_syntax_screen_mount_guards_can_fail():
    """Negative control: each single-point mount removal must be detected.

    One mutation per guarded testid, driven through the same helper the two
    tests above use, so a guard that cannot fail is caught here rather than
    surviving into a release.
    """
    text = _screens_text()
    for screen, testids in (
        ("workbench", WORKBENCH_TESTIDS),
        ("drawings", DRAWINGS_TESTIDS),
    ):
        for testid in testids:
            mount = ' data-testid="%s"' % testid
            assert mount in text, (
                "%s guard names %s but the bundle has no such mount point"
                % (screen, testid)
            )
            mutated = text.replace(mount, "", 1)
            assert mutated != text, "removing %s changed nothing" % mount
            with pytest.raises(AssertionError) as excinfo:
                _assert_mounted_testids(mutated, testids, screen)
            assert testid in str(excinfo.value), (
                "removing the %s mount point was reported, but not by name: %s"
                % (testid, excinfo.value)
            )


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


def _canvas_symbol_kind(text: str, tag: str) -> str:
    """The second argument of ``makeSymbol("<tag>", <kind>, ...)``.

    The kind is read from the object literal itself. A ±200-character window
    around the tag string survives the symbol being reseeded under a different
    kind, because a neighbouring symbol's text still contains 设备.
    """
    match = re.search(
        r'makeSymbol\(\s*"%s"\s*,\s*"([^"]+)"' % re.escape(tag), text
    )
    assert match, "no makeSymbol call places %s on the canvas" % tag
    return match.group(1)


def _drawing_symbol_kinds(text: str, tag: str) -> list:
    """Every ``kind`` bound to ``tag`` inside the drawings seed."""
    block = _block_after(text, "var drawings = [", "\n  ];")
    return re.findall(r'\{ kind: "([^"]+)", tag: "%s"' % re.escape(tag), block)


def _assert_p1001_is_equipment(text: str) -> None:
    """P-1001 must be a 设备 symbol in both seed sources, and the canvas kind
    must map to 设备 through the editor's own category table."""
    assert _canvas_symbol_kind(text, "P-1001") == "equipment", (
        "the canvas seed places P-1001 as %r, expected the equipment kind"
        % _canvas_symbol_kind(text, "P-1001")
    )
    kinds = _drawing_symbol_kinds(text, "P-1001")
    assert kinds, "the drawings seed carries no P-1001 symbol record"
    assert set(kinds) == {"设备"}, (
        "the drawings seed binds P-1001 to %r, expected 设备" % (kinds,)
    )
    categories = _block_after(text, "var ED_CATEGORIES = [", "];")
    assert re.search(r'id: "equipment", label: "设备"', categories), (
        "ED_CATEGORIES no longer maps equipment to 设备, so the canvas kind "
        "and the 设备 requirement would disagree"
    )


def test_syntax_seed_data_contains_the_p1001_equipment_tag():
    """VAL-PROTO-004 and later cross-screen flows depend on this seed tag.

    The kind is parsed off the seed objects, not scanned in a character
    window, so changing P-1001 from 设备 to 管路 fails here.
    """
    text = _screens_text()
    assert "P-1001" in text, "the seed data no longer carries the tag P-1001"
    _assert_p1001_is_equipment(text)


def test_syntax_p1001_equipment_guard_can_fail():
    """Negative control: reseeding P-1001 as a pipe must be detected."""
    text = _screens_text()
    canvas_mutation = text.replace(
        'makeSymbol("P-1001", "equipment",', 'makeSymbol("P-1001", "piping",', 1
    )
    assert canvas_mutation != text, "the canvas kind mutation changed nothing"
    with pytest.raises(AssertionError):
        _assert_p1001_is_equipment(canvas_mutation)

    drawing_mutation = text.replace(
        '{ kind: "设备", tag: "P-1001"', '{ kind: "管路", tag: "P-1001"', 1
    )
    assert drawing_mutation != text, "the drawings kind mutation changed nothing"
    with pytest.raises(AssertionError):
        _assert_p1001_is_equipment(drawing_mutation)


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


# ---------------------------------------------------------------------------
# VAL-CROSS-003 screen-level elements.
#
# The assertion requires that a requirement mapped to a screen leaves its key
# interface element names visible on that screen. The gate had no coverage of
# this, so typelib and symbols shipped without REQ-TYPE-001's 类型编码 field or
# REQ-SYM-001's four legend standards and four contract fields while the suite
# stayed green. Each check below is a subset assertion: it proves the element
# name is present in the bundle, not that every allowed value appears.
# ---------------------------------------------------------------------------

TYPE_LIBRARY_FIELDS = ["类型编码", "名称", "描述", "父类型"]
LEGEND_STANDARD_NAMES = ["HG/T 20519", "SY/T 0003", "DIN", "ISO"]
LEGEND_STANDARD_FIELDS = ["标准名称", "符号数", "是否当前标准", "图例列表选择"]
PBS_CONTRACT_FIELDS = ["节点名称", "节点类型", "父节点", "图纸数"]


def _type_form_block(text: str) -> str:
    at = text.find('data-testid="type-form"')
    assert at != -1, "typelib has no type-form block"
    return text[at: at + 2200]


def test_syntax_typelib_declares_the_four_type_contract_fields():
    """REQ-TYPE-001: 类型编码 / 名称 / 描述 / 父类型 are all录入able."""
    text = _screens_text()
    block = _type_form_block(text)
    missing = [name for name in TYPE_LIBRARY_FIELDS if name not in block]
    assert missing == [], "type-form is missing contract fields: %s" % missing
    for testid in (
        "type-form-code",
        "type-form-name",
        "type-form-desc",
        "type-form-parent",
        "type-form-ok",
        "type-form-cancel",
    ):
        assert 'data-testid="%s"' % testid in block, (
            "type-form is missing data-testid=%s" % testid
        )


def test_syntax_typelib_tree_shows_the_type_code():
    """Every tree node renders name（code） so 类型编码 is visible."""
    text = _screens_text()
    assert 'esc(node.code || node.id)' in text, (
        "the type tree does not render the node code"
    )
    codes = re.findall(r'code: "([A-Z-]+)"', text)
    assert len(codes) >= 13, "expected at least 13 type codes, found %r" % (codes,)
    for required in ("PUMP", "VALVE", "INSTRUMENT-LOOP"):
        assert required in codes, "missing type code %s" % required


def test_syntax_symbols_ships_the_four_legend_standards():
    """REQ-SYM-001: 4 preset standards, names verbatim."""
    text = _screens_text()
    at = text.find("var SL_STANDARDS = [")
    assert at != -1, "SL_STANDARDS is gone"
    block = text[at: at + 2000]
    found = [name for name in LEGEND_STANDARD_NAMES if 'name: "%s"' % name in block]
    assert found == LEGEND_STANDARD_NAMES, (
        "legend standards present: %s" % (found,)
    )
    for stale in ("企业标准 HG/T 20519", "国标 GB/T 6567", "项目自定义标准"):
        assert stale not in block, "stale standard name still present: %s" % stale


def test_syntax_symbols_declares_the_four_standard_contract_fields():
    """REQ-SYM-001: 标准名称 / 符号数 / 是否当前标准 / 图例列表选择."""
    text = _screens_text()
    at = text.find('data-testid="standard-panel"')
    assert at != -1, "symbols has no standard-panel block"
    block = text[at: at + 1600]
    missing = [name for name in LEGEND_STANDARD_FIELDS if name not in block]
    assert missing == [], "standard-panel is missing contract fields: %s" % missing
    assert 'data-testid="legend-standard-select"' in block, (
        "standard-panel has no 图例列表选择 select"
    )


def _unique_index(text: str, marker: str, label: str) -> int:
    """Index of ``marker`` when it occurs exactly once; assert otherwise.

    A slice helper that silently takes the first of several matches changes
    what the assertion looks at as soon as the file gains a second similar
    construct. Zero matches must not return a truncated block either, so both
    counts are hard failures.
    """
    count = text.count(marker)
    assert count == 1, (
        "%s must occur exactly once, found %d: %r" % (label, count, marker)
    )
    return text.find(marker)


# The pbs-node template starts at its own button literal and ends at the
# 图纸数 field plus the closing tag. ``</button>`` alone is not an anchor:
# screens.js carries eight ``"</button>";`` literals, so a second template
# would move the slice without any test noticing.
PBS_NODE_START = '<button type="button" class="pbs-node'
PBS_NODE_END = 'pbsDrawingCount(node.id) +\n        "</button>";'


def _pbs_node_block(text: str) -> str:
    """The pbs-node template, sliced between its own unique terminators.

    A character budget (``at + 900``) silently changes the assertion target
    as the seed grows: it can run past the template into unrelated text, or
    stop short of the last contract field.
    """
    at = _unique_index(text, PBS_NODE_START, "the pbs-node template start")
    end = _unique_index(text, PBS_NODE_END, "the pbs-node template terminator")
    assert end > at, "the pbs-node terminator precedes its start"
    return text[at:end + len(PBS_NODE_END)]


def test_syntax_drawings_pbs_nodes_show_the_four_contract_fields():
    """REQ-DRW-001: PBS nodes expose 节点名称 / 节点类型 / 父节点 / 图纸数."""
    text = _screens_text()
    at = text.find('data-testid="pbs-node"')
    assert at != -1, "the PBS tree renders no pbs-node"
    block = _pbs_node_block(text)
    missing = [name for name in PBS_CONTRACT_FIELDS if name not in block]
    assert missing == [], "pbs-node is missing contract fields: %s" % missing


def test_syntax_side_channel_state_does_not_grow_the_proto_contract():
    """VAL-PROTO-019 freezes window.__proto at 8 keys; new state is side-channel."""
    text = _screens_text()
    keys_block = text[text.find("var PROTO_KEYS = ["):]
    keys_block = keys_block[: keys_block.find("];") + 2]
    keys = re.findall(r'"([A-Za-z]+)"', keys_block)
    assert len(keys) == 8, "window.__proto grew to %d keys: %r" % (len(keys), keys)
    for side in ("window.__typeLibrary", "window.__legendStandard"):
        assert side in text, "side-channel state %s is missing" % side


# renderScreenHtml is the only function whose catch must feed the error sink.
# ``screens.js`` carries two ``      } catch (err) {`` blocks (the second is in
# the masterdata screen), so an unbounded search silently follows the file's
# order. The slice is bounded to the renderScreenHtml body and additionally
# required to sit after its own ``window.PidScreens[id](def);`` call.
RENDER_FN_START = "function renderScreenHtml(id) {"
RENDER_FN_END = "\n  }\n"
RENDER_DISPATCH = "window.PidScreens[id](def);"
CATCH_OPEN = "      } catch (err) {"
CATCH_CLOSE = "\n      }\n"


def _render_catch_block(text: str) -> str:
    """The catch body of renderScreenHtml, sliced to its real terminator.

    Bounded to that one function and anchored after its dispatch call, so a
    reordered second catch block cannot become the assertion target. Both the
    function and the catch must match exactly once, or the helper fails
    rather than returning a fragment.
    """
    fn_at = _unique_index(text, RENDER_FN_START, "the renderScreenHtml definition")
    body_end = text.find(RENDER_FN_END, fn_at)
    assert body_end != -1, "renderScreenHtml has no body terminator"
    body = text[fn_at:body_end]

    dispatch_at = _unique_index(body, RENDER_DISPATCH, "renderScreenHtml's dispatch")
    catch_at = _unique_index(body, CATCH_OPEN, "renderScreenHtml's catch header")
    assert catch_at > dispatch_at, (
        "the catch header precedes window.PidScreens[id](def); it is not the "
        "renderer's error path"
    )
    end = body.find(CATCH_CLOSE, catch_at)
    assert end != -1, "renderScreenHtml's catch has no closing brace"
    return body[catch_at:end]


RENDER_CATCH_WRITES = [
    "window.__protoErrors.push(message);",
    "window.console.error(message, err);",
]


def _assert_renderer_errors_are_recorded(text: str) -> None:
    """A throwing renderer must reach window.__protoErrors and the console.

    Without this, VAL-PROTO-020's four health signals all stay green on a
    blank screen: app-shell 1, screen-title 1, __protoErrors [], console
    errorCount 0 (measured at 88ca2bd8e by replacing window.PidScreens.workbench
    with a thrower; overview-stats fell to 0 and the body text shrank to 158
    characters).

    The needles are the complete call expressions, not just the receiver: a
    guard written as `"window.__protoErrors" in block` would still pass after
    the push was deleted, because the `typeof window.__protoErrors.push`
    feature test remains.
    """
    block = _render_catch_block(text)
    missing = [write for write in RENDER_CATCH_WRITES if write not in block]
    assert missing == [], (
        "renderScreenHtml's catch is missing %r, so a swallowed renderer stays "
        "invisible to the health assertions; block was %r" % (missing, block)
    )


def test_syntax_slice_helpers_require_a_unique_anchor():
    """A slice helper must fail on 0 or 2+ matches, never return a fragment.

    Both helpers used to cut on a token that appears many times in the bundle
    (``</button>`` eight times, the six-space ``} catch (err) {`` twice), so a
    second similar construct would silently change what the guard inspects.
    Here the duplicate is injected into the text the guard already reads, so
    the control fails for the same reason a real edit would.
    """
    text = _screens_text()

    # Two pbs-node template starts: the helper cannot tell which one to slice.
    doubled_pbs = text.replace(
        PBS_NODE_START, PBS_NODE_START + "\n" + PBS_NODE_START, 1
    )
    assert doubled_pbs != text, "the pbs-node duplicate mutation changed nothing"
    with pytest.raises(AssertionError):
        _pbs_node_block(doubled_pbs)

    # No pbs-node template at all.
    without_pbs = text.replace(PBS_NODE_START, "<!-- removed -->", 1)
    assert without_pbs != text, "the pbs-node removal mutation changed nothing"
    with pytest.raises(AssertionError):
        _pbs_node_block(without_pbs)

    # A second renderScreenHtml definition.
    doubled_fn = text.replace(
        RENDER_FN_START, RENDER_FN_START + "\n" + RENDER_FN_START, 1
    )
    assert doubled_fn != text, "the renderScreenHtml duplicate mutation changed nothing"
    with pytest.raises(AssertionError):
        _render_catch_block(doubled_fn)

    # A second catch header inside renderScreenHtml's own body.
    fn_at = text.find(RENDER_FN_START)
    body_end = text.find(RENDER_FN_END, fn_at)
    body = text[fn_at:body_end]
    dispatch_at = body.find(RENDER_DISPATCH)
    assert dispatch_at != -1, "renderScreenHtml no longer dispatches to PidScreens"
    doubled_catch = (
        text[:fn_at + dispatch_at]
        + CATCH_OPEN
        + "\n      }\n"
        + text[fn_at + dispatch_at:]
    )
    with pytest.raises(AssertionError):
        _render_catch_block(doubled_catch)

    # The dispatch call is gone, so the catch cannot be proven to belong to
    # the renderer's error path.
    without_dispatch = text.replace(RENDER_DISPATCH, "return null;", 1)
    assert without_dispatch != text, "the dispatch removal mutation changed nothing"
    with pytest.raises(AssertionError):
        _render_catch_block(without_dispatch)


def test_syntax_pbs_node_block_covers_the_whole_template():
    """The pbs-node slice starts at the template and ends at its own tag.

    The previous helper began at the ``data-testid`` attribute and stopped at
    the first ``</button>`` (a 306-character slice); the template itself is
    435 characters. Tightening the anchors must not shrink the subject, so
    this pins that the slice still covers the template literal, its closing
    tag, and every contract field the guard above checks.
    """
    text = _screens_text()
    block = _pbs_node_block(text)
    assert block.startswith(PBS_NODE_START), (
        "the pbs-node slice no longer starts at the template literal"
    )
    assert block.endswith('"</button>";'), (
        "the pbs-node slice no longer ends at its own closing tag"
    )
    for name in PBS_CONTRACT_FIELDS:
        assert name in block, "the pbs-node slice lost %s" % name
    assert 'data-testid="pbs-node"' in block, (
        "the pbs-node slice lost its own testid"
    )


def test_syntax_renderer_exceptions_reach_the_error_sink():
    """A renderer exception must be recorded, not silently replaced."""
    _assert_renderer_errors_are_recorded(_screens_text())


def test_syntax_renderer_error_sink_guard_can_fail():
    """Negative control: dropping either write must be detected.

    The presence check reads the sliced catch block, not the whole bundle: a
    needle that only survived elsewhere in ``screens.js`` would make the
    mutation look effective while leaving the catch untouched.
    """
    text = _screens_text()
    for needle in RENDER_CATCH_WRITES:
        assert needle in _render_catch_block(text), (
            "the catch block no longer contains %r" % needle
        )
        mutated = text.replace(needle, "", 1)
        assert mutated != text, "removing %r changed nothing" % needle
        with pytest.raises(AssertionError):
            _assert_renderer_errors_are_recorded(mutated)


def test_syntax_cross003_element_checks_can_fail():
    """Negative control: the element-name checks above must reject a rename."""
    text = _screens_text()
    mutated = text.replace('data-testid="type-form-code"', 'data-testid="type-form-kode"')
    mutated = mutated.replace('name: "SY/T 0003"', 'name: "SY/T 9999"')
    mutated = mutated.replace("节点名称：", "节点标题：")
    assert mutated != text, "the mutation did not change the bundle"

    block = _type_form_block(mutated)
    assert "type-form-code" not in block, (
        "renaming type-form-code did not break the field check"
    )
    at = mutated.find("var SL_STANDARDS = [")
    std_block = mutated[at: at + 2000]
    assert 'name: "SY/T 0003"' not in std_block, (
        "renaming a standard did not break the standards check"
    )
    pbs = _pbs_node_block(mutated)
    assert "节点名称：" not in pbs, (
        "renaming 节点名称 did not break the PBS check"
    )


# --- design rule check: rule names, severities, rows, derived summary ---
#
# Every guard below is a pure function of the bundle text so the negative
# control at the end of this section can drive the same code with a mutated
# copy and prove the guard bites. A guard that only re-reads the committed
# file cannot show that.

VIOLATION_ROW_COUNT = 10

VALIDATION_RULE_NAMES = {
    "DR-001": "管线缺管道等级",
    "DR-002": "管线缺公称直径",
    "DR-003": "设备无位号",
    "DR-004": "位号重复",
    "DR-005": "仪表未分配回路",
    "DR-006": "页间连接符对端缺失",
    "DR-007": "阀门压力等级高于管道等级",
    "DR-008": "管口未连接",
    "DR-010": "属性必填项为空",
}

VALIDATION_SEVERITIES = {
    "DR-001": "错误",
    "DR-002": "错误",
    "DR-003": "错误",
    "DR-004": "错误",
    "DR-005": "警告",
    "DR-006": "错误",
    "DR-007": "错误",
    "DR-008": "提示",
    "DR-010": "错误",
}

# Rules whose 位号 must appear in their own message. DR-006 acts on
# 页间连接符, for which requirements/50 defines no tag-prefix convention, so
# it is not constrained here rather than pinned to an invented convention.
TAG_QUOTING_RULES = {"DR-001", "DR-002", "DR-010"}
LINE_TAG_RULES = {"DR-001", "DR-002"}


def _violation_rows(text):
    """One dict per VIOLATIONS record; ``suggestion`` is optional.

    Records are split on the opener and then parsed field by field, so a row
    that legally omits 建议修复 (REQ-DATA-014 makes it optional) is still
    parsed. Matching the whole record with a regex that requires
    ``suggestion`` silently skips such a row, which turns the guard vacuous.
    """
    block = _block_after(text, "var VIOLATIONS = [", "\n  ];")
    rows = []
    for chunk in block.split("{ ruleId: ")[1:]:
        record = chunk.split("}", 1)[0]
        row = dict(re.findall(r'(\w+): "([^"]*)"', record))
        row["ruleId"] = re.match(r'"([^"]+)"', record).group(1)
        rows.append(row)
    return rows


def _required_props(text, kind):
    """Required attribute names of one PROP_DEFS template, in template order."""
    block = _block_after(text, "var PROP_DEFS = {", "\n  };")
    at = block.find("%s: [" % kind)
    assert at != -1, "PROP_DEFS has no %s template" % kind
    end = block.find("]", at)
    assert end != -1, "the %s template has no terminator" % kind
    return [
        name
        for name, required in re.findall(
            r'\{ name: "([^"]+)", required: (true|false) \}', block[at:end]
        )
        if required == "true"
    ]


def _canvas_symbol_extra(text, tag):
    """The props makeSymbol builds for ``<tag>``, as name -> value.

    makeSymbol seeds 位号 and 名称 from its first and sixth arguments and fills
    every other template attribute from the ``extra`` object, defaulting to
    the empty string, so a name absent from ``extra`` is empty.
    """
    at = text.find('makeSymbol("%s"' % tag)
    assert at != -1, "no makeSymbol call places %s on the canvas" % tag
    end = text.find("}),", at)
    assert end != -1, "the makeSymbol call for %s has no terminator" % tag
    call = text[at:end]
    head = re.match(
        r'makeSymbol\("([^"]+)", "[^"]+", "[^"]+", \d+, \d+, "([^"]+)"', call
    )
    assert head, "the makeSymbol call for %s has an unexpected shape" % tag
    props = {"位号": head.group(1), "名称": head.group(2)}
    props.update(dict(re.findall(r'"([^"]+)": "([^"]*)"', call[call.find("{"):])))
    return props


def _instance_props(text, tag):
    """The props object of one INSTANCES record as name -> value."""
    block = _block_after(text, "var INSTANCES = [", "\n  ];")
    at = block.find('{ tag: "%s"' % tag)
    assert at != -1, "INSTANCES has no %s record" % tag
    end = block.find("} }", at)
    assert end != -1, "the %s record has no terminator" % tag
    return dict(re.findall(r'"([^"]+)": "([^"]*)"', block[at:end]))


CANVAS_KIND_LABELS = {
    "equipment": "设备",
    "valve": "阀门",
    "instrument": "仪表",
    "fitting": "管件",
}

# INSTANCES carries the subtype name (容器, 泵, ...) while the drawings seed and
# the BOM rows carry the top-level type. Normalizing keeps one tag described as
# 设备 in one seed and 容器 in another from reading as a type clash.
SUBTYPE_TO_TYPE = {
    "容器": "设备",
    "泵": "设备",
    "换热器": "设备",
    "塔器": "设备",
    "阀门": "阀门",
    "仪表": "仪表",
    "管件": "管件",
    "管路": "管线",
}

# Only the independently tagged types take one literal per object. 管线 and
# 管件 deliberately share a tag family (a 管件 inherits its host line's 位号,
# and requirements/60 keeps 管路 out of the material list altogether), so
# comparing them across the two surfaces would report a clash that is not one.
INDEPENDENT_TAG_TYPES = ("设备", "阀门", "仪表")


def _prototype_tag_types(text):
    """Every seeded 位号 literal mapped to the type words it is bound to.

    Reads the three seed sources that carry a type label next to a tag: the
    drawings seed and the BOM rows (``kind:`` / ``type:`` immediately before
    ``tag:``), the canvas ``makeSymbol`` calls (whose second argument is
    ``equipment``/``valve``/...) and the properties ``INSTANCES`` records
    (``tag:`` followed by ``typeName:``). The canvas kinds are translated
    through ``CANVAS_KIND_LABELS`` so all sources speak one vocabulary. A tag
    bound to several types keeps every one of them, which is what makes a
    collision detectable instead of silently overwritten.
    """
    found = {}

    def add(tag, kind):
        found.setdefault(tag, set()).add(SUBTYPE_TO_TYPE.get(kind, kind))

    for kind, tag in re.findall(r'kind: "([^"]+)", tag: "([^"]+)"', text):
        add(tag, kind)
    for kind, tag in re.findall(r'type: "([^"]+)", tag: "([^"]+)"', text):
        add(tag, kind)
    for tag, kind in re.findall(r'makeSymbol\("([^"]+)", "([a-z]+)", "', text):
        add(tag, CANVAS_KIND_LABELS.get(kind, kind))
    for tag, kind in re.findall(
        r'\{ tag: "([^"]+)", typeId: "[^"]*", typeName: "([^"]+)"', text
    ):
        add(tag, kind)
    return found


def _assert_no_seed_tag_collision(text):
    """No prototype 位号 may be assigned to a type the example table disagrees with."""
    doc = (DOCS / "requirements" / "60-deliverables-and-publish.md").read_text(
        encoding="utf-8"
    )
    table = find_bom_header(doc)
    assert table is not None, (
        "requirements/60 no longer carries the standard material-list table"
    )
    example_tags = {row["位号"]: row["类型"] for row in bom_rows(table)}
    assert example_tags, "the standard table parsed to zero rows"
    assert example_tags.get("V-1001") == "阀门", (
        "the guard assumes requirements/60 binds V-1001 to a 阀门 row, got %r"
        % (example_tags.get("V-1001"),)
    )
    colliding = _prototype_tag_types(text)
    assert colliding, "the prototype seed exposed no tags at all"
    internal = sorted(
        "%s (%s)" % (tag, " / ".join(sorted(kinds)))
        for tag, kinds in colliding.items()
        if len([k for k in kinds if k in INDEPENDENT_TAG_TYPES]) > 1
    )
    assert internal == [], (
        "the prototype binds one literal 位号 to more than one independently "
        "tagged type, so a tag-scoped lookup lands on two objects: %r"
        % (internal,)
    )
    clashes = sorted(
        "%s (%s in the prototype, %s in requirements/60)"
        % (tag, kind, example_tags[tag])
        for tag, kinds in colliding.items()
        for kind in kinds
        if kind in INDEPENDENT_TAG_TYPES
        and example_tags.get(tag) in INDEPENDENT_TAG_TYPES
        and example_tags[tag] != kind
    )
    assert clashes == [], (
        "a literal 位号 names two objects in different type contexts: %r"
        % (clashes,)
    )


def _assert_vessel_tag_is_consistent(text):
    """The 原油缓冲罐 tag is one literal across the drawings, canvas, INSTANCES and SVG."""
    canvas = re.search(
        r'makeSymbol\("([^"]+)", "equipment", "vessel", \d+, \d+, "原油缓冲罐"',
        text,
    )
    assert canvas, "the canvas no longer places the 原油缓冲罐 vessel"
    tag = canvas.group(1)
    sites = {
        "drawings seed": '{ kind: "设备", tag: "%s", name: "原油缓冲罐" }' % tag,
        "BOM_SOURCE row": 'type: "设备", tag: "%s", name: "原油缓冲罐"' % tag,
        "INSTANCES record": '{ tag: "%s", typeId: "vessel", typeName: "容器"' % tag,
        "SVG preview label": '>%s</text>' % tag,
    }
    missing = [name for name, needle in sites.items() if needle not in text]
    assert missing == [], (
        "the vessel tag %r is missing from %r, so the rename was partial"
        % (tag, missing)
    )
    assert tag != "V-1001", (
        "the vessel is back on V-1001, which requirements/60 assigns to a 阀门"
    )


def _assert_rule_names(text):
    """VL_RULE_NAMES binds exactly the nine rules the seed uses."""
    block = _block_after(text, "var VL_RULE_NAMES = {", "\n  };")
    bound = dict(re.findall(r'"(DR-\d{3})": "([^"]+)"', block))
    assert bound == VALIDATION_RULE_NAMES, (
        "VL_RULE_NAMES is %r; the requirement table binds %r"
        % (bound, VALIDATION_RULE_NAMES)
    )


def _assert_severities(text):
    """Every row's severity equals the requirement table's 严重度 column."""
    rows = _violation_rows(text)
    assert len(rows) == VIOLATION_ROW_COUNT, (
        "VIOLATIONS carries %d rows, expected %d"
        % (len(rows), VIOLATION_ROW_COUNT)
    )
    seen = {}
    for row in rows:
        rule_id = row["ruleId"]
        assert rule_id in VALIDATION_SEVERITIES, (
            "%s has a violation row but no expected severity" % rule_id
        )
        seen.setdefault(rule_id, set()).add(row["severity"])
    for rule_id, sev in VALIDATION_SEVERITIES.items():
        assert rule_id in seen, "%s has no violation row" % rule_id
        assert seen[rule_id] == {sev}, (
            "%s is %r, the requirement table says %s"
            % (rule_id, sorted(seen[rule_id]), sev)
        )


def _assert_rows_name_one_object(text):
    """Each parsed row names one object; exactly one row carries P-1001."""
    rows = _violation_rows(text)
    assert len(rows) == VIOLATION_ROW_COUNT, (
        "expected %d violation rows, parsed %d — a row that cannot be parsed "
        "is a row that is not guarded" % (VIOLATION_ROW_COUNT, len(rows))
    )
    for row in rows:
        if row["ruleId"] not in TAG_QUOTING_RULES:
            continue
        assert row["tag"] in row["message"], (
            "%s: 位号 %r does not appear in its message %r — the row names two "
            "different objects" % (row["ruleId"], row["tag"], row["message"])
        )
    for row in rows:
        if row["ruleId"] not in LINE_TAG_RULES:
            continue
        assert re.match(r"^L-\d{4}$", row["tag"]), (
            "%s acts on 管线, so its 位号 must be a line tag such as L-1001, "
            "got %r" % (row["ruleId"], row["tag"])
        )
    tags = [row["tag"] for row in rows]
    assert tags.count("P-1001") == 1, (
        "VAL-CROSS-004 needs exactly one violation whose 位号 is P-1001, "
        "found %d" % tags.count("P-1001")
    )


def _assert_dr010_premise_holds(text):
    """DR-010 must fire on the object it names, per its own DSL.

    requirements/50 defines DR-010 as ``必填项缺失数 > 0``. The row is
    therefore provable only if the attribute its message names is a required
    attribute of the equipment template *and* P-1001 leaves it empty in both
    seed sources while every other required attribute is filled.
    """
    rows = [r for r in _violation_rows(text) if r["ruleId"] == "DR-010"]
    assert len(rows) == 1, "expected exactly one DR-010 row, found %d" % len(rows)
    row = rows[0]
    assert row["tag"] == "P-1001", (
        "the DR-010 row must name P-1001, got %r" % (row["tag"],)
    )
    match = re.search(r"必填项(.+?)为空", row["message"])
    assert match, (
        "DR-010's message must name the missing attribute as 「必填项<名称>为空」, "
        "got %r" % (row["message"],)
    )
    missing = match.group(1)
    required = _required_props(text, "equipment")
    assert missing in required, (
        "DR-010 says %r is an empty required attribute of P-1001, but the "
        "equipment template's required set is %r — 必填项缺失数 would be 0 and "
        "the row could never fire" % (missing, required)
    )
    for source, props in (
        ("canvas seed", _canvas_symbol_extra(text, "P-1001")),
        ("INSTANCES record", _instance_props(text, "P-1001")),
    ):
        value = props.get(missing, "")
        assert value == "", (
            "DR-010 is provable only if %s leaves %r empty on P-1001, but the "
            "%s gives %r" % (row["ruleId"], missing, source, value)
        )
    canvas = _canvas_symbol_extra(text, "P-1001")
    empty = [name for name in required if canvas.get(name, "") == ""]
    assert empty == [missing], (
        "P-1001 must miss exactly the attribute DR-010 names; the empty "
        "required attributes are %r" % (empty,)
    )


def _assert_summary_is_derived(text):
    """The severity summary reads the rows; it is never a literal."""
    body = _block_after(text, "function vlSeverityCounts() {", "\n  }")
    assert "VIOLATIONS[i].severity" in body, (
        "vlSeverityCounts no longer reads VIOLATIONS[i].severity"
    )
    assert "counts[sev] += 1" in body, (
        "vlSeverityCounts no longer increments the bucket it read"
    )
    render = _block_after(text, "function renderValidation(def) {", "\n  }")
    assert "var counts = vlSeverityCounts();" in render, (
        "renderValidation does not call vlSeverityCounts()"
    )
    at = render.find('data-testid="validation-summary"')
    assert at != -1, "the summary block is gone"
    summary = render[at: render.find("</div>", at)]
    for bucket in ("错误", "警告", "提示"):
        assert 'counts["%s"]' % bucket in summary, (
            "the summary does not interpolate counts[%r]; the count is "
            "hard-coded" % bucket
        )
    assert len(re.findall(r"counts\[", summary)) == 3, (
        "the summary interpolates %d derived counts, expected exactly 3"
        % len(re.findall(r"counts\[", summary))
    )


def test_syntax_validation_rule_names_match_the_requirement_table():
    """requirements/50 binds nine rule names; DR-010 is one of them."""
    _assert_rule_names(_screens_text())


def test_syntax_validation_severities_match_the_requirement_table():
    """Every seeded severity matches the rule table, DR-010 = 错误 included."""
    _assert_severities(_screens_text())


def test_syntax_validation_rows_point_at_one_object():
    """requirements/50: the 位号 column must agree with the message it ships.

    Scoped to the rules whose message quotes the object by tag (DR-001,
    DR-002 and DR-010). For DR-001/DR-002 the 作用对象 is 管线, so the tag
    must be a line tag such as L-1001 — never the 管路 tag L-1001-01 and never
    an equipment tag. The other rows are free prose.
    """
    _assert_rows_name_one_object(_screens_text())


def test_syntax_validation_dr010_premise_holds_on_p1001():
    """A seeded violation row must be provable on the object it names."""
    _assert_dr010_premise_holds(_screens_text())


def test_syntax_validation_severity_summary_is_derived():
    """The summary counts are derived from VIOLATIONS, not hard-coded."""
    _assert_summary_is_derived(_screens_text())


def test_syntax_validation_guards_can_fail():
    """Negative control: each validation guard must reject its mutation."""
    text = _screens_text()

    renamed = text.replace('"DR-010": "属性必填项为空"', '"DR-010": "属性缺少必填项"')
    assert renamed != text, "renaming DR-010 changed nothing"
    with pytest.raises(AssertionError):
        _assert_rule_names(renamed)

    flipped = text.replace(
        '{ ruleId: "DR-010", severity: "错误"',
        '{ ruleId: "DR-010", severity: "提示"',
    )
    assert flipped != text, "flipping DR-010's severity changed nothing"
    with pytest.raises(AssertionError):
        _assert_severities(flipped)

    # 建议修复 is optional (REQ-DATA-014): dropping it must not hide the row.
    dropped = text.replace(
        '      message: "管线 L-1001 未填写管道等级",\n'
        '      suggestion: "建议：为 L-1001 指定管道等级。" },',
        '      message: "管线 L-1001 未填写管道等级" },',
    )
    dropped = dropped.replace(
        '{ ruleId: "DR-001", severity: "错误", tag: "L-1001"',
        '{ ruleId: "DR-001", severity: "错误", tag: "P-1001"',
    )
    assert dropped != text, "the suggestion-less DR-001 variant changed nothing"
    assert "suggestion" not in _violation_rows(dropped)[0], (
        "the mutation did not actually drop DR-001's suggestion"
    )
    with pytest.raises(AssertionError):
        _assert_rows_name_one_object(dropped)

    literal = text.replace('counts["错误"] + "</b></span>"', '8 + "</b></span>"')
    assert literal != text, "replacing the summary interpolation changed nothing"
    with pytest.raises(AssertionError):
        _assert_summary_is_derived(literal)

    filled = text.replace(
        '      "设计温度": "120 ℃",\n      "公称直径": "DN80",',
        '      "材质": "Q345R",\n      "设计温度": "120 ℃",\n      "公称直径": "DN80",',
    )
    assert filled != text, "filling 材质 on P-1001 changed nothing"
    with pytest.raises(AssertionError):
        _assert_dr010_premise_holds(filled)


def test_syntax_bom_seed_uses_the_standard_type_vocabulary():
    """requirements/60: 管路 never becomes a row; 管件 inherits the host tag."""
    text = _screens_text()
    block = _block_after(text, "var BOM_SOURCE = [", "\n  ];")
    types = set(re.findall(r'type: "([^"]+)"', block))
    assert "管路" not in types, "BOM still carries a 管路 row: %r" % (types,)
    assert types <= {"管线", "设备", "阀门", "仪表", "管件"}, (
        "unexpected BOM types: %r" % (types,)
    )
    assert "管线" in types, "BOM has no 管线 row"
    assert "-F1" not in block and "-F2" not in block, (
        "BOM still carries per-instance fitting tags"
    )
    assert 'type: "管件", tag: "L-1001-01"' in block, (
        "the fitting row does not inherit the host tag L-1001-01"
    )
    assert 'type: "管线", tag: "L-1001"' in block, (
        "the 管线 row does not carry the line tag L-1001 (the 管件 row "
        "carries L-1001-01, so they must differ)"
    )


def test_syntax_bom_seed_keeps_tags_unique():
    """requirements/60 + VAL-PROTO-016: 位号 is the row key and must be unique."""
    text = _screens_text()
    block = _block_after(text, "var BOM_SOURCE = [", "\n  ];")
    tags = re.findall(r'tag: "([^"]+)"', block)
    assert tags, "BOM_SOURCE carries no tags at all"
    repeated = sorted({t for t in tags if tags.count(t) > 1})
    assert repeated == [], (
        "BOM_SOURCE repeats a 位号, which breaks the row-key uniqueness "
        "invariant of requirements/60 and VAL-PROTO-016: %r" % (repeated,)
    )


def test_syntax_properties_list_filters_by_the_selected_type():
    """REQ-DATA-005 AC2: a parent type lists its subtypes' instances."""
    text = _screens_text()
    assert "function idsUnderType(typeId)" in text, (
        "properties has no descendant-type helper"
    )
    assert "listHtml(state.selectedTypeId)" in text, (
        "listHtml is not called with the selected type"
    )
    assert "allowed.indexOf(inst.typeId) === -1" in text, (
        "listHtml does not filter INSTANCES by type"
    )


def test_syntax_prototype_seed_tags_never_collide_with_the_bom_example_table():
    """One literal 位号 may not name two objects in the same type context.

    requirements/60's standard example table binds V-1001 and V-1002 to 阀门
    rows. The prototype used to bind V-1001 to a 设备 (原油缓冲罐) and V-1002 to
    a 容器 (污油罐), so the same literal named two objects and any
    tag-scoped cross-screen assertion was ambiguous on it. The equipment side
    now uses V-1301 / V-1302; this guard reads the owning document's table and
    rejects every prototype 设备/容器 literal that the table already assigns to
    a different type.
    """
    _assert_no_seed_tag_collision(_screens_text())


def test_syntax_prototype_tag_collision_guard_can_fail():
    """Negative control: restoring V-1001 on the 设备 seed must turn it red."""
    text = _screens_text()
    mutated = text.replace(
        '{ kind: "设备", tag: "V-1301", name: "原油缓冲罐" }',
        '{ kind: "设备", tag: "V-1001", name: "原油缓冲罐" }',
    )
    assert mutated != text, "the mutation did not restore the colliding tag"
    with pytest.raises(AssertionError):
        _assert_no_seed_tag_collision(mutated)

    mutated = text.replace(
        'makeSymbol("V-1301", "equipment", "vessel"',
        'makeSymbol("V-1001", "equipment", "vessel"',
    )
    assert mutated != text, "the canvas-seed mutation changed nothing"
    with pytest.raises(AssertionError):
        _assert_no_seed_tag_collision(mutated)


def test_syntax_prototype_vessel_tag_is_consistent_across_every_seed_site():
    """The 原油缓冲罐 tag must be the same literal in all four seed sites.

    The feature that renamed the vessel off V-1001 had to touch the drawings
    seed, the canvas makeSymbol call, the INSTANCES record and the BOM_SOURCE
    row. A rename that misses one site leaves the prototype describing two
    objects, so this pins all four to one value read from the canvas seed.
    """
    _assert_vessel_tag_is_consistent(_screens_text())


def test_syntax_prototype_vessel_tag_consistency_guard_can_fail():
    """Negative control: a partial rename must be caught site by site."""
    text = _screens_text()
    mutated = text.replace(
        '{ sheet: "PID-1101 原油换热网络.dwg", type: "设备", tag: "V-1301"',
        '{ sheet: "PID-1101 原油换热网络.dwg", type: "设备", tag: "V-9999"',
    )
    assert mutated != text, "the BOM-site mutation changed nothing"
    with pytest.raises(AssertionError):
        _assert_vessel_tag_is_consistent(mutated)

    mutated = text.replace(
        '<text class="pid-tag" x="18" y="49">V-1301</text>',
        '<text class="pid-tag" x="18" y="49">V-9999</text>',
    )
    assert mutated != text, "the SVG-label mutation changed nothing"
    with pytest.raises(AssertionError):
        _assert_vessel_tag_is_consistent(mutated)


def test_syntax_data_correctness_checks_can_fail():
    """Negative control for the four checks above."""
    text = _screens_text()
    mutated = text.replace('"DR-001": "管线缺管道等级"', '"DR-001": "管段未连接到设备管口"')
    mutated = mutated.replace('{ ruleId: "DR-005", severity: "警告"', '{ ruleId: "DR-005", severity: "错误"')
    mutated = mutated.replace('type: "管线", tag: "L-1001"', 'type: "管路", tag: "L-1001"')
    mutated = mutated.replace("function idsUnderType(typeId)", "function idsUnderTypeRenamed(typeId)")
    assert mutated != text, "the mutation changed nothing"

    at = mutated.find("var VL_RULE_NAMES = {")
    assert '"DR-001": "管线缺管道等级"' not in mutated[at: at + 900], (
        "renaming a rule did not break the rule-name check"
    )
    at = mutated.find("var VIOLATIONS = [")
    block = mutated[at: mutated.find("];", at)]
    seen = dict(re.findall(r'\{ ruleId: "(DR-\d{3})", severity: "([^"]+)"', block))
    assert seen.get("DR-005") != "警告", (
        "changing a severity did not break the severity check"
    )
    at = mutated.find("var BOM_SOURCE = [")
    block = mutated[at: mutated.find("\n  ];", at)]
    assert "管路" in set(re.findall(r'type: "([^"]+)"', block)), (
        "renaming a BOM type did not break the type-vocabulary check"
    )
    assert "function idsUnderType(typeId)" not in mutated, (
        "renaming the helper did not break the filter check"
    )


# --- editor and palette screen contracts -------------------------------

EDITOR_REGION_TESTIDS = [
    "ribbon", "palette", "canvas", "property-panel", "statusbar",
]


def _block_after(text, marker, closing):
    """Slice from marker up to the first ``closing`` token after it.

    Slicing on a real terminator instead of a character budget means adding
    entries to the literal never silently truncates the slice and starts
    matching the next block.
    """
    at = text.find(marker)
    assert at != -1, "%s is gone" % marker
    end = text.find(closing, at)
    assert end != -1, "%s has no %r terminator" % (marker, closing)
    return text[at:end]


def test_syntax_editor_regions_carry_their_testids():
    """VAL-PROTO-005: the editor carries exactly five named regions.

    The marker is matched with a leading space so that CSS attribute
    selectors such as ``[data-testid="ribbon"]`` inside delegated listeners
    are not counted as a second mount.
    """
    text = _screens_text()
    for testid in EDITOR_REGION_TESTIDS:
        count = text.count(' data-testid="%s"' % testid)
        assert count == 1, (
            "data-testid=%s is mounted %d times in screens.js, expected 1"
            % (testid, count)
        )


def test_syntax_editor_ribbon_tabs_match_in_order():
    """VAL-PROTO-005: six ribbon tabs, exact text, exact order."""
    text = _screens_text()
    block = _block_after(text, "var RIBBON_TABS = [", "];")
    tabs = re.findall(r'"([^"]+)"', block)
    assert tabs == ["常用", "工具", "显示", "绘图", "注释", "帮助"], (
        "RIBBON_TABS is %r, expected the six documented tabs in order"
        % (tabs,)
    )


def test_syntax_editor_canvas_carries_symbols_and_pipelines():
    """VAL-PROTO-005: at least six placed symbols and three routed pipelines."""
    text = _screens_text()
    sym_block = _block_after(text, "var symbols = [", "];")
    assert sym_block.count('makeSymbol("') >= 6, (
        "the editor canvas seed places fewer than six symbols"
    )
    pipe_block = _block_after(text, "var pipelines = [", "];")
    assert pipe_block.count("points:") >= 3, (
        "the editor canvas seed carries fewer than three pipelines"
    )


def test_syntax_editor_palette_categories_have_distinct_sizes():
    """VAL-PROTO-007: switching any two in-editor tabs changes the item count."""
    text = _screens_text()
    block = _block_after(text, "var ED_SYMBOLS = [", "];")
    counts = {}
    for cat in re.findall(r'category: "([^"]+)"', block):
        counts[cat] = counts.get(cat, 0) + 1
    assert set(counts) == {"equipment", "valve", "instrument", "fitting"}, (
        "ED_SYMBOLS categories are %r, expected the four editor tabs"
        % (sorted(counts),)
    )
    sizes = sorted(counts.values())
    assert len(set(sizes)) == 4, (
        "editor palette category sizes are not pairwise different: %r"
        % (counts,)
    )
    tabs_block = _block_after(text, "var ED_CATEGORIES = [", "];")
    assert tabs_block.count('id: "') >= 4, (
        "ED_CATEGORIES declares fewer than four page tabs"
    )


def test_syntax_palette_screen_lists_at_least_eight_items():
    """VAL-PROTO-007: the standalone palette screen seeds a real grid."""
    text = _screens_text()
    assert text.count('data-testid="palette-item"') == 1, (
        "the palette screen does not render palette-item rows"
    )
    block = _block_after(text, "var SYMBOLS = [", "];")
    favorites = block.count('category: "favorites"')
    assert favorites >= 8, (
        "the default palette category carries %d items, expected >= 8"
        % favorites
    )


def test_syntax_bom_template_changes_the_header_and_rows():
    """REQ-DELV-004: the template selector really swaps the table head."""
    text = _screens_text()
    block = _block_after(text, "var BM_TEMPLATE_DEFS = {", "\n  };")
    assert '"标准材料清单"' in block and '"管件汇总表"' in block and '"阀门汇总表"' in block, (
        "BM_TEMPLATE_DEFS does not define the three templates"
    )
    columns = re.findall(r"columns: \[([^\]]*)\]", block)
    assert len(columns) == 3, "expected three column lists, got %d" % len(columns)
    widths = {len(re.findall(r'"[^"]+"', c)) for c in columns}
    assert widths == {5, 7}, (
        "template column widths are %r, expected the standard seven and the "
        "summary five" % (sorted(widths),)
    )
    assert '"管件"' in block and '"阀门"' in block, (
        "the summary templates do not filter by object type"
    )
    assert '"<thead><tr>" + bmHeadHtml() + "</tr></thead>"' in text, (
        "renderBom does not build the table head from bmHeadHtml()"
    )
    assert "if (def.types && def.types.indexOf(r.type) === -1) {" in text, (
        "bmRowsForScope does not filter rows by the template's types"
    )
    change = _block_after(
        text,
        'var tplSel = closestEl(target, \'[data-testid="bom-template"]\');',
        "});",
    )
    assert "rerenderBom();" in change, (
        "the bom-template change handler does not rerender, so switching the "
        "template cannot change the table"
    )
    assert "BM_TEMPLATES = [" in text and "BM_TEMPLATE_DEFS = {" in text, (
        "the template option list or the template definitions are gone"
    )
    options = re.findall(
        r'"([^"]+)"',
        _block_after(text, "var BM_TEMPLATES = [", "];"),
    )
    defined = re.findall(r'\n    "([^"]+)": \{', block)
    assert sorted(options) == sorted(defined), (
        "the template dropdown offers %r but BM_TEMPLATE_DEFS defines %r"
        % (sorted(options), sorted(defined))
    )


def test_syntax_editor_and_bom_checks_can_fail():
    """Negative control for the six checks above."""
    text = _screens_text()
    mutated = text.replace(
        '{ name: "蝶阀", category: "valve", shape: "butterfly-valve" },', ""
    ).replace(
        '{ name: "安全阀", category: "valve", shape: "relief-valve" },', ""
    )
    mutated = mutated.replace(
        'columns: ["类型", "位号", "名称", "数量", "单位"],\n      types: ["管件"]',
        'columns: ["类型", "位号", "名称", "数量"],\n      types: ["管件"]',
    )
    mutated = mutated.replace(
        '"<thead><tr>" + bmHeadHtml() + "</tr></thead>"',
        '"<thead><tr><th>序号</th><th>类型</th><th>位号</th><th>名称</th><th>数量</th><th>单位</th><th>备注</th></tr></thead>"',
    )
    mutated = mutated.replace(
        'type: "管线", tag: "L-1001"',
        'type: "管线", tag: "L-1001-01"',
    )
    mutated = mutated.replace(
        "if (def.types && def.types.indexOf(r.type) === -1) {",
        "if (false && def.types && def.types.indexOf(r.type) === -1) {",
    )
    mutated = mutated.replace(
        "      bmState.hint = \"已切换模板：\" + bmState.template;\n      rerenderBom();",
        "      bmState.hint = \"已切换模板：\" + bmState.template;",
    )
    assert mutated != text, "the mutation changed nothing"

    block = _block_after(mutated, "var ED_SYMBOLS = [", "];")
    counts = {}
    for cat in re.findall(r'category: "([^"]+)"', block):
        counts[cat] = counts.get(cat, 0) + 1
    assert len(set(counts.values())) != 4, (
        "removing two valve symbols did not break the distinct-size check"
    )
    assert '"<thead><tr>" + bmHeadHtml() + "</tr></thead>"' not in mutated, (
        "reverting the table head to a hard-coded column list did not break "
        "the header check"
    )
    seed = _block_after(mutated, "var BOM_SOURCE = [", "\n  ];")
    tags = re.findall(r'tag: "([^"]+)"', seed)
    assert sorted({t for t in tags if tags.count(t) > 1}) != [], (
        "reusing the 管路 tag on the 管线 row did not break the uniqueness check"
    )
    assert "if (def.types && def.types.indexOf(r.type) === -1) {" not in mutated, (
        "short-circuiting the template type filter did not break the "
        "type-filter check"
    )
    change = _block_after(
        mutated,
        'var tplSel = closestEl(target, \'[data-testid="bom-template"]\');',
        "});",
    )
    assert "rerenderBom();" not in change, (
        "removing the rerender from the change handler did not break the "
        "rerender check"
    )
    tpl = _block_after(mutated, "var BM_TEMPLATE_DEFS = {", "\n  };")
    widths = {len(re.findall(r'"[^"]+"', c)) for c in re.findall(r"columns: \[([^\]]*)\]", tpl)}
    assert widths != {5, 7}, (
        "narrowing a summary template did not break the column-width check"
    )
