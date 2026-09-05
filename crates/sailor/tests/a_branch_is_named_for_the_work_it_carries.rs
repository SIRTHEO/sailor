//! A branch is named for the work it carries, and the judge is pure.
//!
//! The names are a table written here, never the branches of the machine
//! running this. A check reading those goes red the day somebody else leaves
//! a stray branch behind — a verdict on the machine, not on the work — and
//! whoever is handed that red cannot act on it.

use workspace::branches::{against_the_convention, follows_the_convention};

/// The table of names that follow the convention, each for its own reason.
const FOLLOW: &[&str] = &[
    "sorgenti",
    "main",
    "work/terminal-claims",
    "work/toml-graft",
    "work/branch-hygiene",
    "work/delega-d6",
    "work/a",
    "work/f2",
    "worktree-agent-a2d65109fbb768d5b",
    "worktree-agent-1",
];

/// The table of names that break it: a topic that is not one, a shape that is
/// not `work/`, and the exempt prefixes with nothing after them.
const BREAK: &[&str] = &[
    "innesto-toml-codex",
    "matteodimattia/chore-scope-conversation-to",
    "work/Terminal-Claims",
    "work/annunci_terminali",
    "work/nodo porte",
    "work/-porte",
    "work/porte-",
    "work/",
    "work/nested/topic",
    "worktree-agent-",
    "prova/fusione",
    "sorgenti-2",
    "",
];

fn named(names: &[&str]) -> Vec<String> {
    names.iter().map(|name| (*name).to_owned()).collect()
}

#[test]
fn a_name_that_follows_the_convention_is_never_reported() {
    let reported = against_the_convention(&named(FOLLOW));
    assert!(reported.is_empty(), "reported as breaking: {reported:?}");
}

#[test]
fn every_name_that_breaks_the_convention_is_reported() {
    let reported = against_the_convention(&named(BREAK));
    assert_eq!(reported, named(BREAK), "some breaking name went unreported");
}

#[test]
fn the_trunk_is_exempt_and_it_is_the_one_that_releases() {
    assert!(follows_the_convention(workspace::branches::TRUNK));
    assert_eq!(workspace::branches::TRUNK, "sorgenti");
}

#[test]
fn the_history_kept_under_its_own_name_is_exempt() {
    assert!(follows_the_convention(workspace::branches::KEPT_HISTORY));
    assert_eq!(workspace::branches::KEPT_HISTORY, "main");
}

#[test]
fn a_tree_the_mechanism_named_is_exempt_because_nobody_chose_it() {
    assert!(follows_the_convention("worktree-agent-a4d628b5cb6687bc0"));
    assert!(
        !follows_the_convention("worktree-agent-"),
        "the prefix alone names no tree"
    );
}

/// The judge is only half of it: a person needs a way to run it.
#[test]
fn the_command_line_carries_the_naming_check_as_a_verb() {
    let verbs = sailor::verbs_of(sailor::worktree_cmd::USAGE);
    assert!(verbs.contains(&"names"), "{verbs:?}");
}

/// A mixed list answers about each name, in the order it was given.
#[test]
fn the_names_come_back_in_the_order_they_were_given() {
    let mixed = named(&["work/one", "innesto-toml-codex", "sorgenti", "work/Two"]);
    assert_eq!(
        against_the_convention(&mixed),
        named(&["innesto-toml-codex", "work/Two"])
    );
}
