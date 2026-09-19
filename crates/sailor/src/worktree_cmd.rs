//! `sailor worktree`: the trees a repository is checked out into.
//!
//! Every piece of work here starts by cutting a tree for a branch and ends by
//! taking it down. Until now that was `git` typed by hand, so nothing Sailor
//! records knew which tree a run happened in — and the window, where the work
//! is meant to move, had no idea trees existed at all.

use crate::retire_index::{retire, IndexTending, Retired};
use crate::Form;
use ledger::holdings::Whose;
use ledger::Ledger;
use std::path::{Path, PathBuf};
use workspace::branches::against_the_convention;
use workspace::index_identity::{IdentityRule, IndexIdentity};
use workspace::{
    branch_names, close_if_the_trunk_holds_it, create, list, remove, root, run_and_step_of,
    Closing, IdentityLeftBehind, OpenTree, OpenTrees, Swept, Worktree,
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
    Form {
        form: "sailor worktree retire [<identity>]",
        says_key: "cli.worktree.form.retire",
    },
    Form {
        form: "sailor worktree adrift",
        says_key: "cli.worktree.form.adrift",
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
        [command, name] if command == "remove" => remove_one(&repo, name, &IndexTending::of(&repo)),
        [command] if command == "names" => {
            names(&branch_names(&repo)?, &workspace::declared_trunk(&repo)?)
        }
        [command] if command == "adrift" => adrift(
            &workspace::branch_standing(&repo, now())?,
            &workspace::declared_trunk(&repo)?,
        ),
        [command] if command == "retire" => {
            let left = a_store()?.identities_left_behind()?;
            Ok(render_left_behind(&left, now()))
        }
        [command, identity] if command == "retire" => {
            retire_one(identity, &a_store()?, &IndexTending::of)
        }
        [command] if command == "open" => {
            let store = a_store()?;
            let held = store.trees_left_open()?;
            Ok(render_open(&held, now(), still_the_opener))
        }
        [command, word] if command == "close" && word == MERGED => {
            // **NOBODY SWEEPS BLIND**: a store that will not answer is not
            // a machine with nobody in it.
            let Some(occupied) = who_is_standing() else {
                return Err(catalogue::say(
                    "cli.worktree.cannot_ask_who_is_standing",
                    &[],
                ));
            };
            let store = a_store()?;
            let standing = machine::where_processes_stand().map_err(|why| {
                catalogue::say("cli.worktree.cannot_see_processes", &[("why", &why)])
            })?;
            let owner = owner_in(&store);
            let holders = Holders {
                occupied: &occupied,
                standing: &standing,
                owner: &owner,
                now: now(),
            };
            sweep(&repo, &store, &holders, &IdentityRule::from_environment())
        }
        [command, word] if command == "close" && word.starts_with("--") => {
            Err(catalogue::say("cli.unknown_option", &[("option", word)]))
        }
        [command, name] if command == "close" => {
            close_one(&repo, name, &a_store()?, &IndexTending::of(&repo))
        }
        _ => Err(crate::forms_as_lines(USAGE).join("\n")),
    }
}

fn a_store() -> Result<Ledger, String> {
    let directory = ui::gather::default_ledger_dir();
    Ledger::open(&directory).map_err(|error| format!("{}: {error}", directory.display()))
}

fn now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |since| since.as_secs() as i64)
}

/// Whether the process that cut a tree is still the one holding its number.
pub fn still_the_opener(tree: &OpenTree) -> bool {
    ledger::holdings::the_process_that_took_it_is_still_there(
        tree.opened_by_pid,
        tree.opened_by_born_at,
    )
}

/// The trees Sailor wrote down and never took back off the page.
///
/// The opener is asked whether it is still there, because that is what
/// separates a tree somebody is working in from disk nobody will ever claim.
pub fn render_open(held: &[OpenTree], now: i64, alive: impl Fn(&OpenTree) -> bool) -> String {
    if held.is_empty() {
        return catalogue::say("cli.worktree.none_open", &[]);
    }
    held.iter()
        .map(|tree| {
            let state = if alive(tree) {
                "cli.worktree.opener_alive"
            } else {
                "cli.worktree.opener_gone"
            };
            catalogue::say(
                "cli.worktree.open_row",
                &[
                    ("tree", &tree.path),
                    (
                        "hours",
                        &((now - tree.opened_at).max(0) / AN_HOUR).to_string(),
                    ),
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

/// Closes the one tree named, if the trunk already holds everything in it,
/// and retires its index identity once it is down.
pub fn close_one(
    repo: &Path,
    name: &str,
    store: &dyn OpenTrees,
    index: &IndexTending,
) -> Result<String, String> {
    let trees = list(repo)?;
    let found = trees
        .iter()
        .find(|tree| tree.name() == name)
        .ok_or_else(|| catalogue::say("cli.worktree.no_tree_by_that_name", &[("name", name)]))?;
    let at = PathBuf::from(&found.path);
    if let Some(refusal) = why_it_is_not_mine_to_close(&trees, &at) {
        return Err(refusal);
    }
    // Read before the take-down: a pin lives in the tree, and goes with it.
    let identity = index.identity_of(found);
    let became = close_if_the_trunk_holds_it(repo, &at, store);
    let taken_down = matches!(became, Swept::Closed(Closing::TakenDown));
    let mut said = what_became_of_it(&at, became);
    if taken_down {
        said.push('\n');
        said.push_str(&index_after(repo, &at, identity, index));
    }
    Ok(said)
}

/// Takes the one tree named down, work or not as git decides, and retires its
/// index identity once it is down.
pub fn remove_one(repo: &Path, name: &str, index: &IndexTending) -> Result<String, String> {
    let trees = list(repo)?;
    let found = trees
        .iter()
        .find(|tree| tree.name() == name)
        .ok_or_else(|| catalogue::say("cli.worktree.no_tree_by_that_name", &[("name", name)]))?;
    let identity = index.identity_of(found);
    let path = remove(repo, name)?;
    let mut said = catalogue::say("cli.worktree.closed", &[("tree", &path.to_string_lossy())]);
    said.push('\n');
    said.push_str(&index_after(repo, &path, identity, index));
    Ok(said)
}

/// What became of the index once the tree is down, in words: an identity
/// that could not be named or retired is said, never swallowed.
fn index_after(
    repo: &Path,
    at: &Path,
    identity: Result<IndexIdentity, String>,
    index: &IndexTending,
) -> String {
    match identity {
        Err(why) => catalogue::say(
            "cli.worktree.index_identity_unknown",
            &[("tree", &at.to_string_lossy()), ("why", &why)],
        ),
        Ok(identity) => retire(repo, &identity.id, index).render(),
    }
}

/// The operator's gesture over an identity a sweep left behind. Retired or
/// already gone from the index, the row closes; anything else is a refusal
/// with an exit code, so nothing can be built on a retirement that did not happen.
/// **THE ROW'S REPOSITORY IS THE GUARD, NOT WHOEVER TYPES**: an identity is
/// per machine, and typed from elsewhere it would skip the trees that pin it.
pub fn retire_one(
    identity: &str,
    store: &dyn OpenTrees,
    tending_for: &dyn Fn(&Path) -> IndexTending,
) -> Result<String, String> {
    let row = store
        .identities_left_behind()?
        .into_iter()
        .find(|row| row.identity == identity)
        .ok_or_else(|| {
            catalogue::say("cli.worktree.index_not_recorded", &[("identity", identity)])
        })?;
    let repo = Path::new(&row.repo);
    let became = retire(repo, identity, &tending_for(repo));
    match became {
        Retired::Retired { .. } | Retired::NoEntry { .. } => {
            store.identity_retired(identity)?;
            Ok(became.render())
        }
        _ => Err(became.render()),
    }
}

/// The identities the sweeps left behind and nobody has retired.
pub fn render_left_behind(left: &[IdentityLeftBehind], now: i64) -> String {
    if left.is_empty() {
        return catalogue::say("cli.worktree.none_left_behind", &[]);
    }
    left.iter()
        .map(|row| {
            catalogue::say(
                "cli.worktree.left_behind_row",
                &[
                    ("identity", &row.identity),
                    ("tree", &row.tree),
                    ("hours", &((now - row.left_at).max(0) / AN_HOUR).to_string()),
                ],
            )
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// **HISTORY AND GIT CANNOT SEE A PERSON.** A clean tree whose work the trunk
/// holds reads exactly like a finished one, and the sweep took down the tree a
/// live session was working in (fault 167).
pub fn occupied_trees(rows: impl Iterator<Item = (String, bool)>) -> Vec<PathBuf> {
    rows.filter(|(at, open)| *open && !at.is_empty())
        .map(|(at, _)| PathBuf::from(at))
        .collect()
}

/// **A STORE THAT IS NOT THERE IS NOT ONE THAT WILL NOT OPEN**: a machine that
/// never tracked a terminal has nobody standing anywhere, and refusing there
/// would be a sweep that never sweeps.
pub fn who_is_standing() -> Option<Vec<PathBuf>> {
    let Ok(path) = sessions::Sessions::default_path() else {
        return Some(Vec::new());
    };
    if !path.exists() {
        return Some(Vec::new());
    }
    let rows = sessions::Sessions::open(path).ok()?.terminals().ok()?;
    Some(occupied_trees(
        rows.into_iter()
            .map(|row| (row.worktree, row.closed_at.is_none())),
    ))
}

/// Who holds a tree, read from the machine and handed in so the rule is tested
/// without arranging the machine: terminals standing in a tree, every process
/// with its directory, the owner the chain of possession names for a row Sailor
/// wrote down, and the second it is now.
pub struct Holders<'a> {
    pub occupied: &'a [PathBuf],
    pub standing: &'a [(u32, PathBuf)],
    pub owner: &'a dyn Fn(&OpenTree) -> Whose,
    pub now: i64,
}

/// Every tree of this repository the trunk already holds. What it does not
/// hold is named and stays: this is the gesture that must never lose work.
/// `occupied` is handed in and never read in here: a condition read from the
/// machine can only be tested by arranging the machine, and such a test never
/// gets written. [`who_is_standing`] is what the command line hands it.
pub fn sweep(
    repo: &Path,
    store: &dyn OpenTrees,
    holders: &Holders,
    rule: &IdentityRule,
) -> Result<String, String> {
    let occupied = holders.occupied;
    let trees = list(repo)?;
    let rows = store.trees_left_open().unwrap_or_default();
    let mut said: Vec<String> = Vec::new();
    let mut closed = 0usize;
    let mut left_behind: Vec<(PathBuf, IndexIdentity)> = Vec::new();
    for tree in &trees {
        let at = PathBuf::from(&tree.path);
        if why_it_is_not_mine_to_close(&trees, &at).is_some() {
            continue;
        }
        if let Some(line) = why_it_stays(&at, written_down_as(&rows, &at), holders) {
            said.push(line);
            continue;
        }
        let identity = workspace::index_identity::identity_of(&at, tree.branch.as_deref(), rule);
        let became = close_if_the_trunk_holds_it(repo, &at, store);
        if matches!(became, Swept::Closed(Closing::TakenDown)) {
            match identity {
                Ok(identity) => left_behind.push((at.clone(), identity)),
                Err(why) => said.push(catalogue::say(
                    "cli.worktree.index_identity_unknown",
                    &[("tree", &at.to_string_lossy()), ("why", &why)],
                )),
            }
        }
        if matches!(
            became,
            Swept::Closed(Closing::TakenDown) | Swept::AlreadyGone
        ) {
            closed += 1;
        }
        said.push(what_became_of_it(&at, became));
    }
    // **THE REGISTER IS SWEPT TOO, AND NOT ONLY GIT'S LIST.** A row whose tree
    // was taken away behind git's back never appears above, so eleven of them
    // stood for two days waking the flow that exists to clear them (fault 166).
    for row in &rows {
        let at = PathBuf::from(&row.path);
        if trees
            .iter()
            .any(|known| same_place(Path::new(&known.path), &at))
        {
            continue;
        }
        if let Some(line) = held_by_somebody(occupied, &at) {
            said.push(line);
            continue;
        }
        let became = close_if_the_trunk_holds_it(repo, &at, store);
        if matches!(
            became,
            Swept::Closed(Closing::TakenDown) | Swept::AlreadyGone
        ) {
            closed += 1;
        }
        said.push(what_became_of_it(&at, became));
    }
    let kept = said.len() - closed;
    // **A SWEEP DELETES NO INDEX.** It names what it left, for a person.
    said.extend(write_down_what_was_left(repo, store, &left_behind, rule));
    said.push(catalogue::say(
        "cli.worktree.swept",
        &[("closed", &closed.to_string()), ("kept", &kept.to_string())],
    ));
    Ok(said.join("\n"))
}

/// The identities of the trees a sweep took down, minus those a standing tree
/// of the same repository still carries: a pin is shared by design. A git
/// that cannot list says so, per identity, and nothing is written down.
fn write_down_what_was_left(
    repo: &Path,
    store: &dyn OpenTrees,
    left_behind: &[(PathBuf, IndexIdentity)],
    rule: &IdentityRule,
) -> Vec<String> {
    let trees = match list(repo) {
        Ok(trees) => trees,
        Err(why) => {
            return left_behind
                .iter()
                .map(|(at, identity)| {
                    catalogue::say(
                        "cli.worktree.index_could_not_look",
                        &[
                            ("identity", &identity.id),
                            ("tree", &at.to_string_lossy()),
                            ("why", why.trim()),
                        ],
                    )
                })
                .collect()
        }
    };
    let standing: Vec<String> = trees
        .iter()
        .filter_map(|tree| {
            workspace::index_identity::identity_of(
                Path::new(&tree.path),
                tree.branch.as_deref(),
                rule,
            )
            .ok()
            .map(|held| held.id)
        })
        .collect();
    left_behind
        .iter()
        .filter(|(_, identity)| !standing.contains(&identity.id))
        .map(|(at, identity)| {
            let tree = at.to_string_lossy().into_owned();
            let written = store.identity_left_behind(&IdentityLeftBehind {
                identity: identity.id.clone(),
                tree: tree.clone(),
                repo: repo.to_string_lossy().into_owned(),
                left_at: now(),
                left_by_pid: std::process::id(),
            });
            match written {
                Ok(()) => catalogue::say(
                    "cli.worktree.index_left_behind",
                    &[("identity", &identity.id), ("tree", &tree)],
                ),
                Err(why) => catalogue::say(
                    "cli.worktree.index_not_written",
                    &[("identity", &identity.id), ("tree", &tree), ("why", &why)],
                ),
            }
        })
        .collect()
}

/// Why a tree stays whatever history it carries, or `None` when nobody holds it:
/// somebody standing in it, a thing Sailor never took, an age under the hour, and
/// for a row Sailor wrote down the chain of possession. See `ledger::holdings`.
fn why_it_stays(at: &Path, row: Option<&OpenTree>, holders: &Holders) -> Option<String> {
    let tree = at.to_string_lossy().into_owned();
    if let Some(line) = held_by_somebody(holders.occupied, at) {
        return Some(line);
    }
    if let Some(pid) = a_process_in(holders.standing, at) {
        let pid = pid.to_string();
        return Some(catalogue::say(
            "cli.worktree.a_process_is_in_it",
            &[("tree", tree.as_str()), ("pid", pid.as_str())],
        ));
    }
    let Some(row) = row else {
        return Some(catalogue::say(
            "cli.worktree.not_cut_by_sailor",
            &[("tree", tree.as_str())],
        ));
    };
    match cut_at(at) {
        None => {
            return Some(catalogue::say(
                "cli.worktree.cut_at_unknown",
                &[("tree", tree.as_str())],
            ));
        }
        Some(cut) if holders.now - cut < AN_HOUR => {
            let minutes = ((holders.now - cut).max(0) / 60).to_string();
            return Some(catalogue::say(
                "cli.worktree.cut_too_recently",
                &[("tree", tree.as_str()), ("minutes", minutes.as_str())],
            ));
        }
        Some(_) => {}
    }
    let key = match (holders.owner)(row) {
        Whose::Nobody => return None,
        Whose::TheProcessThatTookIt => "cli.worktree.its_opener_is_there",
        Whose::TheRunItWasTakenFor => "cli.worktree.its_run_is_open",
        Whose::KeptOnPurpose => "cli.worktree.kept_on_purpose",
        Whose::Uncertain => "cli.worktree.whose_is_uncertain",
    };
    let pid = row.opened_by_pid.to_string();
    Some(catalogue::say(
        key,
        &[
            ("tree", tree.as_str()),
            ("pid", pid.as_str()),
            ("run", row.run.as_str()),
        ],
    ))
}

/// The chain of possession asked for a tree Sailor wrote down: the process that
/// cut it first, then the run it was cut for.
pub fn owner_in(store: &Ledger) -> impl Fn(&OpenTree) -> Whose + '_ {
    move |tree| {
        let holding = ledger::holdings::Holding {
            kind: "worktree".to_owned(),
            name: tree.path.clone(),
            held_by_pid: tree.opened_by_pid,
            held_by_born_at: tree.opened_by_born_at,
            for_run: (!tree.run.is_empty()).then(|| tree.run.clone()),
            taken_at: tree.opened_at,
            purpose: tree.step.clone(),
        };
        ledger::holdings::whose(&holding, &|run| {
            store
                .run_header(run)
                .map(|header| header.is_none_or(|one| one.ended_at.is_none()))
                .map_err(|error| error.to_string())
        })
    }
}

/// What the beat wakes the sweep on: a tree the sweep would take down, read off
/// the same machine the sweep reads. A reading that fails wakes nothing.
pub fn a_sweep_would_take(store: &Ledger) -> impl Fn(&OpenTree) -> bool + '_ {
    let occupied = who_is_standing();
    let standing = machine::where_processes_stand().ok();
    move |tree| {
        let (Some(occupied), Some(standing)) = (occupied.as_deref(), standing.as_deref()) else {
            return false;
        };
        let owner = owner_in(store);
        let holders = Holders {
            occupied,
            standing,
            owner: &owner,
            now: now(),
        };
        why_it_stays(Path::new(&tree.path), Some(tree), &holders).is_none()
    }
}

/// When `git worktree add` wrote the tree's `.git` file, which is when it was cut.
fn cut_at(at: &Path) -> Option<i64> {
    let written = std::fs::metadata(at.join(".git")).ok()?.modified().ok()?;
    written
        .duration_since(std::time::UNIX_EPOCH)
        .ok()
        .map(|since| since.as_secs() as i64)
}

fn a_process_in(standing: &[(u32, PathBuf)], at: &Path) -> Option<u32> {
    let tree = canonical(at);
    standing
        .iter()
        .find(|(_, cwd)| canonical(cwd).starts_with(&tree))
        .map(|(pid, _)| *pid)
}

fn written_down_as<'a>(rows: &'a [OpenTree], at: &Path) -> Option<&'a OpenTree> {
    rows.iter().find(|row| same_place(Path::new(&row.path), at))
}

/// Git lists a tree by its real path and the register keeps the one it was cut
/// at; on this machine `/var` and `/private/var` are the same directory.
fn same_place(one: &Path, other: &Path) -> bool {
    canonical(one) == canonical(other)
}

/// A tree the sweep has just taken down no longer resolves, so its nearest
/// ancestor that still exists is resolved and the rest joined back on.
fn canonical(at: &Path) -> PathBuf {
    if let Ok(real) = at.canonicalize() {
        return real;
    }
    match (at.parent(), at.file_name()) {
        (Some(parent), Some(name)) => canonical(parent).join(name),
        _ => at.to_path_buf(),
    }
}

/// **A TREE SOMEBODY IS IN IS KEPT AND NAMED**: a session at work is work.
fn held_by_somebody(occupied: &[PathBuf], at: &Path) -> Option<String> {
    let here = at.canonicalize().unwrap_or_else(|_| at.to_path_buf());
    occupied
        .iter()
        .any(|taken| taken.canonicalize().unwrap_or_else(|_| taken.clone()) == here)
        .then(|| {
            catalogue::say(
                "cli.worktree.somebody_is_in_it",
                &[("tree", &at.to_string_lossy())],
            )
        })
}

/// The main tree and the one the command is standing in are never taken down:
/// git refuses both, and a refusal read as a fault sends whoever typed this
/// looking for a break that is not there.
fn why_it_is_not_mine_to_close(trees: &[Worktree], at: &Path) -> Option<String> {
    let here = std::env::current_dir()
        .ok()
        .and_then(|from| workspace::tree_around(&from));
    if trees
        .first()
        .is_some_and(|main| Path::new(&main.path) == at)
    {
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
pub fn names(all: &[String], trunk: &str) -> Result<String, String> {
    let against = against_the_convention(all, trunk);
    if against.is_empty() {
        return Ok(catalogue::say("cli.worktree.names_follow", &[]));
    }
    let count = against.len().to_string();
    let mut lines = vec![catalogue::say(
        "cli.worktree.names_against",
        &[("count", &count), ("trunk", trunk)],
    )];
    lines.extend(against);
    Err(lines.join("\n"))
}

/// The branches nothing is watching any more, and the exit code that says so.
///
/// Nothing is deleted here: a branch holds work, and what happens to work is a
/// person's call. The report is the thing that was missing.
fn adrift(standing: &[workspace::branches::Branch], trunk: &str) -> Result<String, String> {
    let adrift = workspace::branches::adrift(standing, trunk);
    if adrift.is_empty() {
        return Ok(catalogue::say("cli.worktree.adrift_none", &[]));
    }
    let widest = adrift
        .iter()
        .map(|branch| branch.name.len())
        .max()
        .unwrap_or(0);
    let mut lines = vec![catalogue::say(
        "cli.worktree.adrift_some",
        &[
            ("count", &adrift.len().to_string()),
            (
                "hours",
                &workspace::branches::ADRIFT_AFTER_HOURS.to_string(),
            ),
        ],
    )];
    for branch in adrift {
        let days = (branch.idle_hours / 24).to_string();
        lines.push(format!(
            "{:widest$}  {}",
            branch.name,
            catalogue::say("cli.worktree.adrift_for", &[("days", &days)])
        ));
    }
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
