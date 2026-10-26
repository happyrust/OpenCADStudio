"""Contract tests for the three architecture deliverables.

Covers VAL-ARCH-001 (``system-architecture.md``), VAL-ARCH-002
(``data-model.md``) and VAL-ARCH-003 (``non-functional.md``). The negative
controls mutate a synthetic but valid document for each validator, so a
validator that always returns ``[]`` cannot pass this file.
"""
from __future__ import annotations

import pytest

from conftest import DOCS, REPO_ROOT
from architecture_contract import (
    ANCHOR_HEADER,
    ANCHOR_PATHS,
    CHAIN_HEADER,
    CONTRACT_HEADER,
    DATA_MODEL_COLUMNS,
    DESIGN_TARGET_MARK,
    LAYERS,
    LAYER_FIELDS,
    MIN_CAPACITY_ITEMS,
    MIN_CONTRACT_ROWS,
    MIN_LOCALISATION_ITEMS,
    MIN_PERFORMANCE_ITEMS,
    NON_FUNCTIONAL_SECTIONS,
    OBJECT_TABLES,
    OFFLINE_STATEMENT,
    REQUIRED_OBJECTS,
    TRANSFER_CHAINS,
    validate_data_model,
    validate_non_functional,
    validate_system_architecture,
)
from lint_rules import banned_phrase_errors

ARCH = DOCS / "architecture"
SYSTEM = ARCH / "system-architecture.md"
DATA_MODEL = ARCH / "data-model.md"
NON_FUNCTIONAL = ARCH / "non-functional.md"


def _read(path) -> str:
    assert path.exists(), "%s missing at %s" % (path.name, path)
    text = path.read_text(encoding="utf-8")
    assert len(text) > 2000, (
        "%s is %d characters; too thin for an architecture document"
        % (path.name, len(text))
    )
    return text


# ---------------------------------------------------------------------------
# Real documents
# ---------------------------------------------------------------------------

@pytest.fixture(scope="module")
def system_text() -> str:
    return _read(SYSTEM)


@pytest.fixture(scope="module")
def data_model_text() -> str:
    return _read(DATA_MODEL)


@pytest.fixture(scope="module")
def non_functional_text() -> str:
    return _read(NON_FUNCTIONAL)


def test_structure_system_architecture_satisfies_the_contract(system_text):
    errors = validate_system_architecture(system_text, REPO_ROOT)
    assert errors == [], "system architecture contract violations:\n" + "\n".join(
        errors
    )


def test_structure_system_architecture_three_anchor_paths_exist_on_disk():
    """The three reuse anchors must be real repository paths, not prose."""
    from docs_contract import section_body

    text = _read(SYSTEM)
    body = section_body(text, 2, "复用锚点")
    assert body is not None, "复用锚点 section is missing"
    for path in ANCHOR_PATHS:
        assert path in body, "reuse anchor not cited: " + path
        assert (REPO_ROOT / path.rstrip("/")).exists(), (
            "cited reuse anchor does not exist in the repository: " + path
        )


def test_structure_system_architecture_layer_names_are_the_baseline_names(
    system_text,
):
    from docs_contract import sections_at

    found = [name for name, _ in sections_at(system_text, 3)]
    for layer in LAYERS:
        assert layer in found, "layer section missing: " + layer
    for variant in ("校验与交付", "语义层"):
        assert variant not in system_text, (
            "system architecture uses a renamed layer variant: " + variant
        )


def test_structure_data_model_satisfies_the_contract(data_model_text):
    errors = validate_data_model(data_model_text)
    assert errors == [], "data model contract violations:\n" + "\n".join(errors)


def test_structure_data_model_object_counts_meet_the_floors(data_model_text):
    from docs_contract import find_table, section_body

    counted = {}
    for heading in OBJECT_TABLES:
        body = section_body(data_model_text, 3, heading)
        assert body is not None, "object section missing: " + heading
        table = find_table(body, DATA_MODEL_COLUMNS)
        assert table is not None, "object table missing in " + heading
        counted[heading] = len(table["rows"])
    assert counted["P&ID 专业对象"] >= 14, counted
    assert counted["配置对象"] >= 12, counted
    assert counted["容器对象"] >= 4, counted


def test_structure_data_model_transfer_chains_are_all_present(data_model_text):
    from docs_contract import find_table, section_body

    body = section_body(data_model_text, 2, "属性传递链")
    assert body is not None, "属性传递链 section is missing"
    table = find_table(body, CHAIN_HEADER)
    assert table is not None, "属性传递链 table is missing"
    written = {"".join(row[0].split()) for row in table["rows"]}
    for chain in TRANSFER_CHAINS:
        assert "".join(chain.split()) in written, "missing transfer chain: " + chain
    assert len(table["rows"]) == 5, (
        "expected exactly five transfer chains, found %d" % len(table["rows"])
    )


def test_structure_data_model_symbol_standard_cannot_be_switched(data_model_text):
    from docs_contract import section_body

    body = section_body(data_model_text, 2, "图例标准唯一性")
    assert body is not None, "图例标准唯一性 section is missing"
    assert "不可切换" in body, "the no-switch constraint is not stated"
    assert "SYMBOL_STANDARD_LOCKED" in body, "the rejection error code is not stated"


def test_lint_non_functional_satisfies_the_contract(non_functional_text):
    errors = validate_non_functional(non_functional_text)
    assert errors == [], "non-functional contract violations:\n" + "\n".join(errors)


def test_lint_non_functional_sections_are_exactly_the_six(non_functional_text):
    from docs_contract import sections_at

    found = [name for name, _ in sections_at(non_functional_text, 2)]
    assert found == NON_FUNCTIONAL_SECTIONS, (
        "H2 sections are %s, expected %s" % (found, NON_FUNCTIONAL_SECTIONS)
    )


def test_lint_non_functional_targets_are_marked_as_design_goals(non_functional_text):
    from docs_contract import section_body

    for heading, minimum in (
        ("性能", MIN_PERFORMANCE_ITEMS),
        ("容量", MIN_CAPACITY_ITEMS),
    ):
        body = section_body(non_functional_text, 2, heading)
        assert body is not None, "section missing: " + heading
        items = [
            line.strip()[2:].strip()
            for line in body.splitlines()
            if line.startswith("- ") and len(line.strip()) > 2
        ]
        assert len(items) >= minimum, (
            "%s has %d items, needs at least %d" % (heading, len(items), minimum)
        )
        marked = [item for item in items if DESIGN_TARGET_MARK in item]
        assert len(marked) == len(items), (
            "%s: %d of %d items carry %s"
            % (heading, len(marked), len(items), DESIGN_TARGET_MARK)
        )


def test_lint_non_functional_localisation_is_conditional(non_functional_text):
    from docs_contract import section_body

    body = section_body(non_functional_text, 2, "信创适配")
    assert body is not None, "信创适配 section is missing"
    items = [
        line.strip()[2:].strip()
        for line in body.splitlines()
        if line.startswith("- ") and len(line.strip()) > 2
    ]
    assert len(items) >= MIN_LOCALISATION_ITEMS, (
        "信创适配 has %d items, needs at least %d"
        % (len(items), MIN_LOCALISATION_ITEMS)
    )
    unconditional = [item for item in items if "若" not in item or "则" not in item]
    assert unconditional == [], (
        "these localisation items are not 若…则… conditionals: %s" % unconditional
    )


def test_lint_non_functional_has_no_banned_vague_phrases(non_functional_text):
    errors = banned_phrase_errors(non_functional_text, str(NON_FUNCTIONAL))
    assert errors == [], "\n".join(errors)


# ---------------------------------------------------------------------------
# Synthetic documents for the negative controls
# ---------------------------------------------------------------------------

def _valid_system_doc() -> str:
    parts = ["# 系统架构", "", "## 分层架构", ""]
    for layer in LAYERS:
        parts += ["### " + layer, ""]
        for field in LAYER_FIELDS:
            parts.append(
                "- **%s**：本层承担固定职责并只向下依赖一层，任何跨层调用经中间层代理。"
                % field
            )
        parts.append("")
    parts += [
        "## 跨层交互契约",
        "",
        "| " + " | ".join(CONTRACT_HEADER) + " |",
        "| " + " | ".join(["---"] * len(CONTRACT_HEADER)) + " |",
    ]
    for index in range(MIN_CONTRACT_ROWS):
        parts.append(
            "| 调用方%d | 被调用方%d | 交互内容 | 契约载体 | 失败时降级为只读 |"
            % (index, index)
        )
    parts += ["", "## 脱机与在线运行模式", "", OFFLINE_STATEMENT + "，即本地脱机模式。", ""]
    parts += [
        "## 复用锚点",
        "",
        "| " + " | ".join(ANCHOR_HEADER) + " |",
        "| " + " | ".join(["---"] * len(ANCHOR_HEADER)) + " |",
    ]
    for index in range(12):
        path = ANCHOR_PATHS[index % len(ANCHOR_PATHS)]
        parts.append(
            "| 能力%d | `%s` | %s | 角色 | 复用 |" % (index, path, LAYERS[0])
        )
    return "\n".join(parts) + "\n"


def _object_table(heading: str, names: list, extra_rows: int) -> list:
    rows = [
        "| Object%d | %s | 属性 | 关系 | 名称唯一 |" % (index, name)
        for index, name in enumerate(names)
    ]
    for index in range(extra_rows):
        rows.append("| Extra%d | 补充对象%d | 属性 | 关系 | 名称唯一 |" % (index, index))
    return [
        "### " + heading,
        "",
        "| " + " | ".join(DATA_MODEL_COLUMNS) + " |",
        "| " + " | ".join(["---"] * len(DATA_MODEL_COLUMNS)) + " |",
    ] + rows + [""]


def _valid_data_model_doc() -> str:
    parts = ["# 领域数据模型", "", "## 领域对象分类", ""]
    for heading, floor in OBJECT_TABLES.items():
        names = REQUIRED_OBJECTS[heading]
        extra = floor - len(names)
        parts += _object_table(heading, names, extra)
    parts += ["## 对象关系总览", "", "项目空间 → PBS 节点 → 图纸。", ""]
    parts += [
        "## 属性传递链",
        "",
        "| " + " | ".join(CHAIN_HEADER) + " |",
        "| " + " | ".join(["---"] * len(CHAIN_HEADER)) + " |",
    ]
    for chain in TRANSFER_CHAINS:
        parts.append("| %s | 内容 | 时机 | 例外 |" % chain)
    parts += [
        "",
        "## 图例标准唯一性",
        "",
        "一个项目空间选定一套图例标准，产生数据实例后不可切换。",
        "拒绝时返回错误码 SYMBOL_STANDARD_LOCKED。",
    ]
    return "\n".join(parts) + "\n"


def _valid_non_functional_doc() -> str:
    parts = ["# 非功能需求", "", "## 性能", ""]
    for index in range(MIN_PERFORMANCE_ITEMS):
        parts.append("- 性能指标%d：≤ %d 秒。%s。" % (index, index + 1, DESIGN_TARGET_MARK))
    parts += ["", "## 容量", ""]
    for index in range(MIN_CAPACITY_ITEMS):
        parts.append("- 容量指标%d：≤ %d 个。%s。" % (index, index + 10, DESIGN_TARGET_MARK))
    parts += [
        "",
        "## 兼容性",
        "",
        "| 兼容项 | 支持范围 | 判定条件 |",
        "| --- | --- | --- |",
    ]
    for index in range(6):
        parts.append("| 项%d | 范围%d | 丢失数为 0 |" % (index, index))
    parts += ["", "## 信创适配", ""]
    for index in range(MIN_LOCALISATION_ITEMS):
        parts.append("- 若条件%d成立，则须通过 1 条验收用例。" % index)
    parts += [
        "",
        "## 可用性",
        "",
        "| 指标 | 目标值 | 判定条件 |",
        "| --- | --- | --- |",
    ]
    for index in range(5):
        parts.append("| 指标%d | ≥ 99%% | 失败数为 0 |" % index)
    parts += [
        "",
        "## 可观测性",
        "",
        "| 观测信号 | 采集点 | 判定条件 |",
        "| --- | --- | --- |",
    ]
    for index in range(5):
        parts.append("| 信号%d | 采集点%d | 可查询到最近 1 次 |" % (index, index))
    return "\n".join(parts) + "\n"


# ---------------------------------------------------------------------------
# Negative controls: the validators must reject mutated documents
# ---------------------------------------------------------------------------

def test_structure_system_validator_accepts_the_synthetic_baseline():
    assert validate_system_architecture(_valid_system_doc()) == []


def test_structure_system_validator_rejects_a_renamed_layer():
    text = _valid_system_doc().replace("### L4 校验交付", "### L4 校验与交付")
    assert "### L4 校验交付" not in text
    errors = validate_system_architecture(text)
    assert any("missing layer section" in error for error in errors), errors


def test_structure_system_validator_rejects_a_missing_field():
    text = _valid_system_doc().replace("- **向下依赖**：", "- ", 1)
    errors = validate_system_architecture(text)
    assert any("missing field 向下依赖" in error for error in errors), errors


def test_structure_system_validator_rejects_a_missing_anchor_path():
    text = _valid_system_doc().replace("src/mcp.rs", "src/unknown.rs")
    errors = validate_system_architecture(text)
    assert any("src/mcp.rs" in error for error in errors), errors


def test_structure_system_validator_rejects_a_missing_anchor_on_disk(tmp_path):
    text = _valid_system_doc()
    errors = validate_system_architecture(text, tmp_path)
    assert any("does not exist" in error for error in errors), errors


def test_structure_system_validator_rejects_a_thin_contract_table():
    lines = [
        line
        for line in _valid_system_doc().splitlines()
        if not line.startswith("| 调用方")
    ]
    text = "\n".join(lines).replace(
        "| " + " | ".join(["---"] * len(CONTRACT_HEADER)) + " |", ""
    )
    errors = validate_system_architecture(text)
    assert any("跨层交互契约" in error for error in errors), errors


def test_structure_data_model_validator_accepts_the_synthetic_baseline():
    assert validate_data_model(_valid_data_model_doc()) == []


def test_structure_data_model_validator_rejects_a_thin_object_table():
    text = _valid_data_model_doc().replace(
        "| Object0 | 管线 | 属性 | 关系 | 名称唯一 |\n", ""
    )
    text = text.replace("| Extra0 | 补充对象0 | 属性 | 关系 | 名称唯一 |\n", "")
    text = text.replace("| Extra1 | 补充对象1 | 属性 | 关系 | 名称唯一 |\n", "")
    errors = validate_data_model(text)
    assert any("needs at least 14" in error for error in errors) or any(
        "missing object 管线" in error for error in errors
    ), errors


def test_structure_data_model_validator_rejects_a_missing_object():
    text = _valid_data_model_doc().replace("| Object0 | 管线 |", "| Object0 | 管道线 |")
    errors = validate_data_model(text)
    assert any("missing object 管线" in error for error in errors), errors


def test_structure_data_model_validator_rejects_a_missing_chain():
    text = _valid_data_model_doc().replace("关联设备双向共享", "关联设备单向共享")
    errors = validate_data_model(text)
    assert any("missing chain" in error for error in errors), errors


def test_structure_data_model_validator_rejects_a_switchable_standard():
    text = _valid_data_model_doc().replace("不可切换", "可切换")
    errors = validate_data_model(text)
    assert any("不可切换" in error for error in errors), errors


def test_lint_non_functional_validator_accepts_the_synthetic_baseline():
    assert validate_non_functional(_valid_non_functional_doc()) == []


def test_lint_non_functional_validator_rejects_an_unmarked_target():
    lines = _valid_non_functional_doc().splitlines()
    index = next(i for i, line in enumerate(lines) if line.startswith("- 性能指标0"))
    lines[index] = "- 性能指标0：≤ 1 秒。"
    text = "\n".join(lines) + "\n"
    errors = validate_non_functional(text)
    assert any(DESIGN_TARGET_MARK in error for error in errors), errors


def test_lint_non_functional_validator_rejects_a_missing_section():
    lines = [
        line
        for line in _valid_non_functional_doc().splitlines()
        if line.strip() != "## 可观测性"
    ]
    text = "\n".join(lines) + "\n"
    errors = validate_non_functional(text)
    assert any("可观测性" in error for error in errors), errors


def test_lint_non_functional_validator_rejects_an_unconditional_claim():
    text = _valid_non_functional_doc().replace(
        "若条件0成立，则须通过 1 条验收用例。", "本平台已通过国产化验收。"
    )
    errors = validate_non_functional(text)
    assert any("conditional" in error for error in errors), errors
