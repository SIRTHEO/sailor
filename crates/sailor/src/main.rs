//! Il binario unico di sailor, il cui sottocomando si sceglie dal primo
//! argomento — stessa forma di `claude-hooks`, per lo stesso motivo: un
//! sottocomando per binario è un avvio di processo in più a ogni chiamata, e
//! qui l'elenco è chiuso. Niente `clap`.
//!
//! PERCHÉ QUESTO CRATE ESISTE. Il 27/08/2026 il workspace produceva cinque
//! eseguibili e tre non li invocava nessuno (`sweep`, e `release` come
//! binario a sé): nessun documento aveva mai deciso più di un binario di
//! sistema, e il piano `docs/plans/2026-08-22-sailor-il-sistema.md` parla
//! sempre di `sailor <verbo>`. Qui comincia quel binario.
//!
//! Uso:
//!     sailor release <bersaglio> [opzioni]   mette in servizio un binario da HEAD
//!     sailor version                          la versione di questo binario
//!     sailor --help                           l'elenco dei comandi

mod release_cmd;
mod version_cmd;

/// Ogni sottocomando: il nome sulla riga di comando e una riga di spiegazione.
/// Elenco a mano perché il dispatch è un `match`, e i due non possono
/// divergere in silenzio — il test `an_unknown_name_names_every_valid_command`
/// li tiene allineati passando da qui, non da un elenco copiato altrove.
const COMMANDS: &[(&str, &str)] = &[
    ("release", "mette in servizio un binario costruito da HEAD, mai dall'albero di lavoro"),
    ("version", "la versione di questo binario"),
];

fn print_usage() {
    println!("sailor <comando> [opzioni]");
    println!();
    println!("comandi disponibili:");
    for (name, description) in COMMANDS {
        println!("  {name:<10} {description}");
    }
}

/// Dove va un argv, senza toccare processi né disco: la domanda «il dispatch
/// raggiunge il comando giusto?» si prova su questo, non su `main`, che
/// chiamerebbe `std::process::exit` e chiuderebbe la batteria con lui.
#[derive(Debug, PartialEq, Eq)]
enum Route<'a> {
    Help,
    Known(&'static str),
    Unknown(&'a str),
}

fn route(args: &[String]) -> Route<'_> {
    match args.get(1).map(String::as_str) {
        None | Some("--help") | Some("-h") => Route::Help,
        Some(name) => match COMMANDS.iter().find(|(n, _)| *n == name) {
            Some((known, _)) => Route::Known(known),
            None => Route::Unknown(name),
        },
    }
}

/// Il messaggio di un nome sconosciuto, con l'elenco di quelli validi dentro:
/// è la parte che un test può leggere senza catturare `stderr`.
fn unknown_command_message(name: &str) -> String {
    format!(
        "sailor: comando sconosciuto '{name}'; comandi disponibili: {}",
        COMMANDS
            .iter()
            .map(|(n, _)| *n)
            .collect::<Vec<_>>()
            .join(", ")
    )
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    match route(&args) {
        Route::Help => {
            print_usage();
            std::process::exit(0);
        }
        Route::Known("release") => std::process::exit(release_cmd::run(&args[2..])),
        Route::Known("version") => std::process::exit(version_cmd::run(&args[2..])),
        Route::Known(other) => unreachable!("comando registrato senza un braccio: {other}"),
        Route::Unknown(other) => {
            eprintln!("{}", unknown_command_message(other));
            std::process::exit(64);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(words: &[&str]) -> Vec<String> {
        words.iter().map(|w| w.to_string()).collect()
    }

    #[test]
    fn no_arguments_prints_help() {
        assert_eq!(route(&args(&["sailor"])), Route::Help);
    }

    #[test]
    fn the_help_flag_prints_help() {
        assert_eq!(route(&args(&["sailor", "--help"])), Route::Help);
        assert_eq!(route(&args(&["sailor", "-h"])), Route::Help);
    }

    #[test]
    fn release_reaches_the_release_command() {
        assert_eq!(
            route(&args(&["sailor", "release", "notte", "--dry-run"])),
            Route::Known("release")
        );
    }

    #[test]
    fn version_reaches_the_version_command() {
        assert_eq!(route(&args(&["sailor", "version"])), Route::Known("version"));
    }

    #[test]
    fn an_unknown_name_is_reported_as_unknown() {
        assert_eq!(route(&args(&["sailor", "sweep"])), Route::Unknown("sweep"));
    }

    /// La prova che il mandato chiede esplicitamente: un nome ignoto porta con
    /// sé l'elenco di quelli validi, non solo il rifiuto.
    #[test]
    fn an_unknown_name_names_every_valid_command() {
        let message = unknown_command_message("sweep");
        assert!(message.contains("sconosciuto 'sweep'"), "{message}");
        for (name, _) in COMMANDS {
            assert!(message.contains(name), "{message} non nomina {name}");
        }
    }

    /// L'elenco stampato da `--help`/nessun argomento porta una riga per ogni
    /// comando, non un sottoinsieme: è l'unica interfaccia che avrà chi lo usa
    /// da terminale, quindi un nome dimenticato qui è invisibile a chi lo cerca.
    #[test]
    fn every_command_has_exactly_one_line_of_help() {
        for (name, description) in COMMANDS {
            assert!(!name.is_empty());
            assert!(!description.is_empty());
            assert!(!description.contains('\n'), "{name}: la descrizione va su una riga sola");
        }
    }
}
