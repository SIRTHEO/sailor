//! A terminal outlives the sessions that pass through it: one session sending
//! many events, two sessions on one tty at different times, a row left open
//! because the terminal was killed without saying so — and the one that decides
//! the design, **a detach lives on the tty, not on the session**.

//! Detaching a terminal detaches it for the agents that will open one there
//! later too: that is what a person means by "leave this window alone", and not
//! "leave this process alone". **No test here opens the default store**: each
//! has a throwaway file, because opening the real one would measure the machine
//! running the test, which is fault 5.

use sessions::{Anchor, Arrival, SessionError, Sessions, TerminalEvent, SESSIONS_FILE};
use std::path::PathBuf;

/// A throwaway directory for one test only.
struct Scratch {
    directory: PathBuf,
}

impl Scratch {
    fn new(label: &str) -> Scratch {
        let directory = std::env::temp_dir().join(format!(
            "sailor-sessions-{label}-{}-{:?}",
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

fn anchor(tty: &str, worktree: &str) -> Anchor {
    Anchor {
        tty: tty.to_owned(),
        worktree: worktree.to_owned(),
        ancestor: Some("Whatever".to_owned()),
    }
}

fn arrival(tty: &str, worktree: &str, session: &str, at: i64) -> Arrival {
    Arrival {
        anchor: anchor(tty, worktree),
        session_id: Some(session.to_owned()),
        transcript_path: Some(format!("/tmp/{session}.jsonl")),
        at,
    }
}

fn event(tty: &str, session: &str, name: &str, at: i64) -> TerminalEvent {
    TerminalEvent {
        tty: tty.to_owned(),
        session_id: Some(session.to_owned()),
        worktree: Some("/work/sailor".to_owned()),
        ancestor: Some("Whatever".to_owned()),
        name: name.to_owned(),
        transcript_path: None,
        occurred_at: at,
        payload: None,
    }
}

#[test]
fn the_same_session_sends_many_events_and_the_terminal_stays_one() {
    let scratch = Scratch::new("many-events");
    let store = scratch.store();
    store
        .open_terminal(&arrival("ttys001", "/work/sailor", "aaa", 100))
        .expect("open");
    for (index, name) in ["SessionStart", "UserPromptSubmit", "Stop"]
        .iter()
        .enumerate()
    {
        store
            .record_event(&event("ttys001", "aaa", name, 100 + index as i64))
            .expect("record");
    }
    assert_eq!(store.terminals().expect("read").len(), 1);
    let recorded = store.events_on("ttys001").expect("read the events");
    assert_eq!(recorded.len(), 3);
    assert_eq!(recorded[0].name, "SessionStart");
    assert_eq!(recorded[2].name, "Stop");
    assert_eq!(
        store.sessions_on("ttys001").expect("the sessions"),
        vec!["aaa"]
    );
}

#[test]
fn two_sessions_can_share_one_terminal_at_different_times() {
    let scratch = Scratch::new("two-sessions");
    let store = scratch.store();

    store
        .open_terminal(&arrival("ttys001", "/first", "aaa", 100))
        .expect("open the first");
    store
        .record_event(&event("ttys001", "aaa", "SessionStart", 100))
        .expect("record");
    assert!(store.close_terminal("ttys001", 200).expect("close"));

    store
        .open_terminal(&arrival("ttys001", "/second", "bbb", 300))
        .expect("open the second");
    store
        .record_event(&event("ttys001", "bbb", "SessionStart", 300))
        .expect("record");

    let terminals = store.terminals().expect("read");
    assert_eq!(terminals.len(), 1, "the tty is one: {terminals:?}");
    let row = &terminals[0];
    assert_eq!(row.session_id.as_deref(), Some("bbb"));
    assert_eq!(row.worktree, "/second");
    assert_eq!(row.opened_at, 300, "the second opening starts over");
    assert!(row.is_open(), "reopening lifts the earlier close");

    assert_eq!(
        store.sessions_on("ttys001").expect("the sessions"),
        vec!["aaa", "bbb"],
        "the succession of sessions is read from the queue, which is never rewritten"
    );
}

/// A killed terminal closes nothing. The row stays open, and stays open
/// **visibly**: whoever reads the state must be able to say "this one is not
/// alive, it was left there" instead of believing in a session that is gone.
#[test]
fn a_terminal_killed_without_saying_leaves_its_row_open() {
    let scratch = Scratch::new("killed");
    let store = scratch.store();
    store
        .open_terminal(&arrival("ttys004", "/somewhere", "ccc", 10))
        .expect("open");
    let row = store
        .terminal("ttys004")
        .expect("read")
        .expect("it is there");
    assert!(row.is_open());
    assert_eq!(row.closed_at, None);
    assert!(
        !store.close_terminal("ttys009", 20).expect("close"),
        "closing a tty that was never opened must not pretend to have closed anything"
    );
}

/// **THE TEST THAT DECIDES THE DESIGN.** The detach is on the tty: it survives
/// the session that was there, and holds for the one that arrives.
#[test]
fn detaching_holds_the_terminal_and_not_the_session() {
    let scratch = Scratch::new("detach");
    let store = scratch.store();

    store
        .open_terminal(&arrival("ttys002", "/here", "aaa", 100))
        .expect("open");
    store
        .detach(&anchor("ttys002", "/here"), 150)
        .expect("detach");
    assert!(store
        .terminal("ttys002")
        .expect("read")
        .expect("it is there")
        .is_detached());

    // Another agent arrives on the same terminal, later.
    store
        .open_terminal(&arrival("ttys002", "/here", "bbb", 200))
        .expect("open the second");
    let row = store
        .terminal("ttys002")
        .expect("read")
        .expect("it is there");
    assert_eq!(row.session_id.as_deref(), Some("bbb"));
    assert!(
        row.is_detached(),
        "a detached window is detached for whoever arrives after it too: if the \
         detach fell at every opening it would last one session, which is \
         'leave this process alone' and not the 'leave this window alone' that \
         whoever asks for it means"
    );

    assert!(store.attach("ttys002").expect("reattach"));
    assert!(!store
        .terminal("ttys002")
        .expect("read")
        .expect("it is there")
        .is_detached());
    assert!(
        !store.attach("ttys002").expect("reattach twice"),
        "reattaching what is already attached changed nothing, and says so"
    );
}

/// Detaching a terminal nobody has announced yet must stay written down:
/// otherwise `/sailor-off` on a freshly opened window does nothing, and does not
/// say so.
#[test]
fn a_terminal_can_be_detached_before_anyone_has_arrived() {
    let scratch = Scratch::new("detach-first");
    let store = scratch.store();
    store
        .detach(&anchor("ttys007", "/here"), 50)
        .expect("detach");
    let row = store
        .terminal("ttys007")
        .expect("read")
        .expect("it is there");
    assert!(row.is_detached());
    assert_eq!(row.session_id, None);

    store
        .open_terminal(&arrival("ttys007", "/here", "zzz", 60))
        .expect("open after the detach");
    assert!(store
        .terminal("ttys007")
        .expect("read")
        .expect("it is there")
        .is_detached());
}

/// An event from a terminal that was never announced still opens the row: hooks
/// do not arrive in order, and a lost event is lost information.
#[test]
fn an_event_from_an_unannounced_terminal_still_lands() {
    let scratch = Scratch::new("unannounced");
    let store = scratch.store();
    store
        .remember_terminal(&arrival("ttys005", "/elsewhere", "ddd", 10))
        .expect("remember");
    store
        .record_event(&event("ttys005", "ddd", "PostToolUse", 11))
        .expect("record");
    let row = store
        .terminal("ttys005")
        .expect("read")
        .expect("it is there");
    assert_eq!(row.worktree, "/elsewhere");
    assert_eq!(store.events_on("ttys005").expect("the events").len(), 1);
}

/// **THE VERSION IS OURS, AND SO IS THE FILE.** The run ledger has its own
/// `user_version` and raises it when its projections change; this one is
/// unrelated and must not move with it. The test watches the two things that
/// make that true: the number, and that opening the sessions **does not create**
/// `state.db`.
#[test]
fn the_sessions_have_their_own_file_and_their_own_version() {
    let scratch = Scratch::new("version");
    let store = scratch.store();
    assert_eq!(store.schema_version().expect("the version"), 1);
    assert!(store.path().ends_with(SESSIONS_FILE));
    assert!(
        !scratch.directory.join("state.db").exists(),
        "the sessions touched the run ledger"
    );
    assert!(
        !scratch.directory.join("events.db").exists(),
        "the sessions touched the run event log"
    );
}

/// A file written by a newer version is declared, not repaired.
#[test]
fn a_file_from_a_newer_version_is_refused_by_name() {
    let scratch = Scratch::new("newer");
    let path = scratch.directory.join(SESSIONS_FILE);
    {
        let connection = rusqlite::Connection::open(&path).expect("create the file");
        connection
            .pragma_update(None, "user_version", 99_i64)
            .expect("write the version");
    }
    match Sessions::open(&path) {
        Err(SessionError::UnsupportedSchema(found)) => assert_eq!(found, 99),
        Err(other) => panic!("an unknown version must be named for what it is, not \"{other}\""),
        Ok(_) => panic!("an unknown version went through as if it were ours"),
    }
}
