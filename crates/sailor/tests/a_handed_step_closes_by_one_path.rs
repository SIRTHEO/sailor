//! **THE SAME ROUTE `close_handed_step` TAKES, MINUS THE APP-HANDLE RESUME.**
//! Compares the row each route writes on identical ledgers.

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
            "description": "one step handed to a person, and nothing after it",
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

/// Sets `SAILOR_FLOWS` for the body, then restores whatever it was. The
/// variable is process-global; this file has one test, so nothing races it.
fn with_flows_dir<T>(flows_dir: &Path, body: impl FnOnce() -> T) -> T {
    let previous = std::env::var("SAILOR_FLOWS").ok();
    std::env::set_var("SAILOR_FLOWS", flows_dir);
    let result = body();
    match previous {
        Some(value) => std::env::set_var("SAILOR_FLOWS", value),
        None => std::env::remove_var("SAILOR_FLOWS"),
    }
    result
}

/// **THE WINDOW'S ROUTE, minus the resume.** `close_handed_step` also calls
/// `crate::run::resume`, which needs a live `AppHandle`/`State<Runs>` a plain test cannot build.
fn close_as_the_window_does(ledger: &Ledger, flows_dir: &Path) {
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
            ]),
        )
        .expect("the window closes it");
    });
}

/// **THE COMMAND LINE'S ROUTE**: the same `--run`/`--step`/`--as` flags a
/// terminal types, `SAILOR_LEDGER` declaring the store as the binary reads it.
fn close_as_the_command_line_does(scratch: &Scratch, flows_dir: &Path) {
    let previous_ledger = std::env::var("SAILOR_LEDGER").ok();
    std::env::set_var("SAILOR_LEDGER", &scratch.0);
    let (opened, closed) = with_flows_dir(flows_dir, || {
        let opened = sailor::step_cmd::run(&[
            "open".to_owned(),
            "--run".to_owned(),
            "run-1".to_owned(),
            "--step".to_owned(),
            "review".to_owned(),
            "--as".to_owned(),
            "mira".to_owned(),
        ]);
        let closed = sailor::step_cmd::run(&[
            "close".to_owned(),
            "--run".to_owned(),
            "run-1".to_owned(),
            "--step".to_owned(),
            "review".to_owned(),
            "--as".to_owned(),
            "mira".to_owned(),
            "--outcome".to_owned(),
            "went".to_owned(),
            "--said".to_owned(),
            "it holds".to_owned(),
        ]);
        (opened, closed)
    });
    match previous_ledger {
        Some(value) => std::env::set_var("SAILOR_LEDGER", value),
        None => std::env::remove_var("SAILOR_LEDGER"),
    }
    assert_eq!(opened, 0, "the command line opens the step");
    assert_eq!(closed, 0, "the command line closes the step");
}

#[test]
fn the_window_and_the_command_line_write_the_same_row() {
    let flow = a_flow();

    let by_window = Scratch::new("by-window");
    let flows_window = write_flow(&by_window, &flow);
    let ledger_window = a_run_waiting_for_a_person(&by_window);
    close_as_the_window_does(&ledger_window, &flows_window);
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
}
