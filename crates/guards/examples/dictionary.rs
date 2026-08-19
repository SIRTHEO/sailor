//! Quante parole inglesi il gate della lingua scambia per italiane.
//!
//! È la misura che manca a `vocabulary.rs`: quello conta quanto italiano il
//! gate **vede**, questo quanto inglese **rimprovera**. Servono tutte e due,
//! perché allargare l'elenco delle radici migliora il primo numero e peggiora
//! il secondo, e chi guarda solo il primo continua ad allargare. È esattamente
//! come sono nati i 1.564 falsi positivi del 19/08/2026
//! (`docs/2026-08-19-gate-lingua-falsi-positivi.md`).
//!
//! Il corpus è il dizionario inglese di sistema (`/usr/share/dict/words`, il
//! web2 di Webster, edizione 1913).
//!
//! **Come si legge il numero.** Non ogni riconoscimento è un rimprovero a
//! torto: il web2 contiene come voci proprie una ventina di parole italiane
//! brevi — `con`, `che`, `tra`, `tutti`, `non`, `col` — che l'elenco curato di
//! `italian_words()` rivendica di proposito, e sono italiane davvero. Quelle
//! restano, e sono il residuo atteso. Il numero da guardare è **quanto si
//! discosta da lì**: il 19/08/2026 erano 1.574 prima del filtro e 20 dopo, e
//! quei 20 sono esattamente le parole-funzione curate.
//!
//! Uso: `cargo run --release --example dictionary -- [file]`

use guards::language::is_italian_name;

fn main() {
    let path = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "/usr/share/dict/words".to_string());
    let Ok(text) = std::fs::read_to_string(&path) else {
        eprintln!("dictionary not readable: {path}");
        std::process::exit(1);
    };
    let mut words: Vec<&str> = text
        .lines()
        .map(str::trim)
        .filter(|w| !w.is_empty())
        .collect();
    words.sort_unstable();
    words.dedup();

    let wrong: Vec<&&str> = words.iter().filter(|w| is_italian_name(w)).collect();
    println!(
        "{}",
        serde_json::json!({
            "dictionary": path,
            "words": words.len(),
            "judged_italian": wrong.len(),
            "sample": wrong.iter().take(60).collect::<Vec<_>>(),
        })
    );
}
