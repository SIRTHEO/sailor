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
use std::io::Read;
use std::process::{Child, Command, Stdio};
use std::sync::{Arc, Mutex, OnceLock};
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
    let host = start_host(&sailor_binary()?, &store()?)?;
    await_host(client, host, Instant::now() + HOST_STARTS_WITHIN)
}

/// Waits for a host just started to answer, or says why it will not.
///
/// A host that ended says how it ended, in its own words: «did not answer»
/// alone hid a sailor in service that had no `host` form at all.
fn await_host(client: Client, mut host: Child, deadline: Instant) -> Result<Client, String> {
    loop {
        match client.hello() {
            Ok((protocol, pid)) => return checked(client, protocol, pid),
            Err(error) => {
                if let Ok(Some(status)) = host.try_wait() {
                    let mut said = String::new();
                    if let Some(mut stderr) = host.stderr.take() {
                        let _ = stderr.read_to_string(&mut said);
                    }
                    return Err(format!(
                        "`sailor terminal host` ended at once ({status}): {}. If the sailor \
                         in service predates the host, reinstall it from these sources, or \
                         point SAILOR_BIN at one that has it",
                        said.trim()
                    ));
                }
                if Instant::now() >= deadline {
                    return Err(format!(
                        "the terminal host was started and did not answer within {} seconds: {error}",
                        HOST_STARTS_WITHIN.as_secs()
                    ));
                }
                std::thread::sleep(Duration::from_millis(50));
            }
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
/// never waits for it beyond its first answer. Its complaint is kept, for
/// the case it ends before answering.
fn start_host(binary: &Path, store: &Path) -> Result<Child, String> {
    Command::new(binary)
        .args(["terminal", "host", "--store"])
        .arg(store)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|error| {
            format!(
                "could not start `{} terminal host`: {error}",
                binary.display()
            )
        })
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
                    let closed = ClosedEvent {
                        id: id.clone(),
                        status,
                    };
                    let _ = app.emit(CLOSED_EVENT, &closed);
                    crate::events::emit(&app, "terminal", &closed);
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
    let crossing = what_crosses(program, args, &profiles);
    let opened = client_with_host()?.open(
        &workspace_root,
        crossing.program,
        crossing.args,
        crossing.environment,
        rows,
        cols,
        crossing.profile,
    )?;
    follow(&app, &opened.id);
    Ok(opened)
}

/// What the open request carries to the host: the program and its arguments
/// as the window gave them, and the environment of the profiles active now.
#[derive(Debug, PartialEq, Eq)]
struct Crossing {
    program: Option<String>,
    args: Vec<String>,
    environment: Vec<(String, String)>,
    /// The active profile of the command line the program is, when it is one.
    profile: Option<String>,
}

fn what_crosses(
    program: Option<String>,
    args: Option<Vec<String>>,
    profiles: &profiles::ProfileStore,
) -> Crossing {
    let profile = program.as_deref().and_then(|program| {
        let name = std::path::Path::new(program)
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or(program);
        profiles::known_clis()
            .iter()
            .find(|cli| cli.executable == name)
            .and_then(|cli| profiles.active.get(cli.id).cloned())
    });
    Crossing {
        program,
        args: args.unwrap_or_default(),
        environment: profiles::active_environment(profiles),
        profile,
    }
}

/// The line the person confirmed with Enter, **looked at before it runs**: it
/// may go to a flow instead of the shell. A key pressed inside an editor goes
/// through [`terminal_press`] and is never examined.
#[tauri::command]
pub(crate) fn terminal_submit(
    app: AppHandle,
    runs: tauri::State<'_, Arc<crate::run::Runs>>,
    id: String,
    line: String,
) -> Result<Routed, String> {
    let answer = client()?.submit(&id, &line)?;
    Ok(match answer {
        Submitted::Command => Routed::Command,
        // THE TRIGGER LISTENS, AND NOW SOMETHING RUNS. A line the router sent
        // to a flow used to end in a note saying it was not run; the flow
        // starts here, with the line as its mandate, and the pane is told
        // whether it did.
        Submitted::Flow { flow, text, rule } => {
            let origin = format!("terminal · {rule}");
            let started = crate::run::start(&app, runs.inner(), &flow, Some(&text), origin);
            routed(flow, text, rule, started.map(|run| run.run_id))
        }
    })
}

/// What became of the line: the shell has it, or a flow was asked to run
/// and either started or refused, in the engine's words.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub(crate) enum Routed {
    Command,
    Flow {
        flow: String,
        text: String,
        rule: String,
        run_id: Option<String>,
        refused: Option<String>,
    },
}

fn routed(flow: String, text: String, rule: String, started: Result<String, String>) -> Routed {
    let (run_id, refused) = match started {
        Ok(run_id) => (Some(run_id), None),
        Err(why) => (None, Some(why)),
    };
    Routed::Flow {
        flow,
        text,
        rule,
        run_id,
        refused,
    }
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

    /// **THE ARGUMENTS AND THE ENVIRONMENT CROSS THE BRIDGE AS GIVEN.** The
    /// window and the host each prove their own end; this is the piece in
    /// between, where dropping either would turn nothing else red.
    #[test]
    fn what_the_window_gives_is_what_the_host_is_asked_to_open() {
        let mut store = profiles::ProfileStore::default();
        store.profiles.push(profiles::Profile {
            name: "prove".to_owned(),
            cli_id: "claude".to_owned(),
            home_dir: PathBuf::from("/homes/claude/prove"),
            endpoint: None,
        });
        store.active.insert("claude".to_owned(), "prove".to_owned());

        let crossing = what_crosses(
            Some("claude".to_owned()),
            Some(vec!["--resume".to_owned()]),
            &store,
        );
        assert_eq!(
            crossing,
            Crossing {
                program: Some("claude".to_owned()),
                args: vec!["--resume".to_owned()],
                environment: vec![(
                    "CLAUDE_CONFIG_DIR".to_owned(),
                    "/homes/claude/prove".to_owned()
                )],
                profile: Some("prove".to_owned()),
            }
        );
        // A program that is no command line of the profiles runs under none,
        // however many profiles are active.
        let shell = what_crosses(Some("/bin/zsh".to_owned()), None, &store);
        assert_eq!(shell.profile, None);
        // The absurd control: nothing given, nothing invented.
        let bare = what_crosses(None, None, &profiles::ProfileStore::default());
        assert_eq!(bare.program, None);
        assert!(bare.args.is_empty() && bare.environment.is_empty() && bare.profile.is_none());
    }

    /// **A HOST THAT ENDS BEFORE ANSWERING SAYS WHY**, in its own words: a
    /// sailor in service without the `host` form was reported as «did not
    /// answer», which sends a person to look at the wrong thing.
    #[test]
    fn a_host_that_ends_at_once_is_reported_with_its_own_complaint() {
        let scratch = std::env::temp_dir().join(format!("sailor-bridge-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&scratch);
        std::fs::create_dir_all(&scratch).expect("scratch");
        let fake = scratch.join("sailor");
        std::fs::write(
            &fake,
            "#!/bin/sh\necho 'sailor terminal: «host» is not a form of this command' >&2\nexit 2\n",
        )
        .expect("write the fake sailor");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&fake, std::fs::Permissions::from_mode(0o755))
                .expect("make it runnable");
        }

        let host = start_host(&fake, &scratch).expect("the fake starts");
        let refused = await_host(
            Client::in_store(&scratch),
            host,
            Instant::now() + Duration::from_secs(5),
        )
        .err()
        .expect("a host that ended is not a client");
        assert!(
            refused.contains("is not a form of this command") && refused.contains("SAILOR_BIN"),
            "{refused}"
        );
        let _ = std::fs::remove_dir_all(&scratch);
    }

    /// **A LINE SENT TO A FLOW SAYS WHETHER THE FLOW STARTED**, with the run
    /// or with the refusal: «not run» with no reason was the old answer.
    #[test]
    fn a_routed_line_carries_the_run_it_started_or_the_refusal() {
        let went = routed("relay".into(), "go".into(), "question".into(), Ok("relay-7".into()));
        assert_eq!(
            serde_json::to_value(&went).expect("serialise"),
            serde_json::json!({
                "kind": "flow", "flow": "relay", "text": "go", "rule": "question",
                "run_id": "relay-7", "refused": null
            })
        );
        let refused = routed("relay".into(), "go".into(), "question".into(), Err("no flow is called relay".into()));
        match refused {
            Routed::Flow { run_id, refused, .. } => {
                assert_eq!(run_id, None);
                assert_eq!(refused.as_deref(), Some("no flow is called relay"));
            }
            Routed::Command => panic!("a refused flow is not a command"),
        }
    }

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
