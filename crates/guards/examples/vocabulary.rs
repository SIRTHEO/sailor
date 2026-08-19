//! Quanto vede il gate della lingua: la misura, rifatta col codice del gate.
//!
//! La misura del 19/08/2026 (`docs/misura-vocabolario-lingua-2026-08-19/`) è
//! nata da uno strumento usa-e-getta che non è stato conservato, quindi i suoi
//! numeri non erano più riproducibili: allargare il vocabolario senza poter
//! rimisurare vuol dire dichiarare una copertura che nessuno ha contato.
//!
//! Chiama `declared_names` e `is_italian_name` del gancio vero, non una copia:
//! una seconda regex che estrae i nomi divergerebbe dalla prima al primo ramo
//! aggiunto, ed è già successo il 18/08 dentro `italian_names` stessa.
//!
//! Uso: `cargo run --release --example vocabulary -- [radice]`
//! Stampa su stdout un JSON coi nomi dichiarati e quelli riconosciuti.

use guards::code_language::{declared_names, family, is_exempt};
use guards::language::is_italian_name;
use std::collections::BTreeSet;
use std::path::Path;

/// Cartelle che non contano come codice proprio: dipendenze, roba compilata,
/// copie di sicurezza e plugin di terzi. I plugin da soli portavano 3.856
/// identificatori tutti inglesi, che diluivano il segnale fino a renderlo
/// illeggibile — è la separazione che fa la misura del 19/08.
const SKIPPED: &[&str] = &[
    "/node_modules/", "/target/", "/target-verifica/", "/.git/", "/plugins/",
    "/backups/", "/archive-commands/", "/archive-skills/", "/skills-quarantena/",
    "/cache/", "/file-history/", "/paste-cache/", "/projects/", "/downloads/",
];

/// Estensioni da leggere quando si salta il filtro delle famiglie.
const SOURCE_EXTENSIONS: &[&str] = &[".ts", ".tsx", ".js", ".mjs", ".py", ".rs", ".sh"];

fn walk(dir: &Path, names: &mut BTreeSet<String>, files: &mut usize, watched_only: bool) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let shown = format!("{}", path.display());
        if SKIPPED.iter().any(|s| shown.contains(s)) {
            continue;
        }
        let Ok(kind) = entry.file_type() else { continue };
        if kind.is_dir() {
            walk(&path, names, files, watched_only);
            continue;
        }
        if !kind.is_file() {
            continue;
        }
        let taken = if watched_only {
            family(&shown).is_some() && !is_exempt(&shown)
        } else {
            SOURCE_EXTENSIONS.iter().any(|e| shown.ends_with(e))
        };
        if !taken {
            continue;
        }
        let Ok(text) = std::fs::read_to_string(&path) else {
            continue;
        };
        *files += 1;
        names.extend(declared_names(&text));
    }
}

fn main() {
    // `--all` legge ogni sorgente invece dei soli file sorvegliati. Serve per il
    // corpus di controllo: `family()` accetta i test e le cartelle di gate di
    // casa, quindi su codice altrui — `typescript/lib`, `zod` — non prenderebbe
    // niente, e senza un corpus inglese non si misura il verso che conta
    // davvero, cioè quanti nomi inglesi il gate rimprovera a torto.
    let args: Vec<String> = std::env::args().skip(1).collect();
    let watched_only = !args.iter().any(|a| a == "--all");
    let root = args
        .iter()
        .find(|a| !a.starts_with("--"))
        .cloned()
        .unwrap_or_else(|| format!("{}/.claude", std::env::var("HOME").unwrap_or_default()));
    let mut names = BTreeSet::new();
    let mut files = 0usize;
    walk(Path::new(&root), &mut names, &mut files, watched_only);

    let recognised: Vec<&String> = names.iter().filter(|n| is_italian_name(n)).collect();
    let report = serde_json::json!({
        "root": root,
        "files": files,
        "declared": names.iter().collect::<Vec<_>>(),
        "recognised": recognised,
    });
    println!("{report}");
    eprintln!(
        "{files} watched files, {} distinct declared names, {} recognised as Italian",
        names.len(),
        recognised.len()
    );
}
