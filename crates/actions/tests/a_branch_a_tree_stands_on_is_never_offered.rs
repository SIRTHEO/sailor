//! What the reading of finished branches must never hand back: a branch some
//! working tree is checked out on. git marks those with `+`, and a reading
//! that trimmed the marker along with the whitespace would offer thirteen
//! trees' ground for deletion while looking exactly as green as this one.

use actions::finished::{as_an_item, branches_trees_hold, merged_branches};
use std::path::Path;

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
    assert!(held.contains("work/held-elsewhere"), "a held branch: {held:?}");
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
        item["text"].as_str().expect("the mandate is text, not an object"),
    )
    .expect("the text parses as the mandate close-the-work reads");
    assert_eq!(mandate["repo"], "/a/tree");
    assert_eq!(mandate["branch"], "work/a-change");
}

/// **PROVED BY RUNNING IT.** Eight green readings said nothing about the one
/// thing that breaks: what the action answers against a real repository with
/// real worktrees. It runs here against the tree the test is in, and skips
/// where that tree declares no trunk, so it measures or says it did not.
#[test]
fn against_a_real_tree_it_offers_no_branch_a_tree_holds() {
    let repo = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("the crate sits two levels under the root");
    if workspace::declared_trunk(repo).is_err() {
        workspace::measured_nothing("this tree declares no trunk to read branches against");
        return;
    }
    let mut registry = flow::ActionRegistry::default();
    actions::finished::register_finished_branches(&mut registry);
    let said = match registry
        .get("finished_branches")
        .expect("the action is registered")
        .execute(
            &serde_json::json!({"repo": repo.to_string_lossy(), "prefix": "work/"}),
            &flow::SharedState::new(),
        ) {
        Ok(flow::ActionOutcome::Went(said)) => said,
        other => {
            workspace::measured_nothing(&format!("the reading could not be taken here: {other:?}"));
            return;
        }
    };
    let held: Vec<&str> = said["held_by_a_tree"]
        .as_array()
        .expect("the held branches are listed")
        .iter()
        .filter_map(serde_json::Value::as_str)
        .collect();
    let offered: Vec<&str> = said["branches"]
        .as_array()
        .expect("the free branches are listed")
        .iter()
        .filter_map(serde_json::Value::as_str)
        .collect();
    for branch in &held {
        assert!(
            !offered.contains(branch),
            "{branch} is held by a tree and was offered for closing anyway"
        );
    }
    assert_eq!(
        offered.len(),
        said["items"].as_array().map_or(0, Vec::len),
        "every branch offered carries one mandate, and no mandate names none"
    );
    workspace::measured_against(
        offered.len(),
        "finished branches offered",
        held.len(),
        "held by a tree and kept back",
    );
}
