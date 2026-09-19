//! The relay does not stop at the emptying: it starts what the emptying opened.
//!
//! **THE WIRING IS WHAT BREAKS, AND ONLY A RUN SEES IT.** A pointer naming a
//! step that does not produce the field passes every test that reads the file,
//! and hands the typing node an empty terminal name. So the shipped flow runs
//! whole here: the world is scripted, the mandate nodes are the real ones.

use flow::{
    Action, ActionError, ActionOutcome, Clock, Decision, ExecutionRequest, Executor, FlowError,
    FlowFile, InMemoryRecordStore, InProcessExecutor, SharedState, StepSpecies,
};
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

const THE_FLOW: &str = "empty-a-session-that-handed-on";
/// **A «NOT YET» COMES BACK, AND IN PROCESS IT COMES BACK FOR EVER**: out here
/// a parked run waits for a person, here it is re-offered as soon as the clock
/// allows, and this clock is a counter. The wall ends the run nobody arrives at.
const THE_WALL: i64 = 200;
const THE_PREDECESSOR: &str = "the-session-that-filled-up";
const THE_SUCCESSOR: &str = "the-session-the-emptying-opened";

fn store() -> &'static Path {
    &deposit_and_store().1
}

/// One store for the binary: the flow hands the mandate nodes none, so only the
/// variable keeps this run off the store of this machine, and a variable is the
/// process's. Opened once too — three threads on one sqlite get «locked».
fn deposit_and_store() -> &'static (ledger::Ledger, PathBuf) {
    static STORE: std::sync::OnceLock<(ledger::Ledger, PathBuf)> = std::sync::OnceLock::new();
    STORE.get_or_init(|| {
        let dir = std::env::temp_dir().join(format!("sailor-relay-wake-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("a store to work in");
        std::env::set_var("SAILOR_LEDGER", &dir);
        let deposit = ledger::Ledger::open(&dir).expect("a deposit to work in");
        (deposit, dir)
    })
}

struct Tick(std::sync::atomic::AtomicI64);

impl Clock for Tick {
    fn now(&self) -> Result<i64, FlowError> {
        Ok(self.0.fetch_add(1, std::sync::atomic::Ordering::Relaxed) + 1)
    }
}

struct Scripted {
    said: Value,
    seen: Arc<Mutex<Vec<Value>>>,
}

impl Action for Scripted {
    fn execute(&self, input: &Value, _shared: &SharedState) -> Result<ActionOutcome, ActionError> {
        self.seen.lock().expect("nobody breaks here").push(input.clone());
        Ok(ActionOutcome::Went(self.said.clone()))
    }

    fn species(&self) -> StepSpecies {
        StepSpecies::Repeatable
    }
}

/// The emptying, and whether a successor follows it. What the next step waits
/// for is the arrival, and it arrives when the new session's greeting takes the
/// mandate.
struct Empties {
    tty: String,
    successor: Option<String>,
}

impl Action for Empties {
    fn execute(&self, input: &Value, _shared: &SharedState) -> Result<ActionOutcome, ActionError> {
        if let Some(successor) = &self.successor {
            let path = sessions::mandate::address_in(store(), &self.tty);
            sessions::mandate::consume(&path, successor, 1_700_000_000)
                .expect("the greeting of the successor takes the mandate");
        }
        Ok(ActionOutcome::Went(json!({
            "tty": input["tty"].clone(),
            "typed": "the line the descriptor declares",
        })))
    }

    fn species(&self) -> StepSpecies {
        StepSpecies::Repeatable
    }
}

fn deposited(tty: &str) {
    let mandate = sessions::mandate::Mandate {
        written: sessions::mandate::Written {
            tty: tty.to_owned(),
            session: THE_PREDECESSOR.to_owned(),
            engine: "a-command-line".to_owned(),
            ..Default::default()
        },
        work: sessions::mandate::Work {
            goal: "what the session before was doing".to_owned(),
            next: "the one thing it left".to_owned(),
            ..Default::default()
        },
        ..Default::default()
    };
    sessions::mandate::deposit(store(), &mandate).expect("the mandate is on disk");
}

/// The shipped flow, with only the wait for a successor cut to what a test can
/// stand.
fn the_flow(wait_seconds: u64) -> FlowFile {
    let text = flow::system::FLOWS
        .iter()
        .find(|(name, _)| *name == THE_FLOW)
        .map(|(_, text)| *text)
        .expect("the flow is shipped");
    let mut file: Value = serde_json::from_str(text).expect("the flow is JSON");
    let steps = file["graph"]["steps"].as_array_mut().expect("steps");
    let arrived = steps
        .iter_mut()
        .find(|step| step["id"] == "arrived")
        .expect("the flow waits for an arrival");
    arrived["with"]["within_seconds"] = json!(wait_seconds);
    serde_json::from_value(file).expect("the flow file loads")
}

fn relay(tty: &str, successor: Option<&str>, wait_seconds: u64) -> (Vec<Value>, Vec<Decision>) {
    let typed = Arc::new(Mutex::new(Vec::new()));
    let deposit = deposit_and_store().0.clone();
    let mut registry = registry::registry_in(registry::House::empty(), Some(deposit), None);
    registry.register(
        "trigger",
        Scripted {
            said: json!({
                "text": "the session finished a turn",
                "carried": {"tty": tty, "session": THE_PREDECESSOR, "transcript": null},
            }),
            seen: Arc::new(Mutex::new(Vec::new())),
        },
    );
    registry.register(
        "measure_session",
        Scripted {
            said: json!({"state": "oblige", "tokens": 260_000}),
            seen: Arc::new(Mutex::new(Vec::new())),
        },
    );
    registry.register(
        "empty_terminal",
        Empties {
            tty: tty.to_owned(),
            successor: successor.map(str::to_owned),
        },
    );
    // The screen of a terminal this test has none of: here it is always painted.
    registry.register(
        "wait_free",
        Scripted {
            said: json!({"tty": tty, "free": true, "prompt": "the prompt"}),
            seen: Arc::new(Mutex::new(Vec::new())),
        },
    );
    registry.register(
        "type_into_terminal",
        Scripted {
            said: json!({"tty": tty, "typed": "a line", "at": 1}),
            seen: Arc::clone(&typed),
        },
    );

    let flow = the_flow(wait_seconds);
    let records = InMemoryRecordStore::default();
    let run_id = format!("relay-{}-{tty}", std::process::id());
    let request = ExecutionRequest {
        holder: None,
        run_id: run_id.clone(),
        root_inputs: flow.inputs.clone().into_iter().collect(),
        gates: Vec::new(),
        shared: SharedState::new(),
        spend_cap_micros: None,
        // **A «NOT YET» COMES BACK, AND IN A TEST IT COMES BACK FOR EVER.**
        // Out here a parked run waits for somebody to resume it; in process it
        // is re-offered as soon as the clock allows, so the run that finds no
        // successor is given a count of turns and ends.
        stops: flow::RunStops {
            wall_deadline_at: Some(THE_WALL),
            ..Default::default()
        },
    };
    let execution = InProcessExecutor
        .execute(&flow.graph, request, &records, &registry, &Tick(0.into()))
        .expect("the run does not break");
    let asked = typed.lock().expect("nobody breaks here").clone();
    (asked, execution.decisions)
}

/// **A GREETING NOBODY ANSWERS IS NOT A HANDOVER.** The line the relay types is
/// what turns a session that was opened into a session that is working.
#[test]
fn the_successor_that_took_the_mandate_is_started_at_its_own_terminal() {
    deposited("ttys001");

    let (typed, decisions) = relay("ttys001", Some(THE_SUCCESSOR), 120);

    assert_eq!(typed.len(), 1, "one line is typed to the successor: {typed:?}");
    assert_eq!(
        typed[0]["tty"],
        json!("ttys001"),
        "the line goes to the terminal the mandate names: {}",
        typed[0]
    );
    assert!(
        typed[0]["line"].as_str().is_some_and(|line| !line.is_empty()),
        "a wake with no line is a step that does nothing: {}",
        typed[0]
    );
    assert_eq!(decisions.last(), Some(&Decision::Complete), "{decisions:?}");
}

/// Where nobody took the mandate the relay types nothing more: the line would
/// land in whatever is standing at that terminal.
#[test]
fn a_terminal_where_nobody_arrived_is_typed_into_no_further() {
    deposited("ttys002");

    let (typed, decisions) = relay("ttys002", None, 0);

    assert!(typed.is_empty(), "nothing is typed with no successor: {typed:?}");
    assert_ne!(
        decisions.last(),
        Some(&Decision::Complete),
        "a relay that opened a session and never started it is not a finished relay: {decisions:?}"
    );
}

/// And the same run with the mandate taken back by its own author, as a
/// compaction takes it: not an arrival, so nothing is typed.
#[test]
fn a_mandate_its_own_author_took_back_starts_nobody() {
    deposited("ttys003");

    let (typed, _) = relay("ttys003", Some(THE_PREDECESSOR), 0);

    assert!(typed.is_empty(), "the author is not a successor: {typed:?}");
}
