//! Il passo di rilevamento non muore su un campo che non ha chiesto.
//!
//! GUASTO MISURATO IL 01/09/2026, e non su un flusso scritto male: su
//! `strumenti-di-questa-macchina`, spedito col prodotto. Dentro una cartella
//! con `sailor.json` falliva sempre —
//!
//!   unknown field `workdir`, expected one of `descriptor_paths`,
//!   `include_defaults`, `builtin_catalogs`, `family`, `version_probes`
//!
//! — e fuori da un progetto girava. La differenza non è nel flusso: è che
//! l'esecutore offre la radice del progetto a ogni passo il cui schema
//! dichiarato la accetterebbe, e `{"type": "any"}` accetta tutto. Con
//! `deny_unknown_fields` l'azione rifiutava chi la invoca.
//!
//! Le due prove qui sotto tengono ferme le due metà: il campo non fa più
//! cadere il passo, **e** non viene buttato via — un descrittore scritto
//! relativo si conta dalla radice, non da dove sta il processo.

use flow::{Action, ActionOutcome, SharedState};
use serde_json::json;
use std::fs;
use toolbox::DetectToolsAction;

fn a_folder(name: &str) -> std::path::PathBuf {
    let root = std::env::temp_dir().join(format!("toolbox-workdir-{name}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(&root).expect("la cartella di prova si crea");
    root
}

fn descriptor(inside: &std::path::Path, name: &str) -> std::path::PathBuf {
    let file = inside.join(name);
    fs::write(
        &file,
        json!({
            "tools": [{
                "id": "un-nome-che-nessuno-installa",
                "family": "tool",
                "label": "esiste solo in questa prova",
                "detect": [{ "command": "un-binario-che-non-esiste-di-sicuro" }]
            }]
        })
        .to_string(),
    )
    .expect("il descrittore si scrive");
    file
}

fn output(outcome: ActionOutcome) -> serde_json::Value {
    match outcome {
        ActionOutcome::Went(value) => value,
        other => panic!("il passo doveva andare, invece: {other:?}"),
    }
}

#[test]
fn il_workdir_non_fa_cadere_il_rilevamento() {
    let root = a_folder("cade");
    let file = descriptor(&root, "prova.json");

    let outcome = DetectToolsAction
        .execute(
            &json!({
                "descriptor_paths": [file.display().to_string()],
                "include_defaults": false,
                "version_probes": false,
                "workdir": root.display().to_string(),
            }),
            &SharedState::new(),
        )
        .expect("l'esecutore può aggiungere il workdir: non è un ingresso sbagliato");

    assert_eq!(output(outcome)["total"], 1, "il descrittore è stato letto");
}

/// **E IL CAMPO NON È UN POZZO.** Un percorso relativo si conta dalla radice
/// dichiarata. Senza questa prova, «accettalo e buttalo via» passerebbe uguale,
/// e un descrittore relativo si cercherebbe dove sta il processo — guasto 25.
#[test]
fn un_descrittore_relativo_si_conta_dal_workdir() {
    let root = a_folder("relativo");
    fs::create_dir_all(root.join("tools.d")).expect("sottocartella");
    descriptor(&root.join("tools.d"), "prova.json");

    let outcome = DetectToolsAction
        .execute(
            &json!({
                "descriptor_paths": ["tools.d/prova.json"],
                "include_defaults": false,
                "version_probes": false,
                "workdir": root.display().to_string(),
            }),
            &SharedState::new(),
        )
        .expect("il passo va");

    let output = output(outcome);
    assert_eq!(
        output["total"], 1,
        "letto dalla radice, non dal cwd: {}",
        output["problems"]
    );
}
