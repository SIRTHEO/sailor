//! Ogni bersaglio di `sailor release` nomina un binario che il workspace
//! costruisce davvero.
//!
//! **PERCHÉ ESISTE.** Il 01/09/2026 `release::TARGETS` portava tre voci e due
//! erano fossili: `notte` voleva il binario `notte`, `hooks` voleva
//! `claude-hooks`, e nessuno dei due esisteva più — i crate che li producevano
//! erano stati cancellati col mondo che servivano. `sailor release notte` e
//! `sailor release hooks` non potevano costruire niente, e lo scoprivano al
//! momento peggiore: dentro un clone di `HEAD`, dopo aver già compilato. Una
//! tabella che nomina un binario inesistente non è rotta per il compilatore —
//! `bin` è una stringa — quindi finché nessuno lo interroga resta verde per
//! sempre.
//!
//! **L'ELENCO DEI BINARI SI CHIEDE A CARGO, NON SI RICOPIA QUI.** Due liste
//! scritte a mano si confermano a vicenda anche quando sbagliano insieme: è il
//! guasto 19, e in questa casa è già successo più volte. `cargo metadata
//! --no-deps` è l'unico posto che sa davvero quali binari escono da questo
//! workspace, e non risolve nessuna dipendenza — quindi non serve la rete.
//!
//! **PERCHÉ QUI E NON IN `desktop/src-tauri`.** Quel pacchetto dichiara un
//! `[workspace]` vuoto: `cargo test --workspace` non lo compila, e una prova
//! scritta là dentro non diventa mai rossa per nessuno. `crates/sailor` è nel
//! workspace e dipende da `release`, quindi il gate esegue questa.

use std::path::PathBuf;
use std::process::Command;

use serde_json::Value;

fn repository_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|crates| crates.parent())
        .expect("il crate sta in <radice>/crates/sailor")
        .to_path_buf()
}

/// I binari che questo workspace produce, chiesti a chi li costruisce.
///
/// `--no-deps` limita la risposta ai membri del workspace e salta la
/// risoluzione delle dipendenze: è la domanda più stretta che risponda alla
/// nostra, e non tocca la rete.
fn workspace_binaries() -> Vec<String> {
    let cargo = option_env!("CARGO").unwrap_or("cargo");
    let output = Command::new(cargo)
        .current_dir(repository_root())
        .args(["metadata", "--no-deps", "--format-version", "1"])
        .output()
        .unwrap_or_else(|error| panic!("non posso chiedere a cargo i suoi binari: {error}"));
    assert!(
        output.status.success(),
        "`cargo metadata` è fallito, quindi questa prova non ha guardato niente: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let metadata: Value =
        serde_json::from_slice(&output.stdout).expect("`cargo metadata` non ha risposto in JSON");
    let packages = metadata["packages"]
        .as_array()
        .expect("`cargo metadata` risponde sempre con un elenco di pacchetti");

    let mut names: Vec<String> = Vec::new();
    for package in packages {
        for target in package["targets"].as_array().into_iter().flatten() {
            let is_binary = target["kind"]
                .as_array()
                .into_iter()
                .flatten()
                .any(|kind| kind == "bin");
            if is_binary {
                names.push(
                    target["name"]
                        .as_str()
                        .expect("un target di cargo ha sempre un nome")
                        .to_string(),
                );
            }
        }
    }
    names.sort();

    // Una risposta vuota vorrebbe dire che la domanda è cambiata sotto i piedi,
    // non che il workspace non ha binari: senza questa riga la prova passerebbe
    // per approvazione invece che per verifica — è il guasto 22.
    assert!(
        !names.is_empty(),
        "cargo non ha nominato nessun binario: la domanda non è più quella giusta, \
         e questa prova starebbe approvando qualunque tabella"
    );
    names
}

#[test]
fn every_release_target_names_a_binary_the_workspace_really_builds() {
    let binaries = workspace_binaries();

    for candidate in release::TARGETS {
        assert!(
            binaries.iter().any(|name| name == candidate.bin),
            "il bersaglio '{}' vuole costruire il binario '{}', che questo workspace non produce. \
             I binari veri, chiesti a cargo: {}. `sailor release {}` fallirebbe dopo aver già \
             clonato HEAD e avviato la compilazione",
            candidate.name,
            candidate.bin,
            binaries.join(", "),
            candidate.name
        );

        // La copia appena costruita si cerca dove cargo la scrive. Un bersaglio
        // che nomina il binario giusto e guarda nel posto sbagliato è lo stesso
        // fossile un gradino più in là, e nemmeno questo ha un compilatore che
        // lo veda.
        let where_cargo_writes = format!("target/release/{}", candidate.bin);
        assert_eq!(
            candidate.live_rel, where_cargo_writes,
            "il bersaglio '{}' cerca la copia appena costruita in '{}', ma cargo la scrive in '{}'",
            candidate.name, candidate.live_rel, where_cargo_writes
        );
    }
}
