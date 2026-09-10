//! The open terminals: which they are, in which workspace, and what happens to
//! a line typed into one.
//!
//! **THE TWO HALVES MEET HERE.** Below is the pseudo-terminal, which touches
//! the operating system; alongside is the routing, which is a list of data. A
//! [`Terminal`] is the point where the line the user wrote is first looked at
//! and then — and this only if it is a command — run.
//!
//! **THIS CRATE RUNS NO FLOWS, AND THE BORDER IS DELIBERATE.**
//! [`Terminal::submit`] returns [`Routed::Flow`] and starts nothing. Starting a
//! run means the flow engine, the ledger and the triggers: letting those in
//! here would make it impossible to open a terminal without dragging all of
//! Sailor along, and a terminal must be able to open even when the flows are
//! broken. Whoever composes the program takes that `Flow` and hands it to the
//! manual trigger — the test `a_routed_request_reaches_the_trigger` does
//! exactly that, and that is where the whole link is visible.

use crate::inbox::{self, Inbox};
use crate::pty::{Pty, PtyError, Size};
use crate::routing::{PathLookup, Routed, Router};
use crate::tally::{self, Counters};
use crate::{locked, Catalog, Ending, Output, Workspace};
use std::ffi::OsString;
use std::io::Read;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

/// How a terminal opens: what to start, where, how big.
///
/// **A STRUCT AND NOT SIX ARGUMENTS** because the defaults are the part which
/// counts: opening an ordinary terminal names the workspace and nothing else,
/// and opening a particular one changes one field — without a field added
/// later breaking the callers.
#[derive(Debug, Clone)]
pub struct Opening {
    /// The program to start inside the terminal. Default: the launcher's own
    /// shell, read from `SHELL`.
    pub program: OsString,
    pub args: Vec<OsString>,
    pub size: Size,
    /// Variables added to the inherited ones.
    pub environment: Vec<(String, String)>,
    /// Variables taken away before the program starts, as the descriptor of
    /// the command line being started declares them.
    pub not_inherited: Vec<String>,
    /// The profile the program runs under, as whoever opens knows it; `None`
    /// when no profile applies to that program.
    pub profile: Option<String>,
}

impl Default for Opening {
    fn default() -> Opening {
        Opening {
            program: std::env::var_os("SHELL").unwrap_or_else(|| OsString::from("/bin/sh")),
            args: Vec::new(),
            size: Size::default(),
            profile: None,
            not_inherited: Vec::new(),
            environment: vec![
                // Without `TERM` a program does not know what kind of terminal
                // it faces and behaves as if it had none at all: no colours,
                // no cursor positioning.
                ("TERM".to_string(), "xterm-256color".to_string()),
                // Whatever runs inside must be able to know it is inside
                // Sailor: it is the sole way a nested program does not reopen
                // a terminal inside the terminal.
                ("SAILOR_TERMINAL".to_string(), "1".to_string()),
            ],
        }
    }
}

/// An open terminal, bound to its own workspace.
pub struct Terminal {
    id: String,
    workspace: Workspace,
    pty: Pty,
    router: Arc<Router>,
    closed: Mutex<bool>,
    /// The bytes moved so far, each direction on its own: the number the relay
    /// reads from disk, kept here too so a list can show it without the disk.
    counters: Counters,
    /// The program started inside, by its file name, and the profile it runs
    /// under when one applies: fixed at opening, so a switch made later does
    /// not rewrite who this terminal has been running as.
    program: String,
    profile: Option<String>,
}

impl Terminal {
    pub fn id(&self) -> &str {
        &self.id
    }

    pub fn workspace(&self) -> &Workspace {
        &self.workspace
    }

    pub fn alive(&self) -> bool {
        !*locked(&self.closed) && self.pty.alive()
    }

    pub fn process_id(&self) -> u32 {
        self.pty.process_id()
    }

    /// The tty the program inside reports as its own, under its short name.
    ///
    /// **THE ANCHOR OF A TAB, AND NOT A GUESS.** A product name or a title read
    /// out of the output would name the wrong session the day two run the
    /// same program; the device is what the tracking store already keys on.
    pub fn tty(&self) -> &str {
        self.pty.tty()
    }

    /// How many bytes have crossed this terminal so far, both ways.
    pub fn moved(&self) -> u64 {
        self.counters.total()
    }

    /// **THE LINE THE USER WROTE, LOOKED AT BEFORE IT RUNS.**
    ///
    /// If it is a command, it goes in with the newline, as if typed, and the
    /// answer says why it passed. If it is a request about a flow, **it does
    /// not go in**: the answer says which rule recognised it, which flow it
    /// goes to and with what text, and the caller decides what to do with it.
    pub fn submit(&self, line: &str) -> Result<Routed, PtyError> {
        let decision = self.router.route(line);
        if let Routed::Command { line, .. } = &decision {
            let mut typed = line.as_bytes().to_vec();
            typed.push(b'\n');
            self.press(&typed)?;
        }
        Ok(decision)
    }

    /// Raw bytes on the input, with no routing at all: a Ctrl-C, an arrow key,
    /// the answer to an interactive question.
    ///
    /// **WHAT IS NOT A LINE IS NOT ROUTED.** Routing looks at a whole request;
    /// a key pressed inside an editor is no request, and passing it through
    /// here would have it weighed by a list of rules it has nothing to do with.
    pub fn press(&self, bytes: &[u8]) -> Result<(), PtyError> {
        self.pty.write(bytes)?;
        self.counters
            .typed
            .fetch_add(bytes.len() as u64, Ordering::Relaxed);
        Ok(())
    }

    pub fn resize(&self, size: Size) -> Result<(), PtyError> {
        self.pty.resize(size)
    }

    pub fn close(&self) -> Result<(), PtyError> {
        *locked(&self.closed) = true;
        self.pty.close()
    }

    /// What to show of this terminal in a list.
    pub fn summary(&self) -> Summary {
        Summary {
            id: self.id.clone(),
            workspace_root: self.workspace.root.to_string_lossy().into_owned(),
            workspace_name: self.workspace.name.clone(),
            alive: self.alive(),
            process_id: self.process_id(),
            device: self.tty().to_owned(),
            moved: self.moved(),
            estimated_tokens: estimated_tokens(self.moved()),
            program: self.program.clone(),
            profile: self.profile.clone(),
        }
    }
}

/// The file name of the program an opening starts: what a list shows.
pub fn program_name(program: &std::ffi::OsStr) -> String {
    std::path::Path::new(program)
        .file_name()
        .unwrap_or(program)
        .to_string_lossy()
        .into_owned()
}

/// The tokens the bytes moved amount to, by the model the relay measures with.
///
/// No ceiling here: whether this is too full is a budget somebody declares in
/// a flow, and the row only carries the number that budget is compared to.
pub fn estimated_tokens(moved: u64) -> u64 {
    sessions::fullness::measure(moved, &sessions::fullness::Model::default(), 0).estimated_tokens
}

/// One row of the list of open terminals.
///
/// **THIS TYPE IS THE ROW THE WINDOW RECEIVES, AND NO SECOND COPY OF IT IS
/// MADE.** `docs/the-terminal-contract.md` says it at length: the bridge
/// answers with this struct as it stands, instead of declaring the five fields
/// a second time in TypeScript and a third in a convenience type inside the
/// shell. The names come out in `camelCase` because it is the form the
/// contract writes them in and the window reads them in; they stay English
/// identifiers on both sides, which is what `AGENTS.md` asks.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Summary {
    pub id: String,
    pub workspace_root: String,
    pub workspace_name: String,
    pub alive: bool,
    pub process_id: u32,
    /// The tty of the program inside, short form: what a tab is anchored on.
    pub device: String,
    /// Bytes moved so far, both ways: the same number `terminal list` prints.
    pub moved: u64,
    /// What those bytes amount to in tokens, by the relay's model: an estimate.
    pub estimated_tokens: u64,
    /// The program started inside, by its file name.
    #[serde(default)]
    pub program: String,
    /// The profile it runs under, when one applied at opening.
    #[serde(default)]
    pub profile: Option<String>,
}

/// The terminals opened by this process.
///
/// **THE LIST LIVES HERE AND NOT IN A FILE**, because it answers the question
/// «which terminals have **I** opened»: a list on disk would outlive the
/// process and claim terminals are open which died with it — which is fault 4
/// told again, one step further along. The day terminals must outlive whoever
/// opened them, what records them is the ledger, not this struct.
pub struct Terminals {
    open: Mutex<Vec<Arc<Terminal>>>,
    router: Arc<Router>,
    next: AtomicU64,
    /// Where each terminal's letterbox and count go, when they go anywhere.
    mailroom: Option<PathBuf>,
}

impl Terminals {
    /// This process's terminals, with the routing rules shipped with the
    /// product and those written by the user.
    pub fn current() -> Terminals {
        Terminals::with_router(Arc::new(Router::current()))
    }

    /// With a rule list declared by the caller: the shape the tests use, and
    /// the one for whoever wants different rules for different terminals.
    pub fn with_catalog(catalog: &Catalog) -> Terminals {
        Terminals::with_router(Arc::new(Router::new(
            catalog,
            Arc::new(PathLookup::current()),
        )))
    }

    pub fn with_router(router: Arc<Router>) -> Terminals {
        Terminals {
            open: Mutex::new(Vec::new()),
            router,
            next: AtomicU64::new(1),
            mailroom: None,
        }
    }

    /// From here on every terminal opened registers its tty and its count the
    /// way `sailor terminal run` does: a letterbox at `<mailroom>/<tty>.sock`
    /// and a count at `<mailroom>/<tty>.seen`, so a flow can type into it and
    /// `terminal list` can say how full it is.
    pub fn with_mailroom(mut self, mailroom: PathBuf) -> Terminals {
        self.mailroom = Some(mailroom);
        self
    }

    pub fn mailroom(&self) -> Option<&PathBuf> {
        self.mailroom.as_ref()
    }

    pub fn router(&self) -> &Arc<Router> {
        &self.router
    }

    /// Opens a terminal inside `workspace` and hands its output **as it comes
    /// out** to the sink `make_output` builds.
    ///
    /// The reading thread is born here and dies when the terminal ends: reading
    /// on demand would mean either a buffer growing with nobody draining it, or
    /// a child blocked in a write while nobody asks.
    ///
    /// **THE SINK IS BUILT WITH THE TERMINAL'S NAME IN HAND, AND DOES NOT
    /// RECEIVE IT AFTERWARDS.** Whoever hands the output elsewhere — to a
    /// window, to a network — must say *which* terminal each piece belongs to,
    /// and this function is what assigns the id. Passing a ready-made sink left
    /// an instant in which the first bytes existed and the name did not: the
    /// shell prompt comes out in there, the very piece a watcher expects first.
    /// An `FnOnce(&str)` removes that instant by construction, instead of
    /// covering it with a waiting queue.
    pub fn open(
        &self,
        workspace: Workspace,
        opening: &Opening,
        make_output: impl FnOnce(&str) -> Arc<dyn Output>,
    ) -> Result<Arc<Terminal>, PtyError> {
        let args: Vec<&std::ffi::OsStr> = opening.args.iter().map(AsRef::as_ref).collect();
        let pty = Pty::open(
            &workspace,
            &opening.program,
            &args,
            opening.size,
            &opening.environment,
            &opening.not_inherited,
        )?;
        let mut reader = pty.reader()?;
        let id = format!(
            "{}-{}",
            workspace.name,
            self.next.fetch_add(1, Ordering::Relaxed)
        );
        let output = make_output(&id);
        let terminal = Arc::new(Terminal {
            id,
            workspace,
            pty,
            router: Arc::clone(&self.router),
            closed: Mutex::new(false),
            counters: Counters::new(),
            program: program_name(&opening.program),
            profile: opening.profile.clone(),
        });
        let registered = match &self.mailroom {
            Some(mailroom) => Some(register(&terminal, mailroom)?),
            None => None,
        };
        let draining = Arc::clone(&terminal);
        std::thread::spawn(move || {
            let mut buffer = [0u8; 8192];
            loop {
                match reader.read(&mut buffer) {
                    // Zero bytes is the end of the terminal, not something
                    // said: handing it on would make a watcher write a line
                    // for a fact which never happened.
                    Ok(0) => break,
                    Ok(read) => {
                        draining
                            .counters
                            .shown
                            .fetch_add(read as u64, Ordering::Relaxed);
                        output.chunk(&buffer[..read]);
                    }
                    // A signal which arrived during the read is not the end of
                    // the output, and treating it as such would truncate the
                    // text of a healthy terminal.
                    Err(error) if error.kind() == std::io::ErrorKind::Interrupted => continue,
                    // On Linux the last reader of a pseudo-terminal whose
                    // child has died gets `EIO`, not zero bytes: it is the
                    // end, and calling it a fault would make every normally
                    // closed terminal look broken.
                    Err(_) => break,
                }
            }
            if let Some(registered) = registered {
                registered.withdraw();
            }
            output.ended(how_it_ended(&draining.pty));
        });
        locked(&self.open).push(Arc::clone(&terminal));
        Ok(terminal)
    }

    /// Which terminals are open and in which workspace.
    pub fn list(&self) -> Vec<Summary> {
        locked(&self.open)
            .iter()
            .map(|terminal| terminal.summary())
            .collect()
    }

    pub fn find(&self, id: &str) -> Option<Arc<Terminal>> {
        locked(&self.open)
            .iter()
            .find(|terminal| terminal.id == id)
            .map(Arc::clone)
    }

    /// Closes a terminal and takes it off the list.
    pub fn close(&self, id: &str) -> Option<Result<(), PtyError>> {
        let mut open = locked(&self.open);
        let at = open.iter().position(|terminal| terminal.id == id)?;
        let terminal = open.remove(at);
        Some(terminal.close())
    }

    /// Closes everything. Whoever opens terminals needs a single gesture for
    /// the end, or they forget one.
    pub fn close_all(&self) {
        let taken: Vec<Arc<Terminal>> = std::mem::take(&mut *locked(&self.open));
        for terminal in taken {
            let _ = terminal.close();
        }
    }
}

impl Drop for Terminals {
    fn drop(&mut self) {
        self.close_all();
    }
}

/// The letterbox and the count of one terminal, while it lives.
struct Registered {
    letterbox: inbox::Closer,
    recording: tally::Recording,
    seen: PathBuf,
}

impl Registered {
    /// Takes both away: a terminal that has ended answers nobody, and a count
    /// left behind would read as a session still there to be measured.
    fn withdraw(self) {
        self.letterbox.close();
        self.recording.stop();
        let _ = std::fs::remove_file(&self.seen);
    }
}

/// Opens the letterbox and starts the count, keyed on the terminal's own tty.
///
/// The letterbox is named after the terminal the program inside sees, which
/// is the one the tracking store records: keying on anything else would leave
/// whoever reads that store knocking at an address nobody holds.
fn register(terminal: &Arc<Terminal>, mailroom: &std::path::Path) -> Result<Registered, PtyError> {
    let tty = terminal.tty();
    let letterbox = Inbox::open(mailroom.join(format!("{tty}.sock"))).map_err(|error| {
        let _ = terminal.close();
        PtyError::NotRegistered(error)
    })?;
    let closer = letterbox.closer();
    let typing = Arc::clone(terminal);
    std::thread::spawn(move || {
        letterbox.serve(|bytes| {
            let _ = typing.press(bytes);
        });
    });
    let seen = mailroom.join(format!("{tty}.seen"));
    let recording = terminal.counters.recorded_into(seen.clone());
    Ok(Registered {
        letterbox: closer,
        recording,
        seen,
    })
}

/// How long to keep asking how it ended, once the output has ended.
///
/// **NOT A WAIT FOR SAFETY, BUT THE TIME BETWEEN TWO DIFFERENT FACTS.** The
/// terminal's last descriptor closing and the process being reaped by the
/// system are two things, and in the wrong order for a reader: the child dies,
/// the output ends, and a little later `try_wait` has a verdict to give. Two
/// seconds are long against that gap and short against a watcher.
const HOW_LONG_TO_ASK: std::time::Duration = std::time::Duration::from_secs(2);

/// How the process inside ended, asked without ever blocking.
///
/// **WHEN PATIENCE RUNS OUT IT SAYS «STILL ALIVE», NOT «EXITED ZERO».** A
/// program which closes its own descriptors and keeps running exists, and is
/// rare: which is exactly why a verdict invented in its place would be found
/// by nobody.
fn how_it_ended(pty: &Pty) -> Ending {
    let until = std::time::Instant::now() + HOW_LONG_TO_ASK;
    loop {
        if let Some(ending) = pty.finished() {
            return ending;
        }
        if std::time::Instant::now() >= until {
            return Ending::StillRunning;
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The list is what every request of the host goes through: a thread that
    /// died holding it must not leave every terminal unreachable from then on.
    #[test]
    fn the_list_still_answers_after_a_thread_died_holding_it() {
        let terminals = Arc::new(Terminals::with_catalog(&Catalog::default()));
        let poisoning = Arc::clone(&terminals);
        let died = std::thread::spawn(move || {
            let _open = poisoning.open.lock().expect("still clean");
            panic!("died holding the list");
        })
        .join();
        assert!(died.is_err(), "the thread has to die holding the lock");
        assert!(terminals.open.is_poisoned());

        assert!(terminals.list().is_empty());
        assert!(terminals.find("nobody").is_none());
        assert!(terminals.close("nobody").is_none());
        terminals.close_all();
    }
}
