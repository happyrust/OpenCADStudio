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


def test_syntax_drawings_pbs_nodes_show_the_four_contract_fields():
    """REQ-DRW-001: PBS nodes expose 节点名称 / 节点类型 / 父节点 / 图纸数."""
    text = _screens_text()
    at = text.find('data-testid="pbs-node"')
    assert at != -1, "the PBS tree renders no pbs-node"
    block = text[at: at + 900]
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
    at = mutated.find('data-testid="pbs-node"')
    assert "节点名称：" not in mutated[at: at + 900], (
        "renaming 节点名称 did not break the PBS check"
    )


def test_syntax_validation_rule_names_match_the_requirement_table():
    """requirements/50 defines DR-001..DR-008; the screen must not rename them."""
    text = _screens_text()
    expected = {
        "DR-001": "管线缺管道等级",
        "DR-002": "管线缺公称直径",
        "DR-003": "设备无位号",
        "DR-004": "位号重复",
        "DR-005": "仪表未分配回路",
        "DR-006": "页间连接符对端缺失",
        "DR-007": "阀门压力等级高于管道等级",
        "DR-008": "管口未连接",
    }
    at = text.find("var VL_RULE_NAMES = {")
    assert at != -1, "VL_RULE_NAMES is gone"
    block = text[at: at + 900]
    for rule_id, name in expected.items():
        assert '"%s": "%s"' % (rule_id, name) in block, (
            "%s is not bound to %s" % (rule_id, name)
        )


def test_syntax_validation_severities_match_the_requirement_table():
    """DR-001..008 severities: 错误 for 001-004/006/007, 警告 005, 提示 008."""
    text = _screens_text()
    at = text.find("var VIOLATIONS = [")
    assert at != -1, "VIOLATIONS is gone"
    block = text[at: text.find("];", at)]
    expected = {
        "DR-001": "错误", "DR-002": "错误", "DR-003": "错误", "DR-004": "错误",
        "DR-005": "警告", "DR-006": "错误", "DR-007": "错误", "DR-008": "提示",
    }
    seen = {}
    for entry in re.findall(r'\{ ruleId: "(DR-\d{3})", severity: "([^"]+)"', block):
        seen.setdefault(entry[0], entry[1])
    for rule_id, sev in expected.items():
        assert rule_id in seen, "%s has no violation row" % rule_id
        assert seen[rule_id] == sev, (
            "%s is %s, the requirement table says %s" % (rule_id, seen[rule_id], sev)
        )


def test_syntax_bom_seed_uses_the_standard_type_vocabulary():
    """requirements/60: 管路 never becomes a row; 管件 inherits the host tag."""
    text = _screens_text()
    at = text.find("var BOM_SOURCE = [")
    assert at != -1, "BOM_SOURCE is gone"
    block = text[at: text.find("\n  ];", at)]
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
    tags = re.findall(r'tag: "([^"]+)"', block)
    repeated = sorted({t for t in tags if tags.count(t) > 1})
    assert repeated == [], (
        "BOM_SOURCE repeats a 位号, which breaks the row-key uniqueness "
        "invariant of requirements/60 and VAL-PROTO-016: %r" % (repeated,)
    )
    assert 'type: "管线", tag: "L-1001"' in block, (
        "the 管线 row does not carry the line tag L-1001 (the 管件 row "
        "carries L-1001-01, so they must differ)"
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
    assert "def.types.indexOf(r.type) === -1" in text, (
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
