"""Pure validators for the three architecture deliverables.

Covers VAL-ARCH-001 (five layers with fields, cross-layer contract table,
offline statement, three real reuse anchors), VAL-ARCH-002 (three object
tables, five transfer chains, symbol-standard uniqueness) and VAL-ARCH-003
(six non-functional sections, numeric targets marked as design goals,
conditional localisation claims).

Every function takes document text and returns a list of human-readable
errors; an empty list means the document satisfies the contract. The tests
feed both the real files and mutated synthetic documents, so a validator that
always returns ``[]`` cannot pass.
"""
from __future__ import annotations

import re
from pathlib import Path

from docs_contract import find_table, section_body, sections_at

# --- VAL-ARCH-001 -----------------------------------------------------------

LAYERS = [
    "L1 绘图内核",
    "L2 P&ID 语义",
    "L3 数据服务",
    "L4 校验交付",
    "L5 平台",
]

LAYER_FIELDS = ["职责", "上游消费者", "向下依赖"]

# Renamed layers are the failure mode this list exists to catch: the PRD's
# 所属层 column and gap-analysis.md already use the five names above.
FORBIDDEN_LAYER_VARIANTS = ["校验与交付", "语义层"]

# Each path is quoted literally in the document and must exist in the repo.
ANCHOR_PATHS = [
    "src/io/pid_legend/",
    "src/io/pid_pipes.rs",
    "src/mcp.rs",
]

CONTRACT_HEADER = ["调用方", "被调用方", "交互内容", "契约载体", "失败与降级"]
ANCHOR_HEADER = ["现有能力", "仓库路径", "所属层", "在本架构中的角色", "处置"]

OFFLINE_STATEMENT = "L2 可在无 L3 时独立工作"

MIN_FIELD_CHARS = 20
MIN_CONTRACT_ROWS = 8
MIN_ANCHOR_ROWS = 12

# --- VAL-ARCH-002 -----------------------------------------------------------

DATA_MODEL_COLUMNS = ["对象", "中文", "关键属性", "关系", "唯一性或命名规则"]

OBJECT_TABLES = {
    "P&ID 专业对象": 14,
    "配置对象": 12,
    "容器对象": 4,
}

REQUIRED_OBJECTS = {
    "P&ID 专业对象": [
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
        "位号",
        "标注",
    ],
    "配置对象": [
        "类型",
        "属性分组",
        "数据要素",
        "图例标准",
        "符号",
        "连接点",
        "位号规则",
        "颜色规则",
        "公称直径",
        "管道等级",
        "绝热等级",
        "流体介质",
    ],
    "容器对象": ["项目空间", "PBS 节点", "图纸", "图纸版本快照"],
}

CHAIN_HEADER = ["传递链", "传递内容", "生效时机", "冲突与例外"]

TRANSFER_CHAINS = [
    "管线 → 管路 → 管段",
    "设备 → 管口（位号前缀继承）",
    "关联设备双向共享",
    "类型 → 子类型规则继承",
    "页间连接属性传递",
]

SYMBOL_STANDARD_MARKERS = ["不可切换", "一套", "SYMBOL_STANDARD_LOCKED"]

# --- VAL-ARCH-003 -----------------------------------------------------------

NON_FUNCTIONAL_SECTIONS = [
    "性能",
    "容量",
    "兼容性",
    "信创适配",
    "可用性",
    "可观测性",
]

DESIGN_TARGET_MARK = "设计目标（未实测）"

NON_FUNCTIONAL_TABLES = {
    "兼容性": (["兼容项", "支持范围", "判定条件"], 6),
    "可用性": (["指标", "目标值", "判定条件"], 5),
    "可观测性": (["观测信号", "采集点", "判定条件"], 5),
}

MIN_PERFORMANCE_ITEMS = 8
MIN_CAPACITY_ITEMS = 10
MIN_LOCALISATION_ITEMS = 5


# ---------------------------------------------------------------------------
# helpers
# ---------------------------------------------------------------------------

def _squash(text: str) -> str:
    """Drop every whitespace run so arrow spacing cannot fail a comparison."""
    return re.sub(r"\s+", "", text)


def _cell_matches(cell: str, name: str) -> bool:
    """Accept ``name`` alone or ``name（...）`` with a parenthetical note."""
    cell = cell.strip()
    return cell == name or cell.startswith(name + "（") or cell.startswith(name + "(")


def _bullet_items(body: str) -> list:
    """Top-level ``- `` bullets of a section body, continuation lines ignored."""
    return [
        line.strip()[2:].strip()
        for line in body.splitlines()
        if line.startswith("- ") and len(line.strip()) > 2
    ]


def _field_text(body: str, field: str) -> str:
    """Text following a bold field label, or "" when the label is absent."""
    match = re.search(
        r"\*\*" + re.escape(field) + r"\*\*\s*[：:]?\s*([^\n]+)", body
    )
    return match.group(1).strip() if match else ""


def _check_table(body: str, header: list, minimum: int, where: str) -> list:
    errors = []
    table = find_table(body, header)
    if table is None:
        errors.append("%s: missing table with header %s" % (where, " / ".join(header)))
        return errors
    if len(table["rows"]) < minimum:
        errors.append(
            "%s: table has %d data rows, needs at least %d"
            % (where, len(table["rows"]), minimum)
        )
    for index, row in enumerate(table["rows"], start=1):
        if len(row) != len(header):
            errors.append(
                "%s: row %d has %d cells, header has %d"
                % (where, index, len(row), len(header))
            )
            continue
        blank = [header[i] for i, cell in enumerate(row) if not cell.strip()]
        if blank:
            errors.append("%s: row %d has empty cells %s" % (where, index, blank))
    return errors


# ---------------------------------------------------------------------------
# VAL-ARCH-001: system-architecture.md
# ---------------------------------------------------------------------------

def validate_system_architecture(text: str, repo_root=None) -> list:
    errors = []

    for variant in FORBIDDEN_LAYER_VARIANTS:
        if variant in text:
            errors.append(
                "renamed layer variant %r appears; the five layer names are fixed"
                % variant
            )

    layer_bodies = dict(sections_at(text, 3))
    for layer in LAYERS:
        body = layer_bodies.get(layer)
        if body is None:
            errors.append("missing layer section: %s" % layer)
            continue
        for field in LAYER_FIELDS:
            value = _field_text(body, field)
            if not value:
                errors.append("%s: missing field %s" % (layer, field))
            elif len(value) < MIN_FIELD_CHARS:
                errors.append(
                    "%s: field %s has %d characters, needs at least %d"
                    % (layer, field, len(value), MIN_FIELD_CHARS)
                )

    contract_body = section_body(text, 2, "跨层交互契约")
    if contract_body is None:
        errors.append("missing section: 跨层交互契约")
    else:
        errors.extend(
            _check_table(
                contract_body, CONTRACT_HEADER, MIN_CONTRACT_ROWS, "跨层交互契约"
            )
        )

    offline_body = section_body(text, 2, "脱机与在线运行模式")
    if offline_body is None:
        errors.append("missing section: 脱机与在线运行模式")
    else:
        if OFFLINE_STATEMENT not in offline_body:
            errors.append(
                "offline section does not state %r" % OFFLINE_STATEMENT
            )
        if "本地脱机模式" not in offline_body:
            errors.append("offline section does not name 本地脱机模式")

    anchor_body = section_body(text, 2, "复用锚点")
    if anchor_body is None:
        errors.append("missing section: 复用锚点")
    else:
        errors.extend(
            _check_table(anchor_body, ANCHOR_HEADER, MIN_ANCHOR_ROWS, "复用锚点")
        )
        for path in ANCHOR_PATHS:
            if path not in anchor_body:
                errors.append("reuse anchor %s is not cited in 复用锚点" % path)
            elif repo_root is not None:
                target = Path(repo_root) / path.rstrip("/")
                if not target.exists():
                    errors.append(
                        "reuse anchor %s does not exist under %s"
                        % (path, repo_root)
                    )
        table = find_table(anchor_body, ANCHOR_HEADER)
        if table is not None:
            allowed = set(LAYERS) | {"扩展点"}
            for index, row in enumerate(table["rows"], start=1):
                if len(row) != len(ANCHOR_HEADER):
                    continue
                layer_cell = row[2].strip()
                if layer_cell not in allowed:
                    errors.append(
                        "复用锚点 row %d names an unknown layer %r"
                        % (index, layer_cell)
                    )
                disposition = row[4].strip()
                if disposition not in {"复用", "改造", "新增"}:
                    errors.append(
                        "复用锚点 row %d has disposition %r outside {复用, 改造, 新增}"
                        % (index, disposition)
                    )
    return errors


# ---------------------------------------------------------------------------
# VAL-ARCH-002: data-model.md
# ---------------------------------------------------------------------------

def validate_data_model(text: str) -> list:
    errors = []

    for heading, minimum in OBJECT_TABLES.items():
        body = section_body(text, 3, heading)
        if body is None:
            errors.append("missing object section: %s" % heading)
            continue
        errors.extend(_check_table(body, DATA_MODEL_COLUMNS, minimum, heading))
        table = find_table(body, DATA_MODEL_COLUMNS)
        if table is None:
            continue
        names = [row[1] for row in table["rows"] if len(row) > 1]
        for required in REQUIRED_OBJECTS[heading]:
            if not any(_cell_matches(cell, required) for cell in names):
                errors.append("%s: missing object %s" % (heading, required))

    chain_body = section_body(text, 2, "属性传递链")
    if chain_body is None:
        errors.append("missing section: 属性传递链")
    else:
        table = find_table(chain_body, CHAIN_HEADER)
        if table is None:
            errors.append("属性传递链: missing table with header 传递链 / 传递内容 / 生效时机 / 冲突与例外")
        else:
            if len(table["rows"]) != len(TRANSFER_CHAINS):
                errors.append(
                    "属性传递链: %d rows, expected exactly %d"
                    % (len(table["rows"]), len(TRANSFER_CHAINS))
                )
            written = [_squash(row[0]) for row in table["rows"] if row]
            for chain in TRANSFER_CHAINS:
                if _squash(chain) not in written:
                    errors.append("属性传递链: missing chain %s" % chain)
            for index, row in enumerate(table["rows"], start=1):
                if len(row) != len(CHAIN_HEADER):
                    errors.append(
                        "属性传递链: row %d has %d cells, header has %d"
                        % (index, len(row), len(CHAIN_HEADER))
                    )
                elif any(not cell.strip() for cell in row[1:]):
                    errors.append("属性传递链: row %d has an empty cell" % index)

    unique_body = section_body(text, 2, "图例标准唯一性")
    if unique_body is None:
        errors.append("missing section: 图例标准唯一性")
    else:
        for marker in SYMBOL_STANDARD_MARKERS:
            if marker not in unique_body:
                errors.append(
                    "图例标准唯一性: does not state %r" % marker
                )
    return errors


# ---------------------------------------------------------------------------
# VAL-ARCH-003: non-functional.md
# ---------------------------------------------------------------------------

def _design_goal_items(body: str, where: str, minimum: int) -> list:
    errors = []
    items = _bullet_items(body)
    if len(items) < minimum:
        errors.append(
            "%s: %d metric items, needs at least %d" % (where, len(items), minimum)
        )
    for index, item in enumerate(items, start=1):
        if "：" not in item and ":" not in item:
            errors.append("%s: item %d is not of the form 指标：数值" % (where, index))
        if DESIGN_TARGET_MARK not in item:
            errors.append(
                "%s: item %d is not marked %s" % (where, index, DESIGN_TARGET_MARK)
            )
        if not re.search(r"\d", item):
            errors.append("%s: item %d carries no number" % (where, index))
    return errors


def validate_non_functional(text: str) -> list:
    errors = []

    found = [name for name, _ in sections_at(text, 2)]
    if found != NON_FUNCTIONAL_SECTIONS:
        errors.append(
            "H2 sections are %s, expected exactly %s"
            % (found, NON_FUNCTIONAL_SECTIONS)
        )

    performance = section_body(text, 2, "性能")
    if performance is None:
        errors.append("missing section: 性能")
    else:
        errors.extend(
            _design_goal_items(performance, "性能", MIN_PERFORMANCE_ITEMS)
        )

    capacity = section_body(text, 2, "容量")
    if capacity is None:
        errors.append("missing section: 容量")
    else:
        errors.extend(
            _design_goal_items(capacity, "容量", MIN_CAPACITY_ITEMS)
        )

    for heading, (header, minimum) in NON_FUNCTIONAL_TABLES.items():
        body = section_body(text, 2, heading)
        if body is None:
            errors.append("missing section: %s" % heading)
            continue
        errors.extend(_check_table(body, header, minimum, heading))

    localisation = section_body(text, 2, "信创适配")
    if localisation is None:
        errors.append("missing section: 信创适配")
    else:
        items = _bullet_items(localisation)
        if len(items) < MIN_LOCALISATION_ITEMS:
            errors.append(
                "信创适配: %d conditional items, needs at least %d"
                % (len(items), MIN_LOCALISATION_ITEMS)
            )
        for index, item in enumerate(items, start=1):
            if "若" not in item or "则" not in item:
                errors.append(
                    "信创适配: item %d is not a 若…则… conditional" % index
                )
    return errors
