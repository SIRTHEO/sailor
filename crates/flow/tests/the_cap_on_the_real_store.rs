//! Il tetto di spesa contro il deposito **vero**: l'esecutore e `Ledger`
//! insieme, senza nessun finto in mezzo.
//!
//! **PERCHÉ ESISTE, E COSA NON PROVAVA NESSUNO.** Il tetto era provato due
//! volte e mai nel punto in cui vive. `the_spending_cap.rs` costruisce un
//! deposito apposta (`StoreThatCounts`) che risponde a `spent()` da un contatore
//! in memoria: misura l'esecutore, non il deposito. `spent_in_run` è provato in
//! `crates/ledger/src/tests.rs` scrivendo righe e rileggendole: misura il
//! deposito, non l'esecutore. Fra i due c'è la giuntura — l'esecutore che
//! interroga SQLite mentre la corsa gira — e una giuntura non provata è
//! esattamente il posto dove un tetto smette di fermare qualcosa senza dirlo a
//! nessuno.
//!
//! **QUI NON GIRA NESSUN MOTORE E NON SI SPENDE NIENTE.** L'azione scrive nel
//! deposito la riga che scriverebbe un motore vero (`ModelCallRecord` con un
//! costo scelto), e prende la corsa dallo stato condiviso come fa
//! `actions::recording_for`. Il costo è finto; la strada che quel costo percorre
//! — riga scritta, `SUM` riletto, confronto col tetto, fronte non aperto — è
//! quella vera.
//!
//! **CHE COSA IL TETTO GARANTISCE DAVVERO**, e va scritto o qualcuno ci conterà
//! sopra: il controllo sta **prima di aprire un fronte**, mai dentro un passo
//! che sta già girando. Quindi il primo fronte di una corsa non è mai frenato —
//! `how_many_fit` senza nessuna chiamata osservata resta al soffitto di quattro,
//! perché stringere su un numero che non esiste sarebbe inventarlo. Il tetto è
//! un freno dal secondo fronte in poi.

use flow::{
    Action, ActionError, ActionOutcome, Clock, Decision, ExecutionRequest, Executor, FlowError,
    Graph, InProcessExecutor, Outcome, Step, SharedState, ValueSchema, CURRENT_RUN,
};
use ledger::{Ledger, ModelCallRecord};
use serde_json::{json, Value};
use std::path::PathBuf;
use std::sync::atomic::{AtomicI64, AtomicU64, AtomicUsize, Ordering};
use std::sync::Arc;

/// Una cartella usa-e-getta per ogni prova.
///
/// **IL CONTATORE NEL NOME NON È ORNAMENTO**: è il guasto 21. `cargo test`
/// manda le prove sullo stesso processo e l'orologio di macOS non ha la
/// risoluzione del nanosecondo, quindi due prove che si costruiscono il nome dal
/// solo pid si rubano la cartella a vicenda — una esecuzione su venti falliva,
/// ogni volta su una prova diversa.
struct TestDirectory(PathBuf);

static NEXT_DIRECTORY: AtomicU64 = AtomicU64::new(0);

impl TestDirectory {
    fn new(label: &str) -> Self {
        let serial = NEXT_DIRECTORY.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!(
            "sailor-cap-real-store-{label}-{}-{serial}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&path);
        std::fs::create_dir_all(&path).expect("creare la cartella del deposito di prova");
        Self(path)
    }
}

impl Drop for TestDirectory {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// Un'azione che spende per davvero: scrive nel deposito la riga di una
/// chiamata costata `micros`.
///
/// **LA CORSA SE LA PRENDE DALLO STATO CONDIVISO**, esattamente come
/// `actions::recording_for`, e non da un campo suo. È il pezzo che rende questa
/// prova diversa dalle altre: se l'esecutore smettesse di mettere `CURRENT_RUN`
/// nello stato, le righe finirebbero attribuite a nessuno e il tetto non
/// vedrebbe più niente — che è un modo silenzioso di non funzionare.
struct CostsForReal {
    ledger: Ledger,
    micros: i64,
    /// Quante volte è stata eseguita: il numero su cui poggia mezza prova.
    times: Arc<AtomicUsize>,
}

static NEXT_CALL: AtomicU64 = AtomicU64::new(0);

impl Action for CostsForReal {
    fn execute(&self, _input: &Value, shared: &SharedState) -> Result<ActionOutcome, ActionError> {
        self.times.fetch_add(1, Ordering::SeqCst);
        let run_id = shared
            .get(CURRENT_RUN)
            .and_then(Value::as_str)
            .ok_or_else(|| {
                ActionError::new(
                    "no_run",
                    "lo stato condiviso non porta la corsa: nessuna spesa sarebbe attribuibile",
                )
            })?
            .to_owned();
        let sequence = NEXT_CALL.fetch_add(1, Ordering::Relaxed);
        self.ledger
            .record_model_call(&a_call_that_cost(
                &format!("{run_id}:{sequence}"),
                &run_id,
                self.micros,
            ))
            .map_err(|error| ActionError::new("store", error.to_string()))?;
        Ok(ActionOutcome::Went(json!("fatto")))
    }
}

/// La riga che un motore vero lascia dietro di sé, ridotta a ciò che il tetto
/// legge: la corsa e il costo.
fn a_call_that_cost(call_id: &str, run_id: &str, micros: i64) -> ModelCallRecord {
    ModelCallRecord {
        call_id: call_id.to_owned(),
        run_id: run_id.to_owned(),
        step_id: None,
        // Nessuna sessione: questa prova guarda il tetto di spesa, e una riga
        // che non ne apre né ne riprende una è il caso normale.
        session_id: None,
        purpose: "external_engine".to_owned(),
        cli: "motore-di-prova".to_owned(),
        requested_model: String::new(),
        actual_model: String::new(),
        input_tokens: None,
        output_tokens: None,
        cached_tokens: None,
        cache_write_tokens: None,
        cache_write_long_tokens: None,
        total_tokens: None,
        turns: None,
        cost_micros: Some(micros),
        declared_cost_micros: None,
        price_currency: None,
        input_price_micros_per_million: None,
        output_price_micros_per_million: None,
        cached_price_micros_per_million: None,
        cache_write_price_micros_per_million: None,
        cache_write_long_price_micros_per_million: None,
        mandate_name: String::new(),
        mandate_version: String::new(),
        retry_chain: vec![],
        error_type: None,
        started_at: 100,
        ended_at: Some(110),
    }
}

/// Un orologio che avanza di uno a ogni domanda.
struct Ticking(AtomicI64);

impl Clock for Ticking {
    fn now(&self) -> Result<i64, FlowError> {
        Ok(self.0.fetch_add(1, Ordering::Relaxed))
    }
}

fn step(id: &str, deps: Vec<String>) -> Step {
    Step {
        id: id.to_owned(),
        deps,
        input_schema: ValueSchema::Any,
        output_schema: ValueSchema::Any,
        when: None,
        action: "costs".to_owned(),
        max_attempts: 1,
        with: None,
    }
}

/// Due passi in fila: il secondo aspetta il primo, quindi sono due fronti — ed
/// è fra un fronte e l'altro che il tetto lavora.
fn two_in_a_row() -> Graph {
    Graph::new(vec![
        step("first", vec![]),
        step("second", vec!["first".to_owned()]),
    ])
    .expect("grafo valido")
}

/// Com'è andata una corsa sul deposito vero: la decisione finale, quanti passi
/// hanno girato, e cosa risulta scritto nella tabella `steps`.
struct HowItWent {
    execution: flow::Execution,
    ran: usize,
    written: Vec<String>,
    spent_after: flow::Spend,
}

/// Esegue la catena con il tetto dato, su un `Ledger` aperto in una cartella
/// usa-e-getta e passato all'esecutore **come deposito**.
fn run_on_a_real_ledger(label: &str, cap: Option<i64>, price_micros: i64) -> HowItWent {
    let directory = TestDirectory::new(label);
    let ledger = Ledger::open(&directory.0).expect("aprire il deposito");
    let times = Arc::new(AtomicUsize::new(0));
    let mut actions = flow::ActionRegistry::default();
    actions.register(
        "costs",
        CostsForReal {
            ledger: ledger.clone(),
            micros: price_micros,
            times: Arc::clone(&times),
        },
    );

    let run_id = format!("corsa-{label}");
    let execution = InProcessExecutor
        .execute(
            &two_in_a_row(),
            ExecutionRequest {
                run_id: run_id.clone(),
                root_inputs: Default::default(),
                gates: vec![],
                shared: SharedState::new(),
                spend_cap_micros: cap,
            },
            &ledger,
            &actions,
            &Ticking(AtomicI64::new(0)),
        )
        .expect("l'esecuzione non è un guasto");

    let written = ledger
        .steps(&run_id)
        .expect("rileggere la tabella dei passi")
        .into_iter()
        .filter(|record| record.outcome == Some(Outcome::Went))
        .map(|record| record.step_id)
        .collect();
    let spent_after = ledger.spent_in_run(&run_id).expect("rileggere la spesa");

    HowItWent {
        execution,
        ran: times.load(Ordering::SeqCst),
        written,
        spent_after,
    }
}

/// **LA SPESA VERA, LETTA DAL DEPOSITO VERO, FERMA LA CORSA.**
///
/// Il primo passo scrive nel deposito una chiamata da 150; il tetto è 100.
/// Prima di aprire il secondo fronte l'esecutore chiede al `Ledger` quanto è
/// stato speso, il `Ledger` lo conta con `SUM(cost_micros)` sulla tabella
/// `model_calls`, e la corsa si ferma.
///
/// **IL MUTANTE CHE QUESTA PROVA ESISTE PER PRENDERE**: in
/// `crates/ledger/src/lib.rs` sostituire `COALESCE(SUM(cost_micros), 0)` con
/// `0`. Il deposito continua a registrare tutto, ogni altra prova resta verde —
/// comprese le sette di `the_spending_cap.rs`, che il deposito vero non lo
/// toccano — e qui la corsa arriva in fondo invece di fermarsi.
#[test]
fn the_cap_stops_the_run_on_what_the_real_ledger_counted() {
    let went = run_on_a_real_ledger("tetto-sotto-il-costo", Some(100), 150);

    assert_eq!(went.ran, 1, "il primo passo gira, il secondo no");
    let Some(Decision::CapReached(stop)) = went.execution.decisions.last() else {
        panic!(
            "la corsa doveva fermarsi al tetto, invece: {:?}",
            went.execution.decisions.last()
        );
    };
    assert_eq!(stop.cap_micros, 100);
    assert_eq!(
        stop.spent.micros, 150,
        "la cifra arriva dal deposito, non da un contatore della prova"
    );
    assert_eq!(stop.spent.calls, 1, "una chiamata registrata, e una sola");
    assert!(
        stop.spent.is_complete(),
        "il motore di prova dichiara il proprio costo: non c'è niente di ignoto"
    );
    assert_eq!(stop.not_started, vec!["second".to_owned()]);

    // **IL SECONDO PASSO NON ESISTE NELLA TABELLA `steps` VERA.** Non «è
    // fallito», non «è rimasto in attesa»: non è mai stato aperto. È la sola
    // forma in cui fermarsi costa zero, ed è ciò che distingue un tetto da un
    // annullamento a metà strada.
    assert_eq!(
        went.written,
        vec!["first".to_owned()],
        "nel deposito deve esserci solo il passo che è girato"
    );
}

/// **LO STESSO GRAFO, LO STESSO DEPOSITO, SENZA TETTO: ARRIVA IN FONDO.**
///
/// È la metà che rende leggibile quella sopra. Senza, «un passo su due»
/// potrebbe essere un difetto dell'esecutore o del deposito invece dell'effetto
/// del tetto.
///
/// **IL SECONDO MUTANTE, quello che la prova sopra da sola non prende**: nella
/// costruzione della richiesta rimettere `spend_cap_micros: None`. Se la prova
/// sopra restasse verde con quel mutante, starebbe misurando l'esecutore e non
/// il passaggio del tetto.
#[test]
fn without_a_cap_the_same_chain_runs_to_the_end_on_the_same_store() {
    let went = run_on_a_real_ledger("senza-tetto", None, 150);

    assert_eq!(went.ran, 2, "senza tetto girano tutti e due");
    assert_eq!(went.execution.decisions.last(), Some(&Decision::Complete));
    assert_eq!(
        went.written,
        vec!["first".to_owned(), "second".to_owned()],
        "e tutti e due sono nel deposito"
    );
    // La prova che il deposito ha davvero registrato due spese: senza questa
    // riga, un `record_model_call` che fallisse in silenzio lascerebbe verde
    // tutto quanto — e il tetto della prova sopra si fermerebbe su zero
    // chiamate per la ragione sbagliata.
    assert_eq!(went.spent_after.micros, 300);
    assert_eq!(went.spent_after.calls, 2);
}

/// **UN TETTO DI ZERO NON APRE NEMMENO IL PRIMO FRONTE, E IL DEPOSITO RESTA
/// VUOTO.**
///
/// `Some(0)` non è `None`: è qualcuno che ha scritto «questo flusso non deve
/// spendere niente». Il confronto nell'esecutore è `>=` apposta — con `>` la
/// prima chiamata passerebbe, ed è l'unica che contava. Qui si vede sul deposito
/// vero: zero righe in `steps`, zero in `model_calls`.
#[test]
fn a_cap_of_zero_writes_nothing_at_all_in_the_real_store() {
    let went = run_on_a_real_ledger("tetto-a-zero", Some(0), 150);

    assert_eq!(went.ran, 0, "nessun passo è partito");
    assert!(matches!(
        went.execution.decisions.last(),
        Some(Decision::CapReached(_))
    ));
    assert!(went.written.is_empty(), "nessun passo nel deposito");
    assert_eq!(went.spent_after, flow::Spend::default(), "nessuna spesa");
}
