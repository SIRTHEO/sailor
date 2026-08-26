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
    for field in ["outcome", "output", "said", "failure_class", "ended_at"] {
        assert!(object.contains_key(field), "manca il campo {field}");
        assert!(object[field].is_null(), "{field} non è nullo");
    }
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
fn projections_rebuild_identically_and_skipped_event_control_differs() {
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
    drop(ledger);
    let ledger = Ledger::open(&directory.0).expect("riaprire e ricostruire automaticamente");
    assert_eq!(
        ledger.projection_dump().expect("leggere il risultato"),
        expected
    );
}

#[test]
fn checkpoint_is_absent_before_commit_and_present_after() {
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
    assert!(!reopened
        .is_checkpointed("run-crash", "compile", 1)
        .expect("leggere il checkpoint"));
    assert!(reopened.steps("run-crash").expect("leggere il passo")[0]
        .outcome
        .is_none());

    reopened
        .close_step("run-crash", "compile", 1, 7, completion())
        .expect("chiudere dopo la ripresa");
    assert!(reopened
        .is_checkpointed("run-crash", "compile", 1)
        .expect("leggere il checkpoint commesso"));
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
