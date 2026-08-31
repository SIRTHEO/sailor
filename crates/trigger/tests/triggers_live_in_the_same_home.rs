//! Gli inneschi stanno nella stessa casa degli strumenti.
//!
//! **Perché questa prova esiste.** Fino al 30/08/2026 `trigger::default_sources`
//! era una copia a mano di `toolbox::default_sources`, e come lei cadeva su
//! `~/.sailor` ignorando `XDG_CONFIG_HOME`. Le due copie sbagliavano insieme, il
//! che le faceva sembrare giuste: erano d'accordo fra loro e in disaccordo col
//! deposito. La prova gemella su `ledger` sta in
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

/// I due crate hanno ciascuno il proprio tipo `Source`: nominarli entrambi qui è
/// il prezzo per confrontare le due strade davvero, invece di fidarsi.
fn tool_user_dir(sources: &[toolbox::descriptor::Source]) -> PathBuf {
    sources
        .iter()
        .find_map(|source| match source {
            toolbox::descriptor::Source::Dir(path) => Some(path.clone()),
            _ => None,
        })
        .expect("una cartella dell'utente fra le sorgenti degli strumenti")
}

/// **Il percorso atteso è scritto per esteso, di proposito.** La prima versione
/// di questa prova confrontava soltanto la strada degli inneschi con quella
/// degli strumenti, ed è rimasta verde quando ho rimesso il difetto apposta:
/// due copie che sbagliano insieme sono d'accordo fra loro. Una prova che
/// confronta due cose che possono sbagliare nello stesso modo non prova niente;
/// serve un'ancora fuori da tutte e due.
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
