//! A run keeps the definition it executed, not only the name of a file.
//!
//! **NOTHING FAKE IN THE MIDDLE**: the real header writer both launchers call,
//! the real store, and the definition read back through `FlowFile` itself.
//! **THE MUTANT**: take the `record_what_it_executed` line out of `write_run`.
//! The header still lands and every other proof stays green.

use flow::FlowFile;
use ledger::{FlowDefinitionRecord, Ledger, RunFlow};
use registry::{record_flow_run, FlowRun};
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

static NEXT: AtomicU64 = AtomicU64::new(0);

struct Scratch(PathBuf);

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn scratch(label: &str) -> Scratch {
    let serial = NEXT.fetch_add(1, Ordering::Relaxed);
    let path = std::env::temp_dir().join(format!(
        "sailor-kept-flow-{label}-{}-{serial}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&path);
    std::fs::create_dir_all(&path).expect("a scratch directory");
    Scratch(path)
}

fn a_flow() -> FlowFile {
    serde_json::from_str(
        r#"{
            "id": "misura-l-albero",
            "description": "one step that runs a command",
            "graph": {"steps": [{
                "id": "misura",
                "deps": [],
                "input_schema": {"type": "any"},
                "output_schema": {"type": "any"},
                "when": null,
                "action": "shell_check",
                "max_attempts": 1
            }]},
            "inputs": {"misura": {"command": "true", "env": {}, "timeout_secs": 5}}
        }"#,
    )
    .expect("a valid flow")
}

fn header(ledger: &Ledger, flow: &FlowFile, run_id: &str, status: &str, ended_at: Option<i64>) {
    record_flow_run(
        ledger,
        flow,
        FlowRun {
            run_id,
            status,
            started_at: 1000,
            ended_at,
            error: None,
            started_by: "a test",
            stop_reason: None,
        },
    )
    .expect("the header");
}

#[test]
fn a_run_can_say_what_it_executed_after_the_file_is_gone() {
    let home = scratch("kept");
    let ledger = Ledger::open(home.0.join("ledger")).expect("open the ledger");
    let flow = a_flow();
    header(&ledger, &flow, "run-1", "running", None);

    let RunFlow::Recorded(kept) = ledger.flow_of_run("run-1").expect("ask what it ran") else {
        panic!("the run that just started cannot say what it executed");
    };
    assert_eq!(kept.len(), 1);
    let read_back: FlowFile =
        serde_json::from_value(kept[0].definition().expect("the body")).expect("a whole flow");
    assert_eq!(
        read_back, flow,
        "what came back is not the flow the run was handed"
    );
    assert_eq!(
        kept[0].digest,
        FlowDefinitionRecord::of_flow("run-1", &flow, 0)
            .expect("digest the flow")
            .digest
    );
}

/// Opening and closing write the header twice, and the definition is the same
/// bytes both times: the store keeps one row and the run has one answer.
#[test]
fn opening_and_closing_a_run_leaves_one_definition() {
    let home = scratch("twice");
    let ledger = Ledger::open(home.0.join("ledger")).expect("open the ledger");
    let flow = a_flow();
    header(&ledger, &flow, "run-1", "running", None);
    header(&ledger, &flow, "run-1", "complete", Some(2000));

    let RunFlow::Recorded(kept) = ledger.flow_of_run("run-1").expect("ask") else {
        panic!("unknown after two headers");
    };
    assert_eq!(kept.len(), 1);
    assert_eq!(ledger.runs_of_unknown_flow().expect("count"), 0);
    // A definition weighs tens of kilobytes: the second header must not append
    // it to the log a second time.
    assert_eq!(
        ledger
            .events_of_kind("flow_definition_recorded")
            .expect("count the events"),
        1
    );
}
