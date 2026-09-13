//! Seeds a scratch ledger for `take-the-next-work` against the fixture
//! projects `flows/tests/make-fixtures.sh` writes: the `CHEAP_WORKER` role and
//! the four `work-queue` records. Usage:
//! `cargo run -p sailor --example seed_take_the_next_work -- FIXTURES_ROOT [CHEAP_WORKER_TOOL] [--role-only]`.
//! `--role-only` writes just the role row, leaving the queue as a later run left it —
//! for re-pointing `CHEAP_WORKER` at a different engine between two measured runs.
//! Point `sailor flow check` at the same store with
//! `SAILOR_LEDGER=FIXTURES_ROOT/store`, which `ledger::default_directory` reads.

use ledger::{Ledger, StoreRecord};
use serde_json::json;

fn main() {
    let mut args = std::env::args().skip(1);
    let root = args
        .next()
        .unwrap_or_else(|| panic!("usage: seed_take_the_next_work FIXTURES_ROOT [CHEAP_WORKER_TOOL] [--role-only]"));
    let root = std::path::PathBuf::from(root);
    let cheap_worker_tool = args.next().unwrap_or_else(|| "claude-code".to_owned());
    let role_only = args.next().as_deref() == Some("--role-only");
    let ledger = Ledger::open(root.join("store")).expect("opening the ledger");

    ledger
        .put_record(&StoreRecord {
            collection: "roles".to_owned(),
            key: "CHEAP_WORKER".to_owned(),
            value: json!({ "tools": [cheap_worker_tool] }),
            written_by: "seed_take_the_next_work".to_owned(),
            written_at: 0,
        })
        .expect("writing the role");

    if role_only {
        println!("role updated in {}", root.join("store").display());
        return;
    }

    let task = |key: &str, project: &str, title: &str, priority: i64| StoreRecord {
        collection: "work-queue".to_owned(),
        key: key.to_owned(),
        value: json!({
            "project": project,
            "title": title,
            "priority": priority,
            "state": "queued",
            "attempts": 0,
            "acceptance": "./check.sh",
            "workspace": root.join(project).to_string_lossy(),
        }),
        written_by: "seed_take_the_next_work".to_owned(),
        written_at: 0,
    };

    for record in [
        task("alpha-top", "alpha", "the task the cheap worker finishes", 100),
        task("alpha-backup", "alpha", "a lower-priority alpha task", 10),
        task("beta-top", "beta", "the task the cheap worker cannot finish", 90),
        task("beta-backup", "beta", "a lower-priority beta task", 5),
    ] {
        ledger.put_record(&record).expect("writing a queue record");
    }

    println!("seeded {}", root.join("store").display());
}
