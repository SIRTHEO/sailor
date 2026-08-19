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

/// I prefissi che in inglese si attaccano a un'altra parola. `non` è anche una
/// preposizione italiana, e sono la ragione per cui l'elenco esiste: `non-OK`
/// nel testo, `non_empty` in un nome.
const ENGLISH_PREFIXES: &[&str] = &["non", "post", "pre", "multi", "inter", "extra", "sub", "super"];

/// `non-OK`, `non-failed`, `non-CV`: in inglese `non-` è un prefisso, e la
/// parola dopo il trattino non è italiana.
fn compounds() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(&format!(r"(?i)\b({})-\w+", ENGLISH_PREFIXES.join("|"))).unwrap()
    })
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

// ── I nomi ──────────────────────────────────────────────────────────────────
//
// Un identificatore non contiene articoli né congiunzioni: `collect_worktrees` e
// `raccogli_worktree` si distinguono sulla **radice del verbo**, non sulle
// parole funzione. Serve quindi un secondo criterio, con un secondo elenco.

/// Le radici viste davvero nel codice di servizio — misurate il 16/08/2026:
/// **322 identificatori italiani in 29 file su 51**, perché fino a quel giorno
/// nessun controllo li guardava. Sono verbi e sostantivi da attrezzo:
/// `candidato`, `offerta`, `colloquio` restano fuori di proposito, sono i nomi
/// delle cose che Other-repo tratta e in italiano ci stanno bene.
const ITALIAN_ROOTS: &[&str] = &[
    "raccogl", "giudic", "chiud", "apri", "elenc", "rinomin", "sistem", "misur",
    "rilev", "verific", "controll", "cont", "avvis", "segnal", "legg", "scriv",
    "cerc", "trov", "filtr", "ordin", "stamp", "esegu", "lancia", "attend",
    "nome", "nomi", "titol", "riga", "righ", "parol", "elenco", "scheda",
    "schede", "cartell", "file_lung", "errori", "problem", "residu", "voto",
    "prov", "cas", "esit", "motiv", "stat", "radic", "orfan", "agh", "ganci",
    "gancio", "sessione", "lavor", "consegn", "giorn", "minut", "silenzi",
    "attes", "mut", "esist", "deriv", "puliz", "adess", "ultim", "prim",
    "second", "terz", "vecchi", "nuov", "tutt", "ogni", "quest", "quell",
    "finestr", "comand", "regol", "aree", "testo", "dati", "fatti",
    // Entrate il 19/08/2026 con la misura in mano
    // (`docs/misura-vocabolario-lingua-2026-08-19/`): il vocabolario vedeva il
    // 35,5% degli identificatori italiani del corpus, e queste lo portano al
    // 71,5% senza un falso positivo. Due filtri, non uno: zero collisioni nelle
    // 40.000 dichiarazioni di `~/.claude` **e** zero parole del dizionario
    // inglese di sistema fra le prime venti. Il secondo ha scartato `ric`
    // (`rich`), `ferm` (`ferment`), `viv` (`vivid`), `camp` (`campaign`),
    // `testa`, `corpo` — che il solo corpus di casa dava per sicure.
    "agente", "agganciati", "agisci", "albero", "ambiente", "anelli", "archivio", "attuale",
    "avvio", "basso", "buoni", "cacciate", "carta", "catalogo", "cifre", "cresc",
    "decisioni", "degeneri", "dentro", "destinatario", "dett", "difetti", "divieti", "dop",
    "errore", "escludi", "etichetta", "evento", "fatte", "filo", "finiscono", "fuori",
    "getta", "grezz", "gruppi", "guardia", "ingresso", "inizio", "intero", "isolata",
    "istruzioni", "letti", "libera", "messaggio", "migli", "niente", "normalizza", "nostri",
    "opachi", "opzioni", "pezzi", "piatta", "piena", "principale", "punto", "raccolt",
    "rango", "riferimento", "rigenerate", "ritrova", "rumore", "ruota", "salto", "sbagliati",
    "scelt", "scritto", "secco", "senza", "sogli", "sommario", "sorgenti", "successore",
    "sveglia", "tetto", "toccate", "totali", "tronca", "unici", "uscita", "visti",
    "volte",
];

/// Nomi inglesi che una radice troppo corta prenderebbe per italiani.
///
/// Vale il criterio sbilanciato di sopra: `mute` da `mut`, `question` da
/// `quest`, `right` da `righ` erano nomi inglesi corretti segnalati a torto, e
/// un controllo che rimprovera a torto viene spento al primo caso.
fn not_italian() -> &'static HashSet<&'static str> {
    static WORDS: OnceLock<HashSet<&'static str>> = OnceLock::new();
    WORDS.get_or_init(|| {
        [
            "state", "states", "stats", "status", "content", "contents", "context",
            "count", "counter", "counts", "contain", "contains", "container",
            "primary", "prime", "second", "seconds", "name", "names", "namespace",
            "novel", "data", "date", "stat", "catch", "match", "matches", "batch",
            "minute", "minutes", "legs", "legacy", "legal", "apri", // `apri` = API + rilievo
            "contract", "contrast", "ordinal", "order", "orders", "ordered",
            "filter", "filters", "filtered", "find", "finding", "findings",
            "title", "titles", "row", "rows", "testable", "tested", "text", "texts",
            "provider", "providers", "provision", "proven", "probe", "probes",
            "problem", "problems", "problematic", "contained",
            "cast", "case", "cases", "casing", "exist", "exists", "existing",
            "ultimate", "primer", "novelty", "quest", "query", "queries",
            "mute", "muted", "mutes", "muting", "mutation", "mutations", "mutable",
            "question", "questions", "right", "rights", "contest", "contested",
            "statement", "statements", "static", "station", "stale", "standard",
            "continue", "continues", "contribution", "contributor",
            "formatted", "format", "formats", "normal", "normally", "ordinary",
            "apricot", "origin", "original", "originals", "prompt", "prompts",
            "residue", "residues", "residual", "control", "controls",
            // Gli otto rimproveri a torto che la misura del 19/08 ha contato
            // fra i 144 nomi già segnalati: `mut` prendeva `FnMut` e i nomi dei
            // casi mutanti, `stamp` i test inglesi sulle marche temporali,
            // `stat` `hasStatusline`, `motiv` `c_cron_motivated`. Le radici
            // restano: `stampa` e `mutare` in italiano si continuano a vedere.
            "mut", "mutant", "mutants", "stamp", "stamps", "stamped",
            "statusline", "motivate", "motivated", "motivation",
        ]
        .into_iter()
        .collect()
    })
}

/// Spezza un identificatore su underscore, trattino e confini di camelCase.
///
/// Il Python lo fa con `(?<=[a-z0-9])(?=[A-Z])`, due lookaround che il motore di
/// Rust non ha. A mano è anche più chiaro: si taglia dove una minuscola o una
/// cifra è seguita da una maiuscola.
fn split_identifier(name: &str) -> Vec<String> {
    let mut parts = Vec::new();
    let mut current = String::new();
    let chars: Vec<char> = name.chars().collect();
    for (i, &c) in chars.iter().enumerate() {
        if c == '_' || c == '-' {
            if !current.is_empty() {
                parts.push(std::mem::take(&mut current));
            }
            continue;
        }
        let boundary = c.is_uppercase()
            && i > 0
            && (chars[i - 1].is_lowercase() || chars[i - 1].is_numeric());
        if boundary && !current.is_empty() {
            parts.push(std::mem::take(&mut current));
        }
        current.push(c);
    }
    if !current.is_empty() {
        parts.push(current);
    }
    parts
}

/// Vero se un identificatore è scritto in italiano.
///
/// Due criteri, e prendono cose diverse. Le **radici** vedono i nomi corti del
/// codice vero (`soglie`, `_prefisso`, `VERDETTO`); le **parole-funzione** — le
/// stesse che `is_italian` usa sul testo — vedono i nomi-frase dei test
/// (`una_consegna_valida_disarma_tutto`), dove il vocabolario è infinito e ogni
/// frase nuova porta parole che nessun elenco di radici avrà. Misurato il
/// 19/08/2026: le sole radici prendevano 136 dei 383 identificatori italiani
/// del corpus, le parole-funzione ne aggiungono 64 senza un falso positivo.
pub fn is_italian_name(name: &str) -> bool {
    let english = not_italian();
    let words = italian_words();
    let parts = split_identifier(name);
    parts.iter().enumerate().any(|(i, part)| {
        let low = part.to_lowercase();
        if english.contains(low.as_str()) {
            return false;
        }
        if ITALIAN_ROOTS.iter().any(|r| low.starts_with(r)) {
            return true;
        }
        // `non_empty`, `pre_flight`: davanti a un'altra parte questi non sono
        // preposizioni ma prefissi inglesi, ed erano 11 dei 15 falsi positivi
        // che la misura attribuiva a questa strada. È la stessa neutralizzazione
        // che `compounds()` fa sul testo, applicata alle parti di un nome.
        let is_prefix = ENGLISH_PREFIXES.contains(&low.as_str()) && i + 1 < parts.len();
        words.contains(low.as_str()) && !is_prefix
    })
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
    fn it_splits_a_name_the_way_python_splits_it() {
        assert_eq!(split_identifier("raccogli_worktree"), ["raccogli", "worktree"]);
        assert_eq!(split_identifier("collectWorktrees"), ["collect", "Worktrees"]);
        assert_eq!(split_identifier("HTTPServer"), ["HTTPServer"]);
        assert_eq!(split_identifier("parse2Json"), ["parse2", "Json"]);
        assert_eq!(split_identifier("__x__"), ["x"]);
    }

    #[test]
    fn it_recognises_an_italian_identifier() {
        assert!(is_italian_name("raccogli_worktree"));
        assert!(is_italian_name("chiudiSessione"));
        assert!(!is_italian_name("collect_worktrees"));
    }

    /// I nomi inglesi che una radice troppo corta prendeva per italiani: se
    /// tornano a essere segnalati, il controllo viene spento al primo caso.
    #[test]
    fn the_english_names_a_short_root_used_to_catch_stay_clear() {
        for name in ["state", "content", "counter", "mute", "question", "right",
                     "status", "format", "control", "query"] {
            assert!(!is_italian_name(name), "{name} non è italiano");
        }
    }

    /// I nomi-frase dei test: nessun elenco di radici li vedrà mai tutti, e
    /// sono la ragione del secondo criterio.
    #[test]
    fn a_sentence_shaped_name_is_caught_by_its_function_words() {
        assert!(is_italian_name("una_consegna_valida_disarma_tutto"));
        assert!(is_italian_name("il_gradino_si_annuncia_una_volta_sola"));
        assert!(is_italian_name("senza_sessione_si_ricade_sul_percorso"));
        assert!(is_italian_name("dopo_il_tetto_dei_rifiuti_il_gancio_si_arrende"));
    }

    /// `non` è preposizione in italiano e prefisso in inglese: davanti a
    /// un'altra parte vince l'inglese, e da solo resta italiano.
    #[test]
    fn an_english_prefix_is_not_a_preposition() {
        assert!(!is_italian_name("non_empty"));
        assert!(!is_italian_name("NON_STANDARD"));
        assert!(!is_italian_name("pre_flight"));
        assert!(!is_italian_name("post_merge_check"));
        assert!(is_italian_name("il_dubbio_non"));
    }

    /// Gli otto rimproveri a torto contati il 19/08/2026 sul corpus di
    /// `~/.claude`: se tornano, il gate torna a segnalare codice inglese
    /// corretto — ed è così che un controllo viene spento.
    #[test]
    fn the_eight_false_positives_the_measure_counted_stay_clear() {
        for name in [
            "FnMut",
            "hasStatusline",
            "stamp",
            "c_cron_motivated",
            "it_stamps_a_javascript_shaped_timestamp",
            "the_journal_stamps_the_shape_python_writes",
            "mutant_a_relative_path_is_not_a_home_path",
            "mutant_a_scoped_rule_passes",
        ] {
            assert!(!is_italian_name(name), "{name} è inglese");
        }
        // Le radici non sono state tolte: l'italiano che le usa si vede ancora.
        assert!(is_italian_name("stampa_riga"));
        assert!(is_italian_name("mutazione_uccisa"));
    }

    /// Le sei radici che il solo corpus di casa dava per sicure e il dizionario
    /// inglese ha scartato. Chi le riaggiunge guardando i conteggi rompe questo.
    #[test]
    fn the_roots_the_english_dictionary_rejected_stay_out() {
        for name in ["rich_text", "campaign_id", "ferment_queue", "vivid_colour",
                     "testable_unit", "corporate_plan"] {
            assert!(!is_italian_name(name), "{name} è inglese");
        }
    }

    /// Un campione delle 81 radici entrate il 19/08: i nomi corti del codice
    /// vero, che le parole-funzione non vedono mai.
    #[test]
    fn the_short_names_of_real_code_are_seen() {
        for name in ["soglie_opus5", "destinatario", "tetto_byte", "albero_di_lavoro",
                     "gruppi", "sommario", "uscita_grezza"] {
            assert!(is_italian_name(name), "{name} è italiano");
        }
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
