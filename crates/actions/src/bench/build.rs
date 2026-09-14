//! The two steps that build the frozen benchmark: one cuts a task from a fix
//! commit and proves its hidden test red on the base and green on the fix, the
//! other reads a whole set back and gives it a hash a run can be pinned to.
//! A candidate that makes no task is an answer with `kept: false`, not a
//! failure, so the flow running this once per candidate reaches the last one;
//! only a broken environment — no git, no cargo, no repository — is red.

use super::candidates::{fault_number, files_of, git, not_a_repository, repo_of, split_files, DEFAULT_SET};
use super::task::{added_lines, Task, Validation};
use crate::{run_with_timeout, RunOutcome};
use flow::{Action, ActionError, ActionOutcome, RedoEvidence, SharedState, StepRecord, StepSpecies};
use serde::Deserialize;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

/// Cuts one task and validates it.
pub const BENCH_VALIDATE_ACTION: &str = "bench_validate";
/// Reads a set back and hashes it.
pub const BENCH_FREEZE_ACTION: &str = "bench_freeze";

pub const DEFAULT_RUNS: u32 = 3;
pub const DEFAULT_TIMEOUT_SECS: u64 = 1800;
/// Inside `target/`, so `cargo clean` takes it back with everything else.
pub const BUILD_DIR: &str = "target/bench";
/// How many build jobs a validation run may take from a shared machine.
const BUILD_JOBS: &str = "2";

/// What a test says when it failed for the sandbox it ran in and not for the
/// code: a task whose red carries one of these cannot be judged anywhere.
pub const SANDBOX_SHAPES: &[&str] = &[
    "Operation not permitted",
    "openpty",
    "Permission denied",
    "unable to open database",
    "Cannot get process list",
    "Resource temporarily unavailable",
];

const VALIDATE_FIELDS: &[&str] = &[
    "repo",
    "fix_commit",
    "base_commit",
    "set",
    "home",
    "runs",
    "target_dir",
    "timeout_secs",
];
const FREEZE_FIELDS: &[&str] = &["home", "set"];

#[derive(Debug, Deserialize)]
struct ValidateInput {
    #[serde(default)]
    repo: Option<String>,
    fix_commit: String,
    base_commit: String,
    #[serde(default)]
    set: Option<String>,
    #[serde(default)]
    home: Option<String>,
    #[serde(default)]
    runs: Option<u32>,
    #[serde(default)]
    target_dir: Option<String>,
    #[serde(default)]
    timeout_secs: Option<u64>,
}

/// A task, or why the candidate is not one.
#[derive(Debug)]
pub enum Verdict {
    Kept(Task),
    Rejected(String),
}

fn environment(class: &str, said: impl Into<String>) -> ActionError {
    ActionError::new(class, said)
}

fn now_secs() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|elapsed| elapsed.as_secs() as i64)
        .unwrap_or_default()
}

// ── cutting the task ─────────────────────────────────────────────────────

/// The body of a commit message with its trailers taken off the end.
pub fn body_without_trailers(body: &str) -> String {
    let mut lines: Vec<&str> = body.trim().lines().collect();
    while let Some(last) = lines.last() {
        if last.trim().is_empty() || is_trailer(last) {
            lines.pop();
        } else {
            break;
        }
    }
    lines.join("\n").trim().to_owned()
}

fn is_trailer(line: &str) -> bool {
    let Some((token, rest)) = line.split_once(": ") else {
        return false;
    };
    !token.is_empty()
        && !rest.trim().is_empty()
        && token
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-')
}

/// A diff that only moves literals: the seed a ratchet carries — a constant,
/// a row of a table, a bare string or number — re-measured by a fix that did
/// its work elsewhere. It proves nothing about the fix.
pub fn seed_bump_only(diff: &str) -> bool {
    let changed: Vec<&str> = diff
        .lines()
        .filter(|line| {
            (line.starts_with('+') || line.starts_with('-'))
                && !line.starts_with("+++")
                && !line.starts_with("---")
        })
        .collect();
    !changed.is_empty() && changed.iter().all(|line| is_a_seed_line(line[1..].trim()))
}

fn is_a_seed_line(body: &str) -> bool {
    body.starts_with("const ")
        || body.starts_with("pub const ")
        || body.starts_with("(\"")
        || body.starts_with('"')
        || body
            .chars()
            .all(|c| c.is_ascii_digit() || matches!(c, ',' | '(' | ')' | '[' | ']' | '_' | ' '))
}

/// The tests a diff adds or changes: a `fn` added under a `#[test]`, and the
/// function a hunk header names as the one it changes inside.
pub fn test_names_in(diff: &str) -> Vec<String> {
    let mut names = Vec::new();
    let mut under_a_test_attribute = false;
    for line in diff.lines() {
        if let Some(header) = line.strip_prefix("@@") {
            if let Some(name) = header.rsplit_once("@@").and_then(|(_, context)| fn_name(context)) {
                push_once(&mut names, name);
            }
            under_a_test_attribute = false;
            continue;
        }
        if !line.starts_with('+') || line.starts_with("+++") {
            continue;
        }
        let body = line[1..].trim_start();
        if body.starts_with("#[test]") || body.starts_with("#[tokio::test") {
            under_a_test_attribute = true;
            continue;
        }
        if let Some(name) = fn_name(body) {
            if under_a_test_attribute {
                push_once(&mut names, name);
            }
            under_a_test_attribute = false;
            continue;
        }
        if !body.starts_with("#[") && !body.starts_with("//") && !body.starts_with("async ") {
            under_a_test_attribute = false;
        }
    }
    names
}

fn fn_name(text: &str) -> Option<String> {
    let after = text.trim_start();
    let after = after.strip_prefix("pub ").unwrap_or(after);
    let after = after.strip_prefix("async ").unwrap_or(after);
    let after = after.strip_prefix("fn ")?;
    let name: String = after
        .chars()
        .take_while(|c| c.is_ascii_alphanumeric() || *c == '_')
        .collect();
    (!name.is_empty()).then_some(name)
}

fn push_once(names: &mut Vec<String>, name: String) {
    if !names.contains(&name) {
        names.push(name);
    }
}

/// The package a file belongs to at `commit`: the nearest manifest above it
/// that declares one, and how far under it the file sits.
fn package_of(repo: &Path, commit: &str, path: &str) -> Option<(String, String)> {
    let mut parts: Vec<&str> = path.split('/').collect();
    parts.pop()?;
    while !parts.is_empty() {
        let dir = parts.join("/");
        let manifest = format!("{commit}:{dir}/Cargo.toml");
        if let Ok(text) = git(repo, &["show", &manifest]) {
            let name = package_name(&text)?;
            let inside = path.strip_prefix(&format!("{dir}/")).unwrap_or(path).to_owned();
            return Some((name, inside));
        }
        parts.pop();
    }
    let text = git(repo, &["show", &format!("{commit}:Cargo.toml")]).ok()?;
    Some((package_name(&text)?, path.to_owned()))
}

/// The `name` under `[package]`, none for a manifest that is only a workspace.
pub fn package_name(manifest: &str) -> Option<String> {
    let mut in_package = false;
    for line in manifest.lines() {
        let line = line.trim();
        if line.starts_with('[') {
            in_package = line == "[package]";
            continue;
        }
        if in_package {
            if let Some(rest) = line.strip_prefix("name") {
                let value = rest.trim_start().strip_prefix('=')?.trim();
                return Some(value.trim_matches('"').to_owned());
            }
        }
    }
    None
}

/// The command that runs the tests of one file, from the root of the tree.
pub fn test_command_for(package: &str, inside: &str, names: &[String]) -> Vec<String> {
    let mut command: Vec<String> = ["cargo", "test", "-p", package]
        .iter()
        .map(|word| (*word).to_owned())
        .collect();
    match inside.strip_prefix("tests/") {
        Some(rest) if !rest.contains('/') && rest.ends_with(".rs") => {
            command.push("--test".to_owned());
            command.push(rest.trim_end_matches(".rs").to_owned());
        }
        Some(_) => {}
        None => command.push("--lib".to_owned()),
    }
    if !names.is_empty() {
        command.push("--".to_owned());
        command.extend(names.iter().cloned());
    }
    command
}

/// The prompt and where it came from: the fault's own words when the commit
/// names one the register holds, else the commit's reason.
fn prompt_for(subject: &str, body: &str, register: &Path) -> Option<(String, String)> {
    if let Some(number) = fault_number(subject).or_else(|| fault_number(body)) {
        if register.is_file() {
            if let Ok(fault) = faults::Faults::open_for_reading(register).and_then(|faults| faults.get(number)) {
                return Some((
                    format!("{}\n\n{}", fault.what_happened, fault.how_it_showed),
                    format!("fault {number}"),
                ));
            }
        }
    }
    let body = body_without_trailers(body);
    (!body.is_empty()).then(|| (body, "commit body".to_owned()))
}

/// Everything a task is, from git alone: no tree is cut and nothing runs.
pub(crate) fn cut_task(repo: &Path, fix: &str, base: &str, register: &Path) -> Result<Verdict, ActionError> {
    let read = |said: String| not_a_repository(repo, &said);
    let subject = git(repo, &["log", "-1", "--format=%s", fix]).map_err(read)?.trim().to_owned();
    let body = git(repo, &["log", "-1", "--format=%b", fix]).map_err(read)?;
    let files = files_of(repo, fix).map_err(read)?;
    let (all_test_files, _) = split_files(&files);
    if all_test_files.is_empty() {
        return Ok(Verdict::Rejected("no test file".to_owned()));
    }

    let mut test_files = Vec::new();
    let mut hidden_test_patch = String::new();
    let mut commands = Vec::new();
    for file in &all_test_files {
        let diff = git(repo, &["diff", base, fix, "--", file]).map_err(read)?;
        if diff.trim().is_empty() || seed_bump_only(&diff) {
            continue;
        }
        if let Some((package, inside)) = package_of(repo, fix, file) {
            commands.push(test_command_for(&package, &inside, &test_names_in(&diff)));
        }
        hidden_test_patch.push_str(&diff);
        test_files.push(file.clone());
    }
    if test_files.is_empty() {
        return Ok(Verdict::Rejected("no test beyond a seed".to_owned()));
    }
    if commands.is_empty() {
        return Ok(Verdict::Rejected("no test command".to_owned()));
    }

    let mut gold_args: Vec<String> = vec!["diff".into(), base.into(), fix.into(), "--".into(), ".".into()];
    gold_args.extend(all_test_files.iter().map(|file| format!(":(exclude){file}")));
    let gold_args: Vec<&str> = gold_args.iter().map(String::as_str).collect();
    let gold_patch = git(repo, &gold_args).map_err(read)?;

    let Some((prompt, prompt_source)) = prompt_for(&subject, &body, register) else {
        return Ok(Verdict::Rejected("no prompt".to_owned()));
    };

    let test_command = commands[0].clone();
    let test_commands = if commands.len() > 1 { commands } else { Vec::new() };
    Ok(Verdict::Kept(Task {
        id: short(fix),
        repo: repo.display().to_string(),
        base_commit: base.to_owned(),
        fix_commit: fix.to_owned(),
        prompt,
        prompt_source,
        test_files,
        hidden_test_patch,
        gold_added_lines: added_lines(&gold_patch),
        gold_patch,
        test_command,
        test_commands,
        validated: None,
    }))
}

fn short(sha: &str) -> String {
    sha.chars().take(8).collect()
}

// ── running the hidden tests ─────────────────────────────────────────────

#[derive(Debug, PartialEq, Eq)]
enum RunEnd {
    Green,
    Red,
    Sandbox(String),
    TimedOut,
}

fn sandbox_shape_in(text: &str) -> Option<String> {
    SANDBOX_SHAPES
        .iter()
        .find(|shape| text.contains(**shape))
        .map(|shape| (*shape).to_owned())
}

/// Every command of the task once, in the tree, each one's words kept in a
/// file beside the build. Red when any fails; the shapes of a sandbox are
/// looked for only in a red, since a green that mentions one still passed.
fn run_once(
    tree: &Path,
    commands: &[Vec<String>],
    target_dir: &Path,
    timeout: Duration,
    logs: &Path,
    label: &str,
) -> Result<RunEnd, ActionError> {
    let mut end = RunEnd::Green;
    for (nth, words) in commands.iter().enumerate() {
        let Some((program, rest)) = words.split_first() else {
            continue;
        };
        let mut command = Command::new(program);
        for (at, word) in rest.iter().enumerate() {
            command.arg(word);
            if at == 0 && program == "cargo" && word == "test" {
                command.arg("-j").arg(BUILD_JOBS);
            }
        }
        command.current_dir(tree).env("CARGO_TARGET_DIR", target_dir);
        let outcome = run_with_timeout(command, timeout);
        let (status, text) = match outcome {
            RunOutcome::SpawnFailed(said) => {
                return Err(environment("test_runner_missing", format!("«{program}» could not be started: {said}")));
            }
            RunOutcome::TimedOut => {
                std::fs::write(logs.join(format!("{label}-{nth}.log")), "timed out\n").ok();
                return Ok(RunEnd::TimedOut);
            }
            RunOutcome::Finished { status, stdout, stderr } => {
                let mut text = String::from_utf8_lossy(&stdout).into_owned();
                text.push_str(&String::from_utf8_lossy(&stderr));
                (status, text)
            }
        };
        std::fs::write(logs.join(format!("{label}-{nth}.log")), &text).ok();
        if status.success() {
            continue;
        }
        if let Some(shape) = sandbox_shape_in(&text) {
            return Ok(RunEnd::Sandbox(shape));
        }
        end = RunEnd::Red;
    }
    Ok(end)
}

/// One validation at a time on a build directory: the runs share it, and a
/// run waiting on another's lock must not spend its own timeout waiting.
struct OneAtATime {
    _held: std::fs::File,
}

impl OneAtATime {
    fn take(target_dir: &Path) -> Result<Self, ActionError> {
        std::fs::create_dir_all(target_dir)
            .map_err(|error| environment("build_dir_unwritable", format!("{}: {error}", target_dir.display())))?;
        let file = std::fs::OpenOptions::new()
            .create(true)
            .truncate(false)
            .write(true)
            .open(target_dir.join("bench.lock"))
            .map_err(|error| environment("build_dir_unwritable", format!("{}: {error}", target_dir.display())))?;
        file.lock()
            .map_err(|error| environment("build_dir_unwritable", format!("the build lock: {error}")))?;
        Ok(Self { _held: file })
    }
}

struct Tree {
    repo: PathBuf,
    path: PathBuf,
}

impl Tree {
    fn cut(repo: &Path, trees: &Path, id: &str, base: &str) -> Result<Self, ActionError> {
        let path = trees.join(id);
        let tree = Self {
            repo: repo.to_path_buf(),
            path,
        };
        tree.remove();
        std::fs::create_dir_all(trees)
            .map_err(|error| environment("build_dir_unwritable", format!("{}: {error}", trees.display())))?;
        let path = tree.path.display().to_string();
        git(repo, &["worktree", "add", "--detach", &path, base])
            .map_err(|said| environment("tree_not_cut", format!("git worktree add {path} {base}: {said}")))?;
        Ok(tree)
    }

    fn remove(&self) {
        if self.path.exists() {
            let path = self.path.display().to_string();
            let _ = git(&self.repo, &["worktree", "remove", "--force", &path]);
            let _ = std::fs::remove_dir_all(&self.path);
        }
        let _ = git(&self.repo, &["worktree", "prune"]);
    }
}

impl Drop for Tree {
    fn drop(&mut self) {
        self.remove();
    }
}

/// The hidden tests `runs` times on the base and `runs` times on the fix, in
/// a detached tree of the repository under the build directory.
fn validate(
    repo: &Path,
    task: &Task,
    target_dir: &Path,
    runs: u32,
    timeout: Duration,
) -> Result<Result<Validation, String>, ActionError> {
    let trees = target_dir.join("trees");
    let tree = Tree::cut(repo, &trees, &task.id, &task.base_commit)?;
    let logs = target_dir.join("logs").join(&task.id);
    std::fs::create_dir_all(&logs)
        .map_err(|error| environment("build_dir_unwritable", format!("{}: {error}", logs.display())))?;

    let patch = trees.join(format!("{}.patch", task.id));
    std::fs::write(&patch, &task.hidden_test_patch)
        .map_err(|error| environment("build_dir_unwritable", format!("{}: {error}", patch.display())))?;
    let patch_path = patch.display().to_string();
    if git(&tree.path, &["apply", "--whitespace=nowarn", &patch_path]).is_err() {
        return Ok(Err("hidden test does not apply on base".to_owned()));
    }

    let commands = task.commands();
    for nth in 0..runs {
        match run_once(&tree.path, &commands, target_dir, timeout, &logs, &format!("base-{nth}"))? {
            RunEnd::Red => {}
            RunEnd::Green => return Ok(Err("not red on base".to_owned())),
            RunEnd::Sandbox(shape) => return Ok(Err(format!("sandbox_dependent: {shape}"))),
            RunEnd::TimedOut => return Ok(Err("timed out on base".to_owned())),
        }
    }
    git(&tree.path, &["checkout", "--detach", "--force", &task.fix_commit])
        .map_err(|said| environment("tree_not_cut", format!("checkout of the fix: {said}")))?;
    for nth in 0..runs {
        match run_once(&tree.path, &commands, target_dir, timeout, &logs, &format!("fix-{nth}"))? {
            RunEnd::Green => {}
            RunEnd::Red => return Ok(Err("not green on fix".to_owned())),
            RunEnd::Sandbox(shape) => return Ok(Err(format!("sandbox_dependent: {shape}"))),
            RunEnd::TimedOut => return Ok(Err("timed out on fix".to_owned())),
        }
    }
    Ok(Ok(Validation {
        red_on_base: runs,
        green_on_fix: runs,
        validated_at: now_secs(),
    }))
}

pub struct BenchValidateAction {
    home: Option<PathBuf>,
}

impl BenchValidateAction {
    pub fn new(home: Option<PathBuf>) -> Self {
        Self { home }
    }
}

fn answer(task_id: &str, kept: bool, why: &str, seconds: u64, path: Option<&Path>) -> ActionOutcome {
    ActionOutcome::Went(json!({
        "task_id": task_id,
        "kept": kept,
        "why": why,
        "seconds": seconds,
        "path": path.map(|path| path.display().to_string()),
    }))
}

impl Action for BenchValidateAction {
    fn execute(&self, input: &Value, shared: &SharedState) -> Result<ActionOutcome, ActionError> {
        let asked: ValidateInput = serde_json::from_value(input.clone()).map_err(|error| {
            ActionError::new(
                "invalid_input",
                format!("«{BENCH_VALIDATE_ACTION}» takes repo, fix_commit, base_commit, set, home, runs, target_dir and timeout_secs: {error}"),
            )
        })?;
        if asked.fix_commit.trim().is_empty() || asked.base_commit.trim().is_empty() {
            return Err(ActionError::new("invalid_input", "a candidate names its fix commit and its base commit"));
        }
        let repo = repo_of(asked.repo.as_deref(), shared)?;
        let home = asked
            .home
            .map(PathBuf::from)
            .or_else(|| self.home.clone())
            .ok_or_else(|| environment("no_home", "no home to write the task into: name one with `home`"))?;
        let set = asked.set.unwrap_or_else(|| DEFAULT_SET.to_owned());
        let runs = asked.runs.unwrap_or(DEFAULT_RUNS).max(1);
        let target_dir = asked
            .target_dir
            .map(PathBuf::from)
            .unwrap_or_else(|| repo.join(BUILD_DIR));
        let timeout = Duration::from_secs(asked.timeout_secs.unwrap_or(DEFAULT_TIMEOUT_SECS));

        let resolve = |name: &str| {
            git(&repo, &["rev-parse", "--verify", &format!("{name}^{{commit}}")])
                .map(|sha| sha.trim().to_owned())
                .map_err(|said| not_a_repository(&repo, &said))
        };
        let fix = resolve(&asked.fix_commit)?;
        let base = resolve(&asked.base_commit)?;
        let id = short(&fix);
        let path = Task::path_in(&home, &set, &id);
        if let Ok(existing) = Task::load(&path) {
            if existing.validated.is_some() {
                return Ok(answer(&id, true, "already validated", 0, Some(&path)));
            }
        }

        let _one_at_a_time = OneAtATime::take(&target_dir)?;
        let started = Instant::now();
        let register = home.join("ledger").join(faults::FAULTS_FILE);
        let task = match cut_task(&repo, &fix, &base, &register)? {
            Verdict::Kept(task) => task,
            Verdict::Rejected(why) => return Ok(answer(&id, false, &why, started.elapsed().as_secs(), None)),
        };
        match validate(&repo, &task, &target_dir, runs, timeout)? {
            Ok(validation) => {
                let task = Task {
                    validated: Some(validation),
                    ..task
                };
                task.save(&path)
                    .map_err(|error| environment("task_unwritable", error))?;
                let why = format!("red on base {runs} times, green on fix {runs} times");
                Ok(answer(&id, true, &why, started.elapsed().as_secs(), Some(&path)))
            }
            Err(why) => Ok(answer(&id, false, &why, started.elapsed().as_secs(), None)),
        }
    }

    fn unknown_fields(&self, declared: &Value) -> Vec<String> {
        unknown_among(declared, VALIDATE_FIELDS)
    }

    fn may_spend(&self, _declared: Option<&Value>) -> bool {
        false
    }

    /// The task file is named by the fix commit: written twice, it is the same
    /// file with the same content, and a tree cut is removed before the answer.
    fn redo_evidence(&self, _record: &StepRecord) -> RedoEvidence {
        RedoEvidence::SameOperation("the task file of a fix commit".to_owned())
    }

    fn species(&self) -> StepSpecies {
        StepSpecies::Repeatable
    }
}

fn unknown_among(declared: &Value, known: &[&str]) -> Vec<String> {
    declared
        .as_object()
        .map(|object| {
            object
                .keys()
                .filter(|name| !known.contains(&name.as_str()))
                .cloned()
                .collect()
        })
        .unwrap_or_default()
}

// ── freezing the set ─────────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
struct FreezeInput {
    #[serde(default)]
    home: Option<String>,
    #[serde(default)]
    set: Option<String>,
}

/// The hash of a set: SHA-256 over the sorted `id:base:fix:command` lines,
/// one per task, so the same tasks give the same hash whatever order they
/// were validated in, and a changed command changes it.
pub fn set_hash(tasks: &[Task]) -> String {
    let mut lines: Vec<String> = tasks
        .iter()
        .map(|task| {
            let commands: Vec<String> = task.commands().iter().map(|command| command.join(" ")).collect();
            format!("{}:{}:{}:{}", task.id, task.base_commit, task.fix_commit, commands.join(" && "))
        })
        .collect();
    lines.sort();
    let digest = Sha256::digest(lines.join("\n").as_bytes());
    digest.iter().map(|byte| format!("{byte:02x}")).collect()
}

pub struct BenchFreezeAction {
    home: Option<PathBuf>,
}

impl BenchFreezeAction {
    pub fn new(home: Option<PathBuf>) -> Self {
        Self { home }
    }
}

impl Action for BenchFreezeAction {
    fn execute(&self, input: &Value, _shared: &SharedState) -> Result<ActionOutcome, ActionError> {
        let asked: FreezeInput = serde_json::from_value(input.clone()).map_err(|error| {
            ActionError::new("invalid_input", format!("«{BENCH_FREEZE_ACTION}» takes home and set: {error}"))
        })?;
        let home = asked
            .home
            .map(PathBuf::from)
            .or_else(|| self.home.clone())
            .ok_or_else(|| environment("no_home", "no home to read the set from: name one with `home`"))?;
        let set = asked.set.unwrap_or_else(|| DEFAULT_SET.to_owned());
        let tasks = match Task::load_set(&home, &set) {
            Ok(tasks) => tasks,
            Err(_) if !home.join(super::task::BENCH_DIR).join(&set).exists() => Vec::new(),
            Err(error) => return Err(environment("set_unreadable", error)),
        };
        let validated: Vec<&Task> = tasks.iter().filter(|task| task.validated.is_some()).collect();
        let ids: Vec<&str> = validated.iter().map(|task| task.id.as_str()).collect();
        let frozen: Vec<Task> = validated.iter().map(|task| (*task).clone()).collect();
        Ok(ActionOutcome::Went(json!({
            "set": set,
            "n": ids.len(),
            "tasks": ids.len(),
            "ids": ids,
            "hash": set_hash(&frozen),
            "unvalidated": tasks.len() - validated.len(),
        })))
    }

    fn unknown_fields(&self, declared: &Value) -> Vec<String> {
        unknown_among(declared, FREEZE_FIELDS)
    }

    fn may_spend(&self, _declared: Option<&Value>) -> bool {
        false
    }

    fn redo_evidence(&self, _record: &StepRecord) -> RedoEvidence {
        RedoEvidence::TouchesNothing
    }

    fn species(&self) -> StepSpecies {
        StepSpecies::Repeatable
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bench::fixture::{FixtureRepository, FIX_BODY};

    fn validate_input(fixture: &FixtureRepository, fix: &str, base: &str) -> Value {
        json!({
            "repo": fixture.repo.display().to_string(),
            "fix_commit": fix,
            "base_commit": base,
            "home": fixture.home.display().to_string(),
            "target_dir": fixture.target.display().to_string(),
            "runs": 1,
            "set": "stage-test",
        })
    }

    fn went(outcome: Result<ActionOutcome, ActionError>) -> Value {
        match outcome.expect("a candidate is answered, not refused") {
            ActionOutcome::Went(said) => said,
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn a_trailer_leaves_the_body_and_a_sentence_stays() {
        let body = "The adder subtracted.\nSee fault 7.\n\nSigned-off-by: somebody <s@example.invalid>\nRefs: #12\n";
        assert_eq!(body_without_trailers(body), "The adder subtracted.\nSee fault 7.");
        assert_eq!(body_without_trailers("  \n"), "");
    }

    #[test]
    fn a_diff_that_only_moves_constants_is_a_seed_bump() {
        let bump = "--- a/t.rs\n+++ b/t.rs\n@@ -1 +1 @@\n-const TESTS_TODAY: usize = 3;\n+const TESTS_TODAY: usize = 4;\n";
        assert!(seed_bump_only(bump));
        let table = "--- a/t.rs\n+++ b/t.rs\n@@ -10 +10 @@\n-    (\"machine\", 229),\n+    (\"machine\", 227),\n";
        assert!(seed_bump_only(table), "a row of a ratchet's table is a seed too");
        let rows = "--- a/t.rs\n+++ b/t.rs\n@@ -10 +10,2 @@\n+    \"crates/x/tests/new.rs\",\n+    42,\n";
        assert!(seed_bump_only(rows));
        let test = "--- a/t.rs\n+++ b/t.rs\n@@ -1 +1,3 @@\n+#[test]\n+fn it_adds() {}\n";
        assert!(!seed_bump_only(test));
        assert!(!seed_bump_only(""));
    }

    #[test]
    fn the_tests_a_diff_adds_or_changes_are_named() {
        let diff = "--- a/tests/t.rs\n+++ b/tests/t.rs\n@@ -10,3 +10,4 @@ fn an_old_test() {\n     assert!(x);\n+    assert!(y);\n }\n+#[test]\n+fn a_new_test() {\n+    helper();\n+}\n+fn helper() {}\n";
        assert_eq!(test_names_in(diff), vec!["an_old_test", "a_new_test"]);
        assert!(test_names_in("+fn helper() {}\n").is_empty(), "a function without the attribute is not a test");
    }

    #[test]
    fn the_command_names_the_package_the_binary_and_the_tests() {
        let names = vec!["a_test".to_owned()];
        assert_eq!(
            test_command_for("sailor", "tests/a_judge.rs", &names),
            vec!["cargo", "test", "-p", "sailor", "--test", "a_judge", "--", "a_test"]
        );
        assert_eq!(
            test_command_for("flow", "src/tests.rs", &[]),
            vec!["cargo", "test", "-p", "flow", "--lib"]
        );
        assert_eq!(
            test_command_for("flow", "tests/common/mod.rs", &[]),
            vec!["cargo", "test", "-p", "flow"]
        );
        assert_eq!(package_name("[workspace]\nmembers = [\"crates/*\"]\n"), None);
        assert_eq!(package_name("[package]\nname = \"flow\"\nversion = \"0.1.0\"\n"), Some("flow".to_owned()));
    }

    /// The fixture's fix is red on the base and green on the fix, so it is
    /// kept, and the task file carries everything the judge and the agent
    /// need. Asked again, it is not validated twice.
    #[test]
    fn the_fix_is_kept_with_its_hidden_test_its_gold_patch_and_its_command() {
        let fixture = FixtureRepository::new("kept");
        let node = BenchValidateAction::new(None);
        let said = went(node.execute(&validate_input(&fixture, &fixture.fix, &fixture.bug), &SharedState::new()));
        assert_eq!(said["kept"], true, "{said}");
        let id: String = fixture.fix.chars().take(8).collect();
        assert_eq!(said["task_id"], id);
        let path = Task::path_in(&fixture.home, "stage-test", &id);
        assert_eq!(said["path"], path.display().to_string());

        let task = Task::load(&path).expect("the task was written");
        assert_eq!(task.base_commit, fixture.bug);
        assert_eq!(task.fix_commit, fixture.fix);
        assert_eq!(task.prompt, FIX_BODY, "no register at this home: the body is the prompt");
        assert_eq!(task.prompt_source, "commit body");
        assert_eq!(task.test_files, vec!["tests/adds.rs"]);
        assert!(task.hidden_test_patch.contains("+fn two_and_two_make_four"), "{}", task.hidden_test_patch);
        assert!(task.gold_patch.contains("+    a + b"), "{}", task.gold_patch);
        assert!(!task.gold_patch.contains("tests/adds.rs"), "the test is hidden from the gold patch");
        assert_eq!(task.gold_added_lines, 1);
        assert_eq!(
            task.test_command,
            vec!["cargo", "test", "-p", "fixture", "--test", "adds", "--", "two_and_two_make_four"]
        );
        assert!(task.test_commands.is_empty());
        let validated = task.validated.expect("validated");
        assert_eq!((validated.red_on_base, validated.green_on_fix), (1, 1));
        assert!(!fixture.target.join("trees").join(&id).exists(), "the tree is removed");

        let again = went(node.execute(&validate_input(&fixture, &fixture.fix, &fixture.bug), &SharedState::new()));
        assert_eq!(again["kept"], true);
        assert_eq!(again["why"], "already validated");
    }

    /// The fault's own words win over the commit's when the register holds
    /// the number the commit names.
    #[test]
    fn a_fault_the_register_holds_is_the_prompt() {
        let fixture = FixtureRepository::new("fault-prompt");
        let register = fixture.home.join("ledger").join(faults::FAULTS_FILE);
        let store = faults::Faults::open(&register).expect("a register of our own");
        for nth in 1..=7 {
            store
                .record(&faults::Draft {
                    happened_on: "01/09".to_owned(),
                    what_happened: format!("what happened {nth}"),
                    how_it_showed: format!("how it showed {nth}"),
                    what_would_prevent: "a test".to_owned(),
                    status: "**open**".to_owned(),
                    standing: None,
                })
                .expect("recorded");
        }
        let Verdict::Kept(task) = cut_task(&fixture.repo, &fixture.fix, &fixture.bug, &register).expect("git reads") else {
            panic!("the fix is a task");
        };
        assert_eq!(task.prompt_source, "fault 7");
        assert_eq!(task.prompt, "what happened 7\n\nhow it showed 7");
    }

    #[test]
    fn a_test_that_was_green_before_the_fix_is_not_a_task() {
        let fixture = FixtureRepository::new("green");
        let said = went(BenchValidateAction::new(None).execute(
            &validate_input(&fixture, &fixture.green_fix, &fixture.tests_only),
            &SharedState::new(),
        ));
        assert_eq!(said["kept"], false, "{said}");
        assert_eq!(said["why"], "not red on base");
        assert!(said["path"].is_null());
        let id: String = fixture.green_fix.chars().take(8).collect();
        assert!(!Task::path_in(&fixture.home, "stage-test", &id).exists(), "nothing is saved");
    }

    #[test]
    fn a_commit_without_a_body_or_with_only_trailers_has_no_prompt() {
        let nowhere = Path::new("/nowhere/faults.db");
        assert_eq!(prompt_for("fix: x", "", nowhere), None);
        assert_eq!(prompt_for("fix: x", "Refs: #1\n", nowhere), None);
        assert_eq!(
            prompt_for("fix: x", "Because.\n\nRefs: #1\n", nowhere),
            Some(("Because.".to_owned(), "commit body".to_owned()))
        );
        assert_eq!(
            prompt_for("fix: closes fault 7", "Because.", nowhere).map(|(_, source)| source),
            Some("commit body".to_owned()),
            "a fault the register does not hold falls back to the body"
        );
    }

    /// The hash does not depend on the order the tasks came in, and it does
    /// move when a command does.
    #[test]
    fn the_hash_of_a_set_is_stable_in_order_and_moves_with_a_command() {
        let one = Task {
            id: "a".into(),
            repo: "/r".into(),
            base_commit: "1".into(),
            fix_commit: "2".into(),
            prompt: String::new(),
            prompt_source: String::new(),
            test_files: vec![],
            hidden_test_patch: String::new(),
            gold_patch: String::new(),
            gold_added_lines: 0,
            test_command: vec!["cargo".into(), "test".into()],
            test_commands: vec![],
            validated: None,
        };
        let two = Task {
            id: "b".into(),
            ..one.clone()
        };
        assert_eq!(set_hash(&[one.clone(), two.clone()]), set_hash(&[two.clone(), one.clone()]));
        let changed = Task {
            test_command: vec!["cargo".into(), "test".into(), "--lib".into()],
            ..one.clone()
        };
        assert_ne!(set_hash(&[one, two.clone()]), set_hash(&[changed, two]));
    }

    #[test]
    fn freezing_reads_the_validated_tasks_of_a_set_and_an_absent_set_is_empty() {
        let fixture = FixtureRepository::new("freeze");
        let a_task = |id: &str, validated: bool| Task {
            id: id.into(),
            repo: "/r".into(),
            base_commit: "1".into(),
            fix_commit: "2".into(),
            prompt: String::new(),
            prompt_source: String::new(),
            test_files: vec![],
            hidden_test_patch: String::new(),
            gold_patch: String::new(),
            gold_added_lines: 0,
            test_command: vec!["cargo".into(), "test".into()],
            test_commands: vec![],
            validated: validated.then(Validation::default),
        };
        a_task("aa", true).save(&Task::path_in(&fixture.home, "stage-test", "aa")).unwrap();
        a_task("bb", false).save(&Task::path_in(&fixture.home, "stage-test", "bb")).unwrap();
        let node = BenchFreezeAction::new(Some(fixture.home.clone()));
        let said = went(node.execute(&json!({"set": "stage-test"}), &SharedState::new()));
        assert_eq!(said["n"], 1, "{said}");
        assert_eq!(said["ids"], json!(["aa"]));
        assert_eq!(said["unvalidated"], 1);
        assert_eq!(said["hash"], set_hash(&[a_task("aa", true)]));

        let empty = went(node.execute(&json!({"set": "nobody-built-this"}), &SharedState::new()));
        assert_eq!(empty["n"], 0);
    }
}
