//! What a terminal has moved so far, kept where a process that is not this one
//! can read it.
//!
//! Whoever decides that a context is full runs outside the session and is
//! allowed to die at any instant. So the count lives on disk, not in the memory
//! of whoever holds the pipe: a beat that finds the file finds all it needs.

use serde::{Deserialize, Serialize};
use std::io;
use std::path::{Path, PathBuf};

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
