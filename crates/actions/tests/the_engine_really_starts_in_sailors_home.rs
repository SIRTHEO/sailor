//! Il motore parte **davvero** dentro la casa che il profilo dichiara.
//!
//! **PERCHÉ QUESTA PROVA ESISTE ACCANTO A `the_equipment_reaches_the_engines`.**
//! Quella prova la *regola* — quale ambiente si deve comporre — e resterebbe
//! verde con la regola scollegata: basta che `ExternalEngineAction` continui a
//! passare `spec.env` all'invocazione, ed è esattamente il guasto 18. Una regola
//! giusta che nessuno chiama è indistinguibile da una regola assente guardando
//! le prove. Qui si guarda l'unica cosa che non si può fingere: un processo
//! vero, avviato dal passo, che stampa la variabile che ha ricevuto.
//!
//! **UN SOLO `#[test]` IN QUESTO FILE, E NON È PIGRIZIA.** La prova deve
//! dichiarare `PROFILES_STATE_PATH`, che è di **processo**: `cargo test` manda
//! le prove di uno stesso binario su più fili dello stesso processo, e una
//! seconda prova qui dentro leggerebbe una variabile scritta da questa mentre
//! gira. Un file a sé è un processo a sé. I due casi che contano stanno quindi
//! in due bracci dello stesso corpo.

use flow::{Action, ActionOutcome, SharedState};
use serde_json::json;
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

/// Una cartella usa-e-getta sotto `$TMPDIR`, cancellata a fine prova. Nessuna
/// dipendenza esterna: lo stesso schema già usato altrove nell'albero.
struct TempDir(PathBuf);

impl TempDir {
    fn new() -> Self {
        let unique = format!(
            "actions-equipment-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("l'orologio non va all'indietro")
                .as_nanos()
        );
        let path = std::env::temp_dir().join(unique);
        fs::create_dir_all(&path).expect("cartella di prova");
        TempDir(path)
    }

    fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

/// Un finto `codex`: si chiama come l'eseguibile vero — che è il legame su cui
/// `profiles::cli_for_executable` lavora — e stampa la sola cosa che serve
/// sapere, cioè quale casa ha ricevuto.
fn a_fake_codex_that_prints_its_home(dir: &Path) -> String {
    let path = dir.join("codex");
    fs::write(&path, "#!/bin/sh\nprintf 'CASA=%s\\n' \"$CODEX_HOME\"\n")
        .expect("scrivere il finto motore");
    fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).expect("renderlo eseguibile");
    path.to_string_lossy().into_owned()
}

fn what_the_engine_said(outcome: &ActionOutcome) -> String {
    let ActionOutcome::Went(value) = outcome else {
        panic!("il passo doveva andare: {outcome:?}");
    };
    value
        .get("stdout")
        .and_then(serde_json::Value::as_str)
        .expect("l'uscita del motore")
        .to_owned()
}

/// **IL GUASTO 18 CONTRO UN PROCESSO VERO.**
///
/// Primo braccio: un passo che non dichiara niente deve far partire il motore
/// nella casa del profilo attivo. Prima del 01/09/2026 partiva con quella di chi
/// aveva aperto il terminale, e `CODEX_HOME` arrivava vuota.
///
/// Secondo braccio: un passo che dichiara la variabile vince. Serve tutto e due
/// insieme — il primo da solo resterebbe verde se il profilo scavalcasse il
/// passo, il secondo da solo resterebbe verde se il profilo non arrivasse mai.
///
/// *Mutante eseguito*: rimettere `env: spec.env.clone()` nell'invocazione. Il
/// primo braccio diventa rosso, il secondo resta verde.
#[test]
fn the_engine_really_starts_inside_the_home_the_profile_declares() {
    let dir = TempDir::new();
    let bin = a_fake_codex_that_prints_its_home(dir.path());
    let home_of_the_profile = dir.path().join("case").join("codex").join("lavoro");

    let state = dir.path().join("profili.json");
    fs::write(
        &state,
        json!({
            "profiles": [
                {"name": "lavoro", "cli_id": "codex", "home_dir": home_of_the_profile}
            ],
            "active": {"codex": "lavoro"}
        })
        .to_string(),
    )
    .expect("scrivere lo stato dei profili");
    std::env::set_var("PROFILES_STATE_PATH", &state);

    let action = actions::ExternalEngineAction::new();

    let mut shared = SharedState::new();
    let said = what_the_engine_said(
        &action
            .execute(&json!({"bin": bin, "timeout_secs": 30}), &mut shared)
            .expect("il passo doveva riuscire"),
    );
    assert!(
        said.contains(&format!("CASA={}", home_of_the_profile.display())),
        "il motore è partito con la casa di chi ha aperto il terminale: {said:?}"
    );

    let mut shared = SharedState::new();
    let said = what_the_engine_said(
        &action
            .execute(
                &json!({
                    "bin": bin,
                    "env": {"CODEX_HOME": "/una/casa/scritta/nel/passo"},
                    "timeout_secs": 30
                }),
                &mut shared,
            )
            .expect("il passo doveva riuscire"),
    );
    assert!(
        said.contains("CASA=/una/casa/scritta/nel/passo"),
        "il profilo ha scavalcato ciò che il passo dichiara: {said:?}"
    );
}
