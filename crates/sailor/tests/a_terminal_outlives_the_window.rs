//! A terminal the window opens is not the window's: the pseudo-terminal is
//! owned by `sailor terminal host`, a process of its own, and the window is a
//! client that comes and goes.
//!
//! The window here is this test. It opens a shell through the host, walks
//! away, comes back as a fresh client, and finds the shell still there with
//! what it printed meanwhile. Then the absurd control: with the host gone,
//! the shell is gone too — the pty was the host's, and nobody else's.

use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};
use terminal::host::Client;

const PATIENCE: Duration = Duration::from_secs(15);

fn scratch(name: &str) -> PathBuf {
    terminal::scratch::directory(&format!("outlives-{name}")).expect("a scratch directory")
}

/// The host's process, ended with the test whichever way the test ends: a
/// red assertion used to leave a host and its shell holding a pty for good.
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
    let child = Command::new(env!("CARGO_BIN_EXE_sailor"))
        .args(["terminal", "host", "--store"])
        .arg(store)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("start sailor terminal host");
    let client = Client::in_store(store);
    let deadline = Instant::now() + PATIENCE;
    while client.hello().is_err() {
        assert!(Instant::now() < deadline, "the host never answered");
        std::thread::sleep(Duration::from_millis(20));
    }
    (Host(child), client)
}

fn backlog_text(client: &Client, id: &str) -> String {
    String::from_utf8_lossy(&client.backlog(id).expect("read the backlog").bytes).into_owned()
}

fn until_shown(client: &Client, id: &str, needle: &str) {
    let deadline = Instant::now() + PATIENCE;
    while !backlog_text(client, id).contains(needle) {
        assert!(
            Instant::now() < deadline,
            "«{needle}» never came out: {:?}",
            backlog_text(client, id)
        );
        std::thread::sleep(Duration::from_millis(20));
    }
}

fn pid_is_alive(pid: u32) -> bool {
    unsafe { libc::kill(pid as libc::pid_t, 0) == 0 }
}

/// **THE PTY IS NOT OWNED BY THE WINDOW PROCESS.** A client that opened a
/// shell drops everything it holds; a new client finds the shell alive, with
/// its backlog, and can go on typing into it. Only the host's death ends it.
#[test]
fn a_shell_opened_through_the_host_survives_the_client_that_opened_it() {
    let store = scratch("survives");
    let (mut host, client) = host_under(&store);

    let opened = client
        .open(
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
        .submit(&opened.id, "echo before-$((6*7))")
        .expect("type before leaving");
    until_shown(&client, &opened.id, "before-42");
    let shell_pid = opened.process_id;
    drop(client);

    // The window is gone. Nothing here holds a descriptor of that pty.
    let returning = Client::in_store(&store);
    let listed = returning.list().expect("list after coming back");
    let found = listed
        .iter()
        .find(|row| row.id == opened.id)
        .unwrap_or_else(|| panic!("the shell is not listed any more: {listed:?}"));
    assert!(found.alive, "the shell died with the client: {found:?}");
    assert_eq!(found.process_id, shell_pid, "a different process answers");
    assert!(
        backlog_text(&returning, &opened.id).contains("before-42"),
        "what was printed before leaving is not served on return"
    );
    returning
        .submit(&opened.id, "echo after-$((6*8))")
        .expect("type after coming back");
    until_shown(&returning, &opened.id, "after-48");

    // The same terminal is what `sailor terminal list` reports held, by its
    // tty and with its count: the relay reads that list, not the window's.
    // The count lands on disk on its own cadence, so the list is asked again
    // until it carries a number.
    let deadline = Instant::now() + PATIENCE;
    loop {
        let listing = Command::new(env!("CARGO_BIN_EXE_sailor"))
            .args(["terminal", "list", "--store"])
            .arg(&store)
            .output()
            .expect("run sailor terminal list");
        let said = String::from_utf8_lossy(&listing.stdout).into_owned();
        assert!(
            said.contains(&found.device),
            "the command line does not see the window's terminal as held: {said}"
        );
        if said.contains("bytes") {
            // THE SCREEN'S NUMBER IS THE LIST'S NUMBER. `moved` on the row the
            // window draws and the count `terminal list` prints come from one
            // tally; once the shell is quiet they must agree to the byte.
            std::thread::sleep(Duration::from_millis(700));
            let quiet = Client::in_store(&store)
                .list()
                .expect("list once more")
                .into_iter()
                .find(|row| row.id == opened.id)
                .expect("still listed");
            let again = Command::new(env!("CARGO_BIN_EXE_sailor"))
                .args(["terminal", "list", "--store"])
                .arg(&store)
                .output()
                .expect("run sailor terminal list");
            let line = String::from_utf8_lossy(&again.stdout).into_owned();
            assert!(
                line.contains(&format!("{}   {} bytes", quiet.device, quiet.moved)),
                "the window would show {} bytes and the list says: {line}",
                quiet.moved
            );
            break;
        }
        assert!(
            Instant::now() < deadline,
            "the count of the window's terminal never reached the list: {said}"
        );
        std::thread::sleep(Duration::from_millis(100));
    }

    // THE ABSURD CONTROL: the pty is the host's. Take the host away and the
    // shell inside has nobody holding its terminal.
    host.stop();
    let deadline = Instant::now() + PATIENCE;
    while pid_is_alive(shell_pid) {
        assert!(
            Instant::now() < deadline,
            "the shell outlived the host: its pty is held by somebody else"
        );
        std::thread::sleep(Duration::from_millis(50));
    }
    let _ = std::fs::remove_dir_all(&store);
}

/// A closed terminal is closed for whoever asks next: the list is the host's,
/// and the host was told.
#[test]
fn a_terminal_closed_by_one_client_is_gone_for_the_next() {
    let store = scratch("closed");
    let (mut host, client) = host_under(&store);
    let opened = client
        .open(
            &store.to_string_lossy(),
            Some("/bin/sh".to_owned()),
            Vec::new(),
            Vec::new(),
            24,
            80,
            None,
        )
        .expect("open a shell through the host");
    client.close(&opened.id).expect("close it");
    drop(client);

    let returning = Client::in_store(&store);
    let listed = returning.list().expect("list after coming back");
    assert!(
        listed.iter().all(|row| row.id != opened.id),
        "a closed terminal is still listed: {listed:?}"
    );

    host.stop();
    let _ = std::fs::remove_dir_all(&store);
}
