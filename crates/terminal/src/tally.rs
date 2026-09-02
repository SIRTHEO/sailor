//! What a terminal has moved so far, kept where a process that is not this one
//! can read it.
//!
//! Whoever decides that a context is full runs outside the session and is
//! allowed to die at any instant. So the count lives on disk, not in the memory
//! of whoever holds the pipe: a beat that finds the file finds all it needs.

use serde::{Deserialize, Serialize};
use std::io;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;

/// The bytes that crossed a terminal, each direction on its own.
///
/// Two numbers and not their sum, because the sum cannot be taken apart again:
/// what a program shows is most of the traffic, what a person types is almost
/// none, and a later reader may care which is which.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Tally {
    /// Bytes the program inside has shown.
    pub shown: u64,
    /// Bytes that were typed in, by a person or by a stranger.
    pub typed: u64,
    /// When this was last written, in seconds.
    pub at: i64,
}

impl Tally {
    pub fn total(&self) -> u64 {
        self.shown.saturating_add(self.typed)
    }
}

/// Where a terminal's count is kept, beside its letterbox.
pub fn address_in(store: &Path, tty: &str) -> PathBuf {
    crate::inbox::mailroom(store).join(format!("{tty}.seen"))
}

/// Writes the count so that a reader never sees half of it.
///
/// Through a neighbouring file and a rename, which is the one operation the
/// filesystem will not tear: a reader arriving mid-write gets the old count,
/// never a truncated one.
pub fn write(path: &Path, tally: &Tally) -> io::Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let text = serde_json::to_string(tally).map_err(io::Error::other)?;
    let beside = path.with_extension("seen.writing");
    std::fs::write(&beside, text)?;
    std::fs::rename(&beside, path)
}

/// The count, or nothing if it is not there or cannot be understood.
///
/// A file that cannot be read is not a session that moved no bytes. Returning
/// zero here would make a full context look empty, which is the direction a
/// wrong answer must never take.
pub fn read(path: &Path) -> Option<Tally> {
    let text = std::fs::read_to_string(path).ok()?;
    serde_json::from_str(&text).ok()
}

/// Now, in seconds since the epoch.
pub fn now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |elapsed| elapsed.as_secs() as i64)
}

/// The bytes each direction has moved, shared with whoever writes them down.
///
/// One shape for every terminal Sailor holds — the one in the person's own
/// emulator and the one the window opened — so `terminal list` reads the same
/// count from both.
#[derive(Debug)]
pub struct Counters {
    pub shown: Arc<AtomicU64>,
    pub typed: Arc<AtomicU64>,
}

impl Default for Counters {
    fn default() -> Counters {
        Counters::new()
    }
}

impl Counters {
    pub fn new() -> Counters {
        Counters {
            shown: Arc::new(AtomicU64::new(0)),
            typed: Arc::new(AtomicU64::new(0)),
        }
    }

    pub fn total(&self) -> u64 {
        self.shown
            .load(Ordering::Relaxed)
            .saturating_add(self.typed.load(Ordering::Relaxed))
    }

    /// Keeps the count on disk while the session runs.
    ///
    /// On disk and not in here, because whoever reads it is another process
    /// that may start after this one and must not have to ask it anything.
    pub fn recorded_into(&self, path: PathBuf) -> Recording {
        let running = Arc::new(AtomicBool::new(true));
        let shown = Arc::clone(&self.shown);
        let typed = Arc::clone(&self.typed);
        let going = Arc::clone(&running);
        let writing = std::thread::spawn(move || {
            let mut last = Tally::default();
            while going.load(Ordering::Relaxed) {
                last = record(&path, &shown, &typed, last);
                std::thread::sleep(std::time::Duration::from_millis(500));
            }
            record(&path, &shown, &typed, last);
        });
        Recording { running, writing }
    }
}

/// Writes the count if it moved, and gives back what is now on disk.
fn record(path: &Path, shown: &AtomicU64, typed: &AtomicU64, last: Tally) -> Tally {
    let current = Tally {
        shown: shown.load(Ordering::Relaxed),
        typed: typed.load(Ordering::Relaxed),
        at: now(),
    };
    if current.shown == last.shown && current.typed == last.typed {
        return last;
    }
    let _ = write(path, &current);
    current
}

/// The thread that keeps a count on disk, until told to stop.
pub struct Recording {
    running: Arc<AtomicBool>,
    writing: std::thread::JoinHandle<()>,
}

impl Recording {
    /// Stops and waits, so the last count is on disk before the terminal's row
    /// disappears: a session that ended full must not read as one that ended
    /// empty.
    pub fn stop(self) {
        self.running.store(false, Ordering::Relaxed);
        let _ = self.writing.join();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(name: &str) -> PathBuf {
        crate::scratch::directory(&format!("tally-{name}"))
    }

    #[test]
    fn a_count_written_comes_back_the_same() {
        let directory = scratch("roundtrip");
        let path = address_in(&directory, "ttys004");
        let written = Tally {
            shown: 4_000_000,
            typed: 512,
            at: 1_700_000_000,
        };
        write(&path, &written).expect("write the count");
        assert_eq!(read(&path), Some(written));
        let _ = std::fs::remove_dir_all(&directory);
    }

    #[test]
    fn the_count_sits_beside_the_letterbox() {
        let store = Path::new("/somewhere/store");
        assert_eq!(
            address_in(store, "ttys004").parent(),
            crate::inbox::address_in(store, "ttys004").parent(),
            "whoever found the letterbox must find the count without a second address"
        );
    }

    /// Unreadable is not empty. A missing count that answered zero would let a
    /// full session pass for a fresh one.
    #[test]
    fn a_count_that_is_not_there_is_not_a_count_of_zero() {
        let directory = scratch("absent");
        assert_eq!(read(&address_in(&directory, "ttys009")), None);
        let _ = std::fs::remove_dir_all(&directory);
    }

    #[test]
    fn a_file_that_makes_no_sense_is_not_a_count_of_zero() {
        let directory = scratch("nonsense");
        let path = address_in(&directory, "ttys010");
        std::fs::create_dir_all(path.parent().expect("the directory")).expect("the directory");
        std::fs::write(&path, "this is not a count").expect("leave nonsense behind");
        assert_eq!(read(&path), None);
        let _ = std::fs::remove_dir_all(&directory);
    }

    #[test]
    fn writing_twice_leaves_no_half_file_behind() {
        let directory = scratch("clean");
        let path = address_in(&directory, "ttys011");
        write(&path, &Tally::default()).expect("write once");
        write(
            &path,
            &Tally {
                shown: 10,
                typed: 1,
                at: 2,
            },
        )
        .expect("write twice");
        let left: Vec<String> = std::fs::read_dir(path.parent().expect("the directory"))
            .expect("read the directory")
            .flatten()
            .map(|entry| entry.file_name().to_string_lossy().into_owned())
            .collect();
        assert_eq!(left, vec!["ttys011.seen".to_owned()], "{left:?}");
        let _ = std::fs::remove_dir_all(&directory);
    }
}
