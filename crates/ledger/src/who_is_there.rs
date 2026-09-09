//! **THE STORE CANNOT SEE A DEATH.** A row saying «running» outlives the
//! process; only these ask the machine whether it is still the same one.

use crate::{Holder, HolderIdentity, ProcessRecord, StepRecord};

/// Does that pid still exist? **NOT `pgrep`**, which in some sandboxes sees no
/// processes and answers empty *without an error*, so "nobody there" and "not
/// allowed to look" arrive identical. `EPERM` — alive, another user's — is a yes.
pub fn pid_is_alive(pid: u32) -> bool {
    // **LIMIT ONE, AND THERE IS NO BETTER CHECK.** Numbers get reused, so a
    // live pid does not prove it is the *same* process the store wrote:
    // settling that wants a start time, which macOS keeps behind `libproc`.
    // This confirms, it does not decide.

    // **A NUMBER THAT IS NOT A PID IS NOT ASKED ABOUT.** Read signed, 0 is the
    // caller's own group and anything below it is a group or everybody: a
    // stored number past a positive `i32` would answer «alive» about whoever
    // happened to be running.
    if pid == 0 || pid > i32::MAX as u32 {
        return false;
    }
    // **LIMIT TWO, AND IT WAS MEASURED: an unreaped child reads as alive.** A
    // first draft of `the_dead_are_closed_and_the_living_are_left_alone` killed
    // a child without waiting and got "alive" here. That is correct — a zombie
    // *is* a row in the process table — and it reaches only the caller's own
    // children: a real orphan's parent is the init process, which reaps it as
    // soon as it dies. For your own child use `Process::exited`, which waits.

    // SAFETY: `kill` with signal 0 delivers nothing and touches no memory of
    // ours; it reads an int and returns an int.
    let outcome = unsafe { libc::kill(pid as libc::pid_t, 0) };
    if outcome == 0 {
        return true;
    }
    std::io::Error::last_os_error().raw_os_error() == Some(libc::EPERM)
}

/// Whether a pid breathes, and since when. **THE SECOND HALF MAKES A KILL
/// SAFE**: numbers are reused, and no process predates its own row.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WhoHoldsThePid {
    /// Born at this second since the epoch; `AliveButUnsaid` is not a licence.
    Since(i64),
    AliveButUnsaid,
    Nobody,
}

pub fn who_holds_the_pid(pid: u32) -> WhoHoldsThePid {
    if !pid_is_alive(pid) {
        return WhoHoldsThePid::Nobody;
    }
    match born_at(pid) {
        Some(second) => WhoHoldsThePid::Since(second),
        None => WhoHoldsThePid::AliveButUnsaid,
    }
}

pub const A_RECORD_IS_NEVER_THIS_LATE: i64 = 120;

/// The second the kernel says a pid was born, for whoever writes down a
/// process it has just started. `None` is a machine that would not say.
pub fn born_second_of(pid: u32) -> Option<i64> {
    born_at(pid)
}

/// Whether the process a row names is the one holding that number now.
///
/// Exact where the row says when its process was born; where it does not —
/// every row written before that was asked — the old tolerance stands, which
/// is why the two are not one function.
pub fn the_same_process_as(record: &ProcessRecord) -> bool {
    let Some(born) = record.born_at else {
        return the_same_process(record.pid, record.started_at);
    };
    match who_holds_the_pid(record.pid) {
        WhoHoldsThePid::Nobody => false,
        WhoHoldsThePid::AliveButUnsaid => true,
        WhoHoldsThePid::Since(second) => second == born,
    }
}

/// Whether the pid alive now is the one the row named. A machine that will
/// not say answers `true`: a refusal is not evidence.
pub fn the_same_process(pid: u32, started_at: i64) -> bool {
    match who_holds_the_pid(pid) {
        WhoHoldsThePid::Nobody => false,
        WhoHoldsThePid::AliveButUnsaid => true,
        WhoHoldsThePid::Since(born) => born <= started_at + A_RECORD_IS_NEVER_THIS_LATE,
    }
}

#[cfg(target_os = "macos")]
fn born_at(pid: u32) -> Option<i64> {
    let mut about: libc::proc_bsdinfo = unsafe { std::mem::zeroed() };
    let wanted = std::mem::size_of::<libc::proc_bsdinfo>() as i32;
    // SAFETY: the kernel fills at most `wanted` bytes of a zeroed struct we
    // own, reads nothing of ours, and returns how much it wrote.
    let written = unsafe {
        libc::proc_pidinfo(
            pid as libc::c_int,
            libc::PROC_PIDTBSDINFO,
            0,
            std::ptr::addr_of_mut!(about).cast(),
            wanted,
        )
    };
    // A short answer is a refusal: read as a date it would say 1970.
    (written == wanted).then_some(about.pbi_start_tvsec as i64)
}

#[cfg(not(target_os = "macos"))]
fn born_at(_pid: u32) -> Option<i64> {
    None
}

/// One invocation of Sailor, told apart from the next. The kernel knows one
/// process; this says which run of Sailor inside it wrote the row.
pub fn invocation_id() -> String {
    static THIS_ONE: std::sync::OnceLock<String> = std::sync::OnceLock::new();
    THIS_ONE
        .get_or_init(|| {
            let since = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_else(|before_the_epoch| before_the_epoch.duration());
            format!("{}-{}", std::process::id(), since.as_nanos())
        })
        .clone()
}

/// The identity to write beside the pid when this process takes a step.
pub fn this_process_holds() -> HolderIdentity {
    HolderIdentity {
        born_at: born_at(std::process::id()),
        invocation: invocation_id(),
    }
}

/// Whether the process a row names still holds the step.
///
/// **THREE ANSWERS, NOT TWO.** What cannot be settled is said as such: rounded
/// to `Held` it keeps a finished step hostage, rounded to `Released` it hands
/// live work to a second taker.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StillHeld {
    /// No process ever held it: a handed step, held by a deadline instead.
    Nobody,
    Held,
    /// That process is gone, or its number belongs to somebody else now.
    Released,
    Uncertain,
}

pub fn still_held(record: &StepRecord) -> StillHeld {
    match record.holder() {
        Holder::Nobody => StillHeld::Nobody,
        Holder::ANumberAlone(_) | Holder::Ambiguous => StillHeld::Uncertain,
        Holder::Named { pid, identity } => holds_it_now(pid, identity),
    }
}

fn holds_it_now(pid: u32, identity: &HolderIdentity) -> StillHeld {
    if pid == std::process::id() && identity.invocation == invocation_id() {
        return StillHeld::Held;
    }
    let Some(born) = identity.born_at else {
        return StillHeld::Uncertain;
    };
    match who_holds_the_pid(pid) {
        WhoHoldsThePid::Nobody => StillHeld::Released,
        WhoHoldsThePid::AliveButUnsaid => StillHeld::Uncertain,
        WhoHoldsThePid::Since(second) => {
            if second == born {
                StillHeld::Held
            } else {
                StillHeld::Released
            }
        }
    }
}
