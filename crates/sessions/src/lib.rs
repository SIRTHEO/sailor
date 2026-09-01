//! Terminal tracking. Sailor never enters the terminal: it is the agent — or
//! the shell — that announces itself to Sailor, and no product-specific code is
//! in here or can get in. The anchor is `(tty, worktree, ancestor)`: a kernel
//! object, the directory being worked in, and the label of whoever drew the
//! window, which is printed and recorded and read by no condition.

//! **THE IRON RULE.** A product name may appear in a label, never in a
//! condition. Printing "running in Orca" is fine; `if host == "orca"` is
//! forbidden, and the test `no_product_name_decides_anything` holds it still.

//! **THE CENSUS IS TRIGGERED, NOT ON A CLOCK.** No timer, no loop, no waiting:
//! [`census::Census::of`] is called when an event arrives, and at no other
//! moment.

pub mod census;
pub mod fullness;
pub mod store;
pub mod tty;

pub use census::{Census, Inhabitant, LocalMachine, Machine, Refusal, Terminal};
pub use store::{
    Anchor, Arrival, SessionError, Sessions, TerminalEvent, TerminalRow, SESSIONS_FILE,
};

use serde::Deserialize;

/// The payload that arrives on standard input: four fields, all optional, and
/// no name here belongs to a product — a session id, where its transcript
/// lives, which directory it runs in, what the event is called. Whoever writes
/// that JSON is tracked the same way, and whoever sends an empty one is tracked
/// all the same, with less information. Fields we do not know are ignored: one
/// field too many is not a broken payload, and refusing it is fault 8.
#[derive(Debug, Clone, Default, Deserialize, PartialEq, Eq)]
pub struct Payload {
    #[serde(default)]
    pub session_id: Option<String>,
    #[serde(default)]
    pub transcript_path: Option<String>,
    #[serde(default)]
    pub cwd: Option<String>,
    #[serde(default)]
    pub hook_event_name: Option<String>,
}

impl Payload {
    /// Reads the text. Empty text is an empty payload, not an error: whoever
    /// runs `sailor session` by hand from a terminal has nothing to send, and
    /// still has a tty and a directory.
    pub fn parse(text: &str) -> Result<Payload, String> {
        if text.trim().is_empty() {
            return Ok(Payload::default());
        }
        serde_json::from_str(text).map_err(|error| format!("the payload is not JSON: {error}"))
    }
}

/// The anchor, built from whatever is at hand.
///
/// The ancestor is asked of the census, which may not know it — and then it
/// stays `None`, which is not an empty string: `None` means "we do not know".
pub fn anchor_from(payload: &Payload, tty: String, census: &Census) -> Anchor {
    let ancestor = census.ancestor_of(&tty).map(str::to_owned);
    let worktree = payload
        .cwd
        .clone()
        .filter(|found| !found.is_empty())
        .unwrap_or_else(working_directory);
    Anchor {
        tty,
        worktree,
        ancestor,
    }
}

/// This process's working directory, or `.` when the system will not say.
pub fn working_directory() -> String {
    std::env::current_dir()
        .map(|found| found.display().to_string())
        .unwrap_or_else(|_| ".".to_owned())
}

/// Now, in seconds. The same clock the store uses.
pub fn now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|elapsed| elapsed.as_secs() as i64)
        .unwrap_or_default()
}
