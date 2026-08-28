//! I gesti impuri di `profiles`: variabili d'ambiente, disco, collegamenti
//! simbolici. Prima del 27/08/2026 stavano nel `main.rs` del crate, quando
//! `profiles` era ancora un binario a sé; da quando lo esegue `sailor
//! profiles`, la parte pura resta in `lib.rs` e questa è l'unica che tocca
//! il mondo.

use crate::{parse_store, serialize_store, symlink_swap, ProfileStore};
use std::env;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

/// `HOME`, o un errore leggibile se non è impostata.
pub fn home_dir() -> Result<PathBuf, String> {
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

/// `PROFILES_STATE_PATH`, se impostata, altrimenti `~/.claude/state/profili.json`.
pub fn state_path() -> PathBuf {
    env::var_os("PROFILES_STATE_PATH")
        .map(PathBuf::from)
        .unwrap_or_else(default_state_path)
}

/// `PROFILES_HOME_ROOT`, se impostata, altrimenti accanto allo stato, in `profiles-homes/`.
pub fn profiles_root() -> PathBuf {
    env::var_os("PROFILES_HOME_ROOT")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            state_path()
                .parent()
                .map(|p| p.join("profiles-homes"))
                .unwrap_or_else(|| PathBuf::from("profiles-homes"))
        })
}

pub fn load_store_from(path: &Path) -> Result<ProfileStore, String> {
    match fs::read_to_string(path) {
        Ok(content) => {
            parse_store(&content).map_err(|e| format!("stato illeggibile in {}: {e}", path.display()))
        }
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(ProfileStore::default()),
        Err(e) => Err(format!("impossibile leggere {}: {e}", path.display())),
    }
}

/// La cartella da creare prima di scrivere, o niente se non ce n'è una.
///
/// Il filtro sul vuoto non è pignoleria: per un percorso senza cartella
/// ("profili.json") `parent()` risponde `Some("")`, e `create_dir_all("")`
/// fallisce con «file inesistente» — il salvataggio si rifiuterebbe pur avendo
/// i permessi sulla cartella corrente. Segnalato il 27/08/2026 da un revisore
/// indipendente.
///
/// STA IN UNA FUNZIONE A SÉ PERCHÉ LA PROVA NON DEVE SPOSTARSI DI CARTELLA.
/// Provare il nome nudo per il suo effetto voleva dire `set_current_dir`, che è
/// di **processo**: mentre girava, le prove parallele scrivevano altrove e
/// cadevano. Il 28/08/2026 quella prova ha fatto fallire due batterie e
/// fermato un rilascio. La decisione, qui, si prova senza toccare niente.
fn parent_to_create(path: &Path) -> Option<&Path> {
    path.parent().filter(|p| !p.as_os_str().is_empty())
}

pub fn save_store_to(path: &Path, store: &ProfileStore) -> Result<(), String> {
    if let Some(parent) = parent_to_create(path) {
        fs::create_dir_all(parent)
            .map_err(|e| format!("impossibile creare {}: {e}", parent.display()))?;
    }
    let json = serialize_store(store).map_err(|e| format!("serializzazione fallita: {e}"))?;
    fs::write(path, json).map_err(|e| format!("impossibile scrivere {}: {e}", path.display()))
}

pub fn load_store() -> Result<ProfileStore, String> {
    load_store_from(&state_path())
}

pub fn save_store(store: &ProfileStore) -> Result<(), String> {
    save_store_to(&state_path(), store)
}

/// Sposta il collegamento su `profile_home`, senza mai toccare un file
/// reale: rifiuta se `link_path` non è già un collegamento, e pretende che
/// il profilo abbia già le sue credenziali — non ne fabbrica di vuote, che
/// finirebbero prese per vere dalla riga di comando. Così il profilo
/// lasciato non perde nulla: il suo file non viene mai aperto in scrittura.
pub fn apply_symlink_swap(
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
    use crate::Profile;
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

    /// Un percorso senza cartella davanti: `parent()` risponde con la stringa
    /// vuota, e chi la passa a `create_dir_all` si sente rispondere «file
    /// inesistente» pur avendo i permessi. Prima della riparazione del
    /// 27/08/2026 il salvataggio di un nome nudo falliva.
    ///
    /// I TRE BRACCI SONO LA PROVA: il nome nudo non ha una cartella da creare,
    /// quello con una cartella davanti ce l'ha, e quello assoluto pure. Togli il
    /// filtro sul vuoto in `parent_to_create` e il primo diventa rosso.
    #[test]
    fn a_bare_filename_has_no_directory_to_create() {
        assert_eq!(parent_to_create(Path::new("profili.json")), None);
        assert_eq!(
            parent_to_create(Path::new("stato/profili.json")),
            Some(Path::new("stato"))
        );
        assert_eq!(
            parent_to_create(Path::new("/tmp/stato/profili.json")),
            Some(Path::new("/tmp/stato"))
        );
    }

    /// E il salvataggio vero riesce davvero, provato dove nessun'altra prova sta
    /// guardando: una cartella tutta sua, con un percorso assoluto.
    #[test]
    fn the_store_is_written_where_it_is_asked_to_be() {
        let dir = TempDir::new();
        let path = dir.path().join("dentro").join("profili.json");
        save_store_to(&path, &ProfileStore::default()).expect("il salvataggio deve riuscire");
        assert!(path.exists());
    }
}
