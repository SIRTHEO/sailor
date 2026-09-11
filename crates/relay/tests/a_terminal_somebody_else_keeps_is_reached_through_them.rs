//! **SAILOR HOLDS ALMOST NOTHING**, and the terminals that matter are somebody
//! else's. A session Sailor did not open is reached only through the program
//! that opened it, by the door that program declares — and every refusal of the
//! reading holds there too, because a stranger's door is not a weaker one.

use flow::{ActionOutcome, SharedState};
use serde_json::json;
use std::path::PathBuf;
use std::sync::{Mutex, MutexGuard, OnceLock};

fn one_at_a_time() -> MutexGuard<'static, ()> {
    static TURN: OnceLock<Mutex<()>> = OnceLock::new();
    TURN.get_or_init(|| Mutex::new(()))
        .lock()
        .unwrap_or_else(|held| held.into_inner())
}

struct Scratch(PathBuf, #[allow(dead_code)] MutexGuard<'static, ()>);

impl Scratch {
    fn new(name: &str) -> Self {
        let path = std::env::temp_dir().join(format!("sailor-kept-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&path);
        std::fs::create_dir_all(&path).expect("a directory to write in");
        Scratch(path, one_at_a_time())
    }

    fn at(&self, name: &str) -> String {
        self.0.join(name).display().to_string()
    }

    /// A little program of this test's own, standing in for a keeper's.
    fn script(&self, name: &str, body: &str) -> String {
        let path = self.0.join(name);
        std::fs::write(&path, format!("#!/bin/sh\n{body}\n")).expect("the script is written");
        let mut how = std::fs::metadata(&path).expect("it is there").permissions();
        std::os::unix::fs::PermissionsExt::set_mode(&mut how, 0o755);
        std::fs::set_permissions(&path, how).expect("it can run");
        path.display().to_string()
    }

    /// A command line that declares how it is emptied and what a free session
    /// of it shows, and a keeper that declares how it is asked.
    fn declaring(&self, reads: &str, types: &str) -> &Self {
        let file = self.0.join("tools.json");
        std::fs::write(
            &file,
            serde_json::to_string(&json!({"tools": [
                {
                    "id": "a-command-line",
                    "family": "ai_cli",
                    "detect": {"command": "a-command-line"},
                    "reset_context": {"line": "/empty"},
                    "free_when": {
                        "the_prompt_shows": ["│ >"],
                        "and_none_of_these": ["Do you want"],
                        "and_still_for_seconds": 0
                    }
                },
                {
                    "id": "a-keeper",
                    "family": "tool",
                    "detect": {"command": "a-keeper"},
                    "keeps_terminals": {
                        "known_by": ["A_PANE_KEY"],
                        "reads_the_screen": [reads, "{handle}"],
                        "types_a_line": [types, "{handle}", "{line}"]
                    }
                }
            ]}))
            .expect("it serialises"),
        )
        .expect("the descriptor is written");
        // Safety: one test at a time, and the variable is set before it is read.
        unsafe { std::env::set_var("SAILOR_TOOL_DESCRIPTORS", &file) };
        self
    }

    /// The register says this terminal is kept, and under what name.
    fn kept(&self, tty: &str) -> &Self {
        let store = sessions::Sessions::open(self.0.join(sessions::SESSIONS_FILE))
            .expect("a register of this test's own");
        store
            .remember_keeper(&sessions::Kept {
                tty: tty.to_owned(),
                keeper: "a-keeper".to_owned(),
                handle: "pane-7".to_owned(),
                named_by: "A_PANE_KEY".to_owned(),
                seen_at: 1_000,
            })
            .expect("the keeper is written");
        self
    }

    fn asking(&self, action: &str, tty: &str) -> Result<ActionOutcome, flow::ActionError> {
        let mut registry = flow::ActionRegistry::default();
        relay::register_relay(&mut registry);
        registry
            .get(action)
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

fn not_yet(outcome: &Result<ActionOutcome, flow::ActionError>) -> String {
    match outcome {
        Ok(ActionOutcome::NotYet(why)) => why.clone(),
        other => panic!("it should have said not yet: {other:?}"),
    }
}

/// The screen of a terminal Sailor does not hold is asked of whoever does, and
/// read against the same marks as any other.
#[test]
fn the_screen_of_a_kept_terminal_is_asked_of_its_keeper() {
    let scratch = Scratch::new("read");
    let reads = scratch.script("read.sh", "printf '%s' '\\033[2m│ >\\033[0m '");
    let types = scratch.script("type.sh", "true");
    scratch.declaring(&reads, &types).kept("ttys001");

    let outcome = scratch
        .asking(relay::WAIT_FREE_ACTION, "ttys001")
        .expect("it does not break");

    match outcome {
        ActionOutcome::Went(said) => assert_eq!(said["free"], json!(true), "{said}"),
        other => panic!("it should have gone: {other:?}"),
    }
}

/// **A SCREEN THAT MOVES BETWEEN TWO READS IS A SESSION AT WORK.** Somebody
/// else's terminal leaves no file to date, so stillness is measured by asking
/// twice, a declared stillness apart.
#[test]
fn a_kept_screen_that_changes_between_two_reads_is_a_session_at_work() {
    let scratch = Scratch::new("moving");
    let counter = scratch.at("times");
    let reads = scratch.script(
        "read.sh",
        &format!("printf 'x' >> {counter}; printf '│ > %s' \"$(cat {counter})\""),
    );
    let types = scratch.script("type.sh", "true");
    scratch.declaring(&reads, &types).kept("ttys002");

    let why = not_yet(&scratch.asking(relay::WAIT_FREE_ACTION, "ttys002"));

    assert!(why.contains("session at work"), "{why}");
}

/// **A KEEPER NOBODY CAN REACH IS NOT A TERMINAL THAT IS FREE.** The program
/// that owns the session can be shut: that is a refusal with a name, never a
/// quiet yes.
#[test]
fn a_keeper_that_does_not_answer_refuses_by_name() {
    let scratch = Scratch::new("shut");
    let reads = scratch.script("read.sh", "echo 'the app is not running' >&2; exit 1");
    let types = scratch.script("type.sh", "true");
    scratch.declaring(&reads, &types).kept("ttys003");

    let refusal = scratch
        .asking(relay::WAIT_FREE_ACTION, "ttys003")
        .expect_err("it refuses rather than guess");

    assert_eq!(refusal.class, "keeper_did_not_answer", "{refusal:?}");
    assert!(refusal.said.contains("a-keeper"), "{}", refusal.said);
}

/// And the emptying line reaches the session through the same door.
#[test]
fn the_line_that_empties_a_session_goes_through_its_keeper() {
    let scratch = Scratch::new("typed");
    let reads = scratch.script("read.sh", "printf '%s' '│ > '");
    let landed = scratch.at("landed");
    let types = scratch.script(
        "type.sh",
        &format!("printf '%s %s' \"$1\" \"$2\" > {landed}"),
    );
    scratch.declaring(&reads, &types).kept("ttys004");

    let outcome = scratch
        .asking(relay::EMPTY_TERMINAL_ACTION, "ttys004")
        .expect("it does not break");

    assert!(matches!(outcome, ActionOutcome::Went(_)), "{outcome:?}");
    assert_eq!(
        std::fs::read_to_string(&landed).expect("the keeper was asked"),
        "pane-7 /empty",
        "the handle and the line both reach the keeper"
    );
}

/// A terminal with no letterbox and nobody keeping it is one nothing can be
/// said about, and nothing is typed into it.
#[test]
fn a_terminal_nobody_holds_and_nobody_keeps_is_refused_by_name() {
    let scratch = Scratch::new("orphan");
    let reads = scratch.script("read.sh", "printf '%s' '│ > '");
    let types = scratch.script("type.sh", "true");
    scratch.declaring(&reads, &types);

    let why = not_yet(&scratch.asking(relay::WAIT_FREE_ACTION, "ttys005"));

    assert!(why.contains("nobody keeps it"), "{why}");
}
