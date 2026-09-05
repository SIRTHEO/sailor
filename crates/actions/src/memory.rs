//! What Sailor remembers, on purpose: a memory is a typed, labelled fact with
//! a provenance, kept in the store's `memories` collection and never deleted —
//! superseded by a later write under the same label, or closed by `valid_until`.
//! A secret never reaches it: the write is refused before the first byte lands.

use flow::{Action, ActionError, ActionOutcome, SharedState, StepSpecies};
use ledger::{Ledger, StoreRecord};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

pub const REMEMBER_ACTION: &str = "remember";
pub const MEMORY_LIST_ACTION: &str = "memory_list";
pub const MEMORY_REPLACE_ACTION: &str = "memory_replace";
pub const MEMORIES_COLLECTION: &str = "memories";
pub const MEMORY_TYPES: &[&str] = &["user", "feedback", "project", "reference"];
/// The page handed to every command line at its start is cut here, the way
/// the command lines themselves cut their own index.
pub const PAGE_LINES: usize = 200;

pub fn register_memory(registry: &mut flow::ActionRegistry, ledger: Option<Ledger>, home: Option<PathBuf>) {
    registry.register(REMEMBER_ACTION, RememberAction::new(ledger.clone(), home.clone()));
    registry.register(MEMORY_LIST_ACTION, MemoryListAction::new(ledger.clone()));
    registry.register(MEMORY_REPLACE_ACTION, MemoryReplaceAction::new(ledger, home));
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

/// The key a memory is filed under, once it is known to be one the store takes.
fn admitted(memory: &Memory) -> Result<String, ActionError> {
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
    Ok(key)
}

/// Writes or supersedes a memory; `valid_from` survives a rewrite.
pub fn remember(ledger: &Ledger, memory: Memory) -> Result<Memory, ActionError> {
    let key = admitted(&memory)?;
    write_under(ledger, key, memory)
}

fn write_under(ledger: &Ledger, key: String, memory: Memory) -> Result<Memory, ActionError> {
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

/// What one consolidation did: how many it wrote, how many it closed, and the
/// labels asked to be dropped that no live memory carried.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Replacement {
    pub kept: usize,
    pub dropped: usize,
    pub unknown: Vec<String>,
}

/// Closes the memories under the `drop` labels and writes the `keep` ones, in
/// one pass: every kept memory is admitted before the first byte lands, and a
/// label on both lists is refused whole, since the store cannot hold both
/// answers. The store has no eraser — a dropped memory is closed at `at`.
pub fn replace(ledger: &Ledger, keep: Vec<Memory>, drop: &[String], at: i64) -> Result<Replacement, ActionError> {
    let keys = keep.iter().map(admitted).collect::<Result<Vec<_>, _>>()?;
    let dropped_keys: Vec<String> = drop.iter().map(|label| label_key(label)).collect();
    let both: Vec<&str> = drop
        .iter()
        .zip(&dropped_keys)
        .filter(|(_, key)| keys.contains(key))
        .map(|(label, _)| label.as_str())
        .collect();
    if !both.is_empty() {
        return Err(ActionError::new("kept_and_dropped", both.join(", ")));
    }
    let mut done = Replacement { kept: 0, dropped: 0, unknown: Vec::new() };
    for (label, key) in drop.iter().zip(&dropped_keys) {
        let live = ledger
            .read_record(MEMORIES_COLLECTION, key)
            .map_err(|error| ActionError::new("store_unreadable", error.to_string()))?
            .and_then(|record| serde_json::from_value::<Memory>(record.value).ok())
            .filter(|memory| memory.valid_until.is_none_or(|until| until > at));
        match live {
            Some(memory) => {
                write_under(ledger, key.clone(), Memory { valid_until: Some(at), modified: at, ..memory })?;
                done.dropped += 1;
            }
            None => done.unknown.push(label.clone()),
        }
    }
    for (key, memory) in keys.into_iter().zip(keep) {
        write_under(ledger, key, memory)?;
        done.kept += 1;
    }
    Ok(done)
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

/// Where the page lands under Sailor's home: one file, so every command line
/// reads the same bytes.
pub fn page_path(home: &Path) -> PathBuf {
    home.join("state").join("memory.md")
}

/// Renders the page of the memories valid now and writes it under `home`,
/// beside first and renamed over the old one: a reader never sees half a page.
pub fn write_page(ledger: &Ledger, home: &Path) -> Result<PathBuf, ActionError> {
    let memories = remembered(ledger, now())
        .map_err(|error| ActionError::new("store_unreadable", error.to_string()))?;
    let path = page_path(home);
    let unwritten = |error: std::io::Error| ActionError::new("page_unwritten", format!("{}: {error}", path.display()));
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(unwritten)?;
    }
    let beside = path.with_extension("md.part");
    std::fs::write(&beside, page(&memories)).map_err(unwritten)?;
    std::fs::rename(&beside, &path).map_err(unwritten)?;
    Ok(path)
}

pub struct RememberAction {
    ledger: Option<Ledger>,
    /// Where the page is refreshed after a write; `None` writes no page.
    home: Option<PathBuf>,
}

impl RememberAction {
    pub fn new(ledger: Option<Ledger>, home: Option<PathBuf>) -> Self {
        Self { ledger, home }
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
        let page = page_written(ledger, self.home.as_deref())?;
        Ok(ActionOutcome::Went(json!({
            "label": kept.label,
            "type": kept.kind,
            "modified": kept.modified,
            "valid_from": kept.valid_from,
            "page": page,
        })))
    }

    fn species(&self) -> StepSpecies {
        StepSpecies::Repeatable
    }
}

fn page_written(ledger: &Ledger, home: Option<&Path>) -> Result<Option<String>, ActionError> {
    match home {
        Some(home) => Ok(Some(write_page(ledger, home)?.display().to_string())),
        None => Ok(None),
    }
}

/// Every memory valid now, in the shape a flow hands on to the next step.
pub struct MemoryListAction {
    ledger: Option<Ledger>,
}

impl MemoryListAction {
    pub fn new(ledger: Option<Ledger>) -> Self {
        Self { ledger }
    }
}

impl Action for MemoryListAction {
    fn execute(&self, _input: &Value, _shared: &SharedState) -> Result<ActionOutcome, ActionError> {
        let ledger = self
            .ledger
            .as_ref()
            .ok_or_else(|| ActionError::new("no_store", String::new()))?;
        let memories = remembered(ledger, now())
            .map_err(|error| ActionError::new("store_unreadable", error.to_string()))?;
        let listed: Vec<Value> = memories
            .iter()
            .map(|memory| {
                json!({
                    "type": memory.kind,
                    "label": memory.label,
                    "value": memory.value,
                    "written": memory.modified,
                    "first_known": memory.valid_from,
                })
            })
            .collect();
        Ok(ActionOutcome::Went(json!({ "count": listed.len(), "memories": listed })))
    }

    fn species(&self) -> StepSpecies {
        StepSpecies::Repeatable
    }
}

#[derive(Debug, Deserialize)]
struct KeptSpec {
    #[serde(rename = "type")]
    kind: String,
    label: String,
    value: String,
}

#[derive(Debug, Deserialize)]
struct ReplaceSpec {
    keep: Vec<KeptSpec>,
    drop: Vec<String>,
    #[serde(default)]
    provenance: Option<String>,
    #[serde(default)]
    at: Option<i64>,
}

/// One consolidation: what to drop and what to keep, then a fresh page.
pub struct MemoryReplaceAction {
    ledger: Option<Ledger>,
    home: Option<PathBuf>,
}

impl MemoryReplaceAction {
    pub fn new(ledger: Option<Ledger>, home: Option<PathBuf>) -> Self {
        Self { ledger, home }
    }
}

impl Action for MemoryReplaceAction {
    fn execute(&self, input: &Value, _shared: &SharedState) -> Result<ActionOutcome, ActionError> {
        let spec: ReplaceSpec = serde_json::from_value(input.clone())
            .map_err(|error| ActionError::new("invalid_input", error.to_string()))?;
        let ledger = self
            .ledger
            .as_ref()
            .ok_or_else(|| ActionError::new("no_store", String::new()))?;
        let at = spec.at.unwrap_or_else(now);
        let provenance = spec.provenance.unwrap_or_else(|| MEMORY_REPLACE_ACTION.to_owned());
        let keep = spec
            .keep
            .into_iter()
            .map(|kept| Memory {
                kind: kept.kind,
                label: kept.label,
                value: kept.value,
                provenance: provenance.clone(),
                modified: at,
                valid_from: at,
                valid_until: None,
            })
            .collect();
        let done = replace(ledger, keep, &spec.drop, at)?;
        let page = page_written(ledger, self.home.as_deref())?;
        Ok(ActionOutcome::Went(json!({
            "kept": done.kept,
            "dropped": done.dropped,
            "unknown": done.unknown,
            "page": page,
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

    /// The file is the rendering, byte for byte, and writing it again with
    /// nothing changed leaves the same bytes and nothing beside them.
    #[test]
    fn the_page_is_written_as_a_file_identical_to_its_rendering() {
        let (ledger, dir) = a_ledger("file");
        let home = dir.join("home");
        remember(&ledger, a_memory("the trunk", "sorgenti", 10)).expect("first");
        remember(&ledger, a_memory("the home", "under state", 20)).expect("second");

        let path = write_page(&ledger, &home).expect("written");
        let first = std::fs::read(&path).expect("readable");
        let again = write_page(&ledger, &home).expect("written again");
        let second = std::fs::read(&again).expect("readable again");
        let beside: Vec<_> = std::fs::read_dir(path.parent().expect("a parent"))
            .expect("the state dir")
            .flatten()
            .map(|entry| entry.file_name().to_string_lossy().into_owned())
            .collect();
        let rendered = page(&remembered(&ledger, now()).expect("read"));
        drop(ledger);
        let _ = std::fs::remove_dir_all(&dir);

        assert_eq!(path, home.join("state").join("memory.md"));
        assert_eq!(first, rendered.as_bytes(), "the file is not the rendering");
        assert_eq!(first, second, "a second write changed the bytes");
        assert_eq!(beside, vec!["memory.md"], "something was left beside the page");
    }

    /// The action refreshes the page after every write, and writes none where
    /// it was given no home to write it in.
    #[test]
    fn the_remember_action_leaves_the_page_fresh() {
        let (ledger, dir) = a_ledger("action");
        let home = dir.join("home");
        let spec = |label: &str, at: i64| json!({"type": "project", "label": label, "value": "v", "at": at});
        let shared = SharedState::default();

        let with_home = RememberAction::new(Some(ledger.clone()), Some(home.clone()));
        with_home.execute(&spec("first", 1), &shared).expect("went");
        let after_one = std::fs::read_to_string(page_path(&home)).expect("a page");
        with_home.execute(&spec("second", 2), &shared).expect("went again");
        let after_two = std::fs::read_to_string(page_path(&home)).expect("a page again");

        let homeless = RememberAction::new(Some(ledger.clone()), None);
        homeless.execute(&spec("third", 3), &shared).expect("went without a home");
        let untouched = std::fs::read_to_string(page_path(&home)).expect("still a page");
        drop(ledger);
        let _ = std::fs::remove_dir_all(&dir);

        assert_eq!(after_one, "- **first** (project): v");
        assert_eq!(after_two, "- **second** (project): v\n- **first** (project): v");
        assert_eq!(untouched, after_two, "a page was refreshed with no home to write it in");
    }

    /// The list is what a flow hands the engine: type, label, value and when
    /// each was written, the closed ones left out.
    #[test]
    fn the_list_action_hands_on_every_live_memory_with_its_date() {
        let (ledger, dir) = a_ledger("list");
        remember(&ledger, a_memory("the trunk", "sorgenti", 10)).expect("first");
        let mut closed = a_memory("an old one", "gone", 20);
        closed.valid_until = Some(30);
        remember(&ledger, closed).expect("closed");
        let listed = MemoryListAction::new(Some(ledger.clone()))
            .execute(&Value::Null, &SharedState::default())
            .expect("listed");
        drop(ledger);
        let _ = std::fs::remove_dir_all(&dir);

        let ActionOutcome::Went(output) = listed else { panic!("the list went: {listed:?}") };
        assert_eq!(output["count"], 1, "{output}");
        assert_eq!(
            output["memories"][0],
            json!({"type": "project", "label": "the trunk", "value": "sorgenti", "written": 10, "first_known": 10})
        );
    }

    /// Dropped ones leave the live set, kept ones land, and a kept one that
    /// carries an old label keeps the date it was first known.
    #[test]
    fn a_replacement_drops_and_keeps_in_one_pass() {
        let (ledger, dir) = a_ledger("replace");
        remember(&ledger, a_memory("twice a", "the trunk is sorgenti", 10)).expect("a");
        remember(&ledger, a_memory("twice b", "sorgenti is the trunk", 11)).expect("b");
        remember(&ledger, a_memory("stale", "the trunk is main", 12)).expect("stale");
        let done = replace(
            &ledger,
            vec![a_memory("The trunk", "sorgenti, and it is pushed at release", 50)],
            &["twice a".to_owned(), "twice b".to_owned(), "stale".to_owned(), "never was".to_owned()],
            50,
        )
        .expect("replaced");
        let all = remembered(&ledger, 60).expect("read");
        drop(ledger);
        let _ = std::fs::remove_dir_all(&dir);

        assert_eq!(done, Replacement { kept: 1, dropped: 3, unknown: vec!["never was".to_owned()] });
        assert_eq!(all.len(), 1, "{all:?}");
        assert_eq!(all[0].label, "The trunk");
        assert_eq!(all[0].valid_from, 50);
    }

    /// A secret among the kept refuses the whole pass: nothing is dropped, and
    /// nothing before the secret in the list is written either.
    #[test]
    fn a_secret_among_the_kept_refuses_the_whole_replacement() {
        let (ledger, dir) = a_ledger("replace-secret");
        remember(&ledger, a_memory("old", "v", 10)).expect("old");
        let refused = replace(
            &ledger,
            vec![a_memory("fine", "a plain fact", 50), a_memory("token", "sk-abcdefghijklmnopqrstuvwxyz", 50)],
            &["old".to_owned()],
            50,
        )
        .expect_err("refused");
        let all = remembered(&ledger, 60).expect("read");
        drop(ledger);
        let _ = std::fs::remove_dir_all(&dir);

        assert_eq!(refused.class, "secret_in_memory");
        assert_eq!(all.iter().map(|m| m.label.as_str()).collect::<Vec<_>>(), vec!["old"], "{all:?}");
    }

    /// A label on both lists is a contradiction, matched on the key and not the
    /// spelling, and refused before anything moves.
    #[test]
    fn a_label_both_kept_and_dropped_is_refused_before_anything_moves() {
        let (ledger, dir) = a_ledger("replace-both");
        remember(&ledger, a_memory("the trunk", "main", 10)).expect("old");
        let refused = replace(
            &ledger,
            vec![a_memory("The Trunk", "sorgenti", 50)],
            &["the trunk".to_owned()],
            50,
        )
        .expect_err("refused");
        let all = remembered(&ledger, 60).expect("read");
        drop(ledger);
        let _ = std::fs::remove_dir_all(&dir);

        assert_eq!(refused.class, "kept_and_dropped");
        assert_eq!(refused.said, "the trunk");
        assert_eq!(all[0].value, "main", "the old value was touched: {all:?}");
    }

    /// The action rewrites the page, so the command lines read the consolidated
    /// set at their next start and not the one before.
    #[test]
    fn the_replace_action_rewrites_the_page() {
        let (ledger, dir) = a_ledger("replace-page");
        let home = dir.join("home");
        let shared = SharedState::default();
        RememberAction::new(Some(ledger.clone()), Some(home.clone()))
            .execute(&json!({"type": "project", "label": "old", "value": "v", "at": 1}), &shared)
            .expect("went");
        let before = std::fs::read_to_string(page_path(&home)).expect("a page");

        let went = MemoryReplaceAction::new(Some(ledger.clone()), Some(home.clone()))
            .execute(
                &json!({"keep": [{"type": "feedback", "label": "new", "value": "w"}], "drop": ["old"], "at": 2}),
                &shared,
            )
            .expect("replaced");
        let after = std::fs::read_to_string(page_path(&home)).expect("a page again");
        drop(ledger);
        let _ = std::fs::remove_dir_all(&dir);

        let ActionOutcome::Went(output) = went else { panic!("the replacement went: {went:?}") };
        assert_eq!(before, "- **old** (project): v");
        assert_eq!(after, "- **new** (feedback): w");
        assert_eq!(output["kept"], 1);
        assert_eq!(output["dropped"], 1);
        assert_eq!(output["page"], json!(page_path(&home).display().to_string()));
    }
}
