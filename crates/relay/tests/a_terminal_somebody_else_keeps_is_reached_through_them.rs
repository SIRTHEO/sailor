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

    /// The same, for a keeper whose own commands want a name the session does
    /// not carry: the join between the two is declared and looked up.
    fn declaring_a_list(&self, reads: &str, types: &str, lists: &str) -> &Self {
        self.declaring_with(reads, types, Some(lists))
    }

    /// A command line that declares how it is emptied and what a free session
    /// of it shows, and a keeper that declares how it is asked.
    fn declaring(&self, reads: &str, types: &str) -> &Self {
        self.declaring_with(reads, types, None)
    }

    fn declaring_with(&self, reads: &str, types: &str, lists: Option<&str>) -> &Self {
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
                        "lists_them": lists.map(|at| json!([at])).unwrap_or(json!([])),
                        "in_the_list": {
                            "at": ["result", "terminals"],
                            "known_by": ["tabId", "leafId"],
                            "joined_by": ":",
                            "the_handle_is": "handle"
                        },
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
        self.kept_as(tty, "pane-7")
    }

    fn kept_as(&self, tty: &str, named: &str) -> &Self {
        let store = sessions::Sessions::open(self.0.join(sessions::SESSIONS_FILE))
            .expect("a register of this test's own");
        store
            .remember_keeper(&sessions::Kept {
                tty: tty.to_owned(),
                keeper: "a-keeper".to_owned(),
                handle: named.to_owned(),
                named_by: "A_PANE_KEY".to_owned(),
                seen_at: 1_000,
            })
            .expect("the keeper is written");
        self
    }

    /// The session in there stands at oblige and left a mandate nobody took:
    /// what the emptying reads before it types.
    fn handing_on(&self, tty: &str) -> &Self {
        let transcript = self.0.join(format!("{tty}.jsonl"));
        std::fs::write(
            &transcript,
            r#"{"message":{"usage":{"input_tokens":260000}}}"#,
        )
        .expect("the transcript is written");
        sessions::Sessions::open(self.0.join(sessions::SESSIONS_FILE))
            .expect("a register of this test's own")
            .open_terminal(&sessions::Arrival {
                anchor: sessions::Anchor {
                    tty: tty.to_owned(),
                    worktree: "/a/tree".to_owned(),
                    ancestor: None,
                },
                session_id: Some("the-one-in-there".to_owned()),
                transcript_path: Some(transcript.display().to_string()),
                at: 1_000,
            })
            .expect("the session is written");
        let mandate = sessions::mandate::Mandate {
            written: sessions::mandate::Written {
                tty: tty.to_owned(),
                session: "the-one-in-there".to_owned(),
                engine: "a-command-line".to_owned(),
                ..Default::default()
            },
            ..Default::default()
        };
        sessions::mandate::deposit(&self.0, &mandate).expect("the mandate is written");
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

/// **ONE MOVEMENT IS NOT A SESSION AT WORK.** A screen that changed once (a
/// clock, a last repaint after the turn ended) and then stands still is read
/// again, a bounded number of times; a screen that never settles is still
/// refused.
#[test]
fn a_kept_screen_that_settles_after_one_movement_is_free() {
    let scratch = Scratch::new("settling");
    let counter = scratch.at("times");
    let reads = scratch.script(
        "read.sh",
        &format!(
            "printf 'x' >> {counter}; n=$(wc -c < {counter}); [ \"$n\" -gt 2 ] && n=2; printf '│ > %s' \"$n\""
        ),
    );
    let types = scratch.script("type.sh", "true");
    scratch.declaring(&reads, &types).kept("ttys004");

    let outcome = scratch
        .asking(relay::WAIT_FREE_ACTION, "ttys004")
        .expect("it does not break");

    match outcome {
        ActionOutcome::Went(said) => assert_eq!(said["free"], json!(true), "{said}"),
        other => panic!("it settled, so it should have gone: {other:?}"),
    }
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
    scratch
        .declaring(&reads, &types)
        .kept("ttys004")
        .handing_on("ttys004");

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

/// **A SESSION NAMES ITSELF ONE WAY AND ITS KEEPER ANOTHER.** What a terminal
/// carries in its variables is not always what its keeper's commands want, so
/// the handle is looked up in the list the keeper prints, never assumed.
#[test]
fn the_handle_is_looked_up_where_the_keeper_calls_it_something_else() {
    let scratch = Scratch::new("lookup");
    let lists = scratch.script(
        "list.sh",
        "printf '%s' '{\"result\":{\"terminals\":[\
         {\"tabId\":\"other\",\"leafId\":\"one\",\"handle\":\"term_nope\"},\
         {\"tabId\":\"pane\",\"leafId\":\"7\",\"handle\":\"term_yes\"}]}}'",
    );
    let reads = scratch.script("read.sh", "printf '%s' '│ > '");
    let landed = scratch.at("landed");
    let types = scratch.script("type.sh", &format!("printf '%s' \"$1\" > {landed}"));
    scratch
        .declaring_a_list(&reads, &types, &lists)
        .kept_as("ttys006", "pane:7")
        .handing_on("ttys006");

    scratch
        .asking(relay::EMPTY_TERMINAL_ACTION, "ttys006")
        .expect("it does not break");

    assert_eq!(
        std::fs::read_to_string(&landed).expect("the keeper was asked"),
        "term_yes",
        "the handle the keeper knows, not the name the session carries"
    );
}

/// A keeper that no longer knows this terminal refuses by name: the session it
/// was is gone, and typing into whatever took its place is the worst outcome.
#[test]
fn a_terminal_the_keeper_has_forgotten_refuses_by_name() {
    let scratch = Scratch::new("forgotten");
    let lists = scratch.script("list.sh", "printf '%s' '{\"result\":{\"terminals\":[]}}'");
    let reads = scratch.script("read.sh", "printf '%s' '│ > '");
    let types = scratch.script("type.sh", "true");
    scratch
        .declaring_a_list(&reads, &types, &lists)
        .kept_as("ttys007", "pane:9");

    let refusal = scratch
        .asking(relay::WAIT_FREE_ACTION, "ttys007")
        .expect_err("it refuses rather than guess");

    assert_eq!(refusal.class, "keeper_did_not_answer", "{refusal:?}");
    assert!(refusal.said.contains("pane:9"), "{}", refusal.said);
}

/// **A KEEPER THAT STUMBLES ONCE IS ASKED AGAIN.** Reading changes nothing, so a
/// question that got no answer is put a second time before the terminal counts
/// as unreachable.
#[test]
fn a_keeper_that_stumbles_once_is_asked_again() {
    let scratch = Scratch::new("stumble");
    let marker = scratch.at("stumbled");
    let reads = scratch.script(
        "read.sh",
        &format!("[ -e {marker} ] || {{ : > {marker}; exit 1; }}; printf '%s' '│ > '"),
    );
    let types = scratch.script("type.sh", "true");
    scratch.declaring(&reads, &types).kept("ttys005");

    let outcome = scratch
        .asking(relay::WAIT_FREE_ACTION, "ttys005")
        .expect("the second question is answered");

    match outcome {
        ActionOutcome::Went(said) => assert_eq!(said["free"], json!(true), "{said}"),
        other => panic!("it should have gone: {other:?}"),
    }
}

/// **A LINE IS TYPED AT MOST ONCE.** A keeper that fails after the line arrived
/// would type it twice if asked again, so the typing is never repeated.
#[test]
fn a_line_that_got_no_answer_is_not_typed_twice() {
    let scratch = Scratch::new("once");
    let reads = scratch.script("read.sh", "printf '%s' '│ > '");
    let landed = scratch.at("landed");
    let types = scratch.script("type.sh", &format!("echo \"$2\" >> {landed}; exit 1"));
    scratch
        .declaring(&reads, &types)
        .kept("ttys006")
        .handing_on("ttys006");

    let refusal = scratch
        .asking(relay::EMPTY_TERMINAL_ACTION, "ttys006")
        .expect_err("the typing did not answer");

    assert_eq!(refusal.class, "keeper_did_not_answer", "{refusal:?}");
    assert_eq!(
        std::fs::read_to_string(&landed)
            .expect("the keeper was asked")
            .lines()
            .count(),
        1,
        "the line reached the keeper once"
    );
}

/// **TEN SECONDS IS THE WHOLE QUESTION, NOT EACH ASKING.** A keeper that hangs
/// has used the time it was promised; asking again would double what a caller
/// waits for an answer that is not coming.
#[test]
fn a_keeper_that_hangs_is_not_waited_for_twice() {
    let scratch = Scratch::new("hang");
    let reads = scratch.script("read.sh", "sleep 60");
    let types = scratch.script("type.sh", "true");
    scratch.declaring(&reads, &types).kept("ttys008");

    let started = std::time::Instant::now();
    let refusal = scratch
        .asking(relay::WAIT_FREE_ACTION, "ttys008")
        .expect_err("a keeper that hangs does not answer");

    assert_eq!(refusal.class, "keeper_did_not_answer", "{refusal:?}");
    assert!(
        started.elapsed() < std::time::Duration::from_secs(14),
        "the question took {:?}",
        started.elapsed()
    );
}

/// **A REFUSAL SAYS WHAT WAS ASKED AND WHAT EACH ASKING ANSWERED.** Only the
/// last answer used to survive, and the first can be the one that says why.
#[test]
fn a_refusal_after_two_askings_says_both_answers() {
    let scratch = Scratch::new("both");
    let marker = scratch.at("asked-once");
    let reads = scratch.script(
        "read.sh",
        &format!(
            "if [ -e {marker} ]; then echo 'second answer' >&2; else : > {marker}; \
             echo 'first answer' >&2; fi; exit 1"
        ),
    );
    let types = scratch.script("type.sh", "true");
    scratch.declaring(&reads, &types).kept("ttys009");

    let refusal = scratch
        .asking(relay::WAIT_FREE_ACTION, "ttys009")
        .expect_err("neither asking was answered");

    assert!(refusal.said.contains("first answer"), "{}", refusal.said);
    assert!(refusal.said.contains("second answer"), "{}", refusal.said);
}

/// **A KEEPER THAT CANNOT START WILL NOT START THE SECOND TIME.** Asking again
/// what can only fail the same way says it was tried twice when it was not.
#[test]
fn a_keeper_that_cannot_be_started_is_asked_once() {
    let scratch = Scratch::new("absent");
    let types = scratch.script("type.sh", "true");
    let nowhere = scratch.at("no-such-program");
    scratch.declaring(&nowhere, &types).kept("ttys010");

    let refusal = scratch
        .asking(relay::WAIT_FREE_ACTION, "ttys010")
        .expect_err("there is nothing to run");

    assert!(refusal.said.contains("did not start"), "{}", refusal.said);
    assert!(!refusal.said.contains("times: first"), "{}", refusal.said);
}

/// **THE LIST OF TERMINALS IS ASKED AGAIN TOO**: it changes nothing either.
#[test]
fn a_list_that_stumbles_once_is_asked_again() {
    let scratch = Scratch::new("list-stumble");
    let marker = scratch.at("list-stumbled");
    let lists = scratch.script(
        "list.sh",
        &format!(
            "[ -e {marker} ] || {{ : > {marker}; exit 1; }}; \
             printf '%s' '{{\"result\":{{\"terminals\":[\
             {{\"tabId\":\"pane\",\"leafId\":\"7\",\"handle\":\"term_yes\"}}]}}}}'"
        ),
    );
    let reads = scratch.script("read.sh", "printf '%s' '│ > '");
    let types = scratch.script("type.sh", "true");
    scratch
        .declaring_a_list(&reads, &types, &lists)
        .kept_as("ttys011", "pane:7");

    scratch
        .asking(relay::WAIT_FREE_ACTION, "ttys011")
        .expect("the second asking of the list is answered");
}

/// **LOOKING A TERMINAL UP AND READING IT ARE ONE QUESTION.** Ten seconds is
/// what the two of them have together.
#[test]
fn looking_a_terminal_up_and_reading_it_share_the_ten_seconds() {
    let scratch = Scratch::new("shared");
    let lists = scratch.script(
        "list.sh",
        "sleep 6; printf '%s' '{\"result\":{\"terminals\":[\
         {\"tabId\":\"pane\",\"leafId\":\"7\",\"handle\":\"term_yes\"}]}}'",
    );
    let reads = scratch.script("read.sh", "sleep 60");
    let types = scratch.script("type.sh", "true");
    scratch
        .declaring_a_list(&reads, &types, &lists)
        .kept_as("ttys012", "pane:7");

    let started = std::time::Instant::now();
    let refusal = scratch
        .asking(relay::WAIT_FREE_ACTION, "ttys012")
        .expect_err("the reading hangs");

    assert_eq!(refusal.class, "keeper_did_not_answer", "{refusal:?}");
    assert!(
        started.elapsed() < std::time::Duration::from_secs(14),
        "the question took {:?}",
        started.elapsed()
    );
}

/// **NO SECOND ASKING THAT IS BOUND TO FAIL.** An asking that fails when the
/// time is nearly gone is not followed by one with too little left to answer.
#[test]
fn a_keeper_that_fails_late_is_not_asked_with_no_time_left() {
    let scratch = Scratch::new("late");
    let asked = scratch.at("asked");
    let reads = scratch.script("read.sh", &format!("echo x >> {asked}; sleep 8.5; exit 1"));
    let types = scratch.script("type.sh", "true");
    scratch.declaring(&reads, &types).kept("ttys013");

    let refusal = scratch
        .asking(relay::WAIT_FREE_ACTION, "ttys013")
        .expect_err("the keeper failed");
    assert!(!refusal.said.contains("times: first"), "{}", refusal.said);
    assert!(
        !refusal.said.contains("the question was never put"),
        "{}",
        refusal.said
    );

    assert_eq!(
        std::fs::read_to_string(&asked)
            .expect("the keeper was asked")
            .lines()
            .count(),
        1,
        "there was no time to ask again"
    );
}

/// **A QUESTION IS NOT PUT WITH NO TIME LEFT TO ANSWER IT.** When looking the
/// terminal up has used the ten seconds, what follows is refused before it is
/// run: a line typed in the last milliseconds and killed reads as unanswered
/// and would be typed again by whoever retries.
#[test]
fn a_question_with_no_time_left_is_not_put() {
    let scratch = Scratch::new("spent");
    let lists = scratch.script(
        "list.sh",
        "sleep 8.5; printf '%s' '{\"result\":{\"terminals\":[\
         {\"tabId\":\"pane\",\"leafId\":\"7\",\"handle\":\"term_yes\"}]}}'",
    );
    let asked = scratch.at("asked");
    let reads = scratch.script("read.sh", &format!("echo x >> {asked}; printf '%s' '│ > '"));
    let types = scratch.script("type.sh", "true");
    scratch
        .declaring_a_list(&reads, &types, &lists)
        .kept_as("ttys014", "pane:7");

    let refusal = scratch
        .asking(relay::WAIT_FREE_ACTION, "ttys014")
        .expect_err("the lookup used the time");

    assert_eq!(refusal.class, "keeper_did_not_answer", "{refusal:?}");
    assert!(
        std::fs::read_to_string(&asked).is_err(),
        "the keeper was asked with no time left"
    );
    assert!(
        refusal.said.contains("the question was never put"),
        "{}",
        refusal.said
    );
    assert!(
        !refusal.said.contains("nobody can reach"),
        "a keeper that was never asked is not one nobody can reach: {}",
        refusal.said
    );
}
