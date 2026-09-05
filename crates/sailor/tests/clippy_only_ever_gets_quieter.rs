//! Clippy only ever gets quieter: its warnings are counted per crate over the
//! whole workspace, every target included, and each count may only fall. The
//! battery's gate stops on the lints that name a defect; this judge holds the
//! rest where it is instead of leaving it a number in a job summary.

use std::collections::BTreeMap;
use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::process::Command;

/// Warnings per crate, as measured today. Downwards only: a crate that warns
/// more than its seed is red, and so is a seed left above what the tree holds.
const WARNINGS_TODAY: &[(&str, usize)] = &[
    ("actions", 134),
    ("catalogue", 0),
    ("faults", 0),
    ("flow", 0),
    ("inventory", 0),
    ("ledger", 0),
    ("models", 0),
    ("profiles", 0),
    ("registry", 0),
    ("relay", 0),
    ("release", 0),
    ("sailor", 45),
    ("sessions", 0),
    ("supervisor", 0),
    ("terminal", 0),
    ("toolbox", 0),
    ("trigger", 0),
    ("ui", 0),
    ("workspace", 0),
];

/// The linter the seeds were measured with. Another version is another set
/// of lints: under it the numbers are re-measured and this name rewritten,
/// never compared across.
const SEEDS_ARE_FOR: &str = "clippy 0.1.98";

/// How far a seed may sit above what the tree holds. Zero: a seed is a number
/// in a file, and a merge taking the older side raises it with no conflict.
const HOW_STALE_A_SEED_MAY_BE: usize = 0;

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("the crate sits two levels under the root")
        .to_path_buf()
}

/// The build directory of whoever runs the judge, when they named one: the
/// ratchet's own is reused rather than a cold one filled beside it.
fn callers_build_directory() -> Option<OsString> {
    std::env::var_os("CARGO_TARGET_DIR")
}

fn linter(root: &Path, build_directory: Option<OsString>) -> Command {
    let mut command = Command::new("cargo");
    command
        .current_dir(root)
        .arg("clippy")
        .arg("--manifest-path")
        .arg(root.join("Cargo.toml"));
    if let Some(directory) = build_directory {
        command.env("CARGO_TARGET_DIR", directory);
    }
    command
}

/// The name and version of the linter, or nothing when it is not installed.
fn linter_version(root: &Path) -> Option<String> {
    let said = linter(root, callers_build_directory()).arg("--version").output().ok()?;
    if !said.status.success() {
        return None;
    }
    let text = String::from_utf8_lossy(&said.stdout);
    let mut words = text.split_whitespace();
    Some(format!("{} {}", words.next()?, words.next()?))
}

/// Every crate under `crates/`, so a crate that warns nowhere still has a row
/// and its first warning is a rise, not a missing name.
fn crates_of(root: &Path) -> Vec<String> {
    let mut names: Vec<String> = std::fs::read_dir(root.join("crates"))
        .map(|entries| {
            entries
                .flatten()
                .filter(|entry| entry.path().is_dir())
                .map(|entry| entry.file_name().to_string_lossy().into_owned())
                .collect()
        })
        .unwrap_or_default();
    names.sort();
    names
}

/// The linter's whole say over the workspace, counted per crate. A run that
/// does not finish is an error, not a zero.
fn warnings_per_crate(root: &Path) -> Result<BTreeMap<String, usize>, String> {
    let said = linter(root, callers_build_directory())
        .args(["--workspace", "--all-targets", "--message-format=short"])
        .output()
        .map_err(|error| format!("cargo clippy: {error}"))?;
    let text = format!(
        "{}{}",
        String::from_utf8_lossy(&said.stdout),
        String::from_utf8_lossy(&said.stderr)
    );
    if !said.status.success() {
        let lines: Vec<&str> = text.lines().collect();
        let tail = lines.len().saturating_sub(20);
        return Err(format!("the linter did not finish:\n{}", lines[tail..].join("\n")));
    }
    let mut counts: BTreeMap<String, usize> =
        crates_of(root).into_iter().map(|name| (name, 0)).collect();
    for name in warned_crates(&text) {
        *counts.entry(name).or_default() += 1;
    }
    Ok(counts)
}

/// The crate each warning line names: `crates/<name>/src/x.rs:1:2: warning: …`
/// is `<name>`, a path from elsewhere is its first directory, so no warning
/// is dropped for lacking a crate. Summary lines carry no path and no place.
fn warned_crates(text: &str) -> Vec<String> {
    text.lines()
        .filter_map(|line| {
            let (place, _) = line.split_once(": warning: ")?;
            let mut parts = place.rsplitn(3, ':');
            let column = parts.next()?;
            let row = parts.next()?;
            let path = parts.next()?;
            let is_number = |word: &str| !word.is_empty() && word.bytes().all(|b| b.is_ascii_digit());
            (is_number(column) && is_number(row)).then(|| crate_of(path))
        })
        .collect()
}

fn crate_of(path: &str) -> String {
    let mut parts = path.split(['/', '\\']);
    match parts.next() {
        Some("crates") => parts.next().unwrap_or_default().to_owned(),
        Some(first) => first.to_owned(),
        None => String::new(),
    }
}

fn table_of(counts: &BTreeMap<String, usize>) -> String {
    counts
        .iter()
        .map(|(name, howmany)| format!("    (\"{name}\", {howmany}),"))
        .collect::<Vec<_>>()
        .join("\n")
}

/// One number per crate, exact. Measured on one developer machine: 11.5 s
/// cold into an empty build directory, 6.1 s with the dependencies warm and
/// every crate re-linted, 0.2 s with nothing changed — inside the two minutes
/// the ratchet allows a judge. Without the linter installed nothing is
/// compared, and the test says so rather than inventing a zero.
#[test]
fn no_crate_warns_more_than_today() {
    let root = root();
    let Some(version) = linter_version(&root) else {
        println!("the linter is not installed here: nothing measured, nothing compared");
        return;
    };
    let measured = warnings_per_crate(&root).unwrap_or_else(|why| panic!("{why}"));
    let table = table_of(&measured);
    assert_eq!(
        version, SEEDS_ARE_FOR,
        "the seeds were measured with «{SEEDS_ARE_FOR}» and this is «{version}»: another linter is another instrument. Write «{version}» in SEEDS_ARE_FOR and this table:\n{table}"
    );
    let seeded: BTreeMap<&str, usize> = WARNINGS_TODAY.iter().copied().collect();
    let mut complaints = Vec::new();
    for (name, howmany) in &measured {
        let seed = seeded.get(name.as_str()).copied();
        if !seed.is_some_and(|seed| *howmany <= seed) {
            complaints.push(format!(
                "crate «{name}» warns {howmany} times against a seed of {seed:?}: quiet the new ones, or the table is stale"
            ));
        } else if !seed.is_some_and(|seed| seed <= howmany + HOW_STALE_A_SEED_MAY_BE) {
            complaints.push(format!(
                "crate «{name}» is seeded at {seed:?} and warns {howmany} times: lower the seed to {howmany}"
            ));
        }
    }
    assert!(complaints.is_empty(), "{}; measured now:\n{table}", complaints.join("; "));
    assert_eq!(
        seeded.len(),
        measured.len(),
        "the table names crates the tree lacks, or lacks some; measured now:\n{table}"
    );
}

/// Whoever measures gets measured: a reader that lost the warning lines would
/// count zero everywhere, and every seed would be stale for ever.
#[test]
fn the_check_can_still_see_what_it_counts() {
    let said = "\
    Checking flow v0.1.0 (crates/flow)
crates/flow/src/lib.rs:12:5: warning: this `if` has identical blocks
crates/flow/src/lib.rs:40:9: warning: a message that goes on: warning: inside
crates/actions/tests/a_test.rs:1:1: warning: unused import
desktop/src-tauri/src/main.rs:3:3: warning: something
crates/flow/src/lib.rs:12:5: error: an error is not a warning
crates/flow/src/lib.rs:12:x: warning: a place without a column is not a place
warning: `flow` (lib) generated 45 warnings
warning: unused manifest key: package.something
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 0.14s
";
    assert_eq!(
        warned_crates(said),
        ["flow", "flow", "actions", "desktop"],
        "one crate per warning line, in order, and nothing for the rest"
    );
    let root = root();
    let mut named: Vec<&str> = WARNINGS_TODAY.iter().map(|(name, _)| *name).collect();
    named.sort();
    assert_eq!(named, crates_of(&root), "the table names every crate under crates/, once");
    let with = linter(&root, Some(OsString::from("somewhere")));
    assert!(
        with.get_envs().any(|(key, value)| key == "CARGO_TARGET_DIR" && value == Some("somewhere".as_ref())),
        "the caller's build directory is handed on"
    );
    assert!(
        linter(&root, None).get_envs().all(|(key, _)| key != "CARGO_TARGET_DIR"),
        "and nothing is invented when the caller named none"
    );
}
