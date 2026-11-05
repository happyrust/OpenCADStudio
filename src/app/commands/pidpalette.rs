//! `PIDPALETTE` — the P&ID symbol palette, and `PIDINSERT` — placing a symbol.
//!
//! ```text
//! PIDPALETTE            toggle the symbol palette (dock panel)
//! PIDINSERT             prompt for a symbol id
//! PIDINSERT <symbol>    start placing that symbol
//! ```
//!
//! The palette (`ui::window::pid_palette`) is the mouse front of `PIDINSERT`:
//! clicking a cell sends the same `PIDINSERT <block>` the command line runs, so
//! there is one insertion mode, not two.
//!
//! The symbol catalogue is `assets/pid-symbols.json` through
//! [`SymbolLibrary`], the same library `src/pid/symbols.rs` uses to define the
//! blocks, so the palette can never offer a symbol the drawing has no block
//! for. A symbol is named either by its bare id (`vessel`) or by its block name
//! (`PID_SYM_equipment_vessel`); the bare id is what the contract types.
//!
//! This command takes the palette's click as far as the point step — entering
//! insertion mode — and no further: the placement (block instance, `PID_AUTHOR`
//! XDATA, tag, preview, `SPACE` / `CTRL+SPACE`) is the insert command's own
//! path, so the two cannot disagree about what a click means.

use super::*;
use crate::command::{CmdResult, InputKind};
use crate::pid::symbols::{block_name, SymbolLibrary};
use crate::ui::dock::PanelId;
use crate::ui::window::pid_palette::{PidPaletteMsg, CELL_BOUNDS_PREFIX};
use glam::DVec3;

/// The symbol `PIDINSERT` has been asked to place.
///
/// The block is resolved before the command starts, so an unknown name is
/// refused at the command line instead of after the first click.
pub struct PidInsertCommand {
    /// Block name, `PID_SYM_<category>_<symbol>`.
    block: String,
    /// Human label, for the prompt.
    label: String,
    /// Type code the block maps to in `symbol_type_map`. Resolved and carried
    /// here so the commit path writes the type the library names; this command
    /// stops at the point step, so nothing reads it yet.
    #[allow(dead_code)]
    type_code: String,
    /// The point the user has picked, once they have.
    point: Option<DVec3>,
}

impl PidInsertCommand {
    pub fn new(block: String, label: String, type_code: String) -> Self {
        Self {
            block,
            label,
            type_code,
            point: None,
        }
    }

    /// The block this command places.
    #[cfg(test)]
    pub fn block(&self) -> &str {
        &self.block
    }

    /// The type code the placed instance carries.
    #[cfg(test)]
    pub fn type_code(&self) -> &str {
        &self.type_code
    }
}

impl CadCommand for PidInsertCommand {
    fn name(&self) -> &'static str {
        "PIDINSERT"
    }

    fn prompt(&self) -> String {
        if self.point.is_some() {
            format!("PIDINSERT {} ({})  指定插入点:", self.label, self.block)
        } else {
            format!("PIDINSERT {} ({})  选择符号的插入点:", self.label, self.block)
        }
    }

    fn input_kind(&self) -> InputKind {
        InputKind::Point
    }

    fn on_point(&mut self, pt: DVec3) -> CmdResult {
        // The point is remembered so the prompt names the step, but the
        // placement itself (block instance, `PID_AUTHOR` XDATA, tag) is the
        // insert command's commit path, not the palette's.
        self.point = Some(pt);
        CmdResult::NeedPoint
    }

    fn on_enter(&mut self) -> CmdResult {
        CmdResult::Cancel
    }
}

/// A symbol the library knows, resolved from a typed name.
struct ResolvedSymbol {
    block: String,
    label: String,
    type_code: String,
}

/// Resolve `name` against `library`: a block name, or a bare symbol id.
///
/// A bare id that exists in more than one category resolves to the first,
/// which is the category order of `assets/pid-symbols.json` — the same order
/// the palette shows its tabs in.
fn resolve_symbol(library: &SymbolLibrary, name: &str) -> Option<ResolvedSymbol> {
    let name = name.trim();
    if name.is_empty() {
        return None;
    }
    for category in library.categories() {
        for symbol in &category.symbols {
            let block = block_name(&category.id, &symbol.id);
            if symbol.id.eq_ignore_ascii_case(name) || block.eq_ignore_ascii_case(name) {
                let type_code = library
                    .types_of(&block)
                    .into_iter()
                    .next()
                    .unwrap_or_default();
                return Some(ResolvedSymbol {
                    block,
                    label: symbol.label.clone(),
                    type_code,
                });
            }
        }
    }
    None
}

impl OpenCADStudio {
    pub(super) fn dispatch_pidpalette(&mut self, cmd: &str, i: usize) -> Option<Task<Message>> {
        let verb = cmd.split_whitespace().next()?;
        if verb.eq_ignore_ascii_case("PIDPALETTE") {
            // `PIDPALETTE` takes no argument: it is a switch, like the other
            // palette commands.
            if cmd.split_whitespace().nth(1).is_some() {
                return None;
            }
            return Some(self.toggle_pid_palette());
        }
        if !verb.eq_ignore_ascii_case("PIDINSERT") {
            return None;
        }
        let rest = cmd[verb.len()..].trim();
        if rest.is_empty() {
            use crate::command::FreeTextValuePromptCommand;
            let c = FreeTextValuePromptCommand::new(
                "PIDINSERT",
                "PIDINSERT  symbol id <vessel>:",
            );
            self.command_line.push_info(&c.prompt());
            self.tabs[i].active_cmd = Some(Box::new(c));
            return Some(self.finish_dispatch(cmd));
        }
        let Some(symbol) = resolve_symbol(&self.pid_symbols, rest) else {
            self.command_line.push_error(&format!(
                "PIDINSERT: unknown symbol \"{rest}\" -- the palette lists what \
                 assets/pid-symbols.json defines."
            ));
            return Some(Task::none());
        };
        Some(self.start_pid_insert(i, symbol))
    }

    /// Show the palette (docked on the right the first time), or close it when
    /// it is already showing.
    ///
    /// One `PanelId` means one panel: opening it again cannot stack a second
    /// copy, which is what makes the switch idempotent.
    pub(in crate::app) fn toggle_pid_palette(&mut self) -> Task<Message> {
        if self.dock_panel_visible(PanelId::PidPalette) {
            // The panel is about to stop being built, so the rectangles it
            // reported no longer describe anything on screen.
            crate::ui::wrap_bar::forget_bounds(crate::ui::window::pid_palette::PANEL_BOUNDS_ID);
            crate::ui::wrap_bar::forget_bounds(CELL_BOUNDS_PREFIX);
            return self.on_dock(crate::ui::dock::DockMsg::Close(PanelId::PidPalette));
        }
        if self.dock.location(PanelId::PidPalette).is_none() {
            self.dock
                .dock(PanelId::PidPalette, crate::app::config::DockSide::Right, 0);
        }
        self.show_pid_palette = true;
        self.dock_reveal(PanelId::PidPalette);
        Task::none()
    }

    /// Install `PIDINSERT <block>` as the active command, as the palette's cell
    /// click does.
    fn start_pid_insert(&mut self, i: usize, symbol: ResolvedSymbol) -> Task<Message> {
        let command = PidInsertCommand::new(symbol.block, symbol.label, symbol.type_code);
        let prompt = command.prompt();
        self.reset_command_start_state(i);
        self.command_line.push_info(&prompt);
        self.tabs[i].active_cmd = Some(Box::new(command));
        self.finish_dispatch("PIDINSERT")
    }

    /// A palette interaction: switch category, or click a symbol cell.
    pub(in crate::app) fn on_pid_palette(&mut self, m: PidPaletteMsg) -> Task<Message> {
        match m {
            PidPaletteMsg::Tab(index) => {
                // The grid the panel reported belongs to the tab being left;
                // forget it so a bounds reader sees only the cells now shown.
                crate::ui::wrap_bar::forget_bounds(CELL_BOUNDS_PREFIX);
                self.pid_palette.active = index;
                Task::none()
            }
            PidPaletteMsg::Insert(block) => {
                let Some(symbol) = resolve_symbol(&self.pid_symbols, &block) else {
                    self.command_line.push_error(&format!(
                        "PIDINSERT: the palette offered \"{block}\", which the symbol \
                         library no longer defines."
                    ));
                    return Task::none();
                };
                let i = self.active_tab;
                self.start_pid_insert(i, symbol)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::OpenCADStudio;
    use crate::ui::window::pid_palette::PidPaletteMsg as PaletteMsg;

    fn library() -> SymbolLibrary {
        SymbolLibrary::builtin()
    }

    /// A new drawing with the palette's state as `new` leaves it.
    fn app() -> OpenCADStudio {
        let mut app = OpenCADStudio::new_for_test();
        let _ = app.automation_op(r#"{"op":"new"}"#);
        app
    }

    /// A bare symbol id and its block name both resolve, and to the same
    /// block: the contract types `PIDINSERT vessel`, the palette sends the
    /// block name.
    #[test]
    fn a_symbol_resolves_by_id_and_by_block_name() {
        let library = library();
        let by_id = resolve_symbol(&library, "vessel").expect("vessel 存在");
        let by_block =
            resolve_symbol(&library, "PID_SYM_equipment_vessel").expect("块名存在");
        assert_eq!(by_id.block, "PID_SYM_equipment_vessel");
        assert_eq!(by_id.block, by_block.block);
        assert_eq!(by_id.type_code, "VESSEL");
        assert_eq!(by_id.label, "容器");
    }

    /// Every symbol the palette can show is insertable: resolution and the
    /// grid iterate the same library.
    #[test]
    fn every_palette_symbol_resolves() {
        let library = library();
        for category in library.categories() {
            for symbol in &category.symbols {
                let block = block_name(&category.id, &symbol.id);
                let resolved = resolve_symbol(&library, &block)
                    .unwrap_or_else(|| panic!("{block} 无法解析"));
                assert_eq!(resolved.block, block);
                assert!(
                    !resolved.type_code.is_empty(),
                    "{block} 没有类型映射"
                );
            }
        }
    }

    /// An unknown name resolves to nothing rather than to a near match: the
    /// command must refuse it instead of placing the wrong symbol.
    #[test]
    fn an_unknown_symbol_does_not_resolve() {
        let library = library();
        assert!(resolve_symbol(&library, "not_a_real_symbol").is_none());
        assert!(resolve_symbol(&library, "").is_none());
        assert!(resolve_symbol(&library, "   ").is_none());
    }

    /// The command reports the name, a non-empty prompt and a point step —
    /// what `/api/v1/state` exposes as `command`.
    #[test]
    fn the_command_names_itself_and_asks_for_a_point() {
        use crate::command::CadCommand;
        let command = PidInsertCommand::new(
            "PID_SYM_equipment_vessel".into(),
            "容器".into(),
            "VESSEL".into(),
        );
        assert_eq!(command.name(), "PIDINSERT");
        assert!(!command.prompt().is_empty());
        assert!(command.prompt().contains("PID_SYM_equipment_vessel"));
        assert_eq!(command.input_kind(), InputKind::Point);
        assert_eq!(command.block(), "PID_SYM_equipment_vessel");
        assert_eq!(command.type_code(), "VESSEL");
    }

    /// The command carries the block and the type code the instance will
    /// store.
    #[test]
    fn the_command_carries_the_block_and_its_type() {
        use crate::command::CadCommand;
        let command = PidInsertCommand::new(
            "PID_SYM_valve_gate_valve".into(),
            "闸阀".into(),
            "VALVE_GATE".into(),
        );
        assert_eq!(command.block(), "PID_SYM_valve_gate_valve");
        assert_eq!(command.type_code(), "VALVE_GATE");
    }

    /// `PIDPALETTE` opens the panel, a second run closes it, and a third
    /// opens it again — one panel, never two.
    #[test]
    fn pidpalette_toggles_one_panel() {
        let mut app = app();
        let _ = app.dispatch_command("PIDPALETTE");
        assert!(app.dock_panel_visible(PanelId::PidPalette), "首次 PIDPALETTE 打开面板");
        assert_eq!(
            app.dock.location(PanelId::PidPalette).map(|(side, _)| side),
            Some(crate::app::config::DockSide::Right),
            "首次打开停靠在右侧"
        );
        let _ = app.dispatch_command("PIDPALETTE");
        assert!(!app.dock_panel_visible(PanelId::PidPalette), "再次 PIDPALETTE 关闭面板");
        let _ = app.dispatch_command("PIDPALETTE");
        assert!(app.dock_panel_visible(PanelId::PidPalette), "第三次 PIDPALETTE 重新打开");
    }

    /// Clicking a symbol cell starts `PIDINSERT`, and the panel's category tab
    /// is remembered across a document switch — both are app-level state, not
    /// tab state.
    #[test]
    fn a_cell_click_starts_pidinsert_and_the_tab_survives_a_document_switch() {
        use crate::command::CadCommand;
        let mut app = app();
        let _ = app.dispatch_command("PIDPALETTE");
        // Switch to the valve tab, then click a valve cell.
        let categories = app.pid_symbols.categories().to_vec();
        let valve = categories
            .iter()
            .position(|category| category.id == "valve")
            .expect("阀门分类存在");
        let _ = app.on_pid_palette(PaletteMsg::Tab(valve));
        assert_eq!(app.pid_palette.active, valve);
        let _ = app.on_pid_palette(PaletteMsg::Insert("PID_SYM_valve_gate_valve".into()));
        let command = app.tabs[app.active_tab]
            .active_cmd
            .as_ref()
            .expect("点击格位后有活动命令");
        assert_eq!(command.name(), "PIDINSERT");
        assert!(!command.prompt().is_empty());
        assert!(command.prompt().contains("PID_SYM_valve_gate_valve"));

        // A second document, then back: the panel is still open on the same
        // category.
        let second = app.push_test_document();
        assert_ne!(second, app.tabs[app.active_tab].id);
        let _ = app.automation_op(&format!(r#"{{"op":"activate","document_id":{second}}}"#));
        assert!(app.dock_panel_visible(PanelId::PidPalette), "切到新文档后面板仍打开");
        assert_eq!(app.pid_palette.active, valve, "切到新文档后仍停在原页签");
        let first = app.tabs[0].id;
        let _ = app.automation_op(&format!(r#"{{"op":"activate","document_id":{first}}}"#));
        assert!(app.dock_panel_visible(PanelId::PidPalette), "切回后面板仍打开");
        assert_eq!(app.pid_palette.active, valve, "切回后仍停在原页签");
    }

    /// `PIDINSERT vessel` (the bare id the contract types) installs the same
    /// command as the block name does, and an unknown name is refused without
    /// starting anything.
    #[test]
    fn pidinsert_resolves_the_bare_id_and_refuses_an_unknown_one() {
        use crate::command::CadCommand;
        let mut app = app();
        let _ = app.dispatch_command("PIDINSERT vessel");
        let command = app.tabs[app.active_tab]
            .active_cmd
            .as_ref()
            .expect("PIDINSERT vessel 启动了命令");
        assert_eq!(command.name(), "PIDINSERT");
        assert!(command.prompt().contains("PID_SYM_equipment_vessel"));

        let _ = app.automation_op(r#"{"op":"cancel"}"#);
        let _ = app.dispatch_command("PIDINSERT not_a_real_symbol");
        assert!(
            app.tabs[app.active_tab].active_cmd.is_none(),
            "未知符号名不得启动命令"
        );
    }
}
