//! Il rapporto: prima i sopravvissuti, poi i conti.
//!
//! Un sopravvissuto è un guasto che nessuna prova ha notato, cioè un controllo
//! che non controlla. Il rapporto non promette di distinguerlo da un guasto
//! che non cambia comportamento — quella distinzione non è decidibile, e
//! prometterla sarebbe la stessa scorciatoia che si sta rimediando: la
//! dichiara aperta e lascia il giudizio a chi legge, dandogli la riga esatta.

use crate::{Fault, Verdict};

/// Un guasto e come è andata.
#[derive(Debug, Clone)]
pub struct Outcome {
    pub fault: Fault,
    pub verdict: Verdict,
    pub seconds: u64,
}

/// I conti di un giro.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Tally {
    pub killed: usize,
    pub survived: usize,
    pub not_viable: usize,
    pub not_applied: usize,
}

impl Tally {
    /// I guasti che hanno detto qualcosa: gli altri non contano nel punteggio.
    pub fn meaningful(&self) -> usize {
        self.killed + self.survived
    }
}

pub fn tally(outcomes: &[Outcome]) -> Tally {
    let mut counts = Tally::default();
    for outcome in outcomes {
        match outcome.verdict {
            Verdict::Killed => counts.killed += 1,
            Verdict::Survived => counts.survived += 1,
            Verdict::NotViable => counts.not_viable += 1,
            Verdict::NotApplied => counts.not_applied += 1,
        }
    }
    counts
}

/// Il rapporto a schermo.
pub fn render(outcomes: &[Outcome]) -> String {
    let counts = tally(outcomes);
    let mut out = String::new();
    let survivors: Vec<&Outcome> = outcomes
        .iter()
        .filter(|outcome| outcome.verdict == Verdict::Survived)
        .collect();

    if survivors.is_empty() {
        out.push_str("Nessun sopravvissuto: ogni guasto applicato ha fatto arrossire la batteria.\n");
    } else {
        out.push_str(&format!(
            "SOPRAVVISSUTI: {} guasti che nessuna prova ha notato.\n\n",
            survivors.len()
        ));
        for outcome in &survivors {
            out.push_str(&format!(
                "  {}:{}  {}\n      - {}\n      + {}\n",
                outcome.fault.file,
                outcome.fault.line,
                outcome.fault.label,
                one_line(&outcome.fault.before),
                one_line(&outcome.fault.after),
            ));
        }
        out.push('\n');
        out.push_str(
            "Un sopravvissuto è un controllo che non controlla, oppure un guasto che non\n\
             cambia comportamento. Questo programma non sa distinguerli: la riga sopra dice\n\
             dove guardare, il giudizio è di chi legge il codice.\n\n",
        );
    }

    out.push_str(&format!(
        "Conti: {} uccisi, {} sopravvissuti, {} non vitali (non compilano), {} non applicati.\n",
        counts.killed, counts.survived, counts.not_viable, counts.not_applied
    ));
    if counts.meaningful() > 0 {
        let score = counts.killed * 100 / counts.meaningful();
        out.push_str(&format!(
            "Presa: {score}% ({} su {} guasti che dicevano qualcosa).\n",
            counts.killed,
            counts.meaningful()
        ));
    } else {
        out.push_str("Nessun guasto ha detto niente: il giro non misura nulla.\n");
    }
    out
}

/// Il codice d'uscita: un sopravvissuto è un fallimento, come un test rosso.
pub fn exit_code(counts: Tally) -> i32 {
    if counts.survived > 0 {
        1
    } else if counts.meaningful() == 0 {
        2
    } else {
        0
    }
}

/// Il testo su una riga sola, perché il rapporto resti leggibile anche quando
/// il guasto ha buttato via trenta righe di funzione.
fn one_line(text: &str) -> String {
    let flat = text.split_whitespace().collect::<Vec<_>>().join(" ");
    if flat.chars().count() <= 90 {
        return flat;
    }
    let head: String = flat.chars().take(87).collect();
    format!("{head}…")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn outcome(verdict: Verdict, label: &str) -> Outcome {
        Outcome {
            fault: Fault {
                file: "src/a.ts".into(),
                line: 12,
                offset: 0,
                length: 3,
                label: label.into(),
                before: "===".into(),
                after: "!==".into(),
            },
            verdict,
            seconds: 1,
        }
    }

    #[test]
    fn the_counts_split_by_verdict() {
        let counts = tally(&[
            outcome(Verdict::Killed, "a"),
            outcome(Verdict::Killed, "b"),
            outcome(Verdict::Survived, "c"),
            outcome(Verdict::NotViable, "d"),
            outcome(Verdict::NotApplied, "e"),
        ]);
        assert_eq!(counts.killed, 2);
        assert_eq!(counts.survived, 1);
        assert_eq!(counts.not_viable, 1);
        assert_eq!(counts.not_applied, 1);
        assert_eq!(counts.meaningful(), 3);
    }

    #[test]
    fn a_survivor_is_named_with_its_line() {
        let text = render(&[outcome(Verdict::Survived, "confine di stringa tolto in coda")]);
        assert!(text.contains("src/a.ts:12"), "{text}");
        assert!(text.contains("confine di stringa"), "{text}");
        assert!(text.contains("non sa distinguerli"), "{text}");
    }

    #[test]
    fn a_clean_run_says_so() {
        let text = render(&[outcome(Verdict::Killed, "a")]);
        assert!(text.contains("Nessun sopravvissuto"), "{text}");
        assert!(text.contains("Presa: 100%"), "{text}");
    }

    /// Un giro che non misura niente non è un giro riuscito: uscire 0 lo
    /// farebbe passare per verde in una catena di comandi.
    #[test]
    fn a_run_that_measured_nothing_is_not_green() {
        assert_eq!(exit_code(Tally::default()), 2);
        assert_eq!(
            exit_code(Tally {
                killed: 0,
                survived: 1,
                not_viable: 0,
                not_applied: 0
            }),
            1
        );
        assert_eq!(
            exit_code(Tally {
                killed: 3,
                survived: 0,
                not_viable: 1,
                not_applied: 0
            }),
            0
        );
    }

    #[test]
    fn a_long_body_is_folded_into_one_line() {
        let long = "a".repeat(200);
        let text = one_line(&long);
        assert!(text.chars().count() <= 88, "{}", text.chars().count());
        assert!(text.ends_with('…'));
    }
}
