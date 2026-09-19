//! The nodes with which a flow reads and writes the fault register.
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
/// Hands development its next mandate: the oldest fault still open.
pub const FAULT_NEXT_ACTION: &str = "fault_next";

/// Said when this machine has no register. The node stays registered and
/// declares it, the same way `history_ask` declares an absent ledger: a node
/// that vanished when its store did would make a flow fail somewhere else.
const REGISTER_ABSENT: &str = "absent";
const REGISTER_PRESENT: &str = "present";

pub fn register_faults(registry: &mut flow::ActionRegistry, store: Option<PathBuf>) {
    registry.register(FAULT_LIST_ACTION, FaultListAction::new(store.clone()));
    registry.register(FAULT_NEXT_ACTION, FaultNextAction::new(store.clone()));
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

// ── the next one to take ─────────────────────────────────────────────────

/// The oldest fault still open, whole, so a flow can make it a mandate without
/// a second reading. `open` is a boolean here and not the count `fault_list`
/// gives: it is what a step's `when` reads to leave an engine unstarted.
pub struct FaultNextAction {
    store: Option<PathBuf>,
}

impl FaultNextAction {
    pub fn new(store: Option<PathBuf>) -> Self {
        Self { store }
    }
}

impl Action for FaultNextAction {
    fn execute(&self, _input: &Value, _shared: &SharedState) -> Result<ActionOutcome, ActionError> {
        let Some(store) = open(&self.store)? else {
            return Ok(ActionOutcome::Went(
                json!({"register": REGISTER_ABSENT, "open": false, "remaining": 0}),
            ));
        };
        let remaining = store.still_open().map_err(unreadable)?;
        let Some(fault) = store.next_open().map_err(unreadable)? else {
            return Ok(ActionOutcome::Went(
                json!({"register": REGISTER_PRESENT, "open": false, "remaining": 0}),
            ));
        };
        Ok(ActionOutcome::Went(json!({
            "register": REGISTER_PRESENT,
            "open": true,
            "remaining": remaining,
            "number": fault.number,
            "happened_on": fault.happened_on,
            "what_happened": fault.what_happened,
            "how_it_showed": fault.how_it_showed,
            "what_would_prevent": fault.what_would_prevent,
            "status": fault.status,
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

/// Whether a fault already open and word for word the same stops a second one
/// being written. **The step declares it**: a watcher on a schedule repeats
/// itself by construction, a person reporting twice usually means twice.
#[derive(Debug, Deserialize)]
struct RecordInput {
    #[serde(default)]
    only_if_not_already_open: bool,
}

impl Action for FaultRecordAction {
    fn execute(&self, input: &Value, _shared: &SharedState) -> Result<ActionOutcome, ActionError> {
        let asked: RecordInput = serde_json::from_value(input.clone()).map_err(|error| {
            wrong_input(format!("«only_if_not_already_open» is true or false: {error}"))
        })?;
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
        if asked.only_if_not_already_open {
            let already = store
                .all()
                .map_err(unreadable)?
                .into_iter()
                .find(|fault| fault.still_open() && fault.what_happened == draft.what_happened);
            if let Some(already) = already {
                return Ok(ActionOutcome::Went(json!({
                    "register": REGISTER_PRESENT,
                    "number": already.number,
                    "fault": already,
                    "already_open": true,
                })));
            }
        }
        let recorded = store.record(&draft).map_err(unreadable)?;
        Ok(ActionOutcome::Went(json!({
            "register": REGISTER_PRESENT,
            "number": recorded.number,
            "fault": recorded,
            "already_open": false,
        })))
    }

    fn species(&self) -> StepSpecies {
        // Running it again writes a second fault with a second number, unless
        // the step said `only_if_not_already_open` — and a step that did not
        // say it is one nothing here may decide for.
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
            "status": "**open**",
        })
    }

    /// A watcher on a schedule meets the same drift every quarter of an hour.
    /// Twenty rows for four defects is a register nobody reads.
    #[test]
    fn a_step_that_asked_for_it_does_not_open_a_second_fault_saying_the_same_thing() {
        let store = scratch("already-open");
        let action = FaultRecordAction::new(Some(store));
        let mut draft = a_draft("the lab no longer runs what the repository declares");
        draft["only_if_not_already_open"] = json!(true);

        let first = went(action.execute(&draft, &SharedState::new()));
        let again = went(action.execute(&draft, &SharedState::new()));

        assert_eq!(first["already_open"], json!(false));
        assert_eq!(again["already_open"], json!(true));
        assert_eq!(again["number"], first["number"]);
    }

    /// The default, and the reason it is the default: a person reporting the
    /// same sentence twice usually means it happened twice.
    #[test]
    fn a_step_that_did_not_ask_for_it_opens_a_second_fault_as_it_always_did() {
        let store = scratch("twice-over");
        let action = FaultRecordAction::new(Some(store));
        let draft = a_draft("the same sentence, written twice");

        let first = went(action.execute(&draft, &SharedState::new()));
        let again = went(action.execute(&draft, &SharedState::new()));

        assert_ne!(again["number"], first["number"]);
    }

    /// Only an open one holds the door: a defect that comes back after it was
    /// closed is news, and closing it must not silence the next report.
    #[test]
    fn a_fault_that_was_closed_does_not_hold_the_door_against_the_same_one_returning() {
        let store = scratch("closed-then-back");
        let action = FaultRecordAction::new(Some(store.clone()));
        let mut draft = a_draft("the service went down");
        draft["only_if_not_already_open"] = json!(true);
        let first = went(action.execute(&draft, &SharedState::new()));
        let number = first["number"].as_i64().expect("a number");
        Faults::open(&store)
            .expect("the register opens")
            .set_status(number, "**closed** — the service was brought back")
            .expect("the status is written");

        let again = went(action.execute(&draft, &SharedState::new()));

        assert_eq!(again["already_open"], json!(false));
        assert_ne!(again["number"], first["number"]);
    }

    fn went(outcome: Result<ActionOutcome, ActionError>) -> Value {
        match outcome.expect("the step went") {
            ActionOutcome::Went(answer) => answer,
            other => panic!("the step did not go: {other:?}"),
        }
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
            .set_status(1, "**closed** with a mutant")
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

    /// The next fault is the oldest still open, not the oldest: the closed one
    /// in front of it is passed over, and the one behind it waits its turn.
    #[test]
    fn the_next_fault_is_the_oldest_still_open() {
        let path = scratch("next");
        let writer = FaultRecordAction::new(Some(path.clone()));
        for what in ["one", "two", "three"] {
            writer
                .execute(&a_draft(what), &SharedState::new())
                .expect("recording");
        }
        Faults::open(&path)
            .expect("opening")
            .set_status(1, "**closed** with a mutant")
            .expect("closing the oldest");

        let node = FaultNextAction::new(Some(path));
        let ActionOutcome::Went(said) = node
            .execute(&json!({}), &SharedState::new())
            .expect("reading")
        else {
            panic!("reading is not a refusal");
        };

        assert_eq!(said["open"], true);
        assert_eq!(said["number"], 2, "the closed one is passed over: {said}");
        assert_eq!(said["what_happened"], "two");
        assert_eq!(said["happened_on"], "01/09");
        assert_eq!(said["remaining"], 2, "counted over the register, not over the answer");
    }

    /// Nothing open is an answer with `open: false`, not a failure: the flow
    /// that asks has a `when` to read it, and an error would break the run.
    #[test]
    fn with_nothing_open_the_next_fault_says_so_and_does_not_fail() {
        let path = scratch("none-open");
        FaultRecordAction::new(Some(path.clone()))
            .execute(&a_draft("the only one"), &SharedState::new())
            .expect("recording");
        Faults::open(&path)
            .expect("opening")
            .set_status(1, "**closed** in the same test")
            .expect("closing it");

        let ActionOutcome::Went(said) = FaultNextAction::new(Some(path))
            .execute(&json!({}), &SharedState::new())
            .expect("nothing open is not a refusal")
        else {
            panic!("reading is not a refusal");
        };

        assert_eq!(said["open"], false, "{said}");
        assert_eq!(said["register"], REGISTER_PRESENT);
        assert!(said.get("number").is_none(), "no fault is named: {said}");
    }

    /// A machine with no register has nothing open, and says so the same way.
    #[test]
    fn without_a_register_the_next_fault_is_none() {
        let ActionOutcome::Went(said) = FaultNextAction::new(None)
            .execute(&json!({}), &SharedState::new())
            .expect("an absent register is not a fault of the flow")
        else {
            panic!("reading is not a refusal");
        };
        assert_eq!(said["open"], false);
        assert_eq!(said["register"], REGISTER_ABSENT);
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
