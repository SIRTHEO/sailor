//! `dormant_steps`: the other half of `unused_actions`. That one answers
//! "never named by any flow"; this one answers "named, but the step behind
//! its own `when` has never once let the action run" — a different fault,
//! found only by reading the ledger, not the flow files.
//!
//! **A FLOW NEVER LAUNCHED AND A FLOW WHOSE STEP NEVER CLOSES ARE TWO
//! DIFFERENT FACTS**, and this action used to say the first one once per
//! step, which is the same fault this file's own header warns against
//! conflating. A flow with four steps that has never run once said "four
//! dormant steps" — one fact, told four times, drowning the flows that
//! actually run but have one step that never does. `flow_ever_ran` is asked
//! first, per flow, and a never-run flow is named once in `never_run`; only
//! a flow that HAS run gets its steps checked for `always_skipped` or
//! `never_closed`.
//!
//! **WHAT THIS DOES NOT ANSWER.** A run closed by a person testing something
//! by hand counts the same as one an ordinary gesture started — a command
//! typed, a scheduled trigger, a heartbeat, a window. The very first render of
//! this action's own real numbers came from a throwaway flow launched by hand
//! from a terminal to see the output: by this action's own count that run
//! would read as the flow being alive, when it measured someone trying it,
//! not the product being used. `orphans_are_found_and_stopped` stayed green
//! the same way, exercised only by a test; `sailor inventory` already names
//! 31 of 59 skills unreachable by any gesture a person makes. Telling
//! "reached" from "reached only by a test or by hand" is a different, larger
//! piece of work; this action does not attempt it.

use flow::{Action, ActionError, ActionOutcome, SharedState, StepSpecies};
use ledger::{Ledger, StepReach};
use serde_json::{json, Value};
use std::path::{Path, PathBuf};

pub const DORMANT_STEPS_ACTION: &str = "dormant_steps";

/// See `unused_actions::NO_HOME_DECLARED` — the same reasoning, kept local
/// because these two actions do not otherwise share a dependency.
const NO_HOME_DECLARED: &str = "/sailor-no-home-declared";

pub fn register_dormant_steps(
    registry: &mut flow::ActionRegistry,
    home_flows: Option<PathBuf>,
    ledger: Option<Ledger>,
) {
    registry.register(
        DORMANT_STEPS_ACTION,
        DormantStepsAction::new(home_flows, ledger),
    );
}

pub struct DormantStepsAction {
    home_flows: Option<PathBuf>,
    ledger: Option<Ledger>,
}

impl DormantStepsAction {
    pub fn new(home_flows: Option<PathBuf>, ledger: Option<Ledger>) -> Self {
        Self { home_flows, ledger }
    }
}

impl Action for DormantStepsAction {
    fn execute(&self, _input: &Value, _shared: &SharedState) -> Result<ActionOutcome, ActionError> {
        let ledger = self
            .ledger
            .as_ref()
            .ok_or_else(|| ActionError::new("no_store", String::new()))?;
        let home = self
            .home_flows
            .clone()
            .unwrap_or_else(|| Path::new(NO_HOME_DECLARED).to_path_buf());
        let known = flow::system::load_all(&flow::system::sources_from_env(&home));
        let flows_unreadable = known.iter().filter(|(_, _, entry)| entry.is_err()).count();
        let mut never_run = Vec::new();
        let mut always_skipped = Vec::new();
        let mut never_closed = Vec::new();
        for (flow_id, _, entry) in &known {
            let Ok(flow) = entry else { continue };
            if !ledger
                .flow_ever_ran(flow_id)
                .map_err(|error| ActionError::new("ledger_unreadable", error.to_string()))?
            {
                never_run.push(json!({ "flow": flow_id }));
                continue;
            }
            for step in flow.graph.steps() {
                let reach = ledger
                    .step_reach(flow_id, &step.id)
                    .map_err(|error| ActionError::new("ledger_unreadable", error.to_string()))?;
                let named = json!({ "flow": flow_id, "step": step.id, "action": step.action });
                match reach {
                    StepReach::AlwaysSkipped => always_skipped.push(named),
                    // The flow has run, so `NeverClosed` here means this one
                    // step never has — not that nobody launched the flow.
                    StepReach::NeverClosed => never_closed.push(named),
                    StepReach::ReachedAtLeastOnce => {}
                }
            }
        }
        Ok(ActionOutcome::Went(json!({
            "never_run": never_run,
            "always_skipped": always_skipped,
            "never_closed": never_closed,
            "flows_unreadable": flows_unreadable,
        })))
    }

    fn species(&self) -> StepSpecies {
        StepSpecies::Repeatable
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn scratch(name: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("sailor-dormant-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("scratch dir");
        dir
    }

    fn write_flow(dir: &Path, id: &str) {
        let flow = json!({
            "id": id,
            "description": "a two-step flow, for the test alone",
            "graph": { "steps": [
                { "id": "trigger", "deps": [], "action": "trigger", "max_attempts": 1,
                  "with": { "source": "manual" },
                  "input_schema": { "type": "any" }, "output_schema": { "type": "any" } },
                { "id": "gate", "deps": ["trigger"], "action": "work_survey", "max_attempts": 1,
                  "input_schema": { "type": "any" }, "output_schema": { "type": "any" } }
            ] },
            "inputs": { "trigger": { "text": "" } }
        });
        std::fs::write(dir.join(format!("{id}.flow.json")), flow.to_string())
            .expect("write the flow");
    }

    /// **A STEP THIS FLOW HAS ONLY EVER SKIPPED IS DORMANT; ONE NO RUN HAS
    /// EVER CLOSED IS SILENCE, NOT EVIDENCE — TWO DIFFERENT ANSWERS.**
    #[test]
    fn a_step_only_ever_skipped_is_told_apart_from_one_never_run() {
        let dir = scratch("mixed");
        write_flow(&dir, "gated-flow");
        write_flow(&dir, "untouched-flow");
        let ledger = Ledger::open(dir.join("ledger")).expect("open the ledger");
        ledger
            .record_run(&ledger::RunRecord {
                run_id: "run-1".to_owned(),
                kind: "flow".to_owned(),
                entity: "gated-flow".to_owned(),
                parent_run_id: None,
                started_by: "test".to_owned(),
                status: "complete".to_owned(),
                total_cost_micros: 0,
                error: None,
                started_at: 1,
                ended_at: Some(2),
                worktree: None,
                stop_reason: None,
            })
            .expect("record the run");
        ledger
            .append_step_started(&flow::StepRecord::started(
                "run-1",
                "gate",
                1,
                7,
                vec!["trigger".to_owned()],
                json!({}),
                Vec::new(),
                100,
            ))
            .expect("write the record");
        ledger
            .close_step(
                "run-1",
                "gate",
                1,
                7,
                flow::Completion {
                    outcome: flow::Outcome::Skipped,
                    output: None,
                    said: None,
                    failure_class: None,
                    refusal: None,
                    ran: None,
                    ended_at: 2,
                    bytes_seen: None,
                    bytes_discarded: None,
                },
            )
            .expect("close the step");

        let went = DormantStepsAction::new(Some(dir.clone()), Some(ledger))
            .execute(&json!({}), &SharedState::default())
            .expect("a ledger query does not fail");
        let _ = std::fs::remove_dir_all(&dir);

        let ActionOutcome::Went(said) = went else {
            panic!("{went:?}")
        };
        let always_skipped = said["always_skipped"].as_array().expect("a list");
        let never_closed = said["never_closed"].as_array().expect("a list");
        let never_run = said["never_run"].as_array().expect("a list");
        assert!(
            always_skipped
                .iter()
                .any(|entry| entry["flow"] == "gated-flow" && entry["step"] == "gate"),
            "{said:?}"
        );
        // **A FLOW NOBODY HAS LAUNCHED IS NAMED ONCE, NOT ONCE PER STEP**:
        // it belongs in `never_run`, not spread across `never_closed`.
        assert!(
            never_run
                .iter()
                .any(|entry| entry["flow"] == "untouched-flow"),
            "{said:?}"
        );
        assert!(
            !never_closed
                .iter()
                .any(|entry| entry["flow"] == "untouched-flow"),
            "{said:?}"
        );
        assert!(
            !always_skipped
                .iter()
                .any(|entry| entry["flow"] == "untouched-flow"),
            "{said:?}"
        );
    }

    /// **A FLOW THAT RUNS, WITH ONE STEP THAT NEVER DOES, IS THE INTERESTING
    /// CASE** — told apart from a flow nobody has launched at all, which is
    /// silence about the whole flow, not evidence about one step of it.
    #[test]
    fn a_step_inside_a_running_flow_that_never_closes_is_never_closed_not_never_run() {
        let dir = scratch("running-with-a-gap");
        write_flow(&dir, "partly-live-flow");
        let ledger = Ledger::open(dir.join("ledger")).expect("open the ledger");
        ledger
            .record_run(&ledger::RunRecord {
                run_id: "run-1".to_owned(),
                kind: "flow".to_owned(),
                entity: "partly-live-flow".to_owned(),
                parent_run_id: None,
                started_by: "test".to_owned(),
                status: "complete".to_owned(),
                total_cost_micros: 0,
                error: None,
                started_at: 1,
                ended_at: Some(2),
                worktree: None,
                stop_reason: None,
            })
            .expect("record the run");
        // Only `trigger` closes; `gate` never does — the flow ran, one step
        // inside it did not.
        ledger
            .append_step_started(&flow::StepRecord::started(
                "run-1",
                "trigger",
                1,
                7,
                Vec::new(),
                json!({}),
                Vec::new(),
                100,
            ))
            .expect("write the record");
        ledger
            .close_step(
                "run-1",
                "trigger",
                1,
                7,
                flow::Completion {
                    outcome: flow::Outcome::Went,
                    output: Some(json!({})),
                    said: None,
                    failure_class: None,
                    refusal: None,
                    ran: None,
                    ended_at: 2,
                    bytes_seen: None,
                    bytes_discarded: None,
                },
            )
            .expect("close the step");

        let went = DormantStepsAction::new(Some(dir.clone()), Some(ledger))
            .execute(&json!({}), &SharedState::default())
            .expect("a ledger query does not fail");
        let _ = std::fs::remove_dir_all(&dir);

        let ActionOutcome::Went(said) = went else {
            panic!("{went:?}")
        };
        let never_closed = said["never_closed"].as_array().expect("a list");
        let never_run = said["never_run"].as_array().expect("a list");
        assert!(
            never_closed
                .iter()
                .any(|entry| entry["flow"] == "partly-live-flow" && entry["step"] == "gate"),
            "{said:?}"
        );
        assert!(
            !never_run
                .iter()
                .any(|entry| entry["flow"] == "partly-live-flow"),
            "{said:?}"
        );
    }
}
