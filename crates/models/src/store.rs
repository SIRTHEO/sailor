//! Where `modelli.json` lives on disk: reading and writing files, no
//! judgement — the free-only rule is in `config.rs`, not here. The path is not
//! read from the environment in this file: the caller decides it, so tests
//! stay on throwaway paths instead of the process's real environment.

use crate::config::UserConfig;
use std::path::{Path, PathBuf};

/// `MODELS_CONFIG_PATH` when set, otherwise `~/.claude/state/modelli.json`.
/// Never hardcoded anywhere else in the crate.
pub fn config_path() -> PathBuf {
    if let Ok(p) = std::env::var("MODELS_CONFIG_PATH") {
        return PathBuf::from(p);
    }
    let home = std::env::var("HOME").unwrap_or_default();
    PathBuf::from(format!("{home}/.claude/state/modelli.json"))
}

/// Reads the configuration from disk. A missing or unreadable file is not an
/// error: it means "not configured", and the free-only rule rests on that —
/// whoever has not chosen yet gets the free models.
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

    // A test path under the system temp directory, with the pid in the name:
    // two tests running in parallel do not step on each other.
    fn tmp_path(name: &str) -> PathBuf {
        let mut p = std::env::temp_dir();
        p.push(format!("models-crate-test-{}-{name}", std::process::id()));
        p
    }

    #[test]
    fn config_path_honours_the_override_env_var() {
        let key = "MODELS_CONFIG_PATH";
        let previous = std::env::var(key).ok();
        std::env::set_var(key, "/tmp/models-under-test.json");
        assert_eq!(config_path(), PathBuf::from("/tmp/models-under-test.json"));
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
        std::fs::write(&path, "{ this is not valid json").unwrap();
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
