//! **THE SAME ROUTE `close_handed_step` TAKES, AND THE RESUME ITS THREAD RUNS.**
//! Compares what each route leaves on identical ledgers: the step row, the run
//! header, and the work the handoff was holding back.

use flow::{Completion, FlowFile, Outcome, StepRecord, StepSpecies};
use ledger::{Ledger, RunRecord};
use sailor::step_cmd::{close_step_in, flow_of_run, open_step_in};
use serde_json::json;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

struct Scratch(PathBuf);

impl Scratch {
    fn new(name: &str) -> Self {
        let path =
            std::env::temp_dir().join(format!("sailor-handed-close-{name}-{}", std::process::id()));
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
            "id": "un-controllo",
            "description": "one step handed to a person, and the record it unblocks",
            "graph": {
                "steps": [
                    {
                        "id": "review",
                        "deps": [],
                        "input_schema": {"type": "any"},
                        "output_schema": {"type": "any"},
                        "when": null,
                        "action": "handed_to_agent",
                        "max_attempts": 3
                    },
                    {
                        "id": "record",
                        "deps": ["review"],
                        "input_schema": {"type": "any"},
                        "output_schema": {"type": "any"},
                        "when": null,
                        "action": "store_write",
                        "with": {
                            "collection": "verdicts",
                            "key": "run-1",
                            "value": {"$from": "/verdict"},
                            "written_by": "un-controllo"
                        },
                        "max_attempts": 1
                    }
                ]
            },
            "inputs": {}
        }"#,
    )
    .expect("the scratch flow is valid")
}

/// Writes the flow where `flow_of_run`/`SAILOR_FLOWS` reads it from disk.
fn write_flow(scratch: &Scratch, flow: &FlowFile) -> PathBuf {
    let flows_dir = scratch.0.join("flows");
    std::fs::create_dir_all(&flows_dir).expect("the scratch flows directory");
    std::fs::write(
        flows_dir.join(format!("{}.flow.json", flow.id)),
        serde_json::to_string(flow).expect("the flow serialises"),
    )
    .expect("writing the flow file");
    flows_dir
}

/// A store holding a run whose only step is waiting for a person, written the
/// way the engine writes a handoff: opened, then closed with `Waiting`.
fn a_run_waiting_for_a_person(scratch: &Scratch) -> Ledger {
    let ledger = Ledger::open(&scratch.0).expect("the store opens");
    ledger
        .record_run(&RunRecord {
            run_id: "run-1".to_owned(),
            kind: "flow".to_owned(),
            entity: "un-controllo".to_owned(),
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
        "review",
        1,
        1,
        vec![],
        json!({
            "mandate": "read the diff and say whether it holds",
            "holder": "mira",
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
            "review",
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

/// The latest record of a step, however it went.
fn latest<'a>(records: &'a [StepRecord], step_id: &str) -> &'a StepRecord {
    records
        .iter()
        .filter(|record| record.step_id == step_id)
        .max_by_key(|record| (record.attempt, record.epoch))
        .expect("the step has a record")
}

/// The variables below are process-global, and the tests of this file run
/// together: each one holds this while it has them set.
static THE_ENVIRONMENT: std::sync::Mutex<()> = std::sync::Mutex::new(());

/// Sets `SAILOR_FLOWS` for the body, then restores whatever it was.
fn with_flows_dir<T>(flows_dir: &Path, body: impl FnOnce() -> T) -> T {
    let _held = THE_ENVIRONMENT.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    let previous = std::env::var("SAILOR_FLOWS").ok();
    std::env::set_var("SAILOR_FLOWS", flows_dir);
    let result = body();
    match previous {
        Some(value) => std::env::set_var("SAILOR_FLOWS", value),
        None => std::env::remove_var("SAILOR_FLOWS"),
    }
    result
}

/// The verdict the person hands back, as a file the way a terminal passes it.
fn a_verdict(scratch: &Scratch) -> PathBuf {
    let path = scratch.0.join("verdict.json");
    std::fs::write(&path, r#"{"verdict": "approved"}"#).expect("writing the verdict");
    path
}

/// **THE WINDOW'S ROUTE.** `close_handed_step` closes, then `crate::run::resume`
/// runs `resume_run_with` on a thread; the handle and the registry of runs
/// around it need a live window, the resume itself does not.
fn close_as_the_window_does(scratch: &Scratch, ledger: &Ledger, flows_dir: &Path) {
    let verdict = a_verdict(scratch);
    open_step_in(ledger, &options(&[("run", "run-1"), ("step", "review"), ("as", "mira")]))
        .expect("the window takes it on");
    with_flows_dir(flows_dir, || {
        let flow = flow_of_run(ledger, "run-1").expect("the window finds the flow the run declares");
        close_step_in(
            ledger,
            &flow,
            &options(&[
                ("run", "run-1"),
                ("step", "review"),
                ("as", "mira"),
                ("outcome", "went"),
                ("said", "it holds"),
                ("output-file", verdict.to_str().expect("a readable path")),
            ]),
        )
        .expect("the window closes it");
        let mut store = ledger.clone();
        sailor::flow_cmd::resume_run_with(ledger, &flow, "run-1", &mut store, None)
            .expect("the window resumes it");
    });
}

/// **THE COMMAND LINE'S ROUTE**: the flags a terminal types, one line each,
/// `SAILOR_LEDGER` declaring the store as the binary reads it. The exit codes.
fn on_the_command_line(scratch: &Scratch, flows_dir: &Path, lines: &[&[&str]]) -> Vec<i32> {
    with_flows_dir(flows_dir, || {
        let previous_ledger = std::env::var("SAILOR_LEDGER").ok();
        std::env::set_var("SAILOR_LEDGER", &scratch.0);
        let codes = lines
            .iter()
            .map(|line| {
                let args: Vec<String> = line.iter().map(|word| (*word).to_owned()).collect();
                sailor::step_cmd::run(&args)
            })
            .collect();
        match previous_ledger {
            Some(value) => std::env::set_var("SAILOR_LEDGER", value),
            None => std::env::remove_var("SAILOR_LEDGER"),
        }
        codes
    })
}

const TAKE_IT: &[&str] = &["open", "--run", "run-1", "--step", "review", "--as", "mira"];

fn close_as_the_command_line_does(scratch: &Scratch, flows_dir: &Path) {
    let verdict = a_verdict(scratch).display().to_string();
    let codes = on_the_command_line(
        scratch,
        flows_dir,
        &[
            TAKE_IT,
            &[
                "close", "--run", "run-1", "--step", "review", "--as", "mira", "--outcome",
                "went", "--said", "it holds", "--output-file", &verdict,
            ],
        ],
    );
    assert_eq!(codes, [0, 0], "the command line opens the step, then closes it");
}

fn status_of(ledger: &Ledger) -> String {
    ledger
        .run_header("run-1")
        .expect("the store answers")
        .expect("the run is recorded")
        .status
}

fn recorded(ledger: &Ledger) -> Option<serde_json::Value> {
    ledger
        .read_record("verdicts", "run-1")
        .expect("the store answers")
        .map(|record| record.value)
}

#[test]
fn the_window_and_the_command_line_write_the_same_row() {
    let flow = a_flow();

    let by_window = Scratch::new("by-window");
    let flows_window = write_flow(&by_window, &flow);
    let ledger_window = a_run_waiting_for_a_person(&by_window);
    close_as_the_window_does(&by_window, &ledger_window, &flows_window);
    let records_window = ledger_window.steps("run-1").expect("the store answers");
    let window_record = latest(&records_window, "review");

    let by_cli = Scratch::new("by-cli");
    let flows_cli = write_flow(&by_cli, &flow);
    let ledger_cli = a_run_waiting_for_a_person(&by_cli);
    close_as_the_command_line_does(&by_cli, &flows_cli);
    let records_cli = ledger_cli.steps("run-1").expect("the store answers");
    let cli_record = latest(&records_cli, "review");

    assert_eq!(window_record.outcome, Some(Outcome::Went));
    assert_eq!(window_record.outcome, cli_record.outcome);
    assert_eq!(window_record.taken_on_by, cli_record.taken_on_by);
    assert_eq!(window_record.attempt, cli_record.attempt);
    assert_eq!(window_record.said, cli_record.said);
    assert_eq!(window_record.said.as_deref(), Some("it holds"));

    assert_eq!(
        status_of(&ledger_cli),
        status_of(&ledger_window),
        "a close from the command line leaves the run where the window's close does"
    );
    assert_eq!(status_of(&ledger_window), "complete");
    assert_eq!(recorded(&ledger_cli), recorded(&ledger_window));
    assert_eq!(recorded(&ledger_cli), Some(json!("approved")));
}

/// An approval is a close with a verdict, and it moves the run the same way.
#[test]
fn an_approval_from_the_command_line_moves_its_run_too() {
    let scratch = Scratch::new("approved");
    let flows_dir = write_flow(&scratch, &a_flow());
    let ledger = a_run_waiting_for_a_person(&scratch);
    let verdict = a_verdict(&scratch).display().to_string();
    let codes = on_the_command_line(
        &scratch,
        &flows_dir,
        &[&[
            "approve", "--run", "run-1", "--step", "review", "--as", "mira", "--output-file",
            &verdict,
        ]],
    );
    assert_eq!(codes, [0], "the command line approves the step");
    assert_eq!(status_of(&ledger), "complete", "the approval left its run parked");
    assert_eq!(recorded(&ledger), Some(json!("approved")));
}

/// **ONLY A RUN PARKED ON A PERSON IS RESUMED.** A close on a run the header
/// already calls ended writes the step and leaves the run where it was: a
/// close is not the door that brings a failed run back.
#[test]
fn a_close_does_not_reopen_a_run_that_ended() {
    let scratch = Scratch::new("ended");
    let flows_dir = write_flow(&scratch, &a_flow());
    let ledger = a_run_waiting_for_a_person(&scratch);
    let header = ledger.run_header("run-1").expect("the store answers").expect("the run");
    ledger
        .record_run(&RunRecord {
            status: "failed".to_owned(),
            ..header
        })
        .expect("ending the run");
    close_as_the_command_line_does(&scratch, &flows_dir);
    assert_eq!(latest(&ledger.steps("run-1").expect("the store answers"), "review").outcome, Some(Outcome::Went));
    assert_eq!(status_of(&ledger), "failed", "a close brought an ended run back");
    assert_eq!(recorded(&ledger), None, "the step after the handoff ran on an ended run");
}

/// A child run is its parent's to resume: resumed alone it would run without
/// the cap and the wall the parent hands it, and the parent would never learn.
#[test]
fn a_close_leaves_a_child_run_to_its_parent() {
    let scratch = Scratch::new("child");
    let flows_dir = write_flow(&scratch, &a_flow());
    let ledger = a_run_waiting_for_a_person(&scratch);
    let header = ledger.run_header("run-1").expect("the store answers").expect("the run");
    ledger
        .record_run(&RunRecord {
            parent_run_id: Some("the-parent".to_owned()),
            ..header
        })
        .expect("making it a child");
    close_as_the_command_line_does(&scratch, &flows_dir);
    assert_eq!(status_of(&ledger), "waiting", "a close resumed a child on its own");
    assert_eq!(recorded(&ledger), None);
}

/// **THE CLOSE IS WRITTEN WHATEVER THE RESUME COMES TO.** A step handed back
/// broken fails this run on the resume; the command still succeeds, since
/// a failed exit would send whoever closed it to close it again.
#[test]
fn a_close_succeeds_whatever_the_resume_comes_to() {
    let scratch = Scratch::new("broke");
    let flows_dir = write_flow(&scratch, &a_flow());
    let ledger = a_run_waiting_for_a_person(&scratch);
    let codes = on_the_command_line(
        &scratch,
        &flows_dir,
        &[
            TAKE_IT,
            &["close", "--run", "run-1", "--step", "review", "--as", "mira", "--outcome", "broke"],
        ],
    );
    assert_eq!(codes, [0, 0], "the close was written, and said it failed");
    assert_eq!(status_of(&ledger), "failed", "the resume did not run");
}
