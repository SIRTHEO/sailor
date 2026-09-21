//! The commit this binary is built from, so `sailor version` can name the
//! build in service. Sources the repository does not track say they do not
//! know, even when they sit inside some other repository.

use std::path::PathBuf;
use std::process::Command;

fn asked(args: &[&str]) -> Option<String> {
    let out = Command::new("git").args(args).output().ok()?;
    out.status
        .success()
        .then(|| String::from_utf8_lossy(&out.stdout).trim().to_owned())
        .filter(|said| !said.is_empty())
}

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    let tracked = asked(&["ls-files", "build.rs"]).is_some();
    let commit = tracked
        .then(|| asked(&["rev-parse", "--verify", "HEAD^{commit}"]))
        .flatten();
    println!(
        "cargo:rustc-env=SAILOR_BUILD_COMMIT={}",
        commit.as_deref().unwrap_or("")
    );
    let Some(git_dir) = asked(&["rev-parse", "--absolute-git-dir"]).map(PathBuf::from) else {
        return;
    };
    // HEAD moves on a checkout, its log on every commit. A watched file that
    // is missing would make every build dirty, so only those present count.
    for watched in [git_dir.join("HEAD"), git_dir.join("logs").join("HEAD")] {
        if watched.exists() {
            println!("cargo:rerun-if-changed={}", watched.display());
        }
    }
}
