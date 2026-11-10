//! What the server knows of a drawing, and the fixed three-step judgment of a
//! change against it (`architecture.md` §3.3, appendix A.4).
//!
//! The server never sees DWG bytes. What it has is the ops it applied, and
//! from them the value every field was last given, by whom and at which
//! revision. That is enough to say, of a change made against an older
//! baseline, which of three things it is:
//!
//! 1. It writes a field nobody else wrote since that baseline: it merges.
//! 2. Somebody else wrote that field since, to another value: the change is
//!    behind and yields; the value it would have put there is reported so the
//!    caller can audit it.
//! 3. Two changes made against the *same* baseline write one field to
//!    different values and neither has taken effect yet: nothing decides
//!    between them, and a conflict entry is made for a person
//!    ([`collides`]).
//!
//! Everything here is a pure function over that state: no clock, no I/O.

use std::collections::BTreeMap;

use serde_json::Value;

/// Op kinds the judgment understands. Anything else is opaque to it.
pub const KIND_SET_PROPERTY: &str = "set_property";
pub const KIND_SET_TAG: &str = "set_tag";

/// The field a retag writes.
pub const FIELD_TAG: &str = "tag";

/// One change an op asks for, as read off its payload.
#[derive(Debug, Clone, PartialEq)]
pub struct Change {
    pub kind: String,
    /// The object, by the tag the sender knows it under.
    pub object_ref: String,
    /// The field written: the property name, or [`FIELD_TAG`] for a retag.
    /// Empty for a kind that writes no single field.
    pub field: String,
    /// The value written. Empty for a kind that carries none.
    pub value: String,
    /// The value the sender replaced, when it said.
    pub old_value: Option<String>,
    /// The change as it arrived, every key the client sent.
    pub raw: Value,
}

impl Change {
    /// Every change an op payload carries, in order: the payload itself, then
    /// the ones a client batched under `changes` so that one undo step is one
    /// op. A payload that is not an object carries none.
    pub fn list(payload: &Value) -> Vec<Change> {
        let Some(first) = Change::read(payload) else {
            return Vec::new();
        };
        let mut changes = vec![first];
        if let Some(rest) = payload.get("changes").and_then(Value::as_array) {
            changes.extend(rest.iter().filter_map(Change::read));
        }
        changes
    }

    fn read(value: &Value) -> Option<Change> {
        let object = value.as_object()?;
        let text = |key: &str| object.get(key).and_then(Value::as_str).map(str::to_owned);
        let kind = text("kind").unwrap_or_default();
        let field = match text("field") {
            Some(field) => field,
            None if kind == KIND_SET_TAG => FIELD_TAG.to_owned(),
            None => String::new(),
        };
        // The batch rides on the first change; it is not part of that change.
        let mut raw = object.clone();
        raw.remove("changes");
        Some(Change {
            kind,
            object_ref: text("object_ref").unwrap_or_default(),
            field,
            value: object.get("value").map(value_text).unwrap_or_default(),
            old_value: object.get("old_value").map(value_text),
            raw: Value::Object(raw),
        })
    }

    /// Whether this is a write to one field of one named object — the only
    /// kind of change the three steps are defined for.
    pub fn writes_a_field(&self) -> bool {
        (self.kind == KIND_SET_PROPERTY || self.kind == KIND_SET_TAG)
            && !self.object_ref.is_empty()
            && !self.field.is_empty()
    }

    /// The change as it goes out in `op_applied`, naming the object by the tag
    /// it has now and backfilling the canonical keys.
    pub fn broadcast_form(&self, object_ref: &str) -> Value {
        let mut out = self.raw.as_object().cloned().unwrap_or_default();
        out.insert("object_ref".to_owned(), Value::String(object_ref.to_owned()));
        for key in ["kind", "field", "value"] {
            out.entry(key.to_owned()).or_insert(Value::Null);
        }
        Value::Object(out)
    }
}

fn value_text(value: &Value) -> String {
    match value {
        Value::String(text) => text.clone(),
        Value::Null => String::new(),
        other => other.to_string(),
    }
}

/// The value a field holds and the op that put it there.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FieldState {
    pub value: String,
    /// Revision at which it became current.
    pub revision: i64,
    pub session_id: String,
    pub user_id: String,
}

/// Every field the server has seen written on one object.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ObjectState {
    pub fields: BTreeMap<String, FieldState>,
}

/// What the three steps say about one change.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Verdict {
    /// It takes effect. `object_ref` is the tag the object goes by now (a
    /// change made before a retag still names the old one); `replaces` is the
    /// value it overwrites, when the server knows one.
    Apply {
        object_ref: String,
        replaces: Option<String>,
    },
    /// It is behind: since its baseline another session gave the field
    /// another value, at `winner_revision`. The winner stays.
    Yield {
        object_ref: String,
        winner: String,
        winner_revision: i64,
        winner_user: String,
    },
}

/// The semantic state of one drawing: its objects by tag, and the retags that
/// let a change made against an older baseline still find its object.
#[derive(Debug, Clone, Default)]
pub struct DrawingModel {
    revision: i64,
    objects: BTreeMap<String, ObjectState>,
    /// `(old tag, new tag, revision)`, oldest first.
    renames: Vec<(String, String, i64)>,
}

impl DrawingModel {
    /// A drawing at `revision` of which nothing else is known yet.
    pub fn at(revision: i64) -> Self {
        Self {
            revision,
            ..Self::default()
        }
    }

    pub fn revision(&self) -> i64 {
        self.revision
    }

    /// The revision moved without a field write the model tracks (an op of a
    /// kind it does not read).
    pub fn set_revision(&mut self, revision: i64) {
        self.revision = revision;
    }

    /// Every object the server has seen written, by current tag.
    pub fn objects(&self) -> impl Iterator<Item = (&String, &ObjectState)> {
        self.objects.iter()
    }

    pub fn object(&self, object_ref: &str) -> Option<&ObjectState> {
        self.objects.get(object_ref)
    }

    /// The value a field holds now.
    pub fn field(&self, object_ref: &str, field: &str) -> Option<&FieldState> {
        self.objects.get(object_ref)?.fields.get(field)
    }

    /// The tag an object named `object_ref` by someone at `base_revision`
    /// goes by now: every retag made after that baseline is followed.
    pub fn current_name(&self, object_ref: &str, base_revision: i64) -> String {
        let mut name = object_ref.to_owned();
        let mut after = base_revision;
        // Retags are in revision order, so one pass follows a chain.
        for (old, new, revision) in &self.renames {
            if *revision > after && *old == name {
                name = new.clone();
                after = *revision;
            }
        }
        name
    }

    /// Steps 1 and 2 for one field write made against `base_revision` by
    /// `session_id`.
    ///
    /// A session's own earlier writes are never what it is behind: it made
    /// them, so a second edit sent before the first one's `op_applied` came
    /// back is still the newer intent, not a lagging one.
    pub fn judge(&self, change: &Change, base_revision: i64, session_id: &str) -> Verdict {
        let object_ref = self.current_name(&change.object_ref, base_revision);
        let current = self.field(&object_ref, &change.field);
        match current {
            Some(state)
                if state.revision > base_revision
                    && state.session_id != session_id
                    && state.value != change.value =>
            {
                Verdict::Yield {
                    object_ref,
                    winner: state.value.clone(),
                    winner_revision: state.revision,
                    winner_user: state.user_id.clone(),
                }
            }
            Some(state) => Verdict::Apply {
                object_ref,
                replaces: (state.value != change.value).then(|| state.value.clone()),
            },
            None => Verdict::Apply {
                object_ref,
                replaces: None,
            },
        }
    }

    /// Record a field write that took effect at `revision`. `object_ref` is
    /// the name [`Verdict::Apply`] gave.
    pub fn commit(
        &mut self,
        change: &Change,
        object_ref: &str,
        revision: i64,
        session_id: &str,
        user_id: &str,
    ) {
        self.revision = self.revision.max(revision);
        if !change.writes_a_field() {
            return;
        }
        let state = FieldState {
            value: change.value.clone(),
            revision,
            session_id: session_id.to_owned(),
            user_id: user_id.to_owned(),
        };
        if change.kind == KIND_SET_TAG {
            if change.value == object_ref || change.value.is_empty() {
                return;
            }
            // The object keeps its fields under its new tag. Objects are
            // keyed by tag here, so one already carrying the new tag is the
            // same entry from now on; its fields stay where the retagged
            // object has none.
            let mut moved = self.objects.remove(object_ref).unwrap_or_default();
            moved.fields.insert(FIELD_TAG.to_owned(), state);
            let target = self.objects.entry(change.value.clone()).or_default();
            for (field, value) in moved.fields {
                target.fields.insert(field, value);
            }
            self.renames
                .push((object_ref.to_owned(), change.value.clone(), revision));
            return;
        }
        self.objects
            .entry(object_ref.to_owned())
            .or_default()
            .fields
            .insert(change.field.clone(), state);
    }
}

/// Step 3's condition between two field writes made against one baseline,
/// neither of which has taken effect: the same field of the same object, to
/// different values. Which baseline, and that both are still pending, is the
/// caller's to establish.
pub fn collides(a: &Change, b: &Change) -> bool {
    a.writes_a_field()
        && b.writes_a_field()
        && a.object_ref == b.object_ref
        && a.field == b.field
        && a.value != b.value
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn set(object_ref: &str, field: &str, value: &str) -> Change {
        Change::list(&json!({
            "kind": KIND_SET_PROPERTY, "object_ref": object_ref, "field": field, "value": value,
        }))
        .remove(0)
    }

    fn retag(old: &str, new: &str) -> Change {
        Change::list(&json!({"kind": KIND_SET_TAG, "object_ref": old, "value": new})).remove(0)
    }

    /// A model in which `session` wrote `field` of P-1001 at `revision`.
    fn written(field: &str, value: &str, revision: i64, session: &str) -> DrawingModel {
        let mut model = DrawingModel::default();
        model.commit(&set("P-1001", field, value), "P-1001", revision, session, "u1");
        model
    }

    fn applies(verdict: &Verdict) -> bool {
        matches!(verdict, Verdict::Apply { .. })
    }

    // Step 1 (`VAL-CONF-001`): s1 wrote 设计压力 at revision 1; s2, still at
    // baseline 0, writes 公称直径 of the same object. Nothing to decide.
    #[test]
    fn a_field_nobody_else_wrote_since_the_baseline_merges() {
        let model = written("设计压力", "1.6MPa", 1, "s1");
        let verdict = model.judge(&set("P-1001", "公称直径", "DN50"), 0, "s2");
        assert_eq!(
            verdict,
            Verdict::Apply {
                object_ref: "P-1001".into(),
                replaces: None
            }
        );
    }

    // Step 2 (`VAL-CONF-002`, `-005`): the same field, written by another
    // session after the change's baseline.
    #[test]
    fn a_field_someone_else_wrote_since_the_baseline_is_kept_and_the_change_yields() {
        let model = written("设计压力", "1.6MPa", 1, "s1");
        assert_eq!(
            model.judge(&set("P-1001", "设计压力", "2.5MPa"), 0, "s2"),
            Verdict::Yield {
                object_ref: "P-1001".into(),
                winner: "1.6MPa".into(),
                winner_revision: 1,
                winner_user: "u1".into(),
            }
        );
        // `VAL-CONF-005`: written at revision 3, the change carries baseline 1.
        let model = written("公称直径", "DN50", 3, "s1");
        assert!(!applies(&model.judge(&set("P-1001", "公称直径", "DN80"), 1, "s2")));
    }

    // A change that has seen the current value simply replaces it — the
    // ordinary edit — and says what it replaced.
    #[test]
    fn a_change_made_against_the_current_value_replaces_it() {
        let model = written("设计压力", "1.6MPa", 3, "s1");
        assert_eq!(
            model.judge(&set("P-1001", "设计压力", "2.5MPa"), 3, "s2"),
            Verdict::Apply {
                object_ref: "P-1001".into(),
                replaces: Some("1.6MPa".into())
            }
        );
    }

    // Two people arriving at the same value disagree about nothing.
    #[test]
    fn the_same_value_never_yields() {
        let model = written("设计压力", "1.6MPa", 3, "s1");
        assert_eq!(
            model.judge(&set("P-1001", "设计压力", "1.6MPa"), 0, "s2"),
            Verdict::Apply {
                object_ref: "P-1001".into(),
                replaces: None
            }
        );
    }

    // A session is not behind its own writes: a second edit sent before the
    // first came back as `op_applied` carries the older baseline and is still
    // the newer intent.
    #[test]
    fn a_session_does_not_yield_to_its_own_earlier_write() {
        let model = written("设计压力", "1.6MPa", 1, "s1");
        assert_eq!(
            model.judge(&set("P-1001", "设计压力", "2.5MPa"), 0, "s1"),
            Verdict::Apply {
                object_ref: "P-1001".into(),
                replaces: Some("1.6MPa".into())
            }
        );
    }

    #[test]
    fn an_object_never_written_takes_every_field() {
        let model = DrawingModel::default();
        for field in ["设计压力", "公称直径", "绝热等级"] {
            assert!(applies(&model.judge(&set("P-1001", field, "x"), 0, "s1")));
        }
    }

    #[test]
    fn committing_replaces_the_value_and_keeps_one_entry_per_field() {
        let mut model = written("设计压力", "1.6MPa", 1, "s1");
        model.commit(&set("P-1001", "设计压力", "2.5MPa"), "P-1001", 5, "s2", "u2");
        let state = model.field("P-1001", "设计压力").expect("the field");
        assert_eq!(
            (state.value.as_str(), state.revision, state.user_id.as_str()),
            ("2.5MPa", 5, "u2")
        );
        assert_eq!(model.object("P-1001").expect("object").fields.len(), 1);
        assert_eq!(model.revision(), 5);
    }

    // The question a retag raises: the op names the tag the object *had*.
    // Afterwards it is known by the new one — and a change made by someone
    // who had not seen the retag still names the old one. It must reach the
    // same object, and go out naming it as everyone now knows it.
    #[test]
    fn a_change_made_before_a_retag_still_finds_the_object() {
        let mut model = written("设计压力", "1.6MPa", 1, "s1");
        model.commit(&retag("P-1001", "P-1009"), "P-1001", 2, "s1", "u1");
        assert!(model.object("P-1001").is_none(), "known by the new tag only");
        assert_eq!(
            model.field("P-1009", "设计压力").map(|f| f.value.as_str()),
            Some("1.6MPa"),
            "its fields moved with it"
        );

        // s2 had seen revision 1, not the retag at 2.
        assert_eq!(
            model.judge(&set("P-1001", "公称直径", "DN50"), 1, "s2"),
            Verdict::Apply {
                object_ref: "P-1009".into(),
                replaces: None
            }
        );
        // Someone who has seen the retag and says P-1001 means whatever is
        // called P-1001 now — a different object.
        assert_eq!(
            model.judge(&set("P-1001", "公称直径", "DN50"), 2, "s2"),
            Verdict::Apply {
                object_ref: "P-1001".into(),
                replaces: None
            }
        );
        // And a chain is followed to its end.
        model.commit(&retag("P-1009", "P-1010"), "P-1009", 3, "s1", "u1");
        assert_eq!(model.current_name("P-1001", 1), "P-1010");
        assert_eq!(model.current_name("P-1009", 2), "P-1010");
        assert_eq!(model.current_name("P-1010", 3), "P-1010");
    }

    // Two retags of one object made against the same baseline: the tag is a
    // field like any other, and the later one yields.
    #[test]
    fn a_retag_behind_another_retag_yields() {
        let mut model = DrawingModel::default();
        model.commit(&retag("P-1001", "P-1009"), "P-1001", 1, "s1", "u1");
        assert_eq!(
            model.judge(&retag("P-1001", "P-1005"), 0, "s2"),
            Verdict::Yield {
                object_ref: "P-1009".into(),
                winner: "P-1009".into(),
                winner_revision: 1,
                winner_user: "u1".into(),
            }
        );
    }

    // Step 3's condition (`VAL-CONF-004`).
    #[test]
    fn two_writes_collide_only_on_one_field_of_one_object_with_different_values() {
        let a = set("P-1001", "设计压力", "1.6MPa");
        assert!(collides(&a, &set("P-1001", "设计压力", "2.5MPa")));
        assert!(!collides(&a, &set("P-1001", "设计压力", "1.6MPa")), "same value");
        assert!(!collides(&a, &set("P-1001", "公称直径", "DN50")), "another field");
        assert!(!collides(&a, &set("P-1002", "设计压力", "2.5MPa")), "another object");
        assert!(collides(&retag("P-1001", "P-1005"), &retag("P-1001", "P-1009")));
    }

    // One undo step is one op: the first change is the payload, the rest ride
    // under `changes`. Every one of them is read; the batch is not part of
    // the first change.
    #[test]
    fn a_payload_lists_its_own_change_then_the_batched_ones() {
        let payload = json!({
            "kind": "set_tag", "object_ref": "P-1001", "value": "P-1009", "old_value": "P-1001",
            "handle": "1A",
            "changes": [
                {"kind": "set_property", "object_ref": "P-1009", "field": "设计压力",
                 "value": "2.5MPa", "old_value": "1.6MPa"},
                {"kind": "set_property", "object_ref": "P-1009", "field": "公称直径", "value": 50},
            ],
        });
        let changes = Change::list(&payload);
        assert_eq!(changes.len(), 3);
        assert_eq!(
            (changes[0].kind.as_str(), changes[0].field.as_str(), changes[0].value.as_str()),
            ("set_tag", "tag", "P-1009")
        );
        assert!(changes[0].raw.get("changes").is_none());
        assert_eq!(changes[0].raw["handle"], "1A");
        assert_eq!(changes[1].old_value.as_deref(), Some("1.6MPa"));
        assert_eq!(changes[2].value, "50", "a number is read as its text");
        assert!(changes.iter().all(Change::writes_a_field));

        assert!(Change::list(&json!("not an object")).is_empty());
        let opaque = Change::list(&json!({"kind": "insert_symbol", "tag": "V-1001"}));
        assert_eq!(opaque.len(), 1);
        assert!(!opaque[0].writes_a_field());
    }

    #[test]
    fn the_broadcast_form_names_the_object_as_it_is_known_now() {
        let change = set("P-1001", "设计压力", "2.5MPa");
        let out = change.broadcast_form("P-1009");
        assert_eq!(out["object_ref"], "P-1009");
        assert_eq!(out["field"], "设计压力");
        assert_eq!(out["value"], "2.5MPa");
        let bare = Change::list(&json!({"kind": "set_tag", "object_ref": "V-1", "value": "V-2"}))
            .remove(0);
        assert_eq!(bare.broadcast_form("V-1")["field"], Value::Null, "backfilled");
    }
}
