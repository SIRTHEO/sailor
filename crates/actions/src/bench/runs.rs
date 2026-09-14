//! The two readers the baseline needs: the tasks of a frozen set laid out as
//! the items of a `for_each`, and what one run of the ledger came to.

use super::task::Task;
use flow::{Action, ActionError, ActionOutcome, RedoEvidence, SharedState, StepSpecies};
use ledger::Ledger;
use serde::Deserialize;
use serde_json::{json, Value};

pub const BENCH_TASKS_ACTION: &str = "bench_tasks";
pub const RUN_READING_ACTION: &str = "run_reading";

pub fn register_runs(registry: &mut flow::ActionRegistry, ledger: Option<Ledger>) {
    registry.register(BENCH_TASKS_ACTION, BenchTasksAction);
    registry.register(RUN_READING_ACTION, RunReadingAction { ledger });
}

fn unknown_among(declared: &Value, known: &[&str]) -> Vec<String> {
    declared
        .as_object()
        .map(|fields| {
            fields
                .keys()
                .filter(|name| !known.contains(&name.as_str()))
                .cloned()
                .collect()
        })
        .unwrap_or_default()
}

const TASKS_FIELDS: &[&str] = &["home", "set", "runs", "flow", "sailor"];

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct TasksSpec {
    home: Option<String>,
    set: Option<String>,
    runs: Option<u32>,
    /// The flow under measurement and the binary that runs it, carried on
    /// every item because a child run receives its item and nothing else.
    flow: Option<String>,
    sailor: Option<String>,
}

struct BenchTasksAction;

impl Action for BenchTasksAction {
    fn execute(&self, input: &Value, _shared: &SharedState) -> Result<ActionOutcome, ActionError> {
        let spec: TasksSpec = serde_json::from_value(input.clone())
            .map_err(|error| ActionError::new("invalid_input", error.to_string()))?;
        let home = match spec.home {
            Some(home) => std::path::PathBuf::from(home),
            None => ledger::sailor_home().ok_or_else(|| {
                ActionError::new("invalid_input", "no home to read the bench from")
            })?,
        };
        let set = spec.set.unwrap_or_else(|| "stage-1".to_owned());
        let runs = spec.runs.unwrap_or(1).max(1);
        let flow = spec.flow.unwrap_or_else(|| "sviluppa-sailor".to_owned());
        let sailor = spec.sailor.unwrap_or_else(|| "sailor".to_owned());
        let tasks = Task::load_set(&home, &set)
            .map_err(|why| ActionError::new("bench_not_read", why))?;
        let mut items: Vec<Value> = Vec::new();
        for task in &tasks {
            for run_index in 1..=runs {
                items.push(json!({
                    "task_id": task.id,
                    "task_file": Task::path_in(&home, &set, &task.id).display().to_string(),
                    "repo": task.repo,
                    "base_commit": task.base_commit,
                    "prompt": task.prompt,
                    "run_index": run_index,
                    "set": set,
                    "flow": flow,
                    "sailor": sailor,
                }));
            }
        }
        Ok(ActionOutcome::Went(json!({
            "set": set,
            "flow": flow,
            "sailor": sailor,
            "tasks": tasks.len(),
            "runs_per_task": runs,
            "items": items,
        })))
    }

    fn unknown_fields(&self, declared: &Value) -> Vec<String> {
        unknown_among(declared, TASKS_FIELDS)
    }

    fn may_spend(&self, _declared: Option<&Value>) -> bool {
        false
    }

    fn species(&self) -> StepSpecies {
        StepSpecies::Repeatable
    }

    fn redo_evidence(&self, _record: &flow::StepRecord) -> RedoEvidence {
        RedoEvidence::TouchesNothing
    }
}

const READING_FIELDS: &[&str] = &["run_id"];

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct ReadingSpec {
    run_id: Option<String>,
}

struct RunReadingAction {
    ledger: Option<Ledger>,
}

/// What a run came to, read from the ledger: its status, what it and its
/// children spent, how many turns its calls took, how long it ran, and the
/// steps that broke with their class.
pub fn run_reading(ledger: &Ledger, run_id: &str) -> Result<Value, ActionError> {
    let store = |error: ledger::LedgerError| ActionError::new("store_failed", error.to_string());
    // A run the ledger does not hold is an answer the baseline records, not a
    // fault that stops the batch behind it.
    let Some(header) = ledger.run_header(run_id).map_err(store)? else {
        return Ok(json!({ "run_id": run_id, "found": false }));
    };
    let spend = ledger.spent_in_run_and_below(run_id).map_err(store)?;
    let escaped = run_id.replace('\'', "''");
    let turns = ledger
        .browse(
            &format!(
                "WITH RECURSIVE below(run_id) AS (
                     SELECT '{escaped}'
                     UNION
                     SELECT runs.run_id FROM runs JOIN below ON runs.parent_run_id = below.run_id
                 )
                 SELECT COALESCE(SUM(CAST(turns AS INTEGER)), 0), COUNT(*)
                 FROM model_calls WHERE run_id IN (SELECT run_id FROM below)"
            ),
            1,
        )
        .map_err(store)?;
    let (turns, calls) = turns
        .rows
        .first()
        .map(|row| (row[0].as_i64().unwrap_or(0), row[1].as_i64().unwrap_or(0)))
        .unwrap_or((0, 0));
    let steps = ledger.steps(run_id).map_err(store)?;
    let broken: Vec<Value> = steps
        .iter()
        .filter(|step| step.outcome == Some(flow::Outcome::Broke))
        .map(|step| {
            json!({
                "step_id": step.step_id,
                "failure_class": step.failure_class,
                "said": step.said.as_deref().map(|said| said.chars().take(400).collect::<String>()),
            })
        })
        .collect();
    let wall_secs = header
        .ended_at
        .map(|ended| (ended - header.started_at).max(0));
    Ok(json!({
        "run_id": run_id,
        "found": true,
        "flow": header.entity,
        "status": header.status,
        "stop_reason": header.stop_reason,
        "error": header.error,
        "started_at": header.started_at,
        "ended_at": header.ended_at,
        "wall_secs": wall_secs,
        "cost_micros": spend.micros,
        "calls": calls,
        "calls_without_cost": spend.calls_without_cost,
        "turns": turns,
        "broken_steps": broken,
    }))
}

impl Action for RunReadingAction {
    fn execute(&self, input: &Value, _shared: &SharedState) -> Result<ActionOutcome, ActionError> {
        let spec: ReadingSpec = serde_json::from_value(input.clone())
            .map_err(|error| ActionError::new("invalid_input", error.to_string()))?;
        let run_id = spec.run_id.unwrap_or_default();
        if run_id.trim().is_empty() {
            return Ok(ActionOutcome::Went(json!({ "run_id": "", "found": false })));
        }
        let Some(ledger) = &self.ledger else {
            return Err(ActionError::new(
                "no_store",
                "reading a run needs the store it is written in, and this run has none",
            ));
        };
        run_reading(ledger, &run_id).map(ActionOutcome::Went)
    }

    fn unknown_fields(&self, declared: &Value) -> Vec<String> {
        unknown_among(declared, READING_FIELDS)
    }

    fn may_spend(&self, _declared: Option<&Value>) -> bool {
        false
    }

    fn species(&self) -> StepSpecies {
        StepSpecies::Repeatable
    }

    fn redo_evidence(&self, _record: &flow::StepRecord) -> RedoEvidence {
        RedoEvidence::TouchesNothing
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ledger::{EngineIdentity, ModelCallRecord, RunRecord};

    fn scratch(name: &str) -> std::path::PathBuf {
        let path = std::env::temp_dir().join(format!("sailor-bench-runs-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&path);
        std::fs::create_dir_all(&path).expect("a scratch directory");
        path
    }

    fn a_task(id: &str) -> Task {
        Task {
            id: id.to_owned(),
            repo: "/repo".to_owned(),
            base_commit: format!("base-{id}"),
            fix_commit: format!("fix-{id}"),
            prompt: format!("prompt of {id}"),
            prompt_source: "commit body".to_owned(),
            test_files: Vec::new(),
            hidden_test_patch: String::new(),
            gold_patch: String::new(),
            gold_added_lines: 0,
            test_command: vec!["true".to_owned()],
            validated: None,
        }
    }

    #[test]
    fn the_tasks_of_a_set_become_one_item_per_task_and_run() {
        let dir = scratch("tasks");
        for id in ["b", "a"] {
            a_task(id).save(&Task::path_in(&dir, "stage-1", id)).unwrap();
        }
        let said = BenchTasksAction
            .execute(
                &json!({"home": dir.display().to_string(), "set": "stage-1", "runs": 2, "flow": "develop", "sailor": "/bin/sailor"}),
                &SharedState::new(),
            )
            .unwrap();
        let ActionOutcome::Went(said) = said else { panic!("{said:?}") };
        assert_eq!(said["tasks"], 2);
        let items = said["items"].as_array().unwrap();
        assert_eq!(items.len(), 4);
        assert_eq!(items[0]["task_id"], "a");
        assert_eq!(items[0]["run_index"], 1);
        assert_eq!(items[1]["run_index"], 2);
        assert_eq!(items[2]["task_id"], "b");
        assert_eq!(items[0]["prompt"], "prompt of a");
        assert_eq!(items[0]["base_commit"], "base-a");
        assert!(items[0]["task_file"].as_str().unwrap().ends_with("stage-1/a.json"));
        assert_eq!(items[3]["flow"], "develop");
        assert_eq!(items[3]["sailor"], "/bin/sailor");
        assert_eq!(said["flow"], "develop");
        let _ = std::fs::remove_dir_all(&dir);
    }

    fn call(run_id: &str, id: &str, cost: i64, turns: u64) -> ModelCallRecord {
        ModelCallRecord {
            call_id: id.to_owned(),
            run_id: run_id.to_owned(),
            step_id: Some("ask".to_owned()),
            purpose: "external_engine".to_owned(),
            cli: "engine-a".to_owned(),
            requested_model: String::new(),
            actual_model: String::new(),
            input_tokens: Some(1),
            output_tokens: Some(1),
            cached_tokens: None,
            cache_write_tokens: None,
            cache_write_long_tokens: None,
            total_tokens: None,
            turns: Some(turns),
            cost_micros: Some(cost),
            declared_cost_micros: None,
            price_currency: None,
            input_price_micros_per_million: None,
            output_price_micros_per_million: None,
            cached_price_micros_per_million: None,
            cache_write_price_micros_per_million: None,
            cache_write_long_price_micros_per_million: None,
            engine_identity: EngineIdentity::default(),
            retry_chain: Vec::new(),
            error_type: None,
            started_at: 100,
            ended_at: Some(110),
            session_id: None,
            work_kind: None,
            fell_back_from: Vec::new(),
            session_mode: None,
            role: None,
            role_resolved_to: Vec::new(),
        }
    }

    fn run(run_id: &str, parent: Option<&str>, status: &str) -> RunRecord {
        RunRecord {
            run_id: run_id.to_owned(),
            kind: "flow".to_owned(),
            entity: "develop".to_owned(),
            parent_run_id: parent.map(str::to_owned),
            started_by: "test".to_owned(),
            status: status.to_owned(),
            total_cost_micros: 0,
            error: None,
            started_at: 1000,
            ended_at: Some(1300),
            worktree: None,
            stop_reason: None,
        }
    }

    #[test]
    fn a_run_is_read_with_its_children_spend_turns_and_broken_steps() {
        let dir = scratch("reading");
        let ledger = Ledger::open(dir.join("ledger")).unwrap();
        ledger.record_run(&run("parent", None, "failed")).unwrap();
        ledger.record_run(&run("child", Some("parent"), "complete")).unwrap();
        ledger.record_model_call(&call("parent", "c1", 5, 3)).unwrap();
        ledger.record_model_call(&call("child", "c2", 7, 4)).unwrap();
        let step = flow::StepRecord::started("parent", "prove", 1, 1, vec![], json!({}), vec![], 1000);
        ledger.append_step_started(&step).unwrap();
        ledger
            .close_step(
                "parent",
                "prove",
                1,
                1,
                flow::Completion {
                    outcome: flow::Outcome::Broke,
                    output: None,
                    said: Some("cargo exited 101".to_owned()),
                    failure_class: Some("engine_exit_error".to_owned()),
                    refusal: None,
                    ran: None,
                    ended_at: 1200,
                    bytes_seen: None,
                    bytes_discarded: None,
                },
            )
            .unwrap();

        let said = run_reading(&ledger, "parent").unwrap();
        assert_eq!(said["status"], "failed");
        assert_eq!(said["cost_micros"], 12);
        assert_eq!(said["calls"], 2);
        assert_eq!(said["turns"], 7);
        assert_eq!(said["wall_secs"], 300);
        assert_eq!(said["broken_steps"][0]["step_id"], "prove");
        assert_eq!(said["broken_steps"][0]["failure_class"], "engine_exit_error");
        assert_eq!(said["found"], true);
        let nobody = run_reading(&ledger, "nobody").unwrap();
        assert_eq!(nobody["found"], false);
        assert_eq!(nobody["run_id"], "nobody");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
