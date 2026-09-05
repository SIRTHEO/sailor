use serde::{Deserialize, Deserializer, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;

pub const MAX_SAID_BYTES: usize = 16 * 1024;

/// The longest excerpt of an offending value a refusal keeps: enough to
/// recognise the value, and a bound that keeps a row from growing with it.
pub const MAX_SEEN_BYTES: usize = 160;

/// Which declared check refused a value, where in it, by which rule, and an
/// excerpt of what it saw. Written next to the failure class so that a person
/// reads the rule and a count can be taken per check without parsing prose.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Refusal {
    pub check: String,
    /// The path of the field that failed; empty when the whole value did.
    pub path: String,
    pub rule: RefusalRule,
    pub seen: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RefusalRule {
    MissingField,
    UnknownField,
    WrongType,
    NotAllowed,
    NotJson,
    TooLong,
    ExitCode,
}

impl RefusalRule {
    pub fn name(self) -> &'static str {
        match self {
            RefusalRule::MissingField => "missing_field",
            RefusalRule::UnknownField => "unknown_field",
            RefusalRule::WrongType => "wrong_type",
            RefusalRule::NotAllowed => "not_allowed",
            RefusalRule::NotJson => "not_json",
            RefusalRule::TooLong => "too_long",
            RefusalRule::ExitCode => "exit_code",
        }
    }
}

impl Refusal {
    pub fn new(
        check: impl Into<String>,
        path: impl Into<String>,
        rule: RefusalRule,
        seen: &str,
    ) -> Self {
        Self {
            check: check.into(),
            path: path.into(),
            rule,
            seen: head(seen.trim(), MAX_SEEN_BYTES),
        }
    }

    /// The sentence a person reads for it, from the catalogue.
    pub fn explain(&self) -> String {
        let rule = catalogue::say(&format!("run.refusal.rule.{}", self.rule.name()), &[]);
        let values = [
            ("check", self.check.as_str()),
            ("path", self.path.as_str()),
            ("rule", rule.as_str()),
            ("seen", self.seen.as_str()),
        ];
        if self.path.is_empty() {
            catalogue::say("run.refusal.whole", &values)
        } else {
            catalogue::say("run.refusal.at_path", &values)
        }
    }
}

/// The first `limit` bytes of `text`, cut on a character boundary.
fn head(text: &str, limit: usize) -> String {
    if text.len() <= limit {
        return text.to_owned();
    }
    let mut end = limit;
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    text[..end].to_owned()
}

/// A step as a durable record.
/// Intent is written BEFORE running; the outcome AFTER.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StepRecord {
    pub run_id: String,
    pub step_id: String,
    pub attempt: u32,
    /// Monotonic. An attempt given up for dead that comes back cannot overwrite
    /// state written since.
    pub epoch: u64,
    pub deps: Vec<String>,
    /// Digest of the typed input alone; it does not cover the step's gates.
    ///
    /// A declared residue that runs itself out: a run begun while references
    /// were still resolved inside each action, and resumed since, compares an
    /// old raw digest against a new resolved one and calls the same work
    /// `DifferentInput`. That is a wrong label on an attempt, not data lost.
    pub input_digest: String,
    /// The input as the step received it when it stopped being processed. One
    /// rule, three cases from it: **ran** → references resolved, what the
    /// action really read; **skipped** (`when` unsatisfied) → unresolved, the
    /// condition being judged first so a step that does not run pays no
    /// references for work it will not do; **broke resolving one** → unresolved
    /// on purpose, so the reader sees the pointer, not the hole it left.
    pub input: Value,
    /// The gates active when the step started.
    pub gates: Vec<String>,
    /// How the resume relates to the identity already recorded, per the engine.
    #[serde(default)]
    pub attempt_relation: Option<AttemptRelation>,
    /// The pid of the process holding the step while it ran, written by the
    /// engine at open. A field, not a convention inside `input`: whoever
    /// resumes must be able to ask the kernel whether that process is alive
    /// without knowing what the caller named its own key.
    #[serde(default)]
    pub held_by_pid: Option<u32>,
    /// The step's species, frozen at open: it says whether redoing it is safe.
    /// `None` is a record written before species existed, and counts as
    /// `HandToHuman` — never as `Repeatable`.
    #[serde(default)]
    pub species: Option<StepSpecies>,
    pub started_at: i64,

    // Written at close. `deserialize_with` makes even the null fields
    // mandatory: a truncated record must not be able to look valid.
    /// Which of `input`'s three cases holds is said here, not by that field —
    /// worth saying, because a `{"$from": …}` read in there is no defect by
    /// itself: on `Skipped` it is the norm, on `Broke` with the class
    /// `unresolved_reference` it is the diagnosis, on `Went` a real fault.
    #[serde(deserialize_with = "required_option")]
    pub outcome: Option<Outcome>,
    #[serde(deserialize_with = "required_option")]
    pub output: Option<Value>,
    /// **THAT THERE IS AN OUTPUT IS WRITTEN DOWN, NOT DEDUCED FROM IT.** A null
    /// output serialises the way *no* output does, so it came back as none and
    /// the step after it died saying it had no typed output.
    #[serde(default)]
    pub output_was_written: bool,
    /// Raw text, truncated. It serves a person when something goes wrong. It is
    /// NOT the data channel: no condition is ever evaluated on it.
    #[serde(deserialize_with = "required_option")]
    pub said: Option<String>,
    /// Filled by the engine, not by a model. A class, not a diagnosis.
    #[serde(deserialize_with = "required_option")]
    pub failure_class: Option<String>,
    /// Which check refused, and what it saw, when the class is a refusal.
    #[serde(default)]
    pub refusal: Option<Refusal>,
    #[serde(deserialize_with = "required_option")]
    pub ended_at: Option<i64>,
    /// Total bytes emitted; not part of the typed data channel.
    #[serde(default)]
    pub bytes_seen: Option<u64>,
    /// Bytes cut for running past the configured cap.
    #[serde(default)]
    pub bytes_discarded: Option<u64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Outcome {
    Went,
    Broke,
    Waiting,
    /// Not yet: the step could not do its work now and asks to be asked again.
    ///
    /// Neither of the other two, and the distance from each is why it exists.
    /// `Broke` says something went wrong and burns an attempt; `Waiting` says
    /// a person holds the step and it never becomes ready again. How long
    /// before it is asked again is `Step::ask_again_after_secs`.
    NotYet,
    Stopped,
    Skipped,
}

/// The three species of step: an action can be redone as it stands, be undone
/// and redone, or must be left to a person. There is no implicit fourth road —
/// an unknown species counts as `HandToHuman`, never as `Repeatable`, because
/// the mistake this distinction guards against is duplicating an effect the
/// world has already seen.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StepSpecies {
    /// Can be relaunched as it stands: no residual effect to undo.
    Repeatable,
    /// The effect already produced can be undone, then the step redone.
    Compensable,
    /// Neither: no automatic action is safe.
    HandToHuman,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AttemptRelation {
    SameInput,
    SameInputGatesChanged,
    DifferentInput,
}

impl StepRecord {
    #[allow(clippy::too_many_arguments)]
    pub fn started(
        run_id: impl Into<String>,
        step_id: impl Into<String>,
        attempt: u32,
        epoch: u64,
        deps: Vec<String>,
        input: Value,
        gates: Vec<String>,
        started_at: i64,
    ) -> Self {
        let input_digest = digest_input(&input);
        Self {
            run_id: run_id.into(),
            step_id: step_id.into(),
            attempt,
            epoch,
            deps,
            input_digest,
            input,
            gates,
            attempt_relation: None,
            held_by_pid: None,
            species: None,
            started_at,
            outcome: None,
            output: None,
            output_was_written: false,
            said: None,
            failure_class: None,
            refusal: None,
            ended_at: None,
            bytes_seen: None,
            bytes_discarded: None,
        }
    }
}

pub fn digest_input(value: &Value) -> String {
    let canonical = canonical_value(value);
    let bytes = serde_json::to_vec(&canonical)
        .expect("serializing a serde_json::Value in memory cannot fail");
    let digest = Sha256::digest(bytes);
    format!("{digest:x}")
}

pub fn truncate_said(value: String) -> String {
    if value.len() <= MAX_SAID_BYTES {
        return value;
    }
    let mut end = MAX_SAID_BYTES;
    while !value.is_char_boundary(end) {
        end -= 1;
    }
    value[..end].to_owned()
}

fn canonical_value(value: &Value) -> Value {
    match value {
        Value::Array(values) => Value::Array(values.iter().map(canonical_value).collect()),
        Value::Object(values) => {
            let sorted: BTreeMap<_, _> = values
                .iter()
                .map(|(key, value)| (key.clone(), canonical_value(value)))
                .collect();
            Value::Object(sorted.into_iter().collect())
        }
        other => other.clone(),
    }
}

fn required_option<'de, D, T>(deserializer: D) -> Result<Option<T>, D::Error>
where
    D: Deserializer<'de>,
    T: Deserialize<'de>,
{
    Option::<T>::deserialize(deserializer)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn digest_does_not_depend_on_object_insertion_order() {
        let first = json!({"a": 1, "b": {"c": 2, "d": 3}});
        let second = json!({"b": {"d": 3, "c": 2}, "a": 1});
        assert_eq!(digest_input(&first), digest_input(&second));
    }

    #[test]
    fn incomplete_record_is_rejected() {
        let record = StepRecord::started("run", "step", 1, 1, vec![], json!(null), vec![], 1);
        let mut value = serde_json::to_value(record).expect("serializable record");
        value
            .as_object_mut()
            .expect("the record is an object")
            .remove("gates");
        assert!(serde_json::from_value::<StepRecord>(value).is_err());
    }

    #[test]
    fn null_closing_field_must_still_be_present() {
        let record = StepRecord::started("run", "step", 1, 1, vec![], json!(null), vec![], 1);
        let mut value = serde_json::to_value(record).expect("serializable record");
        value
            .as_object_mut()
            .expect("the record is an object")
            .remove("ended_at");
        assert!(serde_json::from_value::<StepRecord>(value).is_err());
    }

    #[test]
    fn truncate_said_preserves_a_multibyte_character_boundary() {
        let value = format!("{}é", "a".repeat(MAX_SAID_BYTES - 1));
        let truncated = truncate_said(value);
        assert_eq!(truncated, "a".repeat(MAX_SAID_BYTES - 1));
    }

    /// The excerpt is bounded, or a refusal of a long answer would carry the
    /// whole answer into every row that records it.
    #[test]
    fn a_refusal_keeps_a_bounded_excerpt_cut_on_a_character_boundary() {
        let long = format!("{}é{}", "a".repeat(MAX_SEEN_BYTES - 1), "b".repeat(50));
        let refusal = Refusal::new("answer_shape", "$", RefusalRule::TooLong, &long);
        assert_eq!(refusal.seen, "a".repeat(MAX_SEEN_BYTES - 1));

        let short = Refusal::new("answer_shape", "$.verdict", RefusalRule::NotAllowed, " remvoe ");
        assert_eq!(short.seen, "remvoe");
    }

    /// A record written before refusals existed still reads, and reads back
    /// without one — an old event log is the only thing a store is rebuilt from.
    #[test]
    fn a_refusal_survives_the_record_and_an_old_record_has_none() {
        let mut record = StepRecord::started("run", "step", 1, 1, vec![], json!(null), vec![], 1);
        record.outcome = Some(Outcome::Broke);
        record.refusal = Some(Refusal::new(
            "output_schema",
            "$.count",
            RefusalRule::WrongType,
            "\"three\"",
        ));
        let text = serde_json::to_string(&record).expect("serializable record");
        let back: StepRecord = serde_json::from_str(&text).expect("readable record");
        assert_eq!(back.refusal, record.refusal);

        let mut value = serde_json::to_value(&record).expect("serializable record");
        value
            .as_object_mut()
            .expect("the record is an object")
            .remove("refusal");
        let old: StepRecord = serde_json::from_value(value).expect("an old record reads");
        assert_eq!(old.refusal, None);
    }

    #[test]
    fn legacy_record_without_byte_counts_is_accepted() {
        let json_str = r#"{
            "run_id": "run-legacy",
            "step_id": "step",
            "attempt": 1,
            "epoch": 1,
            "deps": [],
            "input_digest": "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855",
            "input": null,
            "gates": [],
            "started_at": 1,
            "outcome": "Went",
            "output": null,
            "said": null,
            "failure_class": null,
            "ended_at": 2
        }"#;
        let record: StepRecord = serde_json::from_str(json_str).expect("old record still reads");
        assert_eq!(record.bytes_seen, None);
        assert_eq!(record.bytes_discarded, None);
        // Written before species existed: it stays readable, and does not
        // inherit a species nobody declared.
        assert_eq!(record.species, None);
        assert_eq!(record.held_by_pid, None);
    }

    #[test]
    fn species_and_holder_survive_a_round_trip() {
        let mut record = StepRecord::started("run", "step", 1, 1, vec![], json!(null), vec![], 1);
        record.species = Some(StepSpecies::Compensable);
        record.held_by_pid = Some(4321);
        let text = serde_json::to_string(&record).expect("serializable record");
        assert!(
            text.contains("\"compensable\""),
            "species is written lowercase: {text}"
        );
        let back: StepRecord = serde_json::from_str(&text).expect("record reads back");
        assert_eq!(back.species, Some(StepSpecies::Compensable));
        assert_eq!(back.held_by_pid, Some(4321));
    }
}
