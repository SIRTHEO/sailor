//! The whole command end to end: a real `sailor terminal run` in a real
//! pseudo-terminal, typed into from outside by a process that shares nothing
//! with it but a socket.
//!
//! The emulator here is this test, which is not one. Nothing between the knock
//! and the program inside asks what drew the window.

use std::ffi::OsStr;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use terminal::inbox;
use terminal::pty::{Pty, Size};
use terminal::Workspace;

/// A short directory: a socket address has a hard length cap.
fn scratch(name: &str) -> PathBuf {
    terminal::scratch::directory(name)
}

fn collect(inner: &Arc<Pty>) -> Arc<Mutex<Vec<u8>>> {
    let shown = Arc::new(Mutex::new(Vec::new()));
    let mut reader = inner.reader().expect("a second end for whoever reads");
    let filling = Arc::clone(&shown);
    std::thread::spawn(move || {
        let mut buffer = [0u8; 1024];
        while let Ok(read) = reader.read(&mut buffer) {
            if read == 0 {
                return;
            }
            filling
                .lock()
                .expect("the lock does not panic")
                .extend_from_slice(&buffer[..read]);
        }
    });
    shown
}

fn seen(shown: &Arc<Mutex<Vec<u8>>>) -> String {
    String::from_utf8_lossy(&shown.lock().expect("the lock does not panic").clone()).into_owned()
}

/// The first letterbox to appear under a mailroom, or nothing before the limit.
fn letterbox_under(room: &Path, limit: Duration) -> Option<PathBuf> {
    let deadline = Instant::now() + limit;
    while Instant::now() < deadline {
        if let Ok(entries) = std::fs::read_dir(room) {
            if let Some(found) = entries.flatten().next() {
                return Some(found.path());
            }
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    None
}

/// A terminal with the real command inside, told to keep its letterboxes in a
/// directory of the test's own instead of the store of whoever runs it.
fn held(directory: &Path) -> Arc<Pty> {
    holding(directory, "/bin/cat")
}

/// The same, with the command line inside named: a shell answers what it was
/// asked, and answering is half of a round trip.
fn holding(directory: &Path, program: &str) -> Arc<Pty> {
    let workspace = Workspace::open("/tmp").expect("open the workspace");
    let binary = env!("CARGO_BIN_EXE_sailor");
    let arguments: Vec<&OsStr> = vec![
        OsStr::new("terminal"),
        OsStr::new("run"),
        OsStr::new("--store"),
        directory.as_os_str(),
        OsStr::new("--"),
        OsStr::new(program),
    ];
    Arc::new(
        Pty::open(
            &workspace,
            OsStr::new(binary),
            &arguments,
            Size::default(),
            &[],
        )
        .expect("open a terminal with sailor terminal inside"),
    )
}

#[test]
fn a_line_pressed_from_outside_reaches_the_program_the_command_started() {
    let directory = scratch("held");
    let outer = held(&directory);
    let shown = collect(&outer);

    let room = directory.join("terminals");
    let address = letterbox_under(&room, Duration::from_secs(10)).unwrap_or_else(|| {
        panic!(
            "no letterbox appeared in {}: {}",
            room.display(),
            seen(&shown)
        )
    });

    // The tracking store records the terminal the agent sees, which is the one
    // the command opened — not the one it was started from. A letterbox named
    // after the outer tty would leave that reader knocking nowhere.
    let named = address
        .file_stem()
        .expect("the letterbox has a name")
        .to_string_lossy()
        .into_owned();
    let started_from = outer.device().trim_start_matches("/dev/").to_owned();
    assert_ne!(
        named, started_from,
        "the letterbox must be named after the inner terminal, not the outer one"
    );

    inbox::press(&address, b"a-word-from-outside\r").expect("knock at the letterbox");

    let deadline = Instant::now() + Duration::from_secs(10);
    while Instant::now() < deadline && !seen(&shown).contains("a-word-from-outside") {
        std::thread::sleep(Duration::from_millis(20));
    }
    assert!(
        seen(&shown).contains("a-word-from-outside"),
        "the line pressed from outside must come out of the terminal Sailor holds: {}",
        seen(&shown)
    );

    // The count is what a relay running outside this process will read, and it
    // only means anything if the pipe actually feeds it.
    let counted = terminal::tally::read(&room.join(format!("{named}.seen")));
    let deadline = Instant::now() + Duration::from_secs(5);
    let mut counted = counted;
    while Instant::now() < deadline && counted.map(|seen| seen.total()).unwrap_or(0) == 0 {
        std::thread::sleep(Duration::from_millis(50));
        counted = terminal::tally::read(&room.join(format!("{named}.seen")));
    }
    let counted = counted.expect("the count lands on disk while the session runs");
    assert!(
        counted.total() > 0,
        "the pipe must feed the count: {counted:?}"
    );
    assert!(
        counted.typed >= "a-word-from-outside\r".len() as u64,
        "what a stranger typed must be counted too: {counted:?}"
    );

    let _ = outer.close();
    let _ = std::fs::remove_dir_all(&directory);
}

/// Whoever measures gets measured: with nobody typing, that word cannot show
/// up on its own, or the test above would pass on a command that ignores the
/// letterbox entirely.
#[test]
fn without_a_knock_the_held_terminal_shows_nothing() {
    let directory = scratch("held-quiet");
    let outer = held(&directory);
    let shown = collect(&outer);

    assert!(
        letterbox_under(&directory.join("terminals"), Duration::from_secs(10)).is_some(),
        "the letterbox must open anyway: {}",
        seen(&shown)
    );
    std::thread::sleep(Duration::from_millis(300));
    assert!(
        !seen(&shown).contains("a-word-from-outside"),
        "with nobody typing, that word cannot appear: {}",
        seen(&shown)
    );

    let _ = outer.close();
    let _ = std::fs::remove_dir_all(&directory);
}

/// Types through the command line itself.
///
/// Through the command and not the library it calls: what is being measured is
/// the loop a person drives, and the command is the half of it that a library
/// call would leave untested.
fn typed(directory: &Path, tty: &str, line: &str) {
    let done = std::process::Command::new(env!("CARGO_BIN_EXE_sailor"))
        .args(["terminal", "press", "--tty", tty, "--text", line, "--store"])
        .arg(directory)
        .output()
        .expect("run sailor terminal press");
    assert!(
        done.status.success(),
        "sailor terminal press refused: {}",
        String::from_utf8_lossy(&done.stderr)
    );
}

/// Waits for something to come out of the terminal, and says what did if it
/// never does.
fn until_shown(shown: &Arc<Mutex<Vec<u8>>>, text: &str, limit: Duration) {
    let deadline = Instant::now() + limit;
    while Instant::now() < deadline && !seen(shown).contains(text) {
        std::thread::sleep(Duration::from_millis(20));
    }
    assert!(
        seen(shown).contains(text),
        "«{text}» never came out of the terminal: {}",
        seen(shown)
    );
}

/// How the terminal ended, if it ended before the limit with nobody killing it.
fn ended_on_its_own(outer: &Arc<Pty>, limit: Duration) -> Option<terminal::Ending> {
    let deadline = Instant::now() + limit;
    loop {
        if let Some(ending) = outer.finished() {
            return Some(ending);
        }
        if Instant::now() >= deadline {
            return None;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
}

/// **THE WHOLE LOOP, AND NOBODY LEFT RUNNING.** Open a terminal Sailor owns
/// with a real command line inside, type into it with the command, read what
/// came back, and let it end by itself.
///
/// Nothing here kills anything: a terminal closed by force would prove the loop
/// closes when someone shuts it, which is not the promise.
#[test]
fn the_round_trip_closes_and_leaves_nobody_running() {
    let directory = scratch("round-trip");
    let outer = holding(&directory, "/bin/sh");
    let shown = collect(&outer);

    let room = directory.join("terminals");
    let address = letterbox_under(&room, Duration::from_secs(10)).unwrap_or_else(|| {
        panic!(
            "no letterbox appeared in {}: {}",
            room.display(),
            seen(&shown)
        )
    });
    let tty = address
        .file_stem()
        .expect("the letterbox has a name")
        .to_string_lossy()
        .into_owned();

    // What comes back is not the echo of what went in: a terminal that typed
    // and ran nothing would still show the line back, and pass.
    typed(&directory, &tty, "echo answered-$((6*7))");
    until_shown(&shown, "answered-42", Duration::from_secs(10));

    typed(&directory, &tty, "exit");
    let ending = ended_on_its_own(&outer, Duration::from_secs(10));
    assert!(
        matches!(ending, Some(terminal::Ending::Exited(_))),
        "the command must end when the program inside does, or it is an orphan: \
         {ending:?}"
    );
    assert!(
        !address.exists(),
        "the letterbox goes when the terminal does: {}",
        address.display()
    );

    let _ = std::fs::remove_dir_all(&directory);
}
