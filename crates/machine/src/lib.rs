//! What Sailor lit on this machine, and how it is put out.
//!
//! **APART FROM THE LIVE MODE ON PURPOSE**: the command line must not depend on
//! the supervisor, and this is not rebuild machinery — it is the `processes`
//! table and a signal. Both sides read it from here.

use std::time::Duration;

/// A process the ledger calls running, and what the system says about it.
#[derive(Debug, Clone)]
pub struct LeftRunning {
    pub record: ledger::ProcessRecord,
    /// **Two different questions, kept apart on purpose.** The ledger says
    /// what was started; this says whether that pid still breathes.
    pub still_alive: bool,
}

/// What was left running, confirmed pid by pid.
pub fn left_running(store: &ledger::Ledger) -> Result<Vec<LeftRunning>, ledger::LedgerError> {
    Ok(store
        .processes_left_running()?
        .into_iter()
        .map(|record| LeftRunning {
            still_alive: ledger::pid_is_alive(record.pid),
            record,
        })
        .collect())
}

/// Closes, in the ledger, the rows of processes that stopped breathing.
///
/// **THE LEDGER CANNOT SEE A VIOLENT DEATH**: a process killed from outside
/// writes no ending and stays «running» for ever. A list full of ghosts is a
/// list nobody reads, which is how a process registry stops preventing fault 4.
/// Returns how many it closed.
pub fn close_the_ones_that_stopped_breathing(
    store: &ledger::Ledger,
    now: i64,
) -> Result<usize, ledger::LedgerError> {
    let mut closed = 0;
    for gone in left_running(store)?
        .into_iter()
        .filter(|item| !item.still_alive)
    {
        store.record_process_ended(&ledger::ProcessEndRecord {
            process_id: gone.record.process_id,
            // No exit code is invented: nobody saw it leave.
            exit_code: None,
            ended_at: now,
        })?;
        closed += 1;
    }
    Ok(closed)
}

/// What became of one process this was asked to stop.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Teardown {
    /// Signalled, and confirmed gone afterwards. Only this closes its row.
    Gone { process_id: String, pid: u32, purpose: String },
    /// Signalled and still breathing. The row stays written as running: a
    /// closed row would hide it from every sweep that follows.
    StillThere { process_id: String, pid: u32, purpose: String, why: String },
}

/// How long to wait for a signalled process before asking again. **A SIGNAL IS
/// A REQUEST, NOT AN ENDING**: recording an end because `kill` returned zero
/// writes a death nobody saw.
const A_MOMENT_TO_LEAVE: Duration = Duration::from_millis(600);

/// Stops what Sailor lit and nobody wants any more.
///
/// **UNCERTAINTY STOPS THE ACTION**: `wanted` may fail, and a failure is not
/// permission. Read with `.ok()`, a ledger that would not open became a licence
/// to kill everything it could not be asked about.
pub fn stop_the_ones_nobody_wants(
    store: &ledger::Ledger,
    now: i64,
    wanted: &dyn Fn(&ledger::ProcessRecord) -> Result<bool, ledger::LedgerError>,
) -> Result<Vec<Teardown>, ledger::LedgerError> {
    close_the_ones_that_stopped_breathing(store, now)?;
    let mut done = Vec::new();
    for item in left_running(store)?.into_iter().filter(|item| item.still_alive) {
        if wanted(&item.record)? {
            continue;
        }
        done.push(stop_one(store, now, item.record)?);
    }
    Ok(done)
}

/// **THE GROUP, NOT THE PID**, which is what `Process::stop` signals: reaching
/// for the leader alone leaves the grandchildren holding the port.
fn stop_one(
    store: &ledger::Ledger,
    now: i64,
    record: ledger::ProcessRecord,
) -> Result<Teardown, ledger::LedgerError> {
    let (process_id, pid, purpose) = (record.process_id, record.pid, record.purpose);
    let sent = signal_the_group(pid);
    std::thread::sleep(A_MOMENT_TO_LEAVE);
    if ledger::pid_is_alive(pid) {
        let why = if sent { "it was asked to leave and did not" } else { "the signal did not land" };
        return Ok(Teardown::StillThere { process_id, pid, purpose, why: why.to_owned() });
    }
    store.record_process_ended(&ledger::ProcessEndRecord {
        process_id: process_id.clone(),
        exit_code: None,
        ended_at: now,
    })?;
    Ok(Teardown::Gone { process_id, pid, purpose })
}

/// `SIGTERM` to the whole group. A number outside a pid is never signalled:
/// zero is the caller's own group and a negative one is everybody.
fn signal_the_group(pid: u32) -> bool {
    if pid == 0 || pid > i32::MAX as u32 {
        return false;
    }
    #[cfg(unix)]
    // SAFETY: `kill` reads and writes integers, and the sign says «the group».
    unsafe {
        libc::kill(-(pid as libc::pid_t), libc::SIGTERM) == 0
    }
    #[cfg(not(unix))]
    {
        false
    }
}

