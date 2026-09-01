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

/// I numeri scritti in lettere nella prosa, tradotti. Si fermano dove serve:
/// una tabella più lunga di così vorrà una riga in più qui, e la prova lo dirà
/// invece di tacere.
const IN_WORDS: [(&str, usize); 46] = [
    ("zero", 0), ("uno", 1), ("due", 2), ("tre", 3), ("quattro", 4),
    ("cinque", 5), ("sei", 6), ("sette", 7), ("otto", 8), ("nove", 9),
    ("dieci", 10), ("undici", 11), ("dodici", 12), ("tredici", 13),
    ("quattordici", 14), ("quindici", 15), ("sedici", 16), ("diciassette", 17),
    ("diciotto", 18), ("diciannove", 19), ("venti", 20), ("ventuno", 21),
    ("ventidue", 22), ("ventitré", 23), ("ventiquattro", 24),
    ("venticinque", 25), ("ventisei", 26), ("ventisette", 27),
    ("ventotto", 28), ("ventinove", 29), ("trenta", 30), ("trentuno", 31),
    ("trentadue", 32), ("trentatré", 33), ("trentaquattro", 34),
    ("trentacinque", 35), ("trentasei", 36), ("trentasette", 37),
    ("trentotto", 38), ("trentanove", 39), ("quaranta", 40),
    ("quarantuno", 41), ("quarantadue", 42), ("quarantatré", 43),
    ("quarantaquattro", 44), ("quarantacinque", 45),
];

fn spelled(number: usize) -> &'static str {
    IN_WORDS
        .iter()
        .find(|(_, value)| *value == number)
        .map(|(word, _)| *word)
        .unwrap_or_else(|| panic!("nessuna parola per {number}: allunga IN_WORDS"))
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
