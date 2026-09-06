//! **ONE PLACE KNOWS WHERE THE LEDGER LIVES.**
//!
//! `ledger::default_directory()` is that place: it reads `SAILOR_LEDGER` if it is
//! set, and otherwise picks Sailor's home. Rebuilding that path by hand makes no
//! inert copy — it makes a **different** one, because a hand-written copy does
//! not look at `SAILOR_LEDGER`.
//!
//! **THE HARM IS NOT A MISMATCH, IT IS A SILENT DIVERGENCE.** `sailor inventory`
//! once composed `~/.claude/state/flussi` itself, so with `SAILOR_LEDGER` set it
//! wrote the census into one store while every other command read another. No
//! error: two stores, and the one you look at comes up empty. It is the shape
//! fault 12 comes back in — an empty list with the air of an answer.
//!
//! **THE ANCHOR SITS OUTSIDE BOTH COPIES**, which is why this test reads the
//! sources instead of comparing two functions: two copies wrong together confirm
//! each other. What is watched is the **fact** — that nobody outside
//! `crates/ledger` names the pieces of that path. A claim of uniqueness no test
//! guards goes stale with nobody noticing.

use std::path::{Path, PathBuf};

/// The two ways of building the store's path by hand, which is the gesture
/// looked for: the last piece of that path named while a path is composed.
const BUILT_BY_HAND: &[&str] = &[".join(\"flussi\")", ".claude/state/flussi"];

fn repository_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|crates| crates.parent())
        .expect("il crate sta in <radice>/crates/sailor")
        .to_path_buf()
}

/// The one place allowed to compose the path, and the tests that check it.
fn is_allowed(path: &Path) -> bool {
    let shown = path.to_string_lossy().replace('\\', "/");
    shown.contains("/crates/ledger/")
        // This test itself names the pieces, so that it can look for them.
        || shown.ends_with("only_the_ledger_knows_where_the_ledger_lives.rs")
        // The language gate holds a vocabulary, not a path.
        || shown.ends_with("identifiers_are_in_english.rs")
}

fn sources_under(directory: &Path, found: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(directory) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            // `target/` is generated material: looking inside would read the
            // same lines twice, in a copy nobody repairs.
            if path.file_name().is_some_and(|name| name == "target") {
                continue;
            }
            sources_under(&path, found);
        } else if path.extension().is_some_and(|kind| kind == "rs") {
            found.push(path);
        }
    }
}

/// **NOBODY REBUILDS THE LEDGER PATH BY HAND.**
///
/// *The mutant that puts the original defect back*: make `open_ledger` in
/// `crates/sailor/src/inventory_cmd.rs` compose `HOME/.claude/state/flussi`
/// again. This test goes red naming the file and the line.
#[test]
fn nobody_outside_the_ledger_builds_the_ledger_path_by_hand() {
    let root = repository_root();
    let mut sources = Vec::new();
    sources_under(&root.join("crates"), &mut sources);
    sources_under(
        &root.join("desktop").join("src-tauri").join("src"),
        &mut sources,
    );
    assert!(
        sources.len() > 50,
        "la scansione non ha trovato quasi niente: {} file, e allora questa prova \
         non sta guardando l'albero",
        sources.len()
    );

    let mut guilty = Vec::new();
    let mut read = 0;
    for path in &sources {
        if is_allowed(path) {
            continue;
        }
        let Ok(text) = std::fs::read_to_string(path) else {
            continue;
        };
        read += 1;
        for (number, line) in text.lines().enumerate() {
            // **THE GESTURE IS SOUGHT, NOT THE WORD.** `flussi` alone catches
            // `count(flows_seen, "flusso", "flussi")`, a pluralisation; `.claude`
            // with `state` catches the models file and the profiles file, which
            // are **different** truths and each entitled to its own home. The one
            // thing that means "I am rebuilding the ledger path" is naming its
            // last piece while a path is being composed.
            let trimmed = line.trim_start();
            if trimmed.starts_with("//") {
                continue;
            }
            if BUILT_BY_HAND.iter().any(|gesture| line.contains(gesture)) {
                guilty.push(format!(
                    "{}:{}: {}",
                    path.strip_prefix(&root).unwrap_or(path).display(),
                    number + 1,
                    line.trim()
                ));
            }
        }
    }
    workspace::measured_against(
        read,
        "sources read outside the ledger",
        BUILT_BY_HAND.len(),
        "ways of building the store's path by hand",
    );

    assert!(
        guilty.is_empty(),
        "qualcuno ricompone a mano il percorso del deposito invece di chiedere a \
         `ledger::default_directory()`, che è l'unico posto che guarda anche \
         `SAILOR_LEDGER`. Con quella variabile impostata, chi scrive qui e chi \
         legge altrove lavorano su due depositi diversi, senza nessun errore:\n{}",
        guilty.join("\n")
    );
}
