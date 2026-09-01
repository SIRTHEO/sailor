//! The trees this repository is checked out into, for the window.
//!
//! The knowledge is `crates/workspace`, the same one `sailor worktree` reads:
//! two copies would answer differently about which branch a tree is on, and
//! `remove` acts on that answer.

use std::path::PathBuf;

/// A tree, plus what the window needs to open a terminal on it.
#[derive(serde::Serialize)]
pub(crate) struct Tree {
    pub name: String,
    pub path: String,
    pub branch: Option<String>,
    pub locked: bool,
    pub prunable: bool,
    /// The one the window is running in, which cannot be taken down.
    pub current: bool,
}

fn repo() -> Result<PathBuf, String> {
    workspace::root()
}

fn seen(trees: Vec<workspace::Worktree>, here: &PathBuf) -> Vec<Tree> {
    trees
        .into_iter()
        .map(|tree| Tree {
            name: tree.name().to_owned(),
            current: PathBuf::from(&tree.path) == *here,
            path: tree.path,
            branch: tree.branch,
            locked: tree.locked,
            prunable: tree.prunable,
        })
        .collect()
}

#[tauri::command]
pub(crate) fn worktree_list() -> Result<Vec<Tree>, String> {
    let here = repo()?;
    Ok(seen(workspace::list(&here)?, &here))
}

#[tauri::command]
pub(crate) fn worktree_create(branch: String, name: Option<String>) -> Result<String, String> {
    let path = workspace::create(&repo()?, &branch, name.as_deref())?;
    Ok(path.to_string_lossy().into_owned())
}

/// Git refuses while a tree holds uncommitted work, and that refusal is kept.
#[tauri::command]
pub(crate) fn worktree_remove(name: String) -> Result<String, String> {
    let path = workspace::remove(&repo()?, &name)?;
    Ok(path.to_string_lossy().into_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tree(path: &str, branch: &str) -> workspace::Worktree {
        workspace::Worktree {
            path: path.to_owned(),
            head: "abc".to_owned(),
            branch: Some(branch.to_owned()),
            locked: false,
            prunable: false,
        }
    }

    /// THE MEASURE THAT COULD HAVE COME OUT DIFFERENTLY. Taking down the tree
    /// the window is running in pulls the floor out from under it: git would
    /// allow it, and the window has to know which one is its own before it
    /// offers the gesture.
    #[test]
    fn the_tree_the_window_runs_in_is_marked_as_its_own() {
        let here = PathBuf::from("/repo/main");
        let seen = seen(vec![tree("/repo/main", "sorgenti"), tree("/repo-worktrees/x", "work/x")], &here);
        assert!(seen[0].current);
        assert!(!seen[1].current);
    }

    #[test]
    fn a_tree_keeps_the_name_a_person_calls_it_by() {
        let seen = seen(vec![tree("/repo-worktrees/the-thing", "work/the-thing")], &PathBuf::from("/repo"));
        assert_eq!(seen[0].name, "the-thing");
        assert_eq!(seen[0].branch.as_deref(), Some("work/the-thing"));
    }
}
