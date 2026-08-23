//! I freni: la logica di ciò che si può e non si può fare, senza l'involucro.
//!
//! Ogni modulo qui dentro espone una `judge()` pura — dal comando alla
//! decisione, senza toccare stdin, stderr o l'ambiente. È quello che rende
//! possibile il confronto uno-a-uno con lo script Python che sostituisce: si
//! prova la decisione, non il processo.

pub mod ask_routing;
pub mod cd_guard;
pub mod chain;
pub mod code_language;
pub mod comment_refs;
pub mod cost_ledger;
pub mod destructive_commands;
pub mod duplication;
pub mod handoff;
pub mod hooks_off;
pub mod instincts;
pub mod language;
pub mod linear_readonly;
pub mod long_session;
pub mod memory_anchor;
pub mod message_budget;
pub mod link_worktree_rules;
pub mod live_rules;
pub mod phase_router;
pub mod pr_merge_admin;
pub mod pr_title;
pub mod queue_clock;
pub mod shell;
pub mod successor;
pub mod socraticode_gate;
pub mod handoff_required;
pub mod handoff_on_stop;
pub mod handoff_threshold;
pub mod index_freshness;
pub mod repo_tools;
pub mod restart;
pub mod scope_drift;
pub mod session_messages;
pub mod skill_nudge;
pub mod spotlight_marker;
pub mod stale_facts;
pub mod worktree_create;
pub mod worktree_deletes;
