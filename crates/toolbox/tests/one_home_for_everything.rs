//! La casa di Sailor è una sola, e chi la cerca ci arriva dallo stesso posto.
//!
//! **Perché queste prove esistono.** Il 30/08/2026 la casa era in due posti: chi
//! cercava il deposito e il listino dei prezzi risolveva `~/.config/sailor`
//! (`ledger::sailor_home`), chi cercava i descrittori dell'utente risolveva
//! `~/.sailor` — perché quella seconda regola era una copia scritta a mano che
//! ignorava `XDG_CONFIG_HOME`. Le due case non si vedevano fra loro: un listino
//! messo dove la documentazione diceva non veniva letto da nessuno.
//!
//! Nessuna di queste prove tocca il disco: confrontano dove si andrebbe a
//! guardare, che è esattamente ciò che si era disallineato.

use std::collections::BTreeMap;
use std::path::PathBuf;
use toolbox::descriptor::Source;
use toolbox::probe::Machine;

/// Una macchina descritta, senza niente attorno: solo la casa e l'ambiente che
/// conta per questa domanda.
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

/// Dove `default_sources` è andata a cercare i descrittori dell'utente.
fn user_dir(sources: &[Source]) -> PathBuf {
    sources
        .iter()
        .find_map(|source| match source {
            Source::Dir(path) => Some(path.clone()),
            _ => None,
        })
        .expect("una cartella dell'utente fra le sorgenti")
}

#[test]
fn tools_and_the_ledger_resolve_the_same_home() {
    let machine = machine_with("/home/tizio", &[]);

    let from_tools = toolbox::sailor_home_for(&machine);
    let from_ledger = ledger::sailor_home_in(None, None, PathBuf::from("/home/tizio"));

    assert_eq!(
        from_tools, from_ledger,
        "i descrittori e il deposito devono cercare nella stessa casa"
    );
    assert_eq!(from_tools, PathBuf::from("/home/tizio/.config/sailor"));
}

#[test]
fn the_user_descriptors_live_under_that_same_home() {
    let machine = machine_with("/home/tizio", &[]);

    assert_eq!(
        user_dir(&toolbox::default_sources(&machine)),
        PathBuf::from("/home/tizio/.config/sailor/tools.d"),
        "i descrittori degli strumenti stanno nella casa, non accanto"
    );
}

/// Il caso che il difetto sbagliava: `XDG_CONFIG_HOME` era letto dal deposito e
/// ignorato dai descrittori. Chi lo dichiara — ed è chi tiene la configurazione
/// fuori da `~/.config` — vedeva le due metà separarsi.
#[test]
fn a_declared_config_home_moves_the_descriptors_too() {
    let machine = machine_with("/home/tizio", &[("XDG_CONFIG_HOME", "/altrove/conf")]);

    assert_eq!(
        toolbox::sailor_home_for(&machine),
        PathBuf::from("/altrove/conf/sailor")
    );
    assert_eq!(
        user_dir(&toolbox::default_sources(&machine)),
        PathBuf::from("/altrove/conf/sailor/tools.d")
    );
}

/// `SAILOR_HOME` resta la parola definitiva, e vale per tutti allo stesso modo.
#[test]
fn a_declared_sailor_home_wins_over_everything_for_everyone() {
    let machine = machine_with(
        "/home/tizio",
        &[
            ("SAILOR_HOME", "/casa/dichiarata"),
            ("XDG_CONFIG_HOME", "/altrove/conf"),
        ],
    );

    assert_eq!(
        toolbox::sailor_home_for(&machine),
        PathBuf::from("/casa/dichiarata")
    );
    assert_eq!(
        user_dir(&toolbox::default_sources(&machine)),
        PathBuf::from("/casa/dichiarata/tools.d")
    );
}
