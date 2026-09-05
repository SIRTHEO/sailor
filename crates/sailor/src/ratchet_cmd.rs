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

/// The files this tree has changed or added: what gets laid over the archive.
/// A new file joins only where HEAD already has a directory for it — a whole
/// untracked tree (`node_modules/`, a scratch folder) is not a change.
fn changed_here(root: &Path, archive: &Path) -> Result<Vec<PathBuf>, String> {
    let said = Command::new("git")
        .arg("-C")
        .arg(root)
        .args(["status", "--porcelain", "--untracked-files=all"])
        .output()
        .map_err(|error| format!("git status: {error}"))?;
    if !said.status.success() {
        return Err(catalogue::say("cli.ratchet.not_a_repository", &[]));
    }
    Ok(String::from_utf8_lossy(&said.stdout)
        .lines()
        .filter(|line| line.len() > 3 && !line.starts_with(" D") && !line.starts_with("D "))
        .map(|line| (line.starts_with("??"), PathBuf::from(line[3..].trim())))
        .filter(|(is_new, path)| {
            !is_new || path.parent().is_some_and(|parent| archive.join(parent).is_dir())
        })
        .map(|(_, path)| path)
        .collect())
}

/// A clean copy of HEAD with this tree's changes laid over it.
fn clean_tree_with_changes(root: &Path, into: &Path) -> Result<usize, String> {
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
    let changed = changed_here(root, into)?;
    for relative in &changed {
        println!("  + {}", relative.display());
        let from = root.join(relative);
        let to = into.join(relative);
        if let Some(parent) = to.parent() {
            std::fs::create_dir_all(parent).map_err(|error| format!("{}: {error}", parent.display()))?;
        }
        std::fs::copy(&from, &to).map_err(|error| format!("{}: {error}", from.display()))?;
    }
    Ok(changed.len())
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
    let laid_over = clean_tree_with_changes(&root, &clean)?;
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
                ("laid_over", &laid_over.to_string()),
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
