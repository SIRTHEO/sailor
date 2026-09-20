//! A branch nothing is watching is reported, and the judge is pure.
//!
//! The facts are a table written here, never the branches of the machine
//! running this, for the reason `a_branch_is_named_for_the_work_it_carries`
//! gives: a red caused by somebody else's stray branch is a verdict on the
//! machine, and whoever is handed it can do nothing about it.

use workspace::branches::{adrift, Branch, ADRIFT_AFTER_HOURS};

/// **NOT «main».** The trunk is handed in, so a repository that calls it
/// something else is read correctly.
const A_TRUNK: &str = "tronco";

fn branch(name: &str, in_the_trunk: bool, has_a_tree: bool, idle_hours: i64) -> Branch {
    Branch {
        name: name.to_owned(),
        in_the_trunk,
        has_a_tree,
        idle_hours,
    }
}

const LONG_ENOUGH: i64 = ADRIFT_AFTER_HOURS + 1;

/// Work in the trunk is delivered, however long the branch has stood there.
#[test]
fn a_branch_the_trunk_already_holds_is_not_adrift() {
    let standing = vec![branch("work/delivered", true, false, LONG_ENOUGH * 10)];
    assert!(adrift(&standing, A_TRUNK).is_empty());
}

/// A tree standing on a branch is somebody in the middle of it.
#[test]
fn a_branch_with_a_tree_standing_on_it_is_not_adrift() {
    let standing = vec![branch("work/in-hand", false, true, LONG_ENOUGH * 10)];
    assert!(adrift(&standing, A_TRUNK).is_empty());
}

/// **THE CEILING IS MEASURED, NOT CHOSEN**: every branch that reached this
/// trunk took less. A branch still inside it is slow, not stopped.
#[test]
fn a_branch_younger_than_the_ceiling_is_still_moving() {
    let standing = vec![branch("work/today", false, false, ADRIFT_AFTER_HOURS)];
    assert!(adrift(&standing, A_TRUNK).is_empty());
}

/// **THE CASE THIS EXISTS FOR:** unmerged, no tree, and older than anything
/// that ever arrived.
#[test]
fn work_nothing_holds_and_nothing_merged_is_adrift() {
    let standing = vec![branch("work/forgotten", false, false, LONG_ENOUGH)];
    let reported = adrift(&standing, A_TRUNK);
    assert_eq!(reported.len(), 1);
    assert_eq!(reported[0].name, "work/forgotten");
}

/// The trunk itself is never adrift, whatever the facts around it say.
#[test]
fn the_trunk_is_never_reported() {
    let standing = vec![branch(A_TRUNK, false, false, LONG_ENOUGH * 10)];
    assert!(adrift(&standing, A_TRUNK).is_empty());
}

/// The longest idle comes first: whoever reads this acts from the top.
#[test]
fn the_report_leads_with_the_one_left_longest() {
    let standing = vec![
        branch("work/recent", false, false, LONG_ENOUGH),
        branch("work/ancient", false, false, LONG_ENOUGH * 4),
        branch("work/middle", false, false, LONG_ENOUGH * 2),
    ];
    let names: Vec<&str> = adrift(&standing, A_TRUNK).iter().map(|b| b.name.as_str()).collect();
    assert_eq!(names, ["work/ancient", "work/middle", "work/recent"]);
}
