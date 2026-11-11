//! The typefaces of the P&ID interface are in the font system once the shell
//! is up — and are asked for by nothing that stays in the general interface.

use iced::font::Weight;

use super::*;
use crate::cli::GuiConfig;
use crate::ui::style::pid_fonts;
use crate::ui::style::pid_tokens::{PidTheme, TextToken};

/// The application as a launch with this command line leaves it, on a
/// machine where the startup prompts have been answered.
fn launched(classic: bool) -> OpenCADStudio {
    let mut app = OpenCADStudio::new_for_test();
    app.default_assoc_prompted = true;
    app.donation_prompt_version = env!("OCS_APP_VERSION").to_owned();
    let cfg = GuiConfig {
        classic,
        ..GuiConfig::default()
    };
    let first = app.open_first_screen(&cfg);
    app.drive_headless_task(first)
        .expect("the first screen opens without a window");
    app
}

/// The file of the carried face of `token`'s family at `weight`.
fn carried(token: TextToken, weight: u16) -> &'static str {
    pid_fonts::FACES
        .iter()
        .find(|face| face.family == token.value(PidTheme::Dark) && face.weight == weight)
        .unwrap_or_else(|| panic!("a face of {token:?} at {weight} is carried"))
        .file
}

/// With the shell up, text set in the theme's font names is set in the
/// faces carried in the program — each weight in its own face, not in a
/// neighbouring weight and not in a face the font system fell back to.
#[test]
fn the_fonts_of_the_theme_are_the_faces_carried_in_the_program() {
    let app = launched(false);
    assert_eq!(app.pid_shell.current(), Some(PidScreen::Editor));
    let theme = app.pid_shell.theme;
    let sample = "P&ID Editor 0123 PID-1101 rev 7";
    for (weight, number) in [
        (Weight::Normal, 400),
        (Weight::Medium, 500),
        (Weight::Semibold, 600),
    ] {
        let from = pid_fonts::faces_setting(pid_fonts::ui(theme, weight), sample);
        let file = carried(TextToken::FontUi, number);
        assert!(
            !from.is_empty() && from.iter().all(|face| *face == Some(file)),
            "{weight:?}: every glyph from {file}, got {from:?}"
        );
    }
    for (weight, number) in [
        (Weight::Normal, 400),
        (Weight::Semibold, 600),
        (Weight::Bold, 700),
    ] {
        let from = pid_fonts::faces_setting(pid_fonts::mono(theme, weight), sample);
        let file = carried(TextToken::FontMono, number);
        assert!(
            !from.is_empty() && from.iter().all(|face| *face == Some(file)),
            "{weight:?}: every glyph from {file}, got {from:?}"
        );
    }
    // Neither family has Chinese: those glyphs come from wherever the system
    // has them, and the Latin ones beside them still from the carried face.
    let from = pid_fonts::faces_setting(pid_fonts::ui(theme, Weight::Normal), "编辑器 P&ID");
    let file = carried(TextToken::FontUi, 400);
    assert_eq!(from.len(), 8, "{from:?}");
    assert!(from[3..].iter().all(|face| *face == Some(file)), "{from:?}");
}

/// The faces are asked for when the shell comes up, however it comes up, and
/// by nothing before that: a session that stays in the general interface
/// has the font system it always had.
#[test]
fn only_the_shell_asks_for_the_faces() {
    assert_eq!(pid_fonts::requests(), 0, "nothing has asked on this thread");

    let mut app = launched(true);
    assert_eq!(app.pid_shell.current(), None);
    let _ = app.update(AppMessage::TabNew);
    let _ = app.dispatch_command("LINE 0,0 10,10");
    assert_eq!(
        pid_fonts::requests(),
        0,
        "the general interface does not ask for them"
    );

    // Turned on from the general interface.
    let _ = app.dispatch_command("PIDSCREEN editor");
    assert_eq!(app.pid_shell.current(), Some(PidScreen::Editor));
    assert!(pid_fonts::requests() > 0, "turning the module on asks");

    // Launched into it: on another thread, whose count starts at nothing.
    let asked = std::thread::spawn(|| {
        let before = pid_fonts::requests();
        let app = launched(false);
        assert_eq!(app.pid_shell.current(), Some(PidScreen::Editor));
        (before, pid_fonts::requests())
    })
    .join()
    .expect("the launch");
    assert_eq!(asked.0, 0);
    assert!(asked.1 > 0, "a launch into the module asks");
}
