//! A table of text cells for the detail half of the backup browser.
//!
//! What a backup holds is mostly tables: the objects a manifest names, the
//! tables of a dump with their row counts, the items of a drawing. The grid
//! shows one of them with a header that stays in place, columns as wide as
//! their content, a horizontal scroll bar when they do not fit the panel,
//! and a click on a column title to sort by it.
//!
//! Rows are all the same height, so only the ones in view (and a few spare)
//! are built; spaces stand in for the rest and keep the scroll bar honest. A
//! table of tens of thousands of rows therefore costs what a screenful does.
//!
//! The grid knows nothing of the panel around it: it takes a [`Table`], its
//! [`GridState`] and a function from [`GridMsg`] to the caller's message, so
//! the same grid can sit in a window of its own later.

use std::cmp::Ordering;
use std::ops::Range;
use std::sync::Arc;

use iced::widget::{button, column, container, keyed_column, pin, row, scrollable, text, Space};
use iced::{Element, Fill, Length, Theme};

use crate::io::pid_backup_browse::labels::natural_cmp;

/// Height of one row of cells.
pub const ROW_H: f32 = 18.0;
/// Height of the header.
pub const HEADER_H: f32 = 20.0;
/// Size of the text in the cells and the header.
const TEXT: f32 = 10.0;
/// Space left and right of a cell's text.
const CELL_PAD: f32 = 5.0;
/// The narrowest and the widest a column gets; longer text is cut with "…".
const COLUMN_MIN: f32 = 36.0;
const COLUMN_MAX: f32 = 420.0;
/// The widest a column of a table in file order gets: its cells are the
/// lines of a file, cut to rows beforehand, and are shown whole.
const LINE_COLUMN_MAX: f32 = 2400.0;
/// Rows built beyond either end of the view.
const SPARE_ROWS: usize = 6;
/// Rows built while the view has not said how tall it is. A view reports
/// itself only when its content does not fit it, so a table that never
/// reports is one that fits: this many rows are more than any view shows
/// (3600 px of them), and a longer table overflows and reports at once.
const UNREPORTED_ROWS: usize = 200;
/// Room in a title for the arrow that marks the sorted column.
const SORT_MARK_W: f32 = 12.0;

/// One column of a [`Table`].
#[derive(Debug, Clone, PartialEq)]
pub struct Column {
    /// The title in the header.
    pub title: String,
    /// Numbers: right-aligned, and sorted by value rather than by spelling.
    pub numeric: bool,
}

impl Column {
    /// A column of text.
    pub fn text(title: impl Into<String>) -> Self {
        Column {
            title: title.into(),
            numeric: false,
        }
    }

    /// A column of numbers.
    pub fn number(title: impl Into<String>) -> Self {
        Column {
            title: title.into(),
            numeric: true,
        }
    }
}

/// A table: titled columns and rows of one cell per column.
#[derive(Debug, Clone, PartialEq)]
pub struct Table {
    columns: Vec<Column>,
    rows: Vec<Vec<String>>,
    /// Width of each column, fitted to its title and its widest cell.
    widths: Vec<f32>,
    /// Whether a click on a title sorts. Off for a table whose order is the
    /// content, like the lines of a file.
    sortable: bool,
}

impl Table {
    /// A table of `rows` under `columns`. A row shorter than the header is
    /// read as ending in empty cells; cells beyond it are not shown.
    pub fn new(columns: Vec<Column>, rows: Vec<Vec<String>>) -> Self {
        let widths = column_widths(&columns, &rows, COLUMN_MAX);
        Table {
            columns,
            rows,
            widths,
            sortable: true,
        }
    }

    /// The same table with its rows kept in the order they were given, and
    /// its columns as wide as their longest cell: the lines of a file.
    pub fn in_file_order(mut self) -> Self {
        self.sortable = false;
        self.widths = column_widths(&self.columns, &self.rows, LINE_COLUMN_MAX);
        self
    }

    /// The columns, in display order.
    pub fn columns(&self) -> &[Column] {
        &self.columns
    }

    /// The rows, in the order they were given.
    pub fn rows(&self) -> &[Vec<String>] {
        &self.rows
    }

    /// The cell of `row` under column `column`; empty when the row is short.
    pub fn cell(&self, row: usize, column: usize) -> &str {
        self.rows
            .get(row)
            .and_then(|cells| cells.get(column))
            .map_or("", String::as_str)
    }

    /// Whether a click on a title sorts the rows.
    pub fn is_sortable(&self) -> bool {
        self.sortable
    }

    /// Width of all the columns together.
    pub fn width(&self) -> f32 {
        self.widths.iter().sum()
    }
}

/// How many cells of a column are measured for its width.
const MEASURED: usize = 6;

/// The widest of `texts` as the UI font draws it: measuring every cell of a
/// long table would cost more than drawing it, so the candidates are the few
/// longest by a cheap estimate, and only those are measured.
fn widest(texts: impl Iterator<Item = impl AsRef<str>>) -> f32 {
    // Wide (CJK) characters take about twice the room of a Latin one.
    let estimate = |text: &str| -> usize {
        text.chars()
            .map(|ch| if ch.is_ascii() { 1 } else { 2 })
            .sum()
    };
    let mut longest: Vec<(usize, String)> = Vec::new();
    for text in texts {
        let text = text.as_ref();
        let size = estimate(text);
        if longest.len() < MEASURED {
            longest.push((size, text.to_string()));
        } else if let Some(shortest) = longest.iter_mut().min_by_key(|(size, _)| *size) {
            if size > shortest.0 {
                *shortest = (size, text.to_string());
            }
        }
    }
    longest
        .iter()
        .map(|(_, text)| crate::ui::style::text_metrics::width(text, TEXT, false))
        .fold(0.0, f32::max)
}

fn column_widths(columns: &[Column], rows: &[Vec<String>], widest_column: f32) -> Vec<f32> {
    columns
        .iter()
        .enumerate()
        .map(|(at, column)| {
            let title =
                crate::ui::style::text_metrics::width(&column.title, TEXT, false) + SORT_MARK_W;
            let cells = widest(rows.iter().filter_map(|row| row.get(at)));
            (title.max(cells) + 2.0 * CELL_PAD).clamp(COLUMN_MIN, widest_column)
        })
        .collect()
}

/// What the user did to a grid.
#[derive(Debug, Clone, PartialEq)]
pub enum GridMsg {
    /// A click on the title of this column.
    Sort(usize),
    /// The rows scrolled to this offset in a view this tall.
    Scrolled { x: f32, y: f32, height: f32 },
}

/// Where a grid is: how it is sorted and what part of it is in view.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct GridState {
    /// The sorted column, and whether descending.
    sort: Option<(usize, bool)>,
    /// The rows in sorted order, as indices into the table's rows; `None`
    /// while the table is in the order it was given.
    order: Option<Arc<Vec<u32>>>,
    /// How far the rows are scrolled, right and down.
    offset: (f32, f32),
    /// Height of the view the rows scroll in; zero until a frame said.
    height: f32,
}

impl GridState {
    /// The sorted column, and whether descending.
    pub fn sort(&self) -> Option<(usize, bool)> {
        self.sort
    }

    /// The table row shown at position `at` under the current sort.
    pub fn row_at(&self, at: usize) -> usize {
        self.order
            .as_ref()
            .and_then(|order| order.get(at))
            .map_or(at, |row| *row as usize)
    }
}

/// Apply `msg`. Sorting needs the table and scrolling does not, so the table
/// is asked for only when a title was clicked.
pub fn update(state: &mut GridState, msg: GridMsg, table: impl FnOnce() -> Option<Arc<Table>>) {
    match msg {
        GridMsg::Scrolled { x, y, height } => {
            state.offset = (x, y);
            state.height = height;
        }
        GridMsg::Sort(column) => {
            let Some(table) = table() else {
                return;
            };
            if !table.sortable || column >= table.columns.len() {
                return;
            }
            // Ascending, then descending, then back to the order given.
            state.sort = match state.sort {
                Some((sorted, false)) if sorted == column => Some((column, true)),
                Some((sorted, true)) if sorted == column => None,
                _ => Some((column, false)),
            };
            state.order = state
                .sort
                .map(|(column, descending)| Arc::new(sorted_order(&table, column, descending)));
        }
    }
}

/// The rows of `table` ordered by `column`. Equal cells keep the order they
/// were given in, both ways.
pub fn sorted_order(table: &Table, column: usize, descending: bool) -> Vec<u32> {
    let numeric = table
        .columns
        .get(column)
        .is_some_and(|column| column.numeric);
    let mut order: Vec<u32> = (0..table.rows.len() as u32).collect();
    order.sort_by(|left, right| {
        let (a, b) = (
            table.cell(*left as usize, column),
            table.cell(*right as usize, column),
        );
        let by_cell = if numeric {
            compare_numbers(a, b)
        } else {
            natural_cmp(a, b)
        };
        let by_cell = if descending {
            by_cell.reverse()
        } else {
            by_cell
        };
        by_cell.then(left.cmp(right))
    });
    order
}

/// Numbers by value; a cell that is no number sorts after every number.
fn compare_numbers(left: &str, right: &str) -> Ordering {
    let parse = |text: &str| -> Option<f64> {
        let digits: String = text
            .chars()
            .filter(|ch| !matches!(ch, ' ' | ',' | '\u{a0}'))
            .collect();
        digits.parse().ok()
    };
    match (parse(left), parse(right)) {
        (Some(a), Some(b)) => a.partial_cmp(&b).unwrap_or(Ordering::Equal),
        (Some(_), None) => Ordering::Less,
        (None, Some(_)) => Ordering::Greater,
        (None, None) => natural_cmp(left, right),
    }
}

/// The rows to build, of `rows`, for a view `height` tall scrolled `offset`
/// down: those in view and [`SPARE_ROWS`] either side. While the view has
/// not reported its height (zero), the first [`UNREPORTED_ROWS`]: never the
/// whole of a long table.
pub fn shown_rows(rows: usize, offset: f32, height: f32) -> Range<usize> {
    if height <= 0.0 {
        return 0..rows.min(UNREPORTED_ROWS);
    }
    // The rows scroll no further than their end, whatever a longer table
    // reported before.
    let offset = offset.clamp(0.0, (rows as f32 * ROW_H - height).max(0.0));
    let first = ((offset / ROW_H).floor() as usize).saturating_sub(SPARE_ROWS);
    let end = ((offset + height) / ROW_H).ceil() as usize + SPARE_ROWS;
    first.min(rows)..end.min(rows)
}

fn muted(theme: &Theme) -> iced::widget::text::Style {
    iced::widget::text::Style {
        color: Some(theme.palette().background.base.text.scale_alpha(0.6)),
    }
}

/// One cell: its text on one line, cut with "…" at the column's width.
fn cell<'a, Message: 'a>(content: String, width: f32, numeric: bool) -> Element<'a, Message> {
    let label = text(content)
        .size(TEXT)
        .width(Fill)
        .wrapping(iced::widget::text::Wrapping::None)
        .ellipsis(iced::advanced::text::Ellipsis::End)
        .align_x(if numeric {
            iced::alignment::Horizontal::Right
        } else {
            iced::alignment::Horizontal::Left
        });
    container(label)
        .width(Length::Fixed(width))
        .height(Length::Fixed(ROW_H))
        .padding([0.0, CELL_PAD])
        .align_y(iced::alignment::Vertical::Center)
        .into()
}

/// Draw `table`.
///
/// `key` names the table among the ones this grid shows in turn: a new key
/// starts the rows at their top again instead of where the last table was
/// scrolled to.
pub fn view<'a, Message: Clone + 'a>(
    table: &Table,
    state: &GridState,
    key: u64,
    on: impl Fn(GridMsg) -> Message + Clone + 'a,
) -> Element<'a, Message> {
    // The header: a title per column. It stays put while the rows scroll
    // down, and follows them sideways by the offset they report.
    let mut titles = row![].height(Length::Fixed(HEADER_H));
    for (at, (column, width)) in table.columns.iter().zip(&table.widths).enumerate() {
        let mark = match state.sort {
            Some((sorted, descending)) if sorted == at => {
                if descending {
                    " ▼"
                } else {
                    " ▲"
                }
            }
            _ => "",
        };
        let label = text(format!("{}{mark}", column.title))
            .size(TEXT)
            .style(muted)
            .width(Fill)
            .wrapping(iced::widget::text::Wrapping::None)
            .ellipsis(iced::advanced::text::Ellipsis::End)
            .align_x(if column.numeric {
                iced::alignment::Horizontal::Right
            } else {
                iced::alignment::Horizontal::Left
            });
        let title = button(label)
            .padding([3.0, CELL_PAD])
            .width(Length::Fixed(*width))
            .height(Length::Fixed(HEADER_H))
            .style(button::text);
        titles = titles.push(if table.sortable {
            title.on_press(on(GridMsg::Sort(at)))
        } else {
            title
        });
    }
    // A scrollable that never scrolls (its content is as tall as it is)
    // clips the titles pushed left and keeps the pointer off the clipped
    // part, which a plain container would not.
    let header = scrollable(
        pin(titles)
            .x(-state.offset.0)
            .height(Length::Fixed(HEADER_H)),
    )
    .direction(scrollable::Direction::Vertical(
        scrollable::Scrollbar::hidden(),
    ))
    .width(Fill)
    .height(Length::Fixed(HEADER_H));

    let shown = shown_rows(table.rows.len(), state.offset.1, state.height);
    let mut body = column![].width(Length::Fixed(table.width()));
    if shown.start > 0 {
        body = body.push(Space::new().height(shown.start as f32 * ROW_H));
    }
    for at in shown.clone() {
        let source = state.row_at(at);
        let mut line = row![].height(Length::Fixed(ROW_H));
        for (column, (definition, width)) in table.columns.iter().zip(&table.widths).enumerate() {
            line = line.push(cell(
                table.cell(source, column).to_string(),
                *width,
                definition.numeric,
            ));
        }
        body = body.push(line);
    }
    let below = table.rows.len() - shown.end;
    if below > 0 {
        body = body.push(Space::new().height(below as f32 * ROW_H));
    }
    let rows = scrollable(body)
        .direction(scrollable::Direction::Both {
            vertical: scrollable::Scrollbar::default(),
            horizontal: scrollable::Scrollbar::default(),
        })
        .on_scroll(move |viewport| {
            let offset = viewport.absolute_offset();
            on(GridMsg::Scrolled {
                x: offset.x,
                y: offset.y,
                height: viewport.bounds().height,
            })
        })
        .width(Fill)
        .height(Fill);
    // Keyed, so the rows of another table start from a scroll state of
    // their own.
    let rows = keyed_column([(key, Element::from(rows))])
        .width(Fill)
        .height(Fill);
    column![header, rows].width(Fill).height(Fill).into()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn table() -> Table {
        Table::new(
            vec![Column::text("Table"), Column::number("Rows")],
            vec![
                vec!["T_PIPERUN".to_string(), "4686".to_string()],
                vec!["t_drawing".to_string(), "53".to_string()],
                vec!["T_MODELITEM".to_string(), "47124".to_string()],
                vec!["T_NOTE".to_string(), "800".to_string()],
            ],
        )
    }

    fn names(table: &Table, state: &GridState) -> Vec<String> {
        (0..table.rows().len())
            .map(|at| table.cell(state.row_at(at), 0).to_string())
            .collect()
    }

    fn sort(state: &mut GridState, table: &Table, column: usize) {
        update(state, GridMsg::Sort(column), || {
            Some(Arc::new(table.clone()))
        });
    }

    #[test]
    fn a_title_sorts_ascending_then_descending_then_not_at_all() {
        let table = table();
        let mut state = GridState::default();
        assert_eq!(
            names(&table, &state),
            ["T_PIPERUN", "t_drawing", "T_MODELITEM", "T_NOTE"]
        );
        // Text sorts without regard to case.
        sort(&mut state, &table, 0);
        assert_eq!(state.sort(), Some((0, false)));
        assert_eq!(
            names(&table, &state),
            ["t_drawing", "T_MODELITEM", "T_NOTE", "T_PIPERUN"]
        );
        sort(&mut state, &table, 0);
        assert_eq!(state.sort(), Some((0, true)));
        assert_eq!(
            names(&table, &state),
            ["T_PIPERUN", "T_NOTE", "T_MODELITEM", "t_drawing"]
        );
        sort(&mut state, &table, 0);
        assert_eq!(state.sort(), None);
        assert_eq!(
            names(&table, &state),
            ["T_PIPERUN", "t_drawing", "T_MODELITEM", "T_NOTE"]
        );
    }

    #[test]
    fn a_numeric_column_sorts_by_value_and_another_title_starts_over() {
        let table = table();
        let mut state = GridState::default();
        sort(&mut state, &table, 0);
        // A different title: ascending by that column, not the next step of
        // the old one. As text 47124 would come before 53.
        sort(&mut state, &table, 1);
        assert_eq!(state.sort(), Some((1, false)));
        assert_eq!(
            names(&table, &state),
            ["t_drawing", "T_NOTE", "T_PIPERUN", "T_MODELITEM"]
        );
        sort(&mut state, &table, 1);
        assert_eq!(
            names(&table, &state),
            ["T_MODELITEM", "T_PIPERUN", "T_NOTE", "t_drawing"]
        );
    }

    #[test]
    fn equal_cells_keep_the_order_they_came_in_both_ways() {
        let table = Table::new(
            vec![Column::text("Schema"), Column::text("Table")],
            vec![
                vec!["B".to_string(), "first".to_string()],
                vec!["A".to_string(), "second".to_string()],
                vec!["B".to_string(), "third".to_string()],
                vec!["A".to_string(), "fourth".to_string()],
            ],
        );
        let second = |order: Vec<u32>| -> Vec<&str> {
            order
                .iter()
                .map(|row| table.cell(*row as usize, 1))
                .collect()
        };
        assert_eq!(
            second(sorted_order(&table, 0, false)),
            ["second", "fourth", "first", "third"]
        );
        assert_eq!(
            second(sorted_order(&table, 0, true)),
            ["first", "third", "second", "fourth"]
        );
    }

    #[test]
    fn text_sorts_the_way_a_person_reads_it() {
        assert_eq!(natural_cmp("P-2", "P-10"), Ordering::Less);
        assert_eq!(natural_cmp("P-10", "P-2"), Ordering::Greater);
        assert_eq!(natural_cmp("mm-222", "MM-0222A"), Ordering::Less);
        assert_eq!(natural_cmp("D07", "D7"), Ordering::Less);
        assert_eq!(natural_cmp("abc", "ABC"), Ordering::Greater);
        assert_eq!(natural_cmp("", "a"), Ordering::Less);
        // A number among text: after the numbers, by its spelling.
        assert_eq!(compare_numbers("12", "n/a"), Ordering::Less);
        assert_eq!(compare_numbers("1 024", "999"), Ordering::Greater);
    }

    #[test]
    fn a_table_that_is_its_own_order_does_not_sort_and_a_bad_column_is_ignored() {
        let lines = table().in_file_order();
        let mut state = GridState::default();
        sort(&mut state, &lines, 0);
        assert_eq!(state.sort(), None);
        let table = table();
        sort(&mut state, &table, 9);
        assert_eq!(state.sort(), None);
        // A grid whose table has gone leaves the state alone.
        update(&mut state, GridMsg::Sort(0), || None);
        assert_eq!(state, GridState::default());
    }

    #[test]
    fn only_the_rows_in_view_are_built() {
        // 10 000 rows, a 180 px view: ten rows and the spare ones.
        assert_eq!(shown_rows(10_000, 0.0, 180.0), 0..16);
        assert_eq!(shown_rows(10_000, 1800.0, 180.0), 94..116);
        // Scrolled past the end by a longer table before: clamped.
        assert_eq!(shown_rows(100, 1e9, 180.0), 84..100);
        assert_eq!(shown_rows(3, 0.0, 180.0), 0..3);
        assert_eq!(shown_rows(0, 0.0, 180.0), 0..0);
        // The view has not reported its height: a table that fits it never
        // will, so every row of a short one is built, and of a long one a
        // bounded first part, never the whole.
        assert_eq!(shown_rows(45, 0.0, 0.0), 0..45);
        assert_eq!(shown_rows(10_000, 0.0, 0.0), 0..UNREPORTED_ROWS);
    }

    #[test]
    fn a_column_is_as_wide_as_its_widest_cell_within_limits() {
        let table = Table::new(
            vec![Column::text("A"), Column::text("B")],
            vec![
                vec!["x".to_string(), "y".repeat(400)],
                vec![
                    "a much longer cell than the title".to_string(),
                    String::new(),
                ],
            ],
        );
        assert!(table.widths[0] > COLUMN_MIN && table.widths[0] < COLUMN_MAX);
        assert_eq!(table.widths[1], COLUMN_MAX);
        assert_eq!(table.width(), table.widths[0] + COLUMN_MAX);
        // A short row reads as ending in empty cells.
        assert_eq!(table.cell(0, 5), "");
        assert_eq!(table.cell(9, 0), "");
    }

    #[test]
    fn the_lines_of_a_file_are_as_wide_as_the_longest_of_them() {
        // A row of a text file is up to 160 columns: far past what a column
        // of a data table gets, and shown whole all the same.
        let longest = "<Attribute name=\"x\" ".repeat(8);
        let lines = Table::new(
            vec![Column::number("#"), Column::text("Text")],
            vec![
                vec!["1".to_string(), "<?xml?>".to_string()],
                vec!["2".to_string(), longest.clone()],
                // Fewer characters, but wide ones: the estimate that picks
                // the cells to measure counts them double.
                vec!["3".to_string(), "图".repeat(90)],
            ],
        );
        assert_eq!(lines.widths[1], COLUMN_MAX);
        let lines = lines.in_file_order();
        let needed = [longest.as_str(), &"图".repeat(90)]
            .iter()
            .map(|text| crate::ui::style::text_metrics::width(text, TEXT, false))
            .fold(0.0, f32::max);
        assert!(needed > COLUMN_MAX, "{needed}");
        assert_eq!(lines.widths[1], needed + 2.0 * CELL_PAD);
        assert!(!lines.is_sortable());
    }

    #[derive(Debug, Clone, PartialEq)]
    struct Told(GridMsg);

    /// The grid as it is drawn: the header takes the click that sorts, and
    /// the first frame reports the view the rows scroll in.
    #[test]
    fn the_drawn_title_sorts_and_the_first_frame_reports_the_view() {
        let _guard = crate::headless_ui_lock();
        let rows: Vec<Vec<String>> = (0..500)
            .map(|at| vec![format!("row {at:03}"), (500 - at).to_string()])
            .collect();
        let table = Table::new(vec![Column::text("Name"), Column::number("Count")], rows);
        let mut state = GridState::default();
        let theme = crate::ui::style::fusion_theme::fusion_black();

        let frame = |state: &GridState, click: Option<&str>| -> (Vec<GridMsg>, Vec<bool>) {
            let mut ui = iced_test::simulator::Simulator::with_size(
                iced::Settings::default(),
                (300.0, 200.0),
                view(&table, state, 1, Told),
            );
            if let Some(label) = click {
                let bounds = ui
                    .find(label)
                    .unwrap_or_else(|error| panic!("{label} is drawn: {error:?}"))
                    .visible_bounds()
                    .expect("the title is on screen");
                ui.point_at(bounds.center());
                let _ = ui.simulate(iced_test::simulator::click());
            }
            let _ = ui.snapshot(&theme).expect("the grid draws headless");
            let drawn = ["row 000", "row 499", "row 250"]
                .iter()
                .map(|label| ui.find(*label).is_ok())
                .collect();
            (ui.into_messages().map(|Told(msg)| msg).collect(), drawn)
        };

        // The first frame: the top of the table is built, the rest is not,
        // and the view says how tall it is.
        let (messages, drawn) = frame(&state, None);
        assert_eq!(drawn, [true, false, false]);
        let reported = messages
            .iter()
            .find_map(|msg| match msg {
                GridMsg::Scrolled { height, .. } => Some(*height),
                _ => None,
            })
            .expect("the first frame reports the view");
        assert!(
            reported > 100.0 && reported <= 200.0 - HEADER_H,
            "{reported}"
        );
        for msg in messages {
            update(&mut state, msg, || Some(Arc::new(table.clone())));
        }

        // A click on the drawn title of the numeric column sorts by it.
        let (messages, _) = frame(&state, Some("Count"));
        assert!(messages.contains(&GridMsg::Sort(1)), "{messages:?}");
        for msg in messages {
            update(&mut state, msg, || Some(Arc::new(table.clone())));
        }
        assert_eq!(state.sort(), Some((1, false)));
        // Ascending by count, the last row given comes first: it is the one
        // built at the top now.
        let (_, drawn) = frame(&state, None);
        assert_eq!(drawn, [false, true, false]);

        // Scrolled to the middle, the middle is what is built.
        update(
            &mut state,
            GridMsg::Scrolled {
                x: 0.0,
                y: 250.0 * ROW_H,
                height: reported,
            },
            || None,
        );
        let middle = state.row_at(250);
        assert_eq!(table.cell(middle, 0), "row 249");
    }
}
