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

/// Dal payload del gancio alla riga da dire, se c'è. Tocca il transcript e il
/// file di stato; stdin e stdout restano fuori, così si prova due volte di fila.
fn decide(raw: &str) -> Option<String> {
    if Mode::from_env("LONG_SESSION") == Mode::Off {
        return None;
    }
    let payload = serde_json::from_str::<serde_json::Value>(raw).ok()?;
    let obj = payload.as_object()?;
    let path = obj.get("transcript_path")?.as_str()?;
    let transcript = fs::read_to_string(path).ok()?;
    let state = state_file(&session_id(obj.get("session_id")));
    let said = fs::read_to_string(&state).unwrap_or_default();
    let (step, line) = guards::long_session::judge(&transcript, &said)?;
    // La scrittura precede l'avviso e i suoi errori si ingoiano: se il file non
    // si può scrivere il gancio parla lo stesso, e al massimo si ripeterà.
    let _ = fs::write(&state, step.to_string());
    Some(line)
}

pub fn run() -> i32 {
    let mut raw = String::new();
    if std::io::stdin().read_to_string(&mut raw).is_err() {
        return 0;
    }
    if let Some(line) = decide(&raw) {
        println!("{line}");
    }
    0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_state_lives_in_tmpdir_under_the_session_name() {
        let t = IsolatedTmp::new("sessione-lunga-percorso");
        assert_eq!(state_file("abcdefgh"), t.dir.join("sessione-lunga-abcdefgh.stato"));
    }

    /// Una `TMPDIR` usa-e-getta sotto una casa isolata (che tiene il lucchetto
    /// sulle variabili d'ambiente), rimessa com'era alla fine.
    struct IsolatedTmp {
        _home: crate::test_home::HomeIsolata,
        previous: Vec<(&'static str, Option<String>)>,
        dir: PathBuf,
    }

    impl IsolatedTmp {
        fn new(name: &str) -> Self {
            let home = crate::test_home::HomeIsolata::nuova(name);
            let dir = home.dir.join("tmp");
            let _ = fs::create_dir_all(&dir);
            let previous = ["TMPDIR", "LONG_SESSION"]
                .iter()
                .map(|k| (*k, std::env::var(k).ok()))
                .collect();
            std::env::set_var("TMPDIR", &dir);
            std::env::remove_var("LONG_SESSION");
            Self { _home: home, previous, dir }
        }
    }

    impl Drop for IsolatedTmp {
        fn drop(&mut self) {
            for (key, value) in &self.previous {
                match value {
                    Some(v) => std::env::set_var(key, v),
                    None => std::env::remove_var(key),
                }
            }
        }
    }

    fn payload(dir: &PathBuf, turns: usize) -> String {
        let lines: Vec<String> = (0..turns)
            .map(|i| format!(r#"{{"type":"assistant","message":{{"id":"m{i}","usage":{{"input_tokens":1}}}}}}"#))
            .collect();
        let p = dir.join("t.jsonl");
        fs::write(&p, lines.join("\n")).unwrap();
        serde_json::json!({"session_id": "abcdef01-2345", "transcript_path": p})
            .to_string()
    }

    #[test]
    fn it_speaks_once_per_hundred_and_remembers_on_disk() {
        let t = IsolatedTmp::new("sessione-lunga-freno");
        assert_eq!(decide(&payload(&t.dir, 99)), None);
        let said = decide(&payload(&t.dir, 100)).expect("must speak at 100");
        assert!(said.starts_with("sessione a 100 turni"));
        assert_eq!(fs::read_to_string(t.dir.join("sessione-lunga-abcdef01.stato")).unwrap(), "100");
        assert_eq!(decide(&payload(&t.dir, 150)), None);
        assert!(decide(&payload(&t.dir, 200)).is_some());
    }

    #[test]
    fn the_valve_silences_it() {
        let t = IsolatedTmp::new("sessione-lunga-valvola");
        std::env::set_var("LONG_SESSION", "off");
        assert_eq!(decide(&payload(&t.dir, 300)), None);
        assert!(!t.dir.join("sessione-lunga-abcdef01.stato").exists());
    }

    #[test]
    fn a_missing_transcript_is_silence() {
        let _t = IsolatedTmp::new("sessione-lunga-assente");
        assert_eq!(decide(r#"{"session_id":"x","transcript_path":"/nessuno/qui"}"#), None);
        assert_eq!(decide("non json"), None);
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
