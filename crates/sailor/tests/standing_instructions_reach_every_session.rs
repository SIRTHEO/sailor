//! What a person keeps with `sailor instructions` reaches every session they
//! open, as that session starts, and nothing else: not a prompt event, and not
//! a session once they are cleared.

use std::io::Write;
use std::path::PathBuf;
use std::process::{Command, Output, Stdio};

const A_SESSION_STARTS: &str =
    r#"{"session_id":"s1","cwd":"/tmp","hook_event_name":"SessionStart","source":"startup"}"#;
const A_PROMPT_ARRIVES: &str =
    r#"{"session_id":"s1","cwd":"/tmp","hook_event_name":"UserPromptSubmit","prompt":"hi"}"#;
const THE_WORDS: &str = "Do not flatter me. Challenge the assumption under the question.";

fn a_home(label: &str) -> PathBuf {
    let home = std::env::temp_dir().join(format!("sailor-standing-{label}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&home);
    std::fs::create_dir_all(&home).expect("a home");
    home
}

fn sailor(home: &PathBuf, args: &[&str], stdin: &str) -> Output {
    let mut child = Command::new(env!("CARGO_BIN_EXE_sailor"))
        .args(args)
        .env("SAILOR_HOME", home)
        .env("SAILOR_LEDGER", home.join("ledger"))
        .env("SAILOR_LANG", "en")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("sailor starts");
    child
        .stdin
        .take()
        .expect("its input")
        .write_all(stdin.as_bytes())
        .expect("it reads");
    child.wait_with_output().expect("it ends")
}

fn said(output: &Output) -> String {
    String::from_utf8_lossy(&output.stdout).into_owned()
}

fn greeting(home: &PathBuf, payload: &str) -> String {
    let output = sailor(home, &["session", "open", "--cli", "claude-code"], payload);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    said(&output)
}

#[test]
fn words_kept_once_reach_the_next_session_that_starts() {
    let home = a_home("reach");
    assert!(!greeting(&home, A_SESSION_STARTS).contains(THE_WORDS));

    let kept = sailor(&home, &["instructions", "set", "-"], THE_WORDS);
    assert!(
        kept.status.success(),
        "{}",
        String::from_utf8_lossy(&kept.stderr)
    );

    let hook: serde_json::Value =
        serde_json::from_str(&greeting(&home, A_SESSION_STARTS)).expect("the hook answers in json");
    let context = hook["hookSpecificOutput"]["additionalContext"]
        .as_str()
        .expect("the context a session reads");
    assert!(context.contains(THE_WORDS), "{context}");
    assert!(said(&sailor(&home, &["instructions"], "")).contains(THE_WORDS));
}

#[test]
fn a_prompt_is_not_handed_them_again() {
    let home = a_home("prompt");
    sailor(&home, &["instructions", "set", "-"], THE_WORDS);

    assert!(!greeting(&home, A_PROMPT_ARRIVES).contains(THE_WORDS));
}

#[test]
fn cleared_words_reach_no_session() {
    let home = a_home("clear");
    sailor(&home, &["instructions", "set", "-"], THE_WORDS);
    let cleared = sailor(&home, &["instructions", "clear"], "");
    assert!(cleared.status.success());

    assert!(!greeting(&home, A_SESSION_STARTS).contains(THE_WORDS));
}

/// Refused whole, and said to the person: a cut instruction would be obeyed
/// as if it were the one they wrote.
#[test]
fn words_too_long_for_a_session_are_refused_and_never_cut() {
    let home = a_home("long");
    let long = "x".repeat(sailor::instructions_cmd::THE_MOST_A_SESSION_IS_HANDED + 1);

    let refused = sailor(&home, &["instructions", "set", "-"], &long);
    assert_eq!(refused.status.code(), Some(2));

    std::fs::create_dir_all(home.join("instructions")).expect("the directory");
    std::fs::write(home.join("instructions").join("sessions.md"), &long).expect("edited by hand");
    let context = greeting(&home, A_SESSION_STARTS);
    assert!(!context.contains(&long));
    assert!(context.contains("none of them reached you"), "{context}");
}

#[test]
fn an_empty_text_keeps_nothing() {
    let home = a_home("empty");

    let refused = sailor(&home, &["instructions", "set", "-"], "  \n ");
    assert_eq!(refused.status.code(), Some(2));
    assert!(!home.join("instructions").join("sessions.md").exists());
}
