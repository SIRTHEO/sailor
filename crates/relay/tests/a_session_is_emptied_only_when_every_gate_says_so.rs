//! The relay empties a session only when the session declared its work handed
//! on and every gate agrees: no new instruction, no sub-agent running, no call
//! left running or scheduled, a free screen, and no activity between the checks
//! and the keys. Each test breaks one gate and checks that nothing is typed.

use relay::handover::{hand_over, resume_successor, work_left_running};
use sessions::handover::{NewHandover, State};
use sessions::{Sessions, TerminalEvent, SESSIONS_FILE};
use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::time::{Duration, SystemTime};
use terminal::inbox::{self, Inbox};
use toolbox::{Catalog, Source};

const TTY: &str = "ttys901";
const LINE: &str = "claude-code";

fn catalog() -> Catalog {
    Catalog::load(&[Source::Builtin])
}

struct Stage {
    root: PathBuf,
    transcript: PathBuf,
    typed: mpsc::Receiver<Vec<u8>>,
}

impl Drop for Stage {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

fn event(name: &str) -> TerminalEvent {
    TerminalEvent {
        tty: TTY.to_owned(),
        session_id: Some("s-1".to_owned()),
        worktree: Some("/work/tree".to_owned()),
        ancestor: None,
        name: name.to_owned(),
        transcript_path: None,
        occurred_at: 100,
        payload: None,
    }
}

fn store(root: &Path) -> Sessions {
    Sessions::open(root.join(SESSIONS_FILE)).expect("the sessions")
}

fn a_call(id: &str, name: &str, input: serde_json::Value) -> String {
    serde_json::json!({"type": "assistant", "message": {"role": "assistant",
        "content": [{"type": "tool_use", "id": id, "name": name, "input": input}]}})
    .to_string()
}

fn an_answer(id: &str) -> String {
    serde_json::json!({"type": "user", "message": {"role": "user",
        "content": [{"type": "tool_result", "tool_use_id": id, "content": "done"}]}})
    .to_string()
}

/// A session that deposited a finished mandate and then ended its turn, on a
/// screen showing the prompt and still, with a letterbox that records keys.
fn a_finished_session(name: &str) -> Stage {
    let root = terminal::scratch::directory(&format!("handover-{name}")).expect("a scratch");
    let sessions = store(&root);
    sessions.record_event(&event("UserPromptSubmit")).expect("event");
    let mut mandate = sessions::mandate::Mandate::default();
    mandate.written.tty = TTY.to_owned();
    mandate.written.session = "s-1".to_owned();
    mandate.written.engine = LINE.to_owned();
    mandate.written.at = 150;
    mandate.work.goal = "carry on".to_owned();
    sessions::mandate::deposit(&root, &mandate).expect("the mandate");
    sessions
        .open_handover(&NewHandover {
            tty: TTY.to_owned(),
            tree: "/work/tree".to_owned(),
            session: "s-1".to_owned(),
            engine: LINE.to_owned(),
            mandate_at: 150,
            at: 150,
        })
        .expect("the handover");
    sessions.record_event(&event("Stop")).expect("the turn ended");

    let transcript = root.join("record.jsonl");
    let lines = [a_call("t1", "Bash", serde_json::json!({"command": "ls"})), an_answer("t1")];
    std::fs::write(&transcript, lines.join("\n") + "\n").expect("the record");

    paint(&root, "all done\n\u{276f}\u{a0}");
    let address = inbox::address_in(&root, TTY);
    let letterbox = Inbox::open(&address).expect("a letterbox");
    let (sending, typed) = mpsc::channel();
    std::thread::spawn(move || letterbox.serve(|bytes| drop(sending.send(bytes.to_vec()))));
    Stage { root, transcript, typed }
}

fn paint(root: &Path, text: &str) {
    let path = terminal::screen::address_in(root, TTY);
    terminal::screen::write(&path, std::process::id(), text.as_bytes()).expect("the screen");
    let long_ago = SystemTime::now() - Duration::from_secs(60);
    std::fs::File::options()
        .write(true)
        .open(&path)
        .and_then(|file| file.set_modified(long_ago))
        .expect("the screen has stood still");
}

fn add_to_record(stage: &Stage, lines: &[String]) {
    let mut text = std::fs::read_to_string(&stage.transcript).expect("the record");
    for line in lines {
        text.push_str(line);
        text.push('\n');
    }
    std::fs::write(&stage.transcript, text).expect("the record");
}

fn run(stage: &Stage, dispatch: bool) -> Result<serde_json::Value, flow::ActionError> {
    hand_over(
        &catalog(),
        &stage.root,
        TTY,
        "s-1",
        Some(stage.transcript.to_str().expect("a path")),
        dispatch,
        Duration::ZERO,
    )
}

fn typed(stage: &Stage) -> String {
    let mut seen = Vec::new();
    while let Ok(bytes) = stage.typed.recv_timeout(Duration::from_millis(400)) {
        seen.extend(bytes);
    }
    String::from_utf8_lossy(&seen).into_owned()
}

fn state(stage: &Stage) -> State {
    store(&stage.root)
        .handovers()
        .expect("read")
        .first()
        .expect("a handover")
        .state
}

#[test]
fn every_gate_open_sends_the_clear_once_and_awaits_a_successor() {
    let stage = a_finished_session("open");

    let answer = run(&stage, true).expect("the relay runs");

    assert_eq!(answer["handed_over"], true, "{answer}");
    assert!(typed(&stage).starts_with("/clear"), "the line of the descriptor is typed");
    assert_eq!(state(&stage), State::AwaitingSuccessor);
    let again = run(&stage, true).expect("the relay runs again");
    assert_eq!(again["handed_over"], false, "{again}");
    assert_eq!(typed(&stage), "", "a second run types nothing");
}

#[test]
fn a_new_instruction_after_the_mandate_cancels_the_handover() {
    let stage = a_finished_session("instruction");
    store(&stage.root).record_event(&event("UserPromptSubmit")).expect("a person typed");
    store(&stage.root).record_event(&event("Stop")).expect("and the turn ended");

    let answer = run(&stage, true).expect("the relay runs");

    assert_eq!(answer["handed_over"], false, "{answer}");
    assert_eq!(typed(&stage), "");
    assert_eq!(state(&stage), State::Cancelled);
}

/// A turn that ran after the one that wrote the mandate, with no person asking
/// (a background call that ended wakes the session), may have changed what the
/// mandate says: it is written again, not trusted.
#[test]
fn another_turn_after_the_mandate_cancels_the_handover() {
    let stage = a_finished_session("another-turn");
    store(&stage.root).record_event(&event("Stop")).expect("a second turn ended");

    let answer = run(&stage, true).expect("the relay runs");

    assert_eq!(answer["handed_over"], false, "{answer}");
    assert_eq!(typed(&stage), "");
    assert_eq!(state(&stage), State::Cancelled);
}

#[test]
fn a_sub_agent_still_running_holds_the_session() {
    let stage = a_finished_session("sub-agent");
    store(&stage.root).record_event(&event("SubagentStart")).expect("a sub-agent starts");

    let answer = run(&stage, true).expect("the relay runs");

    assert_eq!(answer["handed_over"], false, "{answer}");
    assert!(answer["why"].as_str().unwrap_or_default().contains("sub-agent"), "{answer}");
    assert_eq!(typed(&stage), "");
    assert_eq!(state(&stage), State::Finished, "held, not cancelled");
}

#[test]
fn a_call_left_in_the_background_holds_the_session_until_it_ends() {
    let stage = a_finished_session("background");
    add_to_record(
        &stage,
        &[a_call("t2", "Bash", serde_json::json!({"command": "cargo test", "run_in_background": true})), an_answer("t2")],
    );

    let held = run(&stage, true).expect("the relay runs");
    assert_eq!(held["handed_over"], false, "{held}");
    assert_eq!(typed(&stage), "");

    add_to_record(&stage, &[serde_json::json!({"type": "queue-operation",
        "content": "<task-notification>\n<tool-use-id>t2</tool-use-id>\n<status>completed</status>"}).to_string()]);
    let ended = run(&stage, true).expect("the relay runs");
    assert_eq!(ended["handed_over"], true, "{ended}");
}

#[test]
fn work_scheduled_after_the_turn_holds_the_session() {
    let stage = a_finished_session("scheduled");
    add_to_record(&stage, &[a_call("t3", "ScheduleWakeup", serde_json::json!({"delaySeconds": 600})), an_answer("t3")]);

    let answer = run(&stage, true).expect("the relay runs");

    assert_eq!(answer["handed_over"], false, "{answer}");
    assert_eq!(typed(&stage), "");
}

#[test]
fn a_screen_asking_a_person_something_holds_the_session() {
    let stage = a_finished_session("asking");
    paint(&stage.root, "Do you want to proceed?\n\u{276f}\u{a0}");

    let answer = run(&stage, true).expect("the relay runs");

    assert_eq!(answer["handed_over"], false, "{answer}");
    assert_eq!(typed(&stage), "");
}

#[test]
fn in_shadow_the_decision_is_recorded_and_nothing_is_typed() {
    let stage = a_finished_session("shadow");

    let answer = run(&stage, false).expect("the relay runs");

    assert_eq!(answer["would_clear"], true, "{answer}");
    assert_eq!(typed(&stage), "");
    assert_eq!(state(&stage), State::Finished);
}

#[test]
fn a_mandate_taken_or_replaced_since_cancels_the_handover() {
    let stage = a_finished_session("replaced");
    let path = sessions::mandate::address_in(&stage.root, TTY);
    sessions::mandate::consume(&path, "somebody", 200).expect("taken");

    let answer = run(&stage, true).expect("the relay runs");

    assert_eq!(answer["handed_over"], false, "{answer}");
    assert_eq!(typed(&stage), "");
    assert_eq!(state(&stage), State::Cancelled);
}

/// **AN UNCERTAIN RESET IS NEVER REPEATED.** Keys that could not be delivered
/// may or may not have landed: the handover stops where a person must look.
#[test]
fn a_clear_that_could_not_be_delivered_is_left_for_a_person_and_not_retried() {
    let stage = a_finished_session("undelivered");
    let _ = std::fs::remove_file(inbox::address_in(&stage.root, TTY));

    let refused = run(&stage, true);

    assert!(refused.is_err(), "{refused:?}");
    assert_eq!(state(&stage), State::RecoveryRequired);
    let again = run(&stage, true).expect("the relay runs again");
    assert_eq!(again["handed_over"], false, "{again}");
}

#[test]
fn a_session_with_no_finished_handover_is_left_alone() {
    let stage = a_finished_session("none");
    let answer = hand_over(&catalog(), &stage.root, TTY, "another-session", None, true, Duration::ZERO)
        .expect("the relay runs");
    assert_eq!(answer["handed_over"], false, "{answer}");
    assert_eq!(typed(&stage), "");
}

#[test]
fn a_record_names_the_calls_left_running() {
    let declared = catalog()
        .live()
        .into_iter()
        .find(|loaded| loaded.descriptor.id == LINE)
        .and_then(|loaded| loaded.descriptor.outlives_the_turn.clone())
        .expect("declared");
    let record = [
        a_call("a", "Bash", serde_json::json!({"run_in_background": true})),
        an_answer("a"),
        a_call("b", "Read", serde_json::json!({})),
    ]
    .join("\n");

    let left = work_left_running(&record, &declared);

    assert_eq!(left.len(), 2, "{left:?}");
}

const GO_ON: &str = "carry on from the mandate";

/// The handover cleared and reserved by `s-2`, the successor, on a free screen.
fn a_reserved_successor(name: &str) -> Stage {
    let stage = a_finished_session(name);
    run(&stage, true).expect("the clear is sent");
    let _ = typed(&stage);
    let sessions = store(&stage.root);
    let id = sessions.handovers().expect("read")[0].id.clone();
    assert!(sessions.reserve(&id, "s-2", 300).expect("reserved"));
    stage
}

fn resume(stage: &Stage) -> Result<serde_json::Value, flow::ActionError> {
    resume_successor(&catalog(), &stage.root, TTY, "s-2", GO_ON, true, Duration::ZERO)
}

#[test]
fn a_reserved_successor_is_set_going_once() {
    let stage = a_reserved_successor("resume");

    let answer = resume(&stage).expect("the relay runs");

    assert_eq!(answer["prompted"], true, "{answer}");
    assert!(typed(&stage).starts_with(GO_ON));
    assert_eq!(state(&stage), State::Prompted);
    let again = resume(&stage).expect("the relay runs again");
    assert_eq!(again["prompted"], false, "{again}");
    assert_eq!(typed(&stage), "");
}

/// A person who already spoke to the successor set it going: nothing is typed.
#[test]
fn a_successor_a_person_already_spoke_to_is_not_prompted_again() {
    let stage = a_reserved_successor("person");
    let mut spoke = event("UserPromptSubmit");
    spoke.session_id = Some("s-2".to_owned());
    store(&stage.root).record_event(&spoke).expect("a person typed");

    let answer = resume(&stage).expect("the relay runs");

    assert_eq!(answer["prompted"], false, "{answer}");
    assert_eq!(typed(&stage), "");
    assert_eq!(state(&stage), State::Resumed);
}

#[test]
fn a_successor_whose_screen_never_comes_free_is_left_for_a_person() {
    let stage = a_reserved_successor("busy");
    paint(&stage.root, "Do you want to proceed?\n\u{276f}\u{a0}");

    let answer = resume(&stage).expect("the relay runs");

    assert_eq!(answer["prompted"], false, "{answer}");
    assert_eq!(typed(&stage), "");
    assert_eq!(state(&stage), State::RecoveryRequired);
}

#[test]
fn a_session_that_reserved_nothing_is_not_prompted() {
    let stage = a_reserved_successor("stranger");
    let answer = resume_successor(&catalog(), &stage.root, TTY, "s-9", GO_ON, true, Duration::ZERO)
        .expect("the relay runs");
    assert_eq!(answer["prompted"], false, "{answer}");
    assert_eq!(typed(&stage), "");
}

/// The line goes to the terminal the handover was cleared on, not to another
/// one the same session is named on.
#[test]
fn a_successor_is_prompted_only_on_the_terminal_that_was_cleared() {
    let stage = a_reserved_successor("other-terminal");
    let answer = resume_successor(&catalog(), &stage.root, "ttys902", "s-2", GO_ON, true, Duration::ZERO)
        .expect("the relay runs");
    assert_eq!(answer["prompted"], false, "{answer}");
    assert_eq!(state(&stage), State::Verifying);
}

/// **THE FLOW OF A START CAN RUN BEFORE ITS GREETING RESERVES.** Seen end to
/// end: the run asked in the same second and found nothing reserved. While a
/// successor is awaited on the terminal, the reservation is waited for.
#[test]
fn a_reservation_that_lands_after_the_flow_started_is_waited_for() {
    let stage = a_finished_session("late-reservation");
    run(&stage, true).expect("the clear is sent");
    let _ = typed(&stage);
    let root = stage.root.clone();
    let reserving = std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(1500));
        let sessions = store(&root);
        let id = sessions.handovers().expect("read")[0].id.clone();
        assert!(sessions.reserve(&id, "s-2", 300).expect("reserved"));
    });

    let answer = resume_successor(&catalog(), &stage.root, TTY, "s-2", GO_ON, true, Duration::from_secs(10))
        .expect("the relay runs");

    reserving.join().expect("the greeting reserved");
    assert_eq!(answer["prompted"], true, "{answer}");
    assert!(typed(&stage).starts_with(GO_ON));
}
