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

/// I prefissi che in inglese si attaccano a un'altra parola. `non` e `un` sono
/// anche parole italiane, e sono la ragione per cui l'elenco esiste: `non-OK`
/// nel testo, `non_empty` e `isUnParenthesizedName` in un nome. Valgono solo
/// davanti a un'altra parte, quindi `il_dubbio_non` resta italiano.
const ENGLISH_PREFIXES: &[&str] = &["non", "un", "post", "pre", "multi", "inter", "extra", "sub", "super"];

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
    // 71,5%. Due filtri, non uno: zero collisioni nelle 40.000 dichiarazioni di
    // `~/.claude` **e** nessuna parola del dizionario inglese fra le prime venti
    // di ciascuna. Il secondo ha scartato `ric` (`rich`), `ferm` (`ferment`),
    // `viv` (`vivid`), `camp` (`campaign`), `testa`, `corpo` — che il solo
    // corpus di casa dava per sicure.
    //
    // «Nessun falso positivo» era una lettura troppo larga di quel filtro, ed è
    // stata corretta il 19/08 dalla misura in
    // `docs/2026-08-19-gate-lingua-falsi-positivi.md`: le prime venti parole non
    // sono tutte le parole, e le radici **vecchie** non ci sono mai passate. Su
    // tutto il dizionario le radici prendevano **1.574** parole inglesi, la
    // grande maggioranza dalle vecchie (`cont`, `cas`, `prov`, `prim`, `stat`,
    // `mut`, `stamp`). La scomposizione «1.426 vecchie + 124 nuove» che girava
    // nei documenti non torna — le due parti non sommano al totale e una parola
    // può cadere sotto più radici — quindi qui resta il solo numero misurato.
    // La cura non è stata potare l'elenco ma `looks_english`, che filtra prima
    // del confronto a prefisso.
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
            // **Il lessico che il 1913 non poteva avere.** `looks_english`
            // consulta il web2 di Webster, che è dell'edizione 1913: le parole
            // dell'informatica non ci sono, e le radici corte se le prendono.
            // `mutex` è il termine di concorrenza più comune in Rust — cioè nel
            // linguaggio in cui è scritto questo gancio — e veniva negato dalla
            // radice `mut`. Qui vanno le parole moderne che il dizionario non
            // conosce, non i falsi positivi che il filtro già copre.
            "mutex", "mutexes", "containerize", "containerized", "containerization",
            "dopamine", "interop", "interoperable", "interoperability",
        ]
        .into_iter()
        .collect()
    })
}

/// Il dizionario inglese di sistema, minuscolo.
///
/// Se il file manca, **solo `looks_english` si spegne** e torna a com'era prima
/// di questo filtro: `un` fra i prefissi e le eccezioni `interop*` restano
/// attive comunque. Su macOS il file c'è sempre; su Linux quasi mai — il
/// pacchetto non è preinstallato — e senza avviso il gate retrocederebbe in
/// silenzio al tasso di rimproveri a torto che questa correzione ha appena
/// tolto. Perciò lo dice, una volta per processo.
///
/// **Il costo non è leggere il file, è quello che se ne fa.** Il gancio è un
/// processo nuovo a ogni scrittura, quindi non c'è niente da ammortizzare: la
/// preparazione si paga intera ogni volta. Misurato il 19/08/2026 sul binario in
/// servizio, mediana su 25 giri di una scrittura che apre il dizionario:
///
/// | | ms |
/// |---|---|
/// | `HashSet<String>`, una `String` allocata per riga | 30,9 |
/// | elenco ordinato e ricerca binaria | 27,0 |
/// | **`HashSet<&str>` su testo prestato una volta** | **18,9** |
///
/// Una scrittura che non apre il dizionario ne costa 4,5. I 2,4 MB si leggono in
/// pochi millisecondi: il costo erano le 236.000 `String` allocate una per riga,
/// e sparisce prestando il testo (`Box::leak` — il processo dura una frazione di
/// secondo) e tenendo **fette** di quel testo.
///
/// **La ricerca binaria è stata provata e scartata**, e vale la pena averlo
/// scritto: sembra la via ovvia — niente impronte da calcolare — ma il
/// sottoinsieme va ordinato prima, e ordinare 210.000 voci costa più di quanto
/// si risparmi. Saltare l'ordinamento non è un'opzione: quel sottoinsieme è
/// ordinato tranne una riga, e la ricerca binaria su dati non ordinati non
/// fallisce, **mente**.
///
/// **Le voci con la maiuscola servono comunque**, ed è la lezione di un
/// tentativo sbagliato: tenere solo quelle già minuscole sembrava gratis —
/// `looks_english` riceve sempre minuscolo — e invece buttava via 25.000 parole
/// che, minuscolizzate, il confronto usa eccome. La radice `cas` si riprendeva
/// `Cassiopeia`, `Casanovanic`, `Castilian`: i rimproveri a torto sul dizionario
/// erano risaliti da 20 a 135. Se ne è accorto `examples/dictionary.rs`, non un
/// test. Quindi si allocano le sole voci da abbassare, non tutte e 236.000.
fn english_dictionary() -> &'static HashSet<&'static str> {
    static WORDS: OnceLock<HashSet<&'static str>> = OnceLock::new();
    WORDS.get_or_init(|| match std::fs::read_to_string(DICTIONARY_PATH) {
        Ok(text) => {
            let text: &'static str = Box::leak(text.into_boxed_str());
            text.lines()
                .map(str::trim)
                .filter(|w| !w.is_empty())
                .map(|w| -> &'static str {
                    if w.bytes().any(|b| b.is_ascii_uppercase()) {
                        Box::leak(w.to_lowercase().into_boxed_str())
                    } else {
                        w
                    }
                })
                .collect()
        }
        Err(_) => {
            eprintln!(
                "code-language: no English dictionary at {DICTIONARY_PATH}, \
                 falling back to root matching alone (expect false positives \
                 on English names)"
            );
            HashSet::new()
        }
    })
}

/// Il dizionario conosce questa parola.
fn in_dictionary(word: &str) -> bool {
    english_dictionary().contains(word)
}

/// Dove vive il dizionario. Costante e non configurabile di proposito: una
/// valvola qui vorrebbe dire poter spegnere il filtro dall'esterno.
const DICTIONARY_PATH: &str = "/usr/share/dict/words";

/// Suffissi flessivi: `stamping` è `stamp` inglese, non l'inizio di «stampa».
const INFLECTIONS: &[&str] = &["s", "es", "ed", "d", "ing", "er", "ers", "est", "ly", "ion", "ions"];

/// Vero se una parte del nome è una parola inglese, o ci si riconduce togliendo
/// un suffisso flessivo.
///
/// È il filtro che chiude la famiglia invece del singolo caso. `not_italian()`
/// è a corrispondenza esatta e compilato a mano, quindi ogni flessione di un
/// falso positivo già curato rientra dalla finestra: `container` era curato,
/// `containers` no; `stamp` sì, `stamping` e `stampede` no.
///
/// **Non decide da solo sulle parole-funzione**: chi lo chiama deve prima
/// chiedere a `italian_words()`. Il dizionario di sistema è il *web2* di
/// Webster, edizione 1913, e contiene come voci proprie `con`, `che`, `chi`,
/// `col`, `del`, `fra`, `non`, `poi`, `tra`, `tutti` — cioè quasi tutto
/// l'elenco curato a mano. Senza quella precedenza il filtro le spegneva prima
/// che l'elenco potesse vederle, e 13 identificatori italiani già scritti in
/// `~/.claude` smettevano di essere riconosciuti: `con_barra`,
/// `worktree_del_percorso`, `una_figlia_non_arma_e_tace`. Una soglia sulla
/// lunghezza non basta: copriva `la` e `un` a due lettere, mai `con` e `del` a
/// tre. La regola giusta non è quanto è corta la parola, è **chi l'ha scritta a
/// mano**: un elenco curato batte un dizionario generico.
fn looks_english(low: &str) -> bool {
    if english_dictionary().is_empty() {
        return false;
    }
    // Le cifre finali non cambiano la lingua: `toLowerCase2` è `case`.
    let base = low.trim_end_matches(|c: char| c.is_ascii_digit());
    // Una lettera sola non è una parola: nel web2 `a` e `i` sono voci.
    if base.len() < 2 {
        return false;
    }
    if in_dictionary(base) {
        return true;
    }
    INFLECTIONS.iter().any(|suffix| {
        let Some(root) = base.strip_suffix(suffix) else {
            return false;
        };
        if root.chars().count() < 3 {
            return false;
        }
        if in_dictionary(root) || in_dictionary(&format!("{root}e")) {
            return true;
        }
        // Il troncamento passa dai **caratteri**, mai dai byte: `&root[..len-1]`
        // su un nome non ASCII taglia dentro un carattere e fa panico, e un
        // gancio che va in errore rifiuta ogni strumento, non solo il proprio.
        // Provato: `is_italian_name("もing")` — も è E3 82 82, ultimo e
        // penultimo byte coincidono, quindi la vecchia riga credeva a una
        // consonante raddoppiata e tagliava a metà del carattere.
        let mut chars = root.chars();
        let Some(last) = chars.next_back() else {
            return false;
        };
        let shorter = chars.as_str();
        // `stopped` → `stop`: la consonante che il suffisso aveva raddoppiato.
        let doubled = shorter.chars().next_back() == Some(last);
        // `primaries` → `primary`: la `y` diventata `i` davanti a `-es`.
        let from_y = last == 'i' && in_dictionary(&format!("{shorter}y"));
        from_y || (doubled && in_dictionary(shorter))
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
        // **Le parole-funzione curate a mano decidono da sole**, senza chiedere
        // al dizionario: `con`, `del`, `tra`, `tutti` sono anche voci del web2
        // del 1913, e lasciarle giudicare da lì spegneva 13 nomi italiani già
        // scritti in `~/.claude`. Un elenco curato batte un dizionario generico.
        //
        // Ma **solo in minuscolo**, che è lo stesso criterio con cui `is_italian`
        // legge il testo: là `DEL-3` è il nome di una rotta, qui `Col` di
        // `HTMLTableColElement` e `Non` di `maxProgramSizeForNonTsFiles` sono
        // pezzi di camelCase inglese. L'italiano di questo repo scrive i
        // nomi-frase in minuscolo con gli underscore, sempre.
        if words.contains(low.as_str()) && is_lowercase(part) {
            // `non_empty`, `pre_flight`, `un_wrap_value`: davanti a una parola
            // **inglese** questi non sono preposizioni ma prefissi. La
            // condizione sulla parte successiva non c'era, e senza di essa
            // `un_valore` e `un_gruppo` diventavano inglesi: in inglese `un-` si
            // attacca a una parola inglese, in italiano l'articolo precede una
            // parola italiana, e questa è la differenza che si può guardare.
            let followed_by_english = parts.get(i + 1).is_some_and(|next| {
                let next = next.to_lowercase();
                english.contains(next.as_str()) || looks_english(&next)
            });
            return !(ENGLISH_PREFIXES.contains(&low.as_str()) && followed_by_english);
        }
        // Le **radici** cedono al dizionario, perché combaciano a prefisso senza
        // confine di parola: tutto ciò che comincia per `stamp` sarebbe
        // altrimenti italiano — `stamping`, `stamper`, `stampede`. Misurato il
        // 19/08/2026: 1.574 parole del dizionario inglese giudicate italiane, e
        // 33 nomi rifiutati a torto su 5.621 di codice inglese vero.
        //
        // Il dizionario si consulta **solo qui**, cioè solo quando una radice ha
        // già combaciato: caricarlo costa ~25 ms e il gancio è un processo nuovo
        // a ogni scrittura, quindi un file di codice inglese puro non lo apre
        // mai. Chi sposta questa chiamata più in alto rimetta quel costo su ogni
        // scrittura.
        if ITALIAN_ROOTS.iter().any(|r| low.starts_with(r)) {
            return !looks_english(&low);
        }
        false
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

    /// Le famiglie che l'elenco a corrispondenza esatta non poteva chiudere: il
    /// confronto con le radici è a prefisso, quindi ogni flessione di un nome
    /// inglese già curato rientrava dalla finestra. Sono 14 dei 283 rifiuti a
    /// torto contati il 19/08/2026 su `typescript/lib` e `zod`.
    #[test]
    fn an_inflected_english_name_is_no_longer_read_as_an_italian_root() {
        for name in [
            "stamping", "stampede", "stamper", "containers", "toLowerCase2",
            "statuses", "provideInlayHints", "contextualType", "primitives",
            "extendStatics", "derived", "verification", "provenance", "interop",
            "dope", "crescentIcon",
            // `-ies` è la `y` che diventa `i` davanti al plurale: senza quel
            // ramo `VideoColorPrimaries` restava preso dalla radice `prim`.
            "VideoColorPrimaries", "dependencies", "boundaries",
        ] {
            assert!(!is_italian_name(name), "{name} è inglese");
        }
    }

    /// In inglese `Un-` si attacca a un'altra parola come `non-`, e non era
    /// nell'elenco dei prefissi: tre dei sette rifiuti residui venivano da qui.
    #[test]
    fn the_english_negative_prefix_is_not_the_italian_article() {
        assert!(!is_italian_name("isUnParenthesizedName"));
        assert!(!is_italian_name("parseUnQuoted"));
        assert!(!is_italian_name("un_wrap_value"));
    }

    /// **Il costo del filtro, scritto perché non sia una sorpresa.** Queste
    /// parole italiane stanno anche nel dizionario inglese di sistema e
    /// arrivano da una *radice*, non dall'elenco curato: il gate ha smesso di
    /// vederle. Misurati sul corpus di `~/.claude`: **8 nomi su 272**, contro
    /// 32 rimproveri a torto tolti. Chi le rivuole le aggiunga a
    /// `italian_words()`, che ha la precedenza sul dizionario — non tocchi
    /// l'ordine dei controlli.
    #[test]
    fn the_italian_words_the_english_dictionary_also_has_are_the_price() {
        for name in ["basso", "doppia", "misura", "nome", "prima", "punto",
                     "filo_err", "filo_out"] {
            assert!(!is_italian_name(name), "{name}: costo noto del filtro");
        }
    }

    /// **Le parole-funzione curate a mano battono il dizionario.** Il web2 di
    /// Webster è del 1913 e contiene `con`, `del`, `non`, `tra`, `tutti` come
    /// voci proprie: senza questa precedenza il filtro le spegneva prima che
    /// l'elenco italiano potesse vederle, e questi nomi — tutti già scritti in
    /// `~/.claude` — smettevano di essere riconosciuti.
    #[test]
    fn a_curated_function_word_beats_the_system_dictionary() {
        for name in [
            "con_barra",
            "worktree_del_percorso",
            "una_figlia_non_arma_e_tace",
            "con_tutti_i_freni_liberi_si_apre",
            "il_dubbio_non",
        ] {
            assert!(is_italian_name(name), "{name} è italiano");
        }
        assert!(is_italian("se la cosa non torna"));
    }

    /// `un_valore` non è `un_wrap_value`: in inglese `un-` si attacca a una
    /// parola inglese, in italiano l'articolo precede una parola italiana.
    /// Senza la condizione sulla parte successiva, l'articolo spegneva quattro
    /// nomi italiani veri.
    #[test]
    fn the_article_only_becomes_a_prefix_in_front_of_an_english_word() {
        for name in ["un_valore", "un_gruppo", "un_tentativo"] {
            assert!(is_italian_name(name), "{name} è italiano");
        }
        for name in ["un_wrap_value", "isUnParenthesizedName", "parseUnQuoted"] {
            assert!(!is_italian_name(name), "{name} è inglese");
        }
        // Il limite, dichiarato: `un_numero` resta invisibile, ma non per
        // l'articolo — `numero` è una voce del dizionario inglese, e nessuna
        // radice lo copre. Si cura aggiungendo `numer` a `ITALIAN_ROOTS`, non
        // toccando i prefissi.
        assert!(!is_italian_name("un_numero"));
    }

    /// Le parole-funzione valgono **solo in minuscolo**, come sul testo, dove
    /// `DEL-3` è il nome di una rotta. In camelCase inglese quelle stesse
    /// lettere sono un pezzo di parola, non una preposizione.
    #[test]
    fn a_function_word_in_camel_case_is_a_word_fragment() {
        for name in ["HTMLTableColElement", "maxProgramSizeForNonTsFiles",
                     "previouslyAcceptedNonCuids", "NON_STANDARD"] {
            assert!(!is_italian_name(name), "{name} è inglese");
        }
    }

    /// **Il dizionario è del 1913 e non conosce l'informatica.** `mutex` è il
    /// termine di concorrenza più comune in Rust — il linguaggio in cui questo
    /// gancio è scritto — e la radice `mut` se lo prendeva senza che nessuna
    /// voce del dizionario potesse smentirla.
    #[test]
    fn the_modern_words_the_1913_dictionary_never_had_stay_clear() {
        for name in ["mutex", "Mutex", "mutexes", "containerize", "containerized",
                     "dopamine"] {
            assert!(!is_italian_name(name), "{name} è inglese");
        }
    }

    /// Un gancio che va in errore **rifiuta ogni strumento**, non solo il
    /// proprio: qui l'aritmetica sui byte tagliava dentro un carattere non
    /// ASCII. `も` è `E3 82 82`, ultimo e penultimo byte coincidono, quindi il
    /// controllo sulla consonante raddoppiata credeva di poter troncare.
    #[test]
    fn a_name_with_multibyte_characters_does_not_panic() {
        for name in ["もing", "もs", "testもing", "日本語_stamping", "é_stamped"] {
            let _ = is_italian_name(name);
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
