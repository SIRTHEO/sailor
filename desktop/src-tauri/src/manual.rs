//! Il manuale della riga di comando, letto dal binario invece che ricopiato.
//!
//! **PERCHÉ QUESTO FILE NON CONTIENE NESSUN COMANDO.** Sailor ha dieci comandi
//! e una trentina di forme; scriverli in TypeScript sarebbe stata mezz'ora di
//! lavoro e una pagina che diverge dal binario alla prima opzione aggiunta. È
//! il guasto 10 — la stessa verità in più posti — che in questo repo si è già
//! ripresentato cinque volte, l'ultima il 01/09/2026 sul vocabolario delle
//! azioni, dove la finestra offriva sei nomi che il motore non conosceva e ne
//! rifiutava cinque che eseguiva.
//!
//! Quindi `crates/sailor` è diventato lib+bin, espone `COMMANDS`, e qui si
//! traduce soltanto la forma: da `&'static str` a JSON. Se un comando nasce, o
//! cambia una riga d'uso, questa pagina lo dice senza che nessuno la tocchi —
//! e se un comando sparisce, sparisce anche da qui.

use serde::Serialize;

/// Un comando come lo legge la finestra: senza il puntatore alla funzione, che
/// è l'unica cosa in `sailor::Command` che non attraversa il ponte.
#[derive(Serialize)]
pub struct CommandDoc {
    /// Il nome che si digita: `flow`, `step`, `release`.
    pub name: &'static str,
    /// Una riga che dice a cosa serve. È la stessa che stampa `sailor --help`.
    pub description: &'static str,
    /// Le forme complete, una per riga, già pronte da mostrare in colonna.
    pub usage: &'static [&'static str],
}

/// I comandi che questo Sailor sa eseguire, nell'ordine in cui li elenca il
/// binario. L'ordine non è alfabetico ed è voluto: è quello della tabella, che
/// mette per primi i comandi che si usano davvero ogni giorno.
#[tauri::command]
pub(crate) fn manual() -> Vec<CommandDoc> {
    sailor::COMMANDS
        .iter()
        .map(|command| CommandDoc {
            name: command.name,
            description: command.description,
            usage: command.usage,
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// **LA PAGINA NON PUÒ ESSERE PIÙ CORTA DEL BINARIO.**
    ///
    /// La garanzia forte è di costruzione — questa funzione mappa `COMMANDS` e
    /// non ha nessun posto dove scrivere un nome — ma un `filter` aggiunto un
    /// giorno per «nascondere i comandi interni» la romperebbe in silenzio, e
    /// la finestra mostrerebbe un manuale incompleto senza dirlo.
    #[test]
    fn the_manual_carries_every_command_the_binary_has() {
        let manual = manual();
        assert_eq!(
            manual.len(),
            sailor::COMMANDS.len(),
            "il manuale della finestra ha {} comandi, il binario ne ha {}",
            manual.len(),
            sailor::COMMANDS.len()
        );
        for (page, command) in manual.iter().zip(sailor::COMMANDS) {
            assert_eq!(page.name, command.name);
            assert!(
                !page.usage.is_empty(),
                "'{}' arriva alla finestra senza dire come si scrive",
                page.name
            );
        }
    }

    /// Ciò che attraversa il ponte è JSON, e un campo che non si serializza si
    /// scopre qui invece che davanti a una pagina vuota.
    #[test]
    fn the_manual_crosses_the_bridge_as_json() {
        let json = serde_json::to_string(&manual()).expect("il manuale si serializza");
        assert!(json.contains("\"flow\""), "manca il comando dei flussi");
        assert!(
            json.contains("sailor flow run <nome> [mandato]"),
            "manca una forma d'uso: {json}"
        );
    }
}
