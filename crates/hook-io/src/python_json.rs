//! `json.dumps` di Python, riprodotto fedelmente.
//!
//! PERCHÉ NON BASTA `serde_json::to_string`. I registri di questa
//! configurazione sono archivi storici continui, letti da script che non
//! conosciamo tutti, e le righe scritte da Python e quelle scritte da Rust
//! finiscono nello **stesso file**. Tre differenze rendono le due
//! serializzazioni non intercambiabili:
//!
//! 1. **`ensure_ascii`** — Python scrive `è` dove serde scrive `è`. Non è
//!    cosmetico: il valore viene poi troncato a un numero di **caratteri**, e
//!    un accento che ne occupa sei invece di uno sposta il punto di taglio. Su
//!    un comando tutto accentato il Python conserva un sesto del testo.
//! 2. **I separatori** — `, ` e `: `, con lo spazio.
//! 3. **L'ordine delle chiavi** — Python conserva quello di inserimento; serde,
//!    senza `preserve_order`, ordina alfabeticamente.
//!
//! Migliorare uno di questi tre è legittimo, ma è un cambio di formato e va
//! deciso e dichiarato: qui si replica, perché l'originale resta l'oracolo
//! finché non lo cancelliamo.

use serde_json::Value;

/// La stessa stringa che produrrebbe `json.dumps(value)` con i parametri
/// predefiniti, cioè con `ensure_ascii=True`.
pub fn dumps(value: &Value) -> String {
    let mut out = String::new();
    write_value(value, true, &mut out);
    out
}

/// `json.dumps(value, ensure_ascii=False)`: gli accenti restano sé stessi.
///
/// Le due forme convivono nella stessa configurazione e non sono
/// intercambiabili: la raccolta delle osservazioni scrive in ASCII, il registro
/// di `linear-sola-lettura` no — e le sue 13.560 righe si leggono a occhio,
/// dove `perché` sarebbe un peggioramento. Chi porta un gancio deve
/// guardare quale delle due usa l'originale, non indovinare.
pub fn dumps_unicode(value: &Value) -> String {
    let mut out = String::new();
    write_value(value, false, &mut out);
    out
}

fn write_value(value: &Value, ascii_only: bool, out: &mut String) {
    match value {
        Value::Null => out.push_str("null"),
        Value::Bool(true) => out.push_str("true"),
        Value::Bool(false) => out.push_str("false"),
        Value::Number(n) => out.push_str(&n.to_string()),
        Value::String(s) => write_string(s, ascii_only, out),
        Value::Array(items) => {
            out.push('[');
            for (i, item) in items.iter().enumerate() {
                if i > 0 {
                    out.push_str(", ");
                }
                write_value(item, ascii_only, out);
            }
            out.push(']');
        }
        Value::Object(map) => {
            out.push('{');
            for (i, (key, item)) in map.iter().enumerate() {
                if i > 0 {
                    out.push_str(", ");
                }
                write_string(key, ascii_only, out);
                out.push_str(": ");
                write_value(item, ascii_only, out);
            }
            out.push('}');
        }
    }
}

/// Le regole di escape di Python: i controlli con i nomi brevi dove esistono,
/// `\uXXXX` per tutto il resto — compreso ogni carattere fuori dall'ASCII.
fn write_string(s: &str, ascii_only: bool, out: &mut String) {
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            '\u{08}' => out.push_str("\\b"),
            '\u{0c}' => out.push_str("\\f"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c if c.is_ascii() || !ascii_only => out.push(c),
            c => {
                // Fuori dal piano base servono due unità: Python emette la
                // coppia di surrogati, non un solo `\u` a cinque cifre.
                let mut buf = [0u16; 2];
                for unit in c.encode_utf16(&mut buf) {
                    out.push_str(&format!("\\u{unit:04x}"));
                }
            }
        }
    }
    out.push('"');
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn it_escapes_every_non_ascii_character() {
        assert_eq!(dumps(&json!("\u{e8}")), r#""\u00e8""#);
        assert_eq!(dumps(&json!("perch\u{e9}")), r#""perch\u00e9""#);
    }

    #[test]
    fn beyond_the_basic_plane_it_writes_a_surrogate_pair() {
        // json.dumps("🙂") == '"\\ud83d\\ude42"'
        assert_eq!(dumps(&json!("\u{1f642}")), r#""\ud83d\ude42""#);
    }

    #[test]
    fn it_separates_with_a_space_the_way_python_does() {
        assert_eq!(dumps(&json!({"a": 1, "b": 2})), r#"{"a": 1, "b": 2}"#);
        assert_eq!(dumps(&json!([1, 2])), "[1, 2]");
    }

    #[test]
    fn the_short_names_win_over_the_numeric_escape() {
        assert_eq!(dumps(&json!("a\nb\tc")), r#""a\nb\tc""#);
        assert_eq!(dumps(&json!("\u{1}")), r#""\u0001""#);
    }

    #[test]
    fn without_ensure_ascii_the_accents_stay_themselves() {
        assert_eq!(dumps_unicode(&json!("perch\u{e9}")), "\"perch\u{e9}\"");
        assert_eq!(dumps_unicode(&json!("\u{1f642}")), "\"\u{1f642}\"");
        // gli escape veri restano tali: `ensure_ascii` non li riguarda
        assert_eq!(dumps_unicode(&json!("a\nb\"c")), r#""a\nb\"c""#);
        assert_eq!(dumps_unicode(&json!("\u{1}")), r#""\u0001""#);
    }

    #[test]
    fn it_keeps_the_insertion_order_of_the_keys() {
        // vale solo con la feature `preserve_order`, che è il motivo per cui è
        // accesa: senza, questo test fallisce e il file diverge in silenzio
        let v: Value = serde_json::from_str(r#"{"z": 1, "a": 2}"#).unwrap();
        assert_eq!(dumps(&v), r#"{"z": 1, "a": 2}"#);
    }
}
