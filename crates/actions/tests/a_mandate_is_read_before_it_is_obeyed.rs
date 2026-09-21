//! What `delivery_request` refuses, proved by making each refusal happen.
//!
//! This replaced 2.593 characters of shell standing in three flow files, with a
//! fourth copy differing by four lines. Nothing ever ran a case against it: a
//! mandate naming `bran` for `branch` delivered something nobody asked for.

use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::process::Command;

fn scratch(name: &str) -> PathBuf {
    let at = std::env::temp_dir().join(format!("mandate-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&at);
    std::fs::create_dir_all(&at).expect("the scratch directory");
    at
}

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

/// The trunk and the remote are names the repository chooses, so the case
/// chooses ones nobody would guess: a product that assumed them would pass on
/// the usual pair.
fn a_repository(at: &Path) {
    git(at, &["init", "-q", "-b", "delivery-line"]);
    git(at, &["config", "sailor.trunk", "delivery-line"]);
    git(at, &["config", "user.email", "delivery@example"]);
    git(at, &["config", "user.name", "delivery"]);
    std::fs::create_dir_all(at.join(".sailor")).expect("the policy directory");
    std::fs::write(
        at.join(".sailor/delivery-policy.json"),
        r#"{"merge":"auto","push":"auto","release":"ask","remote":"far-side"}"#,
    )
    .expect("the policy");
    git(at, &["add", ".sailor/delivery-policy.json"]);
    git(at, &["commit", "-q", "-m", "the policy"]);
}

fn the_mandate(at: &Path, text: Value) -> Result<Value, String> {
    reading(json!({
        "mandate": text.to_string(),
        "required": ["repo", "branch", "title"],
        "optional": { "base": "", "forge_repo": "", "issue": "", "no_issue": "", "version": "" },
        "exactly_one_of": ["issue", "no_issue"],
    }))
    .map_err(|why| format!("{why} (for a repo at {})", at.display()))
}

fn reading(input: Value) -> Result<Value, String> {
    let mut registry = flow::ActionRegistry::default();
    actions::delivery::register_delivery(&mut registry);
    let action = registry.get("delivery_request").expect("registered");
    match action.execute(&input, &flow::SharedState::default()) {
        Ok(flow::ActionOutcome::Went(said)) => Ok(said),
        Ok(other) => Err(format!("{other:?}")),
        Err(refusal) => Err(refusal.said),
    }
}

fn a_whole_mandate(at: &Path) -> Value {
    json!({ "repo": at, "branch": "work/a-change", "title": "a change", "no_issue": "none is open" })
}

#[test]
fn the_trunk_and_the_remote_are_read_out_of_the_repository_rather_than_assumed() {
    let at = scratch("whole");
    a_repository(&at);

    let said = the_mandate(&at, a_whole_mandate(&at)).expect("a whole mandate reads");

    assert_eq!(said["trunk"], "delivery-line");
    assert_eq!(said["remote"], "far-side");
    assert_eq!(said["base"], "delivery-line", "an unsaid base is the trunk");
    assert_eq!(
        said["issue"], "",
        "an unsaid field is its default, not absent"
    );
}

#[test]
fn a_base_the_mandate_names_is_kept_over_the_trunk() {
    let at = scratch("base");
    a_repository(&at);
    let mut asked = a_whole_mandate(&at);
    asked["base"] = json!("work/an-earlier-change");

    let said = the_mandate(&at, asked).expect("a named base reads");

    assert_eq!(said["base"], "work/an-earlier-change");
}

#[test]
fn a_mandate_that_is_not_one_json_object_is_refused_rather_than_half_read() {
    let at = scratch("notjson");
    a_repository(&at);

    for text in ["branch: work/a-change", "[\"work/a-change\"]", ""] {
        let why = reading(json!({
            "mandate": text,
            "required": ["repo"],
        }))
        .expect_err("that is not a mandate");
        assert!(why.contains("JSON object"), "for {text:?}: {why}");
    }
}

#[test]
fn a_field_the_mandate_does_not_name_is_named_back_rather_than_left_empty() {
    let at = scratch("missing");
    a_repository(&at);
    let mut asked = a_whole_mandate(&at);
    asked["branch"] = json!("   ");

    let why = the_mandate(&at, asked).expect_err("a blank field is a missing one");

    assert!(why.contains("does not name"), "{why}");
    assert!(
        why.contains("branch"),
        "the refusal must name the field: {why}"
    );
}

/// **A MISSPELLED FIELD IS A MANDATE NOBODY GAVE.** `bran` for `branch` reads
/// as a mandate with no branch and one word too many, and the flow would
/// deliver whatever the default was.
#[test]
fn a_field_the_flow_does_not_read_stops_the_run_rather_than_being_ignored() {
    let at = scratch("unknown");
    a_repository(&at);
    let mut asked = a_whole_mandate(&at);
    asked["bran"] = json!("work/a-change");

    let why = the_mandate(&at, asked).expect_err("an unread field is a misspelled one");

    assert!(why.contains("bran"), "{why}");
}

/// A branch name reaches git as an argument: one that starts with a hyphen
/// reaches it as an option instead.
#[test]
fn a_branch_that_is_not_a_branch_name_is_refused_before_it_reaches_git() {
    let at = scratch("branch");
    a_repository(&at);

    for named in [
        "--upload-pack=say-anything",
        "work/a change",
        "$(say-anything)",
    ] {
        let mut asked = a_whole_mandate(&at);
        asked["branch"] = json!(named);
        let why = the_mandate(&at, asked).expect_err("that is not a branch name");
        assert!(why.contains("plain branch name"), "for {named}: {why}");
    }
}

#[test]
fn an_issue_a_version_and_a_forge_repo_are_refused_when_they_are_not_shaped_like_one() {
    let at = scratch("shapes");
    a_repository(&at);

    let off_shape = [
        ("issue", "eighty-seven", "is not a number"),
        ("version", "1.0", "MAJOR.MINOR.PATCH"),
        ("version", "one.nought.nought", "MAJOR.MINOR.PATCH"),
        ("forge_repo", "a-name-alone", "owner/name"),
        ("forge_repo", "an/owner/and-a-name", "owner/name"),
    ];
    for (field, said, complaint) in off_shape {
        let mut asked = a_whole_mandate(&at);
        asked.as_object_mut().expect("an object").remove("no_issue");
        asked["issue"] = json!("87");
        asked[field] = json!(said);
        let why = the_mandate(&at, asked).expect_err("that is off shape");
        assert!(why.contains(complaint), "for {field}={said}: {why}");
    }
}

#[test]
fn the_shapes_a_mandate_may_have_are_accepted() {
    let at = scratch("accepted");
    a_repository(&at);
    let mut asked = a_whole_mandate(&at);
    asked.as_object_mut().expect("an object").remove("no_issue");
    asked["issue"] = json!("87");
    asked["version"] = json!("1.0.0-rc.2");
    asked["forge_repo"] = json!("an-owner/a-name");

    let said = the_mandate(&at, asked).expect("these are the shapes");

    assert_eq!(said["issue"], "87");
    assert_eq!(said["version"], "1.0.0-rc.2");
    assert_eq!(said["forge_repo"], "an-owner/a-name");
}

/// Either the change closes an issue or it says why none is open: saying both
/// is a mandate copied from another one and half edited.
#[test]
fn naming_both_an_issue_and_a_reason_there_is_none_is_refused_as_is_naming_neither() {
    let at = scratch("oneof");
    a_repository(&at);

    let mut both = a_whole_mandate(&at);
    both["issue"] = json!("87");
    let why = the_mandate(&at, both).expect_err("it is one or the other");
    assert!(why.contains("more than one of issue or no_issue"), "{why}");

    let mut neither = a_whole_mandate(&at);
    neither
        .as_object_mut()
        .expect("an object")
        .remove("no_issue");
    let why = the_mandate(&at, neither).expect_err("it is one or the other");
    assert!(why.contains("none of issue or no_issue"), "{why}");
}

/// A person writing a mandate by hand has no reason to know whether the flow
/// wants `87` or `"87"`.
#[test]
fn an_issue_written_as_a_number_is_the_same_mandate_as_one_written_as_its_text() {
    let at = scratch("number");
    a_repository(&at);
    let mut asked = a_whole_mandate(&at);
    asked.as_object_mut().expect("an object").remove("no_issue");
    asked["issue"] = json!(87);

    let said = the_mandate(&at, asked).expect("a number is a word too");

    assert_eq!(said["issue"], "87");
}

#[test]
fn a_repo_that_is_not_an_absolute_path_is_refused_rather_than_resolved_from_wherever_this_runs() {
    let why = reading(json!({
        "mandate": json!({ "repo": "a/relative/way" }).to_string(),
        "required": ["repo"],
    }))
    .expect_err("a relative path means a different tree to every caller");

    assert!(why.contains("absolute path"), "{why}");
}

#[test]
fn a_repository_whose_trunk_carries_no_policy_is_refused_by_name() {
    let at = scratch("nopolicy");
    git(&at, &["init", "-q", "-b", "delivery-line"]);
    git(&at, &["config", "sailor.trunk", "delivery-line"]);
    git(&at, &["config", "user.email", "delivery@example"]);
    git(&at, &["config", "user.name", "delivery"]);
    std::fs::write(at.join("readme"), "no policy here\n").expect("a first file");
    git(&at, &["add", "readme"]);
    git(&at, &["commit", "-q", "-m", "first"]);

    let why = reading(json!({
        "mandate": json!({ "repo": &at }).to_string(),
        "required": ["repo"],
    }))
    .expect_err("there is no policy to read");

    assert!(why.contains("not readable from delivery-line"), "{why}");
}

#[test]
fn a_field_the_action_does_not_read_is_named_rather_than_ignored() {
    let mut registry = flow::ActionRegistry::default();
    actions::delivery::register_delivery(&mut registry);

    let unknown = registry
        .get("delivery_request")
        .expect("registered")
        .unknown_fields(&json!({ "mandate": "{}", "required": [], "text": "{}" }));

    assert_eq!(unknown, vec!["text".to_owned()]);
}
