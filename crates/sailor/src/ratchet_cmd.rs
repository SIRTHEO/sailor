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

/// How far the change moved the archive, in the two directions a person needs
/// told apart before trusting the verdict.
#[derive(Debug, Default, PartialEq, Eq)]
struct Overlay {
    laid_over: usize,
    taken_away: usize,
}

/// A clean copy of HEAD with this tree's changes laid over it.
fn clean_tree_with_changes(root: &Path, into: &Path) -> Result<Overlay, String> {
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
    Ok(moved)
}

/// The tree the command is run in, when it is one: a checkout other than the
/// sources in service — a worktree, a clone — is measured for itself, and
/// two checkouts never share one `target/ratchet-tree`.
fn root_to_measure() -> Result<PathBuf, String> {
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
    let mut all_green = true;
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
            .args(["test", "--quiet", "-p", &judge.package, "--test", &judge.test])
            .output()
            .map_err(|error| format!("cargo test: {error}"))?;
        if out.status.success() {
            println!("  {} {}", catalogue::say("cli.ratchet.green", &[]), judge.test);
            continue;
        }
        all_green = false;
        println!("  {} {}", catalogue::say("cli.ratchet.red", &[]), judge.test);
        let text = String::from_utf8_lossy(&out.stdout) + String::from_utf8_lossy(&out.stderr);
        for line in text.lines().filter(|line| says_what_to_write(line)) {
            println!("      {}", line.trim());
        }
    }
    println!(
        "{}",
        catalogue::say(
            if all_green { "cli.ratchet.all_green" } else { "cli.ratchet.some_red" },
            &[],
        )
    );
    Ok(all_green)
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
    /// tree's own: a scan that missed the comment ratchet would run green over
    /// the very rite this command replaces.
    #[test]
    fn the_judges_of_this_tree_are_found_by_their_seed() {
        let root = crate::release_cmd::sources_root().expect("the sources");
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
