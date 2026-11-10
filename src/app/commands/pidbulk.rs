//! `pid_insert_batch` — place many P&ID symbols in one step.
//!
//! `PIDINSERT` is one symbol per interaction cycle: run, pick a point, Enter.
//! Placing two thousand devices that way costs two thousand command cycles,
//! which is a control-plane cost, not a tag cost — the budget in
//! `VAL-DRAW-031` (2000 instances, first instance created → last tag settled
//! ≤ 3000 ms) can only be met by placing the batch in one request.
//!
//! The batch is the same placement the interactive command commits, done many
//! times inside one undo step:
//!
//! * the symbol resolves through [`super::pidpalette::resolve_symbol_for_batch`],
//!   the palette's own resolution, so the batch can never name a block the
//!   palette would refuse;
//! * every instance carries the same `PID_AUTHOR` record the single placement
//!   writes (`type` = canonical category code, `ports` from the symbol
//!   definition, `props` empty), and the same `PID_TAG` attribute label;
//! * the tag sequence comes from [`TagSequencer`], which scans the document
//!   once and then only looks up and advances. Calling `generate` per instance
//!   would rescan the document each time — O(n²) — which is what D22 fixed.
//!
//! Nothing here re-implements the tag rules or the symbol library: a batch is
//! N placements, not a second placement path.

use super::pidpalette::{resolve_symbol_for_batch, sync_pid_tag_attribute};
use super::*;
use crate::pid::persist::PidAuthor;
use crate::pid::tag_engine::{TagSequencer, TagView};
use crate::pid::type_lib::TypeLib;
use crate::scene::ChangeKind;
use glam::DVec3;
use serde_json::{json, Value};

/// Hard ceiling on one request. Well above the contract's 2000 and still
/// small enough that a caller cannot turn one request into an out-of-memory
/// event; 20 000 placements is far beyond any sheet's symbol count.
const MAX_BATCH: usize = 20_000;

/// A placement failure in the control surface's own shape.
fn refuse(code: &str, error: impl Into<String>) -> Value {
    json!({"ok":false,"status":"failed","code":code,"error":error.into()})
}

/// Where the batch places its instances.
///
/// Two spellings are accepted, because the two callers have different needs:
/// an explicit `points` list when the caller already knows every position,
/// and `count` + `origin` + `step` when it only wants a block of symbols laid
/// out on a grid (which keeps the request small for the 2000-instance case).
fn parse_points(req: &Value) -> Result<Vec<DVec3>, Value> {
    if let Some(list) = req["points"].as_array() {
        if list.is_empty() {
            return Err(refuse(
                "points_required",
                "points must hold at least one [x, y] or [x, y, z]",
            ));
        }
        if list.len() > MAX_BATCH {
            return Err(refuse(
                "batch_too_large",
                format!("one batch holds at most {MAX_BATCH} placements, got {}", list.len()),
            ));
        }
        let mut points = Vec::with_capacity(list.len());
        for (index, entry) in list.iter().enumerate() {
            let Some(values) = entry.as_array().filter(|v| v.len() == 2 || v.len() == 3) else {
                return Err(refuse(
                    "invalid_point",
                    format!("points[{index}] must be [x, y] or [x, y, z]"),
                ));
            };
            let mut xyz = [0.0f64; 3];
            for (axis, value) in values.iter().enumerate() {
                xyz[axis] = value
                    .as_f64()
                    .filter(|v| v.is_finite())
                    .ok_or_else(|| {
                        refuse(
                            "invalid_point",
                            format!("points[{index}][{axis}] must be a finite number"),
                        )
                    })?;
            }
            points.push(DVec3::from_array(xyz));
        }
        return Ok(points);
    }

    let count = req["count"]
        .as_u64()
        .ok_or_else(|| {
            refuse(
                "points_required",
                "Supply points:[[x,y],…], or count with origin/step",
            )
        })? as usize;
    if count == 0 {
        return Err(refuse("points_required", "count must be at least 1"));
    }
    if count > MAX_BATCH {
        return Err(refuse(
            "batch_too_large",
            format!("one batch holds at most {MAX_BATCH} placements, got {count}"),
        ));
    }
    let origin = point_or(req, "origin", [0.0, 0.0, 0.0])?;
    let step = point_or(req, "step", [0.0, 0.0, 0.0])?;
    let columns = req["columns"].as_u64().unwrap_or(0) as usize;
    // `columns: 0` (the default) lays the batch out on one row, which is what
    // a caller that only cares about the tags wants.
    let columns = if columns == 0 { count } else { columns };
    let mut points = Vec::with_capacity(count);
    for index in 0..count {
        let column = index % columns;
        let row = index / columns;
        points.push(DVec3::new(
            origin.x + step.x * column as f64,
            origin.y + step.y * row as f64,
            origin.z,
        ));
    }
    Ok(points)
}

/// A two- or three-element point field, defaulting when absent.
fn point_or(req: &Value, key: &str, default: [f64; 3]) -> Result<DVec3, Value> {
    let Some(values) = req.get(key) else {
        return Ok(DVec3::from_array(default));
    };
    let Some(values) = values.as_array().filter(|v| v.len() == 2 || v.len() == 3) else {
        return Err(refuse(
            "invalid_point",
            format!("{key} must be [x, y] or [x, y, z]"),
        ));
    };
    let mut xyz = [0.0f64; 3];
    for (axis, value) in values.iter().enumerate() {
        xyz[axis] = value
            .as_f64()
            .filter(|v| v.is_finite())
            .ok_or_else(|| refuse("invalid_point", format!("{key}[{axis}] must be finite")))?;
    }
    Ok(DVec3::from_array(xyz))
}

/// Add every placement, publishing one geometry change for the whole list.
///
/// The native build has the batched add (`Scene::add_entities`), which exists
/// exactly for this: the per-entity `add_entity` rebuilds derived caches 2000
/// times. The web build has no batched add, so it places them one at a time —
/// the batch is still one request there, just not one bump.
#[cfg(not(target_arch = "wasm32"))]
fn add_placements(
    scene: &mut crate::scene::Scene,
    entities: Vec<document::EntityType>,
) -> Vec<document::Handle> {
    scene.add_entities(entities)
}

#[cfg(target_arch = "wasm32")]
fn add_placements(
    scene: &mut crate::scene::Scene,
    entities: Vec<document::EntityType>,
) -> Vec<document::Handle> {
    entities
        .into_iter()
        .map(|entity| scene.add_entity(entity))
        .collect()
}

impl OpenCADStudio {
    /// `pid_insert_batch` — place `symbol` at every point, generating each
    /// instance's tag from the type's rule, as one undoable step.
    ///
    /// Result: the block name, the author type code, how many instances were
    /// created, how many carry a non-empty tag, and their handles in
    /// placement order (so a caller can read one instance back without
    /// querying the document).
    pub(in crate::app) fn control_pid_insert_batch(
        &mut self,
        req: &Value,
    ) -> Result<Task<Message>, Value> {
        let name = req["symbol"].as_str().unwrap_or("").trim().to_owned();
        if name.is_empty() {
            return Err(refuse(
                "symbol_required",
                "Supply symbol: a symbol id (vessel) or a PID_SYM_* block name",
            ));
        }
        let Some(symbol) = resolve_symbol_for_batch(&self.pid_symbols, &name) else {
            return Err(refuse(
                "unknown_symbol",
                format!(
                    "PIDINSERT: unknown symbol \"{name}\" -- the palette lists what \
                     assets/pid-symbols.json defines."
                ),
            ));
        };
        let points = parse_points(req)?;

        let i = self.active_tab;
        if self.tabs[i].is_start {
            return Err(refuse("no_document", "Create or open a drawing first"));
        }
        if self.tabs[i].active_cmd.is_some() {
            return Err(refuse(
                "command_busy",
                "An interactive command is already active",
            ));
        }

        let block = symbol.block;
        let author_type = symbol.author_type;
        let scale = symbol.scale;
        // The symbol's block definitions come into being with the first
        // placement, exactly as the interactive path defines them: a drawing
        // that has never held a symbol has none, and an Insert naming a block
        // that does not exist draws nothing and fails the save audit.
        if !self.tabs[i].scene.document.block_records.contains(&block) {
            self.pid_symbols.instantiate(&mut self.tabs[i].scene);
        }
        let ports = self.pid_symbols.symbol_ports(&block).join(",");

        // One undo step for the whole batch: the structure snapshot carries
        // the block definitions and the `PID_AUTHOR` APPID an entity delta
        // cannot replay, the entity recording carries the 2000 additions.
        self.push_undo_snapshot(i, "PIDINSERTBATCH");

        // One sequence scan for the batch. `generate` per instance would scan
        // the whole document each time, which is the O(n²) the sequencer
        // exists to remove (D22); the tags themselves come from the same rule
        // resolution either way.
        let types = TypeLib::embedded();
        let mut sequence = TagSequencer::new(&types, &self.tabs[i].scene.document);

        let entities: Vec<document::EntityType> = points
            .iter()
            .map(|at| {
                document::EntityType::Insert(
                    document::entities::Insert::new(
                        block.clone(),
                        document::types::Vector3::new(at.x, at.y, at.z),
                    )
                    .with_uniform_scale(scale),
                )
            })
            .collect();
        // `add_placements` publishes one geometry change for the whole list;
        // the per-entity `add_entity` would rebuild derived caches 2000 times.
        let handles = add_placements(&mut self.tabs[i].scene, entities);

        let props = std::collections::BTreeMap::new();
        let mut created = 0usize;
        let mut tagged = 0usize;
        let mut changed: Vec<(document::Handle, ChangeKind)> = Vec::with_capacity(handles.len());
        {
            let document = &mut self.tabs[i].scene.document;
            for handle in handles.iter().copied() {
                if handle.is_null() {
                    continue;
                }
                // A generation failure is not fatal: `TagView::empty` is what
                // the single placement stores then (D14), which leaves a
                // manual, `ok`, empty tag instead of a made-up one.
                let view: TagView = sequence
                    .next(&author_type, &props)
                    .unwrap_or_else(|_| TagView::empty());
                if !view.tag.is_empty() {
                    tagged += 1;
                }
                PidAuthor {
                    type_code: author_type.clone(),
                    tag: view.tag,
                    tag_source: view.tag_source,
                    tag_status: view.tag_status,
                    ports: ports.clone(),
                    props: "{}".to_string(),
                    ..PidAuthor::default()
                }
                .store(document, handle);
                sync_pid_tag_attribute(document, handle);
                created += 1;
                changed.push((handle, ChangeKind::Modified));
            }
        }
        if !changed.is_empty() {
            self.tabs[i].scene.bump_entities(&changed);
        }
        self.tabs[i].dirty = true;
        self.post_ref_op(i);

        self.set_control_result(json!({
            "symbol": block,
            "type": author_type,
            "created": created,
            "tagged": tagged,
            "handles": handles
                .iter()
                .map(|handle| format!("{:X}", handle.value()))
                .collect::<Vec<_>>(),
        }));
        Ok(Task::none())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::OpenCADStudio;
    use serde_json::json;

    fn app() -> OpenCADStudio {
        let mut app = OpenCADStudio::new_for_test();
        let _ = app.automation_op(r#"{"op":"new"}"#);
        app
    }

    /// One protocol-1 request against the active document, settled.
    fn request(app: &mut OpenCADStudio, mut req: Value) -> Value {
        use std::sync::atomic::{AtomicU64, Ordering};
        static SERIAL: AtomicU64 = AtomicU64::new(0);
        let state = app.control_state();
        req["protocol"] = json!(1);
        req["document_id"] = state["document_id"].clone();
        req["client_id"] = json!("test");
        if req.get("request_id").is_none() {
            req["request_id"] = json!(format!(
                "pidbatch-{}",
                SERIAL.fetch_add(1, Ordering::Relaxed)
            ));
        }
        let (response, task) = app.control_request(req.clone());
        app.drive_headless_task(task).unwrap();
        if matches!(response["status"].as_str(), Some("accepted" | "running")) {
            app.control_request(json!({
                "op":"operation",
                "request_id":req["request_id"],
            }))
            .0
        } else {
            response
        }
    }

    /// Every `PID_AUTHOR` tag in the document, in document order.
    fn tags(app: &OpenCADStudio) -> Vec<String> {
        let document = &app.tabs[app.active_tab].scene.document;
        document
            .entities()
            .filter_map(|entity| {
                PidAuthor::load(document, entity.common().handle).map(|author| author.tag)
            })
            .collect()
    }

    fn pid_count(app: &mut OpenCADStudio) -> u64 {
        request(app, json!({"op":"query","xdata_app":"PID_AUTHOR"}))["count"]
            .as_u64()
            .unwrap_or(0)
    }

    /// The batch places one Insert per point, each carrying its author record
    /// and the tag its position in the sequence earns.
    #[test]
    fn a_batch_places_every_point_with_the_next_tag() {
        let mut app = app();
        let placed = request(
            &mut app,
            json!({
                "op":"pid_insert_batch",
                "symbol":"vessel",
                "points":[[100,100,0],[200,100,0],[300,100,0]],
            }),
        );
        assert_eq!(placed["ok"], true, "{placed}");
        assert_eq!(placed["result"]["created"], 3, "{placed}");
        assert_eq!(placed["result"]["tagged"], 3, "{placed}");
        assert_eq!(placed["result"]["symbol"], "PID_SYM_equipment_vessel");
        assert_eq!(placed["result"]["type"], "EQUIPMENT");
        assert_eq!(pid_count(&mut app), 3);
        assert_eq!(
            tags(&app),
            vec!["P-1001".to_string(), "P-1002".to_string(), "P-1003".to_string()]
        );

        // Each instance sits where its point said.
        let document = &app.tabs[app.active_tab].scene.document;
        let at: Vec<[f64; 2]> = document
            .entities()
            .filter_map(|entity| match entity {
                document::EntityType::Insert(insert) => {
                    Some([insert.insert_point.x, insert.insert_point.y])
                }
                _ => None,
            })
            .collect();
        assert_eq!(at, vec![[100.0, 100.0], [200.0, 100.0], [300.0, 100.0]]);
    }

    /// The grid spelling fills the same sequence.
    #[test]
    fn a_count_and_step_lay_the_batch_out_on_a_grid() {
        let mut app = app();
        let placed = request(
            &mut app,
            json!({
                "op":"pid_insert_batch",
                "symbol":"vessel",
                "count":4,
                "origin":[10,20,0],
                "step":[50,40,0],
                "columns":2,
            }),
        );
        assert_eq!(placed["ok"], true, "{placed}");
        assert_eq!(placed["result"]["created"], 4, "{placed}");
        assert_eq!(pid_count(&mut app), 4);
        let document = &app.tabs[app.active_tab].scene.document;
        let at: Vec<[f64; 2]> = document
            .entities()
            .filter_map(|entity| match entity {
                document::EntityType::Insert(insert) => {
                    Some([insert.insert_point.x, insert.insert_point.y])
                }
                _ => None,
            })
            .collect();
        assert_eq!(
            at,
            vec![[10.0, 20.0], [60.0, 20.0], [10.0, 60.0], [60.0, 60.0]]
        );
    }

    /// An unknown symbol is refused and nothing is added.
    #[test]
    fn an_unknown_symbol_is_refused_without_adding_entities() {
        let mut app = app();
        let refused = request(
            &mut app,
            json!({"op":"pid_insert_batch","symbol":"not_a_real_symbol","count":2}),
        );
        assert_eq!(refused["ok"], false, "{refused}");
        assert_eq!(refused["code"], "unknown_symbol", "{refused}");
        assert_eq!(pid_count(&mut app), 0);
        assert_eq!(
            app.tabs[app.active_tab].scene.document.entities().count(),
            0
        );
    }

    /// A batch that names no placements is refused before anything is built.
    #[test]
    fn an_empty_batch_is_refused() {
        let mut app = app();
        let refused = request(&mut app, json!({"op":"pid_insert_batch","symbol":"vessel"}));
        assert_eq!(refused["ok"], false, "{refused}");
        assert_eq!(refused["code"], "points_required", "{refused}");
        assert_eq!(pid_count(&mut app), 0);
    }

    /// The whole batch is one undo step: one undo removes every instance.
    #[test]
    fn one_undo_removes_the_whole_batch() {
        let mut app = app();
        request(
            &mut app,
            json!({
                "op":"pid_insert_batch",
                "symbol":"vessel",
                "points":[[10,10,0],[20,10,0],[30,10,0]],
            }),
        );
        assert_eq!(pid_count(&mut app), 3);
        let undone = request(&mut app, json!({"op":"undo"}));
        assert_eq!(undone["ok"], true, "{undone}");
        assert_eq!(pid_count(&mut app), 0, "one undo must clear the batch");
        let redone = request(&mut app, json!({"op":"redo"}));
        assert_eq!(redone["ok"], true, "{redone}");
        assert_eq!(pid_count(&mut app), 3, "redo must bring the batch back");
    }

    /// `VAL-DRAW-031` at its contract size: 2000 instances, tags
    /// `P-1001`..`P-3000`, no duplicates.
    ///
    /// The wall-clock figure is printed rather than asserted — the contract's
    /// 3000 ms budget is measured through the GUI control plane, where machine
    /// load is controlled; asserting it in a debug-build unit test would make
    /// a loaded CI box fail a correct implementation.
    #[test]
    fn a_batch_of_two_thousand_tags_runs_to_p_3000() {
        let mut app = app();
        let started = std::time::Instant::now();
        let placed = request(
            &mut app,
            json!({
                "op":"pid_insert_batch",
                "symbol":"vessel",
                "count":2000,
                "origin":[0,0,0],
                "step":[25,25,0],
                "columns":50,
            }),
        );
        let elapsed = started.elapsed();
        assert_eq!(placed["ok"], true, "{placed}");
        assert_eq!(placed["result"]["created"], 2000, "{placed}");
        assert_eq!(pid_count(&mut app), 2000);

        let tags = tags(&app);
        assert_eq!(tags.len(), 2000);
        let unique: std::collections::HashSet<&String> = tags.iter().collect();
        assert_eq!(unique.len(), 2000, "2000 个位号必须两两不重复");
        assert_eq!(tags.first().map(String::as_str), Some("P-1001"));
        assert_eq!(tags.last().map(String::as_str), Some("P-3000"));
        println!(
            "pid_insert_batch 2000 实例：{} ms（含一次撤销步与 2000 次位号落盘）",
            elapsed.as_millis()
        );
    }
}
