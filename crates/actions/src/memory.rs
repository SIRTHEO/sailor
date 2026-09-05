//! What Sailor remembers, on purpose: a memory is a typed, labelled fact with
//! a provenance, kept in the store's `memories` collection and never deleted —
//! superseded by a later write under the same label, or closed by `valid_until`.
//! A secret never reaches it: the write is refused before the first byte lands.

use flow::{Action, ActionError, ActionOutcome, SharedState, StepSpecies};
use ledger::{Ledger, StoreRecord};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::time::{SystemTime, UNIX_EPOCH};

pub const REMEMBER_ACTION: &str = "remember";
pub const MEMORIES_COLLECTION: &str = "memories";
pub const MEMORY_TYPES: &[&str] = &["user", "feedback", "project", "reference"];
/// The page handed to every command line at its start is cut here, the way
/// the command lines themselves cut their own index.
pub const PAGE_LINES: usize = 200;

pub fn register_memory(registry: &mut flow::ActionRegistry, ledger: Option<Ledger>) {
    registry.register(REMEMBER_ACTION, RememberAction::new(ledger));
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Memory {
    #[serde(rename = "type")]
    pub kind: String,
    pub label: String,
    pub value: String,
    /// Who wrote it: a run and step, a session, or a person's own words.
    pub provenance: String,
    pub modified: i64,
    pub valid_from: i64,
    #[serde(default)]
    pub valid_until: Option<i64>,
}

#[derive(Debug, Deserialize)]
struct RememberSpec {
    #[serde(rename = "type")]
    kind: String,
    label: String,
    value: String,
    #[serde(default)]
    provenance: Option<String>,
    #[serde(default)]
    valid_until: Option<i64>,
    #[serde(default)]
    at: Option<i64>,
}

fn now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or_default()
}

/// The shapes a secret takes in text. Prefixes and framings, not entropy: a
/// rule a person can read is a rule a person can extend.
const SECRET_SHAPES: &[&str] = &[
    "sk-", "sk_live_", "sk_test_", "ghp_", "gho_", "github_pat_", "AKIA", "xoxb-", "xoxp-",
    "-----BEGIN", "eyJhbGci", "Bearer ", "AIza",
];

/// Whether the text carries something that looks like a credential.
pub fn looks_like_a_secret(text: &str) -> bool {
    SECRET_SHAPES.iter().any(|shape| {
        text.match_indices(shape).any(|(at, _)| {
            let rest = &text[at + shape.len()..];
            let run = rest.chars().take_while(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | '.' | '/' | '+' | '=')).count();
            shape.ends_with(' ') || shape.starts_with("-----") || run >= 16
        })
    })
}

/// The label as a key: lowercase, words joined by dashes.
pub fn label_key(label: &str) -> String {
    label
        .trim()
        .to_lowercase()
        .split(|c: char| !c.is_alphanumeric())
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>()
        .join("-")
}

/// Writes or supersedes a memory; `valid_from` survives a rewrite.
pub fn remember(ledger: &Ledger, memory: Memory) -> Result<Memory, ActionError> {
    if !MEMORY_TYPES.contains(&memory.kind.as_str()) {
        return Err(ActionError::new("bad_memory_type", memory.kind.clone()));
    }
    let key = label_key(&memory.label);
    if key.is_empty() || memory.value.trim().is_empty() {
        return Err(ActionError::new("invalid_input", "a memory needs a label and a value".to_owned()));
    }
    if looks_like_a_secret(&memory.value) || looks_like_a_secret(&memory.label) {
        return Err(ActionError::new("secret_in_memory", memory.label.clone()));
    }
    let earlier = ledger
        .read_record(MEMORIES_COLLECTION, &key)
        .map_err(|error| ActionError::new("store_unreadable", error.to_string()))?
        .and_then(|record| serde_json::from_value::<Memory>(record.value).ok());
    let kept = Memory {
        valid_from: earlier.map(|it| it.valid_from).unwrap_or(memory.valid_from),
        ..memory
    };
    let record = StoreRecord {
        collection: MEMORIES_COLLECTION.to_owned(),
        key,
        value: serde_json::to_value(&kept).map_err(|error| ActionError::new("invalid_input", error.to_string()))?,
        written_by: kept.provenance.clone(),
        written_at: kept.modified,
    };
    ledger
        .put_record(&record)
        .map_err(|error| ActionError::new("store_refused", error.to_string()))?;
    Ok(kept)
}

/// The memories still valid at `at`, the most recently modified first.
pub fn remembered(ledger: &Ledger, at: i64) -> Result<Vec<Memory>, ledger::LedgerError> {
    let mut memories: Vec<Memory> = ledger
        .records_in(MEMORIES_COLLECTION)?
        .into_iter()
        .filter_map(|record| serde_json::from_value::<Memory>(record.value).ok())
        .filter(|memory| memory.valid_until.is_none_or(|until| until > at))
        .collect();
    memories.sort_by(|a, b| b.modified.cmp(&a.modified).then_with(|| a.label.cmp(&b.label)));
    Ok(memories)
}

/// The page every command line is handed at its start: one line per memory,
/// the most recent first, cut at [`PAGE_LINES`]. One function, so the three
/// command lines read the same bytes.
pub fn page(memories: &[Memory]) -> String {
    let mut lines: Vec<String> = memories
        .iter()
        .map(|memory| format!("- **{}** ({}): {}", memory.label, memory.kind, memory.value.replace('\n', " ")))
        .collect();
    if lines.len() > PAGE_LINES {
        let left = lines.len() - (PAGE_LINES - 1);
        lines.truncate(PAGE_LINES - 1);
        lines.push(format!("- … and {left} more: `sailor search <word>` finds them"));
    }
    lines.join("\n")
}

pub struct RememberAction {
    ledger: Option<Ledger>,
}

impl RememberAction {
    pub fn new(ledger: Option<Ledger>) -> Self {
        Self { ledger }
    }
}

impl Action for RememberAction {
    fn execute(&self, input: &Value, _shared: &SharedState) -> Result<ActionOutcome, ActionError> {
        let spec: RememberSpec = serde_json::from_value(input.clone())
            .map_err(|error| ActionError::new("invalid_input", error.to_string()))?;
        let ledger = self
            .ledger
            .as_ref()
            .ok_or_else(|| ActionError::new("no_store", String::new()))?;
        let at = spec.at.unwrap_or_else(now);
        let kept = remember(
            ledger,
            Memory {
                kind: spec.kind,
                label: spec.label,
                value: spec.value,
                provenance: spec.provenance.unwrap_or_else(|| REMEMBER_ACTION.to_owned()),
                modified: at,
                valid_from: at,
                valid_until: spec.valid_until,
            },
        )?;
        Ok(ActionOutcome::Went(json!({
            "label": kept.label,
            "type": kept.kind,
            "modified": kept.modified,
            "valid_from": kept.valid_from,
        })))
    }

    fn species(&self) -> StepSpecies {
        StepSpecies::Repeatable
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn a_ledger(name: &str) -> (Ledger, std::path::PathBuf) {
        let dir = std::env::temp_dir().join(format!("sailor-memory-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        (Ledger::open(&dir).expect("a ledger"), dir)
    }

    fn a_memory(label: &str, value: &str, at: i64) -> Memory {
        Memory {
            kind: "project".to_owned(),
            label: label.to_owned(),
            value: value.to_owned(),
            provenance: "test".to_owned(),
            modified: at,
            valid_from: at,
            valid_until: None,
        }
    }

    /// Written, read back, and superseded under the same label with its first
    /// `valid_from` kept: the date a fact was first known is part of the fact.
    #[test]
    fn a_memory_is_kept_and_a_rewrite_keeps_when_it_was_first_known() {
        let (ledger, dir) = a_ledger("kept");
        remember(&ledger, a_memory("The trunk", "sorgenti", 10)).expect("first");
        remember(&ledger, a_memory("the trunk", "sorgenti, pushed at release", 20)).expect("again");
        let all = remembered(&ledger, 30).expect("read");
        drop(ledger);
        let _ = std::fs::remove_dir_all(&dir);

        assert_eq!(all.len(), 1, "one label, one memory: {all:?}");
        assert_eq!(all[0].value, "sorgenti, pushed at release");
        assert_eq!(all[0].valid_from, 10);
        assert_eq!(all[0].modified, 20);
    }

    /// **A SECRET NEVER REACHES THE TABLE**: refused before the write, and the
    /// store holds nothing under that label.
    #[test]
    fn a_memory_carrying_a_secret_is_refused_and_nothing_is_written() {
        let (ledger, dir) = a_ledger("secret");
        let refused = remember(&ledger, a_memory("a token", "use ghp_abcdefghijklmnopqrstuvwxyz0123456789 to push", 1))
            .expect_err("refused");
        let all = remembered(&ledger, 2).expect("read");
        drop(ledger);
        let _ = std::fs::remove_dir_all(&dir);

        assert_eq!(refused.class, "secret_in_memory");
        assert!(all.is_empty(), "{all:?}");
    }

    #[test]
    fn the_shapes_of_a_secret_are_recognised_and_plain_words_are_not() {
        assert!(looks_like_a_secret("sk-abcdefghijklmnopqrstuvwxyz"));
        assert!(looks_like_a_secret("-----BEGIN OPENSSH PRIVATE KEY-----"));
        assert!(looks_like_a_secret("Authorization: Bearer x"));
        assert!(looks_like_a_secret("AKIAIOSFODNN7EXAMPLE"));
        assert!(!looks_like_a_secret("the sketch-based draft flow"));
        assert!(!looks_like_a_secret("ask the ledger, never guess"));
    }

    #[test]
    fn a_type_outside_the_four_is_refused() {
        let (ledger, dir) = a_ledger("type");
        let mut wrong = a_memory("x", "y", 1);
        wrong.kind = "rumour".to_owned();
        let refused = remember(&ledger, wrong).expect_err("refused");
        drop(ledger);
        let _ = std::fs::remove_dir_all(&dir);
        assert_eq!(refused.class, "bad_memory_type");
    }

    /// A closed memory is not handed on; the page is the most recent first and
    /// stops at the line every command line stops at.
    #[test]
    fn the_page_is_recent_first_valid_only_and_cut_at_two_hundred_lines() {
        let (ledger, dir) = a_ledger("page");
        for n in 0..250 {
            remember(&ledger, a_memory(&format!("fact {n}"), &format!("value {n}"), n)).expect("write");
        }
        let mut closed = a_memory("fact 249", "value 249", 249);
        closed.valid_until = Some(300);
        remember(&ledger, closed).expect("closed");
        let all = remembered(&ledger, 400).expect("read");
        drop(ledger);
        let _ = std::fs::remove_dir_all(&dir);

        assert_eq!(all.len(), 249, "the closed one is out");
        assert_eq!(all[0].label, "fact 248", "most recent first");
        let text = page(&all);
        assert_eq!(text.lines().count(), PAGE_LINES);
        assert!(text.lines().last().expect("a line").contains("and 50 more"), "{text}");
    }
}
