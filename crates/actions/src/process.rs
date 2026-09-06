//! Running a child within a time limit, and handing its output to whoever
//! watches while it runs: the primitive both actions stand on.

use flow::{Ran, SharedState};
use std::collections::BTreeMap;
use std::io::{Read, Write};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

// ── whoever watches the text as it comes out ────────────────────────────

/// Which of the child's two pipes a chunk of text came from.
///
/// The primitive does not treat them differently — it accumulates both the
/// same way — but handing them to a watcher without saying which is which
/// would put the opacity back somewhere else: an error mixed into ordinary
/// output and indistinguishable from it is no more visible than before.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Pipe {
    Stdout,
    Stderr,
}

impl Pipe {
    /// The short name of the thing, not its presentation: whoever prints it
    /// decides how to show it, but must not reinvent what it is called.
    pub fn name(self) -> &'static str {
        match self {
            Pipe::Stdout => "out",
            Pipe::Stderr => "err",
        }
    }
}

/// Whoever receives a child's output chunks **as** they come out, instead of
/// once the child is dead.
///
/// **RAW BYTES ARE DELIVERED, AND THE CHOICE IS DECLARED.** A read stops
/// wherever it lands, even mid multibyte UTF-8 sequence. Decoding here would
/// swap a replacement character for the accent broken at the boundary — an
/// invisible, permanent fault — or hold the incomplete bytes until the next
/// chunk, reintroducing in miniature the delay this mechanism exists to
/// remove. Decoding belongs to the watcher, the only one who knows what it
/// wants from the bytes, which pass through whole and in order.
///
/// `chunk` must neither block for long nor panic: the two threads draining the
/// pipes call it, and a stalled thread is a child blocked on a write. Nor does
/// it ever receive an empty chunk: "zero bytes" is the end of the pipe, not
/// something the child said, and delivering it would make a watcher write a
/// line for something that never happened.
pub trait LiveSink: Send + Sync {
    fn chunk(&self, pipe: Pipe, bytes: &[u8]);
}

/// A closure will do: a simple watcher should not cost a type.
impl<F> LiveSink for F
where
    F: Fn(Pipe, &[u8]) + Send + Sync,
{
    fn chunk(&self, pipe: Pipe, bytes: &[u8]) {
        self(pipe, bytes)
    }
}

/// Given a step's identifier, who its chunks are handed to.
///
/// **WHY TWO LEVELS AND NOT ONE.** The primitive that reads the pipes does not
/// know what a step is, and must not: that would be policy inside the crate
/// that touches the world. Whoever composes the program knows both and acts as
/// the joint — that is where the text's destination is decided, and where a
/// second consumer attaches tomorrow, with a factory feeding two, without a
/// line of this file changing.
pub trait StepSinks: Send + Sync {
    fn sink_for(&self, step: &str) -> Arc<dyn LiveSink>;
}

/// The watcher for the step in progress, if anybody is watching.
///
/// **WHY THE IDENTIFIER COMES FROM SHARED STATE.** `Action::execute` is not
/// given the step, and changing its signature would touch every implementor in
/// five crates for a datum only one of them needs. The executor writes it into
/// `SharedState` under a reserved key (`flow::CURRENT_STEP`) before every
/// action. With no watcher, or no key, nothing is watched: text delivered
/// without knowing whose it is would be worse than silence, since in a graph
/// with two live steps nobody could attribute it.
pub(crate) fn sink_for_step(
    watcher: &Option<Arc<dyn StepSinks>>,
    shared: &SharedState,
) -> Option<Arc<dyn LiveSink>> {
    let watcher = watcher.as_ref()?;
    let step = shared.get(flow::CURRENT_STEP)?.as_str()?;
    Some(watcher.sink_for(step))
}

// ── the primitive: imposing a time limit ─────────────────────────────────

/// The raw outcome of a command within a time limit. Whoever consumes it
/// decides whether an exit other than zero counts as a real failure or as a
/// fact to report: this primitive does not know, and must not.
pub enum RunOutcome {
    Finished {
        status: std::process::ExitStatus,
        stdout: Vec<u8>,
        stderr: Vec<u8>,
    },
    TimedOut,
    /// With the reason from the operating system. "It did not start" on its
    /// own sends you hunting a missing binary when the file was there and not
    /// executable: two different repairs, and a reader must tell them apart.
    SpawnFailed(String),
}

/// No `timeout(1)`: it does not exist on every machine that runs these
/// binaries. The cap is a `try_wait` loop with a `kill` at expiry, and two
/// threads drain the pipes as they fill — a child that fills them before
/// anybody reads would block on a write forever.
pub fn run_with_timeout(cmd: Command, limit: Duration) -> RunOutcome {
    run_with_timeout_watched(cmd, limit, None)
}

/// Starts the child in a **process group of its own**, so that at expiry it
/// and all its descendants die in one blow.
///
/// Declared price: outside our group it does not get the terminal's Ctrl-C.
/// What keeps it in line is the cap, which now truncates for real, and the
/// sweeper that collects whatever is left standing.
#[cfg(unix)]
fn spawn_in_its_own_group(cmd: &mut Command) -> std::io::Result<std::process::Child> {
    use std::os::unix::process::CommandExt;
    cmd.process_group(0).spawn()
}

#[cfg(not(unix))]
fn spawn_in_its_own_group(cmd: &mut Command) -> std::io::Result<std::process::Child> {
    cmd.spawn()
}

/// Kills the child **and whatever it started**, then reaps it.
///
/// The group carries the leader's number: the minus sign says so to `kill`.
/// Known limit: a grandchild that detached itself with `setsid` leaves the
/// group and survives — no signal of ours reaches it there.
fn kill_the_whole_group(child: &mut std::process::Child) -> Option<std::process::ExitStatus> {
    #[cfg(unix)]
    unsafe {
        libc::kill(-(child.id() as libc::pid_t), libc::SIGKILL);
    }
    let _ = child.kill();
    child.wait().ok()
}

/// Watches the child's words for the ones after which it only waits for a
/// person, forwarding every chunk to whoever else watches. The match runs on
/// a tail of the text, so a word split across two reads is still found and a
/// talkative child is not searched from its first byte at every read.
struct StopOnWords<'a> {
    inner: Option<&'a dyn LiveSink>,
    marks: Vec<String>,
    keep: usize,
    tail: Mutex<String>,
    found: AtomicBool,
}

/// How much text is kept behind the longest word, so the tail never trims
/// away the head of a word that is still arriving.
const TAIL_SLACK: usize = 4096;

impl<'a> StopOnWords<'a> {
    fn new(inner: Option<&'a dyn LiveSink>, marks: &[String]) -> Self {
        let marks: Vec<String> = marks
            .iter()
            .map(|mark| mark.trim().to_lowercase())
            .filter(|mark| !mark.is_empty())
            .collect();
        let longest = marks.iter().map(String::len).max().unwrap_or(0);
        Self {
            inner,
            marks,
            keep: longest + TAIL_SLACK,
            tail: Mutex::new(String::new()),
            found: AtomicBool::new(false),
        }
    }

    fn watching(&self) -> bool {
        !self.marks.is_empty()
    }

    fn found(&self) -> bool {
        self.found.load(Ordering::SeqCst)
    }
}

impl LiveSink for StopOnWords<'_> {
    fn chunk(&self, pipe: Pipe, bytes: &[u8]) {
        if let Some(inner) = self.inner {
            inner.chunk(pipe, bytes);
        }
        if !self.watching() || self.found() {
            return;
        }
        // A poisoned lock means this very function panicked on the other
        // pipe: the watch stops, and the child is waited for as before.
        let Ok(mut tail) = self.tail.lock() else {
            return;
        };
        tail.push_str(&String::from_utf8_lossy(bytes).to_lowercase());
        if self.marks.iter().any(|mark| tail.contains(mark.as_str())) {
            self.found.store(true, Ordering::SeqCst);
            return;
        }
        if tail.len() > self.keep {
            let mut cut = tail.len() - self.keep;
            while !tail.is_char_boundary(cut) {
                cut += 1;
            }
            tail.drain(..cut);
        }
    }
}

/// Like `run_with_timeout`, but hands `sink` every chunk of stdout and stderr
/// **as it arrives**, without waiting for the child to die. With `None` the
/// behaviour is the usual one, byte for byte.
///
/// **ALONGSIDE, NOT ONE MORE PARAMETER ON THE OLD ONE.** Other crates call
/// `run_with_timeout`: changing its signature for a datum they do not need
/// would force them to write `None` to ask for nothing, and an additive
/// promise that breaks its callers is not additive.
pub fn run_with_timeout_watched(
    cmd: Command,
    limit: Duration,
    sink: Option<&dyn LiveSink>,
) -> RunOutcome {
    run_watched_until(cmd, None, limit, sink, None)
}

/// The body both public shapes stand on: a piped input when there is text
/// for it, and a way to stop the child before its limit when whoever watches
/// asks for it — `None` waits for the limit, as before.
fn run_watched_until(
    mut cmd: Command,
    stdin: Option<&[u8]>,
    limit: Duration,
    sink: Option<&dyn LiveSink>,
    stop: Option<&dyn Fn() -> bool>,
) -> RunOutcome {
    if stdin.is_some() {
        cmd.stdin(Stdio::piped());
    }
    cmd.stdout(Stdio::piped()).stderr(Stdio::piped());
    let mut child = match spawn_in_its_own_group(&mut cmd) {
        Ok(c) => c,
        Err(error) => return RunOutcome::SpawnFailed(error.to_string()),
    };
    if let (Some(bytes), Some(mut pipe)) = (stdin, child.stdin.take()) {
        let _ = pipe.write_all(bytes);
        // `pipe` leaves scope here and closes the descriptor: the child sees
        // the EOF even with nothing else to read.
    }
    drain_and_wait(
        child.stdout.take(),
        child.stderr.take(),
        &mut child,
        limit,
        sink,
        stop,
    )
}

/// Like `run_with_timeout`, but writes a text on the child's standard input
/// right after starting it, then closes it — an engine that reads its own
/// input from there (like the OpenRouter test script) would otherwise sit
/// waiting for an EOF that never comes.
pub fn run_with_timeout_and_stdin(cmd: Command, stdin: &[u8], limit: Duration) -> RunOutcome {
    run_with_timeout_and_stdin_watched(cmd, stdin, limit, None)
}

/// The watched twin of `run_with_timeout_and_stdin`, for the same reason.
pub fn run_with_timeout_and_stdin_watched(
    cmd: Command,
    stdin: &[u8],
    limit: Duration,
    sink: Option<&dyn LiveSink>,
) -> RunOutcome {
    run_watched_until(cmd, Some(stdin), limit, sink, None)
}

/// Drains a pipe to EOF, accumulating everything and handing each chunk to a
/// watcher **exactly once**.
///
/// **IN CHUNKS AND NOT `read_to_end`, AND THAT IS THE WHOLE DIFFERENCE.** With
/// `read_to_end` the bytes existed in memory the moment they arrived but
/// nobody could see them before the `join`, that is before the child died:
/// there was no bad buffer to remove, the recipient was missing. Accumulation
/// is unchanged and **does not depend on delivery**: a watcher can neither
/// lose nor double what the outcome reports.
fn drain(pipe: &mut impl Read, which: Pipe, sink: Option<&dyn LiveSink>) -> Vec<u8> {
    let mut all = Vec::new();
    let mut buf = [0u8; 8192];
    loop {
        match pipe.read(&mut buf) {
            Ok(0) => break,
            Ok(read) => {
                all.extend_from_slice(&buf[..read]);
                if let Some(sink) = sink {
                    sink.chunk(which, &buf[..read]);
                }
            }
            // As `read_to_end` did: a signal arriving mid-read is not the end
            // of the output, and treating it so would truncate the text of a
            // healthy child.
            Err(error) if error.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(_) => break,
        }
    }
    all
}

/// The first pause between two `try_wait` calls. Deliberately small: a shell
/// command lasting five milliseconds was waited on for fifty anyway, and in a
/// flow of many short steps that was pure latency, paid at every step.
const FIRST_POLL_PAUSE: Duration = Duration::from_millis(1);

/// Where the growth stops. **Not one millisecond above the fifty it used to
/// be**: on a child lasting minutes the number of wakeups stays what it always
/// was, and the widest gap between the time cap expiring and the `kill` that
/// follows from it is no worse than before.
const MAX_POLL_PAUSE: Duration = Duration::from_millis(50);

/// Doubles up to the cap and stays there.
///
/// **IT LIVES OUTSIDE THE LOOP SO IT CAN BE LOOKED AT ALONE.** Inside the
/// `scope`, between the `try_wait` and the kill, it would be a line nobody can
/// question without starting a process.
fn next_poll_pause(current: Duration) -> Duration {
    (current * 2).min(MAX_POLL_PAUSE)
}

fn drain_and_wait(
    stdout: Option<std::process::ChildStdout>,
    stderr: Option<std::process::ChildStderr>,
    child: &mut std::process::Child,
    limit: Duration,
    sink: Option<&dyn LiveSink>,
    stop: Option<&dyn Fn() -> bool>,
) -> RunOutcome {
    drain_and_wait_paced(stdout, stderr, child, limit, sink, stop, &mut |how_long| {
        std::thread::sleep(how_long)
    })
}

/// The real body, with the pause passed in from outside.
///
/// **WHY THE PAUSE IS A PARAMETER AND NOT A HARD-WIRED `sleep`.** The only
/// thing separating this loop from the old one is the *sequence* of durations
/// it asks for: `1ms, 2ms, 4ms…` in place of `50ms, 50ms…`. Timing it from
/// outside to see that does not work — a loaded machine can stretch times,
/// never shorten them, so the test either lies on a busy machine or gives
/// itself a margin so wide it can no longer tell the two codes apart. That is
/// the same trap fault 7 already turned back. Here a test observes the
/// sequence instead, which the code decides and the clock does not touch.
fn drain_and_wait_paced(
    stdout: Option<std::process::ChildStdout>,
    stderr: Option<std::process::ChildStderr>,
    child: &mut std::process::Child,
    limit: Duration,
    sink: Option<&dyn LiveSink>,
    stop: Option<&dyn Fn() -> bool>,
    pause: &mut dyn FnMut(Duration),
) -> RunOutcome {
    let mut out_pipe = stdout.expect("stdout is piped");
    let mut err_pipe = stderr.expect("stderr is piped");
    // `scope` and not `spawn`: the threads borrow the watcher, which lives on
    // the caller's stack. With detached threads the API would demand a
    // `'static` — an `Arc` — from anybody who wants to watch, a test capturing
    // a local variable included.
    std::thread::scope(|scope| {
        let out_thread = scope.spawn(move || drain(&mut out_pipe, Pipe::Stdout, sink));
        let err_thread = scope.spawn(move || drain(&mut err_pipe, Pipe::Stderr, sink));
        let start = Instant::now();
        let mut poll_pause = FIRST_POLL_PAUSE;
        let status = loop {
            match child.try_wait() {
                Ok(Some(status)) => break Some(status),
                Ok(None) => {
                    // Asked to stop: the child is killed like one over its
                    // limit, but what it said is kept and its end is reported.
                    if stop.is_some_and(|asked| asked()) {
                        break kill_the_whole_group(child);
                    }
                    if start.elapsed() >= limit {
                        kill_the_whole_group(child);
                        break None;
                    }
                    pause(poll_pause);
                    poll_pause = next_poll_pause(poll_pause);
                }
                Err(_) => break None,
            }
        };
        // The `join`s stay after the child's death: that is how the pipes close
        // and the threads end. What was handed over before the kill already
        // reached the watcher, and stays in here too.
        let stdout = out_thread.join().unwrap_or_default();
        let stderr = err_thread.join().unwrap_or_default();
        match status {
            Some(status) => RunOutcome::Finished {
                status,
                stdout,
                stderr,
            },
            None => RunOutcome::TimedOut,
        }
    })
}

// ── invoking an external engine ──────────────────────────────────────────

/// What it takes to invoke an external engine: an already-resolved binary —
/// the path search, with its fallbacks for a service without the installer's
/// shell, stays with the caller, and no locations are hard-wired here — its
/// arguments, the environment, the working directory, an optional text on
/// input, and the time cap.
pub struct EngineInvocation {
    pub bin: String,
    pub args: Vec<String>,
    pub env: BTreeMap<String, String>,
    pub workdir: Option<String>,
    pub stdin: Option<Vec<u8>>,
    pub timeout: Duration,
}

impl EngineInvocation {
    /// The line as `invoke_external_engine` starts it, for the record.
    pub fn ran(&self) -> Ran {
        Ran::new(&self.bin, &self.args)
    }
}

/// The outcome of an invocation: success with the output captured, or one of
/// the failure shapes an external engine can produce — never a panic, and
/// never a judgement on what that failure means to the caller.
///
/// Failures carry what they need to explain themselves: the exit code (`None`
/// when the process was killed by a signal, which is not the same thing as
/// "exited with zero") and the reason from the operating system when it never
/// started. Turning this outcome into a red step loses the typed output, which
/// is why the reason has to live in here, not only in the captured bytes.
pub enum EngineResult {
    Ok {
        stdout: String,
        stderr: String,
    },
    ExitError {
        code: Option<i32>,
        stdout: String,
        stderr: String,
    },
    TimedOut,
    SpawnFailed {
        reason: String,
    },
    /// Stopped by Sailor on the words its descriptor declares mean it only
    /// waits for a person; what it had said travels with it.
    WaitingForAPerson {
        stdout: String,
        stderr: String,
    },
}

pub fn invoke_external_engine(invocation: &EngineInvocation) -> EngineResult {
    invoke_external_engine_watched(invocation, None)
}

/// Like `invoke_external_engine`, but passes the watcher to the primitive.
///
/// **NO NEW FIELD IN `EngineInvocation`**: it is built from a complete literal
/// (`notte` does that), and one more field would break those literals. The
/// watcher is an argument of the call, not a piece of the recipe: it does not
/// describe *what* to run, it describes who is watching.
pub fn invoke_external_engine_watched(
    invocation: &EngineInvocation,
    sink: Option<&dyn LiveSink>,
) -> EngineResult {
    invoke_external_engine_watched_until(invocation, sink, &[])
}

/// Like `invoke_external_engine_watched`, and stops the engine the moment its
/// output carries one of `waits_for_a_person_when`: a sign-in prompt is a wait
/// no step can end, and paying it at every link of a chain is the whole cost.
/// With no words this is the call above, byte for byte.
pub fn invoke_external_engine_watched_until(
    invocation: &EngineInvocation,
    sink: Option<&dyn LiveSink>,
    waits_for_a_person_when: &[String],
) -> EngineResult {
    let watch = StopOnWords::new(sink, waits_for_a_person_when);
    let asked = || watch.found();
    let (sink, stop): (Option<&dyn LiveSink>, Option<&dyn Fn() -> bool>) = if watch.watching() {
        (Some(&watch), Some(&asked))
    } else {
        (sink, None)
    };
    let mut cmd = Command::new(&invocation.bin);
    cmd.args(&invocation.args);
    for (key, value) in &invocation.env {
        cmd.env(key, value);
    }
    if let Some(workdir) = &invocation.workdir {
        cmd.current_dir(workdir);
    }
    if invocation.stdin.is_none() {
        cmd.stdin(Stdio::null());
    }
    let outcome = run_watched_until(
        cmd,
        invocation.stdin.as_deref(),
        invocation.timeout,
        sink,
        stop,
    );
    match outcome {
        RunOutcome::Finished {
            status,
            stdout,
            stderr,
        } => {
            let stdout = String::from_utf8_lossy(&stdout).into_owned();
            let stderr = String::from_utf8_lossy(&stderr).into_owned();
            if watch.found() {
                EngineResult::WaitingForAPerson { stdout, stderr }
            } else if status.success() {
                EngineResult::Ok { stdout, stderr }
            } else {
                EngineResult::ExitError {
                    code: status.code(),
                    stdout,
                    stderr,
                }
            }
        }
        RunOutcome::TimedOut => EngineResult::TimedOut,
        RunOutcome::SpawnFailed(reason) => EngineResult::SpawnFailed { reason },
    }
}

// ── running a check under a time limit ───────────────────────────────

pub struct CheckInvocation {
    pub command: String,
    pub env: BTreeMap<String, String>,
    pub timeout: Duration,
    /// Where the check runs.
    ///
    /// **WITHOUT IT THE DEFECT IS INVISIBLE**: a check always ran where the
    /// process sits, that is wherever the window or the terminal happened to be
    /// launched from. A `cargo test` that passes because it ran in the wrong
    /// tree does not fail: it says yes. The twin `EngineInvocation` already had
    /// this field, and the difference between the two was not a choice.
    pub workdir: Option<String>,
}

impl CheckInvocation {
    /// The program and the arguments a check is started with: the shell, told
    /// to read the command as text. One place, read by the spawn and by the
    /// record alike, so the two cannot drift apart.
    fn command_line(&self) -> (&'static str, [&str; 2]) {
        ("sh", ["-c", self.command.as_str()])
    }

    /// The line as `run_shell_check` starts it, for the record.
    pub fn ran(&self) -> Ran {
        let (program, args) = self.command_line();
        Ran::new(program, args)
    }
}

/// Asymmetric on purpose: what passes has nothing to explain, what fails does
/// — and without these two lines a red step leaves nobody holding the reason,
/// since the typed output of a broken step is never written.
pub enum CheckResult {
    /// **THE OUTPUT TRAVELS WITH THE OUTCOME.** `run_with_timeout` captures it,
    /// and a conversion that dropped it with a `..` let a command say something
    /// nobody could receive. Only the passing branch carries it: a failed
    /// command did not produce the reading it was asked for, and offering it
    /// there would mean reading from a broken instrument.
    Passed {
        stdout: String,
    },
    /// **THE COMPLAINT TRAVELS, THE READING DOES NOT.** `stdout` is here so a
    /// failing check can name what is still unresolved, and most suites name it
    /// there rather than on stderr. It is not the shaped reading: that one
    /// stays on the passing branch, where the command produced what it was
    /// asked for.
    Failed {
        code: Option<i32>,
        stdout: String,
        stderr: String,
    },
    TimedOut,
}

/// Runs `command` with `sh -c`: a task's check is shell text written by
/// whoever defines the task, not a binary resolved upstream.
pub fn run_shell_check(invocation: &CheckInvocation) -> CheckResult {
    run_shell_check_watched(invocation, None)
}

/// Like `run_shell_check`, but passes the watcher to the primitive. Same
/// reason as its twin: `CheckInvocation` gains no fields.
pub fn run_shell_check_watched(
    invocation: &CheckInvocation,
    sink: Option<&dyn LiveSink>,
) -> CheckResult {
    let (program, args) = invocation.command_line();
    let mut cmd = Command::new(program);
    cmd.args(args).stdin(Stdio::null());
    for (key, value) in &invocation.env {
        cmd.env(key, value);
    }
    if let Some(workdir) = &invocation.workdir {
        cmd.current_dir(workdir);
    }
    match run_with_timeout_watched(cmd, invocation.timeout, sink) {
        RunOutcome::Finished {
            status,
            stdout,
            stderr,
        } => {
            if status.success() {
                CheckResult::Passed {
                    stdout: String::from_utf8_lossy(&stdout).into_owned(),
                }
            } else {
                CheckResult::Failed {
                    code: status.code(),
                    stdout: String::from_utf8_lossy(&stdout).into_owned(),
                    stderr: String::from_utf8_lossy(&stderr).into_owned(),
                }
            }
        }
        RunOutcome::TimedOut => CheckResult::TimedOut,
        // An `sh` binary that will not start is a fault of the environment,
        // not of the check: treat it as failed, not as "passed by omission".
        RunOutcome::SpawnFailed(reason) => CheckResult::Failed {
            code: None,
            stdout: String::new(),
            stderr: reason,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn secs(n: u64) -> Duration {
        Duration::from_secs(n)
    }

    // ── run_with_timeout ─────────────────────────────────────────────

    #[test]
    fn a_quick_command_finishes_within_the_limit() {
        let mut cmd = Command::new("sh");
        cmd.arg("-c").arg("echo ciao");
        match run_with_timeout(cmd, secs(5)) {
            RunOutcome::Finished { status, stdout, .. } => {
                assert!(status.success());
                assert_eq!(String::from_utf8_lossy(&stdout).trim(), "ciao");
            }
            _ => panic!("doveva finire in tempo"),
        }
    }

    /// THE MEASURE THAT COULD COME OUT DIFFERENTLY: a command sleeping longer
    /// than the cap is killed, not waited for. A wide sleep (60s) against a
    /// tight limit (1s) would turn this test red if the cap were not really
    /// applied.
    #[test]
    fn a_slow_command_is_killed_at_the_limit() {
        let mut cmd = Command::new("sh");
        cmd.arg("-c").arg("sleep 60");
        let start = Instant::now();
        let outcome = run_with_timeout(cmd, secs(1));
        assert!(matches!(outcome, RunOutcome::TimedOut));
        assert!(
            start.elapsed() < secs(10),
            "il tetto deve troncare, non solo essere misurato: {:?}",
            start.elapsed()
        );
    }

    /// And the grandchild, the real case: an engine starting its own child.
    ///
    /// `sleep & wait` forces the shell to fork, so the grandchild exists on any
    /// shell. Killing the child leaves it alive holding the writing end of the
    /// pipe, and the drainer sits still until its natural death. The clock here
    /// can give a false red, never a false green.
    #[test]
    fn a_grandchild_does_not_keep_the_cap_waiting() {
        let mut cmd = Command::new("sh");
        cmd.arg("-c").arg("sleep 20 & wait");
        let start = Instant::now();
        let outcome = run_with_timeout(cmd, secs(1));
        assert!(matches!(outcome, RunOutcome::TimedOut));
        assert!(
            start.elapsed() < secs(10),
            "il nipote ha tenuto aperta la pipe fino alla fine: {:?}",
            start.elapsed()
        );
    }

    #[test]
    fn a_missing_binary_reports_spawn_failed() {
        let cmd = Command::new("/nessun/binario/qui-di-sicuro");
        assert!(matches!(
            run_with_timeout(cmd, secs(1)),
            RunOutcome::SpawnFailed(_)
        ));
    }

    #[test]
    fn stdin_reaches_the_child_and_gets_echoed_back() {
        let cmd = Command::new("cat");
        match run_with_timeout_and_stdin(cmd, b"un segreto pubblico\n", secs(5)) {
            RunOutcome::Finished { stdout, .. } => {
                assert_eq!(String::from_utf8_lossy(&stdout), "un segreto pubblico\n");
            }
            _ => panic!("cat doveva rispondere"),
        }
    }

    // ── the text delivered while the child is alive ───────────────────

    /// A watcher that records **when** it received each chunk: it is the
    /// instant, not the content, that separates "delivered while it ran" from
    /// "delivered all at the end".
    struct Recorder {
        start: Instant,
        chunks: std::sync::Mutex<Vec<(Duration, Pipe, Vec<u8>)>>,
    }

    impl Recorder {
        fn new() -> Self {
            Self {
                start: Instant::now(),
                chunks: std::sync::Mutex::new(Vec::new()),
            }
        }

        fn seen(&self) -> Vec<(Duration, Pipe, Vec<u8>)> {
            self.chunks.lock().expect("nessuno panica qui").clone()
        }

        /// Every byte of one pipe, rejoined in delivery order.
        fn joined(&self, want: Pipe) -> Vec<u8> {
            self.seen()
                .into_iter()
                .filter(|(_, pipe, _)| *pipe == want)
                .flat_map(|(_, _, bytes)| bytes)
                .collect()
        }
    }

    impl LiveSink for Recorder {
        fn chunk(&self, pipe: Pipe, bytes: &[u8]) {
            self.chunks.lock().expect("nessuno panica qui").push((
                self.start.elapsed(),
                pipe,
                bytes.to_vec(),
            ));
        }
    }

    /// THE TEST THAT COUNTS, AND THAT THE OLD CODE WOULD FAIL: the command
    /// prints, sleeps four seconds, prints again. It does not check the text is
    /// there at the end — that would pass even delivering it all in one block
    /// at the child's death — it checks **when** the first chunk arrived.
    ///
    /// Wide margins on purpose: four seconds of sleep against a threshold of
    /// two, so a loaded machine does not turn the test red at random.
    #[test]
    fn the_first_chunk_arrives_while_the_child_is_still_alive() {
        let mut cmd = Command::new("sh");
        cmd.arg("-c").arg("echo primo; sleep 4; echo secondo");
        let recorder = Recorder::new();
        let start = Instant::now();
        let outcome = run_with_timeout_watched(cmd, secs(30), Some(&recorder));
        let whole = start.elapsed();
        assert!(
            whole >= secs(4),
            "il comando doveva davvero durare quattro secondi, altrimenti la \
             misura non distingue niente: {whole:?}"
        );
        let seen = recorder.seen();
        let (when, pipe, bytes) = seen.first().cloned().expect("qualcosa doveva arrivare");
        // THE TIME BEFORE ALL THE REST: the instant is what this test measures,
        // and reading it last would hide the real reason for a red. It holds
        // against the empty-chunk assertion below too: put block delivery back
        // and that one fires *as well*, since on a silent pipe `read_to_end`
        // yields zero bytes, and whoever reads the red would find the lesser
        // defect in place of the big one.
        assert!(
            when < secs(2),
            "il primo pezzo è arrivato dopo {when:?}, cioè con la fine del \
             comando e non mentre girava"
        );
        assert!(
            seen.iter().all(|(_, _, bytes)| !bytes.is_empty()),
            "un pezzo vuoto non è qualcosa che il figlio ha detto: {seen:?}"
        );
        assert_eq!(pipe, Pipe::Stdout);
        assert!(
            String::from_utf8_lossy(&bytes).contains("primo"),
            "il primo pezzo doveva essere «primo»: {:?}",
            String::from_utf8_lossy(&bytes)
        );
        match outcome {
            RunOutcome::Finished { stdout, .. } => {
                let all = String::from_utf8_lossy(&stdout).into_owned();
                assert!(all.contains("primo") && all.contains("secondo"), "{all}");
            }
            _ => panic!("doveva finire in tempo"),
        }
    }

    /// A watcher must know which of the two pipes text comes from: an error
    /// indistinguishable from ordinary output is no more visible than before.
    #[test]
    fn each_chunk_says_which_pipe_produced_it() {
        let mut cmd = Command::new("sh");
        cmd.arg("-c").arg("echo di-fuori; echo di-errore 1>&2");
        let recorder = Recorder::new();
        let _ = run_with_timeout_watched(cmd, secs(10), Some(&recorder));
        let out = String::from_utf8_lossy(&recorder.joined(Pipe::Stdout)).into_owned();
        let err = String::from_utf8_lossy(&recorder.joined(Pipe::Stderr)).into_owned();
        assert!(out.contains("di-fuori"), "stdout consegnato: {out:?}");
        assert!(err.contains("di-errore"), "stderr consegnato: {err:?}");
        assert!(
            !out.contains("di-errore"),
            "stderr finito su stdout: {out:?}"
        );
        assert!(
            !err.contains("di-fuori"),
            "stdout finito su stderr: {err:?}"
        );
    }

    /// NOTHING LOST AND NOTHING DOUBLED: the sum of the delivered chunks is,
    /// byte for byte, the output the outcome reports. Many lines on purpose:
    /// with one the pipe would empty in a single read and the test would say
    /// nothing about what happens when the chunks are many.
    #[test]
    fn the_delivered_chunks_add_up_to_the_accumulated_output() {
        let mut cmd = Command::new("sh");
        cmd.arg("-c")
            .arg("i=0; while [ $i -lt 500 ]; do echo \"riga $i di uscita normale\"; i=$((i+1)); done; echo problema 1>&2");
        let recorder = Recorder::new();
        match run_with_timeout_watched(cmd, secs(30), Some(&recorder)) {
            RunOutcome::Finished { stdout, stderr, .. } => {
                assert_eq!(recorder.joined(Pipe::Stdout), stdout);
                assert_eq!(recorder.joined(Pipe::Stderr), stderr);
                assert!(!stdout.is_empty(), "l'uscita non doveva essere vuota");
            }
            _ => panic!("doveva finire in tempo"),
        }
    }

    /// THE CAP STILL KILLS, and text already delivered before the kill neither
    /// vanishes nor arrives twice.
    #[test]
    fn what_was_said_before_the_kill_is_delivered_once() {
        let mut cmd = Command::new("sh");
        // `exec` is not ornament: without it `sh` stays the child and `sleep`
        // becomes a grandchild holding the pipe open past the father's kill —
        // and then the grandchild is waited for, not the cap. That is a
        // property of the shell, and not this test's job to judge: the cap is
        // measured here, so the killed process is made the only writer.
        cmd.arg("-c").arg("echo vivo; exec sleep 60");
        let recorder = Recorder::new();
        let start = Instant::now();
        let outcome = run_with_timeout_watched(cmd, secs(3), Some(&recorder));
        assert!(matches!(outcome, RunOutcome::TimedOut));
        assert!(
            start.elapsed() < secs(30),
            "il tetto deve troncare: {:?}",
            start.elapsed()
        );
        let out = String::from_utf8_lossy(&recorder.joined(Pipe::Stdout)).into_owned();
        assert_eq!(
            out.matches("vivo").count(),
            1,
            "«vivo» doveva arrivare una volta sola: {out:?}"
        );
    }

    /// NOT WATCHING GETS EXACTLY WHAT IT GOT BEFORE: the same command, once
    /// with a watcher and once without, and the two accumulated outputs match
    /// byte for byte. Live delivery is not a branch that changes the outcome.
    #[test]
    fn without_a_watcher_the_outcome_is_byte_for_byte_the_same() {
        let command = "echo prima; echo dopo; echo lamentela 1>&2";
        let mut watched_cmd = Command::new("sh");
        watched_cmd.arg("-c").arg(command);
        let recorder = Recorder::new();
        let watched = run_with_timeout_watched(watched_cmd, secs(10), Some(&recorder));
        let mut plain_cmd = Command::new("sh");
        plain_cmd.arg("-c").arg(command);
        let plain = run_with_timeout(plain_cmd, secs(10));
        match (watched, plain) {
            (
                RunOutcome::Finished {
                    status: watched_status,
                    stdout: watched_out,
                    stderr: watched_err,
                },
                RunOutcome::Finished {
                    status: plain_status,
                    stdout: plain_out,
                    stderr: plain_err,
                },
            ) => {
                assert_eq!(watched_status.code(), plain_status.code());
                assert_eq!(watched_out, plain_out);
                assert_eq!(watched_err, plain_err);
                assert_eq!(
                    String::from_utf8_lossy(&plain_out).trim(),
                    "prima\ndopo",
                    "l'uscita intera deve restare nell'esito"
                );
                assert_eq!(String::from_utf8_lossy(&plain_err).trim(), "lamentela");
            }
            _ => panic!("tutti e due dovevano finire in tempo"),
        }
    }

    /// A watcher can be a closure: a simple one should not require declaring a
    /// type to have it.
    #[test]
    fn a_closure_is_a_watcher_too() {
        let seen = std::sync::Mutex::new(Vec::new());
        let sink = |pipe: Pipe, bytes: &[u8]| {
            seen.lock()
                .expect("nessuno panica qui")
                .push((pipe, bytes.to_vec()));
        };
        let mut cmd = Command::new("sh");
        cmd.arg("-c").arg("echo per-la-closure");
        let _ = run_with_timeout_watched(cmd, secs(10), Some(&sink));
        let seen = seen.into_inner().expect("nessuno panica qui");
        assert!(seen.iter().any(|(pipe, bytes)| *pipe == Pipe::Stdout
            && String::from_utf8_lossy(bytes).contains("per-la-closure")));
    }

    // ── invoke_external_engine ───────────────────────────────────────

    #[test]
    fn a_successful_engine_call_is_ok() {
        let invocation = EngineInvocation {
            bin: "sh".to_string(),
            args: vec!["-c".to_string(), "echo 'answer: 42'".to_string()],
            env: BTreeMap::new(),
            workdir: None,
            stdin: None,
            timeout: secs(5),
        };
        match invoke_external_engine(&invocation) {
            EngineResult::Ok { stdout, .. } => assert!(stdout.contains("answer: 42"), "{stdout}"),
            _ => panic!("doveva riuscire"),
        }
    }

    #[test]
    fn a_nonzero_exit_is_exit_error_not_ok() {
        let invocation = EngineInvocation {
            bin: "sh".to_string(),
            args: vec!["-c".to_string(), "echo boom 1>&2; exit 1".to_string()],
            env: BTreeMap::new(),
            workdir: None,
            stdin: None,
            timeout: secs(5),
        };
        match invoke_external_engine(&invocation) {
            EngineResult::ExitError { stderr, .. } => assert!(stderr.contains("boom"), "{stderr}"),
            _ => panic!("un'uscita diversa da zero è un errore di uscita, non un successo"),
        }
    }

    #[test]
    fn an_engine_env_var_is_visible_to_the_child() {
        let mut env = BTreeMap::new();
        env.insert("PROVA_ACTIONS".to_string(), "c'è".to_string());
        let invocation = EngineInvocation {
            bin: "sh".to_string(),
            args: vec!["-c".to_string(), "echo \"$PROVA_ACTIONS\"".to_string()],
            env,
            workdir: None,
            stdin: None,
            timeout: secs(5),
        };
        match invoke_external_engine(&invocation) {
            EngineResult::Ok { stdout, .. } => assert_eq!(stdout.trim(), "c'è"),
            _ => panic!("doveva riuscire"),
        }
    }

    #[test]
    fn a_missing_engine_binary_is_spawn_failed() {
        let invocation = EngineInvocation {
            bin: "/nessun/binario/qui-di-sicuro".to_string(),
            args: vec![],
            env: BTreeMap::new(),
            workdir: None,
            stdin: None,
            timeout: secs(5),
        };
        assert!(matches!(
            invoke_external_engine(&invocation),
            EngineResult::SpawnFailed { .. }
        ));
    }

    #[test]
    fn an_engine_that_never_returns_times_out() {
        let invocation = EngineInvocation {
            bin: "sh".to_string(),
            args: vec!["-c".to_string(), "exec sleep 60".to_string()],
            env: BTreeMap::new(),
            workdir: None,
            stdin: None,
            timeout: secs(1),
        };
        assert!(matches!(
            invoke_external_engine(&invocation),
            EngineResult::TimedOut
        ));
    }

    #[test]
    fn engine_stdin_reaches_an_engine_that_reads_it() {
        let invocation = EngineInvocation {
            bin: "cat".to_string(),
            args: vec![],
            env: BTreeMap::new(),
            workdir: None,
            stdin: Some(b"prompt dall'ingresso\n".to_vec()),
            timeout: secs(5),
        };
        match invoke_external_engine(&invocation) {
            EngineResult::Ok { stdout, .. } => {
                assert_eq!(stdout, "prompt dall'ingresso\n");
            }
            _ => panic!("doveva riuscire"),
        }
    }

    // ── an engine that only waits for a person ───────────────────────

    fn waiting_engine(script: &str) -> EngineInvocation {
        EngineInvocation {
            bin: "sh".to_string(),
            args: vec!["-c".to_string(), script.to_string()],
            env: BTreeMap::new(),
            workdir: None,
            stdin: None,
            timeout: secs(30),
        }
    }

    /// THE MEASURE THAT COULD COME OUT DIFFERENTLY: the child announces the
    /// wait and then sleeps twenty seconds under a thirty-second limit. Waited
    /// in full it would end at the limit; stopped on its words it ends at once,
    /// and what it said before the stop travels with the result. The control:
    /// with no words declared the same child is waited for, and hits the limit.
    #[test]
    fn an_engine_that_says_it_waits_for_a_person_is_stopped_at_once() {
        let script = "echo 'Waiting for somebody at the door' 1>&2; exec sleep 20";
        let words = vec!["waiting for somebody".to_string()];
        let start = Instant::now();
        let result = invoke_external_engine_watched_until(&waiting_engine(script), None, &words);
        let took = start.elapsed();
        match result {
            EngineResult::WaitingForAPerson { stderr, .. } => {
                assert!(stderr.contains("Waiting for somebody at the door"), "{stderr:?}");
            }
            _ => panic!("an engine stopped on its words says so"),
        }
        assert!(took < secs(10), "the wait was paid: {took:?}");

        let mut unwatched = waiting_engine(script);
        unwatched.timeout = secs(1);
        assert!(matches!(
            invoke_external_engine_watched_until(&unwatched, None, &[]),
            EngineResult::TimedOut
        ));
    }

    /// A word cut in two by the pipe is still one word: the watch keeps a tail
    /// of what came before, so the second half completes the first.
    #[test]
    fn a_word_split_across_two_reads_is_still_found() {
        let script = "printf 'Waiting for some'; sleep 0.5; printf 'body at the door\\n'; exec sleep 20";
        let words = vec!["waiting for somebody".to_string()];
        let start = Instant::now();
        let result = invoke_external_engine_watched_until(&waiting_engine(script), None, &words);
        assert!(
            matches!(result, EngineResult::WaitingForAPerson { .. }),
            "the two halves were not joined"
        );
        assert!(start.elapsed() < secs(10), "the wait was paid: {:?}", start.elapsed());
    }

    // ── run_shell_check ───────────────────────────────────────────────

    #[test]
    fn a_true_check_passes() {
        let invocation = CheckInvocation {
            command: "true".to_string(),
            env: BTreeMap::new(),
            timeout: secs(5),
            workdir: None,
        };
        assert!(matches!(
            run_shell_check(&invocation),
            CheckResult::Passed { .. }
        ));
    }

    #[test]
    fn a_false_check_fails() {
        let invocation = CheckInvocation {
            command: "false".to_string(),
            env: BTreeMap::new(),
            timeout: secs(5),
            workdir: None,
        };
        assert!(matches!(
            run_shell_check(&invocation),
            CheckResult::Failed { code: Some(1), .. }
        ));
    }

    #[test]
    fn a_hanging_check_times_out() {
        let invocation = CheckInvocation {
            command: "sleep 60".to_string(),
            env: BTreeMap::new(),
            timeout: secs(1),
            workdir: None,
        };
        assert!(matches!(
            run_shell_check(&invocation),
            CheckResult::TimedOut
        ));
    }

    #[test]
    fn a_check_reads_its_own_env_var() {
        let mut env = BTreeMap::new();
        env.insert("NOTTE_OUTPUT_FILE".to_string(), "/dev/null".to_string());
        let invocation = CheckInvocation {
            command: "test -n \"$NOTTE_OUTPUT_FILE\"".to_string(),
            env,
            timeout: secs(5),
            workdir: None,
        };
        assert!(matches!(
            run_shell_check(&invocation),
            CheckResult::Passed { .. }
        ));
    }

    /// **A CHECK RUNS WHERE IT IS TOLD TO**, and with no way to tell it, it ran
    /// where the process sits. A `cargo test` that passes because it ran in the
    /// wrong tree does not fail — it says yes, and that is the worst defect a
    /// check can have.
    #[test]
    fn a_check_runs_where_it_is_told() {
        let elsewhere =
            std::env::temp_dir().join(format!("sailor-verifica-altrove-{}", std::process::id()));
        std::fs::create_dir_all(&elsewhere).expect("cartella di prova");
        std::fs::write(elsewhere.join("il-testimone"), "x").expect("testimone");
        let invocation = CheckInvocation {
            command: "test -f il-testimone".to_string(),
            env: BTreeMap::new(),
            timeout: secs(5),
            workdir: Some(elsewhere.display().to_string()),
        };

        assert!(matches!(
            run_shell_check(&invocation),
            CheckResult::Passed { .. }
        ));

        let _ = std::fs::remove_dir_all(&elsewhere);
    }

    // ── the pause between two `try_wait` calls ───────────────────────

    /// Starts a child with the pipes attached and records every duration the
    /// loop asks to wait, actually sleeping it.
    ///
    /// **THIS IS THE INJECTION POINT, AND THE REASON THESE TWO TESTS DO NOT
    /// TIME ANYTHING.** The code decides the sequence of durations: it is the
    /// same on an idle machine and on one brought to its knees. Load can change
    /// only *how many* elements it has, and nothing is asserted about that.
    fn pauses_asked_while_running(script: &str) -> Vec<Duration> {
        let mut cmd = Command::new("sh");
        cmd.arg("-c").arg(script);
        cmd.stdout(Stdio::piped()).stderr(Stdio::piped());
        let mut child = cmd.spawn().expect("sh esiste");
        let mut asked = Vec::new();
        let outcome = drain_and_wait_paced(
            child.stdout.take(),
            child.stderr.take(),
            &mut child,
            secs(30),
            None,
            None,
            &mut |how_long| {
                asked.push(how_long);
                std::thread::sleep(how_long);
            },
        );
        assert!(
            matches!(outcome, RunOutcome::Finished { .. }),
            "il figlio doveva finire da solo: il tetto di tempo non c'entra"
        );
        asked
    }

    /// THE MEASURE THAT COULD COME OUT DIFFERENTLY: with the old code the first
    /// duration asked for was 50 ms, and a command finished in five
    /// milliseconds hung on for forty-five anyway.
    #[test]
    fn the_first_poll_pause_is_short_not_fifty_milliseconds() {
        let asked = pauses_asked_while_running("sleep 0.1");
        assert!(
            !asked.is_empty(),
            "il ciclo deve aver aspettato almeno una volta, o non prova niente"
        );
        assert!(
            asked[0] <= Duration::from_millis(2),
            "la prima pausa dev'essere dell'ordine del millisecondo, non di cinquanta: {:?}",
            asked[0]
        );
    }

    /// The other half: the growth stops. With no cap a ten-minute child would
    /// be reaped minutes after it died. Nothing is asserted about the *number*
    /// of wakeups — that depends on the child's real duration, that is on the
    /// machine's load. Only about the shape: it really climbs, then stops at
    /// the cap and stays there.
    ///
    /// **"NEVER DECREASING" WAS NOT ENOUGH, and that is the defect this test
    /// was born with**: the `[50, 50, 50…]` sequence of fixed polling never
    /// decreases, never exceeds the cap and ends at the cap — it passed
    /// all three earlier assertions. The climb *before* the cap has to be asked
    /// for, because that is exactly what the old defect lacks.
    ///
    /// The growth rule questioned alone, **without starting anything**: that is
    /// what the comment on `next_poll_pause` promises its reader. Alone it
    /// proves nothing about the loop — a fixed `sleep` put back inside the
    /// `loop` would leave this green. The test below ties the rule to the loop;
    /// this one pins the rule.
    #[test]
    fn the_growth_rule_doubles_and_saturates_without_running_anything() {
        let mut pause = FIRST_POLL_PAUSE;
        let mut seen = vec![pause];
        for _ in 0..16 {
            pause = next_poll_pause(pause);
            seen.push(pause);
        }
        assert_eq!(
            &seen[..4],
            &[
                Duration::from_millis(1),
                Duration::from_millis(2),
                Duration::from_millis(4),
                Duration::from_millis(8),
            ],
            "la crescita è un raddoppio: {seen:?}"
        );
        assert_eq!(
            seen.last().copied(),
            Some(MAX_POLL_PAUSE),
            "sedici raddoppi devono aver saturato sul tetto: {seen:?}"
        );
    }

    #[test]
    fn the_poll_pause_grows_up_to_the_cap_and_stays_there() {
        assert!(
            MAX_POLL_PAUSE <= Duration::from_millis(50),
            "il tetto non può superare i 50 ms che il ciclo già pagava"
        );
        let asked = pauses_asked_while_running("sleep 0.6");
        let reached = asked
            .iter()
            .position(|&one| one == MAX_POLL_PAUSE)
            .unwrap_or_else(|| {
                panic!("mezzo secondo deve bastare per arrivare al tetto: {asked:?}")
            });
        let (climbing, at_cap) = asked.split_at(reached);
        assert!(
            !climbing.is_empty(),
            "la prima pausa è già il tetto: qui non cresce niente, è l'attesa fissa di prima — {asked:?}"
        );
        assert!(
            climbing.windows(2).all(|pair| pair[0] < pair[1]),
            "finché non tocca il tetto ogni pausa dev'essere più lunga della precedente: {asked:?}"
        );
        assert!(
            at_cap.iter().all(|&one| one == MAX_POLL_PAUSE),
            "arrivata al tetto la pausa non deve più muoversi: {asked:?}"
        );
    }
}
