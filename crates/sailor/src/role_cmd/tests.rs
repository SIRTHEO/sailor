use super::*;
use std::sync::atomic::{AtomicUsize, Ordering};

fn words(list: &[&str]) -> Vec<String> {
    list.iter().map(|word| (*word).to_owned()).collect()
}

fn declares(id: &str) -> bool {
    matches!(id, "claude-code" | "codex")
}

fn fresh_ledger() -> Ledger {
    static NEXT: AtomicUsize = AtomicUsize::new(0);
    let dir = std::env::temp_dir().join(format!(
        "sailor-role-cmd-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::SeqCst)
    ));
    let _ = std::fs::remove_dir_all(&dir);
    Ledger::open(&dir).expect("a scratch ledger")
}

/// **THE WORDS BECOME THE ASK, AND `--replace` MAY STAND ANYWHERE.** The name
/// is the first loose word and every other loose word is a tool, in the order
/// typed: that order is the order a step tries them in.
#[test]
fn the_words_are_a_name_tools_in_order_and_an_optional_replace() {
    let plain = asked(&words(&["set", "CHEAP_WORKER", "codex", "claude-code"])).expect("an ask");
    let replacing = asked(&words(&["set", "--replace", "CHEAP_WORKER", "codex", "claude-code"]))
        .expect("an ask");

    let expected = |replace| {
        Ask::Set {
            name: "CHEAP_WORKER".to_owned(),
            tools: words(&["codex", "claude-code"]),
            replace,
        }
    };
    assert_eq!(plain, expected(false));
    assert_eq!(replacing, expected(true));
    assert_eq!(asked(&words(&["list"])).expect("an ask"), Ask::List);
}

/// A role with no tool, a word that is not a verb, an option nobody offers:
/// each is refused in words, never guessed into a write.
#[test]
fn words_that_are_not_a_form_are_refused_by_name() {
    for bad in [
        words(&[]),
        words(&["set"]),
        words(&["set", "CHEAP_WORKER"]),
        words(&["set", "CHEAP_WORKER", "codex", "--everything"]),
        words(&["list", "extra"]),
        words(&["delete", "CHEAP_WORKER"]),
    ] {
        let said = asked(&bad).expect_err(&format!("{bad:?} is not a form"));
        assert!(said.contains("sailor role set"), "{bad:?} says what to type: {said}");
    }
}

/// What a person types, applied: the row is there for `list`, and the second
/// write to the same name is refused until it says `--replace`.
#[test]
fn a_role_set_is_listed_and_is_kept_until_replace_is_said() {
    let ledger = fresh_ledger();
    let set = |line: &[&str]| {
        applied(&ledger, asked(&words(line)).expect("an ask"), &declares, 5)
    };

    let first = set(&["set", "CHEAP_WORKER", "codex"]).expect("set");
    let kept = set(&["set", "CHEAP_WORKER", "claude-code"]).expect_err("kept");
    let replaced = set(&["set", "CHEAP_WORKER", "claude-code", "--replace"]).expect("replaced");
    let listed = applied(&ledger, Ask::List, &declares, 6).expect("listed");

    assert!(first.contains("CHEAP_WORKER") && first.contains("codex"), "{first}");
    assert_eq!(kept.class, "invalid_input");
    assert!(replaced.contains("codex"), "the tools it replaced are said: {replaced}");
    assert!(listed.contains("CHEAP_WORKER") && listed.contains("claude-code"), "{listed}");
    assert!(!listed.contains("codex"), "{listed}");
}

/// **A TOOL NOBODY DECLARED IS REFUSED, AND NOTHING IS LEFT BEHIND.** The check
/// is the machine's own list, handed in, so this cannot pass by writing first.
#[test]
fn an_undeclared_tool_is_refused_and_no_row_is_written() {
    let ledger = fresh_ledger();

    let refused = applied(
        &ledger,
        asked(&words(&["set", "CHEAP_WORKER", "no-such-engine"])).expect("an ask"),
        &declares,
        5,
    )
    .expect_err("refused");
    let listed = applied(&ledger, Ask::List, &declares, 6).expect("listed");

    assert!(refused.said.contains("no-such-engine"), "{}", refused.said);
    assert_eq!(listed, catalogue::say("cli.role.none", &[]));
}

/// The refusal a flow gives for a role with no row now says what to type.
#[test]
fn a_missing_role_names_the_command_that_writes_it() {
    let said = catalogue::say("cli.role.missing", &[("role", "CHEAP_WORKER")]);

    assert!(said.contains("sailor role set CHEAP_WORKER"), "{said}");
}
