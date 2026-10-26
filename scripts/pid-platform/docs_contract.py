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
from itertools import combinations
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

# Raw manual plain-text extract. Outside the repository by mission rule, so
# every check that needs it must skip rather than fail when it is absent.
DEFAULT_MANUAL_TEXT = Path(
    r"D:\work\plant-code\cad\pid-资料\text"
    r"\5.PIMCenter_Designer_P_ID_V25R4.1用户手册.txt"
)

SECTION_NUMBER_RE = re.compile(r"^\d+(?:\.\d+)+$")

# source-index.md must end with this appendix: the fact table that pins every
# cited section number to the manual heading it actually carries.
CITATION_APPENDIX_HEADING = "引用章节标题对照"
CITATION_APPENDIX_COLUMNS = ["节号", "手册标题"]

# The fixture shipped 80 verified sections before the symbol-editor citations
# were corrected; .3.4/.3.5/.3.7 were added because the corrected citations
# need them. The floor only guards against the list being truncated.
MANUAL_MIN_SECTION_TITLES = 80

# A manual body heading line: number, optional trailing period, then the title.
# Table-of-contents lines (dot leaders) are filtered out by the caller.
MANUAL_HEADING_RE = re.compile(
    r"^(?P<number>\d+(?:\.\d+)+)\.?[ \t]*(?P<title>[^\s.][^\n]*)$"
)

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


def section_titles(fixture_path=None) -> dict:
    """Map each verified manual section to its verbatim manual heading title.

    Two facts are recorded per section, and they are not equally strong
    evidence: the number usually occurs as a literal ``§`` token in the analysis
    extract, but that extract collapses some blocks into ranges (the symbol
    editor is written ``§5.1.4.3.2.2.3.2–5``), so §5.1.4.3.2.2.3.4 and
    §5.1.4.3.2.2.3.5 appear there only inside that interval, and
    §5.1.4.3.2.2.3.7 has no analysis token at all. For those entries the only
    evidence is the second fact: the title is the heading line of that section
    in the raw manual body, which is what :func:`validate_titles_against_manual`
    checks. The legacy form of the fixture was a bare list of numbers; it is
    still accepted, but every entry then carries an empty title so
    :func:`validate_section_fixture` reports the missing fact instead of
    silently passing.
    """
    path = Path(fixture_path) if fixture_path else FIXTURES / "manual-sections.json"
    with path.open(encoding="utf-8") as fh:
        data = json.load(fh)
    sections = data.get("sections", {})
    if isinstance(sections, dict):
        return {str(key): str(value) for key, value in sections.items()}
    return {str(key): "" for key in sections}


def allowed_sections(fixture_path=None) -> list:
    """The verified manual-section whitelist."""
    return list(section_titles(fixture_path).keys())


def validate_section_fixture(fixture_path=None) -> list:
    """Every fixture entry must be a section number with a verbatim title."""
    path = Path(fixture_path) if fixture_path else FIXTURES / "manual-sections.json"
    try:
        titles = section_titles(path)
    except (OSError, ValueError) as exc:
        return ["cannot read the manual-section fixture %s: %s" % (path, exc)]
    if not titles:
        return ["the manual-section fixture lists no sections: " + str(path)]
    errors = []
    if len(titles) < MANUAL_MIN_SECTION_TITLES:
        errors.append(
            "the manual-section fixture lists %d sections; the contract "
            "requires at least %d" % (len(titles), MANUAL_MIN_SECTION_TITLES)
        )
    for section in sorted(titles):
        if not SECTION_NUMBER_RE.match(section):
            errors.append(
                "fixture key %r is not a section number such as 6.2.2.3.4"
                % section
            )
        if len(titles[section].strip()) < 2:
            errors.append(
                "fixture section %s carries no manual heading title" % section
            )
    return errors


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
# Raw manual corpus (outside the repository, read-only)
# ---------------------------------------------------------------------------


def manual_text_path() -> Path:
    override = os.environ.get("PID_MANUAL_TEXT")
    return Path(override) if override else DEFAULT_MANUAL_TEXT


def read_manual_text(path=None):
    """The raw manual plain text, or ``None`` when the corpus is unavailable."""
    target = Path(path) if path is not None else manual_text_path()
    if not target.is_file():
        return None
    try:
        return target.read_text(encoding="utf-8", errors="replace")
    except OSError:
        return None


def manual_heading_lines(corpus_text: str) -> dict:
    """Map section number to the heading title line found in the manual body.

    Table-of-contents lines carry dot leaders and page numbers, so they are
    skipped: only a real body heading counts as evidence for a title. The first
    occurrence wins, which matches how the section tree is laid out.
    """
    found = {}
    for line in corpus_text.splitlines():
        if "..." in line:
            continue
        match = MANUAL_HEADING_RE.match(line.replace("\f", " "))
        if match is None:
            continue
        title = match.group("title").strip().rstrip("。")
        found.setdefault(match.group("number"), title)
    return found


def manual_heading_numbers(corpus_text: str) -> set:
    return set(manual_heading_lines(corpus_text).keys())


def whitelist_gaps(sections, tokens, headings) -> list:
    """Sections proven by neither an analysis token nor a manual heading line.

    An empty result means every whitelisted section is traceable to the manual;
    a non-empty one is the fabrications report. Kept separate from the pytest
    test so the negative control can feed it a known-bad section without
    editing the fixture.
    """
    return [s for s in sections if s not in tokens and s not in headings]


def validate_titles_against_manual(titles: dict, corpus_text: str) -> list:
    """Each section's fixture title must be a real heading line in the manual.

    This is what stops the fixture from being self-referential: a wrong title
    (or a right title attached to the wrong number) is caught here, because the
    corpus is the manual itself, not a file the fixture authors control.
    """
    headings = manual_heading_lines(corpus_text)
    errors = []
    for section in sorted(titles):
        expected = titles[section]
        actual = headings.get(section)
        if actual is None:
            errors.append(
                "the manual has no body heading line for §%s" % section
            )
        elif expected != actual:
            errors.append(
                "fixture title for §%s is %r but the manual heading line "
                "reads %r" % (section, expected, actual)
            )
    return errors


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
# Banned-alias policy (glossary-alias-correction)
# ---------------------------------------------------------------------------

# The alias column is machine-enforced, so it may only carry strings that are
# a wrong name for the row's own concept. These are the sections that have to
# quote the forbidden strings in order to explain the policy, and are
# therefore excluded from the alias scan of the glossary itself.
ALIAS_CRITERIA_HEADING = "禁用别名判定规则"
ALIAS_EXEMPT_SECTIONS = (ALIAS_CRITERIA_HEADING, "已移除与不登记的疑似别名")

ALIAS_CRITERIA_KEYWORDS = ["同一概念", "非普通工程用词", "不另有所指", "可机械判定"]
ALIAS_CRITERIA_MIN_ITEMS = 4
ALIAS_SCAN_SCOPE_TOKEN = "docs/pid-platform"

RETIRED_ALIAS_COLUMNS = ["词", "曾被归到", "理由"]
RETIRED_ALIAS_MIN_ROWS = 15
RETIRED_ALIAS_REASON_MIN_CHARS = 20

# Words the criteria disqualify. Registering any of them as a banned alias is
# the defect this contract exists to prevent, so the validator rejects the
# registration outright rather than trusting the prose around it. "Was never
# a second name for the object" and "requirement prose must use it" are the
# two reasons a word lands here.
NOT_AN_ALIAS = {
    "阀体": "阀体是阀门的组成部分，与阀门不是同一对象",
    "WBS": "WBS 与 PBS 是两套不同标准的分解结构",
    "控制回路": "控制回路属于控制方案，与仪表回路不是同一概念",
    "线段": "线段是几何形态描述，手册用它说明管段在图面上的外观",
    "编号": "编号是通用计数词，需求正文必须使用",
    "数据字段": "手册把数据要素定义为数据字段或属性，正文必须使用",
    "数据项": "手册在权限控制正文用数据项指可修改的数据条目，另有所指",
    "基础数据": "基础数据泛指项目开工前的基础资料，需求正文必须使用",
    "料表": "料表泛指任何材料表，需求正文必须使用",
    "物料表": "物料表泛指任何材料表，需求正文必须使用",
    "支管": "支管是管道专业中真实存在的构件称谓",
    "主线": "手册中主线指绘图栅格主线，另有所指",
    "接管": "接管是真实存在的管道构件，手册正文使用",
    "接口": "接口是通用词，API/数据/界面接口都要用它",
    "配件": "配件泛指备品备件与附件，与管件不是同一类别",
    "接头": "接头兼指电气与机械接头，与管件不是同一类别",
    "仪器": "仪器指实验室与精密测量器具，且仪器仪表是惯用并列短语",
    "阀类": "阀类是类别集合称谓，不是单个对象的错名",
    "设备对象": "设备对象指对象实例层面，需求正文要与其他对象并列使用",
    "设备接口": "设备接口泛指设备对外的一切接口",
    "信号电缆": "信号电缆是实物采购件，信号线是图面连线对象",
    "控制线": "控制线另指控制线路，含义随语境漂移",
    "模板属性": "手册用源模板属性值指模板项的属性值，另有所指",
    "属性集模板": "指属性集的模板，是另一层组合对象，本规范包未定义属性集",
}


def alias_scan_body(text: str) -> str:
    """Glossary text with the parts that must quote aliases removed.

    Only the alias column of the 术语表 table and the two policy sections are
    dropped; the term names, English names, definitions and maintenance rules
    stay in scope, so a stray alias in the glossary's own prose is still
    reported. The scan is a plain substring test, matching how the pytest
    module enforces the column.
    """
    kept = []
    skip_level = None
    table_header = None
    for line in text.splitlines():
        heading = _HEADING_RE.match(line)
        if heading:
            level = len(heading.group(1))
            if skip_level is not None and level <= skip_level:
                skip_level = None
            title = normalize_heading(heading.group(2))
            if skip_level is None and title in ALIAS_EXEMPT_SECTIONS:
                skip_level = level
                table_header = None
                continue
        if skip_level is not None:
            continue
        stripped = line.strip()
        if not stripped.startswith("|"):
            table_header = None
            kept.append(line)
            continue
        cells = _split_cells(line)
        if table_header is None:
            table_header = cells
            kept.append(line)
            continue
        if table_header == GLOSSARY_COLUMNS and len(cells) == len(GLOSSARY_COLUMNS):
            if not _is_separator(line):
                cells = cells[:-1]
            kept.append("| " + " | ".join(cells) + " |")
            continue
        kept.append(line)
    return "\n".join(kept)


def alias_scan_documents(docs_root) -> dict:
    """Every markdown deliverable under ``docs_root`` as {relative_path: text}."""
    root = Path(docs_root)
    documents = {}
    if not root.is_dir():
        return documents
    for path in sorted(root.rglob("*.md")):
        name = str(path.relative_to(root)).replace(os.sep, "/")
        documents[name] = path.read_text(encoding="utf-8")
    return documents


def scan_alias_violations(glossary_text: str, documents: dict) -> list:
    """Report every banned alias found in ``documents`` as (file, word) pairs.

    ``documents`` maps a display path to its text. The glossary is scanned with
    :func:`alias_scan_body`, because it has to spell the aliases out in order to
    ban them. Errors name both the file and the word so a failure is
    actionable without re-running the scan by hand.
    """
    aliases = banned_alias_map(glossary_text)
    errors = []
    for name in sorted(documents):
        body = documents[name]
        if Path(name).name == "glossary.md":
            body = alias_scan_body(body)
        for alias in sorted(aliases):
            if alias and alias in body:
                errors.append(
                    "%s uses banned alias '%s'; use %s instead"
                    % (name, alias, aliases[alias])
                )
    return errors


def retired_alias_rows(text: str):
    """Return (rows, errors) for the 已移除与不登记的疑似别名 table."""
    body = section_body(text, 2, ALIAS_EXEMPT_SECTIONS[1])
    if body is None:
        return [], [
            "glossary.md has no H2 section named " + ALIAS_EXEMPT_SECTIONS[1]
        ]
    table = find_table(body, RETIRED_ALIAS_COLUMNS)
    if table is None:
        return [], [
            "the %s section has no table with header %s"
            % (ALIAS_EXEMPT_SECTIONS[1], " | ".join(RETIRED_ALIAS_COLUMNS))
        ]
    rows = []
    errors = []
    for index, row in enumerate(table["rows"], start=2):
        if len(row) != len(RETIRED_ALIAS_COLUMNS):
            errors.append(
                "retired-alias row %d has %d cells, expected %d"
                % (index, len(row), len(RETIRED_ALIAS_COLUMNS))
            )
            continue
        rows.append(dict(zip(RETIRED_ALIAS_COLUMNS, row)))
    return rows, errors


def alias_hygiene_errors(text: str) -> list:
    """Structural rules that keep the alias column mechanically decidable."""
    rows, _ = glossary_rows(text)
    aliases = banned_alias_map(text)
    canonical = list(rows)
    sanctioned = [s for forms in SANCTIONED_ALIASES.values() for s in forms]
    errors = []
    for alias in sorted(aliases):
        if len(alias) < 2:
            errors.append(
                "banned alias '%s' is shorter than 2 characters and cannot be "
                "scanned safely" % alias
            )
        if alias in rows:
            errors.append(
                "'%s' is registered as both a canonical term and a banned alias"
                % alias
            )
        for term in canonical:
            if alias != term and alias in term:
                errors.append(
                    "banned alias '%s' is a substring of canonical term '%s'"
                    % (alias, term)
                )
        for short in sanctioned:
            if alias in short:
                errors.append(
                    "banned alias '%s' is a substring of the sanctioned short "
                    "form '%s'" % (alias, short)
                )
    for first, second in combinations(sorted(aliases), 2):
        if first in second or second in first:
            errors.append(
                "banned aliases '%s' and '%s' are substrings of one another"
                % (first, second)
            )
    return errors


def validate_alias_policy(text: str) -> list:
    """Check the alias column against the criteria documented in the glossary."""
    errors = []
    body = section_body(text, 2, ALIAS_CRITERIA_HEADING)
    if body is None:
        errors.append("glossary.md is missing the H2 section " + ALIAS_CRITERIA_HEADING)
    else:
        missing = [k for k in ALIAS_CRITERIA_KEYWORDS if k not in body]
        if missing:
            errors.append(
                "the %s section does not state criterion/criteria: %s"
                % (ALIAS_CRITERIA_HEADING, "、".join(missing))
            )
        items = re.findall(r"(?m)^\s*\*{0,2}\s*\d+[.、)]\s*\*{0,2}\s*\S", body)
        if len(items) < ALIAS_CRITERIA_MIN_ITEMS:
            errors.append(
                "the %s section states %d numbered criteria, expected at least %d"
                % (ALIAS_CRITERIA_HEADING, len(items), ALIAS_CRITERIA_MIN_ITEMS)
            )
        if ALIAS_SCAN_SCOPE_TOKEN not in body:
            errors.append(
                "the %s section never names the scan scope %s"
                % (ALIAS_CRITERIA_HEADING, ALIAS_SCAN_SCOPE_TOKEN)
            )

    retired, retired_errors = retired_alias_rows(text)
    errors.extend(retired_errors)
    if retired:
        if len(retired) < RETIRED_ALIAS_MIN_ROWS:
            errors.append(
                "the retired-alias table has %d rows, expected at least %d"
                % (len(retired), RETIRED_ALIAS_MIN_ROWS)
            )
        for row in retired:
            reason = row["理由"].strip()
            if len(reason) < RETIRED_ALIAS_REASON_MIN_CHARS:
                errors.append(
                    "retired entry '%s' has a %d-character reason; it must "
                    "explain the judgement" % (row["词"], len(reason))
                )
            if not re.search(r"(?:判据|第)\s*[1-4]", reason):
                errors.append(
                    "retired entry '%s' does not name the criterion it "
                    "violates" % row["词"]
                )
        listed = {row["词"].strip() for row in retired}
        missing = [w for w in NOT_AN_ALIAS if w not in listed]
        if missing:
            errors.append(
                "the retired-alias table omits %d word(s) the criteria "
                "disqualify: %s" % (len(missing), "、".join(missing))
            )
    else:
        listed = set()

    aliases = banned_alias_map(text)
    for word in sorted(NOT_AN_ALIAS):
        if word in aliases:
            errors.append(
                "glossary registers '%s' as a banned alias of %s, but it is "
                "not an alias: %s" % (word, aliases[word], NOT_AN_ALIAS[word])
            )
    for word in sorted(set(aliases) & listed):
        errors.append(
            "'%s' is both a registered banned alias and a retired suspect"
            % word
        )
    errors.extend(alias_hygiene_errors(text))
    return errors


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


# ---------------------------------------------------------------------------
# Citation-title appendix (source-index.md)
# ---------------------------------------------------------------------------


def _body_before_appendix(text: str) -> str:
    """``text`` with the citation appendix removed, so it cannot self-certify."""
    marker = re.search(
        r"^##[ \t]+" + re.escape(CITATION_APPENDIX_HEADING) + r"[ \t]*$",
        text,
        re.MULTILINE,
    )
    return text[: marker.start()] if marker else text


def document_cited_sections(text: str) -> set:
    """Every manual section a document cites, ignoring its own appendix."""
    return set(section_tokens(_body_before_appendix(text)))


def citation_appendix(text: str):
    """Return ({section: title}, errors) for the 节号 / 手册标题 appendix table."""
    body = section_body(text, 2, CITATION_APPENDIX_HEADING)
    if body is None:
        return {}, ["missing H2 section " + CITATION_APPENDIX_HEADING]
    table = find_table(body, CITATION_APPENDIX_COLUMNS)
    if table is None:
        return {}, [
            "the %s section has no table with header %s"
            % (
                CITATION_APPENDIX_HEADING,
                " | ".join(CITATION_APPENDIX_COLUMNS),
            )
        ]
    rows = {}
    errors = []
    for index, cells in enumerate(table["rows"], start=2):
        if len(cells) != len(CITATION_APPENDIX_COLUMNS):
            errors.append(
                "citation appendix row %d has %d cells, expected %d: %s"
                % (index, len(cells), len(CITATION_APPENDIX_COLUMNS), " | ".join(cells))
            )
            continue
        reference = cells[0].strip()
        title = cells[1].strip()
        match = SECTION_REF_RE.match(reference)
        if match is None:
            errors.append(
                "citation appendix row %d has 节号 %r; expected §x.y.z"
                % (index, reference)
            )
            continue
        section = match.group(1)
        if section in rows:
            errors.append(
                "citation appendix lists §%s more than once" % section
            )
            continue
        if len(title) < 2:
            errors.append(
                "citation appendix row %d (§%s) has an empty 手册标题"
                % (index, section)
            )
        rows[section] = title
    return rows, errors


def _section_sort_key(section: str) -> list:
    return [int(part) for part in section.split(".")]


def validate_citation_appendix(index_text: str, summary_text: str, titles=None) -> list:
    """The appendix must cover exactly the sections the two documents cite.

    Three facts are checked, and each has its own negative control:
    the appendix exists and parses; its section set equals the union of the
    sections cited by source-index.md and reference-product-summary.md (both
    directions, so neither an omission nor a stale row passes); and every row's
    title equals the fixture title character for character.
    """
    if titles is None:
        titles = section_titles()
    errors = []
    rows, row_errors = citation_appendix(index_text)
    errors.extend(row_errors)
    cited = document_cited_sections(index_text) | document_cited_sections(summary_text)
    for section in sorted(cited - set(rows), key=_section_sort_key):
        errors.append(
            "§%s is cited by a document but missing from the %s appendix"
            % (section, CITATION_APPENDIX_HEADING)
        )
    for section in sorted(set(rows) - cited, key=_section_sort_key):
        errors.append(
            "the %s appendix lists §%s but no document cites it"
            % (CITATION_APPENDIX_HEADING, section)
        )
    for section in sorted(rows, key=_section_sort_key):
        expected = titles.get(section)
        if expected is None:
            errors.append(
                "the %s appendix cites §%s, which is absent from the "
                "manual-section fixture" % (CITATION_APPENDIX_HEADING, section)
            )
        elif rows[section] != expected:
            errors.append(
                "the %s appendix title for §%s is %r; the fixture records %r"
                % (CITATION_APPENDIX_HEADING, section, rows[section], expected)
            )
    return errors


# ---------------------------------------------------------------------------
# gap-analysis.md (VAL-DOC-005)
# ---------------------------------------------------------------------------

GAP_COLUMNS = ["能力", "现状", "目标状态", "处置", "证据路径"]
GAP_MATRIX_SECTION = "能力矩阵"
GAP_ABSENT_SECTION = "已确认不存在的能力"
GAP_DISPOSITIONS = ["复用", "改造", "新增"]
GAP_EVIDENCE_PREFIXES = ("src/", "crates/", "assets/", "docs/")
GAP_NO_EVIDENCE = "—"
GAP_MIN_ROWS = 20

# Capabilities the matrix must cover because they exist in the repository
# today. "pid-legend.json" is normally satisfied by the evidence path.
GAP_REQUIRED_TOKENS = [
    "图例识别器",
    "管道连通性推断",
    "视图过滤",
    "PIDLEGEND",
    "PIDLINE",
    "PIDTAG",
    "PIDGROUP",
    "PIDPLOT",
    "自动化 API",
    "插件系统",
    "DWG/DXF",
    "统一导出 schema",
    "图例规则集",
    "pid-legend.json",
]

# The ten capabilities VAL-DOC-005 requires the 已确认不存在的能力 section
# to enumerate, verbatim.
GAP_CONFIRMED_ABSENT = [
    "位号主数据与带类型属性模型",
    "跨图纸拓扑持久化",
    "可配置校验引擎",
    "P&ID 语义材料清单",
    "服务端项目库",
    "多用户权限与审计",
    "图例库可视化编辑器",
    "协同设计",
    "审计日志",
    "数据交换标准（DEXPI/ISO 15926/CFIHOS）",
]


def gap_matrix_rows(text: str):
    """Return (rows, errors) for the five-column capability matrix."""
    body = section_body(text, 2, GAP_MATRIX_SECTION)
    if body is None:
        return [], ["missing H2 section " + GAP_MATRIX_SECTION]
    table = find_table(body, GAP_COLUMNS)
    if table is None:
        return [], [
            "capability matrix table not found; expected header "
            + " | ".join(GAP_COLUMNS)
        ]
    rows = []
    errors = []
    for index, cells in enumerate(table["rows"], start=2):
        if len(cells) != len(GAP_COLUMNS):
            errors.append(
                "capability matrix row %d has %d cells, expected %d: %s"
                % (index, len(cells), len(GAP_COLUMNS), " | ".join(cells))
            )
            continue
        rows.append(dict(zip(GAP_COLUMNS, cells)))
    return rows, errors


def gap_evidence_paths(cell: str) -> list:
    """Split a 证据路径 cell into individual repo-relative paths."""
    parts = re.split(r"[、,，;；\s]+", cell)
    return [p for p in parts if p]


def validate_gap_analysis(text: str, repo_root=None) -> list:
    """Check VAL-DOC-005 against gap-analysis.md.

    ``repo_root`` is optional; when supplied, every evidence path on a
    复用/改造 row must resolve to a real file or directory under it.
    """
    errors = []
    if h1_count(text) != 1:
        errors.append(
            "gap-analysis.md must contain exactly one H1, found %d" % h1_count(text)
        )
    matrix = section_body(text, 2, GAP_MATRIX_SECTION)
    absent = section_body(text, 2, GAP_ABSENT_SECTION)
    if matrix is None:
        errors.append("missing H2 section " + GAP_MATRIX_SECTION)
    if absent is None:
        errors.append("missing H2 section " + GAP_ABSENT_SECTION)

    rows, row_errors = gap_matrix_rows(text)
    errors.extend(row_errors)
    if len(rows) < GAP_MIN_ROWS:
        errors.append(
            "capability matrix has %d data rows, the contract requires at "
            "least %d" % (len(rows), GAP_MIN_ROWS)
        )
    root = Path(repo_root) if repo_root is not None else None
    for index, row in enumerate(rows, start=2):
        capability = row["能力"].strip()
        status = row["现状"].strip()
        target = row["目标状态"].strip()
        disposition = row["处置"].strip()
        evidence = row["证据路径"].strip()
        if len(capability) < 2:
            errors.append("capability matrix row %d has an empty 能力" % index)
        if len(status) < 8:
            errors.append(
                "capability matrix row %d (%s) has a 现状 shorter than 8 "
                "characters" % (index, capability)
            )
        if len(target) < 6:
            errors.append(
                "capability matrix row %d (%s) has a 目标状态 shorter than 6 "
                "characters" % (index, capability)
            )
        if disposition not in GAP_DISPOSITIONS:
            errors.append(
                "capability matrix row %d (%s) has 处置 %r outside the enum 复用/"
                "改造/新增" % (index, capability, disposition)
            )
            continue
        if disposition == "新增":
            if evidence not in (GAP_NO_EVIDENCE, "-", "无"):
                errors.append(
                    "capability matrix row %d (%s) is 新增 but its 证据路径 is %r; "
                    "it must be %s"
                    % (index, capability, evidence, GAP_NO_EVIDENCE)
                )
            continue
        if evidence in ("", GAP_NO_EVIDENCE, "-", "无"):
            errors.append(
                "capability matrix row %d (%s) is %s but carries no 证据路径"
                % (index, capability, disposition)
            )
            continue
        if not evidence.startswith(GAP_EVIDENCE_PREFIXES):
            errors.append(
                "capability matrix row %d (%s) has 证据路径 %r outside src/, "
                "crates/, assets/, docs/" % (index, capability, evidence)
            )
            continue
        if root is not None:
            for path in gap_evidence_paths(evidence):
                if not path.startswith(GAP_EVIDENCE_PREFIXES):
                    errors.append(
                        "capability matrix row %d (%s) has 证据路径 %r outside "
                        "src/, crates/, assets/, docs/"
                        % (index, capability, path)
                    )
                    continue
                if not (root / path).exists():
                    errors.append(
                        "capability matrix row %d (%s) cites a path that does "
                        "not exist in the repository: %s"
                        % (index, capability, path)
                    )

    if matrix is not None:
        missing = [t for t in GAP_REQUIRED_TOKENS if t not in matrix]
        if missing:
            errors.append(
                "capability matrix does not cover these existing capabilities: "
                + "、".join(missing)
            )

    if absent is not None:
        subsections = dict(sections_at(absent, 3))
        for item in GAP_CONFIRMED_ABSENT:
            body = None
            for name, candidate in subsections.items():
                if item in name:
                    body = candidate
                    break
            if body is None:
                pattern = re.compile(
                    r"\*\*" + re.escape(item) + r"\*\*(.*?)"
                    r"(?=\n\s*[-*]\s*\*\*|\Z)",
                    re.DOTALL,
                )
                match = pattern.search(absent)
                body = match.group(1) if match else None
            if body is None:
                errors.append(
                    GAP_ABSENT_SECTION + " does not address: " + item
                )
                continue
            if len(body.strip()) < 60:
                errors.append(
                    "%s entry '%s' is too thin (%d characters) to say what is "
                    "missing" % (GAP_ABSENT_SECTION, item, len(body.strip()))
                )
            if "新增" not in body:
                errors.append(
                    "%s entry '%s' does not state that the capability is 新增"
                    % (GAP_ABSENT_SECTION, item)
                )
    return errors
