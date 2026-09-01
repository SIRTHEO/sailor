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
//!     sailor profiles <list|create|switch|current>  profili per riga di comando
//!     sailor models <list|current|set>       il catalogo dei modelli
//!     sailor ui [opzioni]                    la pagina locale dei flussi
//!     sailor flow <list|check|run|resume> [nome]  elenca, controlla, esegue o riprende
//!     sailor step <open|close> [opzioni]     prende in carico e chiude un passo consegnato
//!     sailor run <cli> [argomenti...]        lancia una CLI col profilo attivo
//!     sailor inventory [--kind K] [--json]   che cosa è installato, e cosa è spento
//!     sailor remaining                        quanta quota della persona è già consumata
//!     sailor version                          la versione di questo binario
//!     sailor --help                           l'elenco dei comandi

mod flow_cmd;
mod inventory_cmd;
mod models_cmd;
mod profiles_cmd;
mod release_cmd;
mod remaining_cmd;
mod run_cmd;
mod step_cmd;
mod version_cmd;
mod workspace_cmd;

/// Un sottocomando: il nome sulla riga di comando, una riga di spiegazione, e
/// **la funzione che lo esegue**.
///
/// **IL CORPO STA NELLA TABELLA, E PRIMA NO.** Fino al 31/08/2026 questa era una
/// coppia `(nome, descrizione)` e il dispatch era un `match` con un braccio per
/// nome, chiuso da `unreachable!("comando registrato senza un braccio")`.
/// Aggiungere un nome senza il suo braccio **compilava**, passava le prove, e
/// andava in panico solo quando qualcuno digitava quel comando: un difetto che
/// nessun controllo vedeva e che si scopriva in mano a chi lo usa. Con la
/// funzione dentro la tabella la divergenza non è più possibile — una voce senza
/// corpo non compila — e l'`unreachable!` è sparito insieme al buco.
#[derive(Debug)]
struct Command {
    name: &'static str,
    description: &'static str,
    run: fn(&[String]) -> i32,
}

const COMMANDS: &[Command] = &[
    Command {
        name: "release",
        description: "mette in servizio un binario costruito da HEAD, mai dall'albero di lavoro",
        run: release_cmd::run,
    },
    Command {
        name: "profiles",
        description: "elenca, crea e scambia i profili di una riga di comando conosciuta",
        run: profiles_cmd::run,
    },
    Command {
        name: "models",
        description: "elenca il catalogo dei modelli, mostra o cambia quale usare",
        run: models_cmd::run,
    },
    Command {
        name: "flow",
        description: "elenca, controlla, esegue o riprende i flussi dichiarati in flows/",
        run: flow_cmd::run,
    },
    Command {
        name: "step",
        description: "prende in carico e chiude un passo che un flusso ha consegnato",
        run: step_cmd::run,
    },
    Command {
        name: "run",
        description: "lancia una riga di comando col suo profilo attivo, sostituendo questo processo",
        run: run_cmd::run,
    },
    Command {
        name: "inventory",
        description: "elenca competenze, agenti, comandi, regole e ganci, e dice quali sono spenti",
        run: inventory_cmd::run,
    },
    Command {
        name: "remaining",
        description: "quanta quota ha già consumato la persona, letta dal motore invece che chiesta",
        run: remaining_cmd::run,
    },
    Command {
        name: "version",
        description: "la versione di questo binario",
        run: version_cmd::run,
    },
    Command {
        name: "workspace",
        description: "dichiara la radice del progetto, così un flusso non deve saperla",
        run: workspace_cmd::run,
    },
];

fn print_usage() {
    println!("sailor <comando> [opzioni]");
    println!();
    println!("comandi disponibili:");
    for command in COMMANDS {
        println!("  {:<10} {}", command.name, command.description);
    }
}

/// Dove va un argv, senza toccare processi né disco: la domanda «il dispatch
/// raggiunge il comando giusto?» si prova su questo, non su `main`, che
/// chiamerebbe `std::process::exit` e chiuderebbe la batteria con lui.
#[derive(Debug)]
enum Route<'a> {
    Help,
    Known(&'static Command),
    Unknown(&'a str),
}

/// **UN COMANDO SI CONFRONTA PER NOME, NON PER INDIRIZZO.** Il confronto
/// derivato guarderebbe anche il puntatore a funzione, e due puntatori alla
/// stessa funzione non sono garantiti uguali: `rustc` lo avverte, e un'uguaglianza
/// che a volte è falsa fra cose identiche renderebbe le prove del dispatch
/// intermittenti. Ciò che identifica un comando è il nome che si digita.
impl PartialEq for Route<'_> {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (Route::Help, Route::Help) => true,
            (Route::Known(left), Route::Known(right)) => left.name == right.name,
            (Route::Unknown(left), Route::Unknown(right)) => left == right,
            _ => false,
        }
    }
}

impl Eq for Route<'_> {}

impl Route<'_> {
    /// Il nome del comando raggiunto, per chi prova il dispatch senza dover
    /// costruire un `Command` intero.
    #[cfg(test)]
    fn reached(&self) -> Option<&'static str> {
        match self {
            Route::Known(command) => Some(command.name),
            Route::Help | Route::Unknown(_) => None,
        }
    }
}

fn route(args: &[String]) -> Route<'_> {
    match args.get(1).map(String::as_str) {
        None | Some("--help") | Some("-h") => Route::Help,
        Some(name) => match COMMANDS.iter().find(|command| command.name == name) {
            Some(known) => Route::Known(known),
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
            .map(|command| command.name)
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
        // Un braccio solo per tutti: il corpo arriva dalla tabella, quindi non
        // esiste più un nome che il dispatch non raggiunge.
        Route::Known(command) => std::process::exit((command.run)(&args[2..])),
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
            // Nominava `notte` fino al 01/09/2026: l'instradamento non guarda il
            // bersaglio, quindi la riga restava verde su un binario cancellato.
            route(&args(&["sailor", "release", "sailor", "--dry-run"])).reached(),
            Some("release")
        );
    }

    #[test]
    fn version_reaches_the_version_command() {
        assert_eq!(route(&args(&["sailor", "version"])).reached(), Some("version"));
    }

    #[test]
    fn profiles_models_flow_and_run_reach_their_commands() {
        assert_eq!(route(&args(&["sailor", "profiles", "list"])).reached(), Some("profiles"));
        assert_eq!(route(&args(&["sailor", "models", "list"])).reached(), Some("models"));
        // `ui` NON C'E' PIU', e la riga che lo provava e' diventata questa:
        // dal 31/08/2026 l'unica interfaccia e' la finestra, e un comando che
        // apriva una seconda pagina su una porta da ricordare non esiste.
        assert!(route(&args(&["sailor", "ui"])).reached().is_none());
        assert_eq!(route(&args(&["sailor", "flow", "list"])).reached(), Some("flow"));
        assert_eq!(route(&args(&["sailor", "run", "codex"])).reached(), Some("run"));
        assert_eq!(
            route(&args(&["sailor", "inventory", "--json"])).reached(),
            Some("inventory")
        );
    }

    /// **OGNI NOME DICHIARATO PORTA A UN CORPO.** Prima del 31/08/2026 questa
    /// non si poteva scrivere: il corpo stava in un `match` che una prova non
    /// può interrogare, e un nome senza braccio andava in panico solo a
    /// esecuzione. Adesso il corpo è nella tabella, quindi la domanda si può
    /// fare — e la risposta la garantisce già il compilatore, che rifiuta una
    /// voce senza `run`. Questa prova resta come dichiarazione: chi tornasse a
    /// un dispatch a bracci separati la vede diventare bugiarda e sa perché.
    #[test]
    fn every_declared_name_reaches_its_own_body() {
        for command in COMMANDS {
            assert_eq!(
                route(&args(&["sailor", command.name])).reached(),
                Some(command.name),
                "il nome {} non si ritrova nella tabella",
                command.name
            );
        }
    }

    /// Il passo consegnato ha il suo comando: senza, un mandato offerto non lo
    /// può prendere in carico nessuno.
    #[test]
    fn step_reaches_the_step_command() {
        assert_eq!(route(&args(&["sailor", "step", "open"])).reached(), Some("step"));
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
        for command in COMMANDS {
            assert!(
                message.contains(command.name),
                "{message} non nomina {}",
                command.name
            );
        }
    }

    /// L'elenco stampato da `--help`/nessun argomento porta una riga per ogni
    /// comando, non un sottoinsieme: è l'unica interfaccia che avrà chi lo usa
    /// da terminale, quindi un nome dimenticato qui è invisibile a chi lo cerca.
    #[test]
    fn every_command_has_exactly_one_line_of_help() {
        for command in COMMANDS {
            assert!(!command.name.is_empty());
            assert!(!command.description.is_empty());
            assert!(
                !command.description.contains('\n'),
                "{}: la descrizione va su una riga sola",
                command.name
            );
        }
    }
}
