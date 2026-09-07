//! Who is working on what, right now — the same reading `watch-the-crew`
//! already gives a flow, one command away instead of behind a whole run.
//!
//! **NO NEW MECHANISM.** Nothing here writes: `sailor board` calls the exact
//! function a flow's `work_survey` step calls — [`actions::presence::survey`]
//! — so a person and a flow never read two different boards.

use crate::Form;
use actions::presence::survey;
use ledger::Ledger;

pub const USAGE: &[Form] = &[Form {
    form: "sailor board",
    says_key: "cli.board.says",
}];

pub fn run(_args: &[String]) -> i32 {
    match reading() {
        Ok(said) => {
            println!("{said}");
            0
        }
        Err(why) => {
            eprintln!("sailor board: {why}");
            2
        }
    }
}

fn open_ledger() -> Result<Ledger, String> {
    let directory =
        ledger::default_directory().ok_or_else(|| catalogue::say("cli.no_home", &[]))?;
    Ledger::open(&directory).map_err(|error| error.to_string())
}

fn now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|since| since.as_secs() as i64)
        .unwrap_or(0)
}

fn reading() -> Result<String, String> {
    let store = open_ledger()?;
    let board = survey(&store, None, now()).map_err(|error| error.to_string())?;
    let mut said = String::new();
    for entry in &board.working {
        said.push_str(&catalogue::say(
            "cli.board.row",
            &[
                ("agent", entry["agent"].as_str().unwrap_or("?")),
                ("doing", entry["doing"].as_str().unwrap_or("")),
                ("workdir", entry["workdir"].as_str().unwrap_or("")),
            ],
        ));
        said.push('\n');
    }
    said.push_str(&catalogue::say(
        "cli.board.standing",
        &[("working", &board.working.len().to_string())],
    ));
    Ok(said)
}

#[cfg(test)]
mod tests {
    use super::*;
    use actions::presence::{claim_record, Claim};
    use std::sync::atomic::{AtomicU64, Ordering};

    static NEXT: AtomicU64 = AtomicU64::new(0);

    fn scratch() -> std::path::PathBuf {
        let sequence = NEXT.fetch_add(1, Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!(
            "sailor-board-cmd-{}-{sequence}",
            std::process::id()
        ));
        std::fs::create_dir_all(&dir).expect("the working directory");
        dir
    }

    /// **ONE COPY OF THE READING.** `sailor board` invents no expiry logic of
    /// its own: it asks the same function a flow's `work_survey` step calls,
    /// so it sees exactly what a flow would see.
    #[test]
    fn the_board_reads_what_a_flows_work_survey_would_read() {
        let dir = scratch();
        let store = Ledger::open(&dir).expect("the store");
        store
            .put_record(&claim_record(&Claim {
                agent: "some-agent".to_owned(),
                key: "some-agent#1".to_owned(),
                repository: "some-repo".to_owned(),
                workdir: Some("/some/tree".to_owned()),
                branch: None,
                paths: Vec::new(),
                doing: Some("doing something".to_owned()),
                pid: 1,
                at: now(),
                lease_seconds: 900,
                conversation: None,
                state: "working".to_owned(),
            }))
            .expect("the claim writes");

        let said = reading_at(&dir).expect("the reading");
        assert!(said.contains("some-agent"), "{said}");
        assert!(said.contains("doing something"), "{said}");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Nobody at work is not an error: it is an empty board.
    #[test]
    fn an_empty_board_is_not_an_error() {
        let dir = scratch();
        Ledger::open(&dir).expect("the store, even with no rows");
        let said = reading_at(&dir).expect("the reading of an empty board");
        assert!(said.contains('0'), "{said}");
        let _ = std::fs::remove_dir_all(&dir);
    }

    fn reading_at(dir: &std::path::Path) -> Result<String, String> {
        let store = Ledger::open(dir).map_err(|error| error.to_string())?;
        let board = survey(&store, None, now()).map_err(|error| error.to_string())?;
        let mut said = String::new();
        for entry in &board.working {
            said.push_str(&catalogue::say(
                "cli.board.row",
                &[
                    ("agent", entry["agent"].as_str().unwrap_or("?")),
                    ("doing", entry["doing"].as_str().unwrap_or("")),
                    ("workdir", entry["workdir"].as_str().unwrap_or("")),
                ],
            ));
            said.push('\n');
        }
        said.push_str(&catalogue::say(
            "cli.board.standing",
            &[("working", &board.working.len().to_string())],
        ));
        Ok(said)
    }
}
