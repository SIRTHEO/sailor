//! The survey of terminals answers from a real sessions file, not from a rule
//! tested against itself.
//!
//! Three states and one store: a terminal still open counts among those
//! working, one that said goodbye and one alive that asked not to be followed
//! are gone *with the reason apart* — one is over, the other somebody else's.

use flow::{ActionOutcome, SharedState};
use serde_json::{json, Value};
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};

/// A sessions file of its own, so no test reads the one this machine keeps.
fn a_store_of_its_own() -> PathBuf {
    static MADE: AtomicUsize = AtomicUsize::new(0);
    let unique = format!(
        "actions-terminals-{}-{}",
        std::process::id(),
        MADE.fetch_add(1, Ordering::Relaxed)
    );
    let directory = std::env::temp_dir().join(unique);
    std::fs::create_dir_all(&directory).expect("a directory to work in");
    directory.join("sessions.db")
}

fn arrival(tty: &str, worktree: &str, at: i64) -> sessions::Arrival {
    sessions::Arrival {
        anchor: sessions::Anchor {
            tty: tty.to_owned(),
            worktree: worktree.to_owned(),
            ancestor: Some("terminal".to_owned()),
        },
        session_id: Some(format!("session-{tty}")),
        transcript_path: None,
        at,
    }
}

fn survey(store: &PathBuf, at: i64) -> Value {
    let mut registry = flow::ActionRegistry::default();
    actions::terminals::register_terminals(&mut registry, None);
    let action = registry
        .get(actions::terminals::TERMINAL_SURVEY_ACTION)
        .expect("the survey is registered");
    let outcome = action
        .execute(
            &json!({"store": store.display().to_string(), "at": at}),
            &SharedState::new(),
        )
        .expect("the survey answers");
    match outcome {
        ActionOutcome::Went(answer) => answer,
        other => panic!("the survey does not wait: {other:?}"),
    }
}

fn ttys(entries: &Value) -> Vec<String> {
    entries
        .as_array()
        .expect("a list")
        .iter()
        .map(|entry| entry["tty"].as_str().expect("a tty").to_owned())
        .collect()
}

#[test]
fn a_terminal_that_closed_and_one_that_detached_are_gone_for_different_reasons() {
    let store = a_store_of_its_own();
    let sessions = sessions::Sessions::open(&store).expect("a store to write");
    sessions
        .open_terminal(&arrival("ttys001", "/trees/one", 1_000))
        .expect("the open one");
    sessions
        .open_terminal(&arrival("ttys002", "/trees/two", 900))
        .expect("the one that closed");
    sessions
        .close_terminal("ttys002", 1_100)
        .expect("it said goodbye");
    sessions
        .open_terminal(&arrival("ttys003", "/trees/three", 800))
        .expect("the one that detached");
    sessions
        .detach(
            &sessions::Anchor {
                tty: "ttys003".to_owned(),
                worktree: "/trees/three".to_owned(),
                ancestor: None,
            },
            1_200,
        )
        .expect("it asked not to be followed");

    let answer = survey(&store, 2_000);

    assert_eq!(
        ttys(&answer["working"]),
        vec!["ttys001".to_owned()],
        "only the terminal still open is working: {answer}"
    );
    let gone = answer["gone"].as_array().expect("a list");
    let why: Vec<(&str, &str)> = gone
        .iter()
        .map(|entry| {
            (
                entry["tty"].as_str().expect("a tty"),
                entry["why"].as_str().expect("a reason"),
            )
        })
        .collect();
    assert!(
        why.contains(&("ttys002", "closed")),
        "the one that said goodbye is closed, not detached: {answer}"
    );
    assert!(
        why.contains(&("ttys003", "detached")),
        "the one alive that asked not to be followed is detached, not closed: {answer}"
    );
}

#[test]
fn the_answer_says_which_tree_and_since_when() {
    let store = a_store_of_its_own();
    let sessions = sessions::Sessions::open(&store).expect("a store to write");
    sessions
        .open_terminal(&arrival("ttys004", "/trees/sailor", 1_000))
        .expect("the terminal");

    let answer = survey(&store, 4_600);
    let working = &answer["working"][0];

    assert_eq!(working["worktree"], json!("/trees/sailor"));
    assert_eq!(working["opened_at"], json!(1_000));
    assert_eq!(
        working["open_for_secs"],
        json!(3_600),
        "an hour of work has to be readable without doing the subtraction: {answer}"
    );
}

#[test]
fn a_field_nobody_knows_is_named_before_the_run() {
    let mut registry = flow::ActionRegistry::default();
    actions::terminals::register_terminals(&mut registry, None);
    let action = registry
        .get(actions::terminals::TERMINAL_SURVEY_ACTION)
        .expect("the survey is registered");

    assert_eq!(
        action.unknown_fields(&json!({"worktre": "/trees/sailor"})),
        vec!["worktre".to_owned()],
        "a typo in a hand-written `with` is a field nobody reads"
    );
}
