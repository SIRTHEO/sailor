//! What the command line and the desktop window must do the same way.
//!
//! Two things today: **which actions** Sailor can run, and **how a run's header
//! is recorded**. Both arrived here the same way — they were written twice and
//! drifted apart — so whoever finds a third brings it here instead of copying
//! it. Adding an action here means everybody gets it.

mod run_record;
mod subflow_host;

pub use run_record::{
    execution_status, halted_by_hand, how_it_stopped, record_child_run, record_flow_run,
    say_the_reason, stopped_by_cap, why_it_halted, why_it_stopped, FlowRun,
};
pub use subflow_host::LedgerHost;

use std::path::{Path, PathBuf};
use std::sync::Arc;

use flow::{ActionRegistry, ExecutionRequest, FlowFile, RunStops, SharedState};
use ledger::Ledger;
use toolbox::{Catalog, Machine, Source, Tools};

/// The request a run starts from, built in one place.
///
/// **A missing root stays missing.** `None` does not become the current
/// directory: whoever needs a root fails saying so, when the step's input is
/// composed. A silent fallback here would put a run launched from the window in
/// a different place from the same run launched from the terminal.
pub fn execution_request(
    ledger: Option<&Ledger>,
    flow: &FlowFile,
    run_id: &str,
    root: Option<&Path>,
    started_at: i64,
) -> ExecutionRequest {
    let mut shared = SharedState::new();
    if let Some(root) = root {
        // The value's type comes from `SharedState`, which keeps this crate
        // free of dependencies of its own — the reason it exists.
        shared.insert(
            flow::WORKSPACE_ROOT.to_owned(),
            root.display().to_string().into(),
        );
    }
    ExecutionRequest {
        run_id: run_id.to_owned(),
        root_inputs: flow.inputs.clone(),
        gates: Vec::new(),
        shared,
        // The cap is the flow's and travels with the run: the launcher only carries it.
        spend_cap_micros: flow.spend_cap_micros,
        stops: RunStops {
            // The start is asked for and not read off the clock: a resume
            // reading it here would grant itself the whole wall again.
            wall_deadline_at: flow.wall_secs.map(|secs| started_at + secs as i64),
            max_turns: flow.max_turns,
            // No store to ask, no count, and that is not zero: a machine that
            // cannot say how many turns were taken must not stop a run on the
            // strength of a number nobody read. Counted only where a flow
            // declares a limit, so an ordinary run asks nothing.
            turns_taken: flow
                .max_turns
                .and(ledger)
                .and_then(|ledger| ledger.turns_of_flow(&flow.id, run_id).ok()),
        },
    }
}

/// What the registry takes from the machine, in one place: Sailor's home,
/// where memories and flows live; the store's directory, where the fault and
/// session registers are kept; and the tools that can be resolved. The product
/// hands in this machine's; a test hands in a house of its own — see fault 5.
pub struct House {
    pub home: Option<PathBuf>,
    pub store_dir: Option<PathBuf>,
    pub tools: Tools,
}

impl House {
    /// The house of whoever runs this process.
    pub fn of_this_machine() -> House {
        House {
            home: ledger::sailor_home(),
            store_dir: ledger::default_directory(),
            tools: Tools::current(),
        }
    }

    /// No home, no store, and only the shipped descriptors on a machine with
    /// nothing installed: what a static check builds its registry from.
    pub fn empty() -> House {
        House {
            home: None,
            store_dir: None,
            tools: Self::shipped_tools(Machine::bare(PathBuf::from(toolbox::probe::NOWHERE))),
        }
    }

    /// Everything under one directory — the home, and the store beside it —
    /// with the shipped descriptors on a bare machine whose home is there.
    pub fn under(directory: &Path) -> House {
        House {
            home: Some(directory.to_path_buf()),
            store_dir: Some(directory.join("ledger")),
            tools: Self::shipped_tools(Machine::bare(directory.to_path_buf())),
        }
    }

    fn shipped_tools(machine: Machine) -> Tools {
        Tools::new(Catalog::load(&[Source::Builtin]), machine)
    }
}

/// The action registry of this machine: [`registry_in`] over the house of
/// whoever runs the process.
pub fn default_registry(
    ledger: Option<Ledger>,
    watcher: Option<Arc<dyn actions::StepSinks>>,
) -> ActionRegistry {
    registry_in(House::of_this_machine(), ledger, watcher)
}

/// The action registry: everything a step can ask to be done, over a house.
///
/// **Line order matters.** `actions::register_default` registers an external
/// engine that cannot resolve a tool by id; a line below *replaces* it with one
/// that can. Swap them and you get a registry that compiles, runs, and fails
/// every step naming a tool instead of a binary.
///
/// **The ledger is optional, and the difference is declared.** Running passes
/// one and gets the spend rows; a static check has none and must not — opening
/// a store to check a graph would create files for a question that touches
/// nothing. Nodes that *write* stay out when it is missing; the one that
/// *reads* history is registered anyway, because "no run recorded" is a good
/// answer rather than a failure.
pub fn registry_in(
    house: House,
    ledger: Option<Ledger>,
    watcher: Option<Arc<dyn actions::StepSinks>>,
) -> ActionRegistry {
    let House {
        home,
        store_dir,
        tools,
    } = house;
    let in_store = |file: &str| store_dir.as_ref().map(|directory| directory.join(file));
    let flows = home.as_ref().map(|home| home.join("flows"));
    let mut registry = ActionRegistry::default();
    actions::register_default(&mut registry);
    // Detecting what is on this machine is an action like any other: a step can
    // ask "which tools do I have here" instead of assuming.
    toolbox::register_default(&mut registry);
    // "Do these flows run here?" — the missing half, because a list of what
    // exists does not tell anyone what will stop working.
    toolbox::register_needs(&mut registry);
    // Where the signal that starts a flow comes from: sources are a list of
    // descriptors too, not a branch of code.
    trigger::register_default(&mut registry);
    // The four nodes a relay is composed of. They need no ledger and no
    // watcher: each one is a single power over a live terminal, and the order
    // they run in is a flow file rather than a function here.
    relay::register_relay(&mut registry);
    // The engine that resolves tools by id and receives the ledger. The
    // `run_id` does not exist yet here — it is born when the run starts and
    // reaches the action through the shared state.
    registry.register(
        actions::EXTERNAL_ENGINE_ACTION,
        actions::ExternalEngineAction::resolving_with(tools)
            .watched_by(watcher.clone())
            .recording_to(ledger.clone()),
    );
    // The watcher attaches to the instance that stays registered, not to the
    // replaced one, so this goes after `register_default` as well.
    registry.register(
        actions::SHELL_CHECK_ACTION,
        actions::ShellCheckAction::new().watched_by(watcher.clone()),
    );
    // The step that starts nothing: it describes the work and leaves it to the
    // agent already alive in the terminal. Same ordering, same reason —
    // without the watcher the mandate would only be visible once the run had
    // finished, which is when it is no use to anybody.
    registry.register(
        actions::handoff::HANDED_TO_AGENT_ACTION,
        actions::handoff::HandoffAction::new().watched_by(watcher.clone()),
    );
    actions::history::register_history(&mut registry, ledger.clone());
    // The fault register, reachable from a flow and not only from a person's
    // hands. Registered even where the store is absent, for the same reason as
    // the two above: `flow check` must be able to say the step names a real
    // action without opening anything.
    actions::faults::register_faults(&mut registry, in_store(faults::FAULTS_FILE));
    // The terminals Sailor follows: only the path is taken here.
    actions::terminals::register_terminals(&mut registry, in_store(sessions::SESSIONS_FILE));
    // A flow that runs another one. Registered **even without a ledger**, for
    // the reason declared above: `flow check` must be able to say a `subflow`
    // step names a real action without opening anything. Running without one
    // refuses instead, because a child run nobody can trace back to the step
    // that asked for it is the opacity this step was built against.
    let host: Arc<dyn flow::subflow::SubflowHost> =
        Arc::new(LedgerHost::new(ledger.clone(), watcher));
    registry.register(
        flow::subflow::SUBFLOW_ACTION,
        flow::subflow::SubflowAction::new(Arc::clone(&host)),
    );
    // The same flow once per element of a list, through the same host: the
    // children are subflow runs in the ledger, and one nesting level still
    // builds one registry.
    registry.register(
        flow::for_each::FOR_EACH_ACTION,
        flow::for_each::ForEachAction::new(host),
    );
    // Who is working on what, because an **agent** must be able to ask. The
    // reading half goes in without a store; the two that write stay out.
    actions::presence::register_presence(&mut registry, ledger.clone());
    actions::memory::register_memory(&mut registry, ledger.clone(), home);
    // The home is taken here, the flows are read when asked.
    actions::search::register_search(
        &mut registry,
        flows.clone(),
        ledger.clone(),
        in_store(faults::FAULTS_FILE),
    );
    if let Some(ledger) = ledger {
        actions::store::register_store(&mut registry, ledger);
    }
    // Last: the list it hands out is everything above.
    actions::draft::register_draft(&mut registry, flows);
    registry
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The actions the window's copy had lost along the way, named one by one.
    /// Removing any of them from `default_registry` turns this red, which is
    /// the only way to tell whoever comes next that a line went missing.
    /// A house of a test's own keeps everything under its directory, and the
    /// empty one holds nothing of any machine: neither reads the runner's.
    #[test]
    fn a_house_is_the_callers_and_never_the_machines() {
        let scratch = std::env::temp_dir().join(format!("sailor-house-{}", std::process::id()));
        let house = House::under(&scratch);
        assert_eq!(house.home.as_deref(), Some(scratch.as_path()));
        assert_eq!(house.store_dir, Some(scratch.join("ledger")));
        assert!(house.tools.declares("claude-code"), "the shipped descriptors are there");

        let empty = House::empty();
        assert_eq!(empty.home, None);
        assert_eq!(empty.store_dir, None);
        assert!(empty.tools.declares("claude-code"));
    }

    #[test]
    fn the_registry_carries_every_action_a_shipped_flow_can_name() {
        let registry = registry_in(House::empty(), None, None);
        for wanted in [
            actions::EXTERNAL_ENGINE_ACTION,
            actions::SHELL_CHECK_ACTION,
            actions::handoff::HANDED_TO_AGENT_ACTION,
            "detect_tools",
            "tool_needs",
            // The two nodes that talk to an MCP server. Sailor *recognised* MCP
            // servers — the detector has an `mcp_server` family — while no
            // action spoke to one.
            actions::mcp::MCP_READY_ACTION,
            actions::mcp::MCP_ASK_ACTION,
            // The window has always offered the `subflow` node; the engine did
            // not know it, so a flow drawn with it was refused as unknown.
            flow::subflow::SUBFLOW_ACTION,
            flow::for_each::FOR_EACH_ACTION,
            // The watch reads the terminals too, store or no store.
            actions::terminals::TERMINAL_SURVEY_ACTION,
            // The two ends of the consolidation a shipped flow runs once a day.
            actions::memory::MEMORY_LIST_ACTION,
            actions::memory::MEMORY_REPLACE_ACTION,
        ] {
            assert!(
                registry.get(wanted).is_some(),
                "«{wanted}» must be in the registry: without it, a flow naming it will not start"
            );
        }
    }

    /// The root reaches every action, not only the ones that go looking for it.
    /// A fake action records the `shared` it receives: the only way to show the
    /// value travels by construction rather than by the reader's courtesy.
    #[test]
    fn the_root_reaches_the_action_through_the_shared_state() {
        use flow::{Action, ActionError, ActionOutcome, Executor, SharedState as Shared};
        use std::sync::Mutex;

        /// Does nothing, remembers what it was given.
        struct Spy(Arc<Mutex<Option<Shared>>>);
        impl Action for Spy {
            fn execute(
                &self,
                _input: &serde_json::Value,
                shared: &Shared,
            ) -> Result<ActionOutcome, ActionError> {
                *self.0.lock().expect("the recorder") = Some(shared.clone());
                Ok(ActionOutcome::Went(serde_json::Value::Null))
            }
        }

        let seen = Arc::new(Mutex::new(None));
        let mut registry = ActionRegistry::default();
        registry.register("spia", Spy(seen.clone()));

        let json = r#"{
            "id": "prova", "description": "un passo solo",
            "graph": {"steps": [{
                "id": "unico", "deps": [], "action": "spia", "max_attempts": 1,
                "when": null, "input_schema": {"type": "any"},
                "output_schema": {"type": "any"}
            }]},
            "inputs": {}
        }"#;
        let flow: FlowFile = serde_json::from_str(json).expect("loading the flow");
        let request = execution_request(None, &flow, "corsa-1", Some(Path::new("/una/radice")), 0);

        let store = flow::InMemoryRecordStore::default();
        flow::InProcessExecutor
            .execute(
                &flow.graph,
                request,
                &store,
                &registry,
                &flow::SystemClock,
            )
            .expect("the run goes");

        let shared = seen
            .lock()
            .expect("the recorder")
            .clone()
            .expect("the step ran");
        assert_eq!(
            shared
                .get(flow::WORKSPACE_ROOT)
                .and_then(|root| root.as_str()),
            Some("/una/radice"),
            "the root must reach the action without the action asking for it"
        );
    }

    /// Without a root nothing is written: **absent is not the current
    /// directory**. A zero standing in for "I do not know" is the lie.
    #[test]
    fn without_a_root_nothing_is_written_into_the_shared_state() {
        let json = r#"{"id": "p", "description": "d",
            "graph": {"steps": []}, "inputs": {}}"#;
        let flow: FlowFile = serde_json::from_str(json).expect("loading the flow");

        let request = execution_request(None, &flow, "corsa-1", None, 0);

        assert!(
            !request.shared.contains_key(flow::WORKSPACE_ROOT),
            "no silent fallback to the process's own directory"
        );
    }

    /// Without a ledger the writing nodes stay out and the reading one stays
    /// in: the difference between a static check that creates files and one
    /// that touches nothing.
    #[test]
    fn without_a_ledger_the_writing_nodes_stay_out_and_the_reading_one_stays_in() {
        let registry = registry_in(House::empty(), None, None);
        assert!(
            registry.get("history_ask").is_some(),
            "reading history works without a ledger: the answer is «there is nothing»"
        );
        assert!(
            registry.get("store_put").is_none(),
            "writing does not: without a ledger it has nowhere to put anything"
        );
    }

    /// `subflow` is there without a ledger, and refuses to run.
    ///
    /// The two halves belong together. It must **be there**, or `flow check` —
    /// which opens no ledger — would call a valid flow unknown. And it must
    /// **refuse to run**, because a child run that never reaches the ledger
    /// cannot be traced back to the step that called it.
    #[test]
    fn without_a_ledger_the_subflow_step_is_registered_but_refuses_to_run() {
        let registry = registry_in(House::empty(), None, None);
        let step = registry
            .get(flow::subflow::SUBFLOW_ACTION)
            .expect("registered even without a ledger");

        let mut shared = flow::SharedState::new();
        shared.insert(flow::CURRENT_RUN.to_owned(), "una-corsa".into());
        shared.insert(flow::CURRENT_STEP.to_owned(), "un-passo".into());
        let refused = step
            .execute(&serde_json::json!({ "flow": "qualunque" }), &shared)
            .expect_err("without a ledger it must not run");

        assert_eq!(refused.class, "no_ledger");
        assert!(
            refused.said.contains("traced back"),
            "and it says why, not only that it cannot: {}",
            refused.said
        );
    }

    /// A child run carries its parent in the ledger, not only in its own name.
    /// Without `parent_run_id` written down, tracing back would mean guessing
    /// from a string.
    #[test]
    fn a_child_run_carries_the_parent_and_the_step_that_started_it() {
        let dir = std::env::temp_dir().join(format!("sailor-registry-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let ledger = Ledger::open(&dir).expect("test ledger");
        let flow: flow::FlowFile = serde_json::from_str(
            r#"{"id":"figlio","description":"x","graph":{"steps":[]},"inputs":{}}"#,
        )
        .expect("valid flow");

        record_child_run(
            &ledger,
            &flow,
            FlowRun {
                run_id: "corsa-figlia",
                status: "complete",
                started_at: 1,
                ended_at: Some(2),
                error: None,
                started_by: "subflow chiamata",
                stop_reason: None,
            },
            "corsa-del-padre",
        )
        .expect("recorded");

        // Read from the projection because the ledger has no reader for this
        // column: `FinishedRun` does not carry `parent_run_id`. The dump
        // exposes it by position — `run_id, kind, entity, parent_run_id,
        // started_by, …` — and it is the only road that does not require
        // changing the ledger.
        let dump = ledger.projection_dump().expect("projection");
        let child = dump["runs"]
            .as_array()
            .expect("the runs")
            .iter()
            .find(|row| row[0] == "corsa-figlia")
            .expect("the child run is there");
        assert_eq!(child[2], "figlio", "the flow that ran");
        assert_eq!(child[3], "corsa-del-padre", "the run that called it");
        assert_eq!(child[4], "subflow chiamata", "and the step that opened it");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
