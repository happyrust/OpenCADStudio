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
    /// Preview rotation, radians. `SPACE` adds a quarter turn
    /// (`VAL-DRAW-015`); the placement is committed with the same angle the
    /// preview showed.
    rotation: f64,
    /// Block-local points the placement can hang from: the block's own
    /// insertion base first, then the symbol's ports. `CTRL+SPACE` walks them
    /// (`VAL-DRAW-016`).
    anchors: Vec<DVec3>,
    /// Which of [`Self::anchors`] the preview hangs from.
    anchor_index: usize,
    /// The symbol's block-local geometry, tessellated once when the command
    /// starts. The preview maps it through the same scale / rotation / anchor
    /// the placement uses, so what the user sees is what Enter commits.
    preview: Vec<crate::scene::WireModel>,
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
            rotation: 0.0,
            anchors: vec![DVec3::ZERO],
            anchor_index: 0,
            preview: Vec::new(),
        }
    }

    /// The block's own geometry, tessellated in block-local coordinates. The
    /// host resolves it once (`refresh_point_pick_scene`) and the preview
    /// maps it per cursor position.
    pub fn with_preview(mut self, preview: Vec<crate::scene::WireModel>) -> Self {
        self.preview = preview;
        self
    }

    /// The block-local points `CTRL+SPACE` walks: the block base, then the
    /// symbol's ports in definition order.
    pub fn with_anchors(mut self, ports: Vec<DVec3>) -> Self {
        let mut anchors = Vec::with_capacity(ports.len() + 1);
        anchors.push(DVec3::ZERO);
        for port in ports {
            if port.is_finite() && !anchors.iter().any(|a| a.distance(port) < 1e-12) {
                anchors.push(port);
            }
        }
        self.anchors = anchors;
        self.anchor_index = 0;
        self
    }

    /// The block-local point the placement hangs from.
    fn anchor(&self) -> DVec3 {
        self.anchors
            .get(self.anchor_index)
            .copied()
            .unwrap_or(DVec3::ZERO)
    }

    /// World offset from the anchor to the insertion point: the rotation and
    /// scale the placement applies, so `cursor - anchor_offset()` is the
    /// insertion point that puts the anchor under the cursor.
    fn anchor_offset(&self) -> DVec3 {
        let (sin, cos) = self.rotation.sin_cos();
        let x = self.anchor().x * self.scale;
        let y = self.anchor().y * self.scale;
        DVec3::new(x * cos - y * sin, x * sin + y * cos, 0.0)
    }

    /// The placement for a cursor at `cursor`, with the preview's rotation,
    /// scale and anchor.
    fn insert_at(&self, cursor: DVec3) -> codec::entities::Insert {
        let at = cursor - self.anchor_offset();
        codec::entities::Insert::new(
            self.block.clone(),
            codec::types::Vector3::new(at.x, at.y, at.z),
        )
        .with_uniform_scale(self.scale)
        .with_rotation(self.rotation)
    }

    /// `SPACE`: a quarter turn, kept in `[0, 2π)`.
    fn rotate_quarter_turn(&mut self) {
        use std::f64::consts::{FRAC_PI_2, TAU};
        self.rotation = (self.rotation + FRAC_PI_2) % TAU;
    }

    /// `CTRL+SPACE`: move to the next anchor. False when the symbol offers no
    /// alternative — the command then has nothing to switch to.
    fn advance_anchor(&mut self) -> bool {
        if self.anchors.len() < 2 {
            return false;
        }
        self.anchor_index = (self.anchor_index + 1) % self.anchors.len();
        true
    }

    /// The preview rotation, for tests.
    #[cfg(test)]
    pub fn rotation_for_test(&self) -> f64 {
        self.rotation
    }

    /// The preview anchor, for tests.
    #[cfg(test)]
    pub fn anchor_for_test(&self) -> DVec3 {
        self.anchor()
    }

    /// The insertion point a cursor would produce, for tests.
    #[cfg(test)]
    pub fn insertion_point_for_test(&self, cursor: DVec3) -> DVec3 {
        cursor - self.anchor_offset()
    }

    /// The preview geometry, for tests.
    #[cfg(test)]
    pub fn preview_for_test(&self) -> &[crate::scene::WireModel] {
        &self.preview
    }

    /// 一个块内点落在屏幕预览上的世界坐标。
    ///
    /// `insert_transform_with_base` 把块点 `P` 放到
    /// `insert_point + R·S·(P − base)`，而符号块的基点是原点，所以把插入点
    /// 取成让插入基准点落在光标处（`cursor − R·S·anchor`）之后，块点就落在
    /// `cursor + R·S·(P − anchor)`。预览与落点共用这一条换算，两处不可能
    /// 不一致（`VAL-DRAW-014`）。
    pub fn preview_point(&self, cursor: DVec3, local: DVec3) -> DVec3 {
        let (sin, cos) = self.rotation.sin_cos();
        let d = local - self.anchor();
        let x = d.x * self.scale;
        let y = d.y * self.scale;
        cursor + DVec3::new(x * cos - y * sin, x * sin + y * cos, d.z)
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

    /// The symbol ghost follows the cursor. Mapping the block-local preview
    /// through the placement's own transform is what makes the screenshot
    /// diff and the committed `Insert` agree (`VAL-DRAW-014`).
    fn on_preview_wires(&mut self, pt: DVec3) -> Vec<crate::scene::WireModel> {
        if self.preview.is_empty() || !pt.is_finite() {
            return Vec::new();
        }
        self.preview
            .iter()
            .map(|wire| wire.mapped(|p| self.preview_point(pt, p)))
            .collect()
    }

    /// `SPACE` rotates the preview a quarter turn instead of finishing the
    /// step (`VAL-DRAW-015`).
    fn rotate_on_space(&mut self) -> bool {
        if self.preview.is_empty() && self.point.is_none() {
            // Nothing to turn yet — leave Space meaning Enter so the command
            // still behaves like every other one before its first point.
            return false;
        }
        self.rotate_quarter_turn();
        true
    }

    /// `CTRL+SPACE` walks the placement's insertion points
    /// (`VAL-DRAW-016`).
    fn cycle_insertion_point(&mut self) -> bool {
        self.advance_anchor()
    }

    /// The Ctrl edge did the switch; the Space that follows belongs to it.
    fn takes_ctrl_space(&self) -> bool {
        self.anchors.len() > 1
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
        CmdResult::CommitAndExit(codec::EntityType::Insert(self.insert_at(point)))
    }
}

/// ATTRIB tag of the label that shows a placement's `PID_AUTHOR` tag.
pub(crate) const PID_TAG_ATTRIBUTE: &str = "PID_TAG";

/// Text height of the tag label, drawing units.
const PID_TAG_HEIGHT: f64 = 4.0;

/// Gap between the symbol's highest point and the bottom of its tag label.
const PID_TAG_GAP: f64 = 3.0;

/// Fallback label offset for a symbol whose block has no measurable extent.
const PID_TAG_FALLBACK_RISE: f64 = 15.0;

/// Highest block-local y of the symbol's own geometry, so the label sits
/// clear of it whatever the symbol's size.
fn symbol_top(document: &codec::Document, block: &str) -> Option<f64> {
    document
        .entities_in_block(block)
        .filter(|entity| !matches!(entity, codec::EntityType::Point(_)))
        .map(|entity| entity.as_entity().bounding_box().max.y)
        .filter(|y| y.is_finite())
        .reduce(f64::max)
}

/// Make the Insert at `handle` carry an ATTRIB showing its `PID_AUTHOR` tag
/// display string (`？` while the tag is duplicated), centred above the
/// symbol. The label is an attribute of the reference, so it moves, rotates
/// and is erased with the symbol, and it adds no entity of its own to the
/// document. Returns whether the reference carries a P&ID tag label.
pub(crate) fn sync_pid_tag_attribute(document: &mut codec::Document, handle: codec::Handle) -> bool {
    use codec::entities::{AttributeEntity, HorizontalAlignment, VerticalAlignment};
    use codec::types::Vector3;
    let Some(author) = PidAuthor::load(document, handle) else {
        return false;
    };
    let display =
        crate::pid::tag_engine::TagView::new(author.tag, author.tag_source, author.tag_status)
            .display;
    let Some(codec::EntityType::Insert(insert)) = document.get_entity(handle) else {
        return false;
    };
    // ATTRIB geometry is stored in world coordinates, so the block-local
    // anchor is carried through the reference's own scale and rotation.
    let rise = symbol_top(document, &insert.block_name)
        .map(|top| top + PID_TAG_GAP)
        .unwrap_or(PID_TAG_FALLBACK_RISE);
    let scale = insert.y_scale().abs();
    let (sin, cos) = insert.rotation.sin_cos();
    let local_y = rise * scale;
    let at = Vector3::new(
        insert.insert_point.x - local_y * sin,
        insert.insert_point.y + local_y * cos,
        insert.insert_point.z,
    );
    let height = PID_TAG_HEIGHT * scale;
    let rotation = insert.rotation;
    let layer = insert.common.layer.clone();
    let Some(codec::EntityType::Insert(insert)) = document.get_entity_mut(handle) else {
        return false;
    };
    let attribute = match insert
        .attributes
        .iter_mut()
        .position(|attribute| attribute.tag == PID_TAG_ATTRIBUTE)
    {
        Some(index) => &mut insert.attributes[index],
        None => {
            let mut attribute =
                AttributeEntity::new(PID_TAG_ATTRIBUTE.to_string(), String::new());
            attribute.common.layer = layer;
            attribute.horizontal_alignment = HorizontalAlignment::Center;
            attribute.vertical_alignment = VerticalAlignment::Bottom;
            insert.attributes.push(attribute);
            insert.attributes.last_mut().expect("just pushed")
        }
    };
    attribute.value = display;
    attribute.insertion_point = at;
    attribute.alignment_point = at;
    attribute.height = height;
    attribute.rotation = rotation;
    crate::scene::Scene::adopt_attributes(document, handle);
    true
}

/// A symbol the library knows, resolved from a typed name.
///
/// `pub(super)` so the batch placement command (`super::pidbulk`) resolves a
/// symbol through this one function instead of carrying a second copy of the
/// symbol-library rules.
pub(super) struct ResolvedSymbol {
    pub(super) block: String,
    pub(super) label: String,
    pub(super) type_code: String,
    /// Canonical type code for `PID_AUTHOR`; see [`author_type_of`].
    pub(super) author_type: String,
    /// The symbol definition's default insertion scale.
    pub(super) scale: f64,
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

/// [`resolve_symbol`] for the batch placement path, which lives in a sibling
/// module and so cannot see the private one.
pub(super) fn resolve_symbol_for_batch(
    library: &SymbolLibrary,
    name: &str,
) -> Option<ResolvedSymbol> {
    resolve_symbol(library, name)
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
    ///
    /// The symbol's own geometry and ports are tessellated here, in block-local
    /// coordinates, before the block definition exists: the preview must show
    /// the symbol from the moment insertion mode starts, and a drawing that has
    /// never held a symbol has no `PID_SYM_*` block yet (the commit path defines
    /// it). `SymbolDefinition` is the same source that definition is built from,
    /// so preview and placement cannot drift apart.
    fn start_pid_insert(&mut self, i: usize, symbol: ResolvedSymbol) -> Task<Message> {
        let preview = self.pid_symbols.symbol(&symbol.block).map_or_else(Vec::new, |definition| {
            self.tabs[i].scene.wires_for_entities(&definition.block_entities())
        });
        let ports = self
            .pid_symbols
            .symbol(&symbol.block)
            .map(|definition| {
                definition
                    .ports
                    .iter()
                    .map(|port| DVec3::new(port.at[0], port.at[1], 0.0))
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        let command = PidInsertCommand::new(symbol.block, symbol.label, symbol.type_code)
            .placing(symbol.author_type, symbol.scale)
            .with_anchors(ports)
            .with_preview(preview);
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
        sync_pid_tag_attribute(&mut self.tabs[i].scene.document, handle);
    }

    /// After `PID_AUTHOR` records on `edited` were rewritten from outside the
    /// placement path, re-run the uniqueness check across the document and
    /// redraw every tag label whose text may have changed. The caller owns
    /// the undo step.
    pub(in crate::app) fn refresh_pid_tags(&mut self, i: usize, edited: &[codec::Handle]) {
        let document = &mut self.tabs[i].scene.document;
        let mut touched =
            crate::pid::tag_engine::refresh_statuses(&TypeLib::embedded(), document, edited);
        for handle in edited {
            if !touched.contains(handle) {
                touched.push(*handle);
            }
        }
        touched.retain(|handle| sync_pid_tag_attribute(document, *handle));
        if touched.is_empty() {
            return;
        }
        let changes: Vec<_> = touched
            .iter()
            .map(|handle| (*handle, crate::scene::ChangeKind::Modified))
            .collect();
        self.tabs[i].scene.bump_entities(&changes);
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
    /// 位号来自 A.9 默认规则表：`VALVE` 不在类型库中，`tag_engine` 按作者类型
    /// 编码查默认表得 `V-{序号}` / 起始 1001，所以首个阀门是 `V-1001`。
    /// `assets/pid-types.json` 由 `pid-type-lib` 拥有（AGENTS.md 3d.4），本
    /// feature 不改它 —— 兜底落在规则解析处。
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
        assert_eq!(author.tag, "V-1001", "首个阀门位号来自 A.9 默认规则表");
        assert_eq!(author.tag_source, "auto");
        assert_eq!(author.tag_status, "ok");
    }

    /// `VAL-DRAW-019` / `020` / `023`：三个容器依次得到 `P-1001`、
    /// `P-1002`、`P-1003`，随后的阀门从 `V-1001` 起（不接续 `P-1003`），
    /// 已有 `P-*` 不变。
    #[test]
    fn consecutive_placements_increment_and_each_category_starts_at_its_own_origin() {
        let mut app = app();
        let mut tags = Vec::new();
        for x in [200.0_f64, 400.0, 600.0] {
            place(&mut app, "vessel", [x, 150.0, 0.0]);
            tags.push(last_author(&app));
        }
        let expected = [
            ("P-1001", "auto", "ok"),
            ("P-1002", "auto", "ok"),
            ("P-1003", "auto", "ok"),
        ];
        for (got, want) in tags.iter().zip(expected.iter()) {
            assert_eq!(
                (got.tag.as_str(), got.tag_source.as_str(), got.tag_status.as_str()),
                *want,
                "设备位号序列"
            );
        }

        place(&mut app, "gate_valve", [800.0, 150.0, 0.0]);
        let valve = last_author(&app);
        assert_eq!(valve.type_code, "VALVE");
        assert_eq!(valve.tag, "V-1001", "阀门从自己的起始编码开始");
        assert_eq!(valve.tag_status, "ok");

        // 已有三个 P-* 位号不变。
        let after: Vec<String> = inserts(&app)
            .iter()
            .filter(|(_, block, _)| block == "PID_SYM_equipment_vessel")
            .map(|(handle, _, _)| {
                PidAuthor::load(
                    &app.tabs[app.active_tab].scene.document,
                    codec::Handle::new(u64::from_str_radix(handle, 16).unwrap()),
                )
                .expect("容器带记录")
                .tag
            })
            .collect();
        assert_eq!(after, vec!["P-1001", "P-1002", "P-1003"]);
    }

    /// 落点实体上 `PID_TAG` 属性的显示值。
    fn tag_label(app: &OpenCADStudio, handle: &str) -> Option<String> {
        let handle = codec::Handle::new(u64::from_str_radix(handle, 16).unwrap());
        match app.tabs[app.active_tab].scene.document.get_entity(handle) {
            Some(codec::EntityType::Insert(insert)) => insert
                .attributes
                .iter()
                .find(|attribute| attribute.tag == PID_TAG_ATTRIBUTE)
                .map(|attribute| attribute.value.clone()),
            _ => None,
        }
    }

    /// `VAL-DRAW-022`：每个落点都带一个显示位号的属性，有 handle、位于
    /// 符号上方且不压住符号几何；三个相邻符号的标签互不重叠，且不增加
    /// 逻辑实体数。
    #[test]
    fn each_placement_shows_its_tag_above_the_symbol() {
        let mut app = app();
        let before = entity_count(&app);
        for x in [200.0_f64, 400.0, 600.0] {
            place(&mut app, "vessel", [x, 150.0, 0.0]);
        }
        assert_eq!(entity_count(&app), before + 3, "标签不是独立实体");
        let placed = inserts(&app);
        let labels: Vec<_> = placed
            .iter()
            .map(|(handle, _, _)| tag_label(&app, handle).expect("落点带位号标签"))
            .collect();
        assert_eq!(labels, vec!["P-1001", "P-1002", "P-1003"]);

        let document = &app.tabs[app.active_tab].scene.document;
        for entity in document.entities() {
            let codec::EntityType::Insert(insert) = entity else {
                continue;
            };
            let attribute = &insert.attributes[0];
            assert!(!attribute.common.handle.is_null(), "标签有自己的 handle");
            assert_eq!(attribute.common.owner_handle, insert.common.handle);
            assert!(insert.seqend_handle.is_some(), "属性序列有 SEQEND");
            assert_eq!(attribute.insertion_point.x, insert.insert_point.x, "水平居中");
            // vessel 的几何上沿是块内 y=12；标签底边在它之上。
            assert!(attribute.insertion_point.y > insert.insert_point.y + 12.0);
            assert!(attribute.insertion_point.y < insert.insert_point.y + 40.0);
        }
        let rendered: usize = document
            .entities()
            .filter_map(|entity| match entity {
                codec::EntityType::Insert(insert) => Some(insert),
                _ => None,
            })
            .map(|insert| {
                crate::entities::insert::insert_attribute_entities(
                    document,
                    insert,
                    1.0,
                    crate::scene::BlockScalePolicy::FromInsert,
                )
                .len()
            })
            .sum();
        assert_eq!(rendered, 3, "三个标签都走插入属性的渲染路径");

        // 聚焦截图按实体框取景时要把标签框进去，否则标签落在视图边缘外。
        let scene = &app.tabs[app.active_tab].scene;
        for (handle, _, at) in &placed {
            let handle = codec::Handle::new(u64::from_str_radix(handle, 16).unwrap());
            let (lo, hi) = scene.insert_attribute_bounds(handle).expect("标签有包围盒");
            assert!(lo.x < at[0] && hi.x > at[0], "标签框水平跨过符号中线");
            assert!(lo.y > at[1] && hi.y < at[1] + 40.0, "标签框在符号上方");
        }
    }

    /// `VAL-DRAW-021`：把第 2 个实体的位号经 `xdata_set` 手工设为 `P-1001`，
    /// 该实体 `tag_status=duplicated`、`tag` 保留 `P-1001`，画布标签显示 `？`；
    /// 第 1 个实体不受影响。撤销回到 `P-1002`，重做再回到 `？`。
    #[test]
    fn a_manual_duplicate_shows_the_conflict_mark_and_undoes() {
        let mut app = app();
        for x in [200.0_f64, 400.0, 600.0] {
            place(&mut app, "vessel", [x, 150.0, 0.0]);
        }
        let placed = inserts(&app);
        let (first, second) = (placed[0].0.clone(), placed[1].0.clone());
        let handle = codec::Handle::new(u64::from_str_radix(&second, 16).unwrap());
        let mut author =
            PidAuthor::load(&app.tabs[app.active_tab].scene.document, handle).unwrap();
        author.tag = "P-1001".into();
        author.tag_source = "manual".into();
        let data: Vec<serde_json::Value> = crate::pid::persist::encode(&author)
            .iter()
            .filter_map(|value| match value {
                codec::xdata::XDataValue::String(text) => {
                    Some(serde_json::json!({"code":1000,"value":text}))
                }
                _ => None,
            })
            .collect();
        let set = request(
            &mut app,
            serde_json::json!({
                "op":"xdata_set",
                "handles":[second],
                "app":"PID_AUTHOR",
                "data":data,
            }),
        );
        assert_eq!(set["ok"], true, "xdata_set 应成功：{set}");

        let document = &app.tabs[app.active_tab].scene.document;
        let edited = PidAuthor::load(document, handle).unwrap();
        assert_eq!(edited.tag, "P-1001", "冲突时位号原值保留");
        assert_eq!(edited.tag_status, "duplicated");
        assert_eq!(tag_label(&app, &second).as_deref(), Some("？"));
        assert_eq!(tag_label(&app, &first).as_deref(), Some("P-1001"));

        let undone = request(&mut app, serde_json::json!({"op":"undo"}));
        assert_eq!(undone["ok"], true, "undo：{undone}");
        assert_eq!(tag_label(&app, &second).as_deref(), Some("P-1002"));
        let redone = request(&mut app, serde_json::json!({"op":"redo"}));
        assert_eq!(redone["ok"], true, "redo：{redone}");
        assert_eq!(tag_label(&app, &second).as_deref(), Some("？"));
    }

    /// 最近一次落点实体的 `PID_AUTHOR` 记录。
    fn last_author(app: &OpenCADStudio) -> PidAuthor {
        let inserted = inserts(app);
        let (handle, _, _) = inserted.last().expect("至少插入过一个符号");
        PidAuthor::load(
            &app.tabs[app.active_tab].scene.document,
            codec::Handle::new(u64::from_str_radix(handle, 16).unwrap()),
        )
        .expect("落点实体必须带 PID_AUTHOR")
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

    /// 带预览与端口的命令，等价于 `PIDINSERT vessel` 在文档里的样子：
    /// vessel 的 geometry 是 16×24 的矩形（块内 -8..8 / -12..12），端口是
    /// `in1 (0,-12)` 与 `out1 (0,12)`。
    fn interactive_command() -> PidInsertCommand {
        let library = library();
        let symbol = resolve_symbol(&library, "vessel").expect("vessel 存在");
        let scene = crate::scene::Scene::new();
        let definition = library.symbol(&symbol.block).expect("符号定义存在");
        let preview = scene.wires_for_entities(&definition.block_entities());
        let ports = definition
            .ports
            .iter()
            .map(|port| DVec3::new(port.at[0], port.at[1], 0.0))
            .collect::<Vec<_>>();
        PidInsertCommand::new(symbol.block, symbol.label, symbol.type_code)
            .placing(symbol.author_type, symbol.scale)
            .with_anchors(ports)
            .with_preview(preview)
    }

    /// 预览外形的包围盒（世界坐标）。
    ///
    /// 取包围盒面积最大的那条线框，也就是符号的主体图形：`vessel` 的块里除了
    /// 16×24 的矩形还有一个居中的 `V` 文字，字形线框并不以块基点对称，会把
    /// 整体包围盒的中心拉偏。契约量的是符号图形本身的位置与宽高比，所以量
    /// 主体那一条。
    fn preview_bounds(wires: &[crate::scene::WireModel]) -> (DVec3, DVec3) {
        let bounds_of = |wire: &crate::scene::WireModel| {
            let mut min = DVec3::splat(f64::INFINITY);
            let mut max = DVec3::splat(f64::NEG_INFINITY);
            for (i, high) in wire.points.iter().enumerate() {
                let low = wire.points_low.get(i).copied().unwrap_or([0.0; 3]);
                let p = DVec3::new(
                    high[0] as f64 + low[0] as f64,
                    high[1] as f64 + low[1] as f64,
                    high[2] as f64 + low[2] as f64,
                );
                min = min.min(p);
                max = max.max(p);
            }
            (min, max)
        };
        wires
            .iter()
            .map(bounds_of)
            .filter(|(min, max)| min.is_finite() && max.is_finite())
            .max_by(|(min_a, max_a), (min_b, max_b)| {
                let area = |min: &DVec3, max: &DVec3| {
                    (max.x - min.x).abs() * (max.y - min.y).abs()
                };
                area(min_a, max_a)
                    .partial_cmp(&area(min_b, max_b))
                    .unwrap_or(std::cmp::Ordering::Equal)
            })
            .expect("预览至少有一条线框")
    }

    /// `VAL-DRAW-014`：喂两个不同的点，预览出现在两个不同的位置，位移与
    /// 两点世界坐标之差一致（预览不建实体）。
    #[test]
    fn the_preview_follows_the_cursor() {
        use crate::command::CadCommand;
        let mut command = interactive_command();
        assert!(
            !command.preview_for_test().is_empty(),
            "命令自带块内几何，否则预览无从生成"
        );
        let first = command.on_preview_wires(DVec3::new(200.0, 150.0, 0.0));
        assert!(!first.is_empty(), "预览必须有几何，否则截图里看不到东西");
        let (min1, max1) = preview_bounds(&first);
        let center1 = (min1 + max1) * 0.5;

        let second = command.on_preview_wires(DVec3::new(600.0, 400.0, 0.0));
        let (min2, max2) = preview_bounds(&second);
        let center2 = (min2 + max2) * 0.5;
        let moved = center2 - center1;
        assert!(
            (moved.x - 400.0).abs() < 1e-6 && (moved.y - 250.0).abs() < 1e-6,
            "预览位移必须等于喂点位移 (400,250)，实得 {moved:?}"
        );
        assert!(
            center1.x > 0.0 && center1.y > 0.0,
            "预览应出现在第一个喂点附近，实得 {center1:?}"
        );
    }

    /// `VAL-DRAW-015`：空格使包围盒宽高互换、中心不变。
    #[test]
    fn space_rotates_the_preview_a_quarter_turn_about_its_centre() {
        use crate::command::CadCommand;
        let mut command = interactive_command();
        let at = DVec3::new(600.0, 400.0, 0.0);
        // vessel 的块内矩形是 (-8,-12)..(8,12)：宽 16、高 24。
        let corners = [
            DVec3::new(-8.0, -12.0, 0.0),
            DVec3::new(8.0, -12.0, 0.0),
            DVec3::new(8.0, 12.0, 0.0),
            DVec3::new(-8.0, 12.0, 0.0),
        ];
        let span = |command: &PidInsertCommand| {
            let pts: Vec<DVec3> = corners
                .iter()
                .map(|corner| command.preview_point(at, *corner))
                .collect();
            let min = pts.iter().copied().fold(DVec3::splat(f64::INFINITY), DVec3::min);
            let max = pts.iter().copied().fold(DVec3::splat(f64::NEG_INFINITY), DVec3::max);
            (max - min, (min + max) * 0.5)
        };
        let before = span(&command);
        assert!(before.0.y > before.0.x, "vessel 块内是竖向矩形（16×24），旋转前应更高");

        assert!(command.rotate_on_space(), "插入模式必须吃掉空格");
        assert!(
            (command.rotation_for_test() - std::f64::consts::FRAC_PI_2).abs() < 1e-12,
            "空格应加 90°，实得 {}",
            command.rotation_for_test()
        );

        let after = span(&command);
        assert!(
            (after.0.x - before.0.y).abs() < 1e-6 && (after.0.y - before.0.x).abs() < 1e-6,
            "旋转后宽高应互换：{:?} -> {:?}",
            before.0,
            after.0
        );
        assert!(
            after.1.distance(before.1) < 1e-6,
            "旋转不移动预览中心：{:?} -> {:?}",
            before.1,
            after.1
        );
        // 预览线框也跟着转：同一个喂点下，旋转前后几何不同。
        let rotated = command.on_preview_wires(at);
        assert!(!rotated.is_empty(), "旋转后预览仍在");
    }

    /// `VAL-DRAW-016`：Ctrl+空格切换插入点，预览相对光标的偏移方向改变，
    /// 且不改变旋转角。
    #[test]
    fn ctrl_space_switches_the_insertion_point() {
        use crate::command::CadCommand;
        let mut command = interactive_command();
        let at = DVec3::new(600.0, 400.0, 0.0);
        let corner = DVec3::new(-8.0, -12.0, 0.0);
        let before = command.preview_point(at, corner);
        assert_eq!(command.anchor_for_test(), DVec3::ZERO, "默认插入点是块基点");

        assert!(command.cycle_insertion_point(), "有端口时必须能切换");
        assert_eq!(
            command.anchor_for_test(),
            DVec3::new(0.0, -12.0, 0.0),
            "第一个端口 in1 在 (0,-12)"
        );

        let after = command.preview_point(at, corner);
        let shift = (after - before).length();
        assert!(shift >= 3.0, "预览相对光标的位置必须明显改变，实得 {shift}");
        assert!(
            (command.rotation_for_test()).abs() < 1e-12,
            "切换插入点不该旋转"
        );
        // 切换后的插入点让端口落在光标上：块点 anchor 映射到喂点本身。
        assert!(
            command
                .preview_point(at, command.anchor_for_test())
                .distance(at)
                < 1e-9,
            "插入基准点必须落在光标处"
        );
    }

    /// `VAL-DRAW-017` / `VAL-DRAW-018`：Esc 取消不建实体；取消后重新插入
    /// 仍能正常提交，落点就是喂进去的坐标。
    #[test]
    fn escape_cancels_without_a_placement_and_a_later_insert_still_commits() {
        let mut app = app();
        let before = entity_count(&app);

        let started = request(
            &mut app,
            serde_json::json!({"op":"run","cmd":"PIDINSERT vessel"}),
        );
        assert_eq!(started["ok"], true, "{started}");
        let picked = request(
            &mut app,
            serde_json::json!({"op":"input","kind":"point","point":[200.0,150.0,0.0],"space":"wcs"}),
        );
        assert_eq!(picked["ok"], true, "{picked}");
        let cancelled = request(&mut app, serde_json::json!({"op":"cancel"}));
        assert_eq!(cancelled["ok"], true, "{cancelled}");
        let state = request(&mut app, serde_json::json!({"op":"state"}));
        assert!(
            state["command"].is_null(),
            "取消后不应有活动命令：{state}"
        );
        assert_eq!(entity_count(&app), before, "取消后实体数不变");

        // 取消后重新插入仍能提交。
        let placed = place(&mut app, "vessel", [250.0, 250.0, 0.0]);
        assert_eq!(placed["ok"], true, "{placed}");
        assert_eq!(entity_count(&app), before + 1, "重新插入应 +1");
        let inserted = inserts(&app);
        assert_eq!(inserted.len(), 1, "应只有一个 Insert，实得 {inserted:?}");
        assert_eq!(inserted[0].1, "PID_SYM_equipment_vessel");
        assert!(
            (inserted[0].2[0] - 250.0).abs() <= 0.5 && (inserted[0].2[1] - 250.0).abs() <= 0.5,
            "落点应贴近 (250,250)，实得 {:?}",
            inserted[0].2
        );
    }

    /// 提交用的是预览显示的角度：空格转 90° 后落点，`Insert.rotation` 就是
    /// 那个角，预览与实体不会不一致。
    #[test]
    fn a_placement_commits_the_rotation_the_preview_showed() {
        let mut app = app();
        request(
            &mut app,
            serde_json::json!({"op":"run","cmd":"PIDINSERT vessel"}),
        );
        request(
            &mut app,
            serde_json::json!({"op":"input","kind":"point","point":[200.0,150.0,0.0],"space":"wcs"}),
        );
        request(
            &mut app,
            serde_json::json!({"op":"input","kind":"token","text":"SPACE"}),
        );
        let placed = request(&mut app, serde_json::json!({"op":"input","kind":"enter"}));
        assert_eq!(placed["ok"], true, "{placed}");

        let document = &app.tabs[app.active_tab].scene.document;
        let rotations: Vec<f64> = document
            .entities()
            .filter_map(|entity| match entity {
                codec::EntityType::Insert(insert) => Some(insert.rotation),
                _ => None,
            })
            .collect();
        assert_eq!(rotations.len(), 1, "应有 1 个 Insert，实得 {rotations:?}");
        assert!(
            (rotations[0] - std::f64::consts::FRAC_PI_2).abs() < 1e-9,
            "落点应带预览的 90°，实得 {}",
            rotations[0]
        );
    }

    /// 一条协议 1 请求，**不**驱动它返回的任务，并断言它一个任务都没排。
    ///
    /// `VAL-DRAW-030` / `VAL-DRAW-032` 的机制前提就在这个断言上：一个
    /// 自动化步骤只要排了异步任务（键盘焦点、重绘请求），请求就先回
    /// `accepted`，REST 侧必须再等一轮轮询才拿到结果。一次插入是三步，
    /// 三步都异步就是三条额外往返 —— 单次往返 30 ms 时正好把中位数顶过
    /// 100 ms。
    fn synchronous_step(
        app: &mut OpenCADStudio,
        mut req: serde_json::Value,
        serial: u64,
    ) -> serde_json::Value {
        let state = app.control_state();
        req["protocol"] = serde_json::json!(1);
        req["document_id"] = state["document_id"].clone();
        req["revision"] = state["revision"].clone();
        req["client_id"] = serde_json::json!("test");
        req["request_id"] = serde_json::json!(format!("perf-{serial}"));
        let (response, task) = app.control_request(req);
        assert_eq!(
            task.units(),
            0,
            "自动化的一步不得为自己排异步任务：{response}"
        );
        response
    }

    /// `VAL-DRAW-030` / `VAL-DRAW-032`：插入的三步（起命令、喂点、回车）在
    /// 控制面路径上都是同步结算的，因此 REST 每次只需一次往返。
    #[test]
    fn an_automated_insert_settles_each_step_without_an_async_task() {
        let mut app = app();
        let started = synchronous_step(
            &mut app,
            serde_json::json!({"op":"run","cmd":"PIDINSERT vessel"}),
            1,
        );
        assert_eq!(started["status"], "waiting_input", "{started}");
        let picked = synchronous_step(
            &mut app,
            serde_json::json!({
                "op":"input","kind":"point","point":[200.0,150.0,0.0],"space":"wcs",
            }),
            2,
        );
        assert_eq!(picked["status"], "waiting_input", "{picked}");
        let placed = synchronous_step(&mut app, serde_json::json!({"op":"input","kind":"enter"}), 3);
        assert_eq!(placed["status"], "completed", "{placed}");
        assert_eq!(inserts(&app).len(), 1, "落点应真的提交");
    }

    /// 同一条路径上的普通命令（LINE）也必须是同步的，否则这条断言只覆盖
    /// 了 PIDINSERT 一个特例。
    #[test]
    fn an_automated_line_command_settles_without_an_async_task() {
        let mut app = app();
        let started = synchronous_step(
            &mut app,
            serde_json::json!({"op":"run","cmd":"LINE"}),
            1,
        );
        assert_eq!(started["status"], "waiting_input", "{started}");
        let first = synchronous_step(
            &mut app,
            serde_json::json!({
                "op":"input","kind":"point","point":[0.0,0.0,0.0],"space":"wcs",
            }),
            2,
        );
        assert_eq!(first["status"], "waiting_input", "{first}");
    }
}
