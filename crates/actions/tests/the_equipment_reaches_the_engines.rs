//! **IL GUASTO 18, PROVATO DOVE VIVEVA.**
//!
//! La sovrapposizione d'ambiente che porta un motore nella casa di Sailor —
//! `CLAUDE_CONFIG_DIR`, `CODEX_HOME`, `GEMINI_CLI_HOME` — esisteva dal 27/08 e
//! la chiamava **un posto solo**: `sailor run`, cioè lo scambio rapido da
//! terminale. Un motore lanciato da un **passo di flusso** non ci passava mai:
//! ereditava l'ambiente di chi aveva aperto il terminale, e leggeva la casa del
//! vicino. Due corse dello stesso flusso, lanciate da due terminali diversi, non
//! erano la stessa misura — e niente lo diceva.
//!
//! **È LA STESSA MALATTIA DEL GUASTO 35**, e le due prove si leggono insieme:
//! Sailor aveva il dato in casa propria e non lo usava. Il listino c'era e non
//! viaggiava col prodotto; la dotazione c'era e non arrivava ai motori.

use actions::equipment_for;
use profiles::{Profile, ProfileStore};
use std::collections::BTreeMap;
use std::path::PathBuf;

/// Uno stato con un profilo attivo per `codex` e uno spento per `claude`: il
/// secondo serve a provare che non basta *esistere*, bisogna essere attivo.
fn a_store_with_one_active_profile() -> ProfileStore {
    let mut store = ProfileStore::default();
    store.profiles.push(Profile {
        name: "lavoro".to_owned(),
        cli_id: "codex".to_owned(),
        home_dir: PathBuf::from("/case/codex/lavoro"),
    });
    store.profiles.push(Profile {
        name: "riposo".to_owned(),
        cli_id: "claude".to_owned(),
        home_dir: PathBuf::from("/case/claude/riposo"),
    });
    store.active.insert("codex".to_owned(), "lavoro".to_owned());
    store
}

fn step_env(pairs: &[(&str, &str)]) -> BTreeMap<String, String> {
    pairs
        .iter()
        .map(|(key, value)| ((*key).to_owned(), (*value).to_owned()))
        .collect()
}

/// **LA PROVA CHE CHIUDE IL GUASTO 18.** Un passo che invoca `codex` deve
/// partire con la casa del profilo attivo, non con quella del terminale.
///
/// Rimetti `equipment_for` a restituire il solo `step_env` e questa diventa
/// rossa: è il difetto originale, non una sua imitazione.
#[test]
fn a_flow_step_launches_the_engine_inside_the_profiles_home() {
    let equipment = equipment_for(
        &a_store_with_one_active_profile(),
        "/opt/homebrew/bin/codex",
        &BTreeMap::new(),
    );

    assert_eq!(
        equipment.env.get("CODEX_HOME").map(String::as_str),
        Some("/case/codex/lavoro"),
        "il passo eredita ancora la casa di chi ha aperto il terminale"
    );
}

/// **CHI SCRIVE UNA VARIABILE NEL PASSO VINCE, E IL VERSO È LA DECISIONE.**
///
/// Una variabile scritta dentro un passo dice qualcosa di preciso su *quella*
/// chiamata — un profilo diverso per un passo solo, una casa usa-e-getta per una
/// prova — e non deve poter essere scavalcata da uno stato che vive altrove e
/// che quel passo non nomina. Il verso opposto renderebbe inerte, in silenzio,
/// una riga scritta apposta nel flusso.
///
/// *Mutante eseguito*: invertire l'ordine della sovrapposizione, cioè far
/// vincere il profilo. Questa prova diventa rossa e quella qui sopra resta
/// verde — che è precisamente perché servono tutte e due.
#[test]
fn what_the_step_declares_beats_the_profile_never_the_other_way_round() {
    let equipment = equipment_for(
        &a_store_with_one_active_profile(),
        "/opt/homebrew/bin/codex",
        &step_env(&[("CODEX_HOME", "/una/casa/scritta/nel/passo")]),
    );

    assert_eq!(
        equipment.env.get("CODEX_HOME").map(String::as_str),
        Some("/una/casa/scritta/nel/passo")
    );
}

/// Le variabili che il passo dichiara e che col profilo non c'entrano arrivano
/// intatte: la dotazione si **aggiunge**, non sostituisce ciò che c'era.
#[test]
fn the_rest_of_what_the_step_declares_arrives_untouched() {
    let equipment = equipment_for(
        &a_store_with_one_active_profile(),
        "codex",
        &step_env(&[("RUST_LOG", "debug")]),
    );

    assert_eq!(equipment.env.get("RUST_LOG").map(String::as_str), Some("debug"));
    assert_eq!(
        equipment.env.get("CODEX_HOME").map(String::as_str),
        Some("/case/codex/lavoro")
    );
}

/// **UN PROFILO CHE ESISTE MA NON È ATTIVO NON CAMBIA NIENTE.** `claude` ha un
/// profilo in tabella e nessuno l'ha acceso: sovrapporgli una casa vorrebbe dire
/// spostare l'identità di una riga di comando che nessuno ha chiesto di
/// spostare, e lo si scoprirebbe da un login perso.
#[test]
fn a_profile_that_exists_but_is_not_active_moves_nothing() {
    let equipment = equipment_for(
        &a_store_with_one_active_profile(),
        "/usr/local/bin/claude",
        &BTreeMap::new(),
    );

    assert!(equipment.env.is_empty(), "{:?}", equipment.env);
    assert!(equipment.profile.is_empty());
}

/// Un comando che non è una riga di comando conosciuta — un `sh` scritto a mano
/// in un passo — non ha nessuna casa da spostare, e non deve riceverne una.
#[test]
fn a_plain_command_gets_no_home_of_anyones() {
    let equipment = equipment_for(&a_store_with_one_active_profile(), "/bin/sh", &BTreeMap::new());

    assert!(equipment.env.is_empty(), "{:?}", equipment.env);
    assert!(equipment.profile.is_empty());
}

/// **IL PROFILO RISOLTO SI SCRIVE, O DUE CORSE NON SONO LA STESSA MISURA.**
///
/// Chi legge una riga del deposito e non sa sotto quale dotazione quella
/// chiamata è girata non può confrontarla con nessun'altra: la stessa catena di
/// passi, sotto due profili, dà due consumi diversi per una ragione che la riga
/// non porta.
#[test]
fn the_resolved_profile_is_written_down_not_left_to_be_guessed() {
    let equipment = equipment_for(&a_store_with_one_active_profile(), "codex", &BTreeMap::new());

    assert_eq!(equipment.profile, "codex/lavoro");
}

/// **UNO STATO CHE NOMINA UN PROFILO SPARITO NON INVENTA UNA CASA.**
///
/// `sailor run` in questo caso si rifiuta di partire, e ha ragione: lì l'intera
/// invocazione è quel profilo. Qui il passo ha comunque un motore da chiamare, e
/// fermarlo per uno stato invecchiato punirebbe chi non c'entra — ma inventare
/// una cartella dal nome del profilo sarebbe peggio: si partirebbe con una casa
/// vuota, cioè senza credenziali, con l'aria di aver applicato un profilo. Non
/// si sovrappone niente, e il deposito lo dice tacendo.
#[test]
fn a_stale_active_name_that_matches_no_profile_moves_nothing() {
    let mut store = a_store_with_one_active_profile();
    store.active.insert("codex".to_owned(), "sparito".to_owned());

    let equipment = equipment_for(&store, "codex", &BTreeMap::new());

    assert!(equipment.env.is_empty(), "{:?}", equipment.env);
    assert!(equipment.profile.is_empty());
}
