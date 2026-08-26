//! Il deposito durevole di Sailor.
//!
//! `events.db` contiene la verità append-only; `state.db` contiene quattro
//! proiezioni interrogabili e ricostruibili. I due file vengono collegati alla
//! stessa transazione logica; con WAL SQLite non promette però atomicità fra
//! database collegati se manca l'alimentazione durante il commit.

use flow::{Completion, Outcome, RecordStore, StepRecord};
use rusqlite::{params, Connection, OptionalExtension, Transaction, TransactionBehavior};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::fmt;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::Duration;
use tracing::field::{Field, Visit};
use tracing::{Event, Subscriber};
use tracing_subscriber::layer::{Context, Layer};

const STATE_FILE: &str = "state.db";
const EVENTS_FILE: &str = "events.db";
const BUSY_TIMEOUT: Duration = Duration::from_secs(5);

#[derive(Debug)]
pub enum LedgerError {
    Sqlite(rusqlite::Error),
    Json(serde_json::Error),
    InvalidRecord(String),
    DuplicateAttempt { step: String, attempt: u32 },
    MissingAttempt { step: String, attempt: u32 },
    AlreadyClosed { step: String, attempt: u32 },
    StaleEpoch { step: String, epoch: u64 },
    Poisoned,
}

impl fmt::Display for LedgerError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Sqlite(error) => write!(formatter, "sqlite: {error}"),
            Self::Json(error) => write!(formatter, "json: {error}"),
            Self::InvalidRecord(message) => write!(formatter, "invalid record: {message}"),
            Self::DuplicateAttempt { step, attempt } => {
                write!(formatter, "duplicate attempt {attempt} for step {step}")
            }
            Self::MissingAttempt { step, attempt } => {
                write!(formatter, "missing attempt {attempt} for step {step}")
            }
            Self::AlreadyClosed { step, attempt } => {
                write!(
                    formatter,
                    "attempt {attempt} for step {step} is already closed"
                )
            }
            Self::StaleEpoch { step, epoch } => {
                write!(formatter, "epoch {epoch} for step {step} is stale")
            }
            Self::Poisoned => write!(formatter, "ledger mutex poisoned"),
        }
    }
}

impl std::error::Error for LedgerError {}

impl From<rusqlite::Error> for LedgerError {
    fn from(error: rusqlite::Error) -> Self {
        Self::Sqlite(error)
    }
}

impl From<serde_json::Error> for LedgerError {
    fn from(error: serde_json::Error) -> Self {
        Self::Json(error)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RunRecord {
    pub run_id: String,
    pub kind: String,
    pub entity: String,
    pub parent_run_id: Option<String>,
    pub started_by: String,
    pub status: String,
    pub total_cost_micros: i64,
    pub error: Option<String>,
    pub started_at: i64,
    pub ended_at: Option<i64>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ModelCallRecord {
    pub call_id: String,
    pub run_id: String,
    pub step_id: Option<String>,
    pub purpose: String,
    pub cli: String,
    pub requested_model: String,
    pub actual_model: String,
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub cached_tokens: u64,
    pub cost_micros: i64,
    pub price_currency: String,
    pub input_price_micros_per_million: i64,
    pub output_price_micros_per_million: i64,
    pub cached_price_micros_per_million: i64,
    pub mandate_name: String,
    pub mandate_version: String,
    pub retry_chain: Vec<String>,
    pub error_type: Option<String>,
    pub started_at: i64,
    pub ended_at: Option<i64>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SnapshotRecord {
    pub snapshot_id: String,
    pub run_id: String,
    pub step_id: Option<String>,
    pub phase: String,
    pub before: Value,
    pub after: Value,
    pub created_at: i64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FailedStep {
    pub run_id: String,
    pub step_id: String,
    pub attempt: u32,
    pub epoch: u64,
    pub failure_class: String,
    pub ended_at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", content = "record", rename_all = "snake_case")]
enum StoredEvent {
    RunRecorded(RunRecord),
    StepStarted(StepRecord),
    StepClosed(StepRecord),
    ModelCallRecorded(ModelCallRecord),
    SnapshotRecorded(SnapshotRecord),
    Trace(TraceRecord),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct TraceRecord {
    level: String,
    target: String,
    fields: Value,
    occurred_at: i64,
}

#[derive(Clone)]
pub struct Ledger {
    connection: Arc<Mutex<Connection>>,
    directory: Arc<PathBuf>,
}

impl Ledger {
    pub fn open(directory: impl AsRef<Path>) -> Result<Self, LedgerError> {
        let directory = directory.as_ref();
        std::fs::create_dir_all(directory).map_err(|error| {
            LedgerError::InvalidRecord(format!("create ledger directory: {error}"))
        })?;
        let state_path = directory.join(STATE_FILE);
        let events_path = directory.join(EVENTS_FILE);
        let connection = Connection::open(state_path)?;
        connection.busy_timeout(BUSY_TIMEOUT)?;
        connection.pragma_update(None, "journal_mode", "WAL")?;
        connection.pragma_update(None, "synchronous", "FULL")?;
        connection.execute(
            "ATTACH DATABASE ?1 AS events",
            [events_path.to_string_lossy().as_ref()],
        )?;
        connection.pragma_update(Some("events"), "journal_mode", "WAL")?;
        connection.pragma_update(Some("events"), "synchronous", "NORMAL")?;
        let mut connection = connection;
        {
            let transaction = immediate(&mut connection)?;
            create_event_schema(&transaction)?;
            rebuild_projections_in(&transaction)?;
            transaction.commit()?;
        }
        Ok(Self {
            connection: Arc::new(Mutex::new(connection)),
            directory: Arc::new(directory.to_path_buf()),
        })
    }

    pub fn directory(&self) -> &Path {
        self.directory.as_path()
    }

    pub fn record_run(&self, record: &RunRecord) -> Result<(), LedgerError> {
        self.write_event(StoredEvent::RunRecorded(record.clone()), |transaction| {
            project_run(transaction, record)
        })
    }

    pub fn record_model_call(&self, record: &ModelCallRecord) -> Result<(), LedgerError> {
        self.write_event(
            StoredEvent::ModelCallRecorded(record.clone()),
            |transaction| project_model_call(transaction, record),
        )
    }

    pub fn record_snapshot(&self, record: &SnapshotRecord) -> Result<(), LedgerError> {
        self.write_event(
            StoredEvent::SnapshotRecorded(record.clone()),
            |transaction| project_snapshot(transaction, record),
        )
    }

    pub fn append_step_started(&self, record: &StepRecord) -> Result<(), LedgerError> {
        validate_started(record)?;
        let mut connection = self.lock()?;
        let transaction = immediate(&mut connection)?;
        let existing: Option<(u32, String)> = transaction
            .query_row(
                "SELECT attempt, epoch FROM steps
                 WHERE run_id = ?1 AND step_id = ?2
                 ORDER BY epoch DESC LIMIT 1",
                params![record.run_id, record.step_id],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()?;
        if existing.is_some_and(|(_, epoch)| padded_u64(record.epoch) <= epoch) {
            return Err(LedgerError::StaleEpoch {
                step: record.step_id.clone(),
                epoch: record.epoch,
            });
        }
        let duplicate: bool = transaction.query_row(
            "SELECT EXISTS(SELECT 1 FROM steps
             WHERE run_id = ?1 AND step_id = ?2 AND attempt = ?3)",
            params![record.run_id, record.step_id, record.attempt],
            |row| row.get(0),
        )?;
        if duplicate {
            return Err(LedgerError::DuplicateAttempt {
                step: record.step_id.clone(),
                attempt: record.attempt,
            });
        }
        test_pause_after_step_read();
        append_event(&transaction, &StoredEvent::StepStarted(record.clone()))?;
        project_step(&transaction, record, false)?;
        transaction.commit()?;
        Ok(())
    }

    pub fn close_step(
        &self,
        run_id: &str,
        step_id: &str,
        attempt: u32,
        epoch: u64,
        completion: Completion,
    ) -> Result<(), LedgerError> {
        let mut connection = self.lock()?;
        let transaction = immediate(&mut connection)?;
        let greatest_epoch: Option<String> = transaction.query_row(
            "SELECT MAX(epoch) FROM steps WHERE run_id = ?1 AND step_id = ?2",
            params![run_id, step_id],
            |row| row.get(0),
        )?;
        if greatest_epoch.as_deref() != Some(padded_u64(epoch).as_str()) {
            return Err(LedgerError::StaleEpoch {
                step: step_id.to_owned(),
                epoch,
            });
        }
        let mut record = read_step(&transaction, run_id, step_id, attempt)?.ok_or_else(|| {
            LedgerError::MissingAttempt {
                step: step_id.to_owned(),
                attempt,
            }
        })?;
        if record.outcome.is_some() {
            return Err(LedgerError::AlreadyClosed {
                step: step_id.to_owned(),
                attempt,
            });
        }
        record.outcome = Some(completion.outcome);
        record.output = completion.output;
        record.said = completion.said.map(truncate_said);
        record.failure_class = completion.failure_class;
        record.ended_at = Some(completion.ended_at);
        append_event(&transaction, &StoredEvent::StepClosed(record.clone()))?;
        test_crash_after_close_event();
        // `checkpointed` cambia insieme all'esito: non esiste una finestra in
        // cui un passo concluso possa apparire rilanciabile.
        project_step(&transaction, &record, true)?;
        transaction.commit()?;
        Ok(())
    }

    pub fn steps(&self, run_id: &str) -> Result<Vec<StepRecord>, LedgerError> {
        let connection = self.lock()?;
        let mut statement = connection.prepare(
            "SELECT run_id, step_id, attempt, epoch, deps, input_digest, input,
                    gates, started_at, outcome, output, said, failure_class, ended_at
             FROM steps WHERE run_id = ?1 ORDER BY started_at, step_id, attempt",
        )?;
        let records = statement
            .query_map([run_id], step_from_row)?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(records)
    }

    pub fn is_checkpointed(
        &self,
        run_id: &str,
        step_id: &str,
        attempt: u32,
    ) -> Result<bool, LedgerError> {
        let connection = self.lock()?;
        Ok(connection
            .query_row(
                "SELECT checkpointed FROM steps
                 WHERE run_id = ?1 AND step_id = ?2 AND attempt = ?3",
                params![run_id, step_id, attempt],
                |row| row.get(0),
            )
            .optional()?
            .unwrap_or(false))
    }

    pub fn failed_steps_in_recent_runs(
        &self,
        failure_class: &str,
        run_limit: usize,
    ) -> Result<Vec<FailedStep>, LedgerError> {
        let connection = self.lock()?;
        let mut statement = connection.prepare(
            "WITH recent AS (
                 SELECT run_id FROM runs ORDER BY started_at DESC LIMIT ?1
             )
             SELECT s.run_id, s.step_id, s.attempt, s.epoch,
                    s.failure_class, s.ended_at
             FROM steps s JOIN recent r ON r.run_id = s.run_id
             WHERE s.failure_class = ?2 AND s.outcome = 'Broke'
             ORDER BY s.ended_at DESC",
        )?;
        let rows = statement.query_map(params![run_limit as i64, failure_class], |row| {
            Ok(FailedStep {
                run_id: row.get(0)?,
                step_id: row.get(1)?,
                attempt: row.get(2)?,
                epoch: u64_column(row, 3)?,
                failure_class: row.get(4)?,
                ended_at: row.get(5)?,
            })
        })?;
        Ok(rows.collect::<Result<Vec<_>, _>>()?)
    }

    pub fn rebuild_projections(&self) -> Result<(), LedgerError> {
        let mut connection = self.lock()?;
        let transaction = immediate(&mut connection)?;
        rebuild_projections_in(&transaction)?;
        transaction.commit()?;
        Ok(())
    }

    pub fn projection_dump(&self) -> Result<Value, LedgerError> {
        let connection = self.lock()?;
        let tables = ["runs", "steps", "model_calls", "snapshots"];
        let mut dump = serde_json::Map::new();
        for table in tables {
            dump.insert(table.to_owned(), dump_table(&connection, table)?);
        }
        Ok(Value::Object(dump))
    }

    fn write_event(
        &self,
        event: StoredEvent,
        project: impl FnOnce(&Transaction<'_>) -> Result<(), LedgerError>,
    ) -> Result<(), LedgerError> {
        let mut connection = self.lock()?;
        let transaction = immediate(&mut connection)?;
        append_event(&transaction, &event)?;
        project(&transaction)?;
        transaction.commit()?;
        Ok(())
    }

    fn append_trace(&self, trace: TraceRecord) -> Result<(), LedgerError> {
        self.write_event(StoredEvent::Trace(trace), |_| Ok(()))
    }

    fn lock(&self) -> Result<MutexGuard<'_, Connection>, LedgerError> {
        self.connection.lock().map_err(|_| LedgerError::Poisoned)
    }
}

impl RecordStore for Ledger {
    fn append_started(&mut self, record: StepRecord) -> Result<(), flow::FlowError> {
        self.append_step_started(&record)
            .map_err(|error| flow::FlowError::Store(error.to_string()))
    }

    fn close(
        &mut self,
        run_id: &str,
        step_id: &str,
        attempt: u32,
        epoch: u64,
        completion: Completion,
    ) -> Result<(), flow::FlowError> {
        self.close_step(run_id, step_id, attempt, epoch, completion)
            .map_err(|error| flow::FlowError::Store(error.to_string()))
    }

    fn records(&self, run_id: &str) -> Result<Vec<StepRecord>, flow::FlowError> {
        self.steps(run_id)
            .map_err(|error| flow::FlowError::Store(error.to_string()))
    }
}

#[derive(Clone)]
pub struct SqliteLayer {
    ledger: Ledger,
    clock: Arc<dyn Fn() -> i64 + Send + Sync>,
}

impl SqliteLayer {
    pub fn new(ledger: Ledger) -> Self {
        Self::with_clock(ledger, || {
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_or(0, |duration| duration.as_secs() as i64)
        })
    }

    pub fn with_clock(ledger: Ledger, clock: impl Fn() -> i64 + Send + Sync + 'static) -> Self {
        Self {
            ledger,
            clock: Arc::new(clock),
        }
    }
}

impl<S> Layer<S> for SqliteLayer
where
    S: Subscriber,
{
    fn on_event(&self, event: &Event<'_>, _context: Context<'_, S>) {
        let mut visitor = JsonVisitor::default();
        event.record(&mut visitor);
        let metadata = event.metadata();
        // `Layer` non può restituire l'errore al chiamante; un guasto del ponte
        // resta fuori dal percorso critico e non deve fermare il lavoro tracciato.
        let _ = self.ledger.append_trace(TraceRecord {
            level: metadata.level().to_string(),
            target: metadata.target().to_owned(),
            fields: Value::Object(visitor.fields),
            occurred_at: (self.clock)(),
        });
    }
}

#[derive(Default)]
struct JsonVisitor {
    fields: serde_json::Map<String, Value>,
}

impl Visit for JsonVisitor {
    fn record_i64(&mut self, field: &Field, value: i64) {
        self.fields.insert(field.name().to_owned(), value.into());
    }

    fn record_u64(&mut self, field: &Field, value: u64) {
        self.fields.insert(field.name().to_owned(), value.into());
    }

    fn record_bool(&mut self, field: &Field, value: bool) {
        self.fields.insert(field.name().to_owned(), value.into());
    }

    fn record_str(&mut self, field: &Field, value: &str) {
        self.fields
            .insert(field.name().to_owned(), value.to_owned().into());
    }

    fn record_debug(&mut self, field: &Field, value: &dyn fmt::Debug) {
        self.fields
            .insert(field.name().to_owned(), format!("{value:?}").into());
    }
}

fn immediate(connection: &mut Connection) -> Result<Transaction<'_>, LedgerError> {
    Ok(connection.transaction_with_behavior(TransactionBehavior::Immediate)?)
}

#[cfg(test)]
fn test_pause_after_step_read() {
    let Some(marker) = std::env::var_os("LEDGER_TEST_STEP_READ_MARKER") else {
        return;
    };
    std::fs::write(marker, b"ready").expect("scrivere il segnale della prova");
    let hold = std::env::var("LEDGER_TEST_STEP_READ_HOLD_MILLIS")
        .ok()
        .and_then(|value| value.parse().ok())
        .unwrap_or(0);
    std::thread::sleep(Duration::from_millis(hold));
}

#[cfg(not(test))]
fn test_pause_after_step_read() {}

#[cfg(test)]
fn test_crash_after_close_event() {
    if std::env::var_os("LEDGER_TEST_CRASH_AFTER_CLOSE_EVENT").is_some() {
        // `exit` salta i distruttori: la connessione muore con la transazione
        // aperta e SQLite deve annullare sia evento sia punto di controllo.
        std::process::exit(86);
    }
}

#[cfg(not(test))]
fn test_crash_after_close_event() {}

fn create_event_schema(connection: &Connection) -> Result<(), LedgerError> {
    connection.execute_batch(
        "CREATE TABLE IF NOT EXISTS events.events (
             seq INTEGER PRIMARY KEY AUTOINCREMENT,
             kind TEXT NOT NULL,
             run_id TEXT,
             step_id TEXT,
             attempt INTEGER,
             epoch TEXT,
             occurred_at INTEGER,
             payload TEXT NOT NULL
         );
         CREATE INDEX IF NOT EXISTS events.events_run_idx
             ON events(run_id, seq);
         CREATE INDEX IF NOT EXISTS events.events_step_idx
             ON events(run_id, step_id, attempt, seq);
         CREATE TRIGGER IF NOT EXISTS events.events_append_only_update
             BEFORE UPDATE ON events
             BEGIN SELECT RAISE(ABORT, 'events are append-only'); END;
         CREATE TRIGGER IF NOT EXISTS events.events_append_only_delete
             BEFORE DELETE ON events
             BEGIN SELECT RAISE(ABORT, 'events are append-only'); END;",
    )?;
    Ok(())
}

fn create_projection_schema(connection: &Connection) -> Result<(), LedgerError> {
    connection.execute_batch(
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
         CREATE INDEX IF NOT EXISTS runs_started_idx ON runs(started_at DESC);
         CREATE TABLE IF NOT EXISTS steps (
             run_id TEXT NOT NULL,
             step_id TEXT NOT NULL,
             attempt INTEGER NOT NULL,
             epoch TEXT NOT NULL,
             deps TEXT NOT NULL,
             input_digest TEXT NOT NULL,
             input TEXT NOT NULL,
             gates TEXT NOT NULL,
             started_at INTEGER NOT NULL,
             outcome TEXT,
             output TEXT,
             said TEXT,
             failure_class TEXT,
             ended_at INTEGER,
             checkpointed INTEGER NOT NULL DEFAULT 0 CHECK(checkpointed IN (0, 1)),
             PRIMARY KEY (run_id, step_id, attempt)
         );
         CREATE UNIQUE INDEX IF NOT EXISTS steps_epoch_idx
             ON steps(run_id, step_id, epoch);
         CREATE INDEX IF NOT EXISTS steps_failure_idx
             ON steps(failure_class, outcome, ended_at DESC);
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
         CREATE INDEX IF NOT EXISTS model_calls_run_idx
             ON model_calls(run_id, step_id, started_at);
         CREATE TABLE IF NOT EXISTS snapshots (
             snapshot_id TEXT PRIMARY KEY,
             run_id TEXT NOT NULL,
             step_id TEXT,
             phase TEXT NOT NULL,
             before_state TEXT NOT NULL,
             after_state TEXT NOT NULL,
             created_at INTEGER NOT NULL
         );
         CREATE INDEX IF NOT EXISTS snapshots_run_idx
             ON snapshots(run_id, step_id, phase);
         CREATE UNIQUE INDEX IF NOT EXISTS snapshots_phase_idx
             ON snapshots(run_id, IFNULL(step_id, ''), phase);",
    )?;
    Ok(())
}

fn drop_projection_schema(transaction: &Transaction<'_>) -> Result<(), LedgerError> {
    transaction.execute_batch(
        "DROP TABLE IF EXISTS snapshots;
         DROP TABLE IF EXISTS model_calls;
         DROP TABLE IF EXISTS steps;
         DROP TABLE IF EXISTS runs;",
    )?;
    Ok(())
}

fn rebuild_projections_in(transaction: &Transaction<'_>) -> Result<(), LedgerError> {
    drop_projection_schema(transaction)?;
    create_projection_schema(transaction)?;
    let events = {
        let mut statement =
            transaction.prepare("SELECT payload FROM events.events ORDER BY seq")?;
        let events = statement
            .query_map([], |row| row.get::<_, String>(0))?
            .collect::<Result<Vec<_>, _>>()?;
        events
    };
    for payload in events {
        let event: StoredEvent = serde_json::from_str(&payload)?;
        project_event(transaction, &event)?;
    }
    Ok(())
}

fn append_event(transaction: &Transaction<'_>, event: &StoredEvent) -> Result<(), LedgerError> {
    // Da qui l'evento va in `events.db`, mentre la successiva proiezione va in
    // `state.db`. Con WAL SQLite non rende atomico il commit dei due database
    // collegati: dopo uno schianto lo stato può restare più avanti del registro,
    // soprattutto con state FULL ed events NORMAL. La ricostruzione considera il
    // registro come verità e, a ogni apertura, rifà da zero le proiezioni,
    // perdendo quell'anticipo.
    let (kind, run_id, step_id, attempt, epoch, occurred_at) = event_metadata(event);
    let payload = serde_json::to_string(event)?;
    transaction.execute(
        "INSERT INTO events.events
         (kind, run_id, step_id, attempt, epoch, occurred_at, payload)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
        params![kind, run_id, step_id, attempt, epoch, occurred_at, payload],
    )?;
    Ok(())
}

type EventMetadata<'a> = (
    &'static str,
    Option<&'a str>,
    Option<&'a str>,
    Option<u32>,
    Option<String>,
    Option<i64>,
);

fn event_metadata(event: &StoredEvent) -> EventMetadata<'_> {
    match event {
        StoredEvent::RunRecorded(record) => (
            "run_recorded",
            Some(&record.run_id),
            None,
            None,
            None,
            Some(record.started_at),
        ),
        StoredEvent::StepStarted(record) => (
            "step_started",
            Some(&record.run_id),
            Some(&record.step_id),
            Some(record.attempt),
            Some(padded_u64(record.epoch)),
            Some(record.started_at),
        ),
        StoredEvent::StepClosed(record) => (
            "step_closed",
            Some(&record.run_id),
            Some(&record.step_id),
            Some(record.attempt),
            Some(padded_u64(record.epoch)),
            record.ended_at,
        ),
        StoredEvent::ModelCallRecorded(record) => (
            "model_call_recorded",
            Some(&record.run_id),
            record.step_id.as_deref(),
            None,
            None,
            Some(record.started_at),
        ),
        StoredEvent::SnapshotRecorded(record) => (
            "snapshot_recorded",
            Some(&record.run_id),
            record.step_id.as_deref(),
            None,
            None,
            Some(record.created_at),
        ),
        StoredEvent::Trace(record) => ("trace", None, None, None, None, Some(record.occurred_at)),
    }
}

fn project_event(transaction: &Transaction<'_>, event: &StoredEvent) -> Result<(), LedgerError> {
    match event {
        StoredEvent::RunRecorded(record) => project_run(transaction, record),
        StoredEvent::StepStarted(record) => project_step(transaction, record, false),
        StoredEvent::StepClosed(record) => project_step(transaction, record, true),
        StoredEvent::ModelCallRecorded(record) => project_model_call(transaction, record),
        StoredEvent::SnapshotRecorded(record) => project_snapshot(transaction, record),
        StoredEvent::Trace(_) => Ok(()),
    }
}

fn project_run(transaction: &Transaction<'_>, record: &RunRecord) -> Result<(), LedgerError> {
    transaction.execute(
        "INSERT INTO runs
         (run_id, kind, entity, parent_run_id, started_by, status,
          total_cost_micros, error, started_at, ended_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)
         ON CONFLICT(run_id) DO UPDATE SET
          kind=excluded.kind, entity=excluded.entity,
          parent_run_id=excluded.parent_run_id, started_by=excluded.started_by,
          status=excluded.status, total_cost_micros=excluded.total_cost_micros,
          error=excluded.error, started_at=excluded.started_at,
          ended_at=excluded.ended_at",
        params![
            record.run_id,
            record.kind,
            record.entity,
            record.parent_run_id,
            record.started_by,
            record.status,
            record.total_cost_micros,
            record.error,
            record.started_at,
            record.ended_at,
        ],
    )?;
    Ok(())
}

fn project_step(
    transaction: &Transaction<'_>,
    record: &StepRecord,
    checkpointed: bool,
) -> Result<(), LedgerError> {
    transaction.execute(
        "INSERT INTO steps
         (run_id, step_id, attempt, epoch, deps, input_digest, input, gates,
          started_at, outcome, output, said, failure_class, ended_at, checkpointed)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15)
         ON CONFLICT(run_id, step_id, attempt) DO UPDATE SET
          epoch=excluded.epoch, deps=excluded.deps,
          input_digest=excluded.input_digest, input=excluded.input,
          gates=excluded.gates, started_at=excluded.started_at,
          outcome=excluded.outcome, output=excluded.output, said=excluded.said,
          failure_class=excluded.failure_class, ended_at=excluded.ended_at,
          checkpointed=excluded.checkpointed",
        params![
            record.run_id,
            record.step_id,
            record.attempt,
            padded_u64(record.epoch),
            serde_json::to_string(&record.deps)?,
            record.input_digest,
            serde_json::to_string(&record.input)?,
            serde_json::to_string(&record.gates)?,
            record.started_at,
            record.outcome.map(outcome_name),
            record
                .output
                .as_ref()
                .map(serde_json::to_string)
                .transpose()?,
            record.said,
            record.failure_class,
            record.ended_at,
            checkpointed,
        ],
    )?;
    Ok(())
}

fn project_model_call(
    transaction: &Transaction<'_>,
    record: &ModelCallRecord,
) -> Result<(), LedgerError> {
    transaction.execute(
        "INSERT INTO model_calls VALUES
         (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13,
          ?14, ?15, ?16, ?17, ?18, ?19, ?20, ?21)
         ON CONFLICT(call_id) DO UPDATE SET
          run_id=excluded.run_id, step_id=excluded.step_id,
          purpose=excluded.purpose, cli=excluded.cli,
          requested_model=excluded.requested_model, actual_model=excluded.actual_model,
          input_tokens=excluded.input_tokens, output_tokens=excluded.output_tokens,
          cached_tokens=excluded.cached_tokens, cost_micros=excluded.cost_micros,
          price_currency=excluded.price_currency,
          input_price_micros_per_million=excluded.input_price_micros_per_million,
          output_price_micros_per_million=excluded.output_price_micros_per_million,
          cached_price_micros_per_million=excluded.cached_price_micros_per_million,
          mandate_name=excluded.mandate_name, mandate_version=excluded.mandate_version,
          retry_chain=excluded.retry_chain, error_type=excluded.error_type,
          started_at=excluded.started_at, ended_at=excluded.ended_at",
        params![
            record.call_id,
            record.run_id,
            record.step_id,
            record.purpose,
            record.cli,
            record.requested_model,
            record.actual_model,
            record.input_tokens.to_string(),
            record.output_tokens.to_string(),
            record.cached_tokens.to_string(),
            record.cost_micros,
            record.price_currency,
            record.input_price_micros_per_million,
            record.output_price_micros_per_million,
            record.cached_price_micros_per_million,
            record.mandate_name,
            record.mandate_version,
            serde_json::to_string(&record.retry_chain)?,
            record.error_type,
            record.started_at,
            record.ended_at,
        ],
    )?;
    Ok(())
}

fn project_snapshot(
    transaction: &Transaction<'_>,
    record: &SnapshotRecord,
) -> Result<(), LedgerError> {
    transaction.execute(
        "INSERT INTO snapshots
         (snapshot_id, run_id, step_id, phase, before_state, after_state, created_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
         ON CONFLICT(snapshot_id) DO UPDATE SET
          run_id=excluded.run_id, step_id=excluded.step_id, phase=excluded.phase,
          before_state=excluded.before_state, after_state=excluded.after_state,
          created_at=excluded.created_at",
        params![
            record.snapshot_id,
            record.run_id,
            record.step_id,
            record.phase,
            serde_json::to_string(&record.before)?,
            serde_json::to_string(&record.after)?,
            record.created_at,
        ],
    )?;
    Ok(())
}

fn validate_started(record: &StepRecord) -> Result<(), LedgerError> {
    if record.outcome.is_some()
        || record.output.is_some()
        || record.said.is_some()
        || record.failure_class.is_some()
        || record.ended_at.is_some()
    {
        return Err(LedgerError::InvalidRecord(
            "a started record contains closing fields".to_owned(),
        ));
    }
    Ok(())
}

fn read_step(
    connection: &Connection,
    run_id: &str,
    step_id: &str,
    attempt: u32,
) -> Result<Option<StepRecord>, LedgerError> {
    Ok(connection
        .query_row(
            "SELECT run_id, step_id, attempt, epoch, deps, input_digest, input,
                    gates, started_at, outcome, output, said, failure_class, ended_at
             FROM steps WHERE run_id = ?1 AND step_id = ?2 AND attempt = ?3",
            params![run_id, step_id, attempt],
            step_from_row,
        )
        .optional()?)
}

fn step_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<StepRecord> {
    let deps: String = row.get(4)?;
    let input: String = row.get(6)?;
    let gates: String = row.get(7)?;
    let outcome: Option<String> = row.get(9)?;
    let output: Option<String> = row.get(10)?;
    Ok(StepRecord {
        run_id: row.get(0)?,
        step_id: row.get(1)?,
        attempt: row.get(2)?,
        epoch: u64_column(row, 3)?,
        deps: json_column(&deps, 4)?,
        input_digest: row.get(5)?,
        input: json_column(&input, 6)?,
        gates: json_column(&gates, 7)?,
        started_at: row.get(8)?,
        outcome: outcome.as_deref().map(parse_outcome).transpose()?,
        output: output
            .as_deref()
            .map(|value| json_column(value, 10))
            .transpose()?,
        said: row.get(11)?,
        failure_class: row.get(12)?,
        ended_at: row.get(13)?,
    })
}

fn json_column<T: serde::de::DeserializeOwned>(value: &str, column: usize) -> rusqlite::Result<T> {
    serde_json::from_str(value).map_err(|error| {
        rusqlite::Error::FromSqlConversionFailure(
            column,
            rusqlite::types::Type::Text,
            Box::new(error),
        )
    })
}

fn u64_column(row: &rusqlite::Row<'_>, column: usize) -> rusqlite::Result<u64> {
    let value: String = row.get(column)?;
    value.parse().map_err(|error| {
        rusqlite::Error::FromSqlConversionFailure(
            column,
            rusqlite::types::Type::Text,
            Box::new(error),
        )
    })
}

fn padded_u64(value: u64) -> String {
    format!("{value:020}")
}

fn outcome_name(outcome: Outcome) -> &'static str {
    match outcome {
        Outcome::Went => "Went",
        Outcome::Broke => "Broke",
        Outcome::Waiting => "Waiting",
        Outcome::Stopped => "Stopped",
    }
}

fn parse_outcome(value: &str) -> rusqlite::Result<Outcome> {
    match value {
        "Went" => Ok(Outcome::Went),
        "Broke" => Ok(Outcome::Broke),
        "Waiting" => Ok(Outcome::Waiting),
        "Stopped" => Ok(Outcome::Stopped),
        other => Err(rusqlite::Error::FromSqlConversionFailure(
            9,
            rusqlite::types::Type::Text,
            std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                format!("unknown outcome {other}"),
            )
            .into(),
        )),
    }
}

fn truncate_said(value: String) -> String {
    let maximum = flow::MAX_SAID_BYTES;
    if value.len() <= maximum {
        return value;
    }
    let mut end = maximum;
    while !value.is_char_boundary(end) {
        end -= 1;
    }
    value[..end].to_owned()
}

fn dump_table(connection: &Connection, table: &str) -> Result<Value, LedgerError> {
    let columns = match table {
        "runs" => "run_id,kind,entity,parent_run_id,started_by,status,total_cost_micros,error,started_at,ended_at",
        "steps" => "run_id,step_id,attempt,epoch,deps,input_digest,input,gates,started_at,outcome,output,said,failure_class,ended_at,checkpointed",
        "model_calls" => "call_id,run_id,step_id,purpose,cli,requested_model,actual_model,input_tokens,output_tokens,cached_tokens,cost_micros,price_currency,input_price_micros_per_million,output_price_micros_per_million,cached_price_micros_per_million,mandate_name,mandate_version,retry_chain,error_type,started_at,ended_at",
        "snapshots" => "snapshot_id,run_id,step_id,phase,before_state,after_state,created_at",
        _ => return Err(LedgerError::InvalidRecord("unknown projection".to_owned())),
    };
    let order = match table {
        "runs" => "run_id",
        "steps" => "run_id,step_id,attempt",
        "model_calls" => "call_id",
        "snapshots" => "snapshot_id",
        _ => unreachable!(),
    };
    let sql = format!("SELECT json_array({columns}) FROM {table} ORDER BY {order}");
    let mut statement = connection.prepare(&sql)?;
    let rows = statement
        .query_map([], |row| row.get::<_, String>(0))?
        .map(|row| row.and_then(|value| json_column(&value, 0)))
        .collect::<Result<Vec<Value>, _>>()?;
    Ok(Value::Array(rows))
}

#[cfg(test)]
mod tests;
