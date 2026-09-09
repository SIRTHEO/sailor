//! A rejection writes the ending, and a resume has to agree with it. The two
//! halves were measured apart — the store keeps a halt request, the engine
//! stops on one — and apart they leave the seam nobody walked: whoever types
//! `sailor flow resume` after a refusal must be told the run is stopped, not
//! handed the next step.

use flow::{Completion, FlowFile, Outcome, StepRecord, StepSpecies};
use ledger::{Ledger, RunRecord};
use sailor::step_cmd::{decide_step_in, Verdict};
use serde_json::json;
use std::collections::BTreeMap;
use std::path::PathBuf;

struct Scratch(PathBuf);

impl Scratch {
    fn new(name: &str) -> Self {
        let path =
            std::env::temp_dir().join(format!("sailor-rejected-{name}-{}", std::process::id()));
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

fn a_flow() -> FlowFile {
    serde_json::from_str(
        r#"{
            "id": "una-decisione",
            "description": "one step handed to a person, and one that follows it",
            "graph": {
                "steps": [
                    {
                        "id": "decidi",
                        "deps": [],
                        "input_schema": {"type": "any"},
                        "output_schema": {"type": "any"},
                        "when": null,
                        "action": "handed_to_agent",
                        "max_attempts": 3
                    },
                    {
                        "id": "prosegui",
                        "deps": ["decidi"],
                        "input_schema": {"type": "any"},
                        "output_schema": {"type": "any"},
                        "when": null,
                        "action": "handed_to_agent",
                        "max_attempts": 3
                    }
                ]
            },
            "inputs": {}
        }"#,
    )
    .expect("the scratch flow is valid")
}

/// A store holding a run whose first step is waiting for a person, written the
/// way the engine writes it: opened, then closed with `Waiting`.
fn a_run_waiting_for_a_person(scratch: &Scratch) -> Ledger {
    let ledger = Ledger::open(&scratch.0).expect("the store opens");
    ledger
        .record_run(&RunRecord {
            run_id: "run-1".to_owned(),
            kind: "flow".to_owned(),
            entity: "una-decisione".to_owned(),
            parent_run_id: None,
            started_by: "a test".to_owned(),
            status: "waiting".to_owned(),
            total_cost_micros: 0,
            error: None,
            started_at: 100,
            ended_at: Some(150),
            worktree: None,
            stop_reason: None,
        })
        .expect("recording the run");
    let mut record = StepRecord::started(
        "run-1",
        "decidi",
        1,
        1,
        vec![],
        json!({
            "mandate": "read the output and say whether it stands",
            "holder": "whoever-is-watching",
            "handoff_timeout_secs": 3600
        }),
        vec![],
        110,
    );
    record.species = Some(StepSpecies::Repeatable);
    ledger
        .append_step_started(&record)
        .expect("opening the step");
    ledger
        .close_step(
            "run-1",
            "decidi",
            1,
            1,
            Completion {
                outcome: Outcome::Waiting,
                output: None,
                said: Some("handed over".to_owned()),
                failure_class: None,
                refusal: None,
                ran: None,
                ended_at: 150,
                bytes_seen: None,
                bytes_discarded: None,
            },
        )
        .expect("handing it over");
    ledger
}

fn options(pairs: &[(&str, &str)]) -> BTreeMap<String, String> {
    pairs
        .iter()
        .map(|(name, value)| ((*name).to_owned(), (*value).to_owned()))
        .collect()
}

fn resume(ledger: &Ledger, flow: &FlowFile) -> Result<String, String> {
    let mut store = ledger.clone();
    sailor::flow_cmd::resume_run_with(ledger, flow, "run-1", &mut store, None)
}

/// **THE ABSURD CONTROL, FIRST.** Approved, the same run resumes into the step
/// that was waiting on the decision. Without this, a resume that stops for any
/// reason at all would pass for a rejection honoured.
#[test]
fn an_approved_run_carries_on_into_the_next_step() {
    let scratch = Scratch::new("approvata");
    let ledger = a_run_waiting_for_a_person(&scratch);
    let flow = a_flow();
    // The step that follows demands a typed output, so an approval declares
    // one: the refusal that would otherwise fire is the close's, not the
    // decision's.
    let produced = scratch.0.join("output.json");
    std::fs::write(&produced, r#"{"verdict": "it stands"}"#).expect("writing the output");
    decide_step_in(
        &ledger,
        &flow,
        &options(&[
            ("run", "run-1"),
            ("step", "decidi"),
            ("as", "whoever-is-watching"),
            ("output-file", &produced.display().to_string()),
        ]),
        Verdict::Approved,
    )
    .expect("the step is approved");

    let outcome = resume(&ledger, &flow);
    let said = match &outcome {
        Ok(report) | Err(report) => report.clone(),
    };
    assert!(
        !said.contains("stopped"),
        "an approved run is not stopped: {said}"
    );
    let records = ledger.steps("run-1").expect("the store answers");
    assert!(
        records.iter().any(|record| record.step_id == "prosegui"),
        "the step waiting on the decision was reached: {said}"
    );
}

/// The seam itself: after a rejection, the resume says the run is stopped and
/// says why, instead of opening what comes next.
#[test]
fn a_resume_after_a_rejection_says_the_run_is_stopped() {
    let scratch = Scratch::new("rifiutata");
    let ledger = a_run_waiting_for_a_person(&scratch);
    let flow = a_flow();
    decide_step_in(
        &ledger,
        &flow,
        &options(&[
            ("run", "run-1"),
            ("step", "decidi"),
            ("as", "whoever-is-watching"),
            ("why", "the brief was answered with a guess"),
        ]),
        Verdict::Rejected,
    )
    .expect("the step is rejected");

    let said = resume(&ledger, &flow).expect_err("a stopped run does not report success");
    assert!(said.contains("stopped"), "{said}");
    let records = ledger.steps("run-1").expect("the store answers");
    assert!(
        !records.iter().any(|record| record.step_id == "prosegui"),
        "nothing after the rejected step may start: {said}"
    );
    let header = ledger
        .run_header("run-1")
        .expect("the store answers")
        .expect("the run has a header");
    assert_eq!(header.status, "stopped");
    assert_eq!(
        header.stop_reason.as_deref(),
        Some("by_hand"),
        "the ending is a person's, and it is written as one"
    );
}
