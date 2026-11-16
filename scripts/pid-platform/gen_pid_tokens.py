#!/usr/bin/env python3
"""Generate the P&ID design-token table from the Pencil design.

    python scripts/pid-platform/gen_pid_tokens.py           rewrite the table
    python scripts/pid-platform/gen_pid_tokens.py --check   exit 1 when it is stale

Reads the `variables` (and `themes`) of
`docs/pid-platform/design/pid-platform.pen` and writes
`src/ui/style/pid_tokens/generated.rs`: one enum per variable type, each
variable's value for the dark and the light theme, and a lookup by the
design's own name. Run it again whenever the design's variables change; the
tests in `src/ui/style/pid_tokens.rs` read the same `.pen` and fail until the
table agrees with it.

How a `.pen` states a value (Pencil's document format):

* `"value": "#0B0E13"` -- one value, the same in every theme;
* `"value": [{"value": "#0B0E13", "theme": {"mode": "dark"}},
              {"value": "#FFFFFF", "theme": {"mode": "light"}}]` -- one value
  per theme; an entry applies when every axis it names has that value, an
  entry without `theme` applies always, and the last entry that applies wins;
* `"value": "$other-variable"` -- the value of another variable.

The axes are declared at the top of the document: `"themes": {"mode":
["dark", "light"]}`; the first value of an axis is its default. A document
without a theme axis has one set of values, and that set is the dark theme
(the design started dark-only); the light column of the table then repeats
the dark one and `DESIGN_THEMES` lists `Dark` alone.

The table has those two columns and no others. A design that declares a
theme value beyond dark and light, or states a value for a theme it does not
declare, is an error here, not a value quietly left out.

Standard library only. The output is passed through `rustfmt` when one is
found (`rustup run stable rustfmt`, then `rustfmt`), so the committed file is
what `cargo fmt` would leave.
"""
from __future__ import annotations

import argparse
import json
import re
import shutil
import subprocess
import sys
import tempfile
from pathlib import Path

REPO = Path(__file__).resolve().parents[2]
PEN = REPO / "docs" / "pid-platform" / "design" / "pid-platform.pen"
OUT = REPO / "src" / "ui" / "style" / "pid_tokens" / "generated.rs"

# What a theme value may be called. The names are compared in lower case.
DARK_NAMES = {"dark", "night", "暗", "暗色", "深色"}
LIGHT_NAMES = {"light", "day", "亮", "亮色", "浅色"}

# Variable type in the design -> (enum, what its doc comment calls it).
KINDS = {
    "color": ("ColorToken", "colour"),
    "number": ("NumberToken", "number"),
    "string": ("TextToken", "text"),
    "boolean": ("FlagToken", "flag"),
}


class DesignError(Exception):
    """The design states something this script does not know how to read."""


def theme_contexts(doc):
    """`[("Dark", context), ("Light", context)?]`: for each theme the design
    has, the value of every axis in it.

    The table has a dark and a light column and nothing else, so a design
    that declares any further theme value (a third value on the dark/light
    axis, a second axis with a choice on it) is refused: its values for that
    theme would have no place in the table."""
    axes = doc.get("themes") or {}
    defaults = {axis: values[0] for axis, values in axes.items() if values}
    if not axes:
        return [("Dark", {})]
    for axis, values in axes.items():
        light = [value for value in values if str(value).lower() in LIGHT_NAMES]
        if not light:
            continue
        dark = [value for value in values if str(value).lower() in DARK_NAMES]
        if not dark:
            # An axis like ["default", "light"]: what is not light is dark.
            dark = [value for value in values if value not in light][:1]
        if not dark:
            raise DesignError(f"theme axis {axis!r} names a light theme and nothing else: {values}")
        contexts = [
            ("Dark", {**defaults, axis: dark[0]}),
            ("Light", {**defaults, axis: light[0]}),
        ]
        beyond = [f"{other}={value}" for other, others in axes.items() for value in others
                  if all(context.get(other) != value for _, context in contexts)]
        if beyond:
            raise DesignError(
                "the design declares theme values the table has no theme for "
                f"(it has dark and light): {', '.join(beyond)}"
            )
        return contexts
    raise DesignError(
        "the design declares theme axes but none of them names a light theme "
        f"({json.dumps(axes, ensure_ascii=False)}); teach DARK_NAMES / LIGHT_NAMES the names"
    )


def undeclared_themes(doc):
    """`["name: axis=value", …]`: the per-theme values that are stated for a
    theme the document does not declare, and so apply nowhere."""
    axes = doc.get("themes") or {}
    return [
        f"{name}: {axis}={value}"
        for name, body in (doc.get("variables") or {}).items()
        if isinstance(body.get("value"), list)
        for entry in body["value"]
        for axis, value in (entry.get("theme") or {}).items()
        if value not in (axes.get(axis) or [])
    ]


def resolve(variables, name, context, trail=()):
    """The value of `name` in the theme `context` describes."""
    if name not in variables:
        raise DesignError(f"{' -> '.join(trail + (name,))}: no such variable")
    if name in trail:
        raise DesignError(f"variables refer to each other in a circle: {' -> '.join(trail + (name,))}")
    raw = variables[name].get("value")
    if isinstance(raw, list):
        chosen = [
            entry.get("value")
            for entry in raw
            if all(context.get(axis) == value for axis, value in (entry.get("theme") or {}).items())
        ]
        if not chosen:
            raise DesignError(f"{name}: no value applies in theme {context}")
        raw = chosen[-1]
    if isinstance(raw, str) and raw.startswith("$"):
        return resolve(variables, raw[1:], context, trail + (name,))
    return raw


def parse_color(text, name):
    match = re.fullmatch(r"#([0-9a-fA-F]{3,4}|[0-9a-fA-F]{6}|[0-9a-fA-F]{8})", str(text).strip())
    if not match:
        raise DesignError(f"{name}: {text!r} is not a #RGB / #RGBA / #RRGGBB / #RRGGBBAA colour")
    digits = match.group(1)
    if len(digits) in (3, 4):
        digits = "".join(ch * 2 for ch in digits)
    if len(digits) == 6:
        digits += "FF"
    return tuple(int(digits[i:i + 2], 16) for i in range(0, 8, 2))


def typed(kind, raw, name):
    """`raw` as the Python value of its kind."""
    if kind == "color":
        return parse_color(raw, name)
    if kind == "number":
        if isinstance(raw, bool) or not isinstance(raw, (int, float)):
            raise DesignError(f"{name}: {raw!r} is not a number")
        return float(raw)
    if kind == "string":
        if not isinstance(raw, str):
            raise DesignError(f"{name}: {raw!r} is not text")
        return raw
    if kind == "boolean":
        if not isinstance(raw, bool):
            raise DesignError(f"{name}: {raw!r} is not a flag")
        return raw
    raise DesignError(f"{name}: unknown variable type {kind!r}")


def variant(name):
    parts = [part for part in re.split(r"[^0-9A-Za-z]+", name) if part]
    ident = "".join(part[:1].upper() + part[1:] for part in parts)
    if not ident:
        raise DesignError(f"{name!r} leaves no identifier")
    return ("N" + ident) if ident[0].isdigit() else ident


def rust_value(kind, value):
    if kind == "color":
        return "[" + ", ".join(f"0x{part:02X}" for part in value) + "]"
    if kind == "number":
        text = repr(value)
        return text if ("." in text or "e" in text or "inf" in text) else text + ".0"
    if kind == "string":
        return json.dumps(value, ensure_ascii=False)
    return "true" if value else "false"


def shown(kind, value):
    """A value as the design writes it, for the doc comments."""
    if kind == "color":
        r, g, b, a = value
        return f"#{r:02X}{g:02X}{b:02X}" + ("" if a == 255 else f"{a:02X}")
    if kind == "number":
        return repr(value)[:-2] if repr(value).endswith(".0") else repr(value)
    if kind == "string":
        return value
    return "true" if value else "false"


def read_design(path=PEN):
    """`(themes, tokens)`: the theme names the design gives values for, and
    per variable `(name, kind, [dark, light])` in the design's order."""
    doc = json.loads(Path(path).read_text(encoding="utf-8"))
    variables = doc.get("variables") or {}
    contexts = theme_contexts(doc)
    stray = undeclared_themes(doc)
    if stray:
        raise DesignError(
            "values are stated for themes the design does not declare: " + ", ".join(stray)
        )
    tokens = []
    for name, body in variables.items():
        kind = body.get("type")
        if kind not in KINDS:
            raise DesignError(f"{name}: unknown variable type {kind!r}")
        values = [typed(kind, resolve(variables, name, context), name) for _, context in contexts]
        if len(values) == 1:
            values.append(values[0])
        tokens.append((name, kind, values))
    return [theme for theme, _ in contexts], tokens


RUST_TYPES = {"color": "Rgba", "number": "f32", "string": "&'static str", "boolean": "bool"}
ACCESSORS = {"color": "rgba", "number": "value", "string": "value", "boolean": "value"}
LOOKUP_VARIANTS = {"color": "Color", "number": "Number", "string": "Text", "boolean": "Flag"}


def render(themes, tokens):
    lines = [
        "// @generated by scripts/pid-platform/gen_pid_tokens.py -- do not edit by hand.",
        "// Source: the `variables` of docs/pid-platform/design/pid-platform.pen.",
        "// After the design changes: python scripts/pid-platform/gen_pid_tokens.py",
        "",
        "use super::{PidTheme, Rgba, TokenValue};",
        "",
        "/// The themes the design gives values for. A theme missing here has no",
        "/// values of its own in the design: the tables repeat the dark ones for it.",
        "pub(super) const DESIGN_THEMES: &[PidTheme] = &["
        + ", ".join(f"PidTheme::{theme}" for theme in themes) + "];",
        "",
        "/// Every variable of the design by the name it has there, in the design's",
        "/// order.",
        "pub(super) const NAMES: &[&str] = &[",
    ]
    lines += [f"    {json.dumps(name, ensure_ascii=False)}," for name, _, _ in tokens]
    lines += ["];", ""]

    present = [kind for kind in KINDS if any(token[1] == kind for token in tokens)]
    for kind in present:
        enum, noun = KINDS[kind]
        mine = [(name, values) for name, token_kind, values in tokens if token_kind == kind]
        seen = {}
        for name, _ in mine:
            ident = variant(name)
            if ident in seen:
                raise DesignError(f"{name!r} and {seen[ident]!r} both become {enum}::{ident}")
            seen[ident] = name
        derive = "#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]"
        lines += [f"/// The {noun} variables of the design.", derive, f"pub enum {enum} {{"]
        for name, values in mine:
            doc = f"dark `{shown(kind, values[0])}`"
            if len(themes) > 1:
                doc += f", light `{shown(kind, values[1])}`"
            lines += [f"    /// `{name}`: {doc}.", f"    {variant(name)},"]
        lines += ["}", "", f"impl {enum} {{"]
        lines += [f"    /// Every {noun} variable, in the design's order.",
                  f"    pub const ALL: &'static [{enum}] = &["]
        lines += [f"        {enum}::{variant(name)}," for name, _ in mine]
        lines += ["    ];", ""]
        lines += ["    /// The variable's name in the design.",
                  "    pub const fn name(self) -> &'static str {", "        match self {"]
        lines += [f"            {enum}::{variant(name)} => {json.dumps(name, ensure_ascii=False)},"
                  for name, _ in mine]
        lines += ["        }", "    }", ""]
        lines += [f"    /// The value in `theme`; the dark one when the design has none for it.",
                  f"    pub const fn {ACCESSORS[kind]}(self, theme: PidTheme) -> {RUST_TYPES[kind]} {{",
                  "        self.themed()[theme.index()]", "    }", ""]
        lines += ["    /// `[dark, light]`.",
                  f"    const fn themed(self) -> [{RUST_TYPES[kind]}; 2] {{", "        match self {"]
        for name, values in mine:
            pair = ", ".join(rust_value(kind, value) for value in values)
            lines.append(f"            {enum}::{variant(name)} => [{pair}],")
        lines += ["        }", "    }", "}", ""]

    lines += ["/// The value of the variable the design calls `name`, in `theme`.",
              "pub(super) fn lookup(name: &str, theme: PidTheme) -> Option<TokenValue> {"]
    for kind in present:
        enum, _ = KINDS[kind]
        lines += [f"    if let Some(token) = {enum}::ALL.iter().find(|token| token.name() == name) {{",
                  f"        return Some(TokenValue::{LOOKUP_VARIANTS[kind]}(token.{ACCESSORS[kind]}(theme)));",
                  "    }"]
    lines += ["    None", "}", ""]
    return "\n".join(lines)


def rustfmt(path):
    """Format `path` in place; False when no rustfmt could be run."""
    for command in (["rustup", "run", "stable", "rustfmt"], ["rustfmt"]):
        if shutil.which(command[0]) is None:
            continue
        done = subprocess.run(command + ["--edition", "2021", str(path)],
                              capture_output=True, text=True)
        if done.returncode == 0:
            return True
        sys.stderr.write(done.stderr)
    return False


def formatted(text):
    """`text` as rustfmt leaves it (unchanged when there is no rustfmt)."""
    with tempfile.TemporaryDirectory() as folder:
        scratch = Path(folder) / "generated.rs"
        scratch.write_text(text, encoding="utf-8", newline="\n")
        if not rustfmt(scratch):
            sys.stderr.write("gen_pid_tokens: no rustfmt found; the table is written unformatted\n")
        return scratch.read_text(encoding="utf-8")


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__.split("\n\n")[0])
    parser.add_argument("--check", action="store_true",
                        help="write nothing; exit 1 when the committed table differs")
    args = parser.parse_args(argv)
    try:
        themes, tokens = read_design()
        text = formatted(render(themes, tokens))
    except DesignError as error:
        print(f"gen_pid_tokens: {error}", file=sys.stderr)
        return 2
    kinds = {kind: sum(1 for token in tokens if token[1] == kind) for kind in KINDS}
    summary = (f"{len(tokens)} variables (" + ", ".join(f"{count} {kind}" for kind, count in kinds.items() if count)
               + f"), themes: {', '.join(themes)}")
    current = OUT.read_text(encoding="utf-8") if OUT.exists() else None
    if args.check:
        if current == text:
            print(f"gen_pid_tokens: {OUT.relative_to(REPO).as_posix()} is up to date: {summary}")
            return 0
        print(f"gen_pid_tokens: {OUT.relative_to(REPO).as_posix()} is STALE against the design "
              f"({summary}); run python scripts/pid-platform/gen_pid_tokens.py", file=sys.stderr)
        return 1
    OUT.parent.mkdir(parents=True, exist_ok=True)
    OUT.write_text(text, encoding="utf-8", newline="\n")
    print(f"gen_pid_tokens: wrote {OUT.relative_to(REPO).as_posix()}: {summary}"
          + ("" if current != text else " (unchanged)"))
    return 0


if __name__ == "__main__":
    sys.exit(main())
