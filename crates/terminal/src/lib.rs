//! Sailor's terminal engine.
//!
//! **A TERMINAL IS BORN INSIDE A WORKSPACE.** It is not a generic terminal that
//! is later told where to go: the directory is part of what the terminal *is*,
//! and it is declared by opening it. This is not a convenience — it is the
//! condition for routing to know which project is being talked about. A
//! terminal that discovers its own directory after being born is a terminal
//! that, for an instant, belongs to the wrong place; and that instant is
//! exactly the one in which the user types the first line.
//!
//! **WHAT IT DOES AND WHAT IT DOES NOT DO.** It opens a real pseudo-terminal,
//! writes to its input, delivers its output **as it comes out**, resizes it,
//! closes it, and says which terminals are open and in which workspace. It
//! draws nothing, interprets no ANSI sequence and does not decide what a flow
//! must do: it delivers raw bytes and a decision, and the caller does the rest.
//!
//! **ROUTING LIVES IN [`routing`], AND ITS RULES ARE DATA.** What the user
//! types is looked at before being executed: if it has the shape of a request
//! about a flow it goes to the flow, otherwise it passes to the terminal. Which
//! shapes, and towards which flow, is said by the descriptors — not by a
//! `match` in this crate. The default is always the terminal: routing is an
//! addition, and in doubt it does not fire.
//!
//! **NO TAURI IN HERE.** The engine is tested from the command line —
//! `cargo test -p terminal` — and the window attaches on top of it. An engine
//! inside the window would be an engine nobody can test without opening it.

pub mod bridge;
pub mod host;
pub mod inbox;
pub mod keeping;
pub mod mandate;
pub mod pty;
pub mod routing;
pub mod scratch;
pub mod session;
pub mod tally;

pub use pty::{Pty, PtyError, Size};
pub use routing::{
    default_sources, Catalog, CommandLookup, Loaded, Match, Passed, PathLookup, Problem, Route,
    Routed, Router, Source,
};
pub use session::{estimated_tokens, Opening, Summary, Terminal, Terminals};

use std::io;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, MutexGuard, PoisonError};

/// The guard, even after another thread died holding the lock: what sits under
/// every lock in this crate is whole after any panic, and a second panic would
/// take the host with it.
pub(crate) fn locked<T>(lock: &Mutex<T>) -> MutexGuard<'_, T> {
    lock.lock().unwrap_or_else(PoisonError::into_inner)
}

/// The workspace a terminal belongs to: a repo, a project's directory.
///
/// **IT IS A DIRECTORY THAT EXISTS, CHECKED ON OPENING.** A path that is not
/// there would become a failed `spawn` carrying an operating-system message
/// about a binary that does exist — the fault would move one step, and whoever
/// read it would go looking for the shell instead of the directory.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Workspace {
    /// The root, made absolute and free of symbolic links: two terminals opened
    /// on `~/x` and on `/Users/tizio/x` are in the same place, and the list has
    /// to say so.
    pub root: PathBuf,
    /// What it is called out loud: the last segment of the root.
    pub name: String,
}

impl Workspace {
    pub fn open(root: impl AsRef<Path>) -> io::Result<Workspace> {
        let root = root.as_ref();
        let root = std::fs::canonicalize(root)?;
        if !root.is_dir() {
            return Err(io::Error::new(
                io::ErrorKind::NotADirectory,
                format!(
                    "a workspace is a directory, and {} is not one",
                    root.display()
                ),
            ));
        }
        let name = root
            .file_name()
            .map(|part| part.to_string_lossy().into_owned())
            // The disk root has no last segment, and stays itself.
            .unwrap_or_else(|| root.to_string_lossy().into_owned());
        Ok(Workspace { root, name })
    }
}

/// Whoever receives a terminal's output **while** it comes out, instead of when
/// the process has died.
///
/// **RAW BYTES ARE DELIVERED, AND THE CHOICE IS NOT NEW.** It is the same one
/// as `actions::LiveSink`, for the same reasons written there: a read stops
/// wherever it happens to, even halfway through a multibyte UTF-8 sequence, and
/// decoding here would replace the accent broken at the edge with a replacement
/// character — an invisible and permanent fault — or would force holding the
/// incomplete bytes back until the next chunk, that is, putting back the delay
/// the mechanism exists to remove. Decoding belongs to whoever watches, the
/// only one who knows what to do with it. In a terminal the reason weighs more
/// than elsewhere: here bytes are not text alone, they are also control
/// sequences, and an emulator that receives them mutilated redraws the screen
/// badly.
///
/// **WHY IT IS NOT THE SAME TRAIT AND `actions` IS NOT REUSED.**
/// `LiveSink::chunk` takes an [`actions::Pipe`] because an ordinary child has
/// two separate outputs; a pseudo-terminal has **one**, which is what the
/// terminal shows — stdout and stderr arrive already mixed by the operating
/// system, and passing `Pipe::Stdout` would declare a distinction that does not
/// exist here. The other reason is the direction of the dependencies: `actions`
/// brings the flow engine and the store with it, and the terminal engine must
/// not depend on them to open a shell.
///
/// `chunk` must not block for long nor panic: the thread that drains the
/// terminal calls it, and a stalled thread is a process blocked on writing. And
/// it never receives an empty chunk: "zero bytes" is the end of the terminal,
/// not something that was said.
///
/// **THE END IS SAID, NOT INFERRED FROM SILENCE.** A terminal that stops
/// talking is indistinguishable from an idle terminal: whoever watches would go
/// on showing it alive forever, which is the shape fault 12 comes back in every
/// time. [`Output::ended`] arrives once only, after the last chunk, and carries
/// **how** it ended.
pub trait Output: Send + Sync {
    fn chunk(&self, bytes: &[u8]);

    /// **IT HAS A DEFAULT BODY BECAUSE THE SIMPLEST RECIPIENT IS A CLOSURE**,
    /// which receives bytes and has nowhere to put an end. Whoever implements
    /// it declares they want to know; whoever leaves it alone is not forced to
    /// write a type just to ignore it.
    fn ended(&self, _ending: Ending) {}
}

/// How what was running inside a terminal ended.
///
/// **"STILL ALIVE" IS A CASE OF ITS OWN, AND IS NOT TURNED INTO ZERO.** A
/// pseudo-terminal's output can end before the process does — the child need
/// only close its own descriptors and carry on — and calling that case "exited
/// with zero" would be inventing a successful outcome for something nobody saw
/// end. It is the same distinction between zero and blind that the rest of
/// Sailor defends wherever a number appears.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Ending {
    /// Exited on its own, with its own code.
    Exited(i32),
    /// Ended without a code: a signal took it away.
    Killed,
    /// The output ended, the process did not — or it could not be asked.
    StillRunning,
}

impl std::fmt::Display for Ending {
    fn fmt(&self, out: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Ending::Exited(code) => write!(out, "exited with {code}"),
            Ending::Killed => write!(out, "stopped by a signal"),
            Ending::StillRunning => write!(
                out,
                "the output ended, but the process inside has not exited yet"
            ),
        }
    }
}

/// A closure is enough: a simple recipient must not cost a type.
impl<F> Output for F
where
    F: Fn(&[u8]) + Send + Sync,
{
    fn chunk(&self, bytes: &[u8]) {
        self(bytes)
    }
}

/// A recipient that accumulates everything, for whoever wants to look later.
///
/// It lives in the library and not in the tests because both need it, and two
/// copies of an accumulator diverge on the detail that matters: whether
/// `text()` decodes leniently or not.
#[derive(Debug, Default)]
pub struct Buffer {
    bytes: Mutex<Vec<u8>>,
    ending: Mutex<Option<Ending>>,
}

impl Buffer {
    pub fn new() -> Buffer {
        Buffer::default()
    }

    pub fn bytes(&self) -> Vec<u8> {
        locked(&self.bytes).clone()
    }

    /// How the terminal ended, if it ended. `None` means "not yet", it does not
    /// mean "well".
    pub fn ending(&self) -> Option<Ending> {
        *locked(&self.ending)
    }

    /// Waits for the end, up to `limit`. Same reason as [`Buffer::wait_for`]:
    /// a fixed `sleep` is either flaky or slow.
    pub fn wait_for_end(&self, limit: std::time::Duration) -> Option<Ending> {
        let until = std::time::Instant::now() + limit;
        loop {
            if let Some(ending) = self.ending() {
                return Some(ending);
            }
            if std::time::Instant::now() >= until {
                return None;
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
    }

    /// The accumulated text, with undecodable bytes replaced: the loss is
    /// acceptable here because it is looked at, not retransmitted.
    pub fn text(&self) -> String {
        String::from_utf8_lossy(&self.bytes()).into_owned()
    }

    /// Waits for `needle` to appear, up to `limit`. Returns `true` if it did.
    ///
    /// **A TEST ON A REAL PROCESS WAITS, IT DOES NOT SLEEP ONCE.** A fixed
    /// `sleep` is either too short — and the test turns flaky — or too long,
    /// and the suite slows down on every case.
    pub fn wait_for(&self, needle: &str, limit: std::time::Duration) -> bool {
        let until = std::time::Instant::now() + limit;
        loop {
            if self.text().contains(needle) {
                return true;
            }
            if std::time::Instant::now() >= until {
                return false;
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
    }
}

impl Output for Buffer {
    fn chunk(&self, bytes: &[u8]) {
        locked(&self.bytes).extend_from_slice(bytes);
    }

    fn ended(&self, ending: Ending) {
        *locked(&self.ending) = Some(ending);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    /// A thread that dies holding the buffer must not take every later reader
    /// with it: the bytes are whole, and the end can still be said.
    #[test]
    fn a_buffer_still_answers_after_a_thread_died_holding_it() {
        let buffer = Arc::new(Buffer::new());
        buffer.chunk(b"before");
        let poisoning = Arc::clone(&buffer);
        let died = std::thread::spawn(move || {
            let _bytes = poisoning.bytes.lock().expect("still clean");
            let _ending = poisoning.ending.lock().expect("still clean");
            panic!("died holding the buffer");
        })
        .join();
        assert!(died.is_err(), "the thread has to die holding both locks");
        assert!(buffer.bytes.is_poisoned() && buffer.ending.is_poisoned());

        buffer.chunk(b" after");
        assert_eq!(buffer.text(), "before after");
        assert_eq!(buffer.ending(), None);
        buffer.ended(Ending::Exited(3));
        assert_eq!(buffer.ending(), Some(Ending::Exited(3)));
    }
}
