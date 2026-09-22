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

/// Every heading that opens lines to run or to confirm, named by the word it
/// leads with. What makes a section a section is that it holds bullets, not the
/// shape of its name: a rule that spelled the shape would be the runner's own
/// pattern written twice, and would go blind wherever the runner did — `E2`
/// renamed one character over is a section this reads and that one would not.
fn the_sections(manifest: &str) -> Vec<String> {
    let mut sections = Vec::new();
    let mut heading: Option<String> = None;
    for line in manifest.lines() {
        if let Some(text) = line.strip_prefix("## ") {
            heading = text
                .split_whitespace()
                .next()
                .map(|word| word.trim_end_matches('.').to_owned());
        } else if line.starts_with("- ") {
            if let Some(named) = heading.take() {
                sections.push(named);
            }
        }
    }
    sections
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

/// The road every real run takes, which the plan never walks: a flag declared
/// only there once stopped every other invocation while these stayed green.
/// Saying something is not enough — a runner that listed nothing still would —
/// so a line the plan holds has to be named back.
#[test]
fn the_runner_still_answers_without_being_asked_for_its_plan() {
    let root = root();
    let said = Command::new("sh")
        .arg(THE_RUNNER)
        .args(["--list", "--base", "HEAD"])
        .current_dir(&root)
        .output()
        .expect("the runner answers");
    assert!(
        said.status.success(),
        "the runner refused the road every run takes: {}",
        String::from_utf8_lossy(&said.stderr)
    );
    let listed = String::from_utf8_lossy(&said.stderr).into_owned();
    let planned = Command::new("sh")
        .arg(THE_RUNNER)
        .args(["--plan", "--base", "HEAD"])
        .current_dir(&root)
        .output()
        .expect("the runner answers");
    let planned = String::from_utf8_lossy(&planned.stdout).into_owned();
    let first = planned
        .lines()
        .filter_map(|line| {
            let mut fields = line.split('\t');
            let letter = fields.next()?;
            let (kind, text) = (fields.next()?, fields.next()?);
            (letter == "A" && kind == "command").then(|| text.to_owned())
        })
        .next()
        .expect("letter A holds a command to run");
    assert!(
        listed.contains(&first),
        "the runner said something without naming «{first}», which its own plan holds: it \
         answered the road every run takes and listed nothing"
    );
}

/// The body of the runner's own answer to «does this letter apply», so that
/// what a check holds it to is the text that decides, never a copy of it.
fn decides(runner: &str) -> String {
    runner
        .split("applies()")
        .nth(1)
        .and_then(|rest| rest.split_once('}'))
        .map(|(body, _)| body.to_owned())
        .expect("the runner says which letters apply")
}

/// Every quoted word a letter's own arm holds, whatever command it is spelled
/// with. Reading the arm by the exact words `grep -E -q` made the check blind
/// to `grep -Eq`, and narrowing the words by how a pattern tends to look would
/// go blind again at the first plain one: nothing here is told what a pattern
/// is, and it is enough that one of them names something real.
fn the_matches(runner: &str) -> Vec<(String, Vec<String>)> {
    decides(runner)
        .lines()
        .filter_map(|line| {
            let (letter, rest) = line.trim().split_once(')')?;
            let quoted = rest
                .split('\'')
                .skip(1)
                .step_by(2)
                .map(str::to_owned)
                .collect();
            Some((letter.to_owned(), quoted))
        })
        .collect()
}

/// A letter is dead when nothing in the tree could ever make it apply, and as
/// dead when its arm names nothing to look for. `A` answers to everything and
/// is written with no word of its own, so it is the one letter exempt.
#[test]
fn every_letter_the_runner_decides_could_be_reached_by_this_tree() {
    let root = root();
    let runner = std::fs::read_to_string(root.join(THE_RUNNER)).expect("the runner");
    let tracked = Command::new("git")
        .args(["ls-files"])
        .current_dir(&root)
        .output()
        .expect("git lists the tree");
    let tracked = String::from_utf8_lossy(&tracked.stdout).into_owned();
    let arms = the_matches(&runner);
    for letter in the_plan(&root) {
        // `A` is every run's, written with no word of its own, and the letters
        // decided elsewhere are held by the check that names them.
        if letter == "A" || DECIDED_ELSEWHERE.iter().any(|(named, _)| *named == letter) {
            continue;
        }
        let words = arms
            .iter()
            .find(|(named, _)| *named == letter)
            .map(|(_, words)| words.clone())
            .unwrap_or_default();
        assert!(
            words.iter().any(|word| matches_a_file(word, &tracked)),
            "«{letter}» is decided by {words:?}, and no file this tree tracks answers to any of \
             them: its lines are read and then run for nobody"
        );
    }
}

/// Whether the runner's own word, read by the tool the runner reads it with,
/// names a file this tree holds.
fn matches_a_file(word: &str, tracked: &str) -> bool {
    use std::io::Write;
    let Ok(mut child) = Command::new("grep")
        .args(["-E", "-q", word])
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
    else {
        return false;
    };
    let wrote = child
        .stdin
        .take()
        .expect("the file list goes in")
        .write_all(tracked.as_bytes());
    let ended = child.wait().expect("grep answers");
    wrote.is_ok() && ended.success()
}

#[test]
fn every_letter_the_runner_reads_is_one_it_can_decide() {
    let root = root();
    let runner = std::fs::read_to_string(root.join(THE_RUNNER)).expect("the runner");
    let decides = decides(&runner);
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
