"""Recursive-descent parser for the design-rule condition DSL (VAL-REQ-008).

The grammar is documented as an EBNF block in
``docs/pid-platform/requirements/50-data-and-rule-engine.md``. This module is the
executable copy of that grammar: every rule expression shipped in the 设计规则清单
table is fed through :func:`parse`, so "the expression can be parsed" is a test
result rather than a claim. The EBNF and the parser must be kept in step by hand;
:func:`grammar_tokens` exposes the terminals the document is checked against.

Grammar (same production names as the document)::

    expression = or_expr ;
    or_expr    = and_expr { "OR" and_expr } ;
    and_expr   = not_expr { "AND" not_expr } ;
    not_expr   = [ "NOT" ] atom ;
    atom       = "(" expression ")" | comparison ;
    comparison = path operator operand ;
    operator   = "=" | "!=" | ">" | "<" | ">=" | "<="
               | "IN" | "NOT IN" | "MATCH" | "NOT MATCH" ;
    operand    = value | path | "(" operand { "," operand } ")" ;
    value      = string | number | enum_ref | boolean | "NULL" ;
    path       = segment { "." segment } ;
    segment    = identifier ;
    identifier = letter { letter | digit | "_" } ;
    string     = '"' { character } '"' ;
    number     = [ "-" ] digit { digit } [ "." digit { digit } ] ;
    enum_ref   = "#" identifier ;
    boolean    = "TRUE" | "FALSE" ;
    letter     = "A".."Z" | "a".."z" | "一".."鿿" ;
    digit      = "0".."9" ;
"""
from __future__ import annotations

import re

# Terminals the EBNF block must show. Kept here so the document check and the
# parser cannot drift into two different grammars without a test failing.
COMPARISON_OPERATORS = ["=", "!=", ">", "<", ">=", "<=", "IN", "NOT IN", "MATCH", "NOT MATCH"]
LOGICAL_CONNECTIVES = ["AND", "OR", "NOT"]
LITERAL_KINDS = ["string", "number", "enum_ref", "boolean", "NULL"]
PRODUCTIONS = [
    "expression",
    "or_expr",
    "and_expr",
    "not_expr",
    "atom",
    "comparison",
    "operator",
    "operand",
    "value",
    "path",
    "segment",
    "identifier",
    "string",
    "number",
    "enum_ref",
    "boolean",
    "letter",
    "digit",
]

KEYWORDS = {"AND", "OR", "NOT", "IN", "MATCH", "NULL", "TRUE", "FALSE"}

# The grammar text published in the 规则 DSL section, without the fence markers.
# The document must reproduce these production lines; the synthetic positive
# control in the test suite builds its EBNF block from this same string, so a
# document that drops a production fails against the parser's own grammar.
EBNF_GRAMMAR = """\
(* 设计规则条件表达式 —— 新增设计 *)
expression    = or_expr ;
or_expr       = and_expr { "OR" and_expr } ;
and_expr      = not_expr { "AND" not_expr } ;
not_expr      = [ "NOT" ] atom ;
atom          = "(" expression ")" | comparison ;
comparison    = path operator operand ;
operator      = "=" | "!=" | ">" | "<" | ">=" | "<="
              | "IN" | "NOT IN" | "MATCH" | "NOT MATCH" ;
operand       = value | path | "(" operand { "," operand } ")" ;
value         = string | number | enum_ref | boolean | "NULL" ;
path          = segment { "." segment } ;
segment       = identifier ;
identifier    = letter { letter | digit | "_" } ;
string        = '"' { character } '"' ;
number        = [ "-" ] digit { digit } [ "." digit { digit } ] ;
enum_ref      = "#" identifier ;
boolean       = "TRUE" | "FALSE" ;
letter        = "A".."Z" | "a".."z" | "一".."鿿" ;
digit         = "0".."9" ;"""

_TOKEN_RE = re.compile(
    r"""
    (?P<ws>\s+)
  | (?P<string>"[^"\n]*")
  | (?P<number>-?\d+(?:\.\d+)?)
  | (?P<enum>\#[A-Za-z_\u4e00-\u9fff][A-Za-z0-9_\u4e00-\u9fff]*)
  | (?P<op><=|>=|!=|=|<|>)
  | (?P<punct>[(),.])
  | (?P<word>[A-Za-z_\u4e00-\u9fff][A-Za-z0-9_\u4e00-\u9fff]*)
    """,
    re.VERBOSE,
)


class DSLParseError(ValueError):
    """Raised when an expression does not derive from ``expression``."""

    def __init__(self, message: str, source: str, offset: int):
        self.message = message
        self.source = source
        self.offset = offset
        marker = " " * offset + "^"
        super().__init__("%s at offset %d: %s\n%s\n%s" % (message, offset, source, marker, source))

    def summary(self) -> str:
        return "%s at offset %d" % (self.message, self.offset)


def tokenize(source: str) -> list:
    """Split ``source`` into ``(kind, text, offset)`` triples."""
    tokens = []
    position = 0
    length = len(source)
    while position < length:
        match = _TOKEN_RE.match(source, position)
        if match is None:
            raise DSLParseError(
                "unexpected character %r" % source[position], source, position
            )
        kind = match.lastgroup
        text = match.group()
        if kind != "ws":
            tokens.append((kind, text, position))
        position = match.end()
    return tokens


class _Parser:
    def __init__(self, source: str):
        self.source = source
        self.tokens = tokenize(source)
        self.index = 0

    # -- token helpers ---------------------------------------------------
    def _peek(self, ahead: int = 0):
        position = self.index + ahead
        if position < len(self.tokens):
            return self.tokens[position]
        return (None, None, len(self.source))

    def _at_end(self) -> bool:
        return self.index >= len(self.tokens)

    def _text(self) -> str:
        return self._peek()[1]

    def _kind(self) -> str:
        return self._peek()[0]

    def _error(self, message: str) -> DSLParseError:
        offset = self._peek()[2]
        return DSLParseError(message, self.source, offset)

    def _expect_text(self, text: str) -> None:
        if self._text() != text:
            raise self._error("expected %r, found %r" % (text, self._text()))
        self.index += 1

    def _expect_kind(self, kind: str, what: str) -> str:
        token_kind, text, _offset = self._peek()
        if token_kind != kind:
            raise self._error("expected %s, found %r" % (what, text))
        self.index += 1
        return text

    # -- productions -----------------------------------------------------
    def parse_expression(self) -> None:
        self._parse_or()
        if not self._at_end():
            raise self._error("trailing input %r after a complete expression" % self._text())

    def _parse_or(self) -> None:
        self._parse_and()
        while self._text() == "OR":
            self.index += 1
            self._parse_and()

    def _parse_and(self) -> None:
        self._parse_not()
        while self._text() == "AND":
            self.index += 1
            self._parse_not()

    def _parse_not(self) -> None:
        if self._text() == "NOT":
            self.index += 1
            if self._text() in ("IN", "MATCH"):
                raise self._error("'NOT %s' is an operator and needs a left-hand property path" % self._text())
        self._parse_atom()

    def _parse_atom(self) -> None:
        if self._text() == "(":
            self.index += 1
            self._parse_or()
            self._expect_text(")")
            return
        self._parse_comparison()

    def _parse_comparison(self) -> None:
        self._parse_path()
        self._parse_operator()
        self._parse_operand()

    def _parse_path(self) -> None:
        self._parse_segment()
        while self._text() == ".":
            self.index += 1
            self._parse_segment()

    def _parse_segment(self) -> None:
        text = self._expect_kind("word", "a property path segment")
        if text in KEYWORDS:
            raise self._error("keyword %r cannot be a property path segment" % text)

    def _parse_operator(self) -> None:
        text = self._text()
        if text in ("=", "!=", ">", "<", ">=", "<="):
            self.index += 1
            return
        if text in ("IN", "MATCH"):
            self.index += 1
            return
        if text == "NOT":
            following = self._peek(1)[1]
            if following in ("IN", "MATCH"):
                self.index += 2
                return
            raise self._error("expected a comparison operator after the property path, found 'NOT %s'" % following)
        raise self._error("expected a comparison operator after the property path, found %r" % text)

    def _parse_operand(self) -> None:
        if self._text() == "(":
            self.index += 1
            self._parse_operand()
            while self._text() == ",":
                self.index += 1
                self._parse_operand()
            self._expect_text(")")
            return
        kind = self._kind()
        if kind in ("string", "number", "enum"):
            self.index += 1
            return
        if kind == "word":
            text = self._text()
            if text in ("NULL", "TRUE", "FALSE"):
                self.index += 1
                return
            self._parse_path()
            return
        raise self._error("expected a value or property path, found %r" % self._text())


def parse(source: str) -> str:
    """Validate ``source`` against the grammar; return it unchanged.

    Raises :class:`DSLParseError` with an offset into ``source`` on failure.
    """
    text = (source or "").strip()
    if not text:
        raise DSLParseError("empty expression", source or "", 0)
    _Parser(text).parse_expression()
    return source


def is_valid(source: str) -> bool:
    try:
        parse(source)
    except DSLParseError:
        return False
    return True


def grammar_tokens() -> dict:
    """Every terminal the document's EBNF block must show, grouped."""
    return {
        "productions": list(PRODUCTIONS),
        "operators": list(COMPARISON_OPERATORS),
        "logical": list(LOGICAL_CONNECTIVES),
        "literals": list(LITERAL_KINDS),
    }
