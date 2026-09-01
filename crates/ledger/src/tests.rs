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
        std::fs::create_dir_all(&path).expect("creare la cartella di prova");
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
        said: Some("errore grezzo".to_owned()),
        failure_class: Some("compiler_error".to_owned()),
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
        })
        .expect("registrare la corsa");
    ledger
        .append_step_started(&started("run-1"))
        .expect("registrare l'intenzione");
    ledger
        .close_step("run-1", "compile", 1, 7, completion())
        .expect("chiudere il passo");
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
            },
            retry_chain: vec!["call-0".to_owned()],
            error_type: Some("rate_limit".to_owned()),
            started_at: 101,
            ended_at: Some(110),
            session_id: None,
        })
        .expect("registrare la chiamata");
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
        .expect("registrare l'istantanea");
}

#[test]
fn step_record_round_trips_without_losing_nulls_or_columns() {
    let directory = TestDirectory::new("round-trip");
    let ledger = Ledger::open(&directory.0).expect("aprire il deposito");
    let record = started("run-null");
    ledger
        .append_step_started(&record)
        .expect("scrivere il record");
    assert_eq!(ledger.steps("run-null").expect("rileggere"), vec![record]);

    let connection = ledger.lock().expect("connessione");
    let payload: String = connection
        .query_row(
            "SELECT payload FROM events.events WHERE kind = 'step_started'",
            [],
            |row| row.get(0),
        )
        .expect("evento");
    let value: Value = serde_json::from_str(&payload).expect("json");
    let object = value["record"].as_object().expect("record oggetto");
    for field in [
        "outcome",
        "output",
        "said",
        "failure_class",
        "ended_at",
        "bytes_seen",
        "bytes_discarded",
    ] {
        assert!(object.contains_key(field), "manca il campo {field}");
        assert!(object[field].is_null(), "{field} non è nullo");
    }
}

#[test]
fn changed_gates_on_same_input_are_queryable_as_a_resume_condition() {
    let directory = TestDirectory::new("changed-gates");
    let ledger = Ledger::open(&directory.0).expect("aprire il deposito");
    let first = started_attempt("run-gates", 1, 7);
    ledger
        .append_step_started(&first)
        .expect("scrivere il primo tentativo");

    let mut resumed = started_attempt("run-gates", 2, 8);
    resumed.gates = vec!["filesystem".to_owned()];
    resumed.attempt_relation = Some(flow::AttemptRelation::SameInputGatesChanged);
    assert_eq!(first.input_digest, resumed.input_digest);
    ledger
        .append_step_started(&resumed)
        .expect("scrivere la ripresa");

    assert_eq!(
        ledger
            .steps_resumed_with_changed_gates()
            .expect("interrogare le riprese"),
        vec![GatesChangedStep {
            run_id: "run-gates".to_owned(),
            step_id: "compile".to_owned(),
            attempt: 2,
            epoch: 8,
        }]
    );
    assert_eq!(ledger.steps("run-gates").expect("rileggere")[1], resumed);
}

#[test]
fn stopped_and_skipped_outcomes_round_trip_through_the_operational_column() {
    let directory = TestDirectory::new("outcomes");
    let ledger = Ledger::open(&directory.0).expect("aprire il deposito");
    for (run_id, outcome, stored_name) in [
        ("run-stopped", Outcome::Stopped, "Stopped"),
        ("run-skipped", Outcome::Skipped, "Skipped"),
    ] {
        ledger
            .append_step_started(&started(run_id))
            .expect("scrivere il record");
        let mut completion = completion();
        completion.outcome = outcome;
        ledger
            .close_step(run_id, "compile", 1, 7, completion)
            .expect("chiudere il passo");
        assert_eq!(
            ledger.steps(run_id).expect("rileggere")[0].outcome,
            Some(outcome)
        );
        let connection = ledger.lock().expect("connessione");
        let stored: String = connection
            .query_row(
                "SELECT outcome FROM steps WHERE run_id = ?1",
                [run_id],
                |row| row.get(0),
            )
            .expect("leggere la colonna operativa");
        assert_eq!(stored, stored_name);
    }
}

#[test]
fn two_processes_using_ledger_api_serialize_writers() {
    let directory = TestDirectory::new("concurrency");
    Ledger::open(&directory.0).expect("inizializzare il deposito");
    let marker = directory.0.join("writer-a-read");
    let mut first = helper_command("writer")
        .env("LEDGER_DIRECTORY", &directory.0)
        .env("LEDGER_RUN_ID", "writer-a")
        .env("LEDGER_TEST_STEP_READ_MARKER", &marker)
        .env("LEDGER_TEST_STEP_READ_HOLD_MILLIS", "500")
        .spawn()
        .expect("avviare il primo processo");
    wait_for(&marker);
    let began = Instant::now();
    let second = helper_command("writer")
        .env("LEDGER_DIRECTORY", &directory.0)
        .env("LEDGER_RUN_ID", "writer-b")
        .status()
        .expect("avviare il secondo processo");
    let waited = began.elapsed();
    let first = first.wait().expect("attendere il primo processo");
    assert!(first.success(), "il primo scrittore è morto: {first}");
    assert!(second.success(), "il secondo scrittore è morto: {second}");
    assert!(
        waited >= Duration::from_millis(300),
        "il secondo non ha atteso"
    );
}

#[test]
fn explicit_projection_rebuild_is_identical_and_skipped_event_control_differs() {
    let directory = TestDirectory::new("rebuild");
    let ledger = Ledger::open(&directory.0).expect("aprire il deposito");
    sample_all(&ledger);
    let expected = ledger.projection_dump().expect("leggere le proiezioni");

    rebuild_skipping(&ledger, Some("step_closed")).expect("ricostruzione rotta");
    let broken = ledger.projection_dump().expect("leggere il controllo");
    assert_ne!(
        broken, expected,
        "saltare l'evento di chiusura doveva cambiare la proiezione"
    );
    ledger
        .rebuild_projections()
        .expect("ricostruire esplicitamente");
    assert_eq!(
        ledger.projection_dump().expect("leggere il risultato"),
        expected
    );
}

#[test]
fn committed_event_is_projected_after_a_crash_between_the_two_phases() {
    let directory = TestDirectory::new("checkpoint");
    let ledger = Ledger::open(&directory.0).expect("aprire il deposito");
    ledger
        .append_step_started(&started("run-crash"))
        .expect("registrare intenzione");
    let crashed = helper_command("crash_before_checkpoint_commit")
        .env("LEDGER_DIRECTORY", &directory.0)
        .env("LEDGER_TEST_CRASH_AFTER_CLOSE_EVENT", "1")
        .status()
        .expect("avviare il processo interrotto");
    assert!(
        !crashed.success(),
        "il processo iniettato doveva interrompersi"
    );
    let reopened = Ledger::open(&directory.0).expect("riaprire dopo lo schianto");
    assert!(reopened
        .is_checkpointed("run-crash", "compile", 1)
        .expect("leggere il checkpoint"));
    assert_eq!(
        reopened.steps("run-crash").expect("leggere il passo")[0].outcome,
        Some(Outcome::Broke)
    );
    assert!(!would_relaunch(&reopened, "run-crash"));
}

#[test]
fn stale_epoch_cannot_close_superseded_attempt() {
    let directory = TestDirectory::new("epoch-fencing");
    let ledger = Ledger::open(&directory.0).expect("aprire il deposito");
    ledger
        .append_step_started(&started_attempt("run-epoch", 1, 5))
        .expect("avviare il primo tentativo");
    ledger
        .append_step_started(&started_attempt("run-epoch", 2, 6))
        .expect("avviare il tentativo sostitutivo");

    let error = ledger
        .close_step("run-epoch", "compile", 1, 5, completion())
        .expect_err("l'epoca superata non deve chiudersi");
    assert!(matches!(error, LedgerError::StaleEpoch { epoch: 5, .. }));
    let steps = ledger.steps("run-epoch").expect("rileggere i tentativi");
    assert!(steps.iter().all(|record| record.outcome.is_none()));
}

#[test]
fn current_epoch_cannot_close_a_previous_attempt() {
    let directory = TestDirectory::new("attempt-epoch-fencing");
    let ledger = Ledger::open(&directory.0).expect("aprire il deposito");
    ledger
        .append_step_started(&started_attempt("run-epoch-pair", 1, 5))
        .expect("avviare il primo tentativo");
    ledger
        .append_step_started(&started_attempt("run-epoch-pair", 2, 6))
        .expect("avviare il tentativo corrente");

    let error = ledger
        .close_step("run-epoch-pair", "compile", 1, 6, completion())
        .expect_err("tentativo ed epoca devono identificare lo stesso record");
    assert!(matches!(
        error,
        LedgerError::MissingAttempt { attempt: 1, .. }
    ));
    assert!(ledger
        .steps("run-epoch-pair")
        .expect("rileggere i tentativi")
        .iter()
        .all(|record| record.outcome.is_none()));
}

#[test]
fn reopening_twice_does_not_read_or_reapply_old_events() {
    let directory = TestDirectory::new("incremental-open");
    let ledger = Ledger::open(&directory.0).expect("aprire il deposito");
    sample_all(&ledger);
    let expected = ledger.projection_dump().expect("leggere lo stato");
    drop(ledger);

    APPLIED_EVENT_READS.set(0);
    let first = Ledger::open(&directory.0).expect("prima riapertura");
    assert_eq!(
        APPLIED_EVENT_READS.get(),
        0,
        "la prima riapertura ha riletto eventi"
    );
    drop(first);
    let second = Ledger::open(&directory.0).expect("seconda riapertura");
    assert_eq!(
        APPLIED_EVENT_READS.get(),
        0,
        "la seconda riapertura ha riletto eventi vecchi"
    );
    assert_eq!(
        second.projection_dump().expect("rileggere lo stato"),
        expected
    );
}

#[test]
fn pruning_applied_events_preserves_projections_on_reopen() {
    let directory = TestDirectory::new("pruned-log");
    let ledger = Ledger::open(&directory.0).expect("aprire il deposito");
    sample_all(&ledger);
    let expected = ledger.projection_dump().expect("leggere lo stato");
    drop(ledger);

    let events = Connection::open(directory.0.join(EVENTS_FILE)).expect("aprire il registro");
    events
        .execute_batch(
            "DROP TRIGGER events_append_only_delete;
             DELETE FROM events;",
        )
        .expect("potare gli eventi già incorporati");
    drop(events);

    let reopened = Ledger::open(&directory.0).expect("riaprire dopo la potatura");
    assert_eq!(
        reopened.projection_dump().expect("rileggere lo stato"),
        expected
    );
}

#[test]
fn event_log_rejects_update_delete_and_upsert_update() {
    let directory = TestDirectory::new("append-only");
    let ledger = Ledger::open(&directory.0).expect("aprire il deposito");
    ledger
        .append_step_started(&started("run-append-only"))
        .expect("aggiungere un evento");
    let connection = ledger.lock().expect("connessione");
    let seq: i64 = connection
        .query_row("SELECT seq FROM events.events LIMIT 1", [], |row| {
            row.get(0)
        })
        .expect("sequenza esistente");

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
    let ledger = Ledger::open(&directory.0).expect("aprire il deposito");
    sample_all(&ledger);
    let connection = ledger.lock().expect("connessione");
    assert_eq!(pragma_text(&connection, "main", "journal_mode"), "wal");
    assert_eq!(pragma_text(&connection, "events", "journal_mode"), "wal");
    assert_eq!(pragma_i64(&connection, "main", "synchronous"), 2);
    assert_eq!(pragma_i64(&connection, "events", "synchronous"), 1);
    assert_eq!(pragma_i64(&connection, "main", "busy_timeout"), 5_000);
    drop(connection);
    let failures = ledger
        .failed_steps_in_recent_runs("compiler_error", 50)
        .expect("interrogare i fallimenti");
    assert_eq!(failures.len(), 1);
    assert_eq!(failures[0].step_id, "compile");
}

#[test]
fn tracing_layer_appends_structured_fields() {
    let directory = TestDirectory::new("tracing");
    let ledger = Ledger::open(&directory.0).expect("aprire il deposito");
    let subscriber =
        tracing_subscriber::registry().with(SqliteLayer::with_clock(ledger.clone(), || 444));
    tracing::subscriber::with_default(subscriber, || {
        tracing::info!(
            run_id = "run-trace",
            attempt = 2_u64,
            ready = true,
            "evento"
        );
    });
    let connection = ledger.lock().expect("connessione");
    let payload: String = connection
        .query_row(
            "SELECT payload FROM events.events WHERE kind = 'trace'",
            [],
            |row| row.get(0),
        )
        .expect("traccia");
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
        other => panic!("azione ausiliaria ignota: {other}"),
    }
}

fn helper_command(action: &str) -> Command {
    let mut command = Command::new(std::env::current_exe().expect("eseguibile di prova"));
    command
        .arg("--exact")
        .arg("tests::process_helper")
        .arg("--nocapture")
        .env("LEDGER_HELPER_ACTION", action);
    command
}

fn writer_helper() {
    let directory = PathBuf::from(std::env::var_os("LEDGER_DIRECTORY").expect("directory"));
    let run_id = std::env::var("LEDGER_RUN_ID").expect("identificatore corsa");
    let ledger = Ledger::open(directory).expect("aprire il deposito");
    ledger
        .append_step_started(&started(&run_id))
        .expect("scrivere tramite Ledger::append_step_started");
}

fn crash_before_checkpoint_commit() -> ! {
    let directory = PathBuf::from(std::env::var_os("LEDGER_DIRECTORY").expect("directory"));
    let ledger = Ledger::open(directory).expect("aprire deposito");
    ledger
        .close_step("run-crash", "compile", 1, 7, completion())
        .expect("il punto d'iniezione deve interrompere close_step");
    panic!("close_step è tornata senza lo schianto iniettato")
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
        .expect("leggere checkpoint")
}

fn wait_for(path: &Path) {
    let deadline = Instant::now() + Duration::from_secs(5);
    while !path.exists() {
        assert!(
            Instant::now() < deadline,
            "segnale non arrivato: {}",
            path.display()
        );
        thread::sleep(Duration::from_millis(10));
    }
}

fn pragma_text(connection: &Connection, database: &str, pragma: &str) -> String {
    connection
        .query_row(&format!("PRAGMA {database}.{pragma}"), [], |row| row.get(0))
        .expect("leggere pragma")
}

fn pragma_i64(connection: &Connection, database: &str, pragma: &str) -> i64 {
    connection
        .query_row(&format!("PRAGMA {database}.{pragma}"), [], |row| row.get(0))
        .expect("leggere pragma")
}

#[test]
fn steps_with_discarded_output_are_queryable_and_match_declared_values() {
    let directory = TestDirectory::new("discarded-output");
    let ledger = Ledger::open(&directory.0).expect("aprire il deposito");
    let record = started("run-discard");
    ledger
        .append_step_started(&record)
        .expect("scrivere intenzione");
    let mut comp = completion();
    comp.outcome = Outcome::Went;
    comp.bytes_seen = Some(150_000);
    comp.bytes_discarded = Some(50_000);
    ledger
        .close_step("run-discard", "compile", 1, 7, comp)
        .expect("chiudere il passo");

    let discarded = ledger
        .steps_with_discarded_output()
        .expect("interrogare passi con scarto");
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
    let steps = ledger.steps("run-discard").expect("rileggere");
    assert_eq!(steps[0].bytes_seen, Some(150_000));
    assert_eq!(steps[0].bytes_discarded, Some(50_000));
}

#[test]
fn schema_v1_legacy_records_without_byte_counts_upgrade_and_rebuild_cleanly() {
    let directory = TestDirectory::new("upgrade-v1");
    // Creiamo un database v1 con lo schema vecchio e un record senza campi bytes_seen/bytes_discarded
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

    let ledger = Ledger::open(&directory.0).expect("apertura e migrazione dello schema vecchio");
    let steps = ledger.steps("legacy-run").expect("lettura passi migrati");
    assert_eq!(steps.len(), 1);
    assert_eq!(steps[0].bytes_seen, None);
    assert_eq!(steps[0].bytes_discarded, None);
    // Un passo scritto prima che esistessero non eredita né un detentore né
    // una specie: restano vuoti, che è ciò che erano davvero.
    assert_eq!(steps[0].held_by_pid, None);
    assert_eq!(steps[0].species, None);
    assert_eq!(steps[0].outcome, Some(Outcome::Went));
}

/// Chi tiene il passo e di che specie è arrivano fino al disco e tornano
/// indietro: sono i due dati su cui la ripresa decide se rilanciare, e un
/// deposito che li perdesse per strada la riporterebbe a indovinare.
#[test]
fn the_holder_and_the_species_survive_the_ledger() {
    let directory = TestDirectory::new("species-round-trip");
    let ledger = Ledger::open(&directory.0).expect("apertura del deposito");
    let mut record = started_attempt("species-run", 1, 7);
    record.held_by_pid = Some(4321);
    record.species = Some(StepSpecies::Compensable);
    ledger
        .append_step_started(&record)
        .expect("scrivere l'intenzione");

    let steps = ledger.steps("species-run").expect("rilettura dei passi");
    assert_eq!(steps.len(), 1);
    assert_eq!(steps[0].held_by_pid, Some(4321));
    assert_eq!(steps[0].species, Some(StepSpecies::Compensable));
}

/// Una specie che il deposito non conosce non si legge come «consegna a una
/// persona»: sarebbe un valore inventato al posto di un dato corrotto.
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
    // Creiamo un database v1 con proiezioni già scritte e registro eventi potato
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
        // Registro eventi vuoto (potato): una ricostruzione fallirebbe
        transaction.commit().expect("commit v1");
    }

    let ledger = Ledger::open(&directory.0).expect("apertura v1 potato senza ricostruzione");
    let steps = ledger.steps("pruned-run").expect("lettura passi preservati");
    assert_eq!(steps.len(), 1);
    assert_eq!(steps[0].bytes_seen, None);
    assert_eq!(steps[0].bytes_discarded, None);
    assert_eq!(steps[0].outcome, Some(Outcome::Went));
}

#[test]
fn corrupted_event_log_behind_projection_watermark_is_rejected_on_open() {
    let directory = TestDirectory::new("corrupted-watermark");
    // Simuliamo un deposito corrotto: la proiezione è al passo 10,
    // ma il registro eventi non è mai andato oltre la sequenza 2.
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
                "messaggio inatteso: {message}"
            );
        }
        Ok(_) => panic!("l'apertura doveva essere rifiutata per watermark corrotto"),
        Err(error) => panic!("tipo di errore inatteso: {error:?}"),
    }
}

fn seen(kind: &str, name: &str, reach: &str) -> InventoryItem {
    InventoryItem {
        kind: kind.to_string(),
        name: name.to_string(),
        origin: "casa".to_string(),
        path: format!("/casa/{kind}/{name}"),
        reach: reach.to_string(),
        reason: (reach != "active").then(|| "il plugin è spento".to_string()),
    }
}

/// Il deposito sa dire che cosa non c'è più — la domanda che un elenco
/// ricalcolato ogni volta non può nemmeno porsi.
///
/// TRE SCANSIONI, PERCHÉ IL TERZO ATTO È QUELLO CHE SI SBAGLIA: una cosa che
/// ricompare deve smettere di risultare sparita. Senza quel braccio, il segno
/// della sparizione resterebbe per sempre e l'elenco mostrerebbe morta una
/// competenza che è tornata al suo posto — e chi cancella leggendo quell'elenco
/// cancellerebbe una cosa viva.
#[test]
fn the_ledger_knows_what_disappeared_and_what_came_back() {
    let directory = TestDirectory::new("inventario");
    let ledger = Ledger::open(&directory.0).expect("aprire il deposito");

    ledger
        .record_inventory(&InventoryScan {
            taken_at: 100,
            items: vec![seen("skill", "prima", "active"), seen("hook", "sola", "active")],
        })
        .expect("prima scansione");

    let present = ledger.inventory_present().expect("presenti");
    assert_eq!(present.len(), 2, "{present:#?}");
    assert!(present.iter().all(|item| item.first_seen == 100));

    // Seconda scansione: la competenza non c'è più, e il gancio si spegne.
    ledger
        .record_inventory(&InventoryScan {
            taken_at: 200,
            items: vec![seen("hook", "sola", "inactive")],
        })
        .expect("seconda scansione");

    let gone = ledger.inventory_gone().expect("sparite");
    assert_eq!(gone.len(), 1, "{gone:#?}");
    assert_eq!(gone[0].name, "prima");
    assert_eq!(gone[0].gone_at, Some(200));
    // E la data della prima volta non si è mossa: è l'unica che dice da quando.
    assert_eq!(gone[0].first_seen, 100);

    let present = ledger.inventory_present().expect("presenti");
    assert_eq!(present.len(), 1, "{present:#?}");
    assert_eq!(present[0].reach, "inactive");
    assert_eq!(present[0].reason.as_deref(), Some("il plugin è spento"));
    assert_eq!(present[0].first_seen, 100, "il gancio c'era già dalla prima");

    // Terza scansione: la competenza torna. Non è più sparita, e resta sua la
    // data d'origine.
    ledger
        .record_inventory(&InventoryScan {
            taken_at: 300,
            items: vec![seen("skill", "prima", "active"), seen("hook", "sola", "active")],
        })
        .expect("terza scansione");

    assert!(
        ledger.inventory_gone().expect("sparite").is_empty(),
        "una voce tornata risulta ancora sparita"
    );
    let back = ledger
        .inventory_present()
        .expect("presenti")
        .into_iter()
        .find(|item| item.name == "prima")
        .expect("la competenza è tornata");
    assert_eq!(back.first_seen, 100);
    assert_eq!(back.last_seen, 300);

    // «Che cosa è nuovo da ieri»: niente, perché tutte e due esistevano già.
    assert!(
        ledger.inventory_new_since(150).expect("nuove").is_empty(),
        "una voce tornata non è una voce nuova"
    );
}

/// La proiezione si ricostruisce dagli eventi, com'è promesso per tutto il
/// resto del deposito: se l'inventario non si ricostruisse, il file di stato
/// diventerebbe l'unica copia di un dato che nessuno può più verificare.
#[test]
fn the_inventory_survives_a_rebuild_from_the_events() {
    let directory = TestDirectory::new("inventario-ricostruito");
    let ledger = Ledger::open(&directory.0).expect("aprire il deposito");
    ledger
        .record_inventory(&InventoryScan {
            taken_at: 10,
            items: vec![seen("skill", "una", "active"), seen("skill", "due", "active")],
        })
        .expect("prima");
    ledger
        .record_inventory(&InventoryScan {
            taken_at: 20,
            items: vec![seen("skill", "una", "active")],
        })
        .expect("seconda");

    ledger.rebuild_projections().expect("ricostruzione");

    let gone = ledger.inventory_gone().expect("sparite");
    assert_eq!(gone.len(), 1, "{gone:#?}");
    assert_eq!(gone[0].name, "due");
    assert_eq!(gone[0].gone_at, Some(20));
}

fn record(collection: &str, key: &str, value: Value, at: i64) -> StoreRecord {
    StoreRecord {
        collection: collection.to_string(),
        key: key.to_string(),
        value,
        written_by: "flusso-di-prova".to_string(),
        written_at: at,
    }
}

/// Una voce che nessuno ha scritto risponde «non lo so».
///
/// È la risposta che permette a chi legge di avere un ripiego. Un deposito che
/// inventasse un valore plausibile sarebbe peggio dell'euristica che
/// sostituisce, perché sembrerebbe un fatto.
#[test]
fn a_record_nobody_wrote_says_it_does_not_know() {
    let directory = TestDirectory::new("voce-mai-scritta");
    let ledger = Ledger::open(&directory.0).expect("aprire il deposito");
    assert_eq!(ledger.read_record("mandate", "current").expect("letta"), None);
}

/// Il motore non conosce le collezioni: le tiene tutte, senza saperle.
///
/// È la prova che questo spazio è del flusso e non del motore. Due collezioni
/// inventate qui — nomi che non compaiono da nessuna parte in Rust — devono
/// convivere senza che nessuno le abbia dichiarate, e la stessa chiave in due
/// collezioni deve restare due voci distinte. Se un giorno qualcuno rimettesse
/// il dominio nel motore, questa prova sarebbe la prima a non compilare più.
#[test]
fn the_engine_keeps_collections_it_knows_nothing_about() {
    let directory = TestDirectory::new("collezioni-ignote");
    let ledger = Ledger::open(&directory.0).expect("aprire il deposito");
    ledger
        .put_record(&record("mandate", "current", json!({"file": "sailor.md"}), 10))
        .expect("mandato");
    ledger
        .put_record(&record("panetteria", "current", json!({"pane": 3}), 20))
        .expect("pane");

    let mandate = ledger.read_record("mandate", "current").expect("letta").expect("c'è");
    assert_eq!(mandate.value, json!({"file": "sailor.md"}));
    let bakery = ledger.read_record("panetteria", "current").expect("letta").expect("c'è");
    assert_eq!(bakery.value, json!({"pane": 3}));
    assert_eq!(ledger.records_in("panetteria").expect("collezione").len(), 1);
}

/// L'ultima scrittura vince, e la voce resta una.
///
/// Il controllo che conta non è rileggere il secondo valore — lo farebbe anche
/// una tabella che li accumula tutti e restituisce il più recente — ma che dopo
/// due scritture la collezione porti **una voce sola**. Due voci
/// significherebbe due risposte possibili alla stessa domanda, ed è la forma da
/// cui nasce ogni «ma allora su cosa stavamo lavorando?».
#[test]
fn the_last_write_wins_and_the_record_stays_one() {
    let directory = TestDirectory::new("voce-sostituita");
    let ledger = Ledger::open(&directory.0).expect("aprire il deposito");
    ledger
        .put_record(&record("mandate", "current", json!({"file": "socraticode.md"}), 100))
        .expect("primo");
    ledger
        .put_record(&record("mandate", "current", json!({"file": "sailor.md"}), 200))
        .expect("secondo");

    let current = ledger.read_record("mandate", "current").expect("letta").expect("c'è");
    assert_eq!(current.value, json!({"file": "sailor.md"}));
    assert_eq!(current.written_at, 200);
    assert_eq!(ledger.records_in("mandate").expect("collezione").len(), 1);
}

/// Una voce senza indirizzo è rifiutata.
///
/// Vale per la chiave quanto per la collezione: chi scrive senza indirizzo
/// crede di aver depositato qualcosa, e nessuno lo ritroverà.
#[test]
fn a_record_without_an_address_is_refused() {
    let directory = TestDirectory::new("voce-senza-indirizzo");
    let ledger = Ledger::open(&directory.0).expect("aprire il deposito");

    let mut homeless = record("mandate", "current", json!(1), 10);
    homeless.collection = "  ".to_string();
    assert!(ledger.put_record(&homeless).is_err());

    let mut keyless = record("mandate", "current", json!(1), 10);
    keyless.key = String::new();
    assert!(ledger.put_record(&keyless).is_err());

    // E il rifiuto non ha lasciato niente dietro di sé.
    assert_eq!(ledger.read_record("mandate", "current").expect("letta"), None);
}

/// La fonte è il registro, non la tabella.
///
/// Cade da sola se `RecordWritten` smette di essere proiettato — cioè se
/// qualcuno lo tratta come una traccia da scartare: la ricostruzione
/// ripartirebbe da un registro pieno e lascerebbe la tabella vuota. È lo stesso
/// controllo che l'inventario ha sopra, e per lo stesso motivo: un dato che
/// vive solo nel file di stato è un dato che nessuno può più verificare.
#[test]
fn a_record_survives_a_rebuild_from_the_events() {
    let directory = TestDirectory::new("voce-ricostruita");
    let ledger = Ledger::open(&directory.0).expect("aprire il deposito");
    ledger
        .put_record(&record("mandate", "current", json!({"file": "vecchio.md"}), 100))
        .expect("primo");
    ledger
        .put_record(&record("mandate", "current", json!({"file": "corrente.md"}), 200))
        .expect("secondo");

    ledger.rebuild_projections().expect("ricostruzione");

    let current = ledger.read_record("mandate", "current").expect("letta").expect("c'è");
    assert_eq!(
        current.value,
        json!({"file": "corrente.md"}),
        "l'ordine degli eventi decide, e l'ultimo vince"
    );
    assert_eq!(current.written_by, "flusso-di-prova");
}

// ── com'è andata: le prove delle letture sullo storico ───────────────────

fn a_run(ledger: &Ledger, run_id: &str, entity: &str, started_at: i64, ended_at: Option<i64>) {
    ledger
        .record_run(&RunRecord {
            run_id: run_id.to_owned(),
            kind: "flow".to_owned(),
            entity: entity.to_owned(),
            parent_run_id: None,
            started_by: "prova".to_owned(),
            status: if ended_at.is_some() { "done" } else { "running" }.to_owned(),
            total_cost_micros: 0,
            error: None,
            started_at,
            ended_at,
        })
        .expect("registrare la corsa");
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
        json!({"segreto": "questo non deve mai uscire"}),
        vec![],
        started_at,
    );
    ledger
        .append_step_started(&record)
        .expect("registrare l'intenzione");
    if let Some((outcome, failure_class, ended_at, said)) = closing {
        ledger
            .close_step(
                run_id,
                step_id,
                attempt,
                epoch,
                Completion {
                    outcome,
                    output: Some(json!({"segreto": "nemmeno questo"})),
                    said: said.map(str::to_owned),
                    failure_class: failure_class.map(str::to_owned),
                    ended_at,
                    bytes_seen: Some(10),
                    bytes_discarded: Some(0),
                },
            )
            .expect("chiudere il passo");
    }
}

/// **UNA CORSA CONSEGNATA SI RITROVA, E `unfinished_runs` NON LA VEDE.**
///
/// Le due domande sembrano una sola e sono opposte. `unfinished_runs` cerca
/// `steps.outcome IS NULL`: un'intenzione scritta senza esito, cioè un processo
/// morto a metà. Un passo consegnato a un agente è **chiuso**, con esito
/// `Waiting` — quindi quella domanda non lo trova, e fino al 31/08/2026 non lo
/// trovava nessuno: una consegna che nessuno raccoglieva spariva dal sistema.
///
/// La prova tiene le due corse insieme apposta. Chiedere l'una e ricevere
/// l'altra è il difetto vero, e con una corsa sola nel deposito non si vedrebbe.
#[test]
fn a_handed_run_is_found_by_the_waiting_question_and_not_by_the_unfinished_one() {
    let directory = TestDirectory::new("corse-in-attesa");
    let ledger = Ledger::open(&directory.0).expect("aprire il deposito");

    // Consegnata: il passo è chiuso con esito «in attesa», e l'intestazione
    // porta lo stato che l'esecutore le scrive.
    a_run(&ledger, "run-handed", "sviluppa-sailor", 100, Some(150));
    ledger
        .record_run(&RunRecord {
            run_id: "run-handed".to_owned(),
            kind: "flow".to_owned(),
            entity: "sviluppa-sailor".to_owned(),
            parent_run_id: None,
            started_by: "prova".to_owned(),
            status: "waiting".to_owned(),
            total_cost_micros: 0,
            error: None,
            started_at: 100,
            ended_at: Some(150),
        })
        .expect("registrare la corsa consegnata");
    a_step(
        &ledger,
        "run-handed",
        "implementa",
        1,
        1,
        110,
        Some((Outcome::Waiting, None, 150, Some("consegnato a «claude-vivo»"))),
    );

    // Interrotta a metà: un passo aperto e nessun esito. Questa è di
    // `unfinished_runs`, e non deve comparire fra quelle in attesa.
    a_run(&ledger, "run-halfway", "sviluppa-sailor", 200, None);
    a_step(&ledger, "run-halfway", "implementa", 1, 1, 210, None);

    let waiting = ledger.waiting_runs().expect("le corse in attesa si chiedono");
    assert_eq!(
        waiting.len(),
        1,
        "una sola corsa aspetta qualcuno, e non è quella interrotta: {waiting:?}"
    );
    assert_eq!(waiting[0].run_id, "run-handed");
    assert_eq!(waiting[0].entity, "sviluppa-sailor");
    assert_eq!(
        waiting[0].waiting_since, 150,
        "aspetta da quando si è fermata, non da quando è partita"
    );

    let unfinished = ledger
        .unfinished_runs()
        .expect("le corse interrotte si chiedono");
    let names: Vec<&str> = unfinished
        .iter()
        .map(|run| run.run_id.as_str())
        .collect();
    assert_eq!(
        names,
        vec!["run-halfway"],
        "la corsa consegnata non è interrotta: il suo passo è chiuso. \
         Se compare qui, le due domande si sono confuse"
    );
}

/// Un deposito appena nato risponde, e risponde zero.
///
/// Cade se una di queste letture tratta l'assenza come un guasto — per esempio
/// con un `query_row` che pretende una riga. È il caso della macchina appena
/// installata: chi interroga lo storico ci passa **prima** di ogni altra cosa,
/// e un errore qui farebbe nascere rossa la prima corsa di ogni flusso nuovo.
#[test]
fn an_empty_ledger_answers_zero_instead_of_breaking() {
    let directory = TestDirectory::new("storico-vuoto");
    let ledger = Ledger::open(&directory.0).expect("aprire il deposito");

    assert_eq!(ledger.recorded_runs().expect("conteggio"), 0);
    assert_eq!(ledger.runs_in_window(None, 50).expect("finestra"), 0);
    let tally = ledger.step_failure_tally("compile", None, 50).expect("conteggio");
    assert_eq!(tally.attempts, 0);
    assert_eq!(tally.failures, 0);
    assert!(tally.by_class.is_empty());
    assert!(ledger.failure_class_tally(None, 50).expect("classi").is_empty());
    assert_eq!(ledger.last_finished_run("qualunque").expect("ultima corsa"), None);
    let durations = ledger.step_durations("compile", None, 50).expect("durate");
    assert!(durations.seconds_sorted.is_empty());
    assert_eq!(durations.failed_samples, 0);
    assert!(ledger.said_of_failed_steps("mai-esistita", 5, 512).expect("detto").is_empty());
}

/// Il filtro per flusso taglia davvero, e taglia passando da `runs`.
///
/// Il mutante che la fa cadere è togliere la giunzione con `runs`: la risposta
/// per `alpha` diventerebbe quella di tutti i flussi insieme, cioè un numero
/// che sembra una misura del proprio flusso e misura anche quello di un altro.
#[test]
fn the_flow_filter_cuts_by_joining_the_run_header() {
    let directory = TestDirectory::new("storico-per-flusso");
    let ledger = Ledger::open(&directory.0).expect("aprire il deposito");
    a_run(&ledger, "run-a1", "alpha", 100, Some(200));
    a_step(&ledger, "run-a1", "compile", 1, 1, 100, Some((Outcome::Broke, Some("timeout"), 150, None)));
    a_run(&ledger, "run-a2", "alpha", 300, Some(400));
    a_step(&ledger, "run-a2", "compile", 1, 1, 300, Some((Outcome::Broke, Some("timeout"), 350, None)));
    a_run(&ledger, "run-b1", "beta", 500, Some(600));
    a_step(&ledger, "run-b1", "compile", 1, 1, 500, Some((Outcome::Broke, Some("timeout"), 550, None)));

    let alpha = ledger.step_failure_tally("compile", Some("alpha"), 50).expect("conteggio");
    let everything = ledger.step_failure_tally("compile", None, 50).expect("conteggio");

    assert_eq!(alpha.failures, 2, "solo le corse di alpha");
    assert_eq!(everything.failures, 3, "senza flusso si guarda tutto");
    assert_eq!(ledger.runs_in_window(Some("alpha"), 50).expect("finestra"), 2);
}

/// I guasti sono i tentativi; le corse toccate sono le corse.
///
/// Cade se qualcuno conta `COUNT(DISTINCT run_id)` al posto dei tentativi: due
/// rotture nella stessa corsa diventerebbero una, e un passo che si sfascia a
/// ogni ritentativo sembrerebbe rompersi la metà delle volte.
#[test]
fn failures_count_attempts_while_runs_affected_counts_runs() {
    let directory = TestDirectory::new("storico-tentativi");
    let ledger = Ledger::open(&directory.0).expect("aprire il deposito");
    a_run(&ledger, "run-1", "alpha", 100, Some(400));
    a_step(&ledger, "run-1", "compile", 1, 1, 100, Some((Outcome::Broke, Some("timeout"), 150, None)));
    a_step(&ledger, "run-1", "compile", 2, 2, 200, Some((Outcome::Broke, Some("timeout"), 250, None)));

    let tally = ledger.step_failure_tally("compile", Some("alpha"), 50).expect("conteggio");

    assert_eq!(tally.attempts, 2);
    assert_eq!(tally.failures, 2, "due tentativi rotti sono due guasti");
    assert_eq!(tally.runs_affected, 1, "in una corsa sola");
}

/// L'ultima corsa è l'ultima **chiusa**, mai quella ancora in volo.
///
/// Il mutante che la fa cadere è togliere `AND ended_at IS NOT NULL`: un flusso
/// che si interroga mentre gira riceverebbe se stesso a metà, e leggerebbe come
/// esito della volta scorsa un elenco di passi che non sono ancora successi.
#[test]
fn the_last_finished_run_is_never_the_one_still_in_flight() {
    let directory = TestDirectory::new("storico-ultima-chiusa");
    let ledger = Ledger::open(&directory.0).expect("aprire il deposito");
    a_run(&ledger, "run-vecchia", "alpha", 100, Some(200));
    a_step(&ledger, "run-vecchia", "compile", 1, 1, 100, Some((Outcome::Went, None, 130, None)));
    a_run(&ledger, "run-in-volo", "alpha", 300, None);
    a_step(&ledger, "run-in-volo", "compile", 1, 1, 300, None);

    let last = ledger.last_finished_run("alpha").expect("lettura").expect("una corsa chiusa c'è");

    assert_eq!(last.run_id, "run-vecchia");
    assert_eq!(last.steps.len(), 1);
    assert_eq!(last.steps[0].outcome.as_deref(), Some("Went"));
}

/// Una finestra di una corsa lascia fuori la precedente.
///
/// Cade se `LIMIT` sparisce o se l'ordine per `started_at DESC` si rovescia:
/// «nelle ultime N corse» diventerebbe «nelle prime N», cioè la risposta su uno
/// storico vecchio consegnata a chi chiede com'è andata ultimamente.
#[test]
fn a_window_of_one_run_leaves_the_older_one_out() {
    let directory = TestDirectory::new("storico-finestra");
    let ledger = Ledger::open(&directory.0).expect("aprire il deposito");
    a_run(&ledger, "run-vecchia", "alpha", 100, Some(200));
    a_step(&ledger, "run-vecchia", "compile", 1, 1, 100, Some((Outcome::Broke, Some("timeout"), 150, None)));
    a_run(&ledger, "run-nuova", "alpha", 300, Some(400));
    a_step(&ledger, "run-nuova", "compile", 1, 1, 300, Some((Outcome::Went, None, 350, None)));

    let narrow = ledger.step_failure_tally("compile", Some("alpha"), 1).expect("conteggio");
    let wide = ledger.step_failure_tally("compile", Some("alpha"), 50).expect("conteggio");

    assert_eq!(narrow.failures, 0, "nell'ultima corsa non si è rotto niente");
    assert_eq!(narrow.attempts, 1);
    assert_eq!(wide.failures, 1, "guardando più indietro il guasto c'è");
}

/// La classe più frequente viene prima, e una rottura senza classe resta senza.
#[test]
fn the_most_frequent_failure_class_comes_first() {
    let directory = TestDirectory::new("storico-classi");
    let ledger = Ledger::open(&directory.0).expect("aprire il deposito");
    a_run(&ledger, "run-1", "alpha", 100, Some(400));
    a_step(&ledger, "run-1", "uno", 1, 1, 100, Some((Outcome::Broke, Some("timeout"), 110, None)));
    a_step(&ledger, "run-1", "due", 1, 1, 120, Some((Outcome::Broke, Some("timeout"), 130, None)));
    a_step(&ledger, "run-1", "tre", 1, 1, 140, Some((Outcome::Broke, Some("exit_error"), 150, None)));
    a_step(&ledger, "run-1", "quattro", 1, 1, 160, Some((Outcome::Broke, None, 170, None)));

    let classes = ledger.failure_class_tally(None, 50).expect("classi");

    assert_eq!(classes.len(), 3);
    assert_eq!(classes[0].failure_class.as_deref(), Some("timeout"));
    assert_eq!(classes[0].failures, 2);
    assert!(
        classes.iter().any(|c| c.failure_class.is_none()),
        "una rottura che il motore non ha classificato resta senza classe: {classes:?}"
    );
}

/// Un tentativo rotto si conta, e non entra nella misura.
///
/// Cade se le durate raccolgono qualunque tentativo chiuso: il guasto lungo qui
/// sotto sposterebbe la mediana, e un passo che si rompe dopo cento secondi
/// sembrerebbe semplicemente un passo lento.
#[test]
fn a_broken_attempt_is_counted_but_not_measured() {
    let directory = TestDirectory::new("storico-durate");
    let ledger = Ledger::open(&directory.0).expect("aprire il deposito");
    a_run(&ledger, "run-1", "alpha", 100, Some(900));
    a_step(&ledger, "run-1", "compile", 1, 1, 100, Some((Outcome::Went, None, 110, None)));
    a_step(&ledger, "run-1", "compile", 2, 2, 200, Some((Outcome::Went, None, 230, None)));
    a_step(&ledger, "run-1", "compile", 3, 3, 300, Some((Outcome::Broke, Some("timeout"), 800, None)));

    let durations = ledger.step_durations("compile", Some("alpha"), 50).expect("durate");

    assert_eq!(durations.seconds_sorted, vec![10, 30], "solo i tentativi riusciti");
    assert_eq!(durations.failed_samples, 1, "il rotto si conta comunque");
    assert_eq!(durations.last_seconds, Some(30), "l'ultima riuscita, non l'ultima chiusa");
}

/// Il testo grezzo esce da una corsa sola, dai soli passi rotti, e troncato.
///
/// Le tre asserzioni sono tre cose diverse: che una corsa vicina non entri
/// nella risposta, che un passo riuscito non porti con sé il proprio testo, e
/// che il taglio venga dichiarato. La terza cade se `truncated` diventa un
/// valore fisso: chi legge una diagnosi tagliata senza saperlo la legge come
/// completa e conclude sul pezzo sbagliato.
#[test]
fn said_leaves_one_run_only_from_broken_steps_and_says_when_it_was_clipped() {
    let directory = TestDirectory::new("storico-detto");
    let ledger = Ledger::open(&directory.0).expect("aprire il deposito");
    a_run(&ledger, "run-1", "alpha", 100, Some(400));
    let long_text = "à".repeat(400);
    a_step(&ledger, "run-1", "rotto", 1, 1, 100, Some((Outcome::Broke, Some("timeout"), 150, Some(&long_text))));
    a_step(&ledger, "run-1", "riuscito", 1, 1, 160, Some((Outcome::Went, None, 170, Some("tutto bene"))));
    a_run(&ledger, "run-2", "alpha", 500, Some(600));
    a_step(&ledger, "run-2", "altrove", 1, 1, 500, Some((Outcome::Broke, Some("timeout"), 550, Some("di un'altra corsa"))));

    let excerpts = ledger.said_of_failed_steps("run-1", 5, 101).expect("detto");

    assert_eq!(excerpts.len(), 1, "una corsa sola, e solo i passi rotti: {excerpts:?}");
    assert_eq!(excerpts[0].step_id, "rotto");
    assert!(excerpts[0].truncated, "il taglio si dichiara");
    assert_eq!(
        excerpts[0].said.len(),
        100,
        "il taglio rispetta il confine di un carattere: 101 byte cadono su mezza «à»"
    );
}


// ── i conteggi che possono essere ignoti ─────────────────────────────────

/// Una chiamata coi conteggi come li si vuole.
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
    }
}

/// **UNO SCONOSCIUTO SOPRAVVIVE AL VIAGGIO FINO AL DISCO E RITORNO.** È il
/// punto in cui, se una colonna avesse un valore predefinito, un `None`
/// tornerebbe indietro come `0` senza che nessuno se ne accorga — e da lì in
/// poi sarebbe indistinguibile da una misura.
#[test]
fn an_unknown_count_stays_unknown_through_the_projection() {
    let directory = TestDirectory::new("token-ignoti");
    let ledger = Ledger::open(&directory.0).expect("aprire il deposito");
    ledger
        .record_model_call(&call_with("ignota", None, None))
        .expect("registrare la chiamata non misurata");
    ledger
        .record_model_call(&call_with("misurata", Some(42), Some(7)))
        .expect("registrare quella misurata");

    let dump = ledger.projection_dump().expect("leggere la proiezione");
    let rows = dump["model_calls"].as_array().expect("l'elenco c'è");
    assert_eq!(rows.len(), 2);
    let unknown = rows.iter().find(|row| row[0] == "ignota").unwrap();
    assert_eq!(unknown[7], Value::Null, "input_tokens ignoto resta NULL");
    assert_eq!(unknown[8], Value::Null, "output_tokens ignoto resta NULL");
    assert_eq!(unknown[10], Value::Null, "cost_micros ignoto resta NULL");
    assert_ne!(unknown[7], json!("0"), "e non diventa mai uno zero");

    let measured = rows.iter().find(|row| row[0] == "misurata").unwrap();
    assert_eq!(measured[7], json!("42"));
    assert_eq!(measured[10], json!(7));
}

/// Le due colonne nate con la versione 4 arrivano fino alla proiezione.
#[test]
fn the_total_and_the_declared_cost_reach_the_projection() {
    let directory = TestDirectory::new("colonne-nuove");
    let ledger = Ledger::open(&directory.0).expect("aprire il deposito");
    let mut record = call_with("solo-totale", None, None);
    record.total_tokens = Some(13_910);
    record.declared_cost_micros = Some(4_200);
    ledger.record_model_call(&record).expect("registrare");

    let dump = ledger.projection_dump().expect("leggere la proiezione");
    let row = &dump["model_calls"][0];
    assert_eq!(row[20], json!("13910"), "total_tokens");
    assert_eq!(row[21], json!(4_200), "declared_cost_micros");
}

/// **UN DEPOSITO GIÀ SCRITTO SI ADEGUA SENZA PERDERE UNA RIGA.** La tabella
/// vecchia ha `NOT NULL` sui conteggi, e SQLite non sa toglierlo con un
/// `ALTER`: si rifà la tabella e ci si copiano dentro le righe. Se quel
/// travaso perdesse qualcosa, chi aggiorna Sailor si troverebbe la propria
/// storia di spesa dimezzata senza nessun avviso.
#[test]
fn an_older_ledger_is_migrated_in_place_without_losing_its_rows() {
    let directory = TestDirectory::new("adeguamento");
    // Si costruisce a mano un deposito nella forma della versione 3.
    {
        let ledger = Ledger::open(&directory.0).expect("aprire il deposito");
        let connection = ledger.connection.lock().expect("nessuno panica qui");
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
            .expect("costruire la forma vecchia");
        connection
            .pragma_update(None, "user_version", 3i64)
            .expect("dichiararsi di versione 3");
    }

    // Riaprirlo lo porta alla forma nuova.
    let ledger = Ledger::open(&directory.0).expect("riaprire il deposito");
    let dump = ledger.projection_dump().expect("leggere la proiezione");
    let rows = dump["model_calls"].as_array().expect("l'elenco c'è");
    assert_eq!(rows.len(), 1, "la riga di prima è ancora lì");
    assert_eq!(rows[0][0], json!("vecchia"));
    assert_eq!(rows[0][7], json!("10"), "e coi suoi valori intatti");
    assert_eq!(rows[0][20], Value::Null, "le colonne nuove nascono ignote");
    // **E IL TESTO DELLA VECCHIA COLONNA NON SI PERDE E NON SI PROMUOVE.** Era
    // `repair` sotto il nome `mandate_name`; adesso sta sotto
    // `engine_identity`, e si rilegge come «non registrata, la colonna diceva
    // così». Riscriverlo come un profilo dichiarato darebbe a un dato che sapeva
    // già mentire la faccia di una misura.
    assert_eq!(rows[0][15], json!("repair"));

    // E adesso accetta ciò che prima avrebbe rifiutato.
    ledger
        .record_model_call(&call_with("nuova-ignota", None, None))
        .expect("una chiamata non misurata ora entra");
    let dump = ledger.projection_dump().expect("rileggere");
    assert_eq!(dump["model_calls"].as_array().unwrap().len(), 2);
}

/// Un evento scritto quando i conteggi erano numeri secchi continua a
/// leggersi: `10` diventa `Some(10)`, e un campo che non c'era diventa `None`.
/// Senza questo, aggiornare Sailor renderebbe illeggibile il registro degli
/// eventi già scritto — che è l'unica cosa da cui tutto il resto si ricostruisce.
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
        serde_json::from_value(old_shape).expect("un evento vecchio si legge ancora");
    assert_eq!(record.input_tokens, Some(10));
    assert_eq!(record.price_currency.as_deref(), Some("USD"));
    assert_eq!(record.total_tokens, None, "un campo che non c'era è ignoto");
    assert_eq!(record.declared_cost_micros, None);
    // **E L'IDENTITÀ NON SI INVENTA DA UN EVENTO CHE NON LA PORTA.** Quell'evento
    // ha `mandate_name: "repair"`, che era il campo di prima: leggerlo come un
    // profilo dichiarato darebbe a una riga vecchia un'affermazione che nessuno
    // ha mai fatto.
    assert_eq!(
        record.engine_identity,
        EngineIdentity::Unrecorded {
            legacy: String::new()
        },
        "un evento scritto prima non porta l'identità, e non se ne deduce una"
    );
}

// ── quanto ha speso una corsa ────────────────────────────────────────────

/// Una chiamata con la sua corsa, per misurare la spesa di una e non dell'altra.
fn call_in_run(call_id: &str, run_id: &str, cost: Option<i64>) -> ModelCallRecord {
    let mut record = call_with(call_id, Some(10), cost);
    record.run_id = run_id.to_owned();
    record
}

/// Una chiamata con la sessione dichiarata, per le prove che seguono.
fn call_with_session(call_id: &str, step_id: &str, cli: &str, session: &str) -> ModelCallRecord {
    let mut record = call_with(call_id, Some(10), Some(1));
    record.step_id = Some(step_id.to_owned());
    record.cli = cli.to_owned();
    record.session_id = Some(session.to_owned());
    record
}

/// **LA SESSIONE DI UN MOTORE NON SI DÀ A UN ALTRO MOTORE.** Un passo con una
/// catena può finire su `codex` perché `claude-code` aveva esaurito: passare al
/// passo dopo una sessione di `claude-code` da riprendere con `codex`
/// significherebbe consegnargli un identificativo che quel motore non conosce,
/// e la chiamata morirebbe **dopo** essere partita, cioè dopo aver speso.
#[test]
fn a_session_belongs_to_the_engine_that_opened_it() {
    let directory = TestDirectory::new("sessione-di-chi");
    let ledger = Ledger::open(&directory.0).expect("aprire il deposito");
    ledger
        .record_model_call(&call_with_session("call-1", "scopri", "un-motore", "sessione-1"))
        .expect("registrare la chiamata");

    assert_eq!(
        ledger
            .session_opened_by("run-1", "scopri", "un-motore")
            .expect("il deposito risponde"),
        Some("sessione-1".to_owned())
    );
    assert_eq!(
        ledger
            .session_opened_by("run-1", "scopri", "un-altro-motore")
            .expect("il deposito risponde"),
        None,
        "un altro motore non eredita la sessione di questo"
    );
    assert_eq!(
        ledger
            .session_opened_by("run-2", "scopri", "un-motore")
            .expect("il deposito risponde"),
        None,
        "e nemmeno un'altra corsa"
    );
}

/// Un passo rifatto ne ha aperte due: quella buona è **l'ultima**, e riprendere
/// la prima vorrebbe dire continuare la conversazione che era andata storta.
#[test]
fn a_step_that_ran_twice_hands_over_its_latest_session() {
    let directory = TestDirectory::new("sessione-rifatta");
    let ledger = Ledger::open(&directory.0).expect("aprire il deposito");
    let mut first = call_with_session("call-1", "scopri", "un-motore", "sessione-vecchia");
    first.started_at = 100;
    let mut second = call_with_session("call-2", "scopri", "un-motore", "sessione-nuova");
    second.started_at = 200;
    ledger.record_model_call(&first).expect("la prima");
    ledger.record_model_call(&second).expect("la seconda");

    assert_eq!(
        ledger
            .session_opened_by("run-1", "scopri", "un-motore")
            .expect("il deposito risponde"),
        Some("sessione-nuova".to_owned())
    );
}

/// **LA SOMMA DICE ANCHE QUANTO NON SA.** Due chiamate, una che ha dichiarato
/// il proprio costo e una no: il totale è quello della prima, e il secondo
/// numero dice che c'è una riga fuori dal conto. Chi guardasse solo `micros`
/// leggerebbe «la corsa è costata 7» dove la verità è «almeno 7».
#[test]
fn what_a_run_spent_says_how_much_of_it_is_unknown() {
    let directory = TestDirectory::new("spesa-parziale");
    let ledger = Ledger::open(&directory.0).expect("aprire il deposito");
    ledger
        .record_model_call(&call_in_run("nota", "run-1", Some(7)))
        .expect("registrare quella col costo");
    ledger
        .record_model_call(&call_in_run("ignota", "run-1", None))
        .expect("registrare quella senza");

    let spend = ledger.spent_in_run("run-1").expect("leggere la spesa");

    assert_eq!(spend.micros, 7, "somma i costi noti");
    assert_eq!(spend.calls, 2, "e conta tutte le chiamate, non solo quelle");
    assert_eq!(spend.calls_without_cost, 1);
    assert!(
        !spend.is_complete(),
        "un totale con una riga fuori non è completo, e chi decide deve saperlo"
    );
}

/// La spesa di una corsa è **sua**: le chiamate di un'altra non entrano.
///
/// Senza questa, un tetto di spesa si chiuderebbe addosso a una corsa per
/// quello che ha speso il vicino — e il primo flusso della giornata girerebbe
/// mentre l'ultimo non partirebbe mai.
#[test]
fn a_runs_spending_does_not_include_the_neighbours() {
    let directory = TestDirectory::new("spesa-di-chi");
    let ledger = Ledger::open(&directory.0).expect("aprire il deposito");
    ledger
        .record_model_call(&call_in_run("mia", "run-1", Some(7)))
        .expect("registrare la mia");
    ledger
        .record_model_call(&call_in_run("altrui", "run-2", Some(1_000)))
        .expect("registrare quella dell'altra corsa");

    let mine = ledger.spent_in_run("run-1").expect("leggere la mia spesa");

    assert_eq!(mine.micros, 7);
    assert_eq!(mine.calls, 1, "una sola chiamata è mia");
    assert!(mine.is_complete());
}

/// Una corsa che non ha chiamato nessun motore ha speso zero **e** non ha
/// niente di ignoto: le due cose insieme, o «zero» resterebbe ambiguo.
#[test]
fn a_run_that_called_no_engine_spent_nothing_and_hides_nothing() {
    let directory = TestDirectory::new("spesa-vuota");
    let ledger = Ledger::open(&directory.0).expect("aprire il deposito");

    let spend = ledger.spent_in_run("mai-girata").expect("leggere la spesa");

    assert_eq!(spend, Spend::default());
    assert!(spend.is_complete(), "niente di ignoto: non c'è niente");
}

/// **UN DEPOSITO CHE SI DICHIARA GIÀ AGGIORNATO, MA NON LO È.**
///
/// Il guasto del 31/08/2026, e il modo esatto in cui è sfuggito a 517 prove.
/// Il 30/08 la migrazione ha imparato le quattro colonne della cache scritta e
/// `PROJECTION_SCHEMA_VERSION` è rimasta a 4: un deposito già esistente si
/// dichiarava della versione corrente, il confronto `4 < 4` era falso, la
/// migrazione non partiva, e **ogni lettura moriva** con «no such column:
/// cache_write_tokens».
///
/// **PERCHÉ LE PROVE CHE C'ERANO NON BASTAVANO.** Un deposito creato in una
/// prova nasce dal `CREATE TABLE` completo e non passa mai di lì; e la prova
/// sulla versione 3 migrava comunque, perché `3 < 4` era vero — quindi
/// l'aggiunta di colonne senza alzare il numero le passava sotto entrambe. Il
/// caso scoperto era proprio questo: **versione già pari alla costante, colonne
/// mancanti**. Si vede solo su una macchina che aveva già usato Sailor, cioè
/// quella di chi lo sviluppa, il giorno dopo.
#[test]
fn a_ledger_that_claims_to_be_current_but_lacks_the_new_columns_is_still_migrated() {
    let directory = TestDirectory::new("versione-bugiarda");
    {
        let ledger = Ledger::open(&directory.0).expect("aprire il deposito");
        let connection = ledger.connection.lock().expect("nessuno panica qui");
        // La forma della versione 4: tutto fino a `declared_cost_micros`, e
        // nessuna delle quattro colonne della cache scritta.
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
            .expect("costruire la forma della versione 4");
        connection
            .pragma_update(None, "user_version", 4i64)
            .expect("e dichiararsi già aggiornato");
    }

    // Riaprirlo deve bastare: è l'unico gesto che chi aggiorna Sailor compie.
    let ledger = Ledger::open(&directory.0).expect("riaprire il deposito");

    // La lettura è il punto: prima moriva qui, non all'apertura.
    let dump = ledger
        .projection_dump()
        .expect("leggere la proiezione di un deposito che si diceva aggiornato");
    let rows = dump["model_calls"].as_array().expect("l'elenco c'è");
    assert_eq!(rows.len(), 1, "la riga di ieri è ancora lì");
    assert_eq!(rows[0][0], json!("di-ieri"));
    assert_eq!(rows[0][7], json!("10"), "coi suoi valori intatti");
    assert_eq!(rows[0][22], Value::Null, "e le colonne nuove nascono ignote");

    // E la somma della spesa, che è ciò che il comando `flow cost` chiede,
    // adesso si può fare: prima era la query che falliva.
    let spend = ledger.spent_in_run("run-1").expect("leggere la spesa");
    assert_eq!(spend.micros, 21);
    assert_eq!(spend.calls, 1);
}

/// **IL DUMP DEVE PORTARE OGNI COLONNA CHE LA TABELLA HA.**
///
/// `dump_table` elenca le colonne **a mano**, e chi legge quel dump lo fa **per
/// posizione**: una colonna aggiunta alla tabella e dimenticata nell'elenco non
/// rompe niente, non fa fallire nessuna prova, e semplicemente non esiste per
/// tutto ciò che sta a valle. È successo il 31/08/2026 con `turns`: la colonna
/// c'era, la migrazione l'aveva creata, la scrittura la riempiva, e `flow cost`
/// mostrava zero — perché il dump non la chiedeva.
///
/// Questa prova non guarda una colonna in particolare: confronta **quante** ne
/// porta una riga del dump con quante ne ha la tabella. Vale per ogni colonna
/// che verrà aggiunta dopo, senza che nessuno debba ricordarsi di aggiornarla.
#[test]
fn the_dump_carries_every_column_the_table_has() {
    let directory = TestDirectory::new("dump-columns");
    let ledger = Ledger::open(&directory.0).expect("aprire il deposito");
    ledger
        .record_model_call(&call_with("call-colonne", Some(7), Some(11)))
        .expect("registrare una chiamata");

    let dump = ledger.projection_dump().expect("il dump si legge");
    let row = dump["model_calls"][0]
        .as_array()
        .expect("la riga c'è")
        .len();

    let connection = ledger.lock().expect("la connessione");
    let mut statement = connection
        .prepare("SELECT COUNT(*) FROM pragma_table_info('model_calls')")
        .expect("interrogare la forma della tabella");
    let columns: i64 = statement
        .query_row([], |row| row.get(0))
        .expect("contare le colonne");

    assert_eq!(
        row as i64, columns,
        "il dump porta {row} colonne e la tabella ne ha {columns}: quelle che mancano \
         sono invisibili a tutto ciò che legge il dump, e nessun errore lo dice"
    );
}

/// **UN DEPOSITO CHE ARRIVA DA UNA VERSIONE VECCHIA DEVE FINIRE CON LA STESSA
/// FORMA DI UNO NUOVO.**
///
/// Il guasto 24 era una colonna imparata dal `CREATE TABLE` e non dalla
/// migrazione, con il numero di versione fermo: un deposito già esistente non
/// migrava e ogni lettura moriva. La prova nata da quel guasto parte dalla
/// versione 4, e quindi **non vede** una colonna aggiunta oggi sotto la 6: la
/// migrazione 5→6 ha funzionato per fortuna, non per costruzione.
///
/// Questa non guarda nessuna colonna in particolare e non invecchia. Confronta
/// la forma di un deposito **migrato** con quella di uno **nato adesso**: ogni
/// colonna che entra nel `CREATE TABLE` senza il proprio `ALTER` la rende rossa,
/// oggi e fra dieci versioni.
#[test]
fn a_migrated_ledger_ends_up_shaped_exactly_like_a_fresh_one() {
    fn columns(ledger: &Ledger, table: &str) -> Vec<String> {
        let connection = ledger.lock().expect("la connessione");
        let mut statement = connection
            .prepare(&format!("SELECT name FROM pragma_table_info('{table}') ORDER BY cid"))
            .expect("interrogare la forma");
        let names = statement
            .query_map([], |row| row.get::<_, String>(0))
            .expect("leggere i nomi")
            .collect::<Result<Vec<_>, _>>()
            .expect("nomi validi");
        names
    }

    let fresh_dir = TestDirectory::new("forma-nuova");
    let fresh = Ledger::open(&fresh_dir.0).expect("aprire un deposito nuovo");
    let expected = columns(&fresh, "model_calls");

    // La forma della versione 3, che è quella con cui `relax_model_calls`
    // rifà la tabella: è **congelata**, perché la versione 3 non guadagnerà
    // mai una colonna.
    //
    // **LA FINESTRA SI RICAVA, NON SI ELENCA, E IL 31/08/2026 QUESTA PROVA HA
    // PRESO SE STESSA.** Prima l'elenco era quello delle colonne **da
    // togliere**: cioè invecchiava a ogni versione, esattamente come le due
    // prove che questa era nata per sostituire. Una colonna aggiunta alla
    // versione 7 restava nel deposito «vecchio», e la prova diventava rossa
    // per un difetto della propria impalcatura invece che del codice — il
    // rumore che fa zittire un controllo.
    const AS_OF_VERSION_3: &[&str] = &[
        "call_id", "run_id", "step_id", "purpose", "cli", "requested_model", "actual_model",
        "input_tokens", "output_tokens", "cached_tokens", "cost_micros", "price_currency",
        "input_price_micros_per_million", "output_price_micros_per_million",
        "cached_price_micros_per_million", "mandate_name", "mandate_version", "retry_chain",
        "error_type", "started_at", "ended_at", "total_tokens", "declared_cost_micros",
    ];

    // Un deposito della versione più vecchia che la migrazione sa ancora
    // riprendere, dichiarato tale.
    let old_dir = TestDirectory::new("forma-vecchia");
    {
        let ledger = Ledger::open(&old_dir.0).expect("aprire il deposito");
        let connection = ledger.connection.lock().expect("nessuno panica qui");
        // **LA VERSIONE 3 CHIAMAVA QUELLA COLONNA IN UN ALTRO MODO, E NE AVEVA
        // UNA IN PIÙ.** Togliere colonne non basta più a ricostruirla: la
        // versione 8 ha rinominato `mandate_name` in `engine_identity` e ha
        // buttato `mandate_version`. Se questa prova partisse dalla forma nuova
        // e si limitasse a togliere, proverebbe una migrazione che non esiste.
        connection
            .execute(
                "ALTER TABLE model_calls RENAME COLUMN engine_identity TO mandate_name",
                [],
            )
            .expect("rimettere il nome che la versione 3 usava");
        connection
            .execute(
                "ALTER TABLE model_calls ADD COLUMN mandate_version TEXT NOT NULL DEFAULT ''",
                [],
            )
            .expect("e la colonna che la versione 3 aveva in più");
        // Si guardano le colonne **di adesso**, non quelle della forma nuova:
        // dopo il rinomino le due liste non coincidono più. La domanda passa da
        // questa connessione, che è già in mano: `columns` ne prenderebbe una
        // seconda sullo stesso lucchetto, e quel lucchetto non è rientrante.
        let now: Vec<String> = {
            let mut statement = connection
                .prepare("SELECT name FROM pragma_table_info('model_calls') ORDER BY cid")
                .expect("interrogare la forma di adesso");
            let names = statement
                .query_map([], |row| row.get::<_, String>(0))
                .expect("leggere i nomi")
                .collect::<Result<Vec<_>, _>>()
                .expect("nomi validi");
            names
        };
        for column in now
            .iter()
            .filter(|name| !AS_OF_VERSION_3.contains(&name.as_str()))
        {
            connection
                .execute(&format!("ALTER TABLE model_calls DROP COLUMN {column}"), [])
                .unwrap_or_else(|error| panic!("togliere {column}: {error}"));
        }
        connection
            .pragma_update(None, "user_version", 3i64)
            .expect("e dichiararsi di tre versioni fa");
    }

    let migrated = Ledger::open(&old_dir.0).expect("riaprire il deposito vecchio");
    assert_eq!(
        columns(&migrated, "model_calls"),
        expected,
        "un deposito migrato non ha la forma di uno nuovo: una colonna è stata \
         aggiunta al CREATE TABLE senza il suo ALTER, e su ogni macchina che \
         aveva già un deposito quella colonna non esisterà mai"
    );
}

// ---------------------------------------------------------------------------
// I processi che Sailor avvia — guasto 4.
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

/// **UN PROCESSO AVVIATO RESTA SCRITTO ANCHE SE CHI L'HA AVVIATO SE N'È ANDATO.**
///
/// È il caso del guasto 4 alla lettera: l'orfano è stato trovato *il giorno
/// dopo*, da un'altra persona, e nessuno sapeva chi l'avesse acceso. Un registro
/// che vive in memoria dentro la finestra non avrebbe risposto — la finestra era
/// chiusa. Qui il deposito si riapre da zero, che è ciò che fa chi arriva dopo.
#[test]
fn a_started_process_survives_the_window_that_started_it() {
    let directory = TestDirectory::new("processi-vivi");

    {
        let ledger = Ledger::open(&directory.0).expect("aprire il deposito");
        ledger
            .record_process_started(&spawned("vite-1", Some(5183)))
            .expect("registrare il processo avviato");
    }

    // Nessuno stato in memoria sopravvive a questa riga: è un altro `Ledger`.
    let later = Ledger::open(&directory.0).expect("riaprire il deposito domani");
    let left = later.processes_left_running().expect("chiedere chi è rimasto");
    assert_eq!(left.len(), 1, "l'orfano non è nel deposito: {left:?}");
    assert_eq!(left[0].process_id, "vite-1");
    assert_eq!(left[0].pid, 4242);
    assert_eq!(left[0].port, Some(5183));
    assert_eq!(left[0].command, "npm");
    assert_eq!(left[0].args, vec!["run".to_owned(), "dev".to_owned()]);
    assert_eq!(
        left[0].started_by, "supervisor",
        "senza chi l'ha avviato, chi lo trova non sa a chi chiedere"
    );
}

/// Un processo chiuso esce dall'elenco. Senza questo, l'elenco cresce e basta,
/// e chi lo legge smette di crederci — che è come non averlo.
#[test]
fn a_closed_process_leaves_the_list() {
    let directory = TestDirectory::new("processi-chiusi");
    let ledger = Ledger::open(&directory.0).expect("aprire il deposito");

    ledger
        .record_process_started(&spawned("vite-1", Some(5183)))
        .expect("registrare l'avvio");
    ledger
        .record_process_started(&spawned("vite-2", Some(5184)))
        .expect("registrare il secondo avvio");
    ledger
        .record_process_ended(&ProcessEndRecord {
            process_id: "vite-1".to_owned(),
            exit_code: Some(0),
            ended_at: 1_700_000_060,
        })
        .expect("registrare la chiusura");

    let left = ledger.processes_left_running().expect("chiedere chi è rimasto");
    let ids: Vec<&str> = left.iter().map(|record| record.process_id.as_str()).collect();
    assert_eq!(ids, vec!["vite-2"], "chi è uscito è rimasto nell'elenco");
}

/// **LA DOMANDA CHE HA CAUSATO IL GUASTO ERA SULLA PORTA**, non sul processo:
/// «chi occupa la 5183 e mi impedisce di partire». Il deposito deve rispondere
/// con quella chiave, altrimenti chi è bloccato deve leggere l'elenco intero e
/// indovinare.
#[test]
fn the_ledger_answers_who_holds_a_port() {
    let directory = TestDirectory::new("processi-porta");
    let ledger = Ledger::open(&directory.0).expect("aprire il deposito");

    ledger
        .record_process_started(&spawned("vite-1", Some(5183)))
        .expect("registrare l'avvio");
    ledger
        .record_process_started(&spawned("senza-porta", None))
        .expect("registrare un processo che non occupa niente");

    let holder = ledger
        .process_holding_port(5183)
        .expect("chiedere chi tiene la porta");
    assert_eq!(
        holder.map(|record| record.process_id),
        Some("vite-1".to_owned())
    );

    assert!(
        ledger
            .process_holding_port(9999)
            .expect("chiedere di una porta libera")
            .is_none(),
        "il deposito ha inventato un occupante per una porta libera"
    );

    // E una volta chiuso, la porta risulta libera: è il segnale che dice a chi
    // arriva dopo che può partire senza uccidere niente.
    ledger
        .record_process_ended(&ProcessEndRecord {
            process_id: "vite-1".to_owned(),
            exit_code: Some(0),
            ended_at: 1_700_000_060,
        })
        .expect("registrare la chiusura");
    assert!(
        ledger
            .process_holding_port(5183)
            .expect("richiedere dopo la chiusura")
            .is_none(),
        "la porta risulta ancora occupata da un processo chiuso"
    );
}

/// **NON È `pgrep`, ED È IL PUNTO.** Il guasto 12 dice che dentro certi
/// perimetri `pgrep` non vede i processi e **risponde vuoto senza errore**:
/// un elenco vuoto è indistinguibile da «non c'è nessuno». Qui si chiede di un
/// solo pid, conosciuto perché il deposito l'ha scritto, e la risposta è un sì o
/// un no su quel pid: non esiste la forma «elenco vuoto» in cui il difetto del
/// guasto 12 possa nascondersi.
#[test]
fn liveness_asks_about_one_known_pid_not_for_a_list() {
    // Questo processo è vivo per definizione: è quello che sta eseguendo la prova.
    assert!(
        pid_is_alive(std::process::id()),
        "il processo che sta girando risulta morto"
    );

    // Un figlio atteso è morto davvero, e il sistema lo sa finché nessuno
    // riusa il numero.
    let mut child = Command::new("/bin/sh")
        .args(["-c", "exit 0"])
        .spawn()
        .expect("avviare un figlio che muore subito");
    let pid = child.id();
    child.wait().expect("aspettarlo");
    assert!(
        !pid_is_alive(pid),
        "un processo atteso e sepolto risulta ancora vivo: pid {pid}"
    );
}
