//! `sailor ui`: la pagina locale con cui una persona vede i flussi di
//! Sailor. Qui solo l'interpretazione degli argomenti e l'avvio del
//! servitore; i conti sui flussi vivono nella libreria `ui`. Prima del
//! 27/08/2026 questo era il `main.rs` di un binario a sé (`ui`).

use std::net::TcpListener;
use std::path::PathBuf;
use std::sync::Arc;
use ui::gather::{default_flows_dir, default_ledger_dir, load_flow_registry};
use ui::server::{run_forever, ServerState, DEFAULT_PORT};

/// Apre la pagina nel browser predefinito, dopo che la porta è già in ascolto.
/// Fallire qui non è un errore del comando: chi lavora via `ssh` o dentro un
/// contenitore non ha un browser da aprire, e la pagina resta raggiungibile
/// all'indirizzo appena stampato. Per questo l'esito si ignora di proposito.
fn open_in_browser(port: u16) {
    let _ = std::process::Command::new("open")
        .arg(format!("http://127.0.0.1:{port}"))
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status();
}

pub fn run(args: &[String]) -> i32 {
    let mut port = DEFAULT_PORT;
    let mut ledger_dir = default_ledger_dir();
    let mut flows_dir = None;
    let mut open = false;
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
            "--open" => open = true,
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

    // I FLUSSI NON STANNO ACCANTO AL DEPOSITO. Fino al 28/08/2026 qui c'era
    // `ledger_dir.join("flows")`, cioè `~/.claude/state/flussi/flows`: una
    // cartella mai esistita. La pagina rispondeva `"flows": []` senza errore, e
    // l'elenco vuoto sembrava un fatto sul mondo invece che una domanda posta
    // nel posto sbagliato. Il deposito è stato, i flussi sono sorgenti.
    let flows_dir = flows_dir.unwrap_or_else(default_flows_dir);
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
    // Dopo il bind, mai prima: aprire il browser su una porta che non ascolta
    // ancora mostrerebbe un errore di connessione al posto della pagina.
    if open {
        open_in_browser(port);
    }
    if let Err(error) = run_forever(listener, state) {
        eprintln!("sailor ui: {error}");
        return 1;
    }
    0
}

fn print_help() {
    println!(
        "uso: sailor ui [--open] [--port N] [--ledger-dir CARTELLA] [--flows-dir CARTELLA]\n\n\
         --open            apre la pagina nel browser appena la porta ascolta\n\
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

    /// `--open` prima di `--help`: si arriva allo 0 solo se la prima opzione è
    /// stata riconosciuta, perché un nome ignoto esce 2 senza proseguire. È il
    /// solo modo di provare che l'opzione esiste senza far partire il servitore,
    /// che non tornerebbe mai.
    #[test]
    fn the_open_flag_is_a_known_option() {
        assert_eq!(run(&a(&["--open", "--help"])), 0);
        assert_eq!(run(&a(&["--opon", "--help"])), 2);
    }
}
