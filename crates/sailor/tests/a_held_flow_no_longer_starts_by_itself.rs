//! A person holds a flow, and nothing starts it by itself until the hold is
//! taken off: not a session event, and not the beat waking a parked run. The
//! same file holds the guard that reads which runs are parked, because both
//! answer from the ledger the runs are written in, and neither did.

use flow::system::FlowSource;
use flow::StepRecord;
use ledger::{Ledger, RunRecord};
use sailor::arc_cmd::{why_it_waits, ALREADY_PARKED, SESSION_EVENT};
use serde_json::json;
use std::path::PathBuf;
use std::process::Command;

struct Scratch(PathBuf);

impl Scratch {
    fn new(name: &str) -> Self {
        let path = std::env::temp_dir().join(format!("sailor-held-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&path);
        std::fs::create_dir_all(path.join("flows")).expect("a place for flows");
        Scratch(path)
    }

    fn ledger(&self) -> Ledger {
        Ledger::open(self.0.join("ledger")).expect("a ledger of this test's own")
    }

    /// One flow whose trigger watches for the end of a turn, so it starts by
    /// itself and a run of it parked is one the beat would wake.
    fn watching(&self, id: &str) -> Vec<FlowSource> {
        self.watching_in(id, id)
    }

    /// The same, in a file named apart from the id it declares.
    fn watching_in(&self, file: &str, id: &str) -> Vec<FlowSource> {
        let with = json!({"source": SESSION_EVENT, "on": {"event": "Stop"}});
        self.written(file, json!({"id": id}), with)
    }

    /// A flow the beat starts every minute, in a file named apart from its id.
    fn scheduled_in(&self, file: &str, id: &str) -> Vec<FlowSource> {
        let head = json!({"id": id, "schedule": {"recurrence": {"kind": "every_seconds", "seconds": 60}, "weight": "light"}});
        self.written(file, head, json!({"source": "manual"}))
    }

    fn written(
        &self,
        file: &str,
        mut flow: serde_json::Value,
        with: serde_json::Value,
    ) -> Vec<FlowSource> {
        flow["description"] = json!("a fixture whose only step is a trigger");
        flow["inputs"] = json!({});
        flow["graph"] = json!({"steps": [{
            "id": "trigger",
            "deps": [],
            "action": "trigger",
            "max_attempts": 1,
            "when": null,
            "with": with,
            "input_schema": {"type": "any"},
            "output_schema": {"type": "any"},
        }]});
        std::fs::write(
            self.0.join("flows").join(format!("{file}.flow.json")),
            serde_json::to_string_pretty(&flow).expect("a flow serialises"),
        )
        .expect("the flow file is written");
        vec![FlowSource {
            origin: "declared",
            dir: self.0.join("flows"),
        }]
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// A run that answered «not yet», lit by an event on that terminal, written
/// the way the engine writes it.
fn parked(ledger: &Ledger, flow: &str, run_id: &str, tty: &str) {
    ledger
        .record_run(&RunRecord {
            run_id: run_id.to_owned(),
            kind: "flow".to_owned(),
            entity: flow.to_owned(),
            parent_run_id: None,
            started_by: "a test".to_owned(),
            status: "not_yet".to_owned(),
            total_cost_micros: 0,
            error: None,
            started_at: 10,
            ended_at: Some(10),
            worktree: None,
            stop_reason: None,
        })
        .expect("recording the run");
    let delivery = json!({"event": "Stop", "tty": tty}).to_string();
    ledger
        .append_step_started(&StepRecord::started(
            run_id,
            "trigger",
            1,
            1,
            vec![],
            json!({ "text": delivery }),
            vec![],
            10,
        ))
        .expect("the trigger step is written");
}

fn status_of(ledger: &Ledger, run_id: &str) -> String {
    ledger
        .run_header(run_id)
        .expect("the header reads")
        .expect("the run is there")
        .status
}

/// **BOTH GUARDS OF THE ARC READ THE LEDGER THE RUNS ARE IN.** The parked
/// guard opened Sailor's home instead, found nothing parked in any of 9,739
/// evaluations, and let one terminal gather several runs of the same flow.
/// One test, because the ledger is named through the environment.
#[test]
fn the_arc_waits_for_a_parked_run_and_for_a_hold_read_where_the_runs_are() {
    let scratch = Scratch::new("arc");
    let ledger = scratch.ledger();
    std::env::set_var("SAILOR_LEDGER", scratch.0.join("ledger"));

    parked(&ledger, "empties", "empties-1", "ttys001");
    assert_eq!(
        why_it_waits("empties", "ttys001").as_deref(),
        Some(ALREADY_PARKED),
        "a run parked on this terminal holds the next one back"
    );
    assert_eq!(
        why_it_waits("empties", "ttys002"),
        None,
        "on another terminal nothing is parked, so it starts"
    );

    ledger
        .hold_flow(
            "asks",
            "it empties sessions nobody finished",
            "a person",
            20,
        )
        .expect("the hold is written");
    let said = why_it_waits("asks", "ttys002").expect("a held flow waits everywhere");
    assert!(
        said.contains("it empties sessions nobody finished"),
        "{said}"
    );
    assert!(said.contains("a person"), "{said}");

    ledger
        .release_flow("asks", "the consent is read when it acts", "a person", 30)
        .expect("the hold comes off");
    assert_eq!(
        why_it_waits("asks", "ttys002"),
        None,
        "off hold, it starts again"
    );

    rusqlite::Connection::open(scratch.0.join("ledger").join("state.db"))
        .and_then(|store| {
            store.execute(
                "INSERT INTO store (collection, key, value, written_by, written_at)
                 VALUES (?1, 'unread', '{}', 'a person', 'not a time')",
                [ledger::flow_holds::FLOW_HOLDS],
            )
        })
        .expect("a hold the store cannot read back");
    let said = why_it_waits("unread", "ttys002").expect("a hold nobody read is not none");
    assert!(
        said.contains("written_at"),
        "it names what it could not read: {said}"
    );
}

/// **A HELD FLOW'S PARKED RUN IS LET GO, NEVER WOKEN**, and the same run
/// under the same flow off hold is woken: the hold is the whole difference.
#[test]
fn the_beat_lets_a_held_flows_parked_run_go_instead_of_waking_it() {
    let scratch = Scratch::new("beat");
    let sources = scratch.watching("empties");
    let ledger = scratch.ledger();
    let mut resumed = Vec::new();
    let mut resume = |run_id: &str| {
        resumed.push(run_id.to_owned());
        Ok(String::new())
    };

    parked(&ledger, "empties", "free", "ttys001");
    let (said, woken, let_go) =
        sailor::flow_cmd::beat::ask_the_parked_again(&sources, &ledger, 100, &mut resume);
    assert_eq!((woken, let_go), (1, 0), "off hold it is woken: {said}");

    ledger
        .hold_flow(
            "empties",
            "held while its consent is mended",
            "a person",
            110,
        )
        .expect("the hold is written");
    parked(&ledger, "empties", "held", "ttys001");
    let (said, woken, let_go) =
        sailor::flow_cmd::beat::ask_the_parked_again(&sources, &ledger, 120, &mut resume);
    assert_eq!((woken, let_go), (0, 2), "held, nothing is woken: {said}");
    assert_eq!(status_of(&ledger, "held"), "stopped", "{said}");
    assert_eq!(resumed, ["free"], "{said}");
}

/// A run is written under its flow's id, and so is a hold: the beat reads
/// both by that id, whatever the file is called.
#[test]
fn the_beat_knows_a_parked_run_by_its_flows_id_not_its_file_name() {
    let scratch = Scratch::new("by-id");
    let sources = scratch.watching_in("a-file-name", "its-id");
    let ledger = scratch.ledger();
    let mut resume = |_run_id: &str| Ok(String::new());

    parked(&ledger, "its-id", "free", "ttys001");
    let (said, woken, let_go) =
        sailor::flow_cmd::beat::ask_the_parked_again(&sources, &ledger, 100, &mut resume);
    assert_eq!((woken, let_go), (1, 0), "{said}");
}

/// The same for a flow that starts on a schedule: its parked run is woken
/// under the id the run carries, not let go for a file name nobody runs.
#[test]
fn the_beat_knows_a_scheduled_flows_parked_run_by_its_id() {
    let scratch = Scratch::new("scheduled-by-id");
    let sources = scratch.scheduled_in("a-file-name", "its-id");
    let ledger = scratch.ledger();
    let mut resume = |_run_id: &str| Ok(String::new());

    parked(&ledger, "its-id", "free", "ttys001");
    let (said, woken, let_go) =
        sailor::flow_cmd::beat::ask_the_parked_again(&sources, &ledger, 100, &mut resume);
    assert_eq!((woken, let_go), (1, 0), "{said}");
}

/// **A HELD FLOW IS NOT LISTED AS DUE.** The due list read the schedule and
/// the last run only, so a flow a person had held still said it was due.
#[test]
fn the_due_list_says_a_held_flow_is_held_not_due() {
    let scratch = Scratch::new("due");
    scratch.scheduled_in("a-file-name", "its-id");
    scratch
        .ledger()
        .hold_flow("its-id", "held while it is mended", "a person", 1)
        .expect("the hold is written");

    let (ok, said) = sailor(&scratch, &["due"]);
    assert!(ok, "{said}");
    let row = said
        .lines()
        .find(|line| line.starts_with("its-id\t"))
        .unwrap_or_else(|| panic!("the flow has a row: {said}"));
    assert!(row.contains("held while it is mended"), "{row}");
    assert!(
        !row.contains("DUE"),
        "a held flow is not said to be due: {row}"
    );
}

/// A hold with no reason is refused, and taking one off keeps the reason it
/// was taken off for, so the story is read back rather than remembered.
#[test]
fn a_hold_and_its_release_each_say_why() {
    let scratch = Scratch::new("why");
    let ledger = scratch.ledger();

    assert!(ledger.hold_flow("empties", "  ", "a person", 1).is_err());
    ledger
        .hold_flow("empties", "held for a reason", "a person", 2)
        .expect("the hold is written");
    assert_eq!(ledger.flow_holds().expect("the holds read").len(), 1);

    ledger
        .release_flow("empties", "mended", "a person", 3)
        .expect("the hold comes off");
    assert!(ledger.flow_holds().expect("the holds read").is_empty());
    let kept = ledger
        .read_record(ledger::flow_holds::FLOW_HOLDS, "empties")
        .expect("the record reads")
        .expect("the release is kept");
    assert_eq!(kept.value["why"], "mended");
}

fn sailor(scratch: &Scratch, args: &[&str]) -> (bool, String) {
    let outside = scratch.0.join("outside");
    std::fs::create_dir_all(&outside).expect("a folder in no project");
    let output = Command::new(env!("CARGO_BIN_EXE_sailor"))
        .args(["flow"])
        .args(args)
        .current_dir(&outside)
        .env("HOME", scratch.0.join("person"))
        .env("SAILOR_HOME", scratch.0.join("home"))
        .env("SAILOR_LEDGER", scratch.0.join("ledger"))
        .env("SAILOR_FLOWS", scratch.0.join("flows"))
        .env_remove("XDG_CONFIG_HOME")
        .output()
        .expect("the built binary starts");
    let said = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    (output.status.success(), said)
}

/// The gestures a person types: a hold needs who and why, the list shows it
/// beside the flow, and taking off a hold that is not there is refused.
#[test]
fn a_person_holds_a_flow_sees_it_held_and_takes_the_hold_off() {
    let scratch = Scratch::new("cli");
    scratch.watching("empties");

    let (ok, said) = sailor(&scratch, &["hold", "empties", "--as", "a-person"]);
    assert!(!ok, "a hold with no reason is refused: {said}");

    let (ok, said) = sailor(
        &scratch,
        &[
            "hold", "empties", "--as", "a-person", "it", "empties", "live", "sessions",
        ],
    );
    assert!(ok, "{said}");
    let (_, list) = sailor(&scratch, &["list"]);
    let row = list
        .lines()
        .find(|line| line.starts_with("empties\t"))
        .unwrap_or_else(|| panic!("the flow is listed: {list}"));
    assert!(
        row.contains("HELD by a-person: it empties live sessions."),
        "{row}"
    );

    let (ok, said) = sailor(
        &scratch,
        &["unhold", "empties", "--as", "a-person", "mended"],
    );
    assert!(ok, "{said}");
    let (_, list) = sailor(&scratch, &["list"]);
    assert!(!list.contains("HELD by"), "{list}");
    let (ok, said) = sailor(
        &scratch,
        &["unhold", "empties", "--as", "a-person", "again"],
    );
    assert!(!ok, "nothing to take off: {said}");
}
