//! What the window shows as «what changed» is git's answer, byte for byte, and
//! not a difference computed in this tree. The judge is git itself, run by
//! this test on the same repository.

use std::path::{Path, PathBuf};
use std::process::Command;

fn scratch(name: &str) -> PathBuf {
    let directory = std::env::temp_dir().join(format!(
        "sailor-changes-{name}-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("the clock does not go backwards")
            .as_nanos()
    ));
    std::fs::create_dir_all(&directory).expect("make the scratch repository");
    directory
}

fn git(repo: &Path, args: &[&str]) -> String {
    let done = Command::new("git")
        .arg("-C")
        .arg(repo)
        .args(args)
        .env("GIT_AUTHOR_NAME", "test")
        .env("GIT_AUTHOR_EMAIL", "test@example.invalid")
        .env("GIT_COMMITTER_NAME", "test")
        .env("GIT_COMMITTER_EMAIL", "test@example.invalid")
        .output()
        .expect("run git");
    assert!(
        done.status.success(),
        "git {} failed: {}",
        args.join(" "),
        String::from_utf8_lossy(&done.stderr)
    );
    String::from_utf8_lossy(&done.stdout).into_owned()
}

/// A repository with one commit, one edited file, one new file and one file
/// an agent already staged: the kinds of change a plain diff cannot all show.
fn a_repository_with_changes() -> PathBuf {
    let repo = scratch("edited");
    git(&repo, &["init", "--quiet"]);
    std::fs::write(repo.join("kept.txt"), "one\ntwo\nthree\n").expect("write");
    git(&repo, &["add", "kept.txt"]);
    git(&repo, &["commit", "--quiet", "-m", "first"]);
    std::fs::write(repo.join("kept.txt"), "one\ntwo changed by an agent\nthree\n").expect("edit");
    std::fs::write(repo.join("fresh.md"), "a file nobody tracked yet\n").expect("write");
    std::fs::write(repo.join("staged.txt"), "added and staged by an agent\n").expect("write");
    git(&repo, &["add", "staged.txt"]);
    repo
}

/// **THE DIFF IS GIT'S, BYTE FOR BYTE.** The test runs the same git command
/// and compares the whole text: a diff computed here would differ in its
/// header, its hunk line or its context, and the comparison would say so.
#[test]
fn the_diff_shown_is_what_git_diff_prints() {
    let repo = a_repository_with_changes();
    let root = repo.canonicalize().expect("canonical root");
    let seen = workspace::changes(&root).expect("read the changes");

    let gits = git(&root, &["diff", "HEAD"]);
    assert!(
        gits.contains("two changed by an agent"),
        "the control is blind: git itself shows no change"
    );
    assert_eq!(seen.diff, gits, "the diff shown is not git's answer");
    // An agent that ran `git add` has still changed the tree: the staged file
    // is in the diff, which only a diff against HEAD can say.
    assert!(
        seen.diff.contains("added and staged by an agent"),
        "what an agent staged is missing from the diff"
    );

    let mut files: Vec<(String, String)> = seen
        .files
        .iter()
        .map(|file| (file.status.clone(), file.path.clone()))
        .collect();
    files.sort();
    assert_eq!(
        files,
        vec![
            (" M".to_owned(), "kept.txt".to_owned()),
            ("??".to_owned(), "fresh.md".to_owned()),
            ("A ".to_owned(), "staged.txt".to_owned()),
        ],
        "the file list is not git's"
    );
    assert_eq!(seen.root, root.to_string_lossy());

    let _ = std::fs::remove_dir_all(&repo);
}

/// The absurd control: a clean tree changes nothing, and says so with an
/// empty list and an empty diff — not with an error and not with a stale one.
#[test]
fn a_clean_tree_has_nothing_to_show() {
    let repo = scratch("clean");
    git(&repo, &["init", "--quiet"]);
    std::fs::write(repo.join("kept.txt"), "one\n").expect("write");
    git(&repo, &["add", "kept.txt"]);
    git(&repo, &["commit", "--quiet", "-m", "first"]);

    let seen = workspace::changes(&repo).expect("read the changes");
    assert!(seen.files.is_empty(), "{:?}", seen.files);
    assert_eq!(seen.diff, "");

    let _ = std::fs::remove_dir_all(&repo);
}

/// A file called with a space or an accent is listed by the name it has on
/// disk, which is the one an editor can open: git's line form would quote it.
#[test]
fn a_path_with_a_space_or_an_accent_is_the_path_on_disk() {
    let repo = scratch("quoted");
    git(&repo, &["init", "--quiet"]);
    std::fs::write(repo.join("kept.txt"), "one\n").expect("write");
    git(&repo, &["add", "kept.txt"]);
    git(&repo, &["commit", "--quiet", "-m", "first"]);
    std::fs::write(repo.join("nuovo file.txt"), "a space\n").expect("write");
    std::fs::write(repo.join("città.txt"), "an accent\n").expect("write");

    let seen = workspace::changes(&repo).expect("read the changes");
    let mut paths: Vec<&str> = seen.files.iter().map(|file| file.path.as_str()).collect();
    paths.sort();
    assert_eq!(paths, vec!["città.txt", "nuovo file.txt"], "{:?}", seen.files);
    for file in &seen.files {
        assert!(repo.join(&file.path).is_file(), "{} is not on disk", file.path);
    }

    let _ = std::fs::remove_dir_all(&repo);
}

/// A renamed file is listed under the name it has now, which is the one an
/// editor can open.
#[test]
fn a_rename_is_listed_under_its_new_name() {
    let listed = workspace::parse_status("R  new.txt\0old.txt\0 M kept.txt\0?? fresh.md\0");
    assert_eq!(
        listed,
        vec![
            workspace::ChangedFile {
                status: "R ".to_owned(),
                path: "new.txt".to_owned()
            },
            workspace::ChangedFile {
                status: " M".to_owned(),
                path: "kept.txt".to_owned()
            },
            workspace::ChangedFile {
                status: "??".to_owned(),
                path: "fresh.md".to_owned()
            },
        ]
    );
}
