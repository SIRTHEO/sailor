//! How full the context of a session is, read from what the command line
//! already writes about itself.
//!
//! **THE THIRD ANSWER IS «I DO NOT KNOW», AND IT IS NOT «BELOW»**: answering
//! «below» would let a full session pass for a fresh one. Two readings are the
//! command line's own; a third estimates from the bytes that crossed the pipe.

use flow::{Action, ActionError, ActionOutcome, SharedState, StepSpecies};
use serde::Deserialize;
use serde_json::{json, Value};

/// The name `MeasureSessionAction` registers under.
pub const MEASURE_SESSION_ACTION: &str = "measure_session";

const KNOWN_FIELDS: &[&str] = &["transcript", "rollout", "bytes", "warn", "oblige"];

/// Where the relay asks for the mandate, and where it stops waiting. Absolute
/// tokens, not a share of the window: over 3,414 sessions an edit refused for
/// a reason of memory rose from 0.22% of attempts below 64k to 1.98% above
/// 300k, and nine tenths of a large window sits past all of it.
pub const WARN_TOKENS: u64 = 150_000;
pub const OBLIGE_TOKENS: u64 = 250_000;

/// What the estimate assumes when nothing else can be read: the prologue that
/// costs tokens without crossing the pipe, and the tokens a byte carries.
const PROLOGUE_TOKENS: u64 = 60_000;
const TOKENS_PER_BYTE: f64 = 0.68;

/// A reading a segment falls to counts as a reset, not as a shrinking context:
/// no command line takes back a third of its own prompt.
const A_RESET_FALLS_BELOW: f64 = 0.66;

/// An edit refused for a reason of memory: the file does not say what the
/// agent believed it said, or was never read where the agent is looking.
const A_MEMORY_MISS: &[&str] = &[
    "string to replace not found",
    "has not been read yet",
    "file has been modified since",
    "file has not been read",
];

pub fn register_measure(registry: &mut flow::ActionRegistry) {
    registry.register(MEASURE_SESSION_ACTION, MeasureSessionAction);
}

#[derive(Debug, Deserialize)]
struct MeasureSpec {
    #[serde(default)]
    transcript: Option<String>,
    #[serde(default)]
    rollout: Option<String>,
    #[serde(default)]
    bytes: Option<u64>,
    #[serde(default)]
    warn: Option<u64>,
    #[serde(default)]
    oblige: Option<u64>,
}

/// What a source answered: the tokens standing in the context now, the model
/// that answered last, and the misses counted since the last reset.
#[derive(Debug, Default, PartialEq)]
pub struct Reading {
    pub tokens: u64,
    pub model: Option<String>,
    pub misses: u64,
    pub records: u64,
}

struct MeasureSessionAction;

impl Action for MeasureSessionAction {
    fn execute(&self, input: &Value, _shared: &SharedState) -> Result<ActionOutcome, ActionError> {
        let spec: MeasureSpec = serde_json::from_value(input.clone())
            .map_err(|error| ActionError::new("invalid_input", error.to_string()))?;
        let warn = spec.warn.unwrap_or(WARN_TOKENS);
        let oblige = spec.oblige.unwrap_or(OBLIGE_TOKENS);
        let (source, reading) = read_in_order(&spec);
        let state = match &reading {
            None => "unknown",
            Some(read) => standing(read.tokens, warn, oblige),
        };
        let read = reading.unwrap_or_default();
        Ok(ActionOutcome::Went(json!({
            "source": source,
            "state": state,
            "tokens": read.tokens,
            "model": read.model,
            "misses": read.misses,
            "records": read.records,
            "warn": warn,
            "oblige": oblige,
        })))
    }

    fn unknown_fields(&self, declared: &Value) -> Vec<String> {
        match declared.as_object() {
            Some(fields) => fields
                .keys()
                .filter(|name| !KNOWN_FIELDS.contains(&name.as_str()))
                .cloned()
                .collect(),
            None => Vec::new(),
        }
    }

    fn species(&self) -> StepSpecies {
        StepSpecies::Repeatable
    }

    fn redo_evidence(&self, _record: &flow::StepRecord) -> flow::RedoEvidence {
        flow::RedoEvidence::TouchesNothing
    }
}

/// The engine's own word first, the estimate only after it, and nothing is
/// invented: a source named and unreadable ends the search at `unknown`.
fn read_in_order(spec: &MeasureSpec) -> (&'static str, Option<Reading>) {
    if let Some(path) = &spec.transcript {
        return (
            "transcript",
            the_last_of(path).and_then(|text| from_transcript(&text)),
        );
    }
    if let Some(path) = &spec.rollout {
        return (
            "rollout",
            the_last_of(path).and_then(|text| from_rollout(&text)),
        );
    }
    match spec.bytes {
        Some(bytes) => ("bytes", Some(from_bytes(bytes))),
        None => ("none", None),
    }
}

/// How much of a record is read from its end.
///
/// **A SESSION'S RECORD REACHES HUNDREDS OF MEGABYTES**, and this is asked once
/// a turn on a machine that stays on for years. The fill is the last answer's
/// prompt, which is at the end. What the cap costs is `misses`, then counted
/// over the last stretch: a floor, in the direction that invents nothing.
const A_TAIL_WE_READ: u64 = 32 * 1024 * 1024;

fn the_last_of(path: &str) -> Option<String> {
    tail_of(path, A_TAIL_WE_READ)
}

/// The end of a file, from the first whole line inside the cap, an argument so
/// a test reaches the second branch without writing tens of megabytes.
pub fn tail_of(path: &str, cap: u64) -> Option<String> {
    use std::io::{Read, Seek, SeekFrom};
    let mut file = std::fs::File::open(path).ok()?;
    let whole = file.metadata().ok()?.len();
    if whole <= cap {
        let mut text = String::new();
        file.read_to_string(&mut text).ok()?;
        return Some(text);
    }
    file.seek(SeekFrom::Start(whole - cap)).ok()?;
    let mut bytes = Vec::new();
    file.read_to_end(&mut bytes).ok()?;
    let text = String::from_utf8_lossy(&bytes).into_owned();
    // The first line is cut in the middle and is not a record: dropping it is
    // what keeps a half-line from being read as a session that says nothing.
    Some(match text.find('\n') {
        Some(at) => text[at + 1..].to_owned(),
        None => String::new(),
    })
}

fn standing(tokens: u64, warn: u64, oblige: u64) -> &'static str {
    if tokens >= oblige {
        return "oblige";
    }
    if tokens >= warn {
        return "warn";
    }
    "below"
}

/// Three fields that are one prompt: counting one alone understates a warm
/// cache by an order of magnitude.
pub fn from_transcript(text: &str) -> Option<Reading> {
    let mut read = Reading::default();
    for line in text.lines() {
        let Ok(row) = serde_json::from_str::<Value>(line) else {
            continue;
        };
        // The marker is read on every row, and not only where a reading is:
        // the row that announces a reset carries no prompt of its own.
        if row["isCompactSummary"].as_bool() == Some(true) {
            read = Reading::default();
        }
        let message = &row["message"];
        if let Some(usage) = message.get("usage").filter(|value| value.is_object()) {
            let now = number(&usage["input_tokens"])
                + number(&usage["cache_read_input_tokens"])
                + number(&usage["cache_creation_input_tokens"]);
            if fell_away(read.tokens, now) {
                read = Reading::default();
            }
            read.tokens = now;
            read.records += 1;
            if let Some(model) = message["model"].as_str() {
                read.model = Some(model.to_owned());
            }
        }
        read.misses += misses_in(&message["content"]);
    }
    (read.records > 0).then_some(read)
}

/// A fall no prompt makes by itself, for the reset nothing announced.
fn fell_away(held: u64, now: u64) -> bool {
    held > 0 && (now as f64) < held as f64 * A_RESET_FALLS_BELOW
}

/// **THE THREAD'S OWN TOTAL IS NOT THE CONTEXT**: it adds every call ever made
/// and reaches tens of millions, so read as fill it says «full» forever.
pub fn from_rollout(text: &str) -> Option<Reading> {
    let mut read = Reading::default();
    for line in text.lines() {
        let Ok(row) = serde_json::from_str::<Value>(line) else {
            continue;
        };
        if row["type"] != json!("token_usage_record") {
            continue;
        }
        let usage = &row["payload"]["usage"];
        if usage.is_object() {
            read.tokens = number(&usage["input_tokens"]);
            read.records += 1;
        }
    }
    (read.records > 0).then_some(read)
}

/// For a held terminal whose engine says nothing: the prologue is paid without
/// a byte crossing the pipe, the rest grows with what does.
pub fn from_bytes(bytes: u64) -> Reading {
    Reading {
        tokens: PROLOGUE_TOKENS + (bytes as f64 * TOKENS_PER_BYTE) as u64,
        ..Reading::default()
    }
}

fn number(value: &Value) -> u64 {
    value.as_u64().unwrap_or(0)
}

fn misses_in(content: &Value) -> u64 {
    let Some(blocks) = content.as_array() else {
        return 0;
    };
    blocks
        .iter()
        .filter(|block| block["is_error"].as_bool() == Some(true))
        .filter(|block| {
            let said = block["content"].to_string().to_lowercase();
            A_MEMORY_MISS.iter().any(|mark| said.contains(mark))
        })
        .count() as u64
}
