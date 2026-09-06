//! Triggers live in the same home as the tools.
//!
//! **Why this proof exists.** `trigger::default_sources` was once a hand copy
//! of `toolbox::default_sources`, and like it fell back on `~/.sailor`,
//! ignoring `XDG_CONFIG_HOME`. The two copies were wrong together, which made
//! them look right: they agreed with each other and disagreed with the store.
//! The twin proof on `ledger` is in
//! `crates/toolbox/tests/one_home_for_everything.rs`.

use std::collections::BTreeMap;
use std::path::PathBuf;
use toolbox::probe::Machine;
use trigger::descriptor::Source;

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

fn user_dir(sources: &[Source]) -> PathBuf {
    sources
        .iter()
        .find_map(|source| match source {
            Source::Dir(path) => Some(path.clone()),
            _ => None,
        })
        .expect("una cartella dell'utente fra le sorgenti degli inneschi")
}

/// Each crate has its own `Source` type: naming both here is the price of
/// comparing the two paths for real instead of trusting them.
fn tool_user_dir(sources: &[toolbox::descriptor::Source]) -> PathBuf {
    sources
        .iter()
        .find_map(|source| match source {
            toolbox::descriptor::Source::Dir(path) => Some(path.clone()),
            _ => None,
        })
        .expect("una cartella dell'utente fra le sorgenti degli strumenti")
}

/// **The expected path is written out in full, on purpose.** A version of this
/// proof that compared only the triggers' path against the tools' stayed green
/// when the defect was put back deliberately: two copies wrong together agree
/// with each other. A proof comparing two things that can be wrong the same way
/// proves nothing; it needs an anchor outside both.
#[test]
fn triggers_and_tools_share_one_home() {
    for (env, expected_home) in [
        (Vec::new(), "/home/tizio/.config/sailor"),
        (
            vec![("XDG_CONFIG_HOME", "/altrove/conf")],
            "/altrove/conf/sailor",
        ),
        (
            vec![("SAILOR_HOME", "/casa/dichiarata")],
            "/casa/dichiarata",
        ),
    ] {
        let machine = machine_with("/home/tizio", &env);
        let home = PathBuf::from(expected_home);

        assert_eq!(
            user_dir(&trigger::default_sources(&machine)),
            home.join("triggers.d"),
            "gli inneschi vanno cercati nella casa, qualunque essa sia (env: {env:?})"
        );
        assert_eq!(
            tool_user_dir(&toolbox::default_sources(&machine)),
            home.join("tools.d"),
            "e gli strumenti nella stessa (env: {env:?})"
        );
    }
}
