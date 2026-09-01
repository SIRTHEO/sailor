//! `sailor profiles`: profili multipli per riga di comando, con o senza il
//! suo supporto nativo — quattro comandi (`list`/`create`/`switch`/`current`).
//! I gesti sul disco e sui collegamenti simbolici stanno nella libreria
//! `profiles` (modulo `store_io`); qui solo l'interpretazione degli
//! argomenti e la stampa. Prima del 27/08/2026 questo era il `main.rs` di un
//! binario a sé (`profiles`).

use profiles::{find_cli, profile_home_path, store_io, HomeMechanism, Profile};

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
    "sailor profiles create <cli> <nome>",
    "sailor profiles switch <cli> <nome>",
    "sailor profiles current <cli>",
];

fn usage() -> String {
    format!("uso:\n  {}", USAGE.join("\n  "))
}

fn cmd_list(args: &[String]) -> Result<(), String> {
    let store = store_io::load_store()?;
    let filter = args.first();
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
        println!(
            "{marker} {} {} -> {}",
            profile.cli_id,
            profile.name,
            profile.home_dir.display()
        );
    }
    Ok(())
}

fn cmd_create(args: &[String]) -> Result<(), String> {
    let [cli_id, name] = args else {
        return Err("uso: sailor profiles create <cli> <nome>".to_owned());
    };
    let cli = find_cli(cli_id)?;
    let home = profile_home_path(&store_io::profiles_root(), cli.id, name)
        .map_err(|e| format!("nome di profilo non valido: {e}"))?;
    std::fs::create_dir_all(&home).map_err(|e| format!("impossibile creare {}: {e}", home.display()))?;

    let mut store = store_io::load_store()?;
    let already_exists = store
        .profiles
        .iter()
        .any(|p| p.cli_id == cli.id && &p.name == name);
    if already_exists {
        return Err(format!("il profilo {name} esiste già per {}", cli.id));
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
        return Err("uso: sailor profiles switch <cli> <nome>".to_owned());
    };
    let cli = find_cli(cli_id)?;
    let mut store = store_io::load_store()?;
    let profile = store
        .profiles
        .iter()
        .find(|p| p.cli_id == cli.id && &p.name == name)
        .cloned()
        .ok_or_else(|| format!("profilo {name} non trovato per {}", cli.id))?;

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
        return Err("uso: sailor profiles current <cli>".to_owned());
    };
    let cli = find_cli(cli_id)?;
    let store = store_io::load_store()?;
    match store.active.get(cli.id) {
        Some(name) => println!("{name}"),
        None => println!("(nessun profilo attivo)"),
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
        assert_eq!(env.get("CODEX_HOME"), Some(&"/home/profiles/codex/lavoro".to_owned()));
    }

    #[test]
    fn no_subcommand_is_a_usage_error() {
        assert!(dispatch(&[]).is_err());
    }
}
