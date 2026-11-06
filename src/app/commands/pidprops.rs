//! The Properties panel's `P&ID 属性` group: the typed author record
//! (`PID_AUTHOR`) a placed symbol carries, shown and edited in place.
//!
//! Row ids are fixed by the architecture: `pid_type`, `pid_tag`,
//! `pid_tag_source`, `pid_tag_status`, then one `pid_props_<key>` row per
//! `props` key. Only the tag and the props rows are editable; a tag edit makes
//! the tag `manual`. Every commit is one undo step that also re-runs the
//! uniqueness check and redraws the canvas tag labels
//! ([`OpenCADStudio::refresh_pid_tags`]), so a conflict mark (`？`) and the
//! label follow the edit and roll back with it.

use super::*;
use crate::pid::persist::PidAuthor;
use crate::pid::propagate::{join_props, set_prop_manual, split_props};
use crate::pid::tag_engine::{TagView, TAG_SOURCE_MANUAL};
use crate::scene::model::object::{PropSection, PropValue, Property};
use crate::t;

/// Title of the author group. Kept apart from the recognizer's `P&ID` group
/// (`PID_SEMANTICS` / marked GROUPs), which merges rows by that exact title.
pub(in crate::app) const PID_AUTHOR_SECTION: &str = "P&ID 属性";

const TAG_FIELD: &str = "pid_tag";
const PROPS_FIELD_PREFIX: &str = "pid_props_";

/// `Property::field` is `&'static str`, but props keys are data. Each distinct
/// row id is leaked once and reused, so repeated refreshes do not grow memory.
fn props_field(key: &str) -> &'static str {
    use std::collections::HashSet;
    use std::sync::{Mutex, OnceLock};
    static FIELDS: OnceLock<Mutex<HashSet<&'static str>>> = OnceLock::new();
    let id = format!("{PROPS_FIELD_PREFIX}{key}");
    let mut fields = FIELDS
        .get_or_init(|| Mutex::new(HashSet::new()))
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    if let Some(existing) = fields.get(id.as_str()) {
        return existing;
    }
    let leaked: &'static str = Box::leak(id.into_boxed_str());
    fields.insert(leaked);
    leaked
}

/// The `P&ID 属性` group for `handle`, or `None` when it carries no
/// `PID_AUTHOR` record.
pub(in crate::app) fn pid_author_section(
    document: &codec::Document,
    handle: codec::Handle,
) -> Option<PropSection> {
    let author = PidAuthor::load(document, handle)?;
    let display = TagView::new(
        author.tag.clone(),
        author.tag_source.clone(),
        author.tag_status.clone(),
    )
    .display;
    let mut props = vec![
        crate::entities::common::ro_prop(t!("Type").as_ref(), "pid_type", author.type_code),
        Property {
            label: t!("Item tag").into_owned(),
            field: TAG_FIELD,
            value: PropValue::PlainText(display),
        },
        crate::entities::common::ro_prop(
            t!("Tag source").as_ref(),
            "pid_tag_source",
            author.tag_source,
        ),
        crate::entities::common::ro_prop(t!("Status").as_ref(), "pid_tag_status", author.tag_status),
    ];
    let (values, _) = split_props(&author.props);
    for (key, value) in values {
        props.push(Property {
            field: props_field(&key),
            label: key,
            value: PropValue::PlainText(value),
        });
    }
    Some(PropSection {
        title: PID_AUTHOR_SECTION.to_string(),
        props,
    })
}

impl OpenCADStudio {
    /// Commit a `P&ID 属性` row edit on every target that carries an author
    /// record. Returns `false` when the field is not an author row or no
    /// target carries a record, so the caller can fall through to its other
    /// handlers (the recognizer group shares the `pid_tag` id).
    pub(in crate::app) fn commit_pid_author_property(
        &mut self,
        i: usize,
        handles: &[codec::Handle],
        field: &str,
        value: &str,
    ) -> bool {
        let key = match field {
            TAG_FIELD => None,
            _ => match field.strip_prefix(PROPS_FIELD_PREFIX) {
                Some(key) if !key.is_empty() => Some(key.to_string()),
                _ => return false,
            },
        };
        let authored: Vec<(codec::Handle, PidAuthor)> = handles
            .iter()
            .filter_map(|handle| {
                PidAuthor::load(&self.tabs[i].scene.document, *handle).map(|a| (*handle, a))
            })
            .collect();
        if authored.is_empty() {
            return false;
        }
        let changed: Vec<(codec::Handle, PidAuthor)> = authored
            .into_iter()
            .filter(|(_, author)| match &key {
                None => {
                    // Committing the row unchanged — including the `？` it
                    // shows for a conflict — is not a manual retag.
                    let shown = TagView::new(
                        author.tag.clone(),
                        author.tag_source.clone(),
                        author.tag_status.clone(),
                    )
                    .display;
                    value != author.tag && value != shown
                }
                Some(key) => split_props(&author.props).0.get(key).map(String::as_str) != Some(value),
            })
            .collect();
        if changed.is_empty() {
            return true;
        }
        self.push_undo_snapshot(i, "PIDPROP");
        let document = &mut self.tabs[i].scene.document;
        for (handle, mut author) in changed.iter().cloned() {
            match &key {
                None => {
                    author.tag = value.to_string();
                    author.tag_source = TAG_SOURCE_MANUAL.to_string();
                    author.store(document, handle);
                }
                // A pipe run's hand-edited line field must stop taking the
                // line's value, so it gets the override mark; nothing else
                // inherits props, and marking them would only bloat the record.
                Some(key) if author.type_code == "PIPE_RUN" => {
                    let _ = set_prop_manual(document, handle, key, value);
                }
                Some(key) => {
                    let (mut values, marks) = split_props(&author.props);
                    values.insert(key.clone(), value.to_string());
                    author.props = join_props(&values, &marks);
                    author.store(document, handle);
                }
            }
        }
        let edited: Vec<codec::Handle> = changed.iter().map(|(handle, _)| *handle).collect();
        self.refresh_pid_tags(i, &edited);
        self.tabs[i].dirty = true;
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{json, Value};

    fn request(app: &mut OpenCADStudio, mut req: Value) -> Value {
        use std::sync::atomic::{AtomicU64, Ordering};
        static SERIAL: AtomicU64 = AtomicU64::new(0);
        let state = app.control_state();
        req["protocol"] = json!(1);
        req["document_id"] = state["document_id"].clone();
        req["revision"] = state["revision"].clone();
        req["client_id"] = json!("test");
        if req.get("request_id").is_none() {
            req["request_id"] = json!(format!("pidprops-{}", SERIAL.fetch_add(1, Ordering::Relaxed)));
        }
        let (response, task) = app.control_request(req.clone());
        app.drive_headless_task(task).unwrap();
        if matches!(response["status"].as_str(), Some("accepted" | "running")) {
            app.control_request(json!({"op":"operation","request_id":req["request_id"]})).0
        } else {
            response
        }
    }

    /// A new drawing with one placed vessel carrying two props; its handle.
    fn placed_vessel(app: &mut OpenCADStudio, x: f64) -> codec::Handle {
        let started = request(app, json!({"op":"run","cmd":"PIDINSERT vessel"}));
        assert_eq!(started["ok"], true, "{started}");
        let picked = request(
            app,
            json!({"op":"input","kind":"point","point":[x,150.0,0.0],"space":"wcs"}),
        );
        assert_eq!(picked["ok"], true, "{picked}");
        request(app, json!({"op":"input","kind":"enter"}));
        let document = &mut app.tabs[app.active_tab].scene.document;
        let handle = document
            .entities()
            .filter_map(|entity| match entity {
                codec::EntityType::Insert(insert) => Some(insert.common.handle),
                _ => None,
            })
            .last()
            .expect("a placement");
        let mut author = PidAuthor::load(document, handle).expect("author record");
        author.props = r#"{"设计压力":"1.6MPa","设计温度":"120℃"}"#.to_string();
        author.store(document, handle);
        handle
    }

    fn app() -> OpenCADStudio {
        let mut app = OpenCADStudio::new_for_test();
        let _ = app.automation_op(r#"{"op":"new"}"#);
        app
    }

    fn hex(handle: codec::Handle) -> String {
        format!("{:X}", handle.value())
    }

    fn select(app: &mut OpenCADStudio, handle: codec::Handle) {
        let _ = app.automation_op(r#"{"op":"select","clear":true}"#);
        let selected = request(app, json!({"op":"select","handles":[hex(handle)]}));
        assert_eq!(selected["ok"], true, "{selected}");
    }

    fn author_rows(app: &mut OpenCADStudio) -> Option<Vec<Value>> {
        let read = request(app, json!({"op":"properties"}));
        let sections = read["sections"]
            .as_array()
            .or_else(|| read["result"]["sections"].as_array())
            .cloned()
            .unwrap_or_default();
        sections
            .into_iter()
            .find(|section| section["title"] == PID_AUTHOR_SECTION)
            .map(|section| section["properties"].as_array().cloned().unwrap_or_default())
    }

    fn row_value(rows: &[Value], id: &str) -> Option<String> {
        rows.iter()
            .find(|row| row["id"] == id)
            .and_then(|row| row["value"].as_str().map(str::to_owned))
    }

    fn author(app: &OpenCADStudio, handle: codec::Handle) -> PidAuthor {
        PidAuthor::load(&app.tabs[app.active_tab].scene.document, handle).expect("author record")
    }

    fn set(app: &mut OpenCADStudio, field: &str, value: &str) {
        let done = request(app, json!({"op":"property","field":field,"value":value}));
        assert_eq!(done["ok"], true, "property {field}={value}: {done}");
    }

    /// `VAL-DRAW-024` / `025`: the group shows for an authored symbol only,
    /// with the four fixed rows and one row per props key, each equal to
    /// the stored value.
    #[test]
    fn the_group_shows_the_author_record_and_only_for_authored_entities() {
        let mut app = app();
        let vessel = placed_vessel(&mut app, 200.0);
        select(&mut app, vessel);
        let rows = author_rows(&mut app).expect("P&ID 属性 group for an authored symbol");
        let stored = author(&app, vessel);
        assert_eq!(row_value(&rows, "pid_type").as_deref(), Some(stored.type_code.as_str()));
        assert_eq!(row_value(&rows, "pid_tag").as_deref(), Some(stored.tag.as_str()));
        assert_eq!(row_value(&rows, "pid_tag_source").as_deref(), Some(stored.tag_source.as_str()));
        assert_eq!(row_value(&rows, "pid_tag_status").as_deref(), Some(stored.tag_status.as_str()));
        assert_eq!(row_value(&rows, "pid_props_设计压力").as_deref(), Some("1.6MPa"));
        assert_eq!(row_value(&rows, "pid_props_设计温度").as_deref(), Some("120℃"));
        assert!(rows.len() >= 4 + 2, "rows: {rows:?}");
        assert!(rows.iter().any(|row| row["label"] == "设计压力"));

        let line = app
            .commit_entity_handle(codec::EntityType::Line(codec::entities::Line::from_points(
                codec::types::Vector3::ZERO,
                codec::types::Vector3::new(10.0, 0.0, 0.0),
            )))
            .expect("line commits");
        select(&mut app, line);
        assert!(author_rows(&mut app).is_none(), "a plain LINE has no P&ID 属性 group");
    }

    /// `VAL-DRAW-026`: a props edit lands in XDATA, other keys untouched.
    #[test]
    fn a_props_edit_lands_in_xdata_and_leaves_the_other_keys() {
        let mut app = app();
        let vessel = placed_vessel(&mut app, 200.0);
        select(&mut app, vessel);
        set(&mut app, "pid_props_设计压力", "2.5MPa");
        let (values, marks) = split_props(&author(&app, vessel).props);
        assert_eq!(values.get("设计压力").map(String::as_str), Some("2.5MPa"));
        assert_eq!(values.get("设计温度").map(String::as_str), Some("120℃"));
        assert_eq!(values.len(), 2);
        assert!(marks.is_empty(), "equipment props carry no override mark");
        let rows = author_rows(&mut app).unwrap();
        assert_eq!(row_value(&rows, "pid_props_设计压力").as_deref(), Some("2.5MPa"));
    }

    /// `VAL-DRAW-027` / `028`: a tag edit makes the tag manual, relabels the
    /// canvas, and undo / redo move tag, source and label together.
    #[test]
    fn a_tag_edit_is_manual_and_undoes_with_its_label() {
        let mut app = app();
        let vessel = placed_vessel(&mut app, 200.0);
        let before = author(&app, vessel);
        assert_eq!((before.tag.as_str(), before.tag_source.as_str()), ("P-1001", "auto"));
        select(&mut app, vessel);
        set(&mut app, "pid_tag", "P-9001");
        let after = author(&app, vessel);
        assert_eq!(after.tag, "P-9001");
        assert_eq!(after.tag_source, "manual");
        assert_eq!(after.tag_status, "ok");
        assert_eq!(label(&app, vessel).as_deref(), Some("P-9001"));

        let undone = request(&mut app, json!({"op":"undo"}));
        assert_eq!(undone["ok"], true, "{undone}");
        let back = author(&app, vessel);
        assert_eq!((back.tag.as_str(), back.tag_source.as_str()), ("P-1001", "auto"));
        assert_eq!(label(&app, vessel).as_deref(), Some("P-1001"));
        select(&mut app, vessel);
        let rows = author_rows(&mut app).unwrap();
        assert_eq!(row_value(&rows, "pid_tag").as_deref(), Some("P-1001"));

        let redone = request(&mut app, json!({"op":"redo"}));
        assert_eq!(redone["ok"], true, "{redone}");
        let again = author(&app, vessel);
        assert_eq!((again.tag.as_str(), again.tag_source.as_str()), ("P-9001", "manual"));
    }

    /// A tag edited into a clash shows `？` in the group and on the canvas.
    #[test]
    fn a_conflicting_tag_edit_shows_the_conflict_mark_in_the_group() {
        let mut app = app();
        let first = placed_vessel(&mut app, 200.0);
        let second = placed_vessel(&mut app, 400.0);
        select(&mut app, second);
        let taken = author(&app, first).tag;
        set(&mut app, "pid_tag", &taken);
        let edited = author(&app, second);
        assert_eq!(edited.tag, "P-1001");
        assert_eq!(edited.tag_status, "duplicated");
        assert_eq!(author(&app, first).tag_status, "ok");
        let rows = author_rows(&mut app).unwrap();
        assert_eq!(row_value(&rows, "pid_tag").as_deref(), Some("？"));
        assert_eq!(row_value(&rows, "pid_tag_status").as_deref(), Some("duplicated"));
        assert_eq!(label(&app, second).as_deref(), Some("？"));
    }

    fn label(app: &OpenCADStudio, handle: codec::Handle) -> Option<String> {
        match app.tabs[app.active_tab].scene.document.get_entity(handle) {
            Some(codec::EntityType::Insert(insert)) => insert
                .attributes
                .iter()
                .find(|attribute| attribute.tag == super::super::pidpalette::PID_TAG_ATTRIBUTE)
                .map(|attribute| attribute.value.clone()),
            _ => None,
        }
    }
}
