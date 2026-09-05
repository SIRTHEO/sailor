//! A flow that looks after its tree leaves one line per closed run.
//!
//! **NOTHING FAKE IN THE MIDDLE**: the real executor finds the run past its
//! wall, the real header keeps the reason, the real store keeps the line, and
//! `history_ask` hands it back. Nothing is spent — no step opens at all.

use flow::{Executor, FlowFile, InProcessExecutor, SystemClock, WORKSPACE_ROOT};
use ledger::{self_care::SELF_CARE_LINES, Ledger};
use registry::{execution_request, record_flow_run, FlowRun, House};
use serde_json::{json, Value};
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
        "sailor-self-care-{label}-{}-{serial}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&path);
    std::fs::create_dir_all(&path).expect("a scratch directory");
    Scratch(path)
}

/// A flow with one step that would run a command, a wall of no seconds at all,
/// and the declaration that it looks after itself.
fn a_self_care_flow(wall_secs: Option<u64>, self_care: bool) -> FlowFile {
    let mut flow: FlowFile = serde_json::from_str(
        r#"{
            "id": "guarda-l-albero",
            "description": "one step behind a wall",
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
    .expect("a valid flow");
    flow.wall_secs = wall_secs;
    flow.self_care = self_care;
    flow
}

/// Runs the flow and writes its header the way both launchers do.
fn closed_run(ledger: &Ledger, flow: &FlowFile, run_id: &str, started_at: i64) -> &'static str {
    let actions = registry::registry_in(House::empty(), Some(ledger.clone()), None);
    record_flow_run(
        ledger,
        flow,
        FlowRun {
            run_id,
            status: "running",
            started_at,
            ended_at: None,
            error: None,
            started_by: "a test",
            stop_reason: None,
        },
    )
    .expect("the open header");
    let execution = InProcessExecutor
        .execute(
            &flow.graph,
            execution_request(Some(ledger), flow, run_id, None, started_at),
            ledger,
            &actions,
            &SystemClock,
        )
        .expect("the run ends without an error");
    let (status, _) = registry::execution_status(&execution);
    record_flow_run(
        ledger,
        flow,
        FlowRun {
            run_id,
            status,
            started_at,
            ended_at: Some(started_at + 1),
            error: registry::halted_by_hand(&execution),
            started_by: "a test",
            stop_reason: registry::how_it_stopped(&execution),
        },
    )
    .expect("the closed header");
    status
}

/// What `history_ask` answers a flow that asks for the lines.
fn asked_for_the_lines(ledger: &Ledger, flow: Option<&str>) -> Value {
    let actions = registry::registry_in(House::empty(), Some(ledger.clone()), None);
    let node = actions
        .get("history_ask")
        .expect("the history node is registered");
    let mut question = json!({"ask": "self_care_lines"});
    if let Some(flow) = flow {
        question["flow"] = json!(flow);
    }
    match node
        .execute(&question, &flow::SharedState::new())
        .expect("the question is answered")
    {
        flow::ActionOutcome::Went(answer) => answer,
        other => panic!("the history node answered {other:?}"),
    }
}

/// **THE PROOF.** A flow past its wall closes `stopped`, the ledger keeps
/// `wall` as the reason, and the line the run leaves reads `crash` — a run
/// that never reached its last step moved no metric.
#[test]
fn a_run_at_its_wall_closes_stopped_and_its_line_reads_crash() {
    let scratch = scratch("wall");
    let ledger = Ledger::open(&scratch.0).expect("a ledger");
    let flow = a_self_care_flow(Some(0), true);

    let status = closed_run(&ledger, &flow, "corsa-1", 1_000);

    assert_eq!(status, "stopped");
    let header = ledger
        .run_header("corsa-1")
        .expect("reading the header")
        .expect("the run is there");
    assert_eq!(header.stop_reason.as_deref(), Some("wall"));

    let answer = asked_for_the_lines(&ledger, Some("guarda-l-albero"));
    let lines = answer["answer"]["lines"]
        .as_array()
        .expect("the question answers with lines");
    assert_eq!(lines.len(), 1, "one closed run, one line: {answer}");
    assert_eq!(lines[0]["verdict"], "crash");
    assert_eq!(lines[0]["run_id"], "corsa-1");
    assert_eq!(lines[0]["flow"], "guarda-l-albero");
    assert_eq!(lines[0]["metric"], Value::Null);
    assert!(
        lines[0]["sentence"]
            .as_str()
            .is_some_and(|said| !said.is_empty()),
        "the reason speaks through the catalogue: {answer}"
    );
}

/// A run whose line is already there does not get a second.
///
/// **A ROW COUNT CANNOT SEE THIS**: the store is keyed, so a second write lands
/// on the same row and leaves one. What tells them apart is the instant the
/// line was written, which a rewrite moves — and the answer the store gives to
/// whoever asked it to write.
#[test]
fn the_line_of_one_run_is_written_once() {
    let scratch = scratch("once");
    let ledger = Ledger::open(&scratch.0).expect("a ledger");
    let flow = a_self_care_flow(Some(0), true);

    closed_run(&ledger, &flow, "corsa-1", 1_000);
    // The same close crossed a second time, as a resume of a closed run
    // crosses it, with a later instant so a rewrite would show.
    record_flow_run(
        &ledger,
        &flow,
        FlowRun {
            run_id: "corsa-1",
            status: "stopped",
            started_at: 1_000,
            ended_at: Some(9_999),
            error: None,
            started_by: "a test",
            stop_reason: Some(flow::StopReason::Wall),
        },
    )
    .expect("the header again");

    let held = ledger
        .records_in(SELF_CARE_LINES)
        .expect("reading the collection");
    assert_eq!(held.len(), 1, "one run leaves one line: {held:?}");
    assert_eq!(
        held[0].written_at, 1_001,
        "the line stays the one the first close wrote"
    );
    assert!(
        !ledger
            .write_self_care_line("corsa-1", None, None, "a test", 12_345)
            .expect("asking the store to write"),
        "and the store says it wrote nothing"
    );
}

/// A flow that does not say it looks after itself leaves nothing, so the
/// collection counts self-care runs and not runs.
#[test]
fn a_flow_that_does_not_look_after_itself_leaves_no_line() {
    let scratch = scratch("quiet");
    let ledger = Ledger::open(&scratch.0).expect("a ledger");
    let flow = a_self_care_flow(Some(0), false);

    closed_run(&ledger, &flow, "corsa-1", 1_000);

    assert!(ledger
        .records_in(SELF_CARE_LINES)
        .expect("reading the collection")
        .is_empty());
    let answer = asked_for_the_lines(&ledger, None);
    assert_eq!(answer["answer"]["lines"], json!([]));
}

/// The root a run works in is still written into the shared state of a walled
/// flow: adding a wall must not cost a run its workspace.
#[test]
fn a_walled_flow_still_carries_its_root() {
    let scratch = scratch("root");
    let ledger = Ledger::open(&scratch.0).expect("a ledger");
    let flow = a_self_care_flow(Some(900), true);

    let request = execution_request(Some(&ledger), &flow, "corsa-1", Some(&scratch.0), 1_000);

    assert!(request.shared.contains_key(WORKSPACE_ROOT));
    assert_eq!(request.stops.wall_deadline_at, Some(1_900));
}

/// A graph with no wall and no self-care is a request with nothing declared:
/// the flows that were here before this piece are untouched.
#[test]
fn a_flow_declaring_nothing_gets_a_request_with_nothing_declared() {
    let scratch = scratch("bare");
    let ledger = Ledger::open(&scratch.0).expect("a ledger");
    let flow = a_self_care_flow(None, false);

    let request = execution_request(Some(&ledger), &flow, "corsa-1", None, 1_000);

    assert_eq!(request.stops, flow::RunStops::default());
}
