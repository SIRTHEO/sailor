//! Il protocollo dei ganci di Claude Code, in un posto solo.
//!
//! Ogni gancio riceve un JSON su stdin e comunica la decisione con il codice di
//! uscita: 0 lascia passare, 2 blocca e manda stderr al modello. Finora quel
//! contratto era ricopiato in 29 script Python, con esiti diversi in caso di
//! guasto — e il 16/08/2026 un gancio che usciva in errore su `PreToolUse` ha
//! rifiutato ogni strumento in tutte le sessioni della macchina.
//!
//! Qui la scelta è dichiarata una volta per tutte: **fail-open, ma non muto.**
//! Un difetto interno lascia passare il comando e lascia una riga nel registro.
//! Un freno che si rompe non deve fermare il lavoro; un freno che si rompe in
//! silenzio è peggio, perché è indistinguibile da uno contento.

pub mod journal;
pub mod local_time;
pub mod observations;
pub mod python_json;

use serde::Deserialize;
use std::io::Read;

/// L'ingresso comune a tutti gli eventi. I campi assenti restano `None`: gli
/// eventi hanno forme diverse e un gancio legge solo ciò che lo riguarda.
#[derive(Debug, Deserialize, Default)]
pub struct HookInput {
    #[serde(default)]
    pub session_id: Option<String>,
    #[serde(default)]
    pub cwd: Option<String>,
    #[serde(default)]
    pub hook_event_name: Option<String>,
    #[serde(default)]
    pub tool_name: Option<String>,
    #[serde(default)]
    pub tool_input: Option<serde_json::Value>,
    /// Il file della conversazione. Arriva anche su PostToolUse, non solo su
    /// Stop: è la fonte con cui si misura quanto è piena la sessione.
    #[serde(default)]
    pub transcript_path: Option<String>,
}

impl HookInput {
    /// Il comando di una chiamata `Bash`, o stringa vuota se non è quella.
    pub fn bash_command(&self) -> &str {
        self.tool_input
            .as_ref()
            .and_then(|v| v.get("command"))
            .and_then(|v| v.as_str())
            .unwrap_or("")
    }

    pub fn is_tool(&self, name: &str) -> bool {
        self.tool_name.as_deref() == Some(name)
    }
}

/// Cosa il gancio ha deciso. `Warn` esiste perché un divieto senza una
/// sostituzione ovvia peggiora le abitudini invece di correggerle: si blocca
/// dove la riscrittura è meccanica, si avvisa dove dipende dal caso.
///
/// `Deny` non è un `Block` più educato: è **un altro canale**. Il blocco esce
/// con codice 2 e scrive su stderr; il diniego esce con codice 0 e scrive su
/// stdout un JSON che l'harness interpreta come rifiuto del permesso. I ganci
/// Python usano l'uno o l'altro, e scambiarli cambia ciò che l'harness fa.
#[derive(Debug, PartialEq, Eq)]
pub enum Decision {
    Pass,
    Warn(String),
    Block(String),
    Deny(String),
}

impl Decision {
    pub fn exit_code(&self) -> i32 {
        match self {
            Decision::Block(_) => 2,
            _ => 0,
        }
    }
}

/// Legge stdin e lo interpreta. Un JSON illeggibile non è un errore da
/// segnalare: è un gancio invocato fuori contesto, e si lascia passare.
pub fn read_input() -> Option<HookInput> {
    let mut buf = String::new();
    std::io::stdin().read_to_string(&mut buf).ok()?;
    serde_json::from_str(&buf).ok()
}

/// Scrive la decisione dove il modello la legge e restituisce il codice di
/// uscita. Il prefisso col nome del gancio non è cosmetico: con 45 ganci
/// registrati, un messaggio anonimo non dice a chi chiedere conto.
pub fn emit(hook: &str, decision: &Decision) -> i32 {
    match decision {
        Decision::Pass => {}
        Decision::Warn(m) => eprintln!("avviso ({hook}): {m}"),
        Decision::Block(m) => eprintln!("BLOCCATO ({hook}): {m}"),
        Decision::Deny(m) => println!("{}", deny_payload(m)),
    }
    decision.exit_code()
}

/// Il JSON che l'harness legge come «permesso negato». La forma è quella che
/// scrivono i ganci Python, campo per campo: cambiarla non fa fallire niente,
/// fa **passare** il comando in silenzio.
///
/// Serializzato come `json.dumps`, non come `serde_json::to_string`: separatori
/// con lo spazio e non-ASCII in `\uXXXX`. L'harness fa il parse e non se ne
/// accorgerebbe, ma questa riga finisce anche nelle trascrizioni, che si
/// confrontano a occhio e con `grep` — e su un messaggio pieno di accenti e
/// trattini lunghi le due forme non si somigliano affatto.
fn deny_payload(reason: &str) -> String {
    python_json::dumps(&serde_json::json!({
        "hookSpecificOutput": {
            "hookEventName": "PreToolUse",
            "permissionDecision": "deny",
            "permissionDecisionReason": reason,
        }
    }))
}

/// La valvola di un gancio, letta dall'ambiente.
///
/// Regola che vale per tutte: **una valvola può solo ammorbidire.** Nessun
/// valore trasforma un avviso in un blocco, altrimenti una variabile
/// d'ambiente potrebbe indurire il sistema senza che nessuno lo abbia deciso.
#[derive(Debug, PartialEq, Eq, Clone, Copy)]
pub enum Mode {
    Enforce,
    WarnOnly,
    Off,
}

impl Mode {
    pub fn from_env(var: &str) -> Mode {
        match std::env::var(var)
            .unwrap_or_default()
            .to_lowercase()
            .as_str()
        {
            "off" => Mode::Off,
            // `avvisa` è il nome storico usato dagli script Python: resta
            // valido perché sta già scritto nelle regole e nelle automazioni.
            "avvisa" | "warn" => Mode::WarnOnly,
            _ => Mode::Enforce,
        }
    }

    /// Applica la valvola a una decisione già presa.
    pub fn soften(self, d: Decision) -> Decision {
        match (self, d) {
            (Mode::Off, _) => Decision::Pass,
            (Mode::WarnOnly, Decision::Block(m)) => Decision::Warn(m),
            (_, d) => d,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_block_exits_two_and_a_pass_exits_zero() {
        assert_eq!(Decision::Block("x".into()).exit_code(), 2);
        assert_eq!(Decision::Warn("x".into()).exit_code(), 0);
        assert_eq!(Decision::Pass.exit_code(), 0);
    }

    #[test]
    fn a_valve_can_only_soften_never_harden() {
        assert_eq!(
            Mode::WarnOnly.soften(Decision::Block("m".into())),
            Decision::Warn("m".into())
        );
        // il caso che conta: un avviso non diventa mai un blocco
        assert_eq!(
            Mode::Enforce.soften(Decision::Warn("m".into())),
            Decision::Warn("m".into())
        );
        assert_eq!(
            Mode::Off.soften(Decision::Block("m".into())),
            Decision::Pass
        );
    }

    #[test]
    fn a_bash_command_is_read_from_the_tool_input() {
        let input: HookInput =
            serde_json::from_str(r#"{"tool_name":"Bash","tool_input":{"command":"git status"}}"#)
                .unwrap();
        assert!(input.is_tool("Bash"));
        assert_eq!(input.bash_command(), "git status");
    }

    #[test]
    fn a_missing_tool_input_is_an_empty_command_not_a_crash() {
        let input: HookInput = serde_json::from_str(r#"{"tool_name":"Read"}"#).unwrap();
        assert_eq!(input.bash_command(), "");
        assert!(!input.is_tool("Bash"));
    }
}
