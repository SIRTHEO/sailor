//! Da un gancio di Antigravity (`agy`, non `gemini` — vedi `from_gemini`) a
//! `Gesture`.
//!
//! COSA HO TROVATO (27/08/2026), da
//! `~/.gemini/antigravity-cli/builtin/skills/agy-customizations/docs/hooks.md`
//! installato in questo ambiente (`agy --version` → 1.1.22). `PreToolUse`
//! riceve `{"toolCall":{"name":…,"args":{"CommandLine":…}}}` e la sua uscita
//! `{"decision":"deny", "reason":…}` BLOCCA DAVVERO l'esecuzione — un vero
//! gate, non un'osservazione.
//!
//! DUE DIFFERENZE VERE rispetto a Claude/Codex/Gemini, non appianabili senza
//! perdita:
//! 1. Il comando arriva sotto `args.CommandLine` (maiuscola), non `command`:
//!    qui si rinomina la CHIAVE dentro `tool_input`, non si inventa il
//!    valore — è la stessa idea di `from_claude`, "costo zero, senza
//!    perdita", applicata a un nome di campo diverso.
//! 2. Il payload NON porta `hook_event_name`: quale evento sia lo sa solo chi
//!    ha registrato lo script in quella voce di `hooks.json` (un array per
//!    evento). Lo prova `~/.orca/agent-hooks/antigravity-hook.sh`, già in
//!    servizio in questo ambiente: usa la variabile `ORCA_ANTIGRAVITY_EVENT`
//!    per saperlo, non legge un campo del payload. Per questo `moment` è un
//!    parametro di questa funzione, non un campo estratto dal JSON.
//!
//! NON ANCORA COLLEGATO: `~/.gemini/config/hooks.json`, letto in questo
//! ambiente, registra per Orca solo `PreInvocation`, `PostInvocation`,
//! `Stop`, `PostToolUse` — NESSUN gestore su `PreToolUse`. Il meccanismo del
//! gate è reale e verificato nella documentazione; nessun comando passa oggi
//! da questo traduttore verso un giudizio vero perché nessuno lo chiama
//! ancora da quel punto. Collegarlo è un passo aperto, come per Codex (vedi
//! `from_codex`), non un difetto di questo file.

use crate::{Gesture, Moment, Source};
use serde::Deserialize;

#[derive(Debug, Deserialize, Default)]
struct AgyToolCall {
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    args: Option<serde_json::Value>,
}

/// Solo i campi comuni documentati, più `toolCall`: `stepIdx`,
/// `artifactDirectoryPath`, `modelName` restano fuori come in `from_codex`,
/// nessuna regola di `guards` li chiede.
#[derive(Debug, Deserialize, Default)]
struct AgyHookPayload {
    #[serde(default, rename = "toolCall")]
    tool_call: Option<AgyToolCall>,
    #[serde(default, rename = "conversationId")]
    conversation_id: Option<String>,
}

/// Legge il JSON che `agy` manda su stdin per un gancio `PreToolUse`.
///
/// `moment` arriva da chi chiama perché il payload non lo porta — vedi la
/// premessa del modulo. `None` per un ingresso che non è JSON valido, stessa
/// scelta fail-open degli altri traduttori.
///
/// `cwd` resta sempre `None`: il payload documentato porta `workspacePaths`,
/// un elenco di radici del progetto, non la cartella di lavoro del comando —
/// concetti diversi, copiare il primo elemento sarebbe indovinare.
pub fn from_agy_hook_json(raw: &str, moment: Moment) -> Option<Gesture> {
    let payload: AgyHookPayload = serde_json::from_str(raw).ok()?;
    let tool_call = payload.tool_call.unwrap_or_default();
    let command_line = tool_call
        .args
        .as_ref()
        .and_then(|a| a.get("CommandLine"))
        .and_then(|v| v.as_str());
    Some(Gesture {
        moment,
        tool: tool_call.name,
        // Rinomina la chiave, non il valore: `Gesture::bash_command()` legge
        // sempre `command`, a prescindere da come la fonte lo chiamava.
        tool_input: command_line.map(|c| serde_json::json!({"command": c})),
        session_id: payload.conversation_id,
        cwd: None,
        agent_id: None,
        source: Source::Agy,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Forma presa dal contratto `PreToolUse` documentato.
    const CAPTURED_SHAPE: &str = r#"{"toolCall":{"name":"run_command",
        "args":{"CommandLine":"echo prova-quattro"}},"stepIdx":19,
        "conversationId":"conv-1","workspacePaths":["/Users/theo/orca/general"],
        "modelName":"auto"}"#;

    #[test]
    fn the_documented_shape_translates_cleanly() {
        let gesture = from_agy_hook_json(CAPTURED_SHAPE, Moment::BeforeExecution).expect("JSON valido");
        assert_eq!(gesture.moment, Moment::BeforeExecution);
        assert_eq!(gesture.source, Source::Agy);
        assert!(gesture.is_tool("run_command"));
        assert_eq!(gesture.bash_command(), "echo prova-quattro");
        assert_eq!(gesture.session_id.as_deref(), Some("conv-1"));
    }

    #[test]
    fn an_invalid_json_body_translates_to_nothing() {
        assert!(from_agy_hook_json("non è json", Moment::BeforeExecution).is_none());
    }

    /// Il momento non è nel filo: lo decide chi chiama, e questa prova lo
    /// dimostra passando due valori diversi sullo stesso payload.
    #[test]
    fn the_moment_comes_from_the_caller_not_the_wire() {
        let after = from_agy_hook_json(CAPTURED_SHAPE, Moment::AfterExecution).unwrap();
        assert_eq!(after.moment, Moment::AfterExecution);
        let before = from_agy_hook_json(CAPTURED_SHAPE, Moment::BeforeExecution).unwrap();
        assert_eq!(before.moment, Moment::BeforeExecution);
    }

    /// Stessa prova di `from_codex`: lo stesso comando da Claude Code e da
    /// `agy` deve produrre lo stesso testo giudicabile, anche se la fonte lo
    /// porta sotto una chiave diversa (`CommandLine`, non `command`).
    #[test]
    fn the_same_command_from_claude_or_agy_yields_the_same_judgeable_text() {
        let claude_raw = r#"{"hook_event_name":"PreToolUse","tool_name":"Bash",
            "tool_input":{"command":"cd /repo && git status"},
            "cwd":"/x","session_id":"s1"}"#;
        let claude_input: hook_io::HookInput = serde_json::from_str(claude_raw).unwrap();
        let from_claude = crate::from_claude::from_claude(&claude_input);

        let agy_raw = r#"{"toolCall":{"name":"run_command",
            "args":{"CommandLine":"cd /repo && git status"}},
            "conversationId":"s1"}"#;
        let from_agy = from_agy_hook_json(agy_raw, Moment::BeforeExecution).unwrap();

        assert_eq!(from_claude.bash_command(), from_agy.bash_command());
        assert_ne!(from_claude.source, from_agy.source);
    }
}
