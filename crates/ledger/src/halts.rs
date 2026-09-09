//! The request to stop a run, which the engine asks the store for.
//!
//! `RecordStore::halt_requested` defaults to `false`, and the real store never
//! answered for itself: `StopReason::ByHand` was a word the ledger could keep
//! and no gesture could produce.

use crate::{Ledger, LedgerError, StoreRecord};
use serde_json::{json, Value};

/// One entry per run: the request to stop it before the next front opens.
pub const HALT_REQUESTS: &str = "run-halts";

/// The field of a halt request that says why it was asked for.
pub const WHY_FIELD: &str = "why";

impl Ledger {
    /// Asks a run to stop, saying who asked and why.
    ///
    /// **THE REASON IS NOT OPTIONAL HERE.** A run found stopped a week later
    /// with `by_hand` and nothing beside it is indistinguishable from one
    /// nobody ever came back to.
    pub fn request_halt(
        &self,
        run_id: &str,
        why: &str,
        who: &str,
        now: i64,
    ) -> Result<(), LedgerError> {
        if why.trim().is_empty() {
            return Err(LedgerError::InvalidRecord(
                "a halt with no reason cannot be read back".into(),
            ));
        }
        self.put_record(&StoreRecord {
            collection: HALT_REQUESTS.to_owned(),
            key: run_id.to_owned(),
            value: json!({ WHY_FIELD: why }),
            written_by: who.to_owned(),
            written_at: now,
        })
    }

    /// The request to stop that run, if somebody wrote one.
    pub fn halt_request(&self, run_id: &str) -> Result<Option<StoreRecord>, LedgerError> {
        self.read_record(HALT_REQUESTS, run_id)
    }

    /// Why the run was asked to stop, and by whom.
    pub fn why_the_halt_was_asked(
        &self,
        run_id: &str,
    ) -> Result<Option<(String, String)>, LedgerError> {
        Ok(self.halt_request(run_id)?.map(|record| {
            let why = record
                .value
                .get(WHY_FIELD)
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_owned();
            (record.written_by, why)
        }))
    }
}
