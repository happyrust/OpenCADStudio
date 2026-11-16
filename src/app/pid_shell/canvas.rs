//! The editor's canvas inside the module: the colour a drawing is cleared
//! to and the way the grid is drawn over it, as the design has them
//! (frame `eSZiQ` of `docs/pid-platform/design/pid-platform.pen`).
//!
//! Both are the module's alone. The general interface keeps clearing to
//! the user's model space background and drawing its own grid, and that
//! setting is not written to: the module only does not use it.

use crate::app::OpenCADStudio;
use crate::ui::overlay::GridStyle;
use crate::ui::style::pid_tokens::{ColorToken, PidTheme};

/// Every how many lines of the grid one is drawn stronger: the design has a
/// line every 40 and a stronger one every 200.
pub const GRID_MAJOR_EVERY: u32 = 5;

/// The alpha of the grid's lines and of its stronger lines: the design's
/// `#FFFFFF06` and `#FFFFFF0D`.
pub const GRID_LINE_ALPHA: (f32, f32) = (6.0 / 255.0, 13.0 / 255.0);

/// The colour a drawing is cleared to: the design's `bg-canvas`.
pub fn colour(theme: PidTheme) -> [f32; 4] {
    let colour = ColorToken::BgCanvas.color(theme);
    [colour.r, colour.g, colour.b, colour.a]
}

/// How the grid is drawn: whole-pixel lines in the theme's text colour at
/// the design's two strengths, and no axis lines through the origin. The
/// spacing stays the drawing's own (it is in drawing units, and follows the
/// zoom), and so does whether the grid is shown at all.
pub fn grid_style(theme: PidTheme, bg_luminance: f32) -> GridStyle {
    let ink = ColorToken::TextPrimary.color(theme);
    GridStyle {
        // Not read while `line_alpha` is given.
        opacity: 0,
        bg_luminance,
        color: [ink.r, ink.g, ink.b],
        axes: false,
        line_alpha: Some(GRID_LINE_ALPHA),
    }
}

impl OpenCADStudio {
    /// Clear the open drawings, and every drawing opened from here on, to
    /// the module's canvas colour. Called as the module comes on;
    /// `sync_model_space_theme` keeps to it while the module is on.
    pub(in crate::app) fn use_pid_canvas(&mut self) {
        let colour = colour(self.pid_shell.theme);
        self.default_bg_color = Some(colour);
        for tab in &mut self.tabs {
            if tab.scene.bg_color == colour {
                continue;
            }
            tab.scene.bg_color = colour;
            tab.bg_color = Some(colour);
            tab.scene.recolor_meshes();
            tab.scene.bump_geometry();
            tab.scene
                .request_refresh(crate::scene::ViewportRefreshScope::All);
        }
    }
}
