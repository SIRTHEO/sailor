//! The ledger, asked the questions nobody was asking it.
//!
//! **SAILOR RECORDS EVERYTHING AND NEVER GOES BACK TO READ IT** — the mandate
//! of August said so, and the window still showed only the history of runs.
//! What is here is the rest: processes left standing, runs that never closed,
//! failures counted by class, and the store a flow writes into.

use serde::Serialize;

/// A process the ledger recorded as started and never as ended.
///
/// **AND WHETHER ITS PID IS STILL ALIVE**, asked now rather than assumed: a
/// record left open is not the same fact as a process still running, and the
/// orphan-port fault was exactly the gap between the two.
#[derive(Serialize)]
pub(crate) struct Leftover {
    process_id: String,
    pid: u32,
    command: String,
    working_directory: String,
    port: Option<u16>,
    alive: bool,
}

#[derive(Serialize)]
pub(crate) struct OpenRun {
    run_id: String,
    entity: String,
    open_steps: usize,
    oldest_started_at: i64,
}

#[derive(Serialize)]
pub(crate) struct Waiting {
    run_id: String,
    entity: String,
    waiting_since: i64,
}

/// How the broken steps of recent runs fall into classes.
///
/// **A CLASS THAT IS MISSING IS NOT A CLASS CALLED «UNKNOWN».** The engine
/// could not classify that failure, and saying so is what keeps somebody from
/// counting it among a class they recognise.
#[derive(Serialize)]
pub(crate) struct FailureClass {
    class: Option<String>,
    failures: i64,
    runs_affected: i64,
}

#[derive(Serialize)]
pub(crate) struct Kept {
    collection: String,
    key: String,
}

/// What the ledger holds, or why it holds nothing.
///
/// **«NOT CREATED YET» IS NOT «EMPTY».** A ledger that has never been written
/// and one that has been emptied read identically from a count, and the first
/// is the normal state of a fresh install — the window says which.
#[derive(Serialize)]
pub(crate) struct Held {
    /// Where it is, or would be.
    directory: String,
    exists: bool,
    runs: i64,
    unfinished: Vec<OpenRun>,
    waiting: Vec<Waiting>,
    leftovers: Vec<Leftover>,
    failures: Vec<FailureClass>,
    /// Every collection a flow has written into, and its keys.
    kept: Vec<Kept>,
    /// What is in the inventory now, and what was there and is gone.
    inventory_present: usize,
    inventory_gone: usize,
}

/// How many runs back the failure tally looks. Not «all of them»: a class that
/// stopped happening two hundred runs ago is history, and mixed into today's
/// count it hides the one that started yesterday.
const RECENT: usize = 50;

#[tauri::command]
pub(crate) fn ledger_held() -> Result<Held, String> {
    held_in(&ui::gather::default_ledger_dir())
}

/// The heart, with the directory passed in: a test reads a throwaway place,
/// never the reader's real ledger — which on this machine is 8 MB of real runs.
fn held_in(directory: &std::path::Path) -> Result<Held, String> {
    if !ui::gather::ledger_present(directory) {
        return Ok(Held {
            directory: directory.display().to_string(),
            exists: false,
            runs: 0,
            unfinished: Vec::new(),
            waiting: Vec::new(),
            leftovers: Vec::new(),
            failures: Vec::new(),
            kept: Vec::new(),
            inventory_present: 0,
            inventory_gone: 0,
        });
    }

    let ledger = ledger::Ledger::open(directory).map_err(|error| error.to_string())?;
    let say = |error: ledger::LedgerError| error.to_string();

    Ok(Held {
        directory: directory.display().to_string(),
        exists: true,
        runs: ledger.recorded_runs().map_err(say)?,
        unfinished: ledger
            .unfinished_runs()
            .map_err(say)?
            .into_iter()
            .map(|run| OpenRun {
                run_id: run.run_id,
                entity: run.entity,
                open_steps: run.open_steps,
                oldest_started_at: run.oldest_started_at,
            })
            .collect(),
        waiting: ledger
            .waiting_runs()
            .map_err(say)?
            .into_iter()
            .map(|run| Waiting {
                run_id: run.run_id,
                entity: run.entity,
                waiting_since: run.waiting_since,
            })
            .collect(),
        leftovers: ledger
            .processes_left_running()
            .map_err(say)?
            .into_iter()
            .map(|process| Leftover {
                alive: ledger::pid_is_alive(process.pid),
                process_id: process.process_id,
                pid: process.pid,
                command: process.command,
                working_directory: process.working_directory,
                port: process.port,
            })
            .collect(),
        failures: ledger
            .failure_class_tally(None, RECENT)
            .map_err(say)?
            .into_iter()
            .map(|count| FailureClass {
                class: count.failure_class,
                failures: count.failures,
                runs_affected: count.runs_affected,
            })
            .collect(),
        kept: collections(&ledger)?,
        inventory_present: ledger.inventory_present().map_err(say)?.len(),
        inventory_gone: ledger.inventory_gone().map_err(say)?.len(),
    })
}

/// The store's entries, collection by collection.
///
/// **THE COLLECTIONS ARE NOT DECLARED ANYWHERE**: a flow names its own, so the
/// only way to know them is to ask for the ones Sailor itself writes and let a
/// reader see nothing when nothing was written. A list that invented names
/// would be worse than a short one.
fn collections(ledger: &ledger::Ledger) -> Result<Vec<Kept>, String> {
    let mut kept = Vec::new();
    for name in ["sailor", "flows", "terminals", "handover"] {
        for record in ledger
            .records_in(name)
            .map_err(|error| error.to_string())?
            .into_iter()
        {
            kept.push(Kept {
                collection: record.collection,
                key: record.key,
            });
        }
    }
    Ok(kept)
}

#[cfg(test)]
mod tests {
    /// **A LEDGER THAT WAS NEVER CREATED ANSWERS, AND SAYS SO.** It must not
    /// fail there: a fresh install has no ledger, and an error would read as a
    /// broken window on the one day everything is fine.
    #[test]
    fn a_ledger_that_does_not_exist_is_not_an_error() {
        let nowhere = std::env::temp_dir().join(format!("no-ledger-{}", std::process::id()));
        let held = super::held_in(&nowhere).expect("it answers even with nothing to read");

        assert!(!held.exists, "an empty directory was taken for a ledger");
        assert_eq!(held.runs, 0, "a ledger that is not there reported runs");
        assert!(held.leftovers.is_empty(), "and processes");
        // AND IT STILL SAYS WHERE IT LOOKED, or «nothing here» cannot be
        // checked by anybody.
        assert!(
            held.directory.contains("no-ledger"),
            "the place it looked in is not said: {}",
            held.directory,
        );

        // THE ABSURD CASE, and it goes with the other: a directory holding the
        // two files must NOT read as absent, or the check above would pass on
        // a function that says «no ledger» to everything.
        std::fs::create_dir_all(&nowhere).expect("the throwaway directory");
        for name in ["state.db", "events.db"] {
            std::fs::write(nowhere.join(name), b"").expect("an empty file");
        }
        let found = super::held_in(&nowhere);
        assert!(
            found.is_err() || found.expect("checked").exists,
            "two files that are there read as no ledger at all",
        );
        std::fs::remove_dir_all(&nowhere).ok();
    }
}
