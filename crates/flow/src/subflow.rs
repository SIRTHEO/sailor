//! The step that runs another flow. The decision "flows compose, they do not
//! merge" is in `docs/decisioni.md`; each invariant sits next to what enforces
//! it — [`system::sources`] for precedence, [`call_cycle`] and [`CALL_CHAIN`]
//! for recursion, [`MAX_DEPTH`] for depth, [`tightest`] and [`remaining_of`]
//! for the cap and for what it does not promise.

use crate::system::{self, FlowSource};
use crate::{
    Action, ActionError, ActionOutcome, ActionRegistry, Clock, Execution, ExecutionRequest,
    Executor, FlowFile, Graph, InProcessExecutor, Outcome, RecordStore, SharedState, StepRecord,
    StepSpecies, SystemClock, CURRENT_CAP, CURRENT_RUN, CURRENT_STEP,
};
use serde::Deserialize;
use serde_json::{json, Map, Value};
use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

/// The name a step uses to ask for another flow to be run.
///
/// This is what the window has always written: `desktop/src/flow.ts` maps
/// `subflow` onto the node family of the same name and offers it in the step
/// palette, so changing it here breaks steps already drawn.
pub const SUBFLOW_ACTION: &str = "subflow";

/// The key under which the chain of already-stacked flows travels.
///
/// It carries the names, not a counter: a number would say "too deep" and
/// nothing more, while the chain lets the error *name* who calls whom, which
/// is the only form in which a person can break the loop. The `flow.` prefix
/// belongs to the executor, as for [`CURRENT_RUN`]: a flow never writes there.
pub const CALL_CHAIN: &str = "flow.subflow.chain";

/// How many flows may be stacked, counting the first one called.
///
/// Not a limit of the machine — the stack would hold many more; what will not
/// hold is the person reading. Four is the depth this house actually composes:
/// research, dispatch, development, interrogation. A fifth level is, today,
/// more likely a typo than a design. Whoever needs one raises this and says why.
pub const MAX_DEPTH: usize = 4;

/// The fields the step knows. For `flow check`, not for execution.
const KNOWN_FIELDS: &[&str] = &["flow", "inputs"];

/// What the step declares: which flow, and with what inputs.
///
/// No `deny_unknown_fields`, and that is not an oversight: at run time a step's
/// input is its dependency's output, where foreign fields are the norm. The
/// strictness sits on what a person writes by hand, in
/// [`SubflowAction::unknown_fields`], which `flow check` asks before the run.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct Call {
    /// The flow's name, as it reads on disk without `.flow.json`.
    pub flow: String,
    /// The `root_inputs` the step imposes on the child, key by key.
    #[serde(default)]
    pub inputs: BTreeMap<String, Value>,
}

/// How a child run ended, for whoever records it.
pub struct RunNote<'a> {
    pub flow: &'a FlowFile,
    pub run_id: &'a str,
    pub parent_run_id: &'a str,
    pub parent_step_id: &'a str,
    /// `running`, `complete`, `failed`, `waiting`, `stopped`, `cap_reached`.
    pub status: &'a str,
    pub started_at: i64,
    /// `None` while the child run is still open.
    pub ended_at: Option<i64>,
    pub error: Option<String>,
}

/// What the `subflow` step cannot know on its own. A trait rather than three
/// fields: the step must run the child with the parent's own actions — the
/// registry it is itself in, which a direct reference makes a cycle the
/// compiler refuses — and must write to the store, which this crate must not
/// know about, `flow` never depending on `ledger`. Whoever builds the registry
/// holds both of those, and passes them through here.
pub trait SubflowHost: Send + Sync {
    /// Where flows are looked for, in [`crate::system::sources`] order.
    fn sources(&self) -> Vec<FlowSource>;

    /// The actions the child runs with: the parent's own.
    fn actions(&self) -> Result<Arc<ActionRegistry>, ActionError>;

    /// Where the child run's steps get written.
    fn store(&self) -> Result<Arc<dyn RecordStore>, ActionError>;

    /// Writes — or updates — the child run's header.
    fn note_run(&self, note: &RunNote<'_>) -> Result<(), ActionError>;

    /// The line that explains to a person how the child run ended.
    ///
    /// Composed by whoever displays, not whoever executes: `SpendStop` carries
    /// the data and the sentence lives elsewhere, so copying it here would make
    /// a second copy to keep aligned. Having no sentence, invent none.
    fn why(&self, _execution: &Execution) -> Option<String> {
        None
    }
}

/// The step that runs another flow.
pub struct SubflowAction {
    host: Arc<dyn SubflowHost>,
}

impl SubflowAction {
    pub fn new(host: Arc<dyn SubflowHost>) -> Self {
        Self { host }
    }
}

impl Action for SubflowAction {
    fn execute(&self, input: &Value, shared: &SharedState) -> Result<ActionOutcome, ActionError> {
        let call: Call = serde_json::from_value(input.clone()).map_err(|error| {
            ActionError::new(
                "invalid_subflow_call",
                format!("the step does not declare which flow to run: {error}"),
            )
        })?;

        // The caller: run and step are written by the executor before every
        // action. Without them the child run could not be traced back, and a
        // child run nobody can tie to the step that asked for it is the fault
        // decision 4 exists to prevent.
        let parent_run = text(shared, CURRENT_RUN).ok_or_else(|| {
            ActionError::new(
                "no_parent_run",
                "no run in progress: a subflow exists only inside a run",
            )
        })?;
        let parent_step = text(shared, CURRENT_STEP).ok_or_else(|| {
            ActionError::new(
                "no_parent_step",
                "no step in progress: there would be nobody to attribute the child run to",
            )
        })?;

        // Ask for the store before reading any file. Not having one is a
        // condition of the step, not of the flow it names: discovering it after
        // walking every source would tell someone "I cannot find that flow"
        // when in truth they could not have run any flow at all.
        let store = self.host.store()?;

        let sources = self.host.sources();
        let found = system::load_all(&sources);
        let (_, origin, entry) = found
            .iter()
            .find(|(name, _, _)| name == &call.flow)
            .ok_or_else(|| {
                ActionError::new(
                    "unknown_subflow",
                    format!(
                        "no flow named \"{}\" among the ones I can see: {}",
                        call.flow,
                        places(&sources)
                    ),
                )
            })?;
        let child = entry
            .clone()
            .map_err(|why| ActionError::new("invalid_subflow", why))?;

        // Before opening, not after spending. The calls declared in `with` read
        // without running anything: a loop across separate files is caught
        // here, at the outermost run's first `subflow` step.
        if let Some(cycle) = call_cycle(&call.flow, &known_flows(&found)) {
            return Err(cyclic(&cycle));
        }

        let chain = extend_chain(&chain_of(shared), &call.flow)?;

        // A call declares no cap of its own: the spend cap belongs to the flow.
        // A step that could raise it for the flow it calls would move the
        // declaration away from whoever has to read it.
        let cap = tightest(
            child.spend_cap_micros,
            remaining_of(shared, &store, &parent_run)?,
        );

        let run_id = child_run_id(&parent_run, &parent_step)?;
        let started_at = SystemClock.now().map_err(clock_broke)?;
        let mut note = RunNote {
            flow: &child,
            run_id: &run_id,
            parent_run_id: &parent_run,
            parent_step_id: &parent_step,
            status: "running",
            started_at,
            ended_at: None,
            error: None,
        };
        self.host.note_run(&note)?;

        // The child's inputs are its own, overridden by the step — never the
        // parent's: what the child receives is written in one place, and reads
        // without knowing anything about whoever calls it.
        let mut root_inputs = child.inputs.clone();
        root_inputs.extend(call.inputs.clone());

        let mut child_shared = SharedState::new();
        child_shared.insert(CALL_CHAIN.to_owned(), chain_value(&chain));

        let actions = self.host.actions()?;
        let outcome = InProcessExecutor.execute(
            &child.graph,
            ExecutionRequest {
                run_id: run_id.clone(),
                root_inputs,
                gates: Vec::new(),
                shared: child_shared,
                spend_cap_micros: cap,
            },
            store.as_ref(),
            actions.as_ref(),
            &SystemClock,
        );

        let ended_at = SystemClock.now().map_err(clock_broke)?;
        note.ended_at = Some(ended_at);

        let execution = match outcome {
            Ok(execution) => execution,
            Err(error) => {
                let said = error.to_string();
                note.status = "failed";
                note.error = Some(said.clone());
                self.host.note_run(&note)?;
                return Err(ActionError::new(
                    "subflow_broke",
                    format!("run {run_id} of flow {} never started: {said}", call.flow),
                ));
            }
        };

        let (status, went_well) = crate::run_status(&execution);
        let why = self.host.why(&execution);
        note.status = status;
        note.error = why.clone();
        self.host.note_run(&note)?;

        if !went_well {
            // Waiting is not breaking. A child stopped on a waiting step makes
            // the parent wait: the parent's run stays restartable instead of
            // reading as broken, which is how any other step behaves while it
            // does not yet know its own outcome.
            if status == "waiting" {
                return Ok(ActionOutcome::Waiting(format!(
                    "run {run_id} of flow {} is waiting",
                    call.flow
                )));
            }
            // **THE CLASS IS WRITTEN OUT, NOT BUILT.** `format!("subflow_{status}")`
            // gave the same four strings, and no reader could find them: a
            // search for `subflow_failed` came back empty, and the check that
            // pairs every class with a sentence could not count them either. A
            // class is a name a person reads and a catalogue answers for, so it
            // has to exist as a name somewhere.
            //
            // `run_status` is a closed set and `waiting` and `complete` have
            // already gone. **The last arm is its own class, not a bucket**: a
            // status added to `run_status` and not here would otherwise arrive
            // wearing the name of a different one, which reads as a diagnosis
            // and is a guess.
            //
            // The arms build the error rather than pick a name for one, so the
            // class is a literal where the error is made. That is what lets the
            // check pair it with a sentence — a name held in a variable is a
            // name the check cannot see, and one it cannot see is one that can
            // go mute without saying so.
            let said = why.unwrap_or_else(|| {
                format!("run {run_id} of flow {} ended in state {status}", call.flow)
            });
            return Err(match status {
                "stopped" => ActionError::new("subflow_stopped", said),
                "failed" => ActionError::new("subflow_failed", said),
                "cap_reached" => ActionError::new("subflow_cap_reached", said),
                "incomplete" => ActionError::new("subflow_incomplete", said),
                _ => ActionError::new("subflow_unknown_state", said),
            });
        }

        let records = store
            .records(&run_id)
            .map_err(|error| ActionError::new("subflow_unreadable", error.to_string()))?;
        Ok(ActionOutcome::Went(json!({
            "flow": call.flow,
            "origin": origin,
            "run_id": run_id,
            "status": status,
            "outputs": last_outputs(&child.graph, &records),
        })))
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

    /// Handed to a person, and not out of generic caution: redoing this step
    /// means redoing a whole flow, with everything that flow touches — paid
    /// engines, files written, panes opened. The child's species cannot be
    /// deduced from here; it is the sum of its steps' species, and one step
    /// needing a person is enough to make the call need one too.
    fn species(&self) -> StepSpecies {
        StepSpecies::HandToHuman
    }
}

/// The chain of already-stacked flows, read from the shared state.
pub fn chain_of(shared: &SharedState) -> Vec<String> {
    shared
        .get(CALL_CHAIN)
        .and_then(Value::as_array)
        .map(|names| {
            names
                .iter()
                .filter_map(Value::as_str)
                .map(str::to_owned)
                .collect()
        })
        .unwrap_or_default()
}

/// The chain with `next` at the end, or the error naming why it does not fit.
///
/// Two different faults, two different words: a loop is a design error and no
/// raised number removes it, while a stack that is merely too tall can be
/// legitimate and can be raised. Calling both "too deep" would send the reader
/// hunting the wrong thing.
pub fn extend_chain(chain: &[String], next: &str) -> Result<Vec<String>, ActionError> {
    if let Some(from) = chain.iter().position(|seen| seen == next) {
        let mut cycle: Vec<String> = chain[from..].to_vec();
        cycle.push(next.to_owned());
        return Err(cyclic(&cycle));
    }
    if chain.len() >= MAX_DEPTH {
        let mut deep: Vec<String> = chain.to_vec();
        deep.push(next.to_owned());
        return Err(ActionError::new(
            "subflow_too_deep",
            format!(
                "more than {MAX_DEPTH} flows stacked one inside the other: {}",
                deep.join(" → ")
            ),
        ));
    }
    let mut extended = chain.to_vec();
    extended.push(next.to_owned());
    Ok(extended)
}

/// The flows named by this flow's `subflow` steps.
///
/// It reads `with`, that is, what is declared. A step that derives the flow
/// name from a dependency's output does not appear here and cannot: at check
/// time that name does not exist yet. This is the declared limit of the static
/// check, and the reason the chain also travels at run time.
pub fn calls_of(flow: &FlowFile) -> Vec<String> {
    flow.graph
        .steps()
        .iter()
        .filter(|step| step.action == SUBFLOW_ACTION)
        .filter_map(|step| {
            step.with
                .as_ref()
                .and_then(|with| with.get("flow"))
                .and_then(Value::as_str)
                .map(str::to_owned)
        })
        .collect()
}

/// The valid flows among those loaded, by name.
///
/// Broken ones stay out: a file that will not read declares no calls, and
/// saying it has zero would be a claim nobody verified.
pub fn known_flows(
    found: &[(String, &'static str, Result<FlowFile, String>)],
) -> BTreeMap<String, FlowFile> {
    found
        .iter()
        .filter_map(|(name, _, entry)| entry.as_ref().ok().map(|flow| (name.clone(), flow.clone())))
        .collect()
}

/// The call chain that loops back on itself from `entry`, if there is one.
///
/// This is the check the graph cannot do: `Graph::validate` refuses cycles but
/// looks inside a single file, while with `subflow` a loop crosses several
/// files and neither graph alone has anything wrong with it. The chain comes
/// back readable as it stands: `research → develop → research`.
pub fn call_cycle(entry: &str, known: &BTreeMap<String, FlowFile>) -> Option<Vec<String>> {
    let mut chain = Vec::new();
    walk(entry, known, &mut chain)
}

fn walk(
    name: &str,
    known: &BTreeMap<String, FlowFile>,
    chain: &mut Vec<String>,
) -> Option<Vec<String>> {
    if let Some(from) = chain.iter().position(|seen| seen == name) {
        let mut cycle: Vec<String> = chain[from..].to_vec();
        cycle.push(name.to_owned());
        return Some(cycle);
    }
    let flow = known.get(name)?;
    chain.push(name.to_owned());
    for next in calls_of(flow) {
        if let Some(cycle) = walk(&next, known, chain) {
            return Some(cycle);
        }
    }
    chain.pop();
    None
}

/// The cap that holds for the child: the tighter of the two declared.
///
/// `None` is not zero here either. Declaring nothing imposes nothing, so the
/// cap that remains is the other one. Both are absent only when nobody set a
/// limit, and that is the one case where the child runs uncapped.
pub fn tightest(declared: Option<i64>, remaining: Option<i64>) -> Option<i64> {
    match (declared, remaining) {
        (Some(one), Some(other)) => Some(one.min(other)),
        (Some(one), None) => Some(one),
        (None, other) => other,
    }
}

/// What is left of the parent's own cap, if it has one.
///
/// Known limit: the store sums per run and the child's spend sits under its own
/// `run_id`, so this remainder does not fall for what children spent. The worst
/// case is the parent's cap times the number of its `subflow` steps. It closes
/// by walking `parent_run_id` up into the sum.
fn remaining_of(
    shared: &SharedState,
    store: &Arc<dyn RecordStore>,
    parent_run: &str,
) -> Result<Option<i64>, ActionError> {
    let Some(cap) = shared.get(CURRENT_CAP).and_then(Value::as_i64) else {
        return Ok(None);
    };
    let spent = store
        .spent(parent_run)
        .map_err(|error| ActionError::new("subflow_unreadable", error.to_string()))?;
    Ok(Some((cap - spent.micros).max(0)))
}

/// The child run's identifier: it carries the parent in its own name, and the
/// why is decision 4. The real link is the store's `parent_run_id` column, and
/// this prefix doubles it where there is no store to ask — a file listing, a
/// log line, a report. The trailing nanoseconds make every attempt unique: a
/// retried `subflow` step opens a *new* run instead of reopening the broken
/// one, which is how every other retry in the tree behaves.
fn child_run_id(parent_run: &str, parent_step: &str) -> Result<String, ActionError> {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|error| ActionError::new("clock_before_epoch", error.to_string()))?;
    Ok(format!("{parent_run}::{parent_step}::{}", now.as_nanos()))
}

/// The outputs of the child run's terminal steps.
///
/// Terminal means "nobody depends on it". A flow does not declare what its
/// output is, and inventing a convention — "the last step in the file" — would
/// tie the result to the order someone wrote the lines in. The steps nothing
/// hangs off are what the flow produced, whatever order the file is in.
fn last_outputs(graph: &Graph, records: &[StepRecord]) -> Value {
    let depended: BTreeSet<&str> = graph
        .steps()
        .iter()
        .flat_map(|step| step.deps.iter().map(String::as_str))
        .collect();
    let mut outputs = Map::new();
    for step in graph.steps() {
        if depended.contains(step.id.as_str()) {
            continue;
        }
        let last = records
            .iter()
            .filter(|record| record.step_id == step.id && record.outcome == Some(Outcome::Went))
            .max_by_key(|record| (record.attempt, record.epoch));
        if let Some(output) = last.and_then(|record| record.output.clone()) {
            outputs.insert(step.id.clone(), output);
        }
    }
    Value::Object(outputs)
}

fn cyclic(cycle: &[String]) -> ActionError {
    ActionError::new(
        "subflow_cycle",
        format!(
            "a flow cannot call itself, not even by way of others: {}",
            cycle.join(" → ")
        ),
    )
}

fn chain_value(chain: &[String]) -> Value {
    Value::Array(chain.iter().cloned().map(Value::String).collect())
}

fn text(shared: &SharedState, key: &str) -> Option<String> {
    shared
        .get(key)
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty())
        .map(str::to_owned)
}

fn places(sources: &[FlowSource]) -> String {
    sources
        .iter()
        .map(|source| format!("{} ({})", source.origin, source.dir.display()))
        .collect::<Vec<_>>()
        .join(", ")
}

fn clock_broke(error: crate::FlowError) -> ActionError {
    ActionError::new("clock_broke", error.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Step, ValueSchema};

    fn calling(id: &str, called: &[&str]) -> FlowFile {
        let steps: Vec<Step> = called
            .iter()
            .enumerate()
            .map(|(nth, target)| Step {
                id: format!("call-{nth}"),
                deps: Vec::new(),
                input_schema: ValueSchema::Any,
                output_schema: ValueSchema::Any,
                with: Some(json!({ "flow": target })),
                when: None,
                action: SUBFLOW_ACTION.to_owned(),
                max_attempts: 1,
            })
            .collect();
        FlowFile {
            id: id.to_owned(),
            description: "a test flow".to_owned(),
            graph: Graph::new(steps).expect("valid graph"),
            inputs: BTreeMap::new(),
            schedule: None,
            spend_cap_micros: None,
        }
    }

    fn map(flows: Vec<FlowFile>) -> BTreeMap<String, FlowFile> {
        flows
            .into_iter()
            .map(|flow| (flow.id.clone(), flow))
            .collect()
    }

    #[test]
    fn two_flows_that_call_each_other_are_a_named_chain() {
        let known = map(vec![
            calling("research", &["develop"]),
            calling("develop", &["research"]),
        ]);

        let cycle = call_cycle("research", &known).expect("the loop is there");

        assert_eq!(cycle, vec!["research", "develop", "research"]);
    }

    #[test]
    fn a_flow_that_calls_itself_is_a_chain_of_two() {
        let known = map(vec![calling("loner", &["loner"])]);

        assert_eq!(
            call_cycle("loner", &known).expect("the loop is there"),
            vec!["loner", "loner"]
        );
    }

    /// Without this, "always finds a loop" would stay green on all the others:
    /// a call tree converging on the same flow from two branches is not a
    /// cycle.
    #[test]
    fn a_diamond_of_calls_is_not_a_cycle() {
        let known = map(vec![
            calling("top", &["left", "right"]),
            calling("left", &["bottom"]),
            calling("right", &["bottom"]),
            calling("bottom", &[]),
        ]);

        assert_eq!(call_cycle("top", &known), None);
    }

    /// A name no source knows is not a cycle: it is a missing flow, and another
    /// error says so with another word.
    #[test]
    fn a_call_to_a_flow_nobody_has_is_not_a_cycle() {
        let known = map(vec![calling("top", &["never-written"])]);

        assert_eq!(call_cycle("top", &known), None);
    }

    #[test]
    fn the_tighter_cap_is_the_one_that_holds() {
        assert_eq!(tightest(Some(500), Some(200)), Some(200));
        assert_eq!(tightest(Some(100), Some(900)), Some(100));
        assert_eq!(tightest(Some(100), None), Some(100));
        assert_eq!(tightest(None, Some(900)), Some(900));
        assert_eq!(tightest(None, None), None);
    }

    /// Zero is a cap, not an absence: a parent that has run out of money does
    /// not let a child start "uncapped".
    #[test]
    fn a_remaining_of_zero_still_caps_the_child() {
        assert_eq!(tightest(Some(1_000_000), Some(0)), Some(0));
    }

    #[test]
    fn the_chain_grows_until_the_declared_depth() {
        let mut chain = Vec::new();
        for nth in 0..MAX_DEPTH {
            chain = extend_chain(&chain, &format!("f{nth}")).expect("it fits");
        }
        assert_eq!(chain.len(), MAX_DEPTH);

        let error = extend_chain(&chain, "one-too-many").expect_err("the cap trips");

        assert_eq!(error.class, "subflow_too_deep");
        assert!(
            error.said.contains("f0 → f1") && error.said.contains("one-too-many"),
            "the error must name the chain: {}",
            error.said
        );
    }

    #[test]
    fn a_repeated_flow_in_the_chain_is_named_as_a_cycle() {
        let chain = vec!["research".to_owned(), "develop".to_owned()];

        let error = extend_chain(&chain, "research").expect_err("it is a loop");

        assert_eq!(error.class, "subflow_cycle");
        assert!(
            error.said.contains("research → develop → research"),
            "the error must name the chain: {}",
            error.said
        );
    }
}
