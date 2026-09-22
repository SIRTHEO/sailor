//! **A SECTION THE RUNNER CANNOT SEE IS A GATE THAT NEVER SAYS NO.** `E2.` was
//! added to `docs/gates.md` with a digit where the runner expected the dot, so
//! its three lines fell out of every plan. Runs reported the letters they had
//! and nothing said one was missing: a change to the code that section names
//! went to the trunk measured by nobody.

use std::path::{Path, PathBuf};
use std::process::Command;

const THE_GATES: &str = "docs/gates.md";
const THE_RUNNER: &str = "scripts/run-gates.sh";

/// A letter whose lines no local run can decide, and why. `F` is held by the
/// delivery flows, which know the commit a review was pinned to and what the
/// combined trunk ran; a checkout on its own knows neither.
const DECIDED_ELSEWHERE: &[(&str, &str)] = &[("F", "the delivery flows hold it")];

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .expect("the workspace root")
}

/// The letter of every section heading, `## A.` through `## E2.`, in the order
/// the manifest writes them. A heading that names no letter opens the prose
/// that follows the sections and closes the list.
fn the_sections(manifest: &str) -> Vec<String> {
    manifest
        .lines()
        .filter_map(|line| line.strip_prefix("## "))
        .filter_map(|heading| heading.split('.').next())
        .filter(|letter| {
            let mut characters = letter.chars();
            characters.next().is_some_and(|first| first.is_ascii_uppercase())
                && characters.all(|rest| rest.is_ascii_digit())
        })
        .map(str::to_owned)
        .collect()
}

/// The letters the runner read out of the manifest, as it prints them before
/// it chooses or runs anything.
fn the_plan(root: &Path) -> Vec<String> {
    let said = Command::new("sh")
        .arg(THE_RUNNER)
        .args(["--plan", "--base", "HEAD"])
        .current_dir(root)
        .output()
        .expect("the runner answers");
    assert!(
        said.status.success(),
        "the runner refused to print its plan: {}",
        String::from_utf8_lossy(&said.stderr)
    );
    let mut letters: Vec<String> = String::from_utf8_lossy(&said.stdout)
        .lines()
        .filter_map(|line| line.split('\t').next())
        .map(str::to_owned)
        .collect();
    letters.sort();
    letters.dedup();
    letters
}

#[test]
fn the_runner_reads_every_section_the_manifest_declares() {
    let root = root();
    let manifest = std::fs::read_to_string(root.join(THE_GATES)).expect("the gates");
    let plan = the_plan(&root);
    for letter in the_sections(&manifest) {
        assert!(
            plan.contains(&letter),
            "«{letter}» is a section of {THE_GATES} and the runner's plan holds none of its \
             lines: {plan:?}"
        );
    }
}

#[test]
fn every_letter_the_runner_reads_is_one_it_can_decide() {
    let root = root();
    let runner = std::fs::read_to_string(root.join(THE_RUNNER)).expect("the runner");
    let decides = runner
        .split("applies()")
        .nth(1)
        .and_then(|rest| rest.split_once('}'))
        .map(|(body, _)| body.to_owned())
        .expect("the runner says which letters apply");
    for letter in the_plan(&root) {
        if let Some((_, why)) = DECIDED_ELSEWHERE.iter().find(|(named, _)| *named == letter) {
            assert!(
                !decides.contains(&format!("{letter})")),
                "«{letter}» is written as decided elsewhere ({why}) and the runner decides it too"
            );
            continue;
        }
        assert!(
            decides.contains(&format!("{letter})")),
            "the runner reads «{letter}» out of {THE_GATES} and then never asks whether it \
             applies, so its lines run for nobody"
        );
    }
}
