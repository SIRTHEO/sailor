//! What the two delivery actions refuse, proved by making each refusal happen.
//!
//! These replaced shell that no case ever ran: the point of the move is that
//! the refusals are reachable from a test at all.

use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::process::Command;
use workspace::delivery::{policy_on_the_trunk, read_policy};

fn scratch(name: &str) -> PathBuf {
    let at = std::env::temp_dir().join(format!("delivery-{name}-{}", std::process::id()));
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

/// The trunk is a name the repository chooses, so the case chooses one nobody
/// would guess: a product that assumed a name would pass on the usual one.
fn a_repository_whose_trunk_is(named: &str, at: &Path, policy: Option<&str>) {
    git(at, &["init", "-q", "-b", named]);
    git(at, &["config", "sailor.trunk", named]);
    git(at, &["config", "user.email", "delivery@example"]);
    git(at, &["config", "user.name", "delivery"]);
    std::fs::write(at.join("readme"), "the repository this case builds\n").expect("a first file");
    git(at, &["add", "readme"]);
    git(at, &["commit", "-q", "-m", "first"]);
    if let Some(text) = policy {
        std::fs::create_dir_all(at.join(".sailor")).expect("the policy directory");
        std::fs::write(at.join(".sailor/delivery-policy.json"), text).expect("the policy");
        git(at, &[
            "add",
            ".sailor/delivery-policy.json",
        ]);
        git(at, &["commit", "-q", "-m", "the policy"]);
    }
}

fn the_action(name: &str, input: Value) -> Result<Value, String> {
    let mut registry = flow::ActionRegistry::default();
    actions::delivery::register_delivery(&mut registry);
    let action = registry.get(name).expect("the action is registered");
    match action.execute(&input, &flow::SharedState::default()) {
        Ok(flow::ActionOutcome::Went(said)) => Ok(said),
        Ok(other) => Err(format!("{other:?}")),
        Err(refusal) => Err(refusal.said),
    }
}

#[test]
fn the_policy_is_read_out_of_the_commit_the_declared_trunk_points_at() {
    let at = scratch("trusted");
    a_repository_whose_trunk_is(
        "delivery-line",
        &at,
        Some(r#"{"merge": "auto", "push": "ask", "release": "ask"}"#),
    );
    // The working tree says the opposite of the commit: a branch under review
    // must never be able to authorize itself.
    std::fs::write(
        at.join(".sailor/delivery-policy.json"),
        r#"{"merge": "auto", "push": "auto", "release": "auto"}"#,
    )
    .expect("the working tree's own policy");

    let said = the_action("delivery_policy", json!({ "repo": at })).expect("the policy reads");

    assert_eq!(said["push"], "ask", "the working tree was read");
    assert_eq!(said["asks_publication"], "ask");
    assert_eq!(said["asks_integration"], "ask", "an integration pushes");
    assert_eq!(said["asks_release"], "ask");
    assert!(
        said["read_from"].as_str().is_some_and(|at| at.len() == 40),
        "the commit it was read from is not named: {said}"
    );
}

#[test]
fn a_repository_that_declares_no_trunk_is_refused_rather_than_guessed_at() {
    let at = scratch("undeclared");
    a_repository_whose_trunk_is("delivery-line", &at, Some(r#"{"merge":"auto","push":"auto","release":"auto"}"#));
    git(&at, &["config", "--unset", "sailor.trunk"]);

    let why = policy_on_the_trunk(&at).expect_err("nothing may be assumed");

    assert_eq!(why, workspace::delivery::Refusal::NoTrunkDeclared);
}

#[test]
fn a_trunk_carrying_no_policy_is_refused_by_name() {
    let at = scratch("nopolicy");
    a_repository_whose_trunk_is("delivery-line", &at, None);

    let why = policy_on_the_trunk(&at).expect_err("there is no policy to read");

    assert_eq!(why, workspace::delivery::Refusal::NoPolicyOnTrunk);
}

#[test]
fn a_word_that_is_neither_auto_nor_ask_is_refused_by_the_field_it_stands_in() {
    let why = read_policy(
        r#"{"merge": "auto", "push": "sometimes", "release": "auto"}"#,
        "a-commit".to_owned(),
    )
    .expect_err("only two words are readable");

    assert_eq!(
        why,
        workspace::delivery::Refusal::NotAutoNorAsk {
            field: "push".to_owned(),
            found: "sometimes".to_owned(),
        },
        "the refusal must name the field and what stood in it"
    );
}

#[test]
fn a_policy_that_is_not_json_is_refused_rather_than_read_as_the_lenient_side() {
    let why = read_policy("merge: auto", "a-commit".to_owned()).expect_err("that is not JSON");

    assert_eq!(why, workspace::delivery::Refusal::NoPolicyOnTrunk);
}

#[test]
fn asking_on_the_push_alone_makes_every_outward_gesture_ask() {
    let read = read_policy(
        r#"{"merge": "auto", "push": "ask", "release": "auto"}"#,
        "a-commit".to_owned(),
    )
    .expect("the policy reads");

    assert_eq!(
        (
            read.asks_publication.as_str(),
            read.asks_integration.as_str(),
            read.asks_release.as_str()
        ),
        ("ask", "ask", "ask"),
        "an integration and a release both push"
    );
}

#[test]
fn the_word_a_person_writes_back_is_true_or_the_flow_stops() {
    let said = the_action(
        "the_person_said",
        json!({ "said": { "authorized": true }, "key": "authorized" }),
    )
    .expect("the person said yes");
    assert_eq!(said["confirmed"], true);

    for refused in [
        json!({ "authorized": false }),
        json!({ "authorized": "true" }),
        json!({ "something_else": true }),
        json!("the person wrote prose"),
        Value::Null,
    ] {
        let why = the_action(
            "the_person_said",
            json!({ "said": refused.clone(), "key": "authorized" }),
        )
        .expect_err("nothing but true proceeds");
        assert!(why.contains("authorized"), "for {refused}: {why}");
    }
}

/// The answer reaches the step as the value or as its text, and the flows this
/// replaced hand on the text: both spellings mean the same yes.
#[test]
fn the_answer_is_read_whether_it_arrives_as_a_value_or_as_its_text() {
    let said = the_action(
        "the_person_said",
        json!({ "said": "{\"authorized\": true}", "key": "authorized" }),
    )
    .expect("the text is read");

    assert_eq!(said["confirmed"], true);
}

#[test]
fn a_field_the_action_does_not_read_is_named_rather_than_ignored() {
    let mut registry = flow::ActionRegistry::default();
    actions::delivery::register_delivery(&mut registry);

    let unknown = registry
        .get("delivery_policy")
        .expect("registered")
        .unknown_fields(&json!({ "repo": "/somewhere", "branch": "a-branch" }));

    assert_eq!(unknown, vec!["branch".to_owned()]);
}
