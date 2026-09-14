//! A release of one named commit: resolved against the trunk, checked out
//! detached in a directory of its own under `target/candidates`, and proved
//! installed by the digest of the copy in service.

use crate::release_cmd::{clone_repository, git_success, git_text};
use sha2::{Digest, Sha256};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

pub(crate) struct Candidate {
    pub(crate) revision: String,
    pub(crate) short: String,
}

/// Where a candidate's checkout and build live: never the shared
/// `target/from-head`, which the next release from HEAD would build over.
pub(crate) struct Place {
    pub(crate) tree: PathBuf,
    pub(crate) build: PathBuf,
}

/// The commit `asked` names, refused unless the HEAD this release stands on
/// reaches it.
pub(crate) fn resolve(root: &Path, asked: &str) -> Result<Candidate, String> {
    let not_a_commit = || catalogue::say("cli.release.candidate_not_a_commit", &[("candidate", asked)]);
    let revision = git_text(root, &["rev-parse", "--verify", "--quiet", &format!("{asked}^{{commit}}")])
        .map_err(|_| not_a_commit())?;
    if revision.is_empty() {
        return Err(not_a_commit());
    }
    let reached = Command::new("git")
        .arg("-C")
        .arg(root)
        .args(["merge-base", "--is-ancestor", &revision, "HEAD"])
        .status()
        .map_err(|_| catalogue::say("cli.release.git_command_failed", &[]))?;
    if !reached.success() {
        let head = git_text(root, &["rev-parse", "--short", "HEAD"])?;
        return Err(catalogue::say(
            "cli.release.candidate_not_on_the_trunk",
            &[("candidate", asked), ("head", &head)],
        ));
    }
    let short = git_text(root, &["rev-parse", "--short", &revision])?;
    Ok(Candidate { revision, short })
}

/// A detached checkout of exactly the candidate, kept between releases of
/// the same commit so cargo rebuilds nothing it already built.
pub(crate) fn check_out(root: &Path, candidate: &Candidate) -> Result<Place, String> {
    let base = root.join("target").join("candidates").join(&candidate.revision);
    let place = Place {
        tree: base.join("tree"),
        build: base.join("build"),
    };
    let failed = || {
        catalogue::say(
            "cli.release.candidate_checkout_failed",
            &[("candidate", &candidate.short)],
        )
    };
    if !place.tree.join(".git").is_dir() {
        if place.tree.exists() {
            fs::remove_dir_all(&place.tree).map_err(|_| failed())?;
        }
        fs::create_dir_all(&base).map_err(|_| failed())?;
        clone_repository(root, &place.tree)?;
    }
    for args in [
        vec!["checkout", "--quiet", "--force", "--detach", candidate.revision.as_str()],
        vec!["clean", "-fdq"],
    ] {
        git_success(
            Command::new("git").arg("-C").arg(&place.tree).args(&args),
            &failed(),
        )?;
    }
    if git_text(&place.tree, &["rev-parse", "HEAD"])? != candidate.revision {
        return Err(failed());
    }
    Ok(place)
}

/// Records beside the stamp the digest of the copy in service, after proving
/// it is the binary this candidate built, and says it.
pub(crate) fn record_what_was_installed(
    candidate: &Candidate,
    built: &Path,
    installed: &Path,
    stamp: &Path,
) -> Result<(), String> {
    let digest = sha256_of(installed)?;
    if digest != sha256_of(built)? {
        return Err(catalogue::say(
            "cli.release.candidate_installed_differs",
            &[("path", &installed.display().to_string())],
        ));
    }
    let record = stamp.with_extension("sha256");
    fs::write(&record, format!("{digest}\n")).map_err(|error| {
        catalogue::say(
            "cli.release.candidate_digest_not_written",
            &[("path", &record.display().to_string()), ("error", &error.to_string())],
        )
    })?;
    println!(
        "   {}",
        catalogue::say(
            "cli.release.candidate_installed",
            &[
                ("commit", &candidate.short),
                ("digest", &digest),
                ("path", &record.display().to_string()),
            ],
        )
    );
    Ok(())
}

fn sha256_of(file: &Path) -> Result<String, String> {
    let bytes = fs::read(file).map_err(|error| {
        catalogue::say(
            "cli.release.cannot_read_what_was_built",
            &[("path", &file.display().to_string()), ("error", &error.to_string())],
        )
    })?;
    Ok(Sha256::digest(&bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect())
}
