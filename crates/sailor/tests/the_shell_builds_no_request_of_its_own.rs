//! Il guscio della finestra non si costruisce da sé la richiesta di una corsa.
//!
//! **PERCHÉ QUESTA PROVA È ROZZA, E PERCHÉ È COMUNQUE MEGLIO DI NIENTE.**
//! Cerca un pezzo di testo dentro un file `.rs`. Non compila quel file, non ne
//! legge l'albero sintattico, e un `ExecutionRequest` costruito con un nome
//! diverso le sfugge. La ragione è che `desktop/` **sta fuori dal workspace
//! Rust**: nessun `cargo test` lo compila, quindi lì dentro non esiste nessun
//! controllo che possa diventare rosso — e infatti è il posto in cui la lista
//! delle azioni è divergisa tre volte senza che nessuna prova lo vedesse
//! (guasto 10).
//!
//! Il costo di una prova sbagliata qui è basso — si legge il file e si capisce
//! —; il costo di non averne nessuna è misurato: tre divergenze silenziose, e
//! l'ultima ha dato al terminale e alla finestra due comportamenti diversi per
//! lo stesso file di flusso.
//!
//! **Se un giorno `desktop/` entra nel workspace, questa prova va buttata** e
//! sostituita da quella vera: il compilatore che vede una sola `execution_request`.

use std::path::PathBuf;

fn repository_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|crates| crates.parent())
        .expect("il crate sta in <radice>/crates/sailor")
        .to_path_buf()
}

/// **UNA SOLA `ExecutionRequest` IN TUTTO L'ALBERO, E STA IN `registry`.**
/// La radice del progetto è il dato che avrebbe fatto divergere le due copie
/// nel modo peggiore: una corsa lanciata dal pulsante che lavora dove sta il
/// processo, mentre la stessa corsa dal terminale lavora nella radice giusta —
/// e nessuna delle due lo direbbe.
#[test]
fn the_window_shell_does_not_build_its_own_execution_request() {
    let path = repository_root().join("desktop/src-tauri/src/run.rs");
    let text = std::fs::read_to_string(&path)
        .unwrap_or_else(|error| panic!("leggere {}: {error}", path.display()));

    assert!(
        !text.contains("ExecutionRequest {"),
        "{} si costruisce la richiesta da sé: la costruisce «registry::execution_request», \
         o le due copie tornano a divergere come nel guasto 10",
        path.display()
    );
}

/// E la chiama davvero: senza questa riga la prova sopra resterebbe verde anche
/// se il guscio smettesse di lanciare del tutto.
#[test]
fn the_window_shell_calls_the_shared_constructor() {
    let path = repository_root().join("desktop/src-tauri/src/run.rs");
    let text = std::fs::read_to_string(&path)
        .unwrap_or_else(|error| panic!("leggere {}: {error}", path.display()));

    assert!(
        text.contains("registry::execution_request("),
        "{} deve chiedere la richiesta a «registry»",
        path.display()
    );
}
