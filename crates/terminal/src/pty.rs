//! The pseudo-terminal: the only part of this crate that touches the operating
//! system.
//!
//! **WHY A PSEUDO-TERMINAL AND NOT A PIPE.** A pipe would be enough to read a
//! command's output, and `actions` indeed uses one. It is not enough for a
//! terminal: a program that finds out it is not talking to a terminal changes
//! behaviour — no colours, no interactive questions, `git` does not page, the
//! shell does not print its own prompt. A terminal that behaves differently
//! from a terminal is not the product.
//!
//! **THE TWO ENDS ARE CALLED `leader` AND `follower`.** It is the pair of names
//! POSIX and the operating systems have adopted for what the old manual pages
//! call master and slave. The `leader` end stays with us: what the user types
//! is written to it, and what the terminal shows is read from it. The
//! `follower` end becomes the child process's three descriptors.
//!
//! **`posix_openpt` AND NOT `openpty`.** They do the same thing, but on Linux
//! `openpty` lives in `libutil` and wants one more link line, while the four
//! POSIX calls (`posix_openpt`, `grantpt`, `unlockpt`, `ptsname`) are in the
//! system library everywhere. Less to explain to whoever compiles elsewhere.

use crate::{locked, Workspace};
use std::ffi::{CStr, CString, OsStr};
use std::fs::File;
use std::io::{self, Read, Write};
use std::os::fd::{AsRawFd, FromRawFd, OwnedFd};
use std::os::unix::process::CommandExt;
use std::process::{Child, Command, Stdio};
use std::sync::Mutex;

/// What can go wrong opening or driving a pseudo-terminal.
///
/// **EVERY VARIANT SAYS WHICH GESTURE FAILED**, not just that something failed:
/// "it did not open" sends you looking in four different places, and the four
/// have different repairs.
pub enum PtyError {
    /// The operating system did not give a new terminal.
    NotOpened(io::Error),
    /// The child's end could not be prepared or opened.
    FollowerNotReady(io::Error),
    /// The program did not start: binary missing, not executable, directory
    /// gone between the check and the launch.
    NotStarted(io::Error),
    /// A write, a read or a resize on a terminal already closed or broken.
    Broken(io::Error),
    /// The terminal opened, but its letterbox or its count could not be put
    /// where whoever types from outside will look for them.
    NotRegistered(io::Error),
}

/// **`.expect()` PRINTS THE `Debug`, NOT THE `Display`.** A derived one showed
/// the `io::Error` bare and hid the gesture that failed.
impl std::fmt::Debug for PtyError {
    fn fmt(&self, out: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        std::fmt::Display::fmt(self, out)
    }
}

impl std::fmt::Display for PtyError {
    fn fmt(&self, out: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            PtyError::NotOpened(error) => {
                write!(out, "the system gave no pseudo-terminal: {error}")
            }
            PtyError::FollowerNotReady(error) => write!(
                out,
                "the pseudo-terminal opened, but the child's end could not be prepared: {error}"
            ),
            PtyError::NotStarted(error) => {
                write!(out, "the terminal's program did not start: {error}")
            }
            PtyError::Broken(error) => write!(out, "the terminal no longer answers: {error}"),
            PtyError::NotRegistered(error) => write!(
                out,
                "the terminal opened, but its letterbox could not be registered: {error}"
            ),
        }
    }
}

impl std::error::Error for PtyError {}

/// How big the terminal window is, in characters.
///
/// **THERE IS NO DEFAULT HIDDEN IN THE SYSTEM.** A pseudo-terminal is born at
/// zero rows and zero columns, and a program that asks how wide the screen is
/// gets told zero: `less` does not page, an editor draws itself on a single
/// row. The size is declared on opening.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Size {
    pub rows: u16,
    pub columns: u16,
}

impl Default for Size {
    /// The twenty-four rows by eighty columns every terminal assumes when
    /// nobody has told it otherwise.
    fn default() -> Size {
        Size {
            rows: 24,
            columns: 80,
        }
    }
}

/// An open pseudo-terminal, with the process running inside it.
pub struct Pty {
    /// The end that stays with us. Under a lock because whoever writes and
    /// whoever resizes can be two different threads.
    leader: Mutex<File>,
    child: Mutex<Child>,
    /// The device the program inside is attached to, kept because it is the
    /// name that program reports as its own terminal.
    device: String,
}

/// The intractable number `ptsname` returns from a shared static buffer: two
/// terminals opened together by two threads would take each other's name away.
///
/// **`ptsname_r` IS NOT PORTABLE**: it exists on Linux and not on macOS. A lock
/// of thirty microseconds around the call costs less than two paths to
/// maintain.
static PTSNAME_LOCK: Mutex<()> = Mutex::new(());

impl Pty {
    /// Opens a pseudo-terminal and runs `program` **inside** `workspace`.
    ///
    /// The directory is not one option among others: it is the first argument
    /// because a terminal without a workspace is not something this crate knows
    /// how to make.
    pub fn open(
        workspace: &Workspace,
        program: &OsStr,
        args: &[&OsStr],
        size: Size,
        environment: &[(String, String)],
    ) -> Result<Pty, PtyError> {
        let leader = open_leader()?;
        let follower_name = follower_name(&leader)?;
        let follower = open_follower(&follower_name)?;
        // **THE SIZE IS GIVEN AFTER THE CHILD'S END IS OPEN, NOT BEFORE.**
        // Measured: on macOS a `leader` end with nobody on the other side yet
        // is not a terminal, and the system answers "inappropriate ioctl for
        // device" (errno 25). Until the size is said, the terminal is born at
        // zero rows by zero columns.
        set_size(&leader, size).map_err(PtyError::Broken)?;

        let mut command = Command::new(program);
        command.args(args);
        command.current_dir(&workspace.root);
        // Three distinct descriptors on the same terminal: if the child closes
        // its own standard input, it must not take its output away with it.
        command.stdin(Stdio::from(
            follower.try_clone().map_err(PtyError::FollowerNotReady)?,
        ));
        command.stdout(Stdio::from(
            follower.try_clone().map_err(PtyError::FollowerNotReady)?,
        ));
        command.stderr(Stdio::from(
            follower.try_clone().map_err(PtyError::FollowerNotReady)?,
        ));
        for (name, value) in environment {
            command.env(name, value);
        }

        // The child must become the leader of a new session and take this
        // terminal as its controlling terminal: without that, a Ctrl-C does not
        // reach it, and programs that ask "who is in the foreground?" are told
        // there is nobody.
        let leader_fd = leader.as_raw_fd();
        unsafe {
            command.pre_exec(move || {
                if libc::setsid() < 0 {
                    return Err(io::Error::last_os_error());
                }
                if libc::ioctl(0, libc::TIOCSCTTY as _, 0) < 0 {
                    return Err(io::Error::last_os_error());
                }
                // Our end must not stay open in the child: as long as a copy
                // exists, whoever reads the terminal never sees the end.
                libc::close(leader_fd);
                Ok(())
            });
        }

        let child = command.spawn().map_err(PtyError::NotStarted)?;
        // The child's end is closed here, in the parent. If it stayed open,
        // reading the terminal would never end: the operating system waits for
        // the last writer to go away.
        drop(follower);

        Ok(Pty {
            leader: Mutex::new(File::from(leader)),
            child: Mutex::new(child),
            device: follower_name.to_string_lossy().into_owned(),
        })
    }

    /// The terminal device the program inside is attached to.
    ///
    /// Whoever wants to reach that program keys on this and not on the caller's
    /// own terminal: the two are different, and only this one is what the
    /// program reports about itself.
    pub fn device(&self) -> &str {
        &self.device
    }

    /// The same device under the short name `ps` uses: `/dev/ttys004` is
    /// `ttys004`. It is the key of the letterbox, the count and the mandate.
    pub fn tty(&self) -> &str {
        self.device.strip_prefix("/dev/").unwrap_or(&self.device)
    }

    /// A second end opened on the same terminal, for the thread that reads.
    ///
    /// Reading and writing on the same `File` under the same lock would block
    /// whoever writes for the whole time whoever reads waits — that is, almost
    /// always, because a terminal stands still almost always.
    pub fn reader(&self) -> Result<impl Read + Send, PtyError> {
        locked(&self.leader).try_clone().map_err(PtyError::Broken)
    }

    /// Writes to the terminal's input, as if somebody had typed.
    pub fn write(&self, bytes: &[u8]) -> Result<(), PtyError> {
        let mut leader = locked(&self.leader);
        leader.write_all(bytes).map_err(PtyError::Broken)?;
        leader.flush().map_err(PtyError::Broken)
    }

    /// Tells the terminal how big it is now.
    pub fn resize(&self, size: Size) -> Result<(), PtyError> {
        set_size(&*locked(&self.leader), size).map_err(PtyError::Broken)
    }

    /// Whether the process inside the terminal is still alive.
    pub fn alive(&self) -> bool {
        matches!(locked(&self.child).try_wait(), Ok(None))
    }

    /// How the process inside ended, if it ended.
    ///
    /// **IT DOES NOT WAIT, AND THE DIFFERENCE IS A DEADLOCK.** The draining
    /// thread calls it as soon as the output ends; a real wait would hold the
    /// child's lock the whole time, and whoever closes the terminal meanwhile
    /// would stand at the door of a lock that never opens.
    ///
    /// A failed `try_wait` returns `None` just like a still-living process: in
    /// both cases the honest answer is "I do not know", and that is what the
    /// caller turns into [`crate::Ending::StillRunning`].
    pub fn finished(&self) -> Option<crate::Ending> {
        match locked(&self.child).try_wait() {
            Ok(Some(status)) => Some(match status.code() {
                Some(code) => crate::Ending::Exited(code),
                None => crate::Ending::Killed,
            }),
            _ => None,
        }
    }

    /// The process identifier of what runs inside.
    pub fn process_id(&self) -> u32 {
        locked(&self.child).id()
    }

    /// Closes the terminal and waits until the process is really finished.
    ///
    /// **IT WAITS, AND THAT IS NOT PEDANTRY.** A `kill` without a `wait` leaves
    /// a zombie process for every closed terminal; over a long session they
    /// become hundreds, and the fault shows up somewhere unrelated.
    pub fn close(&self) -> Result<(), PtyError> {
        let mut child = locked(&self.child);
        // A child already dead gives "no such process": that is not a fault, it
        // is the normal condition of whoever closes a terminal after typing
        // `exit`.
        let _ = child.kill();
        child.wait().map_err(PtyError::Broken)?;
        Ok(())
    }
}

fn open_leader() -> Result<OwnedFd, PtyError> {
    let raw = unsafe { libc::posix_openpt(libc::O_RDWR | libc::O_NOCTTY) };
    if raw < 0 {
        // Blamed where the error is born, not where it is printed: a perimeter
        // denies this call, and the four words it answers with read as a defect
        // of whatever crate the failing test happens to sit in.
        return Err(PtyError::NotOpened(crate::scratch::blamed(
            io::Error::last_os_error(),
        )));
    }
    let leader = unsafe { OwnedFd::from_raw_fd(raw) };
    if unsafe { libc::grantpt(leader.as_raw_fd()) } != 0 {
        return Err(PtyError::FollowerNotReady(io::Error::last_os_error()));
    }
    if unsafe { libc::unlockpt(leader.as_raw_fd()) } != 0 {
        return Err(PtyError::FollowerNotReady(io::Error::last_os_error()));
    }
    Ok(leader)
}

fn follower_name(leader: &OwnedFd) -> Result<CString, PtyError> {
    let _guard = locked(&PTSNAME_LOCK);
    let raw = unsafe { libc::ptsname(leader.as_raw_fd()) };
    if raw.is_null() {
        return Err(PtyError::FollowerNotReady(io::Error::last_os_error()));
    }
    Ok(unsafe { CStr::from_ptr(raw) }.to_owned())
}

fn open_follower(name: &CString) -> Result<OwnedFd, PtyError> {
    let raw = unsafe { libc::open(name.as_ptr(), libc::O_RDWR | libc::O_NOCTTY) };
    if raw < 0 {
        return Err(PtyError::FollowerNotReady(io::Error::last_os_error()));
    }
    Ok(unsafe { OwnedFd::from_raw_fd(raw) })
}

fn set_size(leader: &impl AsRawFd, size: Size) -> io::Result<()> {
    let measured = libc::winsize {
        ws_row: size.rows,
        ws_col: size.columns,
        ws_xpixel: 0,
        ws_ypixel: 0,
    };
    let done = unsafe { libc::ioctl(leader.as_raw_fd(), libc::TIOCSWINSZ as _, &measured) };
    if done < 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use std::time::{Duration, Instant};

    /// A terminal whose leader is the null device and whose child is a shell:
    /// enough to drive the locks, without a pseudo-terminal a perimeter denies.
    fn stub(exit_code: i32) -> Pty {
        let child = Command::new("/bin/sh")
            .arg("-c")
            .arg(format!("exit {exit_code}"))
            .spawn()
            .expect("a shell");
        let sink = std::fs::OpenOptions::new()
            .write(true)
            .open("/dev/null")
            .expect("the null device");
        Pty {
            leader: Mutex::new(sink),
            child: Mutex::new(child),
            device: "/dev/null".to_owned(),
        }
    }

    /// The thread draining a terminal must still learn how it ended after some
    /// other thread died holding its locks, or the pane shows it alive forever.
    #[test]
    fn a_terminal_still_answers_after_a_thread_died_holding_its_locks() {
        let pty = Arc::new(stub(7));
        let poisoning = Arc::clone(&pty);
        let died = std::thread::spawn(move || {
            let _leader = poisoning.leader.lock().expect("still clean");
            let _child = poisoning.child.lock().expect("still clean");
            panic!("died holding the terminal");
        })
        .join();
        assert!(died.is_err(), "the thread has to die holding both locks");
        assert!(pty.leader.is_poisoned() && pty.child.is_poisoned());

        pty.write(b"typed").expect("the null device takes anything");
        assert!(pty.resize(Size::default()).is_err(), "the null device has no size");
        assert!(pty.process_id() > 0);
        let until = Instant::now() + Duration::from_secs(5);
        let ending = loop {
            if let Some(ending) = pty.finished() {
                break ending;
            }
            assert!(Instant::now() < until, "the shell never reported its exit");
            std::thread::sleep(Duration::from_millis(10));
        };
        assert_eq!(ending, crate::Ending::Exited(7));
        assert!(!pty.alive());
        pty.close().expect("closing a child already gone");
    }
}
