//! The shapes written into the store. Data only: no query touches them here.

use crate::identity::EngineIdentity;
use crate::LedgerError;
use serde::{Deserialize, Serialize};
use serde_json::Value;

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

/// The resolved flow definition one run executed, kept whole.
///
/// **THE NAME OF A FLOW IS NOT ITS DEFINITION.** A run recorded only `entity`,
/// and a renamed or deleted file leaves that name pointing at nothing, or at
/// something else. `digest` is taken over the bytes in `body`, so a definition
/// kept beside its hash cannot disagree with it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FlowDefinitionRecord {
    pub run_id: String,
    pub digest: String,
    pub body: String,
    pub recorded_at: i64,
}

impl FlowDefinitionRecord {
    pub fn of(run_id: &str, definition: &Value, recorded_at: i64) -> Self {
        Self {
            run_id: run_id.to_owned(),
            digest: flow::digest_input(definition),
            body: flow::canonical_text(definition),
            recorded_at,
        }
    }

    pub fn of_flow(
        run_id: &str,
        flow: &flow::FlowFile,
        recorded_at: i64,
    ) -> Result<Self, LedgerError> {
        Ok(Self::of(run_id, &serde_json::to_value(flow)?, recorded_at))
    }

    pub fn definition(&self) -> Result<Value, LedgerError> {
        serde_json::from_str(&self.body).map_err(LedgerError::from)
    }
}

/// What is known of the flow a run executed.
///
/// `Unknown` is an answer, not a gap: the run predates the snapshot, and no
/// reading of `entity` can reconstruct what it ran. Making the caller name that
/// case is the whole point — an empty list would let it pass for "a run with no
/// steps", which is a different and false thing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RunFlow {
    Unknown,
    /// Oldest first. More than one means a resume found the file changed, and
    /// both are true of that run.
    Recorded(Vec<FlowDefinitionRecord>),
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
    /// the "wait for an exhausted engine" branch the note `piano-consumo-e-profili`
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
    /// The engines the table put first for this kind that could not be used
    /// here. **Not [`Self::retry_chain`]**: those were started and failed.
    #[serde(default)]
    pub fell_back_from: Vec<String>,
    /// **WHAT THE STEP ASKED OF THE SESSION, AND WHAT IT GOT.** `None` asked
    /// for nothing. Without [`SessionMode::ColdFallback`] a run that never
    /// resumed once reads like one that resumed throughout, and its bill
    /// passes for proof.
    #[serde(default)]
    pub session_mode: Option<SessionMode>,
}

/// How a call stood towards the session of its flow: three say the mechanism
/// engaged, and the fourth that it did not, on a step that had asked.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SessionMode {
    Opened,
    Resumed,
    Forked,
    /// Asked to open, continue or branch, and none of it was possible.
    ColdFallback,
}

impl SessionMode {
    pub fn word(self) -> &'static str {
        match self {
            SessionMode::Opened => "opened",
            SessionMode::Resumed => "resumed",
            SessionMode::Forked => "forked",
            SessionMode::ColdFallback => "cold_fallback",
        }
    }

    /// A word this build does not know stays `None`, never a guess.
    pub fn from_word(word: &str) -> Option<SessionMode> {
        [
            SessionMode::Opened,
            SessionMode::Resumed,
            SessionMode::Forked,
            SessionMode::ColdFallback,
        ]
        .into_iter()
        .find(|mode| mode.word() == word)
    }
}

/// What a session already carried before this call, column by column.
///
/// **`None` IS «NOBODY CAN WORK IT OUT», AND IT SPREADS ON PURPOSE**: a sum
/// skipping an unknown row would be a baseline too low, and every later share
/// measured against it too high.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct SessionSoFar {
    pub input_tokens: Option<u64>,
    pub output_tokens: Option<u64>,
    pub cached_tokens: Option<u64>,
    pub cache_write_tokens: Option<u64>,
    pub cache_write_long_tokens: Option<u64>,
    pub total_tokens: Option<u64>,
    pub turns: Option<u64>,
    pub declared_cost_micros: Option<i64>,
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
    /// The second the kernel says that pid was born. Absent is a machine that
    /// would not say, or a row written before this was asked.
    #[serde(default)]
    pub born_at: Option<i64>,
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
