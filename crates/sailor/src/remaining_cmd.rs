//! `sailor remaining`: quanta quota **una persona** ha già consumato, letta dal
//! motore invece che chiesta a chi lavora.
//!
//! **PERCHÉ UN COMANDO SUO E NON UNA RIGA DENTRO `flow cost`.** Perché è
//! un'altra domanda, e metterle nello stesso rapporto le farebbe leggere come
//! la stessa. `flow cost` risponde a «quanto è costata **questa corsa**»; questo
//! risponde a «quanto **le è rimasto**, contando tutto quello che ha fatto
//! altrove». Un numero sotto l'altro, nello stesso riquadro, si sottrae — ed è
//! il modo in cui una misura giusta diventa una conclusione falsa.
//!
//! **NON COSTA NIENTE.** Non invoca nessun motore: chiede a un indirizzo quanto
//! è già stato consumato. Per questo lo si può chiamare prima di decidere se
//! lanciare qualcosa, che è l'unico momento in cui serve.

use models::remaining::{self, Remaining};
use std::path::PathBuf;

pub fn run(args: &[String]) -> i32 {
    match dispatch(args) {
        Ok(message) => {
            println!("{message}");
            0
        }
        Err(message) => {
            eprintln!("sailor remaining: {message}");
            1
        }
    }
}

/// La forma di `sailor remaining`. Vedi `flow_cmd::USAGE`.
pub const USAGE: &[&str] = &["sailor remaining"];

fn dispatch(args: &[String]) -> Result<String, String> {
    if !args.is_empty() {
        return Err(format!("usage: {}", USAGE[0]));
    }
    let home = home_dir()?;
    let now = now_secs()?;
    match remaining::read_from_claude(&home, now) {
        Ok(found) => Ok(report(&found)),
        // **UN CANALE CHE NON RISPONDE NON È UN GUASTO DI SAILOR.** È beta e
        // versionato: il giorno che cambia, chi lo chiedeva deve sapere che la
        // misura non c'è — mai crederla a zero, che è la direzione
        // rassicurante.
        Err(why) => Err(format!("{why}")),
    }
}

/// Le quote per una persona, una per riga.
///
/// **SI SCRIVE «CONSUMATO», NON «RIMASTO».** Il fornitore dichiara quanto è
/// andato; il resto sarebbe una sottrazione fatta da noi, e su una finestra che
/// non dice qual è il suo tetto una sottrazione è un'invenzione. Il nome del
/// comando dice la domanda, la riga dice la misura.
fn report(found: &[Remaining]) -> String {
    if found.is_empty() {
        return "no quota window declared: the channel answered with no measurements"
            .to_owned();
    }
    let mut lines = vec![
        "quota della PERSONA, non di una corsa: conta ogni sessione, anche quelle \
         fuori da Sailor"
            .to_owned(),
    ];
    for entry in found {
        let resets = match &entry.resets_at {
            Some(when) => format!(", resets on {when}"),
            None => String::new(),
        };
        lines.push(format!(
            "{} · {}: consumato {:.1}%{resets}",
            entry.engine,
            entry.unit,
            entry.used_fraction * 100.0
        ));
    }
    lines.join("\n")
}

/// La casa della persona. Senza, non c'è nessun posto dove cercare credenziali
/// di nessuno.
fn home_dir() -> Result<PathBuf, String> {
    std::env::var_os("HOME")
        .map(PathBuf::from)
        .ok_or_else(|| "HOME is not set: there is no telling where the person's home is".to_owned())
}

fn now_secs() -> Result<i64, String> {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|since| since.as_secs() as i64)
        .map_err(|_| "the machine's clock is before 1970".to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;
    use models::remaining::RemainingError;

    fn a_window(unit: &str, used_fraction: f64, resets_at: Option<&str>) -> Remaining {
        Remaining {
            engine: remaining::CLAUDE_CODE.to_owned(),
            unit: unit.to_owned(),
            used_fraction,
            resets_at: resets_at.map(str::to_owned),
            observed_at: 1_788_000_000,
        }
    }

    /// **LA RIGA DICE DI CHI È LA QUOTA, PRIMA DI DIRE QUANTA.** È l'avvertenza
    /// che rende questo numero utilizzabile: senza, chi lo legge sotto un
    /// rapporto di consumo lo attribuisce alla corsa che ha appena guardato, e
    /// una quota di sette giorni attribuita a una corsa di dieci minuti è un
    /// errore di due ordini di grandezza.
    #[test]
    fn the_report_says_whose_quota_it_is_before_saying_how_much() {
        let said = report(&[a_window("seven_day", 0.32, None)]);
        let first = said.lines().next().expect("almeno una riga");
        assert!(
            first.contains("PERSONA") && first.contains("non di una corsa"),
            "l'avvertenza sta in cima, non in fondo: {said}"
        );
    }

    #[test]
    fn every_window_is_a_line_with_its_reset() {
        let said = report(&[
            a_window("five_hour", 0.5, Some("2026-09-01T03:29:59+00:00")),
            a_window("seven_day", 0.32, None),
        ]);
        assert!(
            said.contains("claude-code · five_hour: consumato 50.0%"),
            "{said}"
        );
        assert!(
            said.contains("resets on 2026-09-01T03:29:59+00:00"),
            "{said}"
        );
        assert!(
            said.contains("claude-code · seven_day: consumato 32.0%"),
            "{said}"
        );
        assert!(
            !said.contains("seven_day: consumato 32.0%, si azzera"),
            "un istante che il fornitore non dice non si inventa: {said}"
        );
    }

    /// **NESSUNA FINESTRA NON È «QUOTA LIBERA».** Una risposta senza misure è
    /// una risposta senza misure, e dirlo con uno zero manderebbe a lanciare
    /// proprio quando non si sa.
    #[test]
    fn no_window_at_all_is_said_and_never_shown_as_zero() {
        let said = report(&[]);
        assert!(said.contains("no quota window"), "{said}");
        assert!(!said.contains("0.0%"), "{said}");
    }

    /// Il canale è beta: quando smetterà di rispondere, il comando dice cosa è
    /// successo invece di far finta di aver misurato.
    #[test]
    fn a_channel_that_does_not_answer_is_reported_and_not_guessed() {
        let said = format!("{}", RemainingError::NotUnderstood);
        assert!(said.contains("beta"), "{said}");
    }
}
