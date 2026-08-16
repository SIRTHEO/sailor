//! Riconoscere l'italiano in una stringa, con la soglia prudente.
//!
//! Porta della funzione `is_italian` di `skills/hooks/code-language.py`. Sta in
//! un modulo suo perché la usano due ganci: `code-language` sui commenti e le
//! stringhe del codice, `pr-title` sul titolo di una richiesta. Nel Python il
//! secondo la caricava dal percorso del primo — «ricopiare qui l'elenco delle
//! parole avrebbe fatto due elenchi che divergono al primo falso positivo
//! corretto da una parte sola», e il 14/08 era già successo.
//!
//! IL CRITERIO, e perché è sbilanciato di proposito. **Meglio lasciar passare
//! una stringa italiana che rimproverare per una inglese**: chi viene
//! rimproverato a torto riscrive il testo per far tacere il controllo, e il
//! 14/08 è successo davvero — `status non-OK` è diventato `error status` per
//! far passare il gate, cioè il controllo ha peggiorato il testo che difendeva.

use regex::Regex;
use std::collections::HashSet;
use std::sync::OnceLock;

/// Parole che in inglese non esistono in questa forma. L'elenco è quello del
/// Python, commenti storici compresi: sono la memoria di cosa è sfuggito e
/// perché qualcosa è stato tolto.
fn italian_words() -> &'static HashSet<&'static str> {
    static WORDS: OnceLock<HashSet<&'static str>> = OnceLock::new();
    WORDS.get_or_init(|| {
        [
            "il", "lo", "la", "gli", "le", "un", "uno", "una", "dei", "delle", "degli",
            "del", "della", "dello", "nel", "nella", "nei", "sul", "sulla", "col",
            // Le articolate al plurale mancavano, e con esse sfuggiva un titolo
            // intero: «I controlli girano sulle richieste» non conteneva nessuna
            // delle parole sopra. Fuori `ai`, che in minuscolo è l'intelligenza
            // artificiale, e `dai`, troppo vicino a nomi propri e sigle.
            "sulle", "sui", "nelle", "negli", "dalla", "dalle", "dagli",
            "alla", "alle", "agli", "quella", "quelle", "questa", "queste",
            "che", "chi", "cui", "non", "con", "senza", "quando", "quindi", "anche",
            "dopo", "sempre", "mai", "ogni", "nessun", "nessuna",
            "deve", "devono", "viene", "vengono", "essere", "sono", "erano",
            "torna", "ritorna", "restituisce", "lancia", "accetta", "rifiuta", "manca",
            "vuoto", "vuota", "valido", "valida", "sbagliato", "corretto", "errore",
            "riga", "righe", "primo", "seconda", "terzo", "ultimo", "altro", "altra",
            "se", "ma", "oppure", "invece", "soltanto", "perche", "poi",
            "dentro", "fuori", "sopra", "sotto", "tra", "fra",
            // Sfuggivano interi messaggi di gate: «tutti i casi ok» non conteneva
            // nessuna delle parole sopra.
            "tutti", "tutte", "casi", "ancora", "nessuno", "niente", "qualcosa",
            // Uscite il 16/08/2026 perché esistono anche in inglese e accusavano
            // frasi inglesi corrette: `come`, `era`, `solo`, `verso`, `prima`.
        ]
        .into_iter()
        .collect()
    })
}

fn accents() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"[àèéìòùÀÈÉÌÒÙ]").unwrap())
}

fn words_pattern() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"(?i)[a-zàèéìòù]+").unwrap())
}

/// `non-OK`, `non-failed`, `non-CV`: in inglese `non-` è un prefisso, e la
/// parola dopo il trattino non è italiana.
fn compounds() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"(?i)\b(non|post|pre|multi|inter|extra|sub|super)-\w+").unwrap())
}

pub fn is_italian(text: &str) -> bool {
    if accents().is_match(text) {
        return true;
    }
    // I composti col trattino escono prima del conteggio: `non-OK` non è «non».
    let bare = compounds().replace_all(text, " ");
    // Solo le parole in minuscolo: le sigle sono maiuscole, e `DEL-3` non è la
    // preposizione «del». Misurato il 14/08 su `DELETE /… (DEL-3)`, segnalata
    // come italiana mentre è il nome di una rotta.
    words_pattern()
        .find_iter(&bare)
        .any(|m| is_lowercase(m.as_str()) && italian_words().contains(m.as_str()))
}

/// `str.islower()` di Python: vero se non c'è nessuna maiuscola **e** almeno un
/// carattere con la distinzione di caso. Non è `!s.chars().any(char::is_uppercase)`,
/// che direbbe sì anche su una stringa senza lettere.
fn is_lowercase(s: &str) -> bool {
    s.chars().any(char::is_lowercase) && !s.chars().any(char::is_uppercase)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_accent_is_enough_on_its_own() {
        assert!(is_italian("perché"));
        assert!(is_italian("è vero"));
    }

    #[test]
    fn it_catches_the_titles_that_used_to_slip_through() {
        assert!(is_italian("I controlli girano sulle richieste"));
        assert!(is_italian("tutti i casi ok"));
        assert!(is_italian("Sistema la tabella dei residui"));
    }

    #[test]
    fn it_leaves_correct_english_alone() {
        assert!(!is_italian("add the filter chip row"));
        assert!(!is_italian("stop losing the draft on reload"));
        assert!(!is_italian("adopt the phone package 0.2.0"));
    }

    /// I tre casi che il 14/08 hanno prodotto rimproveri a torto, e che hanno
    /// portato a riscrivere il testo per far tacere il controllo.
    #[test]
    fn the_false_positives_that_taught_the_threshold_stay_fixed() {
        assert!(!is_italian("status non-OK"));
        assert!(!is_italian("non-failed runs"));
        assert!(!is_italian("DELETE /candidates (DEL-3)"));
        // uscite dall'elenco perché esistono anche in inglese
        assert!(!is_italian("do not come back"));
        assert!(!is_italian("the Victorian era"));
        assert!(!is_italian("solo run"));
    }

    #[test]
    fn an_uppercase_word_is_an_acronym_not_a_preposition() {
        assert!(!is_italian("DEL LA CON"));
        assert!(is_italian("del la con"));
    }
}
