//! The shipped flow that answers «who works on what, where, since when, and
//! what died».
//!
//! The terminals, claims and runs it reads are written here: the machine's
//! would make the proof depend on who had a window open. It names no engine,
//! which is half its point — a watch that costs a call gets switched off.

use flow::{
    ActionRegistry, Clock, Execution, ExecutionRequest, Executor, FlowError, FlowFile, Graph,
    InMemoryRecordStore, InProcessExecutor, SharedState,
};
use ledger::{Ledger, RunRecord};
use serde_json::{json, Value};

const FLOW_ID: &str = "watch-the-crew";

fn flow_file() -> FlowFile {
    let text = flow::system::FLOWS
        .iter()
        .find(|(name, _)| *name == FLOW_ID)
        .map(|(_, text)| *text)
        .unwrap_or_else(|| panic!("«{FLOW_ID}» is not among the shipped flows"));
    serde_json::from_str(text).expect("the shipped flow loads")
}

struct Scratch(std::path::PathBuf);

impl Scratch {
    fn new() -> Self {
        static MADE: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
        let path = std::env::temp_dir().join(format!(
            "sailor-watch-{}-{}",
            std::process::id(),
            MADE.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
        ));
        let _ = std::fs::remove_dir_all(&path);
        std::fs::create_dir_all(&path).expect("a directory to work in");
        Scratch(path)
    }

    fn sessions(&self) -> std::path::PathBuf {
        self.0.join("sessions.db")
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

struct Tick(std::sync::atomic::AtomicI64);

impl Clock for Tick {
    fn now(&self) -> Result<i64, FlowError> {
        Ok(self.0.fetch_add(1, std::sync::atomic::Ordering::Relaxed) + 1)
    }
}

/// A terminal Sailor follows, open in the tree it names.
fn a_terminal(scratch: &Scratch, tty: &str, worktree: &str) {
    let sessions = sessions::Sessions::open(scratch.sessions()).expect("a sessions file of our own");
    sessions
        .open_terminal(&sessions::Arrival {
            anchor: sessions::Anchor {
                tty: tty.to_owned(),
                worktree: worktree.to_owned(),
                ancestor: Some("terminal".to_owned()),
            },
            session_id: Some(format!("session-{tty}")),
            transcript_path: None,
            at: 1_700_000_000,
        })
        .expect("the terminal is written down");
}

/// A claim an agent left for the others, and a run waiting for a person.
fn a_claim_and_a_waiting_run(ledger: &Ledger, agent: &str, repository: &str) {
    let mut registry = ActionRegistry::default();
    actions::presence::register_presence(&mut registry, Some(ledger.clone()));
    let claim = registry.get("work_claim").expect("the claim node");
    flow::Action::execute(
        claim,
        &json!({"agent": agent, "repository": repository, "doing": "the front's second engine"}),
        &SharedState::new(),
    )
    .expect("the claim is written");
    ledger
        .record_run(&RunRecord {
            run_id: "run-che-attende".to_owned(),
            kind: "flow".to_owned(),
            entity: "dispatch-the-work".to_owned(),
            parent_run_id: None,
            started_by: "prova".to_owned(),
            status: "waiting".to_owned(),
            total_cost_micros: 0,
            error: None,
            started_at: 1_700_000_100,
            ended_at: None,
            worktree: None,
            stop_reason: None,
        })
        .expect("the waiting run is recorded");
}

/// The product's registry over a house of this test's own, with its terminals
/// instead of the machine's — the move `write_down_what_broke` makes too.
fn registry(scratch: &Scratch, ledger: &Ledger) -> ActionRegistry {
    let mut registry =
        registry::registry_in(registry::House::under(&scratch.0), Some(ledger.clone()), None);
    actions::terminals::register_terminals(&mut registry, Some(scratch.sessions()));
    registry
}

fn run(scratch: &Scratch, ledger: &Ledger, graph: &Graph) -> (Execution, InMemoryRecordStore) {
    let store = InMemoryRecordStore::default();
    let request = ExecutionRequest {
        run_id: "guardia".to_owned(),
        root_inputs: flow_file().inputs.clone(),
        gates: Vec::new(),
        shared: SharedState::new(),
        spend_cap_micros: None,
        stops: flow::RunStops::default(),
    };
    let execution = InProcessExecutor
        .execute(
            graph,
            request,
            &store,
            &registry(scratch, ledger),
            &Tick(0.into()),
        )
        .expect("the execution does not break");
    (execution, store)
}

/// What the last step printed, which is what a person or an agent reads.
fn what_the_watch_printed(store: &InMemoryRecordStore) -> Value {
    let records = store.all();
    let printed = records
        .iter()
        .filter(|record| record.step_id == "the_watch")
        .filter_map(|record| record.output.clone())
        .next_back()
        .expect("the watch closed");
    printed["answer"].clone()
}

/// **THE THREE READINGS, IN ONE ANSWER.** The terminal with its tree and the
/// hour it opened at, the claim its agent left, the run waiting for somebody:
/// the point of the flow is that they arrive together, because «is anything
/// due» is a question about the comparison.
#[test]
fn the_watch_prints_who_works_where_and_what_waits() {
    let scratch = Scratch::new();
    let ledger = Ledger::open(scratch.0.join("ledger")).expect("a store of our own");
    a_terminal(&scratch, "ttys009", "/trees/sailor");
    a_claim_and_a_waiting_run(&ledger, "quello-di-prova", "sailor");

    let (_, store) = run(&scratch, &ledger, &flow_file().graph);
    let watch = what_the_watch_printed(&store);

    let terminal = &watch["terminals"]["working"][0];
    assert_eq!(terminal["tty"], json!("ttys009"), "the watch: {watch}");
    assert_eq!(
        terminal["worktree"],
        json!("/trees/sailor"),
        "where the work happens is part of the answer: {watch}"
    );
    assert!(
        terminal["opened_at"].is_number(),
        "«since when» has to be in there: {watch}"
    );
    assert_eq!(
        watch["claims"]["working"][0]["agent"],
        json!("quello-di-prova"),
        "the claim an agent left for the others: {watch}"
    );
    let waiting = &watch["open_runs"]["answer"]["waiting"];
    assert!(
        waiting
            .as_array()
            .expect("the waiting runs")
            .iter()
            .any(|run| run["run_id"] == json!("run-che-attende")),
        "a run waiting for a person is what «something is due» means: {watch}"
    );
}

/// **A TERMINAL THAT DETACHED IS NOT A TERMINAL THAT CLOSED**, and the watch
/// keeps them apart — one is over, the other is somebody else's now.
#[test]
fn what_died_is_told_apart_from_what_asked_not_to_be_followed() {
    let scratch = Scratch::new();
    let ledger = Ledger::open(scratch.0.join("ledger")).expect("a store of our own");
    a_terminal(&scratch, "ttys010", "/trees/one");
    a_terminal(&scratch, "ttys011", "/trees/two");
    let sessions = sessions::Sessions::open(scratch.sessions()).expect("the sessions file");
    sessions
        .close_terminal("ttys010", 1_700_000_500)
        .expect("it said goodbye");
    sessions
        .detach(
            &sessions::Anchor {
                tty: "ttys011".to_owned(),
                worktree: "/trees/two".to_owned(),
                ancestor: None,
            },
            1_700_000_600,
        )
        .expect("it asked not to be followed");

    let (_, store) = run(&scratch, &ledger, &flow_file().graph);
    let watch = what_the_watch_printed(&store);

    let gone = watch["terminals"]["gone"].as_array().expect("a list");
    let why: Vec<(&str, &str)> = gone
        .iter()
        .map(|entry| {
            (
                entry["tty"].as_str().expect("a tty"),
                entry["why"].as_str().expect("a reason"),
            )
        })
        .collect();
    assert!(why.contains(&("ttys010", "closed")), "the watch: {watch}");
    assert!(why.contains(&("ttys011", "detached")), "the watch: {watch}");
    assert!(
        watch["terminals"]["working"]
            .as_array()
            .expect("a list")
            .is_empty(),
        "neither of the two is working any more: {watch}"
    );
}

/// **IT NAMES NO ENGINE AND SPENDS NOTHING.** The claim of the flow is that it
/// can run at every beat, and a paid step would take that away without any
/// test noticing.
#[test]
fn the_watch_starts_no_engine() {
    let flow = flow_file();
    assert_eq!(flow.id, FLOW_ID);
    let ids: Vec<&str> = flow
        .graph
        .steps()
        .iter()
        .map(|step| step.id.as_str())
        .collect();
    assert_eq!(
        ids,
        vec![
            "trigger",
            "which_terminals",
            "who_claims",
            "what_is_open",
            "the_watch"
        ]
    );
    for step in flow.graph.steps() {
        assert_ne!(
            step.action, "external_engine",
            "«{}» starts an engine: the watch stops being free",
            step.id
        );
    }
    assert_eq!(flow.spend_cap_micros, None);
}

/// **A WATCH NOBODY WINDS IS A WATCH THAT STOPPED.** The beat reads the
/// recurrence off the flow file; without one the watch runs only when a person
/// asks, which is never at the hour something died. Every half hour, light: 48
/// runs a day on a machine that stays on for years, and no engine in any.
#[test]
fn the_watch_beats_on_its_own_every_half_hour() {
    let flow = flow_file();
    let schedule = serde_json::to_value(flow.schedule.expect("the watch has a schedule"))
        .expect("a schedule serialises");
    assert_eq!(
        schedule,
        json!({
            "recurrence": { "kind": "every_seconds", "seconds": 1800 },
            "weight": "light",
            "perimeter": []
        })
    );
}
