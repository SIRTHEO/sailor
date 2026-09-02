//! A terminal's letterbox: where someone outside leaves the bytes to type in.
//!
//! One letterbox per terminal, addressed by the tty. The tracking anchor is
//! already `(tty, worktree, ancestor)`, so whoever found a session in the store
//! knows where to knock without a second register to keep aligned.

use std::io::{self, Read, Write};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

/// How long an address may be, in bytes.
///
/// The kernel copies a socket path into a fixed field and truncates silently
/// past it. Refusing early turns a letterbox nobody could reach into a sentence
/// that names the home to move.
pub const LONGEST_ADDRESS: usize = 104;

/// Where the letterboxes live, inside the store's home.
pub fn mailroom(store: &Path) -> PathBuf {
    store.join("terminals")
}

/// The address of one terminal's letterbox.
pub fn address_in(store: &Path, tty: &str) -> PathBuf {
    mailroom(store).join(format!("{tty}.sock"))
}

/// A letterbox that is open and listening.
pub struct Inbox {
    path: PathBuf,
    listener: UnixListener,
    closed: Arc<AtomicBool>,
}

/// The handle that closes a letterbox from outside the thread serving it.
///
/// `serve` sits in `accept`, which nothing interrupts: the only way to wake it
/// is to knock, so closing knocks once after raising the flag. A dead
/// terminal whose letterbox kept answering would still be listed as held.
pub struct Closer {
    path: PathBuf,
    closed: Arc<AtomicBool>,
}

impl Closer {
    pub fn close(&self) {
        self.closed.store(true, Ordering::Relaxed);
        let _ = UnixStream::connect(&self.path);
        let _ = std::fs::remove_file(&self.path);
    }
}

impl Inbox {
    /// Opens the letterbox, refusing one that a live process is holding.
    pub fn open(path: impl AsRef<Path>) -> io::Result<Inbox> {
        let path = path.as_ref().to_path_buf();
        let listener = bind_unless_answered(&path)?;
        Ok(Inbox {
            path,
            listener,
            closed: Arc::new(AtomicBool::new(false)),
        })
    }

    pub fn address(&self) -> &Path {
        &self.path
    }

    pub fn closer(&self) -> Closer {
        Closer {
            path: self.path.clone(),
            closed: Arc::clone(&self.closed),
        }
    }

    /// Waits for whoever knocks and hands over what they left, one delivery
    /// per connection. Does not return until the letterbox is closed.
    pub fn serve(&self, mut deliver: impl FnMut(&[u8])) {
        for arriving in self.listener.incoming() {
            if self.closed.load(Ordering::Relaxed) {
                return;
            }
            let Ok(mut caller) = arriving else { continue };
            let mut left = Vec::new();
            if caller.read_to_end(&mut left).is_ok() && !left.is_empty() {
                deliver(&left);
            }
        }
    }
}

impl Drop for Inbox {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.path);
    }
}

/// Binds a socket at `path`, unless a live process is already answering there.
///
/// A socket file outlives a process that died badly. Before taking its place
/// we knock: an answer means the address belongs to someone alive, and
/// stealing it would silently cut them off from anyone calling.
pub fn bind_unless_answered(path: &Path) -> io::Result<UnixListener> {
    if path.as_os_str().len() >= LONGEST_ADDRESS {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!(
                "{}: a socket address may not reach {LONGEST_ADDRESS} bytes; \
                 move the store somewhere shorter",
                path.display()
            ),
        ));
    }
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    if path.exists() {
        if UnixStream::connect(path).is_ok() {
            return Err(io::Error::new(
                io::ErrorKind::AddrInUse,
                format!("{}: someone is already answering here", path.display()),
            ));
        }
        std::fs::remove_file(path)?;
    }
    // A perimeter refuses the bind itself, not the path, and the operating
    // system's four words under a letterbox test read as «this crate is
    // broken». Naming the refuser costs one branch.
    UnixListener::bind(path).map_err(crate::scratch::blamed)
}

/// Leaves bytes in a terminal's letterbox.
///
/// The write half is shut so the far side's `read_to_end` returns instead of
/// waiting for a caller who has already said everything.
pub fn press(address: impl AsRef<Path>, bytes: &[u8]) -> io::Result<()> {
    let mut door = UnixStream::connect(address)?;
    door.write_all(bytes)?;
    door.shutdown(std::net::Shutdown::Write)
}

/// How long the text is left alone before the Enter follows it.
///
/// Two writes are one read to a program that did not get to read in between,
/// and a program reading a whole burst as pasted text leaves the line sitting
/// in its box. Somebody typing always leaves this gap.
pub const ENTER_FOLLOWS_AFTER: Duration = Duration::from_millis(50);

/// Types a whole line into a terminal: the text, then Enter on its own.
///
/// The Enter is a delivery of its own and not the tail of the text, because a
/// command line that reads a long burst as a paste keeps the carriage return
/// inside it as a newline and submits nothing. An empty line is Enter alone,
/// which is what sends whatever is already in the box.
pub fn press_line(address: impl AsRef<Path>, line: &str) -> io::Result<()> {
    let address = address.as_ref();
    if !line.is_empty() {
        press(address, line.as_bytes())?;
        std::thread::sleep(ENTER_FOLLOWS_AFTER);
    }
    press(address, b"\r")
}
