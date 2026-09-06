//! The detection step does not die on a field it never asked for.
//!
//! MEASURED FAULT, and not on a badly written flow: on
//! `what-this-machine-has`, shipped with the product. Inside a directory
//! holding a `sailor.json` it always failed —
//!
//!   unknown field `workdir`, expected one of `descriptor_paths`,
//!   `include_defaults`, `builtin_catalogs`, `family`, `version_probes`
//!
//! — and outside a project it ran. The difference is not in the flow: it is
//! that the executor offers the project root to every step whose declared
//! schema would accept it, and `{"type": "any"}` accepts anything. With
//! `deny_unknown_fields` the action refused its own caller.
//!
//! The two proofs below hold both halves: the field no longer brings the step
//! down, **and** it is not thrown away — a descriptor written relative counts
//! from the root, not from where the process sits.

use flow::{Action, ActionOutcome, SharedState};
use serde_json::json;
use std::fs;
use toolbox::{DetectToolsAction, Machine};

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

    let outcome = DetectToolsAction::on(Machine::bare(root.clone()))
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

/// **AND THE FIELD IS NOT A SINK.** A relative path counts from the declared
/// root. Without this proof "accept it and throw it away" would pass just the
/// same, and a relative descriptor be sought where the process sits — fault 25.
#[test]
fn un_descrittore_relativo_si_conta_dal_workdir() {
    let root = a_folder("relativo");
    fs::create_dir_all(root.join("tools.d")).expect("sottocartella");
    descriptor(&root.join("tools.d"), "prova.json");

    let outcome = DetectToolsAction::on(Machine::bare(root.clone()))
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
