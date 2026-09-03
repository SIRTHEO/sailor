//! Il contratto fra il flusso che smista il lavoro e le azioni che lo eseguono.
//!
//! **PERCHÉ STA QUI E NON NEL CRATE DELLE AZIONI.** Fino al 28/08/2026 stava
//! là, e andava bene finché il flusso nominava solo azioni di quel crate. Ora
//! ne nomina di tre — l'innesco viene da `trigger`, e il motore sa risolvere
//! uno strumento solo se qualcuno gli ha dato un risolutore, che vive in
//! `toolbox` — e `sailor` è l'unico posto del programma dove i tre si
//! incontrano. Una prova che non vede tutte le azioni del flusso non può dire
//! se quel flusso parte.
//!
//! **IL FILE SI LEGGE A TEMPO DI PROVA, NON DI COMPILAZIONE.** Con `include_str!`
//! un flusso cancellato non fa cadere una prova: fa cadere la *compilazione*
//! dell'intero crate, e chi lo scopre non vede il flusso, vede un crate rotto.
//! È successo il 28/08/2026 alle prove di `sailor`, ferme perché
//! `flows/prima-corsa.flow.json` non c'era più.
//!
//! **NESSUNA PROVA QUI INVOCA UN MOTORE VERO.** Gli strumenti nominati dal
//! flusso si risolvono con un risolutore di prova che li manda tutti su `sh`:
//! è la stessa strada che percorre una corsa vera — il passo chiede uno
//! strumento, qualcuno lo risolve — con l'unica differenza che conta, cioè che
//! non si spende una chiamata.

use actions::ToolResolver;
use flow::{
    ActionRegistry, Clock, Decision, Execution, ExecutionRequest, Executor, FlowError, FlowFile,
    Graph, InMemoryRecordStore, InProcessExecutor, Outcome, SharedState, Step, ValueSchema,
};
use serde_json::{json, Value};
use std::collections::BTreeMap;

const FLOW_ID: &str = "dispatch-the-work";

/// **IL FLUSSO NON STA PIÙ SU DISCO, E QUESTA PROVA NON DEVE CERCARLO LÌ.**
/// Fino all'01/09/2026 leggeva `flows/dispatch-the-work.flow.json` dalla radice
/// del progetto. Poi il flusso è entrato nel binario — le regole di
/// instradamento spedite lo nominano, e su un'altra macchina la cartella non
/// c'è — e leggerlo dal disco vorrebbe dire provare un file che il prodotto non
/// spedisce mentre quello spedito non lo prova nessuno. `system::FLOWS` è la
/// stessa sorgente da cui lo prende chi lo esegue.
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

/// Ogni strumento diventa `sh`: quale comando esegua davvero lo decide poi il
/// campo `args` del passo, che le prove sostituiscono.
struct EveryToolIsShell;

impl ToolResolver for EveryToolIsShell {
    fn resolve(&self, _id: &str) -> Result<String, String> {
        Ok("sh".to_owned())
    }
}

/// Il risolutore che non trova niente: serve a provare cosa succede a un flusso
/// portato su una macchina dove quello strumento non c'è.
struct NoToolIsHere;

impl ToolResolver for NoToolIsHere {
    fn resolve(&self, id: &str) -> Result<String, String> {
        Err(format!("lo strumento «{id}» non è su questa macchina"))
    }
}

fn registry_with(resolver: impl ToolResolver + 'static) -> ActionRegistry {
    let mut registry = ActionRegistry::default();
    actions::register_default(&mut registry);
    trigger::register_default(&mut registry);
    registry.register(
        actions::EXTERNAL_ENGINE_ACTION,
        actions::ExternalEngineAction::resolving_with(resolver),
    );
    registry
}

/// Un orologio finto che avanza di uno a ogni domanda. Il contatore è atomico
/// perché l'orologio ora è condiviso fra i fili di un fronte.
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

fn run_with(
    graph: &Graph,
    inputs: &[(&str, Value)],
    registry: &ActionRegistry,
) -> (Execution, InMemoryRecordStore) {
    let mut store = InMemoryRecordStore::default();
    let request = ExecutionRequest {
        run_id: "prova".to_owned(),
        root_inputs: inputs
            .iter()
            .map(|(id, value)| ((*id).to_owned(), value.clone()))
            .collect(),
        gates: Vec::new(),
        shared: SharedState::new(),
        spend_cap_micros: None,
    };
    let execution = InProcessExecutor
        .execute(graph, request, &mut store, registry, &mut Tick::new(0))
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

// ── la forma del file ────────────────────────────────────────────────────

/// I sei nodi e i loro archi: l'innesco che porta la consegna, il nodo che la
/// divide, i due motori, il passo che verifica, il cancello che ne fa un rosso.
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

/// **UN SOLO NODO DI INGRESSO, ED È UN INNESCO.** Un passo senza dipendenze che
/// non sia un innesco è un altro posto da cui la consegna può entrare senza che
/// nessuno l'abbia mandata.
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
    let registry = registry_with(EveryToolIsShell);

    let missing: Vec<&str> = flow
        .graph
        .steps()
        .iter()
        .filter(|step| registry.get(&step.action).is_none())
        .map(|step| step.action.as_str())
        .collect();

    assert!(missing.is_empty(), "azioni non registrate: {missing:?}");
}

/// **NESSUN BINARIO DENTRO IL FLUSSO.** `"bin": "claude"` gira solo dove quel
/// nome è nel percorso di chi esegue; un identificativo di strumento gira
/// ovunque qualcuno sappia risolverlo, e dove non c'è si ferma dicendo quale.
/// Questa prova è il guardiano di quella regola per i flussi spediti.
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
            // UNO O UNA CATENA. Dal 29/08/2026 un passo può dichiarare un
            // elenco di motori da provare in ordine invece di un nome solo.
            // Ciò che questa prova sorveglia non cambia — nessun passo esegue
            // un motore senza dire quale vuole — ma leggere `tool` come sola
            // stringa lo dichiarerebbe assente proprio dove sono tre.
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

/// **LO SCHEMA D'USCITA DICE CHE UN MOTORE QUI NON PUÒ FALLIRE IN SILENZIO**, e
/// dice anche che cosa passa al passo dopo. Sono due dichiarazioni diverse e
/// tutte e due devono stare nel file: `status` ammette solo una chiamata
/// finita, e `answer` è l'unica cosa che attraversa la catena — non il testo
/// grezzo di ciò che il motore ha detto.
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
        // La forma pretesa e la forma dichiarata nell'uscita sono la stessa
        // cosa scritta due volte: se divergono, il passo dopo legge un campo
        // che il motore non ha mai promesso.
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

// ── il flusso, eseguito ──────────────────────────────────────────────────

/// Il segno che questa prova insegue lungo tutta la catena.
const MARK: &str = "SEGNO-DELLA-PROVA";

/// Un motore finto che legge il proprio incarico **dall'ingresso** e risponde
/// bene solo se ci trova il segno; altrimenti risponde fuori forma, e il suo
/// passo diventa rosso. È così che si prova che il testo è arrivato davvero,
/// invece di fidarsi che i rinvii siano scritti bene.
fn reads_stdin(answer: &str) -> Value {
    json!([
        "-c",
        "if grep -q \"$1\"; then printf '%s' \"$2\"; else printf '%s' '{\"segno\":\"non arrivato\"}'; fi",
        "motore",
        MARK,
        answer
    ])
}

/// Lo stesso, per il motore che riceve l'incarico **in un argomento**: è la
/// forma misurata di `agy`, e il rinvio deve arrivare fin dentro l'elenco degli
/// argomenti.
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

/// Sostituisce gli argomenti dei passi che invocano un motore, lasciando tutto
/// il resto del file com'è: i rinvii, le forme pretese, gli schemi, gli archi e
/// l'innesco sono quelli che gireranno.
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
            "dispatch" => with["args"] = reads_stdin(&dispatched),
            "engine_a" => {
                with["args"] = engine_a_args.clone().unwrap_or_else(|| reads_stdin(&found))
            }
            "engine_b" => {
                let carried = with["args"][3].clone();
                with["args"] = reads_args(&found, carried);
            }
            "verify" => with["args"] = reads_stdin(&judged),
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
    let without_whys = json!({
        "first_engine": format!("primo incarico, {MARK}"),
        "second_engine": format!("secondo incarico, {MARK}")
    })
    .to_string();
    let mut steps: Vec<Step> = flow.graph.steps().to_vec();
    for step in &mut steps {
        if step.id == "dispatch" {
            step.with.as_mut().expect("the step carries its values")["args"] =
                reads_stdin(&without_whys);
        }
    }
    let graph = Graph::new(steps).expect("the graph stays valid");

    let (execution, _) = run_with(
        &graph,
        &[("trigger", trigger_input())],
        &registry_with(EveryToolIsShell),
    );

    assert_eq!(
        last_decision(&execution),
        Decision::Failed(vec!["dispatch".to_owned()]),
        "an answer without the whys must not pass the shape"
    );
}

/// La consegna dell'innesco, col segno dentro: è l'unica cosa che entra nel
/// flusso, ed è quella che i motori devono vedersi arrivare.
fn trigger_input() -> Value {
    let mut signal = flow_file().inputs["trigger"].clone();
    signal["text"] = json!(format!("conta i residui, {MARK}"));
    signal
}

/// **LA CATENA INTERA, GIRATA DAVVERO, SENZA SPENDERE UNA CHIAMATA.** Dal
/// segnale al verdetto: il testo entra dall'innesco, arriva al nodo che smista,
/// da lì ai due motori — uno sull'ingresso, l'altro in un argomento — e le due
/// risposte più gli incarichi arrivano al verificatore. Ogni motore finto
/// risponde bene **solo se** ha ricevuto ciò che il flusso gli prometteva:
/// basta un rinvio scritto male e la corsa diventa rossa.
///
/// Due corse, perché una sola non proverebbe niente: cambia il verdetto e la
/// corsa cambia colore.
#[test]
fn the_whole_chain_runs_from_the_signal_to_the_verdict() {
    let outcome = |verdict: &str| {
        let graph = chain_with(verdict, None);
        let (execution, _) = run_with(
            &graph,
            &[("trigger", trigger_input())],
            &registry_with(EveryToolIsShell),
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

/// **CIÒ CHE ATTRAVERSA LA CATENA È SOLO CIÒ CHE LA FORMA DICHIARA.** Il segnale
/// arriva fino ai due motori — lo dimostrano le loro risposte, che escono bene
/// solo se il segno c'era — e nell'uscita di ogni passo non resta niente del
/// testo grezzo: né i preamboli del modello, né i campi che nessuno ha
/// dichiarato.
#[test]
fn only_what_the_shape_declares_travels_down_the_chain() {
    let graph = chain_with("APPROVATO", None);

    let (_, store) = run_with(
        &graph,
        &[("trigger", trigger_input())],
        &registry_with(EveryToolIsShell),
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
    // I due motori hanno risposto bene: vuol dire che il loro incarico — nato
    // dal testo dell'innesco — è arrivato, per due strade diverse.
    assert_eq!(output("engine_a")["answer"]["total"], 1);
    assert_eq!(output("engine_b")["answer"]["total"], 1);
    assert_eq!(output("verify")["answer"]["verdict"], "APPROVATO");
    assert!(
        output("verify")["answer"].get("why").is_some(),
        "il verificatore dichiara anche il perché"
    );
}

/// **IL DIFETTO MISURATO IL 28/08/2026, PROVATO SUL FLUSSO VERO.** Un motore
/// che esce in errore rompeva il proprio passo? No: lo chiudeva verde con
/// `status: exit_error` dentro, e la catena andava avanti. Qui il primo motore
/// esce con 3, e devono valere tutte e tre le cose: il suo passo è rotto, la
/// corsa è rossa **per colpa sua**, e i passi che dipendevano da lui non sono
/// mai partiti — cioè non è stata spesa nessuna chiamata a valle.
#[test]
fn an_engine_that_fails_stops_the_chain_instead_of_colouring_it_green() {
    let graph = chain_with(
        "APPROVATO",
        Some(json!(["-c", "echo il-motivo 1>&2; exit 3"])),
    );

    let (execution, store) = run_with(
        &graph,
        &[("trigger", trigger_input())],
        &registry_with(EveryToolIsShell),
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
    // L'altro motore, che non dipendeva dal primo, ha girato: fermarsi non vuol
    // dire fermare tutto.
    assert!(store
        .all()
        .iter()
        .any(|record| record.step_id == "engine_b"));
}

/// **IL FLUSSO PORTATO SU UNA MACCHINA CHE NON HA QUELLO STRUMENTO.** Non parte
/// e dice quale manca — che è tutto ciò che serve a chi lo riceve. Prima
/// nemmeno la domanda esisteva: il flusso nominava un binario, e un binario
/// assente diventava `spawn_failed` dentro un passo verde.
#[test]
fn a_machine_without_the_tool_stops_the_flow_saying_which_one() {
    let graph = chain_with("APPROVATO", None);

    let (execution, store) = run_with(
        &graph,
        &[("trigger", trigger_input())],
        &registry_with(NoToolIsHere),
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
    // LA CLASSE DIPENDE DA QUANTI NE HA CHIESTI. Un passo che nomina un motore
    // solo e non lo trova resta `tool_unavailable`, col motivo del risolutore;
    // uno che ne dichiara una catena e non ne trova nessuno dà
    // `no_usable_engine`, perché «quello strumento non c'è» sarebbe una risposta
    // parziale su tre. Ciò che questa prova difende è la stessa cosa in
    // entrambi i casi: il flusso si ferma DICENDO quale mancava.
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

/// Il cancello finale, eseguito: **lo stesso passo, preso dal file**, con tre
/// risposte diverse. Un solo caso non proverebbe niente.
///
/// Non c'è più il caso «un motore non ha risposto»: quel passo adesso si rompe
/// da sé, e il cancello non lo vede nemmeno. Il controllo sugli stati altrui
/// che stava in questo comando era l'unica rete del flusso, ed è la rete che
/// qualcuno poteva togliere senza accorgersene.
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
            &registry_with(EveryToolIsShell),
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
    // Nessun verdetto dove il cancello lo cerca: il rinvio non trova niente e
    // il passo si rompe. Un cancello che in questo caso approvasse sarebbe il
    // verde peggiore di tutti, perché nessuno ha giudicato.
    assert_eq!(
        outcome(json!({"why": "ho dimenticato la riga che conta"})),
        Decision::Failed(vec!["verdict".to_owned()]),
        "senza verdetto non si approva"
    );
}

/// **PERCHÉ NESSUN PASSO DI QUESTO FLUSSO PORTA UN `when`.** Un passo saltato
/// non è un rosso: i suoi discendenti non partono mai, il fronte resta vuoto e
/// la corsa si chiude `Complete`. Un flusso che smista lavoro a due motori a
/// pagamento e finisce verde senza averne invocato nessuno è indistinguibile da
/// uno che ha lavorato.
///
/// Questa prova misura il comportamento del motore, non il flusso: se un giorno
/// un passo saltato tingerà di rosso la corsa, cade qui, e la scelta si riapre.
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
    };
    let mut first = step("first", &[], None);
    first.with = None;
    let graph = Graph::new(vec![
        first,
        // «passed» non è «ok»: la condizione non scatta mai, apposta.
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
        &registry_with(EveryToolIsShell),
    );

    assert_eq!(
        last_decision(&execution),
        Decision::Complete,
        "un passo saltato chiude la corsa in verde: è il verde falso da evitare"
    );
}

/// Il rinvio dichiarato nel file arriva davvero al motore, **col `tool` vero
/// del passo**: si sostituiscono solo gli argomenti, e il binario lo sceglie il
/// risolutore come farebbe su una macchina qualunque. Il motore finto risponde
/// bene solo se nel prompt ha trovato l'incarico che il nodo prima gli ha
/// scritto — e quel prompt deve contenere anche la forma pretesa, o l'azione si
/// ferma prima di partire.
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
    with["args"] = reads_stdin(&answer);
    let mut input = with.clone();
    input["status"] = json!("ok");
    input["answer"] = json!({
        "first_engine": format!("conta i ganci morti, {MARK}"),
        "second_engine": "un altro territorio",
        "why_first": "a territory of files",
        "why_second": "the other territory"
    });

    // **L'INGRESSO SI COMPONE COME LO COMPONE L'ESECUTORE.** Dal 01/09/2026 i
    // rinvii li scioglie `flow::step_input` — un posto solo per tutte le
    // azioni, guasto 28 — e questa prova chiama `execute` senza passare di lì.
    // Chiamare la funzione vera è il modo di provare l'azione nel mondo in cui
    // gira: senza, la si proverebbe in uno che non esiste. Che i rinvii
    // arrivino sciolti a **ogni** azione lo prova
    // `crates/flow/tests/a_reference_reaches_every_action.rs`; qui si prova che
    // l'incarico sciolto finisce davvero nel prompt di questo motore.
    let input = flow::reference::resolve_references(&input).expect("i rinvii si sciolgono");

    let registry = registry_with(EveryToolIsShell);
    let action = registry.get(&engine.action).expect("azione registrata");
    let outcome = action
        .execute(&input, &mut SharedState::new())
        .expect("l'azione legge il rinvio e la forma è nel prompt");
    let flow::ActionOutcome::Went(output) = outcome else {
        panic!("un motore che risponde è sempre Went")
    };

    assert_eq!(
        output["answer"]["total"], 0,
        "il motore non ha ricevuto il proprio incarico: {output}"
    );
}

/// Ogni strumento nominato dal flusso è dichiarato da un descrittore: un
/// identificativo scritto male si scopre qui, non a corsa avviata su una
/// macchina che quello strumento ce l'ha.
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
