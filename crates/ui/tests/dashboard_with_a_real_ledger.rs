//! Prova end-to-end: un deposito vero (in una cartella temporanea, fuori
//! dall'albero) con esecuzioni finte, letto dalla pipeline pura.
//!
//! **IL SERVITORE HTTP NON C'E' PIU', LE SUE DOMANDE SI'.** Fino al 31/08/2026
//! tre prove qui dentro aprivano un socket vero e interrogavano
//! `127.0.0.1`. Quel servitore e' stato tolto — l'unica interfaccia e' la
//! finestra — ma cio' che quelle prove difendevano non era il socket: era che
//! i conti arrivino a chi guarda **con i nomi di campo che chi guarda legge**.
//! Quella parte e' rimasta, girata sulla serializzazione invece che sulla
//! rete. Togliere le prove insieme al trasporto avrebbe tolto anche il
//! controllo, che e' il modo in cui una riscrittura perde pezzi senza
//! accorgersene.

use flow::{Completion, Outcome, StepRecord};
use ledger::{Ledger, ModelCallRecord, RunRecord};
use std::path::{Path, PathBuf};

fn temp_dir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("ui-crate-test-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    dir
}

fn seed(dir: &Path) {
    let ledger = Ledger::open(dir).expect("apertura del deposito di prova");
    ledger
        .record_run(&RunRecord {
            run_id: "run-1".into(),
            kind: "sweep".into(),
            entity: "marker-sweep".into(),
            parent_run_id: None,
            started_by: "prova".into(),
            status: "running".into(),
            total_cost_micros: 0,
            error: None,
            started_at: 1000,
            ended_at: None,
        })
        .expect("registrare la corsa");

    ledger
        .append_step_started(&StepRecord::started(
            "run-1",
            "scan_markers",
            1,
            1,
            vec![],
            serde_json::json!({}),
            vec![],
            1000,
        ))
        .expect("passo avviato");
    ledger
        .close_step(
            "run-1",
            "scan_markers",
            1,
            1,
            Completion {
                outcome: Outcome::Went,
                output: Some(serde_json::json!({"ok": true})),
                said: None,
                failure_class: None,
                ended_at: 1010,
                bytes_seen: None,
                bytes_discarded: None,
            },
        )
        .expect("passo chiuso");

    ledger
        .append_step_started(&StepRecord::started(
            "run-1",
            "remove_markers",
            1,
            1,
            vec!["scan_markers".into()],
            serde_json::json!({}),
            vec![],
            1050,
        ))
        .expect("passo aperto avviato, mai chiuso: è ancora in corso");

    ledger
        .record_model_call(&ModelCallRecord {
            call_id: "call-1".into(),
            run_id: "run-1".into(),
            step_id: Some("scan_markers".into()),
            purpose: "classifica".into(),
            cli: "claude".into(),
            requested_model: "sonnet".into(),
            actual_model: "claude-sonnet-5".into(),
            input_tokens: Some(100),
            output_tokens: Some(50),
            cached_tokens: Some(10),
            cache_write_tokens: None,
            cache_write_long_tokens: None,
            total_tokens: None,
            turns: None,
            cost_micros: Some(500),
            declared_cost_micros: None,
            price_currency: Some("USD".into()),
            input_price_micros_per_million: Some(3_000_000),
            output_price_micros_per_million: Some(15_000_000),
            cached_price_micros_per_million: Some(300_000),
            cache_write_price_micros_per_million: None,
            cache_write_long_price_micros_per_million: None,
            mandate_name: "prova".into(),
            mandate_version: "1".into(),
            retry_chain: vec![],
            error_type: None,
            started_at: 1001,
            ended_at: Some(1009),
            session_id: None,
        })
        .expect("registrare la chiamata al modello");
}

#[test]
fn gather_summarizes_a_seeded_ledger() {
    let dir = temp_dir("gather");
    seed(&dir);

    let data = ui::gather::gather(&dir)
        .expect("lettura riuscita")
        .expect("il deposito appena scritto è presente");
    assert_eq!(data.runs.len(), 1);

    let executions =
        ui::dashboard::build_executions(&data.runs, &data.steps_by_run, &data.calls_by_run, 1100);
    let execution = &executions[0];
    assert_eq!(execution.run_id, "run-1");
    assert_eq!(execution.steps_total, 2);
    assert_eq!(execution.steps_went, 1);
    assert_eq!(execution.steps_open.len(), 1);
    assert_eq!(execution.steps_open[0].step_id, "remove_markers");
    assert_eq!(execution.steps_open[0].open_for_secs, 50);
    assert_eq!(execution.tokens.input_tokens, 100);
    assert_eq!(execution.tokens.cost_micros, 500);
    assert_eq!(
        execution.tokens_by_model.get("claude-sonnet-5").expect("modello presente").calls,
        1
    );

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_ledger_directory_that_was_never_written_is_reported_as_absent_not_as_an_error() {
    let dir = temp_dir("missing");
    let data = ui::gather::gather(&dir).expect("nessun errore su un deposito assente");
    assert!(data.is_none());
    assert!(!dir.exists(), "leggere lo stato non deve creare il deposito");
}

#[test]
fn the_shape_the_window_reads_survives_serialization() {
    // I NOMI DEI CAMPI SONO UN CONTRATTO, e vive fra due linguaggi: `ExecutionView`
    // di qui e `Execution` di `desktop/src/engine.ts`. Rinominarne uno da questa
    // parte non fa cadere niente — la finestra legge `undefined` e disegna una
    // colonna vuota. Questa prova e' cio' che rende rossa quella modifica.
    let dir = temp_dir("shape");
    seed(&dir);
    let data = ui::gather::gather(&dir).expect("lettura riuscita").expect("deposito presente");
    let executions =
        ui::dashboard::build_executions(&data.runs, &data.steps_by_run, &data.calls_by_run, 1100);
    let body = serde_json::to_value(&executions).expect("le viste si serializzano");

    assert_eq!(body[0]["run_id"], "run-1");
    assert_eq!(body[0]["tokens"]["input_tokens"].as_u64(), Some(100));
    assert_eq!(body[0]["steps_open"][0]["step_id"], "remove_markers");
    // Le due cifre che devono restare affiancate: quella che Sailor calcola e
    // quella che il motore dichiara. Se una delle due sparisse dal JSON, la
    // finestra mostrerebbe una colonna vuota invece di un disaccordo.
    assert!(body[0]["calls"][0].get("cost_micros").is_some());
    assert!(body[0]["calls"][0].get("declared_cost_micros").is_some());
    // E cio' che non e' stato misurato, che e' la riga piu' importante di tutte.
    assert!(body[0]["tokens"].get("calls_without_tokens").is_some());
    assert!(body[0]["tokens"].get("calls_without_cost").is_some());

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_broken_flow_keeps_its_place_in_the_registry_with_its_reason() {
    // UN FLUSSO ROTTO NON SPARISCE. Un elenco che si accorcia in silenzio fa
    // credere che il flusso non esista, e nessuno va a cercare un file che
    // secondo l'elenco non c'e'.
    let valid_graph = flow::Graph::new(vec![flow::Step {
        id: "step-1".into(),
        deps: vec![],
        input_schema: flow::ValueSchema::Any,
        output_schema: flow::ValueSchema::Any,
        with: None,
        when: None,
        action: "action-1".into(),
        max_attempts: 1,
    }])
    .expect("grafo valido");
    let mut flows = ui::registry::FlowRegistry::new();
    flows.insert(
        "valido".into(),
        Ok(ui::registry::FlowFile {
            id: "valido".into(),
            description: "Flusso valido di prova".into(),
            graph: valid_graph,
            inputs: std::collections::BTreeMap::new(),
            schedule: None,
            spend_cap_micros: None,
        }),
    );
    flows.insert("rotto".into(), Err("errore: ciclo nel grafo".into()));

    let views = serde_json::to_value(ui::registry::flow_views(&flows)).expect("le viste si serializzano");
    let array = views.as_array().expect("array dei flussi");
    assert_eq!(array.len(), 2);

    // I nomi cercati restano in italiano: sono i dati del flusso di prova, non
    // identificatori. E' la variabile che li tiene a dover essere in inglese.
    let broken = array.iter().find(|entry| entry["name"] == "rotto").expect("flusso rotto presente");
    assert_eq!(broken["error"], "errore: ciclo nel grafo");
    assert_eq!(broken["steps"].as_array().map(|steps| steps.len()), Some(0));

    let valid = array.iter().find(|entry| entry["name"] == "valido").expect("flusso valido presente");
    assert_eq!(valid["error"], serde_json::Value::Null);
    assert_eq!(valid["description"], "Flusso valido di prova");
    assert_eq!(valid["steps"].as_array().map(|steps| steps.len()), Some(1));
}
