//! Sailor's one binary, whose subcommand is chosen from the first argument —
//! the same shape as `claude-hooks`, for the same reason: one subcommand per
//! binary is an extra process start on every call, and here the list is closed.
//! No `clap`.
//!
//! **WHY THIS IS A LIBRARY AND NOT ONLY A BINARY.** The window shows Sailor's
//! commands, and there were two ways to do it: copy them into TypeScript, or
//! read them from here. The first is fault 10 — the same truth in two places —
//! which has come back five times in this repo, the last on the vocabulary of
//! actions. So `crates/sailor` exposes `COMMANDS`, `desktop/src-tauri` imports
//! it as it already imports `crates/registry`, and `main.rs` stays the shell
//! calling `dispatch`. Nobody copies anything, and a help page diverging from
//! the binary is no longer expressible.
//!
//! WHY THIS CRATE EXISTS. The workspace produced five executables and three
//! were invoked by nobody (`sweep`, and `release` as a binary of its own): no
//! document had ever decided on more than one system binary, and the plan for
//! Sailor as a system speaks throughout of `sailor <verb>`. That binary starts
//! here.
//!
//! **THE LIST OF COMMANDS IS NOT COPIED HERE.** It used to be in this comment,
//! where it still named `sailor ui` twelve commits after its removal: a prose
//! list beside the real list ages on its own, and nobody reads a comment to
//! update it. It lives in `COMMANDS`, `print_usage` prints it, and the window
//! shows it by reading it from there.

pub mod faults_cmd;
pub mod flow_cmd;
pub mod inventory_cmd;
pub mod memory_cmd;
pub mod models_cmd;
pub mod notes_cmd;
pub mod profiles_cmd;
pub mod publish_cmd;
pub mod ratchet_cmd;
pub mod release_cmd;
pub mod remember_cmd;
pub mod repeats_cmd;
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

/// A subcommand: the name on the command line, a line of explanation, and
/// **the function that runs it**.
///
/// **THE BODY LIVES IN THE TABLE.** It was a `(name, description)` pair with a
/// `match` arm per name closed by an `unreachable!`: a name without its arm
/// **compiled**, passed the tests, and panicked only when somebody typed it.
/// **AND `usage` IS A FIELD BECAUSE A FIELD COMPELS**: printed by a private
/// function per module, no program could ask for it — `sailor flow --help` gave
/// `Err(usage())`. A new field forces every entry, or the crate does not build.
///
/// The lines are a list and not one string because whoever shows them decides
/// the layout: the terminal prints one per line, the window lays them out in a
/// table. The text is the same; the layout is not.
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

/// The verb of each form: the word after the command's own name. The verbs a
/// dispatch accepts are read off the forms it prints, never listed a second
/// time beside them — see fault 10.
pub fn verbs_of(forms: &[Form]) -> Vec<&'static str> {
    forms
        .iter()
        .filter_map(|form| form.form.split_whitespace().nth(2))
        .collect()
}

/// Whether `verb` is one the forms promise.
pub fn is_a_form(forms: &[Form], verb: &str) -> bool {
    verbs_of(forms).contains(&verb)
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
        name: "memory",
        description_key: "cli.command.memory",
        usage: memory_cmd::USAGE,
        run: memory_cmd::run,
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
        name: "repeats",
        description_key: "cli.command.repeats",
        usage: repeats_cmd::USAGE,
        run: repeats_cmd::run,
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
        name: "notes",
        description_key: "cli.command.notes",
        usage: notes_cmd::USAGE,
        run: notes_cmd::run,
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

/// The help as text, so a test can read what whoever types `sailor --help`
/// reads. Merely printing it would make it checkable only by capturing standard
/// output, and a test that does not look at the user's own words is testing
/// something else.
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

/// Where an argv goes, touching neither processes nor disk: the question «does
/// the dispatch reach the right command?» is tested on this, not on `main`,
/// which would call `std::process::exit` and close the suite with it.
#[derive(Debug)]
enum Route<'a> {
    Help,
    Known(&'static Command),
    Unknown(&'a str),
}

/// **A COMMAND IS COMPARED BY NAME, NOT BY ADDRESS.** A derived comparison
/// would look at the function pointer too, and two pointers to the same
/// function are not guaranteed equal: `rustc` warns of it, and an equality
/// sometimes false between identical things would make the dispatch tests
/// flaky. What identifies a command is the name that gets typed.
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
    /// The name of the command reached, for testing the dispatch without
    /// building a whole `Command`.
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

/// The message for an unknown name, with the list of valid ones inside: the
/// part a test can read without capturing `stderr`.
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

/// The exit code for an argv, without exiting: `main` wraps
/// `std::process::exit` around it and nothing else.
///
/// **IT IS HERE AND NOT IN `main` SO A TEST CAN CALL IT.** `main` would close
/// the suite with itself; this function returns the number and stops.
pub fn dispatch(args: &[String]) -> i32 {
    match route(args) {
        Route::Help => {
            print_usage();
            0
        }
        // One arm for all: the body arrives from the table, so a name the
        // dispatch does not reach no longer exists.
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

    /// The verbs a dispatch accepts come off the forms it prints, so the two
    /// cannot part: a form added to the help is accepted, a form removed from
    /// the help is refused, and nothing else is a form.
    #[test]
    fn the_verbs_are_read_off_the_forms_and_are_the_third_word() {
        const FORMS: &[Form] = &[
            Form {
                form: "sailor thing list [--open]",
                says_key: "",
            },
            Form {
                form: "sailor thing add < file.json",
                says_key: "",
            },
        ];
        assert_eq!(verbs_of(FORMS), vec!["list", "add"]);
        assert!(is_a_form(FORMS, "list"));
        assert!(is_a_form(FORMS, "add"));
        assert!(!is_a_form(FORMS, "thing"), "the command's own name is not a verb");
        assert!(!is_a_form(FORMS, "sailor"));
        assert!(!is_a_form(FORMS, "[--open]"), "an option is not a verb");
        assert!(!is_a_form(FORMS, "sweep"));
    }

    /// The commands that dispatch by verb have one per form and no form
    /// without: a usage line missing its verb would silently shrink what is
    /// accepted, since the accepted verbs are read off the usage.
    #[test]
    fn every_shipped_form_carries_a_verb_where_the_command_has_verbs() {
        for (name, usage) in [
            ("faults", faults_cmd::USAGE),
            ("terminal", terminal_cmd::USAGE),
            ("session", session_cmd::USAGE),
        ] {
            let verbs = verbs_of(usage);
            assert_eq!(
                verbs.len(),
                usage.len(),
                "«sailor {name}»: a form without a third word"
            );
            for verb in &verbs {
                assert!(
                    !verb.starts_with('-') && !verb.starts_with('<') && !verb.starts_with('['),
                    "«sailor {name} {verb}» is not a verb but an argument"
                );
            }
        }
    }

    /// **EVERY COMMAND SAYS HOW IT IS WRITTEN, AND THE FIRST WORD IS ITS NAME.**
    ///
    /// The `usage` field's guarantee is a compile-time one — an entry lacking
    /// it does not compile. This test adds what the compiler cannot see: that
    /// the lines are not empty, and that they speak of the command they hang
    /// on. A copy-paste between two neighbouring entries is the expected error
    /// here, and it is mute without this line.
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

    /// The help **as read** names every command and says why it is there.
    ///
    /// Why `help_text` exists instead of printing straight out: a `println!`
    /// cannot be read by a test without capturing standard output, and a test
    /// that does not read what the user reads is testing something else. What
    /// is checked here is the real text, the one `sailor --help` emits.
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
            // Routing does not look at the target, so this line stayed green
            // on a binary that had been deleted.
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
        // `ui` IS GONE, and the line that tested it became this one: the only
        // interface is the window, and a command opening a second page on a
        // port to be remembered does not exist.
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

    /// **EVERY DECLARED NAME LEADS TO A BODY.** This could not be written while
    /// the body sat in a `match` no test can interrogate, and a name without an
    /// arm panicked only at run time. With the body in the table the question
    /// can be asked — and the compiler already guarantees the answer, refusing
    /// an entry with no `run`. The test stays as a declaration: whoever went
    /// back to separate arms sees it turn false, and knows why.
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

    /// The handed step has its own command: without it, nobody can take on an
    /// offered mandate.
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

    /// The test the mandate asks for explicitly: an unknown name carries the
    /// list of valid ones with it, not the refusal alone.
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

    /// The list printed by `--help`/no arguments carries a line per command,
    /// not a subset: it is the only interface a terminal user gets, so a name
    /// forgotten here is invisible to whoever looks for it.
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
