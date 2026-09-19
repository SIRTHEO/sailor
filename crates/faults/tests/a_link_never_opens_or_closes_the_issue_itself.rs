//! Linking a fault to a GitHub issue only writes down that a person already
//! made the link. Nothing in this store opens, closes, edits or comments on
//! an issue — the same boundary `set_public_summary` already holds for
//! whether a sentence gets published.

use faults::{Draft, Faults};
use std::path::PathBuf;

fn scratch(label: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "faults-link-{label}-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0)
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("the scratch directory");
    dir.join(faults::FAULTS_FILE)
}

fn a_draft() -> Draft {
    Draft {
        happened_on: "16/09".to_owned(),
        what_happened: "a place that never declared itself was still proposed".to_owned(),
        how_it_showed: "by reading the code".to_owned(),
        what_would_prevent: "a check that a declaration exists first".to_owned(),
        status: "**open**".to_owned(),
        standing: None,
    }
}

#[test]
fn a_fresh_fault_carries_no_link() {
    let store = Faults::open(scratch("fresh")).expect("opening a new store");
    let fault = store.record(&a_draft()).expect("recording it");
    assert_eq!(fault.github_issue, None, "nothing was linked yet");
}

#[test]
fn linking_records_the_number_and_url_and_nothing_else_changes() {
    let store = Faults::open(scratch("link")).expect("opening a new store");
    let fault = store.record(&a_draft()).expect("recording it");

    let linked = store
        .link(fault.number, 172, "https://github.com/SIRTHEO/sailor/issues/172")
        .expect("linking a real fault");

    let issue = linked.github_issue.as_ref().expect("the link must be there");
    assert_eq!(issue.number, 172);
    assert_eq!(issue.url, "https://github.com/SIRTHEO/sailor/issues/172");
    assert_eq!(linked.what_happened, fault.what_happened, "linking touched the prose");
    assert_eq!(linked.status, fault.status, "linking touched the status");
}

#[test]
fn linking_twice_replaces_the_earlier_link() {
    let store = Faults::open(scratch("relink")).expect("opening a new store");
    let fault = store.record(&a_draft()).expect("recording it");
    store.link(fault.number, 1, "https://github.com/SIRTHEO/sailor/issues/1").expect("first link");

    let relinked = store
        .link(fault.number, 2, "https://github.com/SIRTHEO/sailor/issues/2")
        .expect("relinking after the first issue turned out to be a duplicate");

    assert_eq!(relinked.github_issue.expect("still linked").number, 2);
}

#[test]
fn unlinking_removes_it_without_touching_the_issue() {
    let store = Faults::open(scratch("unlink")).expect("opening a new store");
    let fault = store.record(&a_draft()).expect("recording it");
    store.link(fault.number, 1, "https://github.com/SIRTHEO/sailor/issues/1").expect("linking");

    let unlinked = store.unlink(fault.number).expect("unlinking");
    assert_eq!(unlinked.github_issue, None);
}

#[test]
fn an_issue_number_of_zero_or_less_is_refused() {
    let store = Faults::open(scratch("zero")).expect("opening a new store");
    let fault = store.record(&a_draft()).expect("recording it");
    assert!(
        store.link(fault.number, 0, "https://github.com/SIRTHEO/sailor/issues/0").is_err(),
        "issue 0 names nothing on GitHub"
    );
    assert!(store.link(fault.number, -1, "https://github.com/x/y/issues/-1").is_err());
}

#[test]
fn an_empty_url_is_refused() {
    let store = Faults::open(scratch("empty-url")).expect("opening a new store");
    let fault = store.record(&a_draft()).expect("recording it");
    assert!(store.link(fault.number, 1, "").is_err(), "a link with no address points nowhere");
}

#[test]
fn linking_an_unknown_fault_names_the_number() {
    let store = Faults::open(scratch("unknown")).expect("opening a new store");
    let error = store
        .link(9999, 1, "https://github.com/SIRTHEO/sailor/issues/1")
        .expect_err("no fault 9999 was ever recorded");
    assert!(format!("{error}").contains("9999"), "the error must name the fault it could not find");
}

#[test]
fn a_store_from_before_linking_reads_with_none_and_gains_it_when_opened() {
    let path = scratch("before-linking");
    let store = Faults::open(&path).expect("creating it fresh, with the table");
    let fault = store.record(&a_draft()).expect("recording a fault");
    drop(store);

    let connection = rusqlite::Connection::open(&path).expect("reopening by hand");
    connection
        .execute_batch("DROP TABLE github_issues;")
        .expect("simulating a store written by a binary that predates linking");
    drop(connection);

    let read = Faults::open_for_reading(&path)
        .expect("a store without the table still opens for reading")
        .get(fault.number)
        .expect("and reads the fault");
    assert_eq!(read.github_issue, None, "a link was read out of a store that has none");

    let store = Faults::open(&path).expect("opening for writing recreates the table");
    let linked = store
        .link(fault.number, 1, "https://github.com/SIRTHEO/sailor/issues/1")
        .expect("the table exists once the store is opened for writing");
    assert!(linked.github_issue.is_some());
}
