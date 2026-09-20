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

fn write(path: &Path, all: &BTreeMap<String, SetAside>) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|error| error.to_string())?;
    }
    let text = serde_json::to_string_pretty(all).map_err(|error| error.to_string())?;
    std::fs::write(path, text).map_err(|error| error.to_string())
}

/// Sets `engine` aside for `secs` from `now`, remembering what it said.
///
/// Entries whose time has run out are dropped here: no reader can see one, so
/// keeping them only grows a file nobody prunes.
pub fn set_aside(path: &Path, engine: &str, now: i64, secs: u64, said: &str) -> Result<(), String> {
    let mut all = read(path);
    all.retain(|_, aside| aside.until > now);
    all.insert(
        engine.to_owned(),
        SetAside {
            since: now,
            until: now + secs as i64,
            said: said.split_whitespace().collect::<Vec<_>>().join(" "),
        },
    );
    write(path, &all)
}

/// Until when `engine` is set aside, if it still is at `now`.
pub fn set_aside_until(path: &Path, engine: &str, now: i64) -> Option<SetAside> {
    read(path).remove(engine).filter(|aside| aside.until > now)
}

/// Everything still set aside at `now`, by the clock `set_aside_until` reads.
pub fn all_set_aside(path: &Path, now: i64) -> BTreeMap<String, SetAside> {
    let mut all = read(path);
    all.retain(|_, aside| aside.until > now);
    all
}

/// Takes `key` off the list, whatever its clock still said, and answers whether
/// an entry was there. A list nobody ever wrote holds nothing to bring back,
/// which is an answer and not a failure.
pub fn bring_back(path: &Path, key: &str) -> Result<bool, String> {
    let mut all = read(path);
    if all.remove(key).is_none() {
        return Ok(false);
    }
    write(path, &all)?;
    Ok(true)
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

    fn a_scratch(name: &str) -> PathBuf {
        std::env::temp_dir().join(format!("sailor-cooldown-{}-{name}", std::process::id()))
    }

    #[test]
    fn an_engine_brought_back_is_knocked_on_again_before_its_time() {
        let dir = a_scratch("bring-back");
        let path = dir.join("cooldowns.json");
        set_aside(&path, "x", 100, 600, "quota spent").expect("written");
        assert!(set_aside_until(&path, "x", 200).is_some(), "aside to begin with");
        assert_eq!(bring_back(&path, "x"), Ok(true), "the entry was there");
        assert_eq!(set_aside_until(&path, "x", 200), None, "brought back before its time");
        assert_eq!(bring_back(&path, "x"), Ok(false), "and there is nothing left to bring back");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn bringing_back_what_no_list_holds_is_an_answer_and_not_a_failure() {
        let dir = a_scratch("missing");
        let path = dir.join("cooldowns.json");
        assert_eq!(bring_back(&path, "x"), Ok(false));
        assert!(!path.exists(), "a question does not write a list");
        set_aside(&path, "x", 100, 600, "quota spent").expect("written");
        assert_eq!(bring_back(&path, "y"), Ok(false), "another engine is not this one");
        assert!(set_aside_until(&path, "x", 200).is_some(), "and the one set aside is untouched");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn the_whole_list_names_what_is_still_aside_and_nothing_else() {
        let dir = a_scratch("all");
        let path = dir.join("cooldowns.json");
        assert!(all_set_aside(&path, 0).is_empty(), "no file, nobody aside");
        set_aside(&path, "x", 100, 600, "quota spent").expect("written");
        set_aside(&path, "y@second", 100, 60, "weekly limit").expect("written");
        let named: Vec<String> = all_set_aside(&path, 150).keys().cloned().collect();
        assert_eq!(named, vec!["x".to_owned(), "y@second".to_owned()]);
        assert_eq!(all_set_aside(&path, 150)["y@second"].said, "weekly limit");
        let after = all_set_aside(&path, 300);
        assert_eq!(after.keys().cloned().collect::<Vec<_>>(), vec!["x".to_owned()], "the lapsed one is gone");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_lapsed_entry_leaves_the_file_the_next_time_it_is_written() {
        let dir = a_scratch("prune");
        let path = dir.join("cooldowns.json");
        set_aside(&path, "x", 100, 60, "quota spent").expect("written");
        set_aside(&path, "y", 1_000, 600, "quota spent").expect("written");
        let held: BTreeMap<String, SetAside> =
            serde_json::from_str(&std::fs::read_to_string(&path).expect("a list")).expect("json");
        assert_eq!(
            held.keys().cloned().collect::<Vec<_>>(),
            vec!["y".to_owned()],
            "the file itself no longer carries the lapsed entry"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }
}
