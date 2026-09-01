//! I commenti non soffocano il codice: due numeri che possono solo scendere.
//!
//! Non è gusto. L'indice semantico incorpora i commenti alla lettera — vedi la
//! regola in `AGENTS.md` — quindi un blocco lungo prende il posto del codice
//! nella risposta a una ricerca.

use std::path::{Path, PathBuf};

/// Il tetto per blocco. Sopra, è cronaca: va nel registro dei guasti o nel
/// commit, non qui.
const MAX_BLOCK: usize = 6;

/// Quanti blocchi sforano oggi. **Può solo scendere**: abbassarlo è la
/// riparazione, alzarlo va discusso e si vede nel diff.
const LONG_BLOCKS_TODAY: usize = 635;

/// Quanti commenti citano una data. Stessa regola: solo verso il basso.
const DATED_COMMENTS_TODAY: usize = 310;

fn sources() -> Vec<PathBuf> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("il crate sta due livelli sotto la radice")
        .join("crates");
    let mut found = Vec::new();
    walk(&root, &mut found);
    found
}

fn walk(dir: &Path, found: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let name = entry.file_name().to_string_lossy().into_owned();
        if path.is_dir() {
            if !matches!(name.as_str(), "target" | ".git") {
                walk(&path, found);
            }
        } else if name.ends_with(".rs") && name != "comments_do_not_crowd_out_the_code.rs" {
            found.push(path);
        }
    }
}

fn is_comment(line: &str) -> bool {
    line.trim_start().starts_with("//")
}

/// `31/08/2026` e simili. Una data in un commento è cronaca per definizione.
fn cites_a_date(line: &str) -> bool {
    let bytes: Vec<char> = line.chars().collect();
    bytes.windows(10).any(|w| {
        w[0].is_ascii_digit()
            && w[1].is_ascii_digit()
            && w[2] == '/'
            && w[3].is_ascii_digit()
            && w[4].is_ascii_digit()
            && w[5] == '/'
            && w[6..10].iter().all(char::is_ascii_digit)
    })
}

struct Counts {
    long_blocks: usize,
    dated: usize,
    worst: (usize, String),
}

fn count() -> Counts {
    let mut counts = Counts { long_blocks: 0, dated: 0, worst: (0, String::new()) };
    for path in sources() {
        let Ok(text) = std::fs::read_to_string(&path) else {
            continue;
        };
        let mut run = 0usize;
        for line in text.lines() {
            if is_comment(line) {
                run += 1;
                if cites_a_date(line) {
                    counts.dated += 1;
                }
                continue;
            }
            if run > MAX_BLOCK {
                counts.long_blocks += 1;
                if run > counts.worst.0 {
                    counts.worst = (run, path.display().to_string());
                }
            }
            run = 0;
        }
        if run > MAX_BLOCK {
            counts.long_blocks += 1;
        }
    }
    counts
}

#[test]
fn no_new_comment_block_runs_past_the_cap() {
    let counts = count();
    assert!(
        counts.long_blocks <= LONG_BLOCKS_TODAY,
        "blocchi sopra {MAX_BLOCK} righe: {} (il tetto dichiarato è {LONG_BLOCKS_TODAY}). \
         Il più lungo è di {} righe in {}. Accorcia, o sposta la cronaca nel commit",
        counts.long_blocks,
        counts.worst.0,
        counts.worst.1
    );
}

#[test]
fn no_new_comment_tells_a_date() {
    let counts = count();
    assert!(
        counts.dated <= DATED_COMMENTS_TODAY,
        "commenti che citano una data: {} (dichiarati {DATED_COMMENTS_TODAY}). \
         La data la conserva git, con l'autore vero",
        counts.dated
    );
}

/// **CHI MISURA VA MISURATO.** Se `is_comment` o `cites_a_date` smettessero di
/// vedere, i due numeri crollerebbero a zero e le prove resterebbero verdi per
/// sempre.
#[test]
fn the_check_can_still_see_what_it_counts() {
    assert!(is_comment("    // così"));
    assert!(is_comment("/// e così"));
    assert!(!is_comment("let x = 1; // non così: la riga è codice"));
    assert!(cites_a_date("// misurato il 31/08/2026"));
    assert!(!cites_a_date("// nessuna data qui"));
    let counts = count();
    assert!(counts.long_blocks > 0, "zero blocchi lunghi: il contatore non sta guardando");
    assert!(counts.dated > 0, "zero date: il contatore non sta guardando");
}
