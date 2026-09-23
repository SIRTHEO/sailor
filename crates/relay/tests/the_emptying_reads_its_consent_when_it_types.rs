//! **CONSENT IS READ WHEN THE LINE IS TYPED, NOT WHEN THE RUN STARTED.** A run
//! parks for hours, and a terminal is reused: the reading taken at its start
//! may belong to a session that has since been replaced. The emptying asks the
//! screen, the mandate and the context again, together, before it types.

use flow::{
    Action, ActionError, ActionOutcome, Clock, ExecutionRequest, Executor, FlowError, FlowFile,
    InMemoryRecordStore, InProcessExecutor, SharedState, StepSpecies,
};
use serde_json::{json, Value};
use std::path::PathBuf;
use std::sync::{Mutex, MutexGuard, OnceLock};

const THE_AUTHOR: &str = "the-session-that-filled-up";
const THE_ONE_AFTER: &str = "a-session-opened-since";
const THE_FLOW: &str = "empty-a-session-that-handed-on";

fn one_at_a_time() -> MutexGuard<'static, ()> {
    static TURN: OnceLock<Mutex<()>> = OnceLock::new();
    TURN.get_or_init(|| Mutex::new(()))
        .lock()
        .unwrap_or_else(|held| held.into_inner())
}

/// A store, a command line and a keeper of this test's own. What the keeper is
/// asked to type lands in a file, so no terminal is ever typed into.
struct Scratch(PathBuf, #[allow(dead_code)] MutexGuard<'static, ()>);

impl Scratch {
    fn new(name: &str) -> Self {
        let turn = one_at_a_time();
        let path =
            std::env::temp_dir().join(format!("sailor-consent-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&path);
        std::fs::create_dir_all(&path).expect("a directory to write in");
        let scratch = Scratch(path, turn);
        scratch.declaring();
        scratch
    }

    fn landed(&self) -> PathBuf {
        self.0.join("landed")
    }

    fn what_landed(&self) -> Option<String> {
        std::fs::read_to_string(self.landed()).ok()
    }

    fn script(&self, name: &str, body: &str) -> String {
        let path = self.0.join(name);
        std::fs::write(&path, format!("#!/bin/sh\n{body}\n")).expect("the script is written");
        let mut how = std::fs::metadata(&path).expect("it is there").permissions();
        std::os::unix::fs::PermissionsExt::set_mode(&mut how, 0o755);
        std::fs::set_permissions(&path, how).expect("it can run");
        path.display().to_string()
    }

    fn declaring(&self) {
        let reads = self.script("read.sh", "printf '%s' '│ > '");
        let types = self.script(
            "type.sh",
            &format!("printf '%s' \"$2\" >> {}", self.landed().display()),
        );
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
    }

    fn register(&self) -> sessions::Sessions {
        sessions::Sessions::open(self.0.join(sessions::SESSIONS_FILE))
            .expect("a register of this test's own")
    }

    /// Who is in the terminal now, and the transcript it writes.
    fn in_there(&self, tty: &str, session: &str, tokens: u64) -> &Self {
        let transcript = self.0.join(format!("{session}.jsonl"));
        let row = json!({"message": {"usage": {"input_tokens": tokens}}});
        std::fs::write(&transcript, row.to_string()).expect("the transcript is written");
        self.in_there_writing(tty, session, &transcript.display().to_string())
    }

    fn in_there_writing(&self, tty: &str, session: &str, transcript: &str) -> &Self {
        self.register()
            .open_terminal(&sessions::Arrival {
                anchor: sessions::Anchor {
                    tty: tty.to_owned(),
                    worktree: "/a/tree".to_owned(),
                    ancestor: None,
                },
                session_id: Some(session.to_owned()),
                transcript_path: Some(transcript.to_owned()),
                at: 1_000,
            })
            .expect("the session is written");
        self
    }

    /// A session at oblige that left a mandate nobody took: consent, today.
    fn handing_on(&self, tty: &str) -> &Self {
        self.register()
            .remember_keeper(&sessions::Kept {
                tty: tty.to_owned(),
                keeper: "a-keeper".to_owned(),
                handle: "pane-7".to_owned(),
                named_by: "A_PANE_KEY".to_owned(),
                seen_at: 1_000,
            })
            .expect("the keeper is written");
        self.in_there(tty, THE_AUTHOR, 260_000);
        let mandate = sessions::mandate::Mandate {
            written: sessions::mandate::Written {
                tty: tty.to_owned(),
                session: THE_AUTHOR.to_owned(),
                engine: "a-command-line".to_owned(),
                ..Default::default()
            },
            ..Default::default()
        };
        sessions::mandate::deposit(&self.0, &mandate).expect("the mandate is written");
        self
    }

    fn asking(&self, action: &str, with: Value) -> Result<ActionOutcome, ActionError> {
        let mut registry = flow::ActionRegistry::default();
        relay::register_relay(&mut registry);
        let mut input = with;
        input["store"] = json!(self.0.display().to_string());
        registry
            .get(action)
            .expect("the action is registered")
            .execute(&input, &SharedState::new())
    }

    fn told_to_empty(&self, tty: &str) -> Result<ActionOutcome, ActionError> {
        self.asking(
            relay::EMPTY_TERMINAL_ACTION,
            json!({"tty": tty, "cli": "a-command-line"}),
        )
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn not_yet(outcome: &Result<ActionOutcome, ActionError>) -> String {
    match outcome {
        Ok(ActionOutcome::NotYet(why)) => why.clone(),
        other => panic!("it should have said not yet: {other:?}"),
    }
}

/// The control the others are measured against: the same fixture, untouched,
/// does reach the terminal.
#[test]
fn a_session_that_handed_on_and_stands_at_oblige_now_is_emptied() {
    let scratch = Scratch::new("given");
    scratch.handing_on("ttys001");

    let outcome = scratch.told_to_empty("ttys001").expect("it does not break");

    assert!(matches!(outcome, ActionOutcome::Went(_)), "{outcome:?}");
    assert_eq!(scratch.what_landed().as_deref(), Some("/empty"));
}

/// **THE TTY IS REUSED.** The mandate is still there and still untaken, but the
/// session that wrote it is gone, and the one in there now handed nothing on.
#[test]
fn a_mandate_left_by_a_session_since_replaced_empties_nothing() {
    let scratch = Scratch::new("replaced");
    scratch
        .handing_on("ttys002")
        .in_there("ttys002", THE_ONE_AFTER, 260_000);

    let why = not_yet(&scratch.told_to_empty("ttys002"));

    assert!(why.contains("is in there now"), "{why}");
    assert_eq!(scratch.what_landed(), None, "nothing is typed");
}

#[test]
fn a_mandate_taken_since_is_not_consent() {
    let scratch = Scratch::new("taken");
    scratch.handing_on("ttys003");
    sessions::mandate::consume(
        &sessions::mandate::address_in(&scratch.0, "ttys003"),
        THE_ONE_AFTER,
        2_000,
    )
    .expect("the mandate is taken");

    let why = not_yet(&scratch.told_to_empty("ttys003"));

    assert!(why.contains("was taken by"), "{why}");
    assert_eq!(scratch.what_landed(), None, "nothing is typed");
}

/// A compaction since the reading leaves the session far from full.
#[test]
fn a_context_that_fell_below_oblige_since_empties_nothing() {
    let scratch = Scratch::new("below");
    scratch
        .handing_on("ttys004")
        .in_there("ttys004", THE_AUTHOR, 90_000);

    let why = not_yet(&scratch.told_to_empty("ttys004"));

    assert!(why.contains("does not stand at oblige"), "{why}");
    assert_eq!(scratch.what_landed(), None, "nothing is typed");
}

#[test]
fn a_context_nobody_can_measure_is_not_a_full_one() {
    let scratch = Scratch::new("unmeasured");
    let nowhere = scratch.0.join("no-such-transcript.jsonl");
    scratch
        .handing_on("ttys005")
        .in_there_writing("ttys005", THE_AUTHOR, &nowhere.display().to_string());

    let why = not_yet(&scratch.told_to_empty("ttys005"));

    assert!(why.contains("cannot be measured"), "{why}");
    assert_eq!(scratch.what_landed(), None, "nothing is typed");
}

/// The line typed is the one the mandate's own command line declares.
#[test]
fn a_mandate_left_by_another_command_line_empties_nothing() {
    let scratch = Scratch::new("another-line");
    scratch.handing_on("ttys009");
    let path = sessions::mandate::address_in(&scratch.0, "ttys009");
    let mut left = sessions::mandate::read(&path).expect("the mandate");
    left.written.engine = "another-line".to_owned();
    std::fs::write(&path, serde_json::to_string(&left).expect("it serialises"))
        .expect("the mandate is rewritten");

    let why = not_yet(&scratch.told_to_empty("ttys009"));

    assert!(why.contains("another-line"), "{why}");
    assert_eq!(scratch.what_landed(), None, "nothing is typed");
}

/// A terminal a person asked Sailor to leave alone is left alone.
#[test]
fn a_terminal_left_alone_is_not_emptied() {
    let scratch = Scratch::new("detached");
    scratch.handing_on("ttys010");
    let anchor = sessions::Anchor {
        tty: "ttys010".to_owned(),
        worktree: "/a/tree".to_owned(),
        ancestor: None,
    };
    scratch.register().detach(&anchor, 2_000).expect("it is detached");

    let why = not_yet(&scratch.told_to_empty("ttys010"));

    assert!(why.contains("left alone"), "{why}");
    assert_eq!(scratch.what_landed(), None, "nothing is typed");
}

/// The node that types any line is not a road around the one that reads its
/// consent: a line a descriptor declares as its emptying is refused there.
#[test]
fn the_emptying_line_is_not_typed_by_the_node_that_types_lines() {
    let scratch = Scratch::new("around");
    scratch.handing_on("ttys006");

    let refusal = scratch
        .asking(
            relay::TYPE_INTO_TERMINAL_ACTION,
            json!({"tty": "ttys006", "line": "/empty"}),
        )
        .expect_err("it refuses");

    assert_eq!(refusal.class, "an_emptying_is_not_a_line", "{refusal:?}");
    assert_eq!(scratch.what_landed(), None, "nothing is typed");
}

struct Tick(std::sync::atomic::AtomicI64);

impl Clock for Tick {
    fn now(&self) -> Result<i64, FlowError> {
        Ok(self.0.fetch_add(1, std::sync::atomic::Ordering::Relaxed) + 1)
    }
}

/// The run's start, as the session event carries it.
struct Said(Value);

impl Action for Said {
    fn execute(&self, _input: &Value, _shared: &SharedState) -> Result<ActionOutcome, ActionError> {
        Ok(ActionOutcome::Went(self.0.clone()))
    }

    fn species(&self) -> StepSpecies {
        StepSpecies::Repeatable
    }
}

/// The reading at the start of the run says oblige, and then the world moves
/// on while the run is still open.
struct MeasuredThen(Box<dyn Fn() + Send + Sync>);

impl Action for MeasuredThen {
    fn execute(&self, _input: &Value, _shared: &SharedState) -> Result<ActionOutcome, ActionError> {
        (self.0)();
        Ok(ActionOutcome::Went(json!({"state": "oblige", "tokens": 260_000})))
    }

    fn species(&self) -> StepSpecies {
        StepSpecies::Repeatable
    }
}

/// The shipped flow, pointed at this test's store and given no time to wait.
fn the_flow(store: &str) -> FlowFile {
    let text = flow::system::FLOWS
        .iter()
        .find(|(name, _)| *name == THE_FLOW)
        .map(|(_, text)| *text)
        .expect("the flow is shipped");
    let mut file: Value = serde_json::from_str(text).expect("the flow is JSON");
    for step in file["graph"]["steps"].as_array_mut().expect("steps") {
        if step["id"] == "arrived" {
            step["with"]["within_seconds"] = json!(0);
        }
        if !matches!(step["id"].as_str(), Some("trigger" | "measure")) {
            step["with"]["store"] = json!(store);
        }
    }
    serde_json::from_value(file).expect("the flow file loads")
}

fn run_the_relay(scratch: &Scratch, tty: &str, then: Box<dyn Fn() + Send + Sync>) {
    let mut registry = flow::ActionRegistry::default();
    relay::register_relay(&mut registry);
    actions::mandate::register_mandate(&mut registry);
    registry.register(
        "trigger",
        Said(json!({
            "text": "the session finished a turn",
            "carried": {"tty": tty, "session": THE_AUTHOR, "transcript": null},
        })),
    );
    registry.register("measure_session", MeasuredThen(then));
    let flow = the_flow(&scratch.0.display().to_string());
    let request = ExecutionRequest {
        holder: None,
        run_id: format!("consent-{}-{tty}", std::process::id()),
        root_inputs: flow.inputs.clone().into_iter().collect(),
        gates: Vec::new(),
        shared: SharedState::new(),
        spend_cap_micros: None,
        stops: flow::RunStops {
            wall_deadline_at: Some(60),
            ..Default::default()
        },
    };
    InProcessExecutor
        .execute(
            &flow.graph,
            request,
            &InMemoryRecordStore::default(),
            &registry,
            &Tick(0.into()),
        )
        .expect("the run does not break");
}

/// The shipped flow, whole, with consent that still holds when it types.
#[test]
fn the_shipped_relay_empties_a_session_whose_consent_still_holds() {
    let scratch = Scratch::new("flow-given");
    scratch.handing_on("ttys007");

    run_the_relay(&scratch, "ttys007", Box::new(|| {}));

    assert_eq!(scratch.what_landed().as_deref(), Some("/empty"));
}

/// **THE READING AT THE START IS NOT THE READING AT THE TYPING.** Both of the
/// run's own questions answer yes, and the session is replaced after they do.
#[test]
fn the_shipped_relay_empties_nothing_once_the_session_that_consented_is_replaced() {
    let scratch = Scratch::new("flow-replaced");
    scratch.handing_on("ttys008");
    let store = scratch.0.clone();

    run_the_relay(
        &scratch,
        "ttys008",
        Box::new(move || {
            sessions::Sessions::open(store.join(sessions::SESSIONS_FILE))
                .expect("the register")
                .open_terminal(&sessions::Arrival {
                    anchor: sessions::Anchor {
                        tty: "ttys008".to_owned(),
                        worktree: "/a/tree".to_owned(),
                        ancestor: None,
                    },
                    session_id: Some(THE_ONE_AFTER.to_owned()),
                    transcript_path: None,
                    at: 2_000,
                })
                .expect("a new session arrives");
        }),
    );

    assert_eq!(scratch.what_landed(), None, "nothing is typed");
}
