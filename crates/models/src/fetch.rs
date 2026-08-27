//! Scarica il catalogo vero, via `curl` come processo — la stessa strada di
//! `notte` verso OpenRouter (`crates/notte/src/main.rs:563`). Nessuna
//! autenticazione richiesta: niente chiave qui dentro.
//!
//! Nessuna prova in questo file: la rete vera in una prova è rossa quando
//! cade la linea, non quando sbaglia il codice — come per
//! `notte::fetch_openrouter_body`.

use std::process::Command;

const CATALOG_URL: &str = "https://openrouter.ai/api/v1/models";

/// Scarica il corpo JSON del catalogo. `MODELS_CATALOG_FETCH_OVERRIDE`, se
/// presente, sostituisce `curl` con un comando qualsiasi: serve a chi vuole
/// dare in pasto un catalogo fisso senza toccare la rete, come
/// `NOTTE_OPENROUTER_FETCH` fa già per `notte`.
pub fn fetch_catalog_body() -> String {
    if let Ok(cmd) = std::env::var("MODELS_CATALOG_FETCH_OVERRIDE") {
        return Command::new(cmd)
            .output()
            .ok()
            .and_then(|o| String::from_utf8(o.stdout).ok())
            .unwrap_or_default();
    }
    Command::new("curl")
        .args(["-sS", "-m", "30", CATALOG_URL])
        .output()
        .ok()
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .unwrap_or_default()
}
