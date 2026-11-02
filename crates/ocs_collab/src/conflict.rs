//! Conflict detection and the fixed three-step resolution order
//! (`architecture.md` §3.3, appendix A.4).
//!
//! 1. Changes to different fields merge automatically.
//! 2. The same field changed by two parties is decided by `base_revision`; the
//!    lagging side yields and its original value is reported so the caller can
//!    audit it.
//! 3. When both sides carry the current revision, touch the same field and
//!    disagree on the value, a conflict entry is created for a human.
//!
//! Everything here is a pure function over the object's applied field values.

use serde::{Deserialize, Serialize};

/// A change that has already taken effect on the object.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AppliedChange {
    pub object_ref: String,
    pub field: String,
    pub value: String,
    /// Revision at which this value became current.
    pub revision: i64,
}

/// A change a client wants to apply.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChangeAttempt {
    pub op_id: String,
    pub object_ref: String,
    pub field: String,
    pub value: String,
    pub base_revision: i64,
}

/// What the resolver decided.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConflictOutcome {
    /// The change wins. `overwritten` holds the value it replaced, if any.
    Applied { overwritten: Option<String> },
    /// The change loses to a newer value; `winner` is the surviving value.
    Superseded { winner: String, loser: String },
    /// Neither side can be decided mechanically; a conflict entry is needed.
    Conflict { value_a: String, value_b: String },
}

impl ConflictOutcome {
    pub fn is_applied(&self) -> bool {
        matches!(self, ConflictOutcome::Applied { .. })
    }

    pub fn is_conflict(&self) -> bool {
        matches!(self, ConflictOutcome::Conflict { .. })
    }
}

/// The applied field values of one object, keyed by field name.
#[derive(Debug, Clone, Default)]
pub struct ObjectState {
    fields: Vec<AppliedChange>,
}

impl ObjectState {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn get(&self, field: &str) -> Option<&AppliedChange> {
        self.fields.iter().find(|change| change.field == field)
    }

    pub fn fields(&self) -> &[AppliedChange] {
        &self.fields
    }

    /// Records a value as current. Replacing an existing field keeps one row.
    pub fn set(&mut self, change: AppliedChange) {
        match self
            .fields
            .iter_mut()
            .find(|existing| existing.field == change.field)
        {
            Some(slot) => *slot = change,
            None => self.fields.push(change),
        }
    }

    pub fn len(&self) -> usize {
        self.fields.len()
    }

    pub fn is_empty(&self) -> bool {
        self.fields.is_empty()
    }
}

/// Resolves one attempted change against the object's current state.
///
/// `current_revision` is the server's revision when the change arrives.
pub fn resolve_change(
    state: &ObjectState,
    attempt: &ChangeAttempt,
    current_revision: i64,
) -> ConflictOutcome {
    let existing = match state.get(&attempt.field) {
        // Step 1: an untouched field cannot collide with anything.
        None => {
            return ConflictOutcome::Applied {
                overwritten: None,
            }
        }
        Some(existing) => existing,
    };

    if existing.value == attempt.value {
        // Same value: nothing to decide, nothing to record.
        return ConflictOutcome::Applied { overwritten: None };
    }

    // Step 2: a base revision behind the current one yields.
    if attempt.base_revision < current_revision {
        return ConflictOutcome::Superseded {
            winner: existing.value.clone(),
            loser: attempt.value.clone(),
        };
    }

    // Step 3: both sides at the current revision, same field, different value.
    if attempt.base_revision == current_revision {
        return ConflictOutcome::Conflict {
            value_a: existing.value.clone(),
            value_b: attempt.value.clone(),
        };
    }

    // Ahead of the server: the caller rejects this as STALE_BASE_REVISION
    // before it reaches the resolver, so treat it as a plain write.
    ConflictOutcome::Applied {
        overwritten: Some(existing.value.clone()),
    }
}

/// Applies an accepted change to the state.
pub fn commit(state: &mut ObjectState, attempt: &ChangeAttempt, revision: i64) {
    state.set(AppliedChange {
        object_ref: attempt.object_ref.clone(),
        field: attempt.field.clone(),
        value: attempt.value.clone(),
        revision,
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn state_with(field: &str, value: &str, revision: i64) -> ObjectState {
        let mut state = ObjectState::new();
        state.set(AppliedChange {
            object_ref: "V-1001".into(),
            field: field.into(),
            value: value.into(),
            revision,
        });
        state
    }

    fn attempt(field: &str, value: &str, base_revision: i64) -> ChangeAttempt {
        ChangeAttempt {
            op_id: "op-001".into(),
            object_ref: "V-1001".into(),
            field: field.into(),
            value: value.into(),
            base_revision,
        }
    }

    #[test]
    fn different_fields_merge_without_conflict() {
        let state = state_with("设计压力", "1.6MPa", 1);
        let outcome = resolve_change(&state, &attempt("公称直径", "DN50", 1), 1);
        assert_eq!(
            outcome,
            ConflictOutcome::Applied {
                overwritten: None
            }
        );
    }

    #[test]
    fn same_field_from_a_lagging_client_yields() {
        let state = state_with("设计压力", "1.6MPa", 3);
        let outcome = resolve_change(&state, &attempt("设计压力", "2.5MPa", 1), 3);
        assert_eq!(
            outcome,
            ConflictOutcome::Superseded {
                winner: "1.6MPa".into(),
                loser: "2.5MPa".into(),
            }
        );
    }

    #[test]
    fn lagging_yield_is_not_a_conflict_entry() {
        let state = state_with("设计压力", "1.6MPa", 3);
        let outcome = resolve_change(&state, &attempt("设计压力", "2.5MPa", 2), 3);
        assert!(!outcome.is_conflict(), "step 2 must not create a conflict");
    }

    #[test]
    fn the_superseded_original_value_is_reported_for_audit() {
        let state = state_with("设计压力", "1.6MPa", 3);
        match resolve_change(&state, &attempt("设计压力", "2.5MPa", 3), 3) {
            ConflictOutcome::Conflict { value_a, value_b } => {
                assert_eq!(value_a, "1.6MPa", "the losing value stays visible");
                assert_eq!(value_b, "2.5MPa");
            }
            other => panic!("expected conflict, got {other:?}"),
        }
    }

    #[test]
    fn both_sides_at_the_current_revision_create_a_conflict() {
        let state = state_with("设计压力", "1.6MPa", 4);
        let outcome = resolve_change(&state, &attempt("设计压力", "2.5MPa", 4), 4);
        assert!(outcome.is_conflict());
    }

    #[test]
    fn identical_values_never_conflict() {
        let state = state_with("设计压力", "1.6MPa", 4);
        let outcome = resolve_change(&state, &attempt("设计压力", "1.6MPa", 4), 4);
        assert_eq!(
            outcome,
            ConflictOutcome::Applied {
                overwritten: None
            }
        );
    }

    #[test]
    fn an_empty_object_accepts_every_field() {
        let state = ObjectState::new();
        for field in ["设计压力", "公称直径", "绝热等级"] {
            assert!(resolve_change(&state, &attempt(field, "x", 0), 0).is_applied());
        }
    }

    #[test]
    fn committing_replaces_the_field_value_and_revision() {
        let mut state = state_with("设计压力", "1.6MPa", 1);
        let change = attempt("设计压力", "2.5MPa", 5);
        commit(&mut state, &change, 5);
        let current = state.get("设计压力").expect("field must exist");
        assert_eq!(current.value, "2.5MPa");
        assert_eq!(current.revision, 5);
        assert_eq!(state.len(), 1, "committing must not duplicate the field");
    }

    #[test]
    fn a_lagging_change_does_not_overwrite_the_state() {
        let mut state = state_with("设计压力", "1.6MPa", 3);
        let change = attempt("设计压力", "2.5MPa", 1);
        let outcome = resolve_change(&state, &change, 3);
        if outcome.is_applied() {
            commit(&mut state, &change, 3);
        }
        assert_eq!(
            state.get("设计压力").map(|c| c.value.as_str()),
            Some("1.6MPa"),
            "the winner must survive a rejected change"
        );
    }

    #[test]
    fn three_field_changes_merge_and_each_keeps_its_own_value() {
        let mut state = ObjectState::new();
        for (index, (field, value)) in [("设计压力", "1.6MPa"), ("公称直径", "DN50"), ("流体介质", "水")]
            .into_iter()
            .enumerate()
        {
            let change = attempt(field, value, index as i64);
            let outcome = resolve_change(&state, &change, index as i64);
            assert!(outcome.is_applied(), "{field} must merge: {outcome:?}");
            commit(&mut state, &change, index as i64);
        }
        assert_eq!(state.len(), 3);
        assert_eq!(
            state.get("流体介质").map(|c| c.value.as_str()),
            Some("水")
        );
    }
}
