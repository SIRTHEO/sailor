//! **Keeps the window on while the machine underneath is repaired.**
//!
//! `cargo tauri dev` stops what is running **before** recompiling, so every
//! saved file closes the window. Here the order is reversed, and what is
//! running is touched only when the build succeeded. Every process this
//! lights goes into the ledger, which outlives window and session — fault 4.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

pub mod child;

/// Something running that can be stopped. **A trait and not a process**: the
/// rule this crate defends is one line of sequence, and a line of sequence is
/// proved without lighting anything.
pub trait Running {
    fn stop(&mut self) -> Result<(), String>;
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
                        message: format!("il programma acceso non si è fermato: {error}"),
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

/// In che stato è la modalità viva, per chi la guarda da dentro la finestra.
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
                .map_err(|error| format!("creare {}: {error}", parent.display()))?;
        }
        std::fs::write(path, now().to_string())
            .map_err(|error| format!("scrivere {}: {error}", path.display()))
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

/// The port of the window's development server: **the port of fault 4**, held
/// by an orphan twice in one night. Written down again in
/// `desktop/src-tauri/tauri.conf.json`, and `the_dev_port_matches_the_tauri_config`
/// compares the two copies.
pub const DEV_PORT: u16 = 5183;

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
                .map_err(|error| format!("creare {}: {error}", parent.display()))?;
        }
        let text = serde_json::to_string_pretty(self)
            .map_err(|error| format!("comporre lo stato: {error}"))?;
        let temporary = path.with_extension("json.partial");
        std::fs::write(&temporary, text)
            .map_err(|error| format!("scrivere {}: {error}", temporary.display()))?;
        std::fs::rename(&temporary, path)
            .map_err(|error| format!("spostare su {}: {error}", path.display()))?;
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

/// A process the ledger calls running, and what the system says about it.
#[derive(Debug, Clone)]
pub struct LeftRunning {
    pub record: ledger::ProcessRecord,
    /// **Two different questions, kept apart on purpose.** The ledger says
    /// what was started; this says whether that pid still breathes.
    pub still_alive: bool,
}

/// What was left running, confirmed pid by pid.
pub fn left_running(store: &ledger::Ledger) -> Result<Vec<LeftRunning>, ledger::LedgerError> {
    Ok(store
        .processes_left_running()?
        .into_iter()
        .map(|record| LeftRunning {
            still_alive: ledger::pid_is_alive(record.pid),
            record,
        })
        .collect())
}

/// Closes, in the ledger, the rows of processes that stopped breathing.
///
/// **THE LEDGER CANNOT SEE A VIOLENT DEATH**: a process killed from outside
/// writes no ending and stays «running» for ever. A list full of ghosts is a
/// list nobody reads, which is how a process registry stops preventing fault 4.
/// Returns how many it closed.
pub fn close_the_ones_that_stopped_breathing(
    store: &ledger::Ledger,
    now: i64,
) -> Result<usize, ledger::LedgerError> {
    let mut closed = 0;
    for gone in left_running(store)?
        .into_iter()
        .filter(|item| !item.still_alive)
    {
        store.record_process_ended(&ledger::ProcessEndRecord {
            process_id: gone.record.process_id,
            // No exit code is invented: nobody saw it leave.
            exit_code: None,
            ended_at: now,
        })?;
        closed += 1;
    }
    Ok(closed)
}

/// Why the port cannot be taken, when somebody is on it. **Both localhost
/// addresses**: a server on `::1` leaves `127.0.0.1` free, and asking one of
/// them answers «nobody» with somebody plainly there.
pub fn who_holds(port: u16) -> Option<String> {
    for address in ["127.0.0.1", "::1"] {
        match std::net::TcpListener::bind((address, port)) {
            Ok(_) => {}
            Err(error) => return Some(format!("{address}: {error}")),
        }
    }
    None
}

/// Now, in seconds since the epoch.
pub fn now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |elapsed| elapsed.as_secs() as i64)
}
