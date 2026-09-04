//! **AN AGENT STARTS BELIEVING IT IS ALONE, AND HERE IT USUALLY IS NOT.** The
//! register knows who is open and where; nobody asked it at the one moment the
//! answer changes what a reader does with `git status`. And «where» is the
//! repository: a peer in another worktree writes into the same history.

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

/// A machine where no directory belongs to any repository: every comparison
/// falls back to the path, which is what the register used to do always.
fn by_path(_: &str) -> Option<String> {
    None
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

    let seen: Vec<&str> = others_in_the_tree(&rows, "ttys001", "/un-albero", &by_path)
        .into_iter()
        .map(|row| row.tty.as_str())
        .collect();
    assert_eq!(seen, vec!["ttys002"]);

    // The absurd control: alone is an empty list, not everybody. Without this
    // arm the function could answer «all the open ones» and pass.
    let alone = vec![a_terminal("ttys001", "/un-albero", None)];
    assert!(others_in_the_tree(&alone, "ttys001", "/un-albero", &by_path).is_empty());
}

/// **WHAT IT ANSWERS IS THE REGISTER, NOT WHAT IS ALIVE.** A terminal killed
/// without closing stays in this list, and whoever prints it says so.
#[test]
fn a_terminal_killed_without_closing_is_still_listed() {
    let rows = vec![
        a_terminal("ttys001", "/un-albero", None),
        a_terminal("ttys009", "/un-albero", None),
    ];
    assert_eq!(others_in_the_tree(&rows, "ttys001", "/un-albero", &by_path).len(), 1);
}

/// **A PEER IN ANOTHER WORKTREE IS NOT SOMEBODY ELSE.** On this machine the
/// register anchored a peer under a worktree while the work landed in the
/// main checkout, and the greeting said nobody was there. Two directories,
/// one repository, one answer.
#[test]
fn another_worktree_of_the_same_repository_is_the_same_work() {
    let rows = vec![
        a_terminal("ttys001", "/un-albero", None),
        a_terminal("ttys010", "/un-albero-tagliato", None),
        a_terminal("ttys020", "/una-casa", None),
    ];
    let repository_of = |path: &str| match path {
        "/un-albero" | "/un-albero-tagliato" => Some("/un-albero/.git".to_owned()),
        "/una-casa" => Some("/una-casa/.git".to_owned()),
        _ => None,
    };

    let seen: Vec<&str> = others_in_the_tree(&rows, "ttys001", "/un-albero", &repository_of)
        .into_iter()
        .map(|row| row.tty.as_str())
        .collect();
    assert_eq!(seen, vec!["ttys010"], "the peer of the other worktree");

    // THE ABSURD CONTROL: a repository nobody can name is not everybody's.
    // Without it the function could answer «all the open ones» and pass.
    let unknown = |path: &str| (path == "/un-albero").then(|| "/un-albero/.git".to_owned());
    let seen: Vec<&str> = others_in_the_tree(&rows, "ttys001", "/un-albero", &unknown)
        .into_iter()
        .map(|row| row.tty.as_str())
        .collect();
    assert!(seen.is_empty(), "a path git says nothing about was taken for mine: {seen:?}");
}
