//! `sailor machine turn` runs one heavy command at a time, and a command it
//! started runs inside that turn instead of waiting for its own parent.

use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

fn sailor() -> &'static str {
    env!("CARGO_BIN_EXE_sailor")
}

fn a_ledger(label: &str) -> PathBuf {
    let path = std::env::temp_dir().join(format!(
        "sailor-machine-turn-{label}-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&path);
    std::fs::create_dir_all(&path).expect("a ledger directory");
    path
}

fn in_a_turn(ledger: &PathBuf, script: &str) -> Command {
    let mut command = Command::new(sailor());
    command
        .args(["machine", "turn", "--", "sh", "-c", script])
        .env("SAILOR_LEDGER", ledger)
        .env_remove("SAILOR_MACHINE_TURN")
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    command
}

#[test]
fn a_command_started_inside_a_turn_does_not_wait_for_it() {
    let ledger = a_ledger("inside");
    let inner = format!("'{}' machine turn -- echo inside", sailor());
    let mut outer = in_a_turn(&ledger, &inner).spawn().expect("sailor starts");
    let started = Instant::now();
    while outer.try_wait().expect("it answers").is_none() {
        if started.elapsed() > Duration::from_secs(30) {
            let _ = outer.kill();
            panic!("the inner command waited for its own parent's turn");
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    let said = outer.wait_with_output().expect("its output");

    assert!(
        said.status.success(),
        "{}",
        String::from_utf8_lossy(&said.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&said.stdout).trim(), "inside");
}

#[test]
fn a_second_heavy_command_waits_and_says_for_whom() {
    let ledger = a_ledger("second");
    let marker = ledger.join("first-done");
    let first = in_a_turn(&ledger, &format!("sleep 2; touch '{}'", marker.display()))
        .spawn()
        .expect("the first starts");
    std::thread::sleep(Duration::from_millis(700));
    let second = in_a_turn(&ledger, &format!("test -e '{}'", marker.display()))
        .output()
        .expect("the second runs");
    let first = first.wait_with_output().expect("the first ends");

    assert!(first.status.success());
    assert!(
        second.status.success(),
        "the second ran before the first was done"
    );
    let waited = String::from_utf8_lossy(&second.stderr);
    assert!(
        waited.contains("sleep 2"),
        "the wait did not say whose turn it was: {waited}"
    );
}

#[test]
fn the_exit_of_the_command_is_the_exit_of_the_turn() {
    let ledger = a_ledger("exit");
    let said = in_a_turn(&ledger, "exit 7").output().expect("it runs");

    assert_eq!(said.status.code(), Some(7));
}
