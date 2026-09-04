//! What a machine declares must never leave it.
//!
//! **THE LIST WAS READ BY A JUDGE AND BY NOBODY ELSE.** A gate that only runs
//! at commit catches the leak in the file, long after the words were written
//! into a store that never asked. One rule, read from one place, so whoever
//! writes hears it when they write.

use std::path::{Path, PathBuf};

/// Where a machine keeps the names that must not be committed, below the home:
/// one per line, `#` opens a comment. `SAILOR_PRIVATE_NAMES` names it outright.
pub const PRIVATE_NAMES_BELOW_HOME: &str = "personal/.sailor-notes/private-names";

/// The declared list, or `None` when nothing declares one. A machine that
/// declares nothing forbids nothing, and that is not an error.
pub fn where_the_names_are(declared: Option<String>, home: Option<String>) -> Option<PathBuf> {
    if let Some(path) = declared.filter(|value| !value.is_empty()) {
        return Some(PathBuf::from(path));
    }
    Some(PathBuf::from(home.filter(|value| !value.is_empty())?).join(PRIVATE_NAMES_BELOW_HOME))
}

/// The names a list declares. Blank lines and `#` lines are not names.
pub fn names_in(list: &str) -> Vec<String> {
    list.lines()
        .map(str::trim)
        .filter(|line| !line.is_empty() && !line.starts_with('#'))
        .map(str::to_owned)
        .collect()
}

/// Why a text cannot be published. **THE NAME IS NEVER CARRIED**: this is read
/// on a terminal that may be recorded, and a refusal that echoes the secret to
/// explain itself has published it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Reason {
    /// A name the machine declares private, and where it starts.
    APrivateName { at: usize },
    /// An absolute path of this machine, which is true on no other.
    APathOfThisMachine { at: usize },
}

/// What in this text the repository could not publish, in the order found.
///
/// The comparison is case-insensitive because a name written differently is
/// the same name to whoever reads it afterwards.
pub fn what_cannot_be_published(text: &str, names: &[String], home: Option<&str>) -> Vec<Reason> {
    let lowered = text.to_lowercase();
    let mut found = Vec::new();
    for name in names {
        if name.is_empty() {
            continue;
        }
        if let Some(at) = lowered.find(&name.to_lowercase()) {
            found.push(Reason::APrivateName { at });
        }
    }
    if let Some(home) = home.filter(|value| !value.is_empty()) {
        if let Some(at) = text.find(home) {
            found.push(Reason::APathOfThisMachine { at });
        }
    }
    found.sort_by_key(|reason| match reason {
        Reason::APrivateName { at } | Reason::APathOfThisMachine { at } => *at,
    });
    found
}

/// The names this machine declares, read from disk. Empty when unarmed.
pub fn declared_here(read: &dyn Fn(&Path) -> Option<String>) -> Vec<String> {
    let where_it_is = where_the_names_are(
        std::env::var("SAILOR_PRIVATE_NAMES").ok(),
        std::env::var("HOME").ok(),
    );
    where_it_is
        .and_then(|path| read(&path))
        .map(|text| names_in(&text))
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_declared_list_beats_the_one_below_the_home() {
        assert_eq!(
            where_the_names_are(Some("/altrove/elenco".to_owned()), Some("/casa".to_owned())),
            Some(PathBuf::from("/altrove/elenco"))
        );
        assert_eq!(
            where_the_names_are(None, Some("/casa".to_owned())),
            Some(PathBuf::from("/casa").join(PRIVATE_NAMES_BELOW_HOME))
        );
        // A variable exported empty by a script that could not find the home
        // would put the list at the root of the disk.
        assert_eq!(where_the_names_are(Some(String::new()), None), None);
        assert_eq!(where_the_names_are(None, None), None);
    }

    #[test]
    fn comments_and_blank_lines_are_not_names() {
        let names = names_in("# quelli di questa macchina\n\nunmotore\n  senza-casa  \n");
        assert_eq!(names, vec!["unmotore", "senza-casa"]);
    }

    /// **A REFUSAL THAT ECHOES THE SECRET HAS PUBLISHED IT.** What comes back
    /// says where, never what.
    #[test]
    fn a_private_name_is_found_and_never_carried_back() {
        let names = vec!["unmotore".to_owned()];
        let found = what_cannot_be_published("il passo chiama unmotore e finisce", &names, None);
        assert_eq!(found, vec![Reason::APrivateName { at: 16 }]);

        let said = format!("{found:?}");
        assert!(!said.contains("unmotore"), "the refusal carries the name: {said}");
    }

    /// Written another way it is the same name to whoever reads it after.
    #[test]
    fn a_name_written_in_capitals_is_the_same_name() {
        let names = vec!["unmotore".to_owned()];
        assert_eq!(
            what_cannot_be_published("UnMotore risponde", &names, None),
            vec![Reason::APrivateName { at: 0 }]
        );
    }

    /// The home path is the other half: true on this machine and on no other.
    #[test]
    fn a_path_of_this_machine_is_found_too() {
        let found = what_cannot_be_published(
            "measured in /casa/di-chiunque/personal/sailor",
            &[],
            Some("/casa/di-chiunque"),
        );
        assert_eq!(found, vec![Reason::APathOfThisMachine { at: 12 }]);
    }

    /// A machine that declares nothing forbids nothing, and says so by
    /// answering an empty list rather than by refusing everything.
    #[test]
    fn an_unarmed_machine_forbids_nothing() {
        assert!(what_cannot_be_published("qualunque cosa", &[], None).is_empty());
        assert!(what_cannot_be_published("qualunque cosa", &[String::new()], Some("")).is_empty());
    }
}
