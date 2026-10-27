"""The ``-k traceability`` gate: requirement IDs, area files and the matrix.

Every test name starts with ``test_traceability_`` so ``pytest -k traceability``
selects this module and nothing else. The validators live in
``requirements_contract.py``; the negative controls below prove each of the six
failure classes is actually detected, not merely described.
"""
from __future__ import annotations

import json

import pytest

import conftest
from conftest import CATALOG, DOCS, SCREEN_IDS
from manifest_contract import normalize_heading
from requirements_contract import (
    AREA_FLOOR,
    CONNECTION_POINT_DIRECTIONS,
    CONNECTION_POINT_TYPES,
    DRAWING_SECTIONS,
    MATRIX_COLUMNS,
    OBSERVABLE_WORDS,
    OVERVIEW_PATH,
    REQ_AREA_FILES,
    REQ_ID_RE,
    REQ_ID_TEXT,
    SYMBOL_SECTIONS,
    TAG_RULE_FIELDS,
    TYPE_SECTIONS,
    PLATFORM_SECTIONS,
    definitions,
    load_all_documents,
    load_documents,
    matrix_rows,
    section_requirement_counts,
    section_text,
    split_entries,
    strip_list_number,
    validate_area_floors,
    validate_area_table,
    validate_connection_point_table,
    validate_drawing_coverage,
    validate_matrix,
    validate_platform_coverage,
    validate_requirement_ids,
    validate_entries,
    validate_symbol_library_coverage,
    validate_traceability,
    validate_type_library_coverage,
    whitelisted_sections,
)

PLAT_PATH = "requirements/10-platform.md"
TYPE_PATH = "requirements/20-type-library.md"
SYMBOL_PATH = "requirements/30-symbol-library-and-project-settings.md"
DRAWING_PATH = "requirements/40-drawing-and-drafting.md"

# Every declared section, paired with the area document that must carry it as
# an H2 heading. The names themselves live in the contract module; the pairing
# is what makes the parametrized check below assert file state.
SECTION_HEADINGS = (
    [(PLAT_PATH, name) for name in PLATFORM_SECTIONS]
    + [(TYPE_PATH, name) for name in TYPE_SECTIONS]
    + [(SYMBOL_PATH, name) for name in SYMBOL_SECTIONS]
    + [(DRAWING_PATH, name) for name in DRAWING_SECTIONS]
)


def _screen_ids() -> list:
    """The 14 prototype screen ids, from the catalog once it exists."""
    if CATALOG.exists():
        with CATALOG.open(encoding="utf-8") as handle:
            catalog = json.load(handle)
        return [screen["id"] for screen in catalog.get("screens", [])]
    return list(SCREEN_IDS)


def _entry(req_id: str, source: str = "§4.1.1", title: str = "示例需求") -> str:
    return (
        "### %s %s\n\n"
        "- **优先级**：必须\n"
        "- **用户故事**：作为设计人员，我希望执行示例能力，以便验证契约。\n"
        "- **验收标准**：\n"
        "  1. 返回 `OK` 且计数为 1。\n"
        "  2. 列表行数增加 1。\n"
        "- **数据与界面契约**：\n"
        "  | 字段 | 类型 | 必填 | 约束 |\n"
        "  | --- | --- | --- | --- |\n"
        "  | 示例字段 | String | 是 | ≤ 64 字符 |\n"
        "- **来源**：%s\n"
        % (req_id, title, source)
    )


def _vague_entry(req_id: str) -> str:
    """An entry whose criteria are pure sentiment, each with a different list marker.

    ``1.`` / ``2、`` / ``3)`` are the three markers the contract accepts; all of
    them must be stripped before decidability is judged.
    """
    return (
        "### %s 情感描述不可判定\n\n"
        "- **优先级**：必须\n"
        "- **用户故事**：作为设计人员，我希望系统好用，以便提升效率。\n"
        "- **验收标准**：\n"
        "  1. 用户感觉系统表现良好。\n"
        "  2、用户觉得体验顺畅。\n"
        "  3) 界面令人满意。\n"
        "- **数据与界面契约**：\n"
        "  | 字段 | 类型 | 必填 | 约束 |\n"
        "  | --- | --- | --- | --- |\n"
        "  | 示例字段 | String | 是 | ≤ 64 字符 |\n"
        "- **来源**：§4.1.1\n" % req_id
    )


def _threshold_entry(req_id: str) -> str:
    """An entry decidable only through numeric thresholds, no backticks or state words."""
    return (
        "### %s 账户锁定阈值\n\n"
        "- **优先级**：必须\n"
        "- **用户故事**：作为项目管理员，我希望连续失败后锁定账户，以便抵御暴力破解。\n"
        "- **验收标准**：\n"
        "  1. 连续 5 次登录失败后锁定 30 分钟。\n"
        "  2. 第 31 分钟起计数归零，可再次尝试。\n"
        "- **数据与界面契约**：\n"
        "  | 字段 | 类型 | 必填 | 约束 |\n"
        "  | --- | --- | --- | --- |\n"
        "  | 锁定阈值 | Integer | 是 | 1–10 次 |\n"
        "- **来源**：§4.1.1\n" % req_id
    )


def _h2_sections(text: str) -> set:
    """Normalized names of every H2 heading in ``text``."""
    return set(section_requirement_counts(text))


def _area_table() -> str:
    rows = [
        "| %s | `docs/pid-platform/requirements/%s` | 示例范围 |"
        % (area, name)
        for area, name in REQ_AREA_FILES.items()
    ]
    return (
        "## 区域与文件映射\n\n"
        "| 区域 | 文档 | 范围 |\n"
        "| --- | --- | --- |\n" + "\n".join(rows) + "\n"
    )


def _matrix(rows: list) -> str:
    body = "\n".join(
        "| %s | %s | %s | %s |" % row for row in rows
    )
    return (
        "## 需求追溯矩阵\n\n"
        "| %s |\n"
        "| --- | --- | --- | --- |\n" % " | ".join(MATRIX_COLUMNS) + body + "\n"
    )


def _overview(matrix_rows_: list) -> str:
    return (
        "# 需求体系总纲\n\n"
        "## 需求 ID 规范\n\n"
        "需求 ID 的锚定正则为 `%s`。\n\n" % REQ_ID_TEXT
        + _area_table()
        + "\n"
        + _matrix(matrix_rows_)
        + "\n## 校验规则\n\n锚定、唯一、每区下限与文件对应逐条机械校验。\n"
    )


def _documents(platform_ids: list, type_ids: list, matrix: list) -> dict:
    platform = "# 平台层需求\n\n## 身份与会话\n\n" + "\n".join(
        _entry(req_id) for req_id in platform_ids
    )
    type_library = "# 类型库与属性模型需求\n\n## 业务类型树\n\n" + "\n".join(
        _entry(req_id) for req_id in type_ids
    )
    return {
        OVERVIEW_PATH: _overview(matrix),
        PLAT_PATH: platform,
        TYPE_PATH: type_library,
    }


def _valid_documents() -> dict:
    platform_ids = ["REQ-PLAT-%03d" % number for number in range(1, AREA_FLOOR + 1)]
    type_ids = ["REQ-TYPE-%03d" % number for number in range(1, AREA_FLOOR + 1)]
    matrix = [(req_id, "PLAT", "无界面", "平台认证服务承载，原型不含登录屏") for req_id in platform_ids]
    matrix += [(req_id, "TYPE", "typelib", "类型库编辑器屏承载类型树") for req_id in type_ids]
    return _documents(platform_ids, type_ids, matrix)


CONNECTION_POINT_ROWS = [
    ("等径", "双向点", "等径直通管件两端，介质可双向流动"),
    ("等径", "流入点", "等径单向流元件入口，介质只能流入"),
    ("等径", "流出点", "等径单向流元件出口，介质只能流出"),
    ("异径", "双向点", "异径管两端的变径连接，介质可双向流动"),
    ("异径", "流入点", "异径单向流元件入口，介质只能流入"),
    ("异径", "流出点", "异径单向流元件出口，介质只能流出"),
    ("信号线", "双向点", "仪表之间的双向信号传递"),
    ("信号线", "流入点", "进入仪表的单向信号"),
    ("信号线", "流出点", "离开仪表的单向信号"),
]


def _symbol_document() -> str:
    """A synthetic symbol-library document that satisfies every VAL-REQ-006 check.

    The negative controls below mutate one fact at a time, so each failure class
    is proven to be detected rather than merely described.
    """
    connection_table = (
        "| 类型 | 方向 | 用途 |\n"
        "| --- | --- | --- |\n"
        + "\n".join("| %s | %s | %s |" % row for row in CONNECTION_POINT_ROWS)
        + "\n"
    )
    tag_table = (
        "| 字段 | 类型 | 必填 | 约束 |\n"
        "| --- | --- | --- | --- |\n"
        + "\n".join("| %s | String | 是 | — |" % name for name in TAG_RULE_FIELDS)
        + "\n"
    )
    extras = {
        "连接点": connection_table,
        "位号规则": tag_table + "\n格式 `P-{序号}` 配起始编码 1001 生成 `P-1001`。\n\n",
        "颜色规则": "条件要素：属性、运算符、值、连接符；当前类型无规则时继承父类型规则。\n\n",
        "相交断线与打断符": (
            "7 种管线类型权重中夹套管最高、在役次工艺管线最低；"
            "打断符样式 3 种；打断符高度 默认 3。\n\n"
        ),
    }
    parts = ["# 符号库与项目设置需求\n\n共 %d 条需求。\n\n" % len(SYMBOL_SECTIONS)]
    for index, name in enumerate(SYMBOL_SECTIONS, start=1):
        parts.append("## %s\n\n" % name)
        parts.append(extras.get(name, ""))
        parts.append(_entry("REQ-SYM-%03d" % index, title="%s示例" % name))
        parts.append("\n")
    return "".join(parts)


def _drawing_document() -> str:
    """A synthetic 40-drawing document carrying every VAL-REQ-007 phrase."""
    extras = {
        "管件与阀门插入": "插入时管线自动断开，移动时自动补位。\n\n",
        "设备与管口": (
            "管口位号前缀继承设备位号；未设默认管口时生成逻辑连接点；"
            "关联设备属性共享。\n\n"
        ),
        "页间连接符": (
            "管道连接符流入、管道连接符流出、信号线连接符流入、"
            "信号线连接符流出。\n\n"
        ),
        "草图设置与用户环境": "草图设置随图纸保存，用户环境随用户保存。\n\n",
        "管线与管路层级": "管线 → 管路 → 管段，属性下传。\n\n",
        "仪表与仪表回路": (
            "回路与仪表多对多；支持新建仪表回路、添加仪表、移除仪表。\n\n"
        ),
        "图纸生命周期": "新建、导入 SVG/DWG、复制、移动、权限、预览。\n\n",
        "图纸版本与比对": "版本快照可两两比对。\n\n",
        "对象复制": "支持图纸内与跨图纸复制。\n\n",
    }
    parts = ["# 图纸与绘图需求\n\n共 %d 条需求。\n\n" % len(DRAWING_SECTIONS)]
    for index, name in enumerate(DRAWING_SECTIONS, start=1):
        parts.append("## %s\n\n" % name)
        parts.append(extras.get(name, ""))
        parts.append(_entry("REQ-DRW-%03d" % index, title="%s示例" % name))
        parts.append("\n")
    return "".join(parts)


# ---------------------------------------------------------------------------
# Real documents
# ---------------------------------------------------------------------------


def test_traceability_requirement_ids_follow_the_anchored_pattern():
    documents = load_all_documents(DOCS)
    assert documents, "no markdown documents under " + str(DOCS)
    errors = validate_requirement_ids(documents)
    assert errors == [], "requirement-ID errors: %s" % errors


def test_traceability_requirement_ids_are_globally_unique():
    documents = load_documents(DOCS)
    entries = [
        entry
        for text in documents.values()
        for entry in split_entries(text)
    ]
    assert entries, "no requirement headings found under " + str(DOCS)
    ids = [entry["id"] for entry in entries]
    duplicates = sorted({req_id for req_id in ids if ids.count(req_id) > 1})
    assert duplicates == [], "duplicate requirement IDs: %s" % duplicates
    assert len(definitions(documents)) == len(entries), (
        "definitions() collapsed %d entries into %d IDs"
        % (len(entries), len(definitions(documents)))
    )


def test_traceability_every_authored_area_document_meets_the_five_floor():
    documents = load_documents(DOCS)
    authored = [area for area, name in REQ_AREA_FILES.items() if "requirements/" + name in documents]
    assert "PLAT" in authored and "TYPE" in authored, (
        "the platform and type-library documents must be authored, found: %s" % authored
    )
    errors = validate_area_floors(documents)
    assert errors == [], "area floor errors: %s" % errors
    for area in authored:
        path = "requirements/" + REQ_AREA_FILES[area]
        count = len([entry for entry in split_entries(documents[path]) if entry["area"] == area])
        assert count >= AREA_FLOOR, "%s defines only %d %s requirements" % (path, count, area)


def test_traceability_area_segment_matches_the_owning_document():
    documents = load_documents(DOCS)
    errors = validate_requirement_ids(documents)
    misplaced = [error for error in errors if "belongs in" in error or "unknown area" in error]
    assert misplaced == [], "area/file mismatches: %s" % misplaced


def test_traceability_overview_documents_the_anchored_regex():
    documents = load_documents(DOCS)
    overview = documents.get(OVERVIEW_PATH)
    assert overview is not None, "%s is missing" % OVERVIEW_PATH
    assert REQ_ID_TEXT in overview, (
        "%s does not carry the anchored pattern verbatim: %s" % (OVERVIEW_PATH, REQ_ID_TEXT)
    )
    assert "(?<![A-Za-z-])" in overview and "(?!\\d)" in overview, (
        "the documented pattern is missing a negative lookaround"
    )
    assert REQ_ID_RE.pattern == REQ_ID_TEXT
    assert conftest.REQ_ID_RE.pattern == REQ_ID_TEXT, (
        "conftest and requirements_contract disagree on the anchored pattern"
    )


def test_traceability_area_table_maps_nine_areas_to_nine_files():
    documents = load_documents(DOCS)
    overview = documents.get(OVERVIEW_PATH)
    assert overview is not None, "%s is missing" % OVERVIEW_PATH
    errors = validate_area_table(overview)
    assert errors == [], "area table errors: %s" % errors
    assert conftest.REQ_AREA_FILES == REQ_AREA_FILES, (
        "conftest.REQ_AREA_FILES diverged from requirements_contract.REQ_AREA_FILES"
    )


def test_traceability_matrix_and_definitions_are_mutually_complete():
    documents = load_documents(DOCS)
    overview = documents[OVERVIEW_PATH]
    defined = definitions(documents)
    errors = validate_matrix(overview, defined, _screen_ids())
    missing = [error for error in errors if "missing from the traceability matrix" in error]
    orphans = [error for error in errors if "has no requirement definition" in error]
    assert missing == [], "requirements omitted from the matrix: %s" % missing
    assert orphans == [], "matrix rows without a definition: %s" % orphans
    assert errors == [], "matrix errors: %s" % errors
    assert {row["id"] for row in matrix_rows(overview)} == set(defined)


def test_traceability_matrix_screen_values_are_catalog_screens_or_interface_free():
    documents = load_documents(DOCS)
    rows = matrix_rows(documents[OVERVIEW_PATH])
    assert rows, "the traceability matrix has no rows"
    allowed = set(_screen_ids()) | {"无界面"}
    unknown = [
        "%s: %s" % (row["line"], row["screen"])
        for row in rows
        if row["screen"] not in allowed
    ]
    assert unknown == [], "matrix rows use unknown screens: %s" % unknown
    interface_free = [row for row in rows if row["screen"] == "无界面"]
    assert interface_free, "expected at least one 无界面 row (login, audit, config)"
    thin = [row["id"] for row in interface_free if len(row["note"]) < 8]
    assert thin == [], "无界面 rows without a reason: %s" % thin


def test_traceability_every_requirement_entry_has_the_five_fields():
    documents = load_documents(DOCS)
    errors = validate_entries(documents, allowed=whitelisted_sections())
    assert errors == [], "requirement entry errors: %s" % errors


def test_traceability_platform_document_covers_the_six_topics():
    errors = validate_platform_coverage(load_documents(DOCS))
    assert errors == [], "platform coverage errors: %s" % errors


def test_traceability_type_library_document_covers_the_seven_topics():
    errors = validate_type_library_coverage(load_documents(DOCS))
    assert errors == [], "type-library coverage errors: %s" % errors


def test_traceability_symbol_document_covers_the_thirteen_topics():
    errors = validate_symbol_library_coverage(load_documents(DOCS))
    assert errors == [], "symbol-library coverage errors: %s" % errors


def test_traceability_drawing_document_covers_the_thirteen_topics():
    errors = validate_drawing_coverage(load_documents(DOCS))
    assert errors == [], "drawing coverage errors: %s" % errors


def test_traceability_drawing_connectors_split_kind_by_direction():
    """VAL-REQ-007: 管道/信号线 x 流入/流出, four symbol types, none merged."""
    documents = load_documents(DOCS)
    connectors = section_text(documents[DRAWING_PATH], "页间连接符")
    for kind in ("管道连接符", "信号线连接符"):
        for direction in ("流入", "流出"):
            token = kind + direction
            assert token in connectors, "页间连接符 does not name '%s'" % token


def test_traceability_symbol_document_lists_nine_connection_point_combinations():
    """VAL-REQ-006: 3 types x 3 directions, each row carrying a 用途."""
    documents = load_documents(DOCS)
    connection = section_text(documents[SYMBOL_PATH], "连接点")
    assert validate_connection_point_table(connection) == [], "连接点 table errors: %s" % (
        validate_connection_point_table(connection),
    )
    rows = [
        line
        for line in connection.splitlines()
        if line.startswith("|") and "---" not in line
    ]
    # header + 9 data rows: the combination table is the only 3-column table here
    data_rows = [line for line in rows if line.count("|") == 4]
    assert len(data_rows) == 10, "expected the header and 9 rows, got %d: %s" % (
        len(data_rows),
        data_rows,
    )
    for kind in CONNECTION_POINT_TYPES:
        for direction in CONNECTION_POINT_DIRECTIONS:
            assert any(
                ("| %s | %s |" % (kind, direction)) in line for line in data_rows
            ), "%s/%s is missing from the 连接点 table" % (kind, direction)


def test_traceability_symbol_document_defines_the_eight_tag_rule_fields():
    documents = load_documents(DOCS)
    tag_rules = section_text(documents[SYMBOL_PATH], "位号规则")
    assert len(TAG_RULE_FIELDS) == 8, TAG_RULE_FIELDS
    for name in TAG_RULE_FIELDS:
        assert name in tag_rules, "位号规则 does not define '%s'" % name
    assert "P-1001" in tag_rules, "位号规则 carries no concrete 格式拼接 example"


def test_traceability_screen_catalog_uses_the_fourteen_contract_ids():
    assert len(SCREEN_IDS) == 14, "VAL-CROSS-001 lists 14 screen ids, got %d" % len(SCREEN_IDS)
    if not CATALOG.exists():
        assert _screen_ids() == SCREEN_IDS
        return
    assert _screen_ids() == SCREEN_IDS, (
        "screen-catalog.json ids differ from the contract list: %s" % _screen_ids()
    )


# ---------------------------------------------------------------------------
# Positive control: a synthetic but valid set must pass every validator
# ---------------------------------------------------------------------------


def test_traceability_synthetic_valid_documents_pass_every_validator():
    documents = _valid_documents()
    assert validate_requirement_ids(documents) == []
    assert validate_area_floors(documents, require_all=False) == []
    assert validate_area_table(documents[OVERVIEW_PATH]) == []
    assert validate_matrix(documents[OVERVIEW_PATH], definitions(documents), SCREEN_IDS) == []
    assert validate_entries(documents, allowed=whitelisted_sections()) == []


# ---------------------------------------------------------------------------
# Negative controls: one per failure class, each must name its location
# ---------------------------------------------------------------------------


def test_traceability_final_mode_requires_all_nine_area_documents():
    """The closeout mode must name every area document that is still absent."""
    errors = validate_area_floors(_valid_documents(), require_all=True)
    missing = [error for error in errors if "is missing" in error]
    assert len(missing) == 7, (
        "expected the seven unauthored areas to be reported, got: %s" % errors
    )
    for area in ["SYM", "DRW", "DATA", "DELV", "COLL", "IO", "MIG"]:
        assert any(area in error for error in missing), (area, missing)


def test_traceability_validator_rejects_a_definition_outside_requirements():
    documents = _valid_documents()
    documents["PRD.md"] = "# 主 PRD\n\n### REQ-PLAT-950 主文档里定义需求\n\n" + _entry(
        "REQ-PLAT-950", title="主文档里定义需求"
    )
    errors = validate_requirement_ids(documents)
    assert any("outside the requirements directory" in error for error in errors), errors


def test_traceability_validator_rejects_a_duplicate_requirement_id():
    documents = _valid_documents()
    documents[PLAT_PATH] += "\n" + _entry("REQ-PLAT-001", title="重复定义")
    errors = validate_requirement_ids(documents)
    duplicates = [error for error in errors if "is defined 2 times" in error]
    assert duplicates, "the duplicate-ID control passed: %s" % errors
    assert "REQ-PLAT-001" in duplicates[0] and PLAT_PATH in duplicates[0], duplicates[0]


def test_traceability_validator_rejects_a_wrong_area_segment():
    documents = _valid_documents()
    documents[PLAT_PATH] += "\n" + _entry("REQ-TYPE-901", title="错区域")
    errors = validate_requirement_ids(documents)
    misplaced = [error for error in errors if "belongs in" in error]
    assert misplaced, "the wrong-area control passed: %s" % errors
    assert "REQ-TYPE-901" in misplaced[0] and "20-type-library.md" in misplaced[0], misplaced[0]


def test_traceability_validator_rejects_an_id_missing_from_the_matrix():
    documents = _valid_documents()
    overview = documents[OVERVIEW_PATH]
    broken = overview.replace(
        "| REQ-PLAT-005 | PLAT | 无界面 | 平台认证服务承载，原型不含登录屏 |\n", ""
    )
    assert broken != overview, "the control could not remove the matrix row"
    documents[OVERVIEW_PATH] = broken
    errors = validate_matrix(broken, definitions(documents), SCREEN_IDS)
    missing = [error for error in errors if "missing from the traceability matrix" in error]
    assert missing, "the omission control passed: %s" % errors
    assert "REQ-PLAT-005" in missing[0] and PLAT_PATH in missing[0], missing[0]


def test_traceability_validator_rejects_an_orphan_matrix_row():
    documents = _valid_documents()
    overview = documents[OVERVIEW_PATH]
    broken = overview.replace(
        "| REQ-TYPE-005 | TYPE | typelib | 类型库编辑器屏承载类型树 |",
        "| REQ-TYPE-005 | TYPE | typelib | 类型库编辑器屏承载类型树 |\n"
        "| REQ-TYPE-777 | TYPE | typelib | 没有定义的行 |",
    )
    assert broken != overview, "the control could not append the orphan row"
    errors = validate_matrix(broken, definitions(documents), SCREEN_IDS)
    orphans = [error for error in errors if "has no requirement definition" in error]
    assert orphans, "the orphan control passed: %s" % errors
    assert "REQ-TYPE-777" in orphans[0] and OVERVIEW_PATH in orphans[0], orphans[0]


def test_traceability_validator_rejects_an_unknown_screen_id():
    documents = _valid_documents()
    overview = documents[OVERVIEW_PATH]
    broken = overview.replace(
        "| REQ-TYPE-005 | TYPE | typelib |", "| REQ-TYPE-005 | TYPE | dashboard |"
    )
    assert broken != overview, "the control could not replace the screen value"
    errors = validate_matrix(broken, definitions(documents), SCREEN_IDS)
    unknown = [error for error in errors if "unknown screen 'dashboard'" in error]
    assert unknown, "the unknown-screen control passed: %s" % errors
    assert "REQ-TYPE-005" in unknown[0] and OVERVIEW_PATH in unknown[0], unknown[0]


def test_traceability_validator_rejects_interface_free_without_a_reason():
    documents = _valid_documents()
    overview = documents[OVERVIEW_PATH]
    broken = overview.replace(
        "| REQ-PLAT-005 | PLAT | 无界面 | 平台认证服务承载，原型不含登录屏 |",
        "| REQ-PLAT-005 | PLAT | 无界面 | — |",
    )
    assert broken != overview, "the control could not strip the reason"
    errors = validate_matrix(broken, definitions(documents), SCREEN_IDS)
    thin = [error for error in errors if "does not state a reason" in error]
    assert thin, "the 无界面-reason control passed: %s" % errors
    assert "REQ-PLAT-005" in thin[0] and OVERVIEW_PATH in thin[0], thin[0]


def test_traceability_validator_rejects_a_malformed_id_digit_count():
    documents = _valid_documents()
    documents[TYPE_PATH] += "\n" + _entry("REQ-TYPE-12", title="两位编号")
    errors = validate_requirement_ids(documents)
    malformed = [error for error in errors if "does not match the anchored pattern" in error]
    assert malformed, "the malformed-ID control passed: %s" % errors
    assert "REQ-TYPE-12" in malformed[0], malformed[0]


def test_traceability_validator_rejects_a_definition_in_the_overview():
    documents = _valid_documents()
    documents[OVERVIEW_PATH] += "\n" + _entry("REQ-PLAT-900", title="总纲里定义需求")
    errors = validate_requirement_ids(documents)
    reference_only = [error for error in errors if "must not define" in error]
    assert reference_only, "the reference-only control passed: %s" % errors
    assert OVERVIEW_PATH in reference_only[0], reference_only[0]


def test_traceability_validator_rejects_a_thin_area_document():
    documents = _valid_documents()
    documents[TYPE_PATH] = (
        "# 类型库与属性模型需求\n\n## 业务类型树\n\n" + _entry("REQ-TYPE-001")
    )
    errors = validate_area_floors(documents)
    assert any("area TYPE defines 1" in error for error in errors), errors
    assert any(TYPE_PATH in error for error in errors), errors


def test_traceability_validator_rejects_a_missing_requirement_field():
    documents = _valid_documents()
    broken = _entry("REQ-PLAT-001").replace(
        "- **数据与界面契约**：\n"
        "  | 字段 | 类型 | 必填 | 约束 |\n"
        "  | --- | --- | --- | --- |\n"
        "  | 示例字段 | String | 是 | ≤ 64 字符 |\n",
        "",
    )
    documents[PLAT_PATH] = "# 平台层需求\n\n## 身份与会话\n\n" + broken
    errors = validate_entries(documents)
    assert any("数据与界面契约" in error for error in errors), errors


def test_traceability_validator_rejects_an_unverified_source_section():
    documents = _valid_documents()
    documents[PLAT_PATH] += "\n" + _entry("REQ-PLAT-902", source="§9.9.9.9")
    errors = validate_entries(documents, allowed=whitelisted_sections())
    assert any("9.9.9.9" in error for error in errors), errors


def test_traceability_validator_rejects_vague_acceptance_criteria():
    """VAL-REQ-002 decidability can fail: a criterion's own list number is not evidence.

    The three criteria below use the three list markers the contract accepts
    (``1.`` / ``2、`` / ``3)``). Before the marker was stripped, its digit
    satisfied the "carries a number" branch and every one of them passed.
    """
    documents = _valid_documents()
    documents[PLAT_PATH] = (
        "# 平台层需求\n\n## 身份与会话\n\n" + _vague_entry("REQ-PLAT-001")
    )
    errors = validate_entries(documents)
    vague = [
        error
        for error in errors
        if "no number, named command or observable state" in error
    ]
    assert len(vague) == 3, (
        "expected all three sentiment-only criteria to be rejected, got: %s" % errors
    )
    for index in (1, 2, 3):
        matching = [error for error in vague if "criterion %d" % index in error]
        assert matching, "criterion %d was not rejected: %s" % (index, vague)
        assert "REQ-PLAT-001" in matching[0] and PLAT_PATH in matching[0], matching[0]


def test_traceability_validator_keeps_numeric_threshold_criteria_decidable():
    """Positive control: a threshold-only criterion still passes.

    Both criteria are checked to carry neither a backticked token nor an
    observable word, so only the numeric branch can accept them.
    """
    for criterion in [
        "1. 连续 5 次登录失败后锁定 30 分钟。",
        "2. 第 31 分钟起计数归零，可再次尝试。",
    ]:
        body = strip_list_number(criterion)
        assert "`" not in body, criterion
        assert not any(word in body for word in OBSERVABLE_WORDS), (
            "%s would pass on an observable word rather than its numbers" % criterion
        )
    documents = _valid_documents()
    documents[PLAT_PATH] = (
        "# 平台层需求\n\n## 身份与会话\n\n" + _threshold_entry("REQ-PLAT-001")
    )
    errors = validate_entries(documents, allowed=whitelisted_sections())
    assert errors == [], "a numeric threshold must stay decidable: %s" % errors


@pytest.mark.parametrize("path,section", SECTION_HEADINGS)
def test_traceability_section_names_appear_as_h2_in_their_area_document(path, section):
    """Each declared section must be an H2 heading of the document that owns it."""
    text = load_documents(DOCS)[path]
    present = _h2_sections(text)
    assert normalize_heading(section) in present, (
        "%s does not carry '%s' as an H2 heading; found %s"
        % (path, section, sorted(present))
    )


def test_traceability_section_heading_check_rejects_a_renamed_heading():
    """Negative control: the H2 extraction really notices a heading going away."""
    documents = load_documents(DOCS)
    renamed = documents[PLAT_PATH].replace("## 身份与会话", "## 身份与安全")
    assert renamed != documents[PLAT_PATH], "the control could not rename the heading"
    assert normalize_heading("身份与会话") in _h2_sections(documents[PLAT_PATH])
    assert normalize_heading("身份与会话") not in _h2_sections(renamed)


def test_traceability_symbol_validator_accepts_the_synthetic_document():
    """Positive control: the synthetic document satisfies every VAL-REQ-006 check."""
    errors = validate_symbol_library_coverage({SYMBOL_PATH: _symbol_document()})
    assert errors == [], "the synthetic symbol document must pass: %s" % errors


def test_traceability_symbol_validator_rejects_a_missing_connection_combination():
    """Negative control: dropping one of the nine rows must name the missing pair."""
    document = _symbol_document().replace(
        "| 信号线 | 流出点 | 离开仪表的单向信号 |\n", ""
    )
    assert "| 信号线 | 流出点 |" not in document, "the control removed nothing"
    errors = validate_symbol_library_coverage({SYMBOL_PATH: document})
    assert any("信号线/流出点" in error for error in errors), errors
    assert any("8 rows" in error for error in errors), errors


def test_traceability_symbol_validator_rejects_a_repeated_connection_combination():
    """Negative control: a duplicate pair plus a hole must both be reported."""
    document = _symbol_document().replace(
        "| 异径 | 流入点 | 异径单向流元件入口，介质只能流入 |",
        "| 等径 | 双向点 | 重复的一行 |",
    )
    errors = validate_symbol_library_coverage({SYMBOL_PATH: document})
    assert any("等径/双向点 repeats" in error for error in errors), errors
    assert any("omits the 异径/流入点 combination" in error for error in errors), errors


def test_traceability_symbol_validator_rejects_a_missing_tag_rule_field():
    """Negative control: an absent field name must be reported by name."""
    document = _symbol_document().replace("| 后缀递增 | String | 是 | — |\n", "")
    assert "后缀递增" not in document, "the control removed nothing"
    errors = validate_symbol_library_coverage({SYMBOL_PATH: document})
    assert any("does not define the field '后缀递增'" in error for error in errors), errors


def test_traceability_symbol_validator_rejects_a_tag_rule_field_synonym():
    """Negative control: VAL-DOC-009 forbids rewording the field inside its own document."""
    document = _symbol_document().replace("位号唯一校验", "位号唯一性校验")
    errors = validate_symbol_library_coverage({SYMBOL_PATH: document})
    assert any("uses the synonym" in error for error in errors), errors


def test_traceability_symbol_validator_rejects_a_renamed_section():
    """Negative control: renaming one of the thirteen sections must be detected."""
    document = _symbol_document().replace("## 绝热等级\n", "## 绝热等级设置\n")
    errors = validate_symbol_library_coverage({SYMBOL_PATH: document})
    assert any("missing section '绝热等级'" in error for error in errors), errors


def test_traceability_symbol_validator_rejects_a_missing_color_rule_element():
    """Negative control: the four condition elements are each load-bearing."""
    document = _symbol_document().replace(
        "条件要素：属性、运算符、值、连接符；当前类型无规则时继承父类型规则。",
        "条件要素：属性、运算符、值；当前类型无规则时继承父类型规则。",
    )
    errors = validate_symbol_library_coverage({SYMBOL_PATH: document})
    assert any("condition element '连接符'" in error for error in errors), errors


def test_traceability_symbol_validator_rejects_a_missing_document():
    """Negative control: an absent area document is reported, not silently skipped."""
    errors = validate_symbol_library_coverage({})
    assert errors == ["%s: symbol library requirement document is missing" % SYMBOL_PATH]


# ---------------------------------------------------------------------------
# Drawing document (VAL-REQ-007) controls
# ---------------------------------------------------------------------------


def test_traceability_drawing_validator_accepts_the_synthetic_document():
    """Positive control: the synthetic document satisfies every VAL-REQ-007 check."""
    errors = validate_drawing_coverage({DRAWING_PATH: _drawing_document()})
    assert errors == [], "the synthetic drawing document must pass: %s" % errors


def test_traceability_drawing_validator_rejects_a_missing_break_and_fill_semantics():
    """Negative control: VAL-REQ-007's auto-break/auto-fill pair is load-bearing."""
    document = _drawing_document().replace(
        "插入时管线自动断开，移动时自动补位。",
        "插入时可选择断开管线。",
    )
    assert "自动补位" not in document, "the control removed nothing"
    errors = validate_drawing_coverage({DRAWING_PATH: document})
    assert any(
        "管件与阀门插入 does not state '自动补位'" in error for error in errors
    ), errors


def test_traceability_drawing_validator_rejects_a_missing_connector_kind():
    """Negative control: dropping 信号线连接符流出 must name that kind."""
    document = _drawing_document().replace("信号线连接符流出。", "。")
    assert "信号线连接符流出" not in document, "the control removed nothing"
    errors = validate_drawing_coverage({DRAWING_PATH: document})
    assert any(
        "页间连接符 does not state '信号线连接符流出'" in error for error in errors
    ), errors


def test_traceability_drawing_validator_rejects_a_renamed_section():
    """Negative control: renaming one of the thirteen sections must be detected."""
    document = _drawing_document().replace("## 标注\n", "## 对象标注\n")
    errors = validate_drawing_coverage({DRAWING_PATH: document})
    assert any("missing section '标注'" in error for error in errors), errors


def test_traceability_drawing_validator_rejects_a_section_without_requirements():
    """Negative control: a present-but-empty section is not coverage."""
    parts = _drawing_document().split("\n## ")
    trimmed = []
    removed = False
    for part in parts:
        if part.startswith("打断符"):
            head, separator, _rest = part.partition("### REQ-DRW-")
            assert separator, "打断符 carries no requirement entry to remove"
            trimmed.append(head.rstrip() + "\n\n")
            removed = True
        else:
            trimmed.append(part)
    document = "\n## ".join(trimmed)
    assert removed, "the control did not touch the 打断符 section"
    empty_section = document.split("\n## 打断符")[1].split("\n## ")[0]
    assert "### REQ-DRW-" not in empty_section, "the entry survived the removal"
    errors = validate_drawing_coverage({DRAWING_PATH: document})
    assert any(
        "section '打断符' has 0 requirements" in error for error in errors
    ), errors


def test_traceability_drawing_validator_rejects_a_missing_document():
    """Negative control: an absent area document is reported, not silently skipped."""
    errors = validate_drawing_coverage({})
    assert errors == ["%s: drawing requirement document is missing" % DRAWING_PATH]
