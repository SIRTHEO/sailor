//! **AN AGENT STARTS BELIEVING IT IS ALONE, AND HERE IT USUALLY IS NOT.** The
//! register knows who is open and where; nobody asked it at the one moment the
//! answer changes what a reader does with `git status`.

use sessions::{others_in_the_tree, TerminalRow};

fn a_terminal(tty: &str, worktree: &str, closed_at: Option<i64>) -> TerminalRow {
    TerminalRow {
        tty: tty.to_owned(),
        worktree: worktree.to_owned(),
        ancestor: None,
        session_id: None,
        transcript_path: None,
        opened_at: 1_000,
        closed_at,
        detached_at: None,
    }
}

#[test]
fn the_others_are_the_open_ones_of_this_tree_and_never_me() {
    let rows = vec![
        a_terminal("ttys001", "/un-albero", None),
        a_terminal("ttys002", "/un-albero", None),
        // Closed: gone, whatever tree it was in.
        a_terminal("ttys003", "/un-albero", Some(2_000)),
        // Open, and somewhere else: not a neighbour.
        a_terminal("ttys004", "/un-altro", None),
    ];

    let seen: Vec<&str> = others_in_the_tree(&rows, "ttys001", "/un-albero")
        .into_iter()
        .map(|row| row.tty.as_str())
        .collect();
    assert_eq!(seen, vec!["ttys002"]);

    // The absurd control: alone is an empty list, not everybody. Without this
    // arm the function could answer «all the open ones» and pass.
    let alone = vec![a_terminal("ttys001", "/un-albero", None)];
    assert!(others_in_the_tree(&alone, "ttys001", "/un-albero").is_empty());
}

/// **WHAT IT ANSWERS IS THE REGISTER, NOT WHAT IS ALIVE.** A terminal killed
/// without closing stays in this list, and whoever prints it says so.
#[test]
fn a_terminal_killed_without_closing_is_still_listed() {
    let rows = vec![
        a_terminal("ttys001", "/un-albero", None),
        a_terminal("ttys009", "/un-albero", None),
    ];
    assert_eq!(others_in_the_tree(&rows, "ttys001", "/un-albero").len(), 1);
}
