//! Il gancio UserPromptSubmit che al centesimo turno dice quanto costa restare.
//!
//! Il giudizio sta in `guards::long_session`; qui c'è ciò che tocca il mondo:
//! il payload su stdin, il transcript letto **per intero** (i turni si contano
//! dall'inizio, non dalla coda) e il file di stato per sessione sotto `TMPDIR`,
//! nella stessa sede di `handoff_threshold`, per la stessa ragione.
//!
//! OGNI ERRORE È SILENZIOSO: un `UserPromptSubmit` che si rompe rompe l'invio
//! del prompt. Dove qualcosa manca o non si legge, si esce 0 senza dire niente.
//!
//! Valvola: `LONG_SESSION=off`.

use hook_io::Mode;
use std::fs;
use std::io::Read;
use std::path::PathBuf;

/// Dove la sessione tiene l'ultimo centinaio annunciato.
fn state_file(session: &str) -> PathBuf {
    let base = std::env::var("TMPDIR").unwrap_or_else(|_| "/tmp".to_string());
    PathBuf::from(base).join(format!("sessione-lunga-{session}.stato"))
}

/// I primi otto caratteri dell'identificativo, o `ignota`.
fn session_id(v: Option<&serde_json::Value>) -> String {
    match v {
        Some(serde_json::Value::String(s)) if !s.is_empty() => s.chars().take(8).collect(),
        _ => "ignota".to_string(),
    }
}

pub fn run() -> i32 {
    if Mode::from_env("LONG_SESSION") == Mode::Off {
        return 0;
    }
    let mut raw = String::new();
    if std::io::stdin().read_to_string(&mut raw).is_err() {
        return 0;
    }
    let Ok(payload) = serde_json::from_str::<serde_json::Value>(&raw) else {
        return 0;
    };
    let Some(obj) = payload.as_object() else {
        return 0;
    };
    let Some(path) = obj.get("transcript_path").and_then(|v| v.as_str()) else {
        return 0;
    };
    let Ok(transcript) = fs::read_to_string(path) else {
        return 0;
    };
    let state = state_file(&session_id(obj.get("session_id")));
    let said = fs::read_to_string(&state).unwrap_or_default();
    let Some((step, line)) = guards::long_session::judge(&transcript, &said) else {
        return 0;
    };
    // La scrittura precede l'avviso e i suoi errori si ingoiano: se il file non
    // si può scrivere il gancio parla lo stesso, e al massimo si ripeterà.
    let _ = fs::write(&state, step.to_string());
    println!("{line}");
    0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_state_lives_in_tmpdir_under_the_session_name() {
        let home = crate::test_home::HomeIsolata::nuova("sessione-lunga-percorso");
        let previous = std::env::var("TMPDIR").ok();
        std::env::set_var("TMPDIR", &home.dir);
        let p = state_file("abcdefgh");
        match previous {
            Some(v) => std::env::set_var("TMPDIR", v),
            None => std::env::remove_var("TMPDIR"),
        }
        assert_eq!(p, home.dir.join("sessione-lunga-abcdefgh.stato"));
    }

    #[test]
    fn a_session_without_a_name_is_called_ignota() {
        assert_eq!(session_id(None), "ignota");
        assert_eq!(session_id(Some(&serde_json::json!(""))), "ignota");
        assert_eq!(session_id(Some(&serde_json::json!(42))), "ignota");
        assert_eq!(
            session_id(Some(&serde_json::json!("abcdef01-2345"))),
            "abcdef01"
        );
    }
}
