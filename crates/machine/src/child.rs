//! Real processes: starting them, writing them in the ledger, stopping them.
//!
//! **EVERY PROCESS STARTED HERE IS IN THE LEDGER BEFORE IT IS USED** — fault 4
//! cured where it is born. `Process::start` is the one road, it opens only to
//! a `StartToken`, and it lives here because the command line starts processes
//! too and must not carry the machinery that rebuilds a checkout.

use std::path::PathBuf;
use std::process::{Command, Stdio};

use ledger::{Ledger, ProcessEndRecord, ProcessRecord};

use crate::now;

/// Leave to start a long-lived process, and the ledger the start is written
/// in: one value, so neither travels without the other. Only [`Supervisor`]
/// issues it, and a forged one is a type error, not a fault to find:
/// ```compile_fail
/// let forged = machine::child::StartToken { store: None };
/// ```
pub struct StartToken {
    store: Option<Ledger>,
}

impl StartToken {
    pub fn ledger(&self) -> Option<&Ledger> {
        self.store.as_ref()
    }
}

/// **THE ONE PLACE A LONG PROCESS IS STARTED FROM.** It owns the ledger of
/// started processes and is the only issuer of the token [`Process::start`]
/// takes, so a spawn that never met it does not compile (fault 4).
pub struct Supervisor {
    token: StartToken,
}

impl Supervisor {
    /// Over `None` nothing is recorded: whoever runs without a ledger is told
    /// so where the start happens, and an orphan of theirs has no owner.
    pub fn over(store: Option<Ledger>) -> Self {
        Self {
            token: StartToken { store },
        }
    }

    pub fn ledger(&self) -> Option<&Ledger> {
        self.token.ledger()
    }

    pub fn token(&self) -> &StartToken {
        &self.token
    }

    /// Starts `spec` under this token, recorded in this ledger.
    pub fn start(&self, spec: Spec) -> Result<Process, String> {
        Process::start(spec, &self.token)
    }
}

/// What to light.
#[derive(Debug, Clone)]
pub struct Spec {
    /// The stable name it is found again by after a restart. Not the pid.
    pub process_id: String,
    pub command: String,
    pub args: Vec<String>,
    pub working_directory: PathBuf,
    /// The port it will hold, if it holds one. It is declared **before** the
    /// start: that is what lets whoever comes later ask "who holds 5183"
    /// instead of finding out when their own start fails.
    pub port: Option<u16>,
    pub purpose: String,
    pub started_by: String,
    /// What is laid over the environment of whoever lights it. **Empty means
    /// «whatever the parent has»**, which a development server and the window
    /// both want; it also means a profile never arrives by accident.
    pub environment: Vec<(String, String)>,
    /// Whether it writes on the terminal of whoever lit it: **a child that
    /// outlives its parent lands its lines in somebody else's work.**
    pub speaks: bool,
}

/// A process lit by Sailor, that knows it is one.
pub struct Process {
    spec: Spec,
    child: std::process::Child,
    store: Option<Ledger>,
    stopped: bool,
}

impl Process {
    /// Born in a **process group of its own**, so killing it kills what it lit.
    ///
    /// `sailor-live` starts `cargo` and the window's server, and both have
    /// children: without the group, `kill` reaches the ancestor and the
    /// grandchild survives holding the port. The twin is in
    /// `actions::run_with_timeout`.
    fn in_its_own_group(command: &mut Command) {
        #[cfg(unix)]
        {
            use std::os::unix::process::CommandExt;
            command.process_group(0);
        }
        #[cfg(not(unix))]
        let _ = command;
    }

    /// Signals the **group**, which carries the group leader's number: the
    /// minus sign says so to `kill`. Known limit: a grandchild that detaches
    /// itself with `setsid` leaves the group and survives.
    fn signal_the_whole_group(pid: u32) {
        #[cfg(unix)]
        unsafe {
            libc::kill(-(pid as libc::pid_t), libc::SIGKILL);
        }
        #[cfg(not(unix))]
        let _ = pid;
    }

    /// Spawns, then records the real pid: a record that fails stops what it
    /// started rather than leave an orphan (fault 4). Without the token:
    /// ```compile_fail
    /// # use machine::child::{Process, Spec}; fn spec() -> Spec { unimplemented!() }
    /// let started = Process::start(spec(), None);
    /// ```
    pub fn start(spec: Spec, token: &StartToken) -> Result<Self, String> {
        let mut command = Command::new(&spec.command);
        command
            .args(&spec.args)
            .current_dir(&spec.working_directory)
            .envs(spec.environment.iter().map(|(name, value)| (name, value)))
            .stdin(Stdio::null());
        if !spec.speaks {
            command.stdout(Stdio::null()).stderr(Stdio::null());
        }
        Self::in_its_own_group(&mut command);
        let child = command
            .spawn()
            .map_err(|error| format!("avviare {}: {error}", spec.command))?;

        let mut process = Self {
            spec,
            child,
            store: token.ledger().cloned(),
            stopped: false,
        };

        if let Some(store) = process.store.as_ref() {
            let record = ProcessRecord {
                process_id: process.spec.process_id.clone(),
                pid: process.child.id(),
                command: process.spec.command.clone(),
                args: process.spec.args.clone(),
                working_directory: process.spec.working_directory.display().to_string(),
                port: process.spec.port,
                purpose: process.spec.purpose.clone(),
                started_by: process.spec.started_by.clone(),
                run_id: None,
                started_at: now(),
                born_at: ledger::born_second_of(process.child.id()),
            };
            if let Err(error) = store.record_process_started(&record) {
                Self::signal_the_whole_group(process.child.id());
                let _ = process.child.kill();
                let _ = process.child.wait();
                return Err(format!(
                    "the process started but the ledger did not accept it, \
                     so it was stopped instead of left an orphan: {error}"
                ));
            }
        }

        Ok(process)
    }

    /// Lets it run on after whoever started it has gone.
    ///
    /// **THE ROW STAYS, THE PROCESS STAYS.** Dropping a `Process` stops it, so
    /// a hook that starts a run would kill it on the way out. The ledger holds
    /// the row already: a process let go is still one Sailor can account for.
    pub fn let_it_go(mut self) -> u32 {
        self.stopped = true;
        self.child.id()
    }

    pub fn pid(&self) -> u32 {
        self.child.id()
    }

    pub fn process_id(&self) -> &str {
        &self.spec.process_id
    }

    /// Did it leave on its own? Not a question to the operating system by name:
    /// it interrogates **this** child, which is ours.
    pub fn exited(&mut self) -> Option<Option<i32>> {
        match self.child.try_wait() {
            Ok(Some(status)) => Some(status.code()),
            _ => None,
        }
    }

    /// Writes into the ledger that it ended. Calling it twice does no harm: the
    /// second writes the same closing.
    pub fn record_end(&mut self, exit_code: Option<i32>) {
        if let Some(store) = self.store.as_ref() {
            let _ = store.record_process_ended(&ProcessEndRecord {
                process_id: self.spec.process_id.clone(),
                exit_code,
                ended_at: now(),
            });
        }
        self.stopped = true;
    }
}

impl Process {
    /// Stops it, and writes the end in the ledger.
    pub fn stop_now(&mut self) -> Result<(), String> {
        Process::signal_the_whole_group(self.child.id());
        let outcome = self.child.kill();
        let code = self.child.wait().ok().and_then(|status| status.code());
        self.record_end(code);
        // A child that already left on its own makes `kill` fail: not a fault,
        // it is the very condition wanted.
        match outcome {
            Ok(()) => Ok(()),
            Err(error) if error.kind() == std::io::ErrorKind::InvalidInput => Ok(()),
            Err(error) => Err(error.to_string()),
        }
    }
}

/// **WHOEVER DOES NOT STOP IT EXPLICITLY STOPS IT ANYWAY.** Without this, every
/// error road that abandons a `Process` — a `?`, a panic — leaves running a
/// process the ledger goes on calling alive. It is fault 4 reborn through a
/// door nobody watches.
impl Drop for Process {
    fn drop(&mut self) {
        if !self.stopped {
            let _ = self.stop_now();
        }
    }
}

