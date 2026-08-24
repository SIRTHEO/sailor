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

/// Il punto di ripresa da nominare quando il transcript non porta il documento.
///
/// L'indice delle memorie del progetto sta accanto al transcript
/// (`<progetto>/memory/MEMORY.md`), quindi si ricava dal percorso invece di
/// indovinare la casa. Se non esiste si risponde vuoto: una riga che promette
/// un file inesistente manda chi riprende a cercare a mano, che è il difetto
/// che si sta chiudendo.
fn resume_fallback(transcript: &str) -> String {
    let Some(project) = std::path::Path::new(transcript).parent() else {
        return String::new();
    };
    let index = project.join("memory").join("MEMORY.md");
    if index.is_file() {
        index.to_string_lossy().into_owned()
    } else {
        String::new()
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
    let (next, line) = guards::long_session::judge(&transcript, &resume_fallback(path), &said)?;
    // La scrittura precede l'avviso e i suoi errori si ingoiano: se il file non
    // si può scrivere il gancio parla lo stesso, e al massimo si ripeterà.
    let _ = fs::write(&state, next);
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

    fn payload(dir: &std::path::Path, turns: usize) -> String {
        payload_with(dir, turns, false)
    }

    /// Un transcript finto di `turns` turni, con o senza la consegna in coda.
    fn payload_with(dir: &std::path::Path, turns: usize, handed_off: bool) -> String {
        let mut lines: Vec<String> = (0..turns)
            .map(|i| format!(r#"{{"type":"assistant","message":{{"id":"m{i}","usage":{{"input_tokens":1}}}}}}"#))
            .collect();
        if handed_off {
            lines.push(r#"{"type":"assistant","message":{"content":[{"type":"tool_use","name":"Skill","input":{"skill":"handoff"}}]}}"#.to_string());
        }
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
        let kept = fs::read_to_string(t.dir.join("sessione-lunga-abcdef01.stato")).unwrap();
        assert_eq!(guards::long_session::State::parse(&kept).said, 100);
        assert_eq!(decide(&payload(&t.dir, 150)), None);
        assert!(decide(&payload(&t.dir, 200)).is_some());
    }

    #[test]
    fn the_line_depends_on_whether_the_handoff_was_already_written() {
        // Le due situazioni sono opposte, e finché la riga era una sola il
        // gancio diceva a chi aveva consegnato la stessa cosa che diceva a chi
        // non aveva ancora cominciato a consegnare.
        let t = IsolatedTmp::new("sessione-lunga-consegna");
        let plain = decide(&payload_with(&t.dir, 100, false)).expect("parla a 100");
        assert!(plain.contains("chiudi con `handoff`"), "{plain}");
        let after = decide(&payload_with(&t.dir, 200, true)).expect("parla a 200");
        assert!(after.contains("consegna già scritta"), "{after}");
        assert!(!after.contains("chiudi con `handoff`"), "{after}");
        // Al giro dopo la frase cambia e il numero cresce.
        let again = decide(&payload_with(&t.dir, 300, true)).expect("parla a 300");
        assert!(again.starts_with("2º avviso"), "{again}");
        assert_ne!(after, again);
    }

    #[test]
    fn the_index_of_the_project_is_the_fallback_resume_point() {
        let t = IsolatedTmp::new("sessione-lunga-ripiego");
        assert_eq!(resume_fallback(t.dir.join("t.jsonl").to_str().unwrap()), "");
        let memory = t.dir.join("memory");
        fs::create_dir_all(&memory).unwrap();
        fs::write(memory.join("MEMORY.md"), "indice").unwrap();
        assert_eq!(
            resume_fallback(t.dir.join("t.jsonl").to_str().unwrap()),
            memory.join("MEMORY.md").to_string_lossy()
        );
        let after = decide(&payload_with(&t.dir, 100, true)).expect("parla a 100");
        assert!(after.contains("MEMORY.md"), "{after}");
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
