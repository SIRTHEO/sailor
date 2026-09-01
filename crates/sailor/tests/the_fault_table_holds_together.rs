//! La tabella dei guasti regge da sola: numeri senza buchi né doppioni, ogni
//! voce completa, e i conteggi scritti in prosa uguali a quelli veri.
//!
//! **PERCHÉ ESISTE, E PERCHÉ NON È PIGNOLERIA.** Il 31/08/2026 due sessioni
//! hanno scritto nel file nello stesso minuto e sono nati **due guasti 27 e due
//! guasti 28**: quattro righe, due numeri. Nessuno se ne è accorto, perché un
//! documento non ha un compilatore. È il guasto che `docs/da-fare.md` aveva già
//! previsto — «il file dei guasti è stato modificato durante una corsa che lo
//! citava: l'analisi parlava di dieci guasti, il file ne aveva undici, e il
//! verificatore ha respinto per incoerenza».
//!
//! E i conteggi in prosa erano sbagliati **in quattro punti su quattro**: un
//! numero ricopiato a mano diverge, e questa tabella è la fonte da cui tutti e
//! quattro dicevano di venire. Qui il numero si ricava contando, e la prosa
//! deve dire lo stesso.

use std::collections::BTreeMap;
use std::path::PathBuf;

fn repository_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|crates| crates.parent())
        .expect("il crate sta in <radice>/crates/sailor")
        .to_path_buf()
}

struct Fault {
    number: usize,
    cells: Vec<String>,
    open: bool,
    partly: bool,
}

impl Fault {
    /// Un guasto conta come aperto finché la cura che dichiara non è fatta.
    ///
    /// **«CHIUSO IN PARTE» È APERTO, E IL CONTEGGIO DEVE DIRLO.** Uno stato di
    /// mezzo serve a raccontare quale metà è fatta, non a togliere una riga dal
    /// conto: chi legge «undici aperti» crede che ne restino undici da fare, e
    /// invece ne restano dodici. La direzione dell'errore non è casuale — è
    /// sempre quella che tranquillizza, e per questo la regola sta qui e non
    /// nella testa di chi aggiorna la prosa.
    ///
    /// Il caso vero: il guasto 37 è stato marcato «chiuso in parte» il
    /// 01/09/2026 con la bugia riparata e **la misura ancora da fare**, cioè la
    /// metà che vale. Il campo `partly` esisteva già e non lo interrogava
    /// nessuno: un campo calcolato e mai letto non è una difesa, è la forma di
    /// una difesa.
    fn still_open(&self) -> bool {
        self.open || self.partly
    }
}

fn faults() -> Vec<Fault> {
    let path = repository_root().join("docs/guasti-incontrati.md");
    let text = std::fs::read_to_string(&path)
        .unwrap_or_else(|error| panic!("leggere {}: {error}", path.display()));
    text.lines()
        .filter_map(|line| {
            let trimmed = line.trim();
            if !trimmed.starts_with('|') {
                return None;
            }
            let cells: Vec<String> = trimmed
                .trim_matches('|')
                .split(" | ")
                .map(|cell| cell.trim().to_owned())
                .collect();
            let number: usize = cells.first()?.parse().ok()?;
            let status = cells.last().cloned().unwrap_or_default();
            Some(Fault {
                number,
                // **BASTA CHE COMINCI CON «APERTO», E IL PERCHÉ È IL GUASTO 42
                // STESSO.** Fino al 01/09/2026 qui c'era `status ==
                // "**aperto**"`: uno stato che aggiungesse una sola parola —
                // «**aperto** — le difese di procedura sono in vigore, il codice
                // no» — non era né aperto né chiuso in parte, quindi **spariva
                // dal conto**. È successo scrivendo la riga 42, che parla
                // esattamente di questo: una risorsa condivisa che nessuno
                // sorveglia. Un confronto esatto su un campo di prosa è una
                // difesa che si rompe alla prima sfumatura, e si rompe **verso
                // il basso**, cioè nella direzione che tranquillizza.
                open: status.starts_with("**aperto**"),
                partly: status.contains("chiuso in parte"),
                cells,
            })
        })
        .collect()
}

/// Nessun numero ripetuto, nessun buco. È la prova che oggi sarebbe stata rossa.
#[test]
fn every_fault_has_its_own_number_and_none_is_missing() {
    let faults = faults();
    assert!(faults.len() >= 25, "la tabella si è svuotata: {}", faults.len());

    let mut seen: BTreeMap<usize, usize> = BTreeMap::new();
    for fault in &faults {
        *seen.entry(fault.number).or_default() += 1;
    }

    let twice: Vec<usize> = seen
        .iter()
        .filter(|(_, count)| **count > 1)
        .map(|(number, _)| *number)
        .collect();
    assert!(
        twice.is_empty(),
        "due guasti diversi con lo stesso numero: {twice:?}. Succede quando due \
         sessioni scrivono nel file nello stesso minuto, e nessuno se ne accorge"
    );

    let missing: Vec<usize> = (1..=faults.len()).filter(|n| !seen.contains_key(n)).collect();
    assert!(
        missing.is_empty(),
        "numeri saltati: {missing:?}. Un buco vuol dire che una riga è stata \
         tolta senza rinumerare, e i rinvii da altri documenti puntano al vuoto"
    );
}

/// **UNA VOCE SENZA «COSA LO IMPEDIREBBE» NON È FINITA**, e lo dice il file
/// stesso in testa. Un guasto senza il suo seguito è un diario, che è
/// esattamente ciò che quel file dichiara di non essere.
#[test]
fn no_fault_is_left_without_the_check_that_would_have_stopped_it() {
    for fault in faults() {
        assert_eq!(
            fault.cells.len(),
            6,
            "il guasto {} non ha sei colonne: {:?}",
            fault.number,
            fault.cells
        );
        for (column, name) in ["numero", "data", "cosa è successo", "come si è visto", "cosa lo impedirebbe", "stato"]
            .iter()
            .enumerate()
            .map(|(index, name)| (index, *name))
        {
            assert!(
                !fault.cells[column].is_empty(),
                "il guasto {} ha «{name}» vuoto",
                fault.number
            );
        }
    }
}

/// I numeri fino a diciannove, che in italiano non seguono nessuna regola.
const IRREGULAR: [&str; 20] = [
    "zero", "uno", "due", "tre", "quattro", "cinque", "sei", "sette", "otto",
    "nove", "dieci", "undici", "dodici", "tredici", "quattordici", "quindici",
    "sedici", "diciassette", "diciotto", "diciannove",
];

/// Le decine.
const TENS: [&str; 10] = [
    "", "", "venti", "trenta", "quaranta", "cinquanta", "sessanta", "settanta",
    "ottanta", "novanta",
];

/// Il numero scritto in lettere, come lo scrive la prosa sotto la tabella.
///
/// **PRIMA QUI C'ERA UN ELENCO DI QUARANTOTTO NUMERI SCRITTI A MANO**, e ogni
/// guasto nuovo obbligava ad allungarlo: il 01/09/2026 è stato allungato tre
/// volte in un pomeriggio, e ogni volta la prova falliva con «nessuna parola
/// per N: allunga IN_WORDS». Un elenco che cresce con i dati non è una
/// traduzione, è un debito con la rata mensile — e per giunta un elenco scritto
/// a mano può contenere un refuso che nessuno vede, perché è la sola fonte
/// contro cui si potrebbe controllarlo.
///
/// Le regole invece sono tre, e non cambiano: sotto il venti non c'è regola e
/// si elencano; da lì in su decina più unità; **la decina perde la vocale
/// finale davanti a «uno» e «otto»** (ventuno, ventotto) e **«tre» prende
/// l'accento in coda** (ventitré). Fine.
fn spelled(number: usize) -> String {
    if number < 20 {
        return IRREGULAR[number].to_string();
    }
    assert!(
        number < 100,
        "la prosa non ha mai scritto un numero a tre cifre in lettere: se serve, \
         la regola delle centinaia va aggiunta qui invece che aggirata"
    );
    let (ten, unit) = (number / 10, number % 10);
    let tens = TENS[ten];
    match unit {
        0 => tens.to_string(),
        // La decina si tronca davanti alle due vocali che aprono.
        1 | 8 => format!("{}{}", &tens[..tens.len() - 1], IRREGULAR[unit]),
        // «tre» in coda porta l'accento: ventitré, non ventitre.
        3 => format!("{tens}tré"),
        _ => format!("{tens}{}", IRREGULAR[unit]),
    }
}

/// **CHI TRADUCE VA CONTROLLATO.** La versione vecchia era un elenco scritto a
/// mano: sbagliata o giusta, era comunque la sola fonte, quindi un refuso non
/// si poteva vedere. Una funzione si può sbagliare in modo diverso — sulle
/// eccezioni — e sono quelle che questa prova elenca, non tutti i numeri.
///
/// I casi scelti sono i tre punti in cui la regola generale non basta: la
/// troncatura davanti a «uno» e «otto», l'accento su «tré» in coda, e le decine
/// tonde.
#[test]
fn the_numbers_are_spelled_the_way_italian_spells_them() {
    for (number, word) in [
        (0, "zero"),
        (3, "tre"),
        (16, "sedici"),
        (19, "diciannove"),
        (20, "venti"),
        (21, "ventuno"),
        (23, "ventitré"),
        (28, "ventotto"),
        (30, "trenta"),
        (33, "trentatré"),
        (38, "trentotto"),
        (41, "quarantuno"),
        (47, "quarantasette"),
        (68, "sessantotto"),
        (91, "novantuno"),
        (93, "novantatré"),
    ] {
        assert_eq!(spelled(number), word, "{number} si scrive «{word}»");
    }
}

/// **I CONTEGGI IN PROSA DICONO IL VERO.**
///
/// Il file scrive «**Undici sono ancora aperti** su ventidue» sotto la tabella.
/// Quei due numeri si contano, e finché li ricopia una persona divergono: erano
/// sbagliati in quattro documenti su quattro il 31/08/2026.
#[test]
fn the_counts_written_in_prose_match_the_table_they_come_from() {
    let faults = faults();
    let open = faults.iter().filter(|fault| fault.still_open()).count();
    let total = faults.len();

    let path = repository_root().join("docs/guasti-incontrati.md");
    let text = std::fs::read_to_string(&path).expect("leggere il file dei guasti");
    let prose = text
        .split_once("## Cosa dice questa tabella")
        .map(|(_, after)| after.to_owned())
        .expect("la sezione che commenta la tabella");

    // La maiuscola va sulla parola, non sull'asterisco che la precede: la frase
    // comincia con `**`, e maiuscolare il primo carattere lasciava le due forme
    // identiche — la prova restava rossa su una prosa già giusta.
    let word = spelled(open);
    let capital = {
        let mut chars = word.chars();
        let first = chars.next().expect("la parola non è vuota");
        format!("{}{}", first.to_uppercase(), chars.as_str())
    };
    let sentence = format!("**{word} sono ancora aperti** su {}", spelled(total));
    let capitalized = format!("**{capital} sono ancora aperti** su {}", spelled(total));
    assert!(
        prose.contains(&sentence) || prose.contains(&capitalized),
        "la prosa non dice il conto vero. Contati dalla tabella: {open} aperti su \
         {total}, cioè «{capitalized}». Cambia la frase, non la tabella"
    );
}
