use super::*;
use serde_json::json;
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};
use std::thread;
use std::time::{Duration, Instant};
use tracing_subscriber::prelude::*;

static NEXT_DIRECTORY: AtomicU64 = AtomicU64::new(0);

struct TestDirectory(PathBuf);

impl TestDirectory {
    fn new(label: &str) -> Self {
        let sequence = NEXT_DIRECTORY.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!(
            "sailor-ledger-{label}-{}-{sequence}",
            std::process::id()
        ));
        std::fs::create_dir_all(&path).expect("create the test directory");
        Self(path)
    }
}

impl Drop for TestDirectory {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn started(run_id: &str) -> StepRecord {
    started_attempt(run_id, 1, 7)
}

fn started_attempt(run_id: &str, attempt: u32, epoch: u64) -> StepRecord {
    StepRecord::started(
        run_id,
        "compile",
        attempt,
        epoch,
        vec!["prepare".to_owned()],
        json!({"source": "lib.rs", "options": ["--all"]}),
        vec!["filesystem".to_owned(), "network".to_owned()],
        100,
    )
}

fn completion() -> Completion {
    Completion {
        outcome: Outcome::Broke,
        output: Some(json!({"code": 101})),
        said: Some("raw error text".to_owned()),
        failure_class: Some("compiler_error".to_owned()),
        refusal: None,
        ended_at: 120,
        bytes_seen: None,
        bytes_discarded: None,
    }
}

fn sample_all(ledger: &Ledger) {
    ledger
        .record_run(&RunRecord {
            run_id: "run-1".to_owned(),
            kind: "maintenance".to_owned(),
            entity: "repository".to_owned(),
            parent_run_id: Some("parent".to_owned()),
            started_by: "person".to_owned(),
            status: "broken".to_owned(),
            total_cost_micros: 42,
            error: Some("compile failed".to_owned()),
            started_at: 90,
            ended_at: Some(121),
            worktree: None,
        })
        .expect("record the run");
    ledger
        .append_step_started(&started("run-1"))
        .expect("record the intent");
    ledger
        .close_step("run-1", "compile", 1, 7, completion())
        .expect("close the step");
    ledger
        .record_model_call(&ModelCallRecord {
            call_id: "call-1".to_owned(),
            run_id: "run-1".to_owned(),
            step_id: Some("compile".to_owned()),
            purpose: "repair".to_owned(),
            cli: "codex".to_owned(),
            requested_model: "requested".to_owned(),
            actual_model: "actual".to_owned(),
            input_tokens: Some(10),
            output_tokens: Some(20),
            cached_tokens: Some(3),
            cache_write_tokens: None,
            cache_write_long_tokens: None,
            total_tokens: None,
            turns: None,
            cost_micros: Some(21),
            declared_cost_micros: None,
            price_currency: Some("USD".to_owned()),
            input_price_micros_per_million: Some(100),
            output_price_micros_per_million: Some(200),
            cached_price_micros_per_million: Some(10),
            cache_write_price_micros_per_million: None,
            cache_write_long_price_micros_per_million: None,
            engine_identity: EngineIdentity::ProfileInForce {
                cli_id: "codex".to_owned(),
                profile_name: "lavoro".to_owned(),
                home_dir: PathBuf::from("/case/codex/lavoro"),
                endpoint: None,
            },
            retry_chain: vec!["call-0".to_owned()],
            error_type: Some("rate_limit".to_owned()),
            started_at: 101,
            ended_at: Some(110),
            session_id: None,
            work_kind: None,
        })
        .expect("record the call");
    ledger
        .record_snapshot(&SnapshotRecord {
            snapshot_id: "snapshot-1".to_owned(),
            run_id: "run-1".to_owned(),
            step_id: Some("compile".to_owned()),
            phase: "repair".to_owned(),
            before: json!({"clean": false}),
            after: json!({"clean": true}),
            created_at: 119,
        })
        .expect("record the snapshot");
}

#[test]
fn step_record_round_trips_without_losing_nulls_or_columns() {
    let directory = TestDirectory::new("round-trip");
    let ledger = Ledger::open(&directory.0).expect("open the ledger");
    let record = started("run-null");
    ledger
        .append_step_started(&record)
        .expect("write the record");
    assert_eq!(ledger.steps("run-null").expect("read back"), vec![record]);

    let connection = ledger.lock().expect("connection");
    let payload: String = connection
        .query_row(
            "SELECT payload FROM events.events WHERE kind = 'step_started'",
            [],
            |row| row.get(0),
        )
        .expect("the event");
    let value: Value = serde_json::from_str(&payload).expect("json");
    let object = value["record"].as_object().expect("record object");
    for field in [
        "outcome",
        "output",
        "said",
        "failure_class",
        "ended_at",
        "bytes_seen",
        "bytes_discarded",
    ] {
        assert!(object.contains_key(field), "the field {field} is missing");
        assert!(object[field].is_null(), "{field} is not null");
    }
}

#[test]
fn changed_gates_on_same_input_are_queryable_as_a_resume_condition() {
    let directory = TestDirectory::new("changed-gates");
    let ledger = Ledger::open(&directory.0).expect("open the ledger");
    let first = started_attempt("run-gates", 1, 7);
    ledger
        .append_step_started(&first)
        .expect("write the first attempt");

    let mut resumed = started_attempt("run-gates", 2, 8);
    resumed.gates = vec!["filesystem".to_owned()];
    resumed.attempt_relation = Some(flow::AttemptRelation::SameInputGatesChanged);
    assert_eq!(first.input_digest, resumed.input_digest);
    ledger
        .append_step_started(&resumed)
        .expect("write the resumed attempt");

    assert_eq!(
        ledger
            .steps_resumed_with_changed_gates()
            .expect("query the resumed steps"),
        vec![GatesChangedStep {
            run_id: "run-gates".to_owned(),
            step_id: "compile".to_owned(),
            attempt: 2,
            epoch: 8,
        }]
    );
    assert_eq!(ledger.steps("run-gates").expect("read back")[1], resumed);
}

#[test]
fn stopped_and_skipped_outcomes_round_trip_through_the_operational_column() {
    let directory = TestDirectory::new("outcomes");
    let ledger = Ledger::open(&directory.0).expect("open the ledger");
    for (run_id, outcome, stored_name) in [
        ("run-stopped", Outcome::Stopped, "Stopped"),
        ("run-skipped", Outcome::Skipped, "Skipped"),
    ] {
        ledger
            .append_step_started(&started(run_id))
            .expect("write the record");
        let mut completion = completion();
        completion.outcome = outcome;
        ledger
            .close_step(run_id, "compile", 1, 7, completion)
            .expect("close the step");
        assert_eq!(
            ledger.steps(run_id).expect("read back")[0].outcome,
            Some(outcome)
        );
        let connection = ledger.lock().expect("connection");
        let stored: String = connection
            .query_row(
                "SELECT outcome FROM steps WHERE run_id = ?1",
                [run_id],
                |row| row.get(0),
            )
            .expect("read the operational column");
        assert_eq!(stored, stored_name);
    }
}

#[test]
fn two_processes_using_ledger_api_serialize_writers() {
    let directory = TestDirectory::new("concurrency");
    Ledger::open(&directory.0).expect("initialise the ledger");
    let marker = directory.0.join("writer-a-read");
    let mut first = helper_command("writer")
        .env("LEDGER_DIRECTORY", &directory.0)
        .env("LEDGER_RUN_ID", "writer-a")
        .env("LEDGER_TEST_STEP_READ_MARKER", &marker)
        .env("LEDGER_TEST_STEP_READ_HOLD_MILLIS", "500")
        .spawn()
        .expect("start the first process");
    wait_for(&marker);
    let began = Instant::now();
    let second = helper_command("writer")
        .env("LEDGER_DIRECTORY", &directory.0)
        .env("LEDGER_RUN_ID", "writer-b")
        .status()
        .expect("start the second process");
    let waited = began.elapsed();
    let first = first.wait().expect("wait for the first process");
    assert!(first.success(), "the first writer died: {first}");
    assert!(second.success(), "the second writer died: {second}");
    assert!(
        waited >= Duration::from_millis(300),
        "the second writer did not wait"
    );
}

#[test]
fn explicit_projection_rebuild_is_identical_and_skipped_event_control_differs() {
    let directory = TestDirectory::new("rebuild");
    let ledger = Ledger::open(&directory.0).expect("open the ledger");
    sample_all(&ledger);
    let expected = ledger.projection_dump().expect("read the projections");

    rebuild_skipping(&ledger, Some("step_closed")).expect("the broken rebuild");
    let broken = ledger.projection_dump().expect("read the control");
    assert_ne!(
        broken, expected,
        "skipping the close event should have changed the projection"
    );
    ledger.rebuild_projections().expect("rebuild explicitly");
    assert_eq!(ledger.projection_dump().expect("read the result"), expected);
}

/// **A NULL OUTPUT IS AN OUTPUT, AND THE LOG HAS TO KEEP THAT** — fault 33: a
/// step closed `Went` with one came back with none, and the step after it died
/// saying it had no typed output, three steps from the defect. The projection
/// is **rebuilt from the log** on purpose: the column keeps the two apart on
/// its own, so reading the store without replaying would pass while the log,
/// which is the source of truth, still lost it.
#[test]
fn a_step_closed_with_a_null_output_still_has_one_after_a_rebuild() {
    let directory = TestDirectory::new("uscita-nulla");
    let ledger = Ledger::open(&directory.0).expect("open the ledger");
    ledger
        .append_step_started(&started("run-1"))
        .expect("record the intent");
    ledger
        .close_step(
            "run-1",
            "compile",
            1,
            7,
            Completion {
                outcome: Outcome::Went,
                output: Some(Value::Null),
                ..completion()
            },
        )
        .expect("close it with a null output");

    ledger.rebuild_projections().expect("rebuild from the log");

    let steps = ledger.steps("run-1").expect("read the steps");
    assert_eq!(steps.len(), 1);
    assert_eq!(
        steps[0].output,
        Some(Value::Null),
        "a null output came back as no output at all"
    );
}

/// The other arm: **no output must stay no output.** Without it the repair
/// could hand every step a null output and the case above would stay green.
#[test]
fn a_step_closed_with_no_output_still_has_none_after_a_rebuild() {
    let directory = TestDirectory::new("nessuna-uscita");
    let ledger = Ledger::open(&directory.0).expect("open the ledger");
    ledger
        .append_step_started(&started("run-1"))
        .expect("record the intent");
    ledger
        .close_step(
            "run-1",
            "compile",
            1,
            7,
            Completion {
                outcome: Outcome::Went,
                output: None,
                ..completion()
            },
        )
        .expect("close it with no output");

    ledger.rebuild_projections().expect("rebuild from the log");

    let steps = ledger.steps("run-1").expect("read the steps");
    assert_eq!(steps[0].output, None, "a step with no output grew one");
}

#[test]
fn committed_event_is_projected_after_a_crash_between_the_two_phases() {
    let directory = TestDirectory::new("checkpoint");
    let ledger = Ledger::open(&directory.0).expect("open the ledger");
    ledger
        .append_step_started(&started("run-crash"))
        .expect("record the intent");
    let crashed = helper_command("crash_before_checkpoint_commit")
        .env("LEDGER_DIRECTORY", &directory.0)
        .env("LEDGER_TEST_CRASH_AFTER_CLOSE_EVENT", "1")
        .status()
        .expect("start the interrupted process");
    assert!(
        !crashed.success(),
        "the injected process should have aborted"
    );
    let reopened = Ledger::open(&directory.0).expect("reopen after the crash");
    assert!(reopened
        .is_checkpointed("run-crash", "compile", 1)
        .expect("read the checkpoint"));
    assert_eq!(
        reopened.steps("run-crash").expect("read the step")[0].outcome,
        Some(Outcome::Broke)
    );
    assert!(!would_relaunch(&reopened, "run-crash"));
}

#[test]
fn stale_epoch_cannot_close_superseded_attempt() {
    let directory = TestDirectory::new("epoch-fencing");
    let ledger = Ledger::open(&directory.0).expect("open the ledger");
    ledger
        .append_step_started(&started_attempt("run-epoch", 1, 5))
        .expect("start the first attempt");
    ledger
        .append_step_started(&started_attempt("run-epoch", 2, 6))
        .expect("start the replacement attempt");

    let error = ledger
        .close_step("run-epoch", "compile", 1, 5, completion())
        .expect_err("a superseded epoch must not close");
    assert!(matches!(error, LedgerError::StaleEpoch { epoch: 5, .. }));
    let steps = ledger.steps("run-epoch").expect("read the attempts back");
    assert!(steps.iter().all(|record| record.outcome.is_none()));
}

#[test]
fn current_epoch_cannot_close_a_previous_attempt() {
    let directory = TestDirectory::new("attempt-epoch-fencing");
    let ledger = Ledger::open(&directory.0).expect("open the ledger");
    ledger
        .append_step_started(&started_attempt("run-epoch-pair", 1, 5))
        .expect("start the first attempt");
    ledger
        .append_step_started(&started_attempt("run-epoch-pair", 2, 6))
        .expect("start the current attempt");

    let error = ledger
        .close_step("run-epoch-pair", "compile", 1, 6, completion())
        .expect_err("attempt and epoch must identify the same record");
    assert!(matches!(
        error,
        LedgerError::MissingAttempt { attempt: 1, .. }
    ));
    assert!(ledger
        .steps("run-epoch-pair")
        .expect("read the attempts back")
        .iter()
        .all(|record| record.outcome.is_none()));
}

#[test]
fn reopening_twice_does_not_read_or_reapply_old_events() {
    let directory = TestDirectory::new("incremental-open");
    let ledger = Ledger::open(&directory.0).expect("open the ledger");
    sample_all(&ledger);
    let expected = ledger.projection_dump().expect("read the state");
    drop(ledger);

    APPLIED_EVENT_READS.set(0);
    let first = Ledger::open(&directory.0).expect("the first reopen");
    assert_eq!(
        APPLIED_EVENT_READS.get(),
        0,
        "the first reopen re-read events"
    );
    drop(first);
    let second = Ledger::open(&directory.0).expect("the second reopen");
    assert_eq!(
        APPLIED_EVENT_READS.get(),
        0,
        "the second reopen re-read old events"
    );
    assert_eq!(
        second.projection_dump().expect("read the state back"),
        expected
    );
}

#[test]
fn pruning_applied_events_preserves_projections_on_reopen() {
    let directory = TestDirectory::new("pruned-log");
    let ledger = Ledger::open(&directory.0).expect("open the ledger");
    sample_all(&ledger);
    let expected = ledger.projection_dump().expect("read the state");
    drop(ledger);

    let events = Connection::open(directory.0.join(EVENTS_FILE)).expect("open the event log");
    events
        .execute_batch(
            "DROP TRIGGER events_append_only_delete;
             DELETE FROM events;",
        )
        .expect("prune the already-folded-in events");
    drop(events);

    let reopened = Ledger::open(&directory.0).expect("reopen after the pruning");
    assert_eq!(
        reopened.projection_dump().expect("read the state back"),
        expected
    );
}

#[test]
fn event_log_rejects_update_delete_and_upsert_update() {
    let directory = TestDirectory::new("append-only");
    let ledger = Ledger::open(&directory.0).expect("open the ledger");
    ledger
        .append_step_started(&started("run-append-only"))
        .expect("append an event");
    let connection = ledger.lock().expect("connection");
    let seq: i64 = connection
        .query_row("SELECT seq FROM events.events LIMIT 1", [], |row| {
            row.get(0)
        })
        .expect("an existing sequence number");

    assert!(connection
        .execute(
            "UPDATE events.events SET payload = '{}' WHERE seq = ?1",
            [seq]
        )
        .is_err());
    assert!(connection
        .execute("DELETE FROM events.events WHERE seq = ?1", [seq])
        .is_err());
    assert!(connection
        .execute(
            "INSERT INTO events.events (seq, kind, payload) VALUES (?1, 'tampered', '{}')
             ON CONFLICT(seq) DO UPDATE SET payload = excluded.payload",
            [seq],
        )
        .is_err());
}

#[test]
fn pragmas_and_operational_failure_query_are_effective() {
    let directory = TestDirectory::new("configuration");
    let ledger = Ledger::open(&directory.0).expect("open the ledger");
    sample_all(&ledger);
    let connection = ledger.lock().expect("connection");
    assert_eq!(pragma_text(&connection, "main", "journal_mode"), "wal");
    assert_eq!(pragma_text(&connection, "events", "journal_mode"), "wal");
    assert_eq!(pragma_i64(&connection, "main", "synchronous"), 2);
    assert_eq!(pragma_i64(&connection, "events", "synchronous"), 1);
    assert_eq!(pragma_i64(&connection, "main", "busy_timeout"), 5_000);
    drop(connection);
    let failures = ledger
        .failed_steps_in_recent_runs("compiler_error", 50)
        .expect("query the failures");
    assert_eq!(failures.len(), 1);
    assert_eq!(failures[0].step_id, "compile");
}

#[test]
fn tracing_layer_appends_structured_fields() {
    let directory = TestDirectory::new("tracing");
    let ledger = Ledger::open(&directory.0).expect("open the ledger");
    let subscriber =
        tracing_subscriber::registry().with(SqliteLayer::with_clock(ledger.clone(), || 444));
    tracing::subscriber::with_default(subscriber, || {
        tracing::info!(
            run_id = "run-trace",
            attempt = 2_u64,
            ready = true,
            "an event"
        );
    });
    let connection = ledger.lock().expect("connection");
    let payload: String = connection
        .query_row(
            "SELECT payload FROM events.events WHERE kind = 'trace'",
            [],
            |row| row.get(0),
        )
        .expect("the trace");
    let event: Value = serde_json::from_str(&payload).expect("json");
    assert_eq!(event["record"]["occurred_at"], 444);
    assert_eq!(event["record"]["fields"]["run_id"], "run-trace");
    assert_eq!(event["record"]["fields"]["attempt"], 2);
    assert_eq!(event["record"]["fields"]["ready"], true);
}

#[test]
fn process_helper() {
    let Ok(action) = std::env::var("LEDGER_HELPER_ACTION") else {
        return;
    };
    match action.as_str() {
        "writer" => writer_helper(),
        "crash_before_checkpoint_commit" => crash_before_checkpoint_commit(),
        other => panic!("unknown helper action: {other}"),
    }
}

fn helper_command(action: &str) -> Command {
    let mut command = Command::new(std::env::current_exe().expect("the test executable"));
    command
        .arg("--exact")
        .arg("tests::process_helper")
        .arg("--nocapture")
        .env("LEDGER_HELPER_ACTION", action);
    command
}

fn writer_helper() {
    let directory = PathBuf::from(std::env::var_os("LEDGER_DIRECTORY").expect("ledger directory"));
    let run_id = std::env::var("LEDGER_RUN_ID").expect("the run id");
    let ledger = Ledger::open(directory).expect("open the ledger");
    ledger
        .append_step_started(&started(&run_id))
        .expect("write through Ledger::append_step_started");
}

fn crash_before_checkpoint_commit() -> ! {
    let directory = PathBuf::from(std::env::var_os("LEDGER_DIRECTORY").expect("ledger directory"));
    let ledger = Ledger::open(directory).expect("open the ledger");
    ledger
        .close_step("run-crash", "compile", 1, 7, completion())
        .expect("the injection point must abort close_step");
    panic!("close_step returned without the injected crash")
}

fn rebuild_skipping(ledger: &Ledger, skipped_kind: Option<&str>) -> Result<(), LedgerError> {
    let mut connection = ledger.lock()?;
    let transaction = immediate(&mut connection)?;
    drop_projection_schema(&transaction)?;
    create_projection_schema(&transaction)?;
    let events = {
        let mut statement =
            transaction.prepare("SELECT kind, payload FROM events.events ORDER BY seq")?;
        let events = statement
            .query_map([], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
            })?
            .collect::<Result<Vec<_>, _>>()?;
        events
    };
    for (kind, payload) in events {
        if Some(kind.as_str()) == skipped_kind {
            continue;
        }
        project_event(&transaction, &serde_json::from_str(&payload)?)?;
    }
    transaction.commit()?;
    Ok(())
}

fn would_relaunch(ledger: &Ledger, run_id: &str) -> bool {
    !ledger
        .is_checkpointed(run_id, "compile", 1)
        .expect("read the checkpoint flag")
}

fn wait_for(path: &Path) {
    let deadline = Instant::now() + Duration::from_secs(5);
    while !path.exists() {
        assert!(
            Instant::now() < deadline,
            "the marker never arrived: {}",
            path.display()
        );
        thread::sleep(Duration::from_millis(10));
    }
}

fn pragma_text(connection: &Connection, database: &str, pragma: &str) -> String {
    connection
        .query_row(&format!("PRAGMA {database}.{pragma}"), [], |row| row.get(0))
        .expect("read the pragma")
}

fn pragma_i64(connection: &Connection, database: &str, pragma: &str) -> i64 {
    connection
        .query_row(&format!("PRAGMA {database}.{pragma}"), [], |row| row.get(0))
        .expect("read the pragma")
}

#[test]
fn steps_with_discarded_output_are_queryable_and_match_declared_values() {
    let directory = TestDirectory::new("discarded-output");
    let ledger = Ledger::open(&directory.0).expect("open the ledger");
    let record = started("run-discard");
    ledger
        .append_step_started(&record)
        .expect("write the intent");
    let mut comp = completion();
    comp.outcome = Outcome::Went;
    comp.bytes_seen = Some(150_000);
    comp.bytes_discarded = Some(50_000);
    ledger
        .close_step("run-discard", "compile", 1, 7, comp)
        .expect("close the step");

    let discarded = ledger
        .steps_with_discarded_output()
        .expect("query the steps with discarded output");
    assert_eq!(
        discarded,
        vec![DiscardedOutputStep {
            run_id: "run-discard".to_owned(),
            step_id: "compile".to_owned(),
            attempt: 1,
            epoch: 7,
            bytes_seen: 150_000,
            bytes_discarded: 50_000,
        }]
    );
    let steps = ledger.steps("run-discard").expect("read back");
    assert_eq!(steps[0].bytes_seen, Some(150_000));
    assert_eq!(steps[0].bytes_discarded, Some(50_000));
}

#[test]
fn schema_v1_legacy_records_without_byte_counts_upgrade_and_rebuild_cleanly() {
    let directory = TestDirectory::new("upgrade-v1");
    // Build a v1 database with the old schema and a record with no
    // bytes_seen/bytes_discarded fields.
    {
        let state_path = directory.0.join(STATE_FILE);
        let events_path = directory.0.join(EVENTS_FILE);
        let mut connection = Connection::open(&state_path).expect("open state");
        connection
            .execute(
                "ATTACH DATABASE ?1 AS events",
                [events_path.to_string_lossy().as_ref()],
            )
            .expect("attach");
        let transaction = immediate(&mut connection).expect("tx");
        create_event_schema(&transaction).expect("event schema");
        transaction
            .execute_batch(
                "CREATE TABLE IF NOT EXISTS runs (
                     run_id TEXT PRIMARY KEY,
                     kind TEXT NOT NULL,
                     entity TEXT NOT NULL,
                     parent_run_id TEXT,
                     started_by TEXT NOT NULL,
                     status TEXT NOT NULL,
                     total_cost_micros INTEGER NOT NULL,
                     error TEXT,
                     started_at INTEGER NOT NULL,
                     ended_at INTEGER
                 );
                 CREATE TABLE IF NOT EXISTS steps (
                     run_id TEXT NOT NULL,
                     step_id TEXT NOT NULL,
                     attempt INTEGER NOT NULL,
                     epoch TEXT NOT NULL,
                     deps TEXT NOT NULL,
                     input_digest TEXT NOT NULL,
                     input TEXT NOT NULL,
                     gates TEXT NOT NULL,
                     attempt_relation TEXT,
                     started_at INTEGER NOT NULL,
                     outcome TEXT,
                     output TEXT,
                     said TEXT,
                     failure_class TEXT,
                     ended_at INTEGER,
                     checkpointed INTEGER NOT NULL DEFAULT 0 CHECK(checkpointed IN (0, 1)),
                     PRIMARY KEY (run_id, step_id, attempt)
                 );
                 CREATE TABLE IF NOT EXISTS model_calls (
                     call_id TEXT PRIMARY KEY,
                     run_id TEXT NOT NULL,
                     step_id TEXT,
                     purpose TEXT NOT NULL,
                     cli TEXT NOT NULL,
                     requested_model TEXT NOT NULL,
                     actual_model TEXT NOT NULL,
                     input_tokens TEXT NOT NULL,
                     output_tokens TEXT NOT NULL,
                     cached_tokens TEXT NOT NULL,
                     cost_micros INTEGER NOT NULL,
                     price_currency TEXT NOT NULL,
                     input_price_micros_per_million INTEGER NOT NULL,
                     output_price_micros_per_million INTEGER NOT NULL,
                     cached_price_micros_per_million INTEGER NOT NULL,
                     mandate_name TEXT NOT NULL,
                     mandate_version TEXT NOT NULL,
                     retry_chain TEXT NOT NULL,
                     error_type TEXT,
                     started_at INTEGER NOT NULL,
                     ended_at INTEGER
                 );
                 CREATE TABLE IF NOT EXISTS snapshots (
                     snapshot_id TEXT PRIMARY KEY,
                     run_id TEXT NOT NULL,
                     step_id TEXT,
                     phase TEXT NOT NULL,
                     before_state TEXT NOT NULL,
                     after_state TEXT NOT NULL,
                     created_at INTEGER NOT NULL
                 );
                 CREATE TABLE IF NOT EXISTS projection_watermark (
                     singleton INTEGER PRIMARY KEY CHECK(singleton = 1),
                     last_applied_seq INTEGER NOT NULL CHECK(last_applied_seq >= 0)
                 );",
            )
            .expect("v1 tables");
        initialize_projection_watermark(&transaction).expect("watermark");
        transaction
            .pragma_update(None, "user_version", 1)
            .expect("version 1");

        let legacy_json = serde_json::json!({
            "run_id": "legacy-run",
            "step_id": "compile",
            "attempt": 1,
            "epoch": 1,
            "deps": [],
            "input_digest": "digest",
            "input": null,
            "gates": [],
            "attempt_relation": null,
            "started_at": 100,
            "outcome": "Went",
            "output": {"result": "ok"},
            "said": "[launcher] kept 10 bytes",
            "failure_class": null,
            "ended_at": 110
        });
        transaction
            .execute(
                "INSERT INTO events.events (seq, run_id, step_id, attempt, kind, payload)
                 VALUES (1, 'legacy-run', 'compile', 1, 'step_closed', ?1)",
                [serde_json::to_string(&serde_json::json!({
                    "type": "step_closed",
                    "record": legacy_json
                }))
                .unwrap()],
            )
            .expect("insert legacy event");
        transaction.commit().expect("commit v1");
    }

    let ledger = Ledger::open(&directory.0).expect("open and migrate the old schema");
    let steps = ledger.steps("legacy-run").expect("read the migrated steps");
    assert_eq!(steps.len(), 1);
    assert_eq!(steps[0].bytes_seen, None);
    assert_eq!(steps[0].bytes_discarded, None);
    // A step written before these existed inherits neither a holder nor a
    // species: they stay empty, which is what they really were.
    assert_eq!(steps[0].held_by_pid, None);
    assert_eq!(steps[0].species, None);
    assert_eq!(steps[0].outcome, Some(Outcome::Went));
}

/// Who holds the step and what species it is reach the disk and come back:
/// these are the two facts a resume decides on, and a store that lost them
/// along the way would send it back to guessing.
#[test]
fn the_holder_and_the_species_survive_the_ledger() {
    let directory = TestDirectory::new("species-round-trip");
    let ledger = Ledger::open(&directory.0).expect("open the ledger");
    let mut record = started_attempt("species-run", 1, 7);
    record.held_by_pid = Some(4321);
    record.species = Some(StepSpecies::Compensable);
    ledger
        .append_step_started(&record)
        .expect("write the intent");

    let steps = ledger.steps("species-run").expect("read the steps back");
    assert_eq!(steps.len(), 1);
    assert_eq!(steps[0].held_by_pid, Some(4321));
    assert_eq!(steps[0].species, Some(StepSpecies::Compensable));
}

/// A species the store does not know is not read as "hand to a human": that
/// would be an invented value standing in for a corrupted datum.
#[test]
fn an_unknown_species_is_an_error_not_a_fallback() {
    assert_eq!(species_name(StepSpecies::Repeatable), "repeatable");
    assert_eq!(species_name(StepSpecies::Compensable), "compensable");
    assert_eq!(species_name(StepSpecies::HandToHuman), "hand_to_human");
    assert!(parse_species("repeatable").is_ok());
    assert!(parse_species("ripetibile").is_err());
}

#[test]
fn schema_v1_pruned_database_upgrades_in_place_without_event_rebuild() {
    let directory = TestDirectory::new("upgrade-v1-pruned");
    // Build a v1 database with projections already written and a pruned event log.
    {
        let state_path = directory.0.join(STATE_FILE);
        let events_path = directory.0.join(EVENTS_FILE);
        let mut connection = Connection::open(&state_path).expect("open state");
        connection
            .execute(
                "ATTACH DATABASE ?1 AS events",
                [events_path.to_string_lossy().as_ref()],
            )
            .expect("attach");
        let transaction = immediate(&mut connection).expect("tx");
        create_event_schema(&transaction).expect("event schema");
        transaction
            .execute_batch(
                "CREATE TABLE IF NOT EXISTS runs (
                     run_id TEXT PRIMARY KEY,
                     kind TEXT NOT NULL,
                     entity TEXT NOT NULL,
                     parent_run_id TEXT,
                     started_by TEXT NOT NULL,
                     status TEXT NOT NULL,
                     total_cost_micros INTEGER NOT NULL,
                     error TEXT,
                     started_at INTEGER NOT NULL,
                     ended_at INTEGER
                 );
                 CREATE TABLE IF NOT EXISTS steps (
                     run_id TEXT NOT NULL,
                     step_id TEXT NOT NULL,
                     attempt INTEGER NOT NULL,
                     epoch TEXT NOT NULL,
                     deps TEXT NOT NULL,
                     input_digest TEXT NOT NULL,
                     input TEXT NOT NULL,
                     gates TEXT NOT NULL,
                     attempt_relation TEXT,
                     started_at INTEGER NOT NULL,
                     outcome TEXT,
                     output TEXT,
                     said TEXT,
                     failure_class TEXT,
                     ended_at INTEGER,
                     checkpointed INTEGER NOT NULL DEFAULT 0 CHECK(checkpointed IN (0, 1)),
                     PRIMARY KEY (run_id, step_id, attempt)
                 );
                 CREATE TABLE IF NOT EXISTS model_calls (
                     call_id TEXT PRIMARY KEY,
                     run_id TEXT NOT NULL,
                     step_id TEXT,
                     purpose TEXT NOT NULL,
                     cli TEXT NOT NULL,
                     requested_model TEXT NOT NULL,
                     actual_model TEXT NOT NULL,
                     input_tokens TEXT NOT NULL,
                     output_tokens TEXT NOT NULL,
                     cached_tokens TEXT NOT NULL,
                     cost_micros INTEGER NOT NULL,
                     price_currency TEXT NOT NULL,
                     input_price_micros_per_million INTEGER NOT NULL,
                     output_price_micros_per_million INTEGER NOT NULL,
                     cached_price_micros_per_million INTEGER NOT NULL,
                     mandate_name TEXT NOT NULL,
                     mandate_version TEXT NOT NULL,
                     retry_chain TEXT NOT NULL,
                     error_type TEXT,
                     started_at INTEGER NOT NULL,
                     ended_at INTEGER
                 );
                 CREATE TABLE IF NOT EXISTS snapshots (
                     snapshot_id TEXT PRIMARY KEY,
                     run_id TEXT NOT NULL,
                     step_id TEXT,
                     phase TEXT NOT NULL,
                     before_state TEXT NOT NULL,
                     after_state TEXT NOT NULL,
                     created_at INTEGER NOT NULL
                 );
                 CREATE TABLE IF NOT EXISTS projection_watermark (
                     singleton INTEGER PRIMARY KEY CHECK(singleton = 1),
                     last_applied_seq INTEGER NOT NULL CHECK(last_applied_seq >= 0)
                 );",
            )
            .expect("v1 tables");
        transaction
            .execute(
                "INSERT INTO projection_watermark (singleton, last_applied_seq) VALUES (1, 10)",
                [],
            )
            .expect("watermark");
        transaction
            .execute(
                "INSERT INTO events.sqlite_sequence (name, seq) VALUES ('events', 10)",
                [],
            )
            .expect("sqlite sequence");
        transaction
            .pragma_update(None, "user_version", 1)
            .expect("version 1");
        transaction
            .execute(
                "INSERT INTO steps (run_id, step_id, attempt, epoch, deps, input_digest, input,
                                    gates, attempt_relation, started_at, outcome, output, said,
                                    failure_class, ended_at, checkpointed)
                 VALUES ('pruned-run', 'compile', 1, '00000000000000000001', '[]', 'digest',
                         'null', '[]', null, 100, 'Went', '{\"result\":\"ok\"}',
                         '[launcher] kept 10 bytes', null, 110, 1)",
                [],
            )
            .expect("insert v1 projected step");
        // Empty (pruned) event log: a rebuild would fail here.
        transaction.commit().expect("commit v1");
    }

    let ledger = Ledger::open(&directory.0).expect("open pruned v1 without a rebuild");
    let steps = ledger
        .steps("pruned-run")
        .expect("read the preserved steps");
    assert_eq!(steps.len(), 1);
    assert_eq!(steps[0].bytes_seen, None);
    assert_eq!(steps[0].bytes_discarded, None);
    assert_eq!(steps[0].outcome, Some(Outcome::Went));
}

#[test]
fn corrupted_event_log_behind_projection_watermark_is_rejected_on_open() {
    let directory = TestDirectory::new("corrupted-watermark");
    // Simulate a corrupted store: the projection is at step 10, but the event
    // log never went past sequence 2.
    {
        let state_path = directory.0.join(STATE_FILE);
        let events_path = directory.0.join(EVENTS_FILE);
        let mut connection = Connection::open(&state_path).expect("open state");
        connection
            .execute(
                "ATTACH DATABASE ?1 AS events",
                [events_path.to_string_lossy().as_ref()],
            )
            .expect("attach");
        let transaction = immediate(&mut connection).expect("tx");
        create_event_schema(&transaction).expect("event schema");
        create_projection_tables(&transaction).expect("tables");
        create_projection_indexes(&transaction).expect("indexes");
        transaction
            .execute(
                "INSERT INTO projection_watermark (singleton, last_applied_seq) VALUES (1, 10)",
                [],
            )
            .expect("watermark");
        transaction
            .execute(
                "INSERT INTO events.sqlite_sequence (name, seq) VALUES ('events', 2)",
                [],
            )
            .expect("sqlite sequence");
        transaction.commit().expect("commit");
    }

    match Ledger::open(&directory.0) {
        Err(LedgerError::InvalidRecord(message)) => {
            assert!(
                message.contains("projection watermark 10 is ahead of event log 2"),
                "unexpected message: {message}"
            );
        }
        Ok(_) => panic!("the open should have been refused for a corrupted watermark"),
        Err(error) => panic!("unexpected error kind: {error:?}"),
    }
}

fn seen(kind: &str, name: &str, reach: &str) -> InventoryItem {
    InventoryItem {
        kind: kind.to_string(),
        name: name.to_string(),
        origin: "home".to_string(),
        path: format!("/home/{kind}/{name}"),
        reach: reach.to_string(),
        reason: (reach != "active").then(|| "the plugin is switched off".to_string()),
    }
}

/// The store can say what is no longer there — the question a list recomputed
/// every time cannot even ask.
///
/// THREE SCANS, BECAUSE THE THIRD ACT IS THE ONE THAT GOES WRONG: something
/// that reappears must stop reading as vanished. Without that arm the vanish
/// mark would stay forever and whoever deletes from the list kills a live thing.
#[test]
fn the_ledger_knows_what_disappeared_and_what_came_back() {
    let directory = TestDirectory::new("inventory");
    let ledger = Ledger::open(&directory.0).expect("open the ledger");

    ledger
        .record_inventory(&InventoryScan {
            taken_at: 100,
            items: vec![
                seen("skill", "prima", "active"),
                seen("hook", "sola", "active"),
            ],
        })
        .expect("the first scan");

    let present = ledger.inventory_present().expect("present entries");
    assert_eq!(present.len(), 2, "{present:#?}");
    assert!(present.iter().all(|item| item.first_seen == 100));

    // Second scan: the skill is gone, and the hook switches off.
    ledger
        .record_inventory(&InventoryScan {
            taken_at: 200,
            items: vec![seen("hook", "sola", "inactive")],
        })
        .expect("the second scan");

    let gone = ledger.inventory_gone().expect("vanished entries");
    assert_eq!(gone.len(), 1, "{gone:#?}");
    assert_eq!(gone[0].name, "prima");
    assert_eq!(gone[0].gone_at, Some(200));
    // And the first-seen date has not moved: it is the only one saying since when.
    assert_eq!(gone[0].first_seen, 100);

    let present = ledger.inventory_present().expect("present entries");
    assert_eq!(present.len(), 1, "{present:#?}");
    assert_eq!(present[0].reach, "inactive");
    assert_eq!(
        present[0].reason.as_deref(),
        Some("the plugin is switched off")
    );
    assert_eq!(
        present[0].first_seen, 100,
        "the hook was there from the first scan"
    );

    // Third scan: the skill comes back. It is no longer vanished, and it keeps
    // its original date.
    ledger
        .record_inventory(&InventoryScan {
            taken_at: 300,
            items: vec![
                seen("skill", "prima", "active"),
                seen("hook", "sola", "active"),
            ],
        })
        .expect("the third scan");

    assert!(
        ledger
            .inventory_gone()
            .expect("vanished entries")
            .is_empty(),
        "an entry that came back still reads as vanished"
    );
    let back = ledger
        .inventory_present()
        .expect("present entries")
        .into_iter()
        .find(|item| item.name == "prima")
        .expect("the skill came back");
    assert_eq!(back.first_seen, 100);
    assert_eq!(back.last_seen, 300);

    // "What is new since yesterday": nothing, because both already existed.
    assert!(
        ledger
            .inventory_new_since(150)
            .expect("new entries")
            .is_empty(),
        "an entry that came back is not a new entry"
    );
}

/// The projection rebuilds from the events, as promised for the rest of the
/// store: if the inventory did not rebuild, the state file would become the
/// only copy of a datum nobody can check any more.
#[test]
fn the_inventory_survives_a_rebuild_from_the_events() {
    let directory = TestDirectory::new("inventory-rebuilt");
    let ledger = Ledger::open(&directory.0).expect("open the ledger");
    ledger
        .record_inventory(&InventoryScan {
            taken_at: 10,
            items: vec![
                seen("skill", "una", "active"),
                seen("skill", "due", "active"),
            ],
        })
        .expect("the first scan");
    ledger
        .record_inventory(&InventoryScan {
            taken_at: 20,
            items: vec![seen("skill", "una", "active")],
        })
        .expect("the second scan");

    ledger.rebuild_projections().expect("rebuild");

    let gone = ledger.inventory_gone().expect("vanished entries");
    assert_eq!(gone.len(), 1, "{gone:#?}");
    assert_eq!(gone[0].name, "due");
    assert_eq!(gone[0].gone_at, Some(20));
}

fn record(collection: &str, key: &str, value: Value, at: i64) -> StoreRecord {
    StoreRecord {
        collection: collection.to_string(),
        key: key.to_string(),
        value,
        written_by: "test-flow".to_string(),
        written_at: at,
    }
}

/// An entry nobody wrote answers "I don't know".
///
/// That is the answer that lets the reader have a fallback. A store inventing a
/// plausible value would be worse than the heuristic it replaces, because it
/// would look like a fact.
#[test]
fn a_record_nobody_wrote_says_it_does_not_know() {
    let directory = TestDirectory::new("record-never-written");
    let ledger = Ledger::open(&directory.0).expect("open the ledger");
    assert_eq!(
        ledger
            .read_record("mandate", "current")
            .expect("read the record"),
        None
    );
}

/// The engine knows nothing about collections: it keeps them all regardless.
///
/// The proof that this space belongs to the flow and not to the engine. Two
/// collections invented here — names appearing nowhere in Rust — must coexist
/// undeclared, and one key used in both must stay two distinct entries. Put the
/// domain back into the engine and this test is the first to stop compiling.
#[test]
fn the_engine_keeps_collections_it_knows_nothing_about() {
    let directory = TestDirectory::new("unknown-collections");
    let ledger = Ledger::open(&directory.0).expect("open the ledger");
    ledger
        .put_record(&record(
            "mandate",
            "current",
            json!({"file": "sailor.md"}),
            10,
        ))
        .expect("the mandate entry");
    ledger
        .put_record(&record("panetteria", "current", json!({"pane": 3}), 20))
        .expect("the bakery entry");

    let mandate = ledger
        .read_record("mandate", "current")
        .expect("the read")
        .expect("present");
    assert_eq!(mandate.value, json!({"file": "sailor.md"}));
    let bakery = ledger
        .read_record("panetteria", "current")
        .expect("the read")
        .expect("present");
    assert_eq!(bakery.value, json!({"pane": 3}));
    assert_eq!(
        ledger.records_in("panetteria").expect("collection").len(),
        1
    );
}

/// The last write wins, and the entry stays one.
///
/// The check that matters is not reading the second value back — a table that
/// piled them all up and returned the newest would pass that too — but that
/// two writes leave **one single entry**. Two entries are two possible answers
/// to one question: the shape every "so what were we working on?" grows from.
#[test]
fn the_last_write_wins_and_the_record_stays_one() {
    let directory = TestDirectory::new("record-replaced");
    let ledger = Ledger::open(&directory.0).expect("open the ledger");
    ledger
        .put_record(&record(
            "mandate",
            "current",
            json!({"file": "socraticode.md"}),
            100,
        ))
        .expect("the first write");
    ledger
        .put_record(&record(
            "mandate",
            "current",
            json!({"file": "sailor.md"}),
            200,
        ))
        .expect("the second write");

    let current = ledger
        .read_record("mandate", "current")
        .expect("the read")
        .expect("present");
    assert_eq!(current.value, json!({"file": "sailor.md"}));
    assert_eq!(current.written_at, 200);
    assert_eq!(ledger.records_in("mandate").expect("collection").len(), 1);
}

/// An entry without an address is refused.
///
/// It holds for the key as much as for the collection: whoever writes without
/// an address believes they stored something, and nobody will find it again.
#[test]
fn a_record_without_an_address_is_refused() {
    let directory = TestDirectory::new("record-without-address");
    let ledger = Ledger::open(&directory.0).expect("open the ledger");

    let mut homeless = record("mandate", "current", json!(1), 10);
    homeless.collection = "  ".to_string();
    assert!(ledger.put_record(&homeless).is_err());

    let mut keyless = record("mandate", "current", json!(1), 10);
    keyless.key = String::new();
    assert!(ledger.put_record(&keyless).is_err());

    // And the refusal left nothing behind.
    assert_eq!(
        ledger
            .read_record("mandate", "current")
            .expect("read the record"),
        None
    );
}

/// The source is the log, not the table.
///
/// It falls on its own if `RecordWritten` stops being projected — if somebody
/// treats it as a trace to discard: the rebuild would start from a full log and
/// leave the table empty. A datum living only in the state file is a datum
/// nobody can verify any more.
#[test]
fn a_record_survives_a_rebuild_from_the_events() {
    let directory = TestDirectory::new("record-rebuilt");
    let ledger = Ledger::open(&directory.0).expect("open the ledger");
    ledger
        .put_record(&record(
            "mandate",
            "current",
            json!({"file": "vecchio.md"}),
            100,
        ))
        .expect("the first write");
    ledger
        .put_record(&record(
            "mandate",
            "current",
            json!({"file": "corrente.md"}),
            200,
        ))
        .expect("the second write");

    ledger.rebuild_projections().expect("rebuild");

    let current = ledger
        .read_record("mandate", "current")
        .expect("the read")
        .expect("present");
    assert_eq!(
        current.value,
        json!({"file": "corrente.md"}),
        "the order of the events decides, and the last one wins"
    );
    assert_eq!(current.written_by, "test-flow");
}

// ── how it went: the tests for the reads over the history ───────────────────

fn a_run(ledger: &Ledger, run_id: &str, entity: &str, started_at: i64, ended_at: Option<i64>) {
    ledger
        .record_run(&RunRecord {
            run_id: run_id.to_owned(),
            kind: "flow".to_owned(),
            entity: entity.to_owned(),
            parent_run_id: None,
            started_by: "test".to_owned(),
            status: if ended_at.is_some() {
                "done"
            } else {
                "running"
            }
            .to_owned(),
            total_cost_micros: 0,
            error: None,
            started_at,
            ended_at,
            worktree: None,
        })
        .expect("record the run");
}

#[allow(clippy::too_many_arguments)]
fn a_step(
    ledger: &Ledger,
    run_id: &str,
    step_id: &str,
    attempt: u32,
    epoch: u64,
    started_at: i64,
    closing: Option<(Outcome, Option<&str>, i64, Option<&str>)>,
) {
    let record = StepRecord::started(
        run_id,
        step_id,
        attempt,
        epoch,
        vec![],
        json!({"secret": "this must never leave"}),
        vec![],
        started_at,
    );
    ledger
        .append_step_started(&record)
        .expect("record the intent");
    if let Some((outcome, failure_class, ended_at, said)) = closing {
        ledger
            .close_step(
                run_id,
                step_id,
                attempt,
                epoch,
                Completion {
                    outcome,
                    output: Some(json!({"secret": "nor this one"})),
                    said: said.map(str::to_owned),
                    failure_class: failure_class.map(str::to_owned),
                    refusal: None,
                    ended_at,
                    bytes_seen: Some(10),
                    bytes_discarded: Some(0),
                },
            )
            .expect("close the step");
    }
}

/// **A HANDED-OVER RUN IS FOUND, AND `unfinished_runs` DOES NOT SEE IT.**
///
/// The two questions look like one and are opposites. `unfinished_runs` wants
/// `steps.outcome IS NULL`, an intent with no outcome — a process dead halfway.
/// A step handed to an agent is **closed**, with outcome `Waiting`, so that
/// question never found it and a hand-over nobody picked up simply vanished.
#[test]
fn a_handed_run_is_found_by_the_waiting_question_and_not_by_the_unfinished_one() {
    let directory = TestDirectory::new("waiting-runs");
    let ledger = Ledger::open(&directory.0).expect("open the ledger");

    // Both runs are kept here on purpose: asking one question and getting the
    // other back is the real defect, and one run alone would not show it.
    //
    // Handed over: the step is closed with outcome "waiting", and the header
    // carries the status the executor writes onto it.
    a_run(&ledger, "run-handed", "sviluppa-sailor", 100, Some(150));
    ledger
        .record_run(&RunRecord {
            run_id: "run-handed".to_owned(),
            kind: "flow".to_owned(),
            entity: "sviluppa-sailor".to_owned(),
            parent_run_id: None,
            started_by: "test".to_owned(),
            status: "waiting".to_owned(),
            total_cost_micros: 0,
            error: None,
            started_at: 100,
            ended_at: Some(150),
            worktree: None,
        })
        .expect("record the handed-over run");
    a_step(
        &ledger,
        "run-handed",
        "implementa",
        1,
        1,
        110,
        Some((
            Outcome::Waiting,
            None,
            150,
            Some("handed to \"claude-live\""),
        )),
    );

    // Interrupted halfway: an open step and no outcome. This one belongs to
    // `unfinished_runs`, and must not appear among the waiting ones.
    a_run(&ledger, "run-halfway", "sviluppa-sailor", 200, None);
    a_step(&ledger, "run-halfway", "implementa", 1, 1, 210, None);

    let waiting = ledger.waiting_runs().expect("ask for the waiting runs");
    assert_eq!(
        waiting.len(),
        1,
        "only one run is waiting for somebody, and it is not the interrupted one: {waiting:?}"
    );
    assert_eq!(waiting[0].run_id, "run-handed");
    assert_eq!(waiting[0].entity, "sviluppa-sailor");
    assert_eq!(
        waiting[0].waiting_since, 150,
        "it waits from when it stopped, not from when it started"
    );

    let unfinished = ledger
        .unfinished_runs()
        .expect("ask for the interrupted runs");
    let names: Vec<&str> = unfinished.iter().map(|run| run.run_id.as_str()).collect();
    assert_eq!(
        names,
        vec!["run-halfway"],
        "the handed-over run is not interrupted: its step is closed. \
         If it shows up here, the two questions have been confused"
    );
}

/// A newborn store answers, and answers zero.
///
/// It falls if one of these reads treats absence as a fault — a `query_row`
/// demanding a row, say. This is the freshly installed machine: whoever queries
/// the history passes here **before** anything else, and an error here would
/// make the first run of every new flow start out red.
#[test]
fn an_empty_ledger_answers_zero_instead_of_breaking() {
    let directory = TestDirectory::new("empty-history");
    let ledger = Ledger::open(&directory.0).expect("open the ledger");

    assert_eq!(ledger.recorded_runs().expect("tally"), 0);
    assert_eq!(ledger.runs_in_window(None, 50).expect("window"), 0);
    let tally = ledger
        .step_failure_tally("compile", None, 50)
        .expect("tally");
    assert_eq!(tally.attempts, 0);
    assert_eq!(tally.failures, 0);
    assert!(tally.by_class.is_empty());
    assert!(ledger
        .failure_class_tally(None, 50)
        .expect("failure classes")
        .is_empty());
    assert_eq!(
        ledger.last_finished_run("anything").expect("last run"),
        None
    );
    let durations = ledger
        .step_durations("compile", None, 50)
        .expect("durations");
    assert!(durations.seconds_sorted.is_empty());
    assert_eq!(durations.failed_samples, 0);
    assert!(ledger
        .said_of_failed_steps("never-existed", 5, 512)
        .expect("said")
        .is_empty());
}

/// The per-flow filter really cuts, and it cuts through `runs`.
///
/// The mutant that fells it is dropping the join with `runs`: the answer for
/// `alpha` would become that of every flow together — a number that looks like
/// a measure of your own flow and measures somebody else's too.
#[test]
fn the_flow_filter_cuts_by_joining_the_run_header() {
    let directory = TestDirectory::new("history-per-flow");
    let ledger = Ledger::open(&directory.0).expect("open the ledger");
    a_run(&ledger, "run-a1", "alpha", 100, Some(200));
    a_step(
        &ledger,
        "run-a1",
        "compile",
        1,
        1,
        100,
        Some((Outcome::Broke, Some("timeout"), 150, None)),
    );
    a_run(&ledger, "run-a2", "alpha", 300, Some(400));
    a_step(
        &ledger,
        "run-a2",
        "compile",
        1,
        1,
        300,
        Some((Outcome::Broke, Some("timeout"), 350, None)),
    );
    a_run(&ledger, "run-b1", "beta", 500, Some(600));
    a_step(
        &ledger,
        "run-b1",
        "compile",
        1,
        1,
        500,
        Some((Outcome::Broke, Some("timeout"), 550, None)),
    );

    let alpha = ledger
        .step_failure_tally("compile", Some("alpha"), 50)
        .expect("tally");
    let everything = ledger
        .step_failure_tally("compile", None, 50)
        .expect("tally");

    assert_eq!(alpha.failures, 2, "only alpha's runs");
    assert_eq!(
        everything.failures, 3,
        "with no flow named, everything counts"
    );
    assert_eq!(ledger.runs_in_window(Some("alpha"), 50).expect("window"), 2);
}

/// Failures are attempts; runs affected are runs.
///
/// It falls if somebody counts `COUNT(DISTINCT run_id)` instead of attempts:
/// two breakages in the same run would become one, and a step that shatters on
/// every retry would look like it broke half as often.
#[test]
fn failures_count_attempts_while_runs_affected_counts_runs() {
    let directory = TestDirectory::new("history-attempts");
    let ledger = Ledger::open(&directory.0).expect("open the ledger");
    a_run(&ledger, "run-1", "alpha", 100, Some(400));
    a_step(
        &ledger,
        "run-1",
        "compile",
        1,
        1,
        100,
        Some((Outcome::Broke, Some("timeout"), 150, None)),
    );
    a_step(
        &ledger,
        "run-1",
        "compile",
        2,
        2,
        200,
        Some((Outcome::Broke, Some("timeout"), 250, None)),
    );

    let tally = ledger
        .step_failure_tally("compile", Some("alpha"), 50)
        .expect("tally");

    assert_eq!(tally.attempts, 2);
    assert_eq!(tally.failures, 2, "two broken attempts are two failures");
    assert_eq!(tally.runs_affected, 1, "in a single run");
}

/// The last run is the last **closed** one, never the one still in flight.
///
/// The mutant that fells it is dropping `AND ended_at IS NOT NULL`: a flow
/// querying itself while running would get itself half-done, and read as last
/// time's outcome a list of steps that have not happened yet.
#[test]
fn the_last_finished_run_is_never_the_one_still_in_flight() {
    let directory = TestDirectory::new("history-last-closed");
    let ledger = Ledger::open(&directory.0).expect("open the ledger");
    a_run(&ledger, "run-vecchia", "alpha", 100, Some(200));
    a_step(
        &ledger,
        "run-vecchia",
        "compile",
        1,
        1,
        100,
        Some((Outcome::Went, None, 130, None)),
    );
    a_run(&ledger, "run-in-volo", "alpha", 300, None);
    a_step(&ledger, "run-in-volo", "compile", 1, 1, 300, None);

    let last = ledger
        .last_finished_run("alpha")
        .expect("the read")
        .expect("a closed run exists");

    assert_eq!(last.run_id, "run-vecchia");
    assert_eq!(last.steps.len(), 1);
    assert_eq!(last.steps[0].outcome.as_deref(), Some("Went"));
}

/// A window of one run leaves the previous one out.
///
/// It falls if `LIMIT` disappears or if `started_at DESC` flips: "in the last N
/// runs" would become "in the first N" — an answer about ancient history handed
/// to somebody asking how things have been going lately.
#[test]
fn a_window_of_one_run_leaves_the_older_one_out() {
    let directory = TestDirectory::new("history-window");
    let ledger = Ledger::open(&directory.0).expect("open the ledger");
    a_run(&ledger, "run-vecchia", "alpha", 100, Some(200));
    a_step(
        &ledger,
        "run-vecchia",
        "compile",
        1,
        1,
        100,
        Some((Outcome::Broke, Some("timeout"), 150, None)),
    );
    a_run(&ledger, "run-nuova", "alpha", 300, Some(400));
    a_step(
        &ledger,
        "run-nuova",
        "compile",
        1,
        1,
        300,
        Some((Outcome::Went, None, 350, None)),
    );

    let narrow = ledger
        .step_failure_tally("compile", Some("alpha"), 1)
        .expect("tally");
    let wide = ledger
        .step_failure_tally("compile", Some("alpha"), 50)
        .expect("tally");

    assert_eq!(narrow.failures, 0, "nothing broke in the last run");
    assert_eq!(narrow.attempts, 1);
    assert_eq!(
        wide.failures, 1,
        "looking further back, the failure is there"
    );
}

/// The most frequent class comes first, and a breakage with no class keeps none.
#[test]
fn the_most_frequent_failure_class_comes_first() {
    let directory = TestDirectory::new("history-classes");
    let ledger = Ledger::open(&directory.0).expect("open the ledger");
    a_run(&ledger, "run-1", "alpha", 100, Some(400));
    a_step(
        &ledger,
        "run-1",
        "uno",
        1,
        1,
        100,
        Some((Outcome::Broke, Some("timeout"), 110, None)),
    );
    a_step(
        &ledger,
        "run-1",
        "due",
        1,
        1,
        120,
        Some((Outcome::Broke, Some("timeout"), 130, None)),
    );
    a_step(
        &ledger,
        "run-1",
        "tre",
        1,
        1,
        140,
        Some((Outcome::Broke, Some("exit_error"), 150, None)),
    );
    a_step(
        &ledger,
        "run-1",
        "quattro",
        1,
        1,
        160,
        Some((Outcome::Broke, None, 170, None)),
    );

    let classes = ledger
        .failure_class_tally(None, 50)
        .expect("failure classes");

    assert_eq!(classes.len(), 3);
    assert_eq!(classes[0].failure_class.as_deref(), Some("timeout"));
    assert_eq!(classes[0].failures, 2);
    assert!(
        classes.iter().any(|c| c.failure_class.is_none()),
        "a breakage the engine did not classify keeps no class: {classes:?}"
    );
}

/// A broken attempt is counted, and does not enter the measure.
///
/// It falls if the durations collect any closed attempt: the long failure below
/// would shift the median, and a step that breaks after a hundred seconds would
/// simply look like a slow step.
#[test]
fn a_broken_attempt_is_counted_but_not_measured() {
    let directory = TestDirectory::new("history-durations");
    let ledger = Ledger::open(&directory.0).expect("open the ledger");
    a_run(&ledger, "run-1", "alpha", 100, Some(900));
    a_step(
        &ledger,
        "run-1",
        "compile",
        1,
        1,
        100,
        Some((Outcome::Went, None, 110, None)),
    );
    a_step(
        &ledger,
        "run-1",
        "compile",
        2,
        2,
        200,
        Some((Outcome::Went, None, 230, None)),
    );
    a_step(
        &ledger,
        "run-1",
        "compile",
        3,
        3,
        300,
        Some((Outcome::Broke, Some("timeout"), 800, None)),
    );

    let durations = ledger
        .step_durations("compile", Some("alpha"), 50)
        .expect("durations");

    assert_eq!(
        durations.seconds_sorted,
        vec![10, 30],
        "successful attempts only"
    );
    assert_eq!(
        durations.failed_samples, 1,
        "the broken one is still counted"
    );
    assert_eq!(
        durations.last_seconds,
        Some(30),
        "the last success, not the last close"
    );
}

/// The raw text leaves one run only, from broken steps only, and clipped.
///
/// The three assertions are three different things: that a neighbouring run
/// stays out, that a successful step does not carry its text along, and that
/// the clip is declared. The third falls if `truncated` becomes a fixed value:
/// a clipped diagnosis read as complete leads to a conclusion on the wrong bit.
#[test]
fn said_leaves_one_run_only_from_broken_steps_and_says_when_it_was_clipped() {
    let directory = TestDirectory::new("history-said");
    let ledger = Ledger::open(&directory.0).expect("open the ledger");
    a_run(&ledger, "run-1", "alpha", 100, Some(400));
    let long_text = "à".repeat(400);
    a_step(
        &ledger,
        "run-1",
        "broken",
        1,
        1,
        100,
        Some((Outcome::Broke, Some("timeout"), 150, Some(&long_text))),
    );
    a_step(
        &ledger,
        "run-1",
        "succeeded",
        1,
        1,
        160,
        Some((Outcome::Went, None, 170, Some("all fine"))),
    );
    a_run(&ledger, "run-2", "alpha", 500, Some(600));
    a_step(
        &ledger,
        "run-2",
        "elsewhere",
        1,
        1,
        500,
        Some((
            Outcome::Broke,
            Some("timeout"),
            550,
            Some("from another run"),
        )),
    );

    let excerpts = ledger.said_of_failed_steps("run-1", 5, 101).expect("said");

    assert_eq!(
        excerpts.len(),
        1,
        "one run only, and broken steps only: {excerpts:?}"
    );
    assert_eq!(excerpts[0].step_id, "broken");
    assert!(excerpts[0].truncated, "the clip is declared");
    assert_eq!(
        excerpts[0].said.len(),
        100,
        "the clip respects a character boundary: 101 bytes land mid-'à'"
    );
}

// ── the counts that may be unknown ───────────────────────────────────────

/// A call with whatever counts the caller wants.
fn call_with(call_id: &str, tokens: Option<u64>, cost: Option<i64>) -> ModelCallRecord {
    ModelCallRecord {
        call_id: call_id.to_owned(),
        run_id: "run-1".to_owned(),
        step_id: Some("compile".to_owned()),
        purpose: "external_engine".to_owned(),
        cli: "codex".to_owned(),
        requested_model: String::new(),
        actual_model: String::new(),
        input_tokens: tokens,
        output_tokens: tokens,
        cached_tokens: None,
        cache_write_tokens: None,
        cache_write_long_tokens: None,
        total_tokens: None,
        turns: None,
        cost_micros: cost,
        declared_cost_micros: None,
        price_currency: None,
        input_price_micros_per_million: None,
        output_price_micros_per_million: None,
        cached_price_micros_per_million: None,
        cache_write_price_micros_per_million: None,
        cache_write_long_price_micros_per_million: None,
        engine_identity: EngineIdentity::default(),
        retry_chain: vec![],
        error_type: None,
        started_at: 100,
        ended_at: Some(110),
        session_id: None,
        work_kind: None,
    }
}

/// **AN UNKNOWN SURVIVES THE ROUND TRIP TO DISK.** This is the point where, if
/// a column had a default value, a `None` would come back as `0` with nobody
/// noticing — and from then on it would be indistinguishable from a measure.
#[test]
fn an_unknown_count_stays_unknown_through_the_projection() {
    let directory = TestDirectory::new("unknown-tokens");
    let ledger = Ledger::open(&directory.0).expect("open the ledger");
    ledger
        .record_model_call(&call_with("ignota", None, None))
        .expect("record the unmeasured call");
    ledger
        .record_model_call(&call_with("misurata", Some(42), Some(7)))
        .expect("record the measured one");

    let dump = ledger.projection_dump().expect("read the projection");
    let rows = dump["model_calls"].as_array().expect("the list is there");
    assert_eq!(rows.len(), 2);
    let unknown = rows.iter().find(|row| row[0] == "ignota").unwrap();
    assert_eq!(
        unknown[7],
        Value::Null,
        "an unknown input_tokens stays NULL"
    );
    assert_eq!(
        unknown[8],
        Value::Null,
        "an unknown output_tokens stays NULL"
    );
    assert_eq!(
        unknown[10],
        Value::Null,
        "an unknown cost_micros stays NULL"
    );
    assert_ne!(unknown[7], json!("0"), "and never becomes a zero");

    let measured = rows.iter().find(|row| row[0] == "misurata").unwrap();
    assert_eq!(measured[7], json!("42"));
    assert_eq!(measured[10], json!(7));
}

/// The two columns born with version 4 reach the projection.
#[test]
fn the_total_and_the_declared_cost_reach_the_projection() {
    let directory = TestDirectory::new("new-columns");
    let ledger = Ledger::open(&directory.0).expect("open the ledger");
    let mut record = call_with("solo-totale", None, None);
    record.total_tokens = Some(13_910);
    record.declared_cost_micros = Some(4_200);
    ledger.record_model_call(&record).expect("record the call");

    let dump = ledger.projection_dump().expect("read the projection");
    let row = &dump["model_calls"][0];
    assert_eq!(row[20], json!("13910"), "total_tokens");
    assert_eq!(row[21], json!(4_200), "declared_cost_micros");
}

/// **AN ALREADY-WRITTEN STORE MIGRATES WITHOUT LOSING A ROW.** The old table
/// has `NOT NULL` on the counts and SQLite cannot drop that with an `ALTER`, so
/// the table is rebuilt and the rows copied across. If that transfer lost
/// anything, whoever upgrades Sailor would find their spending history halved
/// with no warning at all.
#[test]
fn an_older_ledger_is_migrated_in_place_without_losing_its_rows() {
    let directory = TestDirectory::new("migration");
    // Build a store by hand in the shape of version 3.
    {
        let ledger = Ledger::open(&directory.0).expect("open the ledger");
        let connection = ledger.connection.lock().expect("nobody panics here");
        connection
            .execute_batch(
                "DROP TABLE model_calls;
                 CREATE TABLE model_calls (
                     call_id TEXT PRIMARY KEY,
                     run_id TEXT NOT NULL,
                     step_id TEXT,
                     purpose TEXT NOT NULL,
                     cli TEXT NOT NULL,
                     requested_model TEXT NOT NULL,
                     actual_model TEXT NOT NULL,
                     input_tokens TEXT NOT NULL,
                     output_tokens TEXT NOT NULL,
                     cached_tokens TEXT NOT NULL,
                     cost_micros INTEGER NOT NULL,
                     price_currency TEXT NOT NULL,
                     input_price_micros_per_million INTEGER NOT NULL,
                     output_price_micros_per_million INTEGER NOT NULL,
                     cached_price_micros_per_million INTEGER NOT NULL,
                     mandate_name TEXT NOT NULL,
                     mandate_version TEXT NOT NULL,
                     retry_chain TEXT NOT NULL,
                     error_type TEXT,
                     started_at INTEGER NOT NULL,
                     ended_at INTEGER
                 );
                 INSERT INTO model_calls VALUES
                   ('vecchia', 'run-1', 'compile', 'repair', 'codex', 'req', 'act',
                    '10', '20', '3', 21, 'USD', 100, 200, 10, 'repair', 'v3',
                    '[]', NULL, 101, 110);",
            )
            .expect("build the old shape");
        connection
            .pragma_update(None, "user_version", 3i64)
            .expect("declare itself version 3");
    }

    // Reopening it brings it to the new shape.
    let ledger = Ledger::open(&directory.0).expect("reopen the ledger");
    let dump = ledger.projection_dump().expect("read the projection");
    let rows = dump["model_calls"].as_array().expect("the list is there");
    assert_eq!(rows.len(), 1, "the earlier row is still there");
    assert_eq!(rows[0][0], json!("vecchia"));
    assert_eq!(rows[0][7], json!("10"), "with its values intact");
    assert_eq!(rows[0][20], Value::Null, "the new columns are born unknown");
    // **AND THE OLD COLUMN'S TEXT IS NEITHER LOST NOR PROMOTED.** It was
    // `repair` under the name `mandate_name`; now it sits under
    // `engine_identity` and reads back as "unrecorded, the column said this".
    // Rewriting it as a declared profile would give a datum that already knew
    // how to lie the face of a measure.
    assert_eq!(rows[0][15], json!("repair"));

    // And now it accepts what it would have refused before.
    ledger
        .record_model_call(&call_with("nuova-ignota", None, None))
        .expect("an unmeasured call is now accepted");
    let dump = ledger.projection_dump().expect("read back");
    assert_eq!(dump["model_calls"].as_array().unwrap().len(), 2);
}

/// An event written when the counts were plain numbers still reads: `10` becomes
/// `Some(10)`, and a field that was not there becomes `None`. Without this,
/// upgrading Sailor would make the already-written event log unreadable — and
/// that log is the only thing everything else is rebuilt from.
#[test]
fn an_event_written_in_the_old_shape_still_deserialises() {
    let old_shape = json!({
        "call_id": "vecchia", "run_id": "run-1", "step_id": "compile",
        "purpose": "repair", "cli": "codex",
        "requested_model": "req", "actual_model": "act",
        "input_tokens": 10, "output_tokens": 20, "cached_tokens": 3,
        "cost_micros": 21, "price_currency": "USD",
        "input_price_micros_per_million": 100,
        "output_price_micros_per_million": 200,
        "cached_price_micros_per_million": 10,
        "mandate_name": "repair", "mandate_version": "v3",
        "retry_chain": [], "error_type": null,
        "started_at": 101, "ended_at": 110
    });
    let record: ModelCallRecord =
        serde_json::from_value(old_shape).expect("an old event still reads");
    assert_eq!(record.input_tokens, Some(10));
    assert_eq!(record.price_currency.as_deref(), Some("USD"));
    assert_eq!(
        record.total_tokens, None,
        "a field that was not there is unknown"
    );
    assert_eq!(record.declared_cost_micros, None);
    // **AND THE IDENTITY IS NOT INVENTED FROM AN EVENT THAT DOES NOT CARRY IT.**
    // That event has `mandate_name: "repair"`, which was the old field: reading
    // it as a declared profile would put into an old row a claim nobody made.
    assert_eq!(
        record.engine_identity,
        EngineIdentity::Unrecorded {
            legacy: String::new()
        },
        "an event written earlier carries no identity, and none is deduced"
    );
}

// ── what a run has spent ─────────────────────────────────────────────────

/// A call with its own run, to measure one run's spend and not another's.
fn call_in_run(call_id: &str, run_id: &str, cost: Option<i64>) -> ModelCallRecord {
    let mut record = call_with(call_id, Some(10), cost);
    record.run_id = run_id.to_owned();
    record
}

/// A call with a declared session, for the tests that follow.
fn call_with_session(call_id: &str, step_id: &str, cli: &str, session: &str) -> ModelCallRecord {
    let mut record = call_with(call_id, Some(10), Some(1));
    record.step_id = Some(step_id.to_owned());
    record.cli = cli.to_owned();
    record.session_id = Some(session.to_owned());
    record
}

/// **ONE ENGINE'S SESSION IS NOT HANDED TO ANOTHER ENGINE.** A step with a
/// chain may end up on `codex` because `claude-code` ran out; handing the next
/// step a `claude-code` session to resume under `codex` would pass it an id
/// that engine does not know, and the call would die **after** starting — that
/// is, after spending.
#[test]
fn a_session_belongs_to_the_engine_that_opened_it() {
    let directory = TestDirectory::new("whose-session");
    let ledger = Ledger::open(&directory.0).expect("open the ledger");
    ledger
        .record_model_call(&call_with_session(
            "call-1",
            "scopri",
            "un-motore",
            "sessione-1",
        ))
        .expect("record the call");

    assert_eq!(
        ledger
            .session_opened_by("run-1", "scopri", "un-motore")
            .expect("the ledger answers"),
        Some("sessione-1".to_owned())
    );
    assert_eq!(
        ledger
            .session_opened_by("run-1", "scopri", "un-altro-motore")
            .expect("the ledger answers"),
        None,
        "another engine does not inherit this one's session"
    );
    assert_eq!(
        ledger
            .session_opened_by("run-2", "scopri", "un-motore")
            .expect("the ledger answers"),
        None,
        "and neither does another run"
    );
}

/// A redone step opened two: the good one is **the latest**, and resuming the
/// first would mean continuing the conversation that had gone wrong.
#[test]
fn a_step_that_ran_twice_hands_over_its_latest_session() {
    let directory = TestDirectory::new("redone-session");
    let ledger = Ledger::open(&directory.0).expect("open the ledger");
    let mut first = call_with_session("call-1", "scopri", "un-motore", "sessione-vecchia");
    first.started_at = 100;
    let mut second = call_with_session("call-2", "scopri", "un-motore", "sessione-nuova");
    second.started_at = 200;
    ledger.record_model_call(&first).expect("the first call");
    ledger.record_model_call(&second).expect("the second call");

    assert_eq!(
        ledger
            .session_opened_by("run-1", "scopri", "un-motore")
            .expect("the ledger answers"),
        Some("sessione-nuova".to_owned())
    );
}

/// **THE SUM ALSO SAYS HOW MUCH IT DOES NOT KNOW.** Two calls, one that
/// declared its cost and one that did not: the total is the first one's, and
/// the second number says a row is outside the count. Looking only at `micros`
/// would read "the run cost 7" where the truth is "at least 7".
#[test]
fn what_a_run_spent_says_how_much_of_it_is_unknown() {
    let directory = TestDirectory::new("partial-spend");
    let ledger = Ledger::open(&directory.0).expect("open the ledger");
    ledger
        .record_model_call(&call_in_run("nota", "run-1", Some(7)))
        .expect("record the one with a cost");
    ledger
        .record_model_call(&call_in_run("ignota", "run-1", None))
        .expect("record the one without");

    let spend = ledger.spent_in_run("run-1").expect("read the spend");

    assert_eq!(spend.micros, 7, "it sums the known costs");
    assert_eq!(spend.calls, 2, "and counts every call, not just those");
    assert_eq!(spend.calls_without_cost, 1);
    assert!(
        !spend.is_complete(),
        "a total with a row left out is not complete, and the decider must know"
    );
}

/// A run's spend is **its own**: another run's calls do not enter.
///
/// Without this, a spend cap would close on a run for what its neighbour spent
/// — and the day's first flow would run while the last would never start.
#[test]
fn a_runs_spending_does_not_include_the_neighbours() {
    let directory = TestDirectory::new("whose-spend");
    let ledger = Ledger::open(&directory.0).expect("open the ledger");
    ledger
        .record_model_call(&call_in_run("mia", "run-1", Some(7)))
        .expect("record my own call");
    ledger
        .record_model_call(&call_in_run("altrui", "run-2", Some(1_000)))
        .expect("record the other run's call");

    let mine = ledger.spent_in_run("run-1").expect("read my own spend");

    assert_eq!(mine.micros, 7);
    assert_eq!(mine.calls, 1, "only one call is mine");
    assert!(mine.is_complete());
}

/// What one engine spent on a window is its own and the window's: another
/// engine's calls and this engine's older calls do not enter, across runs.
#[test]
fn an_engines_spend_on_a_window_leaves_out_the_others_and_the_older() {
    let directory = TestDirectory::new("engine-window-spend");
    let ledger = Ledger::open(&directory.0).expect("open the ledger");
    let mut recent = call_in_run("recente", "run-1", Some(7));
    recent.cli = "this-engine".to_owned();
    recent.started_at = 1_000;
    let mut other_run = call_in_run("altra-corsa", "run-2", Some(5));
    other_run.cli = "this-engine".to_owned();
    other_run.started_at = 1_500;
    let mut older = call_in_run("vecchia", "run-1", Some(1_000));
    older.cli = "this-engine".to_owned();
    older.started_at = 999;
    let mut another_engine = call_in_run("altrui", "run-1", Some(10_000));
    another_engine.cli = "another-engine".to_owned();
    another_engine.started_at = 1_500;
    for call in [&recent, &other_run, &older, &another_engine] {
        ledger.record_model_call(call).expect("record the call");
    }

    let spend = ledger.spent_by_cli_since("this-engine", 1_000).expect("read the spend");

    assert_eq!(spend.micros, 12, "the two calls in the window, across both runs");
    assert_eq!(spend.calls, 2);
    assert!(spend.is_complete());
}

/// A run that called no engine spent zero **and** hides nothing unknown: both
/// together, or "zero" would stay ambiguous.
#[test]
fn a_run_that_called_no_engine_spent_nothing_and_hides_nothing() {
    let directory = TestDirectory::new("empty-spend");
    let ledger = Ledger::open(&directory.0).expect("open the ledger");

    let spend = ledger.spent_in_run("never-ran").expect("read the spend");

    assert_eq!(spend, Spend::default());
    assert!(
        spend.is_complete(),
        "nothing unknown: there is nothing at all"
    );
}

/// A store written by the version before `work_kind` gains the column on
/// reopening and takes a call that names the kind: the first real run of the
/// sweep flow died on «no column named work_kind» because the schema version
/// had not moved with the column.
#[test]
fn a_store_from_before_the_work_kind_column_gains_it_on_reopening() {
    let directory = TestDirectory::new("before-work-kind");
    {
        let ledger = Ledger::open(&directory.0).expect("open the ledger");
        let connection = ledger.connection.lock().expect("nobody panics here");
        connection
            .execute_batch("ALTER TABLE model_calls DROP COLUMN work_kind;")
            .expect("the shape before the column");
        connection
            .pragma_update(None, "user_version", 8i64)
            .expect("declared at the version before it");
    }

    let ledger = Ledger::open(&directory.0).expect("reopen the ledger");
    let mut call = call_with("with-kind", Some(7), Some(11));
    call.work_kind = Some("mechanical".to_owned());
    ledger.record_model_call(&call).expect("a call naming its kind is written");
    let dump = ledger.projection_dump().expect("read the projection");
    let rows = dump["model_calls"].as_array().expect("the list is there");
    assert_eq!(rows[0][28], json!("mechanical"));
}

/// **A STORE THAT CLAIMS TO BE CURRENT WHEN IT IS NOT.**
///
/// Four columns were added while `PROJECTION_SCHEMA_VERSION` stayed at 4: an
/// existing store was already 4, `4 < 4` was false, and every read died with
/// `no such column: cache_write_tokens`. The version-3 test migrated anyway —
/// `3 < 4` is true — and a test store never migrates at all: both were blind.
#[test]
fn a_ledger_that_claims_to_be_current_but_lacks_the_new_columns_is_still_migrated() {
    let directory = TestDirectory::new("lying-version");
    {
        let ledger = Ledger::open(&directory.0).expect("open the ledger");
        let connection = ledger.connection.lock().expect("nobody panics here");
        // The version-4 shape: everything up to `declared_cost_micros`, and
        // none of the four cache-write columns.
        connection
            .execute_batch(
                "DROP TABLE model_calls;
                 CREATE TABLE model_calls (
                     call_id TEXT PRIMARY KEY,
                     run_id TEXT NOT NULL,
                     step_id TEXT,
                     purpose TEXT NOT NULL,
                     cli TEXT NOT NULL,
                     requested_model TEXT NOT NULL,
                     actual_model TEXT NOT NULL,
                     input_tokens TEXT,
                     output_tokens TEXT,
                     cached_tokens TEXT,
                     cost_micros INTEGER,
                     price_currency TEXT,
                     input_price_micros_per_million INTEGER,
                     output_price_micros_per_million INTEGER,
                     cached_price_micros_per_million INTEGER,
                     mandate_name TEXT NOT NULL,
                     mandate_version TEXT NOT NULL,
                     retry_chain TEXT NOT NULL,
                     error_type TEXT,
                     started_at INTEGER NOT NULL,
                     ended_at INTEGER,
                     total_tokens TEXT,
                     declared_cost_micros INTEGER
                 );
                 INSERT INTO model_calls VALUES
                   ('di-ieri', 'run-1', 'compile', 'repair', 'codex', 'req', 'act',
                    '10', '20', '3', 21, 'USD', 100, 200, 10, 'repair', 'v4',
                    '[]', NULL, 101, 110, NULL, NULL);",
            )
            .expect("build the version-4 shape");
        connection
            .pragma_update(None, "user_version", 4i64)
            .expect("and declare itself already current");
    }

    // Reopening must be enough: it is the only gesture an upgrader performs.
    // The case caught here is the one neither earlier test could see — version
    // already equal to the constant, columns missing — and it appears on no
    // fresh machine, only on one that had already used Sailor.
    let ledger = Ledger::open(&directory.0).expect("reopen the ledger");

    // The read is the point: it used to die here, not at open.
    let dump = ledger
        .projection_dump()
        .expect("read the projection of a store that claimed to be current");
    let rows = dump["model_calls"].as_array().expect("the list is there");
    assert_eq!(rows.len(), 1, "yesterday's row is still there");
    assert_eq!(rows[0][0], json!("di-ieri"));
    assert_eq!(rows[0][7], json!("10"), "with its values intact");
    assert_eq!(
        rows[0][22],
        Value::Null,
        "and the new columns are born unknown"
    );

    // And the spend sum, which is what `flow cost` asks for, now works: it was
    // this query that used to fail.
    let spend = ledger.spent_in_run("run-1").expect("read the spend");
    assert_eq!(spend.micros, 21);
    assert_eq!(spend.calls, 1);
}

/// **THE DUMP MUST CARRY EVERY COLUMN THE TABLE HAS.**
///
/// `dump_table` lists the columns **by hand**, and readers go **by position**:
/// a column added to the table and forgotten in the list breaks nothing, fails
/// no test, and simply does not exist downstream. So this test compares **how
/// many** columns a dump row carries against the table's own count.
#[test]
fn the_dump_carries_every_column_the_table_has() {
    // It happened with `turns`: the column was there, the migration had created
    // it, the write filled it, and `flow cost` showed zero — because the dump
    // did not ask for it. Counting instead of naming holds for every column
    // added later, with nobody having to remember this test exists.
    let directory = TestDirectory::new("dump-columns");
    let ledger = Ledger::open(&directory.0).expect("open the ledger");
    ledger
        .record_model_call(&call_with("call-colonne", Some(7), Some(11)))
        .expect("record a call");

    let dump = ledger.projection_dump().expect("the dump reads");
    let row = dump["model_calls"][0]
        .as_array()
        .expect("the row is there")
        .len();

    let connection = ledger.lock().expect("the connection");
    let mut statement = connection
        .prepare("SELECT COUNT(*) FROM pragma_table_info('model_calls')")
        .expect("query the shape of the table");
    let columns: i64 = statement
        .query_row([], |row| row.get(0))
        .expect("count the columns");

    assert_eq!(
        row as i64, columns,
        "the dump carries {row} columns and the table has {columns}: the missing ones \
         are invisible to everything that reads the dump, and no error says so"
    );
}

/// **A STORE COMING FROM AN OLD VERSION MUST END UP SHAPED LIKE A NEW ONE.**
///
/// The earlier test starts from version 4 and **cannot see** a column added
/// under version 6: the 5→6 migration worked by luck, not construction. This
/// names no column and does not age — a **migrated** store against a **newborn**
/// one — so a column entering `CREATE TABLE` with no `ALTER` turns it red.
#[test]
fn a_migrated_ledger_ends_up_shaped_exactly_like_a_fresh_one() {
    fn columns(ledger: &Ledger, table: &str) -> Vec<String> {
        let connection = ledger.lock().expect("the connection");
        let mut statement = connection
            .prepare(&format!(
                "SELECT name FROM pragma_table_info('{table}') ORDER BY cid"
            ))
            .expect("query the table shape");
        let names = statement
            .query_map([], |row| row.get::<_, String>(0))
            .expect("read the names")
            .collect::<Result<Vec<_>, _>>()
            .expect("valid names");
        names
    }

    let fresh_dir = TestDirectory::new("fresh-shape");
    let fresh = Ledger::open(&fresh_dir.0).expect("open a fresh ledger");
    let expected = columns(&fresh, "model_calls");

    // The version-3 shape, the one `relax_model_calls` rebuilds the table with:
    // **frozen**, because version 3 will never gain a column.
    //
    // **THE LIST IS DERIVED, NOT ENUMERATED**, and this test once caught itself:
    // it listed the columns **to drop**, so a column added at version 7 stayed
    // in the "old" store and it went red over its own scaffolding, not the code.
    const AS_OF_VERSION_3: &[&str] = &[
        "call_id",
        "run_id",
        "step_id",
        "purpose",
        "cli",
        "requested_model",
        "actual_model",
        "input_tokens",
        "output_tokens",
        "cached_tokens",
        "cost_micros",
        "price_currency",
        "input_price_micros_per_million",
        "output_price_micros_per_million",
        "cached_price_micros_per_million",
        "mandate_name",
        "mandate_version",
        "retry_chain",
        "error_type",
        "started_at",
        "ended_at",
        "total_tokens",
        "declared_cost_micros",
    ];

    // A store of the oldest version the migration can still pick up, declared
    // as such.
    let old_dir = TestDirectory::new("old-shape");
    {
        let ledger = Ledger::open(&old_dir.0).expect("open the ledger");
        let connection = ledger.connection.lock().expect("nobody panics here");
        // **VERSION 3 CALLED THAT COLUMN SOMETHING ELSE, AND HAD ONE MORE.**
        // Dropping columns no longer rebuilds it: version 8 renamed
        // `mandate_name` to `engine_identity` and dropped `mandate_version`. If
        // this test started from the new shape and only dropped columns, it
        // would exercise a migration that does not exist.
        connection
            .execute(
                "ALTER TABLE model_calls RENAME COLUMN engine_identity TO mandate_name",
                [],
            )
            .expect("put back the name version 3 used");
        connection
            .execute(
                "ALTER TABLE model_calls ADD COLUMN mandate_version TEXT NOT NULL DEFAULT ''",
                [],
            )
            .expect("and the extra column version 3 had");
        // Look at the columns **as they are now**, not at the new shape: after
        // the rename the two lists no longer match. The query goes through this
        // connection, already in hand: `columns` would take a second one on the
        // same lock, and that lock is not reentrant.
        let now: Vec<String> = {
            let mut statement = connection
                .prepare("SELECT name FROM pragma_table_info('model_calls') ORDER BY cid")
                .expect("query the current shape");
            let names = statement
                .query_map([], |row| row.get::<_, String>(0))
                .expect("read the names")
                .collect::<Result<Vec<_>, _>>()
                .expect("valid names");
            names
        };
        for column in now
            .iter()
            .filter(|name| !AS_OF_VERSION_3.contains(&name.as_str()))
        {
            connection
                .execute(&format!("ALTER TABLE model_calls DROP COLUMN {column}"), [])
                .unwrap_or_else(|error| panic!("drop {column}: {error}"));
        }
        connection
            .pragma_update(None, "user_version", 3i64)
            .expect("and declare itself three versions back");
    }

    let migrated = Ledger::open(&old_dir.0).expect("reopen the old ledger");
    assert_eq!(
        columns(&migrated, "model_calls"),
        expected,
        "a migrated store is not shaped like a fresh one: a column was added to \
         the CREATE TABLE without its ALTER, and on every machine that already \
         had a store that column will never exist"
    );
}

// ---------------------------------------------------------------------------
// The processes Sailor starts.
// ---------------------------------------------------------------------------

fn spawned(process_id: &str, port: Option<u16>) -> ProcessRecord {
    ProcessRecord {
        process_id: process_id.to_owned(),
        pid: 4242,
        command: "npm".to_owned(),
        args: vec!["run".to_owned(), "dev".to_owned()],
        working_directory: "/work/sailor/desktop".to_owned(),
        port,
        purpose: "live".to_owned(),
        started_by: "supervisor".to_owned(),
        run_id: None,
        started_at: 1_700_000_000,
    }
}

/// **A STARTED PROCESS STAYS WRITTEN EVEN AFTER ITS STARTER IS GONE.**
///
/// The orphan was found *the day after*, by another person, and nobody knew who
/// had turned it on. A register living in memory inside the window would not
/// have answered — the window was closed. Here the store is reopened from
/// scratch, which is what whoever arrives later does.
#[test]
fn a_started_process_survives_the_window_that_started_it() {
    let directory = TestDirectory::new("live-processes");

    {
        let ledger = Ledger::open(&directory.0).expect("open the ledger");
        ledger
            .record_process_started(&spawned("vite-1", Some(5183)))
            .expect("record the started process");
    }

    // No in-memory state survives this line: it is a different `Ledger`.
    let later = Ledger::open(&directory.0).expect("reopen the ledger tomorrow");
    let left = later
        .processes_left_running()
        .expect("ask what is left running");
    assert_eq!(left.len(), 1, "the orphan is not in the store: {left:?}");
    assert_eq!(left[0].process_id, "vite-1");
    assert_eq!(left[0].pid, 4242);
    assert_eq!(left[0].port, Some(5183));
    assert_eq!(left[0].command, "npm");
    assert_eq!(left[0].args, vec!["run".to_owned(), "dev".to_owned()]);
    assert_eq!(
        left[0].started_by, "supervisor",
        "without who started it, whoever finds it does not know whom to ask"
    );
}

/// A closed process leaves the list. Without this the list only ever grows, and
/// whoever reads it stops believing it — which is the same as not having it.
#[test]
fn a_closed_process_leaves_the_list() {
    let directory = TestDirectory::new("closed-processes");
    let ledger = Ledger::open(&directory.0).expect("open the ledger");

    ledger
        .record_process_started(&spawned("vite-1", Some(5183)))
        .expect("record the start");
    ledger
        .record_process_started(&spawned("vite-2", Some(5184)))
        .expect("record the second start");
    ledger
        .record_process_ended(&ProcessEndRecord {
            process_id: "vite-1".to_owned(),
            exit_code: Some(0),
            ended_at: 1_700_000_060,
        })
        .expect("record the close");

    let left = ledger
        .processes_left_running()
        .expect("ask what is left running");
    let ids: Vec<&str> = left
        .iter()
        .map(|record| record.process_id.as_str())
        .collect();
    assert_eq!(
        ids,
        vec!["vite-2"],
        "the one that exited stayed in the list"
    );
}

/// **THE QUESTION THAT CAUSED THE FAULT WAS ABOUT THE PORT**, not the process:
/// "who holds 5183 and is stopping me from starting". The store must answer by
/// that key, or whoever is blocked has to read the whole list and guess.
#[test]
fn the_ledger_answers_who_holds_a_port() {
    let directory = TestDirectory::new("process-ports");
    let ledger = Ledger::open(&directory.0).expect("open the ledger");

    ledger
        .record_process_started(&spawned("vite-1", Some(5183)))
        .expect("record the start");
    ledger
        .record_process_started(&spawned("senza-porta", None))
        .expect("record a process that holds nothing");

    let holder = ledger
        .process_holding_port(5183)
        .expect("ask who holds the port");
    assert_eq!(
        holder.map(|record| record.process_id),
        Some("vite-1".to_owned())
    );

    assert!(
        ledger
            .process_holding_port(9999)
            .expect("ask about a free port")
            .is_none(),
        "the store invented a holder for a free port"
    );

    // And once it is closed the port reads as free: that is the signal telling
    // whoever comes next they can start without killing anything.
    ledger
        .record_process_ended(&ProcessEndRecord {
            process_id: "vite-1".to_owned(),
            exit_code: Some(0),
            ended_at: 1_700_000_060,
        })
        .expect("record the close");
    assert!(
        ledger
            .process_holding_port(5183)
            .expect("ask again after the close")
            .is_none(),
        "the port still reads as held by a closed process"
    );
}

/// **NOT `pgrep`, AND THAT IS THE POINT.** Inside some sandboxes `pgrep` does
/// not see the processes and **answers empty without an error**: an empty list
/// is indistinguishable from "nobody is there". Here we ask about a single pid,
/// known because the store wrote it, and the answer is a yes or a no about that
/// pid — there is no "empty list" shape for that defect to hide in.
#[test]
fn liveness_asks_about_one_known_pid_not_for_a_list() {
    // This process is alive by definition: it is the one running the test.
    assert!(
        pid_is_alive(std::process::id()),
        "the process that is running reads as dead"
    );

    // A reaped child really is dead, and the system knows it until somebody
    // reuses the number.
    let mut child = Command::new("/bin/sh")
        .args(["-c", "exit 0"])
        .spawn()
        .expect("start a child that dies immediately");
    let pid = child.id();
    child.wait().expect("wait for it");
    assert!(
        !pid_is_alive(pid),
        "a reaped and buried process still reads as alive: pid {pid}"
    );
}

/// **A BROWSER IS NOT A BACK DOOR.** The store is append-only; a person may
/// look at any table through one typed statement, and a statement that would
/// write is refused by SQLite itself, whatever it looked like.
#[test]
fn a_browsed_statement_reads_any_table_and_cannot_write() {
    let directory = TestDirectory::new("browse");
    let ledger = Ledger::open(&directory.0).expect("open the ledger");
    sample_all(&ledger);

    let tables = ledger.tables().expect("list the tables");
    let runs = tables
        .iter()
        .find(|table| table.name == "runs")
        .expect("the runs table is listed");
    assert_eq!(runs.rows, 1, "{tables:?}");

    let answer = ledger
        .browse("SELECT run_id, status FROM runs", 10)
        .expect("a select is answered");
    assert_eq!(answer.columns, vec!["run_id", "status"]);
    assert_eq!(
        answer.rows,
        vec![vec![
            serde_json::Value::from("run-1"),
            serde_json::Value::from("broken")
        ]]
    );
    assert!(!answer.truncated);

    let cut = ledger.browse("SELECT * FROM steps", 0).expect("a limit of zero");
    assert!(cut.rows.is_empty() && cut.truncated, "{cut:?}");

    // The absurd control: a write through the browser is refused, and the row
    // is still there afterwards.
    let refused = ledger.browse("DELETE FROM runs", 10);
    assert!(refused.is_err(), "a delete went through the browser: {refused:?}");
    let still = ledger
        .browse("SELECT COUNT(*) FROM runs", 1)
        .expect("count after the refusal");
    assert_eq!(still.rows[0][0], serde_json::Value::from(1));
    // And an ordinary write still works once the browser is done.
    ledger
        .record_run(&RunRecord {
            run_id: "run-2".to_owned(),
            kind: "maintenance".to_owned(),
            entity: "repository".to_owned(),
            parent_run_id: None,
            started_by: "person".to_owned(),
            status: "complete".to_owned(),
            total_cost_micros: 0,
            error: None,
            started_at: 200,
            ended_at: Some(201),
            worktree: None,
        })
        .expect("the store still writes");
}

/// **EVERYTHING A WORKSPACE OWNS COULD BE ASKED FOR BY TREE EXCEPT ITS RUNS.**
/// `runs` had kind, entity, who started it, status, cost and times — and no
/// answer to «where», so the window showed every tree's runs mixed and no
/// filter could be written for them at all. The answer is optional on purpose:
/// a run started outside every workspace is a real run, and outside is a place.
#[test]
fn a_run_comes_back_with_the_tree_it_was_born_in() {
    let home = TestDirectory::new("run-tree");
    let ledger = Ledger::open(&home.0).expect("open the ledger");

    ledger
        .record_run(&born_in("in-a-tree", Some("/t/un-progetto/un-albero")))
        .expect("record it");
    ledger
        .record_run(&born_in("out-there", None))
        .expect("record it");

    let inside = ledger
        .run_header("in-a-tree")
        .expect("read it")
        .expect("it is there");
    assert_eq!(
        inside.worktree.as_deref(),
        Some("/t/un-progetto/un-albero"),
        "a run recorded in a tree came back without it"
    );

    let outside = ledger
        .run_header("out-there")
        .expect("read it")
        .expect("it is there");
    assert_eq!(
        outside.worktree, None,
        "outside every workspace is an answer, not a value to invent"
    );
}

/// **WHERE A RUN WAS BORN DOES NOT CHANGE WHEN IT ENDS.** The row that closes a
/// run is written by whatever process stands there at the time, and for a run
/// that outlives a `work_here` that is another tree — or none. Without the
/// `COALESCE` the closing row overwrote the birthplace with `NULL`, which reads
/// exactly like «this run happened nowhere».
#[test]
fn closing_a_run_from_elsewhere_does_not_erase_where_it_started() {
    let home = TestDirectory::new("run-tree-close");
    let ledger = Ledger::open(&home.0).expect("open the ledger");

    ledger
        .record_run(&born_in("long-one", Some("/t/un-progetto/un-albero")))
        .expect("record it");

    let mut closing = born_in("long-one", None);
    closing.ended_at = Some(99);
    ledger.record_run(&closing).expect("close it");

    let found = ledger
        .run_header("long-one")
        .expect("read it")
        .expect("it is there");
    assert_eq!(found.ended_at, Some(99), "the closing row did not land");
    assert_eq!(
        found.worktree.as_deref(),
        Some("/t/un-progetto/un-albero"),
        "closing the run erased where it was born"
    );
}

fn born_in(run_id: &str, worktree: Option<&str>) -> RunRecord {
    RunRecord {
        run_id: run_id.to_owned(),
        kind: "flow".to_owned(),
        entity: "sweep-the-tree".to_owned(),
        parent_run_id: None,
        started_by: "the window".to_owned(),
        status: "complete".to_owned(),
        total_cost_micros: 0,
        error: None,
        started_at: 10,
        ended_at: Some(20),
        worktree: worktree.map(str::to_owned),
    }
}

/// **AN EXISTING STORE MUST GAIN THE COLUMN, NOT REFUSE TO OPEN.** The one on
/// this machine is at version 9 and holds months of runs; the version is what
/// tells `Ledger::open` to add what is missing, and a column added without
/// moving it is the failure written at the top of that constant. Simulated by
/// taking a fresh store back to 9: dropping the column and the version is
/// exactly the shape of a store that never had it.
#[test]
fn a_store_written_before_the_column_gains_it_on_open() {
    let home = TestDirectory::new("run-tree-old");
    {
        let ledger = Ledger::open(&home.0).expect("open the ledger");
        ledger
            .record_run(&born_in("from-before", Some("/t/un-progetto/un-albero")))
            .expect("record it");
    }

    {
        let connection = Connection::open(home.0.join(STATE_FILE)).expect("open by hand");
        connection
            .execute_batch("ALTER TABLE runs DROP COLUMN worktree;")
            .expect("take the column away");
        connection
            .pragma_update(None, "user_version", 9_i64)
            .expect("take the version back");
    }

    let reopened = Ledger::open(&home.0).expect("an old store must still open");
    let found = reopened
        .run_header("from-before")
        .expect("read it")
        .expect("the run survived");
    assert_eq!(
        found.worktree, None,
        "a run written before the column knows no tree, and inventing one is worse than none"
    );

    reopened
        .record_run(&born_in("from-after", Some("/t/un-progetto/un-altro")))
        .expect("record a new one");
    assert_eq!(
        reopened
            .run_header("from-after")
            .expect("read it")
            .expect("it is there")
            .worktree
            .as_deref(),
        Some("/t/un-progetto/un-altro"),
        "the migrated store cannot record a tree"
    );
}

/// **A NUMBER THAT IS NOT A PID IS NOT ASKED ABOUT.** Read signed, anything
/// below 1 addresses a process group or everybody, so a stored number past a
/// positive `i32` would answer «alive» about whoever is running.
#[test]
fn a_pid_outside_the_range_of_a_pid_is_not_alive() {
    assert!(super::pid_is_alive(std::process::id()), "this process is running");
    assert!(!super::pid_is_alive(0), "zero is the caller's own group");
    assert!(!super::pid_is_alive(u32::MAX), "read signed, this addresses everybody");
    assert!(!super::pid_is_alive(i32::MAX as u32 + 1), "the first number past the range");
}

fn a_refusal() -> flow::Refusal {
    flow::Refusal::new(
        "answer_shape",
        "$.verdict",
        flow::RefusalRule::NotAllowed,
        "\"remvoe\"",
    )
}

/// The refusal closes with the step and reads back whole — from the rows and
/// from the dump the window reads — and a rebuild from the log keeps it.
#[test]
fn a_refusal_closes_with_the_step_and_reads_back() {
    let directory = TestDirectory::new("refusal");
    let ledger = Ledger::open(&directory.0).expect("open the ledger");
    ledger
        .append_step_started(&started("run-1"))
        .expect("start the step");
    ledger
        .close_step(
            "run-1",
            "compile",
            1,
            7,
            Completion {
                refusal: Some(a_refusal()),
                ..completion()
            },
        )
        .expect("close it refused");

    let steps = ledger.steps("run-1").expect("read the steps");
    assert_eq!(steps[0].refusal, Some(a_refusal()));
    let dump = ledger.projection_dump().expect("read the projection");
    let row = &dump["steps"].as_array().expect("rows")[0];
    let in_dump: flow::Refusal =
        serde_json::from_str(row[20].as_str().expect("the last column is the refusal"))
            .expect("the column holds the refusal as JSON");
    assert_eq!(in_dump, a_refusal());

    ledger.rebuild_projections().expect("rebuild from the log");
    let steps = ledger.steps("run-1").expect("read the rebuilt steps");
    assert_eq!(steps[0].refusal, Some(a_refusal()));
}

/// A store written before refusals existed opens, reads its rows without one,
/// and takes a refusal from then on.
#[test]
fn a_store_from_before_refusals_opens_and_reads_its_steps() {
    let directory = TestDirectory::new("before-refusals");
    {
        let ledger = Ledger::open(&directory.0).expect("open the ledger");
        ledger
            .append_step_started(&started("run-1"))
            .expect("start the step");
        ledger
            .close_step("run-1", "compile", 1, 7, completion())
            .expect("close it");
        let connection = ledger.connection.lock().expect("nobody panics here");
        connection
            .execute_batch("ALTER TABLE steps DROP COLUMN refusal")
            .expect("take the column away");
        connection
            .pragma_update(None, "user_version", 10i64)
            .expect("declare itself version 10");
    }

    let reopened = Ledger::open(&directory.0).expect("reopen the older store");
    let steps = reopened.steps("run-1").expect("read the older rows");
    assert_eq!(steps.len(), 1);
    assert_eq!(steps[0].refusal, None, "a row written before the column has none");

    reopened
        .append_step_started(&started("run-2"))
        .expect("start a new step");
    reopened
        .close_step(
            "run-2",
            "compile",
            1,
            7,
            Completion {
                refusal: Some(a_refusal()),
                ..completion()
            },
        )
        .expect("the migrated store takes a refusal");
    let steps = reopened.steps("run-2").expect("read it back");
    assert_eq!(steps[0].refusal, Some(a_refusal()));
}
