//! A local choice among declared options (ADR-017). The step writes one typed
//! decision row — `state`, `question`, `options` — for a scorer that reads the
//! options' probabilities in one forward pass, and reads back its row:
//! `option_ids`, `probabilities`, `prompt_sha256`, `model`. The threshold is
//! the step's: below it the step abstains. `chose`, `abstained` and `failed`
//! are data a later step branches on, never a broken step.

use crate::process::{
    run_shell_check_watched, sink_for_step, CheckInvocation, CheckResult, Pipe, StepSinks,
};
use flow::{Action, ActionError, ActionOutcome, Ran, SharedState, StepSpecies};
use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Arc;
use std::time::Duration;

pub const LOCAL_CHOICE_ACTION: &str = "local_choice";

/// Where the command reads the decision row, and where it writes the scored one.
pub const DECISION_INPUT_VARIABLE: &str = "SAILOR_DECISION_INPUT";
pub const DECISION_OUTPUT_VARIABLE: &str = "SAILOR_DECISION_OUTPUT";

pub fn register_local_choice(
    registry: &mut flow::ActionRegistry,
    watcher: Option<Arc<dyn StepSinks>>,
) {
    registry.register(LOCAL_CHOICE_ACTION, LocalChoiceAction { watcher });
}

#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
pub struct DeclaredOption {
    pub id: String,
    pub description: String,
}

#[derive(Debug, Deserialize)]
struct ChoiceSpec {
    command: String,
    evidence: Value,
    question: String,
    options: Vec<DeclaredOption>,
    threshold: f64,
    timeout_secs: u64,
    #[serde(default)]
    env: BTreeMap<String, String>,
    #[serde(default)]
    workdir: Option<String>,
    #[serde(flatten)]
    extra: BTreeMap<String, Value>,
}

pub struct LocalChoiceAction {
    watcher: Option<Arc<dyn StepSinks>>,
}

impl Action for LocalChoiceAction {
    /// A command of this machine: no turn, nothing on the metered bill.
    fn may_spend(&self, _declared: Option<&Value>) -> bool {
        false
    }

    fn unknown_fields(&self, declared: &Value) -> Vec<String> {
        match serde_json::from_value::<ChoiceSpec>(declared.clone()) {
            Ok(spec) => spec.extra.into_keys().collect(),
            Err(_) => Vec::new(),
        }
    }

    fn execute(&self, input: &Value, shared: &SharedState) -> Result<ActionOutcome, ActionError> {
        self.execute_and_report(input, shared)
            .map(|(outcome, _)| outcome)
    }

    fn execute_and_report(
        &self,
        input: &Value,
        shared: &SharedState,
    ) -> Result<(ActionOutcome, Option<Ran>), ActionError> {
        let spec: ChoiceSpec = serde_json::from_value(input.clone())
            .map_err(|error| ActionError::new("invalid_input", error.to_string()))?;
        if spec.options.len() < 2 {
            return Err(ActionError::new(
                "invalid_input",
                "a choice needs at least two declared options",
            ));
        }
        let place = Scratch::new()?;
        let row = decision_row(&spec.evidence, &spec.question, &spec.options);
        std::fs::write(&place.input, format!("{row}\n"))
            .map_err(|error| ActionError::new("scratch_unwritable", error.to_string()))?;
        let mut env = spec.env.clone();
        env.insert(DECISION_INPUT_VARIABLE.to_owned(), path_text(&place.input));
        env.insert(DECISION_OUTPUT_VARIABLE.to_owned(), path_text(&place.output));
        let invocation = CheckInvocation {
            command: spec.command.clone(),
            env,
            timeout: Duration::from_secs(spec.timeout_secs),
            workdir: spec.workdir.clone(),
        };
        let ran = invocation.ran();
        let live = sink_for_step(&self.watcher, shared);
        if let Some(live) = live.as_deref() {
            live.chunk(
                Pipe::Stderr,
                format!("[sailor] {}\n", ran.announce()).as_bytes(),
            );
        }
        let answer = run_shell_check_watched(&invocation, live.as_deref());
        let scored = std::fs::read_to_string(&place.output).unwrap_or_default();
        let read = read_choice(&answer, &scored, &spec.options, spec.threshold);
        Ok((ActionOutcome::Went(read), Some(ran)))
    }

    fn species(&self) -> StepSpecies {
        StepSpecies::Repeatable
    }

    fn redo_evidence(&self, _record: &flow::StepRecord) -> flow::RedoEvidence {
        flow::RedoEvidence::TouchesNothing
    }
}

/// The row a scorer reads. A structured state stays structured.
pub fn decision_row(evidence: &Value, question: &str, options: &[DeclaredOption]) -> Value {
    json!({
        "id": "choice",
        "state": evidence,
        "question": question,
        "options": options,
    })
}

/// The scored row held to what ADR-017 asks a choice to publish. What it does
/// not publish turns a winner into an abstention.
pub fn read_choice(
    answer: &CheckResult,
    scored: &str,
    declared: &[DeclaredOption],
    threshold: f64,
) -> Value {
    let declared_ids: Vec<&str> = declared.iter().map(|option| option.id.as_str()).collect();
    let mut reading = Map::new();
    reading.insert("offered".into(), json!(declared_ids));
    reading.insert("threshold".into(), json!(threshold));
    let failure = match answer {
        CheckResult::Passed { .. } => None,
        CheckResult::Failed { code, stderr, .. } => Some(format!(
            "exit {}: {}",
            code.map_or("?".to_owned(), |code| code.to_string()),
            first_line(stderr)
        )),
        CheckResult::TimedOut => Some("timed out".to_owned()),
    };
    if let Some(failure) = failure {
        return with(reading, "failed", None, Some(&failure));
    }
    let Some(row) = last_json_object(scored) else {
        return with(reading, "failed", None, Some("the scorer wrote no row"));
    };
    let ids: Vec<&str> = row
        .get("option_ids")
        .and_then(Value::as_array)
        .map(|all| all.iter().filter_map(Value::as_str).collect())
        .unwrap_or_default();
    let probabilities: Vec<f64> = row
        .get("probabilities")
        .and_then(Value::as_array)
        .map(|all| all.iter().filter_map(Value::as_f64).collect())
        .unwrap_or_default();
    let version = row.get("prompt_sha256").and_then(Value::as_str);
    let model = row.get("model").and_then(model_named);
    for (key, value) in [("version", version.map(str::to_owned)), ("model", model)] {
        if let Some(value) = value {
            reading.insert(key.into(), json!(value));
        }
    }
    let scores: Vec<(&str, f64)> = ids.iter().copied().zip(probabilities.iter().copied()).collect();
    let Some((best, probability)) = scores
        .iter()
        .copied()
        .max_by(|left, right| left.1.total_cmp(&right.1))
        .filter(|_| ids.len() == probabilities.len())
    else {
        return with(reading, "failed", None, Some("the row carries no scores"));
    };
    let by_option: Map<String, Value> = scores
        .iter()
        .map(|(id, p)| ((*id).to_owned(), json!(p)))
        .collect();
    reading.insert("probabilities".into(), Value::Object(by_option));
    reading.insert("probability".into(), json!(probability));
    let why = if version.is_none() {
        "the scorer names no prompt hash"
    } else if !same_options(&declared_ids, &ids) {
        "the options scored are not the ones this step declares"
    } else if probability < threshold {
        "below the threshold"
    } else {
        return with(reading, "chose", Some(best), None);
    };
    with(reading, "abstained", None, Some(why))
}

fn model_named(model: &Value) -> Option<String> {
    let source = model.get("source").and_then(Value::as_str)?;
    let revision = model
        .get("revision")
        .or_else(|| model.get("gguf_sha256"))
        .and_then(Value::as_str);
    Some(match revision {
        Some(revision) => format!("{source}@{revision}"),
        None => source.to_owned(),
    })
}

fn with(
    mut reading: Map<String, Value>,
    outcome: &str,
    chosen: Option<&str>,
    why: Option<&str>,
) -> Value {
    reading.insert("outcome".into(), json!(outcome));
    if let Some(chosen) = chosen {
        reading.insert("chosen".into(), json!(chosen));
    }
    if let Some(why) = why {
        reading.insert("why".into(), json!(why));
    }
    Value::Object(reading)
}

fn same_options(declared: &[&str], scored: &[&str]) -> bool {
    let mut declared = declared.to_vec();
    let mut scored = scored.to_vec();
    declared.sort_unstable();
    scored.sort_unstable();
    declared == scored
}

fn last_json_object(text: &str) -> Option<Map<String, Value>> {
    text.lines()
        .rev()
        .find_map(|line| match serde_json::from_str(line.trim()) {
            Ok(Value::Object(object)) => Some(object),
            _ => None,
        })
}

fn first_line(text: &str) -> &str {
    text.lines()
        .find(|line| !line.trim().is_empty())
        .unwrap_or("")
        .trim()
}

fn path_text(path: &Path) -> String {
    path.to_string_lossy().into_owned()
}

static NEXT_SCRATCH: AtomicU32 = AtomicU32::new(0);

/// The two files of one choice, gone when the step is.
struct Scratch {
    dir: PathBuf,
    input: PathBuf,
    output: PathBuf,
}

impl Scratch {
    fn new() -> Result<Self, ActionError> {
        let dir = std::env::temp_dir().join(format!(
            "sailor-choice-{}-{}",
            std::process::id(),
            NEXT_SCRATCH.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir_all(&dir)
            .map_err(|error| ActionError::new("scratch_unwritable", error.to_string()))?;
        Ok(Self {
            input: dir.join("decision.jsonl"),
            output: dir.join("scored.jsonl"),
            dir,
        })
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

#[cfg(test)]
mod tests;
