//! `sailor worktree`: the trees a repository is checked out into.
//!
//! Every piece of work here starts by cutting a tree for a branch and ends by
//! taking it down. Until now that was `git` typed by hand, so nothing Sailor
//! records knew which tree a run happened in — and the window, where the work
//! is meant to move, had no idea trees existed at all.

use crate::Form;
use workspace::branches::against_the_convention;
use workspace::{branch_names, create, list, remove, root, run_and_step_of, Worktree};

pub const USAGE: &[Form] = &[
    Form {
        form: "sailor worktree list",
        says_key: "",
    },
    Form {
        form: "sailor worktree create <branch> [name]",
        says_key: "",
    },
    Form {
        form: "sailor worktree remove <name>",
        says_key: "",
    },
    Form {
        form: "sailor worktree names",
        says_key: "",
    },
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
        [command] if command == "names" => names(&branch_names(&repo)?),
        _ => Err(crate::forms_as_lines(USAGE).join("\n")),
    }
}

/// The verdict on the branch names, as an error when one breaks the rule.
///
/// An error and not a line of prose: whoever runs this wants an exit code to
/// act on, and a check that says its bad news on standard output at exit zero
/// is a check nothing can be built upon.
fn names(all: &[String]) -> Result<String, String> {
    let against = against_the_convention(all);
    if against.is_empty() {
        return Ok(catalogue::say("cli.worktree.names_follow", &[]));
    }
    let count = against.len().to_string();
    let mut lines = vec![catalogue::say("cli.worktree.names_against", &[("count", &count)])];
    lines.extend(against);
    Err(lines.join("\n"))
}

/// One line per tree, with the state a person acts on beside the name.
///
/// The window draws its own; this is the shape a terminal reads. A tree a
/// step of a run kept is named with its run and step, and counted at the end:
/// kept disk nothing says out loud is fault 89 in a new place.
pub fn render(trees: &[Worktree]) -> String {
    if trees.is_empty() {
        return "no worktrees".to_owned();
    }
    let widest = trees
        .iter()
        .map(|tree| tree.name().len())
        .max()
        .unwrap_or(0);
    let mut kept = 0usize;
    let mut lines: Vec<String> = Vec::new();
    for tree in trees {
        let branch = tree
            .branch
            .clone()
            .unwrap_or_else(|| catalogue::say("cli.worktree.detached", &[]));
        let mut line = format!("{:widest$}  {branch}", tree.name());
        if tree.locked {
            line.push_str("  ");
            line.push_str(&catalogue::say("cli.worktree.locked", &[]));
        }
        if tree.prunable {
            line.push_str("  ");
            line.push_str(&catalogue::say("cli.worktree.directory_gone", &[]));
        }
        if let Some((run, step)) = run_and_step_of(tree) {
            kept += 1;
            line.push_str("  ");
            line.push_str(&catalogue::say(
                "cli.worktree.cut_for",
                &[("run", &run), ("step", &step)],
            ));
        }
        lines.push(line);
    }
    if kept > 0 {
        lines.push(catalogue::say(
            "cli.worktree.kept_from_runs",
            &[("count", &kept.to_string())],
        ));
    }
    lines.join("\n")
}
