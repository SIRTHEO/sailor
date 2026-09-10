//! Real processes: starting them, writing them in the ledger, stopping them.
//!
//! **EVERY PROCESS STARTED HERE IS IN THE LEDGER BEFORE IT IS USED** — fault 4
//! cured where it is born. `Process::start` is the one road, it records, and
//! it opens only to the supervisor's `StartToken`: a second road does not
//! compile.

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use ledger::{Ledger, ProcessEndRecord, ProcessRecord};

use crate::{now, BuildOutcome, Running, StartToken};

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
    /// # use supervisor::child::{Process, Spec}; fn spec() -> Spec { unimplemented!() }
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

impl Running for Process {
    fn stop(&mut self) -> Result<(), String> {
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
            let _ = self.stop();
        }
    }
}

/// Builds, and reports what the compiler said.
///
/// **THE ERROR OUTPUT IS KEPT WHOLE.** `cargo` writes its diagnostics on
/// stderr; throwing them away and reporting only "failed" would send whoever
/// looks back into the terminal — doing by hand the work this mode exists to
/// take away.
pub fn cargo_build(manifest: &Path, jobs: Option<u32>) -> BuildOutcome {
    let mut command = Command::new("cargo");
    command
        .arg("build")
        .arg("--manifest-path")
        .arg(manifest)
        .stdin(Stdio::null());
    if let Some(jobs) = jobs {
        command.arg("-j").arg(jobs.to_string());
    }
    match command.output() {
        Ok(output) if output.status.success() => BuildOutcome::Succeeded,
        Ok(output) => BuildOutcome::Failed {
            message: String::from_utf8_lossy(&output.stderr).into_owned(),
        },
        Err(error) => BuildOutcome::Failed {
            message: format!("`cargo build` did not even start: {error}"),
        },
    }
}

/// The instant of the newest change under these roots, in seconds.
///
/// **A POLL AND NOT A WATCHER, AND THE WHY IS DECLARED.** A real watcher
/// (`notify`) wants a new dependency, and this tree keeps dependencies to a
/// minimum by a choice written in `Cargo.toml`. The cost is half a second on a
/// rebuild lasting tens — unnoticeable; the gain is a function read whole.
pub fn newest_change(roots: &[PathBuf]) -> u64 {
    fn walk(directory: &Path, newest: &mut u64) {
        let Ok(entries) = std::fs::read_dir(directory) else {
            return;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            let name = entry.file_name().to_string_lossy().into_owned();
            if path.is_dir() {
                // `target` changes on every build: watching it would mean every
                // rebuild asks for another, for ever.
                if matches!(name.as_str(), "target" | "node_modules" | ".git" | "dist") {
                    continue;
                }
                walk(&path, newest);
                continue;
            }
            if !matches!(
                path.extension().and_then(|extension| extension.to_str()),
                Some("rs") | Some("toml") | Some("json")
            ) {
                continue;
            }
            let seen = entry
                .metadata()
                .ok()
                .and_then(|metadata| metadata.modified().ok())
                .and_then(|time| time.duration_since(std::time::UNIX_EPOCH).ok())
                .map_or(0, |elapsed| elapsed.as_secs());
            if seen > *newest {
                *newest = seen;
            }
        }
    }

    let mut newest = 0;
    for root in roots {
        walk(root, &mut newest);
    }
    newest
}
