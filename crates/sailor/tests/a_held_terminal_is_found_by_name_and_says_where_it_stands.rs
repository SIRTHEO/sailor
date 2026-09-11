//! The observation the held terminal exists for: a command runs in a terminal
//! Sailor holds, everything that opened it goes away, it comes back, and the
//! command is still running with its output since.
//!
//! Coming back means asking for a name the opener chose, not a number this run
//! assigned; and a terminal that walked must say so, `cd` being state.

use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};
use terminal::consent::{Consent, Hand};
use terminal::host::Client;

const PATIENCE: Duration = Duration::from_secs(20);

fn scratch(name: &str) -> PathBuf {
    terminal::scratch::directory(&format!("byname-{name}")).expect("a scratch directory")
}

struct Host(Child);

impl Host {
    fn stop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

impl Drop for Host {
    fn drop(&mut self) {
        self.stop();
    }
}

/// The real host, in a process of its own, keeping its files under `store`.
fn host_under(store: &Path) -> (Host, Client) {
    let mut child = Command::new(env!("CARGO_BIN_EXE_sailor"))
        .args(["terminal", "host", "--store"])
        .arg(store)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .expect("start sailor terminal host");
    let client = Client::in_store(store);
    let deadline = Instant::now() + PATIENCE;
    while client.hello().is_err() {
        if Instant::now() >= deadline {
            let _ = child.kill();
            let _ = child.wait();
            panic!("the host never answered under {}", store.display());
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    (Host(child), client)
}

fn backlog_text(client: &Client, id: &str) -> String {
    String::from_utf8_lossy(&client.backlog(id).expect("read the backlog").bytes).into_owned()
}

fn until_shown(client: &Client, id: &str, needle: &str) {
    let deadline = Instant::now() + PATIENCE;
    while Instant::now() < deadline {
        if backlog_text(client, id).contains(needle) {
            return;
        }
        std::thread::sleep(Duration::from_millis(40));
    }
    panic!(
        "«{needle}» never appeared in «{id}»; what came was: {}",
        backlog_text(client, id)
    );
}

/// The highest tick the shell has printed so far, or nothing when it has
/// printed none: what tells output produced while nobody watched from output
/// that was already there before the client walked away.
fn highest_tick(text: &str) -> Option<u32> {
    text.split("tick-")
        .skip(1)
        .filter_map(|rest| {
            let digits: String = rest.chars().take_while(char::is_ascii_digit).collect();
            digits.parse().ok()
        })
        .max()
}

/// **THE ONE OBSERVATION.** A shell opened through the host under a name the
/// opener chose keeps running a long command while every client is gone; a new
/// client asks for that same name and is served the ticks printed meanwhile and
/// the directory the shell walked into before anybody left.
#[test]
fn a_terminal_the_host_holds_is_found_by_its_name_with_what_it_printed_and_where_it_stands() {
    let store = scratch("found");
    let elsewhere = store.join("elsewhere");
    std::fs::create_dir_all(&elsewhere).expect("a directory to walk into");
    let (mut host, client) = host_under(&store);

    let opened = client
        .open_named(
            Some("the-long-one"),
            &store.to_string_lossy(),
            Some("/bin/sh".to_owned()),
            Vec::new(),
            Vec::new(),
            24,
            80,
            None,
        )
        .expect("open a shell through the host under a name");
    assert_eq!(
        opened.id, "the-long-one",
        "the name the opener chose is not the name the terminal carries"
    );
    let shell_pid = opened.process_id;

    client
        .submit("the-long-one", &format!("cd {}", elsewhere.display()))
        .expect("walk the shell somewhere");
    client
        .submit(
            "the-long-one",
            "i=0; while [ $i -lt 600 ]; do i=$((i+1)); echo tick-$i; sleep 1; done",
        )
        .expect("start something long");
    until_shown(&client, "the-long-one", "tick-2");
    let seen_before_leaving = highest_tick(&backlog_text(&client, "the-long-one"))
        .expect("the shell printed at least one tick before anybody left");

    // Everything that opened it goes away. Nothing here holds a descriptor of
    // that pty, and nothing is attached to its output.
    drop(client);
    std::thread::sleep(Duration::from_secs(3));

    let returning = Client::in_store(&store);
    let listed = returning.list().expect("list after coming back");
    let found = listed
        .iter()
        .find(|row| row.id == "the-long-one")
        .unwrap_or_else(|| panic!("the terminal is not there under its name: {listed:?}"));
    assert!(found.alive, "the shell died with the client: {found:?}");
    assert_eq!(found.process_id, shell_pid, "a different process answers");
    assert_eq!(found.program, "sh", "the list does not say what is running");
    assert_eq!(
        Path::new(&found.workspace_root).canonicalize().ok(),
        store.canonicalize().ok(),
        "the list does not say which tree it stands in"
    );
    assert_eq!(
        found.attached, 0,
        "the list claims somebody is attached while nobody is"
    );
    assert_eq!(
        Path::new(&found.directory).canonicalize().ok(),
        elsewhere.canonicalize().ok(),
        "the terminal does not say where it is standing: {found:?}"
    );

    // THE OUTPUT PRODUCED WHILE NOBODY WAS ATTACHED IS THERE. Not the screen it
    // had when the client left: the ticks that came after.
    let since = highest_tick(&backlog_text(&returning, "the-long-one"))
        .expect("the backlog still carries ticks");
    assert!(
        since > seen_before_leaving,
        "nothing was kept from while nobody watched: the last tick before leaving was \
         {seen_before_leaving} and the highest now is {since}"
    );

    // The command is still running: it goes on answering to whoever came back.
    until_shown(&returning, "the-long-one", &format!("tick-{}", since + 2));

    // AND THE LIST SAYS WHEN SOMEBODY IS ATTACHED. A watcher holds its
    // connection; the row it is watching says so while it does.
    let watching = Client::in_store(&store);
    let (stop, told) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let _ = watching.attach("the-long-one", |_| {
            let _ = stop.send(());
        });
    });
    told.recv_timeout(PATIENCE).expect("the watcher saw a frame");
    let deadline = Instant::now() + PATIENCE;
    loop {
        let row = returning
            .list()
            .expect("list while somebody watches")
            .into_iter()
            .find(|row| row.id == "the-long-one")
            .expect("still there");
        if row.attached > 0 {
            break;
        }
        assert!(
            Instant::now() < deadline,
            "a client is attached and the list says nobody is"
        );
        std::thread::sleep(Duration::from_millis(50));
    }

    host.stop();
    let _ = std::fs::remove_dir_all(&store);
}

/// **AN IDLE PROMPT IS NOT CONSENT.** A flow writing into a held terminal is
/// refused until a consent recorded on that terminal names it, and the refusal
/// says which of the two was missing. A person's hand needs no record.
#[test]
fn a_flow_that_no_consent_names_is_refused_and_the_refusal_says_what_was_missing() {
    let store = scratch("consent");
    let (mut host, client) = host_under(&store);
    let opened = client
        .open_named(
            Some("the-working-one"),
            &store.to_string_lossy(),
            Some("/bin/sh".to_owned()),
            Vec::new(),
            Vec::new(),
            24,
            80,
            None,
        )
        .expect("open a shell through the host");
    assert_eq!(opened.id, "the-working-one");

    let refused = client
        .submit_by(
            "the-working-one",
            "/clear",
            &Hand::flow("empty-a-session-that-handed-on", "r-17"),
        )
        .expect_err("a flow nobody consented to writes into a working session");
    assert!(
        refused.contains("empty-a-session-that-handed-on"),
        "the refusal does not name the flow: {refused}"
    );
    assert!(
        refused.contains("no consent"),
        "the refusal does not say what was missing: {refused}"
    );
    assert!(
        !backlog_text(&client, "the-working-one").contains("/clear"),
        "the refused line reached the program anyway"
    );

    let also_refused = client
        .press_by(
            "the-working-one",
            b"\r",
            &Hand::flow("empty-a-session-that-handed-on", "r-17"),
        )
        .expect_err("a bare keystroke from an unconsented flow is a write like any other");
    assert!(also_refused.contains("no consent"), "{also_refused}");

    // A person at the keyboard is their own authority.
    client
        .submit("the-working-one", "echo person-$((6*7))")
        .expect("a person types");
    until_shown(&client, "the-working-one", "person-42");

    // Consented, the same flow goes through; withdrawn, it is refused again.
    client
        .consent(
            "the-working-one",
            &Consent {
                flow: "empty-a-session-that-handed-on".to_owned(),
                given_by: "the owner".to_owned(),
                given_for: "emptying a session that handed on".to_owned(),
            },
        )
        .expect("record a whole consent");
    client
        .submit_by(
            "the-working-one",
            "echo flow-$((6*8))",
            &Hand::flow("empty-a-session-that-handed-on", "r-17"),
        )
        .expect("a consented flow writes");
    until_shown(&client, "the-working-one", "flow-48");

    client
        .withdraw("the-working-one", "empty-a-session-that-handed-on")
        .expect("take the consent back");
    let refused_again = client
        .submit_by(
            "the-working-one",
            "echo never",
            &Hand::flow("empty-a-session-that-handed-on", "r-17"),
        )
        .expect_err("nothing given is given for ever");
    assert!(refused_again.contains("no consent"), "{refused_again}");

    // A consent recording nobody and nothing is not recorded at all.
    let hollow = client
        .consent(
            "the-working-one",
            &Consent {
                flow: "empty-a-session-that-handed-on".to_owned(),
                given_by: String::new(),
                given_for: String::new(),
            },
        )
        .expect_err("an empty consent is not a consent");
    assert!(hollow.contains("who gave it"), "{hollow}");

    host.stop();
    let _ = std::fs::remove_dir_all(&store);
}

/// A name is a name: the host refuses a second terminal under one already
/// held, rather than quietly handing back a different terminal later.
#[test]
fn a_name_the_host_already_holds_is_refused_by_name() {
    let store = scratch("twice");
    let (mut host, client) = host_under(&store);
    let open = |name: &str| {
        client.open_named(
            Some(name),
            &store.to_string_lossy(),
            Some("/bin/sh".to_owned()),
            Vec::new(),
            Vec::new(),
            24,
            80,
            None,
        )
    };
    open("the-only-one").expect("the first takes the name");
    let refused = open("the-only-one").expect_err("the second does not");
    assert!(refused.contains("the-only-one"), "{refused}");

    host.stop();
    let _ = std::fs::remove_dir_all(&store);
}

/// **THE SCREEN THAT OUTLIVES ITS TERMINAL CARRIES THE HOST'S PID.** A hold
/// killed with a signal leaves a file that is still and shows a prompt, and
/// only the painter on its first line tells it from a terminal somebody is
/// working in. Once the pty belongs to the host, that painter is the host.
#[test]
fn a_terminal_the_host_holds_paints_a_screen_signed_with_the_host_s_own_pid() {
    let store = scratch("screen");
    let (mut host, client) = host_under(&store);
    let (_, host_pid) = client.hello().expect("the host greets");
    let opened = client
        .open_named(
            Some("the-painted-one"),
            &store.to_string_lossy(),
            Some("/bin/sh".to_owned()),
            Vec::new(),
            Vec::new(),
            24,
            80,
            None,
        )
        .expect("open a shell through the host");
    client
        .submit("the-painted-one", "echo painted-$((6*7))")
        .expect("print something");
    until_shown(&client, "the-painted-one", "painted-42");

    let where_it_is = terminal::screen::address_in(&store, &opened.device);
    let deadline = Instant::now() + PATIENCE;
    let painted = loop {
        if let Some(painted) = terminal::screen::read(&where_it_is) {
            if String::from_utf8_lossy(&painted.bytes).contains("painted-42") {
                break painted;
            }
        }
        assert!(
            Instant::now() < deadline,
            "the terminal the host holds painted no screen at {}",
            where_it_is.display()
        );
        std::thread::sleep(Duration::from_millis(100));
    };
    assert_eq!(
        painted.by, host_pid,
        "the screen is signed by somebody other than the host that holds the pty"
    );
    assert!(
        painted.is_still_held(),
        "a screen signed by a living host reads as abandoned"
    );

    host.stop();
    let _ = std::fs::remove_dir_all(&store);
}
