//! I freni: la logica di ciò che si può e non si può fare, senza l'involucro.
//!
//! Ogni modulo qui dentro espone una `judge()` pura — dal comando alla
//! decisione, senza toccare stdin, stderr o l'ambiente. È quello che rende
//! possibile il confronto uno-a-uno con lo script Python che sostituisce: si
//! prova la decisione, non il processo.

pub mod cd_guard;
pub mod code_language;
pub mod comment_refs;
pub mod duplication;
pub mod handoff;
pub mod hooks_off;
pub mod language;
pub mod linear_readonly;
pub mod live_rules;
pub mod pr_merge_admin;
pub mod pr_title;
pub mod shell;
pub mod successor;
pub mod socraticode_gate;
pub mod handoff_required;
pub mod handoff_on_stop;
pub mod restart;
