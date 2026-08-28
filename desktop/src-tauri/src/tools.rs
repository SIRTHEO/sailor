//! Cosa c'è su questa macchina, come lo chiede la finestra.
//!
//! IL RILEVAMENTO NON VIVE QUI. Sta in `toolbox`, dove l'elenco di cosa cercare
//! è un dato e non codice: questo modulo lo invoca e ne traduce l'esito nella
//! forma che la tela si aspetta. Se un giorno la finestra volesse sapere
//! qualcosa in più, la risposta è aggiungere un descrittore, non un ramo qui.
//!
//! PERCHÉ UNA TRADUZIONE E NON L'ESITO GREZZO. Il rilevatore distingue tre
//! stati — c'è, non c'è, non ho potuto guardare — e li porta col motivo. Alla
//! tela serve sapere se può usarlo (`available`), ma il motivo non si butta: chi
//! vede uno strumento assente deve poter leggere perché, altrimenti l'unica cosa
//! che può fare è non fidarsi dell'elenco.

use serde::Serialize;
use toolbox::{Presence, VersionReading};

/// Uno strumento come lo riceve la tela. I nomi dei campi sono il contratto
/// scritto in `desktop/src/tools.ts`: chi cambia l'uno cambia l'altro.
#[derive(Serialize)]
pub(crate) struct Tool {
    id: String,
    name: String,
    /// `ai_cli` | `mcp` | `tool`, o qualunque famiglia un descrittore dichiari:
    /// la tela tratta il tipo come aperto, e una famiglia nuova si mostra col
    /// suo nome invece di far sparire lo strumento.
    kind: String,
    path: Option<String>,
    version: Option<String>,
    available: bool,
    /// Perché è così: presente da dove, assente perché, o non verificabile
    /// perché. Senza questo un elenco non si può correggere.
    reason: String,
    /// Da quale descrittore è stato riconosciuto — l'indirizzo per chiedere
    /// conto di una riga sbagliata.
    descriptor: String,
}

/// Gli strumenti che questa macchina offre.
///
/// **Si rileva a ogni richiesta**, non una volta all'avvio: chi installa una CLI
/// mentre la finestra è aperta deve poterla usare senza riavviare, e il costo è
/// qualche processo interrogato sulla propria versione.
#[tauri::command]
pub(crate) fn discover_tools() -> Vec<Tool> {
    let machine = toolbox::Machine::current();
    let catalog = toolbox::Catalog::load(&toolbox::default_sources(&machine));
    toolbox::detect(&catalog, &machine)
        .findings
        .into_iter()
        .map(|found| Tool {
            id: found.name.clone(),
            name: if found.label.is_empty() {
                found.name.clone()
            } else {
                found.label
            },
            kind: found.family,
            path: found.executable,
            // Una versione non ottenuta resta assente, non diventa una stringa
            // che sembra un numero. «Non l'ho chiesta» e «l'ho chiesta e non ha
            // risposto» sono due cose diverse dal punto di vista di chi indaga,
            // ma per la tela sono la stessa: non c'è una versione da mostrare.
            version: match found.version {
                VersionReading::Declared(text) => Some(text),
                VersionReading::NotAsked(_) | VersionReading::Unavailable(_) => None,
            },
            available: found.presence.is_present(),
            reason: match &found.presence {
                Presence::Present(why) => why.clone(),
                Presence::Absent(why) => why.clone(),
                Presence::Undetermined(why) => why.clone(),
            },
            descriptor: found.descriptor_id,
        })
        .collect()
}
