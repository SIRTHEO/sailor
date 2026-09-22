//! Heavy work on this machine takes a turn, one at a time, in the order it asked.
//!
//! **THE KERNEL HOLDS THE TURN, NOT A FILE THAT SAYS SO.** It is an exclusive
//! `flock`, which ends with its process however that ends: no sweeper, no age.
//! The tickets only keep the order, and a dead one is dropped by its next reader.

use ledger::born_second_of;
use ledger::holdings::the_process_that_took_it_is_still_there;
use std::fs::{File, OpenOptions};
use std::io::Write;
use std::os::unix::io::AsRawFd;
use std::path::{Path, PathBuf};
use std::time::Duration;

const THE_LOCK: &str = "the-machine";
const THE_HOLDER: &str = "holder";
const THE_QUEUE: &str = "queue";
const BETWEEN_LOOKS: Duration = Duration::from_millis(500);

/// Who holds the turn, or waits for one, as its ticket says.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Ticket {
    pub pid: u32,
    pub born_at: Option<i64>,
    pub purpose: String,
    /// What the holder hands to the processes it starts for this turn.
    pub token: String,
}

/// A turn this process holds until it drops it. An inherited turn holds
/// nothing: the process that took it lets it go.
#[derive(Debug)]
pub struct Turn {
    lock: Option<File>,
    holder: PathBuf,
    token: String,
}

impl Turn {
    pub fn inherited(&self) -> bool {
        self.lock.is_none()
    }

    /// Handed to the processes this turn's work starts, and to no others: a
    /// process carrying it runs inside this turn instead of queueing behind it.
    pub fn token(&self) -> &str {
        &self.token
    }
}

impl Drop for Turn {
    fn drop(&mut self) {
        if self.lock.is_some() {
            let _ = std::fs::remove_file(&self.holder);
        }
    }
}

/// Where the turns of the machine a ledger directory answers for are kept.
pub fn turns_under(ledger_directory: &Path) -> PathBuf {
    ledger_directory.join("turns")
}

/// Wait for the machine, then hold it. `carried` is the token this process
/// was handed, if any: the holder's own work does not queue behind it.
/// `waiting` hears who is ahead each time that changes, so a person watching a
/// run sees why it stands still.
pub fn wait_for_the_machine(
    turns: &Path,
    purpose: &str,
    carried: Option<&str>,
    waiting: &mut dyn FnMut(&Ticket, usize),
) -> std::io::Result<Turn> {
    let holder = turns.join(THE_HOLDER);
    if let Some(held) = carried.and_then(|token| held_with(&holder, token)) {
        return Ok(Turn {
            lock: None,
            holder,
            token: held.token,
        });
    }
    let queue = turns.join(THE_QUEUE);
    std::fs::create_dir_all(&queue)?;
    let me = this_process(purpose);
    let ticket = queue.join(format!("{:020}-{}", nanos_now(), me.pid));
    write_ticket(&ticket, &me)?;
    let lock = OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true)
        .open(turns.join(THE_LOCK));
    let lock = match lock {
        Ok(lock) => lock,
        Err(error) => {
            let _ = std::fs::remove_file(&ticket);
            return Err(error);
        }
    };
    let mut last_said: Option<(Ticket, usize)> = None;
    loop {
        let ahead = live_tickets_ahead_of(&queue, &ticket);
        if ahead.is_empty() && took(&lock) {
            let _ = std::fs::remove_file(&ticket);
            write_ticket(&holder, &me)?;
            return Ok(Turn {
                lock: Some(lock),
                holder,
                token: me.token,
            });
        }
        let first = read_ticket(&holder)
            .filter(|held| the_process_that_took_it_is_still_there(held.pid, held.born_at))
            .or_else(|| ahead.first().cloned());
        if let Some(first) = first {
            let now = (first, ahead.len());
            if last_said.as_ref() != Some(&now) {
                waiting(&now.0, now.1);
                last_said = Some(now);
            }
        }
        std::thread::sleep(BETWEEN_LOOKS);
    }
}

/// Whoever holds the turn now, if a live process does.
pub fn who_holds_the_machine(turns: &Path) -> Option<Ticket> {
    read_ticket(&turns.join(THE_HOLDER))
        .filter(|held| the_process_that_took_it_is_still_there(held.pid, held.born_at))
}

/// Whoever waits for it, oldest first, the dead left out.
pub fn who_waits_for_the_machine(turns: &Path) -> Vec<Ticket> {
    tickets_in(&turns.join(THE_QUEUE))
        .into_iter()
        .map(|(_, ticket)| ticket)
        .collect()
}

fn this_process(purpose: &str) -> Ticket {
    let pid = std::process::id();
    Ticket {
        pid,
        born_at: born_second_of(pid),
        purpose: purpose.replace('\n', " "),
        token: format!("{pid}-{}", nanos_now()),
    }
}

/// The turn a live holder took with this token, if it did.
fn held_with(holder: &Path, token: &str) -> Option<Ticket> {
    read_ticket(holder).filter(|held| {
        held.token == token && the_process_that_took_it_is_still_there(held.pid, held.born_at)
    })
}

fn took(lock: &File) -> bool {
    // SAFETY: `flock` reads a descriptor this process owns and touches no memory.
    unsafe { libc::flock(lock.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) == 0 }
}

fn live_tickets_ahead_of(queue: &Path, mine: &Path) -> Vec<Ticket> {
    tickets_in(queue)
        .into_iter()
        .take_while(|(path, _)| path.as_path() != mine)
        .map(|(_, ticket)| ticket)
        .collect()
}

/// The tickets in the order they were written. A ticket whose process is gone
/// is removed here: its owner cannot, and nobody else would.
fn tickets_in(queue: &Path) -> Vec<(PathBuf, Ticket)> {
    let Ok(entries) = std::fs::read_dir(queue) else {
        return Vec::new();
    };
    let mut paths: Vec<PathBuf> = entries.flatten().map(|entry| entry.path()).collect();
    paths.sort();
    paths
        .into_iter()
        .filter_map(|path| {
            let ticket = read_ticket(&path)?;
            if the_process_that_took_it_is_still_there(ticket.pid, ticket.born_at) {
                Some((path, ticket))
            } else {
                let _ = std::fs::remove_file(&path);
                None
            }
        })
        .collect()
}

fn write_ticket(path: &Path, ticket: &Ticket) -> std::io::Result<()> {
    let born = ticket
        .born_at
        .map(|born| born.to_string())
        .unwrap_or_default();
    let partial = path.with_extension("partial");
    let mut file = File::create(&partial)?;
    writeln!(
        file,
        "{}\n{}\n{}\n{}",
        ticket.pid, born, ticket.purpose, ticket.token
    )?;
    std::fs::rename(&partial, path)
}

fn read_ticket(path: &Path) -> Option<Ticket> {
    if path
        .extension()
        .is_some_and(|extension| extension == "partial")
    {
        return None;
    }
    let text = std::fs::read_to_string(path).ok()?;
    let mut lines = text.lines();
    let pid = lines.next()?.trim().parse().ok()?;
    let born_at = lines.next().and_then(|line| line.trim().parse().ok());
    let purpose = lines.next().unwrap_or("").to_owned();
    let token = lines.next().unwrap_or("").to_owned();
    Some(Ticket {
        pid,
        born_at,
        purpose,
        token,
    })
}

fn nanos_now() -> u128 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |since| since.as_nanos())
}
