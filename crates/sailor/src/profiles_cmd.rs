//! `sailor profiles`: profili multipli per riga di comando, con o senza il
//! suo supporto nativo — quattro comandi (`list`/`create`/`switch`/`current`).
//! I gesti sul disco e sui collegamenti simbolici stanno nella libreria
//! `profiles` (modulo `store_io`); qui solo l'interpretazione degli
//! argomenti e la stampa. Prima del 27/08/2026 questo era il `main.rs` di un
//! binario a sé (`profiles`).

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
pub const USAGE: &[&str] = &[
    "sailor profiles list [cli]",
    "sailor profiles create <cli> <name>",
    "sailor profiles switch <cli> <name>",
    "sailor profiles current <cli>",
];

fn usage() -> String {
    format!("usage:\n  {}", USAGE.join("\n  "))
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
    let store = store_io::load_store()?;
    let filter = args.first();
    let tools = toolbox::Tools::current();
    let probe = actions::RealDryProbe;
    for profile in &store.profiles {
        if let Some(f) = filter {
            if &profile.cli_id != f {
                continue;
            }
        }
        let is_active = store
            .active
            .get(&profile.cli_id)
            .is_some_and(|active_name| active_name == &profile.name);
        let marker = if is_active { "*" } else { " " };
        // Una riga di comando che questa tabella non conosce non ha una casa da
        // spostare, quindi non c'è nessuna domanda da farle.
        let access = match find_cli(&profile.cli_id) {
            Ok(cli) => access_state(&tools, &probe, cli, &profile.home_dir),
            Err(reason) => format!("not known ({reason})"),
        };
        println!(
            "{marker} {} {} -> {} — access: {access}",
            profile.cli_id,
            profile.name,
            profile.home_dir.display()
        );
    }
    Ok(())
}

/// Che cosa il motore dice della casa di **questo** profilo, in una riga.
///
/// **OGNI ESITO CHE NON SIA UN SÌ SI SCRIVE COME NON-SÌ.** «Nessuno ha
/// guardato», «ha risposto e non l'ho capito» e «non è autenticato» sono tre
/// fatti diversi e nessuno dei tre è «puoi usarlo»: si dicono per quello che
/// sono, mai riassunti in un silenzio che si legge come un via libera.
fn access_state(
    tools: &toolbox::Tools,
    probe: &dyn LoginProbe,
    cli: &KnownCli,
    home: &Path,
) -> String {
    let Some((tool, bin)) = tools.declared_as_executable(cli.executable) else {
        return format!(
            "not known: «{}» is not on this machine, or no descriptor declares it",
            cli.executable
        );
    };
    let Some(recipe) = tools.login_recipe(&tool) else {
        return format!(
            "not known: descriptor «{tool}» does not declare how to ask it whether it \
             is authenticated (`login_status`) — nobody looked"
        );
    };
    // **LA CASA DI QUESTO PROFILO, NON QUELLA IN FORZA.** È tutta la ragione per
    // cui `LoginProbe` prende l'ambiente come argomento invece di andarselo a
    // leggere: chiedere sempre al profilo attivo darebbe la stessa risposta a
    // tutte le righe dell'elenco, e sarebbe la risposta di uno solo.
    let env = profiles::build_environment(cli, home);
    if env.is_empty() {
        return format!(
            "not known: the home of «{}» does not move with a variable ({}), so no \
             home other than the one in force can be questioned here",
            cli.id, cli.home_note
        );
    }
    match actions::probe_login_status(probe, &bin, &env, &recipe) {
        LoginVerdict::LoggedIn { said } => format!("authenticated («{}»)", one_line(&said)),
        LoginVerdict::LoggedOut { said } => {
            format!("NOT AUTHENTICATED («{}»)", one_line(&said))
        }
        LoginVerdict::NotDeclared => {
            format!("not known: descriptor «{tool}» declares `login_status` by halves")
        }
        LoginVerdict::Unrecognised { said } => format!(
            "not known: it answered «{}», which resembles neither of the two declared \
             forms",
            one_line(&said)
        ),
        LoginVerdict::NoAnswer { why } => format!("not known: no answer — {why}"),
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
        return Err("usage: sailor profiles create <cli> <name>".to_owned());
    };
    let cli = find_cli(cli_id)?;
    let home = profile_home_path(&store_io::profiles_root(), cli.id, name)
        .map_err(|e| format!("not a valid profile name: {e}"))?;
    std::fs::create_dir_all(&home).map_err(|e| format!("cannot create {}: {e}", home.display()))?;

    let mut store = store_io::load_store()?;
    let already_exists = store
        .profiles
        .iter()
        .any(|p| p.cli_id == cli.id && &p.name == name);
    if already_exists {
        return Err(format!("profile {name} already exists for {}", cli.id));
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
        return Err("usage: sailor profiles switch <cli> <name>".to_owned());
    };
    let cli = find_cli(cli_id)?;
    let mut store = store_io::load_store()?;
    let profile = store
        .profiles
        .iter()
        .find(|p| p.cli_id == cli.id && &p.name == name)
        .cloned()
        .ok_or_else(|| format!("profile {name} not found for {}", cli.id))?;

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
        return Err("usage: sailor profiles current <cli>".to_owned());
    };
    let cli = find_cli(cli_id)?;
    let store = store_io::load_store()?;
    match store.active.get(cli.id) {
        Some(name) => println!("{name}"),
        None => println!("(no active profile)"),
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
    /// `access_state` — il primo braccio diventa rosso.
    #[test]
    fn the_list_asks_the_engine_whether_each_home_has_credentials() {
        let (dir, tools) = a_machine_with_a_fake_codex(true);
        let cli = find_cli("codex").expect("codex sta nella tabella");
        let probe = actions::RealDryProbe;

        let empty = dir.join("casa-vuota");
        std::fs::create_dir_all(&empty).expect("la casa senza credenziali");
        let said = access_state(&tools, &probe, cli, &empty);
        assert!(
            said.contains("NOT AUTHENTICATED") && said.contains("Not logged in"),
            "una casa senza credenziali deve vedersi, con le parole del motore: {said}"
        );

        let full = dir.join("casa-piena");
        std::fs::create_dir_all(&full).expect("la casa autenticata");
        std::fs::write(full.join("auth.json"), "{}").expect("le credenziali");
        let said = access_state(&tools, &probe, cli, &full);
        assert!(
            said.starts_with("authenticated"),
            "a full home has to read as full: {said}"
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

        let said = access_state(&tools, &probe, cli, &empty);
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
