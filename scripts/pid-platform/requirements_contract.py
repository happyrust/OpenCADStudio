"""Requirement-ID and traceability contract for the P&ID requirement documents.

The validators are pure functions over ``{relative path: text}`` maps so the
negative controls can build broken documents in memory. ``load_documents`` and
``load_all_documents`` are the only filesystem readers; every error string is
shaped ``"<path>:<line>: <problem>"`` so a failure points at the offending line.

Contract sources:
- ``VAL-REQ-001`` requirement ID format, uniqueness and the per-area floor
- ``VAL-REQ-002`` the five fields every requirement entry must carry
- ``VAL-REQ-003`` the area table and the bidirectional traceability matrix
- ``VAL-REQ-004`` the six platform sections and their mandatory topics
- ``VAL-REQ-005`` the seven type-library sections and their hard constraints
- ``VAL-REQ-013`` one area, one file; the overview only references IDs
"""
from __future__ import annotations

import os
import re
from pathlib import Path

from docs_contract import NEW_DESIGN, allowed_sections, parse_tables
from manifest_contract import normalize_heading

# Nine requirement areas, in the order the contract lists them. The area code
# is also the ID segment, so this list is the single source for both.
REQ_AREAS = ["PLAT", "TYPE", "SYM", "DRW", "DATA", "DELV", "COLL", "IO", "MIG"]

AREA_NAMES = {
    "PLAT": "平台层",
    "TYPE": "类型库",
    "SYM": "符号与项目设置",
    "DRW": "图纸与绘图",
    "DATA": "数据与规则检查",
    "DELV": "交付与发布",
    "COLL": "权限与协同",
    "IO": "导入导出",
    "MIG": "迁移与互操作",
}

REQ_AREA_FILES = {
    "PLAT": "10-platform.md",
    "TYPE": "20-type-library.md",
    "SYM": "30-symbol-library-and-project-settings.md",
    "DRW": "40-drawing-and-drafting.md",
    "DATA": "50-data-and-rule-engine.md",
    "DELV": "60-deliverables-and-publish.md",
    "COLL": "70-collaboration-and-permissions.md",
    "IO": "80-import-export-matrix.md",
    "MIG": "90-migration-and-interop.md",
}

AREA_FILE_OF = {area: "requirements/" + name for area, name in REQ_AREA_FILES.items()}

OVERVIEW_PATH = "requirements/00-overview.md"
REUSE_MAPPING_PATH = "requirements/95-reuse-mapping.md"
REFERENCE_ONLY_PATHS = {OVERVIEW_PATH, REUSE_MAPPING_PATH}

AREA_FLOOR = 5
NO_INTERFACE = "无界面"

# The anchored pattern is normative: the negative lookbehind stops
# "VAL-REQ-001" from being read as a product requirement, the lookahead stops a
# four-digit number from matching its first three digits.
REQ_ID_TEXT = (
    r"(?<![A-Za-z-])REQ-(PLAT|TYPE|SYM|DRW|DATA|DELV|COLL|IO|MIG)-\d{3}(?!\d)"
)
REQ_ID_RE = re.compile(REQ_ID_TEXT)

# Deliberately looser: it finds anything shaped like a requirement ID so a
# wrong area code or a two-digit number is reported instead of ignored.
LOOSE_REQ_RE = re.compile(r"(?<![A-Za-z-])REQ-[A-Za-z]+-\d+")

REQ_HEADING_RE = re.compile(
    r"^###[ \t]+(?P<id>REQ-(?P<area>[A-Za-z]+)-(?P<number>\d+))[ \t]*(?P<title>.*)$"
)
ANY_HEADING_RE = re.compile(r"^(#{1,6})[ \t]+(.+?)[ \t]*$")

MATRIX_COLUMNS = ["需求 ID", "区域", "屏幕 ID 或 无界面", "说明"]
CONTRACT_COLUMNS = ["字段", "类型", "必填", "约束"]
FIELD_NAMES = ["优先级", "用户故事", "验收标准", "数据与界面契约", "来源"]
PRIORITIES = ["必须", "应该", "可以"]
SOURCE_SECTION_RE = re.compile(r"^§\d+(?:\.\d+)+$")
FIELD_LINE_RE = re.compile(r"^[-*]\s+\*\*(?P<name>[^*]+)\*\*[ \t]*[：:][ \t]*(?P<value>.*)$")
NUMBERED_ITEM_RE = re.compile(r"^\s*\d+[.、)][ \t]*\S")
# The marker a numbered acceptance criterion opens with, in any of the three
# forms the documents use. Stripped before decidability is judged, because the
# marker's own digit is numbering, not evidence.
LIST_NUMBER_RE = re.compile(r"^\s*\d+[.、)][ \t]*")

PLATFORM_SECTIONS = [
    "身份与会话",
    "角色与成员管理",
    "项目空间",
    "权限模型",
    "审计日志",
    "系统配置",
]
TYPE_SECTIONS = [
    "业务类型树",
    "属性分组",
    "数据要素与数据类型",
    "属性级别与默认值",
    "属性拼接",
    "类型与符号的关系",
    "规则继承",
]

# A criterion is decidable when it carries a number, a named command/field in
# backticks, or an observable state. The words below are the observable-state
# vocabulary the contract itself uses. Each one names a state a reviewer can
# look at (a value, a list, a precedence outcome) rather than an opinion.
OBSERVABLE_WORDS = [
    "显示",
    "返回",
    "出现",
    "变为",
    "为空",
    "行数",
    "增加",
    "减少",
    "禁止",
    "拒绝",
    "只读",
    "隐藏",
    "置为",
    "自动",
    # Added when the list-number prefix stopped counting as a digit: two shipped
    # criteria state their outcome in these terms only (REQ-PLAT-006 criterion 1
    # inherits the role's permission items; REQ-TYPE-015 criterion 1 gives the
    # child rule precedence over the parent). No other criterion depends on them.
    "继承",
    "优先于",
]


# ---------------------------------------------------------------------------
# Filesystem readers
# ---------------------------------------------------------------------------


def load_documents(docs_root) -> dict:
    """Every markdown file under ``<docs_root>/requirements`` as rel-path -> text."""
    root = Path(docs_root) / "requirements"
    documents = {}
    if not root.is_dir():
        return documents
    for path in sorted(root.rglob("*.md")):
        name = "requirements/" + str(path.relative_to(root)).replace(os.sep, "/")
        documents[name] = path.read_text(encoding="utf-8")
    return documents


def load_all_documents(docs_root) -> dict:
    """Every markdown file under ``docs_root`` as rel-path -> text."""
    root = Path(docs_root)
    documents = {}
    if not root.is_dir():
        return documents
    for path in sorted(root.rglob("*.md")):
        name = str(path.relative_to(root)).replace(os.sep, "/")
        documents[name] = path.read_text(encoding="utf-8")
    return documents


# ---------------------------------------------------------------------------
# Requirement entries
# ---------------------------------------------------------------------------


def _line_of(text: str, offset: int) -> int:
    return text.count("\n", 0, offset) + 1


def split_entries(text: str) -> list:
    """Return every ``### REQ-<AREA>-<NNN>`` entry with its body and line.

    A body ends at the next heading of level 3 or higher so a requirement that
    is followed by a sibling entry or by an H2 does not inherit its lines.
    """
    lines = text.splitlines()
    entries = []
    current = None
    for index, line in enumerate(lines, start=1):
        heading = ANY_HEADING_RE.match(line)
        if heading:
            level = len(heading.group(1))
            if level <= 3:
                if current is not None:
                    current["body"] = "\n".join(current.pop("_body"))
                    entries.append(current)
                    current = None
                if level == 3:
                    match = REQ_HEADING_RE.match(line)
                    if match:
                        current = {
                            "id": match.group("id"),
                            "area": match.group("area"),
                            "number": match.group("number"),
                            "title": match.group("title").strip(),
                            "line": index,
                            "_body": [],
                        }
                continue
        if current is not None:
            current["_body"].append(line)
    if current is not None:
        current["body"] = "\n".join(current.pop("_body"))
        entries.append(current)
    return entries


def parse_entry_fields(body: str) -> dict:
    """Split an entry body into ``{field name: value text}`` blocks."""
    fields = {}
    current = None
    for line in body.splitlines():
        match = FIELD_LINE_RE.match(line.strip())
        if match:
            current = match.group("name").strip()
            fields[current] = [match.group("value")]
        elif current is not None:
            fields[current].append(line)
    return {name: "\n".join(lines).strip() for name, lines in fields.items()}


def definitions(documents: dict) -> dict:
    """Map every defined requirement ID to its location and metadata."""
    defined = {}
    for path in sorted(documents):
        if not path.startswith("requirements/"):
            continue
        for entry in split_entries(documents[path]):
            defined.setdefault(
                entry["id"],
                {
                    "path": path,
                    "line": entry["line"],
                    "area": entry["area"],
                    "title": entry["title"],
                    "entries": [],
                },
            )
            defined[entry["id"]]["entries"].append(entry)
    return defined


def strip_list_number(criterion: str) -> str:
    """Drop a ``1.`` / ``1、`` / ``1)`` list marker from an acceptance criterion.

    ``validate_entry`` receives the whole stripped line, so the criterion's own
    numbering would otherwise satisfy the "carries a number" branch for any
    text at all, including pure sentiment.
    """
    return LIST_NUMBER_RE.sub("", criterion, count=1).strip()


def _is_observable(criterion: str) -> bool:
    body = strip_list_number(criterion)
    if any(char.isdigit() for char in body):
        return True
    if "`" in body:
        return True
    return any(word in body for word in OBSERVABLE_WORDS)


def validate_entry(entry: dict, path: str, allowed=None) -> list:
    """Validate the five contractual fields of one requirement entry."""
    errors = []
    where = "%s:%d" % (path, entry["line"])
    fields = parse_entry_fields(entry["body"])

    for name in FIELD_NAMES:
        if name not in fields or not fields[name].strip():
            errors.append("%s: %s is missing field '%s'" % (where, entry["id"], name))
    if errors:
        return errors

    priority = fields["优先级"].strip()
    if priority not in PRIORITIES:
        errors.append(
            "%s: %s has priority '%s'; expected one of %s"
            % (where, entry["id"], priority, "/".join(PRIORITIES))
        )

    story = fields["用户故事"]
    if not story.startswith("作为") or "我希望" not in story or "以便" not in story:
        errors.append(
            "%s: %s user story is not shaped like 作为…我希望…以便…"
            % (where, entry["id"])
        )

    criteria = [
        line.strip()
        for line in fields["验收标准"].splitlines()
        if NUMBERED_ITEM_RE.match(line)
    ]
    if len(criteria) < 2:
        errors.append(
            "%s: %s has %d numbered acceptance criteria; at least 2 are required"
            % (where, entry["id"], len(criteria))
        )
    for index, criterion in enumerate(criteria, start=1):
        if not _is_observable(criterion):
            errors.append(
                "%s: %s acceptance criterion %d has no number, named command or "
                "observable state: %s"
                % (where, entry["id"], index, criterion[:60])
            )

    contract_tables = [
        table
        for table in parse_tables(fields["数据与界面契约"])
        if table["header"] == CONTRACT_COLUMNS
    ]
    if not contract_tables:
        headers = [table["header"] for table in parse_tables(fields["数据与界面契约"])]
        errors.append(
            "%s: %s data/UI contract has no table with header %s (found %s)"
            % (where, entry["id"], " | ".join(CONTRACT_COLUMNS), headers or "none")
        )
    elif not contract_tables[0]["rows"]:
        errors.append(
            "%s: %s data/UI contract table has no data rows" % (where, entry["id"])
        )

    # The source field is a single line; an explanatory table may follow it and
    # still belongs to the entry, so only the first line is the citation.
    source = fields["来源"].strip().splitlines()[0].strip().strip("`").strip()
    if source == NEW_DESIGN:
        pass
    elif not SOURCE_SECTION_RE.match(source):
        errors.append(
            "%s: %s source '%s' is neither a §section nor %s"
            % (where, entry["id"], source, NEW_DESIGN)
        )
    else:
        section = source[1:]
        if allowed is not None and section not in set(allowed):
            errors.append(
                "%s: %s cites §%s, which is not in the verified section whitelist"
                % (where, entry["id"], section)
            )
    return errors


def validate_entries(documents: dict, allowed=None) -> list:
    errors = []
    for path in sorted(documents):
        if not path.startswith("requirements/"):
            continue
        for entry in split_entries(documents[path]):
            errors.extend(validate_entry(entry, path, allowed))
    return errors


# ---------------------------------------------------------------------------
# ID format, uniqueness and area/file correspondence
# ---------------------------------------------------------------------------


def validate_requirement_ids(documents: dict) -> list:
    """Malformed IDs, duplicate definitions, and wrong-file definitions."""
    errors = []

    for path in sorted(documents):
        if not path.startswith("requirements/"):
            continue
        for number, line in enumerate(documents[path].splitlines(), start=1):
            for match in LOOSE_REQ_RE.finditer(line):
                token = match.group(0)
                if not REQ_ID_RE.fullmatch(token):
                    errors.append(
                        "%s:%d: requirement ID '%s' does not match the anchored "
                        "pattern %s"
                        % (path, number, token, REQ_ID_TEXT)
                    )

    by_id = {}
    for path in sorted(documents):
        for entry in split_entries(documents[path]):
            if not path.startswith("requirements/"):
                errors.append(
                    "%s:%d: requirement '%s' is defined outside the requirements "
                    "directory"
                    % (path, entry["line"], entry["id"])
                )
                continue
            by_id.setdefault(entry["id"], []).append((path, entry["line"]))

    for req_id in sorted(by_id):
        locations = by_id[req_id]
        if len(locations) > 1:
            rendered = ", ".join("%s:%d" % loc for loc in locations)
            errors.append(
                "%s:%d: requirement ID '%s' is defined %d times (%s)"
                % (locations[0][0], locations[0][1], req_id, len(locations), rendered)
            )

    for req_id, locations in sorted(by_id.items()):
        area = req_id.split("-")[1]
        for path, line in locations:
            name = path.split("/")[-1]
            if path in REFERENCE_ONLY_PATHS:
                errors.append(
                    "%s:%d: %s may reference requirement IDs but must not define "
                    "'%s'"
                    % (path, line, name, req_id)
                )
                continue
            expected = REQ_AREA_FILES.get(area)
            if expected is None:
                errors.append(
                    "%s:%d: requirement ID '%s' uses an unknown area segment"
                    % (path, line, req_id)
                )
            elif name != expected:
                errors.append(
                    "%s:%d: requirement ID '%s' belongs in %s, not %s"
                    % (path, line, req_id, "requirements/" + expected, path)
                )
    return errors


def validate_area_floors(documents: dict, require_all: bool = False) -> list:
    """Every authored area document must define at least five requirements.

    ``require_all`` is for the closeout state, where all nine area documents
    exist. During M2 only PLAT and TYPE are authored, so the default checks the
    documents that exist instead of failing on the seven planned ones.
    """
    errors = []
    for area in REQ_AREAS:
        path = AREA_FILE_OF[area]
        if path not in documents:
            if require_all:
                errors.append("%s: area document %s is missing" % (area, path))
            continue
        entries = [entry for entry in split_entries(documents[path]) if entry["area"] == area]
        if len(entries) < AREA_FLOOR:
            errors.append(
                "%s: area %s defines %d requirements; at least %d are required"
                % (path, area, len(entries), AREA_FLOOR)
            )
    return errors


# ---------------------------------------------------------------------------
# Overview tables
# ---------------------------------------------------------------------------


def find_area_table(text: str):
    """The first table whose first two columns are 区域 / 文档."""
    for table in parse_tables(text):
        header = table["header"]
        if len(header) >= 2 and header[0] == "区域" and header[1] == "文档":
            return table
    return None


def find_matrix_table(text: str):
    """The traceability matrix: four columns with the contract header."""
    for table in parse_tables(text):
        if table["header"] == MATRIX_COLUMNS:
            return table
    return None


def validate_area_table(text: str, path: str = OVERVIEW_PATH) -> list:
    errors = []
    table = find_area_table(text)
    if table is None:
        return ["%s: no table with columns 区域 / 文档" % path]
    seen = {}
    for index, row in enumerate(table["rows"]):
        line = table["line"] + 2 + index
        if len(row) < 2:
            errors.append("%s:%d: area row has fewer than 2 cells" % (path, line))
            continue
        area, document = row[0].strip(), row[1].strip()
        if area not in REQ_AREAS:
            errors.append("%s:%d: unknown area code '%s'" % (path, line, area))
            continue
        if area in seen:
            errors.append(
                "%s:%d: area %s mapped twice (already on line %d)"
                % (path, line, area, seen[area])
            )
        seen[area] = line
        expected = REQ_AREA_FILES[area]
        if expected not in document:
            errors.append(
                "%s:%d: area %s must map to %s, found '%s'"
                % (path, line, area, expected, document)
            )
    if len(table["rows"]) != len(REQ_AREAS):
        errors.append(
            "%s: area table has %d rows; exactly %d are required"
            % (path, len(table["rows"]), len(REQ_AREAS))
        )
    missing = [area for area in REQ_AREAS if area not in seen]
    if missing:
        errors.append(
            "%s: area table omits %s" % (path, ", ".join(missing))
        )
    return errors


def matrix_rows(text: str, path: str = OVERVIEW_PATH) -> list:
    """Return matrix rows as dicts with the source line number."""
    table = find_matrix_table(text)
    if table is None:
        return []
    rows = []
    for index, row in enumerate(table["rows"]):
        cells = list(row) + [""] * (len(MATRIX_COLUMNS) - len(row))
        rows.append(
            {
                "id": cells[0].strip(),
                "area": cells[1].strip(),
                "screen": cells[2].strip(),
                "note": cells[3].strip(),
                "line": table["line"] + 2 + index,
            }
        )
    return rows


def validate_matrix(text: str, defined: dict, screen_ids, path: str = OVERVIEW_PATH) -> list:
    """Bidirectional closure, area agreement and screen values of the matrix."""
    errors = []
    if find_matrix_table(text) is None:
        return [
            "%s: no traceability matrix with header %s"
            % (path, " | ".join(MATRIX_COLUMNS))
        ]

    allowed_screens = set(screen_ids) | {NO_INTERFACE}
    rows = matrix_rows(text, path)
    seen = {}
    for row in rows:
        req_id = row["id"]
        if not REQ_ID_RE.fullmatch(req_id):
            errors.append(
                "%s:%d: matrix row '%s' is not a valid requirement ID"
                % (path, row["line"], req_id)
            )
        if req_id in seen:
            errors.append(
                "%s:%d: matrix row repeats %s (already on line %d)"
                % (path, row["line"], req_id, seen[req_id])
            )
        seen[req_id] = row["line"]

        expected_area = req_id.split("-")[1] if req_id.count("-") >= 2 else ""
        if row["area"] != expected_area:
            errors.append(
                "%s:%d: matrix row %s declares area '%s'; the ID segment is '%s'"
                % (path, row["line"], req_id, row["area"], expected_area)
            )
        if row["screen"] not in allowed_screens:
            errors.append(
                "%s:%d: matrix row %s has unknown screen '%s'; expected one of "
                "the %d screen ids or %s"
                % (path, row["line"], req_id, row["screen"], len(screen_ids), NO_INTERFACE)
            )
        if row["screen"] == NO_INTERFACE and len(row["note"]) < 8:
            errors.append(
                "%s:%d: matrix row %s is %s but its 说明 does not state a reason"
                % (path, row["line"], req_id, NO_INTERFACE)
            )

    defined_ids = set(defined)
    matrix_ids = {row["id"] for row in rows}
    for req_id in sorted(defined_ids - matrix_ids):
        location = defined[req_id]
        errors.append(
            "%s:%d: %s is defined but missing from the traceability matrix"
            % (location["path"], location["line"], req_id)
        )
    for row in rows:
        if row["id"] not in defined_ids:
            errors.append(
                "%s:%d: matrix row %s has no requirement definition"
                % (path, row["line"], row["id"])
            )
    return errors


# ---------------------------------------------------------------------------
# Section coverage (VAL-REQ-004 / VAL-REQ-005)
# ---------------------------------------------------------------------------


def section_requirement_counts(text: str) -> dict:
    """{normalized H2 heading: number of direct REQ entries beneath it}."""
    counts = {}
    current = None
    for line in text.splitlines():
        heading = ANY_HEADING_RE.match(line)
        if heading:
            level = len(heading.group(1))
            if level == 1:
                current = None
                continue
            if level == 2:
                current = normalize_heading(heading.group(2))
                counts.setdefault(current, 0)
                continue
        if current is not None and REQ_HEADING_RE.match(line):
            counts[current] += 1
    return counts


def section_text(text: str, heading: str) -> str:
    """Raw body of the H2 named ``heading`` (normalized comparison)."""
    wanted = normalize_heading(heading)
    current = None
    buffer = []
    for line in text.splitlines():
        match = ANY_HEADING_RE.match(line)
        if match:
            level = len(match.group(1))
            if level <= 2:
                if current == wanted:
                    return "\n".join(buffer)
                current = normalize_heading(match.group(2)) if level == 2 else None
                buffer = []
                continue
        if current == wanted:
            buffer.append(line)
    if current == wanted:
        return "\n".join(buffer)
    return ""


def validate_sections(text: str, sections, path: str, minimum: int = 1) -> list:
    errors = []
    counts = section_requirement_counts(text)
    for name in sections:
        key = normalize_heading(name)
        if key not in counts:
            errors.append(
                "%s: missing section '%s' (found %s)"
                % (path, name, sorted(counts))
            )
        elif counts[key] < minimum:
            errors.append(
                "%s: section '%s' has %d requirements; at least %d required"
                % (path, name, counts[key], minimum)
            )
    return errors


def validate_platform_coverage(documents: dict) -> list:
    """VAL-REQ-004: six sections plus the three mandatory identity topics."""
    path = AREA_FILE_OF["PLAT"]
    text = documents.get(path)
    if text is None:
        return ["%s: platform requirement document is missing" % path]
    errors = validate_sections(text, PLATFORM_SECTIONS, path)
    identity = section_text(text, "身份与会话")
    for topic in ["登录失败锁定", "密码有效期", "找回密码"]:
        if topic not in identity:
            errors.append("%s: 身份与会话 does not cover %s" % (path, topic))
    authorization = section_text(text, "权限模型")
    for token in ["角色权限", "PBS 节点", "图纸状态"]:
        if token not in authorization:
            errors.append(
                "%s: 权限模型 does not state the three-level chain component %s"
                % (path, token)
            )
    audit = section_text(text, "审计日志")
    if "保留" not in audit:
        errors.append("%s: 审计日志 does not state a retention policy" % path)
    return errors


def validate_type_library_coverage(documents: dict) -> list:
    """VAL-REQ-005: seven sections plus the five hard constraints."""
    path = AREA_FILE_OF["TYPE"]
    text = documents.get(path)
    if text is None:
        return ["%s: type-library requirement document is missing" % path]
    errors = validate_sections(text, TYPE_SECTIONS, path)
    tree = section_text(text, "业务类型树")
    if "类型编码" not in tree or not any(
        word in tree for word in ["不可修改", "不可改"]
    ):
        errors.append("%s: 业务类型树 does not state that 类型编码 is immutable" % path)
    if "子节点" not in tree or "删除" not in tree:
        errors.append("%s: 业务类型树 does not state the cascade delete rule" % path)
    level = section_text(text, "属性级别与默认值")
    for value in ["可选信息", "必选信息"]:
        if value not in level:
            errors.append("%s: 属性级别与默认值 does not name %s" % (path, value))
    concatenation = section_text(text, "属性拼接")
    if "&" not in concatenation:
        errors.append("%s: 属性拼接 does not use & as the connector" % path)
    if "单引号" not in concatenation:
        errors.append("%s: 属性拼接 does not require single quotes for constants" % path)
    elements = section_text(text, "数据要素与数据类型")
    if "全局唯一" not in elements:
        errors.append("%s: 数据要素与数据类型 does not state global uniqueness" % path)
    return errors


# ---------------------------------------------------------------------------
# Symbol library & project settings (VAL-REQ-006)
# ---------------------------------------------------------------------------

# The thirteen sections VAL-REQ-006 names, in the order the manifest declares
# them. Each must carry at least one requirement entry.
SYMBOL_SECTIONS = [
    "图例标准",
    "符号分类",
    "符号属性",
    "符号编辑器",
    "连接点",
    "符号标签",
    "位号规则",
    "颜色规则",
    "公称直径",
    "管道等级",
    "绝热等级",
    "流体介质",
    "相交断线与打断符",
]

CONNECTION_POINT_COLUMNS = ["类型", "方向", "用途"]
CONNECTION_POINT_TYPES = ["等径", "异径", "信号线"]
CONNECTION_POINT_DIRECTIONS = ["双向点", "流入点", "流出点"]
CONNECTION_POINT_COMBINATIONS = 9

# VAL-DOC-009 makes this document the authoritative definition site for the
# tag-rule field names. The list is the set the prototype and the data document
# must reproduce character-identically; the synonym below must never stand in
# for the canonical name inside this same document.
TAG_RULE_FIELDS = [
    "格式名称",
    "起始编码",
    "增量",
    "后缀递增",
    "层级选择",
    "拼接字段",
    "位号唯一校验",
    "自动弹窗",
]
TAG_RULE_SYNONYMS = ["位号唯一性校验"]

COLOR_RULE_ELEMENTS = ["属性", "运算符", "值", "连接符"]
INTERSECTION_TOKENS = [
    "夹套管",
    "在役次工艺管线",
    "权重",
    "打断符样式",
    "打断符高度",
]


def _connection_type(cell: str) -> str:
    """Strip an optional 连接点 suffix so 等径 and 等径连接点 compare equal."""
    text = cell.strip()
    if text.endswith("连接点"):
        text = text[: -len("连接点")]
    return text.strip()


def validate_connection_point_table(text: str, path: str = None) -> list:
    """The 3 types x 3 directions table required by VAL-REQ-006."""
    path = path or AREA_FILE_OF["SYM"]
    errors = []
    table = None
    for candidate in parse_tables(text):
        if candidate["header"] == CONNECTION_POINT_COLUMNS:
            table = candidate
            break
    if table is None:
        return [
            "%s: 连接点 has no table with columns %s"
            % (path, " | ".join(CONNECTION_POINT_COLUMNS))
        ]

    if len(table["rows"]) != CONNECTION_POINT_COMBINATIONS:
        errors.append(
            "%s: 连接点 table has %d rows; exactly %d combinations are required"
            % (path, len(table["rows"]), CONNECTION_POINT_COMBINATIONS)
        )

    seen = {}
    for index, row in enumerate(table["rows"]):
        line = table["line"] + 2 + index
        if len(row) < 3:
            errors.append("%s:%d: 连接点 row has fewer than 3 cells" % (path, line))
            continue
        kind = _connection_type(row[0])
        direction = row[1].strip()
        if kind not in CONNECTION_POINT_TYPES:
            errors.append(
                "%s:%d: unknown connection point type '%s'; expected one of %s"
                % (path, line, row[0].strip(), "/".join(CONNECTION_POINT_TYPES))
            )
            continue
        if direction not in CONNECTION_POINT_DIRECTIONS:
            errors.append(
                "%s:%d: unknown connection point direction '%s'; expected one of %s"
                % (path, line, row[1].strip(), "/".join(CONNECTION_POINT_DIRECTIONS))
            )
            continue
        if (kind, direction) in seen:
            errors.append(
                "%s:%d: connection point %s/%s repeats the row on line %d"
                % (path, line, kind, direction, seen[(kind, direction)])
            )
        elif not row[2].strip():
            errors.append(
                "%s:%d: connection point %s/%s has no 用途"
                % (path, line, kind, direction)
            )
        seen[(kind, direction)] = line

    for kind in CONNECTION_POINT_TYPES:
        for direction in CONNECTION_POINT_DIRECTIONS:
            if (kind, direction) not in seen:
                errors.append(
                    "%s: 连接点 table omits the %s/%s combination" % (path, kind, direction)
                )
    return errors


def validate_symbol_library_coverage(documents: dict) -> list:
    """VAL-REQ-006: thirteen sections, nine connection points, eight tag fields."""
    path = AREA_FILE_OF["SYM"]
    text = documents.get(path)
    if text is None:
        return ["%s: symbol library requirement document is missing" % path]

    errors = validate_sections(text, SYMBOL_SECTIONS, path)
    errors.extend(validate_connection_point_table(section_text(text, "连接点"), path))

    tag_rules = section_text(text, "位号规则")
    for name in TAG_RULE_FIELDS:
        if name not in tag_rules:
            errors.append("%s: 位号规则 does not define the field '%s'" % (path, name))
    for synonym in TAG_RULE_SYNONYMS:
        if synonym in tag_rules:
            errors.append(
                "%s: 位号规则 uses the synonym '%s'; the canonical field name is '位号唯一校验'"
                % (path, synonym)
            )

    colors = section_text(text, "颜色规则")
    for element in COLOR_RULE_ELEMENTS:
        if element not in colors:
            errors.append(
                "%s: 颜色规则 does not state the condition element '%s'" % (path, element)
            )
    if "继承" not in colors:
        errors.append("%s: 颜色规则 does not state the inheritance rule" % path)

    crossing = section_text(text, "相交断线与打断符")
    for token in INTERSECTION_TOKENS:
        if token not in crossing:
            errors.append("%s: 相交断线与打断符 does not state '%s'" % (path, token))
    return errors


# ---------------------------------------------------------------------------
# Drawing management (VAL-REQ-007)
# ---------------------------------------------------------------------------

# The thirteen sections VAL-REQ-007 names, in manifest order. Each must carry
# at least one requirement entry.
DRAWING_SECTIONS = [
    "PBS 层级",
    "图纸生命周期",
    "图纸版本与比对",
    "管线与管路层级",
    "管件与阀门插入",
    "设备与管口",
    "仪表与仪表回路",
    "信号线",
    "页间连接符",
    "打断符",
    "标注",
    "对象复制",
    "草图设置与用户环境",
]

# VAL-REQ-007's four mandatory semantics, plus the ones the feature contract
# makes load-bearing: the many-to-many loops, the connector-kind matrix, the
# lifecycle operation set and the three-level pipe hierarchy.
DRAWING_SEMANTICS = [
    ("管件与阀门插入", ["自动断开", "自动补位"]),
    ("设备与管口", ["继承设备位号", "逻辑连接点", "属性共享"]),
    (
        "页间连接符",
        [
            "管道连接符流入",
            "管道连接符流出",
            "信号线连接符流入",
            "信号线连接符流出",
        ],
    ),
    ("草图设置与用户环境", ["随图纸", "随用户"]),
    ("管线与管路层级", ["管段", "下传"]),
    ("仪表与仪表回路", ["多对多", "新建仪表回路", "添加仪表", "移除仪表"]),
    ("图纸生命周期", ["导入", "SVG", "DWG", "复制", "移动", "权限", "预览"]),
    ("图纸版本与比对", ["快照", "比对"]),
    ("对象复制", ["跨图纸"]),
]


def validate_drawing_coverage(documents: dict) -> list:
    """VAL-REQ-007: thirteen sections and the mandatory drawing semantics."""
    path = AREA_FILE_OF["DRW"]
    text = documents.get(path)
    if text is None:
        return ["%s: drawing requirement document is missing" % path]

    errors = validate_sections(text, DRAWING_SECTIONS, path)
    for section, tokens in DRAWING_SEMANTICS:
        body = section_text(text, section)
        for token in tokens:
            if token not in body:
                errors.append(
                    "%s: %s does not state '%s'" % (path, section, token)
                )
    return errors


# ---------------------------------------------------------------------------
# Whole-contract validation
# ---------------------------------------------------------------------------


def validate_traceability(
    documents: dict,
    screen_ids,
    require_all_areas: bool = False,
    allowed=None,
) -> list:
    """Every requirement/traceability rule at once, over a document map."""
    errors = []
    errors.extend(validate_requirement_ids(documents))
    errors.extend(validate_area_floors(documents, require_all=require_all_areas))

    overview = documents.get(OVERVIEW_PATH)
    if overview is None:
        errors.append("%s: requirement overview is missing" % OVERVIEW_PATH)
    else:
        errors.extend(validate_area_table(overview))
        errors.extend(
            validate_matrix(overview, definitions(documents), screen_ids)
        )
        if REQ_ID_TEXT not in overview:
            errors.append(
                "%s: the anchored requirement-ID pattern is not documented"
                % OVERVIEW_PATH
            )

    errors.extend(validate_entries(documents, allowed=allowed))
    return sorted(errors)


def whitelisted_sections():
    """The verified manual section numbers, or None if the fixture is unreadable."""
    try:
        return set(allowed_sections())
    except (OSError, ValueError):
        return None
