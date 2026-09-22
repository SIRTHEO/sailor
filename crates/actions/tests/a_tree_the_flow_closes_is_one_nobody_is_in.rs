//! What a delivery flow does about the tree a finished branch was worked in.
//! Every case here is decided from readings handed in, so a refusal is proved
//! without a machine that happens to have a terminal open in the right place.

use actions::worktree::{what_becomes_of_it, WhatBecomesOfIt};
use std::path::{Path, PathBuf};
use workspace::standing::WhoIsIn;
use workspace::Worktree;

const TOP: &str = "/repo";

fn tree(path: &str, branch: Option<&str>) -> Worktree {
    Worktree {
        path: path.to_owned(),
        head: "c0ffee".to_owned(),
        branch: branch.map(str::to_owned),
        locked: false,
        prunable: false,
    }
}

fn locked(path: &str, branch: &str) -> Worktree {
    Worktree {
        locked: true,
        ..tree(path, Some(branch))
    }
}

/// Every tree the cases name, as Sailor wrote it down when it cut it.
fn cut() -> Vec<PathBuf> {
    ["/trees/work", "/trees/w", "/trees/other"]
        .map(PathBuf::from)
        .to_vec()
}

fn decide(trees: &[Worktree], branch: &str) -> WhatBecomesOfIt {
    what_becomes_of_it(
        trees,
        Path::new(TOP),
        Path::new(TOP),
        branch,
        &[],
        &[],
        &cut(),
    )
}

#[test]
fn a_tree_nobody_is_in_is_taken_down() {
    let trees = [tree(TOP, Some("main")), tree("/trees/work", Some("work"))];
    assert_eq!(
        decide(&trees, "work"),
        WhatBecomesOfIt::TakeItDown(PathBuf::from("/trees/work"))
    );
}

#[test]
fn no_tree_carries_the_branch_and_none_is_invented() {
    let trees = [tree(TOP, Some("main")), tree("/trees/other", Some("other"))];
    assert_eq!(decide(&trees, "work"), WhatBecomesOfIt::NoTreeCarriesIt);
}

/// A tree on a detached head is one a person can be surprised by; it carries
/// no branch, so no branch names it.
#[test]
fn a_tree_on_a_detached_head_carries_no_branch() {
    let trees = [tree(TOP, Some("main")), tree("/trees/work", None)];
    assert_eq!(decide(&trees, "work"), WhatBecomesOfIt::NoTreeCarriesIt);
}

#[test]
fn a_branch_whose_name_begins_the_same_is_not_this_one() {
    let trees = [tree(TOP, Some("main")), tree("/trees/w", Some("work/two"))];
    assert_eq!(decide(&trees, "work"), WhatBecomesOfIt::NoTreeCarriesIt);
}

/// The primary checkout is where the flow itself runs from: taking it down is
/// a flow pulling the floor out from under its own remaining steps.
#[test]
fn the_tree_the_flow_runs_from_is_never_taken_down() {
    let trees = [tree(TOP, Some("work"))];
    assert_eq!(
        decide(&trees, "work"),
        WhatBecomesOfIt::ItIsWhereTheFlowRuns(PathBuf::from(TOP))
    );
}

/// A request may name as repo the linked tree that carries its own branch:
/// every later step of the flow still reads that tree.
#[test]
fn the_linked_tree_named_as_repo_is_never_taken_down() {
    let trees = [tree(TOP, Some("main")), tree("/trees/work", Some("work"))];
    for repo in ["/trees/work", "/trees/work/crates"] {
        assert_eq!(
            what_becomes_of_it(
                &trees,
                Path::new(TOP),
                Path::new(repo),
                "work",
                &[],
                &[],
                &cut()
            ),
            WhatBecomesOfIt::ItIsWhereTheFlowRuns(PathBuf::from("/trees/work")),
            "repo {repo}"
        );
    }
}

#[test]
fn a_tree_with_a_terminal_recorded_in_it_is_kept() {
    let trees = [tree(TOP, Some("main")), tree("/trees/work", Some("work"))];
    let terminals = [PathBuf::from("/trees/work")];
    assert_eq!(
        what_becomes_of_it(
            &trees,
            Path::new(TOP),
            Path::new(TOP),
            "work",
            &terminals,
            &[],
            &cut()
        ),
        WhatBecomesOfIt::SomebodyIsIn(
            PathBuf::from("/trees/work"),
            WhoIsIn::ATerminal(PathBuf::from("/trees/work"))
        )
    );
}

#[test]
fn a_tree_a_process_stands_in_is_kept_and_the_process_is_named() {
    let trees = [tree(TOP, Some("main")), tree("/trees/work", Some("work"))];
    let standing = [(4242, PathBuf::from("/trees/work/crates/flow"))];
    assert_eq!(
        what_becomes_of_it(
            &trees,
            Path::new(TOP),
            Path::new(TOP),
            "work",
            &[],
            &standing,
            &cut()
        ),
        WhatBecomesOfIt::SomebodyIsIn(PathBuf::from("/trees/work"), WhoIsIn::AProcess(4242))
    );
}

#[test]
fn a_terminal_in_a_neighbour_tree_is_not_in_this_one() {
    let trees = [tree(TOP, Some("main")), tree("/trees/work", Some("work"))];
    let terminals = [PathBuf::from("/trees/work-two")];
    assert_eq!(
        what_becomes_of_it(
            &trees,
            Path::new(TOP),
            Path::new(TOP),
            "work",
            &terminals,
            &[],
            &cut()
        ),
        WhatBecomesOfIt::TakeItDown(PathBuf::from("/trees/work"))
    );
}

/// **A TREE SAILOR NEVER CUT IS NOT SAILOR'S TO TAKE DOWN**, whatever branch it
/// carries: a workspace a person opened by hand is named and left to them.
#[test]
fn a_tree_sailor_never_wrote_down_is_named_and_left() {
    let trees = [
        tree(TOP, Some("main")),
        tree("/elsewhere/work", Some("work")),
    ];
    assert_eq!(
        decide(&trees, "work"),
        WhatBecomesOfIt::NotCutBySailor(PathBuf::from("/elsewhere/work"))
    );
}

/// A lock is somebody saying «I am still reading this». It is answered after
/// the people, because a person in the tree is the fact worth naming first.
#[test]
fn a_locked_tree_is_kept() {
    let trees = [tree(TOP, Some("main")), locked("/trees/work", "work")];
    assert_eq!(
        decide(&trees, "work"),
        WhatBecomesOfIt::Locked(PathBuf::from("/trees/work"))
    );
}

#[test]
fn somebody_in_a_locked_tree_is_named_before_the_lock() {
    let trees = [tree(TOP, Some("main")), locked("/trees/work", "work")];
    let standing = [(7, PathBuf::from("/trees/work"))];
    assert_eq!(
        what_becomes_of_it(
            &trees,
            Path::new(TOP),
            Path::new(TOP),
            "work",
            &[],
            &standing,
            &cut()
        ),
        WhatBecomesOfIt::SomebodyIsIn(PathBuf::from("/trees/work"), WhoIsIn::AProcess(7))
    );
}

/// The action itself, on a repository with one linked tree. The terminals it
/// reads are an empty ledger of its own, so no terminal of the machine running
/// the case stands in these trees.
mod on_a_real_repository {
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

    /// A primary checkout on `line`, and a linked tree beside it on `work`.
    fn a_repository(name: &str) -> (PathBuf, PathBuf) {
        let at = std::env::temp_dir().join(format!("closing-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&at);
        let (primary, linked) = (at.join("primary"), at.join("linked"));
        std::fs::create_dir_all(&primary).expect("the scratch directory");
        std::fs::create_dir_all(at.join("ledger")).expect("the ledger directory");
        std::env::set_var("SAILOR_LEDGER", at.join("ledger"));
        git(&primary, &["init", "-q", "-b", "line"]);
        git(&primary, &["config", "user.email", "closing@example"]);
        git(&primary, &["config", "user.name", "closing"]);
        git(&primary, &["commit", "-q", "--allow-empty", "-m", "first"]);
        let linked_arg = linked.to_str().expect("a path in utf-8");
        git(
            &primary,
            &["worktree", "add", "-q", "-b", "work", linked_arg],
        );
        (primary, linked)
    }

    fn close(repo: &Path) -> Result<serde_json::Value, (String, String)> {
        let mut registry = flow::ActionRegistry::default();
        actions::worktree::register_close_the_worktree(&mut registry);
        let action = registry.get("close_the_worktree").expect("registered");
        let input = json!({ "repo": repo, "branch": "work" });
        match action.execute(&input, &flow::SharedState::default()) {
            Ok(flow::ActionOutcome::Went(said)) => Ok(said),
            Ok(other) => Err(("".to_owned(), format!("{other:?}"))),
            Err(refusal) => Err((refusal.class, refusal.said)),
        }
    }

    /// The ledger `a_repository` pointed the action at, beside the two trees.
    fn its_ledger(primary: &Path) -> PathBuf {
        primary.parent().expect("the scratch").join("ledger")
    }

    fn written_down(primary: &Path, linked: &Path) {
        use workspace::OpenTrees;
        let register = ledger::Ledger::open(its_ledger(primary)).expect("the register");
        register
            .tree_opened(&workspace::OpenTree {
                path: linked.to_string_lossy().into_owned(),
                repo: primary.to_string_lossy().into_owned(),
                run: String::new(),
                step: String::new(),
                opened_by_pid: 1,
                opened_at: 0,
                opened_by_born_at: None,
            })
            .expect("the register takes it");
    }

    fn still_open(primary: &Path, linked: &Path) -> bool {
        use workspace::OpenTrees;
        ledger::Ledger::open(its_ledger(primary))
            .expect("the register")
            .trees_left_open()
            .expect("the rows")
            .iter()
            .any(|row| Path::new(&row.path) == linked)
    }

    #[test]
    fn the_linked_tree_goes_when_the_primary_is_named_and_stays_when_it_is() {
        let (primary, linked) = a_repository("both");

        let (class, said) = close(&primary).expect_err("a tree nobody wrote down stays");
        assert_eq!(class, "the_tree_stays", "{said}");
        assert!(said.contains("not cut by Sailor"), "{said}");
        assert!(
            linked.join(".git").exists(),
            "the unwritten tree is still there"
        );
        written_down(&primary, &linked);

        let (class, said) = close(&linked).expect_err("the tree named as repo stays");
        assert_eq!(class, "the_tree_stays", "{said}");
        assert!(said.contains("runs from"), "{said}");
        assert!(
            linked.join(".git").exists(),
            "the linked tree is still there"
        );

        close(&primary).expect("named from the primary, the linked tree is taken down");
        assert!(!linked.exists(), "the linked tree is gone");
        assert!(
            !still_open(&primary, &linked),
            "its row still counts as open"
        );
    }
}
