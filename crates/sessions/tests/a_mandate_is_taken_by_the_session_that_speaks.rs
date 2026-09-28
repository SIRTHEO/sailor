//! A mandate is handed at a session's start and taken once that session speaks.
//!
//! Marked taken by the greeting alone, 27 of 93 went to sessions that started
//! and never ran a turn, and a mandate is taken only once.

use sessions::mandate::{
    address_in, deposit, read, receive, reserve, Mandate, A_RESERVATION_HOLDS_FOR,
};
use std::path::{Path, PathBuf};

struct Scratch(PathBuf);

impl Scratch {
    fn new(name: &str) -> Self {
        let path =
            std::env::temp_dir().join(format!("sailor-received-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&path);
        std::fs::create_dir_all(&path).expect("a directory to write in");
        Scratch(path)
    }

    /// A mandate left at `tty`, and where it waits.
    fn left_at(&self, tty: &str) -> PathBuf {
        let mut mandate = Mandate::default();
        mandate.written.tty = tty.to_owned();
        mandate.written.session = "the-one-that-filled-up".to_owned();
        mandate.work.goal = "carry the work on".to_owned();
        deposit(&self.0, &mandate).expect("the mandate is deposited");
        address_in(&self.0, tty)
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn taken_by(path: &Path) -> Option<String> {
    read(path)
        .expect("still on disk")
        .taken
        .map(|taken| taken.by)
}

#[test]
fn the_greeting_holds_the_mandate_and_only_the_session_that_speaks_takes_it() {
    let scratch = Scratch::new("held");
    let path = scratch.left_at("ttys001");

    let handed = reserve(&path, "the-successor", 100).expect("the greeting holds it");
    assert_eq!(handed.work.goal, "carry the work on");
    assert_eq!(taken_by(&path), None, "handing it is not receiving it");
    assert!(
        reserve(&path, "a-second-greeting", 110).is_err(),
        "held for one session"
    );
    assert!(
        !receive(&path, "a-second-greeting", 111).expect("reads"),
        "not its to take"
    );

    assert!(
        receive(&path, "the-successor", 120).expect("reads"),
        "it speaks, and takes it"
    );
    assert_eq!(taken_by(&path).as_deref(), Some("the-successor"));
    assert!(
        reserve(&path, "a-later-session", 10_000).is_err(),
        "taken once"
    );
}

/// **A SESSION THAT NEVER SPEAKS DOES NOT KEEP IT.** The one that started and
/// was gone in 47 seconds held a mandate nobody was ever handed again.
#[test]
fn a_session_that_never_speaks_lets_the_mandate_go_to_the_next_one() {
    let scratch = Scratch::new("lapsed");
    let path = scratch.left_at("ttys001");

    reserve(&path, "gone-before-its-first-turn", 100).expect("the greeting holds it");
    assert!(reserve(&path, "the-next-one", 100 + A_RESERVATION_HOLDS_FOR - 1).is_err());
    reserve(&path, "the-next-one", 100 + A_RESERVATION_HOLDS_FOR).expect("offered again");

    assert!(!receive(&path, "gone-before-its-first-turn", 200).expect("reads"));
    assert!(receive(&path, "the-next-one", 201).expect("reads"));
    assert_eq!(taken_by(&path).as_deref(), Some("the-next-one"));
}
