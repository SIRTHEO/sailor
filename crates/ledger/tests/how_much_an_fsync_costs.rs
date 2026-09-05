//! What a write to the ledger costs on this disk, with `synchronous = FULL`
//! as the store ships it, and with `NORMAL`. A measurement, not a verdict:
//! ignored by default, printed on request.
//!
//! `cargo test -p ledger --test how_much_an_fsync_costs -- --ignored --nocapture`

use ledger::{Ledger, RunRecord, StoreRecord};
use rusqlite::{params, Connection, Transaction, TransactionBehavior};
use serde_json::json;
use std::path::PathBuf;
use std::time::{Duration, Instant};

const WRITES: usize = 200;

struct Scratch(PathBuf);

impl Scratch {
    fn new(label: &str) -> Self {
        let path = std::env::temp_dir().join(format!(
            "sailor-ledger-fsync-{label}-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&path);
        Self(path)
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// The record a terminal's hook writes at every event: a presence claim, in
/// the shape `actions::presence::claim_record` gives it.
fn claim(index: usize) -> StoreRecord {
    let key = format!("terminal#/dev/ttys{index:03}");
    StoreRecord {
        collection: "work-claims".to_owned(),
        key: key.clone(),
        value: json!({
            "agent": "a line (a profile)",
            "repository": "/somewhere/a-repository",
            "workdir": "/somewhere/a-repository",
            "branch": "a-branch",
            "paths": [],
            "doing": null,
            "pid": 4242,
            "renewed_at": 1_700_000_000 + index as i64,
            "expires_at": 1_700_000_000 + index as i64 + 900,
            "released_at": null,
            "gen_ai.agent.name": "a line (a profile)",
            "gen_ai.agent.id": key,
            "gen_ai.conversation.id": "0c8e1a2b-a-conversation",
            "state": "working",
        }),
        written_by: "a line (a profile)".to_owned(),
        written_at: 1_700_000_000 + index as i64,
    }
}

fn run(index: usize) -> RunRecord {
    RunRecord {
        run_id: format!("run-{index:03}"),
        kind: "flow".to_owned(),
        entity: "a-flow".to_owned(),
        parent_run_id: None,
        started_by: "a test".to_owned(),
        status: "running".to_owned(),
        total_cost_micros: 0,
        error: None,
        started_at: 1_700_000_000 + index as i64,
        ended_at: None,
        worktree: None,
    }
}

fn timed(mut write: impl FnMut(usize)) -> Vec<Duration> {
    (0..WRITES)
        .map(|index| {
            let started = Instant::now();
            write(index);
            started.elapsed()
        })
        .collect()
}

fn report(label: &str, samples: &[Duration]) {
    let mut sorted = samples.to_vec();
    sorted.sort();
    let total: Duration = sorted.iter().sum();
    println!(
        "{label}: {WRITES} writes, median {:.3} ms, p90 {:.3} ms, max {:.3} ms, total {:.1} ms",
        sorted[sorted.len() / 2].as_secs_f64() * 1e3,
        sorted[sorted.len() * 9 / 10].as_secs_f64() * 1e3,
        sorted[sorted.len() - 1].as_secs_f64() * 1e3,
        total.as_secs_f64() * 1e3,
    );
}

/// The ledger's own connection, opened again by hand so the test can choose
/// `synchronous`: the store does not expose that knob, and should not.
/// `fullfsync` asks the platform for a flush that reaches the platter, where a
/// plain `fsync` may stop at the drive's cache.
fn reopen(scratch: &Scratch, synchronous: &str, fullfsync: bool) -> Connection {
    let connection = Connection::open(scratch.0.join("state.db")).expect("open the projection");
    connection
        .busy_timeout(Duration::from_secs(5))
        .expect("a busy timeout");
    connection
        .pragma_update(None, "synchronous", synchronous)
        .expect("the projection's synchronous");
    connection
        .pragma_update(None, "fullfsync", fullfsync)
        .expect("the projection's fullfsync");
    connection
        .execute(
            "ATTACH DATABASE ?1 AS events",
            [scratch.0.join("events.db").to_string_lossy().as_ref()],
        )
        .expect("attach the log");
    connection
        .pragma_update(Some("events"), "synchronous", "NORMAL")
        .expect("the log's synchronous");
    connection
}

fn immediate(connection: &mut Connection) -> Transaction<'_> {
    connection
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .expect("an immediate transaction")
}

/// What `apply_pending_events` does for a record: project every event past
/// the watermark into `store`, and move the watermark.
fn project_pending(transaction: &Transaction<'_>) {
    let watermark: i64 = transaction
        .query_row(
            "SELECT last_applied_seq FROM projection_watermark WHERE singleton = 1",
            [],
            |row| row.get(0),
        )
        .expect("the watermark");
    let pending: Vec<(i64, String)> = {
        let mut statement = transaction
            .prepare("SELECT seq, payload FROM events.events WHERE seq > ?1 ORDER BY seq")
            .expect("the pending events");
        statement
            .query_map([watermark], |row| Ok((row.get(0)?, row.get(1)?)))
            .expect("read the pending events")
            .collect::<Result<_, _>>()
            .expect("every pending event")
    };
    for (seq, payload) in pending {
        let event: serde_json::Value = serde_json::from_str(&payload).expect("a payload");
        let record = &event["record"];
        transaction
            .execute(
                "INSERT INTO store (collection, key, value, written_by, written_at)
                 VALUES (?1, ?2, ?3, ?4, ?5)
                 ON CONFLICT(collection, key) DO UPDATE SET
                  value=excluded.value, written_by=excluded.written_by,
                  written_at=excluded.written_at",
                params![
                    record["collection"].as_str(),
                    record["key"].as_str(),
                    record["value"].to_string(),
                    record["written_by"].as_str(),
                    record["written_at"].as_i64(),
                ],
            )
            .expect("project the record");
        transaction
            .execute(
                "UPDATE projection_watermark SET last_applied_seq = ?1 WHERE singleton = 1",
                [seq],
            )
            .expect("move the watermark");
    }
}

/// The two transactions `Ledger::put_record` runs, statement for statement:
/// one appends the event to the log, the next projects it.
fn write_like_the_ledger(connection: &mut Connection, record: &StoreRecord) {
    let payload = json!({ "type": "record_written", "record": record }).to_string();
    let transaction = immediate(connection);
    project_pending(&transaction);
    transaction
        .execute(
            "INSERT INTO events.events
             (kind, run_id, step_id, attempt, epoch, occurred_at, payload)
             VALUES ('record_written', NULL, NULL, NULL, NULL, ?1, ?2)",
            params![record.written_at, payload],
        )
        .expect("append the event");
    transaction.commit().expect("commit the event");
    let transaction = immediate(connection);
    project_pending(&transaction);
    transaction.commit().expect("commit the projection");
}

fn claims_in(ledger: &Ledger) -> usize {
    ledger.records_in("work-claims").expect("the claims").len()
}

/// The hand-written path must write what the ledger reads: rewind the
/// watermark and let a real `open` project all of it again.
fn replayed_by_the_ledger(scratch: &Scratch, connection: Connection) -> usize {
    connection
        .execute(
            "UPDATE projection_watermark SET last_applied_seq = 0 WHERE singleton = 1",
            [],
        )
        .expect("rewind the watermark");
    connection
        .execute("DELETE FROM store", [])
        .expect("empty the projection");
    drop(connection);
    claims_in(&Ledger::open(&scratch.0).expect("reopen the ledger"))
}

#[test]
#[ignore]
fn two_hundred_writes_under_full_and_under_normal() {
    let shipped = Scratch::new("shipped");
    let ledger = Ledger::open(&shipped.0).expect("a ledger");
    let claims = timed(|index| ledger.put_record(&claim(index)).expect("a claim"));
    let runs = timed(|index| ledger.record_run(&run(index)).expect("a run"));
    assert_eq!(claims_in(&ledger), WRITES);
    drop(ledger);

    let full = by_hand("full", "FULL", false);
    let normal = by_hand("normal", "NORMAL", false);
    let flushed = by_hand("flushed", "FULL", true);

    report("put_record, the ledger as shipped (FULL)", &claims);
    report("record_run, the ledger as shipped (FULL)", &runs);
    report("put_record by hand, FULL", &full);
    report("put_record by hand, NORMAL", &normal);
    report("put_record by hand, FULL and fullfsync", &flushed);
}

/// A fresh ledger, written through a connection of the test's own making.
fn by_hand(label: &str, synchronous: &str, fullfsync: bool) -> Vec<Duration> {
    let scratch = Scratch::new(label);
    drop(Ledger::open(&scratch.0).expect("a ledger to reopen"));
    let mut connection = reopen(&scratch, synchronous, fullfsync);
    let samples = timed(|index| write_like_the_ledger(&mut connection, &claim(index)));
    assert_eq!(replayed_by_the_ledger(&scratch, connection), WRITES);
    samples
}
