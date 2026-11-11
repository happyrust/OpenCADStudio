//! How the drawings of the backup fare when they are read and imported.
//!
//! The details of the backup itself -- the root of the tree -- end with a
//! check of its drawings: a button, and once it was pressed a table of every
//! drawing with what reading it for its items and importing it gave, the
//! ones that failed with the reason. A plant's drawings take most of a
//! minute, so the check runs off the UI thread and says how far it has
//! come: while it runs the panel is drawn anew a few times a second
//! ([`Section::in_progress`]).
//!
//! The details of a drawing that has a file have a button of their own,
//! beside the one that opens it. It imports the drawing as opening it would,
//! without opening it, and shows in its place what the import says --
//! entities drawn, source records left undrawn, the border template and the
//! symbol library that were found. The parser's report of the file follows
//! at the end of the details, line by line. A drawing whose items do not
//! read has the button too: that is the drawing one wants to know more
//! about.
//!
//! The section hangs no rows in the tree. Its nodes are the keys what it
//! read is kept under, which the buttons activate.

use std::borrow::Cow;
use std::sync::Arc;
use std::time::Duration;

use super::grid::{Column, Table};
use super::section::{Cx, Event, EventKind, Section, SectionData, SectionLoad};
use super::{DetailLine, NodeId};
use crate::io::pid_backup_browse::diagnostics::{
    check_all, import_report, Checked, ImportReport, Imported,
};
use crate::io::pid_backup_browse::manifest::HIDDEN;
use crate::io::pid_backup_browse::BackupIndex;
use crate::{t, tf};

/// The diagnoses of the backup's drawings.
pub(super) struct DiagnosticsSection;

/// The address of the check of every drawing: the first index of the
/// section's nodes says which of its two kinds a node is.
const CHECK: usize = 0;
/// The address of one drawing's import is this, then the drawing.
const DRAWING: usize = 1;

/// One drawing's import as the detail half shows it.
struct ReportView {
    report: ImportReport,
    /// The lines of the parser's report as a table, when there is one.
    lines: Option<Arc<Table>>,
}

fn report_view(report: ImportReport) -> ReportView {
    let lines = report.parse_report.as_ref().ok().map(|text| {
        Arc::new(
            Table::new(
                vec![
                    Column::number(t!("Line #")),
                    Column::text(t!("Text content")),
                ],
                text.rows
                    .iter()
                    .map(|(number, text)| {
                        vec![
                            number.map(|number| number.to_string()).unwrap_or_default(),
                            text.clone(),
                        ]
                    })
                    .collect(),
            )
            .in_file_order(),
        )
    });
    ReportView { report, lines }
}

/// The check of every drawing as the detail half shows it.
struct CheckView {
    drawings: usize,
    /// Drawings the backup has no file of, whose items did not read, that
    /// did not import, and that came through with nothing left out.
    without_file: usize,
    unread: usize,
    not_imported: usize,
    clean: usize,
    /// Entities drawn and source records left undrawn, over the drawings.
    drawn: usize,
    missing: usize,
    took: Duration,
    table: Arc<Table>,
}

fn seconds(duration: Duration) -> String {
    format!("{:.1} s", duration.as_secs_f64())
}

fn field(label: Cow<'static, str>, value: impl ToString) -> DetailLine {
    DetailLine::Field(label.into_owned(), value.to_string())
}

fn marker(text: Cow<'static, str>) -> DetailLine {
    DetailLine::Marker(text.into_owned())
}

fn heading(text: Cow<'static, str>) -> DetailLine {
    DetailLine::Heading(text.into_owned())
}

fn check_view(checked: Vec<Checked>) -> CheckView {
    let mut view = CheckView {
        drawings: checked.len(),
        without_file: 0,
        unread: 0,
        not_imported: 0,
        clean: 0,
        drawn: 0,
        missing: 0,
        took: Duration::ZERO,
        table: Arc::new(Table::new(Vec::new(), Vec::new())),
    };
    let mut rows = Vec::with_capacity(checked.len());
    for drawing in checked {
        let Some(diagnosis) = drawing.diagnosis else {
            view.without_file += 1;
            rows.push(vec![
                drawing.name,
                t!("Registered but missing").into_owned(),
                String::new(),
                String::new(),
                String::new(),
                String::new(),
                drawing.group,
            ]);
            continue;
        };
        let took = diagnosis.items_time + diagnosis.import_time;
        view.took += took;
        view.clean += usize::from(diagnosis.is_clean());
        let (items, read) = match &diagnosis.items {
            Ok(items) => (items.items.to_string(), String::new()),
            Err(why) => {
                view.unread += 1;
                (String::new(), why.clone())
            }
        };
        let (drawn, missing, imported) = match &diagnosis.import {
            Ok(import) => {
                view.drawn += import.drawn;
                view.missing += import.missing;
                (
                    import.drawn.to_string(),
                    import.missing.to_string(),
                    String::new(),
                )
            }
            Err(why) => {
                view.not_imported += 1;
                (String::new(), String::new(), why.clone())
            }
        };
        // What went wrong, the read first; nothing for a drawing both
        // reads took.
        let outcome = match (read.is_empty(), imported.is_empty()) {
            (true, true) => String::new(),
            (false, true) => read,
            (true, false) => imported,
            (false, false) => format!("{read}; {imported}"),
        };
        rows.push(vec![
            drawing.name,
            outcome,
            items,
            drawn,
            missing,
            format!("{:.0}", took.as_secs_f64() * 1000.0),
            drawing.group,
        ]);
    }
    view.table = Arc::new(Table::new(
        vec![
            Column::text(t!("Drawing name")),
            Column::text(t!("What went wrong")),
            Column::number(t!("Item count")),
            Column::number(t!("Entities drawn")),
            Column::number(t!("Source records not drawn")),
            Column::number(t!("Time (ms)")),
            Column::text(t!("Group path")),
        ],
        rows,
    ));
    view
}

/// What importing a drawing gave, as detail lines: the import's own words
/// when it failed.
fn import_lines(import: &Result<Imported, String>, took: Duration, lines: &mut Vec<DetailLine>) {
    match import {
        Err(why) => lines.push(DetailLine::Marker(why.clone())),
        Ok(import) => {
            lines.push(field(t!("Entities drawn"), import.drawn));
            lines.push(field(t!("Source records decoded"), import.decoded));
            lines.push(field(t!("Source records not drawn"), import.missing));
            lines.push(field(
                t!("Sheet layers"),
                if import.sheet_layers_off == 0 {
                    import.sheet_layers.to_string()
                } else {
                    tf!(
                        "{}, {} of them off",
                        import.sheet_layers,
                        import.sheet_layers_off
                    )
                    .into_owned()
                },
            ));
            match &import.border_template {
                Some(template) => {
                    lines.push(field(t!("Border template"), template));
                    lines.push(field(
                        t!("Border template entities"),
                        if import.template_missing == 0 {
                            import.template_entities.to_string()
                        } else {
                            tf!(
                                "{}, {} record(s) of it not drawn",
                                import.template_entities,
                                import.template_missing
                            )
                            .into_owned()
                        },
                    ));
                }
                None => lines.push(field(
                    t!("Border template"),
                    t!("none found; the page border is a stand-in"),
                )),
            }
            lines.push(field(
                t!("Symbol bodies"),
                tf!(
                    "{} from the drawing's cache, {} from the library",
                    import.cache_bodies,
                    import.library_bodies
                ),
            ));
            if import.symbol_library.is_empty() {
                lines.push(field(t!("Symbol library"), t!("none found")));
            }
            for library in &import.symbol_library {
                lines.push(field(t!("Symbol library"), library.display()));
            }
            lines.push(field(
                t!("Drawing unit"),
                match &import.unit {
                    Some(unit) => Cow::Borrowed(unit.as_str()),
                    None => t!("none stated; the metre is assumed"),
                },
            ));
            if import.style_tables_failed {
                lines.push(marker(t!(
                    "A style table did not read: line work or lettering is on fallbacks."
                )));
            }
        }
    }
    lines.push(field(t!("Imported in"), seconds(took)));
}

/// Whether the backup has a drawing to check.
fn has_files(index: &BackupIndex) -> bool {
    index.drawings.iter().any(|entry| entry.pid_path.is_some())
}

/// Whether drawing `drawing` of the backup has a file to import.
fn has_file(index: &BackupIndex, drawing: usize) -> bool {
    index
        .drawings
        .get(drawing)
        .is_some_and(|entry| entry.pid_path.is_some())
}

/// The check of every drawing, under the backup's own details.
fn check_lines(cx: &Cx<'_>, lines: &mut Vec<DetailLine>) {
    let check = cx.node([CHECK]);
    let drawings = cx.index.drawings.len();
    lines.push(heading(t!("Check of the drawings")));
    match cx.load(&check) {
        None => {
            lines.push(marker(t!(
                "Reads every drawing for its items and imports it, and lists what failed and why."
            )));
            lines.push(marker(tf!(
                "{} drawing(s); a plant of fifty takes most of a minute.",
                drawings
            )));
            lines.push(DetailLine::Button(
                t!("Check every drawing").into_owned(),
                check,
            ));
        }
        Some(SectionLoad::Loading) => lines.push(marker(tf!(
            "Checking: {} of {} drawing(s) done…",
            cx.index.check_progress.done(),
            drawings
        ))),
        Some(SectionLoad::Failed(error)) => lines.push(DetailLine::Marker(error.clone())),
        Some(SectionLoad::Ready(data)) => {
            let Some(view) = data.get::<CheckView>() else {
                return;
            };
            // Few lines: with their heading they stay beside the table
            // however long the backup's own details are.
            lines.push(field(
                t!("Drawings checked"),
                tf!(
                    "{}, {} of them with nothing left out",
                    view.drawings,
                    view.clean
                ),
            ));
            lines.push(field(
                t!("Failed or missing"),
                tf!(
                    "items did not read: {}; did not import: {}; registered but missing: {}",
                    view.unread,
                    view.not_imported,
                    view.without_file
                ),
            ));
            lines.push(field(
                t!("Imported entities"),
                tf!(
                    "{} entities drawn, {} source record(s) not drawn",
                    view.drawn,
                    view.missing
                ),
            ));
            lines.push(field(t!("Checked in"), seconds(view.took)));
            lines.push(DetailLine::Table(Arc::clone(&view.table)));
        }
    }
}

/// What was read for the button of `drawing`, once it is there.
fn report<'a>(cx: &Cx<'a>, drawing: usize) -> Option<&'a ReportView> {
    match cx
        .state
        .section_data
        .get(&(cx.slot, NodeId::Section(cx.slot, vec![DRAWING, drawing])))
    {
        Some(SectionLoad::Ready(data)) => data.get::<ReportView>(),
        _ => None,
    }
}

/// One drawing's import, right under the drawing's own first lines: the
/// offer, or how it went.
fn import_of_drawing(cx: &Cx<'_>, drawing: usize, lines: &mut Vec<DetailLine>) {
    let node = cx.node([DRAWING, drawing]);
    lines.push(heading(t!("Importing it")));
    match cx.load(&node) {
        None => {
            lines.push(marker(t!(
                "Imports the drawing as opening it would, without opening it: what was drawn, what was left out, and the parser's report of the file."
            )));
            lines.push(DetailLine::Button(
                t!("Import and diagnose").into_owned(),
                node,
            ));
        }
        Some(SectionLoad::Loading) => {
            lines.push(marker(t!("Reading and importing the drawing…")));
        }
        Some(SectionLoad::Failed(error)) => lines.push(DetailLine::Marker(error.clone())),
        Some(SectionLoad::Ready(_)) => {
            if let Some(view) = report(cx, drawing) {
                import_lines(&view.report.import, view.report.import_time, lines);
            }
        }
    }
}

/// The parser's report of an imported drawing's file, at the end of the
/// drawing's details: its lines are a table, which stands last.
fn report_of_drawing(cx: &Cx<'_>, drawing: usize, lines: &mut Vec<DetailLine>) {
    let Some(view) = report(cx, drawing) else {
        return;
    };
    lines.push(heading(t!("Parser's report of the file")));
    match (&view.report.parse_report, &view.lines) {
        (Ok(text), Some(table)) => {
            lines.push(field(t!("Line count"), text.line_count));
            if text.hidden > 0 {
                lines.push(marker(tf!(
                    "{} credential-like value(s) in the text are hidden and shown as {}",
                    text.hidden,
                    HIDDEN
                )));
            }
            if !table.rows().is_empty() {
                lines.push(DetailLine::Table(Arc::clone(table)));
            }
        }
        (Err(why), _) => lines.push(DetailLine::Marker(why.clone())),
        (Ok(_), None) => {}
    }
}

fn nothing_to_read() -> String {
    t!("this node has nothing to read").into_owned()
}

impl Section for DiagnosticsSection {
    fn early_details(&self, cx: &Cx<'_>, id: &NodeId, lines: &mut Vec<DetailLine>) {
        if let NodeId::Drawing(drawing) = id {
            if has_file(cx.index, *drawing) {
                import_of_drawing(cx, *drawing, lines);
            }
        }
    }

    fn details(&self, cx: &Cx<'_>, id: &NodeId, lines: &mut Vec<DetailLine>) {
        match id {
            NodeId::Root if has_files(cx.index) => check_lines(cx, lines),
            NodeId::Drawing(drawing) => report_of_drawing(cx, *drawing, lines),
            _ => {}
        }
    }

    /// Both are started by their button, which activates the section's
    /// node: once each, until the backup is opened anew.
    fn wants(&self, cx: &Cx<'_>, event: &Event<'_>) -> Option<NodeId> {
        let asked = match cx.own(event.node)? {
            [CHECK] => has_files(cx.index),
            [DRAWING, drawing] => has_file(cx.index, *drawing),
            _ => false,
        };
        (asked && event.kind == EventKind::Activated).then(|| event.node.clone())
    }

    fn load(&self, index: &BackupIndex, node: &NodeId) -> Result<SectionData, String> {
        let NodeId::Section(_, path) = node else {
            return Err(nothing_to_read());
        };
        match path[..] {
            [CHECK] => Ok(SectionData::new(check_view(check_all(index)))),
            [DRAWING, drawing] => {
                let file = index
                    .drawings
                    .get(drawing)
                    .and_then(|entry| entry.pid_path.as_deref())
                    .ok_or_else(nothing_to_read)?;
                Ok(SectionData::new(report_view(import_report(file))))
            }
            _ => Err(nothing_to_read()),
        }
    }

    /// The check counts the drawings it has done while it runs, and the
    /// line that says so is only as fresh as the panel's last drawing.
    fn in_progress(&self, cx: &Cx<'_>) -> bool {
        matches!(cx.load(&cx.node([CHECK])), Some(SectionLoad::Loading))
            && !cx.index.check_progress.finished()
    }
}

#[cfg(test)]
mod tests {
    use super::super::detail_lines;
    use super::super::tests::ready;
    use super::*;
    use crate::app::Message;
    use crate::io::pid_backup_browse::diagnostics::{Diagnosis, ItemsRead};
    use crate::io::pid_backup_browse::text_file::TextFile;
    use crate::ui::dock::Chrome;
    use crate::ui::window::pid_backup_browser::{
        detail_rows, update, view, Effect, PidBackupBrowser, PidBackupMsg,
    };

    /// The node the button in the details of `id` activates: the key what
    /// it starts is kept under.
    fn button_node(state: &PidBackupBrowser, id: &NodeId) -> NodeId {
        detail_lines(state, id)
            .into_iter()
            .find_map(|line| match line {
                DetailLine::Button(_, node) => Some(node),
                _ => None,
            })
            .unwrap_or_else(|| panic!("the details of {id:?} offer a button"))
    }

    /// The lines of `id` under `heading`: up to the node's own next line
    /// (`until`, when given) or to the end.
    fn lines_between(
        state: &PidBackupBrowser,
        id: &NodeId,
        heading: &str,
        until: Option<&str>,
    ) -> Vec<(String, String)> {
        let rows = detail_rows(state, id);
        let at = rows
            .iter()
            .position(|(key, _)| key == heading)
            .unwrap_or_else(|| panic!("the details of {id:?} have \"{heading}\": {rows:?}"));
        let end = until.map_or(rows.len(), |until| {
            rows.iter()
                .position(|(key, _)| key == until)
                .unwrap_or_else(|| panic!("the details of {id:?} have \"{until}\": {rows:?}"))
        });
        rows[at + 1..end].to_vec()
    }

    /// The lines of `id` from `heading` on, the heading left out.
    fn lines_under(state: &PidBackupBrowser, id: &NodeId, heading: &str) -> Vec<(String, String)> {
        lines_between(state, id, heading, None)
    }

    /// What the details of drawing `id` say of its import: the lines
    /// between the heading and the first column of the drawing table (the
    /// test backup's drawings have one, `id`).
    fn shown_import(state: &PidBackupBrowser, id: &NodeId) -> Vec<(String, String)> {
        lines_between(state, id, &t!("Importing it"), Some("id"))
    }

    /// The backup's lines under the check's heading.
    fn shown_check(state: &PidBackupBrowser) -> Vec<(String, String)> {
        lines_under(state, &NodeId::Root, &t!("Check of the drawings"))
    }

    /// Hand the panel what the worker thread would have read about `node`.
    fn loaded(state: &mut PidBackupBrowser, node: &NodeId, result: Result<SectionData, String>) {
        let NodeId::Section(slot, _) = node else {
            unreachable!("a section's node");
        };
        let generation = state.generation;
        update(
            state,
            PidBackupMsg::SectionLoaded {
                generation,
                section: *slot,
                node: node.clone(),
                result,
            },
        );
    }

    fn load_of(state: &PidBackupBrowser, node: &NodeId) -> Effect {
        let NodeId::Section(slot, _) = node else {
            unreachable!("a section's node");
        };
        Effect::LoadSection {
            generation: state.generation,
            section: *slot,
            node: node.clone(),
        }
    }

    fn value<'a>(rows: &'a [(String, String)], key: &str) -> Option<&'a str> {
        rows.iter()
            .find(|(found, _)| found == key)
            .map(|(_, value)| value.as_str())
    }

    fn fine() -> Diagnosis {
        Diagnosis {
            items: Ok(ItemsRead {
                items: 19,
                types: 4,
                attribute_records: 182,
                undecoded_records: 0,
            }),
            items_time: Duration::from_millis(40),
            import: Ok(Imported {
                drawn: 100,
                decoded: 90,
                ..Imported::default()
            }),
            import_time: Duration::from_millis(1260),
        }
    }

    /// What a check of the test backup's drawings would find.
    fn found() -> Vec<Checked> {
        let mut unread = fine();
        unread.items = Err("cannot read b.pid".to_string());
        unread.import = Err("no sheet".to_string());
        let mut undrawn = fine();
        undrawn.import = Ok(Imported {
            drawn: 240,
            missing: 3,
            ..Imported::default()
        });
        vec![
            Checked {
                name: "A-101".to_string(),
                group: "SQPlant / 00".to_string(),
                diagnosis: Some(fine()),
            },
            Checked {
                name: "B-202".to_string(),
                group: "SQPlant / 00 / 01".to_string(),
                diagnosis: Some(unread),
            },
            Checked {
                name: "C-303".to_string(),
                group: "SQPlant / 00".to_string(),
                diagnosis: None,
            },
            Checked {
                name: "D-404".to_string(),
                group: "SQPlant / 02".to_string(),
                diagnosis: Some(undrawn),
            },
        ]
    }

    #[test]
    fn the_root_offers_a_check_of_the_drawings_that_starts_from_its_button() {
        let mut state = ready();
        // Selecting the backup reads nothing: its details say what the
        // check is, and offer the button.
        assert_eq!(
            update(&mut state, PidBackupMsg::Select(NodeId::Root)),
            Effect::None
        );
        assert_eq!(
            shown_check(&state),
            [
                (
                    t!("Reads every drawing for its items and imports it, and lists what failed and why.")
                        .into_owned(),
                    String::new()
                ),
                (
                    tf!("{} drawing(s); a plant of fifty takes most of a minute.", 3).into_owned(),
                    String::new()
                ),
                (t!("Check every drawing").into_owned(), String::new()),
            ]
        );
        // The button activates the check's node, which starts the check and
        // the clock that keeps its count on screen.
        let check = button_node(&state, &NodeId::Root);
        assert_eq!(
            update(&mut state, PidBackupMsg::Activate(check.clone())),
            Effect::Batch(vec![load_of(&state, &check), Effect::Tick])
        );
        // While it runs: how far it is, and no button to start it twice.
        assert_eq!(
            shown_check(&state),
            [(
                tf!("Checking: {} of {} drawing(s) done…", 0, 3).into_owned(),
                String::new()
            )]
        );
        assert_eq!(
            update(&mut state, PidBackupMsg::Activate(check.clone())),
            Effect::None
        );

        loaded(
            &mut state,
            &check,
            Ok(SectionData::new(check_view(found()))),
        );
        let rows = shown_check(&state);
        assert_eq!(
            rows[..4],
            [
                (
                    t!("Drawings checked").into_owned(),
                    tf!("{}, {} of them with nothing left out", 4, 1).into_owned()
                ),
                (
                    t!("Failed or missing").into_owned(),
                    tf!(
                        "items did not read: {}; did not import: {}; registered but missing: {}",
                        1,
                        1,
                        1
                    )
                    .into_owned()
                ),
                (
                    t!("Imported entities").into_owned(),
                    tf!("{} entities drawn, {} source record(s) not drawn", 340, 3).into_owned()
                ),
                // Three drawings were read, 1.3 s each.
                (t!("Checked in").into_owned(), "3.9 s".to_string()),
            ]
        );
        let table = rows
            .iter()
            .position(|(titles, _)| titles.contains(" | "))
            .expect("the drawings are a table");
        assert_eq!(rows[table].1, tf!("{} row(s)", 4).as_ref());
        assert_eq!(
            rows[table + 1..],
            [
                (
                    "A-101".to_string(),
                    " | 19 | 100 | 0 | 1300 | SQPlant / 00".to_string()
                ),
                // Neither read took: both reasons, the items' first.
                (
                    "B-202".to_string(),
                    "cannot read b.pid; no sheet |  |  |  | 1300 | SQPlant / 00 / 01".to_string()
                ),
                (
                    "C-303".to_string(),
                    format!(
                        "{} |  |  |  |  | SQPlant / 00",
                        t!("Registered but missing")
                    )
                ),
                (
                    "D-404".to_string(),
                    " | 19 | 240 | 3 | 1300 | SQPlant / 02".to_string()
                ),
            ]
        );
        // A check that failed to start says so where its result would be.
        let mut state = ready();
        let check = button_node(&state, &NodeId::Root);
        update(&mut state, PidBackupMsg::Activate(check.clone()));
        loaded(
            &mut state,
            &check,
            Err("cannot start reading the backup data".to_string()),
        );
        assert_eq!(
            shown_check(&state),
            [(
                "cannot start reading the backup data".to_string(),
                String::new()
            )]
        );
    }

    #[test]
    fn the_clock_goes_while_the_check_counts_and_stops_when_it_ends() {
        let mut state = ready();
        let check = button_node(&state, &NodeId::Root);
        // No check, no clock: a beat that arrives finds nothing to count.
        assert_eq!(update(&mut state, PidBackupMsg::Progress), Effect::None);
        update(&mut state, PidBackupMsg::Activate(check.clone()));
        // Each beat asks for the next for as long as the check runs.
        assert_eq!(update(&mut state, PidBackupMsg::Progress), Effect::Tick);
        assert_eq!(update(&mut state, PidBackupMsg::Progress), Effect::Tick);
        // Something else started meanwhile does not set a second clock
        // going: a drawing's import, which has no count of its own.
        let import = button_node(&state, &NodeId::Drawing(0));
        assert_eq!(
            update(&mut state, PidBackupMsg::Activate(import.clone())),
            load_of(&state, &import)
        );
        // The check runs to its end on its thread (the test backup's files
        // are not there, so each read fails at once). Its result is still
        // on its way to the panel, and the clock stops all the same: a
        // caller that waits for the clock before it hands the result over
        // is not kept waiting.
        let index = state.index().cloned().expect("the backup is ready");
        let checked = check_all(&index);
        assert_eq!(
            shown_check(&state),
            [(
                tf!("Checking: {} of {} drawing(s) done…", 3, 3).into_owned(),
                String::new()
            )]
        );
        assert_eq!(update(&mut state, PidBackupMsg::Progress), Effect::None);
        loaded(
            &mut state,
            &check,
            Ok(SectionData::new(check_view(checked))),
        );
        assert_eq!(update(&mut state, PidBackupMsg::Progress), Effect::None);
        assert_eq!(
            value(&shown_check(&state), &t!("Drawings checked")),
            Some(tf!("{}, {} of them with nothing left out", 3, 0).as_ref())
        );

        // A check whose result arrives before the next beat stops it too.
        let mut state = ready();
        let check = button_node(&state, &NodeId::Root);
        update(&mut state, PidBackupMsg::Activate(check.clone()));
        loaded(
            &mut state,
            &check,
            Ok(SectionData::new(check_view(found()))),
        );
        assert_eq!(update(&mut state, PidBackupMsg::Progress), Effect::None);
        // And a backup closed under a running check leaves the beat that is
        // on its way nothing to count.
        let mut state = ready();
        let check = button_node(&state, &NodeId::Root);
        update(&mut state, PidBackupMsg::Activate(check));
        update(&mut state, PidBackupMsg::Close);
        assert_eq!(update(&mut state, PidBackupMsg::Progress), Effect::None);
    }

    #[test]
    fn a_backup_with_no_drawing_file_offers_no_check() {
        let mut state = PidBackupBrowser::default();
        update(
            &mut state,
            PidBackupMsg::Loaded(Ok(Arc::new(BackupIndex::default()))),
        );
        assert!(detail_rows(&state, &NodeId::Root)
            .iter()
            .all(|(key, _)| *key != t!("Check of the drawings")));
    }

    /// The parser's report as it is read: two lines, the second so long it
    /// takes two rows.
    fn report_text(hidden: usize) -> TextFile {
        TextFile {
            rows: vec![
                (Some(1), "=== Summary ===".to_string()),
                (Some(2), "streams: 41, of which".to_string()),
                (None, " 3 are not decoded".to_string()),
            ],
            line_count: 2,
            encoding: "UTF-8",
            bytes: 58,
            cut_at: None,
            hidden,
        }
    }

    #[test]
    fn a_drawing_with_a_file_offers_its_import_and_shows_the_parsers_report() {
        let mut state = ready();
        let drawing = NodeId::Drawing(0);
        // Under the drawing's first lines and above its columns: the offer.
        assert_eq!(
            shown_import(&state, &drawing),
            [
                (
                    t!("Imports the drawing as opening it would, without opening it: what was drawn, what was left out, and the parser's report of the file.")
                        .into_owned(),
                    String::new()
                ),
                (t!("Import and diagnose").into_owned(), String::new()),
            ]
        );
        // Selecting the drawing imports nothing; the button does, once.
        assert_eq!(
            update(&mut state, PidBackupMsg::Select(drawing.clone())),
            Effect::None
        );
        let import = button_node(&state, &drawing);
        assert_eq!(
            update(&mut state, PidBackupMsg::Activate(import.clone())),
            load_of(&state, &import)
        );
        assert_eq!(
            shown_import(&state, &drawing),
            [(
                t!("Reading and importing the drawing…").into_owned(),
                String::new()
            )]
        );
        assert_eq!(
            update(&mut state, PidBackupMsg::Activate(import.clone())),
            Effect::None
        );

        let report = ImportReport {
            import: Ok(Imported {
                drawn: 240,
                decoded: 230,
                missing: 3,
                ..Imported::default()
            }),
            import_time: Duration::from_millis(1260),
            parse_report: Ok(report_text(0)),
        };
        loaded(
            &mut state,
            &import,
            Ok(SectionData::new(report_view(report))),
        );
        let rows = shown_import(&state, &drawing);
        assert_eq!(
            rows.last(),
            Some(&(t!("Imported in").into_owned(), "1.3 s".to_string()))
        );
        assert_eq!(
            rows[..3],
            [
                (t!("Entities drawn").into_owned(), "240".to_string()),
                (t!("Source records decoded").into_owned(), "230".to_string()),
                (t!("Source records not drawn").into_owned(), "3".to_string()),
            ]
        );
        // At the end of the details, below the columns, the parser's report:
        // how long it is, then its lines as a table, a long line over two
        // rows.
        assert_eq!(
            lines_under(&state, &drawing, &t!("Parser's report of the file")),
            [
                (t!("Line count").into_owned(), "2".to_string()),
                (
                    format!("{} | {}", t!("Line #"), t!("Text content")),
                    tf!("{} row(s)", 3).into_owned()
                ),
                ("1".to_string(), "=== Summary ===".to_string()),
                ("2".to_string(), "streams: 41, of which".to_string()),
                (String::new(), " 3 are not decoded".to_string()),
            ]
        );
        // The button is gone: the drawing is imported once.
        assert!(detail_lines(&state, &drawing)
            .iter()
            .all(|line| !matches!(line, DetailLine::Button(..))));

        // A drawing the backup has no file of offers nothing.
        assert!(detail_rows(&state, &NodeId::Drawing(2))
            .iter()
            .all(|(key, _)| *key != t!("Importing it")));
    }

    #[test]
    fn an_import_or_a_report_that_failed_says_why_and_a_hidden_value_is_counted() {
        let mut state = ready();
        let drawing = NodeId::Drawing(1);
        let import = button_node(&state, &drawing);
        update(&mut state, PidBackupMsg::Activate(import.clone()));
        let report = ImportReport {
            import: Err("the file has no sheet".to_string()),
            import_time: Duration::from_millis(12),
            parse_report: Err("cannot read b.pid: not a compound file".to_string()),
        };
        loaded(
            &mut state,
            &import,
            Ok(SectionData::new(report_view(report))),
        );
        assert_eq!(
            shown_import(&state, &drawing),
            [
                ("the file has no sheet".to_string(), String::new()),
                (t!("Imported in").into_owned(), "0.0 s".to_string()),
            ]
        );
        assert_eq!(
            lines_under(&state, &drawing, &t!("Parser's report of the file")),
            [(
                "cannot read b.pid: not a compound file".to_string(),
                String::new()
            )]
        );

        // A value of the report that looks like a credential is hidden as
        // in any text of the backup, and the details say how many were.
        let mut state = ready();
        update(&mut state, PidBackupMsg::Activate(import.clone()));
        let report = ImportReport {
            import: Ok(Imported::default()),
            import_time: Duration::ZERO,
            parse_report: Ok(report_text(2)),
        };
        loaded(
            &mut state,
            &import,
            Ok(SectionData::new(report_view(report))),
        );
        assert_eq!(
            lines_under(&state, &drawing, &t!("Parser's report of the file"))[..2],
            [
                (t!("Line count").into_owned(), "2".to_string()),
                (
                    tf!(
                        "{} credential-like value(s) in the text are hidden and shown as {}",
                        2,
                        HIDDEN
                    )
                    .into_owned(),
                    String::new()
                ),
            ]
        );

        // The read itself failed to start: said where the import would be.
        let mut state = ready();
        update(&mut state, PidBackupMsg::Activate(import.clone()));
        loaded(&mut state, &import, Err("cannot start reading".to_string()));
        assert_eq!(
            shown_import(&state, &drawing),
            [("cannot start reading".to_string(), String::new())]
        );
        // Nothing was read, so there is no report to head either.
        assert!(detail_rows(&state, &drawing)
            .iter()
            .all(|(key, _)| *key != t!("Parser's report of the file")));
    }

    #[test]
    fn an_import_says_what_it_drew_what_it_left_out_and_what_it_found() {
        let pairs = |import: Result<Imported, String>| -> Vec<(String, String)> {
            let mut lines = Vec::new();
            import_lines(&import, Duration::from_millis(1260), &mut lines);
            lines.into_iter().flat_map(DetailLine::into_pairs).collect()
        };
        let expected: Vec<(String, String)> = [
            (t!("Entities drawn"), "240".to_string()),
            (t!("Source records decoded"), "230".to_string()),
            (t!("Source records not drawn"), "3".to_string()),
            (
                t!("Sheet layers"),
                tf!("{}, {} of them off", 4, 1).into_owned(),
            ),
            (t!("Border template"), "A1.igr".to_string()),
            (t!("Border template entities"), "77".to_string()),
            (
                t!("Symbol bodies"),
                tf!("{} from the drawing's cache, {} from the library", 12, 0).into_owned(),
            ),
            (t!("Symbol library"), "D:/restore/Ref/Symbols".to_string()),
            (t!("Drawing unit"), "m".to_string()),
            (t!("Imported in"), "1.3 s".to_string()),
        ]
        .into_iter()
        .map(|(key, value)| (key.into_owned(), value))
        .collect();
        assert_eq!(
            pairs(Ok(Imported {
                drawn: 240,
                decoded: 230,
                missing: 3,
                sheet_layers: 4,
                sheet_layers_off: 1,
                border_template: Some("A1.igr".to_string()),
                template_entities: 77,
                template_missing: 0,
                cache_bodies: 12,
                library_bodies: 0,
                symbol_library: vec!["D:/restore/Ref/Symbols".into()],
                unit: Some("m".to_string()),
                style_tables_failed: false,
            })),
            expected
        );
        // What is missing is said, each in its own words.
        let rows = pairs(Ok(Imported {
            drawn: 5,
            template_missing: 2,
            border_template: Some("A1.igr".to_string()),
            template_entities: 9,
            style_tables_failed: true,
            ..Imported::default()
        }));
        assert_eq!(value(&rows, &t!("Sheet layers")), Some("0"));
        assert_eq!(
            value(&rows, &t!("Border template entities")),
            Some(tf!("{}, {} record(s) of it not drawn", 9, 2).as_ref())
        );
        assert_eq!(
            value(&rows, &t!("Symbol library")),
            Some(t!("none found").as_ref())
        );
        assert_eq!(
            value(&rows, &t!("Drawing unit")),
            Some(t!("none stated; the metre is assumed").as_ref())
        );
        assert!(rows.contains(&(
            t!("A style table did not read: line work or lettering is on fallbacks.").into_owned(),
            String::new()
        )));
        assert_eq!(
            value(&pairs(Ok(Imported::default())), &t!("Border template")),
            Some(t!("none found; the page border is a stand-in").as_ref())
        );
        // An import that failed: its reason, and how long it took to fail.
        assert_eq!(
            pairs(Err("the file has no sheet".to_string())),
            [
                ("the file has no sheet".to_string(), String::new()),
                (t!("Imported in").into_owned(), "1.3 s".to_string()),
            ]
        );
    }

    /// A table under more lines than the detail half holds: the lines get
    /// a scroll bar of their own, and the table keeps its place on screen
    /// with the heading it belongs to.
    #[test]
    fn the_drawn_table_of_the_check_is_on_screen_under_the_backups_many_lines() {
        let _guard = crate::headless_ui_lock();
        let mut state = ready();
        update(&mut state, PidBackupMsg::Select(NodeId::Root));
        let check = button_node(&state, &NodeId::Root);
        update(&mut state, PidBackupMsg::Activate(check.clone()));
        loaded(
            &mut state,
            &check,
            Ok(SectionData::new(check_view(found()))),
        );
        // The backup's own lines, the warnings, the check's result and the
        // row for every drawing: more than the lower half of a docked panel
        // holds at their full height.
        assert!(detail_rows(&state, &NodeId::Root).len() > 14);
        // A docked panel in a window of ordinary height, the bar where it
        // is when the panel opens.
        let mut ui = iced_test::simulator::Simulator::with_size(
            iced::Settings::default(),
            (340.0, 700.0),
            view(&state, 340.0, Chrome::default()),
        );
        let mut found = |label: &str| {
            ui.find(label)
                .unwrap_or_else(|error| panic!("{label} is drawn: {error:?}"))
        };
        // The table: its titles, and every one of its four rows in full.
        let title = found(t!("What went wrong").as_ref())
            .visible_bounds()
            .expect("the table's titles show");
        let mut last = title.y;
        for drawing in ["A-101", "B-202", "C-303", "D-404"] {
            let row = found(drawing);
            assert_eq!(
                row.visible_bounds(),
                Some(row.bounds()),
                "the row of {drawing} is on screen in full"
            );
            assert!(
                row.bounds().y > last,
                "{drawing} is below the row before it"
            );
            last = row.bounds().y;
        }
        // The check's result stays with it, right above the table, under
        // its heading; the backup's own lines scroll above that.
        let heading = found(t!("Check of the drawings").as_ref())
            .visible_bounds()
            .expect("the check's heading shows with its table");
        let counted = found(t!("Failed or missing").as_ref())
            .visible_bounds()
            .expect("the check's result shows with its table");
        assert!(heading.y < counted.y && counted.y < title.y);
    }

    /// The two buttons as the panel draws them: each is where a click
    /// starts what it names.
    #[test]
    fn the_drawn_buttons_start_the_check_and_a_drawings_import() {
        let _guard = crate::headless_ui_lock();
        let mut state = ready();
        // The details are long; give them most of the panel.
        update(&mut state, PidBackupMsg::SetSplit(0.3));

        fn click(state: &PidBackupBrowser, label: &str) -> Vec<PidBackupMsg> {
            let mut ui = iced_test::simulator::Simulator::with_size(
                iced::Settings::default(),
                (340.0, 700.0),
                view(state, 340.0, Chrome::default()),
            );
            let bounds = ui
                .find(label)
                .unwrap_or_else(|error| panic!("{label} is drawn: {error:?}"))
                .visible_bounds()
                .unwrap_or_else(|| panic!("{label} is on screen"));
            ui.point_at(bounds.center());
            let _ = ui.simulate(iced_test::simulator::click());
            ui.into_messages()
                .filter_map(|message| match message {
                    Message::PidBackup(message) => Some(message),
                    _ => None,
                })
                .collect()
        }

        update(&mut state, PidBackupMsg::Select(NodeId::Root));
        let check = button_node(&state, &NodeId::Root);
        let pressed = click(&state, t!("Check every drawing").as_ref());
        assert!(
            pressed
                .iter()
                .any(|message| matches!(message, PidBackupMsg::Activate(found) if *found == check)),
            "{pressed:?}"
        );

        update(&mut state, PidBackupMsg::Select(NodeId::Drawing(0)));
        let import = button_node(&state, &NodeId::Drawing(0));
        let pressed = click(&state, t!("Import and diagnose").as_ref());
        assert!(
            pressed.iter().any(
                |message| matches!(message, PidBackupMsg::Activate(found) if *found == import)
            ),
            "{pressed:?}"
        );
        // The other button of the drawing's details still opens it.
        let opened = click(&state, t!("Open drawing").as_ref());
        assert!(
            opened
                .iter()
                .any(|message| matches!(message, PidBackupMsg::Activate(NodeId::Drawing(0)))),
            "{opened:?}"
        );
    }
}
