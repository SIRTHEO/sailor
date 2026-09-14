//! The evaluation loop: a frozen set of tasks cut from this repository's own
//! fix commits, and the deterministic judge that accepts or rejects a change
//! made against one of them.

pub mod judge;
pub mod task;
