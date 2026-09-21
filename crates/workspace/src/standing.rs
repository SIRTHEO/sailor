//! **ONE DECISION, THREE CALLERS.** The sweep asked who was in a tree one way,
//! the named close did not ask, and a delivery flow asked in shell. Fault 267.

use std::path::{Path, PathBuf};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WhoIsIn {
    Nobody,
    ATerminal(PathBuf),
    AProcess(u32),
}

pub fn who_is_in(at: &Path, terminals: &[PathBuf], processes: &[(u32, PathBuf)]) -> WhoIsIn {
    let tree = canonical(at);
    if let Some(taken) = terminals.iter().find(|taken| within(taken, &tree)) {
        return WhoIsIn::ATerminal(taken.clone());
    }
    match processes.iter().find(|(_, cwd)| within(cwd, &tree)) {
        Some((pid, _)) => WhoIsIn::AProcess(*pid),
        None => WhoIsIn::Nobody,
    }
}

fn within(somewhere: &Path, tree: &Path) -> bool {
    canonical(somewhere).starts_with(tree)
}

/// A tree just taken down no longer resolves, so its nearest ancestor that
/// still exists is resolved and the rest joined back on.
pub fn canonical(at: &Path) -> PathBuf {
    if let Ok(real) = at.canonicalize() {
        return real;
    }
    match (at.parent(), at.file_name()) {
        (Some(parent), Some(name)) => canonical(parent).join(name),
        _ => at.to_path_buf(),
    }
}
