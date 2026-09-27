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

fn a_tree_with_finished_work() -> (Scratch, PathBuf) {
    let at = std::env::temp_dir().join(format!("finished-{}", std::process::id()));
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
    let (_scratch, tree) = a_tree_with_finished_work();
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
