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

fn decide(trees: &[Worktree], branch: &str) -> WhatBecomesOfIt {
    what_becomes_of_it(trees, Path::new(TOP), branch, &[], &[])
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

#[test]
fn a_tree_with_a_terminal_recorded_in_it_is_kept() {
    let trees = [tree(TOP, Some("main")), tree("/trees/work", Some("work"))];
    let terminals = [PathBuf::from("/trees/work")];
    assert_eq!(
        what_becomes_of_it(&trees, Path::new(TOP), "work", &terminals, &[]),
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
        what_becomes_of_it(&trees, Path::new(TOP), "work", &[], &standing),
        WhatBecomesOfIt::SomebodyIsIn(PathBuf::from("/trees/work"), WhoIsIn::AProcess(4242))
    );
}

#[test]
fn a_terminal_in_a_neighbour_tree_is_not_in_this_one() {
    let trees = [tree(TOP, Some("main")), tree("/trees/work", Some("work"))];
    let terminals = [PathBuf::from("/trees/work-two")];
    assert_eq!(
        what_becomes_of_it(&trees, Path::new(TOP), "work", &terminals, &[]),
        WhatBecomesOfIt::TakeItDown(PathBuf::from("/trees/work"))
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
        what_becomes_of_it(&trees, Path::new(TOP), "work", &[], &standing),
        WhatBecomesOfIt::SomebodyIsIn(PathBuf::from("/trees/work"), WhoIsIn::AProcess(7))
    );
}
