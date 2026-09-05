//! Azioni riusabili da qualunque `flow::Graph`: invocare un motore esterno,
//! eseguire una verifica con un tempo massimo, e la primitiva che impone il
//! limite di durata a entrambe. Agnostiche a qualunque coda, servizio o
//! percorso: chi le usa passa binario, argomenti, ambiente e percorsi
//! nell'ingresso tipato del passo — niente è cablato qui dentro.
//!
//! CONSOLIDA (non duplica) la logica che prima viveva solo in
//! `notte::main::run_with_timeout`: quel file la richiama da qui adesso.
//!
//! Le due azioni registrabili (`ExternalEngineAction`, `ShellCheckAction`)
//! parlano JSON con `flow::ActionRegistry`. Chi compone un passo unico da più
//! azioni (motore poi verifica, come fa `notte`) può anche chiamare
//! direttamente `invoke_external_engine`/`run_shell_check`: sono funzioni
//! semplici, non solo azioni registrate.
//!
//! Tutte e due leggono il proprio ingresso **dopo** che i rinvii sono stati
//! risolti (`reference`): è così che il lavoro deciso da un passo arriva al
//! passo dopo senza uscire dal grafo.
//!
//! **UN PASSO CHE FALLISCE È ROSSO, E NON PER GENTILEZZA DI CHI VIENE DOPO.**
//! Fino al 28/08/2026 un motore uscito in errore lasciava il passo `Went` con
//! dentro un campo `status: exit_error`: la corsa diventava rossa solo se
//! qualcuno, più avanti nel grafo, guardava quel campo. Adesso un esito di
//! fallimento rompe il proprio passo, e i passi che ne dipendono non partono.
//! Chi vuole il contrario lo dichiara nel passo, esito per esito, col campo
//! `accept` — c'è chi esegue un comando apposta per vedere se fallisce. La
//! tolleranza è una decisione scritta; il rigore è il valore predefinito.
//!
//! **UN PASSO NOMINA LO STRUMENTO, NON IL BINARIO.** `bin` resta per un comando
//! qualunque, ma un motore si chiede per identificativo (`tool`) e chi compone
//! il registro delle azioni decide come si risolve — su questa macchina lo fa
//! `toolbox` leggendo i suoi descrittori. Un flusso che scrive `"bin": "claude"`
//! gira solo dove quel nome è nel percorso di chi esegue; uno che scrive
//! `"tool": "claude-code"` gira ovunque quel descrittore trovi qualcosa, e si
//! ferma con un messaggio utile dove non lo trova.

pub mod apply;
pub mod budget;
pub mod cooldown;
pub mod faults;
pub mod handoff;
pub mod history;
pub mod mcp;
pub mod draft;
pub mod memory;
pub mod notes;
pub mod presence;
pub mod search;
pub mod store;
pub mod terminals;

mod answer;
mod candidates;
mod cost;
mod engine;
mod equipment;
mod probe;
mod process;
mod recipe;
mod session;
mod shell;
mod spec;

/// I tipi puri con cui un descrittore dichiara dove stanno i suoi numeri,
/// ri-esportati da qui.
///
/// **PERCHÉ RI-ESPORTATI E NON RIDEFINITI.** `toolbox` deve poter costruire una
/// ricetta senza dipendere a sua volta da `models`, e una copia di questi tipi
/// da questa parte del confine sarebbe una seconda definizione della stessa
/// cosa: due strutture gemelle divergono al primo campo che qualcuno aggiunge a
/// una sola delle due.
pub use models::usage::{
    read_declared, read_scalar, read_text, Declared, Pointer, Reading, Reports, Shape,
};

pub use cost::{current_price_list, price_list_from};
pub use engine::ExternalEngineAction;
pub use equipment::{equipment_for, equipment_with_keys, Equipment};
pub use probe::{
    judge_dry_run, judge_login_status, probe_dry_run, probe_login_status, DryProbe, DryRun,
    EngineProbe, LoginProbe, LoginRecipe, LoginVerdict, ProbeVerdict, RealDryProbe,
    DRY_PROBE_TIMEOUT,
};
pub use process::{
    invoke_external_engine, invoke_external_engine_watched, invoke_external_engine_watched_until,
    run_shell_check,
    run_shell_check_watched, run_with_timeout, run_with_timeout_and_stdin,
    run_with_timeout_and_stdin_watched, run_with_timeout_watched, CheckInvocation, CheckResult,
    EngineInvocation, EngineResult, LiveSink, Pipe, RunOutcome, StepSinks,
};
pub use recipe::{
    command_line, command_line_with, AskRecipe, PromptVia, SessionRecipe, ToolResolver,
    UsageRecipe, SESSION_PLACEHOLDER,
};
pub use shell::ShellCheckAction;
pub use spec::{engines_named_in, private_data_asked_in, A_TREE_OF_ITS_OWN, BLIND, TREE};

pub(crate) use answer::{check_tolerance, tolerates};
pub(crate) use process::sink_for_step;

/// Il nome sotto cui `ExternalEngineAction` si registra in un
/// `flow::ActionRegistry`.
pub const EXTERNAL_ENGINE_ACTION: &str = "external_engine";
/// Il nome sotto cui `ShellCheckAction` si registra.
pub const SHELL_CHECK_ACTION: &str = "shell_check";

/// Registra entrambe le azioni sotto i loro nomi stabili: la scorciatoia per
/// chi vuole entrambe senza scegliere i nomi a mano.
/// Registra entrambe le azioni sotto i loro nomi stabili.
///
/// Il motore registrato qui **non sa risolvere uno strumento per
/// identificativo**: un passo che scrive `tool` riceve un errore che dice come
/// si ripara. Chi vuole quella capacità registra `EXTERNAL_ENGINE_ACTION` con
/// `ExternalEngineAction::resolving_with(...)` dopo questa chiamata — lo fa
/// `sailor flow`, che è l'unico punto dove `toolbox` e le azioni si incontrano.
pub fn register_default(registry: &mut flow::ActionRegistry) {
    registry.register(EXTERNAL_ENGINE_ACTION, ExternalEngineAction::new());
    registry.register(SHELL_CHECK_ACTION, ShellCheckAction::new());
    apply::register_apply_patch(registry);
    mcp::register_mcp(registry);
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::Value;

    /// L'ingresso come lo riceve un'azione **quando gira davvero**: coi rinvii
    /// già sciolti.
    ///
    /// **PERCHÉ UNA PROVA DI QUESTO CRATE NE HA BISOGNO.** Dal 01/09/2026 i
    /// rinvii li scioglie `flow::step_input`, una volta sola dove l'ingresso si
    /// compone — è la cura del guasto 28, e la ragione per cui nel codice di
    /// questo crate non c'è più nessuna chiamata a `resolve_references`. Una
    /// prova che invochi `execute` direttamente salta quel passaggio: senza
    /// questa riga proverebbe l'azione in un mondo in cui non gira mai, che è
    /// il guasto 39.
    ///
    /// **NON È UNA SECONDA COPIA DELLA REGOLA**: chiama la funzione vera. E
    /// non prova niente da sola — ciò che i rinvii arrivino sciolti a **ogni**
    /// azione lo prova `crates/flow/tests/a_reference_reaches_every_action.rs`,
    /// che passa dall'esecutore invece di chiamare la risoluzione a mano.
    pub(crate) fn with_references_resolved(input: Value) -> Value {
        flow::reference::resolve_references(&input).expect("i rinvii della prova si sciolgono")
    }

    #[test]
    fn the_registry_finds_both_actions_by_their_stable_names() {
        let mut registry = flow::ActionRegistry::default();
        register_default(&mut registry);
        assert!(registry.get(EXTERNAL_ENGINE_ACTION).is_some());
        assert!(registry.get(SHELL_CHECK_ACTION).is_some());
    }
}
