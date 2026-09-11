//! **A NAME CANNOT BE UNPUBLISHED**: a forced push leaves the commit reachable
//! by its number, so the cure is to refuse before it. The judge guarding the
//! trunk cannot: a release runs its suite on a `git archive` extract, which is
//! no repository, so it declares «measured nothing» and always passed.

use std::path::{Path, PathBuf};
use std::process::Command;

fn a_scratch(label: &str) -> PathBuf {
    let path = std::env::temp_dir().join(format!("sailor-private-{label}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&path);
    std::fs::create_dir_all(&path).expect("a scratch");
    path
}

fn git(at: &Path, args: &[&str]) {
    let done = Command::new("git")
        .arg("-C")
        .arg(at)
        .args(args)
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .output()
        .expect("git runs");
    assert!(done.status.success(), "git {args:?}");
}

/// A repository holding one tracked file with `what` in it.
fn a_repository_saying(label: &str, what: &str) -> (PathBuf, PathBuf) {
    let scratch = a_scratch(label);
    let repo = scratch.join("project");
    std::fs::create_dir_all(&repo).expect("the repository");
    git(&repo, &["init", "-q"]);
    git(&repo, &["config", "user.email", "test@example.test"]);
    git(&repo, &["config", "user.name", "test"]);
    std::fs::write(repo.join("a-file.txt"), what).expect("a file");
    git(&repo, &["add", "a-file.txt"]);
    git(&repo, &["commit", "-q", "-m", "the first"]);
    (scratch, repo)
}

/// The reading says **where** and never what: scrollback goes anywhere.
#[test]
fn a_tracked_file_naming_something_private_is_found_and_the_name_is_not_echoed() {
    let (scratch, repo) = a_repository_saying("tracked", "a line about mylberry, here\n");

    let hits = toolbox::privacy::where_names_are_tracked(&repo, &["mylberry".to_owned()]);
    let _ = std::fs::remove_dir_all(&scratch);

    assert_eq!(hits, vec!["a-file.txt:1".to_owned()], "the place was not named");
    assert!(
        !hits.iter().any(|hit| hit.contains("mylberry")),
        "the name travelled with the report: {hits:?}"
    );
}

/// **THE ABSURD CASE**: a clean repository lets a push through, or the guard
/// would stop every release forever.
#[test]
fn a_tree_with_nothing_private_in_it_is_not_held_back() {
    let (scratch, repo) = a_repository_saying("clean", "a line about nothing at all\n");

    let hits = toolbox::privacy::where_names_are_tracked(&repo, &["mylberry".to_owned()]);
    let _ = std::fs::remove_dir_all(&scratch);

    assert!(hits.is_empty(), "{hits:?}");
}

/// What git does not track is not published: the sketches of this very tree
/// are untracked, and counting them would refuse every release.
#[test]
fn a_file_git_does_not_track_is_not_something_the_push_publishes() {
    let (scratch, repo) = a_repository_saying("untracked", "nothing here\n");
    std::fs::write(repo.join("a-sketch.txt"), "mylberry sits in here\n").expect("a sketch");

    let hits = toolbox::privacy::where_names_are_tracked(&repo, &["mylberry".to_owned()]);
    let _ = std::fs::remove_dir_all(&scratch);

    assert!(hits.is_empty(), "an untracked file was counted: {hits:?}");
}

/// The one the release asks: that the refusal is written where the push is,
/// and that it comes before it and not after.
#[test]
fn the_release_refuses_the_push_before_making_it() {
    let source = include_str!("../src/release_cmd.rs");
    let from = source
        .find("fn say_whether_pushed(")
        .expect("the release no longer has a push to guard");
    let rest = &source[from..];
    let body = &rest[..rest.find("\n}\n").unwrap_or(rest.len())];

    let refusal = body
        .find("what_must_not_be_published")
        .expect("the push is not guarded at all");
    let push = body
        .find("push_the_trunk")
        .expect("this is no longer the function that pushes");
    assert!(
        refusal < push,
        "the trunk is pushed before anybody asks what it would publish"
    );

    workspace::measured(1, "push written in the release, guarded before it runs");
}
