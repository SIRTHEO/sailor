//! `ui` — la pagina locale con cui una persona vede e capisce i flussi di
//! Sailor: cosa esiste, cosa è girato, cosa è aperto adesso. Diventerà
//! `sailor ui` quando il crate `sailor` esisterà nel workspace; per ora si
//! lancia da sé:
//!
//!     ui --port 47831 --ledger-dir ~/.claude/state/flussi

use std::net::TcpListener;
use std::path::PathBuf;
use std::sync::Arc;
use ui::gather::load_flow_registry;
use ui::server::{run_forever, ServerState};

/// Alta apposta: la mandato la vuole «predefinita alta», per non litigare
/// con le porte di sviluppo comuni (3000, 8000, 8080, ...).
const DEFAULT_PORT: u16 = 47831;

fn main() {
    let mut port = DEFAULT_PORT;
    let mut ledger_dir = default_ledger_dir();
    let mut flows_dir = None;
    let mut args = std::env::args().skip(1);
    while let Some(argument) = args.next() {
        match argument.as_str() {
            "--port" => {
                let value = expect_arg(&mut args, "--port");
                port = value.parse().unwrap_or_else(|_| {
                    eprintln!("ui: --port vuole un numero, ricevuto {value}");
                    std::process::exit(2);
                });
            }
            "--ledger-dir" => ledger_dir = PathBuf::from(expect_arg(&mut args, "--ledger-dir")),
            "--flows-dir" => flows_dir = Some(PathBuf::from(expect_arg(&mut args, "--flows-dir"))),
            "--help" | "-h" => return print_help(),
            other => {
                eprintln!("ui: opzione sconosciuta {other}");
                std::process::exit(2);
            }
        }
    }

    let flows_dir = flows_dir.unwrap_or_else(|| ledger_dir.join("flows"));
    let flows = load_flow_registry(&flows_dir);
    let state = Arc::new(ServerState { ledger_dir, flows });

    let listener = match TcpListener::bind(("127.0.0.1", port)) {
        Ok(listener) => listener,
        Err(error) => {
            eprintln!("ui: non riesco ad ascoltare su 127.0.0.1:{port}: {error}");
            std::process::exit(1);
        }
    };
    println!("ui: in ascolto su http://127.0.0.1:{port}");
    if let Err(error) = run_forever(listener, state) {
        eprintln!("ui: {error}");
        std::process::exit(1);
    }
}

fn expect_arg(args: &mut impl Iterator<Item = String>, flag: &str) -> String {
    args.next().unwrap_or_else(|| {
        eprintln!("ui: {flag} vuole un valore");
        std::process::exit(2);
    })
}

fn default_ledger_dir() -> PathBuf {
    let home = std::env::var("HOME").unwrap_or_else(|_| "/Users/theo".to_owned());
    PathBuf::from(home).join(".claude/state/flussi")
}

fn print_help() {
    println!(
        "uso: ui [--port N] [--ledger-dir CARTELLA] [--flows-dir CARTELLA]\n\n\
         --port N          porta locale su cui ascoltare (predefinita {DEFAULT_PORT})\n\
         --ledger-dir DIR  dove sta il deposito di flow/ledger (predefinito ~/.claude/state/flussi)\n\
         --flows-dir DIR   cartella con un file *.json per grafo (predefinita <ledger-dir>/flows)"
    );
}
