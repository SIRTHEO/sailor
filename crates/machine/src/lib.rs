//! What Sailor lit on this machine, and how it is put out.
//!
//! **APART FROM THE LIVE MODE ON PURPOSE**: the command line must not depend
//! on the supervisor. It is the `processes` table and a signal, read by both.

use ledger::holdings::Whose;
use std::path::{Path, PathBuf};
use std::time::Duration;

pub mod child;

pub use child::{Spec, StartToken, Supervisor};

/// Now, in seconds since the epoch.
pub fn now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |elapsed| elapsed.as_secs() as i64)
}

/// A process the ledger calls running, and what the system says about it.
#[derive(Debug, Clone)]
pub struct LeftRunning {
    pub record: ledger::ProcessRecord,
    /// **Two questions, kept apart.** The ledger says what was started; this
    /// says whether *that* process is there, not whether its number is taken.
    pub still_alive: bool,
}

/// What was left running, confirmed pid by pid.
/// The kind under which a build directory is written down as taken.
pub const BUILD_DIRECTORY: &str = "build-directory";

/// Whether anything Sailor made has nobody answering for it: a row it still
/// calls running over a process that is gone, or a holding whose owner and
/// whose run are both over. The question a flow wakes on, so it wakes on a
/// state and not on a period.
pub fn something_is_left_behind(store: &ledger::Ledger) -> Result<bool, String> {
    let ghost = left_running(store)
        .map_err(|error| error.to_string())?
        .into_iter()
        .any(|one| !one.still_alive);
    if ghost {
        return Ok(true);
    }
    let run_is_open = |run: &str| {
        store
            .run_header(run)
            .map(|header| header.is_none_or(|one| one.ended_at.is_none()))
            .map_err(|error| error.to_string())
    };
    Ok(store
        .holdings_left_held(BUILD_DIRECTORY)?
        .iter()
        .any(|holding| ledger::holdings::whose(holding, &run_is_open) == Whose::Nobody))
}

pub fn left_running(store: &ledger::Ledger) -> Result<Vec<LeftRunning>, ledger::LedgerError> {
    Ok(store
        .processes_left_running()?
        .into_iter()
        .map(|record| LeftRunning {
            // **THE ROW NAMES A PROCESS, NOT A NUMBER.** Numbers come round:
            // asked only whether one is taken, an old row answers for whoever
            // holds it now, and the sweep puts out somebody else's work.
            still_alive: ledger::the_same_process_as(&record),
            record,
        })
        .collect())
}

/// Closes, in the ledger, the rows of processes that stopped breathing.
///
/// **THE LEDGER CANNOT SEE A VIOLENT DEATH**, nor a number handed on: both
/// leave a row saying «running» for ever, and a list of ghosts is a list
/// nobody reads. A handed-on row is closed here and never signalled.
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

/// How long to wait before asking again. **A SIGNAL IS A REQUEST**: an end
/// recorded because `kill` returned zero is a death nobody saw.
const A_MOMENT_TO_LEAVE: Duration = Duration::from_millis(600);

/// Stops what Sailor lit and nobody wants any more.
///
/// **UNCERTAINTY STOPS THE ACTION**: `wanted` may fail, and a failure is not
/// permission. Read with `.ok()`, an unopenable ledger licensed every kill.
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

/// **THE GROUP, NOT THE PID**: the leader alone leaves the grandchildren
/// holding the port.
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


/// The window's development port: **the port of fault 4**, held by an orphan
/// twice in one night. Written again in `desktop/src-tauri/tauri.conf.json`;
/// `the_dev_port_matches_the_tauri_config` compares the two.
pub const DEV_PORT: u16 = 5183;

/// What is on a port, as far as this machine will say.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OnThePort {
    /// Nothing: both localhost addresses took a listener.
    Free,
    /// A process Sailor lit and still calls running.
    Ours(Box<ledger::ProcessRecord>),
    /// Taken, and Sailor has no row for it: what never passed through Sailor
    /// cannot be freed by it. This is the case that fills a machine.
    Somebody,
    /// **A REFUSAL IS NOT AN ANSWER.** In a sandbox binding is denied, and read
    /// as «in use» it accuses a machine that would not let us look.
    CouldNotLook(String),
}

/// Who is on a port: the store first, then the socket. **BOTH LOCALHOST
/// ADDRESSES** — a server on `::1` leaves `127.0.0.1` free, and asking one
/// answers «nobody» with somebody plainly there.
pub fn on_the_port(
    store: &ledger::Ledger,
    port: u16,
) -> Result<OnThePort, ledger::LedgerError> {
    if let Some(record) = store.process_holding_port(port)? {
        if ledger::the_same_process_as(&record) {
            return Ok(OnThePort::Ours(Box::new(record)));
        }
    }
    Ok(try_to_bind(port))
}

/// The socket alone, no store asked first. A refusal is its own answer here,
/// which is why this is not a bool.
pub fn on_the_port_by_binding(port: u16) -> OnThePort {
    try_to_bind(port)
}

fn try_to_bind(port: u16) -> OnThePort {
    for address in ["127.0.0.1", "::1"] {
        match std::net::TcpListener::bind((address, port)) {
            Ok(_) => {}
            Err(error) if error.kind() == std::io::ErrorKind::AddrInUse => {
                return OnThePort::Somebody
            }
            Err(error) => return OnThePort::CouldNotLook(format!("{address}: {error}")),
        }
    }
    OnThePort::Free
}

/// A socket somebody on this machine is listening on.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Listening {
    pub pid: u32,
    pub command: String,
    pub port: u16,
    /// Bound to this machine alone — **the shape of a development server**,
    /// and of both ports fault 4 turned up on. A daemon on every address is
    /// the machine's business, not Sailor's.
    pub only_here: bool,
}

/// Every listening socket of this machine, or why we could not look.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WhatListens {
    Seen(Vec<Listening>),
    /// **A REFUSAL IS NOT A QUIET MACHINE.** Read as an empty list it would
    /// declare every held port free, which is fault 12 wearing this costume.
    CouldNotLook(String),
}

/// What is listening here. **THE QUESTION `on_the_port` CANNOT ASK**: that one
/// answers about a number already known, and the orphan of fault 4 held a port
/// nobody had thought to name.
pub fn what_is_listening() -> WhatListens {
    match std::process::Command::new("lsof")
        .args(["-nP", "-iTCP", "-sTCP:LISTEN", "-F", "cpn"])
        .output()
    {
        Err(error) => WhatListens::CouldNotLook(format!("lsof: {error}")),
        Ok(out) => {
            let text = String::from_utf8_lossy(&out.stdout).into_owned();
            let complaint = String::from_utf8_lossy(&out.stderr).trim().to_owned();
            if text.trim().is_empty() && !complaint.is_empty() {
                return WhatListens::CouldNotLook(complaint);
            }
            WhatListens::Seen(listening_in(&text))
        }
    }
}

/// **FIELDS, NOT COLUMNS.** A command name holds spaces and the column form
/// truncates it, so the two would be read as one process with a short name.
/// Each line is one letter and its value; a socket is complete at its address.
pub fn listening_in(text: &str) -> Vec<Listening> {
    let mut found: Vec<Listening> = Vec::new();
    let (mut pid, mut command) = (0, String::new());
    for line in text.lines() {
        let mut letters = line.chars();
        let (Some(letter), value) = (letters.next(), letters.as_str()) else {
            continue;
        };
        match letter {
            'p' => {
                pid = value.parse().unwrap_or(0);
                command.clear();
            }
            'c' => command = value.to_owned(),
            'n' => {
                let Some(port) = value.rsplit(':').next().and_then(|tail| tail.parse().ok()) else {
                    continue;
                };
                let one = Listening {
                    pid,
                    command: command.clone(),
                    port,
                    only_here: bound_to_this_machine(value),
                };
                if pid != 0 && !found.contains(&one) {
                    found.push(one);
                }
            }
            _ => {}
        }
    }
    found
}

fn bound_to_this_machine(address: &str) -> bool {
    let Some((host, _)) = address.rsplit_once(':') else {
        return false;
    };
    matches!(host.trim_matches(['[', ']']), "127.0.0.1" | "::1" | "localhost")
}

/// The listeners no row of Sailor's reaches: **the orphan of fault 4, named
/// before it blocks somebody's start** instead of hunted for afterwards.
pub fn listeners_nobody_answers_for(
    store: &ledger::Ledger,
    listening: &[Listening],
) -> Result<Vec<Listening>, ledger::LedgerError> {
    let ours: std::collections::BTreeSet<u32> = left_running(store)?
        .into_iter()
        .filter(|item| item.still_alive)
        .map(|item| item.record.pid)
        .collect();
    Ok(listening
        .iter()
        .filter(|one| !a_row_reaches(&ours, one.pid))
        .cloned()
        .collect())
}

/// **A ROW REACHES ITS GROUP, NOT ITS NUMBER.** `Process::start` gives every
/// start a group of its own and `stop` signals that group, so the server one
/// of them lit is still Sailor's to put out.
fn a_row_reaches(ours: &std::collections::BTreeSet<u32>, pid: u32) -> bool {
    ours.contains(&pid) || group_of(pid).is_some_and(|group| ours.contains(&group))
}

/// The group a pid was started in. `None` where the machine will not say, and
/// a refusal is never read as «a group of its own».
pub fn group_of(pid: u32) -> Option<u32> {
    if pid == 0 || pid > i32::MAX as u32 {
        return None;
    }
    // SAFETY: the call reads an int and returns one, touching no memory of ours.
    let group = unsafe { libc::getpgid(pid as libc::pid_t) };
    (group > 0).then_some(group as u32)
}

/// One process weighing on the machine, Sailor's or not; `sailor_lit` is true
/// when a row of Sailor's still calls this pid running.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Weighing {
    pub pid: u32,
    pub kilobytes: u64,
    pub command: String,
    pub sailor_lit: bool,
}

/// What is on the machine, or why we cannot say.
#[derive(Debug, Clone, PartialEq)]
pub enum TheLoad {
    Seen {
        /// The one, five and fifteen minute averages.
        load: [f64; 3],
        /// The heaviest, heaviest first.
        heaviest: Vec<Weighing>,
    },
    /// **NOT AN EMPTY LIST**: «nothing is running» would call a grinding
    /// machine idle.
    CouldNotLook(String),
}

/// How many of the heaviest are worth naming.
const ENOUGH_TO_SEE_THE_TROUBLE: usize = 8;

/// **WHAT SAILOR DID NOT LIGHT STILL FILLS THE MACHINE**, and `left_running`
/// answers «0 of 0» while it grinds. This asks the system, and marks its own.
pub fn what_weighs(store: &ledger::Ledger) -> Result<TheLoad, ledger::LedgerError> {
    let ours: std::collections::BTreeSet<u32> =
        left_running(store)?.into_iter().map(|item| item.record.pid).collect();
    Ok(match ask_the_system() {
        Err(why) => TheLoad::CouldNotLook(why),
        Ok(text) => TheLoad::Seen {
            load: load_average(),
            heaviest: heaviest_of(&text, &ours),
        },
    })
}

fn ask_the_system() -> Result<String, String> {
    let out = std::process::Command::new("ps")
        .args(["-Ao", "pid=,rss=,comm="])
        .output()
        .map_err(|error| format!("ps: {error}"))?;
    if !out.status.success() {
        return Err(String::from_utf8_lossy(&out.stderr).trim().to_owned());
    }
    let text = String::from_utf8_lossy(&out.stdout).into_owned();
    // A sandbox can let `ps` run and show it nothing: silence from a table
    // holding at least this process is a refusal, not a fact.
    if text.trim().is_empty() {
        return Err("the process table came back empty, which cannot be true".to_owned());
    }
    Ok(text)
}

/// The heaviest rows of `ps`, heaviest first. An unreadable line is skipped.
pub fn heaviest_of(text: &str, ours: &std::collections::BTreeSet<u32>) -> Vec<Weighing> {
    let mut found: Vec<Weighing> = text
        .lines()
        .filter_map(|line| {
            let mut parts = line.split_whitespace();
            let pid: u32 = parts.next()?.parse().ok()?;
            let kilobytes: u64 = parts.next()?.parse().ok()?;
            let command: String = parts.collect::<Vec<_>>().join(" ");
            (!command.is_empty()).then_some(Weighing {
                sailor_lit: ours.contains(&pid),
                pid,
                kilobytes,
                command,
            })
        })
        .collect();
    found.sort_by_key(|one| std::cmp::Reverse(one.kilobytes));
    found.truncate(ENOUGH_TO_SEE_THE_TROUBLE);
    found
}

fn load_average() -> [f64; 3] {
    let mut said = [0.0f64; 3];
    // SAFETY: the call writes three doubles into an array we own and sized.
    let got = unsafe { libc::getloadavg(said.as_mut_ptr(), 3) };
    if got == 3 { said } else { [0.0; 3] }
}

/// A build directory on this disk and what it costs; who holds it is the register's affair.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LeftBehind {
    pub path: PathBuf,
    pub bytes: u64,
}

/// **THE TAG IS THE PROOF, NOT THE NAME.** Cargo writes it in the directory it
/// was pointed at and nowhere else, so `target/debug` carries none.
const CARGO_WROTE_THIS: &str = "Signature: 8a477f597d28d172789f06886806bc55";

/// The build directories under `<root>/target`, heaviest first.
pub fn build_directories_left(root: &Path) -> Vec<LeftBehind> {
    let Ok(entries) = std::fs::read_dir(root.join("target")) else {
        return Vec::new();
    };
    let mut found: Vec<LeftBehind> = entries
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| cargo_built_it(path))
        .map(|path| LeftBehind {
            bytes: what_it_holds(&path),
            path,
        })
        .collect();
    found.sort_by_key(|one| std::cmp::Reverse(one.bytes));
    found
}

fn cargo_built_it(path: &Path) -> bool {
    std::fs::read_to_string(path.join("CACHEDIR.TAG"))
        .is_ok_and(|text| text.starts_with(CARGO_WROTE_THIS))
}

fn what_it_holds(path: &Path) -> u64 {
    let Ok(entries) = std::fs::read_dir(path) else {
        return 0;
    };
    entries
        .flatten()
        .map(|entry| match entry.file_type() {
            Ok(kind) if kind.is_dir() => what_it_holds(&entry.path()),
            Ok(_) => entry.metadata().map(|held| held.len()).unwrap_or(0),
            Err(_) => 0,
        })
        .sum()
}

/// Memory going spare, or why we cannot say.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Spare {
    Bytes(u64),
    /// **A REFUSAL IS NEITHER A FULL MACHINE NOR AN EMPTY ONE.**
    CouldNotLook(String),
}

/// What one compiler wants. **MEASURED**: the heaviest `rustc` here peaked at
/// 238 MB resident; the margin is for the linker that follows it.
pub const A_COMPILER_WANTS: u64 = 512 * 1024 * 1024;

/// Pages nobody holds: free, and the ones the system would hand over unasked.
pub fn spare_memory() -> Spare {
    match std::process::Command::new("vm_stat").output() {
        Err(error) => Spare::CouldNotLook(format!("vm_stat: {error}")),
        Ok(out) if !out.status.success() => {
            Spare::CouldNotLook(String::from_utf8_lossy(&out.stderr).trim().to_owned())
        }
        Ok(out) => spare_in(&String::from_utf8_lossy(&out.stdout)),
    }
}

pub fn spare_in(text: &str) -> Spare {
    let Some(page) = text
        .lines()
        .next()
        .and_then(|line| line.split("page size of ").nth(1))
        .and_then(|rest| rest.split_whitespace().next())
        .and_then(|number| number.parse::<u64>().ok())
    else {
        return Spare::CouldNotLook("vm_stat did not say its page size".to_owned());
    };
    let pages = |name: &str| -> Option<u64> {
        text.lines()
            .find(|line| line.starts_with(name))
            .and_then(|line| line.split(':').nth(1))
            .map(|rest| rest.trim().trim_end_matches('.'))
            .and_then(|number| number.parse::<u64>().ok())
    };
    let Some(free) = pages("Pages free") else {
        return Spare::CouldNotLook("vm_stat did not say how many pages are free".to_owned());
    };
    let spare = free
        + pages("Pages inactive").unwrap_or(0)
        + pages("Pages speculative").unwrap_or(0)
        + pages("Pages purgeable").unwrap_or(0);
    Spare::Bytes(spare * page)
}

/// How many compilers this machine holds at once. **IT IS SHARED BETWEEN
/// SESSIONS**, so the core count is a ceiling and not an answer: three gates
/// were killed for memory running one core count of them. Refused a look, it
/// yields the ceiling — a refusal is no grounds for slowing anybody down.
pub fn how_many_compilers(spare: &Spare, cores: usize) -> usize {
    let Spare::Bytes(bytes) = spare else {
        return cores.max(1);
    };
    ((bytes / A_COMPILER_WANTS) as usize).clamp(1, cores.max(1))
}
