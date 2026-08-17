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
    let terminals = match serde_json::from_str::<serde_json::Value>(&raw) {
        Ok(v) => guards::handoff::Terminal::from_response(&v),
        Err(_) => Vec::new(),
    };
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
    fn i_timestamp_del_transcript_si_leggono_in_utc() {
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
    struct Scena {
        _home: crate::test_home::HomeIsolata,
        dir: PathBuf,
    }

    impl Scena {
        fn nuova(nome: &str, consegnato: bool, righe: Vec<String>) -> Self {
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
    fn messaggio(testo: &str) -> String {
        serde_json::json!({
            "type": "user",
            "timestamp": "2100-01-01T00:00:00.000Z",
            "message": {"content": [{"type": "text", "text": testo}]}
        })
        .to_string()
    }

    #[test]
    fn un_mandato_arrivato_dopo_la_consegna_si_vede() {
        let s = Scena::nuova("mandato", true, vec![messaggio("Mandato. Riprendi da qui.")]);
        assert!(worked_after_handoff(&s.transcript(), "provalav"));
    }

    #[test]
    fn un_bollettino_del_monitor_non_e_lavoro() {
        // È il caso vero del 17/08: la sessione aperta dalla staffetta ha
        // ricevuto per primo un evento di monitor. Se contasse come mandato,
        // nessuna sessione verrebbe più rigenerata — la guardia si spegnerebbe
        // da sola restando verde.
        let s = Scena::nuova(
            "bollettino",
            true,
            vec![messaggio("<task-notification> <task-id>x</task-id>")],
        );
        assert!(!worked_after_handoff(&s.transcript(), "provalav"));
    }

    #[test]
    fn un_esito_di_strumento_e_la_sessione_che_parla_da_sola() {
        let riga = serde_json::json!({
            "type": "user",
            "timestamp": "2100-01-01T00:00:00.000Z",
            "message": {"content": [{"type": "tool_result", "content": "ok"}]}
        })
        .to_string();
        let s = Scena::nuova("tool-result", true, vec![riga]);
        assert!(!worked_after_handoff(&s.transcript(), "provalav"));
    }

    #[test]
    fn senza_consegna_questa_guardia_non_ferma_niente() {
        // Il marcatore assente significa «non ha ancora consegnato», e a quel
        // punto ferma tutto la guardia di prima: rispondere `true` qui
        // bloccherebbe la staffetta per ogni sessione, per sempre.
        let s = Scena::nuova("senza-consegna", false, vec![messaggio("Mandato.")]);
        assert!(!worked_after_handoff(&s.transcript(), "provalav"));
    }

    #[test]
    fn un_messaggio_precedente_alla_consegna_non_conta() {
        let s = Scena::nuova(
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
    fn una_riga_tagliata_a_meta_non_interrompe_la_ricerca() {
        // La coda del transcript comincia quasi sempre a metà di una riga: se
        // un parse fallito fermasse il giro, il mandato che sta più in fondo
        // non verrebbe mai visto.
        let s = Scena::nuova(
            "riga-rotta",
            true,
            vec!["ent\":\"x\"}]}}".to_string(), messaggio("Mandato vero.")],
        );
        assert!(worked_after_handoff(&s.transcript(), "provalav"));
    }
}
