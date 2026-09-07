//! What this crate may kill, and what it must leave alone.
//!
//! The processes here are real: a fake that answers «gone» would prove nothing
//! about a server that catches `SIGTERM` and keeps the port, which is the case
//! this exists for.

use std::path::PathBuf;
use std::process::{Command, Stdio};

use machine::{left_running, stop_the_ones_nobody_wants, Teardown};

struct Scratch(PathBuf);

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn scratch(label: &str) -> Scratch {
    let dir = std::env::temp_dir().join(format!(
        "machine-{label}-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|since| since.as_nanos())
            .unwrap_or(0)
    ));
    std::fs::create_dir_all(&dir).expect("the scratch directory");
    Scratch(dir)
}

/// Born in its own group, like `Process::start` does it, so the signal this
/// crate sends reaches the same place in the test as in the product.
fn light(line: &str) -> std::process::Child {
    let mut command = Command::new("sh");
    command.arg("-c").arg(line).stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null());
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        command.process_group(0);
    }
    command.spawn().expect("light the process")
}

/// The second we are in. **A ROW IS WRITTEN WHEN THE PROCESS STARTS**, and
/// these tests light real processes: a row dated years back would name a pid
/// this machine has since handed to somebody else, which is a different case
/// and has a test of its own below.
fn now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("a clock after 1970")
        .as_secs() as i64
}

fn written(store: &ledger::Ledger, process_id: &str, pid: u32, run_id: Option<&str>) {
    written_as_of(store, process_id, pid, run_id, now());
}

fn written_as_of(
    store: &ledger::Ledger,
    process_id: &str,
    pid: u32,
    run_id: Option<&str>,
    started_at: i64,
) {
    store
        .record_process_started(&ledger::ProcessRecord {
            process_id: process_id.to_owned(),
            pid,
            command: "sh".to_owned(),
            args: vec!["-c".to_owned()],
            working_directory: "/somewhere".to_owned(),
            port: None,
            purpose: "a-test".to_owned(),
            started_by: "the test".to_owned(),
            run_id: run_id.map(str::to_owned),
            started_at,
        })
        .expect("write the start");
}

/// **UNCERTAINTY STOPS THE ACTION.** Asked whether anybody still wants this and
/// unable to answer, nothing may be signalled: the first version read that
/// answer with `.ok()`, which turned a ledger that would not open into a
/// licence to kill everything it could not ask about.
#[test]
fn a_question_that_cannot_be_answered_kills_nothing() {
    let dir = scratch("uncertain");
    let store = ledger::Ledger::open(&dir.0).expect("the store");
    let mut alive = light("sleep 30");
    written(&store, "p-1", alive.id(), None);

    let asked = stop_the_ones_nobody_wants(&store, 1_700_000_100, &|_| {
        Err(ledger::LedgerError::InvalidRecord("the store would not answer".to_owned()))
    });

    assert!(asked.is_err(), "a question that could not be answered was taken for «nobody wants it»");
    assert!(
        ledger::pid_is_alive(alive.id()),
        "a process was signalled on an answer nobody gave"
    );
    let _ = alive.kill();
    let _ = alive.wait();
}

/// **A SIGNAL IS A REQUEST.** A process that catches `SIGTERM` goes on holding
/// whatever it holds, and a row closed on the strength of `kill` returning zero
/// would hide it from every sweep that follows.
#[test]
fn a_process_that_will_not_leave_keeps_its_row() {
    let dir = scratch("stubborn");
    let store = ledger::Ledger::open(&dir.0).expect("the store");
    let mut deaf = light("trap '' TERM; sleep 30");
    written(&store, "p-deaf", deaf.id(), None);

    let done = stop_the_ones_nobody_wants(&store, 1_700_000_100, &|_| Ok(false))
        .expect("the sweep answers");

    assert!(
        matches!(done.as_slice(), [Teardown::StillThere { .. }]),
        "a process that ignored the signal was reported gone: {done:?}"
    );
    assert_eq!(
        left_running(&store).expect("read").len(),
        1,
        "its row was closed, and the next sweep will never look at it again"
    );
    let _ = deaf.kill();
    let _ = deaf.wait();
}

/// And one that does leave is confirmed gone before its row is closed.
///
/// **SOMEBODY REAPS IT, AND IT IS NOT THE SWEEP.** A pid nobody has waited on
/// stays a zombie, and a zombie answers «alive»: in the product the process
/// that lit it waits, and this sweep runs somewhere else. The thread here is
/// that parent, and without it the test would measure its own negligence.
#[test]
fn a_process_that_leaves_has_its_row_closed() {
    let dir = scratch("leaves");
    let store = ledger::Ledger::open(&dir.0).expect("the store");
    let mut going = light("sleep 30");
    let pid = going.id();
    written(&store, "p-going", pid, None);
    let reaper = std::thread::spawn(move || going.wait());

    let done = stop_the_ones_nobody_wants(&store, 1_700_000_100, &|_| Ok(false))
        .expect("the sweep answers");
    let _ = reaper.join();

    assert!(
        matches!(done.as_slice(), [Teardown::Gone { .. }]),
        "a process that left was not confirmed: {done:?}"
    );
    assert!(left_running(&store).expect("read").is_empty(), "its row is still called running");
}

/// What somebody still wants is never touched, whatever the sweep is for.
#[test]
fn what_is_still_wanted_is_left_alone() {
    let dir = scratch("wanted");
    let store = ledger::Ledger::open(&dir.0).expect("the store");
    let mut alive = light("sleep 30");
    written(&store, "p-wanted", alive.id(), Some("a-run"));

    let done = stop_the_ones_nobody_wants(&store, 1_700_000_100, &|_| Ok(true))
        .expect("the sweep answers");

    assert!(done.is_empty(), "something still wanted was stopped: {done:?}");
    assert!(ledger::pid_is_alive(alive.id()), "it was signalled anyway");
    let _ = alive.kill();
    let _ = alive.wait();
}

/// **THE GRANDCHILD IS THE ONE HOLDING THE PORT.** A child born in its own
/// group starts children of its own; signalling the leader's pid alone lets
/// them through, and the row closes on a leader that took nothing with it.
/// This is the disagreement between the two stop paths this crate exists to
/// end — one signalled the group, the other one pid.
#[test]
fn what_the_leader_started_goes_with_it() {
    let dir = scratch("grandchild");
    let store = ledger::Ledger::open(&dir.0).expect("the store");
    // The leader prints the grandchild's number and then waits on it: killed by
    // its pid alone, the leader goes and `sleep` stays.
    let mut leader = Command::new("sh");
    leader
        .arg("-c")
        .arg("sleep 30 & echo $!; wait")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        leader.process_group(0);
    }
    let mut leader = leader.spawn().expect("light the leader");
    let grandchild: u32 = {
        use std::io::Read;
        let mut said = String::new();
        let mut out = leader.stdout.take().expect("the leader speaks");
        let mut byte = [0u8; 1];
        while out.read(&mut byte).unwrap_or(0) == 1 && byte[0] != b'\n' {
            said.push(byte[0] as char);
        }
        said.trim().parse().expect("a pid on the first line")
    };
    let pid = leader.id();
    written(&store, "p-leader", pid, None);
    let reaper = std::thread::spawn(move || leader.wait());

    stop_the_ones_nobody_wants(&store, 1_700_000_100, &|_| Ok(false)).expect("the sweep answers");
    let _ = reaper.join();

    assert!(
        !ledger::pid_is_alive(grandchild),
        "the leader was stopped and what it started stayed: pid {grandchild} is still there"
    );
}

/// **THE CASE THAT WOULD PUT OUT SOMEBODY ELSE'S WORK.** A row names a number,
/// and numbers come round again. Asked only whether the number is taken, a row
/// written years ago answers for whoever holds it now — and this sweep, whose
/// whole job is to stop what nobody wants, would signal a stranger's group.
///
/// The process here is alive and is not the one the row names. It must be left
/// breathing, and the row must be closed all the same: the process it named
/// really did end, and a row nobody can act on is a row that hides the rest.
#[test]
fn a_row_whose_number_was_handed_on_closes_without_a_signal() {
    let dir = scratch("handed-on");
    let store = ledger::Ledger::open(&dir.0).expect("the store");
    let mut stranger = light("sleep 30");
    // The row claims a process from years back; the pid it names was lit a
    // moment ago, so it cannot be the same one.
    written_as_of(&store, "p-stale", stranger.id(), None, 1_700_000_000);

    let done = stop_the_ones_nobody_wants(&store, now(), &|_| Ok(false))
        .expect("the sweep answers");

    assert!(done.is_empty(), "a stranger's process was signalled: {done:?}");
    assert!(
        ledger::pid_is_alive(stranger.id()),
        "a process nobody recorded was put out because it inherited a number"
    );
    assert!(
        left_running(&store).expect("read").is_empty(),
        "the row is still called running, and every sweep after this reads a stranger as ours"
    );
    let _ = stranger.kill();
    let _ = stranger.wait();
}
