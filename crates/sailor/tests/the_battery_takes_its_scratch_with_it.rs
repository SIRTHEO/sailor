//! **THE BATTERY WROTE NO END FOR WHAT IT MADE.** The tests name the temporary
//! directory in 278 places across 200 files, and inherited it is the one every
//! session on this machine shares: 16.032 directories weighing 807 MB had piled
//! up there by 22/09/2026, one set per run and per pid. Nobody had to be at
//! fault — a run simply took a place it did not own and left.
//!
//! The gates hand the battery a root of their own instead, and take it away on
//! the way out. This reads the three lines out of the script itself and *runs*
//! them: a test that only looked for the words would pass on a trap that fires
//! on no signal, or on a root that is made and never exported.

use std::path::{Path, PathBuf};
use std::process::Command;

fn gates_script() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("the crate sits two levels under the root")
        .join("scripts/run-gates.sh")
}

/// The lines the script gives the battery its root with, taken from the script
/// and not copied here: a copy would keep passing after the script changed.
fn how_the_root_is_given(text: &str) -> String {
    let opened = text
        .find("gates_tmp=$(mktemp")
        .expect("the gates make a temporary root of their own");
    let closed = text[opened..]
        .find("export TMPDIR")
        .expect("the gates export the root they made")
        + opened
        + "export TMPDIR".len();
    text[opened..closed].to_owned()
}

/// Run those lines, then a body, under a parent temporary directory of this
/// test's own — so what the script does is measured, never described.
fn under_the_gates_root(label: &str, body: &str) -> (String, PathBuf) {
    let text = std::fs::read_to_string(gates_script()).expect("the gates script is readable");
    // The name carries the run *and* the test: these two run side by side, and
    // a shared name would have each sweeping the other's parent away.
    let parent =
        std::env::temp_dir().join(format!("gates-root-{label}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&parent);
    std::fs::create_dir_all(&parent).expect("a parent for the root");
    let output = Command::new("sh")
        .arg("-c")
        .arg(format!("{}\n{body}", how_the_root_is_given(&text)))
        .env("TMPDIR", &parent)
        .output()
        .expect("the lines run");
    assert!(
        output.status.success(),
        "the gates' own lines failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    (
        String::from_utf8_lossy(&output.stdout).trim().to_owned(),
        parent,
    )
}

#[test]
fn the_battery_is_handed_a_root_of_the_run_and_not_the_shared_one() {
    let (said, parent) = under_the_gates_root("handed", "printf '%s' \"$TMPDIR\"");
    assert!(
        said.starts_with(&parent.display().to_string()) && said != parent.display().to_string(),
        "the battery must be handed a root of this run's own, under {}, and it was handed {said}",
        parent.display()
    );
    let _ = std::fs::remove_dir_all(&parent);
}

/// The one that matters: what a test writes into that root is gone afterwards.
/// Without the trap this passes the line above and still leaves the mess.
#[test]
fn what_the_battery_leaves_in_it_goes_when_the_run_goes() {
    let (said, parent) = under_the_gates_root(
        "swept",
        "mkdir -p \"$TMPDIR/prova-something-1\"; printf '%s' \"$TMPDIR\"",
    );
    assert!(
        !Path::new(&said).exists(),
        "the root {said} outlived the run that made it: the battery's scratch is piling up again"
    );
    assert_eq!(
        std::fs::read_dir(&parent)
            .expect("the parent is readable")
            .count(),
        0,
        "nothing of the run may be left beside the root either, in {}",
        parent.display()
    );
    let _ = std::fs::remove_dir_all(&parent);
}
