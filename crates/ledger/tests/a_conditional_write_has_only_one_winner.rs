use ledger::{ConditionalWrite, Ledger, StoreRecord};
use serde_json::json;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};

static NEXT: AtomicU64 = AtomicU64::new(0);

struct Scratch(PathBuf);

impl Scratch {
    fn new() -> Self {
        let serial = NEXT.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!(
            "sailor-conditional-write-{}-{serial}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&path);
        std::fs::create_dir_all(&path).expect("create the test directory");
        Self(path)
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[test]
fn conditional_write_helper() {
    let Ok(root) = std::env::var("CONDITIONAL_WRITE_ROOT") else {
        return;
    };
    let root = PathBuf::from(root);
    let child = std::env::var("CONDITIONAL_WRITE_CHILD").expect("child number");
    std::fs::write(root.join(format!("ready-{child}")), b"")
        .expect("arrive at the barrier");
    while !root.join("go").exists() {
        std::thread::yield_now();
    }
    let result = Ledger::open(root.join("ledger"))
        .expect("open the ledger")
        .put_record_if_absent(&record())
        .expect("conditionally write the record");
    let result = match result {
        ConditionalWrite::Inserted => "inserted",
        ConditionalWrite::AlreadyPresent(_) => "already-present",
    };
    std::fs::write(root.join(format!("result-{child}")), result).expect("report the result");
}

#[test]
fn two_processes_contending_for_a_record_have_only_one_winner() {
    let scratch = Scratch::new();
    drop(Ledger::open(scratch.0.join("ledger")).expect("initialize the ledger"));
    let mut children = (0..2)
        .map(|child| {
            let executable = std::env::current_exe().expect("the test executable");
            let mut command = Command::new(executable);
            command
                .arg("--exact")
                .arg("conditional_write_helper")
                .arg("--nocapture")
                .env("CONDITIONAL_WRITE_ROOT", &scratch.0)
                .env("CONDITIONAL_WRITE_CHILD", child.to_string());
            command.spawn().expect("start a competing process")
        })
        .collect::<Vec<_>>();
    wait_for(&scratch.0, "ready-0");
    wait_for(&scratch.0, "ready-1");
    std::fs::write(scratch.0.join("go"), b"").expect("release both writers");
    for child in &mut children {
        assert!(
            child
                .wait()
                .expect("wait for a competing process")
                .success()
        );
    }

    let results = ["0", "1"]
        .iter()
        .map(|child| std::fs::read_to_string(scratch.0.join(format!("result-{child}"))))
        .collect::<Result<Vec<_>, _>>()
        .expect("read both results");
    assert_eq!(
        results
            .iter()
            .filter(|result| result.as_str() == "inserted")
            .count(),
        1
    );
    assert_eq!(
        results
            .iter()
            .filter(|result| result.as_str() == "already-present")
            .count(),
        1
    );

    let ledger = Ledger::open(scratch.0.join("ledger")).expect("reopen the ledger");
    assert_eq!(
        ledger.events_of_kind("record_written").expect("count events"),
        1
    );
    assert!(matches!(
        ledger
            .put_record_if_absent(&record())
            .expect("write after reopening"),
        ConditionalWrite::AlreadyPresent(_)
    ));
}

fn record() -> StoreRecord {
    StoreRecord {
        collection: "claims".to_owned(),
        key: "work-17".to_owned(),
        value: json!({"runner": "one"}),
        written_by: "a-test".to_owned(),
        written_at: 1,
    }
}

fn wait_for(root: &Path, name: &str) {
    while !root.join(name).exists() {
        std::thread::yield_now();
    }
}
