//! Il tetto di spesa: quando la corsa si ferma da sé, e quanto larga apre.
//!
//! **PERCHÉ IL DEPOSITO DI PROVA TIENE I COSTI.** `InMemoryRecordStore` risponde
//! sempre «zero speso», perché le chiamate ai motori non le registra — ed è la
//! risposta onesta per lui. Ma una prova del tetto costruita su quel deposito
//! sarebbe verde comunque, con o senza tetto: misurerebbe che nessuno spende
//! niente. Qui il deposito è un guscio che i costi li tiene, e le azioni li
//! scrivono mentre girano.
//!
//! **COSA SI PROVA DAVVERO.** Non che esista un `if`: che una corsa con un tetto
//! e una senza si comportino in modo **diverso** sullo stesso grafo e con le
//! stesse azioni. È la sola forma in cui un limite si può misurare.

use flow::{
    Action, ActionError, ActionOutcome, Clock, Completion, Decision, Executor, ExecutionRequest,
    FlowError, Graph, InMemoryRecordStore, InProcessExecutor, Outcome, RecordStore, SharedState,
    Spend, Step, StepRecord, ValueSchema,
};
use serde_json::{json, Value};
use std::sync::atomic::{AtomicI64, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

/// Un deposito che, oltre ai passi, tiene il conto di quanto si è speso.
///
/// Fa da guscio a quello in memoria invece di riscriverlo: le regole su epoche,
/// tentativi doppi e chiusure restano quelle vere, e qui si aggiunge la sola
/// cosa che manca.
struct StoreThatCounts {
    inner: InMemoryRecordStore,
    spent: Mutex<Spend>,
}

impl StoreThatCounts {
    fn new() -> Self {
        Self {
            inner: InMemoryRecordStore::default(),
            spent: Mutex::new(Spend::default()),
        }
    }

    /// Registra una chiamata costata `micros`, come farebbe un motore vero.
    fn charge(&self, micros: i64) {
        let mut spent = self.spent.lock().unwrap_or_else(|held| held.into_inner());
        spent.micros += micros;
        spent.calls += 1;
        spent.dearest_micros = Some(spent.dearest_micros.unwrap_or(0).max(micros));
    }

    /// Registra una chiamata di cui **non si sa** quanto è costata.
    fn charge_unknown(&self) {
        let mut spent = self.spent.lock().unwrap_or_else(|held| held.into_inner());
        spent.calls += 1;
        spent.calls_without_cost += 1;
    }
}

impl RecordStore for StoreThatCounts {
    fn append_started(&self, record: StepRecord) -> Result<(), FlowError> {
        self.inner.append_started(record)
    }

    fn close(
        &self,
        run_id: &str,
        step_id: &str,
        attempt: u32,
        epoch: u64,
        completion: Completion,
    ) -> Result<(), FlowError> {
        self.inner.close(run_id, step_id, attempt, epoch, completion)
    }

    fn records(&self, run_id: &str) -> Result<Vec<StepRecord>, FlowError> {
        self.inner.records(run_id)
    }

    fn spent(&self, _run_id: &str) -> Result<Spend, FlowError> {
        Ok(*self.spent.lock().unwrap_or_else(|held| held.into_inner()))
    }
}

/// Un'azione che costa. Ogni volta che gira, scrive la propria spesa nel
/// deposito — è quello che fa un motore vero, e il tetto la vede solo di lì.
struct CostsMoney {
    store: Arc<StoreThatCounts>,
    micros: i64,
    /// Quante volte è stata eseguita: il numero su cui poggia mezza batteria.
    times: Arc<AtomicUsize>,
}

impl Action for CostsMoney {
    fn execute(&self, _input: &Value, _shared: &SharedState) -> Result<ActionOutcome, ActionError> {
        self.times.fetch_add(1, Ordering::SeqCst);
        self.store.charge(self.micros);
        Ok(ActionOutcome::Went(json!("fatto")))
    }
}

/// Un'azione che spende senza sapere quanto: il caso di codex, che dichiara i
/// token e non il costo.
struct CostsSomethingUnknown {
    store: Arc<StoreThatCounts>,
}

impl Action for CostsSomethingUnknown {
    fn execute(&self, _input: &Value, _shared: &SharedState) -> Result<ActionOutcome, ActionError> {
        self.store.charge_unknown();
        Ok(ActionOutcome::Went(json!("fatto")))
    }
}

/// Un orologio che avanza di uno a ogni domanda.
struct Ticking(AtomicI64);

impl Clock for Ticking {
    fn now(&self) -> Result<i64, FlowError> {
        Ok(self.0.fetch_add(1, Ordering::Relaxed))
    }
}

/// Un passo senza dipendenze che chiama `action`.
fn step(id: &str, action: &str, deps: Vec<String>) -> Step {
    Step {
        id: id.to_owned(),
        deps,
        input_schema: ValueSchema::Any,
        output_schema: ValueSchema::Any,
        when: None,
        action: action.to_owned(),
        max_attempts: 1,
        with: None,
    }
}

/// Una catena di due passi: il secondo aspetta il primo.
fn two_in_a_row() -> Graph {
    Graph::new(vec![
        step("first", "costs", vec![]),
        step("second", "costs", vec!["first".to_owned()]),
    ])
    .expect("grafo valido")
}

/// Esegue la catena con il tetto dato, e dice quanti passi hanno girato.
fn run_with_cap(cap: Option<i64>, price_micros: i64) -> (flow::Execution, usize) {
    let store = Arc::new(StoreThatCounts::new());
    let times = Arc::new(AtomicUsize::new(0));
    let mut actions = flow::ActionRegistry::default();
    actions.register(
        "costs",
        CostsMoney {
            store: Arc::clone(&store),
            micros: price_micros,
            times: Arc::clone(&times),
        },
    );

    let execution = InProcessExecutor
        .execute(
            &two_in_a_row(),
            ExecutionRequest {
                run_id: "corsa".to_owned(),
                root_inputs: Default::default(),
                gates: vec![],
                shared: SharedState::new(),
                spend_cap_micros: cap,
            },
            store.as_ref(),
            &actions,
            &Ticking(AtomicI64::new(0)),
        )
        .expect("l'esecuzione non è un guasto");

    (execution, times.load(Ordering::SeqCst))
}

/// **IL PRIMO PASSO SPENDE PIÙ DEL TETTO, IL SECONDO NON PARTE.**
///
/// È il fatto centrale: chi si ferma non ha speso invano — ha fatto un passo e
/// si è fermato prima del successivo, che è l'unico momento in cui fermarsi
/// costa zero.
#[test]
fn a_run_stops_before_the_step_that_would_break_the_cap() {
    let (execution, ran) = run_with_cap(Some(100), 150);

    assert_eq!(ran, 1, "il primo passo gira, il secondo no");
    let Some(Decision::CapReached(stop)) = execution.decisions.last() else {
        panic!("la corsa doveva fermarsi al tetto, invece: {:?}", execution.decisions.last());
    };
    assert_eq!(stop.cap_micros, 100);
    assert_eq!(stop.spent.micros, 150);
    assert_eq!(
        stop.not_started,
        vec!["second".to_owned()],
        "e dice quale passo è rimasto da fare"
    );
}

/// **LO STESSO GRAFO SENZA TETTO ARRIVA IN FONDO.**
///
/// È la metà che rende leggibile la prova sopra: senza di questa, «un passo su
/// due» potrebbe essere un difetto dell'esecutore invece dell'effetto del tetto.
#[test]
fn the_same_flow_without_a_cap_runs_to_the_end() {
    let (execution, ran) = run_with_cap(None, 150);

    assert_eq!(ran, 2, "senza tetto girano tutti e due");
    assert_eq!(execution.decisions.last(), Some(&Decision::Complete));
}

/// **UN TETTO DI ZERO FERMA PRIMA DELLA PRIMA CHIAMATA.**
///
/// `Some(0)` non è `None`: è qualcuno che ha scritto «questo flusso non deve
/// spendere niente». Il confronto è `>=` apposta — con `>` la prima chiamata
/// passerebbe, e sarebbe l'unica che contava.
#[test]
fn a_cap_of_zero_stops_before_spending_anything() {
    let (execution, ran) = run_with_cap(Some(0), 150);

    assert_eq!(ran, 0, "nessun passo è partito");
    assert!(matches!(
        execution.decisions.last(),
        Some(Decision::CapReached(_))
    ));
}

/// **UN TETTO LARGO NON FERMA NIENTE.** Il tetto c'è, e la corsa arriva in
/// fondo: senza questa prova, un tetto che fermasse *sempre* sarebbe verde su
/// tutte le altre.
#[test]
fn a_cap_that_is_never_reached_changes_nothing() {
    let (execution, ran) = run_with_cap(Some(1_000_000), 150);

    assert_eq!(ran, 2);
    assert_eq!(execution.decisions.last(), Some(&Decision::Complete));
}

/// **LA CORSA FERMATA DICE ANCHE QUELLO CHE NON SA.**
///
/// Un motore che non dichiara il costo lascia una riga senza cifra: la spesa
/// vera è più alta di quella contata, e chi legge deve vederlo scritto invece di
/// dedurlo. Qui il primo passo spende ignoto, il secondo spende oltre il tetto,
/// e il terzo trova la corsa chiusa.
#[test]
fn what_the_cap_does_not_know_is_declared() {
    let store = Arc::new(StoreThatCounts::new());
    let times = Arc::new(AtomicUsize::new(0));
    let mut actions = flow::ActionRegistry::default();
    actions.register(
        "unknown",
        CostsSomethingUnknown {
            store: Arc::clone(&store),
        },
    );
    actions.register(
        "costs",
        CostsMoney {
            store: Arc::clone(&store),
            micros: 150,
            times: Arc::clone(&times),
        },
    );
    let graph = Graph::new(vec![
        step("first", "unknown", vec![]),
        step("second", "costs", vec!["first".to_owned()]),
        step("third", "costs", vec!["second".to_owned()]),
    ])
    .expect("grafo valido");

    let execution = InProcessExecutor
        .execute(
            &graph,
            ExecutionRequest {
                run_id: "corsa".to_owned(),
                root_inputs: Default::default(),
                gates: vec![],
                shared: SharedState::new(),
                spend_cap_micros: Some(100),
            },
            store.as_ref(),
            &actions,
            &Ticking(AtomicI64::new(0)),
        )
        .expect("l'esecuzione non è un guasto");

    let Some(Decision::CapReached(stop)) = execution.decisions.last() else {
        panic!("doveva fermarsi al tetto");
    };
    assert_eq!(stop.spent.calls, 2, "due chiamate in tutto");
    assert_eq!(
        stop.spent.calls_without_cost, 1,
        "una delle due non ha detto quanto è costata"
    );
    assert!(
        !stop.spent.is_complete(),
        "e il totale si dichiara incompleto, invece di passare per esatto"
    );
}

/// **IL FRONTE SI STRINGE QUANDO IL RESIDUO SI STRINGE.**
///
/// Quattro passi indipendenti, tetto e prezzi scelti perché nel residuo ne
/// stiano due: partono a due per volta invece che a quattro. Il numero non è
/// una preferenza — con quattro chiamate in volo lo sforamento peggiore è
/// quattro volte la più cara, e nessuna delle quattro sa delle altre.
#[test]
fn the_front_narrows_as_the_money_runs_out() {
    let store = Arc::new(StoreThatCounts::new());
    // Una chiamata già fatta, da 100: da lì viene la stima del caso peggiore.
    store.charge(100);
    let together = Arc::new(Mutex::new(Vec::new()));
    let mut actions = flow::ActionRegistry::default();
    actions.register(
        "counts",
        CountsCompany {
            live: Arc::new(AtomicUsize::new(0)),
            most: Arc::clone(&together),
        },
    );
    let graph = Graph::new(
        (1..=4)
            .map(|n| step(&format!("s{n}"), "counts", vec![]))
            .collect(),
    )
    .expect("grafo valido");

    InProcessExecutor
        .execute(
            &graph,
            ExecutionRequest {
                run_id: "corsa".to_owned(),
                root_inputs: Default::default(),
                gates: vec![],
                shared: SharedState::new(),
                // Speso 100, tetto 350: ne restano 250, e nella più cara vista
                // (100) ce ne stanno due.
                spend_cap_micros: Some(350),
            },
            store.as_ref(),
            &actions,
            &Ticking(AtomicI64::new(0)),
        )
        .expect("l'esecuzione non è un guasto");

    let seen = together.lock().unwrap_or_else(|held| held.into_inner());
    let most_at_once = seen.iter().copied().max().unwrap_or(0);
    assert_eq!(
        most_at_once, 2,
        "il residuo ne consentiva due per volta, non quattro: {seen:?}"
    );
}

/// Un'azione che dice quanti erano vivi insieme a lei nel momento in cui è
/// entrata.
struct CountsCompany {
    live: Arc<AtomicUsize>,
    most: Arc<Mutex<Vec<usize>>>,
}

impl Action for CountsCompany {
    fn execute(&self, _input: &Value, _shared: &SharedState) -> Result<ActionOutcome, ActionError> {
        let now_live = self.live.fetch_add(1, Ordering::SeqCst) + 1;
        // Abbastanza da lasciare che i compagni di ondata entrino: senza questa
        // pausa un gruppo di due potrebbe sfilare uno alla volta e sembrare uno.
        std::thread::sleep(std::time::Duration::from_millis(50));
        self.most
            .lock()
            .unwrap_or_else(|held| held.into_inner())
            .push(now_live.max(self.live.load(Ordering::SeqCst)));
        self.live.fetch_sub(1, Ordering::SeqCst);
        Ok(ActionOutcome::Went(json!("fatto")))
    }
}

/// Il fatto che tiene insieme le prove sopra: un passo che gira è un passo
/// chiuso come `Went` nel deposito, non solo un contatore che sale.
#[test]
fn the_step_that_ran_is_closed_in_the_store() {
    let store = Arc::new(StoreThatCounts::new());
    let times = Arc::new(AtomicUsize::new(0));
    let mut actions = flow::ActionRegistry::default();
    actions.register(
        "costs",
        CostsMoney {
            store: Arc::clone(&store),
            micros: 150,
            times,
        },
    );

    InProcessExecutor
        .execute(
            &two_in_a_row(),
            ExecutionRequest {
                run_id: "corsa".to_owned(),
                root_inputs: Default::default(),
                gates: vec![],
                shared: SharedState::new(),
                spend_cap_micros: Some(100),
            },
            store.as_ref(),
            &actions,
            &Ticking(AtomicI64::new(0)),
        )
        .expect("l'esecuzione non è un guasto");

    let records = store.records("corsa").expect("leggere i passi");
    assert_eq!(records.len(), 1, "il secondo non è mai stato aperto");
    assert_eq!(records[0].step_id, "first");
    assert_eq!(records[0].outcome, Some(Outcome::Went));
}
