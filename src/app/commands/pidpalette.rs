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
//!
//! Two names travel with a placement, and they are not the same name:
//!
//! * the **mapped type** (`VESSEL`) is the symbol library's own many-to-many
//!   mapping (`symbol_type_map` in `assets/pid-symbols.json`), the vocabulary
//!   `VAL-SEM-032` pins;
//! * the **author type** (`EQUIPMENT`) is the coarse type code
//!   `architecture.md` lists as canonical and that `PID_AUTHOR`'s `type` key
//!   stores, because the tag rules are defined per category — `P-{序号}` hangs
//!   off `EQUIPMENT`, not off each vessel.
//!
//! [`author_type_of`] folds the first into the second.

use super::*;
use crate::command::{CmdResult, InputKind};
use crate::pid::persist::PidAuthor;
use crate::pid::symbols::{block_name, SymbolLibrary};
use crate::pid::type_lib::TypeLib;
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
    /// Type code the block maps to in `symbol_type_map`. The commit path
    /// writes the canonical [`Self::author_type`] instead; this one is kept
    /// because it is the library's own answer for the symbol and the tests
    /// pin it, so a future caller (the port/connection work) has it at hand
    /// without resolving the block again.
    #[cfg_attr(not(test), allow(dead_code))]
    type_code: String,
    /// Type code written to `PID_AUTHOR`'s `type` key: the canonical category
    /// code the mapped type rolls up to.
    author_type: String,
    /// Insertion scale, the symbol definition's default.
    scale: f64,
    /// The point the user has picked, once they have.
    point: Option<DVec3>,
}

impl PidInsertCommand {
    pub fn new(block: String, label: String, type_code: String) -> Self {
        Self {
            block,
            label,
            type_code: type_code.clone(),
            author_type: type_code,
            scale: 1.0,
            point: None,
        }
    }

    /// Carry the author type and the symbol's default scale; the palette sets
    /// both once the symbol is resolved.
    pub fn placing(mut self, author_type: String, scale: f64) -> Self {
        self.author_type = author_type;
        self.scale = scale;
        self
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

    /// The insertion scale a placement would use.
    #[cfg(test)]
    pub fn scale_for_test(&self) -> f64 {
        self.scale
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

    fn pid_author_type(&self) -> Option<&str> {
        Some(&self.author_type)
    }

    fn on_point(&mut self, pt: DVec3) -> CmdResult {
        // A pick is remembered, not placed: the placement is the Enter step,
        // which is also where the contract measures the entity count. That
        // leaves the point available for the preview and the
        // `SPACE` / `CTRL+SPACE` steps the insert command adds later.
        self.point = Some(pt);
        CmdResult::NeedPoint
    }

    fn on_enter(&mut self) -> CmdResult {
        // Enter before a point has been picked only re-asks for one: the
        // placement has no default point, and `run_command_line` ends a
        // multi-token line (`PIDINSERT vessel`) with an Enter of its own — a
        // Cancel here would kill the insertion mode the line just started,
        // leaving the caller's next `input point` with no command to feed.
        // Esc is what leaves insertion mode.
        let Some(point) = self.point else {
            return CmdResult::NeedPoint;
        };
        let insert = codec::entities::Insert::new(
            self.block.clone(),
            codec::types::Vector3::new(point.x, point.y, point.z),
        )
        .with_uniform_scale(self.scale)
        .with_rotation(0.0);
        CmdResult::CommitAndExit(codec::EntityType::Insert(insert))
    }
}

/// A symbol the library knows, resolved from a typed name.
struct ResolvedSymbol {
    block: String,
    label: String,
    type_code: String,
    /// Canonical type code for `PID_AUTHOR`; see [`author_type_of`].
    author_type: String,
    /// The symbol definition's default insertion scale.
    scale: f64,
}

/// Fold a symbol's mapped type code up to the canonical category code the
/// P&ID author record stores.
///
/// `architecture.md` fixes the canonical values as upper-case category codes
/// (`EQUIPMENT`, `VALVE`, `INSTRUMENT`, …) and the default tag rules hang off
/// those same codes, so `PID_AUTHOR` records the category, not the leaf. The
/// symbol library's own mapping stays finer grained on purpose (one symbol may
/// serve several types, `VAL-SEM-032`), so the two vocabularies are joined here
/// rather than collapsed into one:
///
/// * a mapped code that the type library knows rolls up to its root
///   (`VESSEL` → `EQUIPMENT`), which is where the tag rule lives;
/// * a mapped code the type library does not carry (`VALVE_GATE`, and
///   `INSTRUMENT` per D16) falls back to the category id in canonical case
///   (`valve` → `VALVE`).
fn author_type_of(types: &TypeLib, library: &SymbolLibrary, block: &str, category: &str) -> String {
    for mapped in library.types_of(block) {
        if let Some(root) = types.ancestors(&mapped).last() {
            return root.clone();
        }
    }
    category.to_ascii_uppercase()
}

/// Resolve `name` against `library`: a block name, or a bare symbol id.
///
/// A bare id that exists in more than one category resolves to the first,
/// which is the category order of `assets/pid-symbols.json` — the same order
/// the palette shows its tabs in.
fn resolve_symbol(library: &SymbolLibrary, name: &str) -> Option<ResolvedSymbol> {
    let types = TypeLib::embedded();
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
                let author_type = author_type_of(&types, library, &block, &category.id);
                return Some(ResolvedSymbol {
                    block,
                    label: symbol.label.clone(),
                    type_code,
                    author_type,
                    scale: symbol.default_scale,
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
            // `unknown_symbol` is the code the contract names, and it is what
            // lets a caller tell "no such symbol" from "a drawing error" with
            // one field instead of matching the prose.
            self.command_line.push_error_coded(
                "unknown_symbol",
                &format!(
                    "PIDINSERT: unknown symbol \"{rest}\" -- the palette lists what \
                     assets/pid-symbols.json defines."
                ),
            );
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
        let command = PidInsertCommand::new(symbol.block, symbol.label, symbol.type_code)
            .placing(symbol.author_type, symbol.scale);
        let prompt = command.prompt();
        self.reset_command_start_state(i);
        self.command_line.push_info(&prompt);
        self.tabs[i].active_cmd = Some(Box::new(command));
        self.finish_dispatch("PIDINSERT")
    }

    /// Write the `PID_AUTHOR` record a placement just committed carries.
    ///
    /// Called by the commit path once the `Insert` has a handle: the XDATA
    /// record needs one, and the tag needs the record to be readable by the
    /// sequence scan, so this runs inside the same undo step as the entity.
    ///
    /// The tag comes from the engine, never from string assembly: the rule for
    /// the type decides the prefix and the sequence position. A generation
    /// failure is not fatal — [`crate::pid::tag_engine::TagView::empty`] is
    /// what the caller stores then (D14), which leaves a manual, `ok`, empty
    /// tag instead of a made-up one.
    pub(in crate::app) fn write_pid_author(&mut self, i: usize, handle: codec::Handle, type_code: &str) {
        use crate::pid::tag_engine;
        let types = TypeLib::embedded();
        let view = tag_engine::generate(
            &types,
            &self.tabs[i].scene.document,
            type_code,
            &std::collections::BTreeMap::new(),
        )
        .unwrap_or_else(|_| tag_engine::TagView::empty());
        let block = match self.tabs[i].scene.document.get_entity(handle) {
            Some(codec::EntityType::Insert(insert)) => insert.block_name.clone(),
            _ => String::new(),
        };
        let author = PidAuthor {
            type_code: type_code.to_string(),
            tag: view.tag,
            tag_source: view.tag_source,
            tag_status: view.tag_status,
            ports: self.pid_symbols.symbol_ports(&block).join(","),
            props: "{}".to_string(),
            ..PidAuthor::default()
        };
        author.store(&mut self.tabs[i].scene.document, handle);
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
        assert_eq!(by_id.author_type, "EQUIPMENT");
        assert_eq!(by_id.label, "容器");
        assert_eq!(by_id.scale, 1.0);
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

    // ── 落点提交（VAL-DRAW-007..013）─────────────────────────────────────
    //
    // 这些用例走 GUI 控制面的真实路径：`run` 起命令、`input` 喂点与回车。
    // 直接调 `on_point` / `on_enter` 会绕过提交机制与撤销步骤，测不出
    // 「实体真的进了文档」这件事。

    /// 一条协议 1 请求，等到它不再是 `accepted` / `running`。
    fn request(app: &mut OpenCADStudio, mut req: serde_json::Value) -> serde_json::Value {
        use std::sync::atomic::{AtomicU64, Ordering};
        static SERIAL: AtomicU64 = AtomicU64::new(0);
        let state = app.control_state();
        req["protocol"] = serde_json::json!(1);
        req["document_id"] = state["document_id"].clone();
        req["revision"] = state["revision"].clone();
        req["client_id"] = serde_json::json!("test");
        if req.get("request_id").is_none() {
            req["request_id"] = serde_json::json!(format!(
                "pid-{}",
                SERIAL.fetch_add(1, Ordering::Relaxed)
            ));
        }
        let (response, task) = app.control_request(req.clone());
        app.drive_headless_task(task).unwrap();
        if matches!(response["status"].as_str(), Some("accepted" | "running")) {
            app.control_request(serde_json::json!({
                "op":"operation",
                "request_id":req["request_id"],
            }))
            .0
        } else {
            response
        }
    }

    /// 起 `PIDINSERT <symbol>`、喂一个世界坐标点、回车结束命令。
    fn place(app: &mut OpenCADStudio, symbol: &str, at: [f64; 3]) -> serde_json::Value {
        let started = request(
            app,
            serde_json::json!({"op":"run","cmd":format!("PIDINSERT {symbol}")}),
        );
        assert_eq!(
            started["ok"], true,
            "PIDINSERT {symbol} 应启动插入模式：{started}"
        );
        let picked = request(
            app,
            serde_json::json!({
                "op":"input",
                "kind":"point",
                "point":at,
                "space":"wcs",
            }),
        );
        assert_eq!(picked["ok"], true, "喂点应被接受：{picked}");
        request(app, serde_json::json!({"op":"input","kind":"enter"}))
    }

    /// 文档里的 `Insert` 实体：`(handle, block, 插入点)`。
    fn inserts(app: &OpenCADStudio) -> Vec<(String, String, [f64; 3])> {
        let document = &app.tabs[app.active_tab].scene.document;
        document
            .entities()
            .filter_map(|entity| match entity {
                codec::EntityType::Insert(insert) => Some((
                    format!("{:X}", insert.common.handle.value()),
                    insert.block_name.clone(),
                    [
                        insert.insert_point.x,
                        insert.insert_point.y,
                        insert.insert_point.z,
                    ],
                )),
                _ => None,
            })
            .collect()
    }

    /// 逻辑实体数，口径与 `{"op":"query"}` 的 `count` 相同：符号库生成的
    /// `PID_SYM_*` 块内图元是符号自身的画法，不算本文档的实体。
    fn entity_count(app: &OpenCADStudio) -> usize {
        let document = &app.tabs[app.active_tab].scene.document;
        let symbol_blocks: Vec<codec::Handle> = document
            .block_records
            .iter()
            .filter(|record| record.name.starts_with(crate::pid::symbols::BLOCK_PREFIX))
            .map(|record| record.handle)
            .collect();
        document
            .entities()
            .filter(|entity| !symbol_blocks.contains(&entity.common().owner_handle))
            .count()
    }

    /// `VAL-DRAW-007` / `008` / `009` / `011` / `013`（第一次插入）：
    /// 落点后实体数 +1，新实体是 `Insert` 指向 `PID_SYM_equipment_vessel`，
    /// 块定义存在，插入点就是喂进去的坐标，且带 `PID_AUTHOR` 的
    /// `type=EQUIPMENT` 与非空位号。
    #[test]
    fn pidinsert_commits_an_insert_carrying_its_author_record() {
        let mut app = app();
        let before = entity_count(&app);
        let placed = place(&mut app, "vessel", [200.0, 150.0, 0.0]);
        assert_eq!(placed["ok"], true, "回车应提交落点：{placed}");
        assert_eq!(
            entity_count(&app),
            before + 1,
            "落点后实体数应为 N+1；实得 {before} → {}",
            entity_count(&app)
        );

        let inserted = inserts(&app);
        assert_eq!(inserted.len(), 1, "应有 1 个 Insert，实得 {inserted:?}");
        let (handle, block, at) = &inserted[0];
        assert_eq!(block, "PID_SYM_equipment_vessel");
        assert!((at[0] - 200.0).abs() <= 0.5, "插入点 x，实得 {at:?}");
        assert!((at[1] - 150.0).abs() <= 0.5, "插入点 y，实得 {at:?}");
        assert!(
            app.tabs[app.active_tab]
                .scene
                .document
                .block_records
                .contains(block),
            "块定义 {block} 必须存在"
        );
        // 块定义不是空壳：符号的图元与端口都在里面，否则 `Insert` 画不出
        // 任何东西。vessel 的 `geometry` 是 polyline + text，`ports` 是 2 个
        // Point，合计 4 个实体（D29 的口径）。
        let members = app.tabs[app.active_tab]
            .scene
            .document
            .block_records
            .get(block)
            .expect("块存在")
            .entity_handles
            .len();
        assert_eq!(
            members, 4,
            "vessel 块内应有 4 个实体（geometry 2 + ports 2），实得 {members}"
        );

        let author = PidAuthor::load(
            &app.tabs[app.active_tab].scene.document,
            codec::Handle::new(u64::from_str_radix(handle, 16).unwrap()),
        )
        .expect("新实体必须带 PID_AUTHOR 记录");
        assert_eq!(
            author.type_code, "EQUIPMENT",
            "type 键存的是规范类型编码，不是符号映射里的叶子类型"
        );
        assert!(
            !author.tag.is_empty(),
            "位号不得为空：应形如 P-1001，实得 {:?}",
            author.tag
        );
        assert!(author.tag.starts_with("P-"), "设备位号前缀 P-，实得 {:?}", author.tag);
        assert_eq!(author.tag_source, "auto");
        assert_eq!(author.tag_status, "ok");
        assert_eq!(author.ports, "in1,out1", "端口角色来自符号定义");
    }

    /// `VAL-DRAW-013`：连续插入两次得到两个独立实体，插入点各自贴着自己
    /// 喂的坐标，位号按插入顺序递增（`VAL-DRAW-020` 的序号连续性也在这里
    /// 顺带锁定 —— 第二条能拿到 P-1002 说明第一条的记录在提交时就落盘了）。
    #[test]
    fn two_consecutive_inserts_are_two_independent_entities() {
        let mut app = app();
        let before = entity_count(&app);
        place(&mut app, "vessel", [200.0, 150.0, 0.0]);
        place(&mut app, "vessel", [400.0, 150.0, 0.0]);
        assert_eq!(entity_count(&app), before + 2, "两次落点应 +2");

        let inserted = inserts(&app);
        assert_eq!(inserted.len(), 2, "应有两个 Insert，实得 {inserted:?}");
        assert_ne!(inserted[0].0, inserted[1].0, "两个 handle 必须不同");
        assert_eq!(inserted[0].1, "PID_SYM_equipment_vessel");
        assert_eq!(inserted[1].1, "PID_SYM_equipment_vessel");
        assert!((inserted[0].2[0] - 200.0).abs() <= 0.5, "第一个的 x");
        assert!((inserted[1].2[0] - 400.0).abs() <= 0.5, "第二个的 x");

        let tags: Vec<String> = inserted
            .iter()
            .map(|(handle, _, _)| {
                PidAuthor::load(
                    &app.tabs[app.active_tab].scene.document,
                    codec::Handle::new(u64::from_str_radix(handle, 16).unwrap()),
                )
                .expect("每条记录都在")
                .tag
            })
            .collect();
        assert_eq!(
            tags,
            vec!["P-1001".to_string(), "P-1002".to_string()],
            "同类型位号按插入顺序递增，序号连续"
        );
    }

    /// `VAL-DRAW-012`：未知符号名被拒、带机器可读的 `code`、且不建实体。
    #[test]
    fn pidinsert_refuses_an_unknown_symbol_with_a_code_and_no_entity() {
        let mut app = app();
        let before = entity_count(&app);
        let refused = request(
            &mut app,
            serde_json::json!({"op":"run","cmd":"PIDINSERT not_a_real_symbol"}),
        );
        assert_eq!(refused["ok"], false, "未知符号名必须被拒：{refused}");
        assert_eq!(
            refused["code"], "unknown_symbol",
            "拒绝要带机器可读的 code，而不是只有文案：{refused}"
        );
        assert_eq!(entity_count(&app), before, "被拒后实体数不变");
        assert!(inserts(&app).is_empty(), "被拒后不得留下 Insert");
        assert!(
            app.tabs[app.active_tab].active_cmd.is_none(),
            "被拒后不得进入插入模式"
        );
    }

    /// 落点提交是一个可撤销步骤：撤销把实体和它的 `PID_AUTHOR` 一起收走，
    /// 重做再放回来。XDATA 与实体同属一步是这条用例的意义 —— 只回滚几何
    /// 会留下指向不存在实体的记录。
    #[test]
    fn undo_takes_the_insert_and_its_author_record_together() {
        let mut app = app();
        let before = entity_count(&app);
        place(&mut app, "vessel", [200.0, 150.0, 0.0]);
        let handle = codec::Handle::new(
            u64::from_str_radix(&inserts(&app)[0].0, 16).expect("handle 是十六进制"),
        );
        assert!(PidAuthor::load(&app.tabs[app.active_tab].scene.document, handle).is_some());

        let _ = app.update(crate::app::Message::Undo);
        assert_eq!(entity_count(&app), before, "撤销后回到起始实体数");
        assert!(inserts(&app).is_empty(), "撤销后 Insert 消失");
        assert!(
            PidAuthor::load(&app.tabs[app.active_tab].scene.document, handle).is_none(),
            "撤销后 PID_AUTHOR 记录不得残留"
        );

        let _ = app.update(crate::app::Message::Redo);
        assert_eq!(entity_count(&app), before + 1, "重做后实体回来");
        assert!(
            PidAuthor::load(&app.tabs[app.active_tab].scene.document, handle).is_some(),
            "重做后 PID_AUTHOR 记录回来"
        );
    }

    /// 阀门符号存的是它自己的类别编码：`VALVE_GATE` 不在内置类型树里
    /// （D16），所以 [`author_type_of`] 回落到分类 id 的规范写法。
    ///
    /// 位号此时为空是已知且预期的：内置类型树没有 `VALVE` 节点，`generate`
    /// 返回 `TAG_TYPE_UNKNOWN`，落盘的是 `TagView::empty()`（D14）。阀门
    /// 位号（`V-1001`）连同默认规则表播种是 `drawing-tag-display` 的前置
    /// 条件，`assets/pid-types.json` 由 `pid-type-lib` 拥有（AGENTS.md 3d.4），
    /// 本 feature 不改它。
    #[test]
    fn a_valve_placement_records_the_valve_category_code() {
        let mut app = app();
        place(&mut app, "gate_valve", [800.0, 150.0, 0.0]);
        let inserted = inserts(&app);
        assert_eq!(inserted.len(), 1, "应插入 1 个阀门，实得 {inserted:?}");
        assert_eq!(inserted[0].1, "PID_SYM_valve_gate_valve");
        let author = PidAuthor::load(
            &app.tabs[app.active_tab].scene.document,
            codec::Handle::new(u64::from_str_radix(&inserted[0].0, 16).unwrap()),
        )
        .expect("阀门也带 PID_AUTHOR");
        assert_eq!(author.type_code, "VALVE");
        assert_eq!(
            author.ports, "in1,out1",
            "阀门端口同样来自符号定义"
        );
    }

    /// 符号的缺省缩放来自定义，不是硬编码的 1.0。
    #[test]
    fn a_placement_carries_the_symbols_default_scale() {
        use crate::command::CadCommand;
        let command = PidInsertCommand::new(
            "PID_SYM_equipment_vessel".into(),
            "容器".into(),
            "VESSEL".into(),
        )
        .placing("EQUIPMENT".into(), 2.5);
        assert_eq!(command.pid_author_type(), Some("EQUIPMENT"));
        assert_eq!(command.scale_for_test(), 2.5);
    }
}
