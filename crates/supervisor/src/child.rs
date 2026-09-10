//! What the live mode builds and watches. The road that starts a process
//! lives in `machine`: the command line starts processes too, and must not
//! carry the machinery that rebuilds a checkout.

use crate::BuildOutcome;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

pub use machine::child::{Process, Spec, StartToken};

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
