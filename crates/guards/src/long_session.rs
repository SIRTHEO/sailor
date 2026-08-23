//! Una sessione lunga costa di più per turno, e lo si dice una volta ogni cento.
//!
//! Misura del 18/08/2026 (`docs/2026-08-18-token-per-sessione.md`): un turno in
//! una sessione da 101-400 turni costa 137k token contro i 62k di una sotto i
//! 25, e 249k fra 401 e 1600 — quattro volte. Il 97% è contesto riletto. Qui
//! non si giudica niente e non si nega niente: al centesimo turno, e a ogni
//! centinaio dopo, si inietta **una riga** che dice il numero e la via d'uscita.
//! Non è `handoff_threshold`, che misura i token: qui il metro sono i turni.
//!
//! COS'È UN TURNO. Una richiesta al servizio: un record con `message.usage`,
//! **deduplicato su `message.id`**, perché lo streaming ripete lo stesso turno
//! su 2-3 righe (misurato il 23/08: 130k righe per 67k turni). Lo script del
//! 18/08 contava le righe grezze, quindi la sua tabella è gonfiata di ~1,85×:
//! i 100 turni di qui valgono ~185 righe di là, e il 4× resta.
//!
//! Il giudizio è puro: entrano il testo del transcript e lo stato già detto,
//! escono il gradino e la riga. Chi tocca stdin, il file e `TMPDIR` sta in
//! `claude-hooks/src/long_session.rs`.

/// Ogni quanti turni si parla.
pub const STEP: u64 = 100;

/// Quante richieste al servizio contiene il transcript.
///
/// Una riga che non è JSON o non ha `message.usage` non conta; un `usage` di
/// primo livello non conta, perché nei transcript di Claude Code non compare
/// mai da solo. Due righe con lo stesso `message.id` sono un turno; una riga
/// senza `id` vale un turno da sola.
pub fn count_turns(text: &str) -> u64 {
    let mut seen = std::collections::HashSet::new();
    let mut without_id = 0u64;
    for line in text.lines().filter(|l| l.contains("\"usage\"")) {
        let Ok(v) = serde_json::from_str::<serde_json::Value>(line) else {
            continue;
        };
        let Some(msg) = v.get("message") else { continue };
        if !msg.get("usage").is_some_and(|u| u.is_object()) {
            continue;
        }
        match msg.get("id").and_then(|i| i.as_str()) {
            Some(id) => {
                seen.insert(id.to_string());
            }
            None => without_id += 1,
        }
    }
    seen.len() as u64 + without_id
}

/// Il gradino raggiunto: il multiplo di `STEP` più alto non oltre `turns`.
pub fn step_reached(turns: u64) -> Option<u64> {
    let step = turns / STEP * STEP;
    (step >= STEP).then_some(step)
}

/// Già detto questo gradino? Lo stato è il numero dell'ultimo annunciato.
pub fn already_said(state: &str, step: u64) -> bool {
    state.trim().parse::<u64>().is_ok_and(|said| said >= step)
}

/// La riga, una sola.
pub fn message(turns: u64) -> String {
    format!("sessione a {turns} turni: un turno qui costa 4× — chiudi con `handoff`")
}

/// Dal transcript e dallo stato alla decisione: `Some((gradino, riga))` quando
/// c'è qualcosa da dire.
pub fn judge(transcript: &str, state: &str) -> Option<(u64, String)> {
    let turns = count_turns(transcript);
    let step = step_reached(turns)?;
    if already_said(state, step) {
        return None;
    }
    Some((step, message(turns)))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn turn(id: usize) -> String {
        format!(
            r#"{{"type":"assistant","message":{{"id":"msg_{id}","usage":{{"input_tokens":1,"cache_read_input_tokens":5}}}}}}"#
        )
    }

    fn transcript(n: usize) -> String {
        (0..n).map(turn).collect::<Vec<_>>().join("\n")
    }

    #[test]
    fn a_turn_is_a_message_id_with_usage() {
        let text = [
            turn(1),
            r#"{"type":"user","message":{"content":"ciao"}}"#.to_string(),
            r#"{"type":"assistant","usage":{"input_tokens":1}}"#.to_string(),
            r#"{"usage": rotta"#.to_string(),
            r#"{"type":"assistant","message":{"usage":"no"}}"#.to_string(),
            turn(2),
        ]
        .join("\n");
        assert_eq!(count_turns(&text), 2);
    }

    #[test]
    fn streaming_repeats_a_turn_but_it_counts_once() {
        // Tre righe con lo stesso `id` e `output_tokens` crescente: un turno.
        // Una riga senza `id` conta da sola.
        let text = [
            turn(7),
            turn(7),
            turn(7),
            r#"{"type":"assistant","message":{"usage":{"input_tokens":1}}}"#.to_string(),
        ]
        .join("\n");
        assert_eq!(count_turns(&text), 2);
    }

    #[test]
    fn silence_below_one_hundred_and_a_line_at_one_hundred() {
        assert_eq!(judge(&transcript(99), ""), None);
        let (step, line) = judge(&transcript(100), "").expect("must speak at 100");
        assert_eq!(step, 100);
        assert_eq!(
            line,
            "sessione a 100 turni: un turno qui costa 4× — chiudi con `handoff`"
        );
    }

    #[test]
    fn each_hundred_is_said_once() {
        assert_eq!(judge(&transcript(150), "100"), None);
        assert_eq!(judge(&transcript(199), "100"), None);
        let (step, line) = judge(&transcript(203), "100").expect("must speak at 200");
        assert_eq!(step, 200);
        assert!(line.starts_with("sessione a 203 turni"));
        // Un gradino già superato non torna indietro.
        assert!(already_said("200", 100));
        assert!(!already_said("", 100));
        assert!(!already_said("boh", 100));
    }

    #[test]
    fn the_line_is_one_line() {
        assert!(!message(100).contains('\n'));
    }
}
