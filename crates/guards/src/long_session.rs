//! Una sessione lunga costa di più per turno, e lo si dice una volta ogni cento.
//!
//! Misura del 18/08/2026 (`docs/2026-08-18-token-per-sessione.md`): un turno in
//! una sessione da 101-400 turni costa 137k token contro i 62k di una sotto i
//! 25, e 249k fra 401 e 1600 — quattro volte. Il 97% è contesto riletto. Qui
//! non si giudica niente e non si nega niente: si inietta **una riga**.
//! Non è `handoff_threshold`, che misura i token: qui il metro sono i turni.
//!
//! LA RIGA CAMBIA A SECONDA DI CHI LA LEGGE, e questa è la ragione per cui il
//! modulo non è più una `format!`. Una sessione che non ha ancora consegnato e
//! una che ha consegnato e prosegue sono situazioni opposte: alla prima manca
//! il documento, alla seconda manca solo di chiudere. Dirgli la stessa cosa
//! rende la riga sfondo. E una riga già ignorata deve portare un numero che
//! cresce, perché una frase ripetuta uguale insegna che non succede niente.
//!
//! COS'È UN TURNO. Una richiesta al servizio: un record con `message.usage`,
//! **deduplicato su `message.id`**, perché lo streaming ripete lo stesso turno
//! su 2-3 righe (misurato il 23/08: 130k righe per 67k turni). Lo script del
//! 18/08 contava le righe grezze, quindi la sua tabella è gonfiata di ~1,85×:
//! i 100 turni di qui valgono ~185 righe di là, e il 4× resta.
//!
//! Il giudizio è puro: entrano il testo del transcript e lo stato già detto,
//! escono lo stato nuovo e la riga. Chi tocca stdin, il file e `TMPDIR` sta in
//! `claude-hooks/src/long_session.rs`.

/// Ogni quanti turni si parla.
pub const STEP: u64 = 100;

/// Cosa il transcript dice, letto una volta sola.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Scan {
    /// Richieste al servizio, deduplicate.
    pub turns: u64,
    /// Il turno in cui la skill `handoff` è stata invocata l'ultima volta.
    ///
    /// La prova è l'invocazione della skill, non un file che si chiama
    /// `consegna-*.md`: scrivere quel documento non è aver consegnato — lo dice
    /// `handoff::is_handoff_call`, che qui si riusa invece di riscriverlo.
    pub handoff_turn: Option<u64>,
    /// Token entrati nel contesto dopo la consegna: il costo di quel tratto.
    pub tokens_since_handoff: u64,
    /// Il file che la consegna ha scritto: il punto di ripresa da nominare.
    ///
    /// Qui la scrittura di `memory/consegna-*.md` serve solo a dare un NOME al
    /// documento, mai a dedurre che si sia consegnato — la deduzione resta al
    /// campo qui sopra.
    pub resume_path: String,
}

impl Scan {
    /// Turni passati dalla consegna, zero se consegna non ce n'è.
    pub fn turns_since_handoff(&self) -> u64 {
        self.handoff_turn
            .map(|h| self.turns.saturating_sub(h))
            .unwrap_or(0)
    }
}

/// Quello che la sessione si è già sentita dire.
///
/// Si serializza su una riga sola, campi separati da spazio. Un file vecchio
/// contiene il solo numero del gradino: si legge come `repeats = 0`, così
/// adottare questa versione non fa ripartire da capo gli avvisi.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct State {
    /// L'ultimo centinaio annunciato.
    pub said: u64,
    /// Quanti avvisi sono già stati dati **dopo** la consegna.
    pub repeats: u64,
    /// Turni dalla consegna all'avviso precedente: serve a dire «erano N».
    pub last_turns_since: u64,
    /// Il turno di consegna per cui si è già parlato, zero se nessuno.
    pub handoff_said: u64,
}

impl State {
    pub fn parse(text: &str) -> State {
        let mut f = text.split_whitespace();
        let mut next = || f.next().and_then(|v| v.parse::<u64>().ok()).unwrap_or(0);
        State {
            said: next(),
            repeats: next(),
            last_turns_since: next(),
            handoff_said: next(),
        }
    }

    pub fn render(&self) -> String {
        format!(
            "{} {} {} {}",
            self.said, self.repeats, self.last_turns_since, self.handoff_said
        )
    }
}

/// Quante richieste al servizio contiene il transcript.
///
/// Una riga che non è JSON o non ha `message.usage` non conta; un `usage` di
/// primo livello non conta, perché nei transcript di Claude Code non compare
/// mai da solo. Due righe con lo stesso `message.id` sono un turno; una riga
/// senza `id` vale un turno da sola.
pub fn count_turns(text: &str) -> u64 {
    scan(text).turns
}

/// Il turno raggiunto, la consegna e il suo costo, in una passata sola.
pub fn scan(text: &str) -> Scan {
    let mut seen = std::collections::HashSet::new();
    let mut out = Scan::default();
    for line in text.lines() {
        let interesting = line.contains("\"usage\"") || line.contains("tool_use");
        if !interesting {
            continue;
        }
        let Ok(v) = serde_json::from_str::<serde_json::Value>(line) else {
            continue;
        };
        let Some(msg) = v.get("message") else { continue };
        if let Some(parts) = msg.get("content").and_then(|c| c.as_array()) {
            for p in parts {
                if p.get("type").and_then(|t| t.as_str()) != Some("tool_use") {
                    continue;
                }
                let name = p.get("name").and_then(|n| n.as_str()).unwrap_or("");
                if crate::handoff::is_handoff_call(name, p.get("input")) {
                    // Una consegna nuova azzera il conto del tratto che segue:
                    // il costo da mostrare è quello dopo l'ULTIMA consegna.
                    out.handoff_turn = Some(out.turns);
                    out.tokens_since_handoff = 0;
                }
                if let Some(path) = handoff_document(name, p.get("input")) {
                    out.resume_path = path;
                }
            }
        }
        let Some(usage) = msg.get("usage").filter(|u| u.is_object()) else {
            continue;
        };
        // I token si contano sulla PRIMA riga di un turno, come i turni: le
        // ripetizioni dello streaming portano lo stesso ingresso, e sommarle
        // triplicherebbe il costo dichiarato.
        let fresh = match msg.get("id").and_then(|i| i.as_str()) {
            Some(id) => seen.insert(id.to_string()),
            None => true,
        };
        if !fresh {
            continue;
        }
        out.turns += 1;
        if out.handoff_turn.is_some() {
            let field = |n: &str| usage.get(n).and_then(|v| v.as_u64()).unwrap_or(0);
            out.tokens_since_handoff += field("input_tokens")
                + field("cache_read_input_tokens")
                + field("cache_creation_input_tokens");
        }
    }
    out
}

/// Il percorso del documento di consegna, se questa chiamata lo scrive.
fn handoff_document(tool: &str, input: Option<&serde_json::Value>) -> Option<String> {
    if tool != "Write" && tool != "Edit" {
        return None;
    }
    let path = input?.get("file_path")?.as_str()?;
    let name = path.rsplit('/').next().unwrap_or("");
    (path.contains("/memory/") && name.starts_with("consegna") && name.ends_with(".md"))
        .then(|| path.to_string())
}

/// Il gradino raggiunto: il multiplo di `STEP` più alto non oltre `turns`.
pub fn step_reached(turns: u64) -> Option<u64> {
    let step = turns / STEP * STEP;
    (step >= STEP).then_some(step)
}

/// Già detto questo gradino? Lo stato è il numero dell'ultimo annunciato.
pub fn already_said(state: &str, step: u64) -> bool {
    State::parse(state).said >= step && step > 0
}

/// La riga di chi non ha ancora consegnato: identica a quella di sempre.
pub fn message(turns: u64) -> String {
    format!("sessione a {turns} turni: un turno qui costa 4× — chiudi con `handoff`")
}

/// La coda che indica il punto di ripresa, vuota se il file non si conosce.
fn resume_tail(path: &str) -> String {
    if path.is_empty() {
        String::new()
    } else {
        format!(" Punto di ripresa, da leggere per primo: {path}")
    }
}

/// La riga di chi ha già consegnato e sta continuando.
///
/// Il primo avviso spiega cosa sta succedendo; dal secondo in poi la frase
/// cambia e porta il confronto fra il numero di prima e quello di adesso —
/// è l'unica parte che un lettore distratto nota.
pub fn message_after_handoff(scan: &Scan, resume: &str, state: &State) -> String {
    let since = scan.turns_since_handoff();
    let cost = crate::handoff::thousands(scan.tokens_since_handoff);
    let tail = resume_tail(resume);
    if state.repeats == 0 {
        format!(
            "consegna già scritta: da lì {since} turni e {cost} token. \
             Quello che stai facendo adesso è un mandato nuovo, e un mandato nuovo \
             vuole una sessione nuova.{tail}"
        )
    } else {
        let n = state.repeats + 1;
        format!(
            "{n}º avviso da quando hai consegnato, e la sessione va avanti: \
             dalla consegna {since} turni (erano {}) e {cost} token rispediti. \
             Il tratto cresce a ogni turno; il mandato nuovo va in una sessione nuova.{tail}",
            state.last_turns_since
        )
    }
}

/// Dal transcript e dallo stato alla decisione: `Some((stato nuovo, riga))`
/// quando c'è qualcosa da dire.
///
/// DUE MOTIVI PER PARLARE, non uno. Il gradino dei cento turni è quello di
/// sempre. Il secondo è la consegna appena scritta: è l'istante esatto in cui
/// comincia un mandato nuovo, e aspettare il centinaio successivo lo direbbe a
/// cento turni di distanza — o mai, se la sessione consegna a 305 e si ferma a
/// 380. Ciascuno dei due parla una volta sola per il proprio innesco.
pub fn judge(transcript: &str, resume_fallback: &str, state_text: &str) -> Option<(String, String)> {
    let s = scan(transcript);
    let mut st = State::parse(state_text);
    let step = step_reached(s.turns);
    let due_step = step.is_some_and(|k| st.said < k);
    let due_handoff = s.handoff_turn.is_some_and(|h| st.handoff_said != h);
    if !due_step && !due_handoff {
        return None;
    }
    if let Some(k) = step {
        st.said = st.said.max(k);
    }
    let line = match s.handoff_turn {
        None => message(s.turns),
        Some(h) => {
            let resume = if s.resume_path.is_empty() {
                resume_fallback
            } else {
                &s.resume_path
            };
            let line = message_after_handoff(&s, resume, &st);
            st.repeats += 1;
            st.last_turns_since = s.turns_since_handoff();
            st.handoff_said = h;
            line
        }
    };
    Some((st.render(), line))
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

    /// La riga con cui una sessione invoca la skill `handoff`.
    fn handoff_call() -> String {
        r#"{"type":"assistant","message":{"content":[{"type":"tool_use","name":"Skill","input":{"skill":"handoff"}}]}}"#.to_string()
    }

    /// La scrittura del documento, che dà il nome al punto di ripresa.
    fn handoff_write(path: &str) -> String {
        format!(
            r#"{{"type":"assistant","message":{{"content":[{{"type":"tool_use","name":"Write","input":{{"file_path":"{path}"}}}}]}}}}"#
        )
    }

    const DOC: &str = "/home/someone/.claude/projects/-Users-theo-orca-general/memory/consegna-x.md";

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
        assert_eq!(judge(&transcript(99), "", ""), None);
        let (state, line) = judge(&transcript(100), "", "").expect("must speak at 100");
        assert_eq!(State::parse(&state).said, 100);
        assert_eq!(
            line,
            "sessione a 100 turni: un turno qui costa 4× — chiudi con `handoff`"
        );
    }

    #[test]
    fn each_hundred_is_said_once() {
        assert_eq!(judge(&transcript(150), "", "100"), None);
        assert_eq!(judge(&transcript(199), "", "100"), None);
        let (state, line) = judge(&transcript(203), "", "100").expect("must speak at 200");
        assert_eq!(State::parse(&state).said, 200);
        assert!(line.starts_with("sessione a 203 turni"));
        // Un gradino già superato non torna indietro.
        assert!(already_said("200", 100));
        assert!(!already_said("", 100));
        assert!(!already_said("boh", 100));
    }

    #[test]
    fn the_line_is_one_line() {
        assert!(!message(100).contains('\n'));
        let s = Scan {
            turns: 300,
            handoff_turn: Some(200),
            tokens_since_handoff: 1_234_567,
            resume_path: DOC.into(),
        };
        assert!(!message_after_handoff(&s, DOC, &State::default()).contains('\n'));
    }

    #[test]
    fn an_old_state_file_with_a_bare_number_still_reads() {
        // I file già sul disco contengono `100` e nient'altro: adottare i campi
        // nuovi non deve far ricominciare gli avvisi da capo.
        assert_eq!(
            State::parse("100"),
            State { said: 100, repeats: 0, last_turns_since: 0, handoff_said: 0 }
        );
        assert_eq!(State::parse("").said, 0);
        assert_eq!(State::parse("200 3 47 150").repeats, 3);
    }

    #[test]
    fn the_scan_finds_the_handoff_and_what_it_cost_after() {
        let mut lines = vec![turn(1), turn(2), handoff_call(), handoff_write(DOC)];
        lines.push(turn(3));
        lines.push(turn(4));
        let s = scan(&lines.join("\n"));
        assert_eq!(s.turns, 4);
        assert_eq!(s.handoff_turn, Some(2));
        assert_eq!(s.turns_since_handoff(), 2);
        // Due turni dopo la consegna, sei token d'ingresso ciascuno.
        assert_eq!(s.tokens_since_handoff, 12);
        assert_eq!(s.resume_path, DOC);
    }

    #[test]
    fn a_second_handoff_restarts_the_count() {
        // Il costo che si mostra è quello dell'ULTIMO tratto, non della somma:
        // chi ha consegnato due volte sta al secondo mandato di troppo, non al
        // primo, e il numero deve parlare di quello.
        let lines = [
            turn(1),
            handoff_call(),
            turn(2),
            turn(3),
            handoff_call(),
            turn(4),
        ];
        let s = scan(&lines.join("\n"));
        assert_eq!(s.handoff_turn, Some(3));
        assert_eq!(s.tokens_since_handoff, 6);
    }

    #[test]
    fn a_document_that_is_not_a_handoff_does_not_name_the_resume_point() {
        let lines = [
            handoff_call(),
            handoff_write("/home/someone/orca/general/docs/consegna-finta.md"),
            handoff_write("/home/someone/.claude/projects/p/memory/nota.md"),
        ];
        assert_eq!(scan(&lines.join("\n")).resume_path, "");
    }

    #[test]
    fn writing_the_document_alone_is_not_a_handoff() {
        // Il difetto del 13/08/2026: cinque marcatori falsi da chi stava solo
        // scrivendo un file che si chiamava così. La prova resta la skill.
        let s = scan(&[turn(1), handoff_write(DOC), turn(2)].join("\n"));
        assert_eq!(s.handoff_turn, None);
    }

    #[test]
    fn without_a_handoff_the_line_is_the_old_one() {
        let (_, line) = judge(&transcript(100), "", "").expect("must speak");
        assert!(line.contains("chiudi con `handoff`"), "{line}");
        assert!(!line.contains("mandato nuovo"), "{line}");
    }

    #[test]
    fn with_a_handoff_the_line_says_the_new_thing_and_names_the_file() {
        let mut lines: Vec<String> = (0..100).map(turn).collect();
        lines.push(handoff_call());
        lines.push(handoff_write(DOC));
        let (state, line) = judge(&lines.join("\n"), "", "").expect("must speak");
        assert!(line.contains("consegna già scritta"), "{line}");
        assert!(line.contains("mandato nuovo vuole una sessione nuova"), "{line}");
        assert!(line.contains(DOC), "{line}");
        assert!(!line.contains("chiudi con `handoff`"), "{line}");
        assert_eq!(State::parse(&state).repeats, 1);
        assert_eq!(State::parse(&state).handoff_said, 100);
    }

    #[test]
    fn the_handoff_speaks_even_when_no_hundred_has_been_crossed() {
        // Chi consegna a 305 e si ferma a 380 non incontrerebbe mai il gradino
        // dei quattrocento: senza questo innesco la riga nuova non la vedrebbe.
        let mut lines: Vec<String> = (0..30).map(turn).collect();
        lines.push(handoff_call());
        let (state, line) = judge(&lines.join("\n"), "", "").expect("must speak");
        assert!(line.contains("consegna già scritta"), "{line}");
        // E non si ripete al prompt dopo, se la consegna è la stessa.
        assert_eq!(judge(&lines.join("\n"), "", &state), None);
    }

    #[test]
    fn the_second_time_the_message_changes_and_the_number_grows() {
        let mut lines: Vec<String> = (0..100).map(turn).collect();
        lines.push(handoff_call());
        lines.push(handoff_write(DOC));
        let (state, first) = judge(&lines.join("\n"), "", "").expect("primo avviso");
        // Cento turni dopo, la stessa consegna e nessuna nuova.
        lines.extend((100..200).map(turn));
        let (state2, second) = judge(&lines.join("\n"), "", &state).expect("secondo avviso");
        assert_ne!(first, second);
        assert!(second.starts_with("2º avviso da quando hai consegnato"), "{second}");
        assert!(second.contains("dalla consegna 100 turni (erano 0)"), "{second}");
        assert!(second.contains("600 token"), "{second}");
        assert!(second.contains(DOC), "{second}");
        // E il terzo cresce ancora.
        lines.extend((200..300).map(turn));
        let (_, third) = judge(&lines.join("\n"), "", &state2).expect("terzo avviso");
        assert!(third.starts_with("3º avviso"), "{third}");
        assert!(third.contains("dalla consegna 200 turni (erano 100)"), "{third}");
    }

    #[test]
    fn without_a_known_document_the_fallback_names_the_index() {
        let mut lines: Vec<String> = (0..100).map(turn).collect();
        lines.push(handoff_call());
        let index = "/home/someone/.claude/projects/p/memory/MEMORY.md";
        let (_, line) = judge(&lines.join("\n"), index, "").expect("must speak");
        assert!(line.contains(index), "{line}");
        // E senza nemmeno il ripiego, la riga non promette un percorso vuoto.
        let (_, bare) = judge(&lines.join("\n"), "", "").expect("must speak");
        assert!(!bare.contains("Punto di ripresa"), "{bare}");
    }
}
