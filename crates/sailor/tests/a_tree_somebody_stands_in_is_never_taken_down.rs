//! **A CLEAN, MERGED TREE WITH A LIVE SESSION IN IT LOOKS LIKE A FINISHED
//! ONE.** The sweep decided on history and on git's refusal over uncommitted
//! files, and neither can see a person: it took down the tree a peer session
//! was working in, the same minute that session left its mandate.

use sailor::worktree_cmd::occupied_trees;
use std::path::PathBuf;

/// The reading is handed in, so this asks the rule and not the machine.
#[test]
fn a_tree_a_terminal_is_open_in_is_read_as_occupied() {
    let standing = [
        ("/a/tree/somebody/is/in".to_owned(), true),
        ("/a/tree/nobody/is/in".to_owned(), false),
    ];

    let held = occupied_trees(standing.iter().map(|(at, open)| (at.clone(), *open)));

    assert!(held.contains(&PathBuf::from("/a/tree/somebody/is/in")));
    assert!(!held.contains(&PathBuf::from("/a/tree/nobody/is/in")));
}

/// **THE ABSURD CASE**: nobody anywhere, and the sweep is free to work. A
/// guard that held every tree would be a sweep that never sweeps.
#[test]
fn with_no_terminal_open_anywhere_nothing_is_held_back() {
    let held = occupied_trees([("/a/tree".to_owned(), false)].into_iter());

    assert!(held.is_empty());
}

/// The sweep asks before it closes, and it asks in the same pass: read over
/// the whole file this check passed with the guard declared and never called.
#[test]
fn the_sweep_asks_who_is_standing_in_a_tree_before_taking_it_down() {
    let source = include_str!("../src/worktree_cmd.rs");
    let from = source
        .find("pub fn sweep(")
        .expect("the sweep is gone: this check measures nothing");
    let rest = &source[from..];
    let body = &rest[..rest.find("\n}\n").unwrap_or(rest.len())];

    assert!(
        body.contains("occupied"),
        "the sweep takes trees down without asking whether anybody is in them"
    );
    workspace::measured(1, "sweep read, and it asks who is standing in a tree");
}
