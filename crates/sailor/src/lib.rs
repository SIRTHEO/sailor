//! Il binario unico di sailor, il cui sottocomando si sceglie dal primo
//! argomento — stessa forma di `claude-hooks`, per lo stesso motivo: un
//! sottocomando per binario è un avvio di processo in più a ogni chiamata, e
//! qui l'elenco è chiuso. Niente `clap`.
//!
//! **PERCHÉ QUESTO È UNA LIBRERIA E NON SOLO UN BINARIO.** Dal 01/09/2026 la
//! finestra mostra i comandi di Sailor, e c'erano due modi per farlo: ricopiarli
//! in TypeScript, oppure leggerli da qui. Il primo è il guasto 10 — la stessa
//! verità in due posti — che in questo repo si è già ripresentato cinque volte,
//! l'ultima il giorno stesso, sul vocabolario delle azioni. Quindi
//! `crates/sailor` espone `COMMANDS`, `desktop/src-tauri` lo importa come già
//! importa `crates/registry`, e `main.rs` resta il guscio che chiama
//! `dispatch`. Nessuno ricopia niente, e una pagina d'aiuto che diverge dal
//! binario non è più esprimibile.
//!
//! PERCHÉ QUESTO CRATE ESISTE. Il 27/08/2026 il workspace produceva cinque
//! eseguibili e tre non li invocava nessuno (`sweep`, e `release` come
//! binario a sé): nessun documento aveva mai deciso più di un binario di
//! sistema, e il piano `docs/plans/2026-08-22-sailor-il-sistema.md` parla
//! sempre di `sailor <verbo>`. Qui comincia quel binario.
//!
//! **L'ELENCO DEI COMANDI NON È RICOPIATO QUI.** Stava in questo commento, e
//! il 01/09/2026 nominava ancora `sailor ui`, rimosso dodici commit prima:
//! un elenco in prosa accanto all'elenco vero invecchia da solo, e nessuno
//! legge un commento per aggiornarlo. Sta in `COMMANDS`, lo stampa
//! `print_usage`, e la finestra lo mostra leggendolo da lì.

pub mod faults_cmd;
pub mod flow_cmd;
pub mod inventory_cmd;
pub mod models_cmd;
pub mod profiles_cmd;
pub mod release_cmd;
pub mod remaining_cmd;
pub mod run_cmd;
pub mod session_cmd;
pub mod step_cmd;
pub mod terminal_cmd;
pub mod version_cmd;
pub mod workspace_cmd;
pub mod worktree_cmd;

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
/// **IL CAMPO `usage` OBBLIGA, ED È PER QUESTO CHE È UN CAMPO.** Fino al
/// 01/09/2026 la riga d'uso di ogni comando stava dentro il suo modulo, stampata
/// da una funzione privata, e non esisteva nessun modo per un programma di
/// chiederla: `sailor flow --help` finiva in `Err(usage())` e la finestra non
/// aveva niente da leggere. Un campo nuovo qui costringe **tutte** le voci a
/// riempirlo o il crate non compila — la stessa garanzia con cui il 31/08 il
/// corpo è entrato nella tabella e ha ucciso l'`unreachable!`.
///
/// Le righe sono un elenco e non una stringa sola perché chi le mostra decide
/// come impaginarle: il terminale le stampa una per riga, la finestra le
/// dispone in una tabella. Il testo è lo stesso; l'impaginazione no.
#[derive(Debug)]
pub struct Command {
    pub name: &'static str,
    pub description: &'static str,
    pub usage: &'static [&'static str],
    pub run: fn(&[String]) -> i32,
}

pub const COMMANDS: &[Command] = &[
    Command {
        name: "release",
        description: "puts into service a binary built from HEAD, never from the working tree",
        usage: release_cmd::USAGE,
        run: release_cmd::run,
    },
    Command {
        name: "profiles",
        description: "lists, creates and switches the profiles of a known command line",
        usage: profiles_cmd::USAGE,
        run: profiles_cmd::run,
    },
    Command {
        name: "models",
        description: "lists the model catalogue, shows or changes which one is in use",
        usage: models_cmd::USAGE,
        run: models_cmd::run,
    },
    Command {
        name: "flow",
        description: "lists, checks, runs or resumes the flows declared in flows/",
        usage: flow_cmd::USAGE,
        run: flow_cmd::run,
    },
    Command {
        name: "step",
        description: "takes charge of and closes a step a flow has handed over",
        usage: step_cmd::USAGE,
        run: step_cmd::run,
    },
    Command {
        name: "run",
        description: "starts a command line under its active profile, replacing this process",
        usage: run_cmd::USAGE,
        run: run_cmd::run,
    },
    Command {
        name: "inventory",
        description:
            "lists skills, agents, commands, rules and hooks, and says which are switched off",
        usage: inventory_cmd::USAGE,
        run: inventory_cmd::run,
    },
    Command {
        name: "remaining",
        description:
            "how much quota the person has already used, read from the engine rather than guessed",
        usage: remaining_cmd::USAGE,
        run: remaining_cmd::run,
    },
    Command {
        name: "version",
        description: "the version of this binary",
        usage: version_cmd::USAGE,
        run: version_cmd::run,
    },
    Command {
        name: "workspace",
        description: "declares the project root, so a flow does not have to know it",
        usage: workspace_cmd::USAGE,
        run: workspace_cmd::run,
    },
    Command {
        name: "worktree",
        description: "the trees this repository is checked out into",
        usage: worktree_cmd::USAGE,
        run: worktree_cmd::run,
    },
    Command {
        name: "faults",
        description:
            "faults met while building: what happened, and the check that would prevent it",
        usage: faults_cmd::USAGE,
        run: faults_cmd::run,
    },
    Command {
        name: "session",
        description: "tracks terminals: who checks in, what happens, and what is on the machine",
        usage: session_cmd::USAGE,
        run: session_cmd::run,
    },
    Command {
        name: "terminal",
        description: "runs a command line in a terminal Sailor owns, and can be typed into",
        usage: terminal_cmd::USAGE,
        run: terminal_cmd::run,
    },
];

/// L'aiuto come testo, perché una prova possa leggere ciò che legge chi digita
/// `sailor --help`. Stamparlo e basta lo renderebbe verificabile solo
/// catturando lo standard output, e una prova che non guarda le stesse parole
/// dell'utente sta provando un'altra cosa.
pub fn help_text() -> String {
    let mut text = String::from("sailor <command> [options]\n\navailable commands:\n");
    for command in COMMANDS {
        text.push_str(&format!("  {:<10} {}\n", command.name, command.description));
    }
    text
}

fn print_usage() {
    print!("{}", help_text());
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

/// Il codice d'uscita per un argv, senza uscire: `main` ci mette attorno
/// `std::process::exit` e nient'altro.
///
/// **STA QUI E NON IN `main` PERCHÉ UNA PROVA POSSA CHIAMARLO.** `main`
/// chiuderebbe la batteria con sé; questa funzione torna il numero e basta.
pub fn dispatch(args: &[String]) -> i32 {
    match route(args) {
        Route::Help => {
            print_usage();
            0
        }
        // Un braccio solo per tutti: il corpo arriva dalla tabella, quindi non
        // esiste più un nome che il dispatch non raggiunge.
        Route::Known(command) => (command.run)(&args[2..]),
        Route::Unknown(other) => {
            eprintln!("{}", unknown_command_message(other));
            64
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(words: &[&str]) -> Vec<String> {
        words.iter().map(|w| w.to_string()).collect()
    }

    /// **OGNI COMANDO DICE COME SI SCRIVE, E LA PRIMA PAROLA È IL SUO NOME.**
    ///
    /// Il campo `usage` è nuovo del 01/09/2026 e la sua garanzia è di
    /// compilazione — una voce senza non compila. Questa prova aggiunge ciò che
    /// il compilatore non può vedere: che le righe non siano vuote, e che
    /// parlino del comando a cui sono attaccate. Un copia-incolla fra due voci
    /// vicine è l'errore che ci si aspetta qui, ed è muto senza questa riga.
    #[test]
    fn every_command_says_how_it_is_written_and_names_itself() {
        for command in COMMANDS {
            assert!(
                !command.usage.is_empty(),
                "il comando '{}' non dice come si scrive",
                command.name
            );
            for line in command.usage {
                assert!(
                    line.starts_with(&format!("sailor {} ", command.name))
                        || *line == format!("sailor {}", command.name),
                    "la riga d'uso di '{}' parla di un altro comando: {line}",
                    command.name
                );
            }
        }
    }

    /// L'aiuto **letto** nomina ogni comando e ne dice il perché.
    ///
    /// Perché `help_text` esista invece di stampare direttamente: un
    /// `println!` non si può leggere da una prova senza catturare lo standard
    /// output, e una prova che non legge ciò che l'utente legge sta provando
    /// un'altra cosa. Qui si controlla il testo vero, quello che esce da
    /// `sailor --help`.
    #[test]
    fn the_help_text_names_every_command_and_says_what_it_does() {
        let help = help_text();
        for command in COMMANDS {
            assert!(
                help.contains(command.name),
                "l'aiuto non nomina '{}'",
                command.name
            );
            assert!(
                help.contains(command.description),
                "l'aiuto nomina '{}' senza dire cosa fa",
                command.name
            );
        }
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
        assert_eq!(
            route(&args(&["sailor", "version"])).reached(),
            Some("version")
        );
    }

    #[test]
    fn profiles_models_flow_and_run_reach_their_commands() {
        assert_eq!(
            route(&args(&["sailor", "profiles", "list"])).reached(),
            Some("profiles")
        );
        assert_eq!(
            route(&args(&["sailor", "models", "list"])).reached(),
            Some("models")
        );
        // `ui` NON C'E' PIU', e la riga che lo provava e' diventata questa:
        // dal 31/08/2026 l'unica interfaccia e' la finestra, e un comando che
        // apriva una seconda pagina su una porta da ricordare non esiste.
        assert!(route(&args(&["sailor", "ui"])).reached().is_none());
        assert_eq!(
            route(&args(&["sailor", "flow", "list"])).reached(),
            Some("flow")
        );
        assert_eq!(
            route(&args(&["sailor", "run", "codex"])).reached(),
            Some("run")
        );
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
        assert_eq!(
            route(&args(&["sailor", "step", "open"])).reached(),
            Some("step")
        );
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
