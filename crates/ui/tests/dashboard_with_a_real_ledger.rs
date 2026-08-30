//! Prova end-to-end: un deposito vero (in una cartella temporanea, fuori
//! dall'albero) con esecuzioni finte, letto dalla pipeline pura e servito
//! dal vero servitore HTTP su un socket vero.

use flow::{Completion, Outcome, StepRecord};
use ledger::{Ledger, ModelCallRecord, RunRecord};
use std::io::{Read, Write};
use std::net::TcpListener;
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
            total_tokens: None,
            cost_micros: Some(500),
            declared_cost_micros: None,
            price_currency: Some("USD".into()),
            input_price_micros_per_million: Some(3_000_000),
            output_price_micros_per_million: Some(15_000_000),
            cached_price_micros_per_million: Some(300_000),
            mandate_name: "prova".into(),
            mandate_version: "1".into(),
            retry_chain: vec![],
            error_type: None,
            started_at: 1001,
            ended_at: Some(1009),
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
fn the_http_server_answers_the_real_dashboard_over_a_real_socket() {
    let dir = temp_dir("http");
    seed(&dir);
    let state = std::sync::Arc::new(ui::server::ServerState {
        ledger_dir: dir.clone(),
        flows: Default::default(),
    });

    let listener = TcpListener::bind("127.0.0.1:0").expect("bind su una porta libera");
    let addr = listener.local_addr().expect("indirizzo assegnato dal sistema");
    let acceptor = listener.try_clone().expect("duplicare il listener");
    let accepted = std::thread::spawn(move || acceptor.accept().expect("connessione in arrivo"));

    let mut client = std::net::TcpStream::connect(addr).expect("connessione al servitore");
    client
        .write_all(b"GET /api/dashboard HTTP/1.1\r\nHost: localhost\r\n\r\n")
        .expect("richiesta inviata");

    let (connection, _) = accepted.join().expect("il thread di ascolto non è andato in panico");
    let handler =
        std::thread::spawn(move || ui::server::handle_connection(connection, &state).expect("risposta scritta"));

    let mut response = String::new();
    client.read_to_string(&mut response).expect("risposta letta fino alla chiusura");
    handler.join().expect("il servitore ha risposto senza andare in panico");

    assert!(response.starts_with("HTTP/1.1 200"), "risposta inattesa: {response}");
    let body_start = response.find("\r\n\r\n").expect("separatore fra intestazioni e corpo") + 4;
    let body: serde_json::Value = serde_json::from_str(&response[body_start..]).expect("corpo JSON valido");
    assert_eq!(body["ledger_present"], true);
    assert_eq!(body["executions"][0]["run_id"], "run-1");
    assert_eq!(body["executions"][0]["tokens"]["input_tokens"].as_u64(), Some(100));
    assert_eq!(body["executions"][0]["steps_open"][0]["step_id"], "remove_markers");

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn the_http_server_serves_valid_and_broken_flows_in_the_payload() {
    let dir = temp_dir("http_flows");
    let mut flows = ui::registry::FlowRegistry::new();
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
    flows.insert(
        "valido".into(),
        Ok(ui::registry::FlowFile {
            id: "valido".into(),
            description: "Flusso valido di prova".into(),
            graph: valid_graph,
            inputs: std::collections::BTreeMap::new(),
            schedule: None,
        }),
    );
    flows.insert(
        "rotto".into(),
        Err("errore: ciclo nel grafo".into()),
    );

    let state = std::sync::Arc::new(ui::server::ServerState {
        ledger_dir: dir.clone(),
        flows,
    });

    let listener = TcpListener::bind("127.0.0.1:0").expect("bind su una porta libera");
    let addr = listener.local_addr().expect("indirizzo assegnato dal sistema");
    let acceptor = listener.try_clone().expect("duplicare il listener");
    let accepted = std::thread::spawn(move || acceptor.accept().expect("connessione in arrivo"));

    let mut client = std::net::TcpStream::connect(addr).expect("connessione al servitore");
    client
        .write_all(b"GET /api/dashboard HTTP/1.1\r\nHost: localhost\r\n\r\n")
        .expect("richiesta inviata");

    let (connection, _) = accepted.join().expect("il thread di ascolto non è andato in panico");
    let handler =
        std::thread::spawn(move || ui::server::handle_connection(connection, &state).expect("risposta scritta"));

    let mut response = String::new();
    client.read_to_string(&mut response).expect("risposta letta fino alla chiusura");
    handler.join().expect("il servitore ha risposto senza andare in panico");

    assert!(response.starts_with("HTTP/1.1 200"), "risposta inattesa: {response}");
    let body_start = response.find("\r\n\r\n").expect("separatore fra intestazioni e corpo") + 4;
    let body: serde_json::Value = serde_json::from_str(&response[body_start..]).expect("corpo JSON valido");

    let flows_array = body["flows"].as_array().expect("array dei flussi");
    assert_eq!(flows_array.len(), 2);

    let rotto = flows_array.iter().find(|f| f["name"] == "rotto").expect("flusso rotto presente");
    assert_eq!(rotto["error"], "errore: ciclo nel grafo");
    assert_eq!(rotto["steps"].as_array().map(|s| s.len()), Some(0));

    let valido = flows_array.iter().find(|f| f["name"] == "valido").expect("flusso valido presente");
    assert_eq!(valido["error"], serde_json::Value::Null);
    assert_eq!(valido["description"], "Flusso valido di prova");
    assert_eq!(valido["steps"].as_array().map(|s| s.len()), Some(1));

    let _ = std::fs::remove_dir_all(&dir);
}
