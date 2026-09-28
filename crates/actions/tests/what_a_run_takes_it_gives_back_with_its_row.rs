//! **TAKING AND GIVING BACK ARE TWO STEPS, AND THE REGISTER SEES BOTH.** Three
//! flows cut their tree inside a shell and gave it back with a raw `worktree
//! remove`: the first wrote nothing down, so no sweep could take the tree; the
//! second left the row standing, so the register answered with directories that
//! were gone. It knew 4 of the 38 trees on this machine. ADR-026.

use serde_json::json;
use std::path::{Path, PathBuf};
use std::process::Command;

fn git(repo: &Path, args: &[&str]) {
    let ran = Command::new("git")
        .arg("-C")
        .arg(repo)
        .args(args)
        .output()
        .expect("git runs");
    assert!(
        ran.status.success(),
        "git {args:?}: {}",
        String::from_utf8_lossy(&ran.stderr)
    );
}

/// A checkout of its own, with a ledger beside it that only this case writes.
fn a_repository(name: &str) -> PathBuf {
    let at = std::env::temp_dir().join(format!("taking-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&at);
    let primary = at.join("primary");
    std::fs::create_dir_all(&primary).expect("the scratch directory");
    std::fs::create_dir_all(at.join("ledger")).expect("the ledger directory");
    std::env::set_var("SAILOR_LEDGER", at.join("ledger"));
    git(&primary, &["init", "-q", "-b", "line"]);
    git(&primary, &["config", "user.email", "taking@example"]);
    git(&primary, &["config", "user.name", "taking"]);
    git(&primary, &["commit", "-q", "--allow-empty", "-m", "first"]);
    primary
}

/// `SAILOR_LEDGER` is one variable for the whole process, so two cases setting
/// it at once would read each other's rows. They take this in turn instead:
/// the work is milliseconds and the alternative is a flaky pair.
fn one_case_at_a_time() -> std::sync::MutexGuard<'static, ()> {
    static TURN: std::sync::Mutex<()> = std::sync::Mutex::new(());
    TURN.lock().unwrap_or_else(|held| held.into_inner())
}

fn its_ledger(primary: &Path) -> PathBuf {
    primary.parent().expect("the scratch").join("ledger")
}

fn rows_open(primary: &Path) -> Vec<String> {
    use workspace::OpenTrees;
    ledger::Ledger::open(its_ledger(primary))
        .expect("the register")
        .trees_left_open()
        .expect("the rows")
        .into_iter()
        .map(|row| row.path)
        .collect()
}

/// The run and step a tree is cut under: the executor writes them, so a case
/// that calls the action directly writes them too.
fn under(run: &str, step: &str) -> flow::SharedState {
    let mut shared = flow::SharedState::default();
    shared.insert(flow::CURRENT_RUN.to_owned(), json!(run));
    shared.insert(flow::CURRENT_STEP.to_owned(), json!(step));
    shared
}

fn run(
    name: &str,
    input: serde_json::Value,
    shared: &flow::SharedState,
) -> Result<serde_json::Value, (String, String)> {
    let mut registry = flow::ActionRegistry::default();
    actions::worktree::register_take_a_tree(&mut registry, None);
    actions::worktree::register_give_a_tree_back(&mut registry, None);
    let action = registry.get(name).expect("registered");
    match action.execute(&input, shared) {
        Ok(flow::ActionOutcome::Went(said)) => Ok(said),
        Ok(other) => Err((String::new(), format!("{other:?}"))),
        Err(refusal) => Err((refusal.class, refusal.said)),
    }
}

#[test]
fn a_tree_is_on_the_register_the_moment_it_is_cut_and_off_it_when_it_is_given_back() {
    let _turn = one_case_at_a_time();
    let primary = a_repository("both-halves");
    let head = String::from_utf8(
        Command::new("git")
            .arg("-C")
            .arg(&primary)
            .args(["rev-parse", "HEAD"])
            .output()
            .expect("git runs")
            .stdout,
    )
    .expect("a commit in utf-8");
    let head = head.trim();

    let said = run(
        "take_a_tree",
        json!({ "repo": primary, "at": head, "lock": "this case is working in it" }),
        &under("a-run", "take_the_tree"),
    )
    .expect("the tree is cut");
    let cut = PathBuf::from(said["tree"].as_str().expect("a path was answered"));
    assert!(
        cut.is_dir(),
        "{} was answered and is not there",
        cut.display()
    );
    assert_eq!(
        rows_open(&primary),
        vec![cut.to_string_lossy().into_owned()],
        "the tree is written down as it is cut, or no sweep can ever take it",
    );

    let said = run(
        "give_a_tree_back",
        json!({ "repo": primary, "tree": cut }),
        &under("a-run", "release_tree"),
    )
    .expect("the tree is given back");
    assert_eq!(said["removed"], json!(cut.to_string_lossy()));
    assert!(!cut.exists(), "{} is still on disk", cut.display());
    assert!(
        rows_open(&primary).is_empty(),
        "the row stands after the tree is gone: {:?}",
        rows_open(&primary),
    );
}

/// The lock is why a raw `worktree remove` used to refuse: a run locks the tree
/// it works in, and four locked trees stood on this machine for want of an
/// unlock.
#[test]
fn a_locked_tree_is_still_given_back() {
    let _turn = one_case_at_a_time();
    let primary = a_repository("locked");
    let said = run(
        "take_a_tree",
        json!({ "repo": primary, "lock": "locked on purpose" }),
        &under("a-run", "take_the_tree"),
    )
    .expect("the tree is cut");
    let cut = PathBuf::from(said["tree"].as_str().expect("a path"));
    // git answers the canonical path, and macOS keeps the scratch behind a
    // symlink, so the two spellings are compared as git resolves them.
    let cut_as_git_sees_it = cut.canonicalize().expect("the tree that was cut");
    let listed = workspace::list(&primary).expect("git lists the trees");
    assert!(
        listed
            .iter()
            .any(|tree| Path::new(&tree.path) == cut_as_git_sees_it && tree.locked),
        "the tree was asked for locked and is not: {listed:?}",
    );
    run(
        "give_a_tree_back",
        json!({ "repo": primary, "tree": cut }),
        &under("a-run", "release_tree"),
    )
    .expect("a locked tree is unlocked and given back");
    assert!(!cut.exists(), "{} is still on disk", cut.display());
}

/// What the register does not carry belongs to whoever cut it: a person's tree,
/// or another program's. This is the same refusal `close_the_worktree` gives.
#[test]
fn a_tree_this_run_never_took_is_named_and_left_alone() {
    let _turn = one_case_at_a_time();
    let primary = a_repository("not-ours");
    let beside = primary.parent().expect("the scratch").join("by-hand");
    git(
        &primary,
        &[
            "worktree",
            "add",
            "-q",
            "--detach",
            beside.to_str().expect("a path in utf-8"),
        ],
    );
    let (class, said) = run(
        "give_a_tree_back",
        json!({ "repo": primary, "tree": beside }),
        &under("a-run", "release_tree"),
    )
    .expect_err("a tree on no register stays");
    assert_eq!(class, "the_tree_stays", "{said}");
    assert!(beside.is_dir(), "it was taken down anyway");
}

/// A step with no run and step named has no name to cut a tree under, and a
/// tree nobody can name is one nobody comes back for.
#[test]
fn a_tree_is_never_cut_under_no_name() {
    let _turn = one_case_at_a_time();
    let primary = a_repository("no-name");
    let (class, said) = run(
        "take_a_tree",
        json!({ "repo": primary }),
        &flow::SharedState::default(),
    )
    .expect_err("nothing is cut");
    assert_eq!(class, "tree_not_cut", "{said}");
    assert!(rows_open(&primary).is_empty(), "a row was written anyway");
}

/// A worker leaves work behind on nearly every run, and git refuses to take a
/// tree down over it. Refusing there would keep every worker's tree standing,
/// so what the tree holds is written where the repo can reach it first.
#[test]
fn work_left_in_a_tree_is_kept_where_the_repo_can_reach_it() {
    let _turn = one_case_at_a_time();
    let primary = a_repository("with-work");
    let said = run(
        "take_a_tree",
        json!({ "repo": primary, "lock": "a worker is in it" }),
        &under("a-run", "execute"),
    )
    .expect("the tree is cut");
    let cut = PathBuf::from(said["tree"].as_str().expect("a path"));
    std::fs::write(cut.join("what-it-did"), "an answer nobody committed\n").expect("the work");

    run(
        "give_a_tree_back",
        json!({ "repo": primary, "tree": cut }),
        &under("a-run", "release_tree"),
    )
    .expect("a tree with work in it is still given back");
    assert!(!cut.exists(), "{} is still on disk", cut.display());

    let kept = String::from_utf8(
        Command::new("git")
            .arg("-C")
            .arg(&primary)
            .args(["for-each-ref", "--format=%(refname)", "refs/sailor/kept"])
            .output()
            .expect("git runs")
            .stdout,
    )
    .expect("refs in utf-8");
    let kept = kept.trim();
    assert!(!kept.is_empty(), "the work was taken down with the tree");
    let held = String::from_utf8(
        Command::new("git")
            .arg("-C")
            .arg(&primary)
            .args(["show", &format!("{kept}:what-it-did")])
            .output()
            .expect("git runs")
            .stdout,
    )
    .expect("the file in utf-8");
    assert_eq!(held, "an answer nobody committed\n", "kept as {kept}");
}
