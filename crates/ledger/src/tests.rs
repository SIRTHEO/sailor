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
            input_tokens: 10,
            output_tokens: 20,
            cached_tokens: 3,
            cost_micros: 21,
            price_currency: "USD".to_owned(),
            input_price_micros_per_million: 100,
            output_price_micros_per_million: 200,
            cached_price_micros_per_million: 10,
            mandate_name: "repair".to_owned(),
            mandate_version: "v3".to_owned(),
            retry_chain: vec!["call-0".to_owned()],
            error_type: Some("rate_limit".to_owned()),
            started_at: 101,
            ended_at: Some(110),
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

