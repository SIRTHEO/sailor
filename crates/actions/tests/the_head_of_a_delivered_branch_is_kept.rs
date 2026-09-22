//! The archive tag of a delivered branch: what it is decided from, who the
//! push is bound to, and the three ends of a real push against a remote on
//! this disk — it goes up, it goes up only once, and it is never moved.

use actions::archive::{
    credential_helper, how_it_is_bound, tag_for, what_becomes_of_the_tag, HowItIsBound,
    WhatBecomesOfTheTag,
};
use flow::{ActionRegistry, SharedState};
use serde_json::json;
use std::path::{Path, PathBuf};
use std::process::Command;

const TAG: &str = "refs/tags/archive/work/a-change";
const HEAD: &str = "c0ffee00c0ffee00c0ffee00c0ffee00c0ffee00";

#[test]
fn a_remote_naming_nothing_gets_the_tag_pushed() {
    assert_eq!(
        what_becomes_of_the_tag("", TAG, HEAD),
        WhatBecomesOfTheTag::PushIt
    );
}

#[test]
fn a_tag_already_at_the_proved_head_is_left_alone() {
    let listed = format!("{HEAD}\t{TAG}\n");
    assert_eq!(
        what_becomes_of_the_tag(&listed, TAG, HEAD),
        WhatBecomesOfTheTag::AlreadyThere
    );
}

/// An annotated tag lists as its own object first: read by that line alone it
/// would look like a tag naming another commit.
#[test]
fn an_annotated_tag_peeling_to_the_head_is_left_alone() {
    let listed = format!("aaaa1111\t{TAG}\n{HEAD}\t{TAG}^{{}}\n");
    assert_eq!(
        what_becomes_of_the_tag(&listed, TAG, HEAD),
        WhatBecomesOfTheTag::AlreadyThere
    );
}

#[test]
fn a_tag_naming_another_commit_is_named_back() {
    let listed = format!("aaaa1111\t{TAG}\n");
    assert_eq!(
        what_becomes_of_the_tag(&listed, TAG, HEAD),
        WhatBecomesOfTheTag::NamesAnother("aaaa1111".to_owned())
    );
}

const OVER_HTTPS: &str = "https://a-host.example/owner/repo.git";
const TOKEN_COMMAND: &str = "print-a-token --user";

#[test]
fn only_a_remote_that_can_be_bound_to_an_account_is_pushed_to() {
    assert_eq!(
        how_it_is_bound("/srv/origin.git", None, None),
        HowItIsBound::NothingToBind
    );
    // The account is the last word of the command that prints its token.
    assert_eq!(
        how_it_is_bound(OVER_HTTPS, Some("someone"), Some(TOKEN_COMMAND)),
        HowItIsBound::AsThisAccount(vec![
            "print-a-token".to_owned(),
            "--user".to_owned(),
            "someone".to_owned(),
        ])
    );
    // The machine's own credentials are never the fallback: neither half alone
    // binds the push.
    for (who, token) in [
        (None, Some(TOKEN_COMMAND)),
        (Some("someone"), None),
        (Some(""), Some(TOKEN_COMMAND)),
        (Some("someone"), Some("   ")),
    ] {
        assert_eq!(
            how_it_is_bound(OVER_HTTPS, who, token),
            HowItIsBound::Unbindable(OVER_HTTPS.to_owned()),
            "bound with who={who:?} token={token:?}"
        );
    }
    // A transport this action cannot bind at all, however it is declared.
    assert_eq!(
        how_it_is_bound(
            "ssh://a-host.example/owner/repo.git",
            Some("someone"),
            Some(TOKEN_COMMAND)
        ),
        HowItIsBound::Unbindable("ssh://a-host.example/owner/repo.git".to_owned())
    );
}

/// **FOUND BY RUNNING IT, NOT BY THE TESTS ABOVE**: a remote on this disk asks
/// for no credentials, so the push that proves the tag never exercised the
/// helper. git runs it as `<helper> get`, and the first version — a bare
/// `printf` — took that `get` for a second argument, repeated its format around
/// it and left `password=get` as the last line git read. The real push answered
/// «Invalid username or token».
#[test]
fn the_helper_ignores_the_word_git_hands_it() {
    let body = credential_helper()
        .strip_prefix('!')
        .expect("the helper is a shell body");
    let output = Command::new("sh")
        .arg("-c")
        .arg(format!("{body} \"$@\""))
        .arg("sh")
        .arg("get")
        .env("SAILOR_PUSH_TOKEN", "a-token")
        .output()
        .expect("the helper runs");
    assert_eq!(
        String::from_utf8_lossy(&output.stdout),
        "username=x-access-token\npassword=a-token\n",
        "the helper must say the token once and nothing else"
    );
}

#[test]
fn the_whole_branch_name_goes_into_the_tag() {
    assert_eq!(tag_for("work/a-change"), TAG);
}

struct Scratch(PathBuf);

impl Scratch {
    fn new(name: &str) -> Self {
        let at = std::env::temp_dir().join(format!("archive-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&at);
        std::fs::create_dir_all(&at).expect("a scratch directory");
        Self(at)
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn git(at: &Path, args: &[&str]) -> String {
    let output = Command::new("git")
        .arg("-C")
        .arg(at)
        .args(args)
        .output()
        .expect("git runs");
    assert!(
        output.status.success(),
        "git {args:?} in {}: {}",
        at.display(),
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8_lossy(&output.stdout).trim().to_owned()
}

/// A repository with one commit and a bare remote of its own on this disk.
fn a_tree_with_a_remote(scratch: &Scratch) -> (PathBuf, PathBuf, String) {
    let remote = scratch.0.join("origin.git");
    let tree = scratch.0.join("tree");
    std::fs::create_dir_all(&remote).expect("the remote's directory");
    std::fs::create_dir_all(&tree).expect("the tree's directory");
    git(&scratch.0, &["init", "-q", "--bare", "origin.git"]);
    git(&scratch.0, &["init", "-q", "tree"]);
    git(&tree, &["config", "user.email", "sailor@example.invalid"]);
    git(&tree, &["config", "user.name", "Sailor"]);
    std::fs::write(tree.join("a-file"), "the work\n").expect("a file to commit");
    git(&tree, &["add", "a-file"]);
    git(&tree, &["commit", "-q", "-m", "the work"]);
    git(
        &tree,
        &["remote", "add", "origin", &remote.to_string_lossy()],
    );
    let head = git(&tree, &["rev-parse", "HEAD"]);
    (tree, remote, head)
}

fn archive(tree: &Path, head: &str) -> Result<serde_json::Value, flow::ActionError> {
    let mut registry = ActionRegistry::default();
    actions::archive::register_archive_the_head(&mut registry);
    let action = registry
        .get("archive_the_head")
        .expect("the action is registered");
    action
        .execute(
            &json!({
                "repo": tree.to_string_lossy(),
                "remote": "origin",
                "branch": "work/a-change",
                "head": head,
            }),
            &SharedState::new(),
        )
        .map(|outcome| match outcome {
            flow::ActionOutcome::Went(value) => value,
            other => panic!("the action did not go: {other:?}"),
        })
}

#[test]
fn the_head_goes_up_under_its_tag_and_a_second_run_pushes_nothing() {
    let scratch = Scratch::new("kept");
    let (tree, remote, head) = a_tree_with_a_remote(&scratch);

    let said = archive(&tree, &head).expect("the head is archived");
    assert_eq!(said["tag"], json!(TAG));
    assert_eq!(said["archived"], json!(head));
    assert_eq!(git(&remote, &["rev-parse", TAG]), head);

    // Run again: the tag is already at the head, so nothing is pushed and the
    // step still goes — the flow above it must be safe to run a second time.
    let again = archive(&tree, &head).expect("the second run goes too");
    assert_eq!(again["archived"], json!(head));
    assert_eq!(git(&remote, &["rev-parse", TAG]), head);
}

#[test]
fn a_tag_somebody_else_wrote_stops_the_run_instead_of_moving() {
    let scratch = Scratch::new("moved");
    let (tree, remote, head) = a_tree_with_a_remote(&scratch);
    std::fs::write(tree.join("a-file"), "other work\n").expect("a second file");
    git(&tree, &["commit", "-q", "-am", "other work"]);
    let other = git(&tree, &["rev-parse", "HEAD"]);
    git(&tree, &["push", "-q", "origin", &format!("{other}:{TAG}")]);

    let refused = archive(&tree, &head).expect_err("an archive tag is not moved");
    assert_eq!(refused.class, "the_head_is_not_archived");
    assert!(
        refused.said.contains(&other) && refused.said.contains(&head),
        "the refusal names both commits: {}",
        refused.said
    );
    assert_eq!(git(&remote, &["rev-parse", TAG]), other, "nothing moved");
}
