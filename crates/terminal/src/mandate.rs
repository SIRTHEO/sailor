//! The work a session hands to whoever comes after it, left where a process
//! outside the session can read it.
//!
//! Not scraped from what the terminal showed. Looking for a phrase in the
//! scrollback is fault family F: the successor is born crippled whenever the
//! phrase is not there, which was most of the time.

use serde::{Deserialize, Serialize};
use std::io;
use std::path::{Path, PathBuf};

/// What one session leaves for the next.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Mandate {
    pub text: String,
    /// When it was written, in seconds.
    ///
    /// Carried so a reader can tell a mandate written for this handover from
    /// one left over by the previous: the same terminal hands over many times,
    /// and a stale mandate read as fresh sends the successor back to work that
    /// is already done.
    pub at: i64,
}

/// Where a terminal's mandate waits, beside its letterbox.
pub fn address_in(store: &Path, tty: &str) -> PathBuf {
    crate::inbox::mailroom(store).join(format!("{tty}.mandate"))
}

/// Writes it so a reader never sees half of it.
pub fn write(path: &Path, mandate: &Mandate) -> io::Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let text = serde_json::to_string(mandate).map_err(io::Error::other)?;
    let beside = path.with_extension("mandate.writing");
    std::fs::write(&beside, text)?;
    std::fs::rename(&beside, path)
}

/// The mandate, or nothing if none was left or it cannot be understood.
///
/// Nothing, and never an empty mandate. A successor started on an empty
/// mandate looks like one that was handed nothing to do, and would go looking
/// for work of its own.
pub fn read(path: &Path) -> Option<Mandate> {
    let text = std::fs::read_to_string(path).ok()?;
    let found: Mandate = serde_json::from_str(&text).ok()?;
    (!found.text.trim().is_empty()).then_some(found)
}

/// Takes it away once it has been handed on.
///
/// The handover is the moment it stops being current. Left behind, the next
/// beat would read it again and hand the same work over twice.
pub fn taken(path: &Path) -> io::Result<()> {
    match std::fs::remove_file(path) {
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        other => other,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(name: &str) -> PathBuf {
        crate::scratch::directory(&format!("mandate-{name}")).expect("a scratch directory")
    }

    #[test]
    fn a_mandate_written_comes_back_the_same() {
        let directory = scratch("roundtrip");
        let path = address_in(&directory, "ttys004");
        let left = Mandate {
            text: "finish the conduit, then measure it".to_owned(),
            at: 1_700_000_000,
        };
        write(&path, &left).expect("write the mandate");
        assert_eq!(read(&path), Some(left));
        let _ = std::fs::remove_dir_all(&directory);
    }

    #[test]
    fn the_mandate_waits_beside_the_letterbox() {
        let store = Path::new("/somewhere/store");
        assert_eq!(
            address_in(store, "ttys004").parent(),
            crate::inbox::address_in(store, "ttys004").parent()
        );
    }

    #[test]
    fn no_mandate_left_is_not_an_empty_mandate() {
        let directory = scratch("absent");
        assert_eq!(read(&address_in(&directory, "ttys009")), None);
        let _ = std::fs::remove_dir_all(&directory);
    }

    /// An empty text is not a mandate. Handed on it would start a successor
    /// with nothing to do, which reads as «go and find something».
    #[test]
    fn a_mandate_with_nothing_in_it_is_no_mandate() {
        let directory = scratch("empty");
        let path = address_in(&directory, "ttys010");
        write(
            &path,
            &Mandate {
                text: "   \n".to_owned(),
                at: 1,
            },
        )
        .expect("write it anyway");
        assert_eq!(read(&path), None);
        let _ = std::fs::remove_dir_all(&directory);
    }

    #[test]
    fn once_handed_on_it_is_gone() {
        let directory = scratch("taken");
        let path = address_in(&directory, "ttys011");
        write(
            &path,
            &Mandate {
                text: "carry on".to_owned(),
                at: 1,
            },
        )
        .expect("write it");
        taken(&path).expect("take it");
        assert_eq!(
            read(&path),
            None,
            "a mandate handed on twice is work done twice"
        );
    }

    /// Taking one that is not there is not a failure: a beat that finds nothing
    /// must not turn red for having found nothing.
    #[test]
    fn taking_a_mandate_that_is_not_there_is_quiet() {
        let directory = scratch("nothing");
        assert!(taken(&address_in(&directory, "ttys012")).is_ok());
        let _ = std::fs::remove_dir_all(&directory);
    }
}
