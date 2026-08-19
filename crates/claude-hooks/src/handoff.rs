//! La parte con disco e stato della misura di consegna.
//!
//! Il giudizio puro sta in `guards::handoff`; qui c'è solo ciò che ha bisogno
//! del filesystem: leggere la coda di un transcript, e il memo per sessione che
//! evita di riscorrerla a ogni chiamata.
//!
//! IL MEMO NON È UNA CACHE QUALSIASI. Fra due misure vale l'ultima, ed è un
//! **limite inferiore**: il contesto dentro un turno non cala mai. Per questo si
//! può rispondere con un valore vecchio senza sbagliare direzione — al più si
//! consegna un turno più tardi, mai un turno troppo presto.

use guards::handoff::{context_used_found, thresholds_from_lines, Thresholds, MIN_GROWTH,
                      TAIL_BYTES};
use std::fs;
use std::io::{Read, Seek, SeekFrom};
use std::path::PathBuf;

/// Dove vivono i marcatori dei presidi della consegna.
///
/// Sede unica: prima ce n'erano tre copie identiche — qui, nel presidio
/// PostToolUse e nell'involucro del successore — e tre copie divergono la prima
/// volta che qualcuno cambia idea su dove tenere lo stato.
pub(crate) fn state_dir() -> PathBuf {
    dirs_home().join(".claude").join("state")
}

fn dirs_home() -> PathBuf {
    std::env::var("HOME").map(PathBuf::from).unwrap_or_default()
}

/// Le ultime righe del transcript, vuoto in caso di errore.
///
/// Si legge dalla coda perché un transcript di sessione lunga arriva a centinaia
/// di MB. Il taglio cade a metà di una riga e quella riga risulta illeggibile:
/// va bene, chi la usa scorre all'indietro e la salta — ma è il motivo per cui
/// un parse fallito non deve mai interrompere la ricerca.
pub fn transcript_tail(transcript: &str) -> String {
    let Ok(meta) = fs::metadata(transcript) else {
        return String::new();
    };
    let size = meta.len();
    let Ok(mut f) = fs::File::open(transcript) else {
        return String::new();
    };
    if f.seek(SeekFrom::Start(size.saturating_sub(TAIL_BYTES))).is_err() {
        return String::new();
    }
    let mut buf = Vec::new();
    if f.read_to_end(&mut buf).is_err() {
        return String::new();
    }
    String::from_utf8_lossy(&buf).into_owned()
}

/// La sessione ha ricevuto lavoro **dopo** aver consegnato.
///
/// IL DIFETTO CHE CHIUDE, e non è un'ipotesi: il 17/08/2026 la staffetta ha
/// chiuso due volte la stessa sessione mentre lavorava. `cdca7b36` ha consegnato
/// alle 11:35:35, ha ricevuto un mandato da un'altra sessione ventun secondi
/// dopo, ed è stata rigenerata alle 11:44:54 con quel mandato ancora in corso.
///
/// Il confronto è fra il marcatore di consegna — un mtime — e i timestamp dei
/// messaggi che seguono, che i transcript scrivono in UTC. Il fuso non è un
/// dettaglio: in agosto sono due ore, e sbagliarle farebbe sembrare **ogni**
/// messaggio più vecchio della consegna, cioè spegnerebbe la guardia lasciandola
/// scritta.
///
/// In dubbio si risponde `true` (non si chiude): sbagliare qui costa un giro da
/// sessanta secondi, sbagliare dall'altra parte costa il lavoro di una sessione.
pub fn worked_after_handoff(transcript: &str, session: &str) -> bool {
    let marker = state_dir().join(format!("consegna-fatta-{session}"));
    let Ok(since) = marker.metadata().and_then(|m| m.modified()) else {
        return false; // non ha consegnato: non è questa guardia a fermarla
    };
    let since = since
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs_f64())
        .unwrap_or(0.0);
    let tail = transcript_tail(transcript);
    for line in tail.lines() {
        let Ok(d) = serde_json::from_str::<serde_json::Value>(line) else {
            continue; // la prima riga della coda è tagliata a metà: si salta
        };
        if d.get("type").and_then(|v| v.as_str()) != Some("user") {
            continue;
        }
        let at_utc = d
            .get("timestamp")
            .and_then(|v| v.as_str())
            .and_then(epoch_from_iso);
        match at_utc {
            Some(t) if t > since => {}
            _ => continue,
        }
        let content = d.get("message").and_then(|m| m.get("content"));
        let text = match content {
            Some(serde_json::Value::Array(parts)) => {
                // Un esito di strumento è la sessione che parla con se stessa.
                if parts.iter().any(|p| {
                    p.get("type").and_then(|v| v.as_str()) == Some("tool_result")
                }) {
                    continue;
                }
                parts
                    .iter()
                    .filter(|p| p.get("type").and_then(|v| v.as_str()) == Some("text"))
                    .filter_map(|p| p.get("text").and_then(|v| v.as_str()))
                    .collect::<Vec<_>>()
                    .join(" ")
            }
            Some(serde_json::Value::String(s)) => s.clone(),
            _ => String::new(),
        };
        let text = text.trim();
        if text.is_empty()
            || guards::handoff::AUTOMATIC_PREFIXES
                .iter()
                .any(|p| text.starts_with(p))
        {
            continue;
        }
        return true;
    }
    false
}

/// Quanto manca al risveglio che la sessione si è armata da sola, se ce n'è uno.
///
/// UNA SESSIONE CON UN APPUNTAMENTO IN AGENDA NON È UNA SESSIONE FINITA, e la
/// staffetta non aveva modo di distinguerle. Il 19/08/2026 alle 01:30:23 la
/// sessione `23d89176` ha chiuso il giro di un `/loop` armando `ScheduleWakeup`
/// a 1500 secondi; quattro secondi dopo è stata rigenerata perché `tui-idle`
/// rispondeva «non sta scrivendo». Il risveglio vive nel processo che è stato
/// chiuso, e il successore riceve solo la consegna: alle 01:56 non è ripartito
/// niente, e il loop è morto lì.
///
/// Si guarda l'**ultimo** `ScheduleWakeup` della coda, non il primo utile: un
/// loop ne arma uno per giro, e quello che conta è il più recente. `stop: true`
/// è la chiusura esplicita del loop e vale quanto nessun appuntamento.
///
/// In dubbio si risponde `None` — nessun appuntamento — perché questa funzione
/// **ferma** la staffetta: leggere male un transcript non deve poter congelare
/// una sessione che andava sostituita.
pub fn wakeup_pending(transcript: &str, now: f64) -> Option<u64> {
    let (scadenza, _) = last_wakeup(transcript)?;
    let left = scadenza - now;
    if left > 0.0 {
        Some(left as u64)
    } else {
        None
    }
}

/// Il punto di ripresa dichiarato dalla sessione uscente, se l'ha dichiarato.
///
/// LA RIGA ESISTE GIÀ, E NESSUNO LA LEGGEVA. Il `CLAUDE.md` prescrive che ogni
/// turno chiuda con «**Procedo con** — <il passo successivo>»: è il punto di
/// ripresa scritto da chi lavorava, nel momento in cui lo sapeva. Il mandato al
/// successore invece diceva soltanto «leggi la consegna e prosegui», cioè gli
/// chiedeva di ricavarsi da un documento una cosa che era già scritta in chiaro.
///
/// Si prende dall'**ultimo** turno che la contiene, e si taglia a 600 caratteri:
/// oltre non è più un punto di ripresa, è un rendiconto.
pub fn resume_point(transcript: &str) -> Option<String> {
    let tail = transcript_tail(transcript);
    for line in tail.lines().rev() {
        if !line.contains("Procedo con") {
            continue;
        }
        let Ok(d) = serde_json::from_str::<serde_json::Value>(line) else {
            continue; // la prima riga della coda è tagliata a metà: si salta
        };
        if d.get("type").and_then(|v| v.as_str()) != Some("assistant") {
            continue;
        }
        let Some(serde_json::Value::Array(parts)) = d.get("message").and_then(|m| m.get("content"))
        else {
            continue;
        };
        for testo in parts
            .iter()
            .filter(|p| p.get("type").and_then(|v| v.as_str()) == Some("text"))
            .filter_map(|p| p.get("text").and_then(|v| v.as_str()))
        {
            if let Some(riga) = testo.lines().rev().find(|r| r.contains("Procedo con")) {
                let riga = riga.trim();
                if riga.is_empty() {
                    continue;
                }
                return Some(riga.chars().take(600).collect());
            }
        }
    }
    None
}

/// Il mandato che la sessione ha allegato al proprio risveglio.
///
/// È il prompt del `/loop`, e senza di lui il testimone non passa: la consegna
/// dice **da dove** riprendere, il mandato dice **cosa** si stava facendo e a
/// che ritmo. Rigenerare portando solo la consegna trasforma un loop in una
/// sessione che legge un documento e si ferma — che è quanto è successo il
/// 19/08/2026 alle 01:30.
pub fn wakeup_prompt(transcript: &str) -> Option<String> {
    let (_, prompt) = last_wakeup(transcript)?;
    if prompt.trim().is_empty() {
        return None;
    }
    Some(prompt)
}

/// Scadenza e mandato dell'ultimo `ScheduleWakeup` della coda, se ne arma uno.
///
/// Una sola scansione per due domande: il transcript è lo stesso e la riga
/// cercata pure, e due funzioni che lo scorrono separatamente divergono la prima
/// volta che qualcuno cambia idea su cosa conta come appuntamento.
fn last_wakeup(transcript: &str) -> Option<(f64, String)> {
    let tail = transcript_tail(transcript);
    for line in tail.lines().rev() {
        if !line.contains("ScheduleWakeup") {
            continue;
        }
        let Ok(d) = serde_json::from_str::<serde_json::Value>(line) else {
            continue; // la prima riga della coda è tagliata a metà: si salta
        };
        let content = d.get("message").and_then(|m| m.get("content"));
        let Some(serde_json::Value::Array(parts)) = content else {
            continue;
        };
        let Some(input) = parts
            .iter()
            .filter(|p| p.get("type").and_then(|v| v.as_str()) == Some("tool_use"))
            .filter(|p| p.get("name").and_then(|v| v.as_str()) == Some("ScheduleWakeup"))
            .next_back()
            .and_then(|p| p.get("input"))
        else {
            continue;
        };
        if input.get("stop").and_then(|v| v.as_bool()) == Some(true) {
            return None; // il loop è stato chiuso dalla sessione stessa
        }
        let delay = input.get("delaySeconds").and_then(|v| v.as_f64())?;
        let armed = d
            .get("timestamp")
            .and_then(|v| v.as_str())
            .and_then(epoch_from_iso)?;
        let prompt = input
            .get("prompt")
            .and_then(|v| v.as_str())
            .unwrap_or_default()
            .to_string();
        return Some((armed + delay, prompt));
    }
    None
}

/// I secondi dell'epoca da un timestamp ISO-8601 in UTC.
///
/// I transcript scrivono `2026-08-17T09:35:56.123Z`. Si converte a mano invece
/// di aggiungere una dipendenza: il formato è fisso, e ciò che serve è la data
/// civile trasformata in giorni — l'algoritmo dei giorni dall'era, che non ha
/// casi particolari sugli anni bisestili.
fn epoch_from_iso(iso: &str) -> Option<f64> {
    let b = iso.as_bytes();
    if b.len() < 19 || b[4] != b'-' || b[7] != b'-' || b[10] != b'T' {
        return None;
    }
    let num = |a: usize, z: usize| iso.get(a..z)?.parse::<i64>().ok();
    let (y, m, d) = (num(0, 4)?, num(5, 7)?, num(8, 10)?);
    let (hh, mm, ss) = (num(11, 13)?, num(14, 16)?, num(17, 19)?);
    // Giorni dall'era (Howard Hinnant): marzo come primo mese, così il 29
    // febbraio cade in fondo all'anno e non serve nessun caso a parte.
    let y = if m <= 2 { y - 1 } else { y };
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = y - era * 400;
    let mp = (m + 9) % 12;
    let doy = (153 * mp + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    let days = era * 146_097 + doe - 719_468;
    Some((days * 86_400 + hh * 3600 + mm * 60 + ss) as f64)
}

/// Modello, budget e soglie per la sessione che sta scrivendo questo transcript.
pub fn thresholds(transcript: &str) -> Thresholds {
    let tail = transcript_tail(transcript);
    thresholds_from_lines(&tail.lines().collect::<Vec<_>>())
}

/// I token in contesto, rimisurati solo se il transcript è cresciuto abbastanza.
///
/// IL DIFETTO CHE QUESTO PORTING HA TROVATO NELL'ORIGINALE. Su un memo che
/// contiene un numero negativo il Python faceva `int('-5')` e si portava dietro
/// un contesto negativo, che resta per sempre sotto ogni soglia: quella sessione
/// non sarebbe stata rigenerata mai più, per quanto piena. Qui il tipo è `u64` e
/// il parse fallisce, quindi si rimisura — immune per tipo, non per attenzione.
/// Un vaglio indipendente l'ha misurato il 17/08/2026 su due casi, e la
/// correzione è andata anche a monte: adesso rimisurano entrambe.
///
/// Il memo replica il formato del Python — `<byte> <token>` separati da uno
/// spazio — perché lo stesso file lo scrivono e lo leggono entrambe le
/// implementazioni finché convivono. Un formato diverso qui farebbe rimisurare
/// il Python a ogni giro senza che nessuno se ne accorga.
pub fn context_used(transcript: &str, session: &str) -> u64 {
    let memo = if session.is_empty() {
        None
    } else {
        Some(state_dir().join(format!("consegna-misura-{session}")))
    };
    let size = fs::metadata(transcript).map(|m| m.len()).unwrap_or(0);

    if let Some(path) = &memo {
        if let Ok(text) = fs::read_to_string(path) {
            // ESATTAMENTE due campi. Il Python scrive `old_size, old_tokens =
            // text.split()`, che su tre campi alza `ValueError` e fa rimisurare:
            // prendendo i primi due ci si fiderebbe di un memo che l'oracolo ha
            // già giudicato illeggibile. Trovato da un vaglio indipendente su
            // 1932 combinazioni, e in un caso cambiava l'azione — `salta` invece
            // di `rigenera` su una sessione piena e consegnata.
            let parts: Vec<&str> = text.split_whitespace().collect();
            if let [old_size, old_tokens] = parts[..] {
                if let (Ok(old_size), Ok(old_tokens)) =
                    (old_size.parse::<u64>(), old_tokens.parse::<u64>())
                {
                    if size.saturating_sub(old_size) < MIN_GROWTH {
                        return old_tokens;
                    }
                }
            }
        }
    }

    let tail = transcript_tail(transcript);
    // IL MEMO SI SCRIVE QUANDO L'`usage` C'È, non quando la somma è positiva.
    // Qui c'era un commento che affermava il contrario, e nessun caso lo
    // verificava: su una trascrizione vera l'originale lasciava
    // `consegna-misura-* = <size> 0` e il porto non lasciava niente. Un turno
    // con il solo `output_tokens` è una misura fatta che vale zero, ed è diverso
    // da «non ho trovato niente da misurare».
    let trovato = context_used_found(&tail.lines().collect::<Vec<_>>());
    if let (Some(tokens), Some(path)) = (trovato, &memo) {
        let _ = fs::create_dir_all(state_dir());
        let _ = fs::write(path, format!("{size} {tokens}"));
    }
    trovato.unwrap_or(0)
}

/// Risolve un handle leggendo l'elenco dei pannelli da stdin, e lo stampa.
///
/// Legge da stdin invece di chiamare `orca` perché le due implementazioni devono
/// vedere **lo stesso** elenco: chiamandolo ognuna per conto suo, due letture a
/// un secondo di distanza possono già non concordare, e una divergenza così
/// verrebbe letta come un difetto del porting.
pub fn resolve(tab_id: &str, worktree_id: &str, known_handle: &str) -> i32 {
    let mut raw = String::new();
    if std::io::stdin().read_to_string(&mut raw).is_err() {
        println!();
        return 0;
    }
    // Una risposta illeggibile e una di forma sconosciuta portano allo stesso
    // posto — l'elenco vuoto — e da qui è giusto: `resolve_terminal_handle`
    // risponde `''` su un elenco vuoto, che è «non lo so» e non «chiudi».
    // La distinzione conta a monte, in `read_terminals`, dove un elenco vuoto
    // significherebbe invece «i pannelli sono morti tutti».
    let terminals = serde_json::from_str::<serde_json::Value>(&raw)
        .ok()
        .and_then(|v| guards::handoff::Terminal::from_response(&v))
        .unwrap_or_default();
    println!(
        "{}",
        guards::handoff::resolve_terminal_handle(tab_id, worktree_id, known_handle, &terminals)
    );
    0
}

/// Stampa la misura in JSON, per il confronto con l'implementazione Python.
///
/// Non è un gancio: è il punto d'aggancio dello strumento di equivalenza, che
/// chiama le due implementazioni sullo stesso transcript e pretende lo stesso
/// oggetto. Senza un modo di interrogare il Rust dall'esterno, il confronto su
/// materiale vero non si può fare — e i casi scritti a mano non bastano.
pub fn measure(transcript: &str, session: &str) -> i32 {
    let t = thresholds(transcript);
    let used = context_used(transcript, session);
    println!(
        "{}",
        serde_json::json!({
            "model": t.model,
            "budget": t.budget,
            "warn": t.warn,
            "require": t.require,
            "used": used,
        })
    );
    0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn transcript_timestamps_are_read_as_utc() {
        // Il valore atteso viene da `date -u -j -f '%Y-%m-%dT%H:%M:%S' ... +%s`:
        // se questa conversione slittasse di due ore, la guardia sul lavoro
        // arrivato dopo la consegna resterebbe scritta e spenta, perché ogni
        // messaggio sembrerebbe più vecchio del marcatore.
        assert_eq!(epoch_from_iso("2026-08-17T09:35:56.123Z"), Some(1_786_959_356.0));
        assert_eq!(epoch_from_iso("1970-01-01T00:00:00Z"), Some(0.0));
        // Il 29 febbraio è il caso che gli algoritmi ingenui sbagliano.
        assert_eq!(epoch_from_iso("2024-02-29T00:00:00Z"), Some(1_709_164_800.0));
        assert_eq!(epoch_from_iso("non e' una data"), None);
        assert_eq!(epoch_from_iso(""), None);
    }

    /// Il marcatore di consegna e un transcript, dentro una HOME usa-e-getta.
    ///
    /// L'isolamento vero — e il lucchetto, che è uno solo per tutto il binario —
    /// sta in `crate::test_home`: vedi lì perché un mutex per modulo non basta.
    struct Scene {
        _home: crate::test_home::HomeIsolata,
        dir: PathBuf,
    }

    impl Scene {
        fn new(nome: &str, consegnato: bool, righe: Vec<String>) -> Self {
            let home = crate::test_home::HomeIsolata::nuova(nome);
            let dir = home.dir.clone();
            if consegnato {
                let _ = fs::write(home.stato().join("consegna-fatta-provalav"), "1");
            }
            let _ = fs::write(dir.join("transcript.jsonl"), righe.join("\n"));
            Self { _home: home, dir }
        }

        fn transcript(&self) -> String {
            self.dir.join("transcript.jsonl").to_string_lossy().into_owned()
        }
    }

    /// Un messaggio dell'anno 2100: sicuramente più recente del marcatore, che
    /// il test ha appena scritto. Datarlo «adesso» renderebbe il caso una corsa.
    fn user_message(testo: &str) -> String {
        serde_json::json!({
            "type": "user",
            "timestamp": "2100-01-01T00:00:00.000Z",
            "message": {"content": [{"type": "text", "text": testo}]}
        })
        .to_string()
    }

    /// Un `ScheduleWakeup` armato al tempo dato, con quel ritardo.
    fn wakeup(iso: &str, delay: u64, stop: bool) -> String {
        wakeup_with_prompt(iso, delay, stop, "/loop Sistemare la configurazione")
    }

    /// Come sopra, ma col mandato esplicito: è il campo che porta avanti
    /// l'incarico, e senza un caso che lo legga può sparire in silenzio.
    fn wakeup_with_prompt(iso: &str, delay: u64, stop: bool, prompt: &str) -> String {
        let mut input = serde_json::json!({"delaySeconds": delay, "prompt": prompt});
        if stop {
            // Lo strumento dichiara che con `stop` gli altri campi si ignorano,
            // quindi possono benissimo esserci: se il caso non li mettesse,
            // togliere il controllo sullo `stop` non farebbe cadere niente —
            // provato il 19/08/2026, il mutante sopravviveva.
            input = serde_json::json!({"stop": true, "delaySeconds": 1500, "prompt": prompt});
        }
        serde_json::json!({
            "type": "assistant",
            "timestamp": iso,
            "message": {"content": [
                {"type": "tool_use", "name": "ScheduleWakeup", "input": input}
            ]}
        })
        .to_string()
    }

    /// L'istante vero del caso: le 23:30:15 UTC del 18/08/2026, che sulla
    /// macchina di Theo sono le 01:30:15 del 19 — i transcript scrivono in UTC,
    /// il registro della staffetta in ora locale, e le due ore di scarto sono
    /// esattamente il genere di errore contro cui mette in guardia il commento
    /// di `worked_after_handoff` qui sopra.
    const ARMED_AT: &str = "2026-08-18T23:30:15.814Z";
    const ARMED_AT_EPOCH: f64 = 1_787_095_815.0;

    #[test]
    fn a_wakeup_still_pending_is_seen() {
        let s = Scene::new("sveglia", false, vec![wakeup(ARMED_AT, 1500, false)]);
        // Quattro secondi dopo: è l'istante in cui la staffetta ha rigenerato.
        let left = wakeup_pending(&s.transcript(), ARMED_AT_EPOCH + 4.0);
        assert_eq!(left, Some(1496));
    }

    #[test]
    fn an_expired_wakeup_protects_no_longer() {
        let s = Scene::new("scaduta", false, vec![wakeup(ARMED_AT, 1500, false)]);
        assert_eq!(wakeup_pending(&s.transcript(), ARMED_AT_EPOCH + 1501.0), None);
    }

    #[test]
    fn the_last_wakeup_counts_not_the_first() {
        // Un loop ne arma uno per giro: guardare il primo della coda darebbe
        // per armata una sessione il cui ultimo giro ha chiuso il loop.
        let s = Scene::new(
            "ultimo",
            false,
            vec![wakeup(ARMED_AT, 1500, false), wakeup(ARMED_AT, 0, true)],
        );
        assert_eq!(wakeup_pending(&s.transcript(), ARMED_AT_EPOCH + 4.0), None);
    }

    #[test]
    fn a_broken_line_does_not_stop_the_wakeup_search() {
        let s = Scene::new(
            "sveglia-rotta",
            false,
            vec![wakeup(ARMED_AT, 1500, false), "ent\":\"x\"}]}}".to_string()],
        );
        assert_eq!(wakeup_pending(&s.transcript(), ARMED_AT_EPOCH + 4.0), Some(1496));
    }

    /// Un turno dell'assistente che chiude come prescrive il `CLAUDE.md`.
    fn closing_turn(punto: &str) -> String {
        serde_json::json!({
            "type": "assistant",
            "timestamp": ARMED_AT,
            "message": {"content": [{"type": "text", "text":
                format!("**Stato** — chiuso: fatto tutto\n**Procedo con** — {punto}")}]}
        })
        .to_string()
    }

    #[test]
    fn the_resume_point_is_read_from_the_closing_turn() {
        // La riga esiste in ogni turno per prescrizione, e fino al 19/08/2026
        // non la leggeva nessuno: al successore si chiedeva di dedurre da un
        // riassunto una cosa già scritta in chiaro.
        let s = Scene::new("punto", false, vec![closing_turn("la staffetta via /clear")]);
        let punto = resume_point(&s.transcript()).expect("punto non trovato");
        assert!(punto.contains("la staffetta via /clear"), "{punto}");
    }

    #[test]
    fn the_resume_point_of_the_last_turn_counts() {
        let s = Scene::new(
            "punto-ultimo",
            false,
            vec![closing_turn("il primo passo"), closing_turn("il passo di adesso")],
        );
        let punto = resume_point(&s.transcript()).unwrap();
        assert!(punto.contains("il passo di adesso"), "{punto}");
        assert!(!punto.contains("il primo passo"), "{punto}");
    }

    #[test]
    fn without_a_closing_turn_no_resume_point_is_invented() {
        let s = Scene::new("punto-assente", false, vec![user_message("Mandato.")]);
        assert_eq!(resume_point(&s.transcript()), None);
    }

    #[test]
    fn the_resume_point_is_not_taken_from_a_user_message() {
        // Chi scrive «Procedo con» in una richiesta non sta dichiarando il
        // proprio punto di ripresa: il campo `type` è l'unico discrimine.
        let s = Scene::new(
            "punto-utente",
            false,
            vec![user_message("Procedo con questa richiesta, per favore")],
        );
        assert_eq!(resume_point(&s.transcript()), None);
    }

    #[test]
    fn the_prompt_is_read_from_the_wakeup() {
        // È il campo che porta avanti l'incarico: sbagliarne il nome lascerebbe
        // il successore con la sola consegna, cioè col difetto del 19/08/2026
        // intatto — e senza questo caso nessuna prova andrebbe in rosso.
        let s = Scene::new(
            "mandato-sveglia",
            false,
            vec![wakeup_with_prompt(ARMED_AT, 1500, false, "/loop Sistema Orca")],
        );
        assert_eq!(wakeup_prompt(&s.transcript()).as_deref(), Some("/loop Sistema Orca"));
    }

    #[test]
    fn an_empty_prompt_is_not_a_prompt() {
        let s = Scene::new(
            "mandato-vuoto",
            false,
            vec![wakeup_with_prompt(ARMED_AT, 1500, false, "   ")],
        );
        assert_eq!(wakeup_prompt(&s.transcript()), None);
    }

    #[test]
    fn the_prompt_of_the_last_round_counts() {
        let s = Scene::new(
            "mandato-ultimo",
            false,
            vec![
                wakeup_with_prompt(ARMED_AT, 1500, false, "/loop primo giro"),
                wakeup_with_prompt(ARMED_AT, 1500, false, "/loop secondo giro"),
            ],
        );
        assert_eq!(wakeup_prompt(&s.transcript()).as_deref(), Some("/loop secondo giro"));
    }

    #[test]
    fn without_a_wakeup_nothing_is_held_back() {
        let s = Scene::new("niente-sveglia", false, vec![user_message("Mandato.")]);
        assert_eq!(wakeup_pending(&s.transcript(), ARMED_AT_EPOCH), None);
    }

    #[test]
    fn work_arriving_after_the_handoff_is_seen() {
        let s = Scene::new("mandato", true, vec![user_message("Mandato. Riprendi da qui.")]);
        assert!(worked_after_handoff(&s.transcript(), "provalav"));
    }

    #[test]
    fn a_monitor_notification_is_not_work() {
        // È il caso vero del 17/08: la sessione aperta dalla staffetta ha
        // ricevuto per primo un evento di monitor. Se contasse come mandato,
        // nessuna sessione verrebbe più rigenerata — la guardia si spegnerebbe
        // da sola restando verde.
        let s = Scene::new(
            "bollettino",
            true,
            vec![user_message("<task-notification> <task-id>x</task-id>")],
        );
        assert!(!worked_after_handoff(&s.transcript(), "provalav"));
    }

    #[test]
    fn a_tool_result_is_the_session_talking_to_itself() {
        let riga = serde_json::json!({
            "type": "user",
            "timestamp": "2100-01-01T00:00:00.000Z",
            "message": {"content": [{"type": "tool_result", "content": "ok"}]}
        })
        .to_string();
        let s = Scene::new("tool-result", true, vec![riga]);
        assert!(!worked_after_handoff(&s.transcript(), "provalav"));
    }

    #[test]
    fn without_a_handoff_this_guard_holds_nothing_back() {
        // Il marcatore assente significa «non ha ancora consegnato», e a quel
        // punto ferma tutto la guardia di prima: rispondere `true` qui
        // bloccherebbe la staffetta per ogni sessione, per sempre.
        let s = Scene::new("senza-consegna", false, vec![user_message("Mandato.")]);
        assert!(!worked_after_handoff(&s.transcript(), "provalav"));
    }

    #[test]
    fn a_message_older_than_the_handoff_does_not_count() {
        let s = Scene::new(
            "prima",
            true,
            vec![serde_json::json!({
                "type": "user",
                "timestamp": "2020-01-01T00:00:00.000Z",
                "message": {"content": [{"type": "text", "text": "vecchio"}]}
            })
            .to_string()],
        );
        assert!(!worked_after_handoff(&s.transcript(), "provalav"));
    }

    #[test]
    fn a_half_cut_line_does_not_interrupt_the_search() {
        // La coda del transcript comincia quasi sempre a metà di una riga: se
        // un parse fallito fermasse il giro, il mandato che sta più in fondo
        // non verrebbe mai visto.
        let s = Scene::new(
            "riga-rotta",
            true,
            vec!["ent\":\"x\"}]}}".to_string(), user_message("Mandato vero.")],
        );
        assert!(worked_after_handoff(&s.transcript(), "provalav"));
    }
}
