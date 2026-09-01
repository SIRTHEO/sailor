//! What a stranger leaves in a terminal's letterbox arrives, and a letterbox
//! a live process is holding is refused instead of taken over.
//!
//! This is what makes typing `/clear` possible without knowing which emulator
//! is on the machine: the descriptor being typed on is ours, so the knock
//! comes to us and never to a product we would have to recognise.

use std::io;
use std::path::PathBuf;
use std::sync::mpsc;
use std::time::Duration;
use terminal::inbox::{self, Inbox};

/// A directory of its own per test, removed when the test ends.
struct Scratch {
    directory: PathBuf,
}

impl Scratch {
    /// Short on purpose: a socket address has a hard length cap and the
    /// per-user temporary directory eats half of it.
    fn new(name: &str) -> Scratch {
        Scratch {
            directory: terminal::scratch::directory(name),
        }
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.directory);
    }
}

#[test]
fn the_address_of_a_terminal_is_its_tty() {
    let store = PathBuf::from("/somewhere/store");
    assert_eq!(
        inbox::address_in(&store, "ttys004"),
        store.join("terminals").join("ttys004.sock"),
        "the address must be computable from what the tracking store already knows"
    );
}

#[test]
fn what_a_stranger_leaves_arrives() {
    let scratch = Scratch::new("delivery");
    let address = inbox::address_in(&scratch.directory, "ttys001");
    let letterbox = Inbox::open(&address).expect("open the letterbox");

    let (sender, receiver) = mpsc::channel();
    let waiting = std::thread::spawn(move || {
        letterbox.serve(|left| {
            let _ = sender.send(left.to_vec());
        });
    });

    inbox::press(&address, b"/clear\r").expect("knock");
    let delivered = receiver
        .recv_timeout(Duration::from_secs(5))
        .expect("the delivery reaches whoever is listening");
    assert_eq!(delivered, b"/clear\r".to_vec());
    drop(waiting);
}

#[test]
fn a_letterbox_someone_is_answering_is_refused() {
    let scratch = Scratch::new("occupied");
    let address = inbox::address_in(&scratch.directory, "ttys002");
    let held = Inbox::open(&address).expect("open it the first time");
    let listening = std::thread::spawn(move || held.serve(|_| {}));
    // The first box must be listening before the second knocks: with nobody
    // accepting, `connect` fails and a live box would look dead.
    std::thread::sleep(Duration::from_millis(50));

    let second = Inbox::open(&address);
    let error = second.err().expect("the second opening must refuse");
    assert_eq!(error.kind(), io::ErrorKind::AddrInUse);
    drop(listening);
}

#[test]
fn a_letterbox_nobody_answers_is_taken_over() {
    let scratch = Scratch::new("stale");
    let address = inbox::address_in(&scratch.directory, "ttys003");
    drop(Inbox::open(&address).expect("open"));
    // A file left behind by a process that died badly: nobody answers, and
    // refusing here would mean that tty never opens again.
    std::fs::create_dir_all(address.parent().expect("the directory")).expect("the directory");
    std::os::unix::net::UnixListener::bind(&address)
        .map(drop)
        .expect("leave a socket file behind");

    Inbox::open(&address).expect("a letterbox nobody answers is taken back");
}

#[test]
fn an_address_too_long_to_reach_is_refused_by_name() {
    let deep = PathBuf::from("/tmp").join("x".repeat(inbox::LONGEST_ADDRESS));
    let error = Inbox::open(&deep)
        .err()
        .expect("an address too long to reach is refused");
    assert_eq!(error.kind(), io::ErrorKind::InvalidInput);
    assert!(
        error.to_string().contains("shorter"),
        "the refusal must say what to do, not only that it is invalid: {error}"
    );
}

/// Whoever measures gets measured: if `press` wrote into the void, the
/// delivery test would stay green with a letterbox that delivers nothing.
#[test]
fn knocking_where_there_is_no_letterbox_fails() {
    let scratch = Scratch::new("nowhere");
    let address = inbox::address_in(&scratch.directory, "ttys999");
    assert!(
        inbox::press(&address, b"hello").is_err(),
        "knocking at a door that is not there cannot succeed"
    );
}
