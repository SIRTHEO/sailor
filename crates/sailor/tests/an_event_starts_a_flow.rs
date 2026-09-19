//! A session says something happened, and a flow starts because of it.
//!
//! The arc, judged without starting anything: which flows are candidates, what
//! single reason held the others, and that one event starts one run.

use flow::system::FlowSource;
use sailor::arc_cmd::{evaluate, watchers, ALREADY_PARKED, SESSION_EVENT};
use serde_json::json;
use sessions::{Sessions, ACTED, DEFERRED};
use std::path::PathBuf;
use trigger::Happened;

/// A tree of flow files and a store, taken down with the test.
struct Scratch(PathBuf);

impl Scratch {
    fn new(name: &str) -> Self {
        let path = std::env::temp_dir().join(format!("sailor-arc-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&path);
        std::fs::create_dir_all(path.join("flows")).expect("a place for flows");
        Scratch(path)
    }

    /// One flow whose only step is a trigger, declaring what it watches for.
    fn holding(&self, id: &str, with: serde_json::Value) -> &Self {
        let flow = json!({
            "id": id,
            "description": "a fixture whose only step is a trigger",
            "inputs": {},
            "graph": {"steps": [{
                "id": "trigger",
                "deps": [],
                "action": "trigger",
                "max_attempts": 1,
                "when": null,
                "with": with,
                "input_schema": {"type": "object", "properties": {}, "required": [], "allow_extra": true},
                "output_schema": {"type": "object", "properties": {}, "required": [], "allow_extra": true},
            }]},
        });
        std::fs::write(
            self.0.join("flows").join(format!("{id}.flow.json")),
            serde_json::to_string_pretty(&flow).expect("a flow serialises"),
        )
        .expect("the flow file is written");
        self
    }

    fn sources(&self) -> Vec<FlowSource> {
        vec![FlowSource {
            origin: "declared",
            dir: self.0.join("flows"),
        }]
    }

    fn store(&self) -> Sessions {
        Sessions::open(self.0.join("sessions.db")).expect("a store of this test's own")
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn watching(event: &str) -> serde_json::Value {
    json!({"source": SESSION_EVENT, "on": {"event": event}})
}

fn happened() -> Happened {
    Happened {
        event: "UserPromptSubmit".to_owned(),
        tree: "/a/tree".to_owned(),
        tty: "ttys001".to_owned(),
        session: "a-session".to_owned(),
        prompt: Some("carry on".to_owned()),
        transcript: Some("/a/tree/.transcript.jsonl".to_owned()),
    }
}

/// Nothing is parked: the arc's own default in these tests, so a test that
/// wants a parked run says so.
fn never_parked(_flow: &str, _tty: &str) -> bool {
    false
}

/// What was asked of the starter, instead of anything being started.
fn watching_starter(
    asked: &mut Vec<(String, String)>,
) -> impl FnMut(&str, &str) -> Result<String, String> + '_ {
    move |flow: &str, delivery: &str| {
        asked.push((flow.to_owned(), delivery.to_owned()));
        Ok(format!("started {flow}"))
    }
}

#[test]
fn a_flow_that_declares_the_source_is_a_candidate_and_one_that_does_not_is_not() {
    let scratch = Scratch::new("watchers");
    scratch
        .holding("watches", watching("UserPromptSubmit"))
        .holding("by-hand", json!({"source": "manual", "text": "anything"}));

    let found = watchers(&scratch.sources());

    assert_eq!(found.len(), 1, "{found:?}");
    assert_eq!(found[0].0, "watches");
    assert_eq!(found[0].1.event, "UserPromptSubmit");
}

/// **A FLOW ALREADY PARKED ON THIS TERMINAL IS NOT STARTED AGAIN.** Fault 163:
/// starting on every event made 57 parked runs out of 59 actions. Both
/// directions, because the claim is a difference.
#[test]
fn a_flow_parked_on_this_terminal_is_held_back_instead_of_started_again() {
    let scratch = Scratch::new("parked");
    scratch.holding("watches", watching("UserPromptSubmit"));
    let store = scratch.store();

    let mut asked = Vec::new();
    let verdicts = evaluate(
        &store,
        11,
        &happened(),
        &scratch.sources(),
        100,
        &mut watching_starter(&mut asked),
        &mut |_flow, _tty| true,
    );

    assert!(asked.is_empty(), "nothing was started: {asked:?}");
    assert_eq!(verdicts.len(), 1, "the refusal still leaves its row");
    assert_eq!(verdicts[0].verdict, DEFERRED);
    assert_eq!(verdicts[0].why.as_deref(), Some(ALREADY_PARKED));

    // The control: the same flow and the same event, with nothing parked.
    let mut asked = Vec::new();
    let verdicts = evaluate(
        &store,
        12,
        &happened(),
        &scratch.sources(),
        100,
        &mut watching_starter(&mut asked),
        &mut never_parked,
    );

    assert_eq!(asked.len(), 1, "with nothing parked it starts: {asked:?}");
    assert_eq!(verdicts[0].verdict, ACTED);
}

/// **AN EVENT THAT MATCHES STARTS ONE RUN, AND THE ROW POINTS AT THE EVENT.**
#[test]
fn an_event_a_flow_watches_for_starts_it_once() {
    let scratch = Scratch::new("starts");
    scratch.holding("watches", watching("UserPromptSubmit"));
    let store = scratch.store();
    let mut asked = Vec::new();

    let verdicts = evaluate(
        &store,
        7,
        &happened(),
        &scratch.sources(),
        100,
        &mut watching_starter(&mut asked),
        &mut never_parked,
    );

    assert_eq!(asked.len(), 1, "exactly one run: {asked:?}");
    assert_eq!(asked[0].0, "watches");
    assert_eq!(verdicts.len(), 1);
    assert_eq!(verdicts[0].verdict, ACTED);
    assert_eq!(verdicts[0].event_id, 7, "the run points at what started it");
}

/// **A FLOW THAT DOES NOT DECLARE THE SOURCE IS NEVER ASKED.** It leaves no
/// row, because it was not judged: it is not in this conversation at all.
#[test]
fn a_flow_without_the_declaration_starts_nothing_and_leaves_no_row() {
    let scratch = Scratch::new("undeclared");
    scratch.holding("by-hand", json!({"source": "manual", "text": "anything"}));
    let store = scratch.store();
    let mut asked = Vec::new();

    let verdicts = evaluate(
        &store,
        8,
        &happened(),
        &scratch.sources(),
        100,
        &mut watching_starter(&mut asked),
        &mut never_parked,
    );

    assert!(asked.is_empty(), "{asked:?}");
    assert!(verdicts.is_empty(), "{verdicts:?}");
}

/// **THE SAME EVENT REPLAYED STARTS NOTHING A SECOND TIME.** A command line
/// that calls its hook twice would otherwise do the work twice, and neither
/// run would know about the other.
#[test]
fn the_same_event_judged_again_starts_no_second_run() {
    let scratch = Scratch::new("replay");
    scratch.holding("watches", watching("UserPromptSubmit"));
    let store = scratch.store();
    let mut asked = Vec::new();

    for _ in 0..2 {
        evaluate(
            &store,
            9,
            &happened(),
            &scratch.sources(),
            100,
            &mut watching_starter(&mut asked),
            &mut never_parked,
        );
    }

    assert_eq!(asked.len(), 1, "one event, one run: {asked:?}");
    assert_eq!(
        store.verdicts_for(9).expect("the rows are readable").len(),
        1
    );
}

/// **EVERY REFUSAL LEAVES A ROW SAYING WHICH CONDITION REFUSED.** A guard that
/// declines in silence cannot be told from one that has stopped working.
#[test]
fn a_flow_held_back_says_which_condition_held_it() {
    let scratch = Scratch::new("deferred");
    scratch.holding("watches-another", watching("Stop"));
    let store = scratch.store();
    let mut asked = Vec::new();

    let verdicts = evaluate(
        &store,
        11,
        &happened(),
        &scratch.sources(),
        100,
        &mut watching_starter(&mut asked),
        &mut never_parked,
    );

    assert!(asked.is_empty());
    assert_eq!(verdicts.len(), 1);
    assert_eq!(verdicts[0].verdict, DEFERRED);
    assert_eq!(verdicts[0].why.as_deref(), Some("another event"));
    assert_eq!(
        store.verdicts_for(11).expect("the rows are readable").len(),
        1,
        "the refusal is on disk, not only in the answer"
    );
}

/// A flow that names the source and says nothing about which event would fire
/// on everything. It is held back, and told so.
#[test]
fn a_flow_that_names_no_event_is_held_back_instead_of_firing_on_everything() {
    let scratch = Scratch::new("silent");
    scratch.holding("watches-nothing", json!({"source": SESSION_EVENT}));
    let store = scratch.store();
    let mut asked = Vec::new();

    let verdicts = evaluate(
        &store,
        12,
        &happened(),
        &scratch.sources(),
        100,
        &mut watching_starter(&mut asked),
        &mut never_parked,
    );

    assert!(asked.is_empty(), "{asked:?}");
    assert_eq!(verdicts[0].verdict, DEFERRED);
    assert_eq!(
        verdicts[0].why.as_deref(),
        Some("the trigger says nothing about which event")
    );
}

/// **WHAT A PERSON TYPED DOES NOT TRAVEL.** The phrase decides the match and
/// stays in memory: passed on, it would enter an argument list, a process
/// table, and the ledger's own record of the child.
#[test]
fn the_delivery_carries_the_event_and_never_the_prompt() {
    let scratch = Scratch::new("delivery");
    scratch.holding("watches", watching("UserPromptSubmit"));
    let store = scratch.store();
    let mut asked = Vec::new();

    evaluate(
        &store,
        13,
        &happened(),
        &scratch.sources(),
        100,
        &mut watching_starter(&mut asked),
        &mut never_parked,
    );

    let delivery = &asked[0].1;
    assert!(delivery.contains("ttys001"), "{delivery}");
    assert!(
        !delivery.contains("carry on"),
        "the prompt must not leave this process: {delivery}"
    );
}
