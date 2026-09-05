//! The two nodes with which a flow reads and writes the fault register.
//!
//! Without these the register is a filing cabinet in a room only a person can
//! enter: a flow that finds a defect has nowhere to write it, and the step that
//! picks the next piece of work has to read prose someone keeps by hand. That
//! is the loop these close.

use faults::{Draft, Faults};
use flow::{Action, ActionError, ActionOutcome, SharedState, StepSpecies};
use serde::Deserialize;
use serde_json::{json, Value};
use std::path::PathBuf;

/// Reads the register.
pub const FAULT_LIST_ACTION: &str = "fault_list";
/// Records one fault and lets the store give it its number.
pub const FAULT_RECORD_ACTION: &str = "fault_record";

/// Said when this machine has no register. The node stays registered and
/// declares it, the same way `history_ask` declares an absent ledger: a node
/// that vanished when its store did would make a flow fail somewhere else.
const REGISTER_ABSENT: &str = "absent";
const REGISTER_PRESENT: &str = "present";

pub fn register_faults(registry: &mut flow::ActionRegistry, store: Option<PathBuf>) {
    registry.register(FAULT_LIST_ACTION, FaultListAction::new(store.clone()));
    registry.register(FAULT_RECORD_ACTION, FaultRecordAction::new(store));
}

fn unreadable(error: impl std::fmt::Display) -> ActionError {
    ActionError::new(
        "store_unreadable",
        format!("the fault register cannot be read: {error}"),
    )
}

fn wrong_input(said: impl Into<String>) -> ActionError {
    ActionError::new("invalid_input", said)
}

fn open(store: &Option<PathBuf>) -> Result<Option<Faults>, ActionError> {
    let Some(path) = store else {
        return Ok(None);
    };
    Faults::open(path).map(Some).map_err(unreadable)
}

// ── reading ──────────────────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
struct ListInput {
    /// Only what is still open, which is the question a flow actually asks.
    /// Everything is the exception, so it is the one that must be spelled out.
    #[serde(default)]
    everything: bool,
    /// A ceiling on how many come back. Absent means all of them: a silent
    /// default would hide the rest from whoever reads the answer.
    #[serde(default)]
    at_most: Option<usize>,
}

pub struct FaultListAction {
    store: Option<PathBuf>,
}

impl FaultListAction {
    pub fn new(store: Option<PathBuf>) -> Self {
        Self { store }
    }
}

impl Action for FaultListAction {
    fn execute(&self, input: &Value, _shared: &SharedState) -> Result<ActionOutcome, ActionError> {
        let asked: ListInput = serde_json::from_value(input.clone()).map_err(|error| {
            wrong_input(format!(
                "«fault_list» takes «everything» and «at_most»: {error}"
            ))
        })?;
        let Some(store) = open(&self.store)? else {
            return Ok(ActionOutcome::Went(
                json!({"register": REGISTER_ABSENT, "faults": [], "open": 0, "total": 0}),
            ));
        };
        let all = store.all().map_err(unreadable)?;
        let open_now = all.iter().filter(|fault| fault.still_open()).count();
        let mut shown: Vec<&faults::Fault> = if asked.everything {
            all.iter().collect()
        } else {
            all.iter().filter(|fault| fault.still_open()).collect()
        };
        if let Some(ceiling) = asked.at_most {
            shown.truncate(ceiling);
        }
        Ok(ActionOutcome::Went(json!({
            "register": REGISTER_PRESENT,
            "faults": shown,
            // Counted over the whole register, not over what came back: a flow
            // that read ten of twenty must not conclude there are ten.
            "open": open_now,
            "total": all.len(),
            "shown": shown.len(),
        })))
    }

    fn species(&self) -> StepSpecies {
        StepSpecies::Repeatable
    }
}

// ── writing ──────────────────────────────────────────────────────────────

pub struct FaultRecordAction {
    store: Option<PathBuf>,
}

impl FaultRecordAction {
    pub fn new(store: Option<PathBuf>) -> Self {
        Self { store }
    }
}

impl Action for FaultRecordAction {
    fn execute(&self, input: &Value, _shared: &SharedState) -> Result<ActionOutcome, ActionError> {
        // Validated before the store is opened, so a malformed step is wrong on
        // every machine instead of only where the register happens to exist.
        let draft: Draft = serde_json::from_value(input.clone()).map_err(|error| {
            wrong_input(format!(
                "a fault is written with happened_on, what_happened, how_it_showed, \
                 what_would_prevent and status: {error}"
            ))
        })?;
        if draft.what_would_prevent.trim().is_empty() {
            return Err(wrong_input(
                "«what_would_prevent» is missing: a fault without the check that would \
                 have stopped it is an anecdote, not work",
            ));
        }
        let Some(store) = open(&self.store)? else {
            // Refused, not silently dropped. A flow told the machine it found a
            // defect; answering "went" with nothing written would lose it.
            return Err(unreadable("there is no register on this machine"));
        };
        let recorded = store.record(&draft).map_err(unreadable)?;
        Ok(ActionOutcome::Went(json!({
            "register": REGISTER_PRESENT,
            "number": recorded.number,
            "fault": recorded,
        })))
    }

    fn species(&self) -> StepSpecies {
        // Running it again writes a second fault with a second number. Nothing
        // here can tell a repeat from a new defect, so nothing here may decide.
        StepSpecies::HandToHuman
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(label: &str) -> PathBuf {
        let directory = std::env::temp_dir().join(format!(
            "sailor-fault-node-{label}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("the clock does not run backwards")
                .as_nanos()
        ));
        std::fs::create_dir_all(&directory).expect("the scratch directory");
        directory.join(faults::FAULTS_FILE)
    }

    fn a_draft(what: &str) -> Value {
        json!({
            "happened_on": "01/09",
            "what_happened": what,
            "how_it_showed": "by running it",
            "what_would_prevent": "a test that is born red",
            "status": "**aperto**",
        })
    }

    /// A flow writes a fault and the store gives it the number, exactly as the
    /// command line does: one register, one place that decides.
    #[test]
    fn a_flow_records_a_fault_and_the_store_numbers_it() {
        let path = scratch("record");
        let node = FaultRecordAction::new(Some(path.clone()));

        let first = node
            .execute(&a_draft("the first"), &SharedState::new())
            .expect("recording");
        let ActionOutcome::Went(said) = first else {
            panic!("recording a fault is not a refusal");
        };
        assert_eq!(said["number"], 1);

        let second = node
            .execute(&a_draft("the second"), &SharedState::new())
            .expect("recording");
        let ActionOutcome::Went(said) = second else {
            panic!("recording a fault is not a refusal");
        };
        assert_eq!(
            said["number"], 2,
            "the second does not get the first's number"
        );
    }

    /// The same rule the command line enforces, enforced here too. If only one
    /// door checked it, the register would fill with diary entries through the
    /// other one.
    #[test]
    fn a_fault_without_the_check_that_would_stop_it_is_refused() {
        let node = FaultRecordAction::new(Some(scratch("anecdote")));
        let mut draft = a_draft("something");
        draft["what_would_prevent"] = json!("   ");

        let refused = node
            .execute(&draft, &SharedState::new())
            .expect_err("an anecdote is not a fault");

        assert_eq!(refused.class, "invalid_input");
        assert!(
            refused.said.contains("what_would_prevent"),
            "{}",
            refused.said
        );
    }

    /// Reading gives back only what is open, and says how many there are in
    /// total: a flow that reads a slice must not conclude the slice is all.
    #[test]
    fn reading_gives_the_open_ones_and_counts_the_rest() {
        let path = scratch("read");
        let writer = FaultRecordAction::new(Some(path.clone()));
        for what in ["one", "two"] {
            writer
                .execute(&a_draft(what), &SharedState::new())
                .expect("recording");
        }
        let store = Faults::open(&path).expect("opening");
        store
            .set_status(1, "**chiuso** with a mutant")
            .expect("closing one");

        let node = FaultListAction::new(Some(path));
        let ActionOutcome::Went(said) = node
            .execute(&json!({}), &SharedState::new())
            .expect("reading")
        else {
            panic!("reading is not a refusal");
        };

        assert_eq!(said["total"], 2);
        assert_eq!(said["open"], 1, "the closed one is not open");
        assert_eq!(said["shown"], 1);
        assert_eq!(said["faults"][0]["number"], 2);
    }

    /// A machine with no register answers, it does not fail: a flow that asks
    /// what is open has a branch for "nothing is recorded here".
    #[test]
    fn reading_without_a_register_says_so_instead_of_breaking() {
        let node = FaultListAction::new(None);
        let ActionOutcome::Went(said) = node
            .execute(&json!({}), &SharedState::new())
            .expect("an absent register is not a fault of the flow")
        else {
            panic!("reading is not a refusal");
        };
        assert_eq!(said["register"], REGISTER_ABSENT);
        assert_eq!(said["open"], 0);
    }

    /// Writing without a register is refused instead. The flow found a defect
    /// and said so; answering "went" with nothing written would lose it.
    #[test]
    fn writing_without_a_register_is_refused_and_not_swallowed() {
        let node = FaultRecordAction::new(None);
        let refused = node
            .execute(&a_draft("something"), &SharedState::new())
            .expect_err("a fault that goes nowhere is not recorded");
        assert_eq!(refused.class, "store_unreadable");
    }
}
