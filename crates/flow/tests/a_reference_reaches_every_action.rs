//! Un rinvio arriva **a ogni azione** già sciolto, comprese quelle che nessuno
//! ha ancora scritto.
//!
//! **PERCHÉ ESISTE — IL GUASTO 28, E LA CURA CHE NON ERA UNA CURA.** Il
//! 31/08/2026 `resolve_references` era chiamata da **due azioni su nove**: un
//! passo che scriveva nel deposito un valore preso dal passo prima riceveva
//! `{"$from": …}` come oggetto e moriva su «invalid type: map, expected a
//! string», perché `key` vuole un testo. Il deposito non poteva fare il
//! testimone fra due passi.
//!
//! Poi la riga è stata **ricopiata**. Misurato in questo albero il 01/09/2026,
//! prima di toccare niente: **sedici azioni registrate, dodici con quella riga,
//! quattro senza** — `history_ask`, `detect_tools`, `trigger`, `subflow`. Il
//! sintomo era ridotto, il guasto no: dodici copie della stessa riga sono il
//! guasto 10 in dodici esemplari, e ogni azione nuova continuava a nascere
//! senza. Nessun controllo diceva quali fossero le quattro.
//!
//! **COSA PROVA QUESTO FILE, E PERCHÉ QUI.** Che l'ingresso arrivi sciolto non
//! è un merito della singola azione: è come i passi si passano le informazioni,
//! e succede in `flow::step_input` — l'unico punto attraversato da ogni passo
//! di ogni corsa. L'azione di queste prove **non risolve niente e non sa cosa
//! sia un rinvio**: è il modello di ogni azione futura. Se la regola tornasse
//! dentro le azioni, un'azione così tornerebbe a non averla, e questo file
//! diventerebbe rosso.
//!
//! **IL MUTANTE.** Togliere la chiamata a `resolve_references` da `step_input`,
//! cioè rimettere il difetto originale, rende rosse tutte e tre le prove qui
//! dentro; l'esito di ciascuna è scritto accanto.

use flow::{
    Action, ActionError, ActionOutcome, Clock, Decision, Executor, FlowError, Graph,
    InMemoryRecordStore, InProcessExecutor, Outcome, RecordStore, SharedState, Step, ValueSchema,
};
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::sync::atomic::{AtomicI64, Ordering};
use std::sync::{Arc, Mutex};

/// Un'azione che **non sa cosa sia un rinvio**: registra l'ingresso che riceve
/// e lo restituisce così com'è. È il modello di ogni azione registrata domani.
///
/// Restituire l'ingresso invece di una costante non è pigrizia: l'ingresso di
/// un passo *è* l'uscita della sua dipendenza, quindi così il valore che il
/// primo passo dichiara arriva al secondo, che è la strada su cui il guasto 28
/// è stato pagato.
struct KeepsWhatItGets(Arc<Mutex<Vec<Value>>>);

impl Action for KeepsWhatItGets {
    fn execute(&self, input: &Value, _shared: &SharedState) -> Result<ActionOutcome, ActionError> {
        self.0
            .lock()
            .unwrap_or_else(|held| held.into_inner())
            .push(input.clone());
        Ok(ActionOutcome::Went(input.clone()))
    }

    fn species(&self) -> flow::StepSpecies {
        flow::StepSpecies::Repeatable
    }
}

struct Tick(AtomicI64);

impl Clock for Tick {
    fn now(&self) -> Result<i64, FlowError> {
        Ok(self.0.fetch_add(1, Ordering::Relaxed) + 1)
    }
}

fn step(id: &str, deps: &[&str], with: Option<Value>) -> Step {
    Step {
        id: id.to_owned(),
        deps: deps.iter().map(|dep| (*dep).to_owned()).collect(),
        action: "tiene-quel-che-riceve".to_owned(),
        max_attempts: 1,
        when: None,
        with,
        input_schema: ValueSchema::Any,
        output_schema: ValueSchema::Any,
    }
}

/// Lo stesso passo, con la condizione che `flows/chiedi-all-indice.flow.json`
/// mette sui passi `chiedi` e `leggi`: gira solo se ciò che ha ricevuto dice
/// `ok`.
fn step_only_when_ok(id: &str, deps: &[&str], with: Option<Value>, pointer: &str) -> Step {
    let mut step = step(id, deps, with);
    step.when = Some(
        serde_json::from_value(json!({
            "kind": "pointer_equals", "pointer": pointer, "value": "ok"
        }))
        .expect("condizione valida"),
    );
    step
}

/// Fa girare il grafo e restituisce gli ingressi che l'azione ha visto, in
/// ordine.
fn what_the_action_saw(graph: &Graph, root_inputs: BTreeMap<String, Value>) -> Vec<Value> {
    let seen = Arc::new(Mutex::new(Vec::new()));
    let mut actions = flow::ActionRegistry::default();
    actions.register("tiene-quel-che-riceve", KeepsWhatItGets(seen.clone()));
    let mut store = InMemoryRecordStore::default();
    InProcessExecutor
        .execute(
            graph,
            flow::ExecutionRequest {
                run_id: "corsa".to_owned(),
                root_inputs,
                gates: vec![],
                shared: SharedState::new(),
                spend_cap_micros: None,
            },
            &mut store,
            &actions,
            &Tick(AtomicI64::new(0)),
        )
        .expect("la corsa arriva in fondo");
    let saw = seen.lock().unwrap_or_else(|held| held.into_inner()).clone();
    saw
}

/// **IL CASO DEL GUASTO 28, SU UN'AZIONE CHE NON RISOLVE NIENTE.** Il secondo
/// passo prende dal primo la chiave con cui depositare: se il rinvio non fosse
/// sciolto, l'azione riceverebbe `{"$from": "/stdout"}` come oggetto.
///
/// Col mutante: `key` arriva come oggetto e l'asserzione cade dicendo
/// `{"$from":"/stdout"}` invece di `il-lavoro-di-ieri`.
#[test]
fn an_action_that_resolves_nothing_still_gets_its_references_resolved() {
    let graph = Graph::new(vec![
        step("primo", &[], None),
        step(
            "secondo",
            &["primo"],
            Some(json!({"collection": "mandato", "key": {"$from": "/stdout"}})),
        ),
    ])
    .expect("grafo valido");
    let mut root_inputs = BTreeMap::new();
    root_inputs.insert("primo".to_owned(), json!({"stdout": "il-lavoro-di-ieri"}));

    let saw = what_the_action_saw(&graph, root_inputs);

    assert_eq!(saw.len(), 2, "sono girati due passi: {saw:?}");
    assert_eq!(
        saw[1]["key"],
        json!("il-lavoro-di-ieri"),
        "l'azione ha ricevuto il rinvio invece del valore: {}",
        saw[1]
    );
    assert_eq!(saw[1]["collection"], json!("mandato"));
}

/// `$join` e `$json` passano dalla stessa strada: la prova non sta sul solo
/// `$from`, o la metà della sintassi resterebbe scoperta.
///
/// Col mutante: `stdin` resta un oggetto e il confronto col testo cade.
#[test]
fn the_other_two_forms_of_reference_arrive_resolved_too() {
    let graph = Graph::new(vec![
        step("primo", &[], None),
        step(
            "secondo",
            &["primo"],
            Some(json!({
                "stdin": {"$join": ["Esegui solo la tua sezione.\n", {"$from": "/stdout"}]},
                "shape_as_text": {"$json": "/answer_shape"},
            })),
        ),
    ])
    .expect("grafo valido");
    let mut root_inputs = BTreeMap::new();
    root_inputs.insert(
        "primo".to_owned(),
        json!({"stdout": "conta i ganci morti", "answer_shape": {"type": "number"}}),
    );

    let saw = what_the_action_saw(&graph, root_inputs);

    assert_eq!(
        saw[1]["stdin"],
        json!("Esegui solo la tua sezione.\nconta i ganci morti")
    );
    assert_eq!(saw[1]["shape_as_text"], json!("{\"type\":\"number\"}"));
}

/// **UN PASSO SALTATO NON SCIOGLIE NIENTE, E QUINDI NON SI ROMPE.**
///
/// **QUESTA È LA FORMA CHE STA NELL'ALBERO OGGI, NON UN CASO DI SCUOLA.**
/// `flows/chiedi-all-indice.flow.json`, passo `leggi`: `when` su `/status`, un
/// `with` pieno di `$from` verso l'uscita di `chiedi`, e `chiedi` fra le
/// `skippable_dependencies`. Quando l'indice non risponde — il caso che quel
/// flusso dichiara essere il più frequente — `chiedi` viene saltato e `leggi`
/// riceve `{}` più il proprio `with`: i suoi puntatori non trovano niente.
///
/// Sciogliendo i rinvii **prima** della condizione, quel flusso passava da
/// «completato» a «terminato con stato failed — `unresolved_reference`»,
/// misurato col binario vero. Per questo la condizione si valuta sull'ingresso
/// non ancora sciolto: **un passo che non gira non deve pagare i rinvii di un
/// lavoro che non farà.**
///
/// Non si vedeva su quel flusso solo perché `verdetto`, nello stesso fronte, si
/// rompe per primo e la corsa muore lì — cioè per un incidente, non per una
/// proprietà. Qui la proprietà si interroga da sola.
///
/// **E LE DUE DIREZIONI, PERCHÉ UNA SOLA NON PROVEREBBE NIENTE**: con la
/// condizione soddisfatta lo stesso passo gira **e** riceve i rinvii sciolti.
/// Il mutante che scioglie prima della condizione fa cadere la prima metà; il
/// mutante che non scioglie affatto fa cadere la seconda.
#[test]
fn a_step_that_does_not_run_never_pays_for_its_references() {
    let graph = Graph::with_skippable_dependencies(
        vec![
            step("guardia", &[], None),
            step_only_when_ok("chiedi", &["guardia"], None, "/status"),
            // Una dipendenza saltabile arriva **nominata**: l'ingresso è
            // `{"chiedi": …}` quando c'è, e `{}` quando è stata saltata. Per
            // questo il puntatore porta il nome del passo.
            step_only_when_ok(
                "leggi",
                &["chiedi"],
                Some(json!({"stdin": {"$from": "/chiedi/said"}})),
                "/chiedi/status",
            ),
        ],
        [flow::DependencyEdge::new("leggi", "chiedi")],
    )
    .expect("grafo valido");

    // L'indice non risponde: `chiedi` è saltato, `leggi` riceve `{}` più il
    // proprio `with`, e il suo `$from` non trova niente. Deve essere saltato.
    let (decisions, records) = run_and_read(&graph, "non-pronto");
    assert_eq!(
        decisions.last(),
        Some(&Decision::Complete),
        "un passo saltato non è un rosso: {decisions:?}"
    );
    for step_id in ["chiedi", "leggi"] {
        let outcome = closed_outcome(&records, step_id);
        assert_eq!(
            outcome,
            Some(Outcome::Skipped),
            "«{step_id}» doveva essere saltato, non {outcome:?}"
        );
    }

    // E la direzione opposta: quando l'indice risponde, lo stesso passo gira e
    // il rinvio gli arriva sciolto.
    let (decisions, records) = run_and_read(&graph, "ok");
    assert_eq!(decisions.last(), Some(&Decision::Complete));
    let read = records
        .iter()
        .find(|record| record.step_id == "leggi" && record.outcome == Some(Outcome::Went))
        .expect("con l'indice pronto il passo gira");
    assert_eq!(
        read.input["stdin"],
        json!("la risposta dell'indice"),
        "il rinvio doveva arrivare sciolto: {}",
        read.input
    );
}

/// Fa girare il grafo con quello che la guardia dichiara, e restituisce le
/// decisioni e i record.
fn run_and_read(graph: &Graph, guard_says: &str) -> (Vec<Decision>, Vec<flow::StepRecord>) {
    let seen = Arc::new(Mutex::new(Vec::new()));
    let mut actions = flow::ActionRegistry::default();
    actions.register("tiene-quel-che-riceve", KeepsWhatItGets(seen));
    let mut store = InMemoryRecordStore::default();
    let mut root_inputs = BTreeMap::new();
    root_inputs.insert(
        "guardia".to_owned(),
        json!({"status": guard_says, "said": "la risposta dell'indice"}),
    );
    let execution = InProcessExecutor
        .execute(
            graph,
            flow::ExecutionRequest {
                run_id: "corsa".to_owned(),
                root_inputs,
                gates: vec![],
                shared: SharedState::new(),
                spend_cap_micros: None,
            },
            &mut store,
            &actions,
            &Tick(AtomicI64::new(0)),
        )
        .expect("la corsa non deve rompersi");
    let records = store.records("corsa").expect("i record della corsa");
    (execution.decisions, records)
}

fn closed_outcome(records: &[flow::StepRecord], step_id: &str) -> Option<Outcome> {
    records
        .iter()
        .filter(|record| record.step_id == step_id)
        .find_map(|record| record.outcome.clone())
}

/// **UN PUNTATORE CHE NON TROVA NIENTE ROMPE QUEL PASSO — E SOLO QUELLO.**
///
/// È più forte di com'era su un lato e identico sull'altro, e tutti e due
/// contano. Più forte: l'azione **non viene invocata**, mentre prima la
/// risoluzione stava dentro `external_engine` e quindi il passo entrava
/// nell'azione per morirci; le azioni che non risolvevano, invece, passavano
/// l'oggetto al `serde` di turno e sbagliavano campo. Identico: il difetto resta
/// **del passo**, si scrive nel deposito come un passo rotto, e la corsa arriva
/// a `Failed` nominandolo.
///
/// **QUESTA SECONDA METÀ È UNA RIPARAZIONE, NON UN'OSSERVAZIONE.** Il primo
/// tentativo di spostare la risoluzione qui propagava l'errore con un `?` da
/// `execute`: la corsa moriva **senza aprire né chiudere niente**, nessun
/// record, nessuna decisione, e un passo che per il deposito non era mai
/// esistito. L'ha visto `dispatch_the_work`, che pretendeva
/// `Failed(["verdict"])`.
///
/// Col mutante che toglie la risoluzione: nessun passo rotto, l'azione viene
/// invocata e riceve l'oggetto — cade tutto.
#[test]
fn a_pointer_that_finds_nothing_breaks_that_step_and_only_that_one() {
    let graph = Graph::new(vec![
        step("primo", &[], None),
        step(
            "secondo",
            &["primo"],
            Some(json!({"stdin": {"$from": "/non/esiste"}})),
        ),
    ])
    .expect("grafo valido");
    let mut root_inputs = BTreeMap::new();
    root_inputs.insert("primo".to_owned(), json!({"stdout": "qualcosa"}));

    let seen = Arc::new(Mutex::new(Vec::new()));
    let mut actions = flow::ActionRegistry::default();
    actions.register("tiene-quel-che-riceve", KeepsWhatItGets(seen.clone()));
    let mut store = InMemoryRecordStore::default();
    let execution = InProcessExecutor
        .execute(
            &graph,
            flow::ExecutionRequest {
                run_id: "corsa".to_owned(),
                root_inputs,
                gates: vec![],
                shared: SharedState::new(),
                spend_cap_micros: None,
            },
            &mut store,
            &actions,
            &Tick(AtomicI64::new(0)),
        )
        .expect("un passo rotto non è un guasto della corsa");

    assert_eq!(
        execution.decisions.last(),
        Some(&Decision::Failed(vec!["secondo".to_owned()])),
        "la corsa deve dire quale passo si è rotto: {:?}",
        execution.decisions
    );
    assert_eq!(
        seen.lock().unwrap_or_else(|held| held.into_inner()).len(),
        1,
        "solo il primo passo doveva girare: il secondo non deve nemmeno essere invocato"
    );

    let broken = store
        .records("corsa")
        .expect("i record della corsa")
        .into_iter()
        .find(|record| record.step_id == "secondo" && record.outcome == Some(Outcome::Broke))
        .expect("il passo rotto sta nel deposito, o la ripresa non saprebbe dove ripartire");
    assert_eq!(broken.failure_class.as_deref(), Some("unresolved_reference"));
    assert!(
        broken.said.as_deref().is_some_and(|said| said.contains("/non/esiste")),
        "il messaggio deve nominare il puntatore da correggere: {:?}",
        broken.said
    );
    // L'intenzione conserva il puntatore com'era scritto: chi legge il record
    // deve vedere cosa correggere, non il vuoto che ne è uscito.
    assert_eq!(broken.input["stdin"], json!({"$from": "/non/esiste"}));
}
