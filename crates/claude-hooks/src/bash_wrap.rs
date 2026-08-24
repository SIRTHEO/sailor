//! Il gancio `PreToolUse` su `Bash` che manda l'uscita dei comandi rumorosi
//! dentro il filtro, riscrivendo il comando con `updatedInput`.
//!
//! Il giudizio — quali famiglie, quali veti, che forma ha la riscrittura — sta
//! in `guards::bash_wrap`; qui c'è solo ciò che tocca il mondo: il payload su
//! stdin e la riscrittura su stdout.
//!
//! NON NEGA MAI, NON BLOCCA MAI: o riscrive `command`, o non stampa niente. Un
//! `PreToolUse` che nega qui vieterebbe di eseguire il comando, non di
//! filtrarne l'uscita — il danno peggiore che questo gancio possa fare.
//!
//! VA MESSO PER ULTIMO fra i ganci di `PreToolUse`/`Bash`. Gli altri giudicano
//! il comando che l'utente ha scritto; se questo girasse prima, giudicherebbero
//! l'avvolgimento — e `cd-guard`, che cerca un `cd` in testa, leggerebbe un
//! comando che non è quello deciso da nessuno.
//!
//! Valvola: `WRAP_BASH=off` spegne la riscrittura senza toccare
//! `settings.json`, che è di Theo.

use guards::bash_wrap;
use serde_json::Value;
use std::io::{Read, Write};

/// Il binario da invocare nel comando riscritto: **questo stesso eseguibile**,
/// per percorso assoluto. Chiederlo al sistema invece di scriverlo a mano tiene
/// insieme la copia di debug e quella in servizio: chi prova col binario nuovo
/// vede il filtro nuovo, non quello rilasciato.
fn own_path() -> String {
    std::env::current_exe()
        .map(|p| p.to_string_lossy().into_owned())
        .unwrap_or_else(|_| "claude-hooks".to_string())
}

/// Dal payload grezzo alla riscrittura da stampare, se c'è.
///
/// Isolata da stdin/stdout per potersi provare su un payload rotto senza un
/// processo, e col percorso del binario passato da fuori perché il caso di
/// prova non dipenda da dove è compilato.
fn process_with(payload: &Value, binary: &str) -> Option<Value> {
    if payload.get("tool_name").and_then(|v| v.as_str()) != Some("Bash") {
        return None; // invocato fuori dal proprio matcher: non è affar suo
    }
    if hook_io::Mode::from_env("WRAP_BASH") == hook_io::Mode::Off {
        return None;
    }
    let tool_input = payload.get("tool_input").filter(|v| v.is_object())?;
    // Un comando che gira in fondo non torna a nessuno: l'uscita non passerebbe
    // mai dal filtro, e il campo lo dichiara meglio di qualunque indizio nel
    // testo del comando.
    if tool_input
        .get("run_in_background")
        .and_then(|v| v.as_bool())
        .unwrap_or(false)
    {
        return None;
    }
    let command = tool_input.get("command").and_then(|v| v.as_str())?;
    if !bash_wrap::should_wrap(command) {
        return None;
    }

    let mut rewritten = tool_input.clone();
    rewritten
        .as_object_mut()
        .expect("filtrato sopra: e' un oggetto")
        .insert(
            "command".to_string(),
            Value::String(bash_wrap::rewrite(command, binary)),
        );
    Some(serde_json::json!({
        "hookSpecificOutput": {
            "hookEventName": "PreToolUse",
            "updatedInput": rewritten,
        }
    }))
}

pub fn run() -> i32 {
    run_from(&mut std::io::stdin(), &mut std::io::stdout(), &own_path())
}

fn run_from(input: &mut dyn Read, output: &mut dyn Write, binary: &str) -> i32 {
    let mut raw = String::new();
    if input.read_to_string(&mut raw).is_err() {
        return 0;
    }
    let Ok(payload) = serde_json::from_str::<Value>(&raw) else {
        return 0;
    };
    hook_io::mark_live_from_payload(&payload);
    if let Some(out) = process_with(&payload, binary) {
        let _ = writeln!(output, "{out}");
    }
    0
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bash_payload(command: &str) -> Value {
        serde_json::json!({
            "hook_event_name": "PreToolUse",
            "tool_name": "Bash",
            "tool_input": { "command": command, "description": "una prova" },
            "session_id": "abcdef01-2345",
        })
    }

    /// Un comando in lista viene riscritto, e gli altri campi dell'input
    /// restano dov'erano: `updatedInput` è l'input intero, non il solo campo
    /// cambiato.
    #[test]
    fn a_listed_command_is_rewritten_keeping_the_rest_of_the_input() {
        let out = process_with(&bash_payload("cargo test -p guards"), "/bin/ch")
            .expect("doveva riscrivere");
        let input = &out["hookSpecificOutput"]["updatedInput"];
        let command = input["command"].as_str().unwrap();
        assert!(command.contains("cargo test -p guards"), "{command}");
        assert!(command.contains("/bin/ch filter-output"), "{command}");
        assert_eq!(input["description"], "una prova");
        assert_eq!(out["hookSpecificOutput"]["hookEventName"], "PreToolUse");
    }

    /// Fuori lista non si stampa niente: il comando arriva all'esecuzione
    /// identico, e nessun permesso viene rivalutato.
    #[test]
    fn a_command_outside_the_list_produces_no_output() {
        for command in ["echo ciao", "cat file.txt", "git status"] {
            assert_eq!(
                process_with(&bash_payload(command), "/bin/ch"),
                None,
                "{command}"
            );
        }
    }

    /// Un comando mandato in fondo dal campo dello strumento non si avvolge:
    /// l'uscita non tornerebbe mai al filtro.
    #[test]
    fn a_background_run_is_left_alone() {
        let mut payload = bash_payload("cargo test");
        payload["tool_input"]["run_in_background"] = Value::Bool(true);
        assert_eq!(process_with(&payload, "/bin/ch"), None);
    }

    /// La valvola spegne tutto senza toccare `settings.json`.
    #[test]
    fn the_valve_silences_the_hook() {
        std::env::set_var("WRAP_BASH", "off");
        let outcome = process_with(&bash_payload("cargo test"), "/bin/ch");
        std::env::remove_var("WRAP_BASH");
        assert_eq!(outcome, None);
    }

    /// Uno strumento diverso da Bash non è affar suo.
    #[test]
    fn another_tool_is_ignored() {
        let payload = serde_json::json!({
            "tool_name": "Read",
            "tool_input": { "file_path": "/x" },
        });
        assert_eq!(process_with(&payload, "/bin/ch"), None);
    }

    /// L'invariante che conta di più: su qualunque stdin rotto il gancio esce 0
    /// e non stampa niente — mai un diniego, mai un panico.
    #[test]
    fn a_broken_stdin_never_denies() {
        struct Broken;
        impl Read for Broken {
            fn read(&mut self, _: &mut [u8]) -> std::io::Result<usize> {
                Err(std::io::Error::other("stdin chiuso"))
            }
        }
        let mut out = Vec::new();
        assert_eq!(run_from(&mut Broken, &mut out, "/bin/ch"), 0);
        assert!(out.is_empty());
        for raw in [
            "",
            "non è json",
            "[1,2]",
            r#"{"tool_name":"Bash"}"#,
            r#"{"tool_name":"Bash","tool_input":"non un oggetto"}"#,
            r#"{"tool_name":"Bash","tool_input":{"command":7}}"#,
        ] {
            let mut out = Vec::new();
            assert_eq!(
                run_from(&mut raw.as_bytes(), &mut out, "/bin/ch"),
                0,
                "{raw:?}"
            );
            assert!(out.is_empty(), "{raw:?} non doveva stampare: {out:?}");
        }
        // Il caso sano, dallo stesso ingresso: stampa la riscrittura e basta.
        let ok = bash_payload("cargo test -p guards").to_string();
        let mut out = Vec::new();
        assert_eq!(run_from(&mut ok.as_bytes(), &mut out, "/bin/ch"), 0);
        let printed: Value = serde_json::from_slice(&out).expect("una riga JSON");
        assert!(printed["hookSpecificOutput"]["updatedInput"]["command"]
            .as_str()
            .unwrap()
            .contains("cargo test -p guards"));
    }
}
