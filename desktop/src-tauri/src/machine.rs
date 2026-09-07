//! What Sailor lit on this machine, and the gesture that puts it out.
//!
//! **THE COMMAND LINE HAD THIS AND THE WINDOW DID NOT.** A machine that fills
//! up is noticed in the window — everything is slow — and answered nowhere in
//! it, so the answer was a person reading `ps` and guessing which of those
//! lines was Sailor's. The reading is the crate's, not a second one written
//! here: two readings of what is running is how they come to disagree.

use serde::Serialize;

/// One process the store still calls running.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub(crate) struct Standing {
    pub pid: u32,
    pub purpose: String,
    pub command: String,
    /// **Whether the process the row named is still there** — not whether
    /// something now holds the number it was given. Pid numbers come round.
    pub still_there: bool,
    /// The run that wanted it, when a run did.
    pub run_id: Option<String>,
    /// True when that run has ended, or when no run ever claimed it. A
    /// process nobody is waiting on is the one that fills a machine.
    pub run_is_over: bool,
    pub started_at: i64,
}

fn open_store() -> Result<ledger::Ledger, String> {
    let directory = ledger::default_directory().ok_or("no home to read the store from")?;
    ledger::Ledger::open(&directory).map_err(|error| error.to_string())
}

/// **AN ENDED RUN AND A MISSING ONE ARE NOT THE SAME FACT.** A row with no run
/// means nobody wrote down who wanted it, which is not the same as nobody
/// wanting it — so it is shown, and left alone.
fn run_is_over(store: &ledger::Ledger, run_id: Option<&str>) -> Result<bool, String> {
    let Some(run) = run_id else { return Ok(true) };
    store
        .run_header(run)
        .map_err(|error| error.to_string())
        .map(|header| header.is_none_or(|header| header.ended_at.is_some()))
}

/// What is on the window's own development port. **THE CASE THAT FILLED THIS
/// MACHINE**: a server started by hand holds it, sits in no row, and no sweep
/// will ever find it — so the screen says so instead of showing a clean list
/// while the machine grinds.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "on", rename_all = "snake_case")]
pub(crate) enum ThePort {
    Free,
    Ours { pid: u32, purpose: String },
    /// Held, and Sailor has no row for it.
    Somebody,
    /// **UNKNOWN IS NOT FREE.** Inside a sandbox binding is denied, and read as
    /// «in use» it accuses a machine that merely would not let us look.
    CouldNotLook { why: String },
}

#[tauri::command]
pub(crate) fn the_dev_port() -> Result<ThePort, String> {
    let store = open_store()?;
    Ok(
        match machine::on_the_port(&store, machine::DEV_PORT).map_err(|error| error.to_string())? {
            machine::OnThePort::Free => ThePort::Free,
            machine::OnThePort::Ours(record) => {
                ThePort::Ours { pid: record.pid, purpose: record.purpose }
            }
            machine::OnThePort::Somebody => ThePort::Somebody,
            machine::OnThePort::CouldNotLook(why) => ThePort::CouldNotLook { why },
        },
    )
}

#[tauri::command]
pub(crate) fn what_sailor_lit() -> Result<Vec<Standing>, String> {
    let store = open_store()?;
    let mut rows = Vec::new();
    for item in machine::left_running(&store).map_err(|error| error.to_string())? {
        rows.push(Standing {
            run_is_over: run_is_over(&store, item.record.run_id.as_deref())?,
            pid: item.record.pid,
            purpose: item.record.purpose,
            command: item.record.command,
            still_there: item.still_alive,
            run_id: item.record.run_id,
            started_at: item.record.started_at,
        });
    }
    Ok(rows)
}

/// What became of one process the free gesture was asked to stop.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub(crate) struct Freed {
    pub pid: u32,
    pub purpose: String,
    pub stopped: bool,
    /// Why it is still there, when it is. Empty when it went.
    pub why: String,
}

/// **ONLY WHAT A FINISHED RUN LEFT BEHIND.** A run still open means somebody
/// may be using it; no run at all means nobody wrote down who wanted it. Both
/// are left alone, and both are named in the reading above instead.
#[tauri::command]
pub(crate) fn free_the_machine() -> Result<Vec<Freed>, String> {
    let store = open_store()?;
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|since| since.as_secs() as i64)
        .map_err(|error| error.to_string())?;
    let done = machine::stop_the_ones_nobody_wants(&store, now, &|record| {
        let Some(run) = record.run_id.as_deref() else {
            return Ok(true);
        };
        Ok(store.run_header(run)?.is_none_or(|header| header.ended_at.is_none()))
    })
    .map_err(|error| error.to_string())?;
    Ok(done
        .into_iter()
        .map(|one| match one {
            machine::Teardown::Gone { pid, purpose, .. } => {
                Freed { pid, purpose, stopped: true, why: String::new() }
            }
            machine::Teardown::StillThere { pid, purpose, why, .. } => {
                Freed { pid, purpose, stopped: false, why }
            }
        })
        .collect())
}
