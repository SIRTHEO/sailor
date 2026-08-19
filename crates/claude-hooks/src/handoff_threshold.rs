//! Il gancio UserPromptSubmit che dice quanto contesto resta, finché resta.
//!
//! Il giudizio sta in `guards::handoff_threshold`; qui c'è ciò che tocca il
//! mondo: il payload su stdin, la coda del transcript, la finestra dichiarata
//! dall'ambiente e il file di stato per sessione sotto `TMPDIR`.
//!
//! PERCHÉ ESISTE. Il gancio `SessionStart` avvisa DOPO la compattazione: arriva
//! quando il danno è fatto, c'è un compito a metà e il riassunto ha già scelto
//! per te. Misurato il 12/08/2026 sulla sessione `d6cefb4d`: sedici ripartenze,
//! sedici avvisi, zero consegne. Questo parla prima, quando c'è ancora spazio
//! per ragionare su cosa consegnare.
//!
//! LO STATO NON STA IN `state/`, STA IN `TMPDIR`. È l'unico gancio portato
//! finora che scriva fuori da `$HOME/.claude/state`, quindi `crate::handoff::
//! state_dir` qui non si usa: la sede la sceglie l'originale, e cambiarla
//! renderebbe muto ogni gradino già annunciato dalle sessioni vive nel momento
//! in cui la configurazione passa al binario. Ne discende che il confronto ha
//! bisogno di due `TMPDIR` separati, non solo di due `HOME`.
//!
//! OGNI ERRORE È SILENZIOSO, e non è pigrizia: un `UserPromptSubmit` che si
//! rompe rompe l'invio del prompt, cioè fa più danno del problema che risolve.
//! Nell'originale il silenzio arriva per due strade diverse — il `return` dei
//! rami previsti e il `try/except` che avvolge `main()` — ma **da fuori sono
//! indistinguibili**: uscita 0, niente su stdout, nessun file scritto. Per
//! questo qui i due casi non sono separati: dove il Python muore, si esce 0.

use guards::handoff::TAIL_BYTES;
use guards::handoff_threshold::{
    already_said, declared_window, message, percent, step_reached, DEFAULT_WINDOW,
};
use std::fs;
use std::io::Read;
use std::path::PathBuf;

/// Le due variabili che dichiarano la finestra, nell'ordine dell'originale.
///
/// L'ORDINE È COMPORTAMENTO: lo scavalco manuale batte la sessione. Invertirlo
/// renderebbe impossibile provare il gancio su una sessione vera, che è l'unica
/// ragione per cui `SOGLIA_FINESTRA` esiste.
const WINDOW_VARS: [&str; 2] = ["SOGLIA_FINESTRA", "CLAUDE_CODE_AUTO_COMPACT_WINDOW"];

/// Quanto contesto ha questa sessione, secondo chi lo sa.
///
/// Il nome del modello non serve e non basta: nei 23.481 usi registrati compare
/// sempre come `claude-opus-5`, mai con la variante `[1m]`, quindi da lì la
/// finestra non si deduce.
fn window() -> u128 {
    for var in WINDOW_VARS {
        if let Ok(raw) = std::env::var(var) {
            if let Some(v) = declared_window(&raw) {
                return v;
            }
        }
    }
    DEFAULT_WINDOW
}

/// I token in contesto all'ultima risposta, leggendo solo la coda del file.
///
/// NON È `crate::handoff::context_used`, e le differenze non sono di stile —
/// sono tre, e ognuna cambia il numero:
///
/// 1. **nessun memo.** L'altra misura ricorda l'ultimo valore per sessione e non
///    rimisura finché il transcript non è cresciuto di 400 kB. Qui si rimisura
///    sempre: questo gancio parte a ogni prompt, non a ogni chiamata a
///    strumento, e un memo lo farebbe annunciare il gradino con un turno di
///    ritardo proprio nella zona in cui i turni sono lunghi.
/// 2. **solo `message.usage`**, senza il ripiego sull'`usage` di primo livello.
/// 3. **una somma nulla non ferma la ricerca.** Là un `usage` presente vale come
///    misura fatta anche se somma zero; qui `if total:` fa proseguire
///    all'indietro — un turno col solo `output_tokens` viene scavalcato e si
///    prende quello prima.
///
/// `None` significa «niente da misurare», e da fuori è identico a zero.
fn context_used(path: &str) -> Option<i64> {
    let size = fs::metadata(path).ok()?.len();
    let chunk = crate::handoff::transcript_tail(path);
    let mut lines: Vec<&str> = chunk.split('\n').collect();
    // La prima riga può essere tronca dal seek: si scarta. Si scarta **anche
    // quando il taglio è caduto per caso su un a capo** e la riga sarebbe
    // valida, perché è ciò che fa l'originale — `chunk.split('\n')[1:] if start`.
    if size > TAIL_BYTES && !lines.is_empty() {
        lines.remove(0);
    }
    for line in lines.iter().rev() {
        let line = line.trim();
        if line.is_empty() || !line.contains("\"usage\"") {
            continue;
        }
        let Ok(v) = serde_json::from_str::<serde_json::Value>(line) else {
            continue;
        };
        // `json.loads(line).get('message', {})` su una riga che non è un oggetto
        // è un `AttributeError`, e l'originale lo cattura: si prosegue.
        let Some(obj) = v.as_object() else { continue };
        // `... or {}`: un `message` falso — assente, `null`, vuoto — vale come
        // dizionario vuoto, e da lì il totale è zero e si prosegue.
        let msg = match obj.get("message") {
            Some(m) if !is_falsy(m) => m,
            _ => continue,
        };
        // `msg.get('usage')` su un valore che non è un dizionario esce dal
        // `try` dell'originale e uccide il gancio: da fuori è silenzio.
        let usage = msg.as_object()?.get("usage");
        let total = match usage {
            None | Some(serde_json::Value::Null) => Some(0),
            Some(u) if is_falsy(u) => Some(0),
            Some(u) => sum_usage(u),
        };
        // `None` qui è il `TypeError` dell'originale su un `usage` che non è un
        // dizionario o su un campo che non è un numero: silenzio, non «zero».
        let total = total?;
        if total != 0 {
            return Some(total);
        }
    }
    None
}

/// `bool(x)` di Python: vuoto, zero e `false` sono falsi.
fn is_falsy(v: &serde_json::Value) -> bool {
    match v {
        serde_json::Value::Null => true,
        serde_json::Value::Bool(b) => !b,
        serde_json::Value::Number(n) => n.as_f64() == Some(0.0),
        serde_json::Value::String(s) => s.is_empty(),
        serde_json::Value::Array(a) => a.is_empty(),
        serde_json::Value::Object(o) => o.is_empty(),
    }
}

/// I tre campi che compongono l'ingresso reale, sommati come fa l'originale.
///
/// Contarne uno solo sottostima di un ordine di grandezza con la cache calda, ed
/// è la misura su cui si decide se consegnare. `None` = l'originale sarebbe
/// morto qui: un `usage` che non è un dizionario, o un campo che non è un intero
/// (`"abc" + 0` e `None + 0` sono entrambi `TypeError`).
fn sum_usage(u: &serde_json::Value) -> Option<i64> {
    let o = u.as_object()?;
    let mut total: i64 = 0;
    for name in [
        "input_tokens",
        "cache_read_input_tokens",
        "cache_creation_input_tokens",
    ] {
        total = total.saturating_add(match o.get(name) {
            None => 0,
            Some(v) => v.as_i64()?,
        });
    }
    Some(total)
}

/// Dove la sessione tiene il gradino che ha già annunciato.
///
/// `os.path.join(os.environ.get('TMPDIR', '/tmp'), …)`, quirk compreso: una
/// `TMPDIR` presente ma **vuota** produce un percorso relativo, quindi il file
/// finisce nella cartella di lavoro. `PathBuf::from("").join(…)` fa lo stesso.
fn state_file(session: &str) -> PathBuf {
    let base = std::env::var("TMPDIR").unwrap_or_else(|_| "/tmp".to_string());
    PathBuf::from(base).join(format!("soglia-consegna-{session}.stato"))
}

/// `(payload.get('session_id') or 'ignota')[:8]` dell'originale.
///
/// TRE ESITI, non due. Una stringa si tronca a otto **caratteri**; un valore
/// falso qualunque — assente, `null`, `""`, `0`, `false`, lista o oggetto vuoti
/// — diventa `ignota`; e tutto il resto uccide il gancio, perché un intero, un
/// booleano o un dizionario non si affettano (`TypeError`).
///
/// RESIDUO DICHIARATO: una **lista non vuota** in Python si affetta eccome, e
/// `f'{[1, 2]}'` diventa un nome di file valido. Lì l'originale scriverebbe lo
/// stato e parlerebbe, qui si tace. Riprodurre il `repr` di Python per un caso
/// che nessun evento di Claude Code produce costa più di quanto vale — è
/// dichiarato qui invece di essere scoperto dopo, come già per `scope-drift`.
fn session_id(v: Option<&serde_json::Value>) -> Option<String> {
    match v {
        None => Some("ignota".to_string()),
        Some(serde_json::Value::String(s)) if !s.is_empty() => {
            Some(s.chars().take(8).collect())
        }
        Some(x) if is_falsy(x) => Some("ignota".to_string()),
        _ => None,
    }
}

pub fn run() -> i32 {
    let mut raw = String::new();
    if std::io::stdin().read_to_string(&mut raw).is_err() {
        return 0;
    }
    let Ok(payload) = serde_json::from_str::<serde_json::Value>(&raw) else {
        return 0;
    };
    // `payload.get(...)` su una lista o su una stringa è un `AttributeError`.
    let Some(obj) = payload.as_object() else {
        return 0;
    };
    let path = match obj.get("transcript_path") {
        Some(serde_json::Value::String(s)) => s.as_str(),
        _ => "", // `... or ''`, e un percorso non testuale non arriva qui
    };
    let Some(session) = session_id(obj.get("session_id")) else {
        return 0;
    };
    if path.is_empty() || !std::path::Path::new(path).exists() {
        return 0;
    }

    let Some(used) = context_used(path) else {
        return 0;
    };
    if used <= 0 {
        // Zero è «niente da misurare». Un totale negativo l'originale lo
        // restituisce — è vero per Python — ma la sua percentuale resta sotto
        // ogni gradino, quindi il silenzio arriva un ramo dopo, uguale.
        return 0;
    }
    let used = used as u64;
    // IL METRO È IL BUDGET DI QUALITÀ, NON LA FINESTRA DICHIARATA — corretto il
    // 19/08/2026. Misurando sulla finestra (1.000.000) i due gradini cadevano a
    // 700.000 e 850.000 token, ma l'harness compatta da sé molto prima: il
    // massimo mai registrato su questa macchina è 539.022, e con
    // `CLAUDE_AUTOCOMPACT_PCT_OVERRIDE=55` su un modello da 500k la
    // compattazione scatta a 264.000. I due gradini erano quindi irraggiungibili
    // per costruzione, e infatti in 283 sessioni di sette giorni questo gancio
    // ha parlato **quattro volte**. Sul budget (500.000 per opus-5) cadono a
    // 350.000 e 425.000: entrambi prima del presidio che pretende la consegna a
    // 450.000, che è l'ordine giusto — prima si suggerisce, poi si impone.
    // Lo scavalco manuale `SOGLIA_FINESTRA` continua a vincere su tutto.
    let metro = match std::env::var("SOGLIA_FINESTRA")
        .ok()
        .and_then(|raw| declared_window(&raw))
    {
        Some(v) => v,
        None => {
            let tail = crate::handoff::transcript_tail(path);
            let lines: Vec<&str> = tail.lines().collect();
            guards::handoff::thresholds_from_lines(&lines).budget as u128
        }
    };
    let percent = percent(used, metro);
    let Some(step) = step_reached(percent) else {
        return 0;
    };

    let state = state_file(&session);
    let said = fs::read_to_string(&state)
        .map(|t| already_said(&t, step))
        .unwrap_or(false);
    if said {
        return 0;
    }
    // La scrittura precede l'avviso e i suoi errori si ingoiano: se il file non
    // si può scrivere il gancio parla lo stesso, e al massimo si ripeterà.
    let _ = fs::write(&state, step.to_string());
    println!("{}", message(used, percent, step));
    0
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Una `TMPDIR` usa-e-getta: questo gancio scrive lì, non sotto `HOME`.
    ///
    /// Serve per la stessa ragione per cui esiste `HomeIsolata`: col `TMPDIR`
    /// vero, il secondo giro dei casi trova i file del primo e legge silenzio
    /// dove si aspetta un avviso. L'originale lo dice a chiare lettere nelle sue
    /// autoprove — «quattro prove rosse su dieci, e la conclusione sbagliata che
    /// il gancio fosse rotto». Il lucchetto è quello di `HomeIsolata`, perché
    /// anche `TMPDIR` è una variabile del processo e i test girano in parallelo.
    struct TmpIsolata {
        _casa: crate::test_home::HomeIsolata,
        precedenti: Vec<(&'static str, Option<String>)>,
        dir: PathBuf,
    }

    impl TmpIsolata {
        fn nuova(nome: &str) -> Self {
            let casa = crate::test_home::HomeIsolata::nuova(nome);
            let dir = casa.dir.join("tmp");
            let _ = fs::create_dir_all(&dir);
            // Si RIMETTE il valore di prima, non si cancella: `TMPDIR` è la
            // sede che `std::env::temp_dir()` legge, e togliendola per sempre
            // ogni caso che venga dopo — anche di un altro modulo — si
            // ritroverebbe la cartella temporanea altrove.
            let precedenti = ["TMPDIR", "SOGLIA_FINESTRA", "CLAUDE_CODE_AUTO_COMPACT_WINDOW"]
                .iter()
                .map(|k| (*k, std::env::var(k).ok()))
                .collect();
            std::env::set_var("TMPDIR", &dir);
            std::env::remove_var("SOGLIA_FINESTRA");
            std::env::set_var("CLAUDE_CODE_AUTO_COMPACT_WINDOW", "200000");
            Self { _casa: casa, precedenti, dir }
        }
    }

    impl Drop for TmpIsolata {
        fn drop(&mut self) {
            for (chiave, valore) in &self.precedenti {
                match valore {
                    Some(v) => std::env::set_var(chiave, v),
                    None => std::env::remove_var(chiave),
                }
            }
        }
    }

    fn transcript(dir: &PathBuf, nome: &str, righe: &[&str]) -> String {
        let p = dir.join(nome);
        fs::write(&p, righe.join("\n")).unwrap();
        p.to_string_lossy().into_owned()
    }

    fn riga_usage(totale: i64) -> String {
        serde_json::json!({
            "type": "assistant",
            "message": {"model": "claude-opus-5", "usage": {
                "input_tokens": 2,
                "cache_read_input_tokens": totale - 2,
                "cache_creation_input_tokens": 0,
                "output_tokens": 100,
            }}
        })
        .to_string()
    }

    #[test]
    fn la_finestra_la_dichiara_prima_lo_scavalco_poi_la_sessione() {
        let _t = TmpIsolata::nuova("soglia-finestra");
        assert_eq!(window(), 200_000);
        std::env::set_var("CLAUDE_CODE_AUTO_COMPACT_WINDOW", "1000000");
        assert_eq!(window(), 1_000_000);
        std::env::set_var("SOGLIA_FINESTRA", "200000");
        assert_eq!(window(), 200_000, "lo scavalco manuale deve vincere");
        // Illeggibile: si ripiega sulla variabile dopo, non sul valore storico.
        std::env::set_var("SOGLIA_FINESTRA", "boh");
        assert_eq!(window(), 1_000_000);
        std::env::remove_var("CLAUDE_CODE_AUTO_COMPACT_WINDOW");
        assert_eq!(window(), DEFAULT_WINDOW);
    }

    #[test]
    fn una_somma_nulla_non_ferma_la_ricerca_allindietro() {
        // È la differenza con `crate::handoff::context_used`, che si fermerebbe
        // qui restituendo zero: un turno col solo `output_tokens` è una misura
        // che vale zero, e questo gancio scavalca e prende quella prima.
        let t = TmpIsolata::nuova("soglia-somma-nulla");
        let p = transcript(
            &t.dir,
            "t.jsonl",
            &[
                &riga_usage(144_000),
                r#"{"type":"assistant","message":{"usage":{"output_tokens":5}}}"#,
            ],
        );
        assert_eq!(context_used(&p), Some(144_000));
    }

    #[test]
    fn un_usage_di_primo_livello_non_si_guarda() {
        // L'altra misura ha il ripiego, questa no: leggerlo qui farebbe parlare
        // il gancio su righe che l'originale scavalca.
        let t = TmpIsolata::nuova("soglia-usage-radice");
        let p = transcript(
            &t.dir,
            "t.jsonl",
            &[r#"{"type":"assistant","usage":{"input_tokens":144000}}"#],
        );
        assert_eq!(context_used(&p), None);
    }

    #[test]
    fn un_messaggio_che_non_e_un_oggetto_fa_tacere_tutto() {
        // `msg.get('usage')` su una stringa è l'`AttributeError` che sfugge al
        // `try` dell'originale: il gancio muore in silenzio. Se qui si
        // proseguisse, il porto parlerebbe dove l'oracolo tace.
        let t = TmpIsolata::nuova("soglia-msg-stringa");
        let p = transcript(
            &t.dir,
            "t.jsonl",
            &[
                &riga_usage(144_000),
                r#"{"type":"x","message":"ciao","usage":1}"#,
            ],
        );
        assert_eq!(context_used(&p), None);
    }

    #[test]
    fn una_riga_illeggibile_non_interrompe_la_ricerca() {
        let t = TmpIsolata::nuova("soglia-riga-rotta");
        let p = transcript(
            &t.dir,
            "t.jsonl",
            &[&riga_usage(144_000), r#"{"usage": rotta"#],
        );
        assert_eq!(context_used(&p), Some(144_000));
    }

    #[test]
    fn il_gradino_si_annuncia_una_volta_sola_e_poi_quello_dopo() {
        let _t = TmpIsolata::nuova("soglia-gradini");
        let stato = state_file("provasog");
        assert!(!already_said("", 70));
        // Primo annuncio: il file nasce col gradino dentro.
        fs::write(&stato, "70").unwrap();
        assert!(already_said(&fs::read_to_string(&stato).unwrap(), 70));
        // Il gradino superiore non è ancora stato detto.
        assert!(!already_said(&fs::read_to_string(&stato).unwrap(), 85));
        fs::write(&stato, "85").unwrap();
        assert!(already_said(&fs::read_to_string(&stato).unwrap(), 85));
        // E da lì in poi anche quello basso risulta detto: il file non torna
        // indietro, che è il freno «una volta per gradino».
        assert!(already_said(&fs::read_to_string(&stato).unwrap(), 70));
    }

    #[test]
    fn lo_stato_vive_in_tmpdir_col_nome_dell_originale() {
        let t = TmpIsolata::nuova("soglia-percorso");
        assert_eq!(
            state_file("abcdefgh"),
            t.dir.join("soglia-consegna-abcdefgh.stato")
        );
    }

    #[test]
    fn una_sessione_senza_nome_si_chiama_ignota() {
        let _t = TmpIsolata::nuova("soglia-sessione");
        assert_eq!(session_id(None).as_deref(), Some("ignota"));
        assert_eq!(
            session_id(Some(&serde_json::Value::Null)).as_deref(),
            Some("ignota")
        );
        assert_eq!(
            session_id(Some(&serde_json::json!(""))).as_deref(),
            Some("ignota")
        );
        assert_eq!(
            session_id(Some(&serde_json::json!(0))).as_deref(),
            Some("ignota")
        );
        // Otto CARATTERI, non otto byte.
        assert_eq!(
            session_id(Some(&serde_json::json!("abcdef01-2345-6789"))).as_deref(),
            Some("abcdef01")
        );
        // Un intero non si affetta: l'originale muore, e da fuori è silenzio.
        assert_eq!(session_id(Some(&serde_json::json!(4242))), None);
        assert_eq!(session_id(Some(&serde_json::json!({"a": 1}))), None);
    }
}
