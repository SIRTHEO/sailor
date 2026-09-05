//! The claim this whole road rests on: a stranger can type into a live session
//! without knowing which emulator drew the window.
//!
//! Nothing here names a terminal product, and nothing here can. The address is
//! a tty, the conduit is a socket, and the descriptor typed on is ours — so
//! this would pass on an emulator written tomorrow.

use std::ffi::OsStr;
use std::io::Read;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use terminal::inbox::{self, Inbox};
use terminal::pty::{Pty, Size};
use terminal::Workspace;

/// A short directory: a socket address has a hard length cap.
fn scratch(name: &str) -> PathBuf {
    terminal::scratch::directory(name).expect("a scratch directory")
}

/// Everything the terminal has shown so far.
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

fn waits_for(shown: &Arc<Mutex<Vec<u8>>>, needle: &str, limit: Duration) -> bool {
    let deadline = Instant::now() + limit;
    while Instant::now() < deadline {
        if seen(shown).contains(needle) {
            return true;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    false
}

#[test]
fn what_is_left_in_the_letterbox_is_typed_into_the_program() {
    let directory = scratch("typed");
    let workspace = Workspace::open("/tmp").expect("open the workspace");
    let inner = Arc::new(
        Pty::open(
            &workspace,
            OsStr::new("/bin/cat"),
            &[],
            Size::default(),
            &[],
        )
        .expect("open a pseudo-terminal with cat inside"),
    );
    let shown = collect(&inner);

    let address = inbox::address_in(&directory, "ttys000");
    let letterbox = Inbox::open(&address).expect("open the letterbox");
    let typing = Arc::clone(&inner);
    std::thread::spawn(move || {
        letterbox.serve(|bytes| {
            let _ = typing.write(bytes);
        });
    });

    inbox::press(&address, b"a-word-from-outside\r").expect("knock");
    assert!(
        waits_for(&shown, "a-word-from-outside", Duration::from_secs(5)),
        "what was left in the letterbox must come out of the terminal: {}",
        seen(&shown)
    );

    let _ = inner.close();
    let _ = std::fs::remove_dir_all(&directory);
}

/// Whoever measures gets measured. With nothing typed the same wait must come
/// back false, or the test above would pass on a letterbox that delivers
/// nothing and a `cat` echoing on its own.
#[test]
fn a_program_nobody_typed_into_shows_nothing() {
    let workspace = Workspace::open("/tmp").expect("open the workspace");
    let inner = Arc::new(
        Pty::open(
            &workspace,
            OsStr::new("/bin/cat"),
            &[],
            Size::default(),
            &[],
        )
        .expect("open a pseudo-terminal with cat inside"),
    );
    let shown = collect(&inner);

    assert!(
        !waits_for(&shown, "a-word-from-outside", Duration::from_millis(500)),
        "with nobody typing, that word cannot appear"
    );

    let _ = inner.close();
}

/// **A TERMINAL BORN THROUGH THE ENGINE'S OWN `open` CAN BE TYPED INTO FROM
/// OUTSIDE**, the way one held by `sailor terminal run` can: given a mailroom,
/// `Terminals::open` registers the letterbox itself, under the tty of the
/// program inside, and a stranger who knows only that tty reaches it.
///
/// This is the terminal the window opens. Without this the relay could type
/// into a session in the person's emulator and not into one in Sailor's.
#[test]
fn a_line_left_in_the_letterbox_reaches_a_terminal_born_through_the_engine() {
    let directory = scratch("engine-born");
    let workspace = Workspace::open(&directory).expect("open the workspace");
    let terminals = terminal::Terminals::with_router(Arc::new(terminal::Router::without_routes(
        Arc::new(terminal::PathLookup::on(toolbox::Machine::bare(directory.clone()))),
    )))
    .with_mailroom(inbox::mailroom(&directory));
    let shown = Arc::new(terminal::Buffer::new());
    let opened = terminals
        .open(
            workspace,
            &terminal::Opening {
                program: "/bin/cat".into(),
                ..terminal::Opening::default()
            },
            |_| Arc::clone(&shown) as Arc<dyn terminal::Output>,
        )
        .expect("open cat through the engine");

    // The address is derived from the tty the list carries, and from nothing
    // this test was told by the engine directly.
    let tty = terminals.list()[0].device.clone();
    let address = inbox::address_in(&directory, &tty);
    inbox::press_line(&address, "a-word-from-outside").expect("knock at the letterbox");
    assert!(
        shown.wait_for("a-word-from-outside", Duration::from_secs(5)),
        "what was left in the letterbox must come out of the terminal: {}",
        shown.text()
    );
    // And the count knows a stranger typed.
    assert!(
        opened.moved() >= "a-word-from-outside\r".len() as u64,
        "what a stranger typed must be counted: {}",
        opened.moved()
    );

    let _ = terminals.close(opened.id());
    let _ = std::fs::remove_dir_all(&directory);
}
