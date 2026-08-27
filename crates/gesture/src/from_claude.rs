//! Da `hook_io::HookInput` (l'evento di Claude Code) a `Gesture`.
//!
//! COSTO ZERO, SENZA PERDITA. Ogni campo di `HookInput` ha una casa qui
//! dentro — nessuno si scarta, nessuno si inventa — e la conversione è una
//! copia di campi già deserializzati, non un secondo giro di parsing: il
//! cliente che già funziona (`claude-hooks`, dopo `hook_io::read_input()`)
//! può chiamarla senza pagare altro che la copia dei campi che userebbe
//! comunque.

use crate::{Gesture, Moment, Source};

/// Converte l'ingresso già tipizzato di Claude Code in un gesto.
pub fn from_claude(input: &hook_io::HookInput) -> Gesture {
    Gesture {
        moment: input
            .hook_event_name
            .as_deref()
            .map(Moment::from_wire_name)
            .unwrap_or(Moment::Other(String::new())),
        tool: input.tool_name.clone(),
        tool_input: input.tool_input.clone(),
        session_id: input.session_id.clone(),
        cwd: input.cwd.clone(),
        agent_id: input.agent_id.clone(),
        source: Source::ClaudeCode,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Lo stesso comando Bash che `HookInput::bash_command()` leggerebbe
    /// direttamente si legge identico da `Gesture::bash_command()` dopo la
    /// traduzione: è la prova che non si perde niente per strada.
    #[test]
    fn a_pre_tool_use_bash_event_survives_the_crossing_untouched() {
        let raw = r#"{"hook_event_name":"PreToolUse","tool_name":"Bash",
            "tool_input":{"command":"cd /repo && git status"},
            "cwd":"/Users/theo/orca/general","session_id":"abc123"}"#;
        let input: hook_io::HookInput = serde_json::from_str(raw).unwrap();

        assert_eq!(input.bash_command(), "cd /repo && git status");

        let gesture = from_claude(&input);
        assert_eq!(gesture.moment, Moment::BeforeExecution);
        assert_eq!(gesture.source, Source::ClaudeCode);
        assert!(gesture.is_tool("Bash"));
        // La prova che conta: lo stesso identico comando, letto con lo
        // stesso identico accesso al campo `command` di `tool_input`.
        assert_eq!(gesture.bash_command(), input.bash_command());
        assert_eq!(gesture.cwd.as_deref(), Some("/Users/theo/orca/general"));
        assert_eq!(gesture.session_id.as_deref(), Some("abc123"));
    }

    /// Un campo assente in `HookInput` resta assente in `Gesture`: la
    /// traduzione non inventa dati che non c'erano.
    #[test]
    fn a_missing_field_stays_missing() {
        let input: hook_io::HookInput = serde_json::from_str(r#"{"tool_name":"Read"}"#).unwrap();
        let gesture = from_claude(&input);
        assert_eq!(gesture.cwd, None);
        assert_eq!(gesture.session_id, None);
        assert_eq!(gesture.agent_id, None);
        assert_eq!(gesture.moment, Moment::Other(String::new()));
    }
}
