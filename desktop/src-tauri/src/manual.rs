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
    /// Una riga che dice a cosa serve. È la stessa che stampa `sailor --help`,
    /// e come quella arriva dal catalogo: nella tabella dei comandi c'è la
    /// chiave, non la frase, così la finestra la mostra nella lingua di chi
    /// guarda invece che in quella di chi l'ha scritta.
    pub description: String,
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
            description: catalogue::say(command.description_key, &[]),
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
    ///
    /// **LA FORMA D'USO NON SI RICOPIA PIÙ A MANO, e la ragione è un guasto
    /// vero.** Fino all'01/09/2026 questa riga cercava
    /// `sailor flow run <nome> [mandato]`, scritto qui a mano. Quando la riga di
    /// comando è passata all'inglese la stringa è diventata falsa e la prova
    /// rossa — ma **nessuno l'ha vista**, perché `desktop/src-tauri` dichiara un
    /// `[workspace]` suo e `cargo test --workspace` non lo compila: le prove di
    /// questo guscio non stanno nel gate. Un rosso invisibile è peggio di un
    /// verde: chi ha tradotto ha creduto di aver finito.
    ///
    /// Adesso non cerca più nessuna frase: **conta**. Quello che questa prova
    /// deve difendere è che i campi attraversino il ponte, non quali parole
    /// contengano — le parole sono già custodite dove nascono, e chiederle a
    /// `COMMANDS` sarebbe confrontare una fonte con sé stessa, perché è da lì
    /// che `manual()` le prende. *Mutante*: `#[serde(skip)]` su `usage` — «il
    /// binario dichiara 34 forme d'uso e ne attraversano il ponte 0».
    #[test]
    fn the_manual_crosses_the_bridge_as_json() {
        let json = serde_json::to_string(&manual()).expect("il manuale si serializza");
        assert!(json.contains("\"flow\""), "manca il comando dei flussi");

        // **SI CONTA, NON SI CERCA UNA FRASE.** `manual()` nasce da `COMMANDS`,
        // quindi cercare una parola qui vorrebbe dire confrontare una fonte con
        // sé stessa: resterebbe verde comunque, e sarebbe la copia che si
        // conferma da sola. Ciò che questa prova può davvero perdere è **un
        // campo che non attraversa il ponte** — un `skip` di serde, un tipo che
        // non si serializza — e quello si vede contando: se `usage` non passa,
        // il conto cade a zero mentre il binario ne dichiara una trentina.
        let dichiarate: usize = sailor::COMMANDS
            .iter()
            .map(|command| command.usage.len())
            .sum();
        let arrivate = json.matches("sailor ").count();
        assert_eq!(
            arrivate, dichiarate,
            "il binario dichiara {dichiarate} forme d'uso e ne attraversano il ponte {arrivate}: {json}"
        );
        assert!(
            json.contains("\"description\"") && json.contains("\"usage\""),
            "un campo non attraversa il ponte: {json}"
        );
    }
}
