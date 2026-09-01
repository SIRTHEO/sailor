//! The bridge between the terminal a person looks at and the pseudo-terminal
//! we own.
//!
//! To type into a live session one either asks the emulator to type, which
//! needs an adaptation per emulator and a list forever behind what ships next,
//! or owns the descriptor typed on. Owning asks nobody.

use crate::pty::{Pty, PtyError, Size};
use std::io::{self, Read, Write};
use std::os::fd::RawFd;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;

/// The outer terminal, put back the way it was when this is dropped.
///
/// Declared limit: a signal that takes the process without unwinding —
/// `SIGKILL`, or `SIGTERM` with no handler — leaves it raw, and whoever is
/// sitting there has to type `reset`.
pub struct RawMode {
    fd: RawFd,
    saved: libc::termios,
}

impl RawMode {
    /// Takes away line buffering and echo: from here on every keystroke goes
    /// straight through, and what shows it is the program inside.
    pub fn take(fd: RawFd) -> io::Result<RawMode> {
        let mut saved: libc::termios = unsafe { std::mem::zeroed() };
        if unsafe { libc::tcgetattr(fd, &mut saved) } != 0 {
            return Err(io::Error::last_os_error());
        }
        let mut raw = saved;
        unsafe { libc::cfmakeraw(&mut raw) };
        raw.c_cc[libc::VMIN] = 1;
        raw.c_cc[libc::VTIME] = 0;
        if unsafe { libc::tcsetattr(fd, libc::TCSANOW, &raw) } != 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(RawMode { fd, saved })
    }
}

impl Drop for RawMode {
    fn drop(&mut self) {
        unsafe { libc::tcsetattr(self.fd, libc::TCSANOW, &self.saved) };
    }
}

/// Whether a descriptor is a real terminal.
pub fn is_a_terminal(fd: RawFd) -> bool {
    unsafe { libc::isatty(fd) == 1 }
}

/// How big a real terminal is right now.
pub fn size_of(fd: RawFd) -> io::Result<Size> {
    let mut measured: libc::winsize = unsafe { std::mem::zeroed() };
    if unsafe { libc::ioctl(fd, libc::TIOCGWINSZ, &mut measured) } != 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(Size {
        rows: measured.ws_row,
        columns: measured.ws_col,
    })
}

static RESIZED: AtomicBool = AtomicBool::new(false);

/// The only thing a signal handler may do without risking finding the library
/// halfway through another call.
extern "C" fn note_resize(_signal: libc::c_int) {
    RESIZED.store(true, Ordering::Relaxed);
}

/// Arranges for a window change to wake a blocked read.
///
/// No `SA_RESTART`, and that is the point: with the automatic restart the
/// signal would arrive and the read would resume unnoticed, leaving the inner
/// terminal at the old size until someone hits a key.
pub fn notice_resizes() -> io::Result<()> {
    let mut action: libc::sigaction = unsafe { std::mem::zeroed() };
    action.sa_sigaction = note_resize as *const () as usize;
    if unsafe { libc::sigaction(libc::SIGWINCH, &action, std::ptr::null_mut()) } != 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(())
}

/// Whether the window changed since this was last asked. Asking clears it.
pub fn resize_was_noticed() -> bool {
    RESIZED.swap(false, Ordering::Relaxed)
}

/// The pseudo-terminal seen as somewhere to write.
pub struct Keys<'a>(pub &'a Pty);

impl Write for Keys<'_> {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        match self.0.write(bytes) {
            Ok(()) => Ok(bytes.len()),
            Err(error) => Err(as_io(error)),
        }
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

fn as_io(error: PtyError) -> io::Error {
    io::Error::new(io::ErrorKind::Other, error.to_string())
}

/// A writer that keeps count of what passes through it.
///
/// The count is shared rather than returned, because whoever wants to know is
/// not the caller of the copy: that call only returns when the terminal ends,
/// and by then the number is of no use to anyone.
pub struct Counted<W> {
    inner: W,
    seen: Arc<AtomicU64>,
}

impl<W: Write> Counted<W> {
    pub fn new(inner: W, seen: Arc<AtomicU64>) -> Counted<W> {
        Counted { inner, seen }
    }
}

impl<W: Write> Write for Counted<W> {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        let written = self.inner.write(bytes)?;
        self.seen.fetch_add(written as u64, Ordering::Relaxed);
        Ok(written)
    }

    fn flush(&mut self) -> io::Result<()> {
        self.inner.flush()
    }
}

/// Copies bytes until the source ends, telling the caller about each signal.
///
/// The interruption is handed out instead of being swallowed: it is the only
/// moment at which a window change can be noticed while nobody is typing.
pub fn pump(
    mut from: impl Read,
    mut into: impl Write,
    mut interrupted: impl FnMut(),
) -> io::Result<()> {
    let mut buffer = [0u8; 4096];
    loop {
        match from.read(&mut buffer) {
            Ok(0) => return Ok(()),
            Ok(read) => {
                into.write_all(&buffer[..read])?;
                into.flush()?;
            }
            Err(error) if error.kind() == io::ErrorKind::Interrupted => interrupted(),
            Err(error) => return Err(error),
        }
    }
}
