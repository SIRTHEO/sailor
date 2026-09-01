//! A terminal's letterbox: where someone outside leaves the bytes to type in.
//!
//! One letterbox per terminal, addressed by the tty. The tracking anchor is
//! already `(tty, worktree, ancestor)`, so whoever found a session in the store
//! knows where to knock without a second register to keep aligned.

use std::io::{self, Read, Write};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};

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
}

impl Inbox {
    /// Opens the letterbox, refusing one that a live process is holding.
    ///
    /// A socket file outlives a process that died badly. Before taking its
    /// place we knock: an answer means the box belongs to someone alive, and
    /// stealing it would silently cut them off from anyone typing.
    pub fn open(path: impl AsRef<Path>) -> io::Result<Inbox> {
        let path = path.as_ref().to_path_buf();
        if path.as_os_str().len() >= LONGEST_ADDRESS {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                format!(
                    "{}: a letterbox address may not reach {LONGEST_ADDRESS} bytes; \
                     move the store somewhere shorter",
                    path.display()
                ),
            ));
        }
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        if path.exists() {
            if UnixStream::connect(&path).is_ok() {
                return Err(io::Error::new(
                    io::ErrorKind::AddrInUse,
                    format!("{}: someone is already answering here", path.display()),
                ));
            }
            std::fs::remove_file(&path)?;
        }
        let listener = UnixListener::bind(&path)?;
        Ok(Inbox { path, listener })
    }

    pub fn address(&self) -> &Path {
        &self.path
    }

    /// Waits for whoever knocks and hands over what they left, one delivery
    /// per connection. Does not return until the letterbox is closed.
    pub fn serve(&self, mut deliver: impl FnMut(&[u8])) {
        for arriving in self.listener.incoming() {
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

/// Leaves bytes in a terminal's letterbox.
///
/// The write half is shut so the far side's `read_to_end` returns instead of
/// waiting for a caller who has already said everything.
pub fn press(address: impl AsRef<Path>, bytes: &[u8]) -> io::Result<()> {
    let mut door = UnixStream::connect(address)?;
    door.write_all(bytes)?;
    door.shutdown(std::net::Shutdown::Write)
}
