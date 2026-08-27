//! Da un gancio di Codex a `Gesture`.
//!
//! COSA HO TROVATO (26-27/08/2026, vedi
//! `state/plancia/segnalazioni/2026-08-26-i-gate-girano-su-codex-ma-non-negano.md`,
//! letta per intero prima di scrivere questo file). Codex ha gli stessi otto
//! punti d'aggancio di Claude Code (`hooks.json` sotto
//! `codex-runtime-home`, stessa forma `hooks.<Evento>[].hooks[].command`) e
//! manda un JSON con GLI STESSI NOMI DI CAMPO, catturato dal vivo:
//!
//!   {"hook_event_name":"PreToolUse","tool_name":"Bash",
//!    "tool_input":{"command":"echo prova-due"},"cwd":"…",
//!    "session_id":"…","permission_mode":"bypassPermissions",
//!    "model":"gpt-5.6-sol","tool_use_id":"exec-…"}
//!
//! Per questo il traduttore qui sotto è un vero gancio — non un involucro
//! davanti al comando `codex` — perché l'evidenza dice che i ganci esistono
//! e portano il dato giusto. **Ma con una riserva, non taciuta**: nel modo
//! in cui `crates/notte` invoca Codex oggi (`codex exec -s workspace-write`,
//! senza approvazione interattiva, vedi `notte/src/main.rs` intorno alla
//! riga 729), `PermissionRequest` non parte mai e l'uscita 2 di `PreToolUse`
//! non ferma il gesto — provato dal vivo il 26/08, poi ripristinato apposta.
//! Questo file traduce il payload in un `Gesture`; se collegarlo a un
//! divieto che Codex rispetti davvero è un altro passo, ancora aperto nella
//! segnalazione sopra, non di questo crate.

use crate::{Gesture, Moment, Source};
use serde::Deserialize;

/// Solo i campi che `Gesture` porta. `permission_mode`, `model`,
/// `tool_use_id` esistono nel payload reale di Codex ma senza un
/// corrispondente in `Gesture` oggi: restano fuori invece di essere
/// inventati — nessuna regola di `guards` li chiede ancora.
#[derive(Debug, Deserialize, Default)]
struct CodexHookPayload {
    #[serde(default)]
    hook_event_name: Option<String>,
    #[serde(default)]
    tool_name: Option<String>,
    #[serde(default)]
    tool_input: Option<serde_json::Value>,
    #[serde(default)]
    session_id: Option<String>,
    #[serde(default)]
    cwd: Option<String>,
}

/// Legge il JSON che Codex manda su stdin a ogni gancio.
///
/// `None` per un ingresso che non è JSON valido: stessa scelta fail-open,
/// silenziosa sull'esito ma non sull'errore, di `hook_io::read_input` — un
/// ingresso illeggibile è quasi sempre un gancio invocato fuori contesto.
pub fn from_codex_hook_json(raw: &str) -> Option<Gesture> {
    let payload: CodexHookPayload = serde_json::from_str(raw).ok()?;
    Some(Gesture {
        moment: payload
            .hook_event_name
            .as_deref()
            .map(Moment::from_wire_name)
            .unwrap_or(Moment::Other(String::new())),
        tool: payload.tool_name,
        tool_input: payload.tool_input,
        session_id: payload.session_id,
        cwd: payload.cwd,
        // Codex non manda oggi un equivalente di agent_id: assente, non
        // indovinato.
        agent_id: None,
        source: Source::Codex,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Il payload esatto catturato dal vivo il 26/08/2026 (vedi la premessa
    /// del modulo): la prova che questo traduttore lavora su un dato reale,
    /// non su una forma immaginata.
    const CAPTURED_PAYLOAD: &str = r#"{"hook_event_name":"PreToolUse","tool_name":"Bash",
        "tool_input":{"command":"echo prova-due"},"cwd":"/home/someone/orca/general",
        "session_id":"sess-codex-1","permission_mode":"bypassPermissions",
        "model":"gpt-5.6-sol","tool_use_id":"exec-1"}"#;

    #[test]
    fn the_real_captured_payload_translates_cleanly() {
        let gesture = from_codex_hook_json(CAPTURED_PAYLOAD).expect("JSON valido");
        assert_eq!(gesture.moment, Moment::BeforeExecution);
        assert_eq!(gesture.source, Source::Codex);
        assert!(gesture.is_tool("Bash"));
        assert_eq!(gesture.bash_command(), "echo prova-due");
        assert_eq!(gesture.cwd.as_deref(), Some("/home/someone/orca/general"));
    }

    #[test]
    fn an_invalid_json_body_translates_to_nothing() {
        assert!(from_codex_hook_json("non è json").is_none());
    }

    /// Un ingresso Claude Code e uno Codex per LO STESSO gesto producono un
    /// `Gesture` uguale in ogni campo che una regola guarda — cambia solo la
    /// provenienza dichiarata. È la prova che il vocabolario è davvero
    /// comune, non due tipi che si somigliano per caso.
    #[test]
    fn the_same_gesture_from_either_cli_yields_the_same_judgeable_fields() {
        let claude_raw = r#"{"hook_event_name":"PreToolUse","tool_name":"Bash",
            "tool_input":{"command":"cd /repo && git status"},
            "cwd":"/x","session_id":"s1"}"#;
        let claude_input: hook_io::HookInput = serde_json::from_str(claude_raw).unwrap();
        let from_claude = crate::from_claude::from_claude(&claude_input);

        let codex_raw = r#"{"hook_event_name":"PreToolUse","tool_name":"Bash",
            "tool_input":{"command":"cd /repo && git status"},
            "cwd":"/x","session_id":"s1","permission_mode":"bypassPermissions"}"#;
        let from_codex = from_codex_hook_json(codex_raw).unwrap();

        assert_eq!(from_claude.moment, from_codex.moment);
        assert_eq!(from_claude.tool, from_codex.tool);
        assert_eq!(from_claude.bash_command(), from_codex.bash_command());
        // Aggiunti il 27/08/2026 su segnalazione di un revisore indipendente:
        // la prova dichiarava di confrontare «ogni campo che una regola guarda»
        // ma non guardava né la cartella di lavoro né la sessione. Svuotandoli
        // nel traduttore restava verde — e sono i due campi con cui una regola
        // sa dove si sta agendo e per conto di chi.
        assert_eq!(from_claude.cwd, from_codex.cwd);
        assert_eq!(from_claude.session_id, from_codex.session_id);
        assert_eq!(from_codex.cwd.as_deref(), Some("/x"));
        assert_eq!(from_codex.session_id.as_deref(), Some("s1"));
        assert_ne!(from_claude.source, from_codex.source);
    }
}
