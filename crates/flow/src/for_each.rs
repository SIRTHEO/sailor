//! The step that runs a flow once per element of a list. It is `subflow`
//! repeated, and shares its host, its ledger rows, its call chain and its cap:
//! what it adds is the loop, opened as wide as the executor's own front and no
//! wider, and an output that keeps the elements' order whatever order the
//! children finished in.

use crate::subflow::{self, ChildEnd, Located, Prepared, SubflowHost};
use crate::{Action, ActionError, ActionOutcome, SharedState, StepSpecies, AT_ONCE};
use serde::Deserialize;
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::sync::Arc;

/// The name a step uses to run a flow for every element of a list.
pub const FOR_EACH_ACTION: &str = "for_each";

/// The fields the step knows. For `flow check`, not for execution.
const KNOWN_FIELDS: &[&str] = &["flow", "items", "inputs"];

/// What the step declares. `items` arrives already a list: a `$from` written
/// in the flow file is replaced by the executor before any action reads its
/// input, so a pointer and a literal list look the same from here.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct Repeat {
    /// The flow's name, as it reads on disk without `.flow.json`.
    pub flow: String,
    /// One child run per element, in this order.
    pub items: Vec<Value>,
    /// The `root_inputs` the step imposes on every child, key by key.
    #[serde(default)]
    pub inputs: BTreeMap<String, Value>,
}

/// The step that runs a flow for each element of a list.
pub struct ForEachAction {
    host: Arc<dyn SubflowHost>,
}

impl ForEachAction {
    pub fn new(host: Arc<dyn SubflowHost>) -> Self {
        Self { host }
    }
}

impl Action for ForEachAction {
    fn execute(&self, input: &Value, shared: &SharedState) -> Result<ActionOutcome, ActionError> {
        let call: Repeat = serde_json::from_value(input.clone()).map_err(|error| {
            ActionError::new(
                "invalid_for_each_call",
                format!("the step does not declare a flow and a list to run it for: {error}"),
            )
        })?;
        let Prepared {
            caller,
            located,
            store,
        } = subflow::prepare(self.host.as_ref(), shared, &call.flow)?;
        if call.items.is_empty() {
            return Ok(ActionOutcome::Went(json!({ "items": [], "runs": [] })));
        }

        let count = call.items.len();
        let mut ended: Vec<Option<Result<ChildEnd, ActionError>>> =
            (0..count).map(|_| None).collect();
        // In groups of the executor's width, for the executor's reasons: the
        // elements are paid conversations, not sums. A group with a failure in
        // it is the last one opened — the elements after it are never started,
        // which is what the executor does with the fronts after a broken one.
        let mut first = 0;
        for group in call.items.chunks(AT_ONCE) {
            let host = self.host.as_ref();
            let (caller, located, store) = (&caller, &located, store.as_ref());
            let ends: Vec<Result<ChildEnd, ActionError>> = std::thread::scope(|scope| {
                let handles: Vec<_> = group
                    .iter()
                    .enumerate()
                    .map(|(offset, element)| {
                        let nth = first + offset;
                        let root_inputs = inputs_for(located, &call, element);
                        scope.spawn(move || {
                            let run_id = subflow::child_run_id(
                                &caller.parent_run,
                                &format!("{}::{nth}", caller.parent_step),
                            )?;
                            subflow::run_child(host, caller, located, store, root_inputs, run_id)
                        })
                    })
                    .collect();
                handles
                    .into_iter()
                    .enumerate()
                    .map(|(offset, handle)| {
                        handle.join().unwrap_or_else(|_| Err(died(first + offset)))
                    })
                    .collect()
            });
            let failed = ends.iter().any(Result::is_err);
            for (offset, end) in ends.into_iter().enumerate() {
                ended[first + offset] = Some(end);
            }
            if failed {
                break;
            }
            first += group.len();
        }

        // The first failure by index names the element; only when nobody
        // failed does a child still open make the whole step wait.
        for (nth, end) in ended.iter().enumerate() {
            if let Some(Err(error)) = end {
                return Err(child_failed(nth, count, error));
            }
        }
        for (nth, end) in ended.iter().enumerate() {
            if let Some(Ok(end)) = end {
                if end.status == "waiting" || end.status == "not_yet" {
                    return Ok(still_open(nth, count, &call.flow, end));
                }
            }
        }
        let mut items = Vec::with_capacity(count);
        let mut runs = Vec::with_capacity(count);
        for end in ended.into_iter().flatten().flatten() {
            runs.push(Value::String(end.run_id.clone()));
            items.push(subflow::went_output(&located, &end));
        }
        Ok(ActionOutcome::Went(json!({ "items": items, "runs": runs })))
    }

    fn unknown_fields(&self, declared: &Value) -> Vec<String> {
        let Some(object) = declared.as_object() else {
            return Vec::new();
        };
        object
            .keys()
            .filter(|name| !KNOWN_FIELDS.contains(&name.as_str()))
            .cloned()
            .collect()
    }

    /// Handed to a person, for the reason `subflow` gives and once per element.
    fn species(&self) -> StepSpecies {
        StepSpecies::HandToHuman
    }
}

/// The element is what the child's root steps receive. The step's `inputs`
/// win over it and over the file, key by key, as they do in `subflow`.
fn inputs_for(located: &Located, call: &Repeat, element: &Value) -> BTreeMap<String, Value> {
    let mut root_inputs = located.flow.inputs.clone();
    for step in located.flow.graph.steps() {
        if step.deps.is_empty() {
            root_inputs.insert(step.id.clone(), element.clone());
        }
    }
    root_inputs.extend(call.inputs.clone());
    root_inputs
}

fn child_failed(nth: usize, count: usize, error: &ActionError) -> ActionError {
    ActionError::new(
        "for_each_child_failed",
        catalogue::say(
            "flow.for_each.child_failed",
            &[
                ("index", &nth.to_string()),
                ("count", &count.to_string()),
                ("why", &error.to_string()),
            ],
        ),
    )
}

fn died(nth: usize) -> ActionError {
    ActionError::new(
        "for_each_child_failed",
        catalogue::say("flow.for_each.child_died", &[("index", &nth.to_string())]),
    )
}

fn still_open(nth: usize, count: usize, flow: &str, end: &ChildEnd) -> ActionOutcome {
    let values = [
        ("index", nth.to_string()),
        ("count", count.to_string()),
        ("flow", flow.to_owned()),
        ("run_id", end.run_id.clone()),
    ];
    let values: Vec<(&str, &str)> = values
        .iter()
        .map(|(name, value)| (*name, value.as_str()))
        .collect();
    match end.status {
        "waiting" => ActionOutcome::Waiting(catalogue::say("flow.for_each.child_waiting", &values)),
        _ => ActionOutcome::NotYet(catalogue::say("flow.for_each.child_not_yet", &values)),
    }
}
