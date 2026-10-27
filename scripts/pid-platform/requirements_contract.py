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

import rule_dsl

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
# Data management & design-rule checking (VAL-REQ-008)
# ---------------------------------------------------------------------------

# The nine sections VAL-REQ-008 names, in manifest order. Each must carry at
# least one requirement entry.
DATA_SECTIONS = [
    "位号主数据与唯一性",
    "位号冲突处理",
    "对象属性管理器",
    "检索与查询",
    "规则引擎模型",
    "规则 DSL",
    "触发时机",
    "结果模型",
    "设计规则清单",
]

RULE_TABLE_COLUMNS = ["规则 ID", "名称", "作用对象", "严重度", "DSL 表达式"]
RULE_SEVERITIES = ["错误", "警告", "提示"]
MIN_DESIGN_RULES = 20

# The five result fields VAL-REQ-008 makes load-bearing. The severity enum must
# be exactly 错误/警告/提示 -- no fourth level, no renamed middle level.
RESULT_FIELDS = ["规则 ID", "对象引用", "严重度", "消息", "建议修复"]
RESULT_TABLE_COLUMNS = CONTRACT_COLUMNS

# The three trigger modes and the column that states each one's blocking
# behaviour. VAL-REQ-008 requires the modes to be distinguished, so a document
# that lists them without a 阻塞行为 column has not answered the question.
TRIGGER_MODES = ["按需", "保存前", "发布前"]
TRIGGER_COLUMNS = ["触发时机", "执行范围", "阻塞行为", "结果处理"]

# Terminals the EBNF block must show, quoted as EBNF terminals. Parentheses are
# listed because VAL-REQ-008 requires grouping to be part of the grammar.
GRAMMAR_PRODUCTIONS = list(rule_dsl.PRODUCTIONS)
GRAMMAR_TERMINALS = [
    '"AND"',
    '"OR"',
    '"NOT"',
    '"IN"',
    '"NOT IN"',
    '"MATCH"',
    '"NOT MATCH"',
    '"="',
    '"!="',
    '">"',
    '"<"',
    '">="',
    '"<="',
    '"NULL"',
    '"("',
    '")"',
]

# The 25 rules the document ships, in table order. This is the single source the
# synthetic positive control builds from and the real document is compared to;
# ``REQUIRED_DESIGN_RULES`` below derives the tolerant per-rule tokens from it.
SHIPPED_DESIGN_RULES = [
    ("DR-001", "管线缺管道等级", "管线", "错误", "管道等级 = NULL"),
    ("DR-002", "管线缺公称直径", "管线", "错误", "公称直径 = NULL"),
    ("DR-003", "设备无位号", "设备", "错误", '位号 = NULL OR 位号 = ""'),
    ("DR-004", "位号重复", "对象实例", "错误", "位号.同类型重复数 > 1"),
    ("DR-005", "仪表未分配回路", "仪表", "警告", "仪表回路 = NULL"),
    ("DR-006", "页间连接符对端缺失", "页间连接符", "错误", "对端图纸 = NULL OR 对端位号 = NULL"),
    ("DR-007", "阀门压力等级高于管道等级", "阀门", "错误", "压力等级 > 所属管线.管道等级.最大公称压力"),
    ("DR-008", "管口未连接", "管口", "提示", "连接状态 = #未连接"),
    ("DR-009", "图纸未设图例标准", "图纸", "错误", "图例标准 = NULL"),
    ("DR-010", "属性必填项为空", "对象实例", "错误", "必填项缺失数 > 0"),
    ("DR-011", "管路位号后缀缺失", "管路", "错误", '后缀 = NULL OR 后缀 = ""'),
    ("DR-012", "管段长度非法", "管段", "警告", "长度 <= 0"),
    ("DR-013", "仪表回路仪表数不足", "仪表回路", "提示", "仪表数 < 2"),
    ("DR-014", "管线端口未闭合", "管线", "错误", "端口未连接数 > 0"),
    ("DR-015", "信号线端点缺失", "信号线", "错误", "起点仪表 = NULL OR 终点仪表 = NULL"),
    ("DR-016", "标注文字为空", "标注", "提示", '文字 = NULL OR 文字 = ""'),
    ("DR-017", "阀门开度越界", "阀门", "警告", "开度 > 100 OR 开度 < 0"),
    ("DR-018", "位号格式不符", "对象实例", "警告", '位号 NOT MATCH "^[A-Z]{1,2}-[0-9]{4}$"'),
    ("DR-019", "位号长度超限", "对象实例", "提示", "位号.长度 > 32"),
    ("DR-020", "管口位号前缀不继承设备", "管口", "错误", "位号前缀 != 所属设备.位号"),
    (
        "DR-021",
        "介质与管道等级材质不兼容",
        "管线",
        "错误",
        "介质 IN (#氯气, #氢氟酸) AND 管道等级.材质 = #碳钢",
    ),
    ("DR-022", "管道等级不在项目主数据", "管线", "错误", "管道等级 NOT IN (项目.管道等级表)"),
    ("DR-023", "设备管口数不足", "设备", "警告", "管口数 < 2 AND NOT (设备类别 = #储罐)"),
    ("DR-024", "符号实例状态非法", "符号实例", "提示", "符号.状态 != #有效"),
    (
        "DR-025",
        "高温管线缺绝热等级",
        "管线",
        "警告",
        "(设计温度 > 200 OR 介质 = #蒸汽) AND 绝热等级 = NULL",
    ),
]

# The distinctive token each mandated rule's 名称 must mention, and the tokens
# the whole list must cover. Short tokens, not full names, so a legitimate
# rewording of the surrounding prose still passes while a dropped rule, a
# renamed scope or a changed severity does not.
RULE_NAME_TOKENS = {
    "DR-001": "缺管道等级",
    "DR-002": "缺公称直径",
    "DR-003": "无位号",
    "DR-004": "位号重复",
    "DR-005": "未分配回路",
    "DR-006": "对端缺失",
    "DR-007": "管道等级",
    "DR-008": "管口未连接",
    "DR-009": "未设图例标准",
    "DR-010": "必填项为空",
    "DR-011": "后缀缺失",
    "DR-012": "长度非法",
    "DR-013": "仪表数不足",
    "DR-014": "端口未闭合",
    "DR-015": "端点缺失",
    "DR-016": "文字为空",
    "DR-017": "开度越界",
    "DR-018": "格式不符",
    "DR-019": "长度超限",
    "DR-020": "前缀不继承",
    "DR-021": "材质不兼容",
    "DR-022": "不在项目主数据",
    "DR-023": "管口数不足",
    "DR-024": "状态非法",
    "DR-025": "缺绝热等级",
}

REQUIRED_DESIGN_RULES = {
    rule_id: (RULE_NAME_TOKENS[rule_id], scope, severity)
    for rule_id, _name, scope, severity, _expression in SHIPPED_DESIGN_RULES
}

# The ten rule kinds the feature contract requires the list to cover. Each is
# a token that must appear in some rule's 名称 cell.
RULE_TOPIC_TOKENS = [
    "缺管道等级",
    "缺公称直径",
    "无位号",
    "位号重复",
    "未分配回路",
    "对端缺失",
    "阀门",
    "管口未连接",
    "未设图例标准",
    "必填项为空",
]

_FENCE_RE = re.compile(r"^ {0,3}(?P<marker>`{3,}|~{3,})[ \t]*(?P<info>[^`\s]*)")


def fenced_blocks(text: str) -> list:
    """Return ``{'info': str, 'body': str}`` for every fenced code block."""
    blocks = []
    marker = None
    info = ""
    body = []
    for line in text.splitlines():
        if marker is None:
            opening = _FENCE_RE.match(line)
            if opening:
                marker = opening.group("marker")
                info = opening.group("info") or ""
                body = []
            continue
        stripped = line.strip()
        if stripped.startswith(marker[0]) and not stripped.strip(marker[0]):
            blocks.append({"info": info, "body": "\n".join(body)})
            marker = None
            continue
        body.append(line)
    if marker is not None:
        blocks.append({"info": info, "body": "\n".join(body)})
    return blocks


def _plain_cell(cell: str) -> str:
    """Strip inline-code delimiters and surrounding emphasis from a table cell."""
    text = cell.strip()
    if text.startswith("`") and text.endswith("`") and len(text) >= 2:
        text = text[1:-1]
    return text.replace("**", "").replace("`", "").strip()


def _find_table(section: str, header=None, must_contain=()):
    for table in parse_tables(section):
        if header is not None and [cell.strip() for cell in table["header"]] != list(header):
            continue
        if any(token not in " ".join(table["header"]) for token in must_contain):
            continue
        return table
    return None


def validate_rule_dsl_grammar(text: str, path: str = None) -> list:
    """VAL-REQ-008: the 规则 DSL section must carry a real EBNF block."""
    path = path or AREA_FILE_OF["DATA"]
    section = section_text(text, "规则 DSL")
    errors = []
    blocks = [block for block in fenced_blocks(section) if "ebnf" in block["info"].lower()]
    if not blocks:
        return [
            "%s: 规则 DSL has no fenced code block tagged ebnf (found %s)"
            % (path, [block["info"] for block in fenced_blocks(section)])
        ]
    body = blocks[0]["body"]
    for production in GRAMMAR_PRODUCTIONS:
        if not re.search(r"^\s*%s\s*=" % re.escape(production), body, re.MULTILINE):
            errors.append(
                "%s: EBNF block does not define the production '%s'" % (path, production)
            )
    for terminal in GRAMMAR_TERMINALS:
        if terminal not in body:
            errors.append(
                "%s: EBNF block does not show the terminal %s" % (path, terminal)
            )
    return errors


def validate_design_rule_table(text: str, path: str = None) -> list:
    """VAL-REQ-008: >=20 rules, five columns, every expression parseable."""
    path = path or AREA_FILE_OF["DATA"]
    section = section_text(text, "设计规则清单")
    table = _find_table(section, RULE_TABLE_COLUMNS)
    if table is None:
        found = [table["header"] for table in parse_tables(section)]
        return [
            "%s: 设计规则清单 has no table with columns %s (found %s)"
            % (path, " | ".join(RULE_TABLE_COLUMNS), found)
        ]

    errors = []
    if len(table["rows"]) < MIN_DESIGN_RULES:
        errors.append(
            "%s: 设计规则清单 has %d rules; at least %d required"
            % (path, len(table["rows"]), MIN_DESIGN_RULES)
        )

    seen = {}
    names = {}
    for index, row in enumerate(table["rows"]):
        line = table["line"] + 2 + index
        if len(row) != len(RULE_TABLE_COLUMNS):
            errors.append(
                "%s:%d: rule row has %d cells; expected %d"
                % (path, line, len(row), len(RULE_TABLE_COLUMNS))
            )
            continue
        rule_id = _plain_cell(row[0])
        name = _plain_cell(row[1])
        scope = _plain_cell(row[2])
        severity = _plain_cell(row[3])
        expression = _plain_cell(row[4])

        if not re.match(r"^DR-\d{3}$", rule_id):
            errors.append(
                "%s:%d: rule ID '%s' does not match DR-<three digits>" % (path, line, rule_id)
            )
        elif rule_id in seen:
            errors.append(
                "%s:%d: rule %s repeats the row on line %d"
                % (path, line, rule_id, seen[rule_id])
            )
        else:
            seen[rule_id] = line
            names[rule_id] = (name, scope, severity)

        if severity not in RULE_SEVERITIES:
            errors.append(
                "%s:%d: rule %s has severity '%s'; expected one of %s"
                % (path, line, rule_id, severity, "/".join(RULE_SEVERITIES))
            )
        try:
            rule_dsl.parse(expression)
        except rule_dsl.DSLParseError as error:
            errors.append(
                "%s:%d: rule %s has an expression that does not parse (%s): %s"
                % (path, line, rule_id, error.summary(), expression)
            )

    for rule_id, (name_token, scope_token, severity) in sorted(REQUIRED_DESIGN_RULES.items()):
        if rule_id not in names:
            errors.append("%s: 设计规则清单 omits the required rule %s" % (path, rule_id))
            continue
        name, scope, actual_severity = names[rule_id]
        if name_token not in name:
            errors.append(
                "%s: rule %s is named '%s'; it must mention '%s'"
                % (path, rule_id, name, name_token)
            )
        if scope_token not in scope:
            errors.append(
                "%s: rule %s applies to '%s'; it must mention '%s'"
                % (path, rule_id, scope, scope_token)
            )
        if actual_severity != severity:
            errors.append(
                "%s: rule %s has severity '%s'; expected '%s'"
                % (path, rule_id, actual_severity, severity)
            )

    joined_names = " ".join(entry[0] for entry in names.values())
    for topic in RULE_TOPIC_TOKENS:
        if topic not in joined_names:
            errors.append(
                "%s: no rule name covers the required topic '%s'" % (path, topic)
            )
    return errors


def _enum_values(*cells) -> set:
    for cell in cells:
        match = re.search(r"Enum\(([^)]*)\)", cell)
        if match:
            parts = re.split(r"[/、,，\s]+", match.group(1).strip())
            return {part for part in parts if part}
    return set()


def validate_result_model(text: str, path: str = None) -> list:
    """VAL-REQ-008: five result fields and the exact 错误/警告/提示 enum."""
    path = path or AREA_FILE_OF["DATA"]
    section = section_text(text, "结果模型")
    table = _find_table(section, RESULT_TABLE_COLUMNS)
    if table is None:
        return [
            "%s: 结果模型 has no field table with columns %s"
            % (path, " | ".join(RESULT_TABLE_COLUMNS))
        ]

    errors = []
    by_name = {}
    for index, row in enumerate(table["rows"]):
        line = table["line"] + 2 + index
        if len(row) != len(RESULT_TABLE_COLUMNS):
            errors.append(
                "%s:%d: result field row has %d cells; expected %d"
                % (path, line, len(row), len(RESULT_TABLE_COLUMNS))
            )
            continue
        by_name[_plain_cell(row[0])] = row

    for field in RESULT_FIELDS:
        if field not in by_name:
            errors.append("%s: 结果模型 does not define the field '%s'" % (path, field))

    if "严重度" in by_name:
        row = by_name["严重度"]
        values = _enum_values(row[1], row[3])
        if values != set(RULE_SEVERITIES):
            errors.append(
                "%s: 严重度 enumerates %s; it must be exactly %s"
                % (path, sorted(values), "/".join(RULE_SEVERITIES))
            )
    return errors


def validate_trigger_modes(text: str, path: str = None) -> list:
    """VAL-REQ-008: 按需/保存前/发布前, each with its own blocking behaviour."""
    path = path or AREA_FILE_OF["DATA"]
    section = section_text(text, "触发时机")
    errors = []
    for mode in TRIGGER_MODES:
        if mode not in section:
            errors.append("%s: 触发时机 does not define the '%s' mode" % (path, mode))

    table = _find_table(section, must_contain=("触发时机", "阻塞行为"))
    if table is None:
        return errors + [
            "%s: 触发时机 has no table with a 触发时机 column and a 阻塞行为 column"
            % path
        ]

    header = [cell.strip() for cell in table["header"]]
    mode_index = next(i for i, cell in enumerate(header) if "触发时机" in cell)
    block_index = next(i for i, cell in enumerate(header) if "阻塞行为" in cell)
    for mode in TRIGGER_MODES:
        rows = [
            row
            for row in table["rows"]
            if len(row) > max(mode_index, block_index)
            and mode in row[mode_index]
        ]
        if not rows:
            errors.append("%s: 触发时机 has no row for the '%s' mode" % (path, mode))
            continue
        behaviour = _plain_cell(rows[0][block_index])
        if "阻断" not in behaviour:
            errors.append(
                "%s: 触发时机 row for '%s' states no blocking behaviour (%r)"
                % (path, mode, behaviour)
            )
    return errors


def validate_data_rule_engine_coverage(documents: dict) -> list:
    """VAL-REQ-008: nine sections, an EBNF block, a rule table and a result model."""
    path = AREA_FILE_OF["DATA"]
    text = documents.get(path)
    if text is None:
        return ["%s: data and rule engine requirement document is missing" % path]

    errors = validate_sections(text, DATA_SECTIONS, path)
    errors.extend(validate_rule_dsl_grammar(text, path))
    errors.extend(validate_design_rule_table(text, path))
    errors.extend(validate_result_model(text, path))
    errors.extend(validate_trigger_modes(text, path))
    return errors


# ---------------------------------------------------------------------------
# Deliverables & publication (VAL-REQ-009)
# ---------------------------------------------------------------------------

DELIVERABLE_SECTIONS = ["材料清单聚合", "报表模板", "图纸输出", "数据发布", "导出矩阵"]

# VAL-DOC-009 makes this document the authoritative definition site for the six
# material-list columns; the standard header below is the one the prototype's
# bom screen must reproduce.
BOM_COLUMNS = ["序号", "类型", "位号", "名称", "数量", "单位"]
BOM_INVARIANT_PHRASE = "一个位号在同一份材料清单中至多占一行"
BOM_QUANTITY_TOKENS = ["Σ", "管段长度", "实例计数", "台/个", "m"]
DRAWING_OUTPUT_FORMATS = ["DWG", "DXF", "PDF", "SVG"]
EXPORT_FORMAT_TOKENS = ["DWG", "DXF", "PDF", "SVG", "XLSX", "CSV", "JSON"]
MIN_EXPORT_FORMATS = 6


def _squash(text: str) -> str:
    """Drop whitespace and emphasis markers so a sentence compares as one token."""
    return re.sub(r"[\s*_`]+", "", text)


def validate_deliverable_coverage(documents: dict) -> list:
    """VAL-REQ-009: five sections, the BOM invariant, formulas and four formats."""
    path = AREA_FILE_OF["DELV"]
    text = documents.get(path)
    if text is None:
        return ["%s: deliverables and publication requirement document is missing" % path]

    errors = validate_sections(text, DELIVERABLE_SECTIONS, path)

    bom = section_text(text, "材料清单聚合")
    if BOM_INVARIANT_PHRASE not in _squash(bom):
        errors.append(
            "%s: 材料清单聚合 does not state the invariant '%s'"
            % (path, BOM_INVARIANT_PHRASE)
        )
    if _find_table(bom, BOM_COLUMNS) is None:
        errors.append(
            "%s: 材料清单聚合 has no standard header with columns %s"
            % (path, " | ".join(BOM_COLUMNS))
        )
    for token in BOM_QUANTITY_TOKENS:
        if token not in bom:
            errors.append(
                "%s: 材料清单聚合 does not state the quantity term '%s'" % (path, token)
            )

    drawings = section_text(text, "图纸输出")
    for name in DRAWING_OUTPUT_FORMATS:
        if name not in drawings:
            errors.append("%s: 图纸输出 does not cover the %s format" % (path, name))

    export = section_text(text, "导出矩阵")
    present = [name for name in EXPORT_FORMAT_TOKENS if name in export]
    if len(present) < MIN_EXPORT_FORMATS:
        errors.append(
            "%s: 导出矩阵 covers %d of the known formats (%s); at least %d required"
            % (path, len(present), "/".join(present), MIN_EXPORT_FORMATS)
        )
    table = _find_table(export, must_contain=("格式",))
    if table is None:
        errors.append("%s: 导出矩阵 has no table with a 格式 column" % path)
    elif len(table["rows"]) < MIN_EXPORT_FORMATS:
        errors.append(
            "%s: 导出矩阵 table has %d rows; at least %d formats required"
            % (path, len(table["rows"]), MIN_EXPORT_FORMATS)
        )
    return errors


# ---------------------------------------------------------------------------
# Cross-document field names (VAL-DOC-009, document half)
# ---------------------------------------------------------------------------

# The tag-rule fields 30-symbol-library-and-project-settings.md owns. The data
# document must reuse them character-identically when it describes generation
# and uniqueness; a synonym there is drift even though the defining document is
# untouched, because a reader comparing the two documents sees two names.
TAG_RULE_FIELDS_IN_DATA_DOC = [
    "格式名称",
    "起始编码",
    "增量",
    "后缀递增",
    "位号唯一校验",
    "自动弹窗",
]
TAG_RULE_FIELD_SYNONYMS = ["位号唯一性校验"]


def validate_field_group_consistency(documents: dict) -> list:
    """VAL-DOC-009 on the document side: definition site and reference site agree.

    The prototype halves of the assertion (DOM labels, ``screen-catalog.json``)
    are not checked here; they arrive with the prototype. What is enforceable
    today is that the reference documents reuse the defining document's field
    names verbatim and never invent a synonym.
    """
    errors = []
    data_path = AREA_FILE_OF["DATA"]
    data = documents.get(data_path)
    if data is None:
        errors.append("%s: data and rule engine requirement document is missing" % data_path)
    else:
        for name in TAG_RULE_FIELDS_IN_DATA_DOC:
            if name not in data:
                errors.append(
                    "%s: does not reuse the tag-rule field name '%s' defined in %s"
                    % (data_path, name, AREA_FILE_OF["SYM"])
                )
        for synonym in TAG_RULE_FIELD_SYNONYMS:
            if synonym in data:
                errors.append(
                    "%s: uses the synonym '%s'; the canonical field name is '位号唯一校验'"
                    % (data_path, synonym)
                )

    delv_path = AREA_FILE_OF["DELV"]
    delv = documents.get(delv_path)
    if delv is None:
        errors.append("%s: deliverables and publication requirement document is missing" % delv_path)
    else:
        missing = [name for name in BOM_COLUMNS if name not in delv]
        for name in missing:
            errors.append(
                "%s: does not define the material-list column name '%s'" % (delv_path, name)
            )
        if not missing and find_bom_header(delv) is None:
            errors.append(
                "%s: the material-list column names never appear together as the header %s"
                % (delv_path, " | ".join(BOM_COLUMNS))
            )
    return errors


def find_bom_header(text: str):
    """The standard material-list header table, wherever it appears in ``text``."""
    for table in parse_tables(text):
        if [cell.strip() for cell in table["header"]] == BOM_COLUMNS:
            return table
    return None


# ---------------------------------------------------------------------------
# Collaboration, drawing states and the permission matrix (VAL-REQ-010)
# ---------------------------------------------------------------------------

COLLABORATION_SECTIONS = [
    "三级授权链",
    "图纸状态",
    "并发编辑策略",
    "冲突检测与解决",
    "编辑会话",
    "权限矩阵",
]

# The permission field group VAL-DOC-009 makes authoritative in this document.
DRAWING_PERMISSION_FIELDS = ["角色", "PBS 节点", "图纸状态", "允许", "拒绝"]

# The lifecycle-side entry this document has to connect to, so 图纸权限 is no
# longer a dangling forward reference from 40-drawing-and-drafting.md.
DRAWING_PERMISSION_ENTRY = "REQ-DRW-007"

# The closed drawing-state enum. The platform document's REQ-PLAT-010 uses the
# same three values; a fourth state anywhere is a contract breach.
DRAWING_STATES = ["单人编辑", "协同编辑", "只读"]
DRAWING_STATE_COLUMN = "图纸状态"

CONCURRENCY_COLUMNS = ["状态", "事件", "动作"]
CONCURRENCY_FLOOR = 6
# Each tuple is (event name used in messages, tokens the 事件 cell must carry).
CONCURRENCY_EVENTS = [
    ("位号占用申请", ["位号", "占用"]),
    ("占用超时释放", ["超时", "释放"]),
    ("图纸锁定只读降级", ["只读"]),
    ("协同编辑属性合并冲突", ["合并", "冲突"]),
    ("删除已被引用对象", ["删除", "引用"]),
    ("发布后修改", ["发布", "修改"]),
]

PERMISSION_ROLE_COLUMN = "角色"
PERMISSION_ROLE_FLOOR = 4
PERMISSION_OPERATION_FLOOR = 6
PERMISSION_CELL_VALUES = {"允许", "拒绝"}

COLLABORATION_SEMANTICS = [
    (
        "三级授权链",
        ["角色权限", "PBS 节点", "图纸状态", DRAWING_PERMISSION_ENTRY, "判定"],
    ),
    ("冲突检测与解决", ["冲突", "合并"]),
    ("编辑会话", ["会话", "心跳", "超时"]),
]


def _table_with_first_column(text: str, name: str):
    """The first table whose first header cell equals ``name``."""
    for table in parse_tables(text):
        if table["header"] and _plain_cell(table["header"][0]) == name:
            return table
    return None


def _cell(row, index: int) -> str:
    if index >= len(row):
        return ""
    return _plain_cell(row[index])


def validate_drawing_state_table(text: str, path: str = None) -> list:
    """The 图纸状态 enum table must list exactly the three contract states."""
    path = path or AREA_FILE_OF["COLL"]
    section = section_text(text, "图纸状态")
    table = _table_with_first_column(section, DRAWING_STATE_COLUMN)
    if table is None:
        found = [table["header"] for table in parse_tables(section)]
        return [
            "%s: 图纸状态 has no table whose first column is 图纸状态 (found %s)"
            % (path, found)
        ]
    states = [_cell(row, 0) for row in table["rows"] if row]
    if sorted(states) != sorted(DRAWING_STATES):
        return [
            "%s: 图纸状态 enum table lists %s; the enum is exactly %s"
            % (path, states, DRAWING_STATES)
        ]
    return []


def validate_concurrency_table(text: str, path: str = None) -> list:
    """状态 × 事件 → 动作 with at least six rows and all six event classes."""
    path = path or AREA_FILE_OF["COLL"]
    section = section_text(text, "并发编辑策略")
    table = None
    for candidate in parse_tables(section):
        header = [_plain_cell(cell) for cell in candidate["header"]]
        if header[:3] == CONCURRENCY_COLUMNS:
            table = candidate
            break
    if table is None:
        found = [table["header"] for table in parse_tables(section)]
        return [
            "%s: 并发编辑策略 has no table starting with the columns %s (found %s)"
            % (path, " | ".join(CONCURRENCY_COLUMNS), found)
        ]

    errors = []
    if len(table["rows"]) < CONCURRENCY_FLOOR:
        errors.append(
            "%s: 并发编辑策略 has %d rows; at least %d are required"
            % (path, len(table["rows"]), CONCURRENCY_FLOOR)
        )
    events = []
    for index, row in enumerate(table["rows"]):
        line = table["line"] + 2 + index
        if len(row) < 3:
            errors.append(
                "%s:%d: concurrency row has %d cells; expected at least 3"
                % (path, line, len(row))
            )
            continue
        state = _cell(row, 0)
        if state not in DRAWING_STATES:
            errors.append(
                "%s:%d: concurrency row state '%s' is not one of %s"
                % (path, line, state, "/".join(DRAWING_STATES))
            )
        # The scenario may be named in the 事件 cell and completed in 动作
        # ("图纸被他人锁定" -> "降级为只读"), so the tokens are matched over the
        # whole row rather than the event cell alone.
        events.append(" ".join(_plain_cell(cell) for cell in row[1:]))

    for name, tokens in CONCURRENCY_EVENTS:
        if not any(all(token in event for token in tokens) for event in events):
            errors.append(
                "%s: 并发编辑策略 does not cover the event '%s' (no row states %s)"
                % (path, name, " and ".join("'%s'" % token for token in tokens))
            )
    return errors


def validate_permission_matrix(text: str, path: str = None) -> list:
    """A role-by-operation matrix with >=4 roles and >=6 operations."""
    path = path or AREA_FILE_OF["COLL"]
    section = section_text(text, "权限矩阵")
    table = _table_with_first_column(section, PERMISSION_ROLE_COLUMN)
    if table is None:
        found = [table["header"] for table in parse_tables(section)]
        return [
            "%s: 权限矩阵 has no table whose first column is 角色 (found %s)"
            % (path, found)
        ]

    errors = []
    header = [_plain_cell(cell) for cell in table["header"]]
    operations = [cell for cell in header[1:] if cell]
    if len(operations) < PERMISSION_OPERATION_FLOOR:
        errors.append(
            "%s: 权限矩阵 has %d operation columns; at least %d are required"
            % (path, len(operations), PERMISSION_OPERATION_FLOOR)
        )
    if len(table["rows"]) < PERMISSION_ROLE_FLOOR:
        errors.append(
            "%s: 权限矩阵 has %d roles; at least %d are required"
            % (path, len(table["rows"]), PERMISSION_ROLE_FLOOR)
        )
    for index, row in enumerate(table["rows"]):
        line = table["line"] + 2 + index
        if len(row) != len(header):
            errors.append(
                "%s:%d: permission row has %d cells; the header has %d"
                % (path, line, len(row), len(header))
            )
            continue
        if not _cell(row, 0):
            errors.append("%s:%d: permission row has an empty role" % (path, line))
        for position, cell in enumerate(row[1:], start=2):
            value = _plain_cell(cell)
            if value not in PERMISSION_CELL_VALUES:
                errors.append(
                    "%s:%d: permission cell %d is '%s'; cells are only %s"
                    % (
                        path,
                        line,
                        position,
                        value,
                        "/".join(sorted(PERMISSION_CELL_VALUES)),
                    )
                )
    return errors


def validate_permission_matrix_provenance(text: str, path: str = None) -> list:
    """The matrix entry's 来源 must be 新增设计, never a manual section.

    The reference manual's operation table stops at 1-7 while its prose cites
    4-12, and the drawing-permission section is a single sentence, so no manual
    section can be the source of this matrix.
    """
    path = path or AREA_FILE_OF["COLL"]
    section = section_text(text, "权限矩阵")
    sources = [
        parse_entry_fields(entry["body"]).get("来源", "").strip()
        for entry in split_entries(section)
    ]
    if not sources:
        return ["%s: 权限矩阵 defines no requirement entry" % path]
    if not any(source.startswith(NEW_DESIGN) for source in sources):
        return [
            "%s: no 权限矩阵 entry cites %s; the matrix is our own design and "
            "cannot be attributed to a manual section (sources found: %s)"
            % (path, NEW_DESIGN, sources)
        ]
    return []


def validate_collaboration_coverage(documents: dict) -> list:
    """VAL-REQ-010: six sections, the semantics, and the two tables."""
    path = AREA_FILE_OF["COLL"]
    text = documents.get(path)
    if text is None:
        return ["%s: collaboration requirement document is missing" % path]

    errors = validate_sections(text, COLLABORATION_SECTIONS, path)
    for section, tokens in COLLABORATION_SEMANTICS:
        body = section_text(text, section)
        for token in tokens:
            if token not in body:
                errors.append("%s: %s does not state '%s'" % (path, section, token))
    for field in DRAWING_PERMISSION_FIELDS:
        if field not in text:
            errors.append(
                "%s: does not define the permission field name '%s'" % (path, field)
            )
    errors.extend(validate_drawing_state_table(text, path))
    errors.extend(validate_concurrency_table(text, path))
    errors.extend(validate_permission_matrix(text, path))
    errors.extend(validate_permission_matrix_provenance(text, path))
    return errors


# ---------------------------------------------------------------------------
# Import / export matrix (VAL-REQ-011)
# ---------------------------------------------------------------------------

IMPORT_EXPORT_SECTIONS = ["导入导出矩阵", "格式支持与限制"]
IMPORT_EXPORT_COLUMNS = ["数据类别", "导入", "导出", "支持程度", "限制"]
IMPORT_EXPORT_ROW_FLOOR = 6
IMPORT_EXPORT_CATEGORIES = [
    "图纸",
    "P&ID 对象与属性",
    "项目主数据",
    "配置包",
    "报表",
    "符号库",
]
IMPORT_EXPORT_FORMATS = ["DWG", "DXF", "PDF", "SVG", "Excel", "CSV", "JSON"]
FORMAT_TABLE_COLUMNS = ["格式", "导入方向", "导出方向", "支持程度", "限制"]
FORMAT_TABLE_ROW_FLOOR = 7


def validate_import_export_matrix(text: str, path: str = None) -> list:
    """VAL-REQ-011: the category-by-direction matrix and its format coverage."""
    path = path or AREA_FILE_OF["IO"]
    section = section_text(text, "导入导出矩阵")
    table = _find_table(section, IMPORT_EXPORT_COLUMNS)
    if table is None:
        found = [table["header"] for table in parse_tables(section)]
        return [
            "%s: 导入导出矩阵 has no table with columns %s (found %s)"
            % (path, " | ".join(IMPORT_EXPORT_COLUMNS), found)
        ]

    errors = []
    if len(table["rows"]) < IMPORT_EXPORT_ROW_FLOOR:
        errors.append(
            "%s: 导入导出矩阵 has %d data categories; at least %d are required"
            % (path, len(table["rows"]), IMPORT_EXPORT_ROW_FLOOR)
        )
    categories = [_cell(row, 0) for row in table["rows"] if row]
    for category in IMPORT_EXPORT_CATEGORIES:
        if not any(category in value for value in categories):
            errors.append(
                "%s: 导入导出矩阵 has no row for the data category '%s' (rows: %s)"
                % (path, category, categories)
            )
    matrix_text = " ".join(
        _plain_cell(cell) for row in table["rows"] for cell in row
    )
    for fmt in IMPORT_EXPORT_FORMATS:
        if fmt not in matrix_text:
            errors.append(
                "%s: 导入导出矩阵 never mentions the format '%s'" % (path, fmt)
            )
    for index, row in enumerate(table["rows"]):
        line = table["line"] + 2 + index
        if len(row) != len(IMPORT_EXPORT_COLUMNS):
            errors.append(
                "%s:%d: matrix row has %d cells; the header has %d"
                % (path, line, len(row), len(IMPORT_EXPORT_COLUMNS))
            )
            continue
        if not _cell(row, 1) or not _cell(row, 2):
            errors.append(
                "%s:%d: matrix row for '%s' leaves 导入 or 导出 empty"
                % (path, line, _cell(row, 0))
            )
        if not _cell(row, 4):
            errors.append(
                "%s:%d: matrix row for '%s' has no 限制" % (path, line, _cell(row, 0))
            )
    return errors


def validate_format_support_table(text: str, path: str = None) -> list:
    """The per-format table must cover all seven contractual formats."""
    path = path or AREA_FILE_OF["IO"]
    section = section_text(text, "格式支持与限制")
    table = _find_table(section, FORMAT_TABLE_COLUMNS)
    if table is None:
        found = [table["header"] for table in parse_tables(section)]
        return [
            "%s: 格式支持与限制 has no table with columns %s (found %s)"
            % (path, " | ".join(FORMAT_TABLE_COLUMNS), found)
        ]
    errors = []
    if len(table["rows"]) < FORMAT_TABLE_ROW_FLOOR:
        errors.append(
            "%s: 格式支持与限制 has %d format rows; at least %d are required"
            % (path, len(table["rows"]), FORMAT_TABLE_ROW_FLOOR)
        )
    formats = [_cell(row, 0) for row in table["rows"] if row]
    for fmt in IMPORT_EXPORT_FORMATS:
        if not any(value.startswith(fmt) for value in formats):
            errors.append(
                "%s: 格式支持与限制 has no row for the format '%s' (rows: %s)"
                % (path, fmt, formats)
            )
    return errors


def validate_import_export_coverage(documents: dict) -> list:
    """VAL-REQ-011 over the whole document map."""
    path = AREA_FILE_OF["IO"]
    text = documents.get(path)
    if text is None:
        return ["%s: import/export requirement document is missing" % path]
    errors = validate_sections(text, IMPORT_EXPORT_SECTIONS, path)
    errors.extend(validate_import_export_matrix(text, path))
    errors.extend(validate_format_support_table(text, path))
    return errors


# ---------------------------------------------------------------------------
# Migration and interoperability (VAL-REQ-012)
# ---------------------------------------------------------------------------

MIGRATION_SECTIONS = [
    "存量图纸导入",
    "参考产品迁移",
    "三维模型关联",
    "自动化接口",
    "插件扩展",
    "数据交换标准",
]

CONFIDENCE_COLUMN = "识别置信度等级"
CONFIDENCE_LEVEL_FLOOR = 3
CONFIDENCE_COLUMNS = ["识别置信度等级", "置信度区间", "判定依据", "处置"]

MIGRATION_SEMANTICS = [
    ("存量图纸导入", ["人工确认"]),
    ("三维模型关联", ["位号", "关联键"]),
    ("自动化接口", ["--serve", "--http", "--mcp"]),
    ("插件扩展", ["plugin.toml", "api_version"]),
]

EXCHANGE_COLUMNS = ["标准", "是否在范围内", "理由", "后续阶段位置"]
EXCHANGE_STANDARDS = ["DEXPI", "ISO 15926", "CFIHOS"]
EXCHANGE_OUT_OF_SCOPE = "范围外"


def validate_confidence_table(text: str, path: str = None) -> list:
    """At least three recognition-confidence levels with a stated disposition."""
    path = path or AREA_FILE_OF["MIG"]
    section = section_text(text, "存量图纸导入")
    table = _find_table(section, CONFIDENCE_COLUMNS)
    if table is None:
        found = [table["header"] for table in parse_tables(section)]
        return [
            "%s: 存量图纸导入 has no table with columns %s (found %s)"
            % (path, " | ".join(CONFIDENCE_COLUMNS), found)
        ]
    errors = []
    if len(table["rows"]) < CONFIDENCE_LEVEL_FLOOR:
        errors.append(
            "%s: 存量图纸导入 defines %d confidence levels; at least %d are "
            "required" % (path, len(table["rows"]), CONFIDENCE_LEVEL_FLOOR)
        )
    for index, row in enumerate(table["rows"]):
        line = table["line"] + 2 + index
        if len(row) < len(CONFIDENCE_COLUMNS):
            errors.append(
                "%s:%d: confidence row has %d cells; expected %d"
                % (path, line, len(row), len(CONFIDENCE_COLUMNS))
            )
            continue
        for position in (0, 1, 2, 3):
            if not _cell(row, position):
                errors.append(
                    "%s:%d: confidence row leaves column %d empty"
                    % (path, line, position + 1)
                )
    return errors


def validate_exchange_standard_table(text: str, path: str = None) -> list:
    """DEXPI / ISO 15926 / CFIHOS must each be declared in or out of scope."""
    path = path or AREA_FILE_OF["MIG"]
    section = section_text(text, "数据交换标准")
    table = _find_table(section, EXCHANGE_COLUMNS)
    if table is None:
        found = [table["header"] for table in parse_tables(section)]
        return [
            "%s: 数据交换标准 has no table with columns %s (found %s)"
            % (path, " | ".join(EXCHANGE_COLUMNS), found)
        ]
    errors = []
    standards = [_cell(row, 0) for row in table["rows"] if row]
    for standard in EXCHANGE_STANDARDS:
        if standard not in standards:
            errors.append(
                "%s: 数据交换标准 does not declare the standard '%s' (declared: %s)"
                % (path, standard, standards)
            )
    for index, row in enumerate(table["rows"]):
        line = table["line"] + 2 + index
        if len(row) < len(EXCHANGE_COLUMNS):
            errors.append(
                "%s:%d: exchange-standard row has %d cells; expected %d"
                % (path, line, len(row), len(EXCHANGE_COLUMNS))
            )
            continue
        decision = _cell(row, 1)
        if decision not in ("范围内", EXCHANGE_OUT_OF_SCOPE):
            errors.append(
                "%s:%d: '%s' is marked '%s'; expected 范围内 or %s"
                % (path, line, _cell(row, 0), decision, EXCHANGE_OUT_OF_SCOPE)
            )
            continue
        if decision == EXCHANGE_OUT_OF_SCOPE:
            if len(_cell(row, 2)) < 8:
                errors.append(
                    "%s:%d: '%s' is %s but gives no reason"
                    % (path, line, _cell(row, 0), EXCHANGE_OUT_OF_SCOPE)
                )
            if len(_cell(row, 3)) < 4:
                errors.append(
                    "%s:%d: '%s' is %s but names no later phase"
                    % (path, line, _cell(row, 0), EXCHANGE_OUT_OF_SCOPE)
                )
    return errors


def validate_migration_coverage(documents: dict) -> list:
    """VAL-REQ-012 over the whole document map."""
    path = AREA_FILE_OF["MIG"]
    text = documents.get(path)
    if text is None:
        return ["%s: migration requirement document is missing" % path]
    errors = validate_sections(text, MIGRATION_SECTIONS, path)
    for section, tokens in MIGRATION_SEMANTICS:
        body = section_text(text, section)
        for token in tokens:
            if token not in body:
                errors.append("%s: %s does not state '%s'" % (path, section, token))
    errors.extend(validate_confidence_table(text, path))
    errors.extend(validate_exchange_standard_table(text, path))
    return errors


# ---------------------------------------------------------------------------
# Reuse mapping (VAL-REQ-014)
# ---------------------------------------------------------------------------

REUSE_MAPPING_SECTIONS = ["复用映射表", "改造点说明"]
REUSE_TABLE_COLUMNS = ["现有实现", "路径", "复用于", "需改造点"]
REUSE_ROW_FLOOR = 12


def _normalise_repo_path(cell: str) -> str:
    """The repo-relative path a 路径 cell names, with decoration removed."""
    value = _plain_cell(cell).strip().replace("\\", "/")
    while value.startswith("./"):
        value = value[2:]
    return value.rstrip("/")


def validate_reuse_mapping(documents: dict, repo_root=None) -> list:
    """Two sections, the four contract columns, >=12 rows, real paths.

    ``repo_root`` is required: a path column that is never checked against the
    filesystem would accept any string, which is exactly the failure mode
    VAL-REQ-014 exists to prevent.
    """
    path = REUSE_MAPPING_PATH
    text = documents.get(path)
    if text is None:
        return ["%s: reuse mapping document is missing" % path]
    if repo_root is None:
        return ["%s: validate_reuse_mapping needs a repo root to verify paths" % path]

    headings = {normalize_heading(name) for name in section_requirement_counts(text)}
    errors = []
    for section in REUSE_MAPPING_SECTIONS:
        if normalize_heading(section) not in headings:
            errors.append(
                "%s: missing section '%s' (found %s)"
                % (path, section, sorted(headings))
            )

    section = section_text(text, "复用映射表")
    table = _find_table(section, REUSE_TABLE_COLUMNS)
    if table is None:
        found = [table["header"] for table in parse_tables(section)]
        errors.append(
            "%s: 复用映射表 has no table with columns %s (found %s)"
            % (path, " | ".join(REUSE_TABLE_COLUMNS), found)
        )
        return errors

    if len(table["rows"]) < REUSE_ROW_FLOOR:
        errors.append(
            "%s: 复用映射表 has %d rows; at least %d are required"
            % (path, len(table["rows"]), REUSE_ROW_FLOOR)
        )

    root = Path(repo_root)
    for index, row in enumerate(table["rows"]):
        line = table["line"] + 2 + index
        if len(row) != len(REUSE_TABLE_COLUMNS):
            errors.append(
                "%s:%d: reuse row has %d cells; the header has %d"
                % (path, line, len(row), len(REUSE_TABLE_COLUMNS))
            )
            continue
        for position in range(len(REUSE_TABLE_COLUMNS)):
            if not _cell(row, position):
                errors.append(
                    "%s:%d: reuse row leaves column '%s' empty"
                    % (path, line, REUSE_TABLE_COLUMNS[position])
                )
        candidate = _normalise_repo_path(row[1])
        if not candidate:
            continue
        if candidate.startswith("/") or re.match(r"^[A-Za-z]:", candidate) or ".." in candidate.split("/"):
            errors.append(
                "%s:%d: path '%s' is not a repo-relative path" % (path, line, candidate)
            )
            continue
        if not (root / candidate).exists():
            errors.append(
                "%s:%d: path '%s' does not exist in the repository"
                % (path, line, candidate)
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
