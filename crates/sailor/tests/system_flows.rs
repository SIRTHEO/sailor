//! The flows shipped with the product, tested by running them.
//!
//! **WHY HERE AND NOT IN THE FLOW CRATE.** `crates/flow` knows the embedded
//! files are valid flows, and proves it. It does not know whether the actions
//! they name exist: the vocabulary is assembled by the program, a piece per
//! crate, and this is the only place it is seen whole. A shipped flow naming an
//! action nobody registers is the worst fault a product can carry — whoever
//! installs it cannot repair it, the file being inside the binary — so it must
//! fall here, before leaving the house.
//!
//! **AND THEN THEY REALLY RUN.** A flow that loads and does not run is a
//! well-written JSON file. These tests build the same registry
//! `sailor flow run` builds, run the flows over an in-memory store, and look at
//! what came out.

use flow::system;
use flow::{
    Action, ActionError, ActionOutcome, ActionRegistry, Clock, Decision, Execution,
    ExecutionRequest, Executor, FlowError, FlowFile, InMemoryRecordStore, InProcessExecutor,
    Outcome, RecordStore, SharedState, StepSpecies,
};
use sailor::flow_cmd::seeds::flows_in;
use serde_json::Value;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

/// The registry the product builds, not one rebuilt here: rebuilt, it held
/// neither `history_ask` nor the fault nodes, so a shipped flow naming one
/// passed a check the real program fails. Over an empty house — no store, no
/// home, nothing of this machine: the nodes that need one register and say so.
fn product_registry() -> ActionRegistry {
    registry::registry_in(registry::House::empty(), None, None)
}

/// A fake clock that advances by one at every question. The counter is atomic
/// because the clock is now shared across threads: a mutable `i64` would not
/// compile here, the same reason the trait asks for `&self`.
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

fn shipped(name: &str) -> FlowFile {
    system::builtin_registry()
        .remove(name)
        .unwrap_or_else(|| panic!("il flusso spedito «{name}» non c'è"))
        .expect("il flusso spedito si carica")
}

fn run(flow: &FlowFile) -> (Execution, Vec<flow::StepRecord>) {
    let store = InMemoryRecordStore::default();
    let run_id = format!("prova-{}", flow.id);
    let request = ExecutionRequest {
        holder: None,
        run_id: run_id.clone(),
        root_inputs: flow.inputs.clone().into_iter().collect(),
        gates: Vec::new(),
        shared: SharedState::new(),
        spend_cap_micros: None,
        stops: flow::RunStops::default(),
    };
    let execution = InProcessExecutor
        .execute(
            &flow.graph,
            request,
            &store,
            &product_registry(),
            &Tick::new(0),
        )
        .expect("l'esecuzione non deve rompersi");
    let records = store.records(&run_id).expect("le tracce della corsa");
    (execution, records)
}

/// The output of a step, or the reason there is none.
fn output_of(records: &[flow::StepRecord], step: &str) -> Value {
    let record = records
        .iter()
        .filter(|record| record.step_id == step)
        .max_by_key(|record| (record.attempt, record.epoch))
        .unwrap_or_else(|| panic!("il passo «{step}» non ha lasciato traccia"));
    assert_eq!(
        record.outcome,
        Some(Outcome::Went),
        "il passo «{step}» non è andato a buon fine: {:?}",
        record.failure_class
    );
    record
        .output
        .clone()
        .unwrap_or_else(|| panic!("il passo «{step}» non ha prodotto niente"))
}

// ── the vocabulary ───────────────────────────────────────────────────────

/// THE FAULT THIS TEST EXISTS TO CATCH: a shipped flow naming an action the
/// program does not register. Whoever installs it would see the flow listed,
/// run it, and get "missing action" on a file inside the binary. **The
/// registry is built as a run builds it**, with the store, or it would call
/// missing an action every real run has. That store is a scratch directory:
/// this test reads no state of this machine.
#[test]
fn every_action_named_by_a_shipped_flow_is_in_the_vocabulary() {
    let dir = std::env::temp_dir().join(format!("sailor-vocabolario-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("cartella di lavoro");
    let store = ledger::Ledger::open(&dir).expect("deposito di lavoro");
    let registry = registry::registry_in(registry::House::empty(), Some(store), None);
    for (name, entry) in system::builtin_registry() {
        let flow = entry.expect("il flusso spedito si carica");
        for step in flow.graph.steps() {
            assert!(
                registry.get(&step.action).is_some(),
                "il flusso spedito «{name}» chiede l'azione «{}» al passo «{}», \
                 che nessun crate registra",
                step.action,
                step.id
            );
        }
    }
}

/// A shipped flow must name no binary: it would run only where that name is on
/// the path of whoever runs it, and a system flow has to run on any machine at
/// all.
#[test]
fn no_shipped_flow_names_a_binary() {
    for (name, entry) in system::builtin_registry() {
        let flow = entry.expect("il flusso spedito si carica");
        for step in flow.graph.steps() {
            let named = step
                .with
                .as_ref()
                .and_then(|with| with.get("bin"))
                .or_else(|| flow.inputs.get(&step.id).and_then(|input| input.get("bin")));
            assert!(
                named.is_none(),
                "il flusso spedito «{name}» nomina un binario al passo «{}»",
                step.id
            );
        }
    }
}

/// **A MODEL NAMED IN THE FILE MUST REACH THE COMMAND LINE.** Flow and
/// descriptor ship together and never speak: until somebody puts them side by
/// side, a step can name a model to an engine that cannot receive one and
/// nothing goes red. The line is composed from the shipped recipe, not a copy
/// written here. The ordering half bites only for an engine with options glued
/// to the question; the general rule is measured in `actions`.
#[test]
fn a_model_a_shipped_flow_names_reaches_the_command_line_of_that_engine() {
    let catalog = toolbox::Catalog::load(&[toolbox::Source::Builtin]);
    assert!(catalog.problems.is_empty(), "{:?}", catalog.problems);
    let mut asked = 0;
    for (name, entry) in system::builtin_registry() {
        let flow = entry.expect("il flusso spedito si carica");
        for step in flow.graph.steps() {
            let Some(wanted) = step
                .with
                .as_ref()
                .and_then(|with| with.get("model"))
                .and_then(Value::as_object)
            else {
                continue;
            };
            for (id, model) in wanted {
                let model = model.as_str().expect("un nome di modello è testo");
                let descriptor = &catalog
                    .descriptors
                    .iter()
                    .find(|loaded| loaded.descriptor.id == *id)
                    .unwrap_or_else(|| {
                        panic!(
                            "«{name}» chiede un modello a «{id}», che nessun descrittore dichiara"
                        )
                    })
                    .descriptor;
                let recipe = toolbox::ask_recipe_of(descriptor).unwrap_or_else(|| {
                    panic!("«{name}» chiede un modello a «{id}», che non si sa interrogare")
                });
                let option = descriptor.model_option().unwrap_or_else(|| {
                    panic!(
                        "«{name}» chiede a «{id}» il modello «{model}», e il suo descrittore non \
                         dichiara come glielo si nomina"
                    )
                });
                let line = actions::command_line_naming_model(&recipe, &option, model);
                let at = line
                    .iter()
                    .position(|written| written == model)
                    .expect("il nome del modello è sulla riga");
                for glued in &recipe.args_before_prompt {
                    let glued_at = line
                        .iter()
                        .position(|written| written == glued)
                        .expect("ciò che sta attaccato alla domanda è sulla riga");
                    assert!(
                        at < glued_at,
                        "«{id}»: il modello «{model}» sta dopo «{glued}», che deve restare \
                         attaccato alla domanda — verrebbe letto come la domanda. Riga: {line:?}"
                    );
                }
                // The real line, for whoever reads this output instead of the file.
                println!("«{id}» ← «{model}»: {line:?}");
                asked += 1;
            }
        }
    }
    assert!(
        asked > 0,
        "nessun flusso spedito nomina un modello: la prova non ha guardato niente"
    );
}

/// The options a shipped descriptor says this engine's model is named after,
/// and `None` for one that cannot be told a model at all: `git` and `ollama`
/// carry theirs in the arguments, and asking them for a `model` is a repair
/// that would be refused.
fn how_a_model_is_named_to(catalog: &toolbox::Catalog, engine: &str) -> Option<Vec<String>> {
    catalog
        .descriptors
        .iter()
        .find(|loaded| loaded.descriptor.id == engine)
        .and_then(|loaded| loaded.descriptor.model_option())
}

/// **THE CONVERSE, AND THE RULE: A STEP THAT NAMES NO MODEL IS NOT A STEP WITH
/// A DEFAULT ONE.** Whoever answers picks it, and it changes underneath: two
/// runs of the same flow a month apart ask two models at two prices, and the
/// ledger keeps `requested_model` empty on both, with nothing to tell them
/// apart. `sailor flow check` says it of the flow in front of it; this says it
/// of everything the repository ships, from both homes of a flow file.
#[test]
fn no_shipped_flow_asks_an_engine_without_naming_its_model() {
    let catalog = toolbox::Catalog::load(&[toolbox::Source::Builtin]);
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("the crate sits two levels under the root")
        .to_path_buf();
    let flows = flows_in(&root);
    let mut asked = 0usize;
    let mut unnamed: Vec<String> = Vec::new();
    for (name, path) in &flows {
        let text = std::fs::read_to_string(path).expect("a flow file of this tree reads");
        let flow: FlowFile = serde_json::from_str(&text).expect("a flow file of this tree parses");
        for step in flow.graph.steps() {
            if step.action != actions::EXTERNAL_ENGINE_ACTION {
                continue;
            }
            let Some(with) = step.with.as_ref() else {
                continue;
            };
            let named = actions::models_named_in(with);
            // A step writing its own command line is refused a `model` field:
            // the model it asks for is a flag on that line or nowhere.
            let by_hand = with
                .get("args")
                .and_then(Value::as_array)
                .filter(|args| !args.is_empty());
            for engine in actions::engines_named_in(with) {
                let Some(option) = how_a_model_is_named_to(&catalog, &engine) else {
                    continue;
                };
                asked += 1;
                let told = match by_hand {
                    Some(args) => args
                        .iter()
                        .filter_map(Value::as_str)
                        .any(|written| option.iter().any(|flag| flag == written)),
                    None => named.contains_key(&engine),
                };
                if told {
                    continue;
                }
                let repair = match by_hand {
                    Some(_) => format!("«{}» on the command line the step writes", option.join(" ")),
                    None => "a `model` for it in the step".to_owned(),
                };
                unnamed.push(format!("\n  {name} → {} ({engine}): write {repair}", step.id));
            }
        }
    }
    workspace::measured_against(
        asked,
        "engines a shipped step could be told a model of",
        flows.len(),
        "flow files read",
    );
    assert!(
        asked > 0,
        "no engine step was read, and a judge that reads nothing says nothing"
    );
    assert!(
        unnamed.is_empty(),
        "{} engines of the shipped flows run on whatever model answers that day, and the \
         ledger cannot tell two runs of the same flow apart:{}",
        unnamed.len(),
        unnamed.concat()
    );
}

/// **NO SHIPPED FLOW CARRIES A PATH FROM ONE MACHINE.** The guarantee existed
/// and sat in the wrong place: it watched this project's development flows,
/// which left the repository for Sailor's house, where anybody's flows live.
/// Those were ours and could afford an absolute path; **these are installed on
/// machines we do not know**, and that is where the rule is needed.
#[test]
fn no_shipped_flow_carries_a_path_from_one_machine() {
    for (name, _) in system::FLOWS {
        let text = system::FLOWS
            .iter()
            .find(|(id, _)| id == name)
            .map(|(_, body)| *body)
            .expect("il flusso spedito ha un corpo");
        for home in ["/Users/", "/home/", "C:\\Users\\"] {
            assert!(
                !text.contains(home),
                "il flusso spedito «{name}» porta un percorso di una macchina sola ({home}): \
                 su qualunque altra non parte"
            );
        }
    }
}

/// **WHAT A PERSON CAN ASK, THE GATE ASKS BY ITSELF.** The fault it exists to
/// catch: a shipped flow whose references cannot reach the input its steps get.
/// One shipped with three of them, passed the whole gate, went through the
/// merge, and was found by the first person who ran it. It asks `flow check`'s
/// own refusals and keeps no second copy of the rule; a refusal names flow,
/// step, field and pointer, so a red here says what to fix.
#[test]
fn every_shipped_flow_passes_the_check_a_person_would_run_on_it() {
    let registry = product_registry();
    for (name, entry) in system::builtin_registry() {
        let flow = entry.expect("il flusso spedito si carica");
        let refused = sailor::flow_cmd::check::refusals_of(&flow, &registry);
        assert!(
            refused.is_empty(),
            "il flusso spedito «{name}» non passa il proprio controllo:\n{}",
            refused.join("\n")
        );
    }
}

/// An action that answers what it was told to, and keeps what it was asked.
/// It stands in for the engine and the trigger so no paid call is ever made
/// and no descriptor of this machine is read; every other node is the real one.
struct Scripted {
    said: Value,
    seen: Arc<Mutex<Vec<Value>>>,
}

impl Action for Scripted {
    fn execute(&self, input: &Value, _shared: &SharedState) -> Result<ActionOutcome, ActionError> {
        self.seen.lock().expect("nessuno rompe qui").push(input.clone());
        Ok(ActionOutcome::Went(self.said.clone()))
    }

    fn species(&self) -> StepSpecies {
        StepSpecies::Repeatable
    }
}

/// **THE CONSULTATION RUNS, AND LANDS IN THE STORE.** Engine and trigger are
/// scripted — no paid call, no descriptor of this machine — and the rest is
/// real: the check runs, the store node writes into a scratch store. Only a
/// run could say this flow did not run: it loaded and named actions that exist.
#[test]
fn the_consultation_runs_and_the_store_gets_the_entry_it_demands() {
    let dir = std::env::temp_dir().join(format!("sailor-consulto-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("cartella di lavoro");
    let deposit = ledger::Ledger::open(&dir).expect("deposito di lavoro");

    let brief = format!("il materiale della consulenza, {}", "x".repeat(500));
    let answer = serde_json::json!({
        "diagnosis": "la forma sotto gli incidenti",
        "moves": ["la prima mossa"],
        "not_covered": "cosa passerebbe lo stesso",
        "would_change_my_mind": "la misura che ribalta"
    });
    let asked = Arc::new(Mutex::new(Vec::new()));
    let mut registry = registry::registry_in(registry::House::empty(), Some(deposit.clone()), None);
    registry.register(
        "trigger",
        Scripted {
            said: serde_json::json!({"text": brief}),
            seen: Arc::new(Mutex::new(Vec::new())),
        },
    );
    registry.register(
        "external_engine",
        Scripted {
            said: serde_json::json!({"status": "ok", "answer": answer}),
            seen: Arc::clone(&asked),
        },
    );

    let flow = shipped("consult-a-strong-model");
    let store = InMemoryRecordStore::default();
    let run_id = format!("consulto-{}", std::process::id());
    let request = ExecutionRequest {
        holder: None,
        run_id: run_id.clone(),
        root_inputs: flow.inputs.clone().into_iter().collect(),
        gates: Vec::new(),
        shared: SharedState::new(),
        spend_cap_micros: None,
        stops: flow::RunStops::default(),
    };
    let execution = InProcessExecutor
        .execute(&flow.graph, request, &store, &registry, &Tick::new(0))
        .expect("l'esecuzione non deve rompersi");
    let records = store.records(&run_id).expect("le tracce della corsa");

    assert_eq!(
        execution.decisions.last(),
        Some(&Decision::Complete),
        "la corsa deve chiudersi: {:?}",
        execution.decisions
    );
    // THE MATERIAL REACHES THE ENGINE, and this half is the real fault: the
    // step read «/text» from a dependency that does not produce it, so the
    // question would have gone out empty — and been paid for all the same.
    let stdin = asked.lock().expect("nessuno rompe qui")[0]["stdin"]
        .as_str()
        .expect("la domanda è testo")
        .to_owned();
    assert!(stdin.contains(&brief), "il materiale non è nella domanda");
    assert!(
        stdin.contains("would_change_my_mind"),
        "la forma della risposta non è nella domanda:\n{stdin}"
    );
    // The deterministic check really read the material instead of being
    // skipped: it is what keeps an empty consultation away from the engine.
    assert_eq!(output_of(&records, "brief")["answer"]["bytes"], brief.len());

    let kept = deposit
        .read_record("consultations", &run_id)
        .expect("il deposito si legge")
        .expect("la consulenza sta nel deposito, sotto la corsa che l'ha pagata");
    assert_eq!(kept.value, answer, "si tiene la risposta, non lo stato");
    assert_eq!(kept.written_by, "consult-a-strong-model");
    let _ = std::fs::remove_dir_all(&dir);
}

// ── the first flow: what is here, and what is missing ────────────────────

/// IT RUNS, AND THE SECOND HALF IS THE ANSWER. Detection alone is a list; the
/// step after it says which tools the flows of this machine ask for, and which
/// of those are not here.
#[test]
fn the_tools_flow_runs_and_answers_which_flows_would_stop() {
    let flow = shipped("what-this-machine-has");
    let (execution, records) = run(&flow);

    assert_eq!(
        execution.decisions.last(),
        Some(&Decision::Complete),
        "la corsa deve chiudersi: {:?}",
        execution.decisions
    );

    let detected = output_of(&records, "rileva");
    let total = detected["total"]
        .as_u64()
        .expect("un conto dei descrittori");
    assert!(
        total >= 30,
        "il rilevamento ha guardato solo {total} descrittori: il catalogo spedito ne ha molti di più"
    );

    let answer = output_of(&records, "cosa-chiedono-i-flussi");
    let flows_seen = answer["flows_seen"].as_u64().expect("un conto dei flussi");
    assert!(
        flows_seen >= 2,
        "i due flussi spediti si vedono sempre, e ne ho visti {flows_seen}"
    );
    let report = answer["report"].as_str().expect("una risposta da leggere");
    assert!(
        report.contains("flows read in"),
        "la risposta deve essere leggibile da una persona: {report}"
    );
    // THE TOOLS ASKED FOR ARE ONE SET: a tool cannot be present and missing at
    // once, and a reader of two overlapping lists installs the same thing twice.
    let names = |key: &str| -> Vec<String> {
        answer[key]
            .as_array()
            .expect("un elenco")
            .iter()
            .map(|need| need["tool"].as_str().expect("un nome").to_owned())
            .collect()
    };
    let mut all = names("present");
    all.extend(names("missing"));
    all.extend(names("unknown"));
    let mut unique = all.clone();
    unique.sort();
    unique.dedup();
    assert_eq!(
        all.len(),
        unique.len(),
        "uno strumento sta in un elenco solo"
    );
}

/// «NOT ASKED» IS NOT «NOT THERE»: whoever turns the version commands off must
/// see every version become a question never put, not an empty answer.
#[test]
fn the_tools_flow_asks_for_versions_only_if_told_to() {
    let flow = shipped("what-this-machine-has");
    let asked = flow.inputs["rileva"]["version_probes"]
        .as_bool()
        .expect("il flusso dichiara se chiedere le versioni");
    assert!(
        asked,
        "il flusso di sistema chiede le versioni: è la parte del rilevamento che \
         costa, e spegnerla di nascosto renderebbe l'elenco più povero senza dirlo"
    );
}

// ── the second flow: what you already automate ───────────────────────────

/// IT RUNS, AND THE FOUR FAMILIES ARE THE VERDICT. Each step looks at one
/// family of automations, and the step's name says what could be done with it.
#[test]
fn the_migration_flow_runs_and_looks_at_four_families() {
    let flow = shipped("migrate-to-sailor");
    let (execution, records) = run(&flow);

    assert_eq!(
        execution.decisions.last(),
        Some(&Decision::Complete),
        "la corsa deve chiudersi: {:?}",
        execution.decisions
    );

    let steps = [
        "ganci",
        "pianificate",
        "script-sparsi",
        "viste-ma-non-lette",
    ];
    let mut families: Vec<String> = Vec::new();
    for step in steps {
        let output = output_of(&records, step);
        let findings = output["findings"].as_array().expect("un elenco");
        assert!(
            !findings.is_empty(),
            "il passo «{step}» non ha guardato niente: un descrittore che non trova \
             nulla lascia comunque la riga che dice dove ha guardato"
        );
        for finding in findings {
            families.push(finding["family"].as_str().expect("una famiglia").to_owned());
        }
    }
    families.sort();
    families.dedup();
    assert_eq!(
        families,
        vec![
            "automation_hook",
            "automation_opaque",
            "automation_schedule",
            "automation_script"
        ],
        "ogni passo guarda la sua famiglia e nessun'altra"
    );
}

/// LOOKING MUST NOT MEAN LAUNCHING. This flow reads the personal configuration
/// of whoever runs it: if a catalogue descriptor declared a version command,
/// detection would **execute** a program for the sole fact of being looked at.
#[test]
fn the_migration_flow_never_runs_anything() {
    let flow = shipped("migrate-to-sailor");
    let (_, records) = run(&flow);

    for step in [
        "ganci",
        "pianificate",
        "script-sparsi",
        "viste-ma-non-lette",
    ] {
        for finding in output_of(&records, step)["findings"]
            .as_array()
            .expect("un elenco")
        {
            assert_eq!(
                finding["version"]["state"], "notasked",
                "il passo «{step}» ha interrogato «{}»",
                finding["name"]
            );
        }
    }
}

/// OTHER PEOPLE'S AUTOMATIONS ARE NOT TOOLS A STEP MAY INVOKE. They sit in a
/// separate catalogue on purpose: in the tools catalogue their identifier would
/// appear among those Sailor suggests to whoever mistyped one, and a step could
/// name one as if it were a binary.
#[test]
fn the_automations_catalog_does_not_leak_into_the_tools() {
    let tools = registry::House::empty().tools;
    let automations: BTreeMap<String, ()> =
        toolbox::Catalog::load(&[toolbox::Source::BuiltinNamed("automations".to_owned())])
            .live()
            .into_iter()
            .map(|loaded| (loaded.descriptor.id.clone(), ()))
            .collect();

    assert!(
        !automations.is_empty(),
        "il catalogo delle automazioni non è vuoto"
    );
    for id in automations.keys() {
        assert!(
            !tools.declares(id),
            "«{id}» è un'automazione e compare fra gli strumenti invocabili"
        );
    }
}

/// A mistyped catalogue name must become a report, not an empty list: the two
/// read alike — «nothing here» — and one of them is a mistake by whoever wrote
/// the step.
#[test]
fn a_misspelled_catalog_is_a_problem_not_an_empty_list() {
    let catalog =
        toolbox::Catalog::load(&[toolbox::Source::BuiltinNamed("automazioni".to_owned())]);
    assert!(catalog.live().is_empty());
    assert_eq!(catalog.problems.len(), 1, "{:?}", catalog.problems);
    assert!(
        catalog.problems[0].reason.contains("automations"),
        "la segnalazione deve dire quali cataloghi esistono: {}",
        catalog.problems[0].reason
    );
}

/// THE RULE AGAINST EXECUTING LIVES IN THE CATALOGUE, NOT ONLY IN THE FLOW. The
/// migration flow turns version commands off with `"version_probes": false`, but
/// that line is a choice in a file, lost without warning by anyone overwriting
/// the flow with their own. Measured by adding a `version` to an automations
/// descriptor: **no test went red**. This catalogue reads a person's own
/// configuration, and must launch nothing by construction, not by courtesy.
#[test]
fn no_automation_descriptor_may_run_anything() {
    let catalog =
        toolbox::Catalog::load(&[toolbox::Source::BuiltinNamed("automations".to_owned())]);
    assert!(!catalog.live().is_empty(), "il catalogo non è vuoto");
    for loaded in catalog.live() {
        assert!(
            loaded.descriptor.version.is_none(),
            "«{}» dichiara un comando di versione: guardare le automazioni di una \
             persona diventerebbe eseguire qualcosa sulla sua macchina",
            loaded.descriptor.id
        );
    }
}

// ── the heart of the crossing, on flows built here ───────────────────────

/// THE CROSSING ITSELF, ON FLOWS OF OUR OWN. The tests that run the system flow
/// look at the real machine, where the shipped flows ask for no tool: emptying
/// the collection of tools asked for — measured, broken on purpose — turned **no
/// test red**, since «zero tools asked for» is the right answer on a clean
/// machine and the wrong one here. Here we write the flows, so the answer is
/// known, and the three cases nobody may confuse come in one shot: a tool that
/// is present, one no descriptor declares — which installing nothing repairs —
/// and a step naming a binary, which no list of missing tools could ever see.
#[test]
fn the_crossing_says_who_asked_for_what() {
    let dir = std::env::temp_dir().join(format!("sailor-incrocio-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("cartella di prova");
    std::fs::write(
        dir.join("chiede.flow.json"),
        serde_json::json!({
            "id": "chiede",
            "description": "un flusso che chiede",
            "graph": { "steps": [
                {"id": "con-strumento", "deps": [], "action": "external_engine", "max_attempts": 1,
                 "when": null, "with": {"tool": "codex"},
                 "input_schema": {"type": "any"}, "output_schema": {"type": "any"}},
                {"id": "con-nome-inventato", "deps": [], "action": "external_engine", "max_attempts": 1,
                 "when": null, "with": {"tool": "arnese-che-non-esiste"},
                 "input_schema": {"type": "any"}, "output_schema": {"type": "any"}},
                {"id": "con-binario", "deps": [], "action": "shell_check", "max_attempts": 1,
                 "when": null, "with": {"bin": "echo"},
                 "input_schema": {"type": "any"}, "output_schema": {"type": "any"}}
            ]},
            "inputs": {}
        })
        .to_string(),
    )
    .expect("scrittura del flusso");

    let registry = product_registry();
    let action = registry.get("tool_needs").expect("l'azione è registrata");
    let outcome = action
        .execute(
            &serde_json::json!({
                "include_default_sources": false,
                "flows_dirs": [dir.to_string_lossy()],
                "findings": [{
                    "name": "codex",
                    "family": "ai_cli",
                    "label": "Codex",
                    "descriptor_id": "codex",
                    "descriptor_source": "incorporato",
                    "presence": {"state": "present", "reason": "trovato"},
                    "executable": "/da/qualche/parte/codex",
                    "version": {"state": "notasked", "detail": "non chiesta"},
                    "config": [],
                    "note": "si installa così"
                }]
            }),
            &SharedState::new(),
        )
        .expect("l'azione non deve rompersi");

    let flow::ActionOutcome::Went(answer) = outcome else {
        panic!("l'azione deve produrre una risposta");
    };
    let _ = std::fs::remove_dir_all(&dir);

    assert_eq!(answer["flows_seen"], 1);
    assert_eq!(answer["present"][0]["tool"], "codex");
    assert_eq!(answer["present"][0]["asked_by"][0], "chiede/con-strumento");
    assert_eq!(
        answer["present"][0]["executable"],
        "/da/qualche/parte/codex"
    );
    assert_eq!(answer["missing"].as_array().expect("un elenco").len(), 0);
    assert_eq!(answer["unknown"][0]["tool"], "arnese-che-non-esiste");
    assert_eq!(
        answer["unknown"][0]["asked_by"][0],
        "chiede/con-nome-inventato"
    );
    assert_eq!(answer["steps_naming_a_binary"][0], "chiede/con-binario");

    let report = answer["report"].as_str().expect("una risposta da leggere");
    for atteso in [
        "codex",
        "arnese-che-non-esiste",
        "chiede/con-strumento",
        "chiede/con-binario",
    ] {
        assert!(report.contains(atteso), "manca «{atteso}» in:\n{report}");
    }
}

/// A TOOL DETECTED AND ABSENT IS NOT A TOOL UNKNOWN, and the two repairs are
/// opposite: the first is installed, the second is written. A list that mixes
/// them sends someone to install a name that does not exist.
#[test]
fn a_tool_that_is_absent_is_not_a_tool_that_is_unknown() {
    let dir = std::env::temp_dir().join(format!("sailor-assente-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("cartella di prova");
    std::fs::write(
        dir.join("chiede.flow.json"),
        serde_json::json!({
            "id": "chiede",
            "description": "un flusso che chiede uno strumento non installato",
            "graph": { "steps": [
                {"id": "passo", "deps": [], "action": "external_engine", "max_attempts": 1,
                 "when": null, "with": {"tool": "docker"},
                 "input_schema": {"type": "any"}, "output_schema": {"type": "any"}}
            ]},
            "inputs": {}
        })
        .to_string(),
    )
    .expect("scrittura del flusso");

    let registry = product_registry();
    let outcome = registry
        .get("tool_needs")
        .expect("l'azione è registrata")
        .execute(
            &serde_json::json!({
                "include_default_sources": false,
                "flows_dirs": [dir.to_string_lossy()],
                "findings": [{
                    "name": "docker",
                    "family": "tool",
                    "label": "Docker",
                    "descriptor_id": "docker",
                    "descriptor_source": "incorporato",
                    "presence": {"state": "absent", "reason": "nessun `docker` nelle cartelle del percorso"},
                    "version": {"state": "notasked", "detail": "non è qui"},
                    "config": [],
                    "note": "si prende da docker.com"
                }]
            }),
            &SharedState::new(),
        )
        .expect("l'azione non deve rompersi");

    let flow::ActionOutcome::Went(answer) = outcome else {
        panic!("l'azione deve produrre una risposta");
    };
    let _ = std::fs::remove_dir_all(&dir);

    assert_eq!(answer["unknown"].as_array().expect("un elenco").len(), 0);
    assert_eq!(answer["missing"][0]["tool"], "docker");
    assert_eq!(answer["missing"][0]["note"], "si prende da docker.com");
    let report = answer["report"].as_str().expect("una risposta");
    assert!(report.contains("MISSING HERE"), "{report}");
    assert!(report.contains("si prende da docker.com"), "{report}");
}

/// AN INPUT WITH NO DETECTION IS A MISTAKE BY WHOEVER WROTE THE STEP, not an
/// empty list: with no `findings` every tool asked for would look unknown, and
/// the reader would go write descriptors for tools already installed.
#[test]
fn without_a_detection_the_step_refuses_instead_of_guessing() {
    let registry = product_registry();
    let error = registry
        .get("tool_needs")
        .expect("l'azione è registrata")
        .execute(&serde_json::json!({}), &SharedState::new())
        .expect_err("senza rilevamento il passo non può rispondere");
    assert_eq!(error.class, "invalid_input");
}

// ── the graph a flow reads back ──────────────────────────────────────────

/// Like `run`, but over a **real, shared ledger** instead of `House::empty()`
/// with no store: `memory_write`/`memory_query` need one to have anything to
/// disagree about between calls, and the trigger's `text` is overridden
/// instead of taken from the flow's own shipped default.
fn run_with_ledger(
    flow: &FlowFile,
    ledger: &ledger::Ledger,
    mandate_text: &str,
) -> (Execution, Vec<flow::StepRecord>) {
    let store = InMemoryRecordStore::default();
    let run_id = format!(
        "prova-{}-{}",
        flow.id,
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("l'orologio")
            .as_nanos()
    );
    let mut root_inputs: BTreeMap<String, Value> = BTreeMap::new();
    root_inputs.insert(
        "trigger".to_owned(),
        serde_json::json!({"source": "manual", "text": mandate_text}),
    );
    let registry = registry::registry_in(registry::House::empty(), Some(ledger.clone()), None);
    let request = ExecutionRequest {
        holder: None,
        run_id: run_id.clone(),
        root_inputs,
        gates: Vec::new(),
        shared: SharedState::new(),
        spend_cap_micros: None,
        stops: flow::RunStops::default(),
    };
    let execution = InProcessExecutor
        .execute(&flow.graph, request, &store, &registry, &Tick::new(0))
        .expect("l'esecuzione non deve rompersi");
    let records = store.records(&run_id).expect("le tracce della corsa");
    (execution, records)
}

/// **THE FIRST FLOW WHOSE OWN ANSWER CHANGES WITH WHAT THE GRAPH HOLDS.**
/// Three states against the same ledger: nothing written, a node written
/// once, and the same node written again — the superseded revision must
/// never win.
#[test]
fn a_flow_that_reads_the_graph_answers_differently_as_it_changes() {
    let dir = std::env::temp_dir().join(format!("sailor-recall-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("cartella di lavoro");
    let ledger = ledger::Ledger::open(&dir).expect("deposito di lavoro");

    let remember = shipped("remember-in-the-graph");
    let recall = shipped("recall-a-decision");
    let ask = r#"{"workspace_id":"sailor","node_id":"the-question"}"#;

    // Nothing written yet: the flow has to say so, not guess.
    let (_, records) = run_with_ledger(&recall, &ledger, ask);
    let answer = output_of(&records, "answer");
    assert_eq!(answer["answer"]["found"], serde_json::json!(false), "{answer}");

    // Written once: the flow answers with it.
    run_with_ledger(
        &remember,
        &ledger,
        r#"{"workspace_id":"sailor","node_id":"the-question","kind":"decision","title":"Prima versione","written_by":"test"}"#,
    );
    let (_, records) = run_with_ledger(&recall, &ledger, ask);
    let answer = output_of(&records, "answer");
    assert_eq!(answer["answer"]["title"], serde_json::json!("Prima versione"), "{answer}");

    // Written again for the same node_id: the second revision answers, and
    // the first — still sitting in the store, marked superseded — never does.
    run_with_ledger(
        &remember,
        &ledger,
        r#"{"workspace_id":"sailor","node_id":"the-question","kind":"decision","title":"Seconda versione, quella vera","written_by":"test"}"#,
    );
    let (_, records) = run_with_ledger(&recall, &ledger, ask);
    let answer = output_of(&records, "answer");
    assert_eq!(
        answer["answer"]["title"],
        serde_json::json!("Seconda versione, quella vera"),
        "a superseded revision must never win: {answer}"
    );

    let _ = std::fs::remove_dir_all(&dir);
}

/// **A WRITE INTO THE GRAPH IS CLAIMED, NOT JUST SENT.** Two runs racing the
/// same `{workspace_id}:{node_id}` must not both win; two runs on different
/// `node_id`s in the same workspace must not even notice each other.
#[test]
fn a_write_into_the_graph_is_refused_while_another_agent_holds_the_same_node() {
    let dir = std::env::temp_dir().join(format!("sailor-claimed-write-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("cartella di lavoro");
    let ledger = ledger::Ledger::open(&dir).expect("deposito di lavoro");

    // Somebody else is already at work on the very same node. The claim step
    // inside the flow reads the real clock, not the fake one the executor
    // uses for its own sequencing — so this has to be real "now" too.
    let held_at = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("l'orologio")
        .as_secs() as i64;
    let holder = actions::presence::Claim {
        agent: "another-agent".to_owned(),
        key: actions::presence::claim_key("another-agent", "999"),
        repository: "sailor-memory-graph".to_owned(),
        workdir: Some("sailor:the-question".to_owned()),
        branch: None,
        paths: Vec::new(),
        doing: Some("already remembering this one".to_owned()),
        pid: 999,
        at: held_at,
        lease_seconds: 900,
        conversation: None,
        state: "working".to_owned(),
    };
    ledger
        .put_record(&actions::presence::claim_record(&holder))
        .expect("la presa concorrente si scrive");

    let remember = shipped("remember-in-the-graph");
    let mandate = r#"{"workspace_id":"sailor","node_id":"the-question","kind":"decision","title":"Tentativo mentre un altro tiene lo stesso nodo","written_by":"me"}"#;

    // While the other claim holds, the write must not go through.
    let (execution, records) = run_with_ledger(&remember, &ledger, mandate);
    assert_ne!(
        execution.decisions.last(),
        Some(&Decision::Complete),
        "una scrittura concorrente sullo stesso nodo non deve mai completare"
    );
    let claim_record = records
        .iter()
        .find(|record| record.step_id == "claim")
        .expect("il passo «claim» ha lasciato traccia");
    assert_eq!(claim_record.failure_class.as_deref(), Some("work_is_shared"));
    assert!(
        ledger
            .records_in(actions::graph_memory::NODES_COLLECTION)
            .expect("il grafo si legge")
            .is_empty(),
        "il nodo non deve esistere finché la presa altrui è viva"
    );

    // A different node, in the same workspace, is untouched by the collision.
    let (execution, records) = run_with_ledger(
        &remember,
        &ledger,
        r#"{"workspace_id":"sailor","node_id":"unrelated","kind":"decision","title":"Non c'entra niente","written_by":"me"}"#,
    );
    assert_eq!(execution.decisions.last(), Some(&Decision::Complete));
    assert_eq!(
        output_of(&records, "remember")["revision"],
        serde_json::json!(1)
    );

    // Once the other agent releases, the same write goes through.
    actions::presence::release_claim(&ledger, &holder.key, held_at + 60)
        .expect("il rilascio non deve rompersi");
    let (execution, records) = run_with_ledger(&remember, &ledger, mandate);
    assert_eq!(
        execution.decisions.last(),
        Some(&Decision::Complete),
        "rilasciata la presa altrui, la scrittura deve riuscire"
    );
    assert_eq!(
        output_of(&records, "remember")["revision"],
        serde_json::json!(1)
    );

    let _ = std::fs::remove_dir_all(&dir);
}

/// **A DIVERGENCE IS PROPOSED, NEVER PERFORMED.** Text that matches its own
/// workspace completes the run with nothing handed to anyone; text that
/// reads closer to another workspace stops the run at `propose`, `Waiting`,
/// for a person to close — never `Complete` on its own account.
#[test]
fn a_divergence_is_proposed_and_a_match_at_home_completes_quietly() {
    let dir = std::env::temp_dir().join(format!("sailor-notice-divergence-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("cartella di lavoro");
    let ledger = ledger::Ledger::open(&dir).expect("deposito di lavoro");

    let remember = shipped("remember-in-the-graph");
    let notice = shipped("notice-a-divergence");

    run_with_ledger(
        &remember,
        &ledger,
        r#"{"workspace_id":"acme-products","node_id":"pricing","kind":"decision","title":"acme pricing model","summary":"flat fee decision for acme checkout","written_by":"test"}"#,
    );
    run_with_ledger(
        &remember,
        &ledger,
        r#"{"workspace_id":"acme-ads","node_id":"budget","kind":"decision","title":"acme ads campaign budget","summary":"ten k per month on paid ads spend","written_by":"test"}"#,
    );

    // On topic: nothing is proposed, the run completes on its own.
    let (execution, records) = run_with_ledger(
        &notice,
        &ledger,
        r#"{"workspace_id":"acme-products","text":"let's revisit the acme pricing model decision"}"#,
    );
    assert_eq!(execution.decisions.last(), Some(&Decision::Complete), "{execution:?}");
    let propose = records
        .iter()
        .find(|record| record.step_id == "propose")
        .expect("«propose» lascia traccia anche saltato");
    assert_eq!(propose.outcome, Some(Outcome::Skipped), "{propose:?}");

    // Off topic: the flow stops and waits for a person, it does not act.
    let (execution, records) = run_with_ledger(
        &notice,
        &ledger,
        r#"{"workspace_id":"acme-products","text":"what should the ads campaign budget be this month"}"#,
    );
    assert_ne!(
        execution.decisions.last(),
        Some(&Decision::Complete),
        "una divergenza proposta non deve mai completare da sola: {execution:?}"
    );
    let propose = records
        .iter()
        .find(|record| record.step_id == "propose")
        .expect("«propose» lascia traccia");
    assert_eq!(propose.outcome, Some(Outcome::Waiting), "{propose:?}");
    let check = output_of(&records, "check");
    assert_eq!(check["diverges"], serde_json::json!(true), "{check}");
    assert_eq!(check["elsewhere"]["workspace_id"], serde_json::json!("acme-ads"), "{check}");

    let _ = std::fs::remove_dir_all(&dir);
}
