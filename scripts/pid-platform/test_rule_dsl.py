"""The ``-k traceability`` arm of VAL-REQ-008: the rule DSL really parses.

``rule_dsl`` is a recursive-descent parser for the grammar published in
``requirements/50-data-and-rule-engine.md``. These tests prove the parser
accepts every documented shape and rejects malformed input with an offset, so
"each rule expression is parseable" is a measured property rather than a
reading of the table. Names start with ``test_traceability_`` so the contract's
declared tool (``pytest -k traceability``) selects them.
"""
from __future__ import annotations

import re

import pytest

import rule_dsl
from rule_dsl import DSLParseError, grammar_tokens, is_valid, parse

VALID_EXPRESSIONS = [
    ("null_comparison", "管道等级 = NULL"),
    ("empty_string", '位号 = NULL OR 位号 = ""'),
    ("nested_path", "位号.同类型重复数 > 1"),
    ("cross_object_path", "压力等级 > 所属管线.管道等级.最大公称压力"),
    ("less_or_equal", "长度 <= 0"),
    ("arithmetic_or", "开度 > 100 OR 开度 < 0"),
    ("not_match_pattern", '位号 NOT MATCH "^[A-Z]{1,2}-[0-9]{4}$"'),
    ("match_pattern", '位号 MATCH "^P-[0-9]{4}$"'),
    ("in_enum_list", "介质 IN (#氯气, #氢氟酸) AND 管道等级.材质 = #碳钢"),
    ("not_in_property", "管道等级 NOT IN (项目.管道等级表)"),
    ("parenthesised_not", "管口数 < 2 AND NOT (设备类别 = #储罐)"),
    ("grouping_precedence", "(设计温度 > 200 OR 介质 = #蒸汽) AND 绝热等级 = NULL"),
    ("not_equal", "符号.状态 != #有效"),
    ("standalone_not", "NOT 位号 = NULL"),
    ("boolean_literal", "绝热等级 = TRUE"),
    ("negative_number", "开度 >= -1"),
    ("decimal_number", "长度 > 2.5"),
    ("string_literal", '文字 = "工艺水"'),
]

INVALID_EXPRESSIONS = [
    ("empty", ""),
    ("missing_operand", "管道等级 = "),
    ("unclosed_paren", "(管道等级 = NULL"),
    ("stray_close_paren", "管道等级 = NULL)"),
    ("dangling_and", "管道等级 = NULL AND"),
    ("dangling_or", "位号 = NULL OR"),
    ("double_equals", "管道等级 == NULL"),
    ("lowercase_and", "管道等级 = NULL and 介质 = #蒸汽"),
    ("missing_left_path", "= NULL"),
    ("value_on_the_left", "NULL = 管道等级"),
    ("not_as_operator", "位号 NOT = NULL"),
    ("not_in_without_path", "NOT IN (项目.管道等级表)"),
    ("empty_segment", "位号. > 1"),
    ("operator_without_operand", "位号 MATCH"),
    ("two_comparisons", "位号 = NULL 介质 = #蒸汽"),
    ("illegal_character", "位号 = @"),
    ("unbalanced_group_tail", "位号 = NULL AND (介质 = #蒸汽"),
]


@pytest.mark.parametrize("label,expression", VALID_EXPRESSIONS)
def test_traceability_rule_dsl_accepts_documented_shapes(label, expression):
    """Every construct the EBNF block names must derive from ``expression``."""
    assert parse(expression) == expression, label
    assert is_valid(expression), label


@pytest.mark.parametrize("label,expression", INVALID_EXPRESSIONS)
def test_traceability_rule_dsl_rejects_malformed_expressions(label, expression):
    """Malformed input must raise, not be silently tolerated."""
    with pytest.raises(DSLParseError) as caught:
        parse(expression)
    assert caught.value.summary(), label
    assert "at offset" in caught.value.summary(), (label, str(caught.value))


def test_traceability_rule_dsl_reports_the_offending_offset():
    """The parser must point at the position, not just say 'invalid'."""
    with pytest.raises(DSLParseError) as caught:
        parse("管道等级 = NULL AND")
    assert caught.value.offset == len("管道等级 = NULL AND"), caught.value.summary()
    assert caught.value.source == "管道等级 = NULL AND"


def test_traceability_rule_dsl_reports_the_first_unexpected_character():
    with pytest.raises(DSLParseError) as caught:
        parse("位号 = @")
    assert caught.value.offset == len("位号 = "), caught.value.summary()
    assert "unexpected character" in caught.value.summary()


def test_traceability_rule_dsl_grammar_tokens_cover_the_published_terminals():
    """The document checks terminals from ``grammar_tokens``; keep them complete."""
    tokens = grammar_tokens()
    for operator in ["=", "!=", ">", "<", ">=", "<=", "IN", "NOT IN", "MATCH", "NOT MATCH"]:
        assert operator in tokens["operators"], operator
    assert set(tokens["logical"]) == {"AND", "OR", "NOT"}
    for literal in ["string", "number", "enum_ref", "boolean", "NULL"]:
        assert literal in tokens["literals"], literal
    for production in ["expression", "or_expr", "and_expr", "not_expr", "comparison"]:
        assert production in tokens["productions"], production


def test_traceability_rule_dsl_published_grammar_defines_every_production():
    """The constant the positive control builds from is itself complete."""
    body = rule_dsl.EBNF_GRAMMAR
    for production in rule_dsl.PRODUCTIONS:
        assert re.search(
            r"^\s*%s\s*=" % re.escape(production), body, re.MULTILINE
        ), "%s has no production line" % production
    for terminal in ['"AND"', '"OR"', '"NOT"', '"("', '")"', '"NULL"']:
        assert terminal in body, terminal
