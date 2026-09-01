//! The payload gives what it has, and what it lacks stops nothing.
//!
//! **WHOEVER ARRIVES MUST NOT HAVE TO KNOW WHAT WE NEED.** A hook sends its own
//! JSON, written by someone else and free to change without warning. If one
//! extra field made it refuse, every new version of the program writing that
//! JSON would switch tracking off.

//! That is fault 8, which was recorded about descriptors and holds identically
//! here. And if a missing field made it fail, nothing but an exact hook could
//! ever be tracked.

use sessions::{anchor_from, Census, Payload};

fn nothing_seen() -> Census {
    Census::NoTerminal
}

#[test]
fn the_four_fields_are_read_when_they_are_there() {
    let payload = Payload::parse(
        r#"{"session_id":"abc","transcript_path":"/tmp/abc.jsonl",
            "cwd":"/somewhere","hook_event_name":"PreToolUse"}"#,
    )
    .expect("this is JSON");
    assert_eq!(payload.session_id.as_deref(), Some("abc"));
    assert_eq!(payload.transcript_path.as_deref(), Some("/tmp/abc.jsonl"));
    assert_eq!(payload.cwd.as_deref(), Some("/somewhere"));
    assert_eq!(payload.hook_event_name.as_deref(), Some("PreToolUse"));
}

/// A field this version does not know is one field too many, not a broken
/// payload.
#[test]
fn a_field_we_do_not_know_is_not_a_reason_to_refuse_the_rest() {
    let payload = Payload::parse(r#"{"session_id":"abc","something_new":{"deep":[1,2]}}"#)
        .expect("an unknown field must not get the payload refused");
    assert_eq!(payload.session_id.as_deref(), Some("abc"));
}

/// Nothing on standard input is the case of whoever runs the command by hand:
/// they still have a tty and a directory.
#[test]
fn nothing_at_all_is_an_empty_payload_and_not_an_error() {
    assert_eq!(Payload::parse("").expect("empty"), Payload::default());
    assert_eq!(
        Payload::parse("   \n ").expect("whitespace only"),
        Payload::default()
    );
}

/// Text that is not JSON is said out loud instead: it is a badly written hook,
/// and keeping quiet would leave it badly written forever.
#[test]
fn something_that_is_not_json_is_said_out_loud() {
    let complaint = Payload::parse("not json at all").expect_err("this is not JSON");
    assert!(complaint.contains("JSON"), "{complaint}");
}

/// The worktree falls back to the current directory when the payload does not
/// say, and the ancestor stays unknown when the census does not know it:
/// **unknown is not empty**.
#[test]
fn the_anchor_falls_back_without_inventing_anything() {
    let payload = Payload::default();
    let anchor = anchor_from(&payload, "ttys004".to_owned(), &nothing_seen());
    assert_eq!(anchor.tty, "ttys004");
    assert!(!anchor.worktree.is_empty());
    assert_eq!(
        anchor.ancestor, None,
        "an ancestor that could not be read stays None, not an empty string"
    );

    let declared = Payload::parse(r#"{"cwd":"/declared"}"#).expect("this is JSON");
    assert_eq!(
        anchor_from(&declared, "ttys004".to_owned(), &nothing_seen()).worktree,
        "/declared"
    );
}

/// The short name and the long name of one terminal are one key.
#[test]
fn the_long_name_of_a_terminal_and_the_short_one_are_one_key() {
    assert_eq!(sessions::tty::short_name("/dev/ttys004"), "ttys004");
    assert_eq!(sessions::tty::short_name("ttys004"), "ttys004");
}

/// The label of whoever drew the window: the name of the host application, not
/// the wrapper deepest in the path.
#[test]
fn the_label_names_the_application_and_not_the_wrapper() {
    assert_eq!(
        sessions::census::label_for(
            "/Applications/Whatever.app/Contents/Frameworks/Whatever Helper.app/Contents/MacOS/Whatever Helper"
        ),
        "Whatever"
    );
    assert_eq!(sessions::census::label_for("/bin/zsh"), "zsh");
    assert_eq!(sessions::census::label_for("caffeinate"), "caffeinate");
}
