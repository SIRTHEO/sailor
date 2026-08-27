//! Da un gancio di Gemini CLI (`gemini`, non `agy` — vedi `from_agy`) a
//! `Gesture`.
//!
//! COSA HO TROVATO (27/08/2026), da
//! `@google/gemini-cli/bundle/docs/hooks/reference.md`, installato in questo
//! ambiente (`gemini --version` → 0.57.0). `BeforeTool`/`AfterTool` sono un
//! vero gate: uscita 2, o `stdout` con `{"decision":"deny","reason":…}`,
//! blocca l'esecuzione e il messaggio arriva all'agente — non
//! un'osservazione. Il payload porta `hook_event_name` (valori
//! `"BeforeTool"`/`"AfterTool"`, vocabolario diverso da Claude/Codex — vedi
//! la mappa aggiunta a `Moment::from_wire_name`), `tool_name`
//! (`run_shell_command` per la shell, da `docs/tools/shell.md`),
//! `tool_input.command` — STESSO NOME DI CAMPO di Claude Code e Codex,
//! nessuna rinomina qui serve — `session_id`, `cwd`.
//!
//! IL GATE C'È MA NON È COLLEGATO A UN GIUDIZIO VERO, in questo ambiente:
//! `~/.gemini/settings.json` registra già `BeforeTool`/`AfterTool` verso
//! `/Users/theo/.orca/agent-hooks/gemini-hook.sh`, ma quello script stampa
//! `{}\n` PRIMA ANCORA di leggere lo stdin (letto il file: la `printf` è la
//! prima riga, la lettura del payload viene dopo) — osserva, non giudica
//! mai. Collegare questo traduttore a un divieto vero significa cambiare
//! quel gestore, non aggiungerne uno: fuori da questo crate.

use crate::{Gesture, Moment, Source};
use serde::Deserialize;

/// Solo i campi che `Gesture` porta, stessa scelta di `from_codex`.
#[derive(Debug, Deserialize, Default)]
struct GeminiHookPayload {
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

/// Legge il JSON che Gemini CLI manda su stdin a ogni gancio.
///
/// `None` per un ingresso che non è JSON valido: stessa scelta fail-open di
/// `from_codex_hook_json` e di `hook_io::read_input`.
pub fn from_gemini_hook_json(raw: &str) -> Option<Gesture> {
    let payload: GeminiHookPayload = serde_json::from_str(raw).ok()?;
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
        // Gemini CLI non manda un equivalente di agent_id nel payload
        // documentato: assente, non indovinato.
        agent_id: None,
        source: Source::Gemini,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Forma presa da `docs/hooks/reference.md`: `hook_event_name` nel
    /// vocabolario proprio di Gemini CLI, `tool_input.command` nello stesso
    /// campo di Claude Code e Codex.
    const CAPTURED_SHAPE: &str = r#"{"hook_event_name":"BeforeTool","tool_name":"run_shell_command",
        "tool_input":{"command":"echo prova-tre"},"cwd":"/Users/theo/orca/general",
        "session_id":"sess-gemini-1","transcript_path":"/tmp/t.jsonl","timestamp":"2026-08-27T10:00:00Z"}"#;

    #[test]
    fn the_documented_shape_translates_cleanly() {
        let gesture = from_gemini_hook_json(CAPTURED_SHAPE).expect("JSON valido");
        assert_eq!(gesture.moment, Moment::BeforeExecution);
        assert_eq!(gesture.source, Source::Gemini);
        assert!(gesture.is_tool("run_shell_command"));
        assert_eq!(gesture.bash_command(), "echo prova-tre");
        assert_eq!(gesture.cwd.as_deref(), Some("/Users/theo/orca/general"));
    }

    #[test]
    fn an_invalid_json_body_translates_to_nothing() {
        assert!(from_gemini_hook_json("non è json").is_none());
    }

    /// Stessa prova di `from_codex`: lo stesso gesto da Claude Code e da
    /// Gemini CLI deve produrre gli stessi campi giudicabili, a prescindere
    /// dal vocabolario diverso dell'evento.
    #[test]
    fn the_same_gesture_from_claude_or_gemini_yields_the_same_judgeable_fields() {
        let claude_raw = r#"{"hook_event_name":"PreToolUse","tool_name":"Bash",
            "tool_input":{"command":"cd /repo && git status"},
            "cwd":"/x","session_id":"s1"}"#;
        let claude_input: hook_io::HookInput = serde_json::from_str(claude_raw).unwrap();
        let from_claude = crate::from_claude::from_claude(&claude_input);

        let gemini_raw = r#"{"hook_event_name":"BeforeTool","tool_name":"run_shell_command",
            "tool_input":{"command":"cd /repo && git status"},
            "cwd":"/x","session_id":"s1"}"#;
        let from_gemini = from_gemini_hook_json(gemini_raw).unwrap();

        assert_eq!(from_claude.moment, from_gemini.moment);
        assert_eq!(from_claude.bash_command(), from_gemini.bash_command());
        assert_ne!(from_claude.source, from_gemini.source);
    }
}
