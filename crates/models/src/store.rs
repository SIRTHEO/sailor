//! Dove vive `modelli.json` su disco: solo lettura e scrittura di file,
//! nessun giudizio — la regola dei soli gratuiti sta in `config.rs`, non
//! qui. Il percorso non si legge dall'ambiente in questo file: lo decide
//! chi chiama (`main.rs`), così le prove restano su percorsi di comodo e
//! non sull'ambiente reale del processo.

use crate::config::UserConfig;
use std::path::{Path, PathBuf};

/// `MODELS_CONFIG_PATH`, se presente, altrimenti `~/.claude/state/modelli.json`.
/// Mai cablato altrove nel crate: è la riga del mandato.
pub fn config_path() -> PathBuf {
    if let Ok(p) = std::env::var("MODELS_CONFIG_PATH") {
        return PathBuf::from(p);
    }
    let home = std::env::var("HOME").unwrap_or_default();
    PathBuf::from(format!("{home}/.claude/state/modelli.json"))
}

/// Legge la configurazione da disco. Un file assente o illeggibile non è un
/// errore: è "non configurato", e su questo si regge la riga di Theo — chi
/// non ha ancora scelto ottiene solo i gratuiti.
pub fn load(path: &Path) -> UserConfig {
    std::fs::read_to_string(path)
        .ok()
        .and_then(|s| UserConfig::parse(&s).ok())
        .unwrap_or_default()
}

pub fn save(path: &Path, cfg: &UserConfig) -> std::io::Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(path, cfg.to_json())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    // Un percorso di prova sotto la cartella temporanea di sistema, con il
    // pid nel nome: due prove in parallelo non si pestano i piedi.
    fn tmp_path(name: &str) -> PathBuf {
        let mut p = std::env::temp_dir();
        p.push(format!("models-crate-test-{}-{name}", std::process::id()));
        p
    }

    #[test]
    fn config_path_honours_the_override_env_var() {
        let key = "MODELS_CONFIG_PATH";
        let previous = std::env::var(key).ok();
        std::env::set_var(key, "/tmp/modelli-di-prova.json");
        assert_eq!(config_path(), PathBuf::from("/tmp/modelli-di-prova.json"));
        match previous {
            Some(v) => std::env::set_var(key, v),
            None => std::env::remove_var(key),
        }
    }

    #[test]
    fn a_missing_file_loads_as_not_configured() {
        let path = tmp_path("missing.json");
        let _ = std::fs::remove_file(&path);
        assert_eq!(load(&path), UserConfig::default());
    }

    #[test]
    fn save_then_load_round_trips() {
        let path = tmp_path("roundtrip.json");
        let mut cfg = UserConfig::default();
        cfg.set_unchecked("default", "nvidia/nemotron-3-super-120b-a12b:free");
        save(&path, &cfg).unwrap();
        let reloaded = load(&path);
        assert_eq!(
            reloaded.get("default"),
            Some("nvidia/nemotron-3-super-120b-a12b:free")
        );
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn a_corrupted_file_loads_as_not_configured_not_a_panic() {
        let path = tmp_path("corrupt.json");
        std::fs::write(&path, "{ questo non è json valido").unwrap();
        assert_eq!(load(&path), UserConfig::default());
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn save_creates_missing_parent_directories() {
        let mut path = tmp_path("nested-dir");
        path.push("modelli.json");
        let cfg = UserConfig::default();
        save(&path, &cfg).unwrap();
        assert!(path.exists());
        let _ = std::fs::remove_file(&path);
        let _ = std::fs::remove_dir(path.parent().unwrap());
    }
}
