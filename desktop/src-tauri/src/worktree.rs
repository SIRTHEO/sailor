//! The trees this repository is checked out into, for the window.
//!
//! The knowledge is `crates/workspace`, the same one `sailor worktree` reads:
//! two copies would answer differently about which branch a tree is on, and
//! `remove` acts on that answer.

use std::path::{Path, PathBuf};
use workspace::OpenTrees;

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

fn seen(trees: Vec<workspace::Worktree>, here: &Path) -> Vec<Tree> {
    trees
        .into_iter()
        .map(|tree| Tree {
            name: tree.name().to_owned(),
            current: Path::new(&tree.path) == here,
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

/// The register of the trees Sailor cut, where the command line keeps it: two
/// registers would each hold half the trees, and the flow that closes finished
/// work would refuse whichever half it did not read.
fn register() -> Result<ledger::Ledger, String> {
    let directory = ui::gather::default_ledger_dir();
    ledger::Ledger::open(&directory).map_err(|why| format!("{}: {why}", directory.display()))
}

/// Cutting through the door that writes the tree down, so the window answers
/// for what it cuts. The register is handed in, so this is provable without
/// the store of the machine the window is running on.
fn cut(
    repo: &Path,
    branch: &str,
    name: Option<&str>,
    register: &dyn OpenTrees,
) -> Result<String, String> {
    workspace::create(repo, branch, name, register)
        .map(|path| path.to_string_lossy().into_owned())
}

#[tauri::command]
pub(crate) fn worktree_create(branch: String, name: Option<String>) -> Result<String, String> {
    cut(&repo()?, &branch, name.as_deref(), &register()?)
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
        let seen = seen(
            vec![
                tree("/repo/main", "sorgenti"),
                tree("/repo-worktrees/x", "work/x"),
            ],
            &here,
        );
        assert!(seen[0].current);
        assert!(!seen[1].current);
    }

    /// **THE WINDOW CUT TREES NOBODY COULD CLOSE AGAIN.** Its create wrote no
    /// row, so the flow that closes finished work refused the tree it had cut
    /// and the branch behind it stalled. The window's crate is outside the
    /// workspace, so no gate of the battery compiles this: only the window's
    /// own suite can hold its door.
    #[test]
    fn the_window_writes_down_the_tree_it_cuts() {
        let scratch = std::env::temp_dir().join(format!(
            "sailor-window-cuts-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&scratch);
        let repo = a_repository_in(&scratch);
        let register = ledger::Ledger::open(scratch.join("store")).expect("a register");

        let cut_at = cut(&repo, "work/dalla-finestra", None, &register).expect("the tree is cut");
        let rows = workspace::OpenTrees::trees_left_open(&register).expect("the rows");
        let _ = std::fs::remove_dir_all(&scratch);

        assert!(
            rows.iter().any(|row| row.path == cut_at),
            "the window cut {cut_at} and wrote down {rows:?}"
        );
    }

    /// A repository of this test's own, with the trunk it declares: nothing of
    /// the machine the window happens to be running on.
    fn a_repository_in(scratch: &Path) -> PathBuf {
        let repo = scratch.join("project");
        std::fs::create_dir_all(&repo).expect("the repository");
        for args in [
            vec!["init", "-q"],
            vec!["config", "user.email", "prove@example"],
            vec!["config", "user.name", "prove"],
        ] {
            run_git(&repo, &args);
        }
        std::fs::write(repo.join("README"), "a tree to cut from\n").expect("a file");
        run_git(&repo, &["add", "README"]);
        run_git(&repo, &["commit", "-q", "-m", "the first"]);
        run_git(&repo, &["branch", "-M", "tronco"]);
        run_git(&repo, &["config", workspace::branches::TRUNK_KEY, "tronco"]);
        repo
    }

    /// Only the repository's own settings: nothing of the account running this.
    fn run_git(at: &Path, args: &[&str]) {
        let done = std::process::Command::new("git")
            .arg("-C")
            .arg(at)
            .args(args)
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .output()
            .expect("git runs");
        assert!(
            done.status.success(),
            "git {args:?}: {}",
            String::from_utf8_lossy(&done.stderr)
        );
    }

    #[test]
    fn a_tree_keeps_the_name_a_person_calls_it_by() {
        let seen = seen(
            vec![tree("/repo-worktrees/the-thing", "work/the-thing")],
            &PathBuf::from("/repo"),
        );
        assert_eq!(seen[0].name, "the-thing");
        assert_eq!(seen[0].branch.as_deref(), Some("work/the-thing"));
    }
}
