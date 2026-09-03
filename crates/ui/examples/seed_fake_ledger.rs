//! Writes a fake ledger so the window can be looked at without waiting for a
//! real flow to run. Usage: `cargo run --example seed_fake_ledger -- DIRECTORY`.

use flow::{Completion, Outcome, StepRecord};
use ledger::{Ledger, ModelCallRecord, RunRecord};
use serde_json::json;

fn main() {
    let dir = std::env::args()
        .nth(1)
        .unwrap_or_else(|| panic!("usage: seed_fake_ledger DIRECTORY"));
    let ledger = Ledger::open(&dir).expect("opening the ledger");

    ledger
        .record_run(&RunRecord {
            run_id: "marker-sweep-demo-1".into(),
            kind: "sweep".into(),
            entity: "marker-sweep".into(),
            parent_run_id: None,
            started_by: "manual-test".into(),
            status: "succeeded".into(),
            total_cost_micros: 1_250_000,
            error: None,
            started_at: 1_756_000_000,
            ended_at: Some(1_756_000_042),
            worktree: Some("/t/un-progetto/un-albero".into()),
        })
        .expect("recording the finished run");
    close_step(
        &ledger,
        "marker-sweep-demo-1",
        "scan_markers",
        1_756_000_000,
        1_756_000_010,
    );
    close_step(
        &ledger,
        "marker-sweep-demo-1",
        "classify_standard",
        1_756_000_010,
        1_756_000_020,
    );
    close_step(
        &ledger,
        "marker-sweep-demo-1",
        "plan_removals",
        1_756_000_020,
        1_756_000_042,
    );
    ledger
        .record_model_call(&fake_call(
            "marker-sweep-demo-1",
            "classify_standard",
            "claude-sonnet-5",
            40_000,
            900,
        ))
        .expect("recording the call");

    ledger
        .record_run(&RunRecord {
            run_id: "marker-sweep-demo-2".into(),
            kind: "sweep".into(),
            entity: "marker-sweep".into(),
            parent_run_id: None,
            started_by: "manual-test".into(),
            status: "running".into(),
            total_cost_micros: 300_000,
            error: None,
            started_at: 1_756_000_100,
            ended_at: None,
            worktree: Some("/t/un-progetto/un-albero".into()),
        })
        .expect("recording the run still going");
    close_step(
        &ledger,
        "marker-sweep-demo-2",
        "scan_markers",
        1_756_000_100,
        1_756_000_105,
    );
    ledger
        .append_step_started(&StepRecord::started(
            "marker-sweep-demo-2",
            "classify_standard",
            1,
            1,
            vec!["scan_markers".into()],
            json!({}),
            vec![],
            1_756_000_105,
        ))
        .expect("step left open on purpose");
    ledger
        .record_model_call(&fake_call(
            "marker-sweep-demo-2",
            "classify_standard",
            "claude-haiku-5",
            12_000,
            220,
        ))
        .expect("recording the call");

    println!("fake ledger written to {dir}");
}

fn close_step(ledger: &Ledger, run_id: &str, step_id: &str, started_at: i64, ended_at: i64) {
    ledger
        .append_step_started(&StepRecord::started(
            run_id,
            step_id,
            1,
            1,
            vec![],
            json!({}),
            vec![],
            started_at,
        ))
        .expect("step started");
    ledger
        .close_step(
            run_id,
            step_id,
            1,
            1,
            Completion {
                outcome: Outcome::Went,
                output: Some(json!({"ok": true})),
                said: None,
                failure_class: None,
                ended_at,
                bytes_seen: None,
                bytes_discarded: None,
            },
        )
        .expect("step closed");
}

/// An **invented** call, marked as such on every row it produces.
///
/// **WHY THE MARKING IS MANDATORY.** A dashboard fed by fake data looks like it
/// works, and that is worse than having none — whoever reads it believes they
/// know what they spent. The real engine fills the ledger; this example stays
/// for trying the window out without spending, and says so on every row.
fn fake_call(
    run_id: &str,
    step_id: &str,
    model: &str,
    input_tokens: u64,
    cost_micros: i64,
) -> ModelCallRecord {
    // This example was once the only writer of `model_calls` besides the tests:
    // the dashboard summed a cost per model, and that cost was fiction entire.
    ModelCallRecord {
        call_id: format!("call-{run_id}-{step_id}"),
        run_id: run_id.to_owned(),
        step_id: Some(step_id.to_owned()),
        purpose: "FAKE — seeded by seed_fake_ledger, this is not a measurement".into(),
        cli: "claude".into(),
        requested_model: "sonnet".into(),
        actual_model: model.to_owned(),
        input_tokens: Some(input_tokens),
        output_tokens: Some(input_tokens / 8),
        cached_tokens: Some(input_tokens / 4),
        cache_write_tokens: None,
        cache_write_long_tokens: None,
        total_tokens: None,
        turns: None,
        cost_micros: Some(cost_micros),
        declared_cost_micros: None,
        price_currency: Some("USD".into()),
        input_price_micros_per_million: Some(3_000_000),
        output_price_micros_per_million: Some(15_000_000),
        cached_price_micros_per_million: Some(300_000),
        cache_write_price_micros_per_million: None,
        cache_write_long_price_micros_per_million: None,
        engine_identity: ledger::EngineIdentity::InheritedFromTheTerminal {
            cli_id: "claude".into(),
        },
        retry_chain: vec![],
        error_type: None,
        started_at: 1_756_000_001,
        ended_at: Some(1_756_000_009),
        session_id: None,
        work_kind: None,
    }
}
