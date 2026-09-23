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

/// Headings that open prose instead of lines to run, and why.
const HOLDS_NO_LINES_TO_RUN: &[(&str, &str)] = &[(
    "The six the trunk requires, and the day each said no",
    "a table of receipts, not lines a run walks",
)];

/// A heading of the manifest: the word it leads with, which is the letter the
/// runner reads; the whole of it, which says what its lines are for; and where
/// in the file the lines it holds are.
struct Section {
    letter: String,
    says: String,
    holds: Vec<usize>,
}

/// Every heading that opens lines to run or to confirm. What makes a section a
/// section is that it holds lines at all — never the shape of its name, and never
/// the mark those lines open with. Either would be the runner's own pattern
/// written twice and would go blind wherever the runner did: `E2` renamed one
/// character over, and its bullets respelled `*`, are sections this reads and
/// that one would not.
fn the_sections(manifest: &str) -> Vec<Section> {
    let mut sections: Vec<Section> = Vec::new();
    let mut open = false;
    for (index, line) in manifest.lines().enumerate() {
        if let Some(says) = line.strip_prefix("## ") {
            open = !HOLDS_NO_LINES_TO_RUN
                .iter()
                .any(|(named, _)| *named == says);
            if open {
                sections.push(Section {
                    letter: says
                        .split_whitespace()
                        .next()
                        .unwrap_or(says)
                        .trim_end_matches('.')
                        .to_owned(),
                    says: says.to_owned(),
                    holds: Vec::new(),
                });
            }
        } else if open && !line.trim().is_empty() {
            if let Some(last) = sections.last_mut() {
                last.holds.push(index);
            }
        }
    }
    sections.retain(|section| !section.holds.is_empty());
    sections
}

/// A directory of this run's own, so neither two suites at once nor two
/// checks in this one ever share one.
fn scratch(label: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("sailor-gates-{label}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("the scratch directory");
    dir
}

/// Everything the runner reads out of a manifest, as it prints it before it
/// chooses or runs anything. `None` when it refuses that manifest outright,
/// which is one more way of having noticed.
fn the_plan_of(root: &Path, manifest: &Path) -> Option<String> {
    let said = Command::new("sh")
        .arg(THE_RUNNER)
        .args(["--plan", "--base", "HEAD", "--manifest"])
        .arg(manifest)
        .current_dir(root)
        .output()
        .expect("the runner answers");
    said.status
        .success()
        .then(|| String::from_utf8_lossy(&said.stdout).into_owned())
}

/// The letters the runner read out of the manifest this tree ships.
fn the_plan(root: &Path) -> Vec<String> {
    let said = the_plan_of(root, &root.join(THE_GATES)).expect("the runner prints its plan");
    let mut letters: Vec<String> = said
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
    for section in the_sections(&manifest) {
        assert!(
            plan.contains(&section.letter),
            "«{}» is a section of {THE_GATES} and the runner's plan holds none of its lines: \
             {plan:?}",
            section.letter
        );
    }
}

/// Two sections under one letter read exactly as one section read twice: the
/// plan holds the letter either way, and the second section's lines are run,
/// or not run, by the first section's arm.
#[test]
fn no_two_sections_of_the_gates_carry_the_same_letter() {
    let root = root();
    let manifest = std::fs::read_to_string(root.join(THE_GATES)).expect("the gates");
    let mut seen: Vec<Section> = Vec::new();
    for section in the_sections(&manifest) {
        assert!(
            !seen.iter().any(|first| first.letter == section.letter),
            "«{}» opens two sections of {THE_GATES}: «{}» and «{}». The runner decides \
             the letter once, so the second section's lines answer to the first one's arm",
            section.letter,
            seen.iter()
                .find(|first| first.letter == section.letter)
                .map(|first| first.says.as_str())
                .unwrap_or_default(),
            section.says
        );
        seen.push(section);
    }
}

/// The runner is its own oracle here: the manifest is handed back one line
/// short and the plan has to come out different. Nothing tells this what a
/// line of a section looks like, so no mark one opens with can hide it, and
/// no rule of the runner's is written a second time to be disagreed with.
#[test]
fn every_line_a_section_holds_reaches_the_runner() {
    let root = root();
    let manifest = std::fs::read_to_string(root.join(THE_GATES)).expect("the gates");
    let whole = the_plan_of(&root, &root.join(THE_GATES)).expect("the runner prints its plan");
    let scratch = scratch("shortened");
    let shortened = scratch.join("gates.md");
    let lines: Vec<&str> = manifest.lines().collect();
    for section in the_sections(&manifest) {
        for index in section.holds {
            let left: Vec<&str> = lines
                .iter()
                .enumerate()
                .filter_map(|(at, line)| (at != index).then_some(*line))
                .collect();
            std::fs::write(&shortened, left.join("\n")).expect("the shortened manifest");
            assert!(
                the_plan_of(&root, &shortened).as_deref() != Some(whole.as_str()),
                "«{}» of {THE_GATES} holds «{}», and the runner's plan is the same without \
                 it: the line is read by nobody",
                section.letter,
                lines[index]
            );
        }
    }
    let _ = std::fs::remove_dir_all(&scratch);
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

/// The shell the runner is written for, named outright: what road a `sh` on
/// the path would be is a fact about this machine.
const A_SHELL: &str = "/bin/sh";

/// The two places a POSIX system keeps the tools an arm reaches for. Written
/// out for the same reason.
const THE_ONLY_PATH: &str = "/usr/bin:/bin";

/// The line the runner's own answer to «does this letter apply» opens with.
const DECIDES: &str = "applies() {";

/// A tracked file no section of the manifest is about, so every letter but `A`
/// has to answer no to it.
const A_FILE_NO_SECTION_IS_ABOUT: &str = "CODE_OF_CONDUCT.md";

/// The body of the runner's own answer to «does this letter apply», from the
/// line that opens it to the line that closes it, so what a check holds it to
/// is the text that decides, never a copy of it.
fn decides(runner: &str) -> String {
    let lines: Vec<&str> = runner.lines().collect();
    let opened = lines
        .iter()
        .position(|line| *line == DECIDES)
        .expect("the runner says which letters apply")
        + 1;
    let closed = lines[opened..]
        .iter()
        .position(|line| *line == "}")
        .expect("the runner closes what it opened");
    lines[opened..opened + closed].join("\n")
}

/// The runner's own arms, lifted out of the script and asked the one question
/// that decides anything: this file changed — are this letter's lines yours?
/// They are handed one path and nothing else. No repository stands under them,
/// none of this machine's environment reaches them, and the directory they run
/// in is an empty one of this run's own: what an arm can read is what it was
/// given, and how it is spelled is its own business.
fn answers_for(arms: &str, here: &Path, letter: &str, changed: &str) -> bool {
    Command::new(A_SHELL)
        .args([
            "-c",
            &format!("{DECIDES}\n{arms}\n}}\napplies \"$1\""),
            "gates",
            letter,
        ])
        .env_clear()
        .env("PATH", THE_ONLY_PATH)
        .env("changed", changed)
        .current_dir(here)
        .output()
        .expect("the arm answers")
        .status
        .success()
}

/// What a section's heading says its letter is for: the names it spells out
/// between backticks, each broken into the parts a path would carry.
fn what_it_names(says: &str) -> Vec<Vec<String>> {
    says.split('`')
        .skip(1)
        .step_by(2)
        .map(|name| name.split("::").map(str::to_owned).collect())
        .collect()
}

/// The words a heading may carry beside the names it spells out. A heading is
/// a declaration and not prose: a trigger written bare is one no check reads,
/// and `the sweep` in prose left `E2`'s arm free to drop the word while every
/// check stayed green. Widening this list is a decision somebody makes.
const THE_CONNECTIVES: &[&str] = &["When", "a", "or", "changes"];

/// A heading's own words: what falls outside the backticks, less the letter it
/// opens with and the commas that separate the names.
fn the_bare_words(says: &str) -> Vec<String> {
    says.split('`')
        .step_by(2)
        .flat_map(str::split_whitespace)
        .skip(1)
        .map(|word| word.trim_matches(',').to_owned())
        .filter(|word| !word.is_empty())
        .collect()
}

/// A trigger is held only where it is spelled out, so a heading may not name
/// one anywhere else. `E2` said «the sweep» in prose, the check read only what
/// the backticks held, and deleting that word from the arm stopped the sweep's
/// two files running a line while all six checks passed. `A` answers to
/// everything and the letters decided elsewhere have no arm here, so neither
/// has a trigger to declare.
#[test]
fn a_section_spells_out_what_arms_it_and_leaves_nothing_in_prose() {
    let root = root();
    let manifest = std::fs::read_to_string(root.join(THE_GATES)).expect("the gates");
    for section in the_sections(&manifest) {
        let letter = section.letter.as_str();
        if letter == "A" || DECIDED_ELSEWHERE.iter().any(|(named, _)| *named == letter) {
            continue;
        }
        for word in the_bare_words(&section.says) {
            assert!(
                THE_CONNECTIVES.contains(&word.as_str()),
                "«{letter}» opens «{}» and says «{word}» outside backticks. What a letter is \
                 for is held only between them, so a trigger named in prose arms nothing: \
                 spell it out, or drop the claim. Beside its names a heading may say {THE_CONNECTIVES:?}",
                section.says
            );
        }
    }
}

/// The first file this tree tracks whose path carries every part of a name, in
/// the order the name spells them.
fn a_file_named(parts: &[String], tracked: &[String]) -> Option<String> {
    tracked
        .iter()
        .find(|path| {
            let mut rest = path.as_str();
            parts.iter().all(|part| match rest.find(part.as_str()) {
                Some(at) => {
                    rest = &rest[at + part.len()..];
                    true
                }
                None => false,
            })
        })
        .cloned()
}

/// Every file this tree tracks, in the order git lists them.
fn tracked_by(root: &Path) -> Vec<String> {
    let said = Command::new("git")
        .args(["ls-files"])
        .current_dir(root)
        .output()
        .expect("git lists the tree");
    String::from_utf8_lossy(&said.stdout)
        .lines()
        .map(str::to_owned)
        .collect()
}

/// A letter is dead when nothing in this tree could make it apply, and as dead
/// when it applies to everything. Neither is read out of the words its arm is
/// spelled with: the arm is run, over the files its own section says it is
/// for, and over one file no section is about. `A` is every run's and answers
/// to everything, so it is the one letter exempt; the letters decided
/// elsewhere are held by the check that names them.
#[test]
fn every_letter_the_runner_decides_could_be_reached_by_this_tree() {
    let root = root();
    let runner = std::fs::read_to_string(root.join(THE_RUNNER)).expect("the runner");
    let manifest = std::fs::read_to_string(root.join(THE_GATES)).expect("the gates");
    let arms = decides(&runner);
    let tracked = tracked_by(&root);
    let here = scratch("arms");
    assert!(
        tracked.iter().any(|path| path == A_FILE_NO_SECTION_IS_ABOUT),
        "{A_FILE_NO_SECTION_IS_ABOUT} is the file every letter has to answer no to, and this \
         tree does not track it"
    );
    for section in the_sections(&manifest) {
        let letter = section.letter.as_str();
        if letter == "A" || DECIDED_ELSEWHERE.iter().any(|(named, _)| *named == letter) {
            continue;
        }
        let names = what_it_names(&section.says);
        assert!(
            !names.is_empty(),
            "«{letter}» opens «{}» and names nothing between backticks: nothing says what \
             its arm is supposed to answer yes to",
            section.says
        );
        for parts in names {
            let name = parts.join("::");
            let touched = a_file_named(&parts, &tracked).unwrap_or_else(|| {
                panic!("«{letter}» is the section for `{name}`, and this tree tracks no such file")
            });
            assert!(
                answers_for(&arms, &here, letter, &touched),
                "«{letter}» is the section for `{name}`, and the runner answers that {touched} \
                 is none of its business: its lines are read and then run for nobody"
            );
        }
        assert!(
            !answers_for(&arms, &here, letter, A_FILE_NO_SECTION_IS_ABOUT),
            "«{letter}» applies to {A_FILE_NO_SECTION_IS_ABOUT}, which no section is about: an \
             arm that answers yes to everything decides nothing"
        );
    }
    let _ = std::fs::remove_dir_all(&here);
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
