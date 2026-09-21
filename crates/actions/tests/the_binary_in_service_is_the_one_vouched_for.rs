//! What `sailor_in_service` refuses, proved by making each refusal happen.
//!
//! This replaced 1.416 characters of shell standing in three flow files, and
//! nothing ever ran a case against it. Two of the eight refusals below had
//! never been seen: a stamp holding only comments, and a recorded digest of
//! nothing, which the shell reached by way of an empty variable.

use serde_json::{json, Value};
use std::path::{Path, PathBuf};

const A_BINARY: &[u8] = b"#!/bin/sh\nexit 0\n";

fn scratch(name: &str) -> PathBuf {
    let at = std::env::temp_dir().join(format!("in-service-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&at);
    std::fs::create_dir_all(at.join("bin")).expect("the bin directory");
    std::fs::create_dir_all(at.join("state")).expect("the state directory");
    at
}

fn put_in_service(home: &Path, bytes: &[u8]) {
    let binary = home.join("bin/sailor");
    std::fs::write(&binary, bytes).expect("the binary");
    make_runnable(&binary);
}

#[cfg(unix)]
fn make_runnable(path: &Path) {
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755)).expect("runnable");
}

#[cfg(not(unix))]
fn make_runnable(_path: &Path) {}

fn digest_of(bytes: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    format!("{:x}", Sha256::digest(bytes))
}

fn stamp(home: &Path, text: &str) {
    std::fs::write(home.join("state/sailor-binary-commit"), text).expect("the stamp");
}

fn vouch(home: &Path, text: &str) {
    std::fs::write(home.join("state/sailor-binary-commit.sha256"), text).expect("the digest");
}

/// A home holding a binary, its stamp, and a digest that matches it.
fn a_home_in_order(name: &str) -> PathBuf {
    let home = scratch(name);
    put_in_service(&home, A_BINARY);
    stamp(&home, "e465f09d83fbeb0ff3669eca76ef40b8a13dabd2\n");
    vouch(&home, &format!("{}\n", digest_of(A_BINARY)));
    home
}

fn reading(input: Value) -> Result<Value, String> {
    let mut registry = flow::ActionRegistry::default();
    actions::in_service::register_in_service(&mut registry);
    let action = registry.get("sailor_in_service").expect("registered");
    match action.execute(&input, &flow::SharedState::default()) {
        Ok(flow::ActionOutcome::Went(said)) => Ok(said),
        Ok(other) => Err(format!("{other:?}")),
        Err(refusal) => Err(refusal.said),
    }
}

fn the_home(home: &Path) -> Result<Value, String> {
    reading(json!({ "home": home }))
}

#[test]
fn a_binary_whose_digest_was_recorded_is_the_one_in_service() {
    let home = a_home_in_order("whole");

    let said = the_home(&home).expect("a home in order reads");

    assert_eq!(
        said["sailor"],
        json!(home.join("bin/sailor").to_string_lossy())
    );
    assert_eq!(said["commit"], "e465f09d83fbeb0ff3669eca76ef40b8a13dabd2");
    assert_eq!(said["sha256"], digest_of(A_BINARY));
}

#[test]
fn a_home_with_no_binary_is_refused() {
    let home = scratch("empty");

    let why = the_home(&home).expect_err("nothing is in service");

    assert!(why.contains("no sailor in service"), "{why}");
}

/// **A FILE IS NOT A BINARY.** The shell asked `[ -x ]` and this has to ask the
/// same: a file sitting there unrunnable is nothing in service either.
#[cfg(unix)]
#[test]
fn a_file_that_cannot_be_run_is_not_in_service() {
    use std::os::unix::fs::PermissionsExt;
    let home = a_home_in_order("unrunnable");
    std::fs::set_permissions(
        home.join("bin/sailor"),
        std::fs::Permissions::from_mode(0o644),
    )
    .expect("unrunnable");

    let why = the_home(&home).expect_err("an unrunnable file is not in service");

    assert!(why.contains("no sailor in service"), "{why}");
}

#[test]
fn a_binary_with_no_stamp_names_no_commit_it_was_built_from() {
    let home = a_home_in_order("unstamped");
    std::fs::remove_file(home.join("state/sailor-binary-commit")).expect("the stamp goes");

    let why = the_home(&home).expect_err("an unstamped binary is refused");

    assert!(why.contains("no stamp naming the commit"), "{why}");
}

/// A stamp of blank lines and comments reads as a stamp and names nothing: the
/// first reading of the file answers the empty string, and an empty commit
/// travels into whatever the flow does next.
#[test]
fn a_stamp_holding_only_comments_names_no_commit() {
    let home = a_home_in_order("commented");
    stamp(&home, "# written by nobody\n\n   \n");

    let why = the_home(&home).expect_err("a stamp of comments is refused");

    assert!(why.contains("names no commit"), "{why}");
}

#[test]
fn a_binary_nobody_recorded_a_digest_for_is_refused_with_the_command_that_records_it() {
    let home = a_home_in_order("unvouched");
    std::fs::remove_file(home.join("state/sailor-binary-commit.sha256")).expect("the digest goes");

    let why = the_home(&home).expect_err("an unvouched binary is refused");

    assert!(why.contains("nothing vouches for it"), "{why}");
    assert!(
        why.contains("shasum -a 256") && why.contains("sailor-binary-commit.sha256"),
        "the refusal names the command a person runs to vouch for it: {why}"
    );
}

#[test]
fn a_digest_that_does_not_match_the_binary_stops_the_flow() {
    let home = a_home_in_order("moved");
    put_in_service(&home, b"#!/bin/sh\nexit 1\n");

    let why = the_home(&home).expect_err("a binary nobody vouched for is refused");

    assert!(
        why.contains("not the binary that was put into service"),
        "{why}"
    );
    assert!(
        why.contains(&digest_of(b"#!/bin/sh\nexit 1\n")),
        "the refusal names the digest it found: {why}"
    );
}

/// An empty file is not a recorded digest, and the refusal says so by name
/// rather than comparing against the empty string.
#[test]
fn a_recorded_digest_of_nothing_is_named_nothing() {
    let home = a_home_in_order("blank");
    vouch(&home, "\n# nobody wrote one\n");

    let why = the_home(&home).expect_err("an empty record is refused");

    assert!(why.contains("and nothing is recorded for it"), "{why}");
}

/// The stamp a release writes may carry comment lines above the commit, and the
/// first word of the first line that is neither blank nor a comment is it.
#[test]
fn the_commit_is_the_first_word_that_is_neither_blank_nor_a_comment() {
    let home = a_home_in_order("verbose");
    stamp(
        &home,
        "\n# built from\ne465f09d83fbeb0ff3669eca76ef40b8a13dabd2 and more\nanother line\n",
    );

    let said = the_home(&home).expect("a stamp with comments above it reads");

    assert_eq!(said["commit"], "e465f09d83fbeb0ff3669eca76ef40b8a13dabd2");
}

#[test]
fn a_field_the_action_does_not_read_is_named() {
    let mut registry = flow::ActionRegistry::default();
    actions::in_service::register_in_service(&mut registry);
    let action = registry.get("sailor_in_service").expect("registered");

    let unknown = action.unknown_fields(&json!({ "home": "/somewhere", "binary": "/elsewhere" }));

    assert_eq!(unknown, vec!["binary".to_owned()]);
}
