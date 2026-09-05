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
pub mod publish_cmd;
pub mod ratchet_cmd;
pub mod release_cmd;
pub mod remember_cmd;
pub mod search_cmd;
pub mod remaining_cmd;
pub mod run_cmd;
pub mod session_cmd;
pub mod step_cmd;
pub mod terminal_cmd;
// Not a subcommand: the one piece of `session install` that knows a file
// format. It sits apart because it is pure text in and text out, and a graft
// that must leave a hand-written file alone is proved on the text.
pub mod toml_graft;
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
    /// **THE KEY, NOT THE SENTENCE.** A `const` cannot ask the catalogue, so
    /// what is written here is the name of the line and not the line: whoever
    /// shows it says it in the language of whoever is reading.
    pub description_key: &'static str,
    pub usage: &'static [Form],
    pub run: fn(&[String]) -> i32,
}

/// One way of writing a command: what is typed, and what it does.
///
/// **THE TWO HALVES ARE NOT THE SAME KIND OF TEXT, AND THAT IS THE POINT.**
/// Kept in one string, as they were until today, the sentence could not leave
/// for the catalogue and the shape could not stay put: the pair is what lets
/// each half do what it must.
#[derive(Debug)]
pub struct Form {
    /// What is typed: `sailor faults status <n> <text>`. The same in every
    /// language, because translating it would break the command it describes.
    pub form: &'static str,
    /// **THE KEY, NOT THE SENTENCE.** Prose about the form is a sentence like
    /// any other and belongs in the catalogue. Empty when the shape says it
    /// all and there is nothing to add — `sailor version` needs no gloss.
    pub says_key: &'static str,
}

impl Form {
    /// The form and its sentence, in the language of whoever is reading, padded
    /// so that a list of them lines up. `width` is the widest form in the list.
    ///
    /// **THE COLUMN IS MEASURED, NEVER TYPED.** The alignment used to be spaces
    /// inside the literal, counted by hand and right in exactly one language.
    pub fn line(&self, width: usize) -> String {
        if self.says_key.is_empty() {
            return self.form.to_owned();
        }
        format!(
            "{:width$}   {}",
            self.form,
            catalogue::say(self.says_key, &[]),
            width = width
        )
    }
}

/// The width to pad every form to, so a list of them lines up. A list with no
/// sentence in it needs no column at all, and asking for one would leave a
/// trailing hedge of spaces on every row.
pub fn form_width(forms: &[Form]) -> usize {
    if forms.iter().all(|form| form.says_key.is_empty()) {
        return 0;
    }
    forms.iter().map(|form| form.form.len()).max().unwrap_or(0)
}

/// Every form of a command, one per line, each already saying what it does.
pub fn forms_as_lines(forms: &[Form]) -> Vec<String> {
    let width = form_width(forms);
    forms.iter().map(|form| form.line(width)).collect()
}

pub const COMMANDS: &[Command] = &[
    Command {
        name: "release",
        description_key: "cli.command.release",
        usage: release_cmd::USAGE,
        run: release_cmd::run,
    },
    Command {
        name: "remember",
        description_key: "cli.command.remember",
        usage: remember_cmd::USAGE,
        run: remember_cmd::run,
    },
    Command {
        name: "search",
        description_key: "cli.command.search",
        usage: search_cmd::USAGE,
        run: search_cmd::run,
    },
    Command {
        name: "ratchet",
        description_key: "cli.command.ratchet",
        usage: ratchet_cmd::USAGE,
        run: ratchet_cmd::run,
    },
    Command {
        name: "profiles",
        description_key: "cli.command.profiles",
        usage: profiles_cmd::USAGE,
        run: profiles_cmd::run,
    },
    Command {
        name: "models",
        description_key: "cli.command.models",
        usage: models_cmd::USAGE,
        run: models_cmd::run,
    },
    Command {
        name: "flow",
        description_key: "cli.command.flow",
        usage: flow_cmd::USAGE,
        run: flow_cmd::run,
    },
    Command {
        name: "step",
        description_key: "cli.command.step",
        usage: step_cmd::USAGE,
        run: step_cmd::run,
    },
    Command {
        name: "run",
        description_key: "cli.command.run",
        usage: run_cmd::USAGE,
        run: run_cmd::run,
    },
    Command {
        name: "inventory",
        description_key: "cli.command.inventory",
        usage: inventory_cmd::USAGE,
        run: inventory_cmd::run,
    },
    Command {
        name: "remaining",
        description_key: "cli.command.remaining",
        usage: remaining_cmd::USAGE,
        run: remaining_cmd::run,
    },
    Command {
        name: "version",
        description_key: "cli.command.version",
        usage: version_cmd::USAGE,
        run: version_cmd::run,
    },
    Command {
        name: "workspace",
        description_key: "cli.command.workspace",
        usage: workspace_cmd::USAGE,
        run: workspace_cmd::run,
    },
    Command {
        name: "worktree",
        description_key: "cli.command.worktree",
        usage: worktree_cmd::USAGE,
        run: worktree_cmd::run,
    },
    Command {
        name: "faults",
        description_key: "cli.command.faults",
        usage: faults_cmd::USAGE,
        run: faults_cmd::run,
    },
    Command {
        name: "session",
        description_key: "cli.command.session",
        usage: session_cmd::USAGE,
        run: session_cmd::run,
    },
    Command {
        name: "terminal",
        description_key: "cli.command.terminal",
        usage: terminal_cmd::USAGE,
        run: terminal_cmd::run,
    },
];

/// L'aiuto come testo, perché una prova possa leggere ciò che legge chi digita
/// `sailor --help`. Stamparlo e basta lo renderebbe verificabile solo
/// catturando lo standard output, e una prova che non guarda le stesse parole
/// dell'utente sta provando un'altra cosa.
pub fn help_text() -> String {
    let mut text = catalogue::say("cli.help.heading", &[]);
    text.push('\n');
    for command in COMMANDS {
        text.push_str(&format!(
            "  {:<10} {}\n",
            command.name,
            catalogue::say(command.description_key, &[])
        ));
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
                    line.form.starts_with(&format!("sailor {} ", command.name))
                        || line.form == format!("sailor {}", command.name),
                    "la riga d'uso di '{}' parla di un altro comando: {}",
                    command.name,
                    line.form
                );
            }
        }
    }

    /// **WHAT A FORM SAYS IS IN THE CATALOGUE, OR IT IS NOWHERE.** A wrong key
    /// is not a compile error: `catalogue::say` hands back the key itself, and
    /// the help prints `cli.faults.form.lst` to whoever reads it.
    #[test]
    fn every_form_that_says_something_says_it_from_the_catalogue() {
        for (language, _) in catalogue::LANGUAGES {
            let entries = catalogue::entries(language).expect("un catalogo che si legge");
            for command in COMMANDS {
                for form in command.usage {
                    assert!(
                        form.says_key.is_empty() || entries.contains_key(form.says_key),
                        "«{}» non è nel catalogo {language}, e la forma «{}» la mostrerebbe così com'è",
                        form.says_key,
                        form.form
                    );
                }
            }
        }
    }

    /// The column is measured: the widest form decides it, and a form with no
    /// sentence carries no trailing hedge of spaces.
    #[test]
    fn the_column_is_measured_and_a_bare_form_carries_no_padding() {
        let with_prose = &[
            Form {
                form: "sailor x aa",
                says_key: "cli.usage_heading",
            },
            Form {
                form: "sailor x b",
                says_key: "",
            },
        ];
        let lines = forms_as_lines(with_prose);
        assert!(
            lines[0].starts_with("sailor x aa   "),
            "la prima forma non è seguita dalla sua frase: {}",
            lines[0]
        );
        assert_eq!(
            lines[1], "sailor x b",
            "una forma senza frase non si impagina"
        );

        let bare = &[Form {
            form: "sailor x b",
            says_key: "",
        }];
        assert_eq!(
            form_width(bare),
            0,
            "un elenco senza frasi non vuole colonna"
        );
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
                help.contains(&catalogue::say(command.description_key, &[])),
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
            // Through the catalogue: a key that answers with itself is a
            // missing line, and this is where that shows.
            let description = catalogue::say(command.description_key, &[]);
            assert_ne!(
                description, command.description_key,
                "{}: «{}» is not declared in the catalogue",
                command.name, command.description_key
            );
            assert!(
                !description.contains('\n'),
                "{}: la descrizione va su una riga sola",
                command.name
            );
        }
    }
}
