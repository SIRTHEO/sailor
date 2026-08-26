//! Elenchi condivisi fra i freni che leggono un comando: chi riceve del codice
//! come argomento (`INTERPRETERS`) e chi passa il lavoro a un esecutore che i
//! ganci non raggiungono (`DELEGATES`).
//!
//! Nato da un buco misurato il 25/08/2026: `destructive_commands.rs` copriva
//! solo le shell POSIX (`bash`, `sh`, `zsh`, `dash`, `ksh`, `busybox`), quindi
//! `python3 -c "…rm -rf…"` non veniva mai riletto. `linear_readonly.rs` aveva
//! già l'elenco giusto (`python`, `node`, `perl`, `ruby`, …): qui i due freni lo
//! condividono da un punto solo, invece di divergere di nuovo alla prossima
//! correzione fatta su uno solo dei due. (Ricerca di riuso fatta con
//! `codebase_search` prima di scrivere questo file: nessun modulo condiviso
//! esisteva già, solo le due copie che questo file sostituisce.)

/// Chi esegue una stringa come codice: la stringa va guardata, non il verbo.
pub const INTERPRETERS: &[&str] = &[
    "bash", "sh", "zsh", "dash", "ksh", "fish", "busybox", "python", "python3", "node", "perl",
    "ruby", "osascript", "deno", "bun",
];

/// Chi passa il lavoro a un esecutore che i ganci di Claude non raggiungono.
///
/// Non si vietano: Codex serve a lavoro vero ed è già in uso legittimo. Il
/// gancio che li incontra li rende visibili nel registro invece di lasciarli
/// muti — è l'unica cosa che può fare, perché non legge cosa farà quel
/// processo.
pub const DELEGATES: &[&str] = &["codex", "gemini", "claude", "aider", "cursor-agent", "copilot"];
