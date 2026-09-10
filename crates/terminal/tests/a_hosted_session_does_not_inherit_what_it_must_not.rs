//! A command line Sailor hosts inherits the environment of whoever started
//! the host, and one variable of it can tell the new session it is somebody's
//! child. A session told that answered by writing no record of itself at all,
//! which made the one terminal Sailor holds the one terminal Sailor cannot
//! measure. What must be taken away is declared, never guessed.

use std::ffi::OsStr;
use std::io::Read;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use terminal::pty::{Pty, Size};
use terminal::Workspace;

fn collect(pty: &Arc<Pty>) -> Arc<Mutex<Vec<u8>>> {
    let shown = Arc::new(Mutex::new(Vec::new()));
    let mut reader = pty.reader().expect("a second end to read from");
    let writing = Arc::clone(&shown);
    std::thread::spawn(move || {
        let mut chunk = [0u8; 4096];
        while let Ok(read) = reader.read(&mut chunk) {
            if read == 0 {
                break;
            }
            writing
                .lock()
                .expect("the lock does not panic")
                .extend_from_slice(&chunk[..read]);
        }
    });
    shown
}

fn what_it_showed(shown: &Arc<Mutex<Vec<u8>>>, limit: Duration) -> String {
    let deadline = Instant::now() + limit;
    while Instant::now() < deadline {
        let said =
            String::from_utf8_lossy(&shown.lock().expect("the lock does not panic").clone())
                .into_owned();
        if said.contains("the-answer-is-[") {
            return said;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    String::from_utf8_lossy(&shown.lock().expect("the lock does not panic").clone()).into_owned()
}

/// **EACH TEST ITS OWN VARIABLE.** These run side by side in one process, and
/// a name they shared would be set by one while the other's child is being
/// born.
fn what_the_child_saw(name: &str, not_inherited: &[String]) -> String {
    // Safety: the name belongs to this test alone, and is set before its child.
    unsafe { std::env::set_var(name, "the-host-was-here") };
    let workspace = Workspace::open("/tmp").expect("open the workspace");
    let said = format!("echo the-answer-is-[${name}]");
    let arguments = [OsStr::new("-c"), OsStr::new(said.as_str())];
    let inner = Arc::new(
        Pty::open(
            &workspace,
            OsStr::new("/bin/sh"),
            &arguments,
            Size::default(),
            &[],
            not_inherited,
        )
        .expect("open a pseudo-terminal"),
    );
    let shown = collect(&inner);
    let said = what_it_showed(&shown, Duration::from_secs(5));
    let _ = inner.close();
    said
}

/// **THE CONTROL FIRST.** Without the declaration the variable arrives, so the
/// test below cannot pass on a child that never saw it in the first place.
#[test]
fn a_variable_nobody_took_away_reaches_the_hosted_program() {
    let said = what_the_child_saw("A_VARIABLE_NOBODY_TAKES_AWAY", &[]);

    assert!(
        said.contains("the-answer-is-[the-host-was-here]"),
        "the host's environment reaches the child: {said}"
    );
}

#[test]
fn a_variable_the_line_declares_is_taken_away_before_the_program_starts() {
    let said = what_the_child_saw(
        "A_VARIABLE_THE_LINE_DECLARES",
        &["A_VARIABLE_THE_LINE_DECLARES".to_owned()],
    );

    assert!(
        said.contains("the-answer-is-[]"),
        "the declared variable must not reach the child: {said}"
    );
}
