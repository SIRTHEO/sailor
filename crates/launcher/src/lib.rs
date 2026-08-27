//! Esecuzione sincrona di passi `flow` in gruppi di processi Unix.

use flow::{
    Action, ActionError, ActionOutcome, ActionRegistry, AttemptRelation, Clock, Completion,
    Decision,
    EffectStatus, Execution, ExecutionRequest, Executor, FlowError, Graph, InProcessExecutor,
    Outcome, ProcessProbe, RecordStore, SharedState, Step, StepRecord, MAX_SAID_BYTES,
};
use serde_json::Value;
use std::collections::BTreeMap;
use std::ffi::OsString;
use std::fs::{self, File, OpenOptions};
use std::io::{self, Read, Write};
use std::os::fd::{AsRawFd, RawFd};
use std::os::unix::net::UnixStream;
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, ExitStatus, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

const POLL_INTERVAL: Duration = Duration::from_millis(10);
const TERMINATION_GRACE: Duration = Duration::from_millis(100);
const SAID_HEADER_RESERVE: usize = 160;
const MAX_STRUCTURED_BYTES: usize = 16 * 1024 * 1024;
const OUTPUT_FD_ENV: &str = "SAILOR_OUTPUT_FD";

pub trait EffectInspector: Send + Sync {
    fn inspect(
        &self,
        record: &StepRecord,
        shared: &SharedState,
    ) -> Result<EffectStatus, ActionError>;
}

#[derive(Clone)]
pub struct CommandSpec {
    pub executable: PathBuf,
    pub arguments: Vec<OsString>,
    pub working_directory: PathBuf,
    pub environment: BTreeMap<OsString, OsString>,
    pub timeout: Duration,
    pub max_output_bytes: usize,
    inspector: Option<Arc<dyn EffectInspector>>,
}

impl CommandSpec {
    pub fn new(
        executable: impl Into<PathBuf>,
        working_directory: impl Into<PathBuf>,
        timeout: Duration,
        max_output_bytes: usize,
    ) -> Self {
        Self {
            executable: executable.into(),
            arguments: Vec::new(),
            working_directory: working_directory.into(),
            environment: BTreeMap::new(),
            timeout,
            max_output_bytes,
            inspector: None,
        }
    }

    pub fn with_inspector(mut self, inspector: impl EffectInspector + 'static) -> Self {
        self.inspector = Some(Arc::new(inspector));
        self
    }
}

#[derive(Clone, Default)]
pub struct ShutdownHandle {
    requested: Arc<AtomicBool>,
}

impl ShutdownHandle {
    pub fn request(&self) {
        self.requested.store(true, Ordering::SeqCst);
    }

    pub fn is_requested(&self) -> bool {
        self.requested.load(Ordering::SeqCst)
    }
}

pub struct ProcessExecutor {
    lock_directory: PathBuf,
    commands: BTreeMap<String, CommandSpec>,
    shutdown: ShutdownHandle,
}

impl ProcessExecutor {
    pub fn new(lock_directory: impl Into<PathBuf>) -> Self {
        Self {
            lock_directory: lock_directory.into(),
            commands: BTreeMap::new(),
            shutdown: ShutdownHandle::default(),
        }
    }

    pub fn register(
        &mut self,
        name: impl Into<String>,
        command: CommandSpec,
    ) -> Option<CommandSpec> {
        self.commands.insert(name.into(), command)
    }

    pub fn shutdown_handle(&self) -> ShutdownHandle {
        self.shutdown.clone()
    }

    /// Registra soltanto l'ispezione: l'avvio resta responsabilità di questo esecutore.
    pub fn register_effect_inspectors(&self, actions: &mut ActionRegistry) {
        for (name, command) in &self.commands {
            actions.register(
                name,
                InspectionAction {
                    inspector: command.inspector.clone(),
                },
            );
        }
    }

    fn input_for(
        step: &Step,
        root_inputs: &BTreeMap<String, Value>,
        records: &[StepRecord],
    ) -> Result<Value, FlowError> {
        match step.deps.as_slice() {
            [] => Ok(root_inputs.get(&step.id).cloned().unwrap_or(Value::Null)),
            [only] => successful_output(only, records),
            many => {
                let mut values = serde_json::Map::new();
                for dependency in many {
                    values.insert(dependency.clone(), successful_output(dependency, records)?);
                }
                Ok(Value::Object(values))
            }
        }
    }

    fn run(&self, command: &CommandSpec, record: &StepRecord, input: &Value) -> ProcessResult {
        match run_command(
            command,
            &lock_path(&self.lock_directory, record),
            input,
            &self.shutdown,
        ) {
            Ok(result) => result,
            Err(error) => ProcessResult {
                outcome: Outcome::Broke,
                output: None,
                said: Some(format_said(0, &[], command.max_output_bytes)),
                failure_class: Some(error.class),
                detail: Some(error.said),
            },
        }
    }
}

impl Executor for ProcessExecutor {
    fn execute(
        &self,
        graph: &Graph,
        request: ExecutionRequest,
        store: &mut dyn RecordStore,
        _actions: &ActionRegistry,
        clock: &mut dyn Clock,
    ) -> Result<Execution, FlowError> {
        let mut decisions = Vec::new();
        loop {
            let records = store.records(&request.run_id)?;
            let decision = InProcessExecutor.decision(graph, &request.run_id, store)?;
            decisions.push(decision.clone());
            let Decision::Ready(front) = decision else {
                return Ok(Execution {
                    decisions,
                    shared: request.shared,
                });
            };

            for step_id in front {
                let step = graph
                    .step(&step_id)
                    .ok_or_else(|| FlowError::UnknownStep(step_id.clone()))?;
                let input = Self::input_for(step, &request.root_inputs, &records)?;
                step.input_schema.validate(&input)?;
                let should_run = step
                    .when
                    .as_ref()
                    .is_none_or(|condition| condition.matches(&input));
                let command = should_run
                    .then(|| self.commands.get(&step.action))
                    .flatten()
                    .ok_or_else(|| FlowError::UnknownAction(step.action.clone()));
                let previous = latest_for(step, &records);
                let attempt = previous.map_or(1, |record| record.attempt + 1);
                let epoch = records.iter().map(|record| record.epoch).max().unwrap_or(0) + 1;
                let mut started = StepRecord::started(
                    &request.run_id,
                    &step.id,
                    attempt,
                    epoch,
                    step.deps.clone(),
                    input.clone(),
                    request.gates.clone(),
                    clock.now()?,
                );
                started.attempt_relation = relation(previous, &records, &started);
                store.append_started(started.clone())?;

                let completion = if !should_run {
                    Completion {
                        outcome: Outcome::Skipped,
                        output: None,
                        said: None,
                        failure_class: None,
                        ended_at: clock.now()?,
                    }
                } else {
                    let result = match command {
                        Ok(command) => self.run(command, &started, &input),
                        Err(error) => {
                            store.close(
                                &request.run_id,
                                &step.id,
                                attempt,
                                epoch,
                                Completion {
                                    outcome: Outcome::Broke,
                                    output: None,
                                    said: Some(error.to_string()),
                                    failure_class: Some("unknown_action".to_owned()),
                                    ended_at: clock.now()?,
                                },
                            )?;
                            continue;
                        }
                    };
                    let mut said = result.said;
                    if let Some(detail) = result.detail {
                        append_detail(&mut said, &detail);
                    }
                    let (outcome, output, failure_class) = match result.output {
                        Some(output) if result.outcome == Outcome::Went => {
                            match step.output_schema.validate(&output) {
                                Ok(()) => (Outcome::Went, Some(output), None),
                                Err(error) => {
                                    append_detail(&mut said, &error.to_string());
                                    (Outcome::Broke, None, Some("invalid_output".to_owned()))
                                }
                            }
                        }
                        output => (result.outcome, output, result.failure_class),
                    };
                    Completion {
                        outcome,
                        output,
                        said,
                        failure_class,
                        ended_at: clock.now()?,
                    }
                };
                store.close(&request.run_id, &step.id, attempt, epoch, completion)?;
            }
        }
    }
}

struct InspectionAction {
    inspector: Option<Arc<dyn EffectInspector>>,
}

impl Action for InspectionAction {
    fn execute(
        &self,
        _input: &Value,
        _shared: &mut SharedState,
    ) -> Result<ActionOutcome, ActionError> {
        Err(ActionError::new(
            "wrong_executor",
            "external commands must run through ProcessExecutor",
        ))
    }

    fn inspect_effect(
        &self,
        record: &StepRecord,
        shared: &SharedState,
    ) -> Result<EffectStatus, ActionError> {
        self.inspector.as_ref().map_or_else(
            || Ok(EffectStatus::Unknown("effect_not_inspectable".to_owned())),
            |inspector| inspector.inspect(record, shared),
        )
    }
}

pub struct FileLockProbe {
    lock_directory: PathBuf,
}

impl FileLockProbe {
    pub fn new(lock_directory: impl Into<PathBuf>) -> Self {
        Self {
            lock_directory: lock_directory.into(),
        }
    }

    pub fn path_for(&self, record: &StepRecord) -> PathBuf {
        lock_path(&self.lock_directory, record)
    }
}

impl ProcessProbe for FileLockProbe {
    fn is_running(&self, record: &StepRecord) -> Result<bool, FlowError> {
        let path = self.path_for(record);
        let file = match File::open(&path) {
            Ok(file) => file,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(false),
            Err(error) => return Err(FlowError::Store(error.to_string())),
        };
        match try_lock(&file) {
            Ok(true) => {
                unlock(&file).map_err(|error| FlowError::Store(error.to_string()))?;
                Ok(false)
            }
            Ok(false) => Ok(true),
            Err(error) => Err(FlowError::Store(error.to_string())),
        }
    }
}

/// Tiene il lucchetto fino alla morte del processo o alla distruzione del valore.
pub struct ProcessLock(File);

pub fn hold_process_lock(path: &Path) -> io::Result<ProcessLock> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let file = OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .open(path)?;
    lock(&file)?;
    Ok(ProcessLock(file))
}

fn acquire_process_lock(
    path: &Path,
    started: Instant,
    timeout: Duration,
    shutdown: &ShutdownHandle,
) -> io::Result<Result<ProcessLock, StopReason>> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let file = OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .open(path)?;
    loop {
        if try_lock(&file)? {
            return Ok(Ok(ProcessLock(file)));
        }
        if shutdown.is_requested() {
            return Ok(Err(StopReason {
                outcome: Outcome::Stopped,
                class: "shutdown_requested",
            }));
        }
        if started.elapsed() >= timeout {
            return Ok(Err(StopReason {
                outcome: Outcome::Broke,
                class: "timeout",
            }));
        }
        thread::sleep(POLL_INTERVAL);
    }
}

struct ProcessResult {
    outcome: Outcome,
    output: Option<Value>,
    said: Option<String>,
    failure_class: Option<String>,
    detail: Option<String>,
}

struct LaunchError {
    class: String,
    said: String,
}

fn run_command(
    spec: &CommandSpec,
    lock_path: &Path,
    input: &Value,
    shutdown: &ShutdownHandle,
) -> Result<ProcessResult, LaunchError> {
    validate_spec(spec)?;
    let started = Instant::now();
    let lock = match acquire_process_lock(lock_path, started, spec.timeout, shutdown)
        .map_err(|error| launch_error("lock_failed", error))?
    {
        Ok(lock) => lock,
        Err(reason) => {
            return Ok(ProcessResult {
                outcome: reason.outcome,
                output: None,
                said: Some(format_said(0, &[], spec.max_output_bytes)),
                failure_class: Some(reason.class.to_owned()),
                detail: None,
            });
        }
    };
    let (data_parent, data_child) =
        UnixStream::pair().map_err(|error| launch_error("structured_channel_failed", error))?;
    let data_fd = data_child.as_raw_fd();
    let lock_fd = lock.0.as_raw_fd();
    let mut command = Command::new(&spec.executable);
    command
        .args(&spec.arguments)
        .current_dir(&spec.working_directory)
        .env_clear()
        .envs(&spec.environment)
        .env(OUTPUT_FD_ENV, data_fd.to_string())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    // Il figlio diventa capo di un gruppo nuovo e conserva i due descrittori
    // oltre exec: uno prova la vita, l'altro è il solo canale dati tipato.
    unsafe {
        command.pre_exec(move || {
            if libc::setpgid(0, 0) == -1 {
                return Err(io::Error::last_os_error());
            }
            clear_close_on_exec(lock_fd)?;
            clear_close_on_exec(data_fd)?;
            Ok(())
        });
    }
    let mut child = command
        .spawn()
        .map_err(|error| launch_error("spawn_failed", error))?;
    drop(data_child);
    drop(lock);

    let stdin_bytes = serde_json::to_vec(input).map_err(|error| LaunchError {
        class: "input_encoding_failed".to_owned(),
        said: error.to_string(),
    })?;
    let mut stdin = child.stdin.take().expect("stdin configurato come pipe");
    let stdin_thread = thread::spawn(move || {
        let _ = stdin.write_all(&stdin_bytes);
    });
    let capture = Arc::new(Mutex::new(Capture::new(spec.max_output_bytes)));
    let stdout_thread = drain_output(
        child.stdout.take().expect("stdout configurato come pipe"),
        Arc::clone(&capture),
    );
    let stderr_thread = drain_output(
        child.stderr.take().expect("stderr configurato come pipe"),
        Arc::clone(&capture),
    );
    let data_thread = thread::spawn(move || read_structured(data_parent));

    let (status, stop_reason) = wait_for_child(
        &mut child,
        lock_path,
        started,
        spec.timeout,
        shutdown,
        &SystemGroupSignaler,
    )
    .map_err(|error| launch_error("wait_failed", error))?;
    let _ = stdin_thread.join();
    let _ = stdout_thread.join();
    let _ = stderr_thread.join();
    let structured = data_thread
        .join()
        .map_err(|_| LaunchError {
            class: "structured_reader_panicked".to_owned(),
            said: "structured output reader panicked".to_owned(),
        })?
        .map_err(|error| launch_error("structured_output_failed", error));
    let captured = capture
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let said = Some(format_said(
        captured.seen,
        &captured.kept,
        spec.max_output_bytes,
    ));

    if let Some(reason) = stop_reason {
        return Ok(ProcessResult {
            outcome: reason.outcome,
            output: None,
            said,
            failure_class: Some(reason.class.to_owned()),
            detail: None,
        });
    }
    if !status.success() {
        return Ok(ProcessResult {
            outcome: Outcome::Broke,
            output: None,
            said,
            failure_class: Some("exit_status".to_owned()),
            detail: Some(format_exit_status(status)),
        });
    }
    let bytes = structured?;
    let output = serde_json::from_slice(&bytes).map_err(|error| LaunchError {
        class: "invalid_structured_output".to_owned(),
        said: error.to_string(),
    })?;
    Ok(ProcessResult {
        outcome: Outcome::Went,
        output: Some(output),
        said,
        failure_class: None,
        detail: None,
    })
}

fn validate_spec(spec: &CommandSpec) -> Result<(), LaunchError> {
    if !spec.executable.is_absolute() {
        return Err(LaunchError {
            class: "invalid_executable".to_owned(),
            said: "executable must be an absolute path".to_owned(),
        });
    }
    if !spec.working_directory.is_absolute() {
        return Err(LaunchError {
            class: "invalid_working_directory".to_owned(),
            said: "working directory must be an absolute path".to_owned(),
        });
    }
    if spec.max_output_bytes > MAX_SAID_BYTES - SAID_HEADER_RESERVE {
        return Err(LaunchError {
            class: "invalid_output_limit".to_owned(),
            said: format!(
                "max output bytes must not exceed {}",
                MAX_SAID_BYTES - SAID_HEADER_RESERVE
            ),
        });
    }
    Ok(())
}

struct StopReason {
    outcome: Outcome,
    class: &'static str,
}

fn wait_for_child(
    child: &mut Child,
    lock_path: &Path,
    started: Instant,
    timeout: Duration,
    shutdown: &ShutdownHandle,
    signaler: &dyn GroupSignaler,
) -> io::Result<(ExitStatus, Option<StopReason>)> {
    loop {
        if child_has_exited(child)? {
            if descendants_hold_lock(lock_path)? {
                terminate_remaining_group(child.id() as i32, signaler)?;
            }
            return child.wait().map(|status| (status, None));
        }
        let reason = if shutdown.is_requested() {
            Some(StopReason {
                outcome: Outcome::Stopped,
                class: "shutdown_requested",
            })
        } else if started.elapsed() >= timeout {
            Some(StopReason {
                outcome: Outcome::Broke,
                class: "timeout",
            })
        } else {
            None
        };
        if let Some(reason) = reason {
            signaler.send(child.id() as i32, libc::SIGTERM)?;
            let deadline = Instant::now() + TERMINATION_GRACE;
            while Instant::now() < deadline {
                if child_has_exited(child)? {
                    break;
                }
                thread::sleep(POLL_INTERVAL);
            }
            signaler.send(child.id() as i32, libc::SIGKILL)?;
            return child.wait().map(|status| (status, Some(reason)));
        }
        thread::sleep(POLL_INTERVAL);
    }
}

fn child_has_exited(child: &Child) -> io::Result<bool> {
    let mut info = std::mem::MaybeUninit::<libc::siginfo_t>::zeroed();
    let result = unsafe {
        libc::waitid(
            libc::P_PID,
            child.id(),
            info.as_mut_ptr(),
            libc::WEXITED | libc::WNOHANG | libc::WNOWAIT,
        )
    };
    if result == -1 {
        return Err(io::Error::last_os_error());
    }
    let info = unsafe { info.assume_init() };
    Ok(unsafe { info.si_pid() } != 0)
}

fn descendants_hold_lock(path: &Path) -> io::Result<bool> {
    let file = File::open(path)?;
    if try_lock(&file)? {
        unlock(&file)?;
        Ok(false)
    } else {
        Ok(true)
    }
}

fn terminate_remaining_group(pgid: i32, signaler: &dyn GroupSignaler) -> io::Result<()> {
    signaler.send(pgid, libc::SIGTERM)?;
    thread::sleep(TERMINATION_GRACE);
    signaler.send(pgid, libc::SIGKILL)
}

trait GroupSignaler {
    fn send(&self, pgid: i32, signal: i32) -> io::Result<()>;
}

struct SystemGroupSignaler;

impl GroupSignaler for SystemGroupSignaler {
    fn send(&self, pgid: i32, signal: i32) -> io::Result<()> {
        let result = unsafe { libc::kill(-pgid, signal) };
        if result == 0 {
            return Ok(());
        }
        let error = io::Error::last_os_error();
        if error.raw_os_error() == Some(libc::ESRCH) {
            Ok(())
        } else {
            Err(error)
        }
    }
}

struct Capture {
    seen: usize,
    kept: Vec<u8>,
    limit: usize,
}

impl Capture {
    fn new(limit: usize) -> Self {
        Self {
            seen: 0,
            kept: Vec::with_capacity(limit),
            limit,
        }
    }

    fn add(&mut self, bytes: &[u8]) {
        self.seen = self.seen.saturating_add(bytes.len());
        let available = self.limit.saturating_sub(self.kept.len());
        self.kept
            .extend_from_slice(&bytes[..available.min(bytes.len())]);
    }
}

fn drain_output(
    mut reader: impl Read + Send + 'static,
    capture: Arc<Mutex<Capture>>,
) -> thread::JoinHandle<()> {
    thread::spawn(move || {
        let mut buffer = [0_u8; 8192];
        loop {
            match reader.read(&mut buffer) {
                Ok(0) | Err(_) => return,
                Ok(count) => capture
                    .lock()
                    .unwrap_or_else(|poisoned| poisoned.into_inner())
                    .add(&buffer[..count]),
            }
        }
    })
}

fn read_structured(mut stream: UnixStream) -> io::Result<Vec<u8>> {
    let mut bytes = Vec::new();
    let mut buffer = [0_u8; 8192];
    loop {
        let count = stream.read(&mut buffer)?;
        if count == 0 {
            return Ok(bytes);
        }
        if bytes.len().saturating_add(count) > MAX_STRUCTURED_BYTES {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "structured output exceeds 16 MiB",
            ));
        }
        bytes.extend_from_slice(&buffer[..count]);
    }
}

fn format_said(seen: usize, kept: &[u8], configured_limit: usize) -> String {
    let raw_discarded = seen.saturating_sub(kept.len());
    let rendered = String::from_utf8_lossy(kept);
    let mut rendered_end = rendered.len().min(configured_limit);
    while !rendered.is_char_boundary(rendered_end) {
        rendered_end -= 1;
    }
    let truncated = raw_discarded > 0 || rendered_end < rendered.len();
    format!(
        "[launcher-output bytes_seen={seen} bytes_kept={rendered_end} bytes_discarded={raw_discarded} configured_limit={configured_limit} truncated={}]\n{}",
        truncated,
        &rendered[..rendered_end]
    )
}

fn append_detail(said: &mut Option<String>, detail: &str) {
    let value = said.get_or_insert_with(String::new);
    let remaining = MAX_SAID_BYTES.saturating_sub(value.len());
    if remaining == 0 {
        return;
    }
    let addition = format!("\n[launcher-detail] {detail}");
    let mut end = addition.len().min(remaining);
    while !addition.is_char_boundary(end) {
        end -= 1;
    }
    value.push_str(&addition[..end]);
}

fn format_exit_status(status: ExitStatus) -> String {
    status.code().map_or_else(
        || "process ended by signal".to_owned(),
        |code| format!("exit code {code}"),
    )
}

fn latest_for<'a>(step: &Step, records: &'a [StepRecord]) -> Option<&'a StepRecord> {
    records
        .iter()
        .filter(|record| record.step_id == step.id)
        .max_by_key(|record| (record.attempt, record.epoch))
}

fn successful_output(step_id: &str, records: &[StepRecord]) -> Result<Value, FlowError> {
    records
        .iter()
        .filter(|record| record.step_id == step_id && record.outcome == Some(Outcome::Went))
        .max_by_key(|record| (record.attempt, record.epoch))
        .and_then(|record| record.output.clone())
        .ok_or_else(|| FlowError::MissingOutput(step_id.to_owned()))
}

fn relation(
    previous: Option<&StepRecord>,
    records: &[StepRecord],
    started: &StepRecord,
) -> Option<AttemptRelation> {
    previous.map(|previous| {
        if previous.input_digest != started.input_digest {
            AttemptRelation::DifferentInput
        } else {
            let origin = records
                .iter()
                .filter(|record| {
                    record.step_id == started.step_id && record.input_digest == started.input_digest
                })
                .min_by_key(|record| (record.attempt, record.epoch))
                .unwrap_or(previous);
            if normalized_gates(&origin.gates) == normalized_gates(&started.gates) {
                AttemptRelation::SameInput
            } else {
                AttemptRelation::SameInputGatesChanged
            }
        }
    })
}

fn normalized_gates(gates: &[String]) -> Vec<&str> {
    let mut values: Vec<_> = gates.iter().map(String::as_str).collect();
    values.sort_unstable();
    values.dedup();
    values
}

fn lock_path(directory: &Path, record: &StepRecord) -> PathBuf {
    directory.join(format!(
        "{}--{}--{}--{}.lock",
        hex(record.run_id.as_bytes()),
        hex(record.step_id.as_bytes()),
        record.attempt,
        record.epoch
    ))
}

fn hex(bytes: &[u8]) -> String {
    const DIGITS: &[u8; 16] = b"0123456789abcdef";
    let mut encoded = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        encoded.push(DIGITS[(byte >> 4) as usize] as char);
        encoded.push(DIGITS[(byte & 0x0f) as usize] as char);
    }
    encoded
}

fn lock(file: &File) -> io::Result<()> {
    let result = unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX) };
    if result == 0 {
        Ok(())
    } else {
        Err(io::Error::last_os_error())
    }
}

fn try_lock(file: &File) -> io::Result<bool> {
    let result = unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) };
    if result == 0 {
        return Ok(true);
    }
    let error = io::Error::last_os_error();
    if error.raw_os_error() == Some(libc::EWOULDBLOCK) {
        Ok(false)
    } else {
        Err(error)
    }
}

fn unlock(file: &File) -> io::Result<()> {
    let result = unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_UN) };
    if result == 0 {
        Ok(())
    } else {
        Err(io::Error::last_os_error())
    }
}

fn clear_close_on_exec(fd: RawFd) -> io::Result<()> {
    let flags = unsafe { libc::fcntl(fd, libc::F_GETFD) };
    if flags == -1 {
        return Err(io::Error::last_os_error());
    }
    if unsafe { libc::fcntl(fd, libc::F_SETFD, flags & !libc::FD_CLOEXEC) } == -1 {
        Err(io::Error::last_os_error())
    } else {
        Ok(())
    }
}

fn launch_error(class: &str, error: impl std::fmt::Display) -> LaunchError {
    LaunchError {
        class: class.to_owned(),
        said: error.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;
    use std::time::{SystemTime, UNIX_EPOCH};

    #[derive(Default)]
    struct RecordingSignaler {
        signals: Mutex<Vec<(i32, i32)>>,
    }

    impl GroupSignaler for RecordingSignaler {
        fn send(&self, pgid: i32, signal: i32) -> io::Result<()> {
            self.signals
                .lock()
                .expect("registro dei segnali accessibile")
                .push((pgid, signal));
            Ok(())
        }
    }

    #[test]
    fn fast_command_without_descendants_sends_no_group_signal() {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("orologio dopo epoch")
            .as_nanos();
        let directory = std::env::temp_dir().join(format!(
            "launcher-signal-test-{}-{nonce}",
            std::process::id()
        ));
        fs::create_dir(&directory).expect("cartella temporanea creata");
        let lock_path = directory.join("process.lock");
        let lock = hold_process_lock(&lock_path).expect("lock del comando preso");
        let lock_fd = lock.0.as_raw_fd();
        let mut command = Command::new("/usr/bin/true");
        unsafe {
            command.pre_exec(move || {
                if libc::setpgid(0, 0) == -1 {
                    return Err(io::Error::last_os_error());
                }
                clear_close_on_exec(lock_fd)
            });
        }
        let mut child = command.spawn().expect("comando veloce avviato");
        drop(lock);
        let signaler = RecordingSignaler::default();
        let (status, reason) = wait_for_child(
            &mut child,
            &lock_path,
            Instant::now(),
            Duration::from_secs(1),
            &ShutdownHandle::default(),
            &signaler,
        )
        .expect("comando veloce raccolto");
        assert!(status.success());
        assert!(reason.is_none());
        assert_eq!(
            signaler
                .signals
                .lock()
                .expect("registro dei segnali leggibile")
                .as_slice(),
            &[]
        );
        fs::remove_file(lock_path).expect("lock temporaneo rimosso");
        fs::remove_dir(directory).expect("cartella temporanea rimossa");
    }
}
