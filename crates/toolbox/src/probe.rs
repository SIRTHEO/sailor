//! The gestures that touch the world: looking on the search path, seeing whether
//! a file is there, asking a binary its version, reading the keys of a JSON file.
//!
//! THE MACHINE IS A PARAMETER, NOT THE ENVIRONMENT — path directories, home and
//! variables all go through `Machine`. No testing affectation: it is the only way
//! to prove "absent" and "could not check" differ, both built in a temp directory.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::Duration;

/// A home no machine has: what a bare machine gets when its caller has no
/// scratch directory to give it.
pub const NOWHERE: &str = "/nonexistent";

/// The world being searched.
#[derive(Debug, Clone)]
pub struct Machine {
    /// The directories an executable is searched in, in the order they are
    /// looked at.
    pub path_dirs: Vec<PathBuf>,
    pub home: PathBuf,
    /// The variables a descriptor path may name.
    pub env: BTreeMap<String, String>,
    /// Whether a binary may be run to ask it its version. Switched off, every
    /// version becomes "not asked": that serves whoever wants the list without
    /// starting anything, and it is the caller's choice, not a fallback.
    pub version_probes: bool,
}

impl Machine {
    /// The machine this process runs on.
    pub fn current() -> Machine {
        let env: BTreeMap<String, String> = std::env::vars().collect();
        let home = env
            .get("HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from("/"));
        let path_dirs = env
            .get("PATH")
            .map(|p| {
                p.split(':')
                    .filter(|s| !s.is_empty())
                    .map(PathBuf::from)
                    .collect()
            })
            .unwrap_or_default();
        Machine {
            path_dirs,
            home,
            env,
            version_probes: true,
        }
    }

    /// A machine with nothing on it: an empty search path, no variables and
    /// `home` as its home. What a test hands in so that nothing of the machine
    /// it runs on is read, and what a static check builds its world from.
    pub fn bare(home: PathBuf) -> Machine {
        Machine {
            path_dirs: Vec::new(),
            home,
            env: BTreeMap::new(),
            version_probes: false,
        }
    }

    /// `~/x`, `$VAR/x` and `${VAR}/x` become real paths. A variable that does
    /// not exist stays written as it is: replacing it with nothing would build a
    /// plausible, wrong path, and the reader would not know why.
    pub fn expand(&self, raw: &str) -> String {
        let mut text = raw.to_string();
        if text == "~" {
            return self.home.to_string_lossy().into_owned();
        }
        if let Some(rest) = text.strip_prefix("~/") {
            text = self.home.join(rest).to_string_lossy().into_owned();
        }
        let mut out = String::with_capacity(text.len());
        let mut chars = text.chars().peekable();
        while let Some(c) = chars.next() {
            if c != '$' {
                out.push(c);
                continue;
            }
            let braced = chars.peek() == Some(&'{');
            if braced {
                chars.next();
            }
            let mut name = String::new();
            while let Some(&next) = chars.peek() {
                let ok = next.is_alphanumeric() || next == '_';
                if braced && next == '}' {
                    chars.next();
                    break;
                }
                if !ok {
                    break;
                }
                name.push(next);
                chars.next();
            }
            match self.env.get(&name) {
                Some(value) => out.push_str(value),
                None if braced => out.push_str(&format!("${{{name}}}")),
                None => {
                    out.push('$');
                    out.push_str(&name);
                }
            }
        }
        out
    }

    /// The paths a pattern matches. Without a `*` it is the path itself, whether
    /// or not it exists: telling "not there" from "could not look" is the job of
    /// whoever interrogates it, not of whoever expands it.
    pub fn resolve(&self, raw: &str) -> Vec<PathBuf> {
        let expanded = self.expand(raw);
        if !expanded.contains('*') {
            return vec![PathBuf::from(expanded)];
        }
        // The root is the part before the first component holding a star; the
        // rest is the pattern. `inventory::discovery::glob` matches exactly this
        // shape — literal components and `*` — and reusing it avoids a second
        // implementation that drifts from the first.
        let parts: Vec<&str> = expanded.split('/').collect();
        let split = parts.iter().position(|p| p.contains('*')).unwrap_or(0);
        let root = PathBuf::from(parts[..split].join("/"));
        let pattern = parts[split..].join("/");
        let root = if root.as_os_str().is_empty() {
            PathBuf::from("/")
        } else {
            root
        };
        inventory::discovery::glob(&root, &pattern)
    }
}

/// Here, not here, or could not be looked at.
///
/// THE THIRD ARM IS THE POINT OF THE WHOLE CRATE. An inventory that writes "not
/// installed" where it could not actually look is worse than no inventory: the
/// reader installs a second copy of something they already had, or gives up a
/// tool they owned. Every "don't know" carries the measured reason.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Look {
    Found(PathBuf),
    Missing,
    Blocked(String),
}

/// Is a path there? `symlink_metadata` and not `exists()`: `exists()` answers
/// `false` when permission is denied too, which is exactly the lie this crate
/// exists to remove.
pub fn look_at(path: &Path) -> Look {
    match std::fs::symlink_metadata(path) {
        Ok(_) => Look::Found(path.to_path_buf()),
        Err(error) if error.kind() == ErrorKind::NotFound => Look::Missing,
        Err(error) => Look::Blocked(format!("{}: {error}", path.to_string_lossy())),
    }
}

/// Looks for an executable in the search directories.
///
/// "NOT THERE" IS ONLY SAID AFTER LOOKING EVERYWHERE. If even one search
/// directory could not be read the answer is "don't know": the executable could
/// have been in it. With an empty search path the answer is "don't know" all the
/// more — nowhere was looked at.
pub fn look_up(name: &str, machine: &Machine) -> Look {
    if machine.path_dirs.is_empty() {
        return Look::Blocked("no directory to search in: the path is empty".to_string());
    }
    let mut blocked: Vec<String> = Vec::new();
    for dir in &machine.path_dirs {
        let candidate = dir.join(name);
        match std::fs::metadata(&candidate) {
            Ok(meta) if is_runnable(&meta) => return Look::Found(candidate),
            // A name that is there but is not executable is not the binary being
            // looked for: carry on, the way a shell does.
            Ok(_) => continue,
            Err(error) if error.kind() == ErrorKind::NotFound => continue,
            Err(error) => blocked.push(format!("{}: {error}", dir.to_string_lossy())),
        }
    }
    if blocked.is_empty() {
        Look::Missing
    } else {
        Look::Blocked(format!(
            "looked for `{name}`, but {} search {} could not be read: {}",
            blocked.len(),
            if blocked.len() == 1 {
                "directory"
            } else {
                "directories"
            },
            blocked.join("; ")
        ))
    }
}

#[cfg(unix)]
fn is_runnable(meta: &std::fs::Metadata) -> bool {
    use std::os::unix::fs::PermissionsExt;
    meta.is_file() && meta.permissions().mode() & 0o111 != 0
}

#[cfg(not(unix))]
fn is_runnable(meta: &std::fs::Metadata) -> bool {
    meta.is_file()
}

/// What a binary answered when asked its version.
///
/// THREE ARMS FOR THE SAME REASON AS THE THREE OF `Presence`: "I did not ask"
/// and "I asked and it did not answer" tell the reader two different things to
/// do.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "state", content = "detail", rename_all = "lowercase")]
pub enum VersionReading {
    Declared(String),
    /// The descriptor does not say how to ask for it, or the caller switched
    /// executions off. Not a fault: a question that was not asked.
    NotAsked(String),
    /// The question was asked and got no useful answer, with the reason.
    Unavailable(String),
}

/// Asks for the version by running the executable that was found.
///
/// STANDARD INPUT IS CLOSED AT ONCE. An engine that reads its own input hangs on
/// an EOF that never arrives: on this machine that has already cost a job left
/// "in progress" for hours. The time limit would save it anyway, but waiting ten
/// seconds per tool in the list is a detection nobody runs twice.
pub fn read_version(
    bin: &Path,
    args: &[String],
    must_contain: &str,
    limit: Duration,
) -> VersionReading {
    let mut cmd = Command::new(bin);
    cmd.args(args).stdin(Stdio::null());
    let printed = format!("`{} {}`", bin.to_string_lossy(), args.join(" "));
    match actions::run_with_timeout(cmd, limit) {
        actions::RunOutcome::Finished {
            status,
            stdout,
            stderr,
        } => {
            let out = String::from_utf8_lossy(&stdout).into_owned();
            let err = String::from_utf8_lossy(&stderr).into_owned();
            if !status.success() {
                return VersionReading::Unavailable(format!(
                    "{printed} exited with {}: {}",
                    status
                        .code()
                        .map(|c| c.to_string())
                        .unwrap_or_else(|| "a signal".to_string()),
                    pick(&err, must_contain)
                        .or_else(|| pick(&out, must_contain))
                        .unwrap_or_else(|| "no message".to_string())
                ));
            }
            // A BINARY THAT EXITS ZERO WITHOUT SAYING ANYTHING has declared no
            // version, and writing "" in its place would make one look declared:
            // the difference counts when two machines are compared.
            match pick(&out, must_contain).or_else(|| pick(&err, must_contain)) {
                Some(line) => VersionReading::Declared(line),
                None if must_contain.is_empty() => {
                    VersionReading::Unavailable(format!("{printed} printed nothing"))
                }
                None => VersionReading::Unavailable(format!(
                    "no line of {printed} contains `{must_contain}`"
                )),
            }
        }
        actions::RunOutcome::TimedOut => VersionReading::Unavailable(format!(
            "{printed} did not return within {} seconds",
            limit.as_secs()
        )),
        actions::RunOutcome::SpawnFailed(reason) => {
            VersionReading::Unavailable(format!("{printed} did not start: {reason}"))
        }
    }
}

/// The line to keep: the first one containing the requested text, or the first
/// line with anything on it when the descriptor asks for nothing.
fn pick(text: &str, must_contain: &str) -> Option<String> {
    text.lines()
        .map(str::trim)
        .find(|l| !l.is_empty() && (must_contain.is_empty() || l.contains(must_contain)))
        .map(|l| l.to_string())
}

/// The keys found by following a path inside a JSON value, in order.
///
/// A `*` in the path stands for "every key at this level": without it, servers
/// declared project by project would stay invisible and the list would say zero
/// where there are some.
pub fn json_keys(value: &serde_json::Value, pointer: &[String]) -> Vec<String> {
    let Some((head, tail)) = pointer.split_first() else {
        return match value.as_object() {
            Some(map) => map.keys().cloned().collect(),
            None => Vec::new(),
        };
    };
    let Some(map) = value.as_object() else {
        return Vec::new();
    };
    if head == "*" {
        let mut out = Vec::new();
        for child in map.values() {
            out.extend(json_keys(child, tail));
        }
        out.sort();
        out.dedup();
        return out;
    }
    match map.get(head) {
        Some(child) => json_keys(child, tail),
        None => Vec::new(),
    }
}
