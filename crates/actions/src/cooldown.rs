//! Engines set aside after saying their quota is spent, so a chain does not
//! knock again on a door known to be shut before the time the descriptor
//! declares. A file, so every process on the machine reads the same list.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SetAside {
    pub since: i64,
    pub until: i64,
    /// What the engine said, one line, for the person who wonders why.
    pub said: String,
}

/// Where the list lives: `SAILOR_COOLDOWNS`, or `cooldowns.json` in the home.
pub fn default_path() -> Option<PathBuf> {
    if let Some(declared) = std::env::var_os("SAILOR_COOLDOWNS").filter(|value| !value.is_empty()) {
        return Some(PathBuf::from(declared));
    }
    ledger::sailor_home().map(|home| home.join("cooldowns.json"))
}

fn read(path: &Path) -> BTreeMap<String, SetAside> {
    std::fs::read_to_string(path)
        .ok()
        .and_then(|text| serde_json::from_str(&text).ok())
        .unwrap_or_default()
}

/// Sets `engine` aside for `secs` from `now`, remembering what it said.
pub fn set_aside(path: &Path, engine: &str, now: i64, secs: u64, said: &str) -> Result<(), String> {
    let mut all = read(path);
    all.insert(
        engine.to_owned(),
        SetAside {
            since: now,
            until: now + secs as i64,
            said: said.split_whitespace().collect::<Vec<_>>().join(" "),
        },
    );
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|error| error.to_string())?;
    }
    let text = serde_json::to_string_pretty(&all).map_err(|error| error.to_string())?;
    std::fs::write(path, text).map_err(|error| error.to_string())
}

/// Until when `engine` is set aside, if it still is at `now`.
pub fn set_aside_until(path: &Path, engine: &str, now: i64) -> Option<SetAside> {
    read(path).remove(engine).filter(|aside| aside.until > now)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_engine_is_aside_until_its_time_and_not_after() {
        let dir = std::env::temp_dir().join(format!("sailor-cooldown-{}", std::process::id()));
        let path = dir.join("cooldowns.json");
        // The control first: nothing written, nobody aside, and a missing file is not an error.
        assert_eq!(set_aside_until(&path, "x", 0), None);
        set_aside(&path, "x", 100, 60, "weekly  limit\nreached").expect("written");
        let aside = set_aside_until(&path, "x", 159).expect("still aside");
        assert_eq!((aside.since, aside.until, aside.said.as_str()), (100, 160, "weekly limit reached"));
        assert_eq!(set_aside_until(&path, "x", 160), None, "at its time it is back");
        assert_eq!(set_aside_until(&path, "y", 120), None, "another engine is not aside");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
