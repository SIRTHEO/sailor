//! A handed step is closed `Waiting`, so the resume's pass over open records
//! never saw it and the deadline it declares governed nothing. These tests
//! hold the three ends a wait can have on a resume, and the beat that asks for
//! the resume when nobody comes.

use flow::{Completion, FlowFile, Outcome, StepRecord, StepSpecies, HANDOFF_EXPIRED};
use ledger::{Ledger, RunRecord};
use serde_json::json;
use std::path::PathBuf;

struct Scratch(PathBuf);

impl Scratch {
    fn new(name: &str) -> Self {
        let path =
            std::env::temp_dir().join(format!("sailor-lapsed-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&path);
        std::fs::create_dir_all(&path).expect("the scratch directory");
        Scratch(path)
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

const HANDED_AT: i64 = 1_000;
const IN_TIME: i64 = 4_000_000_000;
const LAPSED: i64 = 3_600;

fn the_handover(deadline_secs: i64) -> serde_json::Value {
    json!({
        "mandate": "read the diff and say whether it holds",
        "holder": "mira",
        "handoff_timeout_secs": deadline_secs
    })
}

fn a_flow(deadline_secs: i64, max_attempts: u32) -> FlowFile {
    serde_json::from_value(json!({
        "id": "un-controllo",
        "description": "one step handed to a person",
        "graph": {"steps": [{
            "id": "review",
            "deps": [],
            "input_schema": {"type": "any"},
            "output_schema": {"type": "any"},
            "when": null,
            "action": "handed_to_agent",
            "with": the_handover(deadline_secs),
            "max_attempts": max_attempts
        }]},
        "inputs": {}
    }))
    .expect("the scratch flow is valid")
}

/// A run parked on its handed step, written the way the engine writes it.
fn a_run_waiting(ledger: &Ledger, run_id: &str, deadline_secs: i64) {
    ledger
        .record_run(&RunRecord {
            run_id: run_id.to_owned(),
            kind: "flow".to_owned(),
            entity: "un-controllo".to_owned(),
            parent_run_id: None,
            started_by: "a test".to_owned(),
            status: "waiting".to_owned(),
            total_cost_micros: 0,
            error: None,
            started_at: HANDED_AT,
            ended_at: Some(HANDED_AT),
            worktree: None,
            stop_reason: None,
        })
        .expect("recording the run");
    let mut record = StepRecord::started(
        run_id,
        "review",
        1,
        1,
        vec![],
        the_handover(deadline_secs),
        vec![],
        HANDED_AT,
    );
    record.species = Some(StepSpecies::Repeatable);
    ledger
        .append_step_started(&record)
        .expect("opening the step");
    ledger
        .close_step(run_id, "review", 1, 1, ended(Outcome::Waiting, HANDED_AT))
        .expect("handing it over");
}

/// A run that stopped on a step answering «not yet», which is the other half
/// of what the beat asks again and a different question to the store.
fn a_run_that_answered_not_yet(ledger: &Ledger, run_id: &str) {
    ledger
        .record_run(&RunRecord {
            run_id: run_id.to_owned(),
            kind: "flow".to_owned(),
            entity: "un-controllo".to_owned(),
            parent_run_id: None,
            started_by: "a test".to_owned(),
            status: "not_yet".to_owned(),
            total_cost_micros: 0,
            error: None,
            started_at: HANDED_AT,
            ended_at: Some(HANDED_AT),
            worktree: None,
            stop_reason: None,
        })
        .expect("recording the run");
}

fn ended(outcome: Outcome, at: i64) -> Completion {
    Completion {
        outcome,
        output: None,
        said: None,
        failure_class: None,
        refusal: None,
        ran: None,
        ended_at: at,
        bytes_seen: None,
        bytes_discarded: None,
    }
}

fn resume(ledger: &Ledger, flow: &FlowFile, run_id: &str) -> Result<String, String> {
    let mut store = ledger.clone();
    sailor::flow_cmd::resume_run_with(ledger, flow, run_id, &mut store, None)
}

/// What the resume printed, whichever way it ended: a run left waiting and a
/// run that failed both come back as an error, and a person reads the report.
fn the_report(ended: Result<String, String>) -> String {
    ended.unwrap_or_else(|said| said)
}

fn status_of(ledger: &Ledger, run_id: &str) -> String {
    ledger
        .run_header(run_id)
        .expect("the store answers")
        .expect("the run is recorded")
        .status
}

/// Outcome and failure class of every attempt, in order.
fn attempts(ledger: &Ledger, run_id: &str) -> Vec<(u32, Option<Outcome>, Option<String>)> {
    let mut found: Vec<_> = ledger
        .steps(run_id)
        .expect("the store answers")
        .into_iter()
        .map(|record| (record.attempt, record.outcome, record.failure_class))
        .collect();
    found.sort_by_key(|(attempt, _, _)| *attempt);
    found
}

/// Past its deadline, with no attempt left, the wait ends the run as failed
/// and says why on an attempt of its own.
#[test]
fn a_handover_past_its_deadline_ends_the_run_it_held() {
    let scratch = Scratch::new("ends");
    let ledger = Ledger::open(&scratch.0).expect("the store opens");
    a_run_waiting(&ledger, "run-1", LAPSED);

    let _ = resume(&ledger, &a_flow(LAPSED, 1), "run-1");

    assert_eq!(
        attempts(&ledger, "run-1"),
        [
            (1, Some(Outcome::Waiting), None),
            (2, Some(Outcome::Broke), Some(HANDOFF_EXPIRED.to_owned())),
        ],
        "the lapsed wait left no trace of its own"
    );
    assert_eq!(status_of(&ledger, "run-1"), "failed");
}

/// With attempts left, the lapsed wait is offered again with a fresh deadline,
/// and the run goes back to waiting on it.
#[test]
fn a_lapsed_handover_with_attempts_left_is_offered_again() {
    let scratch = Scratch::new("again");
    let ledger = Ledger::open(&scratch.0).expect("the store opens");
    a_run_waiting(&ledger, "run-1", LAPSED);

    let _ = resume(&ledger, &a_flow(LAPSED, 3), "run-1");

    assert_eq!(
        attempts(&ledger, "run-1"),
        [
            (1, Some(Outcome::Waiting), None),
            (2, Some(Outcome::Broke), Some(HANDOFF_EXPIRED.to_owned())),
            (3, Some(Outcome::Waiting), None),
        ]
    );
    assert_eq!(status_of(&ledger, "run-1"), "waiting");
}

/// **THE CONTROL.** A wait still in time is the person's: the resume writes no
/// attempt, and the run stays where it was.
#[test]
fn a_handover_in_time_is_left_to_the_person() {
    let scratch = Scratch::new("in-time");
    let ledger = Ledger::open(&scratch.0).expect("the store opens");
    a_run_waiting(&ledger, "run-1", IN_TIME);

    let _ = resume(&ledger, &a_flow(IN_TIME, 1), "run-1");

    assert_eq!(
        attempts(&ledger, "run-1"),
        [(1, Some(Outcome::Waiting), None)]
    );
    assert_eq!(status_of(&ledger, "run-1"), "waiting");
}

/// The beat resumes a run whose handover lapsed, and only that one: a wait in
/// time and a wait already answered are left to the person, a child to its
/// parent.
#[test]
fn the_beat_resumes_only_the_runs_whose_handover_lapsed() {
    let scratch = Scratch::new("beat");
    let ledger = Ledger::open(&scratch.0).expect("the store opens");
    a_run_waiting(&ledger, "lapsed", LAPSED);
    a_run_waiting(&ledger, "in-time", IN_TIME);
    a_run_waiting(&ledger, "answered", LAPSED);
    a_run_waiting(&ledger, "a-child", LAPSED);
    let child = ledger
        .run_header("a-child")
        .expect("the store answers")
        .expect("the run");
    ledger
        .record_run(&RunRecord {
            parent_run_id: Some("its-parent".to_owned()),
            ..child
        })
        .expect("making it a child");
    let mut taken = StepRecord::started(
        "answered",
        "review",
        2,
        2,
        vec![],
        the_handover(LAPSED),
        vec![],
        HANDED_AT + 10,
    );
    taken.taken_on_by = Some("mira".to_owned());
    ledger.append_step_started(&taken).expect("taking it on");
    ledger
        .close_step(
            "answered",
            "review",
            2,
            2,
            ended(Outcome::Went, HANDED_AT + 20),
        )
        .expect("answering it");

    let mut resumed = Vec::new();
    let mut resume = |run_id: &str| {
        resumed.push(run_id.to_owned());
        Ok(String::new())
    };
    let (said, woken, _) =
        sailor::flow_cmd::beat::ask_the_parked_again(&[], &ledger, HANDED_AT + LAPSED, &mut resume);

    assert_eq!(resumed, ["lapsed"], "{said}");
    assert_eq!(woken, 1);
}

/// **THE TWO PASSES DO NOT SHARE A RULE.** No flow here starts by itself, so
/// the run parked on «not yet» is let go: the switch a person threw on a flow
/// is what decides that one. The lapsed handover is woken all the same — it
/// answers to the deadline its own step declared, and the resume ends a wait
/// rather than starting again the work the switch was thrown against.
#[test]
fn the_switch_on_a_flow_governs_the_parked_and_not_the_lapsed() {
    let scratch = Scratch::new("two-passes");
    let ledger = Ledger::open(&scratch.0).expect("the store opens");
    a_run_waiting(&ledger, "lapsed", LAPSED);
    a_run_that_answered_not_yet(&ledger, "not-yet");

    let mut resumed = Vec::new();
    let mut resume = |run_id: &str| {
        resumed.push(run_id.to_owned());
        Ok(String::new())
    };
    let (said, woken, let_go) =
        sailor::flow_cmd::beat::ask_the_parked_again(&[], &ledger, HANDED_AT + LAPSED, &mut resume);

    assert_eq!(resumed, ["lapsed"], "{said}");
    assert_eq!((woken, let_go), (1, 1), "{said}");
    assert_eq!(status_of(&ledger, "not-yet"), "stopped", "{said}");
}

/// **THE LINE A PERSON READS SAYS WHAT THE RESUME DID.** A lapsed wait was
/// reported as put back among the ready, and every handed step that ships
/// declares one attempt, so the next line said `failed`. Both sentences are
/// asked of the catalogue here, not copied: a report held against a copy would
/// stay green the day the catalogue says something else.
#[test]
fn the_report_of_a_lapsed_handover_says_what_became_of_the_run() {
    let scratch = Scratch::new("report");
    let ledger = Ledger::open(&scratch.0).expect("the store opens");
    a_run_waiting(&ledger, "no-attempt-left", LAPSED);
    a_run_waiting(&ledger, "an-attempt-left", LAPSED);

    let lapsed = catalogue::say("cli.flow.nobody_took_the_handover", &[("steps", "review")]);
    let ready_again = catalogue::say(
        "cli.flow.expired_back_among_the_ready",
        &[("steps", "review")],
    );
    assert_ne!(
        lapsed, ready_again,
        "one sentence: this test cannot tell them apart"
    );

    let failed = the_report(resume(&ledger, &a_flow(LAPSED, 1), "no-attempt-left"));
    let offered = the_report(resume(&ledger, &a_flow(LAPSED, 3), "an-attempt-left"));

    for said in [&failed, &offered] {
        assert!(
            said.contains(&lapsed),
            "the report does not name the lapse: {said}"
        );
        assert!(
            !said.contains(&ready_again),
            "the wait was not put back among the ready: {said}"
        );
    }
    assert!(
        failed.contains(&catalogue::say(
            "cli.flow.run_status",
            &[("status", "failed")]
        )),
        "{failed}"
    );
    assert!(
        offered.contains(&catalogue::say(
            "cli.flow.run_status",
            &[("status", "waiting")]
        )),
        "{offered}"
    );
}
