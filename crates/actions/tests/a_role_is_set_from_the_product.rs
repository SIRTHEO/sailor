//! A role is a row in the ledger that says which tools a step may use, and until
//! now nothing but a test could write one: a flow naming a role with no row
//! failed its check and the person had nowhere to type the answer.

use actions::roles::{list, set, SetRole};
use ledger::Ledger;
use serde_json::json;
use std::sync::atomic::{AtomicUsize, Ordering};

fn fresh_ledger() -> Ledger {
    static NEXT: AtomicUsize = AtomicUsize::new(0);
    let dir = std::env::temp_dir().join(format!(
        "role-set-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::SeqCst)
    ));
    let _ = std::fs::remove_dir_all(&dir);
    Ledger::open(&dir).expect("a scratch ledger")
}

fn declares(id: &str) -> bool {
    matches!(id, "claude-code" | "codex")
}

fn ask(name: &str, tools: &[&str], replace: bool) -> SetRole {
    SetRole {
        name: name.to_owned(),
        tools: tools.iter().map(|tool| (*tool).to_owned()).collect(),
        replace,
    }
}

fn rows(ledger: &Ledger) -> usize {
    ledger.records_in("roles").expect("read roles").len()
}

#[test]
fn a_row_written_here_is_the_row_a_flow_resolves() {
    let ledger = fresh_ledger();
    let set = set(
        &ledger,
        &ask("CHEAP_WORKER", &["claude-code"], false),
        &declares,
        7,
    )
    .expect("set");
    assert_eq!(set.tools, vec!["claude-code"]);
    assert!(set.replaced.is_none());
    let resolved =
        actions::resolve_role(&json!({"role": "CHEAP_WORKER"}), Some(&ledger)).expect("resolves");
    assert_eq!(resolved["tool"], json!(["claude-code"]));
}

#[test]
fn a_tool_the_machine_does_not_declare_is_refused_by_name_and_nothing_is_written() {
    let ledger = fresh_ledger();
    let error = set(
        &ledger,
        &ask("CHEAP_WORKER", &["claude-code", "no-such-engine"], false),
        &declares,
        7,
    )
    .expect_err("an unknown tool");
    assert!(error.said.contains("no-such-engine"), "{}", error.said);
    assert_eq!(rows(&ledger), 0);
}

#[test]
fn a_tool_that_would_break_the_resolved_line_is_refused() {
    for bad in ["claude-code@work", "claude code", ""] {
        let ledger = fresh_ledger();
        assert!(
            set(&ledger, &ask("CHEAP_WORKER", &[bad], false), &|_| true, 7).is_err(),
            "«{bad}»"
        );
        assert_eq!(rows(&ledger), 0, "«{bad}»");
    }
}

#[test]
fn a_role_with_no_tool_is_refused() {
    let ledger = fresh_ledger();
    assert!(set(&ledger, &ask("CHEAP_WORKER", &[], false), &declares, 7).is_err());
    assert_eq!(rows(&ledger), 0);
}

#[test]
fn a_name_that_is_not_one_word_is_refused() {
    for bad in ["", "two words", "../x", "a/b", "role\nname"] {
        let ledger = fresh_ledger();
        assert!(
            set(&ledger, &ask(bad, &["codex"], false), &declares, 7).is_err(),
            "«{bad}»"
        );
        assert_eq!(rows(&ledger), 0, "«{bad}»");
    }
}

#[test]
fn an_existing_row_is_kept_unless_the_replacement_is_asked_and_then_says_what_it_replaced() {
    let ledger = fresh_ledger();
    set(
        &ledger,
        &ask("reviewer", &["claude-code"], false),
        &declares,
        1,
    )
    .expect("first");
    let refused =
        set(&ledger, &ask("reviewer", &["codex"], false), &declares, 2).expect_err("kept");
    assert!(refused.said.contains("claude-code"), "{}", refused.said);
    let kept =
        actions::resolve_role(&json!({"role": "reviewer"}), Some(&ledger)).expect("still there");
    assert_eq!(kept["tool"], json!(["claude-code"]));

    let done = set(&ledger, &ask("reviewer", &["codex"], true), &declares, 3).expect("replaced");
    assert_eq!(done.replaced, Some(vec!["claude-code".to_owned()]));
    let now = actions::resolve_role(&json!({"role": "reviewer"}), Some(&ledger)).expect("resolves");
    assert_eq!(now["tool"], json!(["codex"]));
}

#[test]
fn the_rows_are_listed_by_name() {
    let ledger = fresh_ledger();
    assert!(list(&ledger).expect("empty").is_empty());
    set(&ledger, &ask("b-role", &["codex"], false), &declares, 1).expect("b");
    set(
        &ledger,
        &ask("a-role", &["claude-code", "codex"], false),
        &declares,
        1,
    )
    .expect("a");
    let listed = list(&ledger).expect("list");
    assert_eq!(
        listed,
        vec![
            (
                "a-role".to_owned(),
                vec!["claude-code".to_owned(), "codex".to_owned()]
            ),
            ("b-role".to_owned(), vec!["codex".to_owned()]),
        ]
    );
}

#[test]
fn a_row_that_carries_an_account_is_not_replaced_from_here() {
    let ledger = fresh_ledger();
    ledger
        .put_record(&ledger::StoreRecord {
            collection: "roles".to_owned(),
            key: "billed".to_owned(),
            value: json!({"tools": ["claude-code"], "account": "work"}),
            written_by: "a person".to_owned(),
            written_at: 0,
        })
        .expect("a row with an account");
    let error = set(&ledger, &ask("billed", &["codex"], true), &declares, 1).expect_err("kept");
    assert!(error.said.contains("account"), "{}", error.said);
    let kept = ledger
        .read_record("roles", "billed")
        .expect("read")
        .expect("there");
    assert_eq!(kept.value["account"], json!("work"));
}
