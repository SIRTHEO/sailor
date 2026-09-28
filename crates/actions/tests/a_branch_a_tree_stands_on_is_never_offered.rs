//! What the reading of finished branches must never hand back: a branch some
//! working tree is checked out on. git marks those with `+`, and a reading
//! that trimmed the marker along with the whitespace would offer thirteen
//! trees' ground for deletion while looking exactly as green as this one.

use actions::finished::{as_an_item, branches_trees_hold, merged_branches};
use std::path::{Path, PathBuf};
use std::process::Command;

/// What `git branch --merged <trunk> --list 'work/*'` prints: the branch this
/// checkout is on carries `*`, one another worktree holds carries `+`.
const LISTED: &str = "\
  work/a-reading-that-was-finished
* work/the-one-this-checkout-is-on
+ work/the-one-another-tree-holds
  work/a-second-finished-reading
";

#[test]
fn the_marker_of_a_tree_is_read_and_not_trimmed_away() {
    let read = merged_branches(LISTED);
    assert_eq!(
        read,
        vec![
            ("work/a-reading-that-was-finished".to_owned(), false),
            ("work/the-one-this-checkout-is-on".to_owned(), false),
            ("work/the-one-another-tree-holds".to_owned(), true),
            ("work/a-second-finished-reading".to_owned(), false),
        ],
        "the `+` of a held branch must survive the reading of the name"
    );
}

/// The `+` is git's own marking and only covers what it marks; the worktree
/// list is the second reading, so a branch held without the marker still
/// stays out.
#[test]
fn a_branch_a_worktree_names_is_found_without_the_marker() {
    let held = branches_trees_hold(
        "worktree /somewhere/main\nHEAD abc\nbranch refs/heads/main\n\n\
         worktree /somewhere/other\nHEAD def\nbranch refs/heads/work/held-elsewhere\n\n\
         worktree /somewhere/detached\nHEAD 012\ndetached\n",
    );
    assert!(
        held.contains("work/held-elsewhere"),
        "a held branch: {held:?}"
    );
    assert!(held.contains("main"), "the trunk's own checkout: {held:?}");
    assert_eq!(held.len(), 2, "a detached tree holds no branch: {held:?}");
}

/// The element `for_each` hands `close-the-work`: its trigger reads a mandate
/// out of `text`, so the element carries one, as a string and not an object.
#[test]
fn an_item_carries_the_mandate_close_the_work_reads() {
    let item = as_an_item(Path::new("/a/tree"), "work/a-change");
    assert_eq!(item["source"], "manual");
    let mandate: serde_json::Value = serde_json::from_str(
        item["text"]
            .as_str()
            .expect("the mandate is text, not an object"),
    )
    .expect("the text parses as the mandate close-the-work reads");
    assert_eq!(mandate["repo"], "/a/tree");
    assert_eq!(mandate["branch"], "work/a-change");
}

/// A repository with a trunk, a policy on it, a finished branch nobody holds
/// and a finished branch a second worktree stands on. Built here rather than
/// read off this machine: a reading that only ran where the machine happened
/// to have thirteen locked trees would measure nothing anywhere else.
struct Scratch(PathBuf);

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn git(at: &Path, args: &[&str]) -> String {
    let out = Command::new("git")
        .arg("-C")
        .arg(at)
        .args(args)
        .output()
        .expect("git runs");
    assert!(
        out.status.success(),
        "git {args:?} in {}: {}",
        at.display(),
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8_lossy(&out.stdout).trim().to_owned()
}

fn a_tree_with_finished_work(label: &str) -> (Scratch, PathBuf) {
    let at = std::env::temp_dir().join(format!("finished-{label}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&at);
    std::fs::create_dir_all(at.join("tree")).expect("the tree's directory");
    let scratch = Scratch(at.clone());
    let (remote, tree) = (at.join("origin.git"), at.join("tree"));
    git(&at, &["init", "-q", "--bare", "origin.git"]);
    git(&at, &["init", "-q", "-b", "main", "tree"]);
    git(&tree, &["config", "user.email", "sailor@example.invalid"]);
    git(&tree, &["config", "user.name", "Sailor"]);
    git(&tree, &["config", "sailor.trunk", "main"]);
    std::fs::create_dir_all(tree.join(".sailor")).expect("the policy's directory");
    std::fs::write(
        tree.join(".sailor/delivery-policy.json"),
        r#"{"merge":"ask","push":"ask","release":"ask","remote":"origin"}"#,
    )
    .expect("a policy on the trunk");
    std::fs::write(tree.join("a-file"), "the work\n").expect("a file");
    git(&tree, &["add", "-A"]);
    git(&tree, &["commit", "-q", "-m", "the work"]);
    git(
        &tree,
        &["remote", "add", "origin", &remote.to_string_lossy()],
    );
    git(&tree, &["push", "-q", "origin", "main"]);
    // Two branches finished at the trunk: one free, one a second tree stands on.
    for branch in ["work/free-to-close", "work/a-tree-stands-on-it"] {
        git(&tree, &["branch", branch, "main"]);
    }
    git(
        &tree,
        &[
            "worktree",
            "add",
            "-q",
            &at.join("second").to_string_lossy(),
            "work/a-tree-stands-on-it",
        ],
    );
    (scratch, tree)
}

/// **PROVED BY RUNNING IT.** Against this repository the action reads the
/// branches, the policy and the worktrees the way it will on a real tree.
#[test]
fn against_a_real_repository_a_held_branch_is_kept_back_and_a_free_one_offered() {
    let (_scratch, tree) = a_tree_with_finished_work("repo");
    let mut registry = flow::ActionRegistry::default();
    actions::finished::register_finished_branches(&mut registry);
    let said = match registry
        .get("finished_branches")
        .expect("the action is registered")
        .execute(
            &serde_json::json!({"repo": tree.to_string_lossy(), "prefix": "work/"}),
            &flow::SharedState::new(),
        )
        .expect("the reading is taken")
    {
        flow::ActionOutcome::Went(said) => said,
        other => panic!("the reading did not go: {other:?}"),
    };
    assert_eq!(
        said["branches"],
        serde_json::json!(["work/free-to-close"]),
        "only the branch no tree stands on is offered: {said}"
    );
    assert_eq!(
        said["held_by_a_tree"],
        serde_json::json!(["work/a-tree-stands-on-it"]),
        "the branch a worktree stands on is named and kept back: {said}"
    );
    assert_eq!(said["items"].as_array().map_or(0, Vec::len), 1);
    workspace::measured_against(
        1,
        "finished branch offered",
        1,
        "held by a tree and kept back",
    );
}

fn read_finished(declared: serde_json::Value) -> Result<flow::ActionOutcome, flow::ActionError> {
    let mut registry = flow::ActionRegistry::default();
    actions::finished::register_finished_branches(&mut registry);
    let action = registry
        .get("finished_branches")
        .expect("the action is registered");
    assert!(
        action.unknown_fields(&declared).is_empty(),
        "the declaration is one the action knows: {declared}"
    );
    action.execute(&declared, &flow::SharedState::new())
}

/// A sensor, and a step with no repository of its own, are offered the
/// project root as `workdir`: that tree is the one read when no repo is named.
#[test]
fn the_tree_it_is_offered_is_read_when_no_repo_is_named() {
    let (_scratch, tree) = a_tree_with_finished_work("workdir");
    let said = match read_finished(
        serde_json::json!({"workdir": tree.to_string_lossy(), "prefix": "work/"}),
    )
    .expect("the reading is taken")
    {
        flow::ActionOutcome::Went(said) => said,
        other => panic!("the reading did not go: {other:?}"),
    };
    assert_eq!(
        said["branches"],
        serde_json::json!(["work/free-to-close"]),
        "{said}"
    );
}

/// With neither a repo nor a tree to work in there is nothing to read, and an
/// empty list would close nothing while looking like a reading.
#[test]
fn with_no_tree_at_all_nothing_is_read() {
    let refused =
        read_finished(serde_json::json!({"prefix": "work/"})).expect_err("no tree is no reading");
    assert!(
        refused.to_string().contains("no tree"),
        "the refusal says what is missing: {refused}"
    );
}

#[test]
fn a_finished_branch_with_no_merged_request_of_its_own_is_named_and_never_offered() {
    let repo = std::path::Path::new("/a/tree");
    let branches = vec!["work/merged".to_owned(), "work/never-requested".to_owned()];
    let merged: std::collections::BTreeSet<String> =
        ["work/merged".to_owned(), "work/elsewhere".to_owned()].into();
    let (items, unproven) = actions::proven::split_by_proof(repo, &branches, &merged);
    assert_eq!(
        items,
        vec![actions::finished::as_an_item(repo, "work/merged")]
    );
    assert_eq!(unproven, vec!["work/never-requested".to_owned()]);
}

fn prove(repo: &Path, forges: Vec<actions::proven::ForgeProgram>) -> serde_json::Value {
    let mut registry = flow::ActionRegistry::default();
    actions::proven::register_proven_branches(&mut registry, forges);
    let action = registry
        .get("proven_branches")
        .expect("the action is registered");
    let declared = serde_json::json!({
        "repo": repo.to_string_lossy(),
        "branches": ["work/finished"],
    });
    match action.execute(&declared, &flow::SharedState::new()) {
        Ok(flow::ActionOutcome::Went(value)) => value,
        other => panic!("a missing declaration completes the run and says why: {other:?}"),
    }
}

fn a_forge_nobody_installed() -> actions::proven::ForgeProgram {
    actions::proven::ForgeProgram {
        forge: "a-forge".to_owned(),
        program: "/nowhere/a-forge-program".to_owned(),
        token_variable: "A_FORGE_TOKEN".to_owned(),
        never_inherited: vec!["A_FORGE_TOKEN".to_owned()],
    }
}

/// A declaration that is missing now is missing on the next beat too: the run
/// completes offering nothing and names it, rather than failing every cooldown.
#[test]
fn a_policy_that_declares_no_forge_offers_nothing_and_says_so() {
    let (_scratch, tree) = a_tree_with_finished_work("no-forge");
    let answer = prove(&tree, vec![a_forge_nobody_installed()]);
    assert_eq!(answer["items"], serde_json::json!([]));
    assert_eq!(answer["unproven"], serde_json::json!(["work/finished"]));
    assert!(
        answer["why"]
            .as_str()
            .unwrap_or("")
            .contains("declares no forge"),
        "the missing declaration is named: {answer}"
    );
}

#[test]
fn a_forge_program_not_installed_offers_nothing_and_says_so() {
    let (_scratch, tree) = a_tree_with_finished_work("no-program");
    std::fs::write(
        tree.join(".sailor/delivery-policy.json"),
        r#"{"merge":"ask","push":"ask","release":"ask","remote":"origin","forge":"a-forge"}"#,
    )
    .expect("a policy that declares a forge");
    git(&tree, &["commit", "-q", "-am", "the forge is declared"]);
    git(&tree, &["config", "sailor.forgeAs", "someone"]);
    let answer = prove(&tree, vec![a_forge_nobody_installed()]);
    assert_eq!(answer["items"], serde_json::json!([]));
    assert!(
        answer["why"]
            .as_str()
            .unwrap_or("")
            .contains("no forge token for the declared account someone"),
        "the account with no token is named: {answer}"
    );
}

/// The fault register refuses a line break, and the reason becomes a fault's
/// text: a program that explains itself over several lines must still leave
/// one line, and the tree it happened in.
#[test]
fn a_program_that_refuses_over_several_lines_leaves_one_line_naming_the_tree() {
    let (scratch, tree) = a_tree_with_finished_work("two-lines");
    std::fs::write(
        tree.join(".sailor/delivery-policy.json"),
        r#"{"merge":"ask","push":"ask","release":"ask","remote":"origin","forge":"a-forge"}"#,
    )
    .expect("a policy that declares a forge");
    git(&tree, &["commit", "-q", "-am", "the forge is declared"]);
    git(&tree, &["config", "sailor.forgeAs", "someone"]);
    let program = scratch.0.join("a-forge-program");
    std::fs::write(
        &program,
        "#!/bin/sh\necho 'no token here' >&2\necho 'try logging in | | again' >&2\nprintf 'a carriage\\rreturn' >&2\nexit 1\n",
    )
    .expect("a program that refuses");
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(&program, std::fs::Permissions::from_mode(0o755)).expect("it can run");
    let mut forge = a_forge_nobody_installed();
    forge.program = program.to_string_lossy().into_owned();
    let answer = prove(&tree, vec![forge]);
    let why = answer["why"].as_str().unwrap_or("");
    assert!(
        !why.contains(['\n', '\r', '|']),
        "one line, with no separator the register refuses: {why:?}"
    );
    assert!(
        why.contains("no token here") && why.contains("try logging in"),
        "{why:?}"
    );
    assert!(
        why.contains(&*tree.to_string_lossy()),
        "the tree is named: {why:?}"
    );
}
