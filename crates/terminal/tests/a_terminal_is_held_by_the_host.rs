//! The host owns the terminals; a client comes and goes. What these prove is
//! the conversation between the two: what a late client is served, what
//! crosses the wire as it was given, and what a terminal registers of itself.
//!
//! The host runs on a thread here, in this process. That it can run in
//! another process — the property a window closing depends on — is proved by
//! `crates/sailor/tests/a_terminal_outlives_the_window.rs`, with the binary.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use terminal::host::{self, Client, Frame, Host};
use terminal::{inbox, tally, PathLookup, Router, Terminals};

const PATIENCE: Duration = Duration::from_secs(10);

/// A short directory: the sockets inside have a hard address cap.
fn scratch(name: &str) -> PathBuf {
    terminal::scratch::directory(&format!("host-{name}"))
}

/// A host answering under `directory`, with its letterboxes beside it.
fn host_in(directory: &Path) -> Client {
    let terminals = Terminals::with_router(Arc::new(Router::without_routes(Arc::new(
        PathLookup::current(),
    ))))
    .with_mailroom(inbox::mailroom(directory));
    let host = Arc::new(Host::new(terminals));
    let address = host::address_in(directory);
    std::thread::spawn(move || {
        let _ = host::serve(host, &address);
    });
    let client = Client::in_store(directory);
    let deadline = Instant::now() + PATIENCE;
    while client.hello().is_err() {
        assert!(Instant::now() < deadline, "the host never answered");
        std::thread::sleep(Duration::from_millis(20));
    }
    client
}

/// `/bin/sh` and not the person's shell: a configured shell prints banners
/// and runs things, and the test must speak of the host, not of a home.
fn shell(client: &Client, workspace: &Path, environment: Vec<(String, String)>) -> String {
    client
        .open(
            &workspace.to_string_lossy(),
            Some("/bin/sh".to_owned()),
            Vec::new(),
            environment,
            24,
            80,
            None,
        )
        .expect("open a shell through the host")
        .id
}

fn backlog_text(client: &Client, id: &str) -> String {
    String::from_utf8_lossy(&client.backlog(id).expect("read the backlog").bytes).into_owned()
}

fn waits_for(client: &Client, id: &str, needle: &str, limit: Duration) -> bool {
    let deadline = Instant::now() + limit;
    while Instant::now() < deadline {
        if backlog_text(client, id).contains(needle) {
            return true;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    false
}

/// **WHAT A TERMINAL PRINTED BEFORE ANYONE LOOKED IS SERVED ON ATTACH**, and
/// the live output that follows starts exactly where the backlog ends: a pane
/// joining the two neither misses a byte nor shows one twice.
///
/// The measure that could have come out differently: the first live frame
/// carries an offset, and it is asserted not to fall inside the backlog.
#[test]
fn what_was_printed_before_anyone_looked_is_served_to_whoever_attaches_late() {
    let directory = scratch("late");
    let client = host_in(&directory);
    let id = shell(&client, &directory, Vec::new());

    // Nobody is attached. The shell prints, and the host keeps it.
    client
        .submit(&id, "echo before-$((6*7))")
        .expect("submit a line");
    assert!(
        waits_for(&client, &id, "before-42", PATIENCE),
        "the backlog never showed what was printed while nobody looked: {:?}",
        backlog_text(&client, &id)
    );

    // A pane arrives now: it reads the backlog, then follows the live output.
    let snapshot = client.backlog(&id).expect("read the backlog");
    assert_eq!(
        snapshot.upto,
        snapshot.at + snapshot.bytes.len() as u64,
        "the offsets must describe the bytes served"
    );
    assert!(snapshot.ended.is_none(), "the shell is still running");

    let seen: Arc<Mutex<Vec<Frame>>> = Arc::new(Mutex::new(Vec::new()));
    let filling = Arc::clone(&seen);
    let following = client.clone();
    let followed = id.clone();
    std::thread::spawn(move || {
        let _ = following.attach(&followed, |frame| {
            filling.lock().expect("the lock does not panic").push(frame);
        });
    });
    // The attach must be in place before the next line is typed, or the test
    // would measure a race instead of a seam.
    std::thread::sleep(Duration::from_millis(200));

    client
        .submit(&id, "echo after-$((6*8))")
        .expect("submit another line");
    let deadline = Instant::now() + PATIENCE;
    loop {
        let frames = seen.lock().expect("the lock does not panic").clone();
        let text: String = frames
            .iter()
            .filter_map(|frame| match frame {
                Frame::Chunk { bytes, .. } => Some(String::from_utf8_lossy(bytes).into_owned()),
                Frame::Ended { .. } => None,
            })
            .collect();
        if text.contains("after-48") {
            for frame in &frames {
                if let Frame::Chunk { at, .. } = frame {
                    assert!(
                        *at >= snapshot.upto,
                        "a live frame at {at} falls inside the backlog served up to {}: \
                         the pane would show those bytes twice",
                        snapshot.upto
                    );
                }
            }
            break;
        }
        assert!(
            Instant::now() < deadline,
            "the live output never reached the watcher: {text:?}"
        );
        std::thread::sleep(Duration::from_millis(20));
    }

    client.close(&id).expect("close the terminal");
    let _ = std::fs::remove_dir_all(&directory);
}

/// The absurd control: a terminal nobody typed into cannot show the word.
#[test]
fn a_terminal_nobody_typed_into_shows_no_such_word() {
    let directory = scratch("quiet");
    let client = host_in(&directory);
    let id = shell(&client, &directory, Vec::new());
    assert!(
        !waits_for(&client, &id, "before-42", Duration::from_millis(500)),
        "with nobody typing, that word cannot appear"
    );
    client.close(&id).expect("close the terminal");
    let _ = std::fs::remove_dir_all(&directory);
}

/// **ARGUMENTS CROSS THE WIRE AS ARGUMENTS.** `program` and `args` are two
/// fields, and the program starts with them. The control is the defect this
/// guards: a program named with its arguments inside one string is looked up
/// as a binary of that name, and refused.
#[test]
fn arguments_cross_the_wire_as_arguments_and_not_as_part_of_the_name() {
    let directory = scratch("args");
    let client = host_in(&directory);
    let root = directory.to_string_lossy().into_owned();

    let argued = client
        .open(
            &root,
            Some("/bin/sh".to_owned()),
            vec!["-c".to_owned(), "echo argued-$((7*6))".to_owned()],
            Vec::new(),
            24,
            80,
            None,
        )
        .expect("open a program with arguments");
    assert!(
        waits_for(&client, &argued.id, "argued-42", PATIENCE),
        "the arguments did not reach the program: {:?}",
        backlog_text(&client, &argued.id)
    );

    let refused = client.open(
        &root,
        Some("/bin/sh -c 'echo argued'".to_owned()),
        Vec::new(),
        Vec::new(),
        24,
        80,
        None,
    );
    assert!(
        refused.is_err(),
        "a name with arguments inside it is not a program, and must not start: {refused:?}"
    );

    let _ = client.close(&argued.id);
    let _ = std::fs::remove_dir_all(&directory);
}

/// **THE ENVIRONMENT GIVEN AT OPENING IS THE PROGRAM'S ENVIRONMENT.** The
/// child is asked, and answers with the value the opening carried; without
/// it, that value is not there to be answered.
#[test]
fn the_environment_given_at_opening_reaches_the_program_inside() {
    let directory = scratch("env");
    let client = host_in(&directory);

    let profiled = shell(
        &client,
        &directory,
        vec![(
            "CLAUDE_CONFIG_DIR".to_owned(),
            "/home/of/the/prove/profile".to_owned(),
        )],
    );
    client
        .submit(&profiled, "echo home=$CLAUDE_CONFIG_DIR")
        .expect("ask the child");
    assert!(
        waits_for(&client, &profiled, "home=/home/of/the/prove/profile", PATIENCE),
        "the child did not see the profile's home: {:?}",
        backlog_text(&client, &profiled)
    );

    let bare = shell(&client, &directory, Vec::new());
    client
        .submit(&bare, "echo home=$CLAUDE_CONFIG_DIR")
        .expect("ask the child");
    assert!(
        !waits_for(&client, &bare, "home=/home/of/the/prove/profile", Duration::from_secs(2)),
        "a terminal opened with no environment answered with the profile's home"
    );

    let _ = client.close(&profiled);
    let _ = client.close(&bare);
    let _ = std::fs::remove_dir_all(&directory);
}

/// **A TERMINAL THE HOST OPENED HAS A LETTERBOX AND A COUNT**, keyed on the
/// tty the list carries, and both go when the terminal does. The letterbox
/// is knocked at, not looked for: a file left behind answers nobody.
#[test]
fn a_terminal_the_host_opened_has_a_letterbox_and_a_count_under_its_tty() {
    let directory = scratch("letterbox");
    let client = host_in(&directory);
    let id = shell(&client, &directory, Vec::new());

    let listed = client.list().expect("list");
    let row = listed
        .iter()
        .find(|row| row.id == id)
        .expect("the terminal is listed");
    let address = inbox::address_in(&directory, &row.device);
    assert!(
        std::os::unix::net::UnixStream::connect(&address).is_ok(),
        "nobody answers at {}",
        address.display()
    );

    // The count lands on disk while the session runs, under the same tty.
    let counted = tally::address_in(&directory, &row.device);
    let deadline = Instant::now() + PATIENCE;
    while tally::read(&counted).map_or(true, |seen| seen.total() == 0) {
        assert!(
            Instant::now() < deadline,
            "no count landed at {}",
            counted.display()
        );
        std::thread::sleep(Duration::from_millis(50));
    }

    client.close(&id).expect("close the terminal");
    let deadline = Instant::now() + PATIENCE;
    while address.exists() || counted.exists() {
        assert!(
            Instant::now() < deadline,
            "the letterbox or the count outlived the terminal: {} {}",
            address.exists(),
            counted.exists()
        );
        std::thread::sleep(Duration::from_millis(50));
    }
    assert!(
        std::os::unix::net::UnixStream::connect(&address).is_err(),
        "a closed terminal must answer nobody"
    );
    let _ = std::fs::remove_dir_all(&directory);
}

/// **THE LIST SAYS WHICH SESSION EACH TERMINAL IS.** The device the list
/// carries is the one the program inside reports as its own.
#[test]
fn the_device_the_list_carries_is_the_one_the_program_inside_reports() {
    let directory = scratch("device");
    let client = host_in(&directory);
    let id = shell(&client, &directory, Vec::new());
    let device = client.list().expect("list")[0].device.clone();
    client.submit(&id, "tty").expect("ask the child");
    assert!(
        waits_for(&client, &id, &format!("/dev/{device}"), PATIENCE),
        "the program reports another terminal than the list: {:?}",
        backlog_text(&client, &id)
    );
    client.close(&id).expect("close the terminal");
    let _ = std::fs::remove_dir_all(&directory);
}

/// Asking about a terminal that is not there is refused by name, and the
/// refusal says what is open instead of only what is not.
#[test]
fn a_terminal_nobody_opened_is_refused_by_name() {
    let directory = scratch("unknown");
    let client = host_in(&directory);
    let refusal = client
        .submit("nobody-9", "ls")
        .err()
        .expect("an unknown terminal is refused");
    assert!(refusal.contains("nobody-9"), "{refusal}");
    let _ = std::fs::remove_dir_all(&directory);
}
