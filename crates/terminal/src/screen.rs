//! The last of what a terminal showed, kept where another process can read it.
//!
//! **SILENCE IS NOT FREEDOM.** A byte count says a terminal is quiet, which is
//! equally true of one waiting for an answer and one waiting for work: only
//! what is painted tells them apart.

use std::io;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime};

/// How much is kept: a screenful and a little more, which is where a prompt
/// and any question above it live. Everything older is scrollback.
pub const A_SCREENFUL: usize = 8 * 1024;

/// The tail of what the program inside has shown.
#[derive(Debug, Default)]
pub struct Screen {
    last: Mutex<Vec<u8>>,
}

impl Screen {
    pub fn new() -> Screen {
        Screen::default()
    }

    pub fn push(&self, bytes: &[u8]) {
        let Ok(mut held) = self.last.lock() else {
            return;
        };
        held.extend_from_slice(bytes);
        if held.len() > A_SCREENFUL {
            let from = held.len() - A_SCREENFUL;
            held.drain(..from);
        }
    }

    pub fn taken(&self) -> Vec<u8> {
        self.last
            .lock()
            .map(|held| held.clone())
            .unwrap_or_default()
    }
}

/// Where a terminal's last screenful is kept, beside its letterbox.
pub fn address_in(store: &Path, tty: &str) -> PathBuf {
    crate::inbox::mailroom(store).join(format!("{tty}.screen"))
}

/// A screen, and the process that painted it. **A SCREEN OUTLIVES ITS
/// TERMINAL**: killed with a signal, a hold leaves behind a file that is still
/// and shows a prompt, so the painter travels with the paint (fault 156).
pub struct Painted {
    pub by: u32,
    pub bytes: Vec<u8>,
}

impl Painted {
    /// Whether whoever painted this is still running. A number given to
    /// somebody else since is the one way this is wrong, and it is wrong
    /// towards holding a terminal back.
    pub fn is_still_held(&self) -> bool {
        self.by != 0 && unsafe { libc::kill(self.by as libc::pid_t, 0) } == 0
    }
}

/// Writes it so a reader never sees half of it, the painter on the first line.
pub fn write(path: &Path, by: u32, bytes: &[u8]) -> io::Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let mut whole = format!("{by}\n").into_bytes();
    whole.extend_from_slice(bytes);
    let beside = path.with_extension("screen.writing");
    std::fs::write(&beside, &whole)?;
    std::fs::rename(&beside, path)
}

/// What is on the screen and who painted it, or nothing when there is no file.
///
/// Nothing, and never an empty screen: a terminal nobody wrote a screen for is
/// one nothing can be said about, and «nothing painted» is the answer that
/// would let a reset be typed into a session waiting for a person.
pub fn read(path: &Path) -> Option<Painted> {
    let whole = std::fs::read(path).ok()?;
    let end = whole.iter().position(|byte| *byte == b'\n')?;
    let by = std::str::from_utf8(&whole[..end]).ok()?.parse().ok()?;
    Some(Painted {
        by,
        bytes: whole[end + 1..].to_vec(),
    })
}

/// How long the screen has stood still, or nothing when there is no file. The
/// recorder writes only what changed, so this is the last paint: a session at
/// work never stands still.
pub fn still_for(path: &Path) -> Option<Duration> {
    let changed = std::fs::metadata(path).and_then(|of| of.modified()).ok()?;
    Some(
        SystemTime::now()
            .duration_since(changed)
            .unwrap_or_default(),
    )
}

/// What a person would see, with the terminal's own control sequences taken
/// out: a mark split by a colour code is a mark nobody would find.
pub fn as_a_person_sees_it(bytes: &[u8]) -> String {
    let seen = String::from_utf8_lossy(bytes);
    let letters: Vec<char> = seen.chars().collect();
    let mut plain = String::new();
    let mut at = 0usize;
    while at < letters.len() {
        if letters[at] != '\u{1b}' {
            plain.push(letters[at]);
            at += 1;
            continue;
        }
        at += 1;
        match letters.get(at) {
            // A control sequence: parameters, then one letter that ends it.
            Some('[') => {
                at += 1;
                while at < letters.len() && !letters[at].is_ascii_alphabetic() && letters[at] != '~'
                {
                    at += 1;
                }
                at += 1;
            }
            // An operating-system command, ended by a bell or by ESC \.
            Some(']') => {
                at += 1;
                while at < letters.len() && letters[at] != '\u{7}' {
                    if letters[at] == '\u{1b}' && letters.get(at + 1) == Some(&'\\') {
                        at += 1;
                        break;
                    }
                    at += 1;
                }
                at += 1;
            }
            Some(_) => at += 1,
            None => {}
        }
    }
    plain
}

/// How often the kept screen is compared with the file, and **how blind the
/// reader is**: what was painted since the last beat is not on disk yet.
const A_BEAT: Duration = Duration::from_millis(200);

/// Keeps the screen on disk while the session runs.
pub fn recorded_into(screen: Arc<Screen>, path: PathBuf) -> Recording {
    let running = Arc::new(AtomicBool::new(true));
    let going = Arc::clone(&running);
    let by = std::process::id();
    let writing = std::thread::spawn(move || {
        let mut last = Vec::new();
        while going.load(Ordering::Relaxed) {
            last = kept(&path, by, &screen, last);
            std::thread::sleep(A_BEAT);
        }
        kept(&path, by, &screen, last);
    });
    Recording { running, writing }
}

fn kept(path: &Path, by: u32, screen: &Screen, last: Vec<u8>) -> Vec<u8> {
    let now = screen.taken();
    if now == last {
        return last;
    }
    let _ = write(path, by, &now);
    now
}

/// The thread that keeps a screen on disk, until told to stop.
pub struct Recording {
    running: Arc<AtomicBool>,
    writing: std::thread::JoinHandle<()>,
}

impl Recording {
    pub fn stop(self) {
        self.running.store(false, Ordering::Relaxed);
        let _ = self.writing.join();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_the_tail_is_kept() {
        let screen = Screen::new();
        screen.push(&vec![b'a'; A_SCREENFUL]);
        screen.push(b"the last word");

        let held = screen.taken();

        assert_eq!(held.len(), A_SCREENFUL);
        assert!(held.ends_with(b"the last word"));
    }

    /// **A MARK SPLIT BY A COLOUR CODE IS A MARK NOBODY WOULD FIND.** Every
    /// prompt worth recognising is painted, so the sequences come out first.
    #[test]
    fn the_control_sequences_come_out_before_anything_is_looked_for() {
        let painted = b"\x1b[38;5;33m\xe2\x94\x82 \x1b[0m> \x1b[2mtype here\x1b[0m";

        assert_eq!(as_a_person_sees_it(painted), "│ > type here");
    }

    #[test]
    fn a_title_the_program_set_is_not_part_of_the_screen() {
        let with_a_title = b"\x1b]0;a title\x07the prompt";

        assert_eq!(as_a_person_sees_it(with_a_title), "the prompt");
    }

    /// A screen nobody wrote is not an empty screen: the difference is what
    /// keeps a reset out of a session waiting for a person.
    #[test]
    fn a_screen_that_was_never_written_is_nothing_and_not_empty() {
        assert!(read(Path::new("/nowhere/at/all.screen")).is_none());
    }
}
