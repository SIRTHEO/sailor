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
use ledger::EngineIdentity;
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

    assert_eq!(
        equipment.env.get("RUST_LOG").map(String::as_str),
        Some("debug")
    );
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
    // **E NON È UN VUOTO: È «EREDITATA».** Il processo parte con la casa di chi
    // ha aperto il terminale, che è un'identità vera e nominabile.
    assert_eq!(
        equipment.identity,
        EngineIdentity::InheritedFromTheTerminal {
            cli_id: "claude".to_owned()
        }
    );
}

/// Un comando che non è una riga di comando conosciuta — un `sh` scritto a mano
/// in un passo — non ha nessuna casa da spostare, e non deve riceverne una.
#[test]
fn a_plain_command_gets_no_home_of_anyones() {
    let equipment = equipment_for(
        &a_store_with_one_active_profile(),
        "/bin/sh",
        &BTreeMap::new(),
    );

    assert!(equipment.env.is_empty(), "{:?}", equipment.env);
    assert_eq!(equipment.identity, EngineIdentity::NotAKnownEngine);
}

/// **IL PROFILO RISOLTO SI SCRIVE, O DUE CORSE NON SONO LA STESSA MISURA.**
///
/// Chi legge una riga del deposito e non sa sotto quale identità quella chiamata
/// è girata non può confrontarla con nessun'altra: la stessa catena di passi,
/// sotto due profili, dà due consumi diversi per una ragione che la riga non
/// porta. **E il percorso della casa ci sta dentro**: è il fondo su cui una
/// diagnostica si appoggia, mentre un nome si riusa e si sposta.
#[test]
fn the_resolved_profile_is_written_down_not_left_to_be_guessed() {
    let equipment = equipment_for(
        &a_store_with_one_active_profile(),
        "codex",
        &BTreeMap::new(),
    );

    assert_eq!(
        equipment.identity,
        EngineIdentity::ProfileInForce {
            cli_id: "codex".to_owned(),
            profile_name: "lavoro".to_owned(),
            home_dir: PathBuf::from("/case/codex/lavoro"),
        }
    );
}

/// **IL PASSO CHE SCAVALCA LO DICE, E QUESTA È LA CURA DEL DIFETTO.**
///
/// Fino al 01/09/2026 la riga nel deposito diceva `codex/lavoro` anche qui: il
/// motore partiva nella casa scritta nel passo e il registro nominava il profilo
/// attivo. **Il registro diceva un'identità e il processo ne aveva usata
/// un'altra**, proprio nel caso in cui qualcuno l'aveva cambiata apposta — cioè
/// quello che una diagnostica o un controllo di sicurezza esiste per vedere.
///
/// *Mutante eseguito*: togliere da `identity_of` il ramo che guarda `step_env`
/// per primo. Questa diventa rossa e le altre restano verdi.
#[test]
fn a_step_that_writes_the_home_variable_is_recorded_as_the_one_who_chose() {
    let equipment = equipment_for(
        &a_store_with_one_active_profile(),
        "codex",
        &step_env(&[("CODEX_HOME", "/una/casa/scritta/nel/passo")]),
    );

    assert_eq!(
        equipment.identity,
        EngineIdentity::ChosenByTheStep {
            cli_id: "codex".to_owned(),
            home_dir: PathBuf::from("/una/casa/scritta/nel/passo"),
        },
        "la riga direbbe un profilo che il processo non ha usato"
    );
}

/// **UN PROFILO DICHIARATO NON È UN PROFILO IN FORZA.** `antigravity` non ha una
/// variabile che sposti la casa: lì l'identità dipende da dove punta un file sul
/// disco, e questa funzione il disco non lo tocca. Prima usciva la stessa
/// stringa vuota di «nessun profilo», e i due casi si confondevano; adesso la
/// riga dice anche **perché**.
#[test]
fn a_cli_whose_home_no_variable_moves_says_so_with_its_reason() {
    let mut store = ProfileStore::default();
    store.profiles.push(Profile {
        name: "lavoro".to_owned(),
        cli_id: "antigravity".to_owned(),
        home_dir: PathBuf::from("/case/antigravity/lavoro"),
    });
    store
        .active
        .insert("antigravity".to_owned(), "lavoro".to_owned());

    let equipment = equipment_for(&store, "antigravity", &BTreeMap::new());

    assert!(equipment.env.is_empty(), "{:?}", equipment.env);
    let EngineIdentity::NotMovedByAnEnvVar {
        cli_id,
        profile_name,
        why,
    } = equipment.identity
    else {
        panic!("un profilo non messo in forza si legge come qualcos'altro");
    };
    assert_eq!(cli_id, "antigravity");
    assert_eq!(profile_name, "lavoro");
    assert!(!why.is_empty(), "manca la ragione, che è metà del dato");
}

/// **UNO STATO CHE NOMINA UN PROFILO SPARITO NON INVENTA UNA CASA.**
///
/// `sailor run` in questo caso si rifiuta di partire, e ha ragione: lì l'intera
/// invocazione è quel profilo. Qui il passo ha comunque un motore da chiamare, e
/// fermarlo per uno stato invecchiato punirebbe chi non c'entra — ma inventare
/// una cartella dal nome del profilo sarebbe peggio: si partirebbe con una casa
/// vuota, cioè senza credenziali, con l'aria di aver applicato un profilo. Non
/// si sovrappone niente — e il deposito lo **dice**, invece di tacerlo: fra i
/// cinque casi che finivano tutti nella stessa stringa vuota, questo è il solo
/// che chiede di intervenire, perché c'è uno stato da riparare.
#[test]
fn a_stale_active_name_that_matches_no_profile_moves_nothing() {
    let mut store = a_store_with_one_active_profile();
    store
        .active
        .insert("codex".to_owned(), "sparito".to_owned());

    let equipment = equipment_for(&store, "codex", &BTreeMap::new());

    assert!(equipment.env.is_empty(), "{:?}", equipment.env);
    assert_eq!(
        equipment.identity,
        EngineIdentity::ProfileVanished {
            cli_id: "codex".to_owned(),
            profile_name: "sparito".to_owned(),
        }
    );
}
