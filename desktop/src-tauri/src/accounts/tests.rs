use super::*;

/// **THE COMMAND IS DECLARED, OR THE WINDOW CANNOT CALL IT.** A module that
/// compiles and is never handed to `generate_handler!` answers nothing, and
/// the screen's only symptom is an error string at the far end.
#[test]
fn the_window_can_ask_for_the_accounts() {
    let main = include_str!("../main.rs");
    let declared = &main[main.find("generate_handler![").expect("the declaration")..];
    assert!(
        declared.contains("accounts::accounts,"),
        "accounts::accounts is not declared in main.rs"
    );
}

/// A command line nothing in the catalogue detects still has to draw: the row
/// falls back to the name the profile table carries, and to no brand at all —
/// which the screen turns into a monogram rather than a hole.
#[test]
fn a_command_line_no_descriptor_detects_still_has_a_name() {
    let (label, brand) = drawn_as("no-such-command-line");
    assert_eq!(label, "no-such-command-line");
    assert_eq!(brand, "");
}

/// **`claude` AND `claude-code` ARE THE SAME ENGINE.** The profile table and
/// the descriptors name it differently on purpose, and the join is the
/// executable: get that wrong and every row loses its mark in silence.
#[test]
fn the_profile_table_and_the_descriptors_meet_on_the_executable() {
    let (label, brand) = drawn_as("claude");
    assert_eq!(
        brand, "claudecode",
        "the descriptor's brand should have been found"
    );
    assert!(!label.is_empty());
}
