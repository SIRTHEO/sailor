//! Sailor's durable store.
//!
//! `events.db` holds the append-only truth; `state.db` holds four queryable
//! projections plus a mark of the last event folded in. The two files are
//! attached, but a new event and its projection are committed in two phases
//! because WAL gives no atomicity across attached databases.

use flow::{AttemptRelation, Completion, Outcome, RecordStore, Spend, StepRecord, StepSpecies};
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

pub mod identity;
pub mod search;
pub mod reports;
pub mod self_care;
pub mod streaks;

pub use identity::EngineIdentity;

const STATE_FILE: &str = "state.db";
const EVENTS_FILE: &str = "events.db";

/// Where this machine's store lives.
///
/// **ONE COPY ONLY** — the flow runner and the relay both ask here. It nearly
/// got a twin: a change taught the window to look elsewhere and left the old
/// path here, so whoever ran the flows would write in one home and whoever
/// watched them read the other, **with neither side reporting an error**.
pub fn default_directory() -> Option<PathBuf> {
    // It was a private function of `sailor flow`, and while one command was the
    // only reader of the store that was enough. The relay reads it too now, to
    // know what work it is on, so finding the home lives here — where everybody
    // already passes — instead of in each caller.
    if let Some(declared) = env_path("SAILOR_LEDGER") {
        return Some(declared);
    }
    // `None` when `HOME` is undefined: every caller has a fallback, and a home
    // deduced from nothing writes in somebody else's place.
    Some(sailor_home()?.join("ledger"))
}

/// Sailor's home: where the store, the flows and the configuration live.
///
/// **No single person's path**: `SAILOR_HOME` if declared, else the standard
/// configuration directory, else the running user's. `None` when the
/// environment declares neither.
pub fn sailor_home() -> Option<PathBuf> {
    Some(sailor_home_in(
        env_path("SAILOR_HOME"),
        env_path("XDG_CONFIG_HOME"),
        env_path("HOME")?,
    ))
}

/// The same rule applied to a declared environment rather than this process's.
///
/// **Exists because the home used to live in two places**, and the second copy
/// was the one whoever searches a *described* machine for descriptors went
/// through. Whoever wants the home asks here now, whoever they are, so there is
/// no second copy left to disagree with this one.
pub fn sailor_home_in(
    declared: Option<PathBuf>,
    xdg_config: Option<PathBuf>,
    home: PathBuf,
) -> PathBuf {
    if let Some(declared) = declared {
        return declared;
    }
    // `toolbox::default_sources` and `trigger::default_sources` used to skip
    // this branch: they ignored `XDG_CONFIG_HOME` and fell back to `~/.sailor`
    // instead of `~/.config/sailor`, so the price list and a person's own
    // descriptors landed in two different homes — and the documentation sent
    // everybody to the home the price-list code does not read.
    if let Some(config) = xdg_config {
        return config.join("sailor");
    }
    home.join(".config").join("sailor")
}

/// An environment variable read as a path. The empty string counts as "not
/// set": that is what a script exporting a valueless variable leaves behind,
/// and taking it literally would write into the filesystem root.
fn env_path(name: &str) -> Option<PathBuf> {
    std::env::var_os(name)
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
}
const BUSY_TIMEOUT: Duration = Duration::from_secs(5);
/// The shape of the projections this code expects.
///
/// **RAISE IT TOGETHER WITH THE COLUMNS, AND ONCE IT WAS NOT**: the migration
/// learned four cache columns while this stayed at 4, an existing store was
/// already registered at 4, `4 < 4` is false, the migration never ran, and
/// every read died with `no such column: cache_write_tokens`.
const PROJECTION_SCHEMA_VERSION: i64 = 13;

pub enum LedgerError {
    Sqlite(rusqlite::Error),
    Json(serde_json::Error),
    InvalidRecord(String),
    DuplicateAttempt { step: String, attempt: u32 },
    MissingAttempt { step: String, attempt: u32 },
    AlreadyClosed { step: String, attempt: u32 },
    StaleEpoch { step: String, epoch: u64 },
    Poisoned,
    /// A question the store will not answer: a browse that would write.
    Refused(String),
}

/// **`.expect()` PRINTS THE `Debug`, NOT THE `Display`.** A derived one showed
/// the fields and hid the sentence in every red test.
impl fmt::Debug for LedgerError {
    fn fmt(&self, out: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(self, out)
    }
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
            Self::Refused(why) => write!(formatter, "refused: {why}"),
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
    /// The workspace this run was born in. `None` is not missing data: a run
    /// started outside every workspace is a real run, and outside is a place.
    pub worktree: Option<String>,
    /// Why the run closed short, as `flow::StopReason` writes it. `None` for a
    /// run that reached its last step, failed, or is still open: only a run
    /// that stopped itself has one.
    ///
    /// Read back by a store written before the column existed, so an old event
    /// still becomes a record.
    #[serde(default)]
    pub stop_reason: Option<String>,
}

/// An entry seen by an inventory scan.
///
/// The fields are plain text: the store does **not** depend on the crate that
/// produces them, and must not. If the inventory learns to recognise a new
/// family, nothing changes here — while a shared `enum` would force a store
/// migration for every new word.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct InventoryItem {
    pub kind: String,
    pub name: String,
    pub origin: String,
    pub path: String,
    /// `active`, `inactive` or `unknown`.
    pub reach: String,
    /// Why it is unreachable, when it is.
    pub reason: Option<String>,
}

/// A whole scan, with its instant.
///
/// THE SCAN IS STORED, NOT THE SINGLE ENTRY, and that difference is everything
/// that makes the store worth having: from a complete list you also know **what
/// is no longer there**. Entry by entry would only say what was seen, and
/// "gone" would stay indistinguishable from "not yet looked at".
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct InventoryScan {
    pub taken_at: i64,
    pub items: Vec<InventoryItem>,
}

/// What changed for one entry between two scans.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct InventoryChange {
    pub kind: String,
    pub name: String,
    pub path: String,
    pub origin: String,
    pub reach: String,
    pub reason: Option<String>,
    pub first_seen: i64,
    pub last_seen: i64,
    /// The instant of the scan in which it vanished, if it did.
    pub gone_at: Option<i64>,
}

/// A call to a model, with what it consumed and what it cost.
///
/// **`None` MEANS "UNKNOWN", AND THERE IS NO FALLBACK ZERO.** "Zero tokens"
/// against "that engine does not say how many it used" is a measure against a
/// lie: a zero written for "unknown" gets summed, and no view downstream can
/// undo it. `None` says the call is unmeasured, which is a fact to act on.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ModelCallRecord {
    pub call_id: String,
    pub run_id: String,
    pub step_id: Option<String>,
    pub purpose: String,
    pub cli: String,
    pub requested_model: String,
    pub actual_model: String,
    /// Every `serde(default)` below is here for one reason, and it is not
    /// laziness. The store is event-based: an event written when these were
    /// plain numbers still reads back — `10` becomes `Some(10)` — one written
    /// without the field becomes `None`, and without that fallback an upgrade
    /// would unread the log everything else is rebuilt from.
    #[serde(default)]
    pub input_tokens: Option<u64>,
    #[serde(default)]
    pub output_tokens: Option<u64>,
    /// Input tokens read from the cache, in a column of their own: they have
    /// their own price per million, often an order of magnitude below that of
    /// fresh input.
    #[serde(default)]
    pub cached_tokens: Option<u64>,
    /// Input tokens **written** to the cache: not the ones read, and dearer.
    ///
    /// **BORN FROM A MEASURE**: a call with two input and four output tokens
    /// cost $0.1285 as the engine declared it, of which the 12,347 tokens
    /// written to cache were 96%. Without a column for them, every row of this
    /// table underestimated the spend 24-fold, and always downwards.
    #[serde(default)]
    pub cache_write_tokens: Option<u64>,
    /// Tokens written to a **long-lived** cache, where the provider offers more
    /// than one and prices them differently.
    #[serde(default)]
    pub cache_write_long_tokens: Option<u64>,
    /// The total, for engines that report **only** that without splitting the
    /// two sides. Without this field the one real measure those engines give
    /// would be thrown away for want of a way to split it in three.
    #[serde(default)]
    pub total_tokens: Option<u64>,
    /// **HOW MANY TURNS THIS CALL TOOK.** Not a curiosity: measured, a chain of
    /// four steps read 8% more per turn than a single session doing the same
    /// work, and consumed twice as much — because it took twice the turns. No
    /// column held them, so whoever set out to make a chain cheaper was working
    /// on a quantity nobody was measuring.
    #[serde(default)]
    pub turns: Option<u64>,
    pub cost_micros: Option<i64>,
    /// The cost the engine declared itself, kept **beside** the price-list one
    /// and never in its place: if the two diverge systematically, that
    /// divergence is itself the information. A cost coming from the same place
    /// as the spend verifies nothing.
    #[serde(default)]
    pub declared_cost_micros: Option<i64>,
    #[serde(default)]
    pub price_currency: Option<String>,
    #[serde(default)]
    pub input_price_micros_per_million: Option<i64>,
    #[serde(default)]
    pub output_price_micros_per_million: Option<i64>,
    #[serde(default)]
    pub cached_price_micros_per_million: Option<i64>,
    /// The price applied to tokens **written** to cache, and the long-lived
    /// one. They sit on the row like the others: a cost must be reproducible by
    /// hand from the row, without knowing which price list was in force.
    #[serde(default)]
    pub cache_write_price_micros_per_million: Option<i64>,
    #[serde(default)]
    pub cache_write_long_price_micros_per_million: Option<i64>,
    /// **WHAT IDENTITY THIS CALL'S PROCESS STARTED WITH**: which home, and how
    /// it was chosen; the shapes live in [`EngineIdentity`]. Without it two
    /// runs of one flow are not the same measure. It replaces `mandate_name`,
    /// which named the profile in force even when the step had overridden it —
    /// it lied exactly where the identity had been changed on purpose — and
    /// `mandate_version`, empty by construction: a profile has no version.
    #[serde(default)]
    pub engine_identity: EngineIdentity,
    pub retry_chain: Vec<String>,
    pub error_type: Option<String>,
    pub started_at: i64,
    pub ended_at: Option<i64>,
    /// **THE SESSION THIS CALL RAN UNDER, WHEN IT IS KNOWN.** What lets a later
    /// step **resume** instead of rediscover; on disk, not in memory, because
    /// the "wait for an exhausted engine" branch `docs/piano-consumo-e-profili.md`
    /// leaves uncovered must resume tomorrow from another process. `None` when
    /// the engine opens no sessions, the step asked for none, or it **branched**:
    /// there the parent's id would resume the trunk, silently, as if the branch.
    #[serde(default)]
    pub session_id: Option<String>,
    /// The kind of work the step declared (`mechanical`, `research`, ...),
    /// so a sum per kind can say who did what at what cost. `None` when the
    /// step declared none.
    #[serde(default)]
    pub work_kind: Option<String>,
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

/// A fact a flow wants to remember, in a collection it named itself.
///
/// **THE SPACE BELONGS TO THE FLOW, NOT TO THE ENGINE**: the flow picks the
/// name, the key inside it and an arbitrary JSON value. The engine must not
/// know that a thing called a "mandate" exists — a domain concept carved into
/// Rust is what turned a four-step flow into a 2,562-line program.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StoreRecord {
    /// The namespace, chosen by whoever writes the flow.
    ///
    /// **Not a real SQL table made from the flow file**: arbitrary DDL from a
    /// data file inside the brakes, the door that *no interpreter inside
    /// Sailor* shuts. A collection buys the same freedom: a reader sees its own
    /// entries and no others, and no file changes the shape of the store.
    pub collection: String,
    /// The entry inside that collection.
    pub key: String,
    /// What it holds, in the shape the flow decided.
    ///
    /// The first draft of this was a `current_mandate` table with columns of
    /// its own, and it was stopped with *"it should exist drawn, not
    /// hardcoded"*: here the engine offers **the space**, and whoever fills it
    /// decides what it means.
    pub value: Value,
    /// Who wrote it: the flow, the run, or a person.
    pub written_by: String,
    pub written_at: i64,
}

/// A process Sailor started.
///
/// **IN THE STORE AND NOT IN MEMORY.** An orphan process held a port and
/// blocked the start *twice, for two different people, in one night*, and the
/// second knew nothing of the first: a register living inside the window
/// answers only to whoever has that window open — not to the day after.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProcessRecord {
    /// The name whoever started it finds it back by. Not the pid: pids get
    /// reused, and whoever resumes after a reboot must be able to name the
    /// thing they are looking for before knowing what number it has today.
    pub process_id: String,
    pub pid: u32,
    /// The **whole** command line, kept because deciding whether to kill an
    /// orphan needs to know *what it is*, and a pid alone will not say. Without
    /// it you go and ask the operating system what that number is — the road
    /// where the empty answer without an error is waiting.
    pub command: String,
    pub args: Vec<String>,
    pub working_directory: String,
    /// The port it holds, if it holds one. This is the key the orphan-process
    /// fault showed up as: the question was not "which processes exist", it was
    /// "who is holding 5183".
    pub port: Option<u16>,
    /// What it is for: `live` for live mode, the action's name for a flow
    /// process. Whoever finds an orphan must tell whether it is still needed.
    pub purpose: String,
    /// Who turned it on. Without this field an orphan has no owner, which is
    /// exactly how it was found.
    pub started_by: String,
    /// The run it belongs to, if it belongs to one.
    pub run_id: Option<String>,
    pub started_at: i64,
}

/// The close of a registered process. Separate from the start because it
/// arrives later and from another point in the code: merging them would mean
/// rewriting the start, and the event log is not rewritten.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProcessEndRecord {
    pub process_id: String,
    pub exit_code: Option<i32>,
    pub ended_at: i64,
}

/// Does that pid still exist?
///
/// **NOT `pgrep`**, which inside some sandboxes sees no processes and answers
/// with an empty list *without an error*, so "nobody there" and "not allowed
/// to look" arrive identical. This asks about **one** pid the store wrote, and
/// `EPERM` — *alive, but another user's* — is a yes; a no would redo the fault.
pub fn pid_is_alive(pid: u32) -> bool {
    // **LIMIT ONE, AND THERE IS NO BETTER CHECK.** Numbers get reused, so a
    // live pid does not prove it is the *same* process the store wrote:
    // settling that wants a start time, which macOS keeps behind `libproc`.
    // This confirms, it does not decide.

    // **A NUMBER THAT IS NOT A PID IS NOT ASKED ABOUT.** Read signed, 0 is the
    // caller's own group and anything below it is a group or everybody: a
    // stored number past a positive `i32` would answer «alive» about whoever
    // happened to be running.
    if pid == 0 || pid > i32::MAX as u32 {
        return false;
    }
    // **LIMIT TWO, AND IT WAS MEASURED: an unreaped child reads as alive.** A
    // first draft of `the_dead_are_closed_and_the_living_are_left_alone` killed
    // a child without waiting and got "alive" here. That is correct — a zombie
    // *is* a row in the process table — and it reaches only the caller's own
    // children: a real orphan's parent is the init process, which reaps it as
    // soon as it dies. For your own child use `Process::exited`, which waits.

    // SAFETY: `kill` with signal 0 delivers nothing and touches no memory of
    // ours; it reads an int and returns an int.
    let outcome = unsafe { libc::kill(pid as libc::pid_t, 0) };
    if outcome == 0 {
        return true;
    }
    std::io::Error::last_os_error().raw_os_error() == Some(libc::EPERM)
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

/// What a record answering a repeated call would have saved here. A saving
/// counted on a key that was a pointer, or on a call no engine ever priced, is
/// one nobody can bank: those stay apart so the headline cannot be read alone.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RepeatedCalls {
    pub calls: i64,
    pub served: i64,
    pub served_on_an_unresolved_prompt: i64,
    pub served_micros: i64,
    pub served_without_a_cost: i64,
    /// Calls whose step this store no longer holds: no key, so never served.
    pub calls_without_a_key: i64,
    pub spent_micros: i64,
}

/// True when the recorded input still names a value instead of carrying it, so
/// two of them matching says nothing about the two prompts.
fn prompt_is_a_pointer(input: &str) -> bool {
    serde_json::from_str::<Value>(input).is_ok_and(|value| names_a_value(&value))
}

fn names_a_value(value: &Value) -> bool {
    match value {
        Value::Object(fields) => fields.iter().any(|(key, inner)| {
            key == flow::reference::FROM_KEY
                || key == flow::reference::JOIN_KEY
                || key == flow::reference::JSON_KEY
                || names_a_value(inner)
        }),
        Value::Array(items) => items.iter().any(names_a_value),
        _ => false,
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GatesChangedStep {
    pub run_id: String,
    pub step_id: String,
    pub attempt: u32,
    pub epoch: u64,
}

/// A run with at least one open step, as whoever resumes sees it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UnfinishedRun {
    pub run_id: String,
    /// What the run was working on. Empty if nobody ever recorded it.
    pub entity: String,
    pub open_steps: usize,
    pub oldest_started_at: i64,
}

/// A run stopped because it is waiting for somebody.
///
/// **IT CARRIES NO `open_steps`, AND THE ABSENCE IS A STATEMENT.** A waiting
/// run has no open steps: the handed-over one is closed with outcome `Waiting`.
/// A field always reading zero would tell the reader the run had been abandoned
/// halfway, which is the wrong story.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WaitingRun {
    pub run_id: String,
    /// Which flow. Empty if nobody ever recorded it.
    pub entity: String,
    /// Since when it has been waiting: the instant the run stopped, or the
    /// instant it started if it has not stopped yet.
    pub waiting_since: i64,
}

/// One table of the projection, and how many rows it holds right now.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct TableCount {
    pub name: String,
    pub rows: i64,
}

/// What a browsed statement answered: the columns, the rows as JSON values,
/// and whether the limit cut the answer short.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Answer {
    pub columns: Vec<String>,
    pub rows: Vec<Vec<serde_json::Value>>,
    pub truncated: bool,
}

fn browse_with(connection: &Connection, sql: &str, limit: usize) -> Result<Answer, LedgerError> {
    let mut statement = connection.prepare(sql)?;
    let columns: Vec<String> = statement
        .column_names()
        .into_iter()
        .map(str::to_owned)
        .collect();
    let width = columns.len();
    let mut rows = statement.query([])?;
    let mut answer = Vec::new();
    let mut truncated = false;
    while let Some(row) = rows.next()? {
        if answer.len() >= limit {
            truncated = true;
            break;
        }
        let mut cells = Vec::with_capacity(width);
        for index in 0..width {
            let value = match row.get_ref(index)? {
                rusqlite::types::ValueRef::Null => serde_json::Value::Null,
                rusqlite::types::ValueRef::Integer(number) => serde_json::Value::from(number),
                rusqlite::types::ValueRef::Real(number) => serde_json::Value::from(number),
                rusqlite::types::ValueRef::Text(text) => {
                    serde_json::Value::from(String::from_utf8_lossy(text).into_owned())
                }
                rusqlite::types::ValueRef::Blob(bytes) => {
                    serde_json::Value::from(format!("{} bytes", bytes.len()))
                }
            };
            cells.push(value);
        }
        answer.push(cells);
    }
    Ok(Answer {
        columns,
        rows: answer,
        truncated,
    })
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

// ── how it went: the answers a flow can get about its own history ──
//
// **NONE OF THESE STRUCTS CARRIES `input` OR `output`, AND IT IS NOT AN
// OVERSIGHT.** History leaves here for an action any flow can name, and those
// two are the typed data channel: prompts, environments, model replies. Held
// out of the *types*, no field is left for a later slip in `actions` to keep.

/// How many times a step broke, and with what failure class.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct StepFailureTally {
    /// The denominator: three failures out of three attempts and three out of
    /// two hundred are the same figure and not the same thing.
    pub attempts: i64,
    pub failures: i64,
    /// The runs touched, which are not the failures: a step can break several
    /// times in the same run, one attempt at a time.
    pub runs_affected: i64,
    pub by_class: Vec<FailureClassCount>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FailureClassCount {
    /// `None` is a broken step the engine could not classify, and differs from
    /// a class literally named "unknown": here the datum is missing.
    pub failure_class: Option<String>,
    pub failures: i64,
    pub runs_affected: i64,
}

/// A **closed** run, step by step.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FinishedRun {
    pub run_id: String,
    pub entity: String,
    pub status: String,
    pub started_at: i64,
    pub ended_at: i64,
    pub steps: Vec<StepOutcome>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StepOutcome {
    pub step_id: String,
    pub attempt: u32,
    /// `None` is a step left open inside an already-closed run.
    pub outcome: Option<String>,
    pub failure_class: Option<String>,
    pub started_at: i64,
    pub ended_at: Option<i64>,
    pub bytes_seen: Option<i64>,
    pub bytes_discarded: Option<i64>,
}

/// How long a step takes, measured on successful attempts only.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct StepDurations {
    /// Whole seconds, already sorted: whoever summarises need not re-sort, and
    /// whoever reads the median need not trust that somebody did.
    pub seconds_sorted: Vec<i64>,
    pub last_seconds: Option<i64>,
    /// Broken attempts, counted but **not** measured: a fast failure would pull
    /// the median down and make a slow step look quick.
    pub failed_samples: i64,
}

/// The raw text of a broken step, as handed to whoever diagnoses it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SaidExcerpt {
    pub step_id: String,
    pub attempt: u32,
    pub said: String,
    pub truncated: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", content = "record", rename_all = "snake_case")]
enum StoredEvent {
    RunRecorded(RunRecord),
    StepStarted(StepRecord),
    StepClosed(StepRecord),
    ModelCallRecorded(ModelCallRecord),
    SnapshotRecorded(SnapshotRecord),
    InventoryScanned(InventoryScan),
    RecordWritten(StoreRecord),
    ProcessStarted(ProcessRecord),
    ProcessEnded(ProcessEndRecord),
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
            // Creating the tables, adjusting the columns and building the
            // indexes are three distinct phases: a new store creates everything
            // at once, an old store must add the missing columns before the
            // indexes can attach, and an up-to-date store touches nothing.
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

    /// Stores an inventory scan.
    pub fn record_inventory(&self, scan: &InventoryScan) -> Result<(), LedgerError> {
        self.write_event(StoredEvent::InventoryScanned(scan.clone()))
    }

    /// Records that Sailor started a process.
    ///
    /// Call it **after** the process exists, with the real pid in hand:
    /// recording an intent to start something that then does not start lists an
    /// orphan that is not there, and its reader goes hunting a pid that never
    /// existed. The failed start costs nothing to lose: nothing to shut down.
    pub fn record_process_started(&self, record: &ProcessRecord) -> Result<(), LedgerError> {
        if record.process_id.trim().is_empty() {
            return Err(LedgerError::InvalidRecord("process id is empty".into()));
        }
        self.write_event(StoredEvent::ProcessStarted(record.clone()))
    }

    /// Records that a registered process has ended.
    pub fn record_process_ended(&self, record: &ProcessEndRecord) -> Result<(), LedgerError> {
        if record.process_id.trim().is_empty() {
            return Err(LedgerError::InvalidRecord("process id is empty".into()));
        }
        self.write_event(StoredEvent::ProcessEnded(record.clone()))
    }

    /// The processes started for which no close ever arrived.
    ///
    /// The answer comes from the data, not the operating system. A process
    /// killed from outside writes no close and stays here, so `pid_is_alive`
    /// **confirms** one row at a time instead of replacing this list: "what did
    /// I start" and "what is breathing" are two questions, kept apart on purpose.
    pub fn processes_left_running(&self) -> Result<Vec<ProcessRecord>, LedgerError> {
        let connection = self.lock()?;
        let mut statement = connection.prepare(&format!(
            "SELECT {PROCESS_COLUMNS} FROM processes
             WHERE ended_at IS NULL ORDER BY started_at DESC"
        ))?;
        let rows = statement
            .query_map([], read_process_row)?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    /// Who holds a port, as far as the store knows.
    ///
    /// Most recent wins: if two starts claimed the same port and only one got
    /// it, it is the last — the first was already dead when the second started,
    /// or the second would not have started.
    pub fn process_holding_port(&self, port: u16) -> Result<Option<ProcessRecord>, LedgerError> {
        let connection = self.lock()?;
        let mut statement = connection.prepare(&format!(
            "SELECT {PROCESS_COLUMNS} FROM processes
             WHERE port = ?1 AND ended_at IS NULL
             ORDER BY started_at DESC LIMIT 1"
        ))?;
        let row = statement.query_row([port], read_process_row).optional()?;
        Ok(row)
    }

    /// Writes an entry into the collection the flow named.
    ///
    /// Collection and key cannot be empty: they are the address, and an entry
    /// without an address is found only by whoever already knows where it is.
    pub fn put_record(&self, record: &StoreRecord) -> Result<(), LedgerError> {
        if record.collection.trim().is_empty() {
            return Err(LedgerError::InvalidRecord(
                "record collection is empty".into(),
            ));
        }
        if record.key.trim().is_empty() {
            return Err(LedgerError::InvalidRecord("record key is empty".into()));
        }
        self.write_event(StoredEvent::RecordWritten(record.clone()))
    }

    /// What an entry holds, if anybody wrote it.
    ///
    /// `None` is not a fault: it is an entry nobody has written yet, and the
    /// reader must have a fallback rather than stop. A store that invented a
    /// plausible value would be worse than not knowing.
    pub fn read_record(
        &self,
        collection: &str,
        key: &str,
    ) -> Result<Option<StoreRecord>, LedgerError> {
        let connection = self.lock()?;
        let found = connection
            .query_row(
                "SELECT collection, key, value, written_by, written_at
                 FROM store WHERE collection = ?1 AND key = ?2",
                params![collection, key],
                read_store_row,
            )
            .optional()?;
        Ok(found)
    }

    /// Every entry in a collection, for whoever wants to show it whole.
    pub fn records_in(&self, collection: &str) -> Result<Vec<StoreRecord>, LedgerError> {
        let connection = self.lock()?;
        let mut statement = connection.prepare(
            "SELECT collection, key, value, written_by, written_at
             FROM store WHERE collection = ?1 ORDER BY key",
        )?;
        let rows = statement.query_map(params![collection], read_store_row)?;
        Ok(rows.collect::<Result<Vec<_>, _>>()?)
    }

    /// The entries that vanished: they were there, and the last scan no longer
    /// saw them.
    ///
    /// A list recomputed every time cannot ask this. It matters before deleting
    /// anything: an entry gone since yesterday is a change to understand, one
    /// gone for a month is already-dead rubbish.
    pub fn inventory_gone(&self) -> Result<Vec<InventoryChange>, LedgerError> {
        self.inventory_where("gone_at IS NOT NULL", "gone_at DESC, kind, name")
    }

    /// Entries that appeared after a given instant — "what changed since
    /// yesterday".
    pub fn inventory_new_since(&self, since: i64) -> Result<Vec<InventoryChange>, LedgerError> {
        let connection = self.lock()?;
        let mut statement = connection.prepare(
            "SELECT kind, name, path, origin, reach, reason, first_seen, last_seen, gone_at
             FROM inventory_items WHERE first_seen >= ?1 AND gone_at IS NULL
             ORDER BY first_seen DESC, kind, name",
        )?;
        let rows = statement.query_map(params![since], read_inventory_row)?;
        Ok(rows.collect::<Result<Vec<_>, _>>()?)
    }

    /// Everything present now, as the last scan saw it.
    pub fn inventory_present(&self) -> Result<Vec<InventoryChange>, LedgerError> {
        self.inventory_where("gone_at IS NULL", "kind, name")
    }

    fn inventory_where(
        &self,
        condition: &str,
        order: &str,
    ) -> Result<Vec<InventoryChange>, LedgerError> {
        let connection = self.lock()?;
        let mut statement = connection.prepare(&format!(
            "SELECT kind, name, path, origin, reach, reason, first_seen, last_seen, gone_at
             FROM inventory_items WHERE {condition} ORDER BY {order}"
        ))?;
        let rows = statement.query_map([], read_inventory_row)?;
        Ok(rows.collect::<Result<Vec<_>, _>>()?)
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
        record.refusal = completion.refusal;
        record.ran = completion.ran;
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
                    held_by_pid, species, refusal, ran
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

    /// Runs left halfway: they have a recorded intent and no outcome.
    ///
    /// THE RESUME QUESTION, and it names no job: whoever resumes does not know
    /// which runs exist, only that they want to close whatever is still open.
    /// Rebuilding the run name from outside — from today's date, say — never
    /// finds a run interrupted before midnight. Longest stuck resumes first.
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

    /// A run's header, if one exists under that name.
    ///
    /// **IT IS HOW THE FLOW A RUN CAME FROM IS FOUND AGAIN.** Whoever resumes
    /// holds the run id and nothing else: without `entity`, the wrong graph
    /// loads and one step's output is validated against another's schema.
    /// `None` is a run nobody recorded, and must read apart from a broken store.
    pub fn run_header(&self, run_id: &str) -> Result<Option<RunRecord>, LedgerError> {
        let connection = self.lock()?;
        let found = connection
            .query_row(
                "SELECT run_id, kind, entity, parent_run_id, started_by, status,
                        total_cost_micros, error, started_at, ended_at, worktree,
                        stop_reason
                 FROM runs WHERE run_id = ?1",
                params![run_id],
                |row| {
                    Ok(RunRecord {
                        run_id: row.get(0)?,
                        kind: row.get(1)?,
                        entity: row.get(2)?,
                        parent_run_id: row.get(3)?,
                        started_by: row.get(4)?,
                        status: row.get(5)?,
                        total_cost_micros: row.get(6)?,
                        error: row.get(7)?,
                        started_at: row.get(8)?,
                        ended_at: row.get(9)?,
                        worktree: row.get(10)?,
                        stop_reason: row.get(11)?,
                    })
                },
            )
            .optional()?;
        Ok(found)
    }

    /// Runs stopped waiting for a person or an agent.
    ///
    /// **NOT `unfinished_runs` WITH ANOTHER FILTER, AND THAT IS THE POINT.**
    /// That question looks for **open** steps — an intent with no outcome. A
    /// handed-over step is **closed**, with outcome `Waiting`, because whoever
    /// must run it is not a process whose death we await. Longest wait first.
    pub fn waiting_runs(&self) -> Result<Vec<WaitingRun>, LedgerError> {
        let connection = self.lock()?;
        // Before this query no interrogation found those runs at all: a
        // hand-over nobody picked up simply vanished, and the only way back to
        // it was to remember it. No migration was owed to add it either —
        // `runs.status` is free text, and `waiting` is already written there
        // by `execution_status`.
        let mut statement = connection.prepare(
            "SELECT run_id, entity, COALESCE(ended_at, started_at)
             FROM runs WHERE status = 'waiting'
             ORDER BY COALESCE(ended_at, started_at), run_id",
        )?;
        let rows = statement.query_map([], |row| {
            Ok(WaitingRun {
                run_id: row.get(0)?,
                entity: row.get(1)?,
                waiting_since: row.get(2)?,
            })
        })?;
        Ok(rows.collect::<Result<Vec<_>, _>>()?)
    }

    /// Runs that stopped on a step saying "not yet", to be run again.
    ///
    /// **A SEPARATE QUESTION FROM `waiting_runs`, AND IT HAS TO BE.** That one
    /// finds work somebody must come and take; merging them would send a person
    /// to take a step nobody handed them. And without it these runs are found
    /// by neither — no open step, no `waiting` status — so they vanish twice.
    pub fn runs_to_ask_again(&self) -> Result<Vec<WaitingRun>, LedgerError> {
        let connection = self.lock()?;
        let mut statement = connection.prepare(
            "SELECT run_id, entity, COALESCE(ended_at, started_at)
             FROM runs WHERE status = 'not_yet'
             ORDER BY COALESCE(ended_at, started_at), run_id",
        )?;
        let rows = statement.query_map([], |row| {
            Ok(WaitingRun {
                run_id: row.get(0)?,
                entity: row.get(1)?,
                waiting_since: row.get(2)?,
            })
        })?;
        Ok(rows.collect::<Result<Vec<_>, _>>()?)
    }

    /// When each entity last began.
    ///
    /// Used to tell whether a scheduled flow is due now, and the question is
    /// **when it started**, not when it finished: a run still in flight has
    /// already used its turn, and counting it as "never ran" would start it
    /// again on top of itself.
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

    /// Every table of the projection with how many rows it holds, for whoever
    /// wants to look at the store as it is rather than through a question
    /// somebody else wrote.
    pub fn tables(&self) -> Result<Vec<TableCount>, LedgerError> {
        let connection = self.lock()?;
        let mut statement = connection.prepare(
            "SELECT name FROM sqlite_master WHERE type = 'table' AND name NOT LIKE 'sqlite_%' \
             ORDER BY name",
        )?;
        let names: Vec<String> = statement
            .query_map([], |row| row.get(0))?
            .collect::<Result<_, _>>()?;
        names
            .into_iter()
            .map(|name| {
                let rows: i64 =
                    connection.query_row(&format!("SELECT COUNT(*) FROM \"{name}\""), [], |row| {
                        row.get(0)
                    })?;
                Ok(TableCount { name, rows })
            })
            .collect()
    }

    /// Answers one `SELECT` a person typed, read-only, at most `limit` rows.
    ///
    /// **THE STORE IS APPEND-ONLY AND A BROWSER MUST NOT BE A BACK DOOR**: the
    /// statement is run with `query_only` set, so anything that would write is
    /// refused by SQLite itself, whatever the text looked like.
    pub fn browse(&self, sql: &str, limit: usize) -> Result<Answer, LedgerError> {
        let connection = self.lock()?;
        connection.pragma_update(None, "query_only", true)?;
        let outcome = browse_with(&connection, sql, limit);
        connection.pragma_update(None, "query_only", false)?;
        outcome
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

    /// How many runs the store knows about, however it knows them.
    ///
    /// **IT SEPARATES "THERE IS NOTHING" FROM "ZERO FAILURES"**: on a fresh
    /// machine a zero without this beside it reads as months of unbroken
    /// running. The union of both tables — a run with steps and no header
    /// happened, and "none" to somebody holding the history is the same lie.
    pub fn recorded_runs(&self) -> Result<i64, LedgerError> {
        let connection = self.lock()?;
        Ok(connection.query_row(
            "SELECT COUNT(*) FROM (SELECT run_id FROM runs UNION SELECT run_id FROM steps)",
            [],
            |row| row.get(0),
        )?)
    }

    /// What a run has spent so far, and on how many calls that is unknown.
    ///
    /// **THE SECOND NUMBER MAKES THE FIRST READABLE.** codex declares a total
    /// and not the two sides, and no cost comes from a total without inventing
    /// the proportion, so its rows keep a `NULL` cost: summing the known ones
    /// **underestimates**, and a cap trusting it lets a doubled run through.
    pub fn spent_in_run(&self, run_id: &str) -> Result<Spend, LedgerError> {
        let connection = self.lock()?;
        // A run with no calls at all answers every field zero, and that is the
        // right answer: it has spent nothing **and** there is nothing unknown.
        //
        // `MAX` over a column where every row is `NULL` answers `NULL`, and
        // `Option<i64>` carries that through to whoever decides: "the dearest
        // is unknown" is not "the dearest is zero".
        let (micros, calls, calls_without_cost, dearest_micros) = connection.query_row(
            "SELECT COALESCE(SUM(cost_micros), 0),
                    COUNT(*),
                    COALESCE(SUM(CASE WHEN cost_micros IS NULL THEN 1 ELSE 0 END), 0),
                    MAX(cost_micros)
             FROM model_calls WHERE run_id = ?1",
            params![run_id],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
        )?;
        Ok(Spend {
            micros,
            calls,
            calls_without_cost,
            dearest_micros,
        })
    }

    /// How many calls a record would have answered instead of making, counted
    /// over the calls this store already holds.
    ///
    /// The key is the step's input fingerprint plus the model that answered:
    /// it leaves out the tree's commit and the descriptor's version, which no
    /// row carries and which can only split a key, so `served` is a ceiling.
    pub fn repeated_engine_calls(&self) -> Result<RepeatedCalls, LedgerError> {
        let connection = self.lock()?;
        // A redone step has several attempts: the one that counts is the one
        // open when the call started, so an attempt after it never wins.
        let mut statement = connection.prepare(
            "WITH ranked AS (
                 SELECT m.call_id, m.started_at, m.cost_micros, m.actual_model,
                        m.error_type, s.input_digest, s.input, s.outcome,
                        ROW_NUMBER() OVER (
                            PARTITION BY m.call_id
                            ORDER BY (s.started_at > m.started_at), s.started_at DESC
                        ) AS choice
                 FROM model_calls m
                 LEFT JOIN steps s
                   ON s.run_id = m.run_id AND s.step_id = m.step_id
             )
             SELECT cost_micros, actual_model, error_type, input_digest, input, outcome
             FROM ranked WHERE choice = 1
             ORDER BY started_at, call_id",
        )?;
        let rows = statement.query_map([], |row| {
            Ok((
                row.get::<_, Option<i64>>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, Option<String>>(2)?,
                row.get::<_, Option<String>>(3)?,
                row.get::<_, Option<String>>(4)?,
                row.get::<_, Option<String>>(5)?,
            ))
        })?;
        let mut tally = RepeatedCalls::default();
        let mut answered = std::collections::HashSet::new();
        for row in rows {
            let (cost, model, error_type, digest, input, outcome) = row?;
            tally.calls += 1;
            tally.spent_micros += cost.unwrap_or(0);
            let Some(digest) = digest else {
                tally.calls_without_a_key += 1;
                continue;
            };
            let key = (digest, model);
            if answered.contains(&key) {
                tally.served += 1;
                match cost {
                    Some(micros) => tally.served_micros += micros,
                    None => tally.served_without_a_cost += 1,
                }
                if input.as_deref().is_some_and(prompt_is_a_pointer) {
                    tally.served_on_an_unresolved_prompt += 1;
                }
            }
            // Only a call that succeeded ever answers for another: a refusal,
            // an exhausted quota or an outcome nobody closed is not an answer.
            if error_type.is_none() && outcome.as_deref() == Some("Went") {
                answered.insert(key);
            }
        }
        Ok(tally)
    }

    /// What one engine has spent since an instant, across every run: the sum
    /// a budget on a window compares itself to, with the same unknowns as
    /// [`Self::spent_in_run`].
    pub fn spent_by_cli_since(&self, cli: &str, since: i64) -> Result<Spend, LedgerError> {
        let connection = self.lock()?;
        let (micros, calls, calls_without_cost, dearest_micros) = connection.query_row(
            "SELECT COALESCE(SUM(cost_micros), 0),
                    COUNT(*),
                    COALESCE(SUM(CASE WHEN cost_micros IS NULL THEN 1 ELSE 0 END), 0),
                    MAX(cost_micros)
             FROM model_calls WHERE cli = ?1 AND started_at >= ?2",
            params![cli, since],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
        )?;
        Ok(Spend {
            micros,
            calls,
            calls_without_cost,
            dearest_micros,
        })
    }

    /// The session a step of this run opened **on that engine**.
    ///
    /// **THE ENGINE IS PART OF THE QUESTION, AND DROPPING IT FAILS SILENTLY**: a
    /// step chained onto `codex` because `claude-code` ran out of quota, handed
    /// a `claude-code` session to resume, passes an id `codex` does not know and
    /// dies **after** starting — after spending. Latest by start: a redo opens two.
    pub fn session_opened_by(
        &self,
        run_id: &str,
        step_id: &str,
        cli: &str,
    ) -> Result<Option<String>, LedgerError> {
        // The caller names **the step**, not the session id, and that is the
        // whole reason for this signature: the id is minted at run time, so
        // whoever writes the flow cannot possibly know it.
        let connection = self.lock()?;
        let mut statement = connection.prepare(
            "SELECT session_id FROM model_calls
             WHERE run_id = ?1 AND step_id = ?2 AND cli = ?3 AND session_id IS NOT NULL
             ORDER BY started_at DESC LIMIT 1",
        )?;
        let mut rows = statement.query(params![run_id, step_id, cli])?;
        match rows.next()? {
            Some(row) => Ok(Some(row.get(0)?)),
            None => Ok(None),
        }
    }

    /// How many runs actually fall inside the window asked for.
    ///
    /// Whoever gets a count must know what it was computed over: a window of
    /// fifty runs over a store holding three is not a window of fifty, and
    /// without this number "zero failures in the last fifty" would sound like a
    /// reassurance nobody measured.
    pub fn runs_in_window(&self, flow: Option<&str>, limit: usize) -> Result<i64, LedgerError> {
        let connection = self.lock()?;
        Ok(connection.query_row(
            "SELECT COUNT(*) FROM (SELECT run_id FROM runs
             WHERE (?1 IS NULL OR entity = ?1)
             ORDER BY started_at DESC LIMIT ?2)",
            params![flow, limit as i64],
            |row| row.get(0),
        )?)
    }

    /// How many times a step broke inside the window, and how.
    ///
    /// **THE PER-FLOW FILTER GOES THROUGH THE JOIN WITH `runs`**: `steps` does
    /// not know its flow, only the run header does. The price is declared —
    /// steps of runs never recorded in `runs` stay outside the window — and the
    /// opposite price is worse: answering across all flows to one named flow.
    pub fn step_failure_tally(
        &self,
        step_id: &str,
        flow: Option<&str>,
        within_last_runs: usize,
    ) -> Result<StepFailureTally, LedgerError> {
        let connection = self.lock()?;
        let limit = within_last_runs as i64;
        let (attempts, failures, runs_affected) = connection.query_row(
            "WITH recent AS (
                 SELECT run_id FROM runs
                 WHERE (?1 IS NULL OR entity = ?1)
                 ORDER BY started_at DESC LIMIT ?2
             )
             SELECT COUNT(*),
                    COALESCE(SUM(CASE WHEN s.outcome = 'Broke' THEN 1 ELSE 0 END), 0),
                    COUNT(DISTINCT CASE WHEN s.outcome = 'Broke' THEN s.run_id END)
             FROM steps s JOIN recent r ON r.run_id = s.run_id
             WHERE s.step_id = ?3",
            params![flow, limit, step_id],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )?;
        let mut statement = connection.prepare(
            "WITH recent AS (
                 SELECT run_id FROM runs
                 WHERE (?1 IS NULL OR entity = ?1)
                 ORDER BY started_at DESC LIMIT ?2
             )
             SELECT s.failure_class, COUNT(*), COUNT(DISTINCT s.run_id)
             FROM steps s JOIN recent r ON r.run_id = s.run_id
             WHERE s.step_id = ?3 AND s.outcome = 'Broke'
             GROUP BY s.failure_class
             ORDER BY COUNT(*) DESC, s.failure_class",
        )?;
        let by_class = statement
            .query_map(params![flow, limit, step_id], read_failure_class_row)?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(StepFailureTally {
            attempts,
            failures,
            runs_affected,
            by_class,
        })
    }

    /// The most frequent failure classes, most frequent first.
    pub fn failure_class_tally(
        &self,
        flow: Option<&str>,
        within_last_runs: usize,
    ) -> Result<Vec<FailureClassCount>, LedgerError> {
        let connection = self.lock()?;
        let mut statement = connection.prepare(
            "WITH recent AS (
                 SELECT run_id FROM runs
                 WHERE (?1 IS NULL OR entity = ?1)
                 ORDER BY started_at DESC LIMIT ?2
             )
             SELECT s.failure_class, COUNT(*), COUNT(DISTINCT s.run_id)
             FROM steps s JOIN recent r ON r.run_id = s.run_id
             WHERE s.outcome = 'Broke'
             GROUP BY s.failure_class
             ORDER BY COUNT(*) DESC, s.failure_class",
        )?;
        let rows = statement.query_map(
            params![flow, within_last_runs as i64],
            read_failure_class_row,
        )?;
        Ok(rows.collect::<Result<Vec<_>, _>>()?)
    }

    /// A flow's last **closed** run, step by step.
    ///
    /// **CLOSED, NOT RECENT**, and not out of taste: a flow querying its own
    /// history while running *is* the most recent run, and answering with
    /// itself half-done hands it an outcome that has not happened. `ended_at IS
    /// NOT NULL` excludes the asker without the action knowing its own name.
    pub fn last_finished_run(&self, flow: &str) -> Result<Option<FinishedRun>, LedgerError> {
        let connection = self.lock()?;
        let head: Option<(String, String, String, i64, i64)> = connection
            .query_row(
                "SELECT run_id, entity, status, started_at, ended_at FROM runs
                 WHERE entity = ?1 AND ended_at IS NOT NULL
                 ORDER BY started_at DESC, run_id DESC LIMIT 1",
                params![flow],
                |row| {
                    Ok((
                        row.get(0)?,
                        row.get(1)?,
                        row.get(2)?,
                        row.get(3)?,
                        row.get(4)?,
                    ))
                },
            )
            .optional()?;
        let Some((run_id, entity, status, started_at, ended_at)) = head else {
            return Ok(None);
        };
        let mut statement = connection.prepare(
            "SELECT step_id, attempt, outcome, failure_class, started_at, ended_at,
                    bytes_seen, bytes_discarded
             FROM steps WHERE run_id = ?1
             ORDER BY started_at, step_id, attempt",
        )?;
        let steps = statement
            .query_map(params![run_id], |row| {
                Ok(StepOutcome {
                    step_id: row.get(0)?,
                    attempt: row.get(1)?,
                    outcome: row.get(2)?,
                    failure_class: row.get(3)?,
                    started_at: row.get(4)?,
                    ended_at: row.get(5)?,
                    bytes_seen: row.get(6)?,
                    bytes_discarded: row.get(7)?,
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(Some(FinishedRun {
            run_id,
            entity,
            status,
            started_at,
            ended_at,
            steps,
        }))
    }

    /// How long a step took, successful attempt by successful attempt.
    ///
    /// Broken attempts are counted apart instead of entering the durations: an
    /// immediate failure is fast, and mixing it into the successes would answer
    /// "quicker than usual" about a step that has stopped working.
    pub fn step_durations(
        &self,
        step_id: &str,
        flow: Option<&str>,
        within_last_runs: usize,
    ) -> Result<StepDurations, LedgerError> {
        let connection = self.lock()?;
        let mut statement = connection.prepare(
            "WITH recent AS (
                 SELECT run_id FROM runs
                 WHERE (?1 IS NULL OR entity = ?1)
                 ORDER BY started_at DESC LIMIT ?2
             )
             SELECT s.outcome, s.ended_at - s.started_at, s.ended_at
             FROM steps s JOIN recent r ON r.run_id = s.run_id
             WHERE s.step_id = ?3 AND s.ended_at IS NOT NULL
             ORDER BY s.ended_at DESC",
        )?;
        let rows = statement.query_map(params![flow, within_last_runs as i64, step_id], |row| {
            let outcome: Option<String> = row.get(0)?;
            let seconds: i64 = row.get(1)?;
            Ok((outcome, seconds))
        })?;
        let mut durations = StepDurations::default();
        for row in rows {
            let (outcome, seconds) = row?;
            match outcome.as_deref() {
                Some("Went") => {
                    // Rows arrive newest first: the first success is "last
                    // time", and that is what the asker compares against.
                    if durations.last_seconds.is_none() {
                        durations.last_seconds = Some(seconds);
                    }
                    durations.seconds_sorted.push(seconds);
                }
                Some("Broke") => durations.failed_samples += 1,
                // Skipped, stopped or waiting: neither a success to measure nor
                // a failure to count. Silence is more honest than classifying.
                _ => {}
            }
        }
        durations.seconds_sorted.sort_unstable();
        Ok(durations)
    }

    /// The raw text of the broken steps of **one** named run.
    ///
    /// **IT IS A GATE, AND IT IS WRITTEN AS ONE.** `said` is all that leaves a
    /// flow and could hold anything a model said: one run, a cap on steps, a cap
    /// on bytes, and no run of questions rakes the history piece by piece. A
    /// method taking a **window of runs** would be the leak, dressed as comfort.
    pub fn said_of_failed_steps(
        &self,
        run_id: &str,
        max_steps: usize,
        max_bytes: usize,
    ) -> Result<Vec<SaidExcerpt>, LedgerError> {
        let connection = self.lock()?;
        let mut statement = connection.prepare(
            "SELECT step_id, attempt, said FROM steps
             WHERE run_id = ?1 AND outcome = 'Broke' AND said IS NOT NULL
             ORDER BY ended_at, step_id, attempt
             LIMIT ?2",
        )?;
        let rows = statement.query_map(params![run_id, max_steps as i64], |row| {
            let step_id: String = row.get(0)?;
            let attempt: u32 = row.get(1)?;
            let said: String = row.get(2)?;
            Ok((step_id, attempt, said))
        })?;
        let mut excerpts = Vec::new();
        for row in rows {
            let (step_id, attempt, said) = row?;
            let (said, truncated) = clip_to_bytes(said, max_bytes);
            excerpts.push(SaidExcerpt {
                step_id,
                attempt,
                said,
                truncated,
            });
        }
        Ok(excerpts)
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

/// **TAKES `&self` BECAUSE THE STORE WAS ALREADY READY FOR SEVERAL THREADS.**
/// The connection has always sat behind an `Arc<Mutex<_>>`, `append_step_started`
/// and `close_step` already work on `&self`, and every write is already a
/// `BEGIN IMMEDIATE` transaction with a five-second wait if somebody else holds
/// it. What kept two steps from running together was not the store: it was the
/// trait signature, asking for a mutability nobody used.
impl RecordStore for Ledger {
    fn append_started(&self, record: StepRecord) -> Result<(), flow::FlowError> {
        self.append_step_started(&record)
            .map_err(|error| flow::FlowError::Store(error.to_string()))
    }

    fn close(
        &self,
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

    /// The store keeps the calls, so it answers for real.
    fn spent(&self, run_id: &str) -> Result<Spend, flow::FlowError> {
        self.spent_in_run(run_id)
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
        // `Layer` cannot return the error to the caller; a failure of this
        // bridge stays off the critical path and must not stop the traced work.
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
    std::fs::write(marker, b"ready").expect("write the test marker");
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
        // The event is durable, the watermark is not: opening must fold it in
        // before the step can look re-runnable.
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
         -- \"What happened between two and three\" was the only expected
         -- question no index served: it read the whole log. Measured on a
         -- synthetic million-event log by switching this index on and off:
         -- 81.93 ms of scan against 0.05 ms — **1,640 times** — for 2.8% more
         -- space. At today's 112 entries it does not show; it will, and adding
         -- it then means adding it once somebody has asked why the dashboard
         -- takes so long.
         CREATE INDEX IF NOT EXISTS events.events_time_idx
             ON events(occurred_at);
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
             ended_at INTEGER,
             worktree TEXT,
             stop_reason TEXT
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
             refusal TEXT,
             ran TEXT,
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
             input_tokens TEXT,
             output_tokens TEXT,
             cached_tokens TEXT,
             cost_micros INTEGER,
             price_currency TEXT,
             input_price_micros_per_million INTEGER,
             output_price_micros_per_million INTEGER,
             cached_price_micros_per_million INTEGER,
             engine_identity TEXT NOT NULL,
             retry_chain TEXT NOT NULL,
             error_type TEXT,
             started_at INTEGER NOT NULL,
             ended_at INTEGER,
             total_tokens TEXT,
             declared_cost_micros INTEGER,
             cache_write_tokens TEXT,
             cache_write_long_tokens TEXT,
             cache_write_price_micros_per_million INTEGER,
             cache_write_long_price_micros_per_million INTEGER,
             turns TEXT,
             session_id TEXT,
             work_kind TEXT
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
         CREATE TABLE IF NOT EXISTS inventory_items (
             kind TEXT NOT NULL,
             name TEXT NOT NULL,
             path TEXT NOT NULL,
             origin TEXT NOT NULL,
             reach TEXT NOT NULL,
             reason TEXT,
             first_seen INTEGER NOT NULL,
             last_seen INTEGER NOT NULL,
             gone_at INTEGER,
             PRIMARY KEY (kind, name, path)
         );
         CREATE TABLE IF NOT EXISTS store (
             collection TEXT NOT NULL,
             key TEXT NOT NULL,
             value TEXT NOT NULL,
             written_by TEXT NOT NULL,
             written_at INTEGER NOT NULL,
             PRIMARY KEY (collection, key)
         );
         -- The processes Sailor started. Born complete, so an existing store
         -- gets it from this `IF NOT EXISTS` and not from
         -- `add_missing_projection_columns`: there are no columns to add to a
         -- table that did not exist before, and for the same reason
         -- `PROJECTION_SCHEMA_VERSION` does not move. The opposite case — new
         -- columns with the version standing still — is the one that broke
         -- every read, and it is written above that constant.
         CREATE TABLE IF NOT EXISTS processes (
             process_id TEXT PRIMARY KEY,
             pid INTEGER NOT NULL,
             command TEXT NOT NULL,
             args TEXT NOT NULL,
             working_directory TEXT NOT NULL,
             port INTEGER,
             purpose TEXT NOT NULL,
             started_by TEXT NOT NULL,
             run_id TEXT,
             started_at INTEGER NOT NULL,
             ended_at INTEGER,
             exit_code INTEGER
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
             ON snapshots(run_id, IFNULL(step_id, ''), phase);
         -- \"Who holds this port\" is the question the orphan-process fault
         -- showed up as, so it gets its own index instead of a scan.
         CREATE INDEX IF NOT EXISTS processes_port_idx
             ON processes(port, ended_at);
         CREATE INDEX IF NOT EXISTS processes_open_idx
             ON processes(ended_at, started_at DESC);",
    )?;
    Ok(())
}

/// Brings an existing store's projection up to the current version by adding
/// the optional columns born after it. Not a chain of numbered migrations: each
/// column is added if missing, so the same function carries a store of any past
/// version home, and re-running it does nothing. None of this invalidates the
/// existing projections or forces a re-read of the event log — the values of
/// already-written records stay null, which is exactly what they were.
fn add_missing_projection_columns(transaction: &Transaction<'_>) -> Result<(), LedgerError> {
    for (column, kind) in [
        // version 2
        ("bytes_seen", "INTEGER"),
        ("bytes_discarded", "INTEGER"),
        // version 3: who held the step, and whether redoing it is safe
        ("held_by_pid", "INTEGER"),
        ("species", "TEXT"),
    ] {
        if !column_exists(transaction, "steps", column)? {
            transaction.execute(&format!("ALTER TABLE steps ADD COLUMN {column} {kind}"), [])?;
        }
    }
    // version 4: a call's counts and prices may be unknown.
    relax_model_calls(transaction)?;
    // version 5: the cache is not one entry. Reading it and writing it are two
    // gestures with two prices, and the missing one — the write — is the dearer.
    // They go at the end, in the same order as in the `CREATE TABLE`: rows are
    // written by position.
    for (column, kind) in [
        ("cache_write_tokens", "TEXT"),
        ("cache_write_long_tokens", "TEXT"),
        ("cache_write_price_micros_per_million", "INTEGER"),
        ("cache_write_long_price_micros_per_million", "INTEGER"),
    ] {
        if !column_exists(transaction, "model_calls", column)? {
            transaction.execute(
                &format!("ALTER TABLE model_calls ADD COLUMN {column} {kind}"),
                [],
            )?;
        }
    }
    // version 10: where a run was born. Everything a workspace owns could be
    // asked for by tree except its runs, so the window showed every tree's runs
    // mixed and had no way to ask for one.
    if !column_exists(transaction, "runs", "worktree")? {
        transaction.execute("ALTER TABLE runs ADD COLUMN worktree TEXT", [])?;
    }
    // version 11: which check refused a step's value, and what it saw. A
    // failure class says only that a check refused; the count per check that
    // `flow cost` prints needs the check named in a column of its own.
    if !column_exists(transaction, "steps", "refusal")? {
        transaction.execute("ALTER TABLE steps ADD COLUMN refusal TEXT", [])?;
    }
    // version 12: the line a step ran, program and arguments as started. Of a
    // step that ran a command the rows kept the outcome and not the text, so
    // whoever read the run later could not tell what had been executed.
    if !column_exists(transaction, "steps", "ran")? {
        transaction.execute("ALTER TABLE steps ADD COLUMN ran TEXT", [])?;
    }
    // version 13: which of the four reasons closed a run short. The status
    // already said `stopped`, and one word for four endings cannot be counted.
    if !column_exists(transaction, "runs", "stop_reason")? {
        transaction.execute("ALTER TABLE runs ADD COLUMN stop_reason TEXT", [])?;
    }
    // version 6: turns. You pay per turn, and no column counted them.
    if !column_exists(transaction, "model_calls", "turns")? {
        transaction.execute("ALTER TABLE model_calls ADD COLUMN turns TEXT", [])?;
    }
    // version 7: the session. A chain of four steps read 2,545,109 tokens from
    // cache to look at the same tree four times; the cure is the second step
    // continuing the first one's session, and continuing it means knowing its
    // name. Without a column to put the name in, "resume the previous step's
    // session" cannot even be expressed — and the constant above goes up with
    // it, or on an existing store this line never runs at all.
    if !column_exists(transaction, "model_calls", "session_id")? {
        transaction.execute("ALTER TABLE model_calls ADD COLUMN session_id TEXT", [])?;
    }
    // version 9: the kind of work, for a sum per kind of who did what.
    if !column_exists(transaction, "model_calls", "work_kind")? {
        transaction.execute("ALTER TABLE model_calls ADD COLUMN work_kind TEXT", [])?;
    }
    // version 8: the identity the process started with, replacing two columns
    // left over from a `current_mandate` table that no longer exists.
    //
    // **A RENAME, NOT A COLUMN AT THE END.** Readers go **by position**:
    // `mandate_name` was the sixteenth and `engine_identity` must sit there;
    // keeping both would say one thing in two ways — the next silent divergence.
    if column_exists(transaction, "model_calls", "mandate_name")?
        && !column_exists(transaction, "model_calls", "engine_identity")?
    {
        // The text already written stays where it is and reads back as
        // `EngineIdentity::Unrecorded`, the only true thing to say of a row
        // written while that field could still lie.
        transaction.execute(
            "ALTER TABLE model_calls RENAME COLUMN mandate_name TO engine_identity",
            [],
        )?;
    }
    // And `mandate_version` goes: it was empty by construction — a profile has
    // no version — and an always-empty column is the emptiness this work exists
    // to remove.
    if column_exists(transaction, "model_calls", "mandate_version")? {
        transaction.execute("ALTER TABLE model_calls DROP COLUMN mandate_version", [])?;
    }
    Ok(())
}

/// Rebuilds `model_calls` in the shape where counts and prices admit NULL,
/// keeping the rows already written.
///
/// **A REBUILD AND NOT AN `ALTER`**: SQLite cannot drop a `NOT NULL` from an
/// existing column. Not the event replay either — `rebuild_projections_in`
/// refuses a pruned log, so on a pruned store it would carry away these rows.
fn relax_model_calls(transaction: &Transaction<'_>) -> Result<(), LedgerError> {
    // Recognition keys off `total_tokens` because that column is born with this
    // version: if it is there the rebuild has already happened, so running this
    // function again does nothing at all.
    if column_exists(transaction, "model_calls", "total_tokens")? {
        return Ok(());
    }
    transaction.execute_batch(
        "CREATE TABLE model_calls_relaxed (
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
         INSERT INTO model_calls_relaxed (
             call_id, run_id, step_id, purpose, cli, requested_model, actual_model,
             input_tokens, output_tokens, cached_tokens, cost_micros, price_currency,
             input_price_micros_per_million, output_price_micros_per_million,
             cached_price_micros_per_million, mandate_name, mandate_version,
             retry_chain, error_type, started_at, ended_at)
         SELECT
             call_id, run_id, step_id, purpose, cli, requested_model, actual_model,
             input_tokens, output_tokens, cached_tokens, cost_micros, price_currency,
             input_price_micros_per_million, output_price_micros_per_million,
             cached_price_micros_per_million, mandate_name, mandate_version,
             retry_chain, error_type, started_at, ended_at
         FROM model_calls;
         DROP TABLE model_calls;
         ALTER TABLE model_calls_relaxed RENAME TO model_calls;",
    )?;
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
        let event = event_read_from(&payload)?;
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
        let event = event_read_from(&payload)?;
        project_event(transaction, &event)?;
        set_projection_watermark(transaction, seq)?;
    }
    Ok(())
}

/// One door for every event out of the log. It repairs nothing: what the record
/// cannot say in JSON, the record's own wire form says (see fault 33).
fn event_read_from(payload: &str) -> Result<StoredEvent, LedgerError> {
    serde_json::from_str(payload).map_err(LedgerError::from)
}

fn set_projection_watermark(transaction: &Transaction<'_>, seq: i64) -> Result<(), LedgerError> {
    transaction.execute(
        "UPDATE projection_watermark SET last_applied_seq = ?1 WHERE singleton = 1",
        [seq],
    )?;
    Ok(())
}

fn append_event(transaction: &Transaction<'_>, event: &StoredEvent) -> Result<(), LedgerError> {
    // From here the event goes into `events.db` while its projection goes into
    // `state.db`. With WAL, SQLite does not make the commit of the two attached
    // databases atomic. A new event is never projected in the transaction that
    // inserts it; the watermark advances only in the following one. A crash can
    // leave the projection behind the log, never ahead of it, and opening
    // applies only the missing tail.
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
        StoredEvent::ProcessStarted(record) => (
            "process_started",
            record.run_id.as_deref(),
            None,
            None,
            None,
            Some(record.started_at),
        ),
        StoredEvent::ProcessEnded(record) => (
            "process_ended",
            None,
            None,
            None,
            None,
            Some(record.ended_at),
        ),
        StoredEvent::InventoryScanned(scan) => (
            "inventory_scanned",
            None,
            None,
            None,
            None,
            Some(scan.taken_at),
        ),
        StoredEvent::RecordWritten(record) => (
            "record_written",
            None,
            None,
            None,
            None,
            Some(record.written_at),
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
        StoredEvent::InventoryScanned(scan) => project_inventory(transaction, scan),
        StoredEvent::RecordWritten(record) => project_record(transaction, record),
        StoredEvent::ProcessStarted(record) => project_process_started(transaction, record),
        StoredEvent::ProcessEnded(record) => project_process_ended(transaction, record),
        StoredEvent::Trace(_) => Ok(()),
    }
}

/// A process start goes into the table that answers "what is running".
///
/// **Restarting under the same name replaces the row.** Whoever relaunches live
/// mode wants today's process, not a collection of its ancestors: the history
/// is in the event log, where every start keeps its date. Without this, every
/// restart fills the "still running" list with ghosts, and nobody reads it.
fn project_process_started(
    transaction: &Transaction<'_>,
    record: &ProcessRecord,
) -> Result<(), LedgerError> {
    transaction.execute(
        "INSERT INTO processes
         (process_id, pid, command, args, working_directory, port, purpose,
          started_by, run_id, started_at, ended_at, exit_code)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, NULL, NULL)
         ON CONFLICT(process_id) DO UPDATE SET
          pid=excluded.pid, command=excluded.command, args=excluded.args,
          working_directory=excluded.working_directory, port=excluded.port,
          purpose=excluded.purpose, started_by=excluded.started_by,
          run_id=excluded.run_id, started_at=excluded.started_at,
          ended_at=NULL, exit_code=NULL",
        params![
            record.process_id,
            record.pid,
            record.command,
            serde_json::to_string(&record.args)?,
            record.working_directory,
            record.port,
            record.purpose,
            record.started_by,
            record.run_id,
            record.started_at,
        ],
    )?;
    Ok(())
}

/// Closing a process nobody had registered creates no row.
///
/// **Deliberate, and silence is the right answer here.** Inventing a row from a
/// close would mean writing a process with no known command, port or owner: an
/// entry that looks like a measure and is not. The event log keeps the close.
fn project_process_ended(
    transaction: &Transaction<'_>,
    record: &ProcessEndRecord,
) -> Result<(), LedgerError> {
    transaction.execute(
        "UPDATE processes SET ended_at = ?2, exit_code = ?3 WHERE process_id = ?1",
        params![record.process_id, record.ended_at, record.exit_code],
    )?;
    Ok(())
}

fn read_process_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<ProcessRecord> {
    let args: String = row.get(3)?;
    Ok(ProcessRecord {
        process_id: row.get(0)?,
        pid: row.get(1)?,
        command: row.get(2)?,
        // A row hand-written into the store might not be JSON: it reads as no
        // arguments rather than killing the read of the whole list. Whoever
        // hunts an orphan needs the list, not one perfect row.
        args: serde_json::from_str(&args).unwrap_or_default(),
        working_directory: row.get(4)?,
        port: row.get(5)?,
        purpose: row.get(6)?,
        started_by: row.get(7)?,
        run_id: row.get(8)?,
        started_at: row.get(9)?,
    })
}

const PROCESS_COLUMNS: &str = "process_id, pid, command, args, working_directory, port, \
                               purpose, started_by, run_id, started_at";

/// One entry, one value: the latest write replaces the previous one.
///
/// The history is not lost and does not belong here: it is in the log, where
/// every `record_written` keeps its date and its author. This table answers one
/// question — *right now*, what is this entry worth — and a table answering one
/// question cannot give two answers that disagree.
fn project_record(transaction: &Transaction<'_>, record: &StoreRecord) -> Result<(), LedgerError> {
    transaction.execute(
        "INSERT INTO store (collection, key, value, written_by, written_at)
         VALUES (?1, ?2, ?3, ?4, ?5)
         ON CONFLICT(collection, key) DO UPDATE SET
          value=excluded.value, written_by=excluded.written_by,
          written_at=excluded.written_at",
        params![
            record.collection,
            record.key,
            serde_json::to_string(&record.value)?,
            record.written_by,
            record.written_at,
        ],
    )?;
    Ok(())
}

fn read_failure_class_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<FailureClassCount> {
    Ok(FailureClassCount {
        failure_class: row.get(0)?,
        failures: row.get(1)?,
        runs_affected: row.get(2)?,
    })
}

/// Clips a text to a byte cap without splitting a character, and says whether
/// it clipped. The "whether" is returned, not inferred from the length: whoever
/// reads a truncated diagnosis unknowingly reads it as complete.
fn clip_to_bytes(value: String, max_bytes: usize) -> (String, bool) {
    if value.len() <= max_bytes {
        return (value, false);
    }
    let mut end = max_bytes;
    while end > 0 && !value.is_char_boundary(end) {
        end -= 1;
    }
    (value[..end].to_owned(), true)
}

fn read_store_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<StoreRecord> {
    let raw: String = row.get(2)?;
    Ok(StoreRecord {
        collection: row.get(0)?,
        key: row.get(1)?,
        // The value went in as JSON and must come out as it went in. If the
        // text on disk no longer parses — a store touched by hand, a truncated
        // file — the raw string comes back instead of failing the read: the
        // reader sees something wrong and notices, whereas an error here would
        // kill the whole row for the sake of one entry.
        value: serde_json::from_str(&raw).unwrap_or(Value::String(raw)),
        written_by: row.get(3)?,
        written_at: row.get(4)?,
    })
}

fn project_run(transaction: &Transaction<'_>, record: &RunRecord) -> Result<(), LedgerError> {
    transaction.execute(
        "INSERT INTO runs
         (run_id, kind, entity, parent_run_id, started_by, status,
          total_cost_micros, error, started_at, ended_at, worktree, stop_reason)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)
         ON CONFLICT(run_id) DO UPDATE SET
          kind=excluded.kind, entity=excluded.entity,
          parent_run_id=excluded.parent_run_id, started_by=excluded.started_by,
          status=excluded.status, total_cost_micros=excluded.total_cost_micros,
          error=excluded.error, started_at=excluded.started_at,
          ended_at=excluded.ended_at, stop_reason=excluded.stop_reason,
          -- Where a run was born does not change when it ends, and the row
          -- that closes it is written from a process that may stand elsewhere.
          worktree=COALESCE(excluded.worktree, runs.worktree)",
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
            record.worktree,
            record.stop_reason,
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
          checkpointed, refusal, ran)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14,
                 ?15, ?16, ?17, ?18, ?19, ?20, ?21, ?22)
         ON CONFLICT(run_id, step_id, attempt) DO UPDATE SET
          epoch=excluded.epoch, deps=excluded.deps,
          input_digest=excluded.input_digest, input=excluded.input,
          gates=excluded.gates, attempt_relation=excluded.attempt_relation,
          started_at=excluded.started_at,
          outcome=excluded.outcome, output=excluded.output, said=excluded.said,
          failure_class=excluded.failure_class, ended_at=excluded.ended_at,
          bytes_seen=excluded.bytes_seen, bytes_discarded=excluded.bytes_discarded,
          held_by_pid=excluded.held_by_pid, species=excluded.species,
          checkpointed=excluded.checkpointed, refusal=excluded.refusal,
          ran=excluded.ran",
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
            record
                .refusal
                .as_ref()
                .map(serde_json::to_string)
                .transpose()?,
            record
                .ran
                .as_ref()
                .map(serde_json::to_string)
                .transpose()?,
        ],
    )?;
    Ok(())
}

fn project_model_call(
    transaction: &Transaction<'_>,
    record: &ModelCallRecord,
) -> Result<(), LedgerError> {
    transaction.execute(
        // The columns are named one by one on purpose: a bare `VALUES` relies
        // on the table's order, and the column added later — there is always
        // one — shifts it without anything turning red.
        "INSERT INTO model_calls (
             call_id, run_id, step_id, purpose, cli, requested_model, actual_model,
             input_tokens, output_tokens, cached_tokens, cost_micros, price_currency,
             input_price_micros_per_million, output_price_micros_per_million,
             cached_price_micros_per_million, engine_identity,
             retry_chain, error_type, started_at, ended_at, total_tokens,
             declared_cost_micros, cache_write_tokens, cache_write_long_tokens,
             cache_write_price_micros_per_million,
             cache_write_long_price_micros_per_million, turns, session_id, work_kind)
         VALUES
         (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13,
          ?14, ?15, ?16, ?17, ?18, ?19, ?20, ?21, ?22, ?23, ?24, ?25, ?26, ?27, ?28, ?29)
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
          engine_identity=excluded.engine_identity,
          retry_chain=excluded.retry_chain, error_type=excluded.error_type,
          started_at=excluded.started_at, ended_at=excluded.ended_at,
          total_tokens=excluded.total_tokens,
          declared_cost_micros=excluded.declared_cost_micros,
          cache_write_tokens=excluded.cache_write_tokens,
          cache_write_long_tokens=excluded.cache_write_long_tokens,
          cache_write_price_micros_per_million=excluded.cache_write_price_micros_per_million,
          cache_write_long_price_micros_per_million=excluded.cache_write_long_price_micros_per_million,
          turns=excluded.turns,
          session_id=excluded.session_id, work_kind=excluded.work_kind",
        params![
            record.call_id,
            record.run_id,
            record.step_id,
            record.purpose,
            record.cli,
            record.requested_model,
            record.actual_model,
            // Counts stay text columns so precision beyond 2^53 is not lost; an
            // unknown count is a NULL, not the string "0".
            record.input_tokens.map(|n| n.to_string()),
            record.output_tokens.map(|n| n.to_string()),
            record.cached_tokens.map(|n| n.to_string()),
            record.cost_micros,
            record.price_currency,
            record.input_price_micros_per_million,
            record.output_price_micros_per_million,
            record.cached_price_micros_per_million,
            // "If an AI process starts there must be a profile associated with
            // it": this column is the requirement, and without it no diagnosis
            // can say what credentials a process ran under.
            record.engine_identity.to_column(),
            serde_json::to_string(&record.retry_chain)?,
            record.error_type,
            record.started_at,
            record.ended_at,
            record.total_tokens.map(|n| n.to_string()),
            record.declared_cost_micros,
            record.cache_write_tokens.map(|n| n.to_string()),
            record.cache_write_long_tokens.map(|n| n.to_string()),
            record.cache_write_price_micros_per_million,
            record.cache_write_long_price_micros_per_million,
            record.turns.map(|n| n.to_string()),
            record.session_id,
            record.work_kind,
        ],
    )?;
    Ok(())
}

fn read_inventory_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<InventoryChange> {
    Ok(InventoryChange {
        kind: row.get(0)?,
        name: row.get(1)?,
        path: row.get(2)?,
        origin: row.get(3)?,
        reach: row.get(4)?,
        reason: row.get(5)?,
        first_seen: row.get(6)?,
        last_seen: row.get(7)?,
        gone_at: row.get(8)?,
    })
}

/// An inventory scan becomes the state of what is, what came back, what is gone.
///
/// **THREE GESTURES, IN THIS ORDER, AND THE ORDER IS THE POINT.** Nothing is
/// cleared before it has been written: a projection that wipes and refills
/// shows an instant with an empty inventory, and whoever reads at that instant
/// — the page, a flow — sees a machine with nothing installed on it.
fn project_inventory(
    transaction: &Transaction<'_>,
    scan: &InventoryScan,
) -> Result<(), LedgerError> {
    // One: every entry seen refreshes `last_seen` and clears any vanish mark.
    // A thing that reappears is no longer gone, and keeping the mark would show
    // it dead for ever — to whoever prunes from this list, dead means deletable.
    for item in &scan.items {
        // Three: `first_seen` is missing from the `DO UPDATE SET` on purpose.
        // It is never rewritten after the first time, being the only date that
        // answers "since when have we had this".
        transaction.execute(
            "INSERT INTO inventory_items
             (kind, name, path, origin, reach, reason, first_seen, last_seen, gone_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?7, NULL)
             ON CONFLICT(kind, name, path) DO UPDATE SET
              origin=excluded.origin, reach=excluded.reach, reason=excluded.reason,
              last_seen=excluded.last_seen, gone_at=NULL",
            params![
                item.kind,
                item.name,
                item.path,
                item.origin,
                item.reach,
                item.reason,
                scan.taken_at,
            ],
        )?;
    }
    // Two: the entries this scan did **not** see, and that are not already
    // marked, take this scan's instant as the moment they vanished.
    transaction.execute(
        "UPDATE inventory_items SET gone_at = ?1
         WHERE last_seen < ?1 AND gone_at IS NULL",
        params![scan.taken_at],
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
        || record.refusal.is_some()
        || record.ran.is_some()
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
                    held_by_pid, species, refusal, ran
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
    let refusal: Option<String> = row.get(19)?;
    let ran: Option<String> = row.get(20)?;
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
        // A null output is the text «null» in the column; no output is SQL NULL.
        output: output
            .as_deref()
            .map(|value| json_column(value, 11))
            .transpose()?,
        said: row.get(12)?,
        failure_class: row.get(13)?,
        refusal: refusal
            .as_deref()
            .map(|value| json_column(value, 19))
            .transpose()?,
        ran: ran
            .as_deref()
            .map(|value| json_column(value, 20))
            .transpose()?,
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
        Outcome::NotYet => "NotYet",
        Outcome::Stopped => "Stopped",
        Outcome::Skipped => "Skipped",
    }
}

fn parse_outcome(value: &str, column: usize) -> rusqlite::Result<Outcome> {
    match value {
        "Went" => Ok(Outcome::Went),
        "Broke" => Ok(Outcome::Broke),
        "Waiting" => Ok(Outcome::Waiting),
        "NotYet" => Ok(Outcome::NotYet),
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

/// A species the store does not recognise is an error, not a silent fallback:
/// reading it as `hand_to_human` would be prudent for the single step and false
/// for whoever reads the history.
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

/// The columns of `model_calls` in the order the dump puts them, by **name**.
///
/// **PUBLIC ON PURPOSE.** `ui::parse` reads the dump by position, and so did a
/// second copy inside `actions` while it existed: two copies getting it wrong
/// together confirm each other, and no test sees it. This list is the anchor
/// outside both — a moved column turns red here.
pub const MODEL_CALL_DUMP_COLUMNS: &str = "call_id,run_id,step_id,purpose,cli,requested_model,actual_model,input_tokens,output_tokens,cached_tokens,cost_micros,price_currency,input_price_micros_per_million,output_price_micros_per_million,cached_price_micros_per_million,engine_identity,retry_chain,error_type,started_at,ended_at,total_tokens,declared_cost_micros,cache_write_tokens,cache_write_long_tokens,cache_write_price_micros_per_million,cache_write_long_price_micros_per_million,turns,session_id,work_kind";

fn dump_table(connection: &Connection, table: &str) -> Result<Value, LedgerError> {
    let columns = match table {
        // The two born later sit at the end, where a positional reader that
        // predates them simply finds no cell.
        "runs" => "run_id,kind,entity,parent_run_id,started_by,status,total_cost_micros,error,started_at,ended_at,worktree,stop_reason",
        "steps" => "run_id,step_id,attempt,epoch,deps,input_digest,input,gates,attempt_relation,started_at,outcome,output,said,failure_class,ended_at,bytes_seen,bytes_discarded,held_by_pid,species,checkpointed,refusal,ran",
        // The two columns born with version 4 sit at the end, and that is not
        // untidiness: readers of this dump go by position, and slotting them in
        // the middle would shift every index downstream without anything
        // noticing until a token appeared where a price should be.
        "model_calls" => MODEL_CALL_DUMP_COLUMNS,
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
