//! `sailor worktree`: the trees a repository is checked out into.
//!
//! Every piece of work here starts by cutting a tree for a branch and ends by
//! taking it down. Until now that was `git` typed by hand, so nothing Sailor
//! records knew which tree a run happened in — and the window, where the work
//! is meant to move, had no idea trees existed at all.

use crate::Form;
use ledger::Ledger;
use std::path::{Path, PathBuf};
use workspace::branches::against_the_convention;
use workspace::{
    branch_names, close_if_the_trunk_holds_it, create, list, remove, root, run_and_step_of, Closing,
    OpenTree, OpenTrees, Swept, Worktree,
};

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
    Form {
        form: "sailor worktree open",
        says_key: "cli.worktree.form.open",
    },
    Form {
        form: "sailor worktree close <name>",
        says_key: "cli.worktree.form.close",
    },
    Form {
        form: "sailor worktree close --merged",
        says_key: "cli.worktree.form.close_merged",
    },
];

/// The word that turns one close into the sweep of everything closable.
const MERGED: &str = "--merged";

const AN_HOUR: i64 = 3_600;

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
        [command] if command == "open" => {
            let store = a_store()?;
            let held = store.trees_left_open()?;
            Ok(render_open(&held, now(), ledger::pid_is_alive))
        }
        [command, word] if command == "close" && word == MERGED => sweep(&repo, &a_store()?),
        [command, word] if command == "close" && word.starts_with("--") => Err(catalogue::say(
            "cli.unknown_option",
            &[("option", word)],
        )),
        [command, name] if command == "close" => close_one(&repo, name, &a_store()?),
        _ => Err(crate::forms_as_lines(USAGE).join("\n")),
    }
}

fn a_store() -> Result<Ledger, String> {
    let directory = ui::gather::default_ledger_dir();
    Ledger::open(&directory)
        .map_err(|error| format!("{}: {error}", directory.display()))
}

fn now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |since| since.as_secs() as i64)
}

/// The trees Sailor wrote down and never took back off the page.
///
/// The opener's pid is asked whether it is still there, because that is what
/// separates a tree somebody is working in from disk nobody will ever claim.
pub fn render_open(held: &[OpenTree], now: i64, alive: impl Fn(u32) -> bool) -> String {
    if held.is_empty() {
        return catalogue::say("cli.worktree.none_open", &[]);
    }
    held.iter()
        .map(|tree| {
            let state = if alive(tree.opened_by_pid) {
                "cli.worktree.opener_alive"
            } else {
                "cli.worktree.opener_gone"
            };
            catalogue::say(
                "cli.worktree.open_row",
                &[
                    ("tree", &tree.path),
                    ("hours", &((now - tree.opened_at).max(0) / AN_HOUR).to_string()),
                    ("step", &tree.step),
                    ("run", &tree.run),
                    ("pid", &tree.opened_by_pid.to_string()),
                    ("state", &catalogue::say(state, &[])),
                ],
            )
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// Closes the one tree named, if the trunk already holds everything in it.
pub fn close_one(repo: &Path, name: &str, store: &dyn OpenTrees) -> Result<String, String> {
    let trees = list(repo)?;
    let found = trees
        .iter()
        .find(|tree| tree.name() == name)
        .ok_or_else(|| catalogue::say("cli.worktree.no_tree_by_that_name", &[("name", name)]))?;
    let at = PathBuf::from(&found.path);
    if let Some(refusal) = why_it_is_not_mine_to_close(&trees, &at) {
        return Err(refusal);
    }
    Ok(what_became_of_it(&at, close_if_the_trunk_holds_it(repo, &at, store)))
}

/// Every tree of this repository the trunk already holds. What it does not
/// hold is named and stays: this is the gesture that must never lose work.
pub fn sweep(repo: &Path, store: &dyn OpenTrees) -> Result<String, String> {
    let trees = list(repo)?;
    let mut said: Vec<String> = Vec::new();
    let mut closed = 0usize;
    for tree in &trees {
        let at = PathBuf::from(&tree.path);
        if why_it_is_not_mine_to_close(&trees, &at).is_some() {
            continue;
        }
        let became = close_if_the_trunk_holds_it(repo, &at, store);
        if matches!(became, Swept::Closed(Closing::TakenDown) | Swept::AlreadyGone) {
            closed += 1;
        }
        said.push(what_became_of_it(&at, became));
    }
    said.push(catalogue::say(
        "cli.worktree.swept",
        &[
            ("closed", &closed.to_string()),
            ("kept", &(said.len() - closed).to_string()),
        ],
    ));
    Ok(said.join("\n"))
}

/// The main tree and the one the command is standing in are never taken down:
/// git refuses both, and a refusal read as a fault sends whoever typed this
/// looking for a break that is not there.
fn why_it_is_not_mine_to_close(trees: &[Worktree], at: &Path) -> Option<String> {
    let here = std::env::current_dir().ok().and_then(|from| workspace::tree_around(&from));
    if trees.first().is_some_and(|main| Path::new(&main.path) == at) {
        return Some(catalogue::say("cli.worktree.that_is_the_main_tree", &[]));
    }
    let same = here.is_some_and(|here| at.canonicalize().is_ok_and(|at| at == here));
    same.then(|| catalogue::say("cli.worktree.that_is_where_you_are", &[]))
}

fn what_became_of_it(at: &Path, became: Swept) -> String {
    let tree = at.to_string_lossy().into_owned();
    match became {
        Swept::Closed(Closing::TakenDown) => {
            catalogue::say("cli.worktree.closed", &[("tree", &tree)])
        }
        Swept::Closed(Closing::GitRefused(said)) => catalogue::say(
            "cli.worktree.kept_over_work",
            &[("tree", &tree), ("said", said.trim())],
        ),
        Swept::Closed(Closing::HoldsACommitNobodyElseHas(commit)) => catalogue::say(
            "cli.worktree.kept_over_a_commit",
            &[("tree", &tree), ("commit", &commit)],
        ),
        Swept::NotInTheTrunkYet(carries) => catalogue::say(
            "cli.worktree.kept_out_of_the_trunk",
            &[("tree", &tree), ("carries", &carries)],
        ),
        Swept::AlreadyGone => catalogue::say("cli.worktree.already_gone", &[("tree", &tree)]),
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
