//! `sailor profiles`: profili multipli per riga di comando, con o senza il
//! suo supporto nativo — quattro comandi (`list`/`create`/`switch`/`current`).
//! I gesti sul disco e sui collegamenti simbolici stanno nella libreria
//! `profiles` (modulo `store_io`); qui solo l'interpretazione degli
//! argomenti e la stampa. Prima del 27/08/2026 questo era il `main.rs` di un
//! binario a sé (`profiles`).

use crate::Form;
use actions::{LoginProbe, LoginVerdict, ToolResolver};
use profiles::{find_cli, profile_home_path, store_io, HomeMechanism, KnownCli, Profile};
use std::path::Path;

pub fn run(args: &[String]) -> i32 {
    match dispatch(args) {
        Ok(()) => 0,
        Err(message) => {
            eprintln!("sailor profiles: {message}");
            1
        }
    }
}

fn dispatch(args: &[String]) -> Result<(), String> {
    match args {
        [cmd, rest @ ..] if cmd == "list" => cmd_list(rest),
        [cmd, rest @ ..] if cmd == "create" => cmd_create(rest),
        [cmd, rest @ ..] if cmd == "switch" => cmd_switch(rest),
        [cmd, rest @ ..] if cmd == "current" => cmd_current(rest),
        _ => Err(usage()),
    }
}

/// Le forme di `sailor profiles`, una per riga. Vedi `flow_cmd::USAGE`.
pub const USAGE: &[Form] = &[
    Form {
        form: "sailor profiles list [cli]",
        says_key: "",
    },
    Form {
        form: "sailor profiles create <cli> <name>",
        says_key: "",
    },
    Form {
        form: "sailor profiles switch <cli> <name>",
        says_key: "",
    },
    Form {
        form: "sailor profiles current <cli>",
        says_key: "",
    },
];

fn usage() -> String {
    format!(
        "{}\n  {}",
        catalogue::say("cli.usage_heading", &[]),
        crate::forms_as_lines(USAGE).join("\n  ")
    )
}

/// **UN ELENCO DI PROFILI CHE NON DICE SE SONO USABILI È UN ELENCO CHE
/// TRANQUILLIZZA.** Questo è il posto dove una persona guarda quando si chiede
/// «quale profilo uso», e fino al 01/09/2026 rispondeva col solo nome: i due
/// profili `codex` di questa macchina puntavano tutti e due a cartelle **senza
/// credenziali**, e da qui si vedevano identici a due case piene.
///
/// **LO STATO LO DICE IL MOTORE, NON IL DISCO**: si esegue la domanda che il suo
/// descrittore dichiara (`login_status`), dentro la casa di *quel* profilo — non
/// di quello attivo, che è l'unico modo per rispondere sui profili che non sono
/// in forza. Costa zero: `codex login status` e `claude auth status` leggono un
/// file locale, senza chiamare nessun modello.
fn cmd_list(args: &[String]) -> Result<(), String> {
    for row in overview(args.first().map(String::as_str))? {
        let marker = if row.active { "*" } else { " " };
        println!(
            "{marker} {} {} -> {} — access: {}",
            row.cli_id,
            row.name,
            row.home_dir.display(),
            row.said
        );
    }
    Ok(())
}

/// What is true of a profile's access, as a value and not only as a sentence.
///
/// **THE SENTENCE TRAVELS WITH IT AND IS NEVER PARSED BACK.** A surface that
/// had to read "authenticated" out of a translated line would break the day the
/// line is translated, and would break silently — it would read as «not
/// authenticated», the safe-looking answer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Access {
    Yes,
    No,
    /// Nobody could look, for any of several reasons — the sentence says which.
    /// **NOT A NO**: an absence and a refusal lead to different gestures.
    NotKnown,
    /// The profile exists and moves nothing: this command line has no known way
    /// to be sent elsewhere, so two profiles start it in the same place.
    HomeDoesNotMove,
}

/// One profile, as both surfaces show it.
#[derive(Debug, Clone)]
pub struct ProfileView {
    pub cli_id: String,
    pub name: String,
    pub home_dir: std::path::PathBuf,
    pub active: bool,
    pub access: Access,
    /// The engine's own words, already in the reader's language.
    pub said: String,
}

/// Every profile, with what the engine says about each one's home.
///
/// **ONE COPY FOR BOTH SURFACES.** The window asks the same question the
/// command line asks, and a second implementation of "which profile is usable"
/// is the shape of fault 10: the two would answer differently on the machine
/// where it matters, and neither would say so.
pub fn overview(filter: Option<&str>) -> Result<Vec<ProfileView>, String> {
    let store = store_io::load_store()?;
    let tools = toolbox::Tools::current();
    Ok(views_of(&store, &tools, &actions::RealDryProbe, filter))
}

/// The heart of [`overview`], with the store, the toolbox and the probe passed
/// in: a test measures profiles of its own, never the reader's real ones.
fn views_of(
    store: &profiles::ProfileStore,
    tools: &toolbox::Tools,
    probe: &dyn LoginProbe,
    filter: Option<&str>,
) -> Vec<ProfileView> {
    store
        .profiles
        .iter()
        .filter(|profile| filter.is_none_or(|wanted| profile.cli_id == wanted))
        .map(|profile| {
            // A command line this table does not know has no home to move, so
            // there is no question to ask it.
            let (access, said) = match find_cli(&profile.cli_id) {
                Ok(cli) => access_of(tools, probe, cli, &profile.home_dir),
                Err(reason) => (
                    Access::NotKnown,
                    catalogue::say(
                        "cli.profiles.access.not_known_because",
                        &[("reason", &reason)],
                    ),
                ),
            };
            ProfileView {
                cli_id: profile.cli_id.clone(),
                name: profile.name.clone(),
                home_dir: profile.home_dir.clone(),
                active: store
                    .active
                    .get(&profile.cli_id)
                    .is_some_and(|active_name| active_name == &profile.name),
                access,
                said,
            }
        })
        .collect()
}

/// What the engine says about **this** profile's home: the verdict to act on,
/// and the words to show.
///
/// **ANY OUTCOME THAT IS NOT A YES IS WRITTEN AS A NON-YES**, and the verdict
/// comes from the branch, never read back out of the sentence: «nobody looked»
/// and «not authenticated» are different facts, and neither is «you can use it».
fn access_of(
    tools: &toolbox::Tools,
    probe: &dyn LoginProbe,
    cli: &KnownCli,
    home: &Path,
) -> (Access, String) {
    let Some((tool, bin)) = tools.declared_as_executable(cli.executable) else {
        return (
            Access::NotKnown,
            catalogue::say(
                "cli.profiles.access.no_such_executable",
                &[("executable", cli.executable)],
            ),
        );
    };
    let Some(recipe) = tools.login_recipe(&tool) else {
        return (
            Access::NotKnown,
            catalogue::say("cli.profiles.access.no_recipe", &[("tool", &tool)]),
        );
    };
    // **LA CASA DI QUESTO PROFILO, NON QUELLA IN FORZA.** È tutta la ragione per
    // cui `LoginProbe` prende l'ambiente come argomento invece di andarselo a
    // leggere: chiedere sempre al profilo attivo darebbe la stessa risposta a
    // tutte le righe dell'elenco, e sarebbe la risposta di uno solo.
    let env = profiles::build_environment(cli, home);
    if env.is_empty() {
        return (
            Access::HomeDoesNotMove,
            catalogue::say(
                "cli.profiles.access.home_does_not_move",
                &[("id", cli.id), ("note", cli.home_note)],
            ),
        );
    }
    match actions::probe_login_status(probe, &bin, &env, &recipe) {
        LoginVerdict::LoggedIn { said } => (
            Access::Yes,
            catalogue::say(
                "cli.profiles.access.authenticated",
                &[("said", &one_line(&said))],
            ),
        ),
        LoginVerdict::LoggedOut { said } => (
            Access::No,
            catalogue::say(
                "cli.profiles.access.not_authenticated",
                &[("said", &one_line(&said))],
            ),
        ),
        LoginVerdict::NotDeclared => (
            Access::NotKnown,
            catalogue::say("cli.profiles.access.recipe_by_halves", &[("tool", &tool)]),
        ),
        LoginVerdict::Unrecognised { said } => (
            Access::NotKnown,
            catalogue::say(
                "cli.profiles.access.unrecognised",
                &[("said", &one_line(&said))],
            ),
        ),
        LoginVerdict::NoAnswer { why } => (
            Access::NotKnown,
            catalogue::say("cli.profiles.access.no_answer", &[("why", &why)]),
        ),
    }
}

/// Le parole del motore su una riga sola: un elenco si legge a colpo d'occhio, e
/// una risposta su tre righe lo spezza. Si stringono gli spazi, non si taglia
/// niente — la diagnosi è quella frase.
fn one_line(said: &str) -> String {
    said.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn cmd_create(args: &[String]) -> Result<(), String> {
    let [cli_id, name] = args else {
        return Err(catalogue::say("cli.profiles.usage_create", &[]));
    };
    create(cli_id, name)
}

/// Makes a profile: the directory, and the row that remembers it. Public
/// because the window offers the same gesture, and a second implementation
/// would be free to disagree about what a valid name is — which is the one
/// thing here that is a security rule and not a preference.
pub fn create(cli_id: &str, name: &String) -> Result<(), String> {
    let cli = find_cli(cli_id)?;
    let home = profile_home_path(&store_io::profiles_root(), cli.id, name)
        .map_err(|e| catalogue::say("cli.profiles.name_not_valid", &[("error", &e.to_string())]))?;
    std::fs::create_dir_all(&home).map_err(|e| {
        catalogue::say(
            "cli.profiles.cannot_create",
            &[
                ("path", &home.display().to_string()),
                ("error", &e.to_string()),
            ],
        )
    })?;

    let mut store = store_io::load_store()?;
    let already_exists = store
        .profiles
        .iter()
        .any(|p| p.cli_id == cli.id && &p.name == name);
    if already_exists {
        return Err(catalogue::say(
            "cli.profiles.already_exists",
            &[("name", name), ("id", cli.id)],
        ));
    }
    store.profiles.push(Profile {
        name: name.clone(),
        cli_id: cli.id.to_owned(),
        home_dir: home,
    });
    store_io::save_store(&store)
}

fn cmd_switch(args: &[String]) -> Result<(), String> {
    let [cli_id, name] = args else {
        return Err(catalogue::say("cli.profiles.usage_switch", &[]));
    };
    switch(cli_id, name)
}

/// Puts a profile in force. For the symlink mechanism this touches the disk;
/// for the environment one it only records, and `build_environment` reads it
/// when something is launched.
pub fn switch(cli_id: &str, name: &String) -> Result<(), String> {
    let cli = find_cli(cli_id)?;
    let mut store = store_io::load_store()?;
    let profile = store
        .profiles
        .iter()
        .find(|p| p.cli_id == cli.id && &p.name == name)
        .cloned()
        .ok_or_else(|| {
            catalogue::say("cli.profiles.not_found", &[("name", name), ("id", cli.id)])
        })?;

    if let HomeMechanism::CredentialSymlink { relative_path } = cli.home {
        store_io::apply_symlink_swap(&store_io::home_dir()?, relative_path, &profile.home_dir)?;
    }
    // Per il meccanismo a variabile d'ambiente non c'è nulla da spostare sul
    // filesystem: registrare l'attivo qui basta, `build_environment` legge
    // `profile.home_dir` al momento del lancio — vedi `sailor run`.
    store.active.insert(cli.id.to_owned(), name.clone());
    store_io::save_store(&store)
}

fn cmd_current(args: &[String]) -> Result<(), String> {
    let [cli_id] = args else {
        return Err(catalogue::say("cli.profiles.usage_current", &[]));
    };
    let cli = find_cli(cli_id)?;
    let store = store_io::load_store()?;
    match store.active.get(cli.id) {
        Some(name) => println!("{name}"),
        None => println!("{}", catalogue::say("cli.profiles.none_active", &[])),
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use profiles::build_environment;
    use std::path::PathBuf;

    /// Lo stesso fatto che prova `profiles::store_io`, visto dal lato di
    /// `sailor`: l'ambiente da sovrapporre usa la casa registrata per il
    /// profilo, non un valore fisso.
    #[test]
    fn build_environment_uses_the_profile_home_recorded_in_the_store() {
        let cli = find_cli("codex").unwrap();
        let home = PathBuf::from("/home/profiles/codex/lavoro");
        let env = build_environment(cli, &home);
        assert_eq!(
            env.get("CODEX_HOME"),
            Some(&"/home/profiles/codex/lavoro".to_owned())
        );
    }

    #[test]
    fn no_subcommand_is_a_usage_error() {
        assert!(dispatch(&[]).is_err());
    }

    /// Un finto `codex` che risponde sulla propria casa come quello vero: su
    /// stderr, e guardando se in casa c'è `auth.json`. Si chiama `codex` perché
    /// il legame fra la tabella dei profili e il descrittore è **l'eseguibile**.
    fn a_machine_with_a_fake_codex(declares_login: bool) -> (PathBuf, toolbox::Tools) {
        static SERIAL: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
        let serial = SERIAL.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        let dir =
            std::env::temp_dir().join(format!("prova-accesso-{}-{serial}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("la cartella di prova");
        let bin = dir.join("codex");
        std::fs::write(
            &bin,
            "#!/bin/sh\n\
             if [ -f \"$CODEX_HOME/auth.json\" ]; then\n\
             \x20 echo 'Logged in using ChatGPT' >&2; exit 0\n\
             fi\n\
             echo 'Not logged in' >&2; exit 1\n",
        )
        .expect("scrivere il finto motore");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&bin, std::fs::Permissions::from_mode(0o755))
                .expect("bit di esecuzione");
        }
        let login = if declares_login {
            r#","login_status":{"args":["login","status"],
               "logged_in_when":["logged in using"],
               "logged_out_when":["not logged in"]}"#
        } else {
            ""
        };
        let file = dir.join("tools.json");
        std::fs::write(
            &file,
            format!(
                r#"{{"tools":[{{"id":"codex","family":"ai_cli","label":"codex",
                   "detect":{{"command":"codex"}}{login}}}]}}"#
            ),
        )
        .expect("scrivere i descrittori");
        let tools = toolbox::Tools::new(
            toolbox::Catalog::load(&[toolbox::Source::File(file)]),
            toolbox::Machine {
                path_dirs: vec![dir.clone()],
                home: dir.clone(),
                env: std::collections::BTreeMap::new(),
                version_probes: false,
            },
        );
        (dir, tools)
    }

    /// **L'ELENCO DEI PROFILI DICE SE SONO USABILI, E LO CHIEDE AL MOTORE.**
    ///
    /// I due profili `codex` di questa macchina puntavano tutti e due a cartelle
    /// senza credenziali, e da qui si vedevano identici a due case piene: il
    /// posto dove una persona guarda per scegliere rispondeva col solo nome.
    ///
    /// **DUE BRACCI, PERCHÉ UNO SOLO NON POTREBBE VENIRE DIVERSO.** Un controllo
    /// che gridasse sempre passerebbe il primo; uno che tacesse sempre passerebbe
    /// il secondo. E la sonda è quella vera: una finta proverebbe che sappiamo
    /// scrivere una risposta, non che qualcuno la va a chiedere.
    ///
    /// *Mutante eseguito*: far rispondere «autenticato» anche al `LoggedOut` in
    /// `access_of` — il primo braccio diventa rosso.
    #[test]
    fn the_list_asks_the_engine_whether_each_home_has_credentials() {
        let (dir, tools) = a_machine_with_a_fake_codex(true);
        let cli = find_cli("codex").expect("codex sta nella tabella");
        let probe = actions::RealDryProbe;

        let empty = dir.join("casa-vuota");
        std::fs::create_dir_all(&empty).expect("la casa senza credenziali");
        let said = access_of(&tools, &probe, cli, &empty).1;
        assert!(
            said.contains("NOT AUTHENTICATED") && said.contains("Not logged in"),
            "una casa senza credenziali deve vedersi, con le parole del motore: {said}"
        );

        let full = dir.join("casa-piena");
        std::fs::create_dir_all(&full).expect("la casa autenticata");
        std::fs::write(full.join("auth.json"), "{}").expect("le credenziali");
        let said = access_of(&tools, &probe, cli, &full).1;
        assert!(
            said.starts_with("authenticated"),
            "a full home has to read as full: {said}"
        );
    }

    /// **A SURFACE HAS TO TELL THEM APART, NOT ONLY READ THEM.** The list on
    /// the command line makes do with the sentence; anything that colours or
    /// sorts needs the verdict as a value. *Mutant run*: return `Access::Yes`
    /// from the `LoggedOut` branch — the first arm goes red.
    #[test]
    fn two_homes_of_one_command_line_do_not_get_the_same_verdict() {
        let (dir, tools) = a_machine_with_a_fake_codex(true);
        let probe = actions::RealDryProbe;

        let empty = dir.join("senza-credenziali");
        let full = dir.join("con-credenziali");
        std::fs::create_dir_all(&empty).expect("la casa vuota");
        std::fs::create_dir_all(&full).expect("la casa piena");
        std::fs::write(full.join("auth.json"), "{}").expect("le credenziali");

        let store = profiles::ProfileStore {
            profiles: vec![
                Profile {
                    name: "vuoto".to_owned(),
                    cli_id: "codex".to_owned(),
                    home_dir: empty,
                },
                Profile {
                    name: "pieno".to_owned(),
                    cli_id: "codex".to_owned(),
                    home_dir: full,
                },
            ],
            active: [("codex".to_owned(), "pieno".to_owned())]
                .into_iter()
                .collect(),
        };

        let rows = views_of(&store, &tools, &probe, None);
        assert_eq!(rows.len(), 2, "both profiles have to reach the surface");
        assert_eq!(
            (rows[0].access, rows[1].access),
            (Access::No, Access::Yes),
            "the two homes differ and the verdicts have to differ with them: {} / {}",
            rows[0].said,
            rows[1].said,
        );
        assert_eq!(
            (rows[0].active, rows[1].active),
            (false, true),
            "the active one is the one the store names, not the first in the list",
        );
        // AND THE SENTENCE TRAVELS: a verdict with no words behind it cannot be
        // acted on — «no» that does not say why sends nobody anywhere.
        assert!(
            rows.iter().all(|row| !row.said.trim().is_empty()),
            "a verdict arrived with no words behind it",
        );

        // THE FILTER IS NOT DECORATION: asking for a command line nobody has a
        // profile for must answer nothing, never everything.
        assert!(
            views_of(&store, &tools, &probe, Some("claude")).is_empty(),
            "the filter let through profiles of another command line",
        );
    }

    /// **NESSUNO HA GUARDATO NON È «AUTENTICATO», E NEMMENO «NON
    /// AUTENTICATO».** Un motore il cui descrittore non dice come si chiede
    /// lascia la domanda senza risposta, e la riga dell'elenco deve dirlo: chi
    /// legge deve sapere se rimediare cambiando profilo o misurando il motore.
    #[test]
    fn a_home_nobody_can_ask_about_is_neither_authenticated_nor_broken() {
        let (dir, tools) = a_machine_with_a_fake_codex(false);
        let cli = find_cli("codex").expect("codex sta nella tabella");
        let probe = actions::RealDryProbe;
        let empty = dir.join("casa-vuota");
        std::fs::create_dir_all(&empty).expect("la casa senza credenziali");

        let said = access_of(&tools, &probe, cli, &empty).1;
        // **IL VERDETTO È LA TESTA DELLA RIGA, E SI GUARDA LÌ.** La spiegazione
        // che segue nomina per forza la parola «autenticato» — sta dicendo che
        // nessuno ha chiesto se lo è — quindi cercarla dentro tutta la riga
        // sarebbe un controllo che non può che essere rosso, cioè non un
        // controllo.
        assert!(
            said.starts_with("not known"),
            "an absence is declared, and declared first: {said}"
        );
        assert!(
            said.contains("nobody looked"),
            "and it says why, or the reader cannot tell whether to fix it by changing \
             profile or by measuring the engine: {said}"
        );
    }
}
