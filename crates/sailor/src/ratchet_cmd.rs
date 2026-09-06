//! `sailor ratchet`: the judges that read the sources, run on a clean HEAD
//! with only this tree's own changes laid over it.
//!
//! **THE RITE WAS DONE BY HAND MORE THAN TWENTY TIMES IN ONE NIGHT**, and twice
//! the seeds were taken over another session's uncommitted file. A measurement
//! on the working tree measures whoever else is writing in it.

use std::path::{Path, PathBuf};
use std::process::Command;



pub const USAGE: &[crate::Form] = &[crate::Form {
    form: "sailor ratchet [--only <judge>]",
    says_key: "cli.ratchet.says",
}];

/// A judge is a test that reads the sources, and every one of them finds them
/// the same way. Found, not listed: a list here would stop naming the judge
/// somebody adds next month.
const READS_THE_SOURCES: &str = "CARGO_MANIFEST_DIR";

pub fn run(args: &[String]) -> i32 {
    let only = match parse_options(args) {
        Ok(only) => only,
        Err(message) => {
            eprintln!("sailor ratchet: {message}");
            return 2;
        }
    };
    match measured(&only) {
        Ok(true) => 0,
        Ok(false) => 1,
        Err(message) => {
            eprintln!("sailor ratchet: {message}");
            2
        }
    }
}

fn parse_options(args: &[String]) -> Result<Option<String>, String> {
    let mut only = None;
    let mut rest = args.iter();
    while let Some(word) = rest.next() {
        match word.as_str() {
            "--only" => {
                only = Some(rest.next().cloned().ok_or_else(|| {
                    catalogue::say("cli.option_wants_a_value", &[("option", "--only")])
                })?)
            }
            other => {
                return Err(catalogue::say(
                    "cli.ratchet.unknown_option",
                    &[("option", other), ("usage", USAGE[0].form)],
                ))
            }
        }
    }
    Ok(only)
}

/// One judge: the crate that holds it and the test's name.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Judge {
    pub package: String,
    pub test: String,
}

/// The judges under `crates/*/tests`: the tests that read the sources.
pub fn judges_in(root: &Path) -> Vec<Judge> {
    let mut found = Vec::new();
    let Ok(crates) = std::fs::read_dir(root.join("crates")) else {
        return found;
    };
    for package in crates.flatten() {
        let Ok(tests) = std::fs::read_dir(package.path().join("tests")) else {
            continue;
        };
        for file in tests.flatten() {
            let path = file.path();
            if path.extension().is_some_and(|kind| kind == "rs")
                && std::fs::read_to_string(&path).is_ok_and(|text| text.contains(READS_THE_SOURCES))
            {
                found.push(Judge {
                    package: package.file_name().to_string_lossy().into_owned(),
                    test: path
                        .file_stem()
                        .map(|stem| stem.to_string_lossy().into_owned())
                        .unwrap_or_default(),
                });
            }
        }
    }
    found.sort_by(|a, b| (&a.package, &a.test).cmp(&(&b.package, &b.test)));
    found
}

/// What one path the change touches becomes in the measured tree.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Change {
    LaidOver(PathBuf),
    TakenAway(PathBuf),
}

/// The two columns are the index and the working tree. A removal shows in
/// either: staged, the tree column agrees and stays blank; unstaged, the tree
/// column carries the `D` itself — see fault 99.
fn says_removed(state: &str) -> bool {
    let mut columns = state.chars();
    let staged = columns.next();
    let in_the_tree = columns.next();
    in_the_tree == Some('D') || (staged == Some('D') && in_the_tree == Some(' '))
}

/// What this tree changed against HEAD: files to lay over the archive, and
/// files to take out of it. A new file joins only where HEAD already has a
/// directory for it — a whole untracked tree (`node_modules/`, a scratch
/// folder) is not a change. `--no-renames` splits a rename into the removal
/// and the addition it is made of; `-z` keeps a path with a space in it whole.
fn changed_here(root: &Path, archive: &Path) -> Result<Vec<Change>, String> {
    let said = Command::new("git")
        .arg("-C")
        .arg(root)
        .args(["status", "--porcelain", "--untracked-files=all", "--no-renames", "-z"])
        .output()
        .map_err(|error| format!("git status: {error}"))?;
    if !said.status.success() {
        return Err(catalogue::say("cli.ratchet.not_a_repository", &[]));
    }
    let mut changes = Vec::new();
    for record in String::from_utf8_lossy(&said.stdout).split('\0') {
        if record.len() < 4 {
            continue;
        }
        let (state, path) = record.split_at(3);
        let path = PathBuf::from(path);
        if says_removed(state) {
            changes.push(Change::TakenAway(path));
        } else if state != "?? "
            || path.parent().is_some_and(|parent| archive.join(parent).is_dir())
        {
            changes.push(Change::LaidOver(path));
        }
    }
    Ok(changes)
}

/// Takes the file out of the measured tree, and with it every directory it
/// leaves empty: a judge that walks the tree reads a directory still standing
/// as a place the change kept.
fn take_out_of(tree: &Path, relative: &Path) -> Result<(), String> {
    let path = tree.join(relative);
    if path.is_symlink() || path.is_file() {
        std::fs::remove_file(&path).map_err(|error| format!("{}: {error}", path.display()))?;
    }
    let mut emptied = path.parent().map(Path::to_path_buf);
    while let Some(directory) = emptied.filter(|at| at.starts_with(tree) && at != tree) {
        if std::fs::remove_dir(&directory).is_err() {
            break;
        }
        emptied = directory.parent().map(Path::to_path_buf);
    }
    Ok(())
}

/// How one judge came back. Not measured and no receipt are two states and
/// not one: declaring an empty oracle is evidence, handing in nothing is the
/// absence of it, and the design begins by refusing to read one as the other.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Verdict {
    Green,
    Red,
    NotMeasured,
    NoReceipt,
}

/// What a judge's own words prove about the perimeter it walked. **WHAT THIS
/// CANNOT CATCH**: a judge printing a perimeter it never walked is green here
/// and nothing below can tell. The gate demands evidence, it does not audit it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Receipt {
    Walked { perimeter: usize, oracle: Option<usize> },
    Absent,
    Incomplete,
}

/// The first number, when it is one and not zero: a walk of nothing and an
/// oracle of nothing are the emptiness this line stops.
fn a_standing_count(text: &str) -> Option<usize> {
    text.split_whitespace().next()?.parse::<usize>().ok().filter(|held| *held > 0)
}

/// Read off the line the judge began, not from anywhere in the text: a judge
/// quoting the word in an assertion never walked a tree. Exiting zero says the
/// process ended, and only this line says the tree was opened.
fn receipt_in(said: &str) -> Receipt {
    let found = said.lines().map(str::trim).find(|line| line.starts_with(workspace::MEASURED));
    let Some(line) = found else {
        return Receipt::Absent;
    };
    let rest = &line[workspace::MEASURED.len()..];
    let Some(perimeter) = a_standing_count(rest) else {
        return Receipt::Incomplete;
    };
    match rest.split_once(workspace::AGAINST) {
        None => Receipt::Walked { perimeter, oracle: None },
        Some((_, weighed)) => match a_standing_count(weighed) {
            Some(oracle) => Receipt::Walked { perimeter, oracle: Some(oracle) },
            None => Receipt::Incomplete,
        },
    }
}

/// Passing is not the same as having measured, and the judge is the only one
/// that can tell: it says so on its own output, and this reads it there.
pub fn verdict_of(passed: bool, said: &str) -> Verdict {
    if !passed {
        Verdict::Red
    } else if said.contains(workspace::MEASURED_NOTHING) {
        Verdict::NotMeasured
    } else if matches!(receipt_in(said), Receipt::Walked { .. }) {
        Verdict::Green
    } else {
        Verdict::NoReceipt
    }
}

/// How many judges hand in no receipt today. **It can only fall**, and no run
/// is called clean while it stands above zero.
const NO_RECEIPT_TODAY: usize = 0;

/// The tally of the run, kept apart so a green count never absorbs the others.
#[derive(Debug, Default, PartialEq, Eq)]
struct Verdicts {
    green: usize,
    red: usize,
    not_measured: usize,
    no_receipt: usize,
}

impl Verdicts {
    fn saw(&mut self, verdict: Verdict) {
        match verdict {
            Verdict::Green => self.green += 1,
            Verdict::Red => self.red += 1,
            Verdict::NotMeasured => self.not_measured += 1,
            Verdict::NoReceipt => self.no_receipt += 1,
        }
    }

    /// All four numbers, with each key written where the scan can see it.
    fn closing_line(&self) -> String {
        let held = [
            ("green", self.green.to_string()),
            ("red", self.red.to_string()),
            ("not_measured", self.not_measured.to_string()),
            ("no_receipt", self.no_receipt.to_string()),
        ];
        let said: Vec<(&str, &str)> =
            held.iter().map(|(name, value)| (*name, value.as_str())).collect();
        if self.red > 0 {
            catalogue::say("cli.ratchet.some_red", &said)
        } else if self.not_measured > 0 || self.no_receipt > 0 {
            catalogue::say("cli.ratchet.nothing_red_but_unmeasured", &said)
        } else {
            catalogue::say("cli.ratchet.all_green", &said)
        }
    }
}

/// What one judge handed the gate. It weighs nothing else, and an exit code
/// alone is not enough.
pub struct Handed<'a> {
    pub judge: &'a str,
    pub passed: bool,
    pub said: &'a str,
}

/// The gate's public verdict over a run.
#[derive(Debug, Default)]
pub struct Gate {
    counted: Verdicts,
}

impl Gate {
    pub fn over(handed: &[Handed]) -> Self {
        let mut gate = Gate::default();
        for one in handed {
            gate.counted.saw(verdict_of(one.passed, one.said));
        }
        gate
    }

    pub fn green(&self) -> usize {
        self.counted.green
    }

    pub fn red(&self) -> usize {
        self.counted.red
    }

    pub fn measured_nothing(&self) -> usize {
        self.counted.not_measured
    }

    /// Judges that proved no perimeter: receipt absent or incomplete.
    pub fn unmeasured(&self) -> usize {
        self.counted.no_receipt
    }

    pub fn closing_line(&self) -> String {
        self.counted.closing_line()
    }

    /// A red stops the run, and so does a judge that proved no perimeter.
    /// `silent_allowed` is the declared debt, passed at the call rather than
    /// hidden here: a run weighed on its own terms is asked for zero.
    pub fn lets_through(&self, silent_allowed: usize) -> bool {
        self.counted.red == 0 && self.counted.no_receipt <= silent_allowed
    }

    /// The seed sits above what this run holds, so the debt can be paid now.
    /// Only over a whole run: one judge alone says nothing about the rest.
    fn seed_may_fall_to(&self, whole_run: bool, seed: usize) -> Option<usize> {
        (whole_run && self.counted.no_receipt < seed).then_some(self.counted.no_receipt)
    }
}

/// How far the change moved the archive, in the two directions a person needs
/// told apart before trusting the verdict.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Overlay {
    pub laid_over: usize,
    pub taken_away: usize,
}

/// A clean copy of HEAD with this tree's changes laid over it, tracked by a
/// repository of its own.
pub fn clean_tree_with_changes(root: &Path, into: &Path) -> Result<Overlay, String> {
    let _ = std::fs::remove_dir_all(into);
    std::fs::create_dir_all(into).map_err(|error| format!("{}: {error}", into.display()))?;
    let archive = Command::new("git")
        .arg("-C")
        .arg(root)
        .args(["archive", "HEAD"])
        .output()
        .map_err(|error| format!("git archive: {error}"))?;
    if !archive.status.success() {
        return Err(catalogue::say("cli.ratchet.archive_failed", &[]));
    }
    let mut untar = Command::new("tar")
        .args(["-x", "-C"])
        .arg(into)
        .stdin(std::process::Stdio::piped())
        .spawn()
        .map_err(|error| format!("tar: {error}"))?;
    {
        use std::io::Write;
        let mut stdin = untar
            .stdin
            .take()
            .ok_or_else(|| catalogue::say("cli.ratchet.archive_failed", &[]))?;
        stdin
            .write_all(&archive.stdout)
            .map_err(|error| format!("tar: {error}"))?;
    }
    let status = untar.wait().map_err(|error| format!("tar: {error}"))?;
    if !status.success() {
        return Err(catalogue::say("cli.ratchet.archive_failed", &[]));
    }
    let mut moved = Overlay::default();
    for change in changed_here(root, into)? {
        match change {
            Change::LaidOver(relative) => {
                println!("  + {}", relative.display());
                let from = root.join(&relative);
                let to = into.join(&relative);
                if let Some(parent) = to.parent() {
                    std::fs::create_dir_all(parent)
                        .map_err(|error| format!("{}: {error}", parent.display()))?;
                }
                std::fs::copy(&from, &to).map_err(|error| format!("{}: {error}", from.display()))?;
                moved.laid_over += 1;
            }
            Change::TakenAway(relative) => {
                println!("  - {}", relative.display());
                take_out_of(into, &relative)?;
                moved.taken_away += 1;
            }
        }
    }
    tracked_by_a_repository_of_its_own(into)?;
    Ok(moved)
}

/// **A JUDGE THAT ASKS GIT MUST HAVE SOMETHING TO ASK.** `git archive` carries
/// the files and not the `.git`, so the guards that read what is tracked found
/// nothing here. An index is all they read; the person's own settings are shut
/// out, because what the gate sees must not depend on whose machine it runs on.
/// `add --all` is safe exactly here — a copy under `target/`, remade each run.
fn tracked_by_a_repository_of_its_own(into: &Path) -> Result<(), String> {
    let git = |args: &[&str]| -> Result<(), String> {
        let out = Command::new("git")
            .arg("-C")
            .arg(into)
            .args(args)
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .output()
            .map_err(|error| format!("git {}: {error}", args.join(" ")))?;
        if out.status.success() {
            return Ok(());
        }
        Err(format!("git {}: {}", args.join(" "), String::from_utf8_lossy(&out.stderr).trim()))
    };
    // An empty template: no sample hook of this machine is copied in, for the
    // same reason the global configuration is shut out.
    git(&["init", "--quiet", "--template="])?;
    git(&["add", "--all"])
}

/// The tree the command is run in, when it is one: a checkout other than the
/// sources in service — a worktree, a clone — is measured for itself, and
/// two checkouts never share one `target/ratchet-tree`.
pub(crate) fn root_to_measure() -> Result<PathBuf, String> {
    match std::env::current_dir().ok().and_then(|here| workspace::tree_around(&here)) {
        Some(tree) => Ok(tree),
        None => crate::release_cmd::sources_root(),
    }
}

fn measured(only: &Option<String>) -> Result<bool, String> {
    let root = root_to_measure()?;
    let clean = root.join("target").join("ratchet-tree");
    let moved = clean_tree_with_changes(&root, &clean)?;
    let judges: Vec<Judge> = judges_in(&root)
        .into_iter()
        .filter(|judge| only.as_ref().is_none_or(|name| &judge.test == name))
        .collect();
    if judges.is_empty() {
        return Err(catalogue::say("cli.ratchet.no_judge", &[]));
    }
    println!(
        "{}",
        catalogue::say(
            "cli.ratchet.measuring",
            &[
                ("judges", &judges.len().to_string()),
                ("laid_over", &moved.laid_over.to_string()),
                ("taken_away", &moved.taken_away.to_string()),
            ],
        )
    );
    let mut counted = Verdicts::default();
    for judge in &judges {
        // Touched so it recompiles: `env!("CARGO_MANIFEST_DIR")` is baked in at
        // compile time, and a cached binary would measure the previous tree.
        let file = clean
            .join("crates")
            .join(&judge.package)
            .join("tests")
            .join(format!("{}.rs", judge.test));
        let _ = std::fs::OpenOptions::new().append(true).open(&file).and_then(|f| f.set_len(
            std::fs::metadata(&file).map(|m| m.len()).unwrap_or(0),
        ));
        let out = Command::new("cargo")
            .current_dir(&clean)
            // Its own target: sharing `target/from-head` with the release put two
            // trees' binaries in one place, and a release running at the same
            // time went red on a target it could not name.
            .env("CARGO_TARGET_DIR", root.join("target").join("ratchet"))
            // `--nocapture`: saying it measured nothing is what a judge does
            // while passing, and a passing judge's words are otherwise dropped.
            .args(["test", "--quiet", "-p", &judge.package, "--test", &judge.test, "--", "--nocapture"])
            .output()
            .map_err(|error| format!("cargo test: {error}"))?;
        let text = String::from_utf8_lossy(&out.stdout) + String::from_utf8_lossy(&out.stderr);
        let verdict = verdict_of(out.status.success(), &text);
        counted.saw(verdict);
        match verdict {
            Verdict::Green => {
                println!("  {} {}", catalogue::say("cli.ratchet.green", &[]), judge.test);
            }
            Verdict::NotMeasured => {
                println!("  {} {}", catalogue::say("cli.ratchet.not_measured", &[]), judge.test);
                let said = |line: &&str| line.contains(workspace::MEASURED_NOTHING);
                for line in text.lines().filter(said) {
                    println!("      {}", line.trim());
                }
            }
            Verdict::NoReceipt => {
                println!("  {} {}", catalogue::say("cli.ratchet.no_receipt", &[]), judge.test);
                let started = |line: &&str| line.trim_start().starts_with(workspace::MEASURED);
                for line in text.lines().filter(started) {
                    println!("      {}", line.trim());
                }
            }
            Verdict::Red => {
                println!("  {} {}", catalogue::say("cli.ratchet.red", &[]), judge.test);
                for line in text.lines().filter(|line| says_what_to_write(line)) {
                    println!("      {}", line.trim());
                }
            }
        }
    }
    let gate = Gate { counted };
    println!("{}", gate.closing_line());
    if let Some(fallen) = gate.seed_may_fall_to(only.is_none(), NO_RECEIPT_TODAY) {
        println!(
            "{}",
            catalogue::say(
                "cli.ratchet.receipt_seed_may_fall",
                &[("seed", &NO_RECEIPT_TODAY.to_string()), ("measured", &fallen.to_string())],
            )
        );
    }
    Ok(gate.lets_through(NO_RECEIPT_TODAY))
}

/// The lines of a red judge worth reading: what the judge said, and the
/// compiler's errors. Cargo's own narration is not.
fn says_what_to_write(line: &str) -> bool {
    let trimmed = line.trim_start();
    if trimmed.is_empty() {
        return false;
    }
    const CARGO_NOISE: &[&str] = &[
        "Compiling", "Running", "Finished", "running ", "test ", "failures", "----",
        "thread '", "note:", "test result", "warning:", "-->", "|", "=",
    ];
    !CARGO_NOISE.iter().any(|noise| trimmed.starts_with(noise))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// **THE JUDGES ARE FOUND, NOT LISTED**, and the finding has to see this
    /// tree's own — the one this file is compiled in, never the one the
    /// environment declares: `sources_root` answers with `SAILOR_SOURCES` or
    /// the launcher's home, and under the release, which runs the suite in an
    /// empty scratch home on purpose, the scan honestly found nothing.
    #[test]
    fn the_judges_of_this_tree_are_found_by_their_seed() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .and_then(std::path::Path::parent)
            .expect("the crate lives in <root>/crates/sailor")
            .to_path_buf();
        let found = judges_in(&root);
        let names: Vec<&str> = found.iter().map(|judge| judge.test.as_str()).collect();
        assert!(names.contains(&"comments_do_not_crowd_out_the_code"), "{names:?}");
        assert!(names.contains(&"no_engine_is_named_in_the_code"), "{names:?}");
        assert!(names.contains(&"no_product_home_is_written_into_the_code"), "{names:?}");
    }

    /// A test that never opens the sources is not a judge, whatever its name.
    #[test]
    fn a_test_that_does_not_read_the_sources_is_not_a_judge() {
        let scratch = std::env::temp_dir().join(format!("sailor-ratchet-{}", std::process::id()));
        let tests = scratch.join("crates").join("una-cassa").join("tests");
        std::fs::create_dir_all(&tests).expect("the scratch tree");
        std::fs::write(tests.join("con_seme.rs"), "fn root() { env!(\"CARGO_MANIFEST_DIR\"); }").expect("write");
        std::fs::write(tests.join("senza_seme.rs"), "fn x() {}").expect("write");

        let found = judges_in(&scratch);
        let _ = std::fs::remove_dir_all(&scratch);

        assert_eq!(
            found,
            vec![Judge { package: "una-cassa".to_owned(), test: "con_seme".to_owned() }]
        );
    }

    fn git(root: &Path, args: &[&str]) {
        let done = Command::new("git").arg("-C").arg(root).args(args).output().expect("git");
        assert!(done.status.success(), "{args:?}: {}", String::from_utf8_lossy(&done.stderr));
    }

    /// A scratch repository with one commit: the overlay needs an archive to
    /// unpack and a working tree to read the change from, and neither can be
    /// faked. Answers with the repository and the place to measure into.
    fn a_repository_holding(named: &str, files: &[(&str, &str)]) -> (PathBuf, PathBuf) {
        let scratch = std::env::temp_dir().join(format!("sailor-overlay-{}-{named}", std::process::id()));
        let root = scratch.join("sources");
        let _ = std::fs::remove_dir_all(&scratch);
        std::fs::create_dir_all(&root).expect("the scratch repository");
        git(&root, &["init", "--quiet"]);
        for (relative, text) in files {
            let path = root.join(relative);
            std::fs::create_dir_all(path.parent().expect("a parent")).expect("the directory");
            std::fs::write(&path, text).expect("the file");
            git(&root, &["add", relative]);
        }
        let who = ["-c", "user.name=a", "-c", "user.email=a@b"];
        let mut commit = who.to_vec();
        commit.extend(["commit", "--quiet", "-m", "first"]);
        git(&root, &commit);
        (root, scratch.join("measured"))
    }

    /// **A REMOVAL IS A CHANGE THE OVERLAY HAS TO CARRY** — fault 99. Both
    /// places it can live: staged in the index, and taken out of the working
    /// tree without being staged.
    #[test]
    fn a_file_the_change_removes_leaves_the_measured_tree() {
        let (root, measured) = a_repository_holding(
            "removed",
            &[("kept.md", "kept\n"), ("staged.md", "gone\n"), ("unstaged.md", "gone\n")],
        );
        git(&root, &["rm", "--quiet", "staged.md"]);
        std::fs::remove_file(root.join("unstaged.md")).expect("the removal");

        let moved = clean_tree_with_changes(&root, &measured).expect("the overlay");
        let standing: Vec<&str> = ["kept.md", "staged.md", "unstaged.md"]
            .into_iter()
            .filter(|name| measured.join(name).exists())
            .collect();
        let _ = std::fs::remove_dir_all(measured.parent().expect("the scratch"));

        assert_eq!(standing, ["kept.md"], "a file the change removes stayed in the measured tree");
        assert_eq!(moved, Overlay { laid_over: 0, taken_away: 2 });
    }

    /// A judge that walks the tree reads a directory as a place that exists,
    /// whatever is left in it.
    #[test]
    fn a_directory_the_change_empties_does_not_stay_standing() {
        let (root, measured) =
            a_repository_holding("emptied", &[("notes/only.md", "one\n"), ("kept.md", "kept\n")]);
        git(&root, &["rm", "--quiet", "notes/only.md"]);

        clean_tree_with_changes(&root, &measured).expect("the overlay");
        let standing = measured.join("notes").exists();
        let _ = std::fs::remove_dir_all(measured.parent().expect("the scratch"));

        assert!(!standing, "the emptied directory stayed in the measured tree");
    }

    /// **THE COUNT IS WHAT A PERSON READS BEFORE TRUSTING THE VERDICT**, so it
    /// says removals as removals: a line reporting nothing while files leave
    /// buys a false green.
    #[test]
    fn the_count_says_removals_as_removals() {
        let (root, measured) =
            a_repository_holding("counted", &[("kept.md", "kept\n"), ("gone.md", "gone\n")]);
        git(&root, &["rm", "--quiet", "gone.md"]);
        std::fs::write(root.join("kept.md"), "changed\n").expect("the change");

        let moved = clean_tree_with_changes(&root, &measured).expect("the overlay");
        let _ = std::fs::remove_dir_all(measured.parent().expect("the scratch"));

        assert_eq!(moved, Overlay { laid_over: 1, taken_away: 1 });
        let said = catalogue::look(
            "en",
            "cli.ratchet.measuring",
            &[("judges", "1"), ("laid_over", "1"), ("taken_away", "1")],
        )
        .expect("the sentence");
        assert!(said.contains("1 removed file(s) taken away"), "{said}");
    }

    /// **A RUN WITH NOTHING RED IS NOT YET A CLEAN RUN**, and the closing line
    /// refuses to say every seed holds while a judge measured nothing.
    #[test]
    fn a_judge_that_measured_nothing_is_not_counted_among_the_ones_that_held() {
        let clean = Verdicts { green: 44, red: 0, not_measured: 0, no_receipt: 0 };
        let blind = Verdicts { green: 41, red: 0, not_measured: 3, no_receipt: 0 };
        let fallen = Verdicts { green: 40, red: 1, not_measured: 3, no_receipt: 0 };

        assert!(clean.closing_line().contains("every seed holds"), "{}", clean.closing_line());
        assert!(!blind.closing_line().contains("every seed holds"), "{}", blind.closing_line());
        assert!(blind.closing_line().contains("3 not measured"), "{}", blind.closing_line());
        assert!(blind.closing_line().contains("41 green"), "{}", blind.closing_line());
        assert!(fallen.closing_line().contains("a seed does not hold"), "{}", fallen.closing_line());
        assert!(fallen.closing_line().contains("3 not measured"), "{}", fallen.closing_line());
    }

    /// **A RUN WITH A SILENT JUDGE IN IT IS NOT A CLEAN RUN EITHER.** The count
    /// is its own column, and the closing line refuses the word «clean» while
    /// it stands above zero.
    #[test]
    fn a_judge_that_handed_in_no_receipt_is_counted_and_the_run_is_not_clean() {
        let silent = Verdicts { green: 43, red: 0, not_measured: 0, no_receipt: 1 };

        assert!(!silent.closing_line().contains("every seed holds"), "{}", silent.closing_line());
        assert!(
            silent.closing_line().contains("1 without a receipt"),
            "{}",
            silent.closing_line()
        );
    }

    /// The receipt is read off the judge's own line, and the numbers in it have
    /// to stand: a walk of none and an oracle of none are the emptiness the
    /// whole receipt exists to stop being read as a result.
    #[test]
    fn a_receipt_is_read_only_where_its_numbers_stand() {
        let walked = format!("{} 214 sources under crates", workspace::MEASURED);
        let weighed = format!(
            "{} 214 sources{}214 paths git tracks",
            workspace::MEASURED,
            workspace::AGAINST
        );
        assert_eq!(receipt_in(&walked), Receipt::Walked { perimeter: 214, oracle: None });
        assert_eq!(
            receipt_in(&weighed),
            Receipt::Walked { perimeter: 214, oracle: Some(214) }
        );
        assert_eq!(receipt_in("running 2 tests\nok."), Receipt::Absent);
        assert_eq!(
            receipt_in(&format!("{} 0 sources under crates", workspace::MEASURED)),
            Receipt::Incomplete
        );
        assert_eq!(
            receipt_in(&format!("{} many sources", workspace::MEASURED)),
            Receipt::Incomplete
        );
        assert_eq!(
            receipt_in(&format!("{} 9 sources{}0 tracked", workspace::MEASURED, workspace::AGAINST)),
            Receipt::Incomplete
        );
    }

    /// **AN EXIT CODE OF ZERO IS NOT A MEASUREMENT.** A judge that passes and
    /// says nothing about what it opened is the emptiness of the diagnosis, and
    /// the gate has only its words to tell that from a real walk.
    #[test]
    fn passing_without_a_receipt_is_not_green() {
        let mute = "running 3 tests\ntest result: ok. 3 passed";
        let handed = format!("running 3 tests\n{} 214 sources under crates", workspace::MEASURED);
        assert_eq!(verdict_of(true, mute), Verdict::NoReceipt);
        assert_eq!(verdict_of(true, &handed), Verdict::Green);
        // A red is a red whatever it wrote: the receipt is not a way out.
        assert_eq!(verdict_of(false, &handed), Verdict::Red);
        // Declaring an empty oracle stays its own state, not folded into this.
        let blind = format!("{} nothing to ask", workspace::MEASURED_NOTHING);
        assert_eq!(verdict_of(true, &blind), Verdict::NotMeasured);
    }

    /// The seed is a debt, and the run says so the moment it holds less than
    /// the seed claims — but only when the whole battery was asked.
    #[test]
    fn the_receipt_seed_is_nudged_down_only_over_a_whole_run() {
        let seed = 3;
        let mut gate = Gate::default();
        gate.counted.no_receipt = seed - 1;
        assert_eq!(gate.seed_may_fall_to(true, seed), Some(seed - 1));
        assert_eq!(gate.seed_may_fall_to(false, seed), None);
        gate.counted.no_receipt = seed;
        assert_eq!(gate.seed_may_fall_to(true, seed), None);
    }

    /// **PASSING IS NOT MEASURING.** A judge that exits zero having said it
    /// measured nothing is the false green of fault 100, and only its own
    /// output tells the two apart.
    #[test]
    fn a_judge_that_passed_without_measuring_is_not_counted_green() {
        let blind = format!("running 2 tests\n{} nothing to ask\nok.", workspace::MEASURED_NOTHING);
        let handed = format!("running 2 tests\n{} 7 files under crates\nok.", workspace::MEASURED);
        assert_eq!(verdict_of(true, &blind), Verdict::NotMeasured);
        assert_eq!(verdict_of(true, &handed), Verdict::Green);
        assert_eq!(verdict_of(false, &blind), Verdict::Red);
        assert_eq!(verdict_of(false, "assertion failed"), Verdict::Red);
    }

    /// Only the lines a person acts on come through.
    #[test]
    fn only_the_lines_that_say_what_to_write_are_kept() {
        assert!(says_what_to_write("crate «flow» carries 222‰ comment lines against a seed of Some(221)."));
        assert!(says_what_to_write("     1  crates/sailor/src/ratchet_cmd.rs"));
        assert!(says_what_to_write("error[E0432]: unresolved import `crate::catalogue`"));
        assert!(!says_what_to_write("   Compiling sailor v0.1.0"));
        assert!(!says_what_to_write("test result: FAILED. 0 passed; 1 failed"));
        assert!(!says_what_to_write("note: run with `RUST_BACKTRACE=1` for a backtrace"));
    }
}
