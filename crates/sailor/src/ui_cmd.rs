//! `sailor ui`: la pagina locale con cui una persona vede i flussi di
//! Sailor. Qui solo l'interpretazione degli argomenti e l'avvio del
//! servitore; i conti sui flussi vivono nella libreria `ui`. Prima del
//! 27/08/2026 questo era il `main.rs` di un binario a sé (`ui`).

use std::net::TcpListener;
use std::path::PathBuf;
use std::sync::Arc;
use ui::gather::{default_ledger_dir, load_flow_registry};
use ui::server::{run_forever, ServerState, DEFAULT_PORT};

pub fn run(args: &[String]) -> i32 {
    let mut port = DEFAULT_PORT;
    let mut ledger_dir = default_ledger_dir();
    let mut flows_dir = None;
    let mut args = args.iter().cloned();
    while let Some(argument) = args.next() {
        match argument.as_str() {
            "--port" => {
                let Some(value) = args.next() else {
                    eprintln!("sailor ui: --port vuole un valore");
                    return 2;
                };
                match value.parse() {
                    Ok(parsed) => port = parsed,
                    Err(_) => {
                        eprintln!("sailor ui: --port vuole un numero, ricevuto {value}");
                        return 2;
                    }
                }
            }
            "--ledger-dir" => {
                let Some(value) = args.next() else {
                    eprintln!("sailor ui: --ledger-dir vuole un valore");
                    return 2;
                };
                ledger_dir = PathBuf::from(value);
            }
            "--flows-dir" => {
                let Some(value) = args.next() else {
                    eprintln!("sailor ui: --flows-dir vuole un valore");
                    return 2;
                };
                flows_dir = Some(PathBuf::from(value));
            }
            "--help" | "-h" => {
                print_help();
                return 0;
            }
            other => {
                eprintln!("sailor ui: opzione sconosciuta {other}");
                return 2;
            }
        }
    }

    let flows_dir = flows_dir.unwrap_or_else(|| ledger_dir.join("flows"));
    let flows = load_flow_registry(&flows_dir);
    let state = Arc::new(ServerState { ledger_dir, flows });

    let listener = match TcpListener::bind(("127.0.0.1", port)) {
        Ok(listener) => listener,
        Err(error) => {
            eprintln!("sailor ui: non riesco ad ascoltare su 127.0.0.1:{port}: {error}");
            return 1;
        }
    };
    println!("sailor ui: in ascolto su http://127.0.0.1:{port}");
    if let Err(error) = run_forever(listener, state) {
        eprintln!("sailor ui: {error}");
        return 1;
    }
    0
}

fn print_help() {
    println!(
        "uso: sailor ui [--port N] [--ledger-dir CARTELLA] [--flows-dir CARTELLA]\n\n\
         --port N          porta locale su cui ascoltare (predefinita {DEFAULT_PORT})\n\
         --ledger-dir DIR  dove sta il deposito di flow/ledger (predefinito ~/.claude/state/flussi)\n\
         --flows-dir DIR   cartella con un file *.json per grafo (predefinita <ledger-dir>/flows)"
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    fn a(words: &[&str]) -> Vec<String> {
        words.iter().map(|w| w.to_string()).collect()
    }

    #[test]
    fn an_unparsable_port_is_a_usage_error() {
        assert_eq!(run(&a(&["--port", "non-un-numero"])), 2);
    }

    #[test]
    fn an_unknown_option_is_a_usage_error() {
        assert_eq!(run(&a(&["--turbo"])), 2);
    }

    #[test]
    fn the_help_flag_exits_clean_without_binding_a_port() {
        assert_eq!(run(&a(&["--help"])), 0);
    }
}
