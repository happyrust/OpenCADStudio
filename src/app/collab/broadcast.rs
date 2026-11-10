//! Turning local edits into `op` payloads, and remote `op_applied` payloads
//! back into local document edits.
//!
//! The two directions are deliberately pure functions over `Document` /
//! `PidAuthor` so both can be tested without a socket and without a GUI:
//!
//! * [`collect_changes`] reads the `(handle, before, after)` triples an undo
//!   step already carries and states what changed in P&ID semantics — a tag,
//!   a property — instead of shipping geometry. The server only ever sees
//!   semantics (`architecture.md` §3).
//! * [`apply_changes`] writes those semantics onto the local document, using
//!   the same `PidAuthor` store the properties panel writes through, so a
//!   remote tag edit relabels the drawing exactly like a local one.
//!
//! An object is named by its **tag** (`object_ref`), as `architecture.md` §3.2
//! writes every payload: the tag is what the server audits and adjudicates
//! by, and what a client with no drawing of its own can name. The entity
//! handle rides along as a hint and nothing more — it tells two symbols that
//! carry one tag apart, and it is believed only when the entity it names
//! carries that tag here too. Handles are not shared identity: two clients
//! that each inserted a symbol since they opened the file gave the same handle
//! to different things.

use document::{Document, EntityType, Handle};
use serde_json::{json, Value};

use crate::pid::persist::PidAuthor;
use crate::pid::propagate::{join_props, split_props};
use crate::pid::tag_engine::TAG_SOURCE_MANUAL;
use crate::pid::topology::parse_handle;

/// Op kinds this client emits and understands.
pub const KIND_SET_TAG: &str = "set_tag";
pub const KIND_SET_PROPERTY: &str = "set_property";

/// One change an op carries, as this module writes and reads it.
///
/// `field` is `tag` for [`KIND_SET_TAG`] and the property key for
/// [`KIND_SET_PROPERTY`], matching the server's canonical change shape
/// (`op_changes` backfills `kind`, `object_ref`, `field` and `value`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Change {
    pub kind: String,
    /// The tag the object carried when the change was made. For a retag that
    /// is the old tag: the name the peers still know it by.
    pub object_ref: String,
    /// The entity the sender changed, for a peer holding the same file.
    pub handle: Option<Handle>,
    pub field: String,
    pub value: String,
    /// The value being replaced. The server's audit line reads it
    /// (`old_value`), which is how a change says what it overwrote.
    pub old: Option<String>,
}

impl Change {
    /// The JSON body one change becomes inside an `op` payload.
    pub fn to_json(&self) -> Value {
        let mut change = json!({
            "kind": self.kind,
            "object_ref": self.object_ref,
            "field": self.field,
            "value": self.value,
        });
        if let Some(old) = &self.old {
            change["old_value"] = json!(old);
        }
        if let Some(handle) = self.handle {
            change["handle"] = json!(format!("{:X}", handle.value()));
        }
        change
    }

    /// Read one change back out of an `op_applied` `changes` entry. `None` for
    /// a change this client cannot act on (an unknown kind, no object named, a
    /// missing value) — applying half of it would be worse.
    pub fn from_json(value: &Value) -> Option<Self> {
        let kind = value["kind"].as_str()?.to_owned();
        if kind != KIND_SET_TAG && kind != KIND_SET_PROPERTY {
            return None;
        }
        let object_ref = value["object_ref"].as_str()?.to_owned();
        let handle = value["handle"].as_str().and_then(parse_handle);
        let field = match value["field"].as_str() {
            Some(field) => field.to_owned(),
            // `architecture.md` writes a retag without the field.
            None if kind == KIND_SET_TAG => "tag".to_owned(),
            None => return None,
        };
        let value_text = match &value["value"] {
            Value::String(text) => text.clone(),
            Value::Null => return None,
            other => other.to_string(),
        };
        Some(Self {
            kind,
            object_ref,
            handle,
            field,
            value: value_text,
            old: value["old_value"].as_str().map(str::to_owned),
        })
    }
}

/// What one handle's before/after images amount to in P&ID semantics: a
/// retag, if the tag moved, then every property whose value moved.
///
/// The retag comes first and names the object by the tag it had; the property
/// changes after it name it by the tag it has now, so a peer applying them in
/// order finds it each time.
///
/// A created or erased entity carries no semantic change a peer can replay
/// from this payload alone (the geometry is not in it), so it yields nothing
/// rather than a half-truth. An image whose `props` overflowed into an XRecord
/// is "not known here", not "no properties": the properties of such a step are
/// left out rather than broadcast as removals.
pub fn changes_of(
    handle: Handle,
    before: Option<&EntityType>,
    after: Option<&EntityType>,
) -> Vec<Change> {
    let (Some(before), Some(after)) = (before, after) else {
        return Vec::new();
    };
    let (Some(before_author), Some(after_author)) = (
        PidAuthor::from_entity(before),
        PidAuthor::from_entity(after),
    ) else {
        return Vec::new();
    };

    let mut changes = Vec::new();
    if before_author.tag != after_author.tag {
        changes.push(Change {
            kind: KIND_SET_TAG.to_owned(),
            object_ref: before_author.tag.clone(),
            handle: Some(handle),
            field: "tag".to_owned(),
            value: after_author.tag.clone(),
            old: Some(before_author.tag.clone()),
        });
    }

    if !PidAuthor::props_are_inline(before) || !PidAuthor::props_are_inline(after) {
        return changes;
    }
    let (before_props, _) = split_props(&before_author.props);
    let (after_props, _) = split_props(&after_author.props);
    let property = |field: &str, value: &str, old: Option<&String>| Change {
        kind: KIND_SET_PROPERTY.to_owned(),
        object_ref: after_author.tag.clone(),
        handle: Some(handle),
        field: field.to_owned(),
        value: value.to_owned(),
        old: old.cloned(),
    };
    for (key, value) in &after_props {
        if before_props.get(key) != Some(value) {
            changes.push(property(key, value, before_props.get(key)));
        }
    }
    // A property that is gone travels as an empty value.
    for (key, old) in &before_props {
        if !after_props.contains_key(key) {
            changes.push(property(key, "", Some(old)));
        }
    }
    changes
}

/// The `(handle, before, after)` triples one undo delta carries, as the
/// history module stores them.
pub type EntityImages = [(
    Handle,
    Option<std::sync::Arc<EntityType>>,
    Option<std::sync::Arc<EntityType>>,
)];

/// Every semantic change one undo step contains, in handle order.
pub fn collect_changes(entities: &EntityImages) -> Vec<Change> {
    entities
        .iter()
        .flat_map(|(handle, before, after)| {
            changes_of(*handle, before.as_deref(), after.as_deref())
        })
        .collect()
}

/// What undoing that step amounts to: the same images read the other way.
pub fn collect_undone_changes(entities: &EntityImages) -> Vec<Change> {
    entities
        .iter()
        .flat_map(|(handle, before, after)| {
            changes_of(*handle, after.as_deref(), before.as_deref())
        })
        .collect()
}

/// The `op` payload for a batch of changes: the first change is the op's own
/// body, so a single-change op reads exactly like the server's canonical shape
/// (`{kind, object_ref, field, value}`). Additional changes ride along in
/// `changes` for the peer to apply in order.
///
/// One step is one op on purpose. The server accepts or refuses an op as a
/// whole (`STALE_BASE_REVISION`), so a step split over several ops could land
/// in part — a retag refused and the property change behind it accepted.
pub fn payload_for(changes: &[Change]) -> Option<Value> {
    let (first, rest) = changes.split_first()?;
    let mut payload = first.to_json();
    if !rest.is_empty() {
        payload["changes"] = Value::Array(rest.iter().map(Change::to_json).collect());
    }
    Some(payload)
}

/// Every change an `op_applied` `changes` array carries, including the ones a
/// batched payload nested under `changes`.
pub fn changes_from_payload(changes: &[Value]) -> Vec<Change> {
    let mut out = Vec::new();
    for entry in changes {
        if let Some(change) = Change::from_json(entry) {
            out.push(change);
        }
        if let Some(nested) = entry["changes"].as_array() {
            out.extend(nested.iter().filter_map(Change::from_json));
        }
    }
    out
}

/// What a change list did to the document.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Applied {
    /// Handles whose stored record was rewritten.
    pub handles: Vec<Handle>,
    /// Handles whose tag changed, so their labels and uniqueness status need
    /// re-deriving.
    pub retagged: Vec<Handle>,
    /// Changes whose object this drawing does not have, by the name they used
    /// for it. Nothing was written for them, and the two drawings now differ.
    pub unresolved: Vec<String>,
}

impl Applied {
    /// Nothing was written.
    pub fn is_empty(&self) -> bool {
        self.handles.is_empty()
    }
}

/// The entity a change is about, or `None` when this drawing has no single
/// such object.
///
/// The tag decides. The handle the sender named is taken only when the entity
/// under it carries the tag the change names — it then settles which of
/// several same-tagged symbols is meant. Without that, exactly one symbol must
/// carry the tag; none, or several, is no answer.
fn resolve(doc: &Document, change: &Change) -> Option<Handle> {
    if let Some(handle) = change.handle {
        if PidAuthor::load(doc, handle).is_some_and(|author| author.tag == change.object_ref) {
            return Some(handle);
        }
    }
    if change.object_ref.is_empty() {
        return None;
    }
    let mut carrying = doc.entities().filter_map(|entity| {
        let handle = entity.common().handle;
        let author = PidAuthor::load(doc, handle)?;
        (author.tag == change.object_ref).then_some(handle)
    });
    let found = carrying.next()?;
    carrying.next().is_none().then_some(found)
}

/// Whether a retag has been applied here already: nothing carries the old tag
/// any more and something carries the new one. Re-reading the room after a
/// reconnect must not report that as a missing object.
fn already_retagged(doc: &Document, change: &Change) -> bool {
    change.kind == KIND_SET_TAG
        && doc.entities().any(|entity| {
            PidAuthor::load(doc, entity.common().handle)
                .is_some_and(|author| author.tag == change.value)
        })
}

/// Write one change onto `doc`.
///
/// A change whose value is already in place writes nothing and is not an
/// error: re-applying an already-applied change must not fail. A change
/// naming an object this drawing does not have is recorded in
/// `applied.unresolved`.
pub fn apply_change(doc: &mut Document, change: &Change, applied: &mut Applied) {
    let Some(handle) = resolve(doc, change) else {
        if !already_retagged(doc, change) {
            applied.unresolved.push(change.object_ref.clone());
        }
        return;
    };
    let Some(mut author) = PidAuthor::load(doc, handle) else {
        return;
    };
    match change.kind.as_str() {
        KIND_SET_TAG => {
            if author.tag == change.value {
                return;
            }
            author.tag = change.value.clone();
            author.tag_source = TAG_SOURCE_MANUAL.to_owned();
            author.store(doc, handle);
            applied.retagged.push(handle);
        }
        KIND_SET_PROPERTY => {
            let (mut values, marks) = split_props(&author.props);
            let current = values.get(&change.field).cloned().unwrap_or_default();
            if current == change.value {
                return;
            }
            if change.value.is_empty() {
                values.remove(&change.field);
            } else {
                values.insert(change.field.clone(), change.value.clone());
            }
            author.props = join_props(&values, &marks);
            author.store(doc, handle);
        }
        _ => return,
    }
    if !applied.handles.contains(&handle) {
        applied.handles.push(handle);
    }
}

/// Write a whole change list onto `doc`, in order.
pub fn apply_changes(doc: &mut Document, changes: &[Change]) -> Applied {
    let mut applied = Applied::default();
    for change in changes {
        apply_change(doc, change, &mut applied);
    }
    applied
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pid::persist::PidAuthor;
    use document::entities::Insert;
    use document::types::Vector3;

    fn add_author(doc: &mut Document, tag: &str, props: &str) -> Handle {
        let handle = doc
            .add_entity(EntityType::Insert(Insert::new(
                "PID_SYM_equipment_vessel",
                Vector3::ZERO,
            )))
            .expect("entity");
        PidAuthor {
            type_code: "EQUIPMENT".to_owned(),
            tag: tag.to_owned(),
            tag_source: "auto".to_owned(),
            tag_status: "ok".to_owned(),
            props: props.to_owned(),
            ..PidAuthor::default()
        }
        .store(doc, handle);
        handle
    }

    fn doc_with_author(tag: &str, props: &str) -> (Document, Handle) {
        let mut doc = Document::new();
        let handle = add_author(&mut doc, tag, props);
        (doc, handle)
    }

    /// A change as a peer with the same file would send it.
    fn change(
        kind: &str,
        object_ref: &str,
        handle: Option<Handle>,
        field: &str,
        value: &str,
    ) -> Change {
        Change {
            kind: kind.to_owned(),
            object_ref: object_ref.to_owned(),
            handle,
            field: field.to_owned(),
            value: value.to_owned(),
            old: None,
        }
    }

    fn tag_of(doc: &Document, handle: Handle) -> String {
        PidAuthor::load(doc, handle).expect("author").tag
    }

    #[test]
    fn a_tag_edit_becomes_one_set_tag_change_naming_the_old_tag() {
        let (mut doc, handle) = doc_with_author("P-1001", "");
        let before = doc.get_entity_arc(handle).expect("before");
        let mut author = PidAuthor::load(&doc, handle).expect("author");
        author.tag = "P-1009".to_owned();
        author.store(&mut doc, handle);
        let after = doc.get_entity_arc(handle).expect("after");

        let changes = changes_of(handle, Some(&before), Some(&after));
        assert_eq!(changes.len(), 1, "{changes:?}");
        let change = &changes[0];
        assert_eq!(change.kind, KIND_SET_TAG);
        assert_eq!(change.object_ref, "P-1001");
        assert_eq!(change.handle, Some(handle));
        assert_eq!(change.field, "tag");
        assert_eq!(change.value, "P-1009");
        assert_eq!(change.old.as_deref(), Some("P-1001"));
    }

    #[test]
    fn a_property_edit_becomes_one_set_property_change_naming_the_tag() {
        let (mut doc, handle) = doc_with_author("P-1001", r#"{"设计压力":"1.6MPa"}"#);
        let before = doc.get_entity_arc(handle).expect("before");
        let mut author = PidAuthor::load(&doc, handle).expect("author");
        author.props = r#"{"设计压力":"2.5MPa"}"#.to_owned();
        author.store(&mut doc, handle);
        let after = doc.get_entity_arc(handle).expect("after");

        let changes = changes_of(handle, Some(&before), Some(&after));
        assert_eq!(changes.len(), 1, "{changes:?}");
        let change = &changes[0];
        assert_eq!(change.kind, KIND_SET_PROPERTY);
        assert_eq!(change.object_ref, "P-1001");
        assert_eq!(change.field, "设计压力");
        assert_eq!(change.value, "2.5MPa");
        assert_eq!(change.old.as_deref(), Some("1.6MPa"));
    }

    /// One step can move the tag and several properties of one symbol. Every
    /// one of them is stated: the retag first, by the old tag; the properties
    /// after it, by the new one; a property that is gone as an empty value.
    #[test]
    fn a_step_that_moves_the_tag_and_properties_states_all_of_them_in_order() {
        let (mut doc, handle) = doc_with_author(
            "P-1001",
            r#"{"设计压力":"1.6MPa","介质":"水","设计温度":"80℃"}"#,
        );
        let before = doc.get_entity_arc(handle).expect("before");
        let mut author = PidAuthor::load(&doc, handle).expect("author");
        author.tag = "P-1009".to_owned();
        author.props = r#"{"设计压力":"2.5MPa","设计温度":"80℃","公称直径":"DN50"}"#.to_owned();
        author.store(&mut doc, handle);
        let after = doc.get_entity_arc(handle).expect("after");

        let changes = changes_of(handle, Some(&before), Some(&after));
        let stated: Vec<(&str, &str, &str, &str)> = changes
            .iter()
            .map(|change| {
                (
                    change.kind.as_str(),
                    change.object_ref.as_str(),
                    change.field.as_str(),
                    change.value.as_str(),
                )
            })
            .collect();
        assert_eq!(
            stated,
            [
                (KIND_SET_TAG, "P-1001", "tag", "P-1009"),
                (KIND_SET_PROPERTY, "P-1009", "公称直径", "DN50"),
                (KIND_SET_PROPERTY, "P-1009", "设计压力", "2.5MPa"),
                (KIND_SET_PROPERTY, "P-1009", "介质", ""),
            ]
        );
        assert_eq!(changes[3].old.as_deref(), Some("水"));
    }

    #[test]
    fn an_unchanged_entity_yields_no_change() {
        let (doc, handle) = doc_with_author("P-1001", r#"{"设计压力":"1.6MPa"}"#);
        let image = doc.get_entity_arc(handle).expect("image");
        assert!(changes_of(handle, Some(&image), Some(&image)).is_empty());
    }

    /// A created or erased entity is skipped: its geometry is not in the
    /// payload, so replaying it would produce a half-truth.
    #[test]
    fn a_created_or_erased_entity_is_skipped() {
        let (doc, handle) = doc_with_author("P-1001", "");
        let image = doc.get_entity_arc(handle).expect("image");
        assert!(changes_of(handle, None, Some(&image)).is_empty());
        assert!(changes_of(handle, Some(&image), None).is_empty());
    }

    /// A record whose `props` overflowed into an XRecord reads back from a
    /// snapshot with an empty `props`. That is "not known here", not "every
    /// property removed", and no removal may be broadcast for it.
    #[test]
    fn an_overflowed_props_record_is_not_read_as_every_property_removed() {
        let (mut doc, handle) = doc_with_author("P-1001", r#"{"设计压力":"1.6MPa"}"#);
        let before = doc.get_entity_arc(handle).expect("before");
        let mut author = PidAuthor::load(&doc, handle).expect("author");
        // Past the soft limit, so `store` moves `props` out of the record.
        let long = "x".repeat(crate::pid::persist::XDATA_SOFT_LIMIT);
        author.props = format!(r#"{{"设计压力":"1.6MPa","备注":"{long}"}}"#);
        author.store(&mut doc, handle);
        let after = doc.get_entity_arc(handle).expect("after");
        assert!(
            !PidAuthor::props_are_inline(&after),
            "the fixture must overflow"
        );

        let changes = collect_changes(&[(handle, Some(before), Some(after))]);
        assert!(changes.is_empty(), "{changes:?}");
    }

    /// The tag of such a step is still in the record itself, so a retag made
    /// in it is still stated.
    #[test]
    fn a_retag_is_still_stated_when_the_props_overflowed() {
        let long = "x".repeat(crate::pid::persist::XDATA_SOFT_LIMIT);
        let (mut doc, handle) = doc_with_author("P-1001", &format!(r#"{{"备注":"{long}"}}"#));
        let before = doc.get_entity_arc(handle).expect("before");
        let mut author = PidAuthor::load(&doc, handle).expect("author");
        author.tag = "P-1009".to_owned();
        author.store(&mut doc, handle);
        let after = doc.get_entity_arc(handle).expect("after");

        let changes = collect_changes(&[(handle, Some(before), Some(after))]);
        assert_eq!(changes.len(), 1, "{changes:?}");
        assert_eq!(changes[0].kind, KIND_SET_TAG);
        assert_eq!(changes[0].value, "P-1009");
    }

    /// Undoing a step is the same images read the other way.
    #[test]
    fn an_undone_step_states_the_change_backwards() {
        let (mut doc, handle) = doc_with_author("P-1001", "");
        let before = doc.get_entity_arc(handle).expect("before");
        let mut author = PidAuthor::load(&doc, handle).expect("author");
        author.tag = "P-1009".to_owned();
        author.store(&mut doc, handle);
        let after = doc.get_entity_arc(handle).expect("after");

        let undone = collect_undone_changes(&[(handle, Some(before), Some(after))]);
        assert_eq!(undone.len(), 1, "{undone:?}");
        assert_eq!(undone[0].object_ref, "P-1009");
        assert_eq!(undone[0].value, "P-1001");
        assert_eq!(undone[0].old.as_deref(), Some("P-1009"));
    }

    /// The payload for one change reads exactly like `architecture.md` §3.2
    /// writes it, plus what the audit and a same-file peer want: the value it
    /// replaced, and the entity.
    #[test]
    fn a_single_change_payload_carries_the_canonical_fields() {
        let change = Change {
            kind: KIND_SET_TAG.to_owned(),
            object_ref: "P-1001".to_owned(),
            handle: Some(Handle::new(0x1A)),
            field: "tag".to_owned(),
            value: "P-1009".to_owned(),
            old: Some("P-1001".to_owned()),
        };
        let payload = payload_for(std::slice::from_ref(&change)).expect("payload");
        assert_eq!(payload["kind"], KIND_SET_TAG);
        assert_eq!(payload["object_ref"], "P-1001");
        assert_eq!(payload["field"], "tag");
        assert_eq!(payload["value"], "P-1009");
        assert_eq!(payload["old_value"], "P-1001");
        assert_eq!(payload["handle"], "1A");
        assert!(payload.get("changes").is_none());
    }

    #[test]
    fn extra_changes_ride_along_under_changes() {
        let a = change(
            KIND_SET_TAG,
            "P-1001",
            Some(Handle::new(0x1A)),
            "tag",
            "P-1009",
        );
        let b = change(
            KIND_SET_PROPERTY,
            "V-1001",
            Some(Handle::new(0x1B)),
            "设计压力",
            "2.5MPa",
        );
        let payload = payload_for(&[a, b]).expect("payload");
        assert_eq!(payload["object_ref"], "P-1001");
        assert_eq!(payload["changes"].as_array().unwrap().len(), 1);
        assert_eq!(payload["changes"][0]["object_ref"], "V-1001");
    }

    /// VAL-CLI-009: the receiving side writes the new tag onto its own copy.
    #[test]
    fn a_set_tag_change_rewrites_the_local_tag() {
        let (mut doc, handle) = doc_with_author("P-1001", "");
        let applied = apply_changes(
            &mut doc,
            &[change(
                KIND_SET_TAG,
                "P-1001",
                Some(handle),
                "tag",
                "P-1009",
            )],
        );
        assert_eq!(applied.handles, vec![handle]);
        assert_eq!(applied.retagged, vec![handle]);
        assert!(applied.unresolved.is_empty());
        let author = PidAuthor::load(&doc, handle).expect("author");
        assert_eq!(author.tag, "P-1009");
        assert_eq!(author.tag_source, "manual");
    }

    #[test]
    fn a_set_property_change_rewrites_only_that_key() {
        let (mut doc, handle) =
            doc_with_author("P-1001", r#"{"设计压力":"1.6MPa","设计温度":"120℃"}"#);
        apply_changes(
            &mut doc,
            &[change(
                KIND_SET_PROPERTY,
                "P-1001",
                Some(handle),
                "设计压力",
                "2.5MPa",
            )],
        );
        let (values, _) = split_props(&PidAuthor::load(&doc, handle).expect("author").props);
        assert_eq!(values.get("设计压力").map(String::as_str), Some("2.5MPa"));
        assert_eq!(values.get("设计温度").map(String::as_str), Some("120℃"));
    }

    /// A change that names only the tag — a client with no drawing of its own
    /// has nothing else to name — finds the one symbol carrying it.
    #[test]
    fn a_change_naming_only_the_tag_finds_the_symbol_carrying_it() {
        let mut doc = Document::new();
        let pump = add_author(&mut doc, "P-1001", "");
        let vessel = add_author(&mut doc, "V-1001", "");
        let applied = apply_changes(
            &mut doc,
            &[change(KIND_SET_TAG, "V-1001", None, "tag", "V-1002")],
        );
        assert_eq!(applied.handles, vec![vessel]);
        assert_eq!(tag_of(&doc, vessel), "V-1002");
        assert_eq!(tag_of(&doc, pump), "P-1001");
    }

    /// A handle is not shared identity. Two clients that each inserted a
    /// symbol since they opened the file gave the same handle to different
    /// things, so a handle whose entity carries another tag is not believed.
    #[test]
    fn a_handle_whose_entity_carries_another_tag_is_not_believed() {
        let mut doc = Document::new();
        let pump = add_author(&mut doc, "P-1003", "");
        // The sender means its own vessel V-1002, which sits under the handle
        // this drawing gave to a pump.
        let applied = apply_changes(
            &mut doc,
            &[change(
                KIND_SET_PROPERTY,
                "V-1002",
                Some(pump),
                "设计压力",
                "2.5MPa",
            )],
        );
        assert!(applied.is_empty());
        assert_eq!(applied.unresolved, ["V-1002"]);
        assert_eq!(
            PidAuthor::load(&doc, pump).expect("author").props,
            "",
            "the pump must be left alone"
        );
    }

    /// Two symbols carrying one tag: the handle says which is meant, and
    /// without it there is no answer.
    #[test]
    fn the_handle_tells_same_tagged_symbols_apart() {
        let mut doc = Document::new();
        let first = add_author(&mut doc, "P-1001", "");
        let second = add_author(&mut doc, "P-1001", "");

        let applied = apply_changes(
            &mut doc,
            &[change(
                KIND_SET_TAG,
                "P-1001",
                Some(second),
                "tag",
                "P-1002",
            )],
        );
        assert_eq!(applied.handles, vec![second]);
        assert_eq!(tag_of(&doc, first), "P-1001");
        assert_eq!(tag_of(&doc, second), "P-1002");

        let mut doc = Document::new();
        let first = add_author(&mut doc, "P-1001", "");
        let second = add_author(&mut doc, "P-1001", "");
        let applied = apply_changes(
            &mut doc,
            &[change(KIND_SET_TAG, "P-1001", None, "tag", "P-1002")],
        );
        assert!(applied.is_empty());
        assert_eq!(applied.unresolved, ["P-1001"]);
        assert_eq!(tag_of(&doc, first), "P-1001");
        assert_eq!(tag_of(&doc, second), "P-1001");
    }

    /// Re-applying the same change is a no-op, not a failure: a client that
    /// reconnects and re-reads the room must not report an error.
    #[test]
    fn reapplying_a_change_writes_nothing_and_reports_nothing() {
        let (mut doc, handle) = doc_with_author("P-1009", r#"{"设计压力":"2.5MPa"}"#);
        let applied = apply_changes(
            &mut doc,
            &[
                // The retag from P-1001 has been applied: P-1001 is gone and
                // P-1009 is here.
                change(KIND_SET_TAG, "P-1001", Some(handle), "tag", "P-1009"),
                change(
                    KIND_SET_PROPERTY,
                    "P-1009",
                    Some(handle),
                    "设计压力",
                    "2.5MPa",
                ),
            ],
        );
        assert!(applied.is_empty());
        assert!(applied.unresolved.is_empty(), "{:?}", applied.unresolved);
    }

    /// A change naming an object this drawing lacks writes nothing, and says
    /// so: the two drawings differ from here on.
    #[test]
    fn a_change_for_an_absent_object_is_reported_as_unresolved() {
        let (mut doc, _) = doc_with_author("P-1001", "");
        let applied = apply_changes(
            &mut doc,
            &[change(
                KIND_SET_PROPERTY,
                "P-9001",
                Some(Handle::new(0xBEEF)),
                "设计压力",
                "2.5MPa",
            )],
        );
        assert!(applied.is_empty());
        assert_eq!(applied.unresolved, ["P-9001"]);
    }

    /// The `changes` array on the wire round-trips through `from_json`, and an
    /// unknown kind is dropped rather than half-applied.
    #[test]
    fn the_wire_shape_round_trips_and_unknown_kinds_are_dropped() {
        let sent = Change {
            kind: KIND_SET_TAG.to_owned(),
            object_ref: "P-1001".to_owned(),
            handle: Some(Handle::new(0x1A)),
            field: "tag".to_owned(),
            value: "P-1009".to_owned(),
            old: Some("P-1001".to_owned()),
        };
        assert_eq!(Change::from_json(&sent.to_json()), Some(sent));

        // As `architecture.md` §3.2 writes a retag: no field, no handle.
        let bare = json!({"kind":"set_tag","object_ref":"V-1001","value":"V-1002"});
        let change = Change::from_json(&bare).expect("parsed");
        assert_eq!(change.object_ref, "V-1001");
        assert_eq!(change.field, "tag");
        assert_eq!(change.handle, None);

        // The server backfills absent canonical keys with null.
        let backfilled = json!({
            "kind":"set_tag","object_ref":"V-1001","field":null,"value":"V-1002"
        });
        assert_eq!(
            Change::from_json(&backfilled).map(|change| change.field),
            Some("tag".to_owned())
        );

        let unknown = json!({"kind":"delete_object","object_ref":"P-1001"});
        assert!(Change::from_json(&unknown).is_none());
        let nameless = json!({"kind":"set_tag","value":"x"});
        assert!(Change::from_json(&nameless).is_none());
        let valueless = json!({"kind":"set_property","object_ref":"P-1001","field":"设计压力"});
        assert!(Change::from_json(&valueless).is_none());
        let fieldless = json!({"kind":"set_property","object_ref":"P-1001","value":"x"});
        assert!(Change::from_json(&fieldless).is_none());
    }

    /// A batched payload nests the rest under `changes`; the receiver flattens
    /// it so every change is applied.
    #[test]
    fn a_batched_payload_flattens_on_the_receiving_side() {
        let changes = vec![json!({
            "kind":"set_tag","object_ref":"P-1001","field":"tag","value":"P-1009",
            "changes":[{"kind":"set_property","object_ref":"P-1009","field":"设计压力","value":"2.5MPa"}]
        })];
        let parsed = changes_from_payload(&changes);
        assert_eq!(parsed.len(), 2);
        assert_eq!(parsed[0].object_ref, "P-1001");
        assert_eq!(parsed[1].object_ref, "P-1009");
        assert_eq!(parsed[1].field, "设计压力");
    }
}
