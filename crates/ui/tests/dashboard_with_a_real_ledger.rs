//! End-to-end test: a real ledger in a temporary directory outside the tree,
//! fake runs, read through the pure pipeline. **THE HTTP SERVER IS GONE, ITS
//! QUESTIONS ARE NOT.** Three tests here opened a real socket on `127.0.0.1`;
//! what they defended was not the socket but that the counts reach whoever
//! looks **under the field names whoever looks reads**. Dropping them with the
//! transport would have dropped the check: that is how a rewrite loses pieces.

use flow::{Completion, Outcome, StepRecord};
use ledger::{Ledger, ModelCallRecord, RunRecord};
use std::path::{Path, PathBuf};

fn temp_dir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("ui-crate-test-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    dir
}

fn seed(dir: &Path) {
    let ledger = Ledger::open(dir).expect("opening the test ledger");
    ledger
        .record_run(&RunRecord {
            run_id: "run-1".into(),
            kind: "sweep".into(),
            entity: "marker-sweep".into(),
            parent_run_id: None,
            started_by: "test".into(),
            status: "running".into(),
            total_cost_micros: 0,
            error: None,
            started_at: 1000,
            ended_at: None,
            worktree: None,
            stop_reason: None,
        })
        .expect("recording the run");

    ledger
        .append_step_started(&StepRecord::started(
            "run-1",
            "scan_markers",
            1,
            1,
            vec![],
            serde_json::json!({}),
            vec![],
            1000,
        ))
        .expect("step started");
    ledger
        .close_step(
            "run-1",
            "scan_markers",
            1,
            1,
            Completion {
                outcome: Outcome::Went,
                output: Some(serde_json::json!({"ok": true})),
                said: None,
                failure_class: None,
                refusal: None,
                ran: None,
                ended_at: 1010,
                bytes_seen: None,
                bytes_discarded: None,
            },
        )
        .expect("step closed");

    ledger
        .append_step_started(&StepRecord::started(
            "run-1",
            "remove_markers",
            1,
            1,
            vec!["scan_markers".into()],
            serde_json::json!({}),
            vec![],
            1050,
        ))
        .expect("open step started and never closed: it is still running");

    ledger
        .record_model_call(&ModelCallRecord {
            call_id: "call-1".into(),
            run_id: "run-1".into(),
            step_id: Some("scan_markers".into()),
            purpose: "classify".into(),
            cli: "claude".into(),
            requested_model: "sonnet".into(),
            actual_model: "claude-sonnet-5".into(),
            input_tokens: Some(100),
            output_tokens: Some(50),
            cached_tokens: Some(10),
            cache_write_tokens: None,
            cache_write_long_tokens: None,
            total_tokens: None,
            turns: None,
            cost_micros: Some(500),
            declared_cost_micros: None,
            price_currency: Some("USD".into()),
            input_price_micros_per_million: Some(3_000_000),
            output_price_micros_per_million: Some(15_000_000),
            cached_price_micros_per_million: Some(300_000),
            cache_write_price_micros_per_million: None,
            cache_write_long_price_micros_per_million: None,
            engine_identity: ledger::EngineIdentity::ProfileInForce {
                cli_id: "claude".into(),
                profile_name: "test".into(),
                home_dir: "/homes/claude/test".into(),
                endpoint: None,
            },
            retry_chain: vec![],
            error_type: None,
            started_at: 1001,
            ended_at: Some(1009),
            session_id: None,
            work_kind: None,
        })
        .expect("recording the model call");
}

#[test]
fn gather_summarizes_a_seeded_ledger() {
    let dir = temp_dir("gather");
    seed(&dir);

    let data = ui::gather::gather(&dir)
        .expect("the read succeeded")
        .expect("the ledger just written is present");
    assert_eq!(data.runs.len(), 1);

    let executions =
        ui::dashboard::build_executions(&data.runs, &data.steps_by_run, &data.calls_by_run, 1100);
    let execution = &executions[0];
    assert_eq!(execution.run_id, "run-1");
    assert_eq!(execution.steps_total, 2);
    assert_eq!(execution.steps_went, 1);
    assert_eq!(execution.steps_open.len(), 1);
    assert_eq!(execution.steps_open[0].step_id, "remove_markers");
    assert_eq!(execution.steps_open[0].open_for_secs, 50);
    assert_eq!(execution.tokens.input_tokens, 100);
    assert_eq!(execution.tokens.cost_micros, 500);
    assert_eq!(
        execution
            .tokens_by_model
            .get("claude-sonnet-5")
            .expect("model present")
            .calls,
        1
    );

    let _ = std::fs::remove_dir_all(&dir);
}

/// **THE ANCHOR AGAINST A COLUMN MOVING IN SILENCE.** `ui::parse` reads the
/// projection **by position**: twenty-eight hand-written numbers. Comparing one
/// reader against another leaves both wrong together and both green, and the
/// real fault is exactly that — a price showing up where a token belongs, with
/// no red anywhere. So here **every field gets a different value**, goes through
/// the real ledger, and comes back: a misplaced index returns its neighbour.
#[test]
fn a_record_comes_back_from_the_projection_field_for_field() {
    // There were two copies of that reader, one here and one inside `actions`:
    // a moved column made them wrong together, and the tests that compared one
    // with the other would have stayed green. *Mutant run*: one index moved by
    // hand turns this test red whatever the column that shifted.
    let dir = temp_dir("round-trip");
    let written = ModelCallRecord {
        call_id: "call-only".into(),
        run_id: "run-only".into(),
        step_id: Some("step-only".into()),
        purpose: "purpose-only".into(),
        cli: "cli-only".into(),
        requested_model: "model-requested".into(),
        actual_model: "model-answered".into(),
        // Every count different from every other: two alike would cover for
        // each other in exactly the case this test exists to catch.
        input_tokens: Some(11),
        output_tokens: Some(22),
        cached_tokens: Some(33),
        cache_write_tokens: Some(44),
        cache_write_long_tokens: Some(55),
        total_tokens: Some(66),
        turns: Some(77),
        cost_micros: Some(101),
        declared_cost_micros: Some(202),
        price_currency: Some("EUR".into()),
        input_price_micros_per_million: Some(303),
        output_price_micros_per_million: Some(404),
        cached_price_micros_per_million: Some(505),
        cache_write_price_micros_per_million: Some(606),
        cache_write_long_price_micros_per_million: Some(707),
        engine_identity: ledger::EngineIdentity::ChosenByTheStep {
            cli_id: "codex".into(),
            home_dir: "/a/home/written/in/the/step".into(),
        },
        retry_chain: vec!["call-earlier".into()],
        error_type: Some("error-kind".into()),
        started_at: 808,
        ended_at: Some(909),
        session_id: Some("session-only".into()),
        work_kind: None,
    };

    {
        let ledger = Ledger::open(&dir).expect("opening the ledger");
        ledger
            .record_model_call(&written)
            .expect("recording the call");
    }

    let ledger = Ledger::open(&dir).expect("reopening the ledger");
    let dump = ledger.projection_dump().expect("reading the projection");
    let read = ui::parse::parse_model_calls(&dump);
    assert_eq!(read.len(), 1, "one row written, one row read");
    assert_eq!(
        read[0], written,
        "a field came back different from how it went in: an index is reading its neighbour's column"
    );

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_ledger_directory_that_was_never_written_is_reported_as_absent_not_as_an_error() {
    let dir = temp_dir("missing");
    let data = ui::gather::gather(&dir).expect("no error on a ledger that is absent");
    assert!(data.is_none());
    assert!(
        !dir.exists(),
        "reading the state must never create the ledger"
    );
}

#[test]
fn the_shape_the_window_reads_survives_serialization() {
    // THE FIELD NAMES ARE A CONTRACT, and it spans two languages: `ExecutionView`
    // here and `Execution` in `desktop/src/engine.ts`. Renaming one on this side
    // breaks nothing — the window reads `undefined` and draws an empty column.
    // This test is what turns that change red.
    let dir = temp_dir("shape");
    seed(&dir);
    let data = ui::gather::gather(&dir)
        .expect("the read succeeded")
        .expect("ledger present");
    let executions =
        ui::dashboard::build_executions(&data.runs, &data.steps_by_run, &data.calls_by_run, 1100);
    let body = serde_json::to_value(&executions).expect("the views serialize");

    assert_eq!(body[0]["run_id"], "run-1");
    assert_eq!(body[0]["tokens"]["input_tokens"].as_u64(), Some(100));
    assert_eq!(body[0]["steps_open"][0]["step_id"], "remove_markers");
    // The two figures that must stay side by side: the one Sailor computes and
    // the one the engine declares. Were either to vanish from the JSON, the
    // window would show an empty column instead of a disagreement.
    assert!(body[0]["calls"][0].get("cost_micros").is_some());
    assert!(body[0]["calls"][0].get("declared_cost_micros").is_some());
    // And what was never measured, which is the most important row of all.
    assert!(body[0]["tokens"].get("calls_without_tokens").is_some());
    assert!(body[0]["tokens"].get("calls_without_cost").is_some());

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_broken_flow_keeps_its_place_in_the_registry_with_its_reason() {
    // A BROKEN FLOW NEVER VANISHES. A list that shortens in silence leaves
    // people believing the flow does not exist, and nobody goes looking for a
    // file the list says is absent.
    let valid_graph = flow::Graph::new(vec![flow::Step {
        id: "step-1".into(),
        deps: vec![],
        input_schema: flow::ValueSchema::Any,
        output_schema: flow::ValueSchema::Any,
        with: None,
        when: None,
        action: "action-1".into(),
        max_attempts: 1,
        ask_again_after_secs: None,
        retry_after_secs: None,
        phase: None,
        stops_when: None,
    }])
    .expect("valid graph");
    let mut flows = ui::registry::FlowRegistry::new();
    flows.insert(
        "valid".into(),
        Ok(ui::registry::FlowFile {
            id: "valid".into(),
            description: "A valid test flow".into(),
            graph: valid_graph,
            inputs: std::collections::BTreeMap::new(),
            schedule: None,
            spend_cap_micros: None,
            wall_secs: None,
            max_turns: None,
            self_care: false,
        }),
    );
    flows.insert("broken".into(), Err("error: cycle in the graph".into()));

    let views =
        serde_json::to_value(ui::registry::flow_views(&flows)).expect("the views serialize");
    let array = views.as_array().expect("array of flows");
    assert_eq!(array.len(), 2);

    let broken = array
        .iter()
        .find(|entry| entry["name"] == "broken")
        .expect("broken flow present");
    assert_eq!(broken["error"], "error: cycle in the graph");
    assert_eq!(broken["steps"].as_array().map(|steps| steps.len()), Some(0));

    let valid = array
        .iter()
        .find(|entry| entry["name"] == "valid")
        .expect("valid flow present");
    assert_eq!(valid["error"], serde_json::Value::Null);
    assert_eq!(valid["description"], "A valid test flow");
    assert_eq!(valid["steps"].as_array().map(|steps| steps.len()), Some(1));
}
