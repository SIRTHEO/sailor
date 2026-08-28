//! Il deposito durevole di Sailor.
//!
//! `events.db` contiene la verità append-only; `state.db` contiene quattro
//! proiezioni interrogabili e un segno dell'ultimo evento incorporato. I due
//! file sono collegati, ma il nuovo evento e la sua proiezione vengono commessi
//! in due fasi perché WAL non offre atomicità fra database collegati.

use flow::{AttemptRelation, Completion, Outcome, RecordStore, StepRecord, StepSpecies};
use rusqlite::{params, Connection, OptionalExtension, Transaction, TransactionBehavior};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeMap;
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
const PROJECTION_SCHEMA_VERSION: i64 = 3;

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

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GatesChangedStep {
    pub run_id: String,
    pub step_id: String,
    pub attempt: u32,
    pub epoch: u64,
}

/// Una corsa con almeno un passo aperto, come la vede chi riprende.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UnfinishedRun {
    pub run_id: String,
    /// Su cosa lavorava la corsa. Vuota se nessuno l'ha mai registrata.
    pub entity: String,
    pub open_steps: usize,
    pub oldest_started_at: i64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiscardedOutputStep {
    pub run_id: String,
    pub step_id: String,
    pub attempt: u32,
    pub epoch: u64,
    pub bytes_seen: u64,
    pub bytes_discarded: u64,
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
            let projection_schema_version: i64 =
                transaction.pragma_query_value(None, "user_version", |row| row.get(0))?;
            if !(0..=PROJECTION_SCHEMA_VERSION).contains(&projection_schema_version) {
                return Err(LedgerError::InvalidRecord(format!(
                    "unsupported projection schema version {projection_schema_version}"
                )));
            }
            // La creazione delle tabelle, l'adeguamento delle colonne e la nascita
            // degli indici sono tre fasi distinte: un deposito nuovo crea tutto subito,
            // un deposito vecchio deve prima aggiungere le colonne mancanti affinché
            // gli indici possano agganciarsi, e un deposito aggiornato non tocca nulla.
            create_projection_tables(&transaction)?;
            initialize_projection_watermark(&transaction)?;
            if projection_schema_version < PROJECTION_SCHEMA_VERSION {
                add_missing_projection_columns(&transaction)?;
                transaction.pragma_update(None, "user_version", PROJECTION_SCHEMA_VERSION)?;
            }
            create_projection_indexes(&transaction)?;
            apply_pending_events(&transaction)?;
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
        self.write_event(StoredEvent::RunRecorded(record.clone()))
    }

    pub fn record_model_call(&self, record: &ModelCallRecord) -> Result<(), LedgerError> {
        self.write_event(StoredEvent::ModelCallRecorded(record.clone()))
    }

    pub fn record_snapshot(&self, record: &SnapshotRecord) -> Result<(), LedgerError> {
        self.write_event(StoredEvent::SnapshotRecorded(record.clone()))
    }

    pub fn append_step_started(&self, record: &StepRecord) -> Result<(), LedgerError> {
        validate_started(record)?;
        let mut connection = self.lock()?;
        let transaction = immediate(&mut connection)?;
        apply_pending_events(&transaction)?;
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
        transaction.commit()?;
        apply_pending(&mut connection)?;
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
        apply_pending_events(&transaction)?;
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
        let mut record =
            read_step(&transaction, run_id, step_id, attempt, epoch)?.ok_or_else(|| {
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
        record.said = completion.said.map(flow::truncate_said);
        record.failure_class = completion.failure_class;
        record.ended_at = Some(completion.ended_at);
        record.bytes_seen = completion.bytes_seen;
        record.bytes_discarded = completion.bytes_discarded;
        append_event(&transaction, &StoredEvent::StepClosed(record.clone()))?;
        transaction.commit()?;
        test_crash_after_close_event();
        apply_pending(&mut connection)?;
        Ok(())
    }

    pub fn steps(&self, run_id: &str) -> Result<Vec<StepRecord>, LedgerError> {
        let connection = self.lock()?;
        let mut statement = connection.prepare(
            "SELECT run_id, step_id, attempt, epoch, deps, input_digest, input,
                    gates, attempt_relation, started_at, outcome, output, said,
                    failure_class, ended_at, bytes_seen, bytes_discarded,
                    held_by_pid, species
             FROM steps WHERE run_id = ?1 ORDER BY started_at, step_id, attempt",
        )?;
        let records = statement
            .query_map([run_id], step_from_row)?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(records)
    }

    pub fn steps_resumed_with_changed_gates(&self) -> Result<Vec<GatesChangedStep>, LedgerError> {
        let connection = self.lock()?;
        let mut statement = connection.prepare(
            "SELECT run_id, step_id, attempt, epoch FROM steps
             WHERE attempt_relation = 'same_input_gates_changed'
             ORDER BY started_at, run_id, step_id, attempt",
        )?;
        let rows = statement.query_map([], |row| {
            Ok(GatesChangedStep {
                run_id: row.get(0)?,
                step_id: row.get(1)?,
                attempt: row.get(2)?,
                epoch: u64_column(row, 3)?,
            })
        })?;
        Ok(rows.collect::<Result<Vec<_>, _>>()?)
    }

    /// Le corse rimaste a metà: hanno un'intenzione scritta e nessun esito.
    ///
    /// È LA DOMANDA DELLA RIPRESA, e non nomina nessuna lavorazione: chi
    /// riprende non sa quali corse esistano, sa solo di voler chiudere ciò che
    /// è rimasto aperto. Prima di questo metodo il nome della corsa andava
    /// ricostruito da fuori — nel servizio notturno, con la data di oggi — e
    /// chi si era interrotto prima di mezzanotte non veniva ritrovato mai.
    ///
    /// L'ordine è quello di apertura: si riprende ciò che è fermo da più tempo.
    pub fn unfinished_runs(&self) -> Result<Vec<UnfinishedRun>, LedgerError> {
        let connection = self.lock()?;
        let mut statement = connection.prepare(
            "SELECT steps.run_id, COALESCE(runs.entity, ''), COUNT(*), MIN(steps.started_at)
             FROM steps LEFT JOIN runs ON runs.run_id = steps.run_id
             WHERE steps.outcome IS NULL
             GROUP BY steps.run_id
             ORDER BY MIN(steps.started_at), steps.run_id",
        )?;
        let rows = statement.query_map([], |row| {
            let open: i64 = row.get(2)?;
            Ok(UnfinishedRun {
                run_id: row.get(0)?,
                entity: row.get(1)?,
                open_steps: open as usize,
                oldest_started_at: row.get(3)?,
            })
        })?;
        Ok(rows.collect::<Result<Vec<_>, _>>()?)
    }

    /// Quando ciascuna entità ha cominciato l'ultima volta.
    ///
    /// Serve a sapere se un flusso pianificato è dovuto adesso, e la domanda è
    /// **quando è partito**, non quando è finito: una corsa ancora in volo ha
    /// già consumato il suo turno, e contarla come «mai girata» la farebbe
    /// ripartire sopra se stessa.
    pub fn last_started_at(&self) -> Result<BTreeMap<String, i64>, LedgerError> {
        let connection = self.lock()?;
        let mut statement = connection.prepare(
            "SELECT entity, MAX(started_at) FROM runs WHERE entity <> '' GROUP BY entity",
        )?;
        let rows = statement.query_map([], |row| {
            let entity: String = row.get(0)?;
            let started: i64 = row.get(1)?;
            Ok((entity, started))
        })?;
        Ok(rows.collect::<Result<BTreeMap<_, _>, _>>()?)
    }

    pub fn steps_with_discarded_output(&self) -> Result<Vec<DiscardedOutputStep>, LedgerError> {
        let connection = self.lock()?;
        let mut statement = connection.prepare(
            "SELECT run_id, step_id, attempt, epoch, bytes_seen, bytes_discarded
             FROM steps
             WHERE bytes_discarded > 0
             ORDER BY started_at, run_id, step_id, attempt",
        )?;
        let rows = statement.query_map([], |row| {
            let seen: i64 = row.get(4)?;
            let discarded: i64 = row.get(5)?;
            Ok(DiscardedOutputStep {
                run_id: row.get(0)?,
                step_id: row.get(1)?,
                attempt: row.get(2)?,
                epoch: u64_column(row, 3)?,
                bytes_seen: seen as u64,
                bytes_discarded: discarded as u64,
            })
        })?;
        Ok(rows.collect::<Result<Vec<_>, _>>()?)
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

    fn write_event(&self, event: StoredEvent) -> Result<(), LedgerError> {
        let mut connection = self.lock()?;
        let transaction = immediate(&mut connection)?;
        apply_pending_events(&transaction)?;
        append_event(&transaction, &event)?;
        transaction.commit()?;
        apply_pending(&mut connection)?;
        Ok(())
    }

    fn append_trace(&self, trace: TraceRecord) -> Result<(), LedgerError> {
        self.write_event(StoredEvent::Trace(trace))
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

fn apply_pending(connection: &mut Connection) -> Result<(), LedgerError> {
    let transaction = immediate(connection)?;
    apply_pending_events(&transaction)?;
    transaction.commit()?;
    Ok(())
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
        // L'evento è durevole, il watermark no: l'apertura deve incorporarlo
        // prima che il passo possa apparire rilanciabile.
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
    create_projection_tables(connection)?;
    create_projection_indexes(connection)?;
    Ok(())
}

fn create_projection_tables(connection: &Connection) -> Result<(), LedgerError> {
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
             held_by_pid INTEGER,
             species TEXT,
             started_at INTEGER NOT NULL,
             outcome TEXT,
             output TEXT,
             said TEXT,
             failure_class TEXT,
             ended_at INTEGER,
             bytes_seen INTEGER,
             bytes_discarded INTEGER,
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
    )?;
    Ok(())
}

fn create_projection_indexes(connection: &Connection) -> Result<(), LedgerError> {
    connection.execute_batch(
        "CREATE INDEX IF NOT EXISTS runs_started_idx ON runs(started_at DESC);
         CREATE UNIQUE INDEX IF NOT EXISTS steps_epoch_idx
             ON steps(run_id, step_id, epoch);
         CREATE INDEX IF NOT EXISTS steps_failure_idx
             ON steps(failure_class, outcome, ended_at DESC);
         CREATE INDEX IF NOT EXISTS steps_attempt_relation_idx
             ON steps(attempt_relation, started_at);
         CREATE INDEX IF NOT EXISTS steps_discarded_idx
             ON steps(bytes_discarded, started_at);
         CREATE INDEX IF NOT EXISTS model_calls_run_idx
             ON model_calls(run_id, step_id, started_at);
         CREATE INDEX IF NOT EXISTS snapshots_run_idx
             ON snapshots(run_id, step_id, phase);
         CREATE UNIQUE INDEX IF NOT EXISTS snapshots_phase_idx
             ON snapshots(run_id, IFNULL(step_id, ''), phase);",
    )?;
    Ok(())
}

/// Porta la proiezione di un deposito già esistente alla versione corrente
/// aggiungendo le colonne opzionali nate dopo di lui. Non è una catena di
/// migrazioni numerate: ogni colonna si aggiunge se manca, quindi la stessa
/// funzione porta a destinazione un deposito di qualunque versione passata,
/// e rieseguirla non fa niente. Nessuna invalida le proiezioni esistenti né
/// obbliga a rileggere il registro degli eventi — i valori dei record già
/// scritti restano nulli, che è esattamente ciò che erano.
fn add_missing_projection_columns(transaction: &Transaction<'_>) -> Result<(), LedgerError> {
    for (column, kind) in [
        // versione 2
        ("bytes_seen", "INTEGER"),
        ("bytes_discarded", "INTEGER"),
        // versione 3: chi teneva il passo, e se rifarlo è sicuro
        ("held_by_pid", "INTEGER"),
        ("species", "TEXT"),
    ] {
        if !column_exists(transaction, "steps", column)? {
            transaction.execute(&format!("ALTER TABLE steps ADD COLUMN {column} {kind}"), [])?;
        }
    }
    Ok(())
}

fn column_exists(connection: &Connection, table: &str, column: &str) -> Result<bool, LedgerError> {
    let mut statement = connection.prepare(&format!("PRAGMA table_info({table})"))?;
    let mut rows = statement.query([])?;
    while let Some(row) = rows.next()? {
        let name: String = row.get(1)?;
        if name == column {
            return Ok(true);
        }
    }
    Ok(false)
}

fn initialize_projection_watermark(connection: &Connection) -> Result<(), LedgerError> {
    connection.execute(
        "INSERT OR IGNORE INTO projection_watermark (singleton, last_applied_seq) VALUES (1, 0)",
        [],
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
    let (event_count, log_high_water): (i64, i64) = transaction.query_row(
        "SELECT COUNT(*), COALESCE((SELECT seq FROM events.sqlite_sequence
                                    WHERE name = 'events'), 0)
         FROM events.events",
        [],
        |row| Ok((row.get(0)?, row.get(1)?)),
    )?;
    if event_count != log_high_water {
        return Err(LedgerError::InvalidRecord(
            "cannot rebuild projections from a pruned event log".to_owned(),
        ));
    }
    drop_projection_schema(transaction)?;
    create_projection_schema(transaction)?;
    transaction.execute("DELETE FROM projection_watermark", [])?;
    initialize_projection_watermark(transaction)?;
    let events = {
        let mut statement =
            transaction.prepare("SELECT seq, payload FROM events.events ORDER BY seq")?;
        let events = statement
            .query_map([], |row| {
                Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?))
            })?
            .collect::<Result<Vec<_>, _>>()?;
        events
    };
    for (seq, payload) in events {
        let event: StoredEvent = serde_json::from_str(&payload)?;
        project_event(transaction, &event)?;
        set_projection_watermark(transaction, seq)?;
    }
    transaction.pragma_update(None, "user_version", PROJECTION_SCHEMA_VERSION)?;
    Ok(())
}

fn apply_pending_events(transaction: &Transaction<'_>) -> Result<(), LedgerError> {
    let watermark: Option<i64> = transaction
        .query_row(
            "SELECT last_applied_seq FROM projection_watermark WHERE singleton = 1",
            [],
            |row| row.get(0),
        )
        .optional()?;
    let watermark = watermark
        .ok_or_else(|| LedgerError::InvalidRecord("projection watermark is missing".to_owned()))?;
    let log_high_water: i64 = transaction.query_row(
        "SELECT COALESCE((SELECT seq FROM events.sqlite_sequence
                              WHERE name = 'events'), 0)",
        [],
        |row| row.get(0),
    )?;
    if watermark > log_high_water {
        return Err(LedgerError::InvalidRecord(format!(
            "projection watermark {watermark} is ahead of event log {log_high_water}"
        )));
    }
    let events = {
        let mut statement = transaction
            .prepare("SELECT seq, payload FROM events.events WHERE seq > ?1 ORDER BY seq")?;
        let events = statement
            .query_map([watermark], |row| {
                Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?))
            })?
            .collect::<Result<Vec<_>, _>>()?;
        events
    };
    for (seq, payload) in events {
        test_count_applied_event_read();
        let event: StoredEvent = serde_json::from_str(&payload)?;
        project_event(transaction, &event)?;
        set_projection_watermark(transaction, seq)?;
    }
    Ok(())
}

fn set_projection_watermark(transaction: &Transaction<'_>, seq: i64) -> Result<(), LedgerError> {
    transaction.execute(
        "UPDATE projection_watermark SET last_applied_seq = ?1 WHERE singleton = 1",
        [seq],
    )?;
    Ok(())
}

fn append_event(transaction: &Transaction<'_>, event: &StoredEvent) -> Result<(), LedgerError> {
    // Da qui l'evento va in `events.db`, mentre la successiva proiezione va in
    // `state.db`. Con WAL SQLite non rende atomico il commit dei due database
    // collegati. Il nuovo evento non viene mai proiettato nella transazione che
    // lo inserisce; il watermark avanza solo nella transazione seguente. Uno
    // schianto può lasciare la proiezione indietro, mai davanti al registro, e
    // l'apertura applica soltanto la coda che manca.
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
          attempt_relation, started_at, outcome, output, said, failure_class,
          ended_at, bytes_seen, bytes_discarded, held_by_pid, species,
          checkpointed)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14,
                 ?15, ?16, ?17, ?18, ?19, ?20)
         ON CONFLICT(run_id, step_id, attempt) DO UPDATE SET
          epoch=excluded.epoch, deps=excluded.deps,
          input_digest=excluded.input_digest, input=excluded.input,
          gates=excluded.gates, attempt_relation=excluded.attempt_relation,
          started_at=excluded.started_at,
          outcome=excluded.outcome, output=excluded.output, said=excluded.said,
          failure_class=excluded.failure_class, ended_at=excluded.ended_at,
          bytes_seen=excluded.bytes_seen, bytes_discarded=excluded.bytes_discarded,
          held_by_pid=excluded.held_by_pid, species=excluded.species,
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
            record.attempt_relation.map(attempt_relation_name),
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
            record.bytes_seen.map(|bytes| bytes as i64),
            record.bytes_discarded.map(|bytes| bytes as i64),
            record.held_by_pid.map(|pid| pid as i64),
            record.species.map(species_name),
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
        || record.bytes_seen.is_some()
        || record.bytes_discarded.is_some()
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
    epoch: u64,
) -> Result<Option<StepRecord>, LedgerError> {
    Ok(connection
        .query_row(
            "SELECT run_id, step_id, attempt, epoch, deps, input_digest, input,
                    gates, attempt_relation, started_at, outcome, output, said,
                    failure_class, ended_at, bytes_seen, bytes_discarded,
                    held_by_pid, species
             FROM steps
             WHERE run_id = ?1 AND step_id = ?2 AND attempt = ?3 AND epoch = ?4",
            params![run_id, step_id, attempt, padded_u64(epoch)],
            step_from_row,
        )
        .optional()?)
}

#[cfg(test)]
thread_local! {
    static APPLIED_EVENT_READS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

#[cfg(test)]
fn test_count_applied_event_read() {
    APPLIED_EVENT_READS.set(APPLIED_EVENT_READS.get() + 1);
}

#[cfg(not(test))]
fn test_count_applied_event_read() {}

fn step_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<StepRecord> {
    let deps: String = row.get(4)?;
    let input: String = row.get(6)?;
    let gates: String = row.get(7)?;
    let attempt_relation: Option<String> = row.get(8)?;
    let outcome: Option<String> = row.get(10)?;
    let output: Option<String> = row.get(11)?;
    let bytes_seen: Option<i64> = row.get(15)?;
    let bytes_discarded: Option<i64> = row.get(16)?;
    let held_by_pid: Option<i64> = row.get(17)?;
    let species: Option<String> = row.get(18)?;
    Ok(StepRecord {
        run_id: row.get(0)?,
        step_id: row.get(1)?,
        attempt: row.get(2)?,
        epoch: u64_column(row, 3)?,
        deps: json_column(&deps, 4)?,
        input_digest: row.get(5)?,
        input: json_column(&input, 6)?,
        gates: json_column(&gates, 7)?,
        attempt_relation: attempt_relation
            .as_deref()
            .map(parse_attempt_relation)
            .transpose()?,
        started_at: row.get(9)?,
        outcome: outcome
            .as_deref()
            .map(|value| parse_outcome(value, 10))
            .transpose()?,
        output: output
            .as_deref()
            .map(|value| json_column(value, 11))
            .transpose()?,
        said: row.get(12)?,
        failure_class: row.get(13)?,
        ended_at: row.get(14)?,
        bytes_seen: bytes_seen.map(|b| b as u64),
        bytes_discarded: bytes_discarded.map(|b| b as u64),
        held_by_pid: held_by_pid.map(|pid| pid as u32),
        species: species.as_deref().map(parse_species).transpose()?,
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
        Outcome::Skipped => "Skipped",
    }
}

fn parse_outcome(value: &str, column: usize) -> rusqlite::Result<Outcome> {
    match value {
        "Went" => Ok(Outcome::Went),
        "Broke" => Ok(Outcome::Broke),
        "Waiting" => Ok(Outcome::Waiting),
        "Stopped" => Ok(Outcome::Stopped),
        "Skipped" => Ok(Outcome::Skipped),
        other => Err(rusqlite::Error::FromSqlConversionFailure(
            column,
            rusqlite::types::Type::Text,
            std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                format!("unknown outcome {other}"),
            )
            .into(),
        )),
    }
}

fn species_name(species: StepSpecies) -> &'static str {
    match species {
        StepSpecies::Repeatable => "repeatable",
        StepSpecies::Compensable => "compensable",
        StepSpecies::HandToHuman => "hand_to_human",
    }
}

/// Una specie che il deposito non riconosce è un errore, non un ripiego
/// silenzioso: leggerla come `hand_to_human` sarebbe prudente per il singolo
/// passo e falso per chi legge la storia.
fn parse_species(value: &str) -> rusqlite::Result<StepSpecies> {
    match value {
        "repeatable" => Ok(StepSpecies::Repeatable),
        "compensable" => Ok(StepSpecies::Compensable),
        "hand_to_human" => Ok(StepSpecies::HandToHuman),
        other => Err(rusqlite::Error::FromSqlConversionFailure(
            18,
            rusqlite::types::Type::Text,
            std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                format!("unknown step species {other}"),
            )
            .into(),
        )),
    }
}

fn attempt_relation_name(relation: AttemptRelation) -> &'static str {
    match relation {
        AttemptRelation::SameInput => "same_input",
        AttemptRelation::SameInputGatesChanged => "same_input_gates_changed",
        AttemptRelation::DifferentInput => "different_input",
    }
}

fn parse_attempt_relation(value: &str) -> rusqlite::Result<AttemptRelation> {
    match value {
        "same_input" => Ok(AttemptRelation::SameInput),
        "same_input_gates_changed" => Ok(AttemptRelation::SameInputGatesChanged),
        "different_input" => Ok(AttemptRelation::DifferentInput),
        other => Err(rusqlite::Error::FromSqlConversionFailure(
            8,
            rusqlite::types::Type::Text,
            std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                format!("unknown attempt relation {other}"),
            )
            .into(),
        )),
    }
}

fn dump_table(connection: &Connection, table: &str) -> Result<Value, LedgerError> {
    let columns = match table {
        "runs" => "run_id,kind,entity,parent_run_id,started_by,status,total_cost_micros,error,started_at,ended_at",
        "steps" => "run_id,step_id,attempt,epoch,deps,input_digest,input,gates,attempt_relation,started_at,outcome,output,said,failure_class,ended_at,bytes_seen,bytes_discarded,held_by_pid,species,checkpointed",
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
