//! The whole command end to end: a real `sailor accompany run` in a real
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
    let directory = PathBuf::from("/tmp").join(format!("sr-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&directory);
    std::fs::create_dir_all(&directory).expect("create the test directory");
    directory
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
fn accompanied(directory: &Path) -> Arc<Pty> {
    let workspace = Workspace::open("/tmp").expect("open the workspace");
    let binary = env!("CARGO_BIN_EXE_sailor");
    let arguments: Vec<&OsStr> = vec![
        OsStr::new("accompany"),
        OsStr::new("run"),
        OsStr::new("--store"),
        directory.as_os_str(),
        OsStr::new("--"),
        OsStr::new("/bin/cat"),
    ];
    Arc::new(
        Pty::open(&workspace, OsStr::new(binary), &arguments, Size::default(), &[])
            .expect("open a terminal with sailor accompany inside"),
    )
}

#[test]
fn a_line_pressed_from_outside_reaches_the_program_the_command_started() {
    let directory = scratch("accompany");
    let outer = accompanied(&directory);
    let shown = collect(&outer);

    let room = directory.join("terminals");
    let address = letterbox_under(&room, Duration::from_secs(10))
        .unwrap_or_else(|| panic!("no letterbox appeared in {}: {}", room.display(), seen(&shown)));

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
        "the line pressed from outside must come out of the accompanied terminal: {}",
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
fn without_a_knock_the_accompanied_terminal_shows_nothing() {
    let directory = scratch("accompany-quiet");
    let outer = accompanied(&directory);
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
