//! The fault table holds together on its own: numbers with no holes and no
//! duplicates, every entry complete, and the counts written in prose equal to
//! the real ones.
//!
//! **WHY IT EXISTS, AND WHY IT IS NOT FUSSINESS.** Two sessions wrote into the
//! file in the same minute and **two fault 27s and two fault 28s** were born:
//! four rows, two numbers. Nobody noticed, because a document has no compiler.
//! And the prose counts were wrong **in four places out of four**: a number
//! copied by hand diverges, and this table is the source all four claimed to
//! come from. Here the number is obtained by counting, and the prose must say
//! the same.

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
    standing: faults::Standing,
}

impl Fault {
    /// A fault counts as open until the repair it declares is done.
    ///
    /// **«PARTLY CLOSED» IS OPEN, AND THE COUNT MUST SAY SO.** A middle state
    /// tells which half is done; it does not take a row out of the tally. A
    /// reader of «eleven open» believes eleven remain, when twelve do, and the
    /// direction of that error is never random — it is always the reassuring
    /// one, which is why the rule lives here and not in the head of whoever
    /// updates the prose. The real case: fault 37 was marked partly closed with
    /// the lie repaired and **the measure still to make**, the half that counts.
    /// The `partly` field already existed and nobody asked it: a field computed
    /// and never read is the shape of a defence, not a defence.
    fn still_open(&self) -> bool {
        matches!(
            self.standing,
            faults::Standing::Open | faults::Standing::PartlyClosed
        )
    }
}

/// The rows of the table, read from the document.
///
/// **THIS READING IS BLIND TO A BLANK LINE INSIDE THE TABLE**, worth knowing
/// before trusting what this test claims. It skips every line not starting with
/// `|`, so a hole between the rows fells nothing: the faults above and below
/// stay correctly numbered and the table goes on «holding together». A merge
/// once removed one at line 64, and an eye found it, not this test. Closing that
/// gap means to stop filtering and measure the block instead: from the first
/// line starting with `|` to the last, every line between must be a table row.
fn faults() -> Vec<Fault> {
    let path = repository_root().join("docs/faults-encountered.md");
    let text = std::fs::read_to_string(&path)
        .unwrap_or_else(|error| panic!("leggere {}: {error}", path.display()));
    let rows: Vec<Fault> = text
        .lines()
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
                // **ONE READING, AND IT LIVES IN THE CRATE.** This test kept
                // its own — `contains` against the crate's `starts_with` — and
                // two hand-written readings of one column drift apart: fault 57
                // between a test and the thing it tests. It asks instead, and
                // in exchange it guards what the crate cannot guard alone: that
                // **no row comes out unrecognised**.
                standing: faults::standing_of(&status),
                cells,
            })
        })
        .collect();
    workspace::measured(rows.len(), "rows of the fault table read");
    rows
}

/// No repeated number, no hole: the test that would have caught the collision.
#[test]
fn every_fault_has_its_own_number_and_none_is_missing() {
    let faults = faults();
    assert!(
        faults.len() >= 25,
        "la tabella si è svuotata: {}",
        faults.len()
    );

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

    let missing: Vec<usize> = (1..=faults.len())
        .filter(|n| !seen.contains_key(n))
        .collect();
    assert!(
        missing.is_empty(),
        "numeri saltati: {missing:?}. Un buco vuol dire che una riga è stata \
         tolta senza rinumerare, e i rinvii da altri documenti puntano al vuoto"
    );
}

/// **The document has no door to refuse at, so it is guarded here.** The store
/// turns away a status the count cannot read; a markdown table takes anything
/// typed into it, and an unreadable status leaves the open tally in silence.
#[test]
fn every_row_says_where_it_stands_in_words_the_count_can_read() {
    let unread: Vec<usize> = faults()
        .iter()
        .filter(|fault| fault.standing == faults::Standing::Unrecognised)
        .map(|fault| fault.number)
        .collect();

    assert!(
        unread.is_empty(),
        "lo stato di {:?} non comincia con nessuno dei marcatori che il conto \
         sa leggere, quindi quei guasti sono già usciti dal conto degli aperti \
         senza che niente lo dicesse. È il difetto che il deposito rifiuta alla \
         porta; qui si sorveglia il documento, che porta non ne ha",
        unread
    );
}

/// The half-done repair is the real case: translating the column one row at a
/// time would drop the open tally with every row translated.
#[test]
fn a_marker_translated_halfway_leaves_the_count_instead_of_lowering_it() {
    assert_eq!(
        faults::standing_of("**open** — measured and not yet built"),
        faults::Standing::Unrecognised,
        "un marcatore tradotto senza insegnare la lettura dev'essere non \
         riconosciuto, non chiuso: la direzione dell'errore sarebbe quella che \
         tranquillizza, e sette righe uscirebbero dal conto in una modifica"
    );
}

/// **AN ENTRY WITH NO «WHAT WOULD HAVE STOPPED IT» IS NOT FINISHED**, as the
/// file says in its own header. A fault with no sequel is a diary, which is
/// exactly what that file declares it is not.
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
        for (column, name) in [
            "numero",
            "data",
            "cosa è successo",
            "come si è visto",
            "cosa lo impedirebbe",
            "stato",
        ]
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

/// The numbers up to nineteen, which follow no rule at all in Italian.
const IRREGULAR: [&str; 20] = [
    "zero",
    "uno",
    "due",
    "tre",
    "quattro",
    "cinque",
    "sei",
    "sette",
    "otto",
    "nove",
    "dieci",
    "undici",
    "dodici",
    "tredici",
    "quattordici",
    "quindici",
    "sedici",
    "diciassette",
    "diciotto",
    "diciannove",
];

/// The tens.
const TENS: [&str; 10] = [
    "",
    "",
    "venti",
    "trenta",
    "quaranta",
    "cinquanta",
    "sessanta",
    "settanta",
    "ottanta",
    "novanta",
];

/// The number written in letters, the way the prose under the table writes it.
///
/// **THIS WAS ONCE A LIST OF FORTY-EIGHT HAND-WRITTEN NUMBERS**, and every new
/// fault forced it longer — three times in one afternoon, failing each time with
/// «no word for N: extend IN_WORDS». A list that grows with the data is not a
/// translation but a debt on monthly instalments, and a hand-written list can
/// hold a typo nobody sees, being the only source to check it against. The rules
/// instead are three, and they do not change: below twenty there is no rule and
/// the words are listed; above it, ten plus unit; **the ten loses its final
/// vowel before «uno» and «otto»** (ventuno, ventotto) and **«tre» takes an
/// accent in the tail** (ventitré). That is all.
fn spelled(number: usize) -> String {
    if number < 20 {
        return IRREGULAR[number].to_string();
    }
    assert!(
        number < 1000,
        "la prosa non ha mai scritto un numero a quattro cifre in lettere: se serve, \
         la regola delle migliaia va aggiunta qui invece che aggirata"
    );
    if number >= 100 {
        return with_hundreds(number);
    }
    let (ten, unit) = (number / 10, number % 10);
    let tens = TENS[ten];
    match unit {
        0 => tens.to_string(),
        // The ten is truncated before the two vowels that open.
        1 | 8 => format!("{}{}", &tens[..tens.len() - 1], IRREGULAR[unit]),
        // «tre» in the tail carries the accent: ventitré, never ventitre.
        3 => format!("{tens}tré"),
        _ => format!("{tens}{}", IRREGULAR[unit]),
    }
}

/// The hundreds: «cento» keeps its vowel before «uno» — centouno — but before
/// «otto» the two o merge into one, centotto. Past it the rule is the same.
fn with_hundreds(number: usize) -> String {
    let (hundred, rest) = (number / 100, number % 100);
    let prefix = match hundred {
        1 => "cento".to_string(),
        _ => format!("{}cento", IRREGULAR[hundred]),
    };
    if rest == 0 {
        return prefix;
    }
    // «tre» takes its accent after a hundred as it does after a ten.
    let tail = match rest {
        3 => "tré".to_string(),
        _ => spelled(rest),
    };
    match tail.starts_with('o') {
        true => format!("{}{tail}", &prefix[..prefix.len() - 1]),
        false => format!("{prefix}{tail}"),
    }
}

/// **A TRANSLATOR MUST BE CHECKED.** The old version was a hand-written list:
/// right or wrong, it was still the only source, so a typo could not be seen. A
/// function goes wrong differently — on the exceptions — and those are what this
/// test lists, not every number. The chosen cases are the three points where the
/// general rule falls short: the truncation before «uno» and «otto», the accent
/// on «tré» in the tail, and the round tens.
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
        (100, "cento"),
        (101, "centouno"),
        (103, "centotré"),
        (108, "centotto"),
        (121, "centoventuno"),
        (180, "centottanta"),
        (200, "duecento"),
        (308, "trecentotto"),
        (47, "quarantasette"),
        (68, "sessantotto"),
        (91, "novantuno"),
        (93, "novantatré"),
    ] {
        assert_eq!(spelled(number), word, "{number} si scrive «{word}»");
    }
}

/// **THE COUNTS IN THE PROSE TELL THE TRUTH.** Under the table the file writes
/// how many faults are still open, out of how many. Those two numbers can be
/// counted, and while a person copies them by hand they diverge: they were wrong
/// in four documents out of four.
#[test]
fn the_counts_written_in_prose_match_the_table_they_come_from() {
    let faults = faults();
    let open = faults.iter().filter(|fault| fault.still_open()).count();
    let total = faults.len();

    let path = repository_root().join("docs/faults-encountered.md");
    let text = std::fs::read_to_string(&path).expect("leggere il file dei guasti");
    let prose = text
        .split_once("## Cosa dice questa tabella")
        .map(|(_, after)| after.to_owned())
        .expect("la sezione che commenta la tabella");

    // The capital goes on the word, not on the asterisk before it: the sentence
    // opens with `**`, and capitalising the first character left both forms
    // identical — the test stayed red over prose that was already right.
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
