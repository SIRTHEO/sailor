//! The commit this binary is built from, so `sailor version` can name the
//! build in service. A build outside a repository says it does not know.

use std::path::PathBuf;
use std::process::Command;

fn git(args: &[&str]) -> Option<String> {
    let out = Command::new("git").args(args).output().ok()?;
    out.status
        .success()
        .then(|| String::from_utf8_lossy(&out.stdout).trim().to_owned())
        .filter(|said| !said.is_empty())
}

fn main() {
    let commit = git(&["rev-parse", "--verify", "HEAD^{commit}"]);
    println!(
        "cargo:rustc-env=SAILOR_BUILD_COMMIT={}",
        commit.as_deref().unwrap_or("")
    );
    // A new commit moves HEAD, or the branch HEAD points at: either reruns this.
    if let Some(git_dir) = git(&["rev-parse", "--absolute-git-dir"]).map(PathBuf::from) {
        println!("cargo:rerun-if-changed={}", git_dir.join("HEAD").display());
        if let Some(branch) = git(&["symbolic-ref", "--quiet", "HEAD"]) {
            let common = git(&["rev-parse", "--path-format=absolute", "--git-common-dir"])
                .map(PathBuf::from)
                .unwrap_or(git_dir);
            println!("cargo:rerun-if-changed={}", common.join(&branch).display());
            println!("cargo:rerun-if-changed={}", common.join("packed-refs").display());
        }
    }
    println!("cargo:rerun-if-changed=build.rs");
}
