//! What this crate may kill, and what it must leave alone.
//!
//! The processes here are real: a fake that answers «gone» would prove nothing
//! about a server that catches `SIGTERM` and keeps the port, which is the case
//! this exists for.

use std::path::PathBuf;
use std::process::{Command, Stdio};

use machine::{
    close_the_ones_that_stopped_breathing, left_running, stop_the_ones_nobody_wants, Teardown,
};

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

/// The second we are in: a row is written when the process starts.
fn now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("a clock after 1970")
        .as_secs() as i64
}

fn written(store: &ledger::Ledger, process_id: &str, pid: u32, run_id: Option<&str>) {
    written_as_of(store, process_id, pid, run_id, now());
}

fn written_born(store: &ledger::Ledger, process_id: &str, pid: u32, born_at: Option<i64>) {
    store
        .record_process_started(&ledger::ProcessRecord {
            born_at,
            process_id: process_id.to_owned(),
            pid,
            command: "sh".to_owned(),
            args: vec!["-c".to_owned()],
            working_directory: "/somewhere".to_owned(),
            port: None,
            purpose: "a-test".to_owned(),
            started_by: "the test".to_owned(),
            run_id: None,
            started_at: now(),
        })
        .expect("write the start");
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
            born_at: None,
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

/// **A NUMBER COMES ROUND, A BIRTH SECOND DOES NOT.** A row naming a pid that
/// is alive but was born at another second names a process that is gone, and
/// the sweep must close its row instead of signalling whoever holds it now.
#[test]
fn a_row_whose_process_was_born_at_another_second_is_not_that_process() {
    let dir = scratch("rinato");
    let store = ledger::Ledger::open(&dir.0).expect("the store");
    let mut alive = light("sleep 30");
    let pid = alive.id();
    let truly = ledger::born_second_of(pid);

    written_born(&store, "p-mio", pid, truly);
    written_born(&store, "p-di-un-altro", pid, Some(1));

    let seen = left_running(&store).expect("read");
    let held = |process_id: &str| {
        seen.iter()
            .find(|item| item.record.process_id == process_id)
            .map(|item| item.still_alive)
    };
    assert_eq!(held("p-mio"), Some(true), "this is the process the row named");
    assert_eq!(
        held("p-di-un-altro"),
        Some(false),
        "the same number, another second: the process the row named is gone"
    );

    let closed = close_the_ones_that_stopped_breathing(&store, now()).expect("the sweep answers");
    assert_eq!(closed, 1, "only the row nobody holds any more is closed");
    assert!(ledger::pid_is_alive(pid), "and nothing was signalled");
    let _ = alive.kill();
    let _ = alive.wait();
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

/// **THE CASE THAT WOULD PUT OUT SOMEBODY ELSE'S WORK.** A row written years
/// ago answers for whoever holds its number now, and this sweep would signal
/// a stranger's group. It stays breathing; the row closes anyway.
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

/// The four answers in one test: **two asking for an ephemeral port at once
/// take each other's number**, and one test cannot race itself.
#[test]
fn a_port_says_which_of_the_four_things_is_true_about_it() {
    let dir = scratch("port");
    let store = ledger::Ledger::open(&dir.0).expect("the store");

    let held = std::net::TcpListener::bind(("127.0.0.1", 0)).expect("a port to hold");
    let number = held.local_addr().expect("its number").port();
    assert_eq!(
        machine::on_the_port(&store, number).expect("the reading"),
        machine::OnThePort::Somebody,
        "a port held by something sailor never lit read as free"
    );

    // With a row claiming it: **a port Sailor lit is Sailor's**.
    let mut ours = light("sleep 30");
    store
        .record_process_started(&ledger::ProcessRecord {
            born_at: None,
            process_id: "p-port".to_owned(),
            pid: ours.id(),
            command: "npx".to_owned(),
            args: vec!["vite".to_owned()],
            working_directory: "/somewhere".to_owned(),
            port: Some(number),
            purpose: "live".to_owned(),
            started_by: "the test".to_owned(),
            run_id: None,
            started_at: now(),
        })
        .expect("write the start");
    match machine::on_the_port(&store, number).expect("the reading") {
        machine::OnThePort::Ours(record) => assert_eq!(record.pid, ours.id()),
        other => panic!("a port sailor lit was read as a stranger's: {other:?}"),
    }
    let _ = ours.kill();
    let _ = ours.wait();

    // Let go, its row gone with the process: nobody's.
    drop(held);
    let store = ledger::Ledger::open(&scratch("port-free").0).expect("a store with no rows");
    assert_eq!(
        machine::on_the_port(&store, number).expect("the reading"),
        machine::OnThePort::Free,
        "a port nobody holds read as held"
    );
}

/// **A REFUSAL IS NOT AN EMPTY MACHINE.** `left_running` reads Sailor's own
/// rows, so on a machine ground to a halt by processes nobody recorded it
/// answers «0 of 0». This asks the system, and a look it was refused must not
/// come back as «nothing is running» while the machine grinds.
#[test]
fn the_heaviest_are_read_from_the_system_and_sailors_own_are_marked() {
    let ours = std::collections::BTreeSet::from([4321u32]);
    let said = "  4321 175360 node\n   777  20000 sh\n  1313 478160 Orca Helper (Renderer)\n";
    let heaviest = machine::heaviest_of(said, &ours);

    assert_eq!(heaviest.len(), 3, "a readable line was dropped: {heaviest:?}");
    assert_eq!(heaviest[0].pid, 1313, "the list is not heaviest first");
    assert_eq!(heaviest[0].kilobytes, 478_160);
    assert_eq!(heaviest[0].command, "Orca Helper (Renderer)", "a command with spaces was cut");
    assert!(!heaviest[0].sailor_lit, "a process nobody recorded was called sailor's");
    assert!(
        heaviest.iter().any(|one| one.pid == 4321 && one.sailor_lit),
        "a process sailor lit was not marked as its own"
    );
}

/// A line this cannot read is skipped, never guessed at: a header, a truncated
/// row, a number that is not one.
#[test]
fn a_line_that_cannot_be_read_is_left_out_rather_than_invented() {
    let ours = std::collections::BTreeSet::new();
    let said = "  PID   RSS COMM\n   nope 100 x\n   55 notanumber y\n   66 100\n   77 100 real\n";
    let heaviest = machine::heaviest_of(said, &ours);
    assert_eq!(heaviest.len(), 1, "an unreadable line was invented into a row: {heaviest:?}");
    assert_eq!(heaviest[0].pid, 77);
}

/// **THE MACHINE IS SHARED, SO THE CORE COUNT IS A CEILING AND NOT AN ANSWER.**
/// Three gates were killed for memory on 07/09 while a core count of compilers
/// ran beside another session's.
#[test]
fn the_compilers_are_counted_from_what_is_spare_not_from_the_cores() {
    let said = "Mach Virtual Memory Statistics: (page size of 16384 bytes)\n\
                Pages free:                                     3813.\n\
                Pages active:                                 188777.\n\
                Pages inactive:                               186518.\n\
                Pages speculative:                               738.\n\
                Pages wired down:                             211282.\n\
                Pages purgeable:                                   2.\n";
    let machine::Spare::Bytes(spare) = machine::spare_in(said) else {
        panic!("a plain vm_stat reading was refused");
    };
    // Free, inactive, speculative and purgeable pages, and nothing wired or
    // active: those are somebody's.
    assert_eq!(spare, (3813 + 186_518 + 738 + 2) * 16384);

    let plenty = machine::Spare::Bytes(machine::A_COMPILER_WANTS * 100);
    assert_eq!(machine::how_many_compilers(&plenty, 12), 12, "the cores are a ceiling");
    let scarce = machine::Spare::Bytes(machine::A_COMPILER_WANTS * 2);
    assert_eq!(machine::how_many_compilers(&scarce, 12), 2, "it took more than fits");
    let none = machine::Spare::Bytes(0);
    assert_eq!(machine::how_many_compilers(&none, 12), 1, "it asked for no compilers at all");

    // **A REFUSAL IS NOT GROUNDS FOR SLOWING ANYBODY DOWN**, nor for speeding
    // up: it yields the ceiling, which is what cargo would have done unasked.
    let refused = machine::Spare::CouldNotLook("not permitted".to_owned());
    assert_eq!(machine::how_many_compilers(&refused, 12), 12);
    assert!(matches!(machine::spare_in("nothing like vm_stat"), machine::Spare::CouldNotLook(_)));
}
