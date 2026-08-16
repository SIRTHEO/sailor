//! Equivalenza col gancio Python, su comandi realmente eseguiti.
//!
//! Il corpus lo genera `tools/build-corpus.py` dai registri della macchina:
//! 17.444 comandi unici, di cui si tengono tutti quelli su cui il Python prende
//! una decisione più tremila innocui. L'esito atteso è **quello del Python**,
//! perché è lo strumento che stiamo sostituendo: finché non lo cancelliamo,
//! l'oracolo è lui e non il giudizio di chi porta il codice.
//!
//! Una divergenza qui non è per forza un difetto del Rust — può essere il
//! Python che sbagliava. Ma va guardata una per una, mai sanata cambiando il
//! corpus.

use guards::cd_guard::judge;
use hook_io::Decision;

/// Il corpus non è versionato: sono comandi realmente eseguiti su questa
/// macchina, cioè dato di sessione. Chi lo trova mancante deve rigenerarlo, non
/// vedere un verde: un test che passa perché il suo materiale non c'è è
/// esattamente la forma di gate cieco che questa configurazione ha già trovato
/// quattro volte in un giorno.
const CORPUS: &str = "crates/guards/tests/corpus.jsonl";

#[test]
fn it_decides_exactly_like_the_python_hook_on_real_commands() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|p| p.parent())
        .expect("radice del workspace")
        .to_path_buf();
    let corpus = std::fs::read_to_string(root.join(CORPUS)).unwrap_or_else(|_| {
        panic!(
            "corpus assente ({CORPUS}). Rigeneralo:\n    \
             python3 rust/tools/build-corpus.py > rust/{CORPUS}"
        )
    });
    let mut checked = 0usize;
    let mut divergences = Vec::new();

    for line in corpus.lines().filter(|l| !l.trim().is_empty()) {
        let Some(command) = field(line, "command") else {
            continue;
        };
        let Some(expected) = field(line, "expected") else {
            continue;
        };
        let got = match judge(&command) {
            Decision::Pass => "pass",
            // i nomi vengono dallo script Python: `blocca` e `avvisa` sono le
            // sue etichette, e il corpus le porta così
            Decision::Warn(_) => "avvisa",
            Decision::Block(_) => "blocca",
            // `cd-guard` non nega permessi: qui sarebbe un difetto, e il
            // confronto col Python lo farebbe vedere come divergenza
            Decision::Deny(_) => "nega",
        };
        checked += 1;
        if got != expected {
            divergences.push(format!(
                "atteso {expected}, ottenuto {got}\n    comando: {:?}",
                truncate(&command, 160)
            ));
        }
    }

    assert!(checked > 3000, "corpus troppo piccolo: {checked} casi");
    assert!(
        divergences.is_empty(),
        "{} divergenze su {checked} casi:\n  {}",
        divergences.len(),
        divergences
            .iter()
            .take(10)
            .cloned()
            .collect::<Vec<_>>()
            .join("\n  ")
    );
}

/// Estrae un campo stringa dal JSON di una riga senza tirarsi dietro serde:
/// il corpus lo scriviamo noi, la forma è nota, e il test deve dipendere da
/// meno cose possibile di ciò che sta provando.
fn field(line: &str, name: &str) -> Option<String> {
    let key = format!("\"{name}\":");
    let start = line.find(&key)? + key.len();
    let rest = line[start..].trim_start();
    let mut chars = rest.chars();
    if chars.next()? != '"' {
        return None;
    }
    let mut out = String::new();
    let mut escaped = false;
    for c in chars {
        if escaped {
            out.push(match c {
                'n' => '\n',
                't' => '\t',
                'r' => '\r',
                other => other,
            });
            escaped = false;
        } else if c == '\\' {
            escaped = true;
        } else if c == '"' {
            return Some(out);
        } else {
            out.push(c);
        }
    }
    None
}

fn truncate(s: &str, n: usize) -> String {
    if s.chars().count() <= n {
        s.to_string()
    } else {
        s.chars().take(n).collect::<String>() + "…"
    }
}
