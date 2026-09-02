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

use models::remaining::Remaining;

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
pub const USAGE: &[crate::Form] = &[crate::Form {
    form: "sailor remaining",
    says_key: "",
}];

fn dispatch(args: &[String]) -> Result<String, String> {
    if !args.is_empty() {
        return Err(format!(
            "{} {}",
            catalogue::say("cli.usage_heading", &[]),
            USAGE[0].form
        ));
    }
    let now = now_secs()?;
    // **EVERY ENGINE THAT DECLARES A CHANNEL, NONE NAMED HERE.** The catalogue
    // says who can be asked; an engine that cannot is not in the list, and a
    // channel that does not answer is a line saying so, never a zero.
    let machine = toolbox::Machine::current();
    let catalog = toolbox::Catalog::load(&toolbox::default_sources(&machine));
    let readings = toolbox::quota::read_all(&catalog, &machine, now);
    if readings.is_empty() {
        return Err(catalogue::say("cli.remaining.no_channel", &[]));
    }
    let mut found = Vec::new();
    let mut refused = Vec::new();
    for reading in readings {
        match reading.result {
            Ok(windows) => found.extend(windows),
            Err(why) => refused.push(format!("{} · cannot read: {why}", reading.engine)),
        }
    }
    if found.is_empty() {
        return Err(refused.join("\n"));
    }
    let mut said = report(&found);
    for line in refused {
        said.push('\n');
        said.push_str(&line);
    }
    Ok(said)
}

/// Le quote per una persona, una per riga.
///
/// **SI SCRIVE «CONSUMATO», NON «RIMASTO».** Il fornitore dichiara quanto è
/// andato; il resto sarebbe una sottrazione fatta da noi, e su una finestra che
/// non dice qual è il suo tetto una sottrazione è un'invenzione. Il nome del
/// comando dice la domanda, la riga dice la misura.
fn report(found: &[Remaining]) -> String {
    if found.is_empty() {
        return catalogue::say("cli.remaining.no_window", &[]);
    }
    let mut lines = vec![catalogue::say("cli.remaining.whose_quota", &[])];
    for entry in found {
        let resets = match &entry.resets_at {
            Some(when) => format!(", resets on {when}"),
            None => String::new(),
        };
        lines.push(format!(
            "{} · {}: used {:.1}%{resets}",
            entry.engine,
            entry.unit,
            entry.used_fraction * 100.0
        ));
    }
    lines.join("\n")
}

fn now_secs() -> Result<i64, String> {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|since| since.as_secs() as i64)
        .map_err(|error| catalogue::say("cli.clock_before_epoch", &[("error", &error.to_string())]))
}

#[cfg(test)]
mod tests {
    use super::*;
    use models::remaining::RemainingError;

    fn a_window(unit: &str, used_fraction: f64, resets_at: Option<&str>) -> Remaining {
        Remaining {
            engine: "an-engine".to_owned(),
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
            first.contains("PERSON") && first.contains("not a run"),
            "the warning goes at the top, not the bottom: {said}"
        );
    }

    #[test]
    fn every_window_is_a_line_with_its_reset() {
        let said = report(&[
            a_window("five_hour", 0.5, Some("2026-09-01T03:29:59+00:00")),
            a_window("seven_day", 0.32, None),
        ]);
        assert!(
            said.contains("an-engine · five_hour: used 50.0%"),
            "{said}"
        );
        assert!(
            said.contains("resets on 2026-09-01T03:29:59+00:00"),
            "{said}"
        );
        assert!(
            said.contains("an-engine · seven_day: used 32.0%"),
            "{said}"
        );
        assert!(
            !said.contains("seven_day: used 32.0%, resets"),
            "an instant the provider does not give is not invented: {said}"
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
