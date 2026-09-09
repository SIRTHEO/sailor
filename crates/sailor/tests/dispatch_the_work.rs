//! The contract between the flow that dispatches the work and the actions that
//! run it.
//!
//! **WHY IT LIVES HERE AND NOT IN THE ACTIONS CRATE.** The flow now names
//! actions from three crates — the trigger comes from `trigger`, and the engine
//! can resolve a tool only if someone handed it a resolver, which lives in
//! `toolbox` — and `sailor` is the one place in the program where the three
//! meet. A test that cannot see every action of the flow cannot say whether
//! that flow starts.
//!
//! **THE FILE IS READ AT TEST TIME, NOT AT COMPILE TIME.** With `include_str!`
//! a deleted flow does not fail a test: it fails the *compilation* of the whole
//! crate, and whoever meets it does not see a flow, they see a broken crate.
//!
//! **NO TEST HERE CALLS A REAL ENGINE.** The tools the flow names are resolved
//! by a test resolver that sends them all to `sh`: the same road a real run
//! walks — the step asks for a tool, someone resolves it — with the one
//! difference that matters, that no call is spent.

use actions::ToolResolver;
use flow::{
    ActionRegistry, Clock, Decision, Execution, ExecutionRequest, Executor, FlowError, FlowFile,
    Graph, InMemoryRecordStore, InProcessExecutor, Outcome, SharedState, Step, ValueSchema,
};
use serde_json::{json, Value};
use std::collections::BTreeMap;

const FLOW_ID: &str = "dispatch-the-work";

/// **THE FLOW IS NOT ON DISK ANY MORE, AND THIS TEST MUST NOT LOOK FOR IT
/// THERE.** The flow went into the binary — the shipped routing rules name it,
/// and on another machine the directory is missing — so reading it from disk
/// would mean testing a file the product does not ship while nobody tests the
/// one it does. `system::FLOWS` is the same source whoever runs it takes it
/// from.
fn flow_text() -> String {
    flow::system::FLOWS
        .iter()
        .find(|(name, _)| *name == FLOW_ID)
        .map(|(_, text)| (*text).to_owned())
        .unwrap_or_else(|| panic!("«{FLOW_ID}» non è fra i flussi spediti col binario"))
}

fn flow_file() -> FlowFile {
    serde_json::from_str(&flow_text()).expect("il flusso deve caricarsi come FlowFile")
}

/// Every tool becomes `sh`: which command actually runs is then decided by the
/// step's `args` field, which the tests replace.
struct EveryToolIsShell;

impl ToolResolver for EveryToolIsShell {
    fn resolve(&self, _id: &str) -> Result<String, String> {
        Ok("sh".to_owned())
    }
}

/// The resolver that finds nothing: it tests what happens to a flow carried to
/// a machine where that tool is missing.
struct NoToolIsHere;

impl ToolResolver for NoToolIsHere {
    fn resolve(&self, id: &str) -> Result<String, String> {
        Err(format!("lo strumento «{id}» non è su questa macchina"))
    }
}

/// The store is where a tree Sailor cuts is written down, and a step asking for
/// a tree of its own without one is refused: no register, no tree.
fn registry_with(
    resolver: impl ToolResolver + 'static,
    store: Option<ledger::Ledger>,
) -> ActionRegistry {
    let mut registry = ActionRegistry::default();
    actions::register_default(&mut registry);
    trigger::register_default(&mut registry);
    registry.register(
        actions::EXTERNAL_ENGINE_ACTION,
        actions::ExternalEngineAction::resolving_with(resolver).recording_to(store),
    );
    registry
}

/// A fake clock that steps forward by one on every question. The counter is
/// atomic because the clock is now shared between the threads of a front.
struct Tick(std::sync::atomic::AtomicI64);

impl Tick {
    fn new(start: i64) -> Self {
        Tick(std::sync::atomic::AtomicI64::new(start))
    }
}

impl Clock for Tick {
    fn now(&self) -> Result<i64, FlowError> {
        Ok(self.0.fetch_add(1, std::sync::atomic::Ordering::Relaxed) + 1)
    }
}

/// A repository the run can cut trees from, taken down with the test.
///
/// The two engine steps ask for a tree of their own, and a run that cannot say
/// which project it is in refuses rather than putting them both in the
/// directory the tests happen to start in.
struct Project(std::path::PathBuf, ledger::Ledger);

impl Project {
    fn new() -> Self {
        static MADE: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
        let root = std::env::temp_dir()
            .join(format!(
                "dispatch-{}-{}",
                std::process::id(),
                MADE.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
            ))
            .join("repo");
        std::fs::create_dir_all(&root).expect("a project to run in");
        for args in [
            vec!["init", "-q"],
            vec!["config", "user.email", "prove@example"],
            vec!["config", "user.name", "prove"],
        ] {
            let done = std::process::Command::new("git")
                .arg("-C")
                .arg(&root)
                .args(&args)
                .env("GIT_CONFIG_GLOBAL", "/dev/null")
                .env("GIT_CONFIG_NOSYSTEM", "1")
                .output()
                .expect("git runs");
            assert!(done.status.success(), "git {args:?}");
        }
        std::fs::write(root.join("README"), "a tree to cut from\n").expect("a file");
        for args in [vec!["add", "README"], vec!["commit", "-q", "-m", "first"]] {
            let done = std::process::Command::new("git")
                .arg("-C")
                .arg(&root)
                .args(&args)
                .env("GIT_CONFIG_GLOBAL", "/dev/null")
                .env("GIT_CONFIG_NOSYSTEM", "1")
                .output()
                .expect("git runs");
            assert!(done.status.success(), "git {args:?}");
        }
        let store = root
            .parent()
            .map(|above| above.join("store"))
            .expect("the project sits under a scratch");
        let store = ledger::Ledger::open(store).expect("a store to write the trees into");
        Project(root, store)
    }
}

impl Drop for Project {
    fn drop(&mut self) {
        if let Some(above) = self.0.parent() {
            let _ = std::fs::remove_dir_all(above);
        }
    }
}

fn run_with(
    graph: &Graph,
    inputs: &[(&str, Value)],
    resolver: impl ToolResolver + 'static,
) -> (Execution, InMemoryRecordStore) {
    let project = Project::new();
    let registry = &registry_with(resolver, Some(project.1.clone()));
    let store = InMemoryRecordStore::default();
    let mut shared = SharedState::new();
    shared.insert(
        flow::WORKSPACE_ROOT.to_owned(),
        json!(project.0.to_string_lossy()),
    );
    let request = ExecutionRequest {
        holder: None,
        run_id: "prova".to_owned(),
        root_inputs: inputs
            .iter()
            .map(|(id, value)| ((*id).to_owned(), value.clone()))
            .collect(),
        gates: Vec::new(),
        shared,
        spend_cap_micros: None,
        stops: flow::RunStops::default(),
    };
    let execution = InProcessExecutor
        .execute(graph, request, &store, registry, &Tick::new(0))
        .expect("l'esecuzione non deve rompersi");
    (execution, store)
}

fn last_decision(execution: &Execution) -> Decision {
    execution
        .decisions
        .last()
        .cloned()
        .expect("almeno una decisione")
}

// ── the shape of the file ────────────────────────────────────────────────

/// The six nodes and their edges: the trigger carrying the errand, the node that
/// splits it, the two engines, the verifying step, the gate that makes it red.
#[test]
fn the_flow_declares_a_trigger_a_dispatch_two_engines_and_a_verdict() {
    let flow = flow_file();

    assert_eq!(flow.id, FLOW_ID);
    let ids: Vec<&str> = flow
        .graph
        .steps()
        .iter()
        .map(|step| step.id.as_str())
        .collect();
    assert_eq!(
        ids,
        vec!["trigger", "dispatch", "engine_a", "engine_b", "verify", "verdict"]
    );

    let deps = |id: &str| flow.graph.step(id).expect("il passo esiste").deps.clone();
    assert!(
        deps("trigger").is_empty(),
        "l'innesco è il nodo di ingresso"
    );
    assert_eq!(deps("dispatch"), vec!["trigger".to_owned()]);
    assert_eq!(deps("engine_a"), vec!["dispatch".to_owned()]);
    assert_eq!(deps("engine_b"), vec!["dispatch".to_owned()]);
    assert_eq!(deps("verify"), vec!["dispatch", "engine_a", "engine_b"]);
    assert_eq!(deps("verdict"), vec!["verify".to_owned()]);
}

/// **ONE ENTRY NODE ONLY, AND IT IS A TRIGGER.** A step without dependencies
/// that is not a trigger is another place the errand can enter from with nobody
/// having sent it.
#[test]
fn the_only_step_without_dependencies_is_the_trigger() {
    let flow = flow_file();

    let roots: Vec<&Step> = flow
        .graph
        .steps()
        .iter()
        .filter(|step| step.deps.is_empty())
        .collect();

    assert_eq!(roots.len(), 1, "un solo ingresso");
    assert_eq!(roots[0].action, "trigger");
    assert_eq!(flow.inputs.keys().collect::<Vec<_>>(), vec!["trigger"]);
}

#[test]
fn every_action_the_flow_names_is_registered() {
    let flow = flow_file();
    let registry = registry_with(EveryToolIsShell, None);

    let missing: Vec<&str> = flow
        .graph
        .steps()
        .iter()
        .filter(|step| registry.get(&step.action).is_none())
        .map(|step| step.action.as_str())
        .collect();

    assert!(missing.is_empty(), "azioni non registrate: {missing:?}");
}

/// **NO BINARY INSIDE THE FLOW.** `"bin": "claude"` runs only where that name is
/// on the runner's path; a tool identifier runs wherever someone knows how to
/// resolve it, and where it is missing it stops saying which. This test is the
/// guard of that rule for the shipped flows.
#[test]
fn no_step_names_a_binary_and_no_path_belongs_to_one_machine() {
    let flow = flow_file();
    let text = flow_text();

    assert!(!text.contains("/Users/"), "un percorso di casa è cablato");
    assert!(!text.contains("/home/"), "un percorso di casa è cablato");

    for step in flow.graph.steps() {
        let Some(with) = step.with.as_ref() else {
            continue;
        };
        assert!(
            with.get("bin").is_none(),
            "il passo {} nomina un binario invece di uno strumento",
            step.id
        );
        if step.action == actions::EXTERNAL_ENGINE_ACTION {
            // ONE OR A CHAIN. A step may declare a list of engines to try in
            // order instead of a single name. What this test guards does not
            // change — no step runs an engine without saying which it wants —
            // but reading `tool` as a lone string would call it absent exactly
            // where there are three.
            let named: Vec<&str> = match with.get("tool") {
                Some(Value::String(id)) => vec![id.as_str()],
                Some(Value::Array(chain)) => chain.iter().filter_map(Value::as_str).collect(),
                _ => Vec::new(),
            };
            assert!(
                !named.is_empty() && named.iter().all(|id| !id.is_empty()),
                "il passo {} esegue un motore senza dire quale strumento vuole",
                step.id
            );
        }
    }
}

/// **THE OUTPUT SCHEMA SAYS AN ENGINE HERE CANNOT FAIL IN SILENCE**, and it also
/// says what passes to the next step. Two different declarations, and both must
/// be in the file: `status` admits a finished call and nothing else, and
/// `answer` is the one thing that crosses the chain — not the raw text of what
/// the engine said.
#[test]
fn an_engine_step_declares_what_it_can_return_and_what_it_hands_on() {
    let flow = flow_file();
    let mut engines = 0;

    for step in flow.graph.steps() {
        if step.action != actions::EXTERNAL_ENGINE_ACTION {
            continue;
        }
        engines += 1;
        let ValueSchema::Object {
            properties,
            required,
            allow_extra,
        } = &step.output_schema
        else {
            panic!("il passo {} non dichiara un'uscita a oggetto", step.id);
        };
        assert_eq!(
            properties.get("status"),
            Some(&ValueSchema::OneOf {
                values: vec![json!("ok")]
            }),
            "il passo {} accetta ancora l'uscita di un motore fallito",
            step.id
        );
        assert!(
            required.contains("answer"),
            "il passo {} non pretende nessuna risposta",
            step.id
        );
        assert!(
            !allow_extra,
            "il passo {} lascia passare campi non dichiarati",
            step.id
        );
        assert!(
            properties.get("stdout").is_none(),
            "il passo {} inoltra ancora il testo grezzo del motore",
            step.id
        );
        // The demanded shape and the shape declared in the output are the same
        // thing written twice: if they diverge, the next step reads a field the
        // engine never promised.
        let shape: ValueSchema = serde_json::from_value(
            step.with
                .as_ref()
                .and_then(|with| with.get("answer_shape"))
                .unwrap_or_else(|| {
                    panic!("il passo {} non dichiara la forma della risposta", step.id)
                })
                .clone(),
        )
        .expect("la forma dichiarata deve essere uno schema valido");
        assert_eq!(
            properties.get("answer"),
            Some(&shape),
            "nel passo {} la forma pretesa e quella dichiarata nell'uscita non coincidono",
            step.id
        );
    }
    assert_eq!(engines, 4, "i motori del flusso");
}

#[test]
fn the_last_step_accepts_only_a_check_that_passed() {
    let flow = flow_file();
    let verdict = flow.graph.step("verdict").expect("il cancello esiste");

    assert!(verdict
        .output_schema
        .validate(&json!({"status": "passed"}))
        .is_ok());
    assert!(verdict
        .output_schema
        .validate(&json!({"status": "failed"}))
        .is_err());
    assert!(verdict
        .output_schema
        .validate(&json!({"status": "timed_out"}))
        .is_err());
}

// ── the flow, actually run ───────────────────────────────────────────────

/// The mark this test chases all the way down the chain.
const MARK: &str = "SEGNO-DELLA-PROVA";

/// A fake engine that reads its errand **from its input** and answers well only
/// if it finds the mark there; otherwise it answers out of shape and its step
/// turns red. That is how the text is proved to have really arrived, instead of
/// trusting that the references are written right.
fn reads_stdin(answer: &str) -> Value {
    json!([
        "-c",
        "if grep -q \"$1\"; then printf '%s' \"$2\"; else printf '%s' '{\"segno\":\"non arrivato\"}'; fi",
        "motore",
        MARK,
        answer
    ])
}

/// The same, for the engine that receives its errand **in an argument**: it is
/// the measured shape of `agy`, and the reference must reach all the way into
/// the argument list.
fn reads_args(answer: &str, carried: Value) -> Value {
    json!([
        "-c",
        "case \"$1\" in *\"$2\"*) printf '%s' \"$3\";; *) printf '%s' '{\"segno\":\"non arrivato\"}';; esac",
        "motore",
        carried,
        MARK,
        answer
    ])
}

/// A fake command line in place of the engine's own. The model goes with it: a
/// line written by hand already says which model to ask for, and a step
/// declaring both is refused.
fn answering_with(with: &mut Value, args: Value) {
    with.as_object_mut().expect("the step carries a map of values").remove("model");
    with["args"] = args;
}

/// Replaces the arguments of the steps that call an engine, leaving all the rest
/// of the file as it is: the references, the demanded shapes, the schemas, the
/// edges and the trigger are the ones that will run.
fn chain_with(verdict: &str, engine_a_args: Option<Value>) -> Graph {
    let flow = flow_file();
    let dispatched = json!({
        "first_engine": format!("primo incarico, {MARK}"),
        "second_engine": format!("secondo incarico, {MARK}"),
        "why_first": "a territory of files",
        "why_second": "the other territory"
    })
    .to_string();
    let found = json!({"findings": ["src/a.rs"], "total": 1}).to_string();
    let judged = json!({"verdict": verdict, "why": "ho guardato"}).to_string();

    let mut steps: Vec<Step> = flow.graph.steps().to_vec();
    for step in &mut steps {
        let Some(with) = step.with.as_mut() else {
            continue;
        };
        match step.id.as_str() {
            "dispatch" => answering_with(with, reads_stdin(&dispatched)),
            "engine_a" => answering_with(
                with,
                engine_a_args.clone().unwrap_or_else(|| reads_stdin(&found)),
            ),
            "engine_b" => {
                let carried = with["stdin"].clone();
                answering_with(with, reads_args(&found, carried));
            }
            "verify" => answering_with(with, reads_stdin(&judged)),
            _ => {}
        }
    }
    Graph::new(steps).expect("il grafo del file resta valido")
}

/// A dispatch that names the two engines and not the why of either is out
/// of shape, and the run stops on that step: a choice without its reason is
/// not a choice the ledger can keep.
#[test]
fn a_dispatch_without_a_why_per_choice_fails_the_shape() {
    let flow = flow_file();
    for missing in ["why_first", "why_second"] {
        let mut answer = json!({
            "first_engine": format!("primo incarico, {MARK}"),
            "second_engine": format!("secondo incarico, {MARK}"),
            "why_first": "a territory",
            "why_second": "the other"
        });
        answer.as_object_mut().expect("an object").remove(missing);
        let mut steps: Vec<Step> = flow.graph.steps().to_vec();
        for step in &mut steps {
            if step.id == "dispatch" {
                let with = step.with.as_mut().expect("the step carries its values");
                answering_with(with, reads_stdin(&answer.to_string()));
            }
        }
        let graph = Graph::new(steps).expect("the graph stays valid");

        let (execution, _) = run_with(
            &graph,
            &[("trigger", trigger_input())],
            EveryToolIsShell,
        );

        assert_eq!(
            last_decision(&execution),
            Decision::Failed(vec!["dispatch".to_owned()]),
            "an answer without «{missing}» must not pass the shape"
        );
    }
}

/// The trigger's errand, with the mark inside: the one thing that enters the
/// flow, and the one the engines must see arrive.
fn trigger_input() -> Value {
    let mut signal = flow_file().inputs["trigger"].clone();
    signal["text"] = json!(format!("conta i residui, {MARK}"));
    signal
}

/// **THE WHOLE CHAIN, ACTUALLY RUN, WITHOUT SPENDING A CALL.** From the signal
/// to the verdict: the text enters at the trigger, reaches the dispatching node,
/// from there the two engines — one on its input, the other in an argument — and
/// the two answers plus the errands reach the verifier. Every fake engine answers
/// well **only if** it received what the flow promised it: one badly written
/// reference and the run turns red.
///
/// Two runs, because one alone would prove nothing: change the verdict and the
/// run changes colour.
#[test]
fn the_whole_chain_runs_from_the_signal_to_the_verdict() {
    let outcome = |verdict: &str| {
        let graph = chain_with(verdict, None);
        let (execution, _) = run_with(
            &graph,
            &[("trigger", trigger_input())],
            EveryToolIsShell,
        );
        last_decision(&execution)
    };

    assert_eq!(
        outcome("APPROVATO"),
        Decision::Complete,
        "i sei nodi devono potersi chiudere tutti"
    );
    assert_eq!(
        outcome("RESPINTO"),
        Decision::Failed(vec!["verdict".to_owned()]),
        "la stessa catena, con il verdetto contrario, deve finire rossa"
    );
}

/// **ONLY WHAT THE SHAPE DECLARES TRAVELS DOWN THE CHAIN.** The signal reaches
/// both engines — their answers prove it, since they come out well only if the
/// mark was there — and in each step's output nothing of the raw text remains:
/// neither the model's preambles nor the fields nobody declared.
#[test]
fn only_what_the_shape_declares_travels_down_the_chain() {
    let graph = chain_with("APPROVATO", None);

    let (_, store) = run_with(
        &graph,
        &[("trigger", trigger_input())],
        EveryToolIsShell,
    );

    let output = |step: &str| {
        store
            .all()
            .iter()
            .find(|record| record.step_id == step)
            .and_then(|record| record.output.clone())
            .unwrap_or_else(|| panic!("il passo {step} non ha lasciato un'uscita"))
    };
    assert!(output("trigger")["text"]
        .as_str()
        .expect("testo")
        .contains(MARK));
    for engine in ["dispatch", "engine_a", "engine_b", "verify"] {
        let seen = output(engine);
        assert_eq!(seen["status"], "ok", "il passo {engine}: {seen}");
        assert!(
            seen.get("stdout").is_none(),
            "il passo {engine} porta ancora il testo grezzo del motore: {seen}"
        );
    }
    // Both engines answered well: their errand — born from the trigger's text —
    // arrived, by two different roads.
    assert_eq!(output("engine_a")["answer"]["total"], 1);
    assert_eq!(output("engine_b")["answer"]["total"], 1);
    assert_eq!(output("verify")["answer"]["verdict"], "APPROVATO");
    assert!(
        output("verify")["answer"].get("why").is_some(),
        "il verificatore dichiara anche il perché"
    );
}

/// **THE MEASURED DEFECT, TESTED ON THE REAL FLOW.** An engine exiting in error
/// did not break its own step: it closed it green with `status: exit_error`
/// inside, and the chain went on. Here the first engine exits with 3, and all
/// three things must hold: its step is broken, the run is red **because of it**,
/// and the steps that depended on it never started — no call was spent
/// downstream.
#[test]
fn an_engine_that_fails_stops_the_chain_instead_of_colouring_it_green() {
    let graph = chain_with(
        "APPROVATO",
        Some(json!(["-c", "echo il-motivo 1>&2; exit 3"])),
    );

    let (execution, store) = run_with(
        &graph,
        &[("trigger", trigger_input())],
        EveryToolIsShell,
    );

    assert_eq!(
        last_decision(&execution),
        Decision::Failed(vec!["engine_a".to_owned()]),
        "la corsa è rossa, e il rosso porta il nome del passo che ha fallito"
    );
    let records = store.all();
    let record = records
        .iter()
        .find(|record| record.step_id == "engine_a")
        .expect("il passo è stato aperto");
    assert_eq!(record.outcome, Some(Outcome::Broke));
    assert_eq!(record.failure_class.as_deref(), Some("engine_exit_error"));
    let said = record.said.clone().unwrap_or_default();
    assert!(said.contains("code 3"), "{said}");
    assert!(said.contains("il-motivo"), "{said}");
    for never_ran in ["verify", "verdict"] {
        assert!(
            !store.all().iter().any(|record| record.step_id == never_ran),
            "il passo {never_ran} è partito lo stesso: una chiamata spesa nel vuoto"
        );
    }
    // The other engine, which did not depend on the first, ran: stopping does
    // not mean stopping everything.
    assert!(store
        .all()
        .iter()
        .any(|record| record.step_id == "engine_b"));
}

/// **THE FLOW CARRIED TO A MACHINE THAT LACKS THAT TOOL.** It does not start and
/// it says which one is missing — all whoever receives it needs. Before, the
/// question did not exist: the flow named a binary, and a missing binary became
/// `spawn_failed` inside a green step.
#[test]
fn a_machine_without_the_tool_stops_the_flow_saying_which_one() {
    let graph = chain_with("APPROVATO", None);

    let (execution, store) = run_with(
        &graph,
        &[("trigger", trigger_input())],
        NoToolIsHere,
    );

    assert_eq!(
        last_decision(&execution),
        Decision::Failed(vec!["dispatch".to_owned()])
    );
    let records = store.all();
    let record = records
        .iter()
        .find(|record| record.step_id == "dispatch")
        .expect("il passo è stato aperto");
    // THE CLASS DEPENDS ON HOW MANY IT ASKED FOR. A step that names a single
    // engine and does not find it stays `tool_unavailable`, with the resolver's
    // reason; one that declares a chain and finds none gives `no_usable_engine`,
    // since "that tool is missing" would be a partial answer out of three. What
    // this test defends is the same in both cases: the flow stops SAYING which
    // one was missing.
    assert!(
        matches!(
            record.failure_class.as_deref(),
            Some("tool_unavailable") | Some("no_usable_engine")
        ),
        "una macchina senza lo strumento deve fermare il flusso dicendolo: {:?}",
        record.failure_class
    );
    assert!(
        record
            .said
            .clone()
            .unwrap_or_default()
            .contains("claude-code"),
        "il messaggio deve dire quale strumento mancava: {:?}",
        record.said
    );
}

/// The final gate, run: **the same step, taken from the file**, with three
/// different answers. One case alone would prove nothing.
///
/// The case "an engine did not answer" is gone: that step now breaks by itself,
/// and the gate never even sees it. The check on other steps' states that used
/// to live in this command was the flow's only net, and the net someone could
/// remove without noticing.
#[test]
fn the_verdict_gate_closes_green_only_on_an_approved_verdict() {
    let flow = flow_file();
    let mut verdict = flow
        .graph
        .step("verdict")
        .expect("il cancello esiste")
        .clone();
    verdict.deps.clear();
    let graph = Graph::new(vec![verdict]).expect("un passo solo è un grafo valido");

    let outcome = |answer: Value| {
        let input = json!({"status": "ok", "answer": answer});
        let (execution, _) = run_with(
            &graph,
            &[("verdict", input)],
            EveryToolIsShell,
        );
        last_decision(&execution)
    };

    assert_eq!(
        outcome(json!({"verdict": "APPROVATO", "why": "ho guardato tre voci"})),
        Decision::Complete,
        "un verdetto favorevole chiude il flusso"
    );
    assert_eq!(
        outcome(json!({"verdict": "RESPINTO", "why": "due voci non esistono"})),
        Decision::Failed(vec!["verdict".to_owned()]),
        "il verdetto contrario deve tingere di rosso la corsa"
    );
    // No verdict where the gate looks for it: the reference finds nothing and
    // the step breaks. A gate that approved in this case would be the worst
    // green of all, because nobody judged.
    assert_eq!(
        outcome(json!({"why": "ho dimenticato la riga che conta"})),
        Decision::Failed(vec!["verdict".to_owned()]),
        "senza verdetto non si approva"
    );
}

/// **WHY NO STEP OF THIS FLOW CARRIES A `when`.** A skipped step is not a red:
/// its descendants never start, the front stays empty and the run closes
/// `Complete`. A flow that dispatches work to two paid engines and finishes
/// green without having called either is indistinguishable from one that worked.
///
/// This test measures the executor's behaviour, not the flow: the day a skipped
/// step paints the run red, it falls here, and the choice reopens.
#[test]
fn a_skipped_step_leaves_the_run_green_and_its_children_unrun() {
    let step = |id: &str, deps: &[&str], when: Option<Value>| Step {
        id: id.to_owned(),
        deps: deps.iter().map(|dep| (*dep).to_owned()).collect(),
        input_schema: flow::ValueSchema::Any,
        output_schema: flow::ValueSchema::Any,
        with: Some(json!({"command": "true", "timeout_secs": 5})),
        when: when.map(|value| serde_json::from_value(value).expect("condizione valida")),
        action: "shell_check".to_owned(),
        max_attempts: 1,
        ask_again_after_secs: None,
        retry_after_secs: None,
        phase: None,
        stops_when: None,
        decides_done: false,
    };
    let mut first = step("first", &[], None);
    first.with = None;
    let graph = Graph::new(vec![
        first,
        // "passed" is not "ok": the condition never fires, on purpose.
        step(
            "middle",
            &["first"],
            Some(json!({"kind": "pointer_equals", "pointer": "/status", "value": "ok"})),
        ),
        step("last", &["middle"], None),
    ])
    .expect("grafo valido");

    let (execution, _) = run_with(
        &graph,
        &[("first", json!({"command": "true", "timeout_secs": 5}))],
        EveryToolIsShell,
    );

    assert_eq!(
        last_decision(&execution),
        Decision::Complete,
        "un passo saltato chiude la corsa in verde: è il verde falso da evitare"
    );
}

/// The reference declared in the file really reaches the engine, **with the
/// step's real `tool`**: only the arguments are replaced, and the binary is
/// chosen by the resolver as it would be on any machine. The fake engine answers
/// well only if it found in the prompt the errand the node before wrote for it —
/// and that prompt must carry the demanded shape too, or the action stops before
/// starting.
#[test]
fn the_declared_reference_puts_the_dispatch_answer_on_the_engines_input() {
    let flow = flow_file();
    let mut engine = flow
        .graph
        .step("engine_a")
        .expect("il motore esiste")
        .clone();
    engine.deps.clear();
    let answer = json!({"findings": [], "total": 0}).to_string();
    let with = engine.with.as_mut().expect("il passo porta i suoi valori");
    answering_with(with, reads_stdin(&answer));
    let mut input = with.clone();
    input["status"] = json!("ok");
    input["answer"] = json!({
        "first_engine": format!("conta i ganci morti, {MARK}"),
        "second_engine": "un altro territorio",
        "why_first": "a territory of files",
        "why_second": "the other territory"
    });

    // **THE INPUT IS COMPOSED THE WAY THE EXECUTOR COMPOSES IT.** References are
    // resolved by `flow::step_input` — one place for all the actions, fault 28 —
    // and this test calls `execute` without going through it. Calling the real
    // function is how the action is tested in the world it runs in; without it,
    // it would be tested in a world that does not exist. That references reach
    // **every** action resolved is proved by
    // `crates/flow/tests/a_reference_reaches_every_action.rs`; here we prove the
    // resolved errand really lands in this engine's prompt.
    let input = flow::reference::resolve_references(&input).expect("i rinvii si sciolgono");

    // The step asks for a tree of its own, so this call needs a project to cut
    // it from and a store to write it down in: calling the action outside a run
    // is what the executor never does.
    let project = Project::new();
    let registry = registry_with(EveryToolIsShell, Some(project.1.clone()));
    let action = registry.get(&engine.action).expect("azione registrata");
    let mut shared = SharedState::new();
    shared.insert(flow::WORKSPACE_ROOT.to_owned(), json!(project.0.to_string_lossy()));
    shared.insert(flow::CURRENT_RUN.to_owned(), json!("prova"));
    shared.insert(flow::CURRENT_STEP.to_owned(), json!("engine_a"));
    let outcome = action
        .execute(&input, &shared)
        .expect("l'azione legge il rinvio e la forma è nel prompt");
    let flow::ActionOutcome::Went(output) = outcome else {
        panic!("un motore che risponde è sempre Went")
    };

    assert_eq!(
        output["answer"]["total"], 0,
        "il motore non ha ricevuto il proprio incarico: {output}"
    );
}

/// Every tool the flow names is declared by a descriptor: a misspelled
/// identifier is caught here, not with the run already started on a machine that
/// does have that tool.
#[test]
fn every_tool_the_flow_asks_for_is_declared_by_some_descriptor() {
    let flow = flow_file();
    let declared: BTreeMap<String, ()> = toolbox::Catalog::load(&[toolbox::Source::Builtin])
        .live()
        .into_iter()
        .map(|loaded| (loaded.descriptor.id.clone(), ()))
        .collect();

    for step in flow.graph.steps() {
        let Some(tool) = step
            .with
            .as_ref()
            .and_then(|with| with.get("tool"))
            .and_then(Value::as_str)
        else {
            continue;
        };
        assert!(
            declared.contains_key(tool),
            "il passo {} chiede «{tool}», che nessun descrittore spedito dichiara",
            step.id
        );
    }
}

/// The step that judges says so in the flow, where its author can read it and
/// change it. Nothing in the code decides which step is the judge.
#[test]
fn the_step_that_judges_is_declared_blind_in_the_flow() {
    let flow = flow_file();
    let blind: Vec<&str> = flow
        .graph
        .steps()
        .iter()
        .filter(|step| {
            step.with
                .as_ref()
                .and_then(|with| with.get(actions::BLIND))
                .and_then(Value::as_bool)
                == Some(true)
        })
        .map(|step| step.id.as_str())
        .collect();
    assert_eq!(
        blind,
        vec!["verify"],
        "the verify step is the one that must not continue the session of the \
         work it judges, and it is the only one held to it"
    );
}

/// The two engines of the parallel front each work in a tree of their own, so
/// neither can write over what the other is doing. Declared in the flow: the
/// tree is a property of the step, not a rule about steps called `engine_*`.
#[test]
fn the_two_engines_of_the_front_each_work_in_a_tree_of_their_own() {
    let flow = flow_file();
    let alone: Vec<&str> = flow
        .graph
        .steps()
        .iter()
        .filter(|step| {
            step.with
                .as_ref()
                .and_then(|with| with.get(actions::TREE))
                .and_then(Value::as_str)
                == Some(actions::A_TREE_OF_ITS_OWN)
        })
        .map(|step| step.id.as_str())
        .collect();
    assert_eq!(
        alone,
        vec!["engine_a", "engine_b"],
        "the two steps that write are the two that must not share a tree"
    );
}
