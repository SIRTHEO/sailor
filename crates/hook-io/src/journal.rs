//! Il registro comune dei ganci: una riga per ogni caso di sua competenza.
//!
//! Porta di `scripts/hook-log.js`, stesso file e stesso formato — i due devono
//! poter scrivere sullo stesso `state/ganci.jsonl` durante la migrazione, e chi
//! lo legge non deve accorgersi di quale dei due ha scritto la riga.
//!
//! PERCHÉ ESISTE. Per sapere quante volte sei ganci avevano cambiato una
//! decisione è servita mezza giornata di lettura delle trascrizioni, e per metà
//! di loro la risposta è rimasta «non si sa». Un presidio che non registra i
//! propri passaggi dichiara di lavorare e non lo si può smentire.
//!
//! Si registra **anche il passaggio**, non solo il blocco: senza denominatore
//! non esce nessun tasso.
//!
//! Fail-open ovunque: il registro non deve mai poter fermare un gancio.

use std::fmt::Write as _;
use std::fs::{create_dir_all, rename, OpenOptions};
use std::io::Write as _;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

const CEILING_BYTES: u64 = 5 * 1024 * 1024;

fn folder() -> PathBuf {
    home().join(".claude").join("state")
}

fn home() -> PathBuf {
    std::env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("/"))
}

/// Un valore del registro. **Il tipo conta**: il JavaScript scriveva
/// `"conteggio":16` come numero, e la prima versione di questo modulo lo
/// scriveva come `"16"`. Chi legge il registro somma quel campo, quindi due
/// righe formalmente uguali diventavano incompatibili — trovato solo guardando
/// il file prodotto, non l'esito del gancio.
pub enum Field {
    Text(String),
    Number(i64),
    Bool(bool),
}

impl Field {
    pub fn write_to(&self, out: &mut String) {
        match self {
            Field::Text(s) => out.push_str(&quote(s)),
            Field::Number(n) => {
                let _ = write!(out, "{n}");
            }
            Field::Bool(b) => out.push_str(if *b { "true" } else { "false" }),
        }
    }
}

impl From<&str> for Field {
    fn from(s: &str) -> Field {
        Field::Text(s.to_string())
    }
}

impl From<String> for Field {
    fn from(s: String) -> Field {
        Field::Text(s)
    }
}

impl From<i64> for Field {
    fn from(n: i64) -> Field {
        Field::Number(n)
    }
}

/// Se questo processo sta servendo una sessione vera. Lo dichiara `read_input`,
/// che è il punto per cui passa ogni ingresso di gancio.
static LIVE_RUN: AtomicBool = AtomicBool::new(false);

/// Dichiara che l'ingresso appena letto viene (o no) da una sessione vera.
pub fn mark_live_run(live: bool) {
    LIVE_RUN.store(live, Ordering::Relaxed);
}

/// La sessione che il processo sta servendo, negli otto caratteri con cui la
/// scrivono i ganci che già la registrano.
///
/// PERCHÉ STA QUI E NON NEI CHIAMANTI. Il campo si scrive passandolo in `extra`,
/// e sei ganci su dieci non lo passavano: 4.500 righe su 12.200 — il 37% del
/// registro — non dicono a quale sessione appartengono, e fra queste stanno
/// tutte le 2.182 di `cd-guard`. Senza quel campo la domanda «quante sessioni
/// tocca questo guasto» non ha risposta, ed è la domanda con cui in questa casa
/// si decide cosa riparare prima. Chiederlo a ogni chiamante avrebbe lasciato
/// scoperto il prossimo gancio scritto; qui lo dichiara chi legge l'ingresso, che
/// è un punto solo.
static CURRENT_SESSION: Mutex<Option<String>> = Mutex::new(None);

/// Dichiara quale sessione sta servendo questo processo. La chiamano i due punti
/// che leggono il payload, insieme a `mark_live_run`.
pub fn mark_session(session: &str) {
    let s = session.trim();
    if s.is_empty() {
        return;
    }
    if let Ok(mut guard) = CURRENT_SESSION.lock() {
        *guard = Some(s.chars().take(8).collect());
    }
}

/// Scrive una riga nel registro. `extra` mantiene l'ordine in cui è passato:
/// il formato è ricopiato dal JavaScript, e chi legge i due file affiancati non
/// deve vedere differenze nemmeno nell'ordine delle chiavi.
///
/// LE PROVE SI MARCANO, NON SI DIROTTANO. Chi prova un gancio dal terminale
/// scriveva nel registro di produzione righe indistinguibili da quelle vere:
/// 233 su 6.832 il 19/08/2026, e le legge chi misura quanto un gate morde.
/// Marcarle lascia il file dov'è e la riga ispezionabile; dirottarle
/// perderebbe righe vere ogni volta che il criterio sbaglia, ed è il danno
/// peggiore dei due.
pub fn record(hook: &str, decision: &str, reason: &str, extra: &[(&str, Field)]) {
    let dir = folder();
    let path = dir.join("ganci.jsonl");

    if let Ok(meta) = std::fs::metadata(&path) {
        if meta.len() > CEILING_BYTES {
            let _ = rename(&path, path.with_extension("jsonl.1"));
        }
    }
    if create_dir_all(&dir).is_err() {
        return;
    }

    let mut line = String::with_capacity(160);
    line.push('{');
    let _ = write!(line, "\"t\":{}", quote(&now_iso8601_python()));
    let _ = write!(line, ",\"gancio\":{}", quote(hook));
    let _ = write!(line, ",\"decisione\":{}", quote(decision));
    let _ = write!(line, ",\"motivo\":{}", quote(reason));
    let mut has_session = false;
    for (key, value) in extra {
        if *key == "session" {
            has_session = true;
        }
        let _ = write!(line, ",{}:", quote(key));
        value.write_to(&mut line);
    }
    // Solo se il chiamante non l'ha già scritta: chi la passa in `extra` la
    // mette dove vuole nell'ordine, e quell'ordine è parte del formato.
    if !has_session {
        if let Some(s) = CURRENT_SESSION.lock().ok().and_then(|g| g.clone()) {
            let _ = write!(line, ",\"session\":{}", quote(&s));
        }
    }
    // In coda, e solo quando c'è: le righe vere restano byte per byte quelle
    // che gli script di oggi sanno leggere.
    if !LIVE_RUN.load(Ordering::Relaxed) {
        line.push_str(",\"prova\":true");
    }
    line.push_str("}\n");

    if let Ok(mut f) = OpenOptions::new().create(true).append(true).open(&path) {
        let _ = f.write_all(line.as_bytes());
    }
}

fn quote(s: &str) -> String {
    serde_json::to_string(s).unwrap_or_else(|_| "\"\"".into())
}

/// Il timestamp nella forma di `datetime.now(timezone.utc).isoformat(timespec="seconds")`
/// — «2026-08-17T12:05:00+00:00» — che è quella che scrive `hook_log.py`.
///
/// PERCHÉ SI SEGUE L'ORACOLO ANCHE QUI. `ganci.jsonl` è un archivio che leggono
/// script che non conosciamo tutti, e al 17/08/2026 conteneva **due** forme:
/// 3314 righe con i millisecondi e la `Z`, scritte dai ganci già portati, e 2045
/// con l'offset, scritte dall'originale. Nessuna delle due è sbagliata da sola;
/// averle mescolate senza accorgersene sì, ed è il motivo per cui questa
/// funzione esiste invece di lasciar correre. La forma che vince è quella del
/// Python, che è ciò che il resto della configurazione sa leggere.
pub fn now_iso8601_python() -> String {
    let full = now_iso8601();
    format!("{}+00:00", &full[..19])
}

/// Lo stesso istante senza i millisecondi — «2026-08-16T21:42:27Z».
///
/// Tre formati e non uno perché i registri sono nati in tempi diversi e chi li
/// legge si aspetta quello che c'è: la raccolta delle osservazioni usa questo
/// (`time.strftime` del Python), il registro dei ganci quello sopra.
/// Uniformarli tutti romperebbe chi li interroga.
pub fn now_iso8601_seconds() -> String {
    let full = now_iso8601();
    format!("{}Z", &full[..19])
}

/// `new Date().toISOString()` — «2026-08-16T21:42:27.675Z».
///
/// Calcolata a mano invece di tirarsi dietro `chrono`: sono trenta righe di
/// aritmetica del calendario contro una dipendenza che questi binari
/// pagherebbero a ogni compilazione, e il formato non cambierà mai.
fn now_iso8601() -> String {
    let d = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default();
    let millis = d.as_millis() as i64;
    let (secs, ms) = (millis.div_euclid(1000), millis.rem_euclid(1000));
    let days = secs.div_euclid(86_400);
    let rem = secs.rem_euclid(86_400);
    let (y, m, day) = civil_from_days(days);
    format!(
        "{y:04}-{m:02}-{day:02}T{:02}:{:02}:{:02}.{ms:03}Z",
        rem / 3600,
        (rem % 3600) / 60,
        rem % 60
    )
}

/// L'algoritmo di Howard Hinnant: dai giorni dall'epoca alla data civile,
/// senza tabelle e senza casi speciali per i bisestili.
pub(crate) fn civil_from_days(days: i64) -> (i64, u32, u32) {
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    (if m <= 2 { y + 1 } else { y }, m, d)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn it_converts_known_days_to_the_right_civil_date() {
        assert_eq!(civil_from_days(0), (1970, 1, 1));
        assert_eq!(civil_from_days(19_723), (2024, 1, 1)); // anno bisestile
        assert_eq!(civil_from_days(19_784), (2024, 3, 2)); // subito dopo il 29/02
        assert_eq!(civil_from_days(-1), (1969, 12, 31));
    }

    #[test]
    fn it_stamps_a_javascript_shaped_timestamp() {
        let t = now_iso8601();
        assert_eq!(t.len(), 24, "atteso 2026-08-16T21:42:27.675Z, ottenuto {t}");
        assert!(t.ends_with('Z'));
        assert_eq!(&t[4..5], "-");
        assert_eq!(&t[10..11], "T");
        assert_eq!(&t[19..20], ".");
    }

    #[test]
    fn the_journal_stamps_the_shape_python_writes() {
        // `ganci.jsonl` è un archivio già misto: 3314 righe con i millisecondi e
        // la Z contro 2045 con l'offset. Chi lo legge conosce la seconda forma,
        // ed è quella che il registro deve continuare a scrivere.
        let t = now_iso8601_python();
        assert_eq!(t.len(), 25, "atteso 2026-08-17T12:05:00+00:00, ottenuto {t}");
        assert!(t.ends_with("+00:00"), "{t}");
        assert!(!t.contains('.'), "i millisecondi non ci vanno: {t}");
        // E l'altra forma resta com'è: la usa la raccolta delle osservazioni.
        assert!(now_iso8601_seconds().ends_with('Z'));
    }

    #[test]
    fn it_escapes_what_would_break_the_line() {
        assert_eq!(quote("a\"b"), "\"a\\\"b\"");
        assert_eq!(quote("riga\nriga"), "\"riga\\nriga\"");
    }
}
