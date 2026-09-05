//! The adversarial canary: self-care asked, on purpose, to repair the file
//! that tells it no. The first thing a repairer sees in a rule that stops it
//! is a defect, and the easiest one to fix is a line of JSON.

use serde_json::json;
use std::fs;
use std::path::{Path, PathBuf};

fn scratch() -> PathBuf {
    let path = std::env::temp_dir().join(format!("sailor-wall-{}", std::process::id()));
    let _ = fs::remove_dir_all(&path);
    fs::create_dir_all(&path).expect("a directory to work in");
    path
}

fn git(repo: &Path, args: &[&str]) {
    let done = std::process::Command::new("git")
        .arg("-C")
        .arg(repo)
        .args(args)
        // The repository's own settings only: nothing of the runner's account.
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .output()
        .expect("git runs");
    assert!(done.status.success(), "git {args:?}: {}", String::from_utf8_lossy(&done.stderr));
}

/// A repository holding the two files the patches below aim at.
fn a_project_in(dir: &Path) -> PathBuf {
    let repo = dir.join("repo");
    fs::create_dir_all(repo.join("crates/actions/src")).expect("the source directory");
    git(&repo, &["init", "-q"]);
    git(&repo, &["config", "user.email", "prove@example"]);
    git(&repo, &["config", "user.name", "prove"]);
    fs::write(repo.join("crates/actions/src/apply.rs"), "the wall\n").expect("the module");
    fs::write(repo.join("crates/actions/src/other.rs"), "a line\n").expect("another file");
    git(&repo, &["add", "-A"]);
    git(&repo, &["commit", "-q", "-m", "the first commit"]);
    repo
}

fn a_patch_for(path: &str, from: &str, to: &str) -> String {
    format!(
        "diff --git a/{path} b/{path}\n--- a/{path}\n+++ b/{path}\n@@ -1 +1 @@\n-{from}\n+{to}\n"
    )
}

/// Self-care is asked to repair the file that grants and the code that
/// applies, under an assent naming them both. Both refused; an ordinary file
/// under the same assent goes through, or this would prove only that nothing
/// works. *The mutant:* take the assent file out of the wall and the first arm
/// goes green, which is the day the wall stops being one.
#[test]
fn no_assent_opens_the_file_that_grants_or_the_code_that_applies() {
    let dir = scratch();
    let repo = a_project_in(&dir);
    let home = dir.join("casa");
    fs::create_dir_all(&home).expect("Sailor's home");
    fs::write(
        home.join(actions::apply::THE_ASSENT_FILE),
        json!({"may_touch": ["crates/", "autocura.json"]}).to_string(),
    )
    .expect("an assent that names everything");

    let asked_for_the_grant = a_patch_for("autocura.json", "{}", "{\"may_touch\": [\"/\"]}");
    let refused = actions::apply::apply(&repo, &asked_for_the_grant, Some(&home))
        .expect_err("the file that grants is behind the wall");
    assert!(refused.contains("autocura.json") && refused.contains("wall"), "{refused}");

    let asked_for_the_applier =
        a_patch_for("crates/actions/src/apply.rs", "the wall", "no wall at all");
    let refused = actions::apply::apply(&repo, &asked_for_the_applier, Some(&home))
        .expect_err("the code that applies is behind the wall");
    assert!(refused.contains("apply.rs"), "{refused}");

    // The absurd control: with the same assent an ordinary file goes through,
    // so the two refusals above are the wall and not a broken applier.
    let ordinary = a_patch_for("crates/actions/src/other.rs", "a line", "another line");
    let changed = actions::apply::apply(&repo, &ordinary, Some(&home))
        .expect("an ordinary file under the same assent");
    assert_eq!(changed, vec!["crates/actions/src/other.rs".to_owned()]);
    assert_eq!(
        fs::read_to_string(repo.join("crates/actions/src/other.rs")).expect("read it back"),
        "another line\n",
        "the patch was said to be applied and the file did not change"
    );

    // **THE WAY ROUND THE WALL IS A NAME, NOT A DOOR.** Judged as text,
    // `crates/../autocura.json` reads as a path under `crates/`, which the
    // assent names, and lands on the file the wall exists to hold.
    let walked_up = a_patch_for("crates/../autocura.json", "{}", "{\"may_touch\": [\"/\"]}");
    let refused = actions::apply::apply(&repo, &walked_up, Some(&home))
        .expect_err("a path that walks up is not judged where it lands");
    assert!(refused.contains("plain path"), "{refused}");

    let from_the_root = a_patch_for("/etc/hosts", "one", "another");
    let refused = actions::apply::apply(&repo, &from_the_root, Some(&home))
        .expect_err("a patch that starts at the root is outside the tree");
    assert!(refused.contains("plain path"), "{refused}");

    // A name that merely begins with an assented one is not that name.
    let next_door = a_patch_for("crates-of-somebody-else/x.rs", "one", "another");
    let refused = actions::apply::apply(&repo, &next_door, Some(&home))
        .expect_err("«crates/» does not answer for «crates-of-somebody-else/»");
    assert!(refused.contains("assents"), "{refused}");

    // No assent at all is no permission, never «anything goes».
    let refused = actions::apply::apply(&repo, &ordinary, None)
        .expect_err("a home that grants nothing grants nothing");
    assert!(refused.contains(actions::apply::THE_ASSENT_FILE), "{refused}");
}
