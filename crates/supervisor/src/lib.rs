//! **Keeps the window on while the machine underneath is repaired.**
//!
//! `cargo tauri dev` stops what is running **before** recompiling, so every
//! saved file closes the window. Here the order is reversed, and what is
//! running is touched only when the build succeeded. Every process this
//! lights goes into the ledger, which outlives window and session — fault 4.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

pub mod child;

pub use machine::{now, StartToken, Supervisor};

/// The trait the live mode swaps things through: a trait and not a process.
pub trait Running {
    fn stop(&mut self) -> Result<(), String>;
}

impl Running for machine::child::Process {
    fn stop(&mut self) -> Result<(), String> {
        self.stop_now()
    }
}

/// How the build went.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BuildOutcome {
    Succeeded,
    /// What the compiler printed, **carried whole to whoever is looking**:
    /// «build failed» alone sends them to a terminal to find out why.
    Failed {
        message: String,
    },
}

/// What happened to one round of rebuilding.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Rebuild {
    /// Built, the old one stopped, the new one lit.
    Replaced,
    /// The build failed: **the one from before is still running**, which is
    /// what fault 11 asked for.
    KeptRunning { message: String },
    /// Built, and the new one did not start. Distinct from `KeptRunning`:
    /// there something is still on the screen, here nothing is.
    StartFailed { message: String },
}

/// **BUILD FIRST, REPLACE AFTER.** The order is the whole content of this
/// function, and the inverse of `tauri-cli`'s. `start` is not even called when
/// the build fails: there is nothing new to light, and calling it would put
/// the **old** binary back up wearing the face of the new one.
pub fn rebuild_then_swap<R: Running>(
    running: &mut Option<R>,
    build: impl FnOnce() -> BuildOutcome,
    start: impl FnOnce() -> Result<R, String>,
) -> Rebuild {
    match build() {
        BuildOutcome::Failed { message } => Rebuild::KeptRunning { message },
        BuildOutcome::Succeeded => {
            // From here on what is running is touched, and only because the
            // new binary is already on disk.
            if let Some(previous) = running.as_mut() {
                if let Err(error) = previous.stop() {
                    // No way back: keeping something that will not stop would
                    // leave two programs on the same port.
                    return Rebuild::StartFailed {
                        message: format!("the running program did not stop: {error}"),
                    };
                }
            }
            *running = None;
            match start() {
                Ok(fresh) => {
                    *running = Some(fresh);
                    Rebuild::Replaced
                }
                Err(message) => Rebuild::StartFailed { message },
            }
        }
    }
}

/// What state live mode is in, for whoever watches it from inside the window.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LiveState {
    /// Rebuilding. What is on the screen is still the one before.
    Building,
    /// What is on the screen is the newest build.
    Running,
    /// The build failed. **What is on the screen is old**, and whoever looks
    /// must know it: without this state the window lies by omission.
    BuildFailed,
    /// A build is done and waiting, and the window on the screen is the one
    /// before it. Nothing takes it until somebody asks.
    Ready,
}

/// What the live loop does on this turn.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Turn {
    /// Nothing was saved and nobody asked.
    Wait,
    /// Something was saved: build it, and leave the window where it is.
    Build,
    /// Put the build that is waiting on the screen.
    Swap,
}

/// **A BUILD DOES NOT TAKE THE WINDOW AWAY FROM YOU.** Building on every save
/// is right: it is how you learn the code compiles. Swapping on every save is
/// not — it closes the pane being typed in — so the fresh binary waits to be
/// asked for, and goes on by itself only when the screen holds nothing.
pub fn turn_now(saved: bool, waiting: bool, asked: bool, nothing_on_screen: bool) -> Turn {
    if saved {
        return Turn::Build;
    }
    if waiting && (asked || nothing_on_screen) {
        return Turn::Swap;
    }
    Turn::Wait
}

/// The window's request for the build that is waiting.
///
/// **A FILE, FOR THE REASON `LiveStatus` IS A FILE**, read the other way: the
/// supervisor cannot open a channel towards a window it did not start, and the
/// window cannot towards a supervisor it does not know. Asking is creating the
/// file; the answer is the supervisor removing it.
pub struct SwapRequest;

/// What the file is called, under Sailor's home.
pub const SWAP_FILE: &str = "live-swap";

impl SwapRequest {
    pub fn path_in(home: &Path) -> PathBuf {
        home.join(SWAP_FILE)
    }

    /// Asks. Writing it twice is asking once: the file is the whole message.
    pub fn ask(path: &Path) -> Result<(), String> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|error| format!("creating {}: {error}", parent.display()))?;
        }
        std::fs::write(path, now().to_string())
            .map_err(|error| format!("writing {}: {error}", path.display()))
    }

    /// Whether somebody asked, taking the request away as it answers. An ask
    /// that stayed on disk would swap the window again at the next build.
    pub fn take(path: &Path) -> bool {
        std::fs::remove_file(path).is_ok()
    }
}

/// What the supervisor publishes and the window reads.
///
/// **A FILE AND NOT A CHANNEL**: the reader is the program **already running**,
/// built before the supervisor started, and no channel reaches a process born
/// without knowing about you. A file in an agreed place outlives either.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LiveStatus {
    pub state: LiveState,
    /// Empty when all is well; the compiler's output when it is not.
    pub message: String,
    pub changed_at: i64,
    /// Since when what is on the screen has been running: with
    /// `build_failed`, the answer to «how old is what I am looking at».
    pub running_since: Option<i64>,
    /// **WHO IS SAYING THIS, SO A READER CAN ASK WHETHER THEY ARE STILL
    /// THERE.** The file outlives its writer, and a window reading yesterday's
    /// «a build is waiting» offers a gesture nobody listens for. `0` comes
    /// from a file written before the field, and means «cannot tell».
    #[serde(default)]
    pub supervisor_pid: u32,
}

/// What the file is called, under Sailor's home.
pub const STATUS_FILE: &str = "live-status.json";

impl LiveStatus {
    /// Where the status file is, given Sailor's home.
    pub fn path_in(home: &Path) -> PathBuf {
        home.join(STATUS_FILE)
    }

    /// **WHOLE OR NOTHING.** The reader is another process reading whenever
    /// it likes: written in place it would be caught mid-file, and truncated
    /// JSON looks absent exactly when there is an error to show.
    pub fn write(&self, path: &Path) -> Result<(), String> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|error| format!("creating {}: {error}", parent.display()))?;
        }
        let text = serde_json::to_string_pretty(self)
            .map_err(|error| format!("composing the status: {error}"))?;
        let temporary = path.with_extension("json.partial");
        std::fs::write(&temporary, text)
            .map_err(|error| format!("writing {}: {error}", temporary.display()))?;
        std::fs::rename(&temporary, path)
            .map_err(|error| format!("moving onto {}: {error}", path.display()))?;
        Ok(())
    }

    /// **A MISSING OR BROKEN FILE IS NOT AN ERROR, IT IS AN «I DO NOT KNOW».**
    /// A window that died over a half-written status file would be fault 11
    /// remade from this side.
    pub fn read(path: &Path) -> Option<Self> {
        let text = std::fs::read_to_string(path).ok()?;
        serde_json::from_str(&text).ok()
    }
}

pub use machine::{
    close_the_ones_that_stopped_breathing, left_running, stop_the_ones_nobody_wants, LeftRunning,
    Teardown, DEV_PORT,
};

/// Why the port cannot be taken, when somebody is on it.
///
/// **KEPT BECAUSE THE LIVE MODE ASKS A NARROWER QUESTION**: it wants the reason
/// its own bind will fail, in the words the operating system used. Whoever asks
/// «what is on this port» wants `machine::on_the_port`, which tells a port in
/// use from a machine that would not let us look.
pub fn who_holds(port: u16) -> Option<String> {
    match machine::on_the_port_by_binding(port) {
        machine::OnThePort::Free => None,
        machine::OnThePort::Somebody => Some(format!("port {port} is in use")),
        machine::OnThePort::CouldNotLook(why) => Some(why),
        machine::OnThePort::Ours(record) => Some(format!("pid {}", record.pid)),
    }
}

/// The row of a process really holding the port, if there is one.
///
/// **FAULT 4.** A row saying «running» over a number somebody else holds now
/// would stop the start with a name that means nothing, so the row is settled
/// against the kernel before it is believed.
pub fn who_the_ledger_says_holds(
    store: &ledger::Ledger,
    port: u16,
) -> Option<ledger::ProcessRecord> {
    store
        .process_holding_port(port)
        .ok()
        .flatten()
        .filter(ledger::the_same_process_as)
}


#[cfg(test)]
mod tests {
    use super::*;
    use ledger::Ledger;
    use crate::child::Spec;

    struct Scratch(PathBuf);

    impl Scratch {
        fn new() -> Self {
            let path = std::env::temp_dir()
                .join(format!("sailor-supervisor-token-{}", std::process::id()));
            std::fs::create_dir_all(&path).expect("a scratch directory");
            Self(path)
        }
    }

    impl Drop for Scratch {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    /// The token opens the road it guards: a short process starts under it,
    /// stands in the ledger while it runs, and leaves the ledger when stopped.
    #[test]
    fn the_supervisor_starts_and_stops_a_process_with_its_token() {
        let scratch = Scratch::new();
        let supervisor = Supervisor::over(Some(Ledger::open(&scratch.0).expect("a ledger")));
        let mut process = child::Process::start(
            Spec {
                process_id: "a-short-one".to_owned(),
                command: "sh".to_owned(),
                args: vec!["-c".to_owned(), "sleep 0.2".to_owned()],
                working_directory: scratch.0.clone(),
                port: None,
                purpose: "a test".to_owned(),
                started_by: "a test".to_owned(),
                environment: Vec::new(),
                speaks: true,
            },
            supervisor.token(),
        )
        .expect("the process starts");
        let store = supervisor.ledger().expect("the ledger it was given");
        let running: Vec<String> = left_running(store)
            .expect("the ledger is read")
            .into_iter()
            .map(|item| item.record.process_id)
            .collect();
        assert_eq!(running, vec!["a-short-one".to_owned()]);

        let pid = process.pid();
        process.stop().expect("the process stops");

        assert!(!ledger::pid_is_alive(pid), "pid {pid} outlived its stop");
        assert!(left_running(store).expect("the ledger is read").is_empty());
    }
}
