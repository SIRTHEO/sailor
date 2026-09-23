//! A flow held by a person: it no longer starts by itself, and says why.
//!
//! A hold is one entry in the store, not a copy of the flow: a copy outlives
//! every later correction of the flow it replaces, in silence. The entry names
//! who asked and why, and taking it off writes a second one, so the story of
//! a hold is read back rather than remembered.

use crate::{Ledger, LedgerError, StoreRecord};
use serde_json::{json, Value};
use std::collections::BTreeMap;

/// One entry per flow, keyed by its id.
pub const FLOW_HOLDS: &str = "flow-holds";

const HELD_FIELD: &str = "held";
const WHY_FIELD: &str = "why";

/// A hold standing on a flow.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Hold {
    pub flow: String,
    pub why: String,
    pub by: String,
    pub since: i64,
}

impl Ledger {
    /// Holds a flow: nothing starts it by itself until the hold is taken off.
    pub fn hold_flow(&self, flow: &str, why: &str, who: &str, now: i64) -> Result<(), LedgerError> {
        self.write_hold(flow, true, why, who, now)
    }

    /// Takes the hold off a flow, keeping the reason it was taken off.
    pub fn release_flow(
        &self,
        flow: &str,
        why: &str,
        who: &str,
        now: i64,
    ) -> Result<(), LedgerError> {
        self.write_hold(flow, false, why, who, now)
    }

    /// The hold standing on this flow, if one does.
    pub fn flow_hold(&self, flow: &str) -> Result<Option<Hold>, LedgerError> {
        Ok(self.read_record(FLOW_HOLDS, flow)?.and_then(standing))
    }

    /// Every hold standing, by flow.
    pub fn flow_holds(&self) -> Result<BTreeMap<String, Hold>, LedgerError> {
        Ok(self
            .records_in(FLOW_HOLDS)?
            .into_iter()
            .filter_map(standing)
            .map(|hold| (hold.flow.clone(), hold))
            .collect())
    }

    fn write_hold(
        &self,
        flow: &str,
        held: bool,
        why: &str,
        who: &str,
        now: i64,
    ) -> Result<(), LedgerError> {
        if why.trim().is_empty() {
            return Err(LedgerError::InvalidRecord(
                "a hold with no reason cannot be read back".into(),
            ));
        }
        self.put_record(&StoreRecord {
            collection: FLOW_HOLDS.to_owned(),
            key: flow.to_owned(),
            value: json!({ HELD_FIELD: held, WHY_FIELD: why }),
            written_by: who.to_owned(),
            written_at: now,
        })
    }
}

fn standing(record: StoreRecord) -> Option<Hold> {
    if record.value.get(HELD_FIELD).and_then(Value::as_bool) != Some(true) {
        return None;
    }
    Some(Hold {
        why: record
            .value
            .get(WHY_FIELD)
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_owned(),
        flow: record.key,
        by: record.written_by,
        since: record.written_at,
    })
}
