//! `sailor run <cli> [argomenti...]`: lo swap rapido. Trova il profilo
//! attivo di `cli` e **sostituisce** questo processo con il suo eseguibile —
//! mai un figlio, perché segnali, codice d'uscita e terminale interattivo
//! devono comportarsi come se l'avessi invocata a mano. Senza un profilo
//! attivo, o con un meccanismo di casa ancora ignoto, si rifiuta: lanciare
//! con l'identità sbagliata è il guasto peggiore che questo comando possa
//! fare.

use profiles::{build_environment, find_cli, store_io, HomeMechanism, ProfileStore};
use std::collections::BTreeMap;
use std::os::unix::process::CommandExt;
use std::process::Command;

/// Cosa lanciare per `cli_id`, secondo lo stato dato: pura — non tocca
/// processi — così la prova che conta (lo scambio raggiunge l'ambiente
/// giusto) non deve rimpiazzare se stessa per verificarlo.
#[derive(Debug)]
struct Launch {
    executable: String,
    env: BTreeMap<String, String>,
    args: Vec<String>,
}

fn resolve(cli_id: &str, store: &ProfileStore, rest: &[String]) -> Result<Launch, String> {
    let cli = find_cli(cli_id)?;
    if matches!(cli.home, HomeMechanism::Unknown) {
        return Err(format!(
            "{}: non si sa ancora come sposti la sua cartella di casa ({}); sailor run si rifiuta di indovinare",
            cli.display_name, cli.home_note
        ));
    }
    let Some(active_name) = store.active.get(cli.id) else {
        return Err(format!(
            "nessun profilo attivo per {}: usa 'sailor profiles switch {} <nome>' prima di lanciarla",
            cli.display_name, cli.id
        ));
    };
    let profile = store
        .profiles
        .iter()
        .find(|p| p.cli_id == cli.id && &p.name == active_name)
        .ok_or_else(|| {
            format!(
                "il profilo attivo '{active_name}' per {} non esiste più nello stato",
                cli.id
            )
        })?;

    Ok(Launch {
        executable: cli.executable.to_owned(),
        env: build_environment(cli, &profile.home_dir),
        args: rest.to_vec(),
    })
}

pub fn run(args: &[String]) -> i32 {
    let [cli_id, rest @ ..] = args else {
        eprintln!("uso: sailor run <cli> [argomenti...]");
        return 2;
    };
    let store = match store_io::load_store() {
        Ok(store) => store,
        Err(message) => {
            eprintln!("sailor run: {message}");
            return 1;
        }
    };
    let launch = match resolve(cli_id, &store, rest) {
        Ok(launch) => launch,
        Err(message) => {
            eprintln!("sailor run: {message}");
            return 1;
        }
    };
    // `exec` sostituisce l'immagine di questo processo: se riesce, il codice
    // sotto non gira più. Torna solo per dire che il lancio è fallito.
    let error = Command::new(&launch.executable)
        .args(&launch.args)
        .envs(&launch.env)
        .exec();
    eprintln!("sailor run: non riesco ad avviare {}: {error}", launch.executable);
    1
}

#[cfg(test)]
mod tests {
    use super::*;
    use profiles::Profile;
    use std::path::PathBuf;

    fn two_profile_store() -> ProfileStore {
        let mut store = ProfileStore::default();
        store.profiles.push(Profile {
            name: "primo".to_owned(),
            cli_id: "codex".to_owned(),
            home_dir: PathBuf::from("/prova/codex/primo"),
        });
        store.profiles.push(Profile {
            name: "secondo".to_owned(),
            cli_id: "codex".to_owned(),
            home_dir: PathBuf::from("/prova/codex/secondo"),
        });
        store
    }

    /// La prova che conta: lo scambio deve raggiungere l'ambiente che
    /// verrebbe passato al processo, non restare scritto solo nello stato.
    /// Se qui vedesse lo stesso valore prima e dopo, lo swap non
    /// funzionerebbe: per questo il primo e il secondo profilo hanno case
    /// diverse e la prova le confronta entrambe.
    #[test]
    fn switching_the_active_profile_changes_what_the_launch_would_see() {
        let mut store = two_profile_store();
        store.active.insert("codex".to_owned(), "primo".to_owned());
        let launch = resolve("codex", &store, &[]).unwrap();
        assert_eq!(
            launch.env.get("CODEX_HOME"),
            Some(&"/prova/codex/primo".to_owned())
        );

        store.active.insert("codex".to_owned(), "secondo".to_owned());
        let launch = resolve("codex", &store, &[]).unwrap();
        assert_eq!(
            launch.env.get("CODEX_HOME"),
            Some(&"/prova/codex/secondo".to_owned())
        );
    }

    #[test]
    fn without_an_active_profile_the_launch_is_refused() {
        let store = two_profile_store();
        let error = resolve("codex", &store, &[]).unwrap_err();
        assert!(error.contains("nessun profilo attivo"), "{error}");
    }

    #[test]
    fn a_stale_active_name_that_matches_no_profile_is_refused() {
        let mut store = two_profile_store();
        store.active.insert("codex".to_owned(), "sparito".to_owned());
        let error = resolve("codex", &store, &[]).unwrap_err();
        assert!(error.contains("non esiste più"), "{error}");
    }

    #[test]
    fn an_unverified_home_mechanism_is_refused_with_the_reason() {
        let mut store = ProfileStore::default();
        store.profiles.push(Profile {
            name: "prova".to_owned(),
            cli_id: "antigravity".to_owned(),
            home_dir: PathBuf::from("/prova/antigravity/prova"),
        });
        store.active.insert("antigravity".to_owned(), "prova".to_owned());
        let error = resolve("antigravity", &store, &[]).unwrap_err();
        assert!(error.contains("non si sa ancora"), "{error}");
    }

    #[test]
    fn an_unknown_cli_is_refused() {
        let store = ProfileStore::default();
        assert!(resolve("non-esiste", &store, &[]).is_err());
    }

    #[test]
    fn no_cli_named_is_a_usage_error() {
        assert_eq!(run(&[]), 2);
    }
}
