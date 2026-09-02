//! The bridge between the terminal host and the window.
//!
//! **NO TERMINAL LIVES HERE, AND NONE MAY.** The pseudo-terminals are owned by
//! `sailor terminal host`, a process that outlives this one: close the window
//! and the sessions go on. This module is a client of it — it translates seven
//! calls into requests, carries the output to the window as events, and starts
//! the host when nobody answers. Every decision about a terminal is in
//! `crates/terminal`, provable with `cargo test -p terminal`.
//!
//! **THE NAMES AND SHAPES COME FROM THE CONTRACT**, not from this file:
//! `docs/2026-09-01-il-contratto-del-terminale.md`. Whoever changes one
//! changes it there and says so.

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

use base64::Engine;
use serde::Serialize;
use tauri::{AppHandle, Emitter};
use terminal::host::{Client, Frame, Submitted, PROTOCOL};

/// The channel the window receives what a terminal prints on.
pub const OUTPUT_EVENT: &str = "terminal_output";

/// The channel the window learns on that the process inside has ended.
///
/// Without it the window lies by omission: a terminal that stops talking is
/// indistinguishable from a dead one, and would stay drawn alive for ever —
/// fault 12, a sensor that confuses «zero» with «blind».
pub const CLOSED_EVENT: &str = "terminal_closed";

/// How long a freshly started host is given to answer.
const HOST_STARTS_WITHIN: Duration = Duration::from_secs(5);

// ── what the window receives ────────────────────────────────────────────

/// A piece of output, marked with the terminal it comes from and where it
/// sits in that terminal's output.
///
/// **THE BYTES TRAVEL IN BASE64.** What comes out of a pseudo-terminal is a
/// byte sequence cut wherever a read happened to stop, even inside a multibyte
/// character; delivered as a string it would be corrupted, and the lost accent
/// would show only on one word in a long output. `at` is the offset of the
/// first byte since the terminal opened: a pane that read the backlog uses it
/// to drop what it already has.
#[derive(Clone, Serialize)]
struct OutputEvent {
    id: String,
    bytes: String,
    at: u64,
}

/// The process inside a terminal has ended, and how: the engine's own
/// sentence, not a number, because «output ended, process alive» is not zero.
#[derive(Clone, Serialize)]
struct ClosedEvent {
    id: String,
    status: String,
}

/// What a terminal printed before this pane looked, and where it ends.
#[derive(Clone, Serialize)]
pub(crate) struct Backlog {
    at: u64,
    bytes: String,
    upto: u64,
    ended: Option<String>,
}

// ── the host ────────────────────────────────────────────────────────────

fn store() -> Result<PathBuf, String> {
    ledger::default_directory()
        .ok_or_else(|| "no store to hold terminals in: HOME is not set".to_owned())
}

fn client() -> Result<Client, String> {
    Ok(Client::in_store(&store()?))
}

/// A client whose host is answering, started if nobody was.
fn client_with_host() -> Result<Client, String> {
    let client = client()?;
    if let Ok((protocol, pid)) = client.hello() {
        return checked(client, protocol, pid);
    }
    start_host(&store()?)?;
    let deadline = Instant::now() + HOST_STARTS_WITHIN;
    loop {
        match client.hello() {
            Ok((protocol, pid)) => return checked(client, protocol, pid),
            Err(error) if Instant::now() >= deadline => {
                return Err(format!(
                    "the terminal host was started and did not answer within {} seconds: {error}",
                    HOST_STARTS_WITHIN.as_secs()
                ))
            }
            Err(_) => std::thread::sleep(Duration::from_millis(50)),
        }
    }
}

/// A host from another build answers with another number, and is refused by
/// name rather than misread.
fn checked(client: Client, protocol: u32, pid: u32) -> Result<Client, String> {
    if protocol != PROTOCOL {
        return Err(format!(
            "the terminal host answering (pid {pid}) speaks protocol {protocol} and this \
             window speaks {PROTOCOL}: stop that host and open a terminal again"
        ));
    }
    Ok(client)
}

/// The binary the host is a form of. `SAILOR_BIN` names it outright; otherwise
/// it is looked for the way a shell would, and «not there» is only said after
/// looking everywhere.
fn sailor_binary() -> Result<PathBuf, String> {
    if let Some(declared) = std::env::var_os("SAILOR_BIN").filter(|value| !value.is_empty()) {
        return Ok(PathBuf::from(declared));
    }
    match toolbox::probe::look_up("sailor", &toolbox::Machine::current()) {
        toolbox::probe::Look::Found(path) => Ok(path),
        toolbox::probe::Look::Missing => Err(
            "no `sailor` on the search path, and the terminal host is `sailor terminal host`: \
             install sailor, or say where it is with SAILOR_BIN"
                .to_owned(),
        ),
        toolbox::probe::Look::Blocked(why) => Err(format!("could not look for `sailor`: {why}")),
    }
}

/// Starts the host, detached: it makes its own session, and this process
/// never waits for it.
fn start_host(store: &Path) -> Result<(), String> {
    let binary = sailor_binary()?;
    Command::new(&binary)
        .args(["terminal", "host", "--store"])
        .arg(store)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|error| {
            format!(
                "could not start `{} terminal host`: {error}",
                binary.display()
            )
        })?;
    Ok(())
}

// ── the output, as it comes ─────────────────────────────────────────────

/// The terminals whose output a thread of this process is following.
fn following() -> &'static Mutex<HashSet<String>> {
    static FOLLOWING: OnceLock<Mutex<HashSet<String>>> = OnceLock::new();
    FOLLOWING.get_or_init(|| Mutex::new(HashSet::new()))
}

/// Follows one terminal's output on a thread of its own, once per terminal.
///
/// The thread ends with the terminal, or with the host: either way the id
/// leaves the set, so the next list attaches again if there is anything to
/// attach to.
fn follow(app: &AppHandle, id: &str) {
    {
        let mut followed = following()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if !followed.insert(id.to_owned()) {
            return;
        }
    }
    let app = app.clone();
    let id = id.to_owned();
    std::thread::spawn(move || {
        if let Ok(client) = client() {
            // A lost event does not stop anything: the window may be closing,
            // and the host keeps the bytes in its backlog regardless.
            let _ = client.attach(&id, |frame| match frame {
                Frame::Chunk { at, bytes } => {
                    let _ = app.emit(
                        OUTPUT_EVENT,
                        OutputEvent {
                            id: id.clone(),
                            bytes: base64::engine::general_purpose::STANDARD.encode(bytes),
                            at,
                        },
                    );
                }
                Frame::Ended { status } => {
                    let _ = app.emit(
                        CLOSED_EVENT,
                        ClosedEvent {
                            id: id.clone(),
                            status,
                        },
                    );
                }
            });
        }
        following()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .remove(&id);
    });
}

// ── the seven commands ──────────────────────────────────────────────────

/// Opens a terminal inside a workspace, under the active profiles.
///
/// The size is declared at opening and has no convenient default: a
/// pseudo-terminal is born at zero rows by zero columns, and whoever opens it
/// knows how big the pane is. The environment is the profile store's, read
/// now: a terminal opened after a switch runs under the profile just chosen.
#[tauri::command]
pub(crate) fn terminal_open(
    app: AppHandle,
    workspace_root: String,
    program: Option<String>,
    args: Option<Vec<String>>,
    cols: u16,
    rows: u16,
) -> Result<terminal::Summary, String> {
    let profiles = profiles::store_io::load_store()
        .map_err(|error| format!("the profile store cannot be read, so no terminal opens under a profile: {error}"))?;
    let environment = profiles::active_environment(&profiles);
    let opened = client_with_host()?.open(
        &workspace_root,
        program,
        args.unwrap_or_default(),
        environment,
        rows,
        cols,
    )?;
    follow(&app, &opened.id);
    Ok(opened)
}

/// The line the person confirmed with Enter, **looked at before it runs**: it
/// may go to a flow instead of the shell. A key pressed inside an editor goes
/// through [`terminal_press`] and is never examined.
#[tauri::command]
pub(crate) fn terminal_submit(id: String, line: String) -> Result<Submitted, String> {
    client()?.submit(&id, &line)
}

/// Raw bytes on the input: a Ctrl-C, an arrow, the answer to a question.
#[tauri::command]
pub(crate) fn terminal_press(id: String, bytes: String) -> Result<(), String> {
    let pressed = base64::engine::general_purpose::STANDARD
        .decode(bytes.as_bytes())
        // Malformed base64 is not a key: writing it anyway would send rubbish
        // into somebody's terminal.
        .map_err(|error| format!("the bytes to press are not valid base64: {error}"))?;
    client()?.press(&id, &pressed)
}

/// Tells the terminal how big it is now.
#[tauri::command]
pub(crate) fn terminal_resize(id: String, cols: u16, rows: u16) -> Result<(), String> {
    client()?.resize(&id, rows, cols)
}

/// Closes a terminal and takes it off the list.
#[tauri::command]
pub(crate) fn terminal_close(id: String) -> Result<(), String> {
    client()?.close(&id)
}

/// Which terminals are open and in which workspace.
///
/// **NO HOST IS NO TERMINALS, NOT A FAILURE.** The host starts when the first
/// terminal is opened; until then the honest answer is an empty list, and
/// starting a resident process to say so would be starting it for nothing.
#[tauri::command]
pub(crate) fn terminal_list(app: AppHandle) -> Result<Vec<terminal::Summary>, String> {
    let client = client()?;
    if client.hello().is_err() {
        return Ok(Vec::new());
    }
    let listed = client.list()?;
    for row in &listed {
        follow(&app, &row.id);
    }
    Ok(listed)
}

/// What a terminal printed before this pane looked at it.
///
/// Served by the host from its bounded backlog: a pane that mounts five
/// minutes after the terminal opened shows what happened in between, and
/// `upto` is where the live events it already receives take over.
#[tauri::command]
pub(crate) fn terminal_backlog(id: String) -> Result<Backlog, String> {
    let snapshot = client()?.backlog(&id)?;
    Ok(Backlog {
        at: snapshot.at,
        bytes: base64::engine::general_purpose::STANDARD.encode(snapshot.bytes),
        upto: snapshot.upto,
        ended: snapshot.ended,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// **THE EVENTS KEEP THE NAMES THE WINDOW LISTENS TO.** A typo here breaks
    /// nothing that compiles, and leaves a window mute in front of a terminal
    /// that talks.
    #[test]
    fn the_two_events_keep_the_names_the_window_listens_to() {
        assert_eq!(OUTPUT_EVENT, "terminal_output");
        assert_eq!(CLOSED_EVENT, "terminal_closed");
    }

    /// **AN OUTPUT EVENT CARRIES ITS OFFSET**, or a pane that read the backlog
    /// could not tell a live piece it already has from one it lacks.
    #[test]
    fn an_output_event_says_where_its_bytes_sit() {
        let event = serde_json::to_value(OutputEvent {
            id: "t-1".to_owned(),
            bytes: "AA==".to_owned(),
            at: 4096,
        })
        .expect("serialises");
        assert_eq!(event["at"], 4096);
        assert_eq!(event["id"], "t-1");
    }

    /// **THE BYTES LEAVE IN STANDARD BASE64**, which `atob` reads. The case
    /// that matters is the invisible one: an accent split between two pieces
    /// stays two bytes, and the window puts them back together.
    #[test]
    fn a_chunk_cut_in_the_middle_of_a_letter_survives_the_trip() {
        let first = base64::engine::general_purpose::STANDARD.encode([0xC3]);
        let second = base64::engine::general_purpose::STANDARD.encode([0xA0]);
        let mut back = base64::engine::general_purpose::STANDARD
            .decode(first)
            .expect("comes back");
        back.extend(
            base64::engine::general_purpose::STANDARD
                .decode(second)
                .expect("comes back"),
        );
        assert_eq!(String::from_utf8(back).expect("is «à»"), "à");
    }
}
