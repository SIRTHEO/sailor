//! Heavy work takes the machine one at a time: a second asker waits until the
//! first lets go, the holder's own work does not queue behind it, and nobody
//! gone can hold a place.

use machine::turn::{wait_for_the_machine, who_holds_the_machine, who_waits_for_the_machine};
use std::path::PathBuf;
use std::sync::mpsc;
use std::time::Duration;

fn a_place(label: &str) -> PathBuf {
    let path = std::env::temp_dir().join(format!("sailor-turns-{label}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&path);
    path
}

fn a_gone_pid() -> u32 {
    let mut child = std::process::Command::new("true")
        .spawn()
        .expect("true starts");
    let pid = child.id();
    child.wait().expect("true ends");
    pid
}

#[test]
fn a_second_asker_waits_until_the_first_lets_go() {
    let turns = a_place("second");
    let first = wait_for_the_machine(&turns, "the first build", None, &mut |_, _| {})
        .expect("the first takes it");
    assert_eq!(
        who_holds_the_machine(&turns).map(|held| held.purpose),
        Some("the first build".to_owned())
    );

    let (heard, hear) = mpsc::channel();
    let (took, taken) = mpsc::channel();
    let place = turns.clone();
    let second = std::thread::spawn(move || {
        let turn = wait_for_the_machine(&place, "the second build", None, &mut |ahead, _| {
            let _ = heard.send(ahead.purpose.clone());
        })
        .expect("the second takes it");
        took.send(()).expect("the test listens");
        drop(turn);
    });

    assert_eq!(
        hear.recv_timeout(Duration::from_secs(10)).as_deref(),
        Ok("the first build")
    );
    assert!(
        taken.recv_timeout(Duration::from_millis(1500)).is_err(),
        "the second did not wait"
    );
    assert_eq!(who_waits_for_the_machine(&turns).len(), 1);

    drop(first);
    assert!(
        taken.recv_timeout(Duration::from_secs(10)).is_ok(),
        "the second never got its turn"
    );
    second.join().expect("the second ends");
    assert!(
        who_holds_the_machine(&turns).is_none(),
        "a turn let go is still held"
    );
}

#[test]
fn the_holders_own_work_runs_inside_its_turn() {
    let turns = a_place("inside");
    let held =
        wait_for_the_machine(&turns, "a release", None, &mut |_, _| {}).expect("it takes it");

    let inner = wait_for_the_machine(
        &turns,
        "the release's build",
        Some(held.token()),
        &mut |_, _| panic!("the holder's own work queued behind it"),
    )
    .expect("it runs inside");

    assert!(inner.inherited());
    drop(inner);
    assert!(
        who_holds_the_machine(&turns).is_some(),
        "the inner work let go of its holder's turn"
    );
}

#[test]
fn a_token_the_holder_did_not_hand_out_queues_like_anybody() {
    let turns = a_place("stranger");
    let _held =
        wait_for_the_machine(&turns, "a release", None, &mut |_, _| {}).expect("it takes it");
    let place = turns.clone();
    let (heard, hear) = mpsc::channel();
    std::thread::spawn(move || {
        let _ = wait_for_the_machine(&place, "a stranger", Some("not-the-token"), &mut |_, _| {
            let _ = heard.send(());
        });
    });

    assert!(
        hear.recv_timeout(Duration::from_secs(10)).is_ok(),
        "a wrong token skipped the queue"
    );
}

#[test]
fn a_place_held_by_somebody_gone_holds_nothing() {
    let turns = a_place("gone");
    std::fs::create_dir_all(turns.join("queue")).expect("the queue");
    let gone = a_gone_pid();
    std::fs::write(
        turns.join("queue").join("00000000000000000001-1"),
        format!("{gone}\n\nleft behind\nt\n"),
    )
    .expect("a ticket");
    std::fs::write(
        turns.join("holder"),
        format!("{gone}\n\na crashed build\nt\n"),
    )
    .expect("a holder note");

    assert!(
        who_holds_the_machine(&turns).is_none(),
        "a dead holder still holds"
    );
    let turn = wait_for_the_machine(&turns, "the next build", None, &mut |ahead, _| {
        panic!("waited behind {ahead:?}")
    })
    .expect("it takes it");

    assert!(!turn.inherited());
    assert!(
        who_waits_for_the_machine(&turns).is_empty(),
        "the dead ticket stayed"
    );
}

/// The lock alone is not the queue: with the machine free, whoever asked first
/// and is still there goes first.
#[test]
fn an_older_ticket_goes_first_even_with_the_machine_free() {
    let turns = a_place("order");
    std::fs::create_dir_all(turns.join("queue")).expect("the queue");
    let older = turns.join("queue").join("00000000000000000001-1");
    std::fs::write(
        &older,
        format!("{}\n\nasked first\nt\n", std::process::id()),
    )
    .expect("a ticket");
    let (heard, hear) = mpsc::channel();
    let (took, taken) = mpsc::channel();
    let place = turns.clone();
    let asker = std::thread::spawn(move || {
        let turn = wait_for_the_machine(&place, "asked second", None, &mut |ahead, _| {
            let _ = heard.send(ahead.purpose.clone());
        })
        .expect("it takes it");
        took.send(()).expect("the test listens");
        drop(turn);
    });

    assert_eq!(
        hear.recv_timeout(Duration::from_secs(10)).as_deref(),
        Ok("asked first")
    );
    assert!(
        taken.recv_timeout(Duration::from_millis(1200)).is_err(),
        "the second went first"
    );
    std::fs::remove_file(&older).expect("the first leaves");
    assert!(
        taken.recv_timeout(Duration::from_secs(10)).is_ok(),
        "the second never got its turn"
    );
    asker.join().expect("it ends");
}
