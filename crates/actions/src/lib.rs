//! Actions reusable from any `flow::Graph`: invoking an external engine,
//! running a check under a deadline, and the primitive that imposes that
//! deadline on both. Agnostic to any queue, service or path: the caller passes
//! binary, arguments, environment and paths in the step's typed input, and
//! nothing is wired in here.
//!
//! The two registrable actions (`ExternalEngineAction`, `ShellCheckAction`)
//! speak JSON with `flow::ActionRegistry`; `invoke_external_engine` and
//! `run_shell_check` are plain functions for whoever composes several actions
//! into one step. Both read their input **after** references are resolved, so
//! work decided by one step reaches the next without ever leaving the graph.
//!
//! **A FAILING STEP IS RED**, and the steps depending on it do not start. A
//! step that runs a command on purpose to watch it fail declares its tolerance
//! outcome by outcome with `accept`: leniency is a written decision, strictness
//! is the default.
//!
//! **A STEP NAMES THE TOOL, NOT THE BINARY.** `bin` stays for an arbitrary
//! command, but an engine is asked for by identifier (`tool`) and whoever
//! composes the action registry decides how it resolves — here `toolbox` does,
//! reading its descriptors. A flow writing `"bin": "claude"` runs only where
//! that name is on the caller's path; one writing `"tool": "claude-code"` runs
//! wherever that descriptor finds something, and stops with a useful message
//! where it does not.

pub mod apply;
pub mod budget;
pub mod cooldown;
pub mod dormant_steps;
pub mod draft;
pub mod faults;
pub mod graph_memory;
pub mod handoff;
pub mod history;
pub mod mandate;
pub mod mcp;
pub mod memory;
pub mod notes;
pub mod presence;
pub mod reserve;
pub mod search;
pub mod session_fill;
pub mod store;
pub mod terminals;
pub mod topic_drift;
pub mod unused_actions;

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

/// The pure types a descriptor declares where its numbers live with,
/// re-exported from here.
///
/// **RE-EXPORTED, NOT REDEFINED.** `toolbox` builds a recipe without depending
/// on `models` itself, and a copy of these types on this side of the boundary
/// would be a second definition: twin structs diverge at the first added field.
pub use models::usage::{
    read_declared, read_scalar, read_text, Declared, Pointer, Reading, Reports, Shape,
};

pub use cost::{current_price_list, price_list_from};
pub use engine::ExternalEngineAction;
pub use equipment::{equipment_for, equipment_with_keys, equipment_with_keys_and_disk, Equipment};
pub use probe::{
    judge_dry_run, judge_login_status, probe_dry_run, probe_dry_run_with, probe_login_status,
    DryProbe, DryRun, EngineProbe, LoginProbe, LoginRecipe, LoginVerdict, ProbeVerdict,
    RealDryProbe, DRY_PROBE_TIMEOUT,
};
pub use process::{
    invoke_external_engine, invoke_external_engine_watched, invoke_external_engine_watched_until,
    run_shell_check, run_shell_check_watched, run_with_timeout, run_with_timeout_and_stdin,
    run_with_timeout_and_stdin_watched, run_with_timeout_watched, CheckInvocation, CheckResult,
    EngineInvocation, EngineResult, LiveSink, Pipe, RunOutcome, StepSinks,
};
pub use recipe::{
    command_line, command_line_naming_model, command_line_with, AskRecipe, PromptVia,
    SessionRecipe, ToolResolver, UsageRecipe, SESSION_PLACEHOLDER,
};
pub use shell::ShellCheckAction;
pub use spec::{
    ceiling_declared_in, engines_named_in, models_named_in, private_data_asked_in,
    A_TREE_OF_ITS_OWN, BLIND, TREE,
};

pub(crate) use answer::{check_tolerance, tolerates};
pub(crate) use process::sink_for_step;

/// The name `ExternalEngineAction` registers itself under in a
/// `flow::ActionRegistry`.
pub const EXTERNAL_ENGINE_ACTION: &str = "external_engine";
/// The name `ShellCheckAction` registers itself under.
pub const SHELL_CHECK_ACTION: &str = "shell_check";

/// Registers both actions under their stable names.
///
/// The engine registered here **cannot resolve a tool by identifier**: a step
/// writing `tool` gets an error saying how to repair it. For that capability,
/// register `EXTERNAL_ENGINE_ACTION` with `ExternalEngineAction::resolving_with`
/// after this call — `sailor flow` does, the one place toolbox and it meet.
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

    /// The input as an action receives it **when it really runs**: with
    /// references already resolved.
    ///
    /// **WHY A TEST OF THIS CRATE NEEDS IT.** References are resolved by
    /// `flow::step_input`, once, where the input is composed — the cure for
    /// fault 28, and why this crate's code holds no call to
    /// `resolve_references`. A test invoking `execute` directly skips that step
    /// and would prove the action in a world it never runs in, which is fault
    /// 39. It is no second copy of the rule: it calls the real function, and
    /// proves nothing by itself. That references reach **every** action already
    /// resolved is proved by
    /// `crates/flow/tests/a_reference_reaches_every_action.rs`, which goes
    /// through the executor instead of resolving by hand.
    pub(crate) fn with_references_resolved(input: Value) -> Value {
        flow::reference::resolve_references(&input).expect("the test's references resolve")
    }

    #[test]
    fn the_registry_finds_both_actions_by_their_stable_names() {
        let mut registry = flow::ActionRegistry::default();
        register_default(&mut registry);
        assert!(registry.get(EXTERNAL_ENGINE_ACTION).is_some());
        assert!(registry.get(SHELL_CHECK_ACTION).is_some());
    }
}
