//! **SILENCE IS NOT FREEDOM.** Everything destructive in the relay stands
//! behind this reading, so it is worth more than the rest put together: a
//! terminal is free when its prompt is painted and nothing on the screen is
//! waiting for a person, and in every other case the answer is «not yet».

use flow::{ActionOutcome, SharedState};
use serde_json::{json, Value};
use std::path::PathBuf;
use std::sync::{Mutex, MutexGuard, OnceLock};

/// The catalogue is named by one variable of the whole process, so two of these
/// at once would each read the other's descriptor.
fn one_at_a_time() -> MutexGuard<'static, ()> {
    static TURN: OnceLock<Mutex<()>> = OnceLock::new();
    let held = TURN.get_or_init(|| Mutex::new(()));
    held.lock().unwrap_or_else(|held| held.into_inner())
}

/// A store of this test's own, with a descriptor beside it.
struct Scratch(PathBuf, #[allow(dead_code)] MutexGuard<'static, ()>);

impl Scratch {
    fn new(name: &str) -> Self {
        let turn = one_at_a_time();
        let path = std::env::temp_dir().join(format!("sailor-free-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&path);
        std::fs::create_dir_all(&path).expect("a directory to write in");
        Scratch(path, turn)
    }

    /// What a command line of this test declares about a free session of it.
    fn declaring(&self, free_when: Value) -> &Self {
        let mut line = json!({
            "id": "a-command-line",
            "family": "ai_cli",
            "detect": {"command": "a-command-line"},
        });
        if !free_when.is_null() {
            line["free_when"] = free_when;
        }
        let file = self.0.join("tools.json");
        std::fs::write(
            &file,
            serde_json::to_string(&json!({"tools": [line]})).expect("it serialises"),
        )
        .expect("the descriptor is written");
        // Safety: one test process, one descriptor path, set before it is read.
        unsafe { std::env::set_var("SAILOR_TOOL_DESCRIPTORS", &file) };
        self
    }

    fn painted(&self, tty: &str, bytes: &[u8]) -> &Self {
        terminal::screen::write(&terminal::screen::address_in(&self.0, tty), bytes)
            .expect("the screen is written");
        self
    }

    fn asked(&self, tty: &str) -> Result<ActionOutcome, flow::ActionError> {
        let mut registry = flow::ActionRegistry::default();
        relay::register_relay(&mut registry);
        registry
            .get(relay::WAIT_FREE_ACTION)
            .expect("the action is registered")
            .execute(
                &json!({"tty": tty, "cli": "a-command-line", "store": self.0.display().to_string()}),
                &SharedState::new(),
            )
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn a_declaration() -> Value {
    json!({
        "the_prompt_shows": ["│ >"],
        "and_none_of_these": ["Do you want", "❯ 1."],
        "and_still_for_seconds": 0,
    })
}

fn not_yet(outcome: &Result<ActionOutcome, flow::ActionError>) -> String {
    match outcome {
        Ok(ActionOutcome::NotYet(why)) => why.clone(),
        other => panic!("it should have said not yet: {other:?}"),
    }
}

#[test]
fn a_prompt_painted_with_nothing_waiting_is_free() {
    let scratch = Scratch::new("free");
    scratch
        .declaring(a_declaration())
        .painted("ttys001", "\u{1b}[2m│ >\u{1b}[0m ".as_bytes());

    let outcome = scratch.asked("ttys001").expect("it does not break");

    match outcome {
        ActionOutcome::Went(said) => assert_eq!(said["free"], json!(true), "{said}"),
        other => panic!("it should have gone: {other:?}"),
    }
}

/// **A QUESTION ON THE SCREEN HOLDS THE TERMINAL**, prompt or no prompt: the
/// one thing a reset must never land on is a person's unanswered question.
#[test]
fn a_question_waiting_for_a_person_holds_it_back() {
    let scratch = Scratch::new("asking");
    scratch.declaring(a_declaration()).painted(
        "ttys002",
        "Do you want to make this edit?\n❯ 1. Yes\n│ > ".as_bytes(),
    );

    let why = not_yet(&scratch.asked("ttys002"));

    assert!(why.contains("Do you want"), "{why}");
    assert!(why.contains("waited for"), "{why}");
}

/// **AND A QUIET SCREEN IS NOT A FREE ONE.** A terminal showing nothing is as
/// likely to be waiting for an answer as waiting for work.
#[test]
fn a_screen_without_a_prompt_is_not_free() {
    let scratch = Scratch::new("quiet");
    scratch
        .declaring(a_declaration())
        .painted("ttys003", b"working on it...");

    let why = not_yet(&scratch.asked("ttys003"));

    assert!(why.contains("not painted"), "{why}");
}

/// A terminal nobody paints a screen for is one nothing can be said about.
#[test]
fn a_terminal_with_no_screen_at_all_is_not_free_either() {
    let scratch = Scratch::new("nothing");
    scratch.declaring(a_declaration());

    let why = not_yet(&scratch.asked("ttys004"));

    assert!(why.contains("nothing has been painted"), "{why}");
}

/// **A MARK SPLIT BY A COLOUR CODE IS A MARK NOBODY WOULD FIND.** Every prompt
/// worth recognising is painted, so this is the ordinary case and not an edge.
#[test]
fn the_marks_are_looked_for_in_what_a_person_sees() {
    let scratch = Scratch::new("painted");
    scratch.declaring(a_declaration()).painted(
        "ttys005",
        "\u{1b}[38;5;33m│\u{1b}[0m \u{1b}[1m>\u{1b}[0m ".as_bytes(),
    );

    let outcome = scratch.asked("ttys005").expect("it does not break");

    assert!(
        matches!(outcome, ActionOutcome::Went(_)),
        "the prompt is painted in pieces and is still the prompt: {outcome:?}"
    );
}

/// **A LINE THAT DECLARES NOTHING IS NOT A FREE ONE.** Absent means nobody
/// measured it, and a default saying «free» would type into the one session
/// that must not be typed into.
#[test]
fn a_command_line_that_declares_nothing_refuses_instead_of_guessing() {
    let scratch = Scratch::new("undeclared");
    scratch.declaring(Value::Null).painted("ttys006", b"| > ");

    let refusal = scratch
        .asked("ttys006")
        .expect_err("it refuses rather than guess");

    assert_eq!(refusal.class, "freedom_not_declared", "{refusal:?}");
}

/// **A SESSION AT WORK REPAINTS.** What is written under a spinner is the state
/// before it, so a screen that moved a moment ago says nothing about now.
#[test]
fn a_screen_still_being_painted_is_a_session_at_work() {
    let scratch = Scratch::new("working");
    let mut declared = a_declaration();
    declared["and_still_for_seconds"] = json!(3600);
    scratch
        .declaring(declared)
        .painted("ttys007", "│ > ".as_bytes());

    let why = not_yet(&scratch.asked("ttys007"));

    assert!(why.contains("session at work"), "{why}");
}
