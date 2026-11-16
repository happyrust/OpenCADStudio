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
//! * [`apply_change`] writes those semantics onto the local document, using
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
//!
//! An object with no tag — a pipe segment, a break, a connector — therefore
//! has no name on the wire, and nothing is said about it. What it carries is
//! derived from a tagged object (a segment takes its run's values), and each
//! drawing derives it for itself when the tagged change arrives
//! (`apply_remote_changes` runs the same carry-down a local edit does).

use document::{Document, EntityType, Handle};
use serde_json::{json, Value};

use crate::pid::persist::PidAuthor;
use crate::pid::propagate::{join_props, split_props};
use crate::pid::tag_engine::TAG_SOURCE_MANUAL;
use crate::pid::topology::{
    parse_handle, ROLE_FITTING, ROLE_PIPE_LINE, ROLE_PIPE_RUN, ROLE_SIGNAL_LINE,
};

/// Op kinds this client emits and understands.
pub const KIND_SET_TAG: &str = "set_tag";
pub const KIND_SET_PROPERTY: &str = "set_property";
/// Not a wire kind: this client's note to itself that the server gave a
/// symbol it placed another tag than the one it was sent under, because
/// that one was taken (`State::apply` writes it when its own `insert_symbol`
/// comes back saying `reassigned_from`). Written like a retag, to the entity
/// the symbol was placed as.
pub const KIND_REASSIGN_TAG: &str = "reassign_tag";
/// A symbol placed: the object comes into being under the tag it was given.
pub const KIND_INSERT_SYMBOL: &str = "insert_symbol";
/// An object erased.
pub const KIND_DELETE_OBJECT: &str = "delete_object";

/// What a placed symbol is, beyond its tag: enough for a peer to place the
/// same symbol in its own drawing. This is the one place geometry travels —
/// an insertion point, a rotation and a scale, which is what a symbol's
/// placement is; the symbol's own lines come from the library on each side.
#[derive(Clone, Debug, PartialEq)]
pub struct Placed {
    /// The symbol's block (`PID_SYM_<category>_<id>`).
    pub block: String,
    /// The author type code (`VALVE`, `EQUIPMENT`, …).
    pub type_code: String,
    pub position: [f64; 3],
    /// Radians, as the entity stores it.
    pub rotation: f64,
    /// Uniform, as every placement is made.
    pub scale: f64,
    pub ports: String,
    /// The author record's `props` text.
    pub props: String,
    pub tag_source: String,
}

// Placements are finite numbers; `Change` is compared and kept in sets of
// its own, which is all this is for.
impl Eq for Placed {}

/// What a change naming no tag is called when it is reported as unresolved.
pub const UNTAGGED: &str = "（无位号）";

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
    /// The entities the sender changed for it, for a peer holding the same
    /// file. Usually one. A pipe line or a signal line is one object drawn
    /// as several entities that all carry its tag, and a change made to
    /// several of them is still one change to one object — stated once,
    /// with every entity it was written to.
    pub handles: Vec<Handle>,
    pub field: String,
    pub value: String,
    /// The value being replaced. The server's audit line reads it
    /// (`old_value`), which is how a change says what it overwrote.
    ///
    /// For [`KIND_INSERT_SYMBOL`] it is the tag the sender had given the
    /// symbol, when the server gave it another because that one was taken
    /// (`reassigned_from`).
    pub old: Option<String>,
    /// What was placed, for [`KIND_INSERT_SYMBOL`]; `None` for every change
    /// to an object that exists.
    pub placed: Option<Placed>,
    /// What a statement about a pipe line or a run says of it — the
    /// stretches the line has, the run as the step left it
    /// (`super::pipes`); `None` for every other change. `object_ref` is
    /// then the line's tag or the run's.
    pub pipe: Option<super::pipes::Pipe>,
}

impl Change {
    /// The JSON body one change becomes inside an `op` payload.
    pub fn to_json(&self) -> Value {
        if let Some(pipe) = &self.pipe {
            return super::pipes::to_json(self, pipe);
        }
        let mut change = json!({
            "kind": self.kind,
            "object_ref": self.object_ref,
            "field": self.field,
            "value": self.value,
        });
        if let Some(placed) = &self.placed {
            // `tag` and `type` are the keys the server reads a new symbol
            // by (`architecture.md` §3.2); `props` goes as an object, so the
            // server knows the fields the symbol starts with.
            change["tag"] = json!(self.object_ref);
            change["type"] = json!(placed.type_code);
            change["block"] = json!(placed.block);
            change["position"] = json!(placed.position);
            change["rotation"] = json!(placed.rotation);
            change["scale"] = json!(placed.scale);
            change["ports"] = json!(placed.ports);
            change["tag_source"] = json!(placed.tag_source);
            let (values, _) = split_props(&placed.props);
            change["props"] = json!(values);
        } else if let Some(old) = &self.old {
            change["old_value"] = json!(old);
        }
        // `handle` is the first, as it has always been written; `handles`
        // appears only when there is more than one to say.
        let text = |handle: &Handle| format!("{:X}", handle.value());
        if let Some(first) = self.handles.first() {
            change["handle"] = json!(text(first));
        }
        if self.handles.len() > 1 {
            change["handles"] = json!(self.handles.iter().map(text).collect::<Vec<_>>());
        }
        change
    }

    /// Read one change back out of an `op_applied` `changes` entry. `None` for
    /// a change this client cannot act on (an unknown kind, no object named, a
    /// missing value) — applying half of it would be worse.
    pub fn from_json(value: &Value) -> Option<Self> {
        let kind = value["kind"].as_str()?.to_owned();
        if matches!(
            kind.as_str(),
            super::pipes::KIND_CREATE_PIPE_LINE
                | super::pipes::KIND_ADD_PIPE_RUN
                | super::pipes::KIND_INSERT_FITTING
                | super::pipes::KIND_UPDATE_PIPE_RUN
        ) {
            return super::pipes::from_json(&kind, value);
        }
        if matches!(
            kind.as_str(),
            super::connectors::KIND_CREATE_CONNECTOR
                | super::connectors::KIND_DELETE_CONNECTOR
                | super::connectors::KIND_PAIR_CONNECTOR
        ) {
            return super::connectors::from_json(&kind, value);
        }
        let handles: Vec<Handle> = match value["handles"].as_array() {
            Some(listed) => listed
                .iter()
                .filter_map(|entry| entry.as_str().and_then(parse_handle))
                .collect(),
            None => value["handle"]
                .as_str()
                .and_then(parse_handle)
                .into_iter()
                .collect(),
        };
        if kind == KIND_INSERT_SYMBOL {
            // The server names the symbol by the tag it has now
            // (`object_ref`), which is not the tag it was sent under when
            // that one was taken; a payload nobody has rewritten says `tag`.
            let tag = value["object_ref"]
                .as_str()
                .filter(|tag| !tag.is_empty())
                .or_else(|| value["tag"].as_str())
                .filter(|tag| !tag.is_empty())?
                .to_owned();
            let number = |entry: &Value| entry.as_f64().filter(|n| n.is_finite());
            let position = value["position"].as_array()?;
            let position = [
                number(position.first()?)?,
                number(position.get(1)?)?,
                position.get(2).and_then(number).unwrap_or(0.0),
            ];
            let props = match &value["props"] {
                Value::Object(map) => Value::Object(map.clone()).to_string(),
                Value::String(text) => text.clone(),
                _ => "{}".to_owned(),
            };
            return Some(Self {
                kind,
                object_ref: tag,
                handles,
                field: String::new(),
                value: String::new(),
                old: value["reassigned_from"].as_str().map(str::to_owned),
                placed: Some(Placed {
                    block: value["block"].as_str().filter(|block| !block.is_empty())?.to_owned(),
                    type_code: value["type"].as_str()?.to_owned(),
                    position,
                    rotation: number(&value["rotation"]).unwrap_or(0.0),
                    scale: number(&value["scale"]).filter(|scale| *scale != 0.0).unwrap_or(1.0),
                    ports: value["ports"].as_str().unwrap_or_default().to_owned(),
                    props,
                    tag_source: value["tag_source"].as_str().unwrap_or("auto").to_owned(),
                }),
                pipe: None,
            });
        }
        if kind == KIND_DELETE_OBJECT {
            return Some(Self {
                kind,
                object_ref: value["object_ref"].as_str()?.to_owned(),
                handles,
                field: String::new(),
                value: String::new(),
                old: None,
                placed: None,
                pipe: None,
            });
        }
        if kind != KIND_SET_TAG && kind != KIND_SET_PROPERTY && kind != KIND_REASSIGN_TAG {
            return None;
        }
        let object_ref = value["object_ref"].as_str()?.to_owned();
        let field = match value["field"].as_str() {
            Some(field) => field.to_owned(),
            // `architecture.md` writes a retag without the field.
            None if kind != KIND_SET_PROPERTY => "tag".to_owned(),
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
            handles,
            field,
            value: value_text,
            old: value["old_value"].as_str().map(str::to_owned),
            placed: None,
            pipe: None,
        })
    }

    /// Whether `other` says the same thing of the same object: everything
    /// but the entities it was written to.
    fn states_the_same_as(&self, other: &Self) -> bool {
        self.kind == other.kind
            && self.object_ref == other.object_ref
            && self.field == other.field
            && self.value == other.value
            && self.old == other.old
            && self.placed == other.placed
            && self.pipe == other.pipe
    }

    /// Whether this brings an object into the drawing or takes one out —
    /// or says what a pipe line or a run is made of — rather than changing
    /// a field of one that is there.
    pub fn is_structural(&self) -> bool {
        self.kind == KIND_INSERT_SYMBOL || self.kind == KIND_DELETE_OBJECT || self.pipe.is_some()
    }
}

/// Whether `entity` is a symbol standing on its own: an `Insert` carrying an
/// author record with a tag, that is not part of a pipe line (a fitting on a
/// run belongs to the run; the pipe's own pieces are not `Insert`s).
///
/// Those are the objects whose coming and going is said entity by entity.
/// A pipe line, its runs and what is fitted on them are made of several
/// entities and a table between them; what a step did to those is said by
/// `super::pipes::statements`, from the drawing as the step left it
/// ([`collect_step_changes`]).
fn standalone_symbol(entity: &EntityType) -> Option<(&document::entities::Insert, PidAuthor)> {
    let EntityType::Insert(insert) = entity else {
        return None;
    };
    let author = PidAuthor::from_entity(entity)?;
    (!author.tag.is_empty() && author.owner.is_empty() && author.topology.is_empty())
        .then_some((insert, author))
}

/// A symbol that was not there before the step and is after it.
fn placed_symbol(handle: Handle, entity: &EntityType) -> Option<Change> {
    let (insert, author) = standalone_symbol(entity)?;
    Some(Change {
        kind: KIND_INSERT_SYMBOL.to_owned(),
        object_ref: author.tag.clone(),
        handles: vec![handle],
        field: String::new(),
        value: String::new(),
        old: None,
        placed: Some(Placed {
            block: insert.block_name.clone(),
            type_code: author.type_code,
            position: [
                insert.insert_point.x,
                insert.insert_point.y,
                insert.insert_point.z,
            ],
            rotation: insert.rotation,
            scale: insert.x_scale(),
            ports: author.ports,
            // Properties that overflowed into an XRecord are not in the
            // image; the symbol is said to start with none rather than
            // not said at all.
            props: if PidAuthor::props_are_inline(entity) {
                author.props
            } else {
                "{}".to_owned()
            },
            tag_source: author.tag_source,
        }),
        pipe: None,
    })
}

/// A symbol that was there before the step and is not after it.
fn erased_symbol(handle: Handle, entity: &EntityType) -> Option<Change> {
    let (_, author) = standalone_symbol(entity)?;
    Some(Change {
        kind: KIND_DELETE_OBJECT.to_owned(),
        object_ref: author.tag,
        handles: vec![handle],
        field: String::new(),
        value: String::new(),
        old: None,
        placed: None,
        pipe: None,
    })
}

/// What one handle's before/after images amount to in P&ID semantics: a
/// retag, if the tag moved, then every property whose value moved.
///
/// The retag comes first and names the object by the tag it had; the property
/// changes after it name it by the tag it has now, so a peer applying them in
/// order finds it each time.
///
/// A symbol that came into being or went out of it is said as that — where
/// it stands and what it is, or that it is gone (`placed_symbol`,
/// `erased_symbol`). Any other created or erased entity yields nothing
/// rather than a half-truth: the geometry of a pipe is not in this payload.
/// An image whose `props` overflowed into an XRecord is "not known here",
/// not "no properties": the properties of such a step are left out rather
/// than broadcast as removals.
pub fn changes_of(
    handle: Handle,
    before: Option<&EntityType>,
    after: Option<&EntityType>,
) -> Vec<Change> {
    let (before, after) = match (before, after) {
        (Some(before), Some(after)) => (before, after),
        (None, Some(after)) => return placed_symbol(handle, after).into_iter().collect(),
        (Some(before), None) => return erased_symbol(handle, before).into_iter().collect(),
        (None, None) => return Vec::new(),
    };
    let (Some(before_author), Some(after_author)) = (
        PidAuthor::from_entity(before),
        PidAuthor::from_entity(after),
    ) else {
        return Vec::new();
    };
    // An object the peers know by no tag cannot be named to them at all (see
    // `resolve`), so nothing is stated about it — not even the tag it is
    // being given, which they would have nothing to attach to.
    if before_author.tag.is_empty() {
        return Vec::new();
    }

    let mut changes = Vec::new();
    if before_author.tag != after_author.tag {
        changes.push(Change {
            kind: KIND_SET_TAG.to_owned(),
            object_ref: before_author.tag.clone(),
            handles: vec![handle],
            field: "tag".to_owned(),
            value: after_author.tag.clone(),
            old: Some(before_author.tag.clone()),
            placed: None,
            pipe: None,
        });
    }

    // Retagged to nothing: from here on it has no name either.
    if after_author.tag.is_empty() {
        return changes;
    }
    if !PidAuthor::props_are_inline(before) || !PidAuthor::props_are_inline(after) {
        return changes;
    }
    let (before_props, _) = split_props(&before_author.props);
    let (after_props, _) = split_props(&after_author.props);
    let property = |field: &str, value: &str, old: Option<&String>| Change {
        kind: KIND_SET_PROPERTY.to_owned(),
        object_ref: after_author.tag.clone(),
        handles: vec![handle],
        field: field.to_owned(),
        value: value.to_owned(),
        old: old.cloned(),
        placed: None,
        pipe: None,
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
    stated_once(entities.iter().flat_map(|(handle, before, after)| {
        changes_of(*handle, before.as_deref(), after.as_deref())
    }))
}

/// What undoing that step amounts to: the same images read the other way.
pub fn collect_undone_changes(entities: &EntityImages) -> Vec<Change> {
    stated_once(entities.iter().flat_map(|(handle, before, after)| {
        changes_of(*handle, after.as_deref(), before.as_deref())
    }))
}

/// Everything one step says to the room: what [`collect_changes`] reads
/// off its images, then what it did to pipe lines and runs — which the
/// images only point at, and `doc`, the drawing as the step left it, says
/// (`super::pipes::statements`).
pub fn collect_step_changes(doc: &Document, entities: &EntityImages) -> Vec<Change> {
    let mut changes = collect_changes(entities);
    let images: Vec<super::pipes::Image<'_>> = entities
        .iter()
        .map(|(handle, before, after)| (*handle, before.as_deref(), after.as_deref()))
        .collect();
    changes.extend(super::pipes::statements(doc, &images));
    changes.extend(super::connectors::statements(doc, &images));
    changes
}

/// [`collect_step_changes`] for a step that has just been taken back:
/// its images read the other way, and `doc` the drawing without the step.
pub fn collect_undone_step_changes(doc: &Document, entities: &EntityImages) -> Vec<Change> {
    let mut changes = collect_undone_changes(entities);
    let images: Vec<super::pipes::Image<'_>> = entities
        .iter()
        .map(|(handle, before, after)| (*handle, after.as_deref(), before.as_deref()))
        .collect();
    changes.extend(super::pipes::statements(doc, &images));
    changes.extend(super::connectors::statements(doc, &images));
    changes
}

/// One object, one statement. A pipe line renamed is every one of its
/// entities retagged, which read entity by entity is the same change said
/// once per entity; the room — and the audit log — should hear that the line
/// was renamed, once, with the entities it was written to riding along.
/// Order is kept: the first time a thing is said is where it stays.
fn stated_once(changes: impl Iterator<Item = Change>) -> Vec<Change> {
    let mut out: Vec<Change> = Vec::new();
    for change in changes {
        match out.iter_mut().find(|said| said.states_the_same_as(&change)) {
            Some(said) => {
                for handle in change.handles {
                    if !said.handles.contains(&handle) {
                        said.handles.push(handle);
                    }
                }
            }
            None => out.push(change),
        }
    }
    out
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

/// What an `op_applied` `changes` array says, as far as this client can
/// read it.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Read {
    /// The changes it can act on, in order.
    pub changes: Vec<Change>,
    /// The ones it cannot, each by its `kind` (or `?` when it states none).
    /// They took effect in the room all the same, so a drawing that skips
    /// them is short of it — which is for the caller to say, not to pass
    /// over because the rest of the op could be applied.
    pub unread: Vec<String>,
}

/// Every change an `op_applied` `changes` array carries, including the ones a
/// batched payload nested under `changes`.
pub fn changes_from_payload(changes: &[Value]) -> Read {
    let mut read = Read::default();
    let mut take = |entry: &Value| match Change::from_json(entry) {
        Some(change) => read.changes.push(change),
        None => read
            .unread
            .push(entry["kind"].as_str().unwrap_or("?").to_owned()),
    };
    for entry in changes {
        take(entry);
        if let Some(nested) = entry["changes"].as_array() {
            nested.iter().for_each(&mut take);
        }
    }
    read
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
    /// Every change that found its object, with the entities it was found to
    /// mean — whether or not a value had to be written. The caller keeps
    /// these: taking back a local step restores entity images from before
    /// the change, and the change is then written to the same entities
    /// again ([`write_to`]).
    pub resolved: Vec<(Change, Vec<Handle>)>,
    /// Entities that were erased because a peer erased their object. They
    /// are not in `handles`: nothing is left of them to re-derive from.
    pub erased: Vec<Handle>,
}

impl Applied {
    /// Nothing was written.
    pub fn is_empty(&self) -> bool {
        self.handles.is_empty() && self.erased.is_empty()
    }
}

/// The entities a change is to be written to — empty when this drawing has
/// no such object, or cannot tell which of several is meant.
///
/// The tag decides. The entities the sender named are taken when they carry
/// the tag the change names here too: a peer holding the same file then
/// writes exactly what the sender wrote, and of several symbols carrying one
/// tag the handle says which is meant.
///
/// Without that — a sender with another file, or with no drawing at all —
/// the tag has to name one object:
///
/// * one entity carrying it is that object;
/// * several entities carrying it are one object when they are the pieces
///   of a line. A pipe line or a signal line is drawn as one entity per
///   stretch, each carrying the line's tag (`PIDPIPE`), so the change goes
///   to all of them — among them the one the topology table reads the
///   line's values from, which is what carries them down to its runs;
/// * several separate symbols carrying one tag are a duplicate, and name
///   nothing.
///
/// A change naming no tag names nothing. Pipe segments, breaks and connectors
/// carry no tag, and "the untagged thing under handle H" is a different thing
/// in every drawing that has had something drawn in it since it was opened.
/// The comparison is the stored text exactly — case and surrounding blanks
/// included — which is how the tag engine's own uniqueness check compares,
/// and what `canonical_tag` turns a typed tag into before it is claimed.
pub fn resolve(doc: &Document, change: &Change) -> Vec<Handle> {
    if change.object_ref.is_empty() {
        return Vec::new();
    }
    let carries = |handle: &Handle| {
        PidAuthor::load(doc, *handle).is_some_and(|author| author.tag == change.object_ref)
    };
    // All of them or none: a sender whose entities only partly match is not
    // holding this file, and its handles say nothing about this drawing.
    if !change.handles.is_empty() && change.handles.iter().all(carries) {
        return change.handles.clone();
    }
    let carrying: Vec<(Handle, PidAuthor)> = doc
        .entities()
        .filter_map(|entity| {
            let handle = entity.common().handle;
            let author = PidAuthor::load(doc, handle)?;
            (author.tag == change.object_ref).then_some((handle, author))
        })
        .collect();
    let one_line = carrying.len() > 1
        && carrying.iter().all(|(_, author)| {
            author.type_code == carrying[0].1.type_code
                && author.topology == carrying[0].1.topology
                && matches!(
                    author.topology.as_str(),
                    ROLE_PIPE_LINE | ROLE_SIGNAL_LINE
                )
        });
    if carrying.len() == 1 || one_line {
        return carrying.into_iter().map(|(handle, _)| handle).collect();
    }
    // A run and the fittings on it that have no tag of their own: those
    // show the run's tag without being the run, and the tag means the run.
    let mut runs = carrying
        .iter()
        .filter(|(_, author)| author.topology == ROLE_PIPE_RUN);
    match (runs.next(), runs.next()) {
        (Some((run, _)), None)
            if carrying.iter().all(|(_, author)| {
                matches!(author.topology.as_str(), ROLE_PIPE_RUN | ROLE_FITTING)
            }) =>
        {
            vec![*run]
        }
        _ => Vec::new(),
    }
}

/// Whether a retag is in place here already: the entities the sender wrote
/// it to carry the new tag. That is certain only of a peer holding the same
/// file, and only then is the change passed over without a word.
///
/// It used to be enough that nothing carried the old tag and *something*
/// carried the new one. That also holds when this drawing never had the
/// object and has another under the new tag — a rename that did not happen
/// here, reported as done.
fn already_retagged(doc: &Document, change: &Change) -> bool {
    (change.kind == KIND_SET_TAG || change.kind == KIND_REASSIGN_TAG)
        && !change.handles.is_empty()
        && change.handles.iter().all(|handle| {
            PidAuthor::load(doc, *handle).is_some_and(|author| author.tag == change.value)
        })
}

/// Write one change onto `doc`.
///
/// A change whose value is already in place writes nothing and is not an
/// error: re-applying an already-applied change must not fail. A change
/// naming an object this drawing does not have is recorded in
/// `applied.unresolved`.
pub fn apply_change(doc: &mut Document, change: &Change, applied: &mut Applied) {
    let targets = resolve(doc, change);
    if targets.is_empty() {
        if change.object_ref.is_empty() {
            applied.unresolved.push(UNTAGGED.to_owned());
        } else if !already_retagged(doc, change) {
            applied.unresolved.push(change.object_ref.clone());
        }
        return;
    }
    write_to(doc, &targets, change, applied);
    applied.resolved.push((change.clone(), targets));
}

/// Write `change` to the entities `targets`, whatever tag they carry now.
///
/// This is the second half of [`apply_change`], and what applies a peer's
/// change a second time after a local undo or redo put back entity images
/// from before it: the entities are the ones it was resolved to when it
/// arrived, and by then the object may be under another name here (the undo
/// took back a retag). An entity that is gone, or has no record, is skipped.
pub fn write_to(doc: &mut Document, targets: &[Handle], change: &Change, applied: &mut Applied) {
    for &handle in targets {
        let Some(mut author) = PidAuthor::load(doc, handle) else {
            continue;
        };
        match change.kind.as_str() {
            KIND_SET_TAG => {
                if author.tag == change.value {
                    continue;
                }
                author.tag = change.value.clone();
                author.tag_source = TAG_SOURCE_MANUAL.to_owned();
                author.store(doc, handle);
                applied.retagged.push(handle);
            }
            // The server's number in place of the one this drawing worked
            // out: still a tag nobody typed, so its source stays what it was.
            KIND_REASSIGN_TAG => {
                if author.tag == change.value {
                    continue;
                }
                author.tag = change.value.clone();
                author.store(doc, handle);
                applied.retagged.push(handle);
            }
            KIND_SET_PROPERTY => {
                let (mut values, marks) = split_props(&author.props);
                let current = values.get(&change.field).cloned().unwrap_or_default();
                if current == change.value {
                    continue;
                }
                if change.value.is_empty() {
                    values.remove(&change.field);
                } else {
                    values.insert(change.field.clone(), change.value.clone());
                }
                author.props = join_props(&values, &marks);
                author.store(doc, handle);
            }
            _ => continue,
        }
        if !applied.handles.contains(&handle) {
            applied.handles.push(handle);
        }
    }
}

/// Write a whole change list onto `doc`, in order. For the tests of this
/// module: the app goes change by change (`write_remote_changes`), since a
/// symbol placed or an object erased is its business, not the document's.
#[cfg(test)]
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

    /// A symbol that was not in the drawing before a step and is after it
    /// is said as placed — what, where, under which tag — and one that was
    /// and is not, as erased. What a peer needs to place it survives the
    /// wire. A fitting on a run, and a symbol with no tag, are not said.
    #[test]
    fn a_symbol_that_came_or_went_is_said_as_that() {
        let mut doc = Document::new();
        let handle = add_author(&mut doc, "V-1001", r#"{"公称直径":"DN50"}"#);
        let entity = doc.get_entity(handle).cloned().expect("entity");

        let placed = changes_of(handle, None, Some(&entity));
        assert_eq!(placed.len(), 1, "{placed:?}");
        assert_eq!(placed[0].kind, KIND_INSERT_SYMBOL);
        assert_eq!(placed[0].object_ref, "V-1001");
        assert_eq!(placed[0].handles, [handle]);
        let what = placed[0].placed.clone().expect("what was placed");
        assert_eq!(what.block, "PID_SYM_equipment_vessel");
        assert_eq!(what.type_code, "EQUIPMENT");
        assert_eq!(what.position, [0.0, 0.0, 0.0]);

        let wire = placed[0].to_json();
        assert_eq!(wire["tag"], "V-1001");
        assert_eq!(wire["type"], "EQUIPMENT");
        assert_eq!(wire["props"]["公称直径"], "DN50");
        let back = Change::from_json(&wire).expect("readable");
        assert_eq!(back.object_ref, "V-1001");
        assert_eq!(back.handles, [handle]);
        let read = back.placed.expect("what was placed");
        assert_eq!((read.block, read.type_code, read.position), (what.block, what.type_code, what.position));
        assert_eq!(split_props(&read.props).0.get("公称直径").map(String::as_str), Some("DN50"));

        // As the server sends one it took under another tag.
        let mut renumbered = wire.clone();
        renumbered["object_ref"] = json!("V-1002");
        renumbered["tag"] = json!("V-1002");
        renumbered["reassigned_from"] = json!("V-1001");
        let back = Change::from_json(&renumbered).expect("readable");
        assert_eq!((back.object_ref.as_str(), back.old.as_deref()), ("V-1002", Some("V-1001")));

        let erased = changes_of(handle, Some(&entity), None);
        assert_eq!(erased.len(), 1, "{erased:?}");
        assert_eq!((erased[0].kind.as_str(), erased[0].object_ref.as_str()), (KIND_DELETE_OBJECT, "V-1001"));
        assert!(Change::from_json(&erased[0].to_json()).is_some_and(|change| change.is_structural()));

        // Part of a pipe run: the run's business, not a symbol of its own.
        let fitting = add_author(&mut doc, "V-2001", "{}");
        let mut record = PidAuthor::load(&doc, fitting).expect("author");
        record.owner = "1F".to_owned();
        record.store(&mut doc, fitting);
        let entity = doc.get_entity(fitting).cloned().expect("entity");
        assert!(changes_of(fitting, None, Some(&entity)).is_empty());
        // No tag: nothing a peer could call it.
        let nameless = add_author(&mut doc, "", "{}");
        let entity = doc.get_entity(nameless).cloned().expect("entity");
        assert!(changes_of(nameless, None, Some(&entity)).is_empty());
    }

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
            handles: handle.into_iter().collect(),
            field: field.to_owned(),
            value: value.to_owned(),
            old: None,
            placed: None,
            pipe: None,
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
        assert_eq!(change.handles, [handle]);
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

    /// A created or erased entity the room has no name for is skipped: there
    /// is nothing a peer could call it. (A symbol with a tag is said — see
    /// `a_symbol_that_came_or_went_is_said_as_that`; this used to be the
    /// rule for every created or erased entity.)
    #[test]
    fn a_created_or_erased_entity_with_no_tag_is_skipped() {
        let (doc, handle) = doc_with_author("", "");
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
            handles: vec![Handle::new(0x1A)],
            field: "tag".to_owned(),
            value: "P-1009".to_owned(),
            old: Some("P-1001".to_owned()),
            placed: None,
            pipe: None,
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
            handles: vec![Handle::new(0x1A)],
            field: "tag".to_owned(),
            value: "P-1009".to_owned(),
            old: Some("P-1001".to_owned()),
            placed: None,
            pipe: None,
        };
        assert_eq!(Change::from_json(&sent.to_json()), Some(sent));

        // As `architecture.md` §3.2 writes a retag: no field, no handle.
        let bare = json!({"kind":"set_tag","object_ref":"V-1001","value":"V-1002"});
        let change = Change::from_json(&bare).expect("parsed");
        assert_eq!(change.object_ref, "V-1001");
        assert_eq!(change.field, "tag");
        assert!(change.handles.is_empty());

        // The server backfills absent canonical keys with null.
        let backfilled = json!({
            "kind":"set_tag","object_ref":"V-1001","field":null,"value":"V-1002"
        });
        assert_eq!(
            Change::from_json(&backfilled).map(|change| change.field),
            Some("tag".to_owned())
        );

        // A kind this client has no way to act on. (`delete_object` stood
        // here until this client learned to erase, and `create_pipe_line`
        // until it learned to draw the line — see `super::pipes`.)
        let unknown = json!({"kind":"rename_drawing","tag":"L-1001","object_ref":"L-1001"});
        assert!(Change::from_json(&unknown).is_none());
        // A pipe line that does not say where it runs is still not one it
        // can draw.
        let nowhere = json!({"kind":"create_pipe_line","tag":"L-1001","object_ref":"L-1001"});
        assert!(Change::from_json(&nowhere).is_none());
        // And a symbol that says neither where it stands nor what it is.
        let placeless = json!({"kind":"insert_symbol","tag":"V-1001","type":"VALVE"});
        assert!(Change::from_json(&placeless).is_none());
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
        let read = changes_from_payload(&changes);
        let parsed = read.changes;
        assert_eq!(parsed.len(), 2);
        assert_eq!(parsed[0].object_ref, "P-1001");
        assert_eq!(parsed[1].object_ref, "P-1009");
        assert_eq!(parsed[1].field, "设计压力");
        assert!(read.unread.is_empty());
    }

    /// An op that carries changes this client can act on beside ones it
    /// cannot is not "applied": the ones it cannot read are named, so the
    /// caller can say the drawing is short of them. They used to be passed
    /// over as long as anything else in the op could be read.
    #[test]
    fn changes_this_client_cannot_read_are_named_not_dropped() {
        let changes = vec![
            json!({"kind":"set_tag","object_ref":"P-1001","field":"tag","value":"P-1009"}),
            json!({"kind":"insert_symbol","object_ref":"V-1001","symbol":"gate_valve"}),
            json!({
                "kind":"set_property","object_ref":"P-1009","field":"设计压力","value":"2.5MPa",
                "changes":[{"object_ref":"X-1"}]
            }),
        ];
        let read = changes_from_payload(&changes);
        assert_eq!(read.changes.len(), 2, "{read:?}");
        assert_eq!(read.unread, ["insert_symbol", "?"]);
    }

    fn add_line_piece(doc: &mut Document, tag: &str, role: &str, type_code: &str) -> Handle {
        let handle = doc
            .add_entity(EntityType::Line(document::entities::Line::from_points(
                Vector3::ZERO,
                Vector3::new(10.0, 0.0, 0.0),
            )))
            .expect("entity");
        PidAuthor {
            type_code: type_code.to_owned(),
            tag: tag.to_owned(),
            tag_source: "auto".to_owned(),
            tag_status: "ok".to_owned(),
            topology: role.to_owned(),
            props: "{}".to_owned(),
            ..PidAuthor::default()
        }
        .store(doc, handle);
        handle
    }

    /// A pipe line is one object drawn as one entity per stretch, every one
    /// carrying its tag. A change that names only the tag — from a client
    /// with no drawing, or with another file — means the line, and goes to
    /// all of it; it used to be "several carry it, no answer".
    #[test]
    fn a_line_drawn_as_several_entities_is_one_object_to_a_change_naming_its_tag() {
        let mut doc = Document::new();
        let pieces: Vec<Handle> = (0..3)
            .map(|_| add_line_piece(&mut doc, "L-1001", ROLE_PIPE_LINE, "PIPE_LINE"))
            .collect();
        let pump = add_author(&mut doc, "P-1001", "");

        let applied = apply_changes(
            &mut doc,
            &[change(KIND_SET_PROPERTY, "L-1001", None, "管道等级", "A1A")],
        );
        assert!(applied.unresolved.is_empty(), "{applied:?}");
        assert_eq!(applied.handles, pieces);
        for piece in &pieces {
            let props = PidAuthor::load(&doc, *piece).expect("author").props;
            assert!(props.contains("A1A"), "{props}");
        }

        let applied = apply_changes(
            &mut doc,
            &[change(KIND_SET_TAG, "L-1001", None, "tag", "L-1002")],
        );
        assert!(applied.unresolved.is_empty(), "{applied:?}");
        assert_eq!(applied.retagged, pieces);
        for piece in &pieces {
            assert_eq!(tag_of(&doc, *piece), "L-1002");
        }
        assert_eq!(tag_of(&doc, pump), "P-1001");
    }

    /// A signal line is the same; two separate symbols that happen to carry
    /// one tag are not — a line's pieces share a role, a duplicate does not
    /// become one object by being one.
    #[test]
    fn only_the_pieces_of_one_line_count_as_one_object() {
        let mut doc = Document::new();
        let signal: Vec<Handle> = (0..2)
            .map(|_| add_line_piece(&mut doc, "S-1", ROLE_SIGNAL_LINE, "SIGNAL_LINE"))
            .collect();
        let applied = apply_changes(
            &mut doc,
            &[change(KIND_SET_PROPERTY, "S-1", None, "信号类型", "4-20mA")],
        );
        assert_eq!(applied.handles, signal);

        // A line piece and a symbol under one tag: not one line.
        let mut doc = Document::new();
        add_line_piece(&mut doc, "L-1001", ROLE_PIPE_LINE, "PIPE_LINE");
        add_author(&mut doc, "L-1001", "");
        let applied = apply_changes(
            &mut doc,
            &[change(KIND_SET_PROPERTY, "L-1001", None, "管道等级", "A1A")],
        );
        assert!(applied.is_empty(), "{applied:?}");
        assert_eq!(applied.unresolved, ["L-1001"]);
    }

    /// What was done to several entities of one object is said once, with
    /// the entities it was written to — and a peer holding the same file
    /// writes exactly those.
    #[test]
    fn a_change_made_to_several_entities_of_one_object_is_stated_once() {
        let mut doc = Document::new();
        let pieces: Vec<Handle> = (0..3)
            .map(|_| add_line_piece(&mut doc, "L-1001", ROLE_PIPE_LINE, "PIPE_LINE"))
            .collect();
        let mut images = Vec::new();
        // The first two pieces are retagged; the third is left as it is.
        for piece in &pieces[..2] {
            let before = doc.get_entity_arc(*piece);
            let mut author = PidAuthor::load(&doc, *piece).expect("author");
            author.tag = "L-1002".to_owned();
            author.store(&mut doc, *piece);
            images.push((*piece, before, doc.get_entity_arc(*piece)));
        }

        let stated = collect_changes(&images);
        assert_eq!(stated.len(), 1, "{stated:?}");
        assert_eq!(stated[0].kind, KIND_SET_TAG);
        assert_eq!(stated[0].object_ref, "L-1001");
        assert_eq!(stated[0].handles, pieces[..2]);
        let wire = stated[0].to_json();
        assert_eq!(wire["handle"], format!("{:X}", pieces[0].value()));
        assert_eq!(wire["handles"].as_array().map(Vec::len), Some(2));
        assert_eq!(Change::from_json(&wire).as_ref(), Some(&stated[0]));

        // A peer with the same file, before the change: only those two.
        let mut peer = Document::new();
        let theirs: Vec<Handle> = (0..3)
            .map(|_| add_line_piece(&mut peer, "L-1001", ROLE_PIPE_LINE, "PIPE_LINE"))
            .collect();
        assert_eq!(theirs, pieces, "the same file gives the same handles");
        let applied = apply_changes(&mut peer, &stated);
        assert_eq!(applied.retagged, pieces[..2]);
        assert_eq!(tag_of(&peer, pieces[2]), "L-1001");
    }

    /// Handles are believed all together or not at all. A sender whose
    /// entities only partly carry the tag here is not holding this file, and
    /// one of its handles matching by chance does not pick an object out of
    /// a duplicate.
    #[test]
    fn handles_that_only_partly_match_are_not_believed() {
        let mut doc = Document::new();
        let first = add_author(&mut doc, "P-1001", "");
        let second = add_author(&mut doc, "P-1001", "");
        let other = add_author(&mut doc, "V-1001", "");
        let mut both = change(KIND_SET_PROPERTY, "P-1001", None, "设计压力", "2.5MPa");
        both.handles = vec![first, other];
        let applied = apply_changes(&mut doc, &[both]);
        assert!(applied.is_empty(), "{applied:?}");
        assert_eq!(applied.unresolved, ["P-1001"]);
        assert_eq!(PidAuthor::load(&doc, first).expect("author").props, "");
        assert_eq!(PidAuthor::load(&doc, second).expect("author").props, "");
    }

    /// A rename of an object this drawing never had did not happen here,
    /// whatever else carries the new tag. Only the sender's own entities
    /// carrying it already say "done".
    #[test]
    fn a_retag_this_drawing_never_had_is_reported_though_the_new_tag_is_here() {
        let mut doc = Document::new();
        let other = add_author(&mut doc, "P-1009", "");
        let applied = apply_changes(
            &mut doc,
            &[change(KIND_SET_TAG, "P-1001", None, "tag", "P-1009")],
        );
        assert!(applied.is_empty());
        assert_eq!(
            applied.unresolved,
            ["P-1001"],
            "nothing was renamed here, and that has to be said"
        );

        // The sender's entity is another one, carrying something else.
        let stranger = add_author(&mut doc, "V-1001", "");
        let applied = apply_changes(
            &mut doc,
            &[change(KIND_SET_TAG, "P-1001", Some(stranger), "tag", "P-1009")],
        );
        assert_eq!(applied.unresolved, ["P-1001"]);

        // The sender's entity carries the new tag: the same file, renamed
        // already.
        let applied = apply_changes(
            &mut doc,
            &[change(KIND_SET_TAG, "P-1001", Some(other), "tag", "P-1009")],
        );
        assert!(applied.unresolved.is_empty(), "{applied:?}");
        assert!(applied.is_empty());
    }

    /// An object without a tag — a pipe segment, a break, a connector — has
    /// no name a peer can check, and its handle is not one: a peer that drew
    /// a pipe of its own since it opened the file has another untagged
    /// segment under that very handle. A change naming no tag is applied to
    /// nothing, whatever handle rides along.
    #[test]
    fn a_change_naming_no_tag_is_not_applied_by_its_handle_alone() {
        let mut doc = Document::new();
        let segment = add_author(&mut doc, "", r#"{"公称直径":"DN50"}"#);
        let applied = apply_changes(
            &mut doc,
            &[change(
                KIND_SET_PROPERTY,
                "",
                Some(segment),
                "公称直径",
                "DN80",
            )],
        );
        assert!(applied.is_empty(), "{applied:?}");
        assert_eq!(applied.unresolved, [UNTAGGED]);
        assert_eq!(
            PidAuthor::load(&doc, segment).expect("author").props,
            r#"{"公称直径":"DN50"}"#,
            "this drawing's own segment must be left alone"
        );
    }

    /// So the sender does not state such a change at all. What an untagged
    /// object carries is derived — a segment takes its run's values — and the
    /// peer derives it again from the tagged change that caused it.
    #[test]
    fn a_change_to_an_untagged_object_is_not_stated() {
        let (mut doc, handle) = doc_with_author("", r#"{"公称直径":"DN50"}"#);
        let before = doc.get_entity_arc(handle).expect("before");
        let mut author = PidAuthor::load(&doc, handle).expect("author");
        author.props = r#"{"公称直径":"DN80"}"#.to_owned();
        author.store(&mut doc, handle);
        let after = doc.get_entity_arc(handle).expect("after");

        let changes = changes_of(handle, Some(&before), Some(&after));
        assert!(changes.is_empty(), "{changes:?}");
    }

    /// A retag whose old tag fits several symbols here names none of them.
    /// That the new tag is already carried by something else does not make it
    /// "applied already": the report must still be made.
    #[test]
    fn a_retag_fitting_several_symbols_is_reported_even_when_the_new_tag_exists() {
        let mut doc = Document::new();
        let first = add_author(&mut doc, "P-1001", "");
        let second = add_author(&mut doc, "P-1001", "");
        let other = add_author(&mut doc, "P-1009", "");
        let applied = apply_changes(
            &mut doc,
            &[change(KIND_SET_TAG, "P-1001", None, "tag", "P-1009")],
        );
        assert!(applied.is_empty(), "{applied:?}");
        assert_eq!(applied.unresolved, ["P-1001"]);
        assert_eq!(tag_of(&doc, first), "P-1001");
        assert_eq!(tag_of(&doc, second), "P-1001");
        assert_eq!(tag_of(&doc, other), "P-1009");
    }
}
