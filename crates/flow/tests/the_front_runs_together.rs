//! Il fronte parte insieme, e ogni passo sa di essere se stesso.
//!
//! **PERCHÉ QUESTE PROVE NON CRONOMETRANO.** La misura che ha smascherato il
//! guasto 7 era un cronometro — due passi da sei secondi ne impiegavano dodici —
//! ma un cronometro dentro una batteria è una prova che diventa rossa quando la
//! macchina è carica e verde quando qualcuno ha spento tutto. Qui si osserva
//! invece il fatto che conta e che il tempo misurava solo di riflesso: **i due
//! passi sono vivi nello stesso istante**. Ogni passo annuncia di essere entrato
//! e aspetta che sia entrato anche l'altro; se l'esecutore li mette in fila, il
//! primo aspetta uno che non arriverà mai e la scadenza lo dice.
//!
//! Nel caso buono queste prove durano millisecondi; nel caso rotto, la scadenza.

use flow::{
    Action, ActionError, ActionOutcome, Clock, Decision, Executor, Graph, InMemoryRecordStore,
    InProcessExecutor, Outcome, SharedState, Step, ValueSchema, CURRENT_STEP,
};
use serde_json::{json, Value};
use std::sync::atomic::{AtomicI64, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// Quanto si aspetta l'altro prima di dichiarare che non arriverà. Generoso: il
/// caso buono non ci arriva mai, e il caso rotto può permettersi di essere lento
/// una volta.
const DEADLINE: Duration = Duration::from_secs(5);

/// Un'azione che entra, dice di essere entrata, e non esce finché non sono
/// entrati tutti quelli che aspetta.
struct MeetsTheOthers {
    arrived: Arc<AtomicUsize>,
    expected: usize,
    /// Chi ha visto come proprio identificativo, nell'ordine in cui è arrivato.
    seen_as: Arc<Mutex<Vec<String>>>,
}

impl Action for MeetsTheOthers {
    fn execute(&self, _input: &Value, shared: &SharedState) -> Result<ActionOutcome, ActionError> {
        // Di chi è questo passo, secondo lo stato condiviso che ha ricevuto.
        let mine = shared
            .get(CURRENT_STEP)
            .and_then(Value::as_str)
            .unwrap_or("(nessuno)")
            .to_owned();
        self.seen_as
            .lock()
            .unwrap_or_else(|held| held.into_inner())
            .push(mine.clone());

        self.arrived.fetch_add(1, Ordering::SeqCst);
        let until = Instant::now() + DEADLINE;
        while self.arrived.load(Ordering::SeqCst) < self.expected {
            if Instant::now() >= until {
                return Err(ActionError::new(
                    "da_solo",
                    format!(
                        "«{mine}» ha aspettato {} secondi gli altri {} passi del fronte e non è \
                         arrivato nessuno: l'esecutore li sta mettendo in fila",
                        DEADLINE.as_secs(),
                        self.expected - 1
                    ),
                ));
            }
            std::thread::sleep(Duration::from_millis(5));
        }
        Ok(ActionOutcome::Went(json!({ "io": mine })))
    }

    fn species(&self) -> flow::StepSpecies {
        flow::StepSpecies::Repeatable
    }
}

struct Tick(AtomicI64);

impl Clock for Tick {
    fn now(&self) -> Result<i64, flow::FlowError> {
        Ok(self.0.fetch_add(1, Ordering::Relaxed) + 1)
    }
}

fn step(id: &str) -> Step {
    Step {
        id: id.to_owned(),
        deps: vec![],
        action: "incontra".to_owned(),
        max_attempts: 1,
        when: None,
        with: None,
        input_schema: ValueSchema::Any,
        output_schema: ValueSchema::Any,
    }
}

/// Fa girare `count` passi indipendenti, ognuno dei quali aspetta `expected`
/// compagni. Torna gli identificativi che i passi hanno visto come propri.
fn run_front(count: usize, expected: usize) -> (Vec<Decision>, Vec<String>, Vec<Outcome>) {
    let arrived = Arc::new(AtomicUsize::new(0));
    let seen_as = Arc::new(Mutex::new(Vec::new()));
    let mut actions = flow::ActionRegistry::default();
    actions.register(
        "incontra",
        MeetsTheOthers {
            arrived: Arc::clone(&arrived),
            expected,
            seen_as: Arc::clone(&seen_as),
        },
    );

    let steps: Vec<Step> = (1..=count).map(|n| step(&format!("passo{n}"))).collect();
    let graph = Graph::new(steps).expect("grafo valido");
    let store = InMemoryRecordStore::default();
    let request = flow::ExecutionRequest {
        run_id: "corsa".to_owned(),
        root_inputs: Default::default(),
        gates: vec![],
        shared: SharedState::new(),
        spend_cap_micros: None,
    };

    let execution = InProcessExecutor
        .execute(&graph, request, &store, &actions, &Tick(AtomicI64::new(0)))
        .expect("l'esecuzione arriva in fondo");

    let outcomes = store
        .all()
        .iter()
        .filter_map(|record| record.outcome)
        .collect();
    let names = seen_as.lock().unwrap_or_else(|held| held.into_inner()).clone();
    (execution.decisions, names, outcomes)
}

/// **LA PROVA DEL GUASTO 7.** Due passi senza dipendenze devono essere vivi
/// nello stesso momento. Rimettendo il `for` sequenziale al posto dello
/// `scope`, questa diventa rossa con scritto «ha aspettato 5 secondi gli altri
/// passi del fronte e non è arrivato nessuno».
#[test]
fn two_independent_steps_are_alive_at_the_same_time() {
    let (_, _, outcomes) = run_front(2, 2);
    assert_eq!(outcomes.len(), 2);
    assert!(
        outcomes.iter().all(|outcome| *outcome == Outcome::Went),
        "nessuno dei due deve essere rimasto ad aspettare l'altro: {outcomes:?}"
    );
}

/// **OGNI PASSO SI VEDE COL PROPRIO NOME, E QUESTO È IL PEZZO CHE SI POTEVA
/// SBAGLIARE IN SILENZIO.** L'identificativo del passo corrente viaggia in una
/// chiave sola dello stato condiviso, e le azioni la leggono per attribuire il
/// testo che producono e **la spesa che fanno**. Con due passi vivi e una mappa
/// sola, entrambi leggerebbero lo stesso nome: i costi di uno finirebbero
/// addosso all'altro, e niente diventerebbe rosso. Ogni filo riceve la propria
/// copia — questa prova è ciò che lo tiene vero.
#[test]
fn each_step_sees_its_own_identity_not_the_neighbour_one() {
    let (_, mut seen, _) = run_front(3, 3);
    seen.sort();
    assert_eq!(
        seen,
        vec!["passo1".to_owned(), "passo2".to_owned(), "passo3".to_owned()],
        "tre passi vivi insieme devono vedersi con tre nomi diversi, ciascuno il proprio"
    );
}

/// Il tetto è una decisione dichiarata, e si vede: con cinque passi che
/// aspettano di essere in cinque, il quinto non entra finché non esce qualcuno
/// del gruppo prima — e i primi quattro aspettano invano. La corsa arriva in
/// fondo lo stesso, coi passi rossi: un tetto che blocca deve dirlo, non
/// appendere il programma.
#[test]
fn the_ceiling_holds_and_the_run_still_ends() {
    let (decisions, _, outcomes) = run_front(5, 5);
    assert_eq!(outcomes.len(), 5, "tutti e cinque i passi sono stati aperti");
    assert!(
        outcomes.iter().any(|outcome| *outcome == Outcome::Broke),
        "chi ha aspettato oltre il tetto lo dice, invece di restare appeso"
    );
    assert!(
        !decisions.is_empty(),
        "e la corsa produce comunque le sue decisioni"
    );
}
