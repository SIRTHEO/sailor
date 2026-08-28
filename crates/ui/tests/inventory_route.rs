//! Prova end-to-end: la rotta `/api/inventory` sul vero servitore HTTP,
//! su un socket vero — sulla falsariga di `dashboard_with_a_real_ledger.rs`.
//!
//! Le radici sono quelle vere di questa macchina (`$HOME`), non un deposito
//! finto: l'inventario non ha un punto di iniezione, e verificarne la forma
//! non richiede contarne il contenuto esatto.

use std::io::{Read, Write};
use std::net::TcpListener;
use std::path::{Path, PathBuf};

fn temp_dir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("ui-crate-test-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    dir
}

fn get(path: &Path, target: &str) -> serde_json::Value {
    let state = std::sync::Arc::new(ui::server::ServerState {
        ledger_dir: path.to_path_buf(),
        flows: Default::default(),
    });

    let listener = TcpListener::bind("127.0.0.1:0").expect("bind su una porta libera");
    let addr = listener.local_addr().expect("indirizzo assegnato dal sistema");
    let acceptor = listener.try_clone().expect("duplicare il listener");
    let accepted = std::thread::spawn(move || acceptor.accept().expect("connessione in arrivo"));

    let mut client = std::net::TcpStream::connect(addr).expect("connessione al servitore");
    client
        .write_all(format!("GET {target} HTTP/1.1\r\nHost: localhost\r\n\r\n").as_bytes())
        .expect("richiesta inviata");

    let (connection, _) = accepted.join().expect("il thread di ascolto non è andato in panico");
    let handler =
        std::thread::spawn(move || ui::server::handle_connection(connection, &state).expect("risposta scritta"));

    let mut response = String::new();
    client.read_to_string(&mut response).expect("risposta letta fino alla chiusura");
    handler.join().expect("il servitore ha risposto senza andare in panico");

    assert!(response.starts_with("HTTP/1.1 200"), "risposta inattesa: {response}");
    let body_start = response.find("\r\n\r\n").expect("separatore fra intestazioni e corpo") + 4;
    serde_json::from_str(&response[body_start..]).expect("corpo JSON valido")
}

#[test]
fn the_inventory_route_answers_with_the_shape_the_page_expects() {
    let dir = temp_dir("inventory");
    let body = get(&dir, "/api/inventory");

    let entries = body["entries"].as_array().expect("array di voci");
    let roots = body["roots"].as_array().expect("array di radici");
    assert!(!roots.is_empty(), "la casa è sempre una radice");
    let stale = body["stale_plugin_copies"].as_u64().expect("numero di copie in cache");

    // Sulla macchina di prova esistono davvero competenze e ganci: se
    // l'elenco fosse vuoto la rotta risponderebbe con la forma giusta ma un
    // contenuto sbagliato, e questo controllo lo prenderebbe.
    assert!(!entries.is_empty(), "ci si aspetta almeno una voce sulla macchina di prova");

    let known_kinds = ["skill", "agent", "command", "rule", "hook"];
    for entry in entries {
        let kind = entry["kind"].as_str().expect("ogni voce dichiara un genere");
        assert!(known_kinds.contains(&kind), "genere inatteso: {kind}");
        assert!(entry["name"].as_str().is_some(), "ogni voce ha un nome");
        assert!(entry["origin"].as_str().is_some(), "ogni voce dichiara l'origine");
        let state = entry["reach"]["state"].as_str().expect("ogni voce dichiara reach.state");
        assert!(
            ["active", "inactive", "unknown"].contains(&state),
            "stato di raggiungibilità inatteso: {state}"
        );
        if state != "active" {
            assert!(
                entry["reach"]["reason"].as_str().is_some(),
                "chi non è attivo porta il motivo scritto"
            );
        }
    }

    // stale_plugin_copies è un conteggio, non parte dell'elenco: non deve
    // superare in modo assurdo il numero di voci trovate — è solo una difesa
    // contro un errore grossolano di lettura, non un valore atteso preciso.
    assert!(stale < 1_000_000, "numero di copie in cache implausibile: {stale}");

    let _ = std::fs::remove_dir_all(&dir);
}
