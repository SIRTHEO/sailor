//! La parte impura: stato su disco, cartelle dei profili, e lo scambio —
//! per collegamento simbolico dove serve — fra i profili di una riga di
//! comando conosciuta. Quattro comandi: `list`, `create`, `switch`,
//! `current`. Percorsi da ambiente, mai cablati: `PROFILES_STATE_PATH`
//! (default `~/.claude/state/profili.json`) e `PROFILES_HOME_ROOT`
//! (default accanto allo stato, in `profiles-homes/`).

use profiles::{
    build_environment, known_clis, parse_store, profile_home_path, serialize_store,
    symlink_swap, HomeMechanism, KnownCli, Profile, ProfileStore,
};
use std::env;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

fn main() -> ExitCode {
    let args: Vec<String> = env::args().skip(1).collect();
    match run(&args) {
        Ok(()) => ExitCode::SUCCESS,
        Err(message) => {
            eprintln!("{message}");
            ExitCode::FAILURE
        }
    }
}

fn run(args: &[String]) -> Result<(), String> {
    match args {
        [cmd, rest @ ..] if cmd == "list" => cmd_list(rest),
        [cmd, rest @ ..] if cmd == "create" => cmd_create(rest),
        [cmd, rest @ ..] if cmd == "switch" => cmd_switch(rest),
        [cmd, rest @ ..] if cmd == "current" => cmd_current(rest),
        _ => Err(usage()),
    }
}

fn usage() -> String {
    "uso: profiles <list [cli]|create <cli> <nome>|switch <cli> <nome>|current <cli>>".to_owned()
}

fn find_cli(id: &str) -> Result<&'static KnownCli, String> {
    known_clis()
        .iter()
        .find(|c| c.id == id)
        .ok_or_else(|| format!("riga di comando sconosciuta: {id}"))
}

fn home_dir() -> Result<PathBuf, String> {
    env::var_os("HOME")
        .map(PathBuf::from)
        .ok_or_else(|| "HOME non impostata".to_owned())
}

fn default_state_path() -> PathBuf {
    home_dir()
        .unwrap_or_else(|_| PathBuf::from("."))
        .join(".claude")
        .join("state")
        .join("profili.json")
}

fn state_path() -> PathBuf {
    env::var_os("PROFILES_STATE_PATH")
        .map(PathBuf::from)
        .unwrap_or_else(default_state_path)
}

fn profiles_root() -> PathBuf {
    env::var_os("PROFILES_HOME_ROOT")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            state_path()
                .parent()
                .map(|p| p.join("profiles-homes"))
                .unwrap_or_else(|| PathBuf::from("profiles-homes"))
        })
}

fn load_store_from(path: &Path) -> Result<ProfileStore, String> {
    match fs::read_to_string(path) {
        Ok(content) => {
            parse_store(&content).map_err(|e| format!("stato illeggibile in {}: {e}", path.display()))
        }
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(ProfileStore::default()),
        Err(e) => Err(format!("impossibile leggere {}: {e}", path.display())),
    }
}

fn save_store_to(path: &Path, store: &ProfileStore) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .map_err(|e| format!("impossibile creare {}: {e}", parent.display()))?;
    }
    let json = serialize_store(store).map_err(|e| format!("serializzazione fallita: {e}"))?;
    fs::write(path, json).map_err(|e| format!("impossibile scrivere {}: {e}", path.display()))
}

fn load_store() -> Result<ProfileStore, String> {
    load_store_from(&state_path())
}

fn save_store(store: &ProfileStore) -> Result<(), String> {
    save_store_to(&state_path(), store)
}

fn cmd_list(args: &[String]) -> Result<(), String> {
    let store = load_store()?;
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
        return Err("uso: profiles create <cli> <nome>".to_owned());
    };
    let cli = find_cli(cli_id)?;
    let home = profile_home_path(&profiles_root(), cli.id, name)
        .map_err(|e| format!("nome di profilo non valido: {e}"))?;
    fs::create_dir_all(&home).map_err(|e| format!("impossibile creare {}: {e}", home.display()))?;

    let mut store = load_store()?;
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
    save_store(&store)
}

fn cmd_switch(args: &[String]) -> Result<(), String> {
    let [cli_id, name] = args else {
        return Err("uso: profiles switch <cli> <nome>".to_owned());
    };
    let cli = find_cli(cli_id)?;
    let mut store = load_store()?;
    let profile = store
        .profiles
        .iter()
        .find(|p| p.cli_id == cli.id && &p.name == name)
        .cloned()
        .ok_or_else(|| format!("profilo {name} non trovato per {}", cli.id))?;

    if let HomeMechanism::CredentialSymlink { relative_path } = cli.home {
        apply_symlink_swap(&home_dir()?, relative_path, &profile.home_dir)?;
    }
    // Per il meccanismo a variabile d'ambiente non c'è nulla da spostare sul
    // filesystem: registrare l'attivo qui basta, `build_environment` legge
    // `profile.home_dir` al momento del lancio — vedi lib.rs.
    store.active.insert(cli.id.to_owned(), name.clone());
    save_store(&store)
}

fn cmd_current(args: &[String]) -> Result<(), String> {
    let [cli_id] = args else {
        return Err("uso: profiles current <cli>".to_owned());
    };
    let cli = find_cli(cli_id)?;
    let store = load_store()?;
    match store.active.get(cli.id) {
        Some(name) => println!("{name}"),
        None => println!("(nessun profilo attivo)"),
    }
    Ok(())
}

/// Sposta il collegamento su `profile_home`, senza mai toccare un file
/// reale: rifiuta se `link_path` non è già un collegamento, e pretende che
/// il profilo abbia già le sue credenziali — non ne fabbrica di vuote, che
/// finirebbero prese per vere dalla riga di comando. Così il profilo
/// lasciato non perde nulla: il suo file non viene mai aperto in scrittura.
fn apply_symlink_swap(
    fixed_home: &Path,
    relative_path: &str,
    profile_home: &Path,
) -> Result<(), String> {
    let swap = symlink_swap(fixed_home, relative_path, profile_home);
    if !swap.target_path.exists() {
        return Err(format!(
            "{} non esiste ancora: il profilo non ha credenziali da collegare",
            swap.target_path.display()
        ));
    }
    match fs::symlink_metadata(&swap.link_path) {
        Ok(meta) if meta.file_type().is_symlink() => {
            fs::remove_file(&swap.link_path).map_err(|e| {
                format!(
                    "impossibile rimuovere il vecchio collegamento {}: {e}",
                    swap.link_path.display()
                )
            })?;
        }
        Ok(_) => {
            return Err(format!(
                "{} non è un collegamento simbolico: lo scambio si ferma per non perdere credenziali reali",
                swap.link_path.display()
            ));
        }
        Err(e) if e.kind() == io::ErrorKind::NotFound => {}
        Err(e) => {
            return Err(format!(
                "impossibile leggere {}: {e}",
                swap.link_path.display()
            ))
        }
    }
    if let Some(parent) = swap.link_path.parent() {
        fs::create_dir_all(parent)
            .map_err(|e| format!("impossibile creare {}: {e}", parent.display()))?;
    }
    std::os::unix::fs::symlink(&swap.target_path, &swap.link_path)
        .map_err(|e| format!("impossibile collegare {}: {e}", swap.link_path.display()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    /// Cartella usa-e-getta sotto `$TMPDIR`, cancellata a fine prova. Niente
    /// dipendenza esterna: lo stesso schema già usato altrove nell'albero.
    struct TempDir(PathBuf);

    impl TempDir {
        fn new() -> Self {
            let unique = format!(
                "profiles-test-{}-{}",
                std::process::id(),
                SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .unwrap()
                    .as_nanos()
            );
            let path = env::temp_dir().join(unique);
            fs::create_dir_all(&path).expect("cartella di prova");
            TempDir(path)
        }

        fn path(&self) -> &Path {
            &self.0
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn store_roundtrip_survives_disk() {
        let dir = TempDir::new();
        let path = dir.path().join("stato").join("profili.json");
        let mut store = ProfileStore::default();
        store.profiles.push(Profile {
            name: "lavoro".to_owned(),
            cli_id: "claude".to_owned(),
            home_dir: dir.path().join("claude").join("lavoro"),
        });
        store.active.insert("claude".to_owned(), "lavoro".to_owned());

        save_store_to(&path, &store).expect("salvataggio");
        let reloaded = load_store_from(&path).expect("ricarica");
        assert_eq!(reloaded, store);
    }

    #[test]
    fn load_store_from_missing_file_is_an_empty_store() {
        let dir = TempDir::new();
        let path = dir.path().join("non-esiste.json");
        assert_eq!(
            load_store_from(&path).expect("nessun errore su file assente"),
            ProfileStore::default()
        );
    }

    /// La prova che conta per lo scambio rapido: due profili finti in una
    /// cartella temporanea, e il primo — quello che si lascia — deve uscirne
    /// con le sue credenziali intatte dopo lo scambio sul secondo.
    #[test]
    fn switching_profiles_never_loses_the_one_left_behind() {
        let dir = TempDir::new();
        let fixed_home = dir.path().join("casa-fissa");
        let profile_a = dir.path().join("profili").join("acme").join("a");
        let profile_b = dir.path().join("profili").join("acme").join("b");
        fs::create_dir_all(&profile_a).unwrap();
        fs::create_dir_all(&profile_b).unwrap();
        let relative = "credentials.json";
        fs::write(profile_a.join(relative), "credenziali-a").unwrap();
        fs::write(profile_b.join(relative), "credenziali-b").unwrap();

        apply_symlink_swap(&fixed_home, relative, &profile_a).expect("scambio su a");
        assert_eq!(
            fs::read_to_string(fixed_home.join(relative)).unwrap(),
            "credenziali-a"
        );

        apply_symlink_swap(&fixed_home, relative, &profile_b).expect("scambio su b");
        assert_eq!(
            fs::read_to_string(fixed_home.join(relative)).unwrap(),
            "credenziali-b"
        );

        assert_eq!(
            fs::read_to_string(profile_a.join(relative)).unwrap(),
            "credenziali-a",
            "il profilo lasciato ha perso le sue credenziali"
        );
    }

    #[test]
    fn apply_symlink_swap_refuses_to_clobber_a_real_file() {
        let dir = TempDir::new();
        let fixed_home = dir.path().join("casa-fissa");
        fs::create_dir_all(&fixed_home).unwrap();
        let relative = "credentials.json";
        fs::write(fixed_home.join(relative), "credenziali-vere").unwrap();

        let profile_a = dir.path().join("profili").join("acme").join("a");
        fs::create_dir_all(&profile_a).unwrap();
        fs::write(profile_a.join(relative), "credenziali-a").unwrap();

        let result = apply_symlink_swap(&fixed_home, relative, &profile_a);
        assert!(result.is_err());
        assert_eq!(
            fs::read_to_string(fixed_home.join(relative)).unwrap(),
            "credenziali-vere",
            "un file reale è stato toccato invece di essere rifiutato"
        );
    }

    #[test]
    fn apply_symlink_swap_refuses_a_profile_without_credentials_yet() {
        let dir = TempDir::new();
        let fixed_home = dir.path().join("casa-fissa");
        let profile_a = dir.path().join("profili").join("acme").join("a");
        fs::create_dir_all(&profile_a).unwrap();

        let result = apply_symlink_swap(&fixed_home, "credentials.json", &profile_a);
        assert!(result.is_err());
    }

    #[test]
    fn build_environment_uses_the_profile_home_recorded_in_the_store() {
        let cli = find_cli("codex").unwrap();
        let home = PathBuf::from("/home/profiles/codex/lavoro");
        let env = build_environment(cli, &home);
        assert_eq!(env.get("CODEX_HOME"), Some(&"/home/profiles/codex/lavoro".to_owned()));
    }
}
