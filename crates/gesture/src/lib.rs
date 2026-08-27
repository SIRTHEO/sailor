//! Il vocabolario comune del gesto: cosa succede, a prescindere da chi lo
//! compie.
//!
//! Nasce spogliando `hook_io::HookInput` (crates/hook-io, «il protocollo dei
//! ganci di Claude Code») dei nomi propri di quella CLI: stesso schema,
//! stessi campi, un nome che non promette che arrivi solo da lì. Il motivo è
//! misurato, non estetico — Codex manda un JSON con GLI STESSI NOMI DI CAMPO
//! (vedi `from_codex`), e una regola in `guards` che giudica un comando non
//! ha bisogno di sapere chi lo ha lanciato.
//!
//! RICERCA DI RIUSO PRIMA DI SCRIVERE (27/08/2026): nessun tipo equivalente
//! esiste già nella configurazione — `codebase_search` su «tipo comune per un
//! evento di gancio indipendente dalla CLI» e su «traduttore da evento Codex a
//! formato comune» non ha trovato altro che questo stesso genere di problema
//! discusso in prosa, mai un tipo scritto.

pub mod from_claude;
pub mod from_codex;

use serde::{Deserialize, Serialize};

/// In quale punto del ciclo di vita arriva questo gesto.
///
/// Otto varianti nominate perché Claude Code e Codex condividono proprio
/// questi otto punti d'aggancio (misurato il 26/08/2026, vedi
/// `state/plancia/segnalazioni/2026-08-26-i-gate-girano-su-codex-ma-non-negano.md`):
/// `PreToolUse`, `PostToolUse`, `SessionStart`, `UserPromptSubmit`,
/// `PermissionRequest`, `Stop`, `SubagentStart`, `SubagentStop`. `PreCompact`
/// è solo di Claude Code oggi, ma non costa tenerlo. Un nome non ancora
/// catalogato finisce in `Other` invece di sparire: un momento nuovo deve
/// restare visibile a chi lo cerca, non azzerarsi in silenzio.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum Moment {
    BeforeExecution,
    AfterExecution,
    SessionStart,
    UserPrompt,
    PermissionRequest,
    Stop,
    SubagentStart,
    SubagentStop,
    PreCompact,
    Other(String),
}

impl Moment {
    /// Il nome che arriva sul filo in `hook_event_name`, per entrambe le CLI.
    pub fn from_wire_name(name: &str) -> Moment {
        match name {
            "PreToolUse" => Moment::BeforeExecution,
            "PostToolUse" => Moment::AfterExecution,
            "SessionStart" => Moment::SessionStart,
            "UserPromptSubmit" => Moment::UserPrompt,
            "PermissionRequest" => Moment::PermissionRequest,
            "Stop" => Moment::Stop,
            "SubagentStart" => Moment::SubagentStart,
            "SubagentStop" => Moment::SubagentStop,
            "PreCompact" => Moment::PreCompact,
            other => Moment::Other(other.to_string()),
        }
    }
}

/// Chi ha compiuto il gesto.
///
/// Non entra MAI nel giudizio di una regola — `guards` giudica l'azione, non
/// l'attore, ed è proprio questo a rendere una regola riusabile fra CLI
/// diverse. Serve solo a chi misura o scrive un rapporto e vuole dividere il
/// traffico per fonte.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Source {
    ClaudeCode,
    Codex,
}

/// Un gesto: quale strumento, con quali argomenti, in quale sessione, in
/// quale momento del ciclo — indipendente da chi lo ha compiuto.
///
/// Ricalca `hook_io::HookInput` campo per campo (vedi `from_claude`): questo
/// tipo non aggiunge e non toglie informazione al payload di Claude Code, lo
/// spoglia solo del nome proprio della CLI.
#[derive(Debug, Clone, PartialEq)]
pub struct Gesture {
    pub moment: Moment,
    pub tool: Option<String>,
    pub tool_input: Option<serde_json::Value>,
    pub session_id: Option<String>,
    pub cwd: Option<String>,
    /// Presente solo quando il gesto arriva da dentro un subagent — vedi
    /// `hook_io::agent_id_says_subagent`, la stessa regola vale qui.
    pub agent_id: Option<String>,
    pub source: Source,
}

impl Gesture {
    /// Il comando di una chiamata Bash, o stringa vuota se non è quella.
    ///
    /// STESSA FORMA DI `hook_io::HookInput::bash_command`: una regola come
    /// `guards::cd_guard::judge`, che prende una `&str`, non deve sapere che
    /// è cambiato il chiamante — è il punto di tutto questo crate.
    pub fn bash_command(&self) -> &str {
        self.tool_input
            .as_ref()
            .and_then(|v| v.get("command"))
            .and_then(|v| v.as_str())
            .unwrap_or("")
    }

    pub fn is_tool(&self, name: &str) -> bool {
        self.tool.as_deref() == Some(name)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wire_names_shared_by_both_clis_map_to_the_same_moment() {
        assert_eq!(Moment::from_wire_name("PreToolUse"), Moment::BeforeExecution);
        assert_eq!(Moment::from_wire_name("PostToolUse"), Moment::AfterExecution);
        assert_eq!(Moment::from_wire_name("SessionStart"), Moment::SessionStart);
        assert_eq!(Moment::from_wire_name("UserPromptSubmit"), Moment::UserPrompt);
    }

    #[test]
    fn an_uncatalogued_name_is_kept_not_dropped() {
        assert_eq!(
            Moment::from_wire_name("SomeFutureEvent"),
            Moment::Other("SomeFutureEvent".to_string())
        );
    }

    #[test]
    fn bash_command_reads_the_same_field_hook_io_reads() {
        let g = Gesture {
            moment: Moment::BeforeExecution,
            tool: Some("Bash".to_string()),
            tool_input: Some(serde_json::json!({"command": "git status"})),
            session_id: None,
            cwd: None,
            agent_id: None,
            source: Source::ClaudeCode,
        };
        assert!(g.is_tool("Bash"));
        assert_eq!(g.bash_command(), "git status");
    }
}
