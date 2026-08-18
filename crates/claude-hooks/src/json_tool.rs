//! Le tre righe di `python3 -c` che restavano dentro i ganci scritti in shell.
//!
//! PERCHE' ESISTE. Tolti i ripieghi Python da `settings.json` (18/08/2026), i
//! ganci chiamano solo il binario — tranne tre script di shell che invocavano
//! `python3` per un mestiere solo: leggere un campo dal JSON del gancio, o
//! avvolgere del testo in un JSON di risposta. Tre interpreti avviati per
//! `d.get("session_id", "")`.
//!
//! I TRE MODI corrispondono uno a uno a cio' che quegli script facevano:
//!
//!     json get <chiave>     `print(json.load(sys.stdin).get(chiave, ""))`
//!     json context <evento> avvolge stdin in `hookSpecificOutput.additionalContext`
//!     json message          avvolge stdin in `systemMessage`
//!
//! L'EQUIVALENZA SI MISURA SUL COMPORTAMENTO VERO, non su quello promesso. Il
//! `print` di Python non stampa il JSON di un valore, stampa la sua `repr`: un
//! booleano diventa `True`, un assente `None`, una lista `['a']` con gli apici
//! semplici. Nei quattro usi reali i campi sono stringhe, ma riprodurre la
//! forma Python costa poche righe e toglie di mezzo la domanda «e se un giorno
//! il campo non fosse una stringa».
//!
//! NON FALLISCE MAI RUMOROSAMENTE: ogni `python3 -c` era seguito da
//! `2>/dev/null || true`, quindi qualunque cosa succedesse la vita andava
//! avanti. Qui vale lo stesso, e l'uscita e' 0 sempre.
//!
//! MA «non fallisce» non vuol dire «stampa sempre qualcosa», ed e' la cosa che
//! ho scritto sbagliata alla prima stesura: su un JSON valido che non e' un
//! dizionario l'originale muore **prima** di stampare, mentre su uno illeggibile
//! stampa una riga vuota. Due casi che si assomigliano e si comportano al
//! contrario. La differenza sta in `field`, ed e' venuta fuori dal confronto.

use serde_json::Value;
use std::io::Read;

/// La forma che `print()` di Python darebbe a questo valore.
///
/// Serve per i campi che non sono stringhe: `json.dumps` scriverebbe `true`
/// dove Python stampa `True`, e la differenza si vedrebbe nel primo script che
/// legge un campo booleano.
fn python_repr(v: &Value) -> String {
    match v {
        Value::Null => "None".into(),
        Value::Bool(true) => "True".into(),
        Value::Bool(false) => "False".into(),
        Value::String(s) => s.clone(),
        Value::Number(n) => n.to_string(),
        Value::Array(items) => {
            let inner: Vec<String> = items.iter().map(python_quoted).collect();
            format!("[{}]", inner.join(", "))
        }
        Value::Object(map) => {
            let inner: Vec<String> = map
                .iter()
                .map(|(k, v)| format!("'{}': {}", k, python_quoted(v)))
                .collect();
            format!("{{{}}}", inner.join(", "))
        }
    }
}

/// Dentro una lista o un dizionario, Python mette gli apici alle stringhe.
fn python_quoted(v: &Value) -> String {
    match v {
        Value::String(s) => format!("'{s}'"),
        other => python_repr(other),
    }
}

fn read_stdin() -> String {
    let mut buf = String::new();
    let _ = std::io::stdin().read_to_string(&mut buf);
    buf
}

/// Il campo di primo livello. `None` dove l'originale non stampava affatto.
///
/// I TRE CASI SONO TRE, e il confronto col Python li ha separati a forza:
///
///   - JSON illeggibile → il `try` dell'originale mette `{}`, e `.get` restituisce
///     la stringa vuota: si stampa **una riga vuota**;
///   - JSON valido ma **non un dizionario** (`[1,2,3]`, `"testo"`, `null`) → il
///     `try` non lo cattura, perche' il caricamento e' riuscito: e' `.get` a
///     sollevare `AttributeError`, il processo muore e su stdout **non arriva
///     niente**, nemmeno l'a capo;
///   - dizionario senza quella chiave → riga vuota.
///
/// Il secondo caso e' l'unico che si vede solo confrontando: la prima stesura
/// stampava una riga vuota anche li', e `tools/compare-json-tool.py` lo ha
/// trovato su 15 casi. In uno script di shell la differenza non e' cosmetica —
/// `$(...)` toglie gli a capo finali, ma il codice di uscita cambia, e con
/// `set -o pipefail` intorno cambierebbe anche il seguito.
pub fn field(raw: &str, key: &str) -> Option<String> {
    match serde_json::from_str::<Value>(raw) {
        Ok(Value::Object(map)) => Some(match map.get(key) {
            Some(v) => python_repr(v),
            None => String::new(),
        }),
        // Caricato ma non un dizionario: qui l'originale moriva.
        Ok(_) => None,
        // Non caricato: `except Exception` lo trasforma in `{}`.
        Err(_) => Some(String::new()),
    }
}

/// Una stringa nella forma esatta che `json.dumps` le darebbe.
///
/// DUE DIFFERENZE DA `serde_json`, e nessuna delle due e' cosmetica quando il
/// confronto e' byte a byte:
///
///   1. `ensure_ascii=True` e' il **default** di Python: una `e` accentata esce
///      come la sequenza `\u00e8`, e un'emoji come una coppia di surrogati,
///      mentre `serde_json` scrive l'UTF-8 cosi' com'e';
///   2. i separatori: `", "` e `": "` contro i compatti di serde. E' la stessa
///      trappola gia' incontrata in `relay.py`, dove pero' si e' scelto il verso
///      opposto perche' li' i file li scriveva il Rust.
///
/// `ensure_ascii` sta come parametro perche' in questa configurazione le due
/// forme convivono: i ganci di shell chiamano `json.dumps` col default, mentre
/// i registri e i messaggi per Theo usano `ensure_ascii=False` — dove una riga
/// piena di `\u00e8` sarebbe illeggibile proprio a chi deve leggerla.
pub fn python_json_string_with(s: &str, ensure_ascii: bool) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            '\u{8}' => out.push_str("\\b"),
            '\u{c}' => out.push_str("\\f"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c if (c as u32) < 0x7f => out.push(c),
            c if !ensure_ascii => out.push(c),
            c => {
                let mut buf = [0u16; 2];
                for unit in c.encode_utf16(&mut buf) {
                    out.push_str(&format!("\\u{unit:04x}"));
                }
            }
        }
    }
    out.push('"');
    out
}

fn python_json_string(s: &str) -> String {
    python_json_string_with(s, true)
}

/// La risposta che un gancio SessionStart da' per aggiungere contesto.
pub fn context(event: &str, text: &str) -> String {
    format!(
        "{{\"hookSpecificOutput\": {{\"hookEventName\": {}, \"additionalContext\": {}}}}}",
        python_json_string(event),
        python_json_string(text)
    )
}

/// La risposta che porta una riga all'utente e nient'altro.
pub fn message(text: &str) -> String {
    format!("{{\"systemMessage\": {}}}", python_json_string(text))
}

pub fn run() -> i32 {
    let args: Vec<String> = std::env::args().skip(2).collect();
    let mode = args.first().map(String::as_str).unwrap_or("");
    let arg = args.get(1).cloned().unwrap_or_default();
    match mode {
        // Niente `println!` quando l'originale moriva: la riga vuota di troppo
        // e' proprio la divergenza che il confronto ha trovato.
        "get" => {
            if let Some(value) = field(&read_stdin(), &arg) {
                println!("{value}");
            }
        }
        "context" => println!("{}", context(&arg, &read_stdin())),
        "message" => println!("{}", message(&read_stdin())),
        other => {
            eprintln!("json: unknown mode {other:?} (expected: get <key> | context <event> | message)");
            // Uscita 0 lo stesso: questi tre modi vivono dentro ganci di shell,
            // e un codice non-zero da qui si vedrebbe come un gancio guasto
            // invece che come un argomento sbagliato.
        }
    }
    0
}

#[cfg(test)]
mod tests {
    use super::*;

    const PAYLOAD: &str = r#"{"session_id":"abc","cwd":"/tmp","trigger":"auto","n":3,"ok":true,"nothing":null}"#;

    #[test]
    fn reads_string_fields_the_way_python_printed_them() {
        assert_eq!(field(PAYLOAD, "session_id").as_deref(), Some("abc"));
        assert_eq!(field(PAYLOAD, "cwd").as_deref(), Some("/tmp"));
        assert_eq!(field(PAYLOAD, "trigger").as_deref(), Some("auto"));
    }

    #[test]
    fn a_missing_key_is_an_empty_line() {
        assert_eq!(field(PAYLOAD, "transcript_path").as_deref(), Some(""));
    }

    #[test]
    fn unreadable_json_prints_an_empty_line() {
        // `except Exception` dell'originale trasforma questi in `{}`.
        assert_eq!(field("non sono json", "session_id").as_deref(), Some(""));
        assert_eq!(field("", "session_id").as_deref(), Some(""));
    }

    #[test]
    fn valid_json_that_is_not_an_object_prints_nothing() {
        // Qui il `try` non copre: e' `.get` a sollevare, e Python muore prima
        // di stampare. Trovato da `tools/compare-json-tool.py`, non a mente.
        assert_eq!(field("[1,2]", "session_id"), None);
        assert_eq!(field("\"just a string\"", "session_id"), None);
        assert_eq!(field("null", "session_id"), None);
        assert_eq!(field("3", "session_id"), None);
    }

    #[test]
    fn non_string_values_keep_the_python_shape() {
        assert_eq!(field(PAYLOAD, "n").as_deref(), Some("3"));
        assert_eq!(field(PAYLOAD, "ok").as_deref(), Some("True"));
        assert_eq!(field(PAYLOAD, "nothing").as_deref(), Some("None"));
        assert_eq!(field(r#"{"a":["x","y"]}"#, "a").as_deref(), Some("['x', 'y']"));
        assert_eq!(field(r#"{"a":{"b":1}}"#, "a").as_deref(), Some("{'b': 1}"));
    }

    #[test]
    fn context_and_message_are_valid_json() {
        let c: Value = serde_json::from_str(&context("SessionStart", "riga\ncon a capo")).unwrap();
        assert_eq!(c["hookSpecificOutput"]["hookEventName"], "SessionStart");
        assert_eq!(c["hookSpecificOutput"]["additionalContext"], "riga\ncon a capo");

        let m: Value = serde_json::from_str(&message("con \"virgolette\" dentro")).unwrap();
        assert_eq!(m["systemMessage"], "con \"virgolette\" dentro");
    }

    #[test]
    fn the_wire_shape_is_the_one_python_wrote() {
        // Gli spazi dopo `,` e `:` sono il default di `json.dumps`, e senza di
        // loro il confronto byte a byte diverge su ogni caso.
        assert_eq!(message("ok"), r#"{"systemMessage": "ok"}"#);
        assert_eq!(
            context("SessionStart", "x"),
            r#"{"hookSpecificOutput": {"hookEventName": "SessionStart", "additionalContext": "x"}}"#
        );
    }

    #[test]
    fn non_ascii_is_escaped_the_way_ensure_ascii_does() {
        assert_eq!(python_json_string("caffe\u{300}"), r#""caffe\u0300""#);
        assert_eq!(python_json_string("\u{e0}"), r#""\u00e0""#);
        // Fuori dal piano base: la coppia di surrogati, come in UTF-16.
        assert_eq!(python_json_string("\u{1F680}"), r#""\ud83d\ude80""#);
        assert_eq!(python_json_string("a\u{1}b"), r#""a\u0001b""#);
    }
}
