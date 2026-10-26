"""Machine-checkable contract for glossary.md and the reference baseline docs.

Pure stdlib: importable without pytest. Every public validator returns a
list of English error strings; an empty list means the document satisfies
the contract. All validators are pure functions of their text argument so
tests can point them at scratch content under ``tmp_path``.
"""
from __future__ import annotations

import json
import os
import re
from pathlib import Path

from manifest_contract import normalize_heading

# ---------------------------------------------------------------------------
# Normative vocabulary (VAL-DOC-002 / VAL-DOC-003 / VAL-DOC-004)
# ---------------------------------------------------------------------------

GLOSSARY_COLUMNS = ["中文名", "英文名", "定义", "禁用别名"]

# The 24 core concepts named by VAL-DOC-002, in the order the contract lists
# them. Coverage is what matters; the order is only used for error messages.
GLOSSARY_TERMS = [
    "位号",
    "管线",
    "管路",
    "管段",
    "管件",
    "阀门",
    "设备",
    "管口",
    "仪表",
    "仪表回路",
    "信号线",
    "页间连接符",
    "打断符",
    "图例标准",
    "符号",
    "连接点",
    "类型库",
    "数据要素",
    "属性模板",
    "材料清单",
    "设计规则检查",
    "图纸版本快照",
    "PBS",
    "项目主数据",
]

# Short forms that may appear in prose but never replace the canonical term
# in headings, tables or UI labels.
SANCTIONED_ALIASES = {
    "类型库": ["类库"],
    "位号": ["TAG"],
    "材料清单": ["BOM"],
}

NO_ALIAS_MARKERS = {"—", "-", "无"}

NINE_MODULES = [
    "业务类型库及属性管理",
    "标准图例定制",
    "项目设置",
    "用户设置",
    "图纸权限管理",
    "2D 绘图",
    "PID 绘图",
    "数据管理",
    "数据发布",
]

MODULE_FIELDS = ["能力描述", "关键对象", "实现方式", "手册依据"]

UNDOCUMENTED_CAPABILITIES = [
    "设计规则检查引擎",
    "材料清单生成",
    "协同设计",
    "对象级数据回退",
]

# Each undocumented capability must state that the manual offers neither a
# data model nor an interface nor a rule definition (VAL-DOC-003).
UNDOCUMENTED_MISSING_PHRASES = ["无数据模型", "无界面", "无规则定义"]

SOURCE_AREAS = ["PLAT", "TYPE", "SYM", "DRW", "DATA", "DELV", "COLL", "IO", "MIG"]

NEW_DESIGN = "新增设计"
SECTION_REF_RE = re.compile(r"^§(\d+(?:\.\d+)+)$")
ANY_SECTION_TOKEN_RE = re.compile(r"§(\d+(?:\.\d+)+)(?![\d.])")

DEFAULT_ANALYSIS_DIR = Path(r"D:\work\plant-code\cad\pid-资料\analysis")

# ---------------------------------------------------------------------------
# Markdown primitives
# ---------------------------------------------------------------------------

_HEADING_RE = re.compile(r"^(#{1,6})[ \t]+(.+?)[ \t]*$")
_SEPARATOR_CELL_RE = re.compile(r"^:?-{3,}:?$")


def _split_cells(line: str) -> list:
    s = line.strip()
    if s.startswith("|"):
        s = s[1:]
    if s.endswith("|"):
        s = s[:-1]
    return [cell.strip() for cell in s.split("|")]


def _is_separator(line: str) -> bool:
    if not line.strip().startswith("|"):
        return False
    cells = _split_cells(line)
    return bool(cells) and all(_SEPARATOR_CELL_RE.match(c) for c in cells)


def parse_tables(text: str) -> list:
    """Return every pipe table as {'header': [...], 'rows': [[...]], 'line': n}."""
    lines = text.splitlines()
    tables = []
    i = 0
    while i < len(lines) - 1:
        if lines[i].strip().startswith("|") and _is_separator(lines[i + 1]):
            header = _split_cells(lines[i])
            rows = []
            j = i + 2
            while j < len(lines) and lines[j].strip().startswith("|"):
                rows.append(_split_cells(lines[j]))
                j += 1
            tables.append({"header": header, "rows": rows, "line": i + 1})
            i = j
        else:
            i += 1
    return tables


def find_table(text: str, header: list):
    """Return the first table whose header equals ``header`` exactly."""
    for table in parse_tables(text):
        if table["header"] == header:
            return table
    return None


def sections_at(text: str, level: int) -> list:
    """Return [(normalized_heading, body)] for headings of exactly ``level``.

    The body runs until the next heading of the same or a higher level, so a
    section at level 3 keeps its level-4 subsections.
    """
    return _sections(text, level, normalize=True)


def raw_sections_at(text: str, level: int) -> list:
    """Like :func:`sections_at` but keyed by the heading text as written.

    Needed for names that start with a digit, because heading normalization
    strips leading numbering and would turn "2D 绘图" into "D 绘图".
    """
    return _sections(text, level, normalize=False)


def _sections(text: str, level: int, normalize: bool) -> list:
    out = []
    current = None
    buf = []
    for line in text.splitlines():
        m = _HEADING_RE.match(line)
        if m:
            depth = len(m.group(1))
            if depth <= level:
                if current is not None:
                    out.append((current, "\n".join(buf)))
                    current = None
                buf = []
                if depth == level:
                    current = (
                        normalize_heading(m.group(2))
                        if normalize
                        else m.group(2).strip()
                    )
                continue
        if current is not None:
            buf.append(line)
    if current is not None:
        out.append((current, "\n".join(buf)))
    return out


def section_body(text: str, level: int, heading: str):
    """Return the body of the first heading of ``level`` named ``heading``."""
    for name, body in sections_at(text, level):
        if name == heading:
            return body
    return None


def h1_count(text: str) -> int:
    return sum(1 for line in text.splitlines() if re.match(r"^#[ \t]", line))


def section_tokens(text: str) -> list:
    """Every ``§x.y.z`` token in ``text``, de-duplicated, order preserved."""
    seen = []
    for token in ANY_SECTION_TOKEN_RE.findall(text):
        if token not in seen:
            seen.append(token)
    return seen


# ---------------------------------------------------------------------------
# Fixtures
# ---------------------------------------------------------------------------

FIXTURES = Path(__file__).resolve().parent / "fixtures"


def allowed_sections(fixture_path=None) -> list:
    """The verified manual-section whitelist."""
    path = Path(fixture_path) if fixture_path else FIXTURES / "manual-sections.json"
    with path.open(encoding="utf-8") as fh:
        data = json.load(fh)
    return list(data.get("sections", []))


def analysis_dir() -> Path:
    override = os.environ.get("PID_ANALYSIS_DIR")
    return Path(override) if override else DEFAULT_ANALYSIS_DIR


def analysis_section_tokens(directory=None) -> set:
    """Every ``§`` token present in the extracted analysis markdown files."""
    root = Path(directory) if directory else analysis_dir()
    tokens = set()
    if not root.is_dir():
        return tokens
    for path in sorted(root.glob("*.md")):
        tokens.update(section_tokens(path.read_text(encoding="utf-8")))
    return tokens


def split_aliases(cell: str) -> list:
    return [a.strip() for a in re.split(r"[、,，;；/]", cell) if a.strip()]


# ---------------------------------------------------------------------------
# glossary.md
# ---------------------------------------------------------------------------


def glossary_rows(text: str):
    """Return ({term: row}, errors) for the glossary table."""
    errors = []
    table = find_table(text, GLOSSARY_COLUMNS)
    if table is None:
        return {}, [
            "glossary table not found; expected header "
            + " | ".join(GLOSSARY_COLUMNS)
        ]
    rows = {}
    for index, row in enumerate(table["rows"], start=2):
        if len(row) != len(GLOSSARY_COLUMNS):
            errors.append(
                "glossary row %d has %d cells, expected %d: %s"
                % (index, len(row), len(GLOSSARY_COLUMNS), " | ".join(row))
            )
            continue
        term = row[0]
        if term in rows:
            errors.append("glossary term duplicated: " + term)
            continue
        rows[term] = dict(zip(GLOSSARY_COLUMNS, row))
    return rows, errors


def validate_glossary(text: str) -> list:
    """Check VAL-DOC-002 against the markdown text of glossary.md."""
    errors = []
    if h1_count(text) != 1:
        errors.append(
            "glossary.md must contain exactly one H1, found %d" % h1_count(text)
        )
    rows, row_errors = glossary_rows(text)
    errors.extend(row_errors)
    missing = [t for t in GLOSSARY_TERMS if t not in rows]
    if missing:
        errors.append(
            "glossary is missing %d of the 24 core concepts: %s"
            % (len(missing), "、".join(missing))
        )
    for term in GLOSSARY_TERMS:
        row = rows.get(term)
        if row is None:
            continue
        english = row["英文名"]
        if len(re.sub(r"[^A-Za-z]", "", english)) < 2:
            errors.append(
                "glossary row '%s' has no usable English name: %r"
                % (term, english)
            )
        definition = row["定义"]
        if len(definition) < 10:
            errors.append(
                "glossary row '%s' has a definition shorter than 10 characters: %r"
                % (term, definition)
            )
        aliases = row["禁用别名"]
        if not aliases:
            errors.append(
                "glossary row '%s' has an empty 禁用别名 cell; use — when there "
                "is no banned alias" % term
            )
        elif aliases not in NO_ALIAS_MARKERS and not split_aliases(aliases):
            errors.append(
                "glossary row '%s' has an unreadable 禁用别名 cell: %r"
                % (term, aliases)
            )
    declaration = section_body(text, 2, "唯一词源声明")
    if declaration is None:
        errors.append("glossary.md is missing the H2 section 唯一词源声明")
    else:
        if "唯一词源" not in declaration:
            errors.append(
                "the 唯一词源声明 section never states 唯一词源"
            )
        if len(declaration.strip()) < 80:
            errors.append(
                "the 唯一词源声明 section is too thin (%d characters) to be a "
                "binding declaration" % len(declaration.strip())
            )
    return errors


def banned_alias_map(text: str) -> dict:
    """Return {banned_alias: canonical_term} from the glossary text."""
    rows, _ = glossary_rows(text)
    out = {}
    for term, row in rows.items():
        for alias in split_aliases(row.get("禁用别名", "")):
            if alias in NO_ALIAS_MARKERS:
                continue
            out[alias] = term
    return out


# ---------------------------------------------------------------------------
# reference-product-summary.md
# ---------------------------------------------------------------------------


def module_sections(text: str) -> dict:
    """Map each of the nine normative module names to its section body."""
    found = {}
    for name, body in raw_sections_at(text, 3):
        for module in NINE_MODULES:
            if module in name:
                found.setdefault(module, body)
    return found


def field_present(body: str, field: str) -> bool:
    """A field counts as present when it is a bold label or a table column."""
    if "**%s**" % field in body:
        return True
    return bool(
        re.search(r"^\|\s*" + re.escape(field) + r"\s*\|", body, re.MULTILINE)
    )


def validate_reference_summary(text: str, allowed=None) -> list:
    """Check VAL-DOC-003 against reference-product-summary.md."""
    allowed = set(allowed if allowed is not None else allowed_sections())
    errors = []
    if h1_count(text) != 1:
        errors.append(
            "reference-product-summary.md must contain exactly one H1, found %d"
            % h1_count(text)
        )
    baseline = section_body(text, 2, "模块能力基线")
    if baseline is None:
        errors.append("missing H2 section 模块能力基线")
        errors.append("cannot check the nine modules without 模块能力基线")
        modules = {}
    else:
        modules = module_sections(text)
    for module in NINE_MODULES:
        body = modules.get(module)
        if body is None:
            errors.append(
                "no level-3 section whose heading contains the module name: "
                + module
            )
            continue
        missing_fields = [f for f in MODULE_FIELDS if not field_present(body, f)]
        if missing_fields:
            errors.append(
                "module section '%s' is missing fields: %s"
                % (module, "、".join(missing_fields))
            )
        if "手册依据" in body:
            cited = section_tokens(body.split("手册依据", 1)[1])
            if not cited:
                errors.append(
                    "module section '%s' cites no manual section in 手册依据"
                    % module
                )
            unknown = [t for t in cited if t not in allowed]
            if unknown:
                errors.append(
                    "module section '%s' cites sections outside the verified "
                    "whitelist: %s" % (module, ", ".join(unknown))
                )
    undocumented = section_body(text, 2, "无文档支撑的能力")
    if undocumented is None:
        errors.append("missing H2 section 无文档支撑的能力")
        return errors
    sub = dict(sections_at(undocumented, 3))
    if not sub:
        sub = {}
    for capability in UNDOCUMENTED_CAPABILITIES:
        body = None
        for name, candidate in sub.items():
            if capability in name:
                body = candidate
                break
        if body is None:
            # allow a bold bullet instead of a heading
            pattern = re.compile(
                r"\*\*" + re.escape(capability) + r"\*\*(.*?)(?=\n\s*[-*]\s*\*\*|\Z)",
                re.DOTALL,
            )
            match = pattern.search(undocumented)
            body = match.group(1) if match else None
        if body is None:
            errors.append(
                "无文档支撑的能力 does not address: " + capability
            )
            continue
        if len(body.strip()) < 60:
            errors.append(
                "无文档支撑的能力 entry '%s' is too thin (%d characters)"
                % (capability, len(body.strip()))
            )
        missing_phrases = [
            p for p in UNDOCUMENTED_MISSING_PHRASES if p not in body
        ]
        if missing_phrases:
            errors.append(
                "无文档支撑的能力 entry '%s' does not state: %s"
                % (capability, "、".join(missing_phrases))
            )
        if "手册" not in body:
            errors.append(
                "无文档支撑的能力 entry '%s' cites no manual evidence"
                % capability
            )
        if NEW_DESIGN not in body and NEW_DESIGN not in undocumented:
            errors.append(
                "无文档支撑的能力 entry '%s' does not mark our own design as %s"
                % (capability, NEW_DESIGN)
            )
    return errors


# ---------------------------------------------------------------------------
# source-index.md
# ---------------------------------------------------------------------------


def source_index_rows(text: str):
    """Return (rows, errors) for the 区域/主题/来源/说明 mapping table."""
    errors = []
    header = ["区域", "主题", "来源", "说明"]
    table = find_table(text, header)
    if table is None:
        return [], [
            "source index table not found; expected header " + " | ".join(header)
        ]
    rows = []
    for index, cells in enumerate(table["rows"], start=2):
        if len(cells) != len(header):
            errors.append(
                "source index row %d has %d cells, expected %d: %s"
                % (index, len(cells), len(header), " | ".join(cells))
            )
            continue
        rows.append(dict(zip(header, cells)))
    return rows, errors


def validate_source_index(text: str, allowed=None, minimum_rows: int = 30,
                          minimum_per_area: int = 3) -> list:
    """Check VAL-DOC-004 against source-index.md."""
    allowed = set(allowed if allowed is not None else allowed_sections())
    errors = []
    if h1_count(text) != 1:
        errors.append(
            "source-index.md must contain exactly one H1, found %d" % h1_count(text)
        )
    codes = find_table(text, ["区域代码", "需求文档", "范围"])
    if codes is None:
        errors.append(
            "missing the 区域代码 table with columns 区域代码 | 需求文档 | 范围"
        )
    else:
        listed = [row[0] for row in codes["rows"] if row]
        missing_codes = [c for c in SOURCE_AREAS if c not in listed]
        extra_codes = [c for c in listed if c not in SOURCE_AREAS]
        if missing_codes:
            errors.append(
                "区域代码 table is missing area codes: "
                + ", ".join(missing_codes)
            )
        if extra_codes:
            errors.append(
                "区域代码 table has unknown area codes: " + ", ".join(extra_codes)
            )
    rows, row_errors = source_index_rows(text)
    errors.extend(row_errors)
    if len(rows) < minimum_rows:
        errors.append(
            "source index has %d mapping rows, the contract requires at least %d"
            % (len(rows), minimum_rows)
        )
    counts = {code: 0 for code in SOURCE_AREAS}
    for index, row in enumerate(rows, start=2):
        area = row["区域"]
        topic = row["主题"]
        source = row["来源"]
        note = row["说明"]
        if area not in SOURCE_AREAS:
            errors.append(
                "source index row %d has area %r outside the fixed enum" % (index, area)
            )
        else:
            counts[area] += 1
        if len(topic) < 2:
            errors.append("source index row %d has an empty 主题" % index)
        match = SECTION_REF_RE.match(source)
        if match:
            if match.group(1) not in allowed:
                errors.append(
                    "source index row %d cites an unverified manual section: %s"
                    % (index, source)
                )
        elif source != NEW_DESIGN:
            errors.append(
                "source index row %d has 来源 %r; expected §x.y.z or %s"
                % (index, source, NEW_DESIGN)
            )
        if len(note) < 12:
            errors.append(
                "source index row %d has a 说明 shorter than 12 characters" % index
            )
    thin = [
        "%s=%d" % (code, n)
        for code, n in counts.items()
        if n < minimum_per_area
    ]
    if thin:
        errors.append(
            "every area needs at least %d rows; short areas: %s"
            % (minimum_per_area, ", ".join(thin))
        )
    return errors


def area_counts(text: str) -> dict:
    rows, _ = source_index_rows(text)
    counts = {code: 0 for code in SOURCE_AREAS}
    for row in rows:
        if row.get("区域") in counts:
            counts[row["区域"]] += 1
    return counts
