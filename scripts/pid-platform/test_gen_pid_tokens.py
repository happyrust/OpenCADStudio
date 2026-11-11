"""The design-token generator: how it reads a `.pen`'s variables and themes.

The committed design draws one theme, so the per-theme half of the reader is
tried here on documents that have two. The Rust side has its own reading of
the same rules (`src/ui/style/pid_tokens.rs`, the tests) and is held to the
same examples.
"""
import json
import shutil

import pytest

import gen_pid_tokens as gen

TWO_THEMES = {
    # `density` has one value, its default: an axis with no choice on it.
    "themes": {"density": ["regular"], "mode": ["dark", "light"]},
    "variables": {
        "bg": {"type": "color", "value": [
            {"value": "#0B0E13", "theme": {"mode": "dark"}},
            {"value": "#FFFFFF", "theme": {"mode": "light"}},
        ]},
        "accent": {"type": "color", "value": "#4C9EFF"},
        "accent-soft": {"type": "color", "value": [
            {"value": "#4C9EFF1F"},
            {"value": "#000", "theme": {"mode": "light"}},
            {"value": "#4C9EFF33", "theme": {"mode": "light", "density": "regular"}},
        ]},
        "focus": {"type": "color", "value": "$accent"},
        "radius": {"type": "number", "value": 4},
        "font": {"type": "string", "value": "Inter"},
    },
}


def read(tmp_path, doc):
    path = tmp_path / "design.pen"
    path.write_text(json.dumps(doc, ensure_ascii=False), encoding="utf-8")
    themes, tokens = gen.read_design(path)
    return themes, {name: (kind, values) for name, kind, values in tokens}


def test_a_document_without_a_theme_axis_is_the_dark_theme(tmp_path):
    themes, tokens = read(tmp_path, {"variables": {"bg": {"type": "color", "value": "#abc"}}})
    assert themes == ["Dark"]
    # The light column repeats the dark one.
    assert tokens["bg"] == ("color", [(0xAA, 0xBB, 0xCC, 0xFF)] * 2)


def test_a_document_with_two_themes_gives_each_variable_two_values(tmp_path):
    themes, tokens = read(tmp_path, TWO_THEMES)
    assert themes == ["Dark", "Light"]
    assert tokens["bg"][1] == [(0x0B, 0x0E, 0x13, 0xFF), (0xFF, 0xFF, 0xFF, 0xFF)]
    # One value: the same in both themes.
    assert tokens["accent"][1] == [(0x4C, 0x9E, 0xFF, 0xFF)] * 2
    # An entry without a theme applies always; the last entry that applies wins.
    assert tokens["accent-soft"][1] == [(0x4C, 0x9E, 0xFF, 0x1F), (0x4C, 0x9E, 0xFF, 0x33)]
    # A reference takes the other variable's value in the same theme.
    assert tokens["focus"][1] == tokens["accent"][1]
    assert tokens["radius"] == ("number", [4.0, 4.0])
    assert tokens["font"] == ("string", ["Inter", "Inter"])


def test_theme_names_other_than_dark_and_light(tmp_path):
    themes, tokens = read(tmp_path, {
        "themes": {"主题": ["默认", "亮色"]},
        "variables": {"bg": {"type": "color", "value": [
            {"value": "#000000"},
            {"value": "#FFFFFF", "theme": {"主题": "亮色"}},
        ]}},
    })
    assert themes == ["Dark", "Light"]
    assert tokens["bg"][1] == [(0, 0, 0, 255), (255, 255, 255, 255)]


def test_what_the_reader_cannot_read_is_an_error_not_a_guess(tmp_path):
    with pytest.raises(gen.DesignError, match="light theme"):
        read(tmp_path, {"themes": {"mode": ["a", "b"]}, "variables": {}})
    with pytest.raises(gen.DesignError, match="circle"):
        read(tmp_path, {"variables": {"a": {"type": "color", "value": "$b"},
                                      "b": {"type": "color", "value": "$a"}}})
    with pytest.raises(gen.DesignError, match="not a #RGB"):
        read(tmp_path, {"variables": {"a": {"type": "color", "value": "blue"}}})
    with pytest.raises(gen.DesignError, match="unknown variable type"):
        read(tmp_path, {"variables": {"a": {"type": "gradient", "value": "x"}}})


def test_a_theme_value_the_table_has_no_column_for_is_refused(tmp_path):
    """Dark and light are all the table holds: a design that states more is
    not read as if it stated less."""
    with pytest.raises(gen.DesignError, match="no theme for.*mode=contrast"):
        read(tmp_path, {"themes": {"mode": ["dark", "light", "contrast"]}, "variables": {}})
    with pytest.raises(gen.DesignError, match="no theme for.*density=compact"):
        read(tmp_path, {"themes": {"mode": ["dark", "light"], "density": ["regular", "compact"]},
                        "variables": {}})
    # A value for a theme the document does not declare applies nowhere.
    with pytest.raises(gen.DesignError, match="does not declare.*bg: mode=Light"):
        read(tmp_path, {"themes": {"mode": ["dark", "light"]}, "variables": {
            "bg": {"type": "color", "value": [
                {"value": "#000000"},
                {"value": "#FFFFFF", "theme": {"mode": "Light"}},
            ]}}})


def test_the_rendered_table_names_both_themes_and_every_variable(tmp_path):
    path = tmp_path / "design.pen"
    path.write_text(json.dumps(TWO_THEMES), encoding="utf-8")
    text = gen.render(*gen.read_design(path))
    assert "DESIGN_THEMES: &[PidTheme] = &[PidTheme::Dark, PidTheme::Light];" in text
    assert "ColorToken::Bg => [[0x0B, 0x0E, 0x13, 0xFF], [0xFF, 0xFF, 0xFF, 0xFF]]," in text
    assert "/// `bg`: dark `#0B0E13`, light `#FFFFFF`." in text
    assert "NumberToken::Radius => [4.0, 4.0]," in text
    assert 'TextToken::Font => ["Inter", "Inter"],' in text
    for name in TWO_THEMES["variables"]:
        assert json.dumps(name) + "," in text


def test_the_committed_table_is_what_the_design_generates():
    if shutil.which("rustup") is None and shutil.which("rustfmt") is None:
        pytest.skip("no rustfmt to format the table with")
    assert gen.main(["--check"]) == 0, "run python scripts/pid-platform/gen_pid_tokens.py"
