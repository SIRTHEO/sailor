//! `sailor worktree`: the trees a repository is checked out into.
//!
//! Every piece of work here starts by cutting a tree for a branch and ends by
//! taking it down. Until now that was `git` typed by hand, so nothing Sailor
//! records knew which tree a run happened in — and the window, where the work
//! is meant to move, had no idea trees existed at all.

use workspace::{create, list, remove, root, Worktree};

pub const USAGE: &[&str] = &[
    "sailor worktree list",
    "sailor worktree create <branch> [name]",
    "sailor worktree remove <name>",
];

pub fn run(args: &[String]) -> i32 {
    match dispatch(args) {
        Ok(message) => {
            println!("{message}");
            0
        }
        Err(error) => {
            eprintln!("{error}");
            1
        }
    }
}

fn dispatch(args: &[String]) -> Result<String, String> {
    let repo = root()?;
    match args {
        [command] if command == "list" => {
            let trees = list(&repo)?;
            Ok(render(&trees))
        }
        [command, branch] if command == "create" => {
            let path = create(&repo, branch, None)?;
            Ok(format!("{}", path.display()))
        }
        [command, branch, name] if command == "create" => {
            let path = create(&repo, branch, Some(name))?;
            Ok(format!("{}", path.display()))
        }
        [command, name] if command == "remove" => {
            let path = remove(&repo, name)?;
            Ok(format!("taken down: {}", path.display()))
        }
        _ => Err(USAGE.join("\n")),
    }
}

/// One line per tree, with the state a person acts on beside the name.
///
/// The window draws its own; this is the shape a terminal reads, and the two
/// have no reason to be the same thing.
pub fn render(trees: &[Worktree]) -> String {
    if trees.is_empty() {
        return "no worktrees".to_owned();
    }
    let widest = trees
        .iter()
        .map(|tree| tree.name().len())
        .max()
        .unwrap_or(0);
    trees
        .iter()
        .map(|tree| {
            let branch = tree.branch.clone().unwrap_or_else(|| "detached".to_owned());
            let mut line = format!("{:widest$}  {branch}", tree.name());
            if tree.locked {
                line.push_str("  locked");
            }
            if tree.prunable {
                line.push_str("  its directory is gone");
            }
            line
        })
        .collect::<Vec<_>>()
        .join("\n")
}
