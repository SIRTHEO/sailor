//! A review is recorded only when its verdict names the pinned commit, says
//! clean or findings, and lists what it checked, and only when the sailor it
//! was closed with is the one the flow verified.

use actions::review_verdict::{what_the_verdict_binds, Bound, Refusal};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::path::PathBuf;

const PINNED: &str = "c0ffee";

fn clean() -> Value {
    json!({ "commit": PINNED, "verdict": "clean", "findings": [], "checked": ["the gates"] })
}

#[test]
fn a_clean_verdict_on_the_pinned_commit_binds_its_counts() {
    assert_eq!(
        what_the_verdict_binds(&clean(), PINNED),
        Ok(Bound {
            commit: PINNED.to_owned(),
            verdict: "clean".to_owned(),
            findings: 0,
            checked: 1,
        })
    );
}

/// A reviewer's file that holds a JSON string is a string, whatever the string
/// spells: only an object is a verdict.
#[test]
fn a_verdict_handed_in_as_text_is_not_taken() {
    let text = Value::String(clean().to_string());
    assert_eq!(
        what_the_verdict_binds(&text, PINNED),
        Err(Refusal::NotAnObject)
    );
}

#[test]
fn a_verdict_on_another_commit_is_not_taken() {
    let mut verdict = clean();
    verdict["commit"] = json!("beef00");
    assert_eq!(
        what_the_verdict_binds(&verdict, PINNED),
        Err(Refusal::OtherCommit(Some("beef00".to_owned())))
    );
    verdict["commit"] = json!(7);
    assert_eq!(
        what_the_verdict_binds(&verdict, PINNED),
        Err(Refusal::OtherCommit(None))
    );
}

#[test]
fn a_verdict_is_clean_or_findings_and_nothing_else() {
    let mut verdict = clean();
    verdict["verdict"] = json!("fine");
    assert_eq!(
        what_the_verdict_binds(&verdict, PINNED),
        Err(Refusal::NeitherCleanNorFindings)
    );
}

#[test]
fn findings_name_at_least_one() {
    let mut verdict = clean();
    verdict["verdict"] = json!("findings");
    assert_eq!(
        what_the_verdict_binds(&verdict, PINNED),
        Err(Refusal::FindingsNamedNone)
    );
    verdict["findings"] = json!(["a line that reads the wrong remote"]);
    assert_eq!(
        what_the_verdict_binds(&verdict, PINNED).map(|bound| bound.findings),
        Ok(1)
    );
}

#[test]
fn a_verdict_that_checked_nothing_is_not_taken() {
    let mut verdict = clean();
    verdict
        .as_object_mut()
        .expect("an object")
        .remove("checked");
    assert_eq!(
        what_the_verdict_binds(&verdict, PINNED),
        Err(Refusal::NothingChecked)
    );
}

#[test]
fn findings_and_checked_are_lists() {
    let mut verdict = clean();
    verdict["checked"] = json!("the gates");
    assert_eq!(
        what_the_verdict_binds(&verdict, PINNED),
        Err(Refusal::NotLists)
    );
}

#[test]
fn a_verdict_that_is_no_object_is_not_taken() {
    assert_eq!(
        what_the_verdict_binds(&json!(["clean"]), PINNED),
        Err(Refusal::NotAnObject)
    );
    assert_eq!(
        what_the_verdict_binds(&json!("not json"), PINNED),
        Err(Refusal::NotAnObject)
    );
}

/// A stand-in sailor in the temporary directory, gone when the case ends.
struct Sailor(PathBuf);

impl Drop for Sailor {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}

fn a_sailor(name: &str) -> (Sailor, String) {
    let at = std::env::temp_dir().join(format!("verdict-{name}-{}", std::process::id()));
    std::fs::write(&at, b"a binary").expect("the stand-in sailor");
    (Sailor(at), format!("{:x}", Sha256::digest(b"a binary")))
}

fn the_step(input: Value) -> Result<Value, (String, String)> {
    let mut registry = flow::ActionRegistry::default();
    actions::review_verdict::register_review_verdict(&mut registry);
    let action = registry
        .get("review_verdict")
        .expect("the action is registered");
    match action.execute(&input, &flow::SharedState::default()) {
        Ok(flow::ActionOutcome::Went(said)) => Ok(said),
        Ok(other) => Err(("not_went".to_owned(), format!("{other:?}"))),
        Err(refusal) => Err((refusal.class, refusal.said)),
    }
}

#[test]
fn the_step_answers_with_the_counts_the_record_keeps() {
    let (sailor, sha256) = a_sailor("went");
    assert_eq!(
        the_step(
            json!({ "verdict": clean(), "commit": PINNED, "sailor": sailor.0, "sha256": sha256 })
        ),
        Ok(json!({ "commit": PINNED, "verdict": "clean", "findings": 0, "checked": 1 }))
    );
}

#[test]
fn a_sailor_that_changed_while_the_review_was_open_takes_no_verdict() {
    let (sailor, sha256) = a_sailor("changed");
    std::fs::write(&sailor.0, b"another binary").expect("the sailor changes");
    let (class, said) = the_step(
        json!({ "verdict": clean(), "commit": PINNED, "sailor": sailor.0, "sha256": sha256 }),
    )
    .unwrap_err();
    assert_eq!(class, "not_in_service");
    assert!(said.contains("changed while the review was open"), "{said}");
}

#[test]
fn a_verdict_the_step_refuses_says_why() {
    let (sailor, sha256) = a_sailor("refused");
    let mut verdict = clean();
    verdict["commit"] = json!("beef00");
    let (class, said) = the_step(
        json!({ "verdict": verdict, "commit": PINNED, "sailor": sailor.0, "sha256": sha256 }),
    )
    .unwrap_err();
    assert_eq!(class, "the_verdict_is_not_bound");
    assert_eq!(
        said,
        "the verdict names commit beef00, not the pinned c0ffee"
    );
}
