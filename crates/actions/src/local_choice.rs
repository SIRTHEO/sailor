//! A local choice among declared options (ADR-017): a command of this machine
//! reads the evidence against a versioned table and answers which option won,
//! with the probability it won by. The step never fails on the answer: `chose`,
//! `abstained` and `failed` are data a later step branches on.

use crate::process::{run_shell_check_watched, sink_for_step, CheckInvocation, CheckResult, Pipe, StepSinks};
use flow::{Action, ActionError, ActionOutcome, Ran, SharedState, StepSpecies};
use serde::Deserialize;
use serde_json::{json, Map, Value};
use std::collections::BTreeMap;
use std::sync::Arc;
use std::time::Duration;

pub const LOCAL_CHOICE_ACTION: &str = "local_choice";

/// The variable the evidence reaches the command through: inside the
/// environment it stays data, as `ShellCheckAction` requires of any answer.
pub const EVIDENCE_VARIABLE: &str = "SAILOR_EVIDENCE";

/// The exit a decider gives when it looked and stayed below its threshold.
const ABSTAINED_EXIT: i32 = 3;

pub fn register_local_choice(registry: &mut flow::ActionRegistry, watcher: Option<Arc<dyn StepSinks>>) {
    registry.register(LOCAL_CHOICE_ACTION, LocalChoiceAction { watcher });
}

#[derive(Debug, Deserialize)]
struct ChoiceSpec {
    command: String,
    evidence: Value,
    /// The options the flow branches on. Declared, an answer offering another
    /// set abstains: a winner no branch expects would run nothing downstream.
    #[serde(default)]
    options: Option<Vec<String>>,
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
        self.execute_and_report(input, shared).map(|(outcome, _)| outcome)
    }

    fn execute_and_report(
        &self,
        input: &Value,
        shared: &SharedState,
    ) -> Result<(ActionOutcome, Option<Ran>), ActionError> {
        let spec: ChoiceSpec = serde_json::from_value(input.clone())
            .map_err(|error| ActionError::new("invalid_input", error.to_string()))?;
        let evidence = match &spec.evidence {
            Value::String(text) => text.clone(),
            other => other.to_string(),
        };
        let mut env = spec.env.clone();
        env.insert(EVIDENCE_VARIABLE.to_owned(), evidence);
        let invocation = CheckInvocation {
            command: spec.command.clone(),
            env,
            timeout: Duration::from_secs(spec.timeout_secs),
            workdir: spec.workdir.clone(),
        };
        let ran = invocation.ran();
        let live = sink_for_step(&self.watcher, shared);
        if let Some(live) = live.as_deref() {
            live.chunk(Pipe::Stderr, format!("[sailor] {}\n", ran.announce()).as_bytes());
        }
        let answer = run_shell_check_watched(&invocation, live.as_deref());
        Ok((ActionOutcome::Went(read_choice(&answer, spec.options.as_deref())), Some(ran)))
    }

    fn species(&self) -> StepSpecies {
        StepSpecies::Repeatable
    }

    fn redo_evidence(&self, _record: &flow::StepRecord) -> flow::RedoEvidence {
        flow::RedoEvidence::TouchesNothing
    }
}

/// What the decider said, held to the five conditions a choice must publish.
/// Anything it did not publish turns a `chose` into an abstention.
pub fn read_choice(answer: &CheckResult, declared: Option<&[String]>) -> Value {
    let (code, stdout, stderr) = match answer {
        CheckResult::Passed { stdout } => (0, stdout.as_str(), ""),
        CheckResult::Failed { code, stdout, stderr } => (code.unwrap_or(-1), stdout.as_str(), stderr.as_str()),
        CheckResult::TimedOut => return failed("timed_out"),
    };
    let Some(said) = last_json_object(stdout) else {
        return failed(&format!("no answer on stdout (exit {code}): {}", first_line(stderr)));
    };
    let offered: Vec<String> = said
        .get("offered")
        .and_then(Value::as_array)
        .map(|all| all.iter().filter_map(Value::as_str).map(str::to_owned).collect())
        .unwrap_or_default();
    let chosen = said.get("chosen").and_then(Value::as_str);
    let probability = said.get("probability").and_then(Value::as_f64);
    let threshold = said.get("threshold").and_then(Value::as_f64);
    let version = said.get("version").and_then(Value::as_str).filter(|version| !version.is_empty() && *version != "?");
    let mut reading = Map::new();
    reading.insert("offered".into(), json!(offered));
    for (key, value) in [("probability", probability.map(Value::from)), ("threshold", threshold.map(Value::from)), ("version", version.map(Value::from))] {
        if let Some(value) = value {
            reading.insert(key.into(), value);
        }
    }
    let why = if code != 0 && code != ABSTAINED_EXIT {
        return with(reading, "failed", None, Some(&format!("exit {code}: {}", first_line(stderr))));
    } else if code == ABSTAINED_EXIT || said.get("outcome").and_then(Value::as_str) != Some("chose") {
        "below its threshold"
    } else if version.is_none() {
        "the table carries no version"
    } else if !matches!((probability, threshold), (Some(p), Some(t)) if p >= t) {
        "no probability at or above a threshold"
    } else if !chosen.is_some_and(|chosen| offered.iter().any(|option| option == chosen)) {
        "the winner is not among the options offered"
    } else if declared.is_some_and(|declared| !same_options(declared, &offered)) {
        "the options offered are not the ones this step declares"
    } else {
        return with(reading, "chose", chosen, None);
    };
    with(reading, "abstained", None, Some(why))
}

fn with(mut reading: Map<String, Value>, outcome: &str, chosen: Option<&str>, why: Option<&str>) -> Value {
    reading.insert("outcome".into(), json!(outcome));
    if let Some(chosen) = chosen {
        reading.insert("chosen".into(), json!(chosen));
    }
    if let Some(why) = why {
        reading.insert("why".into(), json!(why));
    }
    Value::Object(reading)
}

fn failed(why: &str) -> Value {
    with(Map::new(), "failed", None, Some(why))
}

fn same_options(declared: &[String], offered: &[String]) -> bool {
    let mut declared = declared.to_vec();
    let mut offered = offered.to_vec();
    declared.sort();
    offered.sort();
    declared == offered
}

fn last_json_object(stdout: &str) -> Option<Map<String, Value>> {
    stdout.lines().rev().find_map(|line| match serde_json::from_str(line.trim()) {
        Ok(Value::Object(object)) => Some(object),
        _ => None,
    })
}

fn first_line(text: &str) -> &str {
    text.lines().find(|line| !line.trim().is_empty()).unwrap_or("").trim()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn said(code: i32, line: &str) -> CheckResult {
        match code {
            0 => CheckResult::Passed { stdout: format!("{line}\n") },
            _ => CheckResult::Failed { code: Some(code), stdout: format!("{line}\n"), stderr: String::new() },
        }
    }

    const WON: &str = r#"{"outcome":"chose","chosen":"Plan","probability":0.91,"threshold":0.7,"offered":["Explore","Plan"],"version":"agents-v1"}"#;

    #[test]
    fn a_choice_that_publishes_everything_is_taken() {
        let read = read_choice(&said(0, WON), None);
        assert_eq!(read["outcome"], "chose");
        assert_eq!(read["chosen"], "Plan");
        assert_eq!(read["version"], "agents-v1");
    }

    #[test]
    fn a_choice_that_leaves_out_what_it_decided_by_abstains() {
        for broken in [
            WON.replace(r#","version":"agents-v1""#, ""),
            WON.replace("0.91", "0.5"),
            WON.replace(r#""chosen":"Plan""#, r#""chosen":"Deploy""#),
        ] {
            let read = read_choice(&said(0, &broken), None);
            assert_eq!(read["outcome"], "abstained", "{broken}");
            assert!(read.get("chosen").is_none(), "an abstention hands on no winner");
        }
    }

    #[test]
    fn options_the_step_did_not_declare_are_not_a_choice() {
        let declared = ["Explore".to_owned(), "Plan".to_owned(), "Deploy".to_owned()];
        assert_eq!(read_choice(&said(0, WON), Some(&declared))["outcome"], "abstained");
        assert_eq!(read_choice(&said(0, WON), Some(&declared[..2]))["outcome"], "chose");
    }

    #[test]
    fn a_decider_below_its_threshold_or_down_is_data_not_an_error() {
        let below = WON.replace(r#""outcome":"chose""#, r#""outcome":"abstained""#);
        assert_eq!(read_choice(&said(3, &below), None)["outcome"], "abstained");
        assert_eq!(read_choice(&said(4, r#"{"outcome":"unavailable"}"#), None)["outcome"], "failed");
        assert_eq!(read_choice(&said(0, "not json"), None)["outcome"], "failed");
        assert_eq!(read_choice(&CheckResult::TimedOut, None)["outcome"], "failed");
    }

    #[test]
    fn the_evidence_reaches_the_command_as_data() {
        let action = LocalChoiceAction { watcher: None };
        let input = json!({
            "command": r#"printf '{"outcome":"chose","chosen":"%s","probability":1,"threshold":0.5,"offered":["a","b"],"version":"t1"}' "$SAILOR_EVIDENCE""#,
            "evidence": "b",
            "timeout_secs": 5
        });
        let ActionOutcome::Went(read) = action.execute(&input, &SharedState::new()).expect("a choice never fails its step") else {
            panic!("a choice that ran is Went");
        };
        assert_eq!((read["outcome"].as_str(), read["chosen"].as_str()), (Some("chose"), Some("b")));
    }
}
