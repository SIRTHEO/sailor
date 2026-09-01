//! Sailor's home is one place, and all who look for it arrive by the same rule.
//!
//! **WHY THESE TESTS EXIST.** Home was once in two places: the ledger resolved
//! `~/.config/sailor`, the user's descriptors `~/.sailor`, because that second
//! rule was a hand-written copy ignoring `XDG_CONFIG_HOME`. The two homes could
//! not see each other: a price list put where the docs said went unread by all.

use std::collections::BTreeMap;
use std::path::PathBuf;
use toolbox::descriptor::Source;
use toolbox::probe::Machine;

/// A described machine with nothing around it: only home and the environment
/// that matters to this question. No test here touches the disk — they compare
/// where the lookup would go, which is exactly what had drifted apart.
fn machine_with(home: &str, env: &[(&str, &str)]) -> Machine {
    Machine {
        path_dirs: Vec::new(),
        home: PathBuf::from(home),
        env: env
            .iter()
            .map(|(k, v)| ((*k).to_owned(), (*v).to_owned()))
            .collect::<BTreeMap<_, _>>(),
        version_probes: false,
    }
}

/// Where `default_sources` went looking for the user's descriptors.
fn user_dir(sources: &[Source]) -> PathBuf {
    sources
        .iter()
        .find_map(|source| match source {
            Source::Dir(path) => Some(path.clone()),
            _ => None,
        })
        .expect("a user directory among the sources")
}

#[test]
fn tools_and_the_ledger_resolve_the_same_home() {
    let machine = machine_with("/home/someone", &[]);

    let from_tools = toolbox::sailor_home_for(&machine);
    let from_ledger = ledger::sailor_home_in(None, None, PathBuf::from("/home/someone"));

    assert_eq!(
        from_tools, from_ledger,
        "the descriptors and the ledger must look in the same home"
    );
    assert_eq!(from_tools, PathBuf::from("/home/someone/.config/sailor"));
}

#[test]
fn the_user_descriptors_live_under_that_same_home() {
    let machine = machine_with("/home/someone", &[]);

    assert_eq!(
        user_dir(&toolbox::default_sources(&machine)),
        PathBuf::from("/home/someone/.config/sailor/tools.d"),
        "the tool descriptors live inside home, not beside it"
    );
}

/// The case the defect got wrong: `XDG_CONFIG_HOME` was read by the ledger and
/// ignored by the descriptors. Whoever declares it — that is, whoever keeps
/// their configuration outside `~/.config` — saw the two halves come apart.
#[test]
fn a_declared_config_home_moves_the_descriptors_too() {
    let machine = machine_with("/home/someone", &[("XDG_CONFIG_HOME", "/elsewhere/conf")]);

    assert_eq!(
        toolbox::sailor_home_for(&machine),
        PathBuf::from("/elsewhere/conf/sailor")
    );
    assert_eq!(
        user_dir(&toolbox::default_sources(&machine)),
        PathBuf::from("/elsewhere/conf/sailor/tools.d")
    );
}

/// `SAILOR_HOME` stays the last word, and it counts the same way for everyone.
#[test]
fn a_declared_sailor_home_wins_over_everything_for_everyone() {
    let machine = machine_with(
        "/home/someone",
        &[
            ("SAILOR_HOME", "/declared/home"),
            ("XDG_CONFIG_HOME", "/elsewhere/conf"),
        ],
    );

    assert_eq!(
        toolbox::sailor_home_for(&machine),
        PathBuf::from("/declared/home")
    );
    assert_eq!(
        user_dir(&toolbox::default_sources(&machine)),
        PathBuf::from("/declared/home/tools.d")
    );
}
