//! PIDPLOT -- plot the active `.pid` drawing to PDF with pid-parse's own
//! plotter (`io::pid_pdf`).
//!
//! ```text
//! PIDPLOT              prompt for the PDF file (Enter = <name>.pidplot.pdf
//!                      next to the drawing)
//! PIDPLOT <file.pdf>   write the plot there; a relative path is taken from
//!                      the drawing's folder
//! ```
//!
//! The plot is made from the `.pid` on disk -- the sheet as SmartPlant prints
//! it, border template and library symbol bodies included -- not from the
//! drawing as edited here. It runs on a worker thread and reports on the
//! command line when the file is written. Bare `PIDPLOT` only prompts, like
//! bare PIDTAG: the driver dispatches the first word of an inline line on its
//! own first.

use super::*;

fn unquoted(arg: &str) -> &str {
    let quoted = arg.len() >= 2
        && ((arg.starts_with('"') && arg.ends_with('"'))
            || (arg.starts_with('\'') && arg.ends_with('\'')));
    if quoted {
        &arg[1..arg.len() - 1]
    } else {
        arg
    }
}

impl OpenCADStudio {
    pub(super) fn dispatch_pidplot(&mut self, cmd: &str, i: usize) -> Option<Task<Message>> {
        let rest = cmd.strip_prefix("PIDPLOT")?;
        if !(rest.is_empty() || rest.starts_with(char::is_whitespace)) {
            return None;
        }
        let arg = unquoted(rest.trim());
        if arg.is_empty() && cmd.trim_end() == cmd {
            use crate::command::FreeTextValuePromptCommand;
            let c = FreeTextValuePromptCommand::new(
                "PIDPLOT",
                "PIDPLOT  PDF file <next to the drawing>:",
            );
            self.command_line.push_info(&c.prompt());
            self.tabs[i].active_cmd = Some(Box::new(c));
            return Some(self.finish_dispatch(cmd));
        }
        Some(self.start_pid_pdf(i, arg))
    }

    #[cfg(target_arch = "wasm32")]
    fn start_pid_pdf(&mut self, _i: usize, _arg: &str) -> Task<Message> {
        self.command_line
            .push_error("PIDPLOT: plotting a .pid to PDF is not available in the web build.");
        Task::none()
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn start_pid_pdf(&mut self, i: usize, arg: &str) -> Task<Message> {
        let Some(drawing) = self.pid_source(i) else {
            self.command_line
                .push_error("PIDPLOT: the active drawing was not opened from a .pid file.");
            return Task::none();
        };
        let target = if arg.is_empty() {
            crate::io::pid_pdf::default_target(&drawing)
        } else {
            let mut target = std::path::PathBuf::from(arg);
            if target.is_relative() {
                if let Some(folder) = drawing.parent() {
                    target = folder.join(target);
                }
            }
            if target.extension().is_none() {
                target.set_extension("pdf");
            }
            target
        };
        self.plot_pid_pdf(drawing, target)
    }

    /// The `.pid` file tab `i` was opened from, if it was.
    #[cfg(not(target_arch = "wasm32"))]
    pub(in crate::app) fn pid_source(&self, i: usize) -> Option<std::path::PathBuf> {
        self.tabs[i]
            .current_path
            .clone()
            .filter(|path| crate::io::pid_pdf::is_pid(path))
    }

    /// EXPORT / EXPORTPDF / Plot to a PDF file, for a drawing opened from a
    /// `.pid`: pid-parse's plotter makes the file, as PIDPLOT does, so the
    /// export is the sheet SmartPlant prints. An SVG target, or any other
    /// drawing, stays with the plot pipeline (`None`).
    #[cfg(not(target_arch = "wasm32"))]
    pub(in crate::app) fn export_pid_pdf(
        &mut self,
        i: usize,
        target: &std::path::Path,
    ) -> Option<Task<Message>> {
        let drawing = self.pid_source(i)?;
        if !target
            .extension()
            .is_some_and(|extension| extension.eq_ignore_ascii_case("pdf"))
        {
            return None;
        }
        self.command_line.push_info(
            "PDF export of a .pid: pid-parse's plotter draws the sheet; the plot dialog's paper, scale and plot style settings do not apply.",
        );
        Some(self.plot_pid_pdf(drawing, target.to_path_buf()))
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn plot_pid_pdf(
        &mut self,
        drawing: std::path::PathBuf,
        target: std::path::PathBuf,
    ) -> Task<Message> {
        if target == drawing {
            self.command_line
                .push_error("PIDPLOT: the PDF cannot replace the drawing itself.");
            return Task::none();
        }
        self.command_line.push_output(&format!(
            "PIDPLOT: plotting {} (the .pid on disk; edits made here are not in it) ...",
            drawing.display()
        ));
        Task::perform(
            crate::io::pid_pdf::plot_pdf_in_background(drawing, target),
            Message::PidPdfFinished,
        )
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub(in crate::app) fn on_pid_pdf_finished(
        &mut self,
        result: Result<crate::io::pid_pdf::PidPdfReceipt, String>,
    ) {
        let receipt = match result {
            Ok(receipt) => receipt,
            Err(error) => {
                self.command_line.push_error(&format!("PIDPLOT: {error}"));
                return;
            }
        };
        let page = receipt
            .page_mm
            .map_or_else(String::new, |[w, h]| format!(", page {w:.0} x {h:.0} mm"));
        self.command_line.push_output(&format!(
            "PIDPLOT: wrote {} ({} bytes, {} primitives{page}).",
            receipt.path.display(),
            receipt.bytes,
            receipt.primitives,
        ));
        match &receipt.template {
            Some(template) => self.command_line.push_output(&format!(
                "PIDPLOT: border template {}.",
                template.display()
            )),
            None => self.command_line.push_info(&format!(
                "PIDPLOT: no border template of this page size under {}.",
                receipt.corpus.display()
            )),
        }
        for note in &receipt.notes {
            self.command_line.push_info(&format!("PIDPLOT: {note}."));
        }
        self.command_line.push_output(&format!(
            "PIDPLOT: {} of {} symbol files found in the library.",
            receipt.symbols_found, receipt.symbols_referenced
        ));
        if !receipt.missing_symbols.is_empty() {
            self.command_line.push_warning(&format!(
                "PIDPLOT: not in the library (boxed on the page): {}",
                receipt.missing_symbols.join(", ")
            ));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::unquoted;

    #[test]
    fn a_quoted_path_loses_its_quotes() {
        assert_eq!(unquoted("\"D:/out/a b.pdf\""), "D:/out/a b.pdf");
        assert_eq!(unquoted("'a.pdf'"), "a.pdf");
        assert_eq!(unquoted("a.pdf"), "a.pdf");
        assert_eq!(unquoted("\""), "\"");
    }

    /// `PIDPLOT` on sheets of a real backup, each opened from the backup
    /// panel of the P&ID module. The made-up backups of
    /// `app/pid_shell/pid_backup_in_shell.rs` show which file the command
    /// plots; here a page is written and held against the export's.
    #[cfg(not(target_arch = "wasm32"))]
    mod pidplot_real {
        use super::super::*;
        use crate::app::pid_shell::PidScreen;
        use crate::io::pid_backup_browse::BackupIndex;
        use crate::ui::command_line::EntryKind;
        use crate::ui::window::pid_backup_browser::{self, NodeId, PidBackupMsg};
        use std::path::Path;
        use std::sync::Arc;
        use std::time::Instant;

        /// The backup taken when `OCS_PIDPLOT_REAL` names none: the plant
        /// sample, where it lies on the machine the plan was written on.
        const REAL_BACKUP: &str = r"D:\work\plant-code\cad\pid-test-data";

        /// The sheets taken when `OCS_PIDPLOT_REAL_SHEETS` names none: the
        /// plant's second smallest, median and largest `.pid`, the three that
        /// `io::pid_pdf` holds to the bytes `pid_plot` writes.
        const SHEETS: [&str; 3] = [
            "D04",
            "集注站 低压气及轻烃处理单元 轻烃分离器二工艺及自控流程图",
            "EF井场 注采阀组工艺及自控流程图",
        ];

        /// The application as a launch without `--classic` leaves it once
        /// the startup prompts are answered: in the P&ID module, on its
        /// editor.
        fn in_the_module() -> OpenCADStudio {
            let mut app = OpenCADStudio::new_for_test();
            app.default_assoc_prompted = true;
            app.donation_prompt_version = env!("OCS_APP_VERSION").to_owned();
            let launch = crate::cli::GuiConfig::default();
            assert!(
                !launch.classic,
                "a launch is in the module unless it says otherwise"
            );
            let first = app.open_first_screen(&launch);
            app.drive_headless_task(first)
                .expect("the first screen opens without a window");
            let _ = app.update(Message::WindowResized(1600.0, 1000.0));
            app
        }

        /// What the command line said: every line but the echo of a command,
        /// with its kind.
        type Said = Vec<(EntryKind, String)>;

        /// What the command line holds.
        fn said(app: &OpenCADStudio) -> Said {
            let entries = app.command_line.history.iter();
            let spoken = entries.filter(|entry| entry.kind != EntryKind::Command);
            spoken
                .map(|entry| (entry.kind.clone(), entry.text.clone()))
                .collect()
        }

        /// Type `line` and let what it starts run to its end: what the
        /// command line said, and how long that took in seconds.
        fn say(app: &mut OpenCADStudio, line: &str) -> (Said, f64) {
            app.command_line.history.clear();
            let clock = Instant::now();
            let task = app.run_command_line(line);
            app.drive_headless_task(task)
                .expect("the command needs no window");
            (said(app), clock.elapsed().as_secs_f64())
        }

        /// The lines of `said` that are of `kind`.
        fn of_kind(said: &[(EntryKind, String)], kind: EntryKind) -> Vec<&str> {
            let lines = said.iter().filter(|(of, _)| *of == kind);
            lines.map(|(_, text)| text.as_str()).collect()
        }

        /// `said` for the report: every line with its kind before it.
        fn written_down(said: &[(EntryKind, String)]) -> Vec<String> {
            let line = |(kind, text): &(EntryKind, String)| format!("{kind:?}: {text}");
            said.iter().map(line).collect()
        }

        fn sha256(bytes: &[u8]) -> String {
            use sha2::{Digest, Sha256};
            format!("{:x}", Sha256::digest(bytes))
        }

        /// Open the drawing `at` of the backup the panel holds as a double
        /// click on its row opens it, and let the open run to its end. What
        /// was measured, for the report; what is not as it should be goes to
        /// `unmet`.
        fn open_from_the_panel(
            app: &mut OpenCADStudio,
            index: &BackupIndex,
            (name, at): (&str, usize),
            unmet: &mut Vec<String>,
        ) -> serde_json::Value {
            let file = index.drawings[at].pid_path.clone().unwrap_or_default();
            let tabs = app.tabs.len();
            app.command_line.history.clear();
            let clock = Instant::now();
            let open = PidBackupMsg::Activate(NodeId::Drawing(at));
            let task = app.update(Message::PidBackup(open));
            let ran = app.drive_headless_task(task);
            let seconds = clock.elapsed().as_secs_f64();
            let said = said(app);
            let front = &app.tabs[app.active_tab];
            let shown = front.current_path.clone();
            let entities = front.scene.document.entities().count();
            let screen = app.pid_shell.current();
            if let Err(why) = &ran {
                unmet.push(format!("{name}: the open did not run to its end: {why}"));
            }
            if !file.starts_with(&index.restored) {
                let file = file.display();
                unmet.push(format!(
                    "{name}: {file} is not in the folder the backup is restored to"
                ));
            }
            if shown.as_deref() != Some(file.as_path()) {
                let file = file.display();
                unmet.push(format!(
                    "{name}: the tab in front holds {shown:?}, not {file}"
                ));
            }
            if entities == 0 {
                unmet.push(format!("{name}: the drawing in front is empty"));
            }
            if screen != Some(PidScreen::Editor) {
                unmet.push(format!(
                    "{name}: the module is on {screen:?}, not on its editor"
                ));
            }
            if !of_kind(&said, EntryKind::Error).is_empty() {
                unmet.push(format!("{name}: the open said {said:?}"));
            }
            serde_json::json!({
                "seconds": seconds,
                "stopped": ran.err(),
                "tab": shown.map(|path| path.display().to_string()),
                "tabs": [tabs, app.tabs.len()],
                "entities": entities,
                "said": written_down(&said),
            })
        }

        /// Plot the drawing in front, opened from `file`, with
        /// `PIDPLOT <pdf>`. What was measured, for the report; what is not as
        /// it should be goes to `unmet`.
        fn plot_the_drawing_in_front(
            app: &mut OpenCADStudio,
            name: &str,
            (file, pdf): (&Path, &Path),
            unmet: &mut Vec<String>,
        ) -> serde_json::Value {
            let (said, seconds) = say(app, &format!("PIDPLOT \"{}\"", pdf.display()));
            let lines = of_kind(&said, EntryKind::Output);
            let starting = format!(
                "PIDPLOT: plotting {} (the .pid on disk; edits made here are not in it) ...",
                file.display()
            );
            if !lines.contains(&starting.as_str()) {
                unmet.push(format!(
                    "{name}: PIDPLOT did not say `{starting}`: {said:?}"
                ));
            }
            if !of_kind(&said, EntryKind::Error).is_empty() {
                unmet.push(format!("{name}: PIDPLOT said {said:?}"));
            }
            let bytes = std::fs::read(pdf).ok();
            match &bytes {
                Some(bytes) => {
                    let wrote =
                        format!("PIDPLOT: wrote {} ({} bytes, ", pdf.display(), bytes.len());
                    if !lines.iter().any(|line| line.starts_with(&wrote)) {
                        unmet.push(format!(
                            "{name}: PIDPLOT did not say `{wrote}...`: {said:?}"
                        ));
                    }
                }
                None => unmet.push(format!("{name}: PIDPLOT wrote no {}", pdf.display())),
            }
            serde_json::json!({
                "pdf": pdf.display().to_string(),
                "seconds": seconds,
                "bytes": bytes.as_ref().map(Vec::len),
                "sha256": bytes.as_deref().map(sha256),
                "said": written_down(&said),
            })
        }

        /// How the PDF `PIDPLOT` wrote of a sheet and the one the export
        /// wrote of it compare.
        fn compared(single: Option<&[u8]>, exported: Option<&[u8]>) -> String {
            let (Some(single), Some(exported)) = (single, exported) else {
                let none = if single.is_none() {
                    "PIDPLOT"
                } else {
                    "the export"
                };
                return format!("{none} wrote no PDF of it");
            };
            match single.iter().zip(exported).position(|(a, b)| a != b) {
                Some(at) => format!("first differ at byte {at}"),
                None if single.len() == exported.len() => "the same bytes".to_string(),
                None => "one is the start of the other".to_string(),
            }
        }

        /// Sheets of a real backup through the commands, in the P&ID module:
        /// `PIDBACKUPOPEN` opens the backup, every sheet is opened from the
        /// backup panel as a double click on its row opens it, and
        /// `PIDPLOT <file>` plots it; then `PIDBACKUPPDF` exports the whole
        /// backup. Of SPEC-sppid-pdf §6, criterion 11:
        ///
        /// - the tab a sheet is opened in holds the `.pid` the restore made
        ///   of it, and the module stays on its editor;
        /// - `PIDPLOT` plots that file, without an error, and writes the PDF
        ///   it was asked for;
        /// - that PDF and the one the export writes of the same drawing are
        ///   one file, byte for byte.
        ///
        /// What was measured goes to `report.json` before anything is
        /// asserted. `OCS_PIDPLOT_REAL` names the backup, a folder or a zip;
        /// `OCS_PIDPLOT_REAL_SHEETS` the sheets, by the names of their files
        /// with `;` between them; `OCS_PIDPLOT_REAL_OUT` a folder to keep the
        /// restore, the pages and the report in. A backup that is not there
        /// fails the test: a comparison that did not run must not read as one
        /// that passed.
        #[test]
        #[ignore = "opens sheets of a real SmartPlant backup and exports the backup; run alone with --ignored --nocapture"]
        fn a_sheet_plotted_from_the_panel_is_the_page_the_export_of_the_backup_writes() {
            let named = |variable: &str| std::env::var_os(variable).map(PathBuf::from);
            let source = named("OCS_PIDPLOT_REAL").unwrap_or_else(|| REAL_BACKUP.into());
            assert!(source.exists(), "no backup at {}", source.display());
            let names: Vec<String> = match std::env::var("OCS_PIDPLOT_REAL_SHEETS") {
                Ok(names) => names
                    .split(';')
                    .map(|name| name.trim().to_owned())
                    .collect(),
                Err(_) => SHEETS.map(str::to_owned).to_vec(),
            };
            let kept = named("OCS_PIDPLOT_REAL_OUT");
            let own = format!("ocs-pidplot-real-{}", std::process::id());
            let own = std::env::temp_dir().join(own);
            let _ = std::fs::remove_dir_all(&own);
            let out = kept.clone().unwrap_or_else(|| own.clone());
            // What an earlier run left must not pass for this one's.
            for left in ["restores", "single", "whole", "report.json"] {
                let left = out.join(left);
                assert!(!left.exists(), "{} is there already", left.display());
            }
            std::fs::create_dir_all(&out).expect("the folder the pages go to");

            let mut app = in_the_module();
            let cache = Some(out.join("restores"));
            pid_backup_browser::set_cache_folder(&mut app.pid_backup_browser, cache);
            let open = format!("PIDBACKUPOPEN \"{}\"", source.display());
            let (answer, open_seconds) = say(&mut app, &open);
            let Some(index) = app.pid_backup_browser.index().map(Arc::clone) else {
                panic!("{} did not open: {answer:?}", source.display());
            };

            // Every sheet opened from the panel and plotted, one after the
            // other, the tabs of the ones before it left open.
            let mut unmet = Vec::new();
            let mut sheets = Vec::new();
            for name in &names {
                let named_so = index.drawings.iter().enumerate().filter(|(_, drawing)| {
                    let stem = drawing.pid_path.as_deref().and_then(Path::file_stem);
                    stem.is_some_and(|stem| stem == name.as_str())
                });
                let named_so: Vec<usize> = named_so.map(|(at, _)| at).collect();
                let [at] = named_so[..] else {
                    let found = named_so.len();
                    panic!("{found} drawing(s) of the backup have a file named {name}.pid");
                };
                let file = index.drawings[at].pid_path.clone().unwrap_or_default();
                let pdf = out.join("single").join(format!("{name}.pdf"));
                let opened = open_from_the_panel(&mut app, &index, (name, at), &mut unmet);
                let plotted = plot_the_drawing_in_front(&mut app, name, (&file, &pdf), &mut unmet);
                sheets.push((name, file, pdf, opened, plotted));
            }

            // The whole backup, exported by the same application.
            let whole = out.join("whole");
            let export = format!("PIDBACKUPPDF {}", whole.display());
            let (exporting, export_seconds) = say(&mut app, &export);
            if !of_kind(&exporting, EntryKind::Error).is_empty() {
                unmet.push(format!("the export said {exporting:?}"));
            }
            let listed = whole.join(crate::io::pid_backup_pdf::INDEX_NAME);
            let listed = std::fs::read_to_string(&listed).ok();
            let listed = listed.and_then(|text| serde_json::from_str(&text).ok());
            let listed: serde_json::Value = listed.unwrap_or_default();
            let pages = listed.get("pages").and_then(serde_json::Value::as_array);
            if pages.is_none() {
                unmet.push(format!("the export into {} left no index", whole.display()));
            }

            let mut measured = Vec::new();
            for (name, file, pdf, opened, plotted) in sheets {
                // The export's entry of this drawing: the one whose file,
                // from the restore's folder or the backup's own, is the
                // sheet's.
                let of_this = |entry: &&serde_json::Value| {
                    let inside = entry.get("pid").and_then(serde_json::Value::as_str);
                    let holders = [&index.restored, &index.backup_root];
                    inside.is_some_and(|inside| holders.iter().any(|at| at.join(inside) == file))
                };
                let entry = pages.into_iter().flatten().find(of_this);
                let page = entry.and_then(|entry| entry.get("pdf"));
                let page = page.and_then(serde_json::Value::as_str);
                let page = page.map(|page| whole.join(page));
                let single = std::fs::read(&pdf).ok();
                let exported = page.as_ref().and_then(|page| std::fs::read(page).ok());
                let verdict = compared(single.as_deref(), exported.as_deref());
                let same = single.is_some() && single == exported;
                if !same {
                    unmet.push(format!("{name}: PIDPLOT's PDF and the export's: {verdict}"));
                }
                eprintln!(
                    "{name}: PIDPLOT {:?} bytes, the export {:?} bytes, {verdict}; opened in \
                     {:.1} s, plotted in {:.1} s",
                    single.as_ref().map(Vec::len),
                    exported.as_ref().map(Vec::len),
                    opened["seconds"].as_f64().unwrap_or_default(),
                    plotted["seconds"].as_f64().unwrap_or_default()
                );
                measured.push(serde_json::json!({
                    "name": name,
                    "pid": file.display().to_string(),
                    "opened": opened,
                    "plotted": plotted,
                    "exported": {
                        "pdf": page.map(|page| page.display().to_string()),
                        "bytes": exported.as_ref().map(Vec::len),
                        "sha256": exported.as_deref().map(sha256),
                        "entry": entry,
                    },
                    "compared": verdict,
                    "same": same,
                }));
            }

            let set = |variable: &str| {
                (
                    variable.to_string(),
                    serde_json::json!(std::env::var(variable).ok()),
                )
            };
            let variables = [
                "PID_PLOT_FONTS",
                "PID_PLOT_CJK_FONT",
                "PID_SYMBOL_LIBRARY",
                "PID_PLOT_CORPUS",
            ];
            let environment: serde_json::Map<String, serde_json::Value> =
                variables.into_iter().map(set).collect();
            let report = serde_json::json!({
                "source": index.source.display().to_string(),
                "package": index.backup_root.display().to_string(),
                "restored": index.restored.display().to_string(),
                "used_in_place": index.read_only,
                "drawings": index.drawings.len(),
                "debug_assertions": cfg!(debug_assertions),
                "environment": environment,
                "open_seconds": open_seconds,
                "sheets": measured,
                "export": {
                    "folder": whole.display().to_string(),
                    "seconds": export_seconds,
                    "written": listed.get("written"),
                    "failed": listed.get("failed"),
                    "without_file": listed.get("without_file"),
                    "said": written_down(&exporting),
                },
                "unmet": unmet,
            });
            let text = serde_json::to_string_pretty(&report).expect("the report as JSON");
            std::fs::write(out.join("report.json"), text + "\n").expect("the report is written");
            // Only a folder that stays is named: this run's own goes below.
            let kept_in = match &kept {
                Some(folder) => format!("report in {}", folder.display()),
                None => "nothing kept (OCS_PIDPLOT_REAL_OUT names no folder)".to_string(),
            };
            eprintln!(
                "{}: {} drawings, opened in {open_seconds:.1} s; {} sheet(s) plotted from the \
                 panel; the export took {export_seconds:.1} s; {kept_in}",
                source.display(),
                index.drawings.len(),
                names.len()
            );
            drop(app);
            if kept.is_none() {
                let _ = std::fs::remove_dir_all(&own);
            }
            assert!(
                unmet.is_empty(),
                "{} thing(s) are not as they should be:\n{}",
                unmet.len(),
                unmet.join("\n")
            );
        }
    }
}
