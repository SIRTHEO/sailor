//! The evaluation loop: a frozen set of tasks cut from this repository's own
//! fix commits, and the deterministic judge that accepts or rejects a change
//! made against one of them.

pub mod build;
pub mod candidates;
pub mod judge;
pub mod runs;
pub mod task;

#[cfg(test)]
pub(crate) mod fixture;

use std::path::PathBuf;

/// The three steps of the builder. `home` is where the sets are written and
/// read; a step may name another with `home`.
pub fn register_bench(registry: &mut flow::ActionRegistry, home: Option<PathBuf>) {
    registry.register(candidates::BENCH_CANDIDATES_ACTION, candidates::BenchCandidatesAction::new(home.clone()));
    registry.register(build::BENCH_VALIDATE_ACTION, build::BenchValidateAction::new(home.clone()));
    registry.register(build::BENCH_FREEZE_ACTION, build::BenchFreezeAction::new(home));
}
