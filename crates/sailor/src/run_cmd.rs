//! `sailor run <cli> [argomenti...]`: lo swap rapido. Trova il profilo
//! attivo di `cli` e **sostituisce** questo processo con il suo eseguibile —
//! mai un figlio, perché segnali, codice d'uscita e terminale interattivo
//! devono comportarsi come se l'avessi invocata a mano. Senza un profilo
//! attivo, o con un meccanismo di casa ancora ignoto, si rifiuta: lanciare
//! con l'identità sbagliata è il guasto peggiore che questo comando possa
//! fare.

use profiles::{
    build_environment, find_cli, store_io, symlink_swap, HomeMechanism, ProfileStore, SymlinkSwap,
};
use std::collections::BTreeMap;
use std::os::unix::process::CommandExt;
use std::path::PathBuf;
use std::process::Command;

/// Cosa lanciare per `cli_id`, secondo lo stato dato: pura — non tocca
/// processi — così la prova che conta (lo scambio raggiunge l'ambiente
/// giusto) non deve rimpiazzare se stessa per verificarlo.
#[derive(Debug)]
struct Launch {
    executable: String,
    env: BTreeMap<String, String>,
    args: Vec<String>,
    /// Per le righe di comando che non spostano la casa con una variabile ma
    /// scambiano un collegamento sulle credenziali, il collegamento che deve
    /// risultare in piedi perché il lancio abbia l'identità giusta. `None` per
    /// tutte le altre. Chi lancia lo verifica **prima** di sostituire il
    /// processo; qui resta un dato, così questa funzione non tocca il disco.
    expected_link: Option<SymlinkSwap>,
}

fn resolve(
    cli_id: &str,
    store: &ProfileStore,
    rest: &[String],
    fixed_home: &std::path::Path,
) -> Result<Launch, String> {
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

    // Il meccanismo a collegamento non passa da nessuna variabile: l'identità
    // del lancio dipende **solo** da dove punta un collegamento sul disco. Se
    // lo stato dice «attivo X» ma il collegamento punta ancora a Y, la riga di
    // comando parte con le credenziali di Y e nessuno se ne accorge. Qui si
    // calcola il collegamento atteso; chi lancia lo confronta con quello vero.
    let expected_link = match cli.home {
        HomeMechanism::CredentialSymlink { relative_path } => {
            Some(symlink_swap(fixed_home, relative_path, &profile.home_dir))
        }
        _ => None,
    };

    Ok(Launch {
        executable: cli.executable.to_owned(),
        env: build_environment(cli, &profile.home_dir),
        args: rest.to_vec(),
        expected_link,
    })
}

/// Il collegamento sul disco punta davvero al profilo attivo? Si rifiuta invece
/// di ripararlo: `sailor run` non deve avere effetti collaterali sul disco, e un
/// rifiuto esplicito è più sicuro di una riparazione silenziosa — chi legge il
/// messaggio sa che stato e disco si erano separati, e lo scambio ha già il suo
/// comando.
fn link_points_at_the_active_profile(expected: &SymlinkSwap) -> Result<(), String> {
    match std::fs::read_link(&expected.link_path) {
        Ok(actual) if actual == expected.target_path => Ok(()),
        Ok(actual) => Err(format!(
            "{} punta a {}, ma il profilo attivo vuole {}: lo stato e il disco si sono separati. \
             Rifai 'sailor profiles switch' per quella riga di comando prima di lanciarla",
            expected.link_path.display(),
            actual.display(),
            expected.target_path.display()
        )),
        Err(error) => Err(format!(
            "non riesco a leggere il collegamento {}: {error}. \
             Senza, non so con quale identità partirebbe",
            expected.link_path.display()
        )),
    }
}

fn home_dir() -> PathBuf {
    std::env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."))
}

/// La forma di `sailor run`. Vedi `flow_cmd::USAGE`.
pub const USAGE: &[&str] = &["sailor run <cli> [arguments...]"];

pub fn run(args: &[String]) -> i32 {
    let [cli_id, rest @ ..] = args else {
        eprintln!("usage: {}", USAGE[0]);
        return 2;
    };
    let store = match store_io::load_store() {
        Ok(store) => store,
        Err(message) => {
            eprintln!("sailor run: {message}");
            return 1;
        }
    };
    let launch = match resolve(cli_id, &store, rest, &home_dir()) {
        Ok(launch) => launch,
        Err(message) => {
            eprintln!("sailor run: {message}");
            return 1;
        }
    };
    // Prima di `exec`, mai dopo: dopo non si torna.
    if let Some(expected) = &launch.expected_link {
        if let Err(message) = link_points_at_the_active_profile(expected) {
            eprintln!("sailor run: {message}");
            return 1;
        }
    }
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
    use std::path::{Path, PathBuf};

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
        let launch = resolve("codex", &store, &[], Path::new("/casa")).unwrap();
        assert_eq!(
            launch.env.get("CODEX_HOME"),
            Some(&"/prova/codex/primo".to_owned())
        );

        store.active.insert("codex".to_owned(), "secondo".to_owned());
        let launch = resolve("codex", &store, &[], Path::new("/casa")).unwrap();
        assert_eq!(
            launch.env.get("CODEX_HOME"),
            Some(&"/prova/codex/secondo".to_owned())
        );
    }

    #[test]
    fn without_an_active_profile_the_launch_is_refused() {
        let store = two_profile_store();
        let error = resolve("codex", &store, &[], Path::new("/casa")).unwrap_err();
        assert!(error.contains("nessun profilo attivo"), "{error}");
    }

    #[test]
    fn a_stale_active_name_that_matches_no_profile_is_refused() {
        let mut store = two_profile_store();
        store.active.insert("codex".to_owned(), "sparito".to_owned());
        let error = resolve("codex", &store, &[], Path::new("/casa")).unwrap_err();
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
        let error = resolve("antigravity", &store, &[], Path::new("/casa")).unwrap_err();
        assert!(error.contains("non si sa ancora"), "{error}");
    }

    #[test]
    fn an_unknown_cli_is_refused() {
        let store = ProfileStore::default();
        assert!(resolve("non-esiste", &store, &[], Path::new("/casa")).is_err());
    }

    #[test]
    fn no_cli_named_is_a_usage_error() {
        assert_eq!(run(&[]), 2);
    }

    /// Il difetto trovato il 27/08/2026 da un revisore indipendente, che non
    /// aveva scritto questo codice: per una riga di comando che scambia un
    /// collegamento invece di leggere una variabile, l'ambiente costruito è
    /// **vuoto** — l'identità del lancio dipende solo da dove punta un file sul
    /// disco. Nessuna riga di comando in tabella lo usa oggi, quindi non era
    /// sfruttabile; la prima che lo userà sarebbe partita con le credenziali di
    /// un altro profilo, in silenzio.
    ///
    /// Le tre situazioni contano tutte e tre, e la prova sa fallire in tutte:
    /// il collegamento giusto passa, quello che punta altrove è rifiutato, e
    /// quello assente pure — «non lo so leggere» non è «va bene».
    #[test]
    fn a_link_pointing_at_another_profile_stops_the_launch() {
        let dir = std::env::temp_dir().join(format!("sailor-run-link-{}", std::process::id()));
        let fixed_home = dir.join("casa-fissa");
        let wanted = dir.join("profili").join("voluto");
        let other = dir.join("profili").join("altro");
        std::fs::create_dir_all(&fixed_home).unwrap();
        std::fs::create_dir_all(&wanted).unwrap();
        std::fs::create_dir_all(&other).unwrap();
        std::fs::write(wanted.join("credentials.json"), "quelle giuste").unwrap();
        std::fs::write(other.join("credentials.json"), "quelle di un altro").unwrap();

        let expected = symlink_swap(&fixed_home, "credentials.json", &wanted);

        // Nessun collegamento: si rifiuta invece di lanciare alla cieca.
        let error = link_points_at_the_active_profile(&expected).unwrap_err();
        assert!(error.contains("non riesco a leggere"), "{error}");

        // Collegamento verso un altro profilo: si rifiuta, e dice quale.
        std::os::unix::fs::symlink(other.join("credentials.json"), &expected.link_path).unwrap();
        let error = link_points_at_the_active_profile(&expected).unwrap_err();
        assert!(error.contains("si sono separati"), "{error}");
        assert!(error.contains("altro"), "{error}");

        // Collegamento verso il profilo attivo: passa.
        std::fs::remove_file(&expected.link_path).unwrap();
        std::os::unix::fs::symlink(&expected.target_path, &expected.link_path).unwrap();
        assert!(link_points_at_the_active_profile(&expected).is_ok());

        let _ = std::fs::remove_dir_all(&dir);
    }
}
