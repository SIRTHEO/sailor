//! A handover is a transaction between one session, its checkpoint and one
//! successor. Every move is a compare-and-swap on the state it leaves, so two
//! controllers racing on the same terminal cannot both empty it, and a move
//! read on stale activity does not happen.

use sessions::handover::{NewHandover, State};
use sessions::{Sessions, TerminalEvent, SESSIONS_FILE};
use std::path::PathBuf;

struct Scratch {
    directory: PathBuf,
}

impl Scratch {
    fn new(label: &str) -> Scratch {
        let directory = std::env::temp_dir().join(format!(
            "sailor-handover-{label}-{}-{:?}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|elapsed| elapsed.as_nanos())
                .unwrap_or_default()
        ));
        std::fs::create_dir_all(&directory).expect("create the test directory");
        Scratch { directory }
    }

    fn store(&self) -> Sessions {
        Sessions::open(self.directory.join(SESSIONS_FILE)).expect("open the sessions")
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.directory);
    }
}

fn event(session: &str, name: &str, at: i64) -> TerminalEvent {
    TerminalEvent {
        tty: "ttys004".to_owned(),
        session_id: Some(session.to_owned()),
        worktree: Some("/work/tree".to_owned()),
        ancestor: None,
        name: name.to_owned(),
        transcript_path: None,
        occurred_at: at,
        payload: None,
    }
}

fn finished(store: &Sessions, mandate_at: i64) -> sessions::handover::Handover {
    store
        .open_handover(&NewHandover {
            tty: "ttys004".to_owned(),
            tree: "/work/tree".to_owned(),
            session: "s-1".to_owned(),
            engine: "a-line".to_owned(),
            mandate_at,
            at: mandate_at,
        })
        .expect("the handover opens")
}

#[test]
fn a_finished_handover_records_the_activity_it_was_written_on() {
    let scratch = Scratch::new("generation");
    let store = scratch.store();
    store.record_event(&event("s-1", "UserPromptSubmit", 10)).expect("event");
    let last = store.record_event(&event("s-1", "Stop", 20)).expect("event");
    store.record_event(&event("s-2", "Stop", 30)).expect("another session");

    let opened = finished(&store, 25);

    assert_eq!(opened.state, State::Finished);
    assert_eq!(opened.generation, last, "only this session's events count");
    assert_eq!(store.activity_of("s-1").expect("read"), last);
}

#[test]
fn a_move_happens_once_and_only_from_the_state_it_names() {
    let scratch = Scratch::new("once");
    let store = scratch.store();
    let opened = finished(&store, 25);

    let generation = store.activity_of("s-1").expect("read");
    assert!(store
        .clear_if_still(&opened.id, generation, 30)
        .expect("the first controller moves it"));
    assert!(!store
        .clear_if_still(&opened.id, generation, 30)
        .expect("the second is told no"));
    assert!(!store
        .advance(&opened.id, State::Finished, State::Cancelled, "late", 31)
        .expect("a move from a state it left does nothing"));
    assert_eq!(
        store.handover(&opened.id).expect("read").expect("there").state,
        State::Clearing
    );
}

/// **ACTIVITY BETWEEN THE CHECKS AND THE KEYS IS A NO.** The controller read the
/// session quiet; an event recorded since then means it is not quiet any more.
#[test]
fn activity_after_the_checks_stops_the_clear() {
    let scratch = Scratch::new("activity");
    let store = scratch.store();
    let opened = finished(&store, 25);
    let read = store.activity_of("s-1").expect("read");
    store.record_event(&event("s-1", "UserPromptSubmit", 40)).expect("a person typed");

    assert!(!store.clear_if_still(&opened.id, read, 41).expect("the check runs"));
    assert_eq!(
        store.handover(&opened.id).expect("read").expect("there").state,
        State::Finished
    );
}

/// A second finished mandate from the same session supersedes the first: two
/// open handovers for one session would be two licences to empty it.
#[test]
fn a_newer_finished_mandate_cancels_the_one_before() {
    let scratch = Scratch::new("supersede");
    let store = scratch.store();
    let first = finished(&store, 25);
    let second = finished(&store, 35);

    assert_eq!(
        store.handover(&first.id).expect("read").expect("there").state,
        State::Cancelled
    );
    assert_eq!(
        store.open_for("ttys004", "s-1").expect("read").map(|it| it.id),
        Some(second.id)
    );
}

/// The successor is looked up by terminal and tree, and only while one is
/// awaited: a handover still being prepared has no successor to wait for.
#[test]
fn a_successor_is_awaited_only_after_the_clear_was_sent() {
    let scratch = Scratch::new("awaited");
    let store = scratch.store();
    let opened = finished(&store, 25);
    assert!(store.awaiting("ttys004", "/work/tree").expect("read").is_none());

    let generation = store.activity_of("s-1").expect("read");
    assert!(store.clear_if_still(&opened.id, generation, 30).expect("moves"));
    assert!(store
        .advance(&opened.id, State::Clearing, State::AwaitingSuccessor, "sent", 31)
        .expect("moves"));

    assert!(store.awaiting("ttys004", "/elsewhere").expect("read").is_none());
    let awaited = store.awaiting("ttys004", "/work/tree").expect("read").expect("awaited");
    assert!(
        !store.reserve(&awaited.id, "s-1", 32).expect("read"),
        "the session that was cleared is not its own successor"
    );
    assert!(store.reserve(&awaited.id, "s-2", 32).expect("the successor reserves it"));
    assert!(!store.reserve(&awaited.id, "s-3", 33).expect("a second one does not"));
    let reserved = store.handover(&awaited.id).expect("read").expect("there");
    assert_eq!(reserved.state, State::Verifying);
    assert_eq!(reserved.successor.as_deref(), Some("s-2"));
    assert_eq!(
        store.successor_in("s-2", State::Verifying).expect("read").map(|it| it.id),
        Some(reserved.id)
    );
}

/// **A MOVE NOBODY FINISHED IS LEFT FOR A PERSON, NOT FOR A RETRY.** A
/// controller that died holding the licence, or a successor that never came,
/// would otherwise hold the row in place forever and in silence.
#[test]
fn a_handover_stuck_past_its_deadline_is_left_for_a_person() {
    let scratch = Scratch::new("overdue");
    let store = scratch.store();
    let opened = finished(&store, 25);
    let generation = store.activity_of("s-1").expect("read");
    assert!(store.clear_if_still(&opened.id, generation, 100).expect("the licence"));

    assert_eq!(store.overdue("ttys004", 100 + 59).expect("read"), 0, "not yet");
    assert_eq!(store.overdue("ttys004", 100 + 61).expect("read"), 1);
    let stuck = store.handover(&opened.id).expect("read").expect("there");
    assert_eq!(stuck.state, State::RecoveryRequired);
    assert!(stuck.why.unwrap_or_default().contains("clearing"));

    let waiting = finished(&store, 400);
    assert_eq!(store.overdue("ttys004", 400 + 100_000).expect("read"), 0, "finished has no deadline");
    assert_eq!(store.handover(&waiting.id).expect("read").expect("there").state, State::Finished);
}
