//! **DEVELOPING SAILOR INSIDE SAILOR.** Fault 11 asked that a failed build not
//! take the window away; a build that *succeeds* took it away just as surely,
//! with the pane being typed in and the run being watched inside it. So the
//! two are separated: building stays automatic, **swapping is asked for**.

use std::path::PathBuf;

use supervisor::{turn_now, SwapRequest, Turn};

/// A directory of this test's own, under `SAILOR_TEST_TMP` where it is set.
fn scratch(name: &str) -> PathBuf {
    let root = std::env::var("SAILOR_TEST_TMP")
        .map(PathBuf::from)
        .unwrap_or_else(|_| std::env::temp_dir());
    let mine = root.join(format!("live-swap-{name}-{}", std::process::id()));
    std::fs::create_dir_all(&mine).expect("a directory to write in");
    mine
}

#[test]
fn a_save_builds_and_leaves_the_window_where_it_is() {
    assert_eq!(turn_now(true, false, false, false), Turn::Build);
    // Even with a build already waiting, and even while somebody is asking:
    // what was just saved is built first, and the screen is not touched.
    assert_eq!(turn_now(true, true, true, false), Turn::Build);
}

#[test]
fn the_build_that_is_waiting_goes_on_the_screen_when_it_is_asked_for() {
    assert_eq!(turn_now(false, true, true, false), Turn::Swap);
    assert_eq!(turn_now(false, true, false, false), Turn::Wait);
    // Nothing waiting is nothing to put on: an ask answers with silence.
    assert_eq!(turn_now(false, false, true, false), Turn::Wait);
    assert_eq!(turn_now(false, false, false, false), Turn::Wait);
}

#[test]
fn with_no_window_on_the_screen_there_is_nothing_to_take_away() {
    // The first build of the session, and the one after a window somebody
    // closed: waiting for an ask here would leave an empty desk.
    assert_eq!(turn_now(false, true, false, true), Turn::Swap);
}

#[test]
fn asking_is_a_file_and_the_answer_is_taking_it_away() {
    let path = scratch("ask").join("live-swap");
    assert!(!SwapRequest::take(&path), "nobody asked yet");
    SwapRequest::ask(&path).expect("the ask is written");
    assert!(path.exists(), "the ask is a file the other process can see");
    assert!(SwapRequest::take(&path));
    // **AN ASK IS ANSWERED ONCE.** Left on disk it would swap the window again
    // at the next build, which is the thing this whole file exists to prevent.
    assert!(!SwapRequest::take(&path), "the ask was taken away by the answer");
}

/// **THE LEDGER KNOWS WHO SAILOR STARTED, AND NOBODY ELSE.** A page server left
/// running by hand holds the port all the same, and vite answers a taken port
/// by moving to the next one while the window keeps loading from this one.
#[test]
fn a_port_somebody_else_holds_is_seen_by_asking_the_port() {
    // **BOTH LOCALHOST ADDRESSES**: the page server of this repository sits on
    // `::1`, leaving `127.0.0.1` free.
    for address in ["127.0.0.1", "::1"] {
        let held = std::net::TcpListener::bind((address, 0)).expect("a port to hold");
        let port = held.local_addr().expect("its number").port();
        assert!(
            supervisor::who_holds(port).is_some(),
            "a port held on {address} answered «free»"
        );
        drop(held);
    }
    // The absurd control: with nobody on it, the same question answers nothing.
    // A check that says «taken» about everything would stop every start.
    let free = std::net::TcpListener::bind(("127.0.0.1", 0)).expect("a free port");
    let number = free.local_addr().expect("its number").port();
    drop(free);
    assert_eq!(supervisor::who_holds(number), None);
}
