//! Objects whose properties are too long for the entity's own record: what
//! a step that brings one into the drawing says of it, and what a peer has
//! once it has followed (plan T2.4 ③; task `CC-1` of
//! `docs/plans/2026-11-16-pid-dev-spec/todo.md`).
//!
//! `PidAuthor::store` keeps a `props` text that would take the record past
//! `XDATA_SOFT_LIMIT` in an XRecord of the drawing, and leaves the record a
//! key to it. A step's images are of entities, so the image of such an
//! object only points at its properties; the drawing the step was made in
//! holds them.
//!
//! Symbols first. Three tests are of the reading alone, the fourth of a
//! peer's writing; the next two are two editors in one process with the
//! server's own judgment between them (`ocs_collab::conflict::DrawingModel`,
//! the code the server runs), as in `collab_pipe_ops`: what one places,
//! erases, takes back and makes again goes through the room to the other,
//! and the two drawings — and the room — are then asked for the symbol.
//!
//! Then pipe lines, runs and what is fitted on them, which
//! `super::pipes::statements` says off the drawing: the reading of a line
//! alone, and each of the three erased and put back between two editors.

use crate::app::collab::{broadcast, CollabEvent};
use crate::app::OpenCADStudio;
use crate::pid::persist::PidAuthor;
use crate::pid::propagate::{join_props, split_props};
use crate::pid::topology::{
    handle_text, ROLE_PIPE_LINE, ROLE_PIPE_RUN, ROLE_PIPE_SEGMENT, TYPE_PIPE_LINE, TYPE_VALVE,
};
use document::entities::{Insert, Line};
use document::types::Vector3;
use document::{Document, EntityType, Handle};
use ocs_collab::conflict::{Change as Judged, DrawingModel, Verdict};
use serde_json::{json, Value};
use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

static SERIAL: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

const BLOCK: &str = "PID_SYM_equipment_vessel";
const TYPE: &str = "EQUIPMENT";
const TAG: &str = "E-9001";
const AT: [f64; 2] = [100.0, 50.0];

/// Sixty properties, some three and a half kilobytes of them: well past
/// what an entity's own record keeps.
fn many_props() -> BTreeMap<String, String> {
    (1..=60)
        .map(|n| {
            (
                format!("字段{n:02}"),
                format!("值{n:02}-{}", "x".repeat(40)),
            )
        })
        .collect()
}

/// The author record of a symbol carrying `props` under [`TAG`].
fn record_with(props: &BTreeMap<String, String>, ports: String) -> PidAuthor {
    PidAuthor {
        type_code: TYPE.to_owned(),
        tag: TAG.to_owned(),
        tag_source: "manual".to_owned(),
        tag_status: "ok".to_owned(),
        props: join_props(props, &BTreeSet::new()),
        ports,
        ..PidAuthor::default()
    }
}

/// A JSON object of texts, as the map it is.
fn texts(object: &Value) -> BTreeMap<String, String> {
    object
        .as_object()
        .into_iter()
        .flatten()
        .filter_map(|(key, value)| Some((key.clone(), value.as_str()?.to_owned())))
        .collect()
}

/// A drawing holding the symbol with `props`, the symbol's handle, and the
/// image a step has of it.
fn drawing_with(props: &BTreeMap<String, String>) -> (Document, Handle, Arc<EntityType>) {
    let mut doc = Document::new();
    let handle = doc
        .add_entity(EntityType::Insert(Insert::new(
            BLOCK,
            Vector3::new(AT[0], AT[1], 0.0),
        )))
        .expect("entity");
    record_with(props, String::new()).store(&mut doc, handle);
    let image = doc.get_entity_arc(handle).expect("entity");
    assert!(
        !PidAuthor::props_are_inline(&image),
        "the fixture's properties must be too long for the entity's record"
    );
    (doc, handle, image)
}

/// `said` is one placement of the symbol under [`TAG`] carrying `props` —
/// as the change is kept, and as it goes on the wire.
#[track_caller]
fn assert_placed_with(said: &[broadcast::Change], props: &BTreeMap<String, String>) {
    assert_eq!(said.len(), 1, "{said:?}");
    assert_eq!(
        (said[0].kind.as_str(), said[0].object_ref.as_str()),
        (broadcast::KIND_INSERT_SYMBOL, TAG)
    );
    let placed = said[0].placed.as_ref().expect("what was placed");
    assert_eq!(
        &split_props(&placed.props).0,
        props,
        "the properties the change carries"
    );
    assert_eq!(
        &texts(&said[0].to_json()["props"]),
        props,
        "the properties on the wire"
    );
}

/// A step that placed such a symbol says it with every property it has:
/// they are read off the drawing the step left, which holds them.
#[test]
fn collab_large_props_a_symbol_placed_with_them_is_said_with_every_one() {
    let props = many_props();
    let (doc, handle, image) = drawing_with(&props);
    let said = broadcast::collect_step_changes(&doc, &[(handle, None, Some(image))]);
    assert_placed_with(&said, &props);
}

/// An erase taken back puts the symbol in the drawing again, which to the
/// room is a placement: said the same way, off the drawing without the
/// erase.
#[test]
fn collab_large_props_an_erase_taken_back_says_the_symbol_with_every_one() {
    let props = many_props();
    let (doc, handle, image) = drawing_with(&props);
    let said = broadcast::collect_undone_step_changes(&doc, &[(handle, Some(image), None)]);
    assert_placed_with(&said, &props);
}

/// Where the drawing does not hold what the image points at, the symbol is
/// still said — as starting with no properties, which is all that is known
/// of them.
#[test]
fn collab_large_props_a_symbol_whose_properties_the_drawing_lacks_is_still_said() {
    let (_, handle, image) = drawing_with(&many_props());
    // A drawing that holds the entity, record and all, but not the XRecord
    // the record points at.
    let mut bare = Document::new();
    let copied = bare.add_entity((*image).clone()).expect("entity");
    let copy = bare.get_entity_arc(copied).expect("entity");
    assert!(!PidAuthor::props_are_inline(&copy));
    // And one that does not hold the entity at all.
    let empty = Document::new();
    for said in [
        broadcast::collect_step_changes(&bare, &[(copied, None, Some(copy))]),
        broadcast::collect_step_changes(&empty, &[(handle, None, Some(image))]),
    ] {
        assert_placed_with(&said, &BTreeMap::new());
    }
}

/// An editor on a new drawing, in a session.
fn editor(session: &str) -> OpenCADStudio {
    let mut app = OpenCADStudio::new_for_test();
    let _ = app.automation_op(r#"{"op":"new"}"#);
    app.collab.connected = true;
    app.collab.session_id = Some(session.to_owned());
    app.collab.user_id = session.to_owned();
    app
}

fn request(app: &mut OpenCADStudio, mut req: Value) -> Value {
    if let Some(object) = req.as_object_mut() {
        object.insert("document_id".into(), json!(app.tabs[app.active_tab].id));
        let serial = SERIAL.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        object.insert("request_id".into(), json!(format!("collab-large-{serial}")));
    }
    let (response, task) = app.control_request(req);
    app.drive_headless_task(task)
        .expect("a task that needs no window");
    response
}

/// The symbol comes into the drawing in front with `props` in one step,
/// the way a paste of one does: the entity, its record and the XRecord the
/// record points at are all the step's. Its handle.
fn place(app: &mut OpenCADStudio, props: &BTreeMap<String, String>) -> Handle {
    let i = app.active_tab;
    if !app.tabs[i].scene.document.block_records.contains(BLOCK) {
        app.pid_symbols.instantiate(&mut app.tabs[i].scene);
    }
    let ports = app.pid_symbols.symbol_ports(BLOCK).join(",");
    app.push_undo_snapshot(i, "PASTECLIP");
    let handle = app.tabs[i].scene.add_entity(EntityType::Insert(Insert::new(
        BLOCK,
        Vector3::new(AT[0], AT[1], 0.0),
    )));
    assert!(!handle.is_null(), "the symbol was not added");
    let document = &mut app.tabs[i].scene.document;
    record_with(props, ports).store(document, handle);
    crate::app::commands::pidpalette::sync_pid_tag_attribute(document, handle);
    app.tabs[i].dirty = true;
    app.finish_all_pending_history();
    handle
}

fn erase(app: &mut OpenCADStudio, handles: &[Handle]) {
    let list: Vec<String> = handles.iter().map(|handle| handle_text(*handle)).collect();
    let selected = request(app, json!({"op":"select","handles":list}));
    assert_eq!(selected["ok"], true, "{selected}");
    let erased = request(app, json!({"op":"run","cmd":"ERASE"}));
    assert_eq!(erased["ok"], true, "{erased}");
    app.finish_all_pending_history();
    let document = &app.tabs[app.active_tab].scene.document;
    for handle in handles {
        assert!(
            document.get_entity(*handle).is_none(),
            "{handle:?} was not erased"
        );
    }
}

fn undo(app: &mut OpenCADStudio) {
    let _ = app.automation_op(r#"{"op":"undo"}"#);
}

fn redo(app: &mut OpenCADStudio) {
    let _ = app.automation_op(r#"{"op":"redo"}"#);
}

/// The room: the server's picture of the drawing and its revision.
#[derive(Default)]
struct Room {
    model: DrawingModel,
    revision: i64,
}

impl Room {
    /// What the server makes of one op: the changes it tells the room, every
    /// one of them having taken effect, at the revision it moved to.
    fn take(&mut self, session: &str, base: i64, payload: &Value) -> (i64, Vec<Value>) {
        let changes = Judged::list(payload);
        assert!(
            !changes.is_empty() && changes.iter().all(Judged::is_understood),
            "the server has no rule for {payload}"
        );
        let revision = self.revision + 1;
        let mut said = Vec::new();
        for change in changes {
            let change = self.model.completed(&change);
            match self.model.judge(&change, base, session) {
                Verdict::Apply { object_ref, .. } => {
                    said.push((change, object_ref));
                }
                other => panic!("the server refused {}: {other:?}", change.raw),
            }
        }
        let mut broadcast = Vec::new();
        for (change, object_ref) in said {
            self.model
                .commit(&change, &object_ref, revision, session, session);
            broadcast.push(change.broadcast_form(&object_ref));
        }
        self.revision = revision;
        (revision, broadcast)
    }
}

/// Everything `from` has said since `seen` goes through the room: back to
/// `from` as its own op, and to `to` as a peer's.
fn relay(room: &mut Room, from: &mut OpenCADStudio, to: &mut OpenCADStudio, seen: &mut usize) {
    let session = from.collab.session_id.clone().expect("a session");
    let payloads: Vec<Value> = from.collab.sent[*seen..].to_vec();
    *seen = from.collab.sent.len();
    let own: Vec<String> = from.collab.own_ops.iter().cloned().collect();
    let first = own.len() - payloads.len();
    for (index, payload) in payloads.iter().enumerate() {
        let base = from.collab.base_revision.min(room.revision);
        let (revision, changes) = room.take(&session, base, payload);
        from.collab.apply(CollabEvent::OpApplied {
            op_id: own[first + index].clone(),
            revision,
            changes: changes.clone(),
        });
        from.apply_remote_changes();
        to.collab.apply(CollabEvent::OpApplied {
            op_id: format!("op-of-{session}-{revision}"),
            revision,
            changes,
        });
        to.apply_remote_changes();
    }
    assert_eq!(to.collab.diverged, None, "the peer could not follow");
    assert_eq!(to.collab.last_apply_error, None);
    assert_eq!(from.collab.diverged, None);
}

/// What the drawing in front holds of one symbol.
#[derive(Debug, PartialEq)]
struct Held {
    /// Its block and where it stands.
    shape: String,
    /// Its record, the properties as `join_props` writes them.
    record: PidAuthor,
    /// Whether the properties are in the entity's own record.
    inline: bool,
}

/// Every symbol of the drawing in front that carries [`TAG`].
fn held(app: &OpenCADStudio) -> Vec<Held> {
    let document = &app.tabs[app.active_tab].scene.document;
    document
        .entities()
        .filter_map(|entity| {
            let EntityType::Insert(insert) = entity else {
                return None;
            };
            let mut record = PidAuthor::load(document, insert.common.handle)?;
            if record.tag != TAG {
                return None;
            }
            let (values, marks) = split_props(&record.props);
            record.props = join_props(&values, &marks);
            Some(Held {
                shape: format!(
                    "{} {:.4},{:.4}",
                    insert.block_name, insert.insert_point.x, insert.insert_point.y
                ),
                record,
                inline: PidAuthor::props_are_inline(entity),
            })
        })
        .collect()
}

/// Both drawings hold the symbol once and the same in everything, with
/// every one of `props`, kept outside the entity's record on both sides.
#[track_caller]
fn assert_whole(
    a: &OpenCADStudio,
    b: &OpenCADStudio,
    props: &BTreeMap<String, String>,
    when: &str,
) {
    let (here, there) = (held(a), held(b));
    assert_eq!(here.len(), 1, "the sender's drawing {when}: {here:?}");
    let theirs = there.first().map(|held| held.record.props.clone());
    assert_eq!(
        &split_props(&theirs.unwrap_or_default()).0,
        props,
        "the peer's properties {when}"
    );
    assert_eq!(there, here, "the peer's drawing {when}");
    assert!(
        !there[0].inline,
        "the peer keeps them in the entity's own record {when}"
    );
}

/// A peer's placement that carries such properties is written with every
/// one of them, and kept the way a local one is: outside the entity's
/// record.
#[test]
fn collab_large_props_a_peer_s_symbol_is_written_with_every_one_and_kept_the_same_way() {
    let props = many_props();
    let mut b = editor("s-b");
    let placement = broadcast::Change {
        kind: broadcast::KIND_INSERT_SYMBOL.to_owned(),
        object_ref: TAG.to_owned(),
        handles: Vec::new(),
        field: String::new(),
        value: String::new(),
        old: None,
        placed: Some(broadcast::Placed {
            block: BLOCK.to_owned(),
            type_code: TYPE.to_owned(),
            position: [AT[0], AT[1], 0.0],
            rotation: 0.0,
            scale: 1.0,
            ports: String::new(),
            props: join_props(&props, &BTreeSet::new()),
            tag_source: "manual".to_owned(),
        }),
        pipe: None,
        tag_source: None,
        overrides: None,
    };
    b.collab.apply(CollabEvent::OpApplied {
        op_id: "op-of-s-a-1".to_owned(),
        revision: 1,
        changes: vec![placement.to_json()],
    });
    b.apply_remote_changes();
    assert_eq!(b.collab.last_apply_error, None);
    assert_eq!(b.collab.diverged, None);

    let there = held(&b);
    assert_eq!(there.len(), 1, "{there:?}");
    assert_eq!(split_props(&there[0].record.props).0, props);
    assert!(
        !there[0].inline,
        "properties this long are kept outside the entity's record"
    );
}

/// One places such a symbol; the step says every property, the other's
/// drawing has the same symbol with the same record, and the room knows
/// each field. None of it is a step of the second person's, and none of it
/// goes back to the room.
#[test]
fn collab_large_props_a_symbol_placed_with_them_reaches_the_peer_and_the_room_whole() {
    let (mut a, mut b) = (editor("s-a"), editor("s-b"));
    let (mut room, mut seen) = (Room::default(), 0);
    let steps = b.tabs[b.active_tab].history.undo_stack.len();
    let props = many_props();

    place(&mut a, &props);
    assert_eq!(a.collab.sent.len(), 1, "the placement was not said");
    let said = a.collab.sent[0].clone();
    assert_eq!(said["kind"], "insert_symbol", "{said}");
    assert_eq!(said["tag"], TAG, "{said}");
    assert_eq!(texts(&said["props"]), props, "what the step said of them");

    relay(&mut room, &mut a, &mut b, &mut seen);
    assert_whole(&a, &b, &props, "after the placement");
    let known = room.model.object(TAG).expect("the room knows the symbol");
    for (field, value) in &props {
        assert_eq!(
            known.fields.get(field).map(|state| state.value.as_str()),
            Some(value.as_str()),
            "the room's {field}"
        );
    }
    assert_eq!(b.tabs[b.active_tab].history.undo_stack.len(), steps);
    assert!(b.collab.sent.is_empty(), "{:?}", b.collab.sent);
}

/// Erased and the erase taken back, then the placement itself taken back
/// and made again: each time the symbol is back at the peer it is whole.
#[test]
fn collab_large_props_taken_out_and_put_back_the_symbol_is_whole_at_the_peer_each_time() {
    let (mut a, mut b) = (editor("s-a"), editor("s-b"));
    let (mut room, mut seen) = (Room::default(), 0);
    let props = many_props();
    let handle = place(&mut a, &props);
    relay(&mut room, &mut a, &mut b, &mut seen);

    erase(&mut a, &[handle]);
    relay(&mut room, &mut a, &mut b, &mut seen);
    assert!(held(&b).is_empty(), "the peer still has it: {:?}", held(&b));
    undo(&mut a);
    relay(&mut room, &mut a, &mut b, &mut seen);
    assert_whole(&a, &b, &props, "after the erase was taken back");

    undo(&mut a);
    relay(&mut room, &mut a, &mut b, &mut seen);
    assert!(held(&a).is_empty(), "the placement was not taken back");
    assert!(held(&b).is_empty(), "the peer still has it: {:?}", held(&b));
    redo(&mut a);
    relay(&mut room, &mut a, &mut b, &mut seen);
    assert_whole(&a, &b, &props, "after the placement was made again");
}

// ── pipe lines, runs and what is fitted on them ─────────────────────────

const LINE_TAG: &str = "L-9001";
/// Where the line the editors draw runs, and where the valve stands on it.
const LINE: &[[f64; 2]] = &[[0.0, 0.0], [200.0, 0.0]];
const VALVE_AT: &[[f64; 2]] = &[[100.0, 0.0]];

/// A drawing holding one stretch of a pipe line with `props`, the stretch's
/// handle, and the image a step has of it.
fn drawing_with_a_line(props: &BTreeMap<String, String>) -> (Document, Handle, Arc<EntityType>) {
    let mut doc = Document::new();
    let handle = doc
        .add_entity(EntityType::Line(Line::from_points(
            Vector3::new(0.0, 0.0, 0.0),
            Vector3::new(200.0, 0.0, 0.0),
        )))
        .expect("entity");
    PidAuthor {
        type_code: TYPE_PIPE_LINE.to_owned(),
        tag: LINE_TAG.to_owned(),
        tag_source: "manual".to_owned(),
        tag_status: "ok".to_owned(),
        topology: ROLE_PIPE_LINE.to_owned(),
        props: join_props(props, &BTreeSet::new()),
        ..PidAuthor::default()
    }
    .store(&mut doc, handle);
    let image = doc.get_entity_arc(handle).expect("entity");
    assert!(
        !PidAuthor::props_are_inline(&image),
        "the fixture's properties must be too long for the entity's record"
    );
    (doc, handle, image)
}

/// `said` is the line under [`LINE_TAG`] coming into being with `props`.
#[track_caller]
fn assert_line_said_with(said: &[broadcast::Change], props: &BTreeMap<String, String>) {
    assert_eq!(said.len(), 1, "{said:?}");
    let wire = said[0].to_json();
    assert_eq!(
        (&wire["kind"], &wire["tag"]),
        (&json!("create_pipe_line"), &json!(LINE_TAG)),
        "{wire}"
    );
    assert_eq!(&texts(&wire["props"]), props, "the properties on the wire");
}

/// A step that drew a stretch of such a line says the line with every
/// property it has.
#[test]
fn collab_large_props_a_line_drawn_with_them_is_said_with_every_one() {
    let props = many_props();
    let (doc, handle, image) = drawing_with_a_line(&props);
    let said = broadcast::collect_step_changes(&doc, &[(handle, None, Some(image))]);
    assert_line_said_with(&said, &props);
}

/// Where the drawing does not hold what the image points at, the line is
/// still said, as starting with no properties.
#[test]
fn collab_large_props_a_line_whose_properties_the_drawing_lacks_is_still_said() {
    let (_, handle, image) = drawing_with_a_line(&many_props());
    // A drawing that holds the stretch, record and all, but not the XRecord
    // the record points at.
    let mut bare = Document::new();
    let copied = bare.add_entity((*image).clone()).expect("entity");
    let copy = bare.get_entity_arc(copied).expect("entity");
    assert!(!PidAuthor::props_are_inline(&copy));
    // And one that does not hold the stretch at all.
    let empty = Document::new();
    for said in [
        broadcast::collect_step_changes(&bare, &[(copied, None, Some(copy))]),
        broadcast::collect_step_changes(&empty, &[(handle, None, Some(image))]),
    ] {
        assert_line_said_with(&said, &BTreeMap::new());
    }
}

fn is_line(author: &PidAuthor) -> bool {
    author.topology == ROLE_PIPE_LINE
}

fn is_run(author: &PidAuthor) -> bool {
    author.topology == ROLE_PIPE_RUN
}

fn is_valve(author: &PidAuthor) -> bool {
    author.type_code == TYPE_VALVE
}

/// Start `cmd`, give it `points`, press Enter; the step is closed.
fn draw(app: &mut OpenCADStudio, cmd: &str, points: &[[f64; 2]]) {
    let started = request(app, json!({"op":"run","cmd":cmd}));
    assert_eq!(started["ok"], true, "{cmd}: {started}");
    for point in points {
        let fed = request(
            app,
            json!({"op":"input","kind":"point","point":[point[0], point[1], 0.0],"space":"wcs"}),
        );
        assert_eq!(fed["ok"], true, "{cmd}: {fed}");
    }
    let done = request(app, json!({"op":"input","kind":"enter"}));
    app.finish_all_pending_history();
    assert_eq!(done["ok"], true, "{cmd}: {done}");
}

/// The entities of the drawing in front whose record `keep`s.
fn handles_where(app: &OpenCADStudio, keep: fn(&PidAuthor) -> bool) -> Vec<Handle> {
    let document = &app.tabs[app.active_tab].scene.document;
    document
        .entities()
        .map(|entity| entity.common().handle)
        .filter(|handle| PidAuthor::load(document, *handle).is_some_and(|author| keep(&author)))
        .collect()
}

/// The properties of the one entity of the drawing in front whose record
/// `keep`s, and whether they are in the entity's own record.
#[track_caller]
fn props_where(
    app: &OpenCADStudio,
    keep: fn(&PidAuthor) -> bool,
) -> (BTreeMap<String, String>, bool) {
    let handles = handles_where(app, keep);
    assert_eq!(handles.len(), 1, "one such entity: {handles:?}");
    let document = &app.tabs[app.active_tab].scene.document;
    let record = PidAuthor::load(document, handles[0]).expect("a record");
    let entity = document.get_entity(handles[0]).expect("entity");
    (
        split_props(&record.props).0,
        PidAuthor::props_are_inline(entity),
    )
}

/// Every change `app` has said since the first `seen` of its ops, one by
/// one.
fn said_since(app: &OpenCADStudio, seen: usize) -> Vec<Value> {
    app.collab.sent[seen..]
        .iter()
        .flat_map(Judged::list)
        .map(|change| change.raw)
        .collect()
}

/// The one change of `kind` among `said`.
#[track_caller]
fn the_one<'a>(said: &'a [Value], kind: &str) -> &'a Value {
    let of_kind: Vec<&Value> = said
        .iter()
        .filter(|change| change["kind"] == kind)
        .collect();
    assert_eq!(of_kind.len(), 1, "one {kind} among {said:?}");
    of_kind[0]
}

/// What [`erased_and_put_back`] leaves.
struct PutBack {
    /// The peer.
    b: OpenCADStudio,
    room: Room,
    /// What taking the erase back said, change by change.
    said: Vec<Value>,
    /// The properties the thing has in the first drawing.
    props: BTreeMap<String, String>,
}

/// One draws what `cmds` draw and the peer follows. The thing whose record
/// `is` it then has [`many_props`] on top of its own in the first drawing —
/// by no step: the drawing has them, as one read from a file does — and is
/// erased there with whatever `goes` with it, which the peer follows; then
/// the erase is taken back, and the peer follows that.
fn erased_and_put_back(
    cmds: &[(&str, &[[f64; 2]])],
    is: fn(&PidAuthor) -> bool,
    goes: fn(&PidAuthor) -> bool,
) -> PutBack {
    let (mut a, mut b) = (editor("s-a"), editor("s-b"));
    let (mut room, mut seen) = (Room::default(), 0);
    for (cmd, points) in cmds {
        draw(&mut a, cmd, points);
    }
    relay(&mut room, &mut a, &mut b, &mut seen);

    let grown = handles_where(&a, is);
    assert_eq!(grown.len(), 1, "one such thing: {grown:?}");
    let props = {
        let document = &mut a.tabs[a.active_tab].scene.document;
        let mut record = PidAuthor::load(document, grown[0]).expect("a record");
        let (mut values, marks) = split_props(&record.props);
        values.extend(many_props());
        record.props = join_props(&values, &marks);
        record.store(document, grown[0]);
        values
    };
    assert_eq!(props_where(&a, is), (props.clone(), false), "the fixture");

    let gone = handles_where(&a, goes);
    erase(&mut a, &gone);
    relay(&mut room, &mut a, &mut b, &mut seen);
    assert!(handles_where(&b, is).is_empty(), "the peer still has it");

    undo(&mut a);
    assert_eq!(
        props_where(&a, is),
        (props.clone(), false),
        "the first drawing has it back as it was"
    );
    let said = said_since(&a, seen);
    relay(&mut room, &mut a, &mut b, &mut seen);
    PutBack {
        b,
        room,
        said,
        props,
    }
}

/// A line erased and the erase taken back: the line is said with every
/// property, the peer's line has them — outside the entity's record, as
/// here — and the room knows each field.
#[test]
fn collab_large_props_a_line_put_back_is_whole_at_the_peer_and_in_the_room() {
    let back = erased_and_put_back(&[("PIDPIPE", LINE)], is_line, is_line);
    let line = the_one(&back.said, "create_pipe_line");
    assert_eq!(texts(&line["props"]), back.props, "what was said of them");
    assert_eq!(props_where(&back.b, is_line), (back.props.clone(), false));
    let tag = line["tag"].as_str().expect("the line's tag");
    let known = back.room.model.object(tag).expect("the room knows it");
    for (field, value) in &back.props {
        assert_eq!(
            known.fields.get(field).map(|state| state.value.as_str()),
            Some(value.as_str()),
            "the room's {field}"
        );
    }
}

/// A run erased with its segments and the erase taken back: the account of
/// the run is said with every property, and the peer's run has them.
#[test]
fn collab_large_props_a_run_put_back_is_whole_at_the_peer() {
    let back = erased_and_put_back(
        &[("PIDPIPE", LINE), ("PIDPIPE run", LINE)],
        is_run,
        |author| author.topology == ROLE_PIPE_RUN || author.topology == ROLE_PIPE_SEGMENT,
    );
    let run = the_one(&back.said, "add_pipe_run");
    assert_eq!(
        texts(&run["run"]["props"]),
        back.props,
        "what was said of them"
    );
    assert_eq!(props_where(&back.b, is_run), (back.props.clone(), false));
}

/// A valve erased from its run and the erase taken back: the run's account
/// says the valve with every property, and the peer's valve has them.
#[test]
fn collab_large_props_a_valve_put_back_on_its_run_is_whole_at_the_peer() {
    let back = erased_and_put_back(
        &[
            ("PIDPIPE", LINE),
            ("PIDPIPE run", LINE),
            ("PIDFITTING gate_valve", VALVE_AT),
        ],
        is_valve,
        is_valve,
    );
    let fitted = the_one(&back.said, "insert_fitting");
    let valves = fitted["run"]["fittings"].as_array().expect("the fittings");
    assert_eq!(valves.len(), 1, "{fitted}");
    assert_eq!(
        texts(&valves[0]["props"]),
        back.props,
        "what was said of them"
    );
    assert_eq!(props_where(&back.b, is_valve), (back.props.clone(), false));
}
