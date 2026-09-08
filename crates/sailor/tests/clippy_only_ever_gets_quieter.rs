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
    ("actions", 0),
    ("catalogue", 0),
    ("desktop", 0),
    ("faults", 0),
    ("flow", 0),
    ("inventory", 0),
    ("ledger", 0),
    ("machine", 0),
    ("models", 0),
    ("profiles", 0),
    ("registry", 0),
    ("relay", 0),
    ("release", 0),
    ("sailor", 0),
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

fn linter(root: &Path, build_directory: Option<OsString>, manifest: &Path) -> Command {
    let mut command = Command::new("cargo");
    command
        .current_dir(root)
        .arg("clippy")
        .arg("--manifest-path")
        .arg(manifest);
    if let Some(directory) = build_directory {
        command.env("CARGO_TARGET_DIR", directory);
    }
    command
}

/// The name and version of the linter, or nothing when it is not installed.
fn linter_version(root: &Path) -> Option<String> {
    let said = linter(root, callers_build_directory(), &root.join("Cargo.toml"))
        .arg("--version")
        .output()
        .ok()?;
    version_in(said.status.success(), &String::from_utf8_lossy(&said.stdout))
}

/// What the linter answered when asked its name, read apart from running it so
/// the answer of a machine without one can be put to the judge.
fn version_in(answered: bool, said: &str) -> Option<String> {
    if !answered {
        return None;
    }
    let mut words = said.split_whitespace();
    Some(format!("{} {}", words.next()?, words.next()?))
}

/// The window's shell, and the name its warnings are counted under. It is a
/// workspace of its own, so `--workspace` at the root never reached it.
const THE_SHELL: (&str, &str) = ("desktop", "desktop/src-tauri/Cargo.toml");

/// Every crate under `crates/`, and the shell, so a crate that warns nowhere
/// still has a row and its first warning is a rise, not a missing name.
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
    if root.join(THE_SHELL.1).is_file() {
        names.push(THE_SHELL.0.to_owned());
    }
    names.sort();
    names
}

/// The linter's whole say over the workspace, counted per crate. A run that
/// does not finish is an error, not a zero.
fn warnings_per_crate(root: &Path) -> Result<BTreeMap<String, usize>, String> {
    let mut counts = counted_over(root, &linted(root, &root.join("Cargo.toml"), &["--workspace"])?);
    let shell = root.join(THE_SHELL.1);
    if shell.is_file() {
        // Its paths come out relative to its own manifest, so they cannot be
        // read back to a crate name: the whole run counts under one.
        let said = linted(root, &shell, &[])?;
        counts.insert(THE_SHELL.0.to_owned(), warned_crates(&said).len());
    }
    Ok(counts)
}

/// One linter run over the manifest it is handed. Not finishing is an error.
fn linted(root: &Path, manifest: &Path, over: &[&str]) -> Result<String, String> {
    let said = linter(root, callers_build_directory(), manifest)
        .args(over)
        .args(["--all-targets", "--message-format=short"])
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
    Ok(text)
}

/// One linter run counted against the crates of one tree, taken apart from
/// running it so a tree with a warning planted in it can be counted the same
/// way.
fn counted_over(root: &Path, text: &str) -> BTreeMap<String, usize> {
    let mut counts: BTreeMap<String, usize> =
        crates_of(root).into_iter().map(|name| (name, 0)).collect();
    for name in warned_crates(text) {
        *counts.entry(name).or_default() += 1;
    }
    counts
}

/// What one measurement owes one table of seeds. Empty is the tree holding.
fn complaints_over(measured: &BTreeMap<String, usize>, seeds: &[(&str, usize)]) -> Vec<String> {
    let seeded: BTreeMap<&str, usize> = seeds.iter().copied().collect();
    let mut complaints = Vec::new();
    for (name, howmany) in measured {
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
    if seeded.len() != measured.len() {
        complaints.push(format!(
            "the table names {} crates and the tree holds {}",
            seeded.len(),
            measured.len()
        ));
    }
    complaints
}

/// How one run of this judge came out.
#[derive(Debug, PartialEq, Eq)]
enum Verdict {
    /// There was no linter to answer, so no count was compared.
    NotMeasured(String),
    Louder(Vec<String>),
    /// Another linter is another set of lints, and its numbers are not these.
    AnotherInstrument(String),
    Quiet {
        linted: usize,
    },
}

/// The judge itself, over the tree and the linter it is handed. With no linter
/// it settles before running one, which is what lets a test ask for that answer.
fn verdict_of(root: &Path, version: Option<String>) -> Verdict {
    let Some(version) = version else {
        return Verdict::NotMeasured(
            "the linter is not installed here, so nothing was compared".to_owned(),
        );
    };
    let measured = warnings_per_crate(root).unwrap_or_else(|why| panic!("{why}"));
    if version != SEEDS_ARE_FOR {
        return Verdict::AnotherInstrument(format!(
            "the seeds were measured with «{SEEDS_ARE_FOR}» and this is «{version}»: another linter is another instrument. Write «{version}» in SEEDS_ARE_FOR and this table:\n{}",
            table_of(&measured)
        ));
    }
    let complaints = complaints_over(&measured, WARNINGS_TODAY);
    if complaints.is_empty() {
        Verdict::Quiet { linted: measured.len() }
    } else {
        Verdict::Louder(vec![format!(
            "{}; measured now:\n{}",
            complaints.join("; "),
            table_of(&measured)
        )])
    }
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
    match verdict_of(&root(), linter_version(&root())) {
        Verdict::Quiet { linted } => workspace::measured_against(
            linted,
            "crates linted over every target",
            WARNINGS_TODAY.len(),
            "seeded crates",
        ),
        Verdict::NotMeasured(why) => workspace::measured_nothing(&why),
        Verdict::AnotherInstrument(said) => panic!("{said}"),
        Verdict::Louder(said) => panic!("{}", said.join("; ")),
    }
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
    assert_eq!(
        named,
        crates_of(&root),
        "the table names every crate under crates/ and the shell, once"
    );
    let manifest = root.join("Cargo.toml");
    let with = linter(&root, Some(OsString::from("somewhere")), &manifest);
    assert!(
        with.get_envs().any(|(key, value)| key == "CARGO_TARGET_DIR" && value == Some("somewhere".as_ref())),
        "the caller's build directory is handed on"
    );
    assert!(
        linter(&root, None, &manifest).get_envs().all(|(key, _)| key != "CARGO_TARGET_DIR"),
        "and nothing is invented when the caller named none"
    );
}

/// A throwaway tree holding the named crate directories and nothing else.
fn a_tree_of_crates(label: &str, named: &[&str]) -> PathBuf {
    let root = std::env::temp_dir().join(format!("sailor-clippy-{label}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    for name in named {
        std::fs::create_dir_all(root.join("crates").join(name)).expect("a crate directory");
    }
    root
}

/// **A QUIET TREE IS NOT A WORKING CHECK.** Every crate is seeded at zero and
/// the linter has nothing to say, so the comparison has never returned a
/// complaint: it has only ever been handed an empty count. Here a throwaway
/// tree of two crates is counted against a linter run with a warning planted
/// in it, and the same comparison is asked of that.
#[test]
fn a_warning_planted_in_a_throwaway_tree_is_counted_and_complained_about() {
    let root = a_tree_of_crates("planted", &["flow", "ledger"]);
    let said = "\
crates/flow/src/lib.rs:12:5: warning: this `if` has identical blocks
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 0.14s
";
    let seeds = [("flow", 0usize), ("ledger", 0)];

    let counted = counted_over(&root, said);
    assert_eq!(counted.get("flow").copied(), Some(1), "the planted warning was not counted");
    assert_eq!(counted.get("ledger").copied(), Some(0), "a quiet crate must still have its row");

    let complaints = complaints_over(&counted, &seeds);
    assert_eq!(complaints.len(), 1, "{complaints:?}");
    assert!(
        complaints[0].contains("«flow» warns 1 times against a seed of Some(0)"),
        "the complaint does not name the crate that got louder: {}",
        complaints[0]
    );
    assert!(
        complaints_over(&counted_over(&root, "    Finished `dev` profile"), &seeds).is_empty(),
        "the control: a run with no warning in it must raise nothing"
    );

    // A seed left above the tree is the other direction, and it is a complaint
    // too: a table nobody lowered hides the next rise inside its slack.
    let stale = complaints_over(&counted_over(&root, ""), &[("flow", 3), ("ledger", 0)]);
    assert_eq!(stale.len(), 1, "{stale:?}");
    assert!(stale[0].contains("lower the seed to 0"), "{}", stale[0]);

    // And a table that does not name the tree's crates is a complaint of its
    // own: a crate dropped from it would otherwise warn freely.
    let unnamed = complaints_over(&counted_over(&root, ""), &[("flow", 0)]);
    assert!(
        unnamed.iter().any(|said| said.contains("the table names 1 crates and the tree holds 2")),
        "{unnamed:?}"
    );

    let _ = std::fs::remove_dir_all(&root);
}

/// **THE JUDGE MUST BE ABLE TO SAY IT DID NOT MEASURE.** With no linter to
/// answer, the count is not zero: there is no count. The verdict is asked for
/// that answer here, and it settles before a linter is ever run.
#[test]
fn with_no_linter_to_answer_the_judge_declares_it_measured_nothing() {
    assert_eq!(version_in(false, ""), None, "a cargo that refused the question answered nothing");
    assert_eq!(version_in(true, "clippy"), None, "a half-answer is not a version");
    assert_eq!(version_in(true, "clippy 0.1.98 (abcdef 2026-01-01)").as_deref(), Some("clippy 0.1.98"));

    let settled = verdict_of(&a_tree_of_crates("unlinted", &["flow"]), None);
    assert!(
        matches!(settled, Verdict::NotMeasured(_)),
        "with no linter the judge must declare it measured nothing, never a quiet tree: {settled:?}"
    );
    let Verdict::NotMeasured(why) = settled else { unreachable!() };
    assert!(why.contains("linter"), "whoever reads must be told what was missing: {why}");
}
