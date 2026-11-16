//! P&ID symbol palette — docked panel (PIDPALETTE).
//!
//! The palette is the index of what `PIDINSERT` can place: one tab per
//! category of `assets/pid-symbols.json` (its `categories[].label` is the tab
//! text, so the panel and the symbol library never disagree), and a grid of
//! symbol cells under the active tab. Every cell draws its symbol's own
//! `geometry` fit to the cell, so a cell is never an empty box — what the
//! palette shows is exactly what the block will contain.
//!
//! Clicking a cell starts `PIDINSERT <block name>`: the palette is the
//! mouse-driven front of the same command the command line runs, so the
//! insertion mode, its preview and its commit path have one implementation.
//!
//! The active category lives on the application, not on the tab, so switching
//! documents leaves the palette open on the tab the user left it on.

use crate::app::Message;
use crate::pid::symbols::{block_name, Geometry, SymbolCategory, SymbolDefinition};
use crate::ui::dock::{Chrome, PanelId};
use iced::widget::canvas::{Frame, Path, Program, Stroke, Text as CanvasText};
use iced::widget::{
    button, canvas, column, container, row, scrollable, text, tooltip, Space,
};
use iced::{Background, Border, Element, Fill, Length, Point, Size, Theme};

/// What the user did in the palette.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PidPaletteMsg {
    /// Bring category `index` (into `SymbolLibrary::categories()`) forward.
    Tab(usize),
    /// Insert the symbol whose block name is this.
    Insert(String),
}

impl PidPaletteMsg {
    pub(crate) fn message(self) -> Message {
        Message::PidPalette(self)
    }
}

/// The palette's own state: which category tab is active.
///
/// Deliberately app-level (not per tab): the dock keeps the panel open across
/// document switches, so the tab the user chose has to survive them too.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PidPalette {
    /// Index into `SymbolLibrary::categories()`. Kept in range by
    /// [`Self::category_index`] rather than clamped on write, because the
    /// symbol library can be swapped (a different standard) under it.
    pub active: usize,
}

impl PidPalette {
    /// `active` clamped to the categories `count` actually has.
    pub fn category_index(&self, count: usize) -> usize {
        if count == 0 {
            0
        } else {
            self.active.min(count - 1)
        }
    }
}

/// Side of one grid cell's icon box, in pixels.
const CELL: f32 = 46.0;

/// Padding inside a cell's icon box, so the drawing never touches the border.
const CELL_PAD: f32 = 5.0;

/// The active tab's category, or `None` when the library has none.
pub fn active_category<'a>(
    palette: &PidPalette,
    categories: &'a [SymbolCategory],
) -> Option<&'a SymbolCategory> {
    categories.get(palette.category_index(categories.len()))
}

/// Build the docked panel.
///
/// `categories` is `SymbolLibrary::categories()` — the single source for both
/// the tab texts and the grid's contents.
pub fn view<'a>(
    palette: &'a PidPalette,
    categories: &'a [SymbolCategory],
    width: f32,
    chrome: Chrome,
) -> Element<'a, Message> {
    let title_bar = crate::ui::dock::title_bar(
        PanelId::PidPalette,
        "P&ID 符号".to_string(),
        chrome,
    );

    if categories.is_empty() {
        let hint = container(text("符号库为空").size(12).style(muted))
            .center_x(Fill)
            .center_y(Fill)
            .width(Fill)
            .height(Fill);
        return crate::ui::dock::frame(
            title_bar,
            column![hint].spacing(6).height(Fill),
            width,
        );
    }

    let active = palette.category_index(categories.len());

    // ── Category tabs ────────────────────────────────────────────────────
    let mut tabs = row![].spacing(2);
    for (index, category) in categories.iter().enumerate() {
        let selected = index == active;
        let tab = button(text(category.label.clone()).size(11))
            .on_press(PidPaletteMsg::Tab(index).message())
            .padding([3, 8])
            .style(move |theme: &Theme, status| {
                if selected {
                    button::primary(theme, status)
                } else {
                    button::secondary(theme, status)
                }
            });
        tabs = tabs.push(tooltip(
            tab,
            text(format!("{}（{} 个符号）", category.label, category.symbols.len())).size(10),
            tooltip::Position::Bottom,
        )
        .gap(4));
    }
    let tabs = scrollable(tabs)
        .direction(scrollable::Direction::Horizontal(
            scrollable::Scrollbar::default(),
        ))
        .width(Fill)
        .height(Length::Fixed(28.0));

    // ── Symbol grid of the active category ───────────────────────────────
    let category = &categories[active];
    let mut grid = column![].spacing(4);
    let mut cells = row![].spacing(4);
    let mut in_row = 0usize;
    // Columns that fit the docked width, leaving room for the padding and
    // the scroll bar.
    let columns = (((width - 24.0) / (CELL + 4.0)).floor() as usize).max(1);
    for symbol in &category.symbols {
        let block = block_name(&category.id, &symbol.id);
        cells = cells.push(cell(symbol, block));
        in_row += 1;
        if in_row == columns {
            grid = grid.push(cells);
            cells = row![].spacing(4);
            in_row = 0;
        }
    }
    if in_row > 0 {
        grid = grid.push(cells);
    }
    let count = category.symbols.len();
    let heading = container(
        row![
            text(category.label.clone()).size(12),
            Space::new().width(Fill),
            text(format!("{count} 个符号")).size(10).style(muted),
        ]
        .align_y(iced::Center),
    )
    .width(Fill);

    let body = scrollable(column![heading, grid].spacing(6))
        .direction(scrollable::Direction::Vertical(
            scrollable::Scrollbar::default(),
        ))
        .width(Fill)
        .height(Fill);

    let content = column![tabs, body].spacing(6).height(Fill);
    let reported = crate::ui::wrap_bar::PosReport::new(PANEL_BOUNDS_ID, content);
    crate::ui::dock::frame(title_bar, reported, width)
}

/// `PosReport` id of the panel body, so a client can locate it on screen.
pub const PANEL_BOUNDS_ID: &str = "pid-palette-panel";

/// `PosReport` id prefix of the grid cells, so a client can select them all
/// (and so a tab switch can forget the ones it no longer shows).
pub const CELL_BOUNDS_PREFIX: &str = "pid-palette-cell-";

/// `PosReport` id of the grid cell for `block`.
///
/// The palette's own background matches the drawing area's, so a screenshot
/// cannot be segmented by colour alone; reporting each cell's rectangle lets a
/// client check the cell it means instead of guessing where the grid is.
pub fn cell_bounds_id(block: &str) -> String {
    format!("{CELL_BOUNDS_PREFIX}{block}")
}

/// One symbol cell: its icon, its label, and the click that starts the
/// insertion.
fn cell<'a>(symbol: &'a SymbolDefinition, block: String) -> Element<'a, Message> {
    let icon = canvas(SymbolIcon {
        geometry: &symbol.geometry,
        ink: None,
        pad: CELL_PAD,
    })
    .width(Length::Fixed(CELL))
    .height(Length::Fixed(CELL));
    let tip = format!("{} — PIDINSERT {}", symbol.label, block);
    let target = block.clone();
    let hit = button(
        column![
            icon,
            text(symbol.label.clone())
                .size(9)
                .width(Length::Fixed(CELL))
                .align_x(iced::Center),
        ]
        .spacing(1)
        .align_x(iced::Center),
    )
    .on_press(PidPaletteMsg::Insert(target).message())
    .padding(2)
    .style(|theme: &Theme, status| {
        let mut style = button::text(theme, status);
        style.border = Border {
            color: match status {
                button::Status::Hovered | button::Status::Pressed => {
                    theme.palette().primary.base.color
                }
                _ => theme.palette().background.neutral.color,
            },
            width: 1.0,
            radius: 3.0.into(),
        };
        if matches!(status, button::Status::Hovered | button::Status::Pressed) {
            style.background = Some(Background::Color(theme.palette().primary.weak.color));
        }
        style
    });
    // The reported bounds are the cell's own, so a client can read a cell's
    // rectangle out of the panel instead of inferring it from the layout.
    let reported = crate::ui::wrap_bar::PosReport::owned(cell_bounds_id(&block), hit);
    tooltip(reported, text(tip).size(10), tooltip::Position::Right)
        .gap(4)
        .into()
}

fn muted(theme: &Theme) -> iced::widget::text::Style {
    iced::widget::text::Style {
        color: Some(theme.palette().background.base.text.scale_alpha(0.6)),
    }
}

/// The box a symbol's geometry is fitted into, in cell-local pixels.
///
/// Split out so the fit is testable without a renderer.
pub fn fit_rect(size: Size) -> (f32, f32, f32, f32) {
    fit_rect_padded(size, CELL_PAD)
}

/// [`fit_rect`] with `pad` pixels left free on every side.
pub fn fit_rect_padded(size: Size, pad: f32) -> (f32, f32, f32, f32) {
    let inner_w = (size.width - 2.0 * pad).max(1.0);
    let inner_h = (size.height - 2.0 * pad).max(1.0);
    (pad, pad, inner_w, inner_h)
}

/// A symbol's own drawing as an icon `side` pixels square, stroked in
/// `ink`: what a palette cell draws, for whoever else shows the symbol.
pub fn symbol_icon<'a>(
    geometry: &'a [Geometry],
    side: f32,
    ink: iced::Color,
) -> Element<'a, Message> {
    canvas(SymbolIcon {
        geometry,
        ink: Some(ink),
        pad: 2.0,
    })
    .width(Length::Fixed(side))
    .height(Length::Fixed(side))
    .into()
}

/// Bounding box of `geometry` in symbol coordinates, `None` when it has no
/// drawable point.
///
/// An arc contributes its circle's box: the icon is small enough that the
/// extra margin is invisible, and it keeps the fit monotone instead of
/// snapping while the arc's quadrant count changes.
pub fn geometry_bounds(geometry: &[Geometry]) -> Option<(f64, f64, f64, f64)> {
    let mut points: Vec<(f64, f64)> = Vec::new();
    let mut add = |x: f64, y: f64| {
        if x.is_finite() && y.is_finite() {
            points.push((x, y));
        }
    };
    for item in geometry {
        match item {
            Geometry::Line { from, to } => {
                add(from[0], from[1]);
                add(to[0], to[1]);
            }
            Geometry::Polyline { points, .. } => {
                for point in points {
                    add(point[0], point[1]);
                }
            }
            Geometry::Circle { center, radius } | Geometry::Arc { center, radius, .. } => {
                add(center[0] - radius, center[1] - radius);
                add(center[0] + radius, center[1] + radius);
            }
            // A text's box is its insertion point plus roughly its height;
            // widening it by the height on both axes keeps a lettered symbol
            // from being clipped when it is the only geometry.
            Geometry::Text { at, height, .. } => {
                let half = height.abs() * 0.5;
                add(at[0] - half, at[1] - half);
                add(at[0] + half, at[1] + half);
            }
        }
    }
    let (first, rest) = points.split_first()?;
    let mut bounds = (first.0, first.1, first.0, first.1);
    for (x, y) in rest {
        bounds.0 = bounds.0.min(*x);
        bounds.1 = bounds.1.min(*y);
        bounds.2 = bounds.2.max(*x);
        bounds.3 = bounds.3.max(*y);
    }
    Some(bounds)
}

/// Canvas program drawing one symbol's geometry fit-to-box.
struct SymbolIcon<'a> {
    geometry: &'a [Geometry],
    /// The stroke colour; the theme's text colour when `None`.
    ink: Option<iced::Color>,
    /// Pixels left free on every side.
    pad: f32,
}

impl<'a> Program<Message> for SymbolIcon<'a> {
    type State = ();

    fn draw(
        &self,
        _state: &(),
        renderer: &iced::Renderer,
        theme: &Theme,
        bounds: iced::Rectangle,
        _cursor: iced::mouse::Cursor,
    ) -> Vec<canvas::Geometry> {
        let mut frame = Frame::new(renderer, bounds.size());
        let ink = self
            .ink
            .unwrap_or_else(|| theme.palette().background.base.text);
        draw_symbol(&mut frame, self.geometry, bounds.size(), ink, self.pad);
        vec![frame.into_geometry()]
    }
}

/// Draw `geometry` fit into `size` less `pad` on every side, centred, in
/// `ink`.
fn draw_symbol(frame: &mut Frame, geometry: &[Geometry], size: Size, ink: iced::Color, pad: f32) {
    let stroke = Stroke::default().with_color(ink).with_width(1.4);
    let (pad_x, pad_y, inner_w, inner_h) = fit_rect_padded(size, pad);

    let Some((min_x, min_y, max_x, max_y)) = geometry_bounds(geometry) else {
        // Nothing drawable: a mark so the cell is still visibly occupied.
        frame.fill(
            &Path::circle(Point::new(size.width * 0.5, size.height * 0.5), 2.0),
            ink,
        );
        return;
    };
    let span_x = (max_x - min_x).max(1e-6);
    let span_y = (max_y - min_y).max(1e-6);
    let scale = (inner_w as f64 / span_x).min(inner_h as f64 / span_y) as f32;
    let drawn_w = span_x as f32 * scale;
    let drawn_h = span_y as f32 * scale;
    let origin_x = pad_x + (inner_w - drawn_w) * 0.5;
    let origin_y = pad_y + (inner_h - drawn_h) * 0.5;
    // Symbol y grows up, screen y grows down.
    let map = |x: f64, y: f64| {
        Point::new(
            origin_x + (x - min_x) as f32 * scale,
            origin_y + (max_y - y) as f32 * scale,
        )
    };

    for item in geometry {
        match item {
            Geometry::Line { from, to } => {
                frame.stroke(&Path::line(map(from[0], from[1]), map(to[0], to[1])), stroke);
            }
            Geometry::Polyline { points, closed } => {
                if points.len() < 2 {
                    continue;
                }
                let path = Path::new(|builder| {
                    builder.move_to(map(points[0][0], points[0][1]));
                    for point in &points[1..] {
                        builder.line_to(map(point[0], point[1]));
                    }
                    if *closed {
                        builder.close();
                    }
                });
                frame.stroke(&path, stroke);
            }
            Geometry::Circle { center, radius } => {
                let radius_px = (*radius as f32 * scale).max(0.5);
                frame.stroke(
                    &Path::circle(map(center[0], center[1]), radius_px),
                    stroke,
                );
            }
            Geometry::Arc {
                center,
                radius,
                start_deg,
                end_deg,
            } => {
                // Approximated as a polyline: the icon is 46 px wide, so the
                // chord error of a 2° step is far below a pixel, and this
                // keeps the arc out of the canvas builder's lyon types.
                let radius_px = (*radius as f32 * scale).max(0.5);
                let center_px = map(center[0], center[1]);
                let mut sweep = end_deg - start_deg;
                if sweep <= 0.0 {
                    sweep += 360.0;
                }
                let steps = ((sweep.abs() / 2.0).ceil() as usize).clamp(2, 180);
                let mut points = Vec::with_capacity(steps + 1);
                for step in 0..=steps {
                    let angle =
                        (start_deg + sweep * (step as f64 / steps as f64)).to_radians();
                    points.push(Point::new(
                        center_px.x + radius_px * angle.cos() as f32,
                        // Screen y grows down, so the arc's y is negated.
                        center_px.y - radius_px * angle.sin() as f32,
                    ));
                }
                let path = Path::new(|builder| {
                    builder.move_to(points[0]);
                    for point in &points[1..] {
                        builder.line_to(*point);
                    }
                });
                frame.stroke(&path, stroke);
            }
            Geometry::Text { value, at, height } => {
                let size_px = (*height as f32 * scale).clamp(6.0, size.height * 0.5);
                frame.fill_text(CanvasText {
                    content: value.clone(),
                    position: map(at[0], at[1]),
                    color: ink,
                    size: iced::Pixels(size_px),
                    align_x: iced::advanced::text::Alignment::Center,
                    align_y: iced::alignment::Vertical::Center,
                    ..Default::default()
                });
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pid::symbols::SymbolLibrary;

    fn library() -> SymbolLibrary {
        SymbolLibrary::builtin()
    }

    /// Every category of the shipped library becomes a tab, and its label is
    /// what the tab reads: the panel and `assets/pid-symbols.json` cannot
    /// drift apart.
    #[test]
    fn tabs_come_from_the_symbol_library_categories() {
        let library = library();
        let labels: Vec<String> = library
            .categories()
            .iter()
            .map(|category| category.label.clone())
            .collect();
        assert!(
            labels.contains(&"设备".to_string()),
            "设备 分类必须存在，实得 {labels:?}"
        );
        assert!(
            labels.contains(&"阀门".to_string()),
            "阀门 分类必须存在，实得 {labels:?}"
        );
        assert!(
            labels.contains(&"仪表".to_string()),
            "仪表 分类必须存在（VAL-DRAW-001 的「至少三类」），实得 {labels:?}"
        );
    }

    /// The grid of a category holds exactly its symbols -- no filler cells,
    /// no dropped ones.
    #[test]
    fn a_categorys_grid_holds_exactly_its_symbols() {
        let library = library();
        let palette = PidPalette::default();
        let categories = library.categories();
        for (index, category) in categories.iter().enumerate() {
            let active = PidPalette { active: index };
            let shown = active_category(&active, categories).expect("分类存在");
            assert_eq!(shown.id, category.id);
            assert_eq!(
                shown.symbols.len(),
                category.symbols.len(),
                "{} 的格位数应等于符号数",
                category.label
            );
        }
        // The default tab is the first category.
        assert_eq!(
            active_category(&palette, categories).map(|c| c.id.as_str()),
            categories.first().map(|c| c.id.as_str())
        );
    }

    /// The active index is clamped, not dropped, when the library shrinks --
    /// a standard switch must not leave the palette on a tab that is gone.
    #[test]
    fn the_active_tab_is_clamped_to_the_library() {
        let palette = PidPalette { active: 9 };
        assert_eq!(palette.category_index(3), 2);
        assert_eq!(palette.category_index(1), 0);
        assert_eq!(palette.category_index(0), 0);
    }

    /// Every symbol in the shipped library has drawable geometry with a
    /// finite, non-degenerate box: no cell can come out empty.
    #[test]
    fn every_symbol_has_a_drawable_bounds() {
        let library = library();
        for category in library.categories() {
            for symbol in &category.symbols {
                let bounds = geometry_bounds(&symbol.geometry).unwrap_or_else(|| {
                    panic!("{}/{} 没有可绘制的几何", category.id, symbol.id)
                });
                assert!(
                    bounds.2 > bounds.0 || bounds.3 > bounds.1,
                    "{}/{} 的包围盒退化：{bounds:?}",
                    category.id,
                    symbol.id
                );
                for value in [bounds.0, bounds.1, bounds.2, bounds.3] {
                    assert!(value.is_finite(), "{}/{} 的包围盒含非有限值", category.id, symbol.id);
                }
            }
        }
    }

    /// Arcs contribute their circle's box, so a symbol drawn only of arcs
    /// still gets a sane fit.
    #[test]
    fn arc_bounds_use_the_full_circle() {
        let geometry = vec![Geometry::Arc {
            center: [10.0, 20.0],
            radius: 4.0,
            start_deg: 0.0,
            end_deg: 90.0,
        }];
        assert_eq!(geometry_bounds(&geometry), Some((6.0, 16.0, 14.0, 24.0)));
    }

    /// The fit leaves the padding on all four sides and never collapses.
    #[test]
    fn fit_rect_keeps_a_positive_inner_box() {
        let (x, y, w, h) = fit_rect(Size::new(CELL, CELL));
        assert_eq!((x, y), (CELL_PAD, CELL_PAD));
        assert!(w > 0.0 && h > 0.0);
        let (_, _, tiny_w, tiny_h) = fit_rect(Size::new(1.0, 1.0));
        assert!(tiny_w > 0.0 && tiny_h > 0.0);
    }
}
