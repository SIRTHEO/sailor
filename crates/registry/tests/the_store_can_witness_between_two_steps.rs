//! Il deposito fa il testimone fra due passi: **col registro vero, il deposito
//! vero e l'esecutore vero**, senza nessun finto in mezzo.
//!
//! **PERCHÉ ESISTE — IL SINTOMO DEL GUASTO 28, RESO UN FATTO A COSTO ZERO.** Il
//! 31/08/2026 un passo che scriveva nel deposito una chiave presa dal passo
//! prima moriva con «invalid type: map, expected a string»: `store_write` non
//! scioglieva i rinvii, quindi riceveva `{"$from": "/stdout"}` come oggetto.
//! Il deposito accettava solo valori scritti a mano dentro il flusso, cioè non
//! poteva fare il testimone che `docs/decisioni.md` gli attribuisce. A rivelarlo
//! era stata una corsa da **8,95 $** morta sull'ultimo passo; le due fixture che
//! l'hanno inchiodato — `flows/prova-deposito.flow.json` e la gemella con la
//! chiave letterale — non esistono più nell'albero, e la loro misura si era
//! persa con loro.
//!
//! **PERCHÉ QUI E NON IN `crates/actions`.** La prova che stava là chiamava
//! `execute` a mano: adesso che i rinvii li scioglie `flow::step_input`, una
//! prova così dovrebbe scioglierli lei prima di chiamare — cioè provare se
//! stessa. Qui invece passano tutti e tre i pezzi che quel giorno erano in
//! gioco: il registro che monta le azioni (`default_registry`), l'esecutore che
//! compone l'ingresso, e SQLite che riceve la chiave.
//!
//! **NON SI SPENDE NIENTE E NON SI CHIAMA NESSUN MODELLO**: il primo passo è
//! `sh -c printf`, cioè un motore che risponde senza fornitori.
//!
//! **IL MUTANTE**: togliere `resolve_references` da `step_input` — il difetto
//! originale. La corsa non arriva in fondo e il passo che deposita si rompe con
//! `invalid_input`, le stesse parole del 31/08.

use flow::{
    ExecutionRequest, Executor, Graph, InProcessExecutor, Outcome, RecordStore, SharedState, Step,
    SystemClock, ValueSchema,
};
use ledger::Ledger;
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

/// Una cartella usa-e-getta per ogni prova.
///
/// **IL CONTATORE NEL NOME NON È ORNAMENTO**: è il guasto 21. `cargo test`
/// manda le prove sullo stesso processo e l'orologio di macOS non ha la
/// risoluzione del nanosecondo.
struct TestDirectory(PathBuf);

static NEXT_DIRECTORY: AtomicU64 = AtomicU64::new(0);

impl TestDirectory {
    fn new(label: &str) -> Self {
        let serial = NEXT_DIRECTORY.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!(
            "sailor-store-witness-{label}-{}-{serial}",
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

fn step(id: &str, deps: &[&str], action: &str, with: Value) -> Step {
    Step {
        id: id.to_owned(),
        deps: deps.iter().map(|dep| (*dep).to_owned()).collect(),
        action: action.to_owned(),
        max_attempts: 1,
        when: None,
        with: Some(with),
        input_schema: ValueSchema::Any,
        output_schema: ValueSchema::Any,
    }
}

/// **LA CHIAVE VIENE DAL PASSO PRIMA, E IL DEPOSITO LA RICEVE COME TESTO.**
///
/// Tre passi: un motore risponde, il deposito scrive sotto la chiave che quel
/// motore ha detto, e un terzo passo rilegge la voce nominando la chiave a mano.
/// Se il rinvio non fosse sciolto, il secondo passo si romperebbe e il terzo non
/// troverebbe niente — che è esattamente com'è andata il 31/08.
#[test]
fn a_key_decided_by_the_step_before_reaches_the_real_store() {
    let dir = TestDirectory::new("chiave-dal-passo-prima");
    let ledger = Ledger::open(&dir.0.join("deposito")).expect("aprire il deposito");
    let actions = registry::default_registry(Some(ledger.clone()), None);

    let graph = Graph::new(vec![
        step(
            "chiedi",
            &[],
            "external_engine",
            json!({"bin": "sh", "args": ["-c", "printf 'il-lavoro-di-ieri'"], "timeout_secs": 10}),
        ),
        step(
            "deposita",
            &["chiedi"],
            "store_write",
            json!({
                "collection": "mandato",
                "key": {"$from": "/stdout"},
                "value": {"deciso_da": {"$from": "/stdout"}},
                "written_by": "prova-del-testimone",
                "written_at": 1_756_400_000i64,
            }),
        ),
        step(
            "rileggi",
            &["deposita"],
            "store_read",
            json!({"collection": "mandato", "key": "il-lavoro-di-ieri"}),
        ),
    ])
    .expect("grafo valido");

    let mut store = ledger.clone();
    InProcessExecutor
        .execute(
            &graph,
            ExecutionRequest {
                run_id: "corsa-del-testimone".to_owned(),
                root_inputs: BTreeMap::new(),
                gates: vec![],
                shared: SharedState::new(),
                spend_cap_micros: None,
            },
            &mut store,
            &actions,
            &SystemClock,
        )
        .expect("la corsa arriva in fondo");

    let records = store
        .records("corsa-del-testimone")
        .expect("rileggere i record della corsa");
    let closed: BTreeMap<String, Outcome> = records
        .iter()
        .filter_map(|record| {
            record
                .outcome
                .clone()
                .map(|outcome| (record.step_id.clone(), outcome))
        })
        .collect();
    assert_eq!(
        closed.get("deposita"),
        Some(&Outcome::Went),
        "il passo che deposita non è andato: {:?}",
        records
            .iter()
            .filter(|record| record.step_id == "deposita")
            .map(|record| record.said.clone())
            .collect::<Vec<_>>()
    );

    let read = records
        .iter()
        .find(|record| record.step_id == "rileggi" && record.outcome == Some(Outcome::Went))
        .and_then(|record| record.output.clone())
        .expect("il passo che rilegge ha un'uscita");
    assert_eq!(
        read["found"],
        json!(true),
        "la voce non è stata trovata sotto la chiave che il motore ha detto: {read}"
    );
    assert_eq!(read["value"]["deciso_da"], json!("il-lavoro-di-ieri"));
    assert_eq!(read["written_by"], json!("prova-del-testimone"));
}
