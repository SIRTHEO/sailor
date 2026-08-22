//! Nega un messaggio fra sessioni che è un documento e non una domanda.
//!
//! PERCHÉ ESISTE, misurato il 22/08/2026 su 726 `SendMessage` in sette giorni:
//! lunghezza media 2.307 caratteri, mediana 2.201, massimo 10.149. Letti uno per
//! uno 40 a campione: il 75% era stato o consegna («ho finito», «prendi la
//! voce X», «tre cose per il foglio»), cioè contenuto che vive in un file che
//! chi riprende legge quando gli serve; la domanda vera, l'unico caso in cui un
//! messaggio è lo strumento giusto, era una su quaranta. La regola «un messaggio
//! sta in dieci righe» era già scritta nel mandato del macchinista, in prosa, e
//! non ha spostato la mediana. Un lavoro del 2026 su Claude Code (arXiv
//! 2608.16801, 1902 esecuzioni) misura la stessa cosa dal lato del costo:
//! sostituire i messaggi con un file condiviso taglia il 42% dei token in
//! uscita a otto agenti.
//!
//! LA REGOLA È SULLA FORMA, NON SUL CONTENUTO: sopra `LIMIT` caratteri e senza
//! una domanda dentro, il messaggio è un documento spedito per posta. Il
//! diniego dice dove va il documento e com'è fatto il puntatore che lo
//! sostituisce — un gancio che nega senza offrire il gesto sostitutivo viene
//! scavalcato (misurato su `cd-guard`: 80% di ripetizioni dove avvisava senza
//! alternativa, 48% dove ne dava una).
//!
//! UNA DOMANDA PASSA A QUALUNQUE LUNGHEZZA. È una maglia larga per scelta: chi
//! vuole aggirare scrive un punto interrogativo. Si misura — la quota di stato e
//! consegna che passa ancora per messaggio, oggi 75% — e si stringe solo se il
//! numero non scende.
//!
//! Valvola: `MESSAGE_BUDGET=off|avvisa`.

use hook_io::Decision;

/// Sopra questa misura un messaggio senza domanda è un documento.
///
/// 600 e non la mediana: il puntatore che il diniego chiede (voce, esito,
/// commit, prossimo passo, percorso) sta in 200-400 caratteri, e il più corto
/// dei 726 misurati era di 271. Sotto i 600 si passa senza guardare il testo.
pub const LIMIT: usize = 600;

/// Il messaggio chiede qualcosa a chi lo riceve?
///
/// Un punto interrogativo basta. Non si cerca di capire se la domanda è vera:
/// sarebbe un giudizio sul contenuto, e questo gancio non ne dà.
fn asks_something(message: &str) -> bool {
    message.contains('?')
}

pub fn judge(message: &str) -> Decision {
    let chars = message.chars().count();
    if chars <= LIMIT {
        return Decision::Pass;
    }
    if asks_something(message) {
        return Decision::Pass;
    }
    Decision::Deny(format!(
        "messaggio di {chars} caratteri senza una domanda: è stato o consegna, non un \
         messaggio, e chi lo riceve lo paga in contesto a ogni turno.\n  \
         · scrivi il dettaglio su disco: la tua consegna, una voce della coda \
         (`~/.claude/state/plancia/segnalazioni/`), o un file sotto `docs/`\n  \
         · poi manda il puntatore, sotto i {LIMIT} caratteri:  \
         voce: … · esito: … · commit: … · prossimo: … · dettaglio: <percorso>\n  \
         Una domanda vera (col suo punto interrogativo) passa a qualunque lunghezza."
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn long(n: usize) -> String {
        "a".repeat(n)
    }

    /// Sotto il limite si passa senza guardare il testo.
    #[test]
    fn a_short_message_passes_whatever_it_says() {
        assert_eq!(judge(""), Decision::Pass);
        assert_eq!(judge(&long(LIMIT)), Decision::Pass);
    }

    /// Il caso che il gancio esiste per fermare: un documento senza domanda.
    ///
    /// MUTANTE: tolto il ramo `asks_something`, questo resta verde e il caso
    /// sotto va in rosso; tolto il confronto con `LIMIT`, va in rosso il primo.
    #[test]
    fn a_long_message_without_a_question_is_denied() {
        match judge(&long(LIMIT + 1)) {
            Decision::Deny(m) => {
                assert!(m.contains("601 caratteri"), "{m}");
                assert!(m.contains("voce:"), "manca il puntatore sostitutivo: {m}");
                assert!(
                    m.contains("segnalazioni"),
                    "manca dove va il documento: {m}"
                );
            }
            other => panic!("atteso un diniego, non {other:?}"),
        }
    }

    /// Una domanda passa a qualunque lunghezza, per scelta dichiarata.
    #[test]
    fn a_long_message_that_asks_something_passes() {
        let m = format!(
            "{} questo gesto resta nostro o passa a Theo?",
            long(LIMIT + 500)
        );
        assert_eq!(judge(&m), Decision::Pass);
    }

    /// Si contano i caratteri, non i byte: un testo italiano con accenti non
    /// deve essere negato prima di uno senza.
    #[test]
    fn the_limit_counts_characters_not_bytes() {
        let m = "è".repeat(LIMIT);
        assert!(m.len() > LIMIT);
        assert_eq!(judge(&m), Decision::Pass);
    }
}
