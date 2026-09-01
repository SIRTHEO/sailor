//! **UN SOLO POSTO SA DOVE STA IL DEPOSITO.**
//!
//! `ledger::default_directory()` è quel posto: legge `SAILOR_LEDGER` se c'è,
//! altrimenti riconosce la casa di chi c'era prima e infine sceglie quella
//! nuova. Chi ricompone quel percorso a mano non fa una copia inerte — ne fa una
//! **diversa**, perché la copia scritta a mano non guarda `SAILOR_LEDGER`.
//!
//! **IL DANNO NON È UN DISALLINEAMENTO, È UNA DIVERGENZA SILENZIOSA.** Trovato
//! il 01/09/2026: `sailor inventory` componeva `~/.claude/state/flussi` da sé,
//! quindi con `SAILOR_LEDGER` impostato scriveva il censimento in un deposito e
//! ogni altro comando lo leggeva da un altro. Nessun errore: due depositi, e
//! quello che si guarda risulta vuoto. È la forma in cui il guasto 12 si
//! ripresenta — un elenco vuoto che ha l'aria di una risposta.
//!
//! **L'ANCORA STA FUORI DA TUTTE E DUE LE COPIE**, ed è il motivo per cui questa
//! prova legge i sorgenti invece di confrontare due funzioni: due copie che
//! sbagliano insieme si confermano a vicenda. Qui si guarda il **fatto** — che
//! nessuno, fuori da `crates/ledger`, nomini i pezzi di quel percorso.
//!
//! Il commento in `crates/ledger/src/lib.rs` dichiarava di aver unificato la
//! scoperta della casa il 28/08/2026, «prima che avesse una gemella». Ce l'aveva
//! già. Una dichiarazione di unicità che nessuna prova sorveglia invecchia senza
//! che nessuno se ne accorga.

use std::path::{Path, PathBuf};

fn repository_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|crates| crates.parent())
        .expect("il crate sta in <radice>/crates/sailor")
        .to_path_buf()
}

/// Il solo posto autorizzato a comporre il percorso, e le prove che lo
/// verificano.
fn is_allowed(path: &Path) -> bool {
    let shown = path.to_string_lossy().replace('\\', "/");
    shown.contains("/crates/ledger/")
        // Questa prova stessa nomina i pezzi per poterli cercare.
        || shown.ends_with("only_the_ledger_knows_where_the_ledger_lives.rs")
        // Il gate della lingua tiene un vocabolario, non un percorso.
        || shown.ends_with("identifiers_are_in_english.rs")
}

fn sources_under(directory: &Path, found: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(directory) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            // `target/` è materiale generato: guardarci dentro vorrebbe dire
            // leggere le stesse righe due volte, e in una copia che non si
            // corregge.
            if path.file_name().is_some_and(|name| name == "target") {
                continue;
            }
            sources_under(&path, found);
        } else if path.extension().is_some_and(|kind| kind == "rs") {
            found.push(path);
        }
    }
}

/// **NESSUNO RICOMPONE A MANO IL PERCORSO DEL DEPOSITO.**
///
/// *Mutante che rimette il difetto originale*: far tornare `open_ledger` in
/// `crates/sailor/src/inventory_cmd.rs` a comporre `HOME/.claude/state/flussi`.
/// Questa prova torna rossa nominando file e riga.
#[test]
fn nobody_outside_the_ledger_builds_the_ledger_path_by_hand() {
    let root = repository_root();
    let mut sources = Vec::new();
    sources_under(&root.join("crates"), &mut sources);
    sources_under(&root.join("desktop").join("src-tauri").join("src"), &mut sources);
    assert!(
        sources.len() > 50,
        "la scansione non ha trovato quasi niente: {} file, e allora questa prova \
         non sta guardando l'albero",
        sources.len()
    );

    let mut guilty = Vec::new();
    for path in &sources {
        if is_allowed(path) {
            continue;
        }
        let Ok(text) = std::fs::read_to_string(path) else {
            continue;
        };
        for (number, line) in text.lines().enumerate() {
            // **SI CERCA IL GESTO, NON LA PAROLA**, e il primo tentativo di
            // questa prova sbagliava proprio qui: cercare `flussi` da solo
            // prendeva `count(flows_seen, "flusso", "flussi")`, che è una
            // pluralizzazione; cercare `.claude` con `state` prendeva il file
            // dei modelli e quello dei profili, che sono verità **diverse** e
            // hanno il diritto di avere ognuna la propria casa. L'unica cosa che
            // vuol dire «sto ricomponendo il percorso del deposito» è nominare
            // il suo ultimo pezzo mentre si costruisce un percorso.
            let trimmed = line.trim_start();
            if trimmed.starts_with("//") {
                continue;
            }
            if line.contains(".join(\"flussi\")") || line.contains(".claude/state/flussi") {
                guilty.push(format!(
                    "{}:{}: {}",
                    path.strip_prefix(&root).unwrap_or(path).display(),
                    number + 1,
                    line.trim()
                ));
            }
        }
    }

    assert!(
        guilty.is_empty(),
        "qualcuno ricompone a mano il percorso del deposito invece di chiedere a \
         `ledger::default_directory()`, che è l'unico posto che guarda anche \
         `SAILOR_LEDGER`. Con quella variabile impostata, chi scrive qui e chi \
         legge altrove lavorano su due depositi diversi, senza nessun errore:\n{}",
        guilty.join("\n")
    );
}
