//! `sailor worktree`, the part with a right and a wrong answer.
//!
//! What talks to git is a thin wrapper; what is tested here is what it reads
//! back, because that is where a tree can be misread into the wrong branch or
//! the wrong name — and a wrong name is what `remove` acts on.

use sailor::worktree_cmd::render;
use std::path::Path;
use workspace::{name_for, parse_worktrees, tree_path};

const PORCELAIN: &str = "\
worktree /somewhere/project
HEAD 495a93344af1912bfb72d85f9caf4ee70f11cdd8
branch refs/heads/sorgenti

worktree /somewhere/project-worktrees/accompagnatore
HEAD 0897a7ceaf694a5a1e630bc03551eae47bbd17d9
branch refs/heads/work/accompagnatore

worktree /somewhere/project-worktrees/staccato
HEAD 17c1dd3eee3145297f5fc8b18378903a6fbfddfc
detached
";

#[test]
fn every_tree_is_read_with_its_branch() {
    let trees = parse_worktrees(PORCELAIN);
    assert_eq!(trees.len(), 3);
    assert_eq!(trees[0].branch.as_deref(), Some("sorgenti"));
    assert_eq!(trees[1].branch.as_deref(), Some("work/accompagnatore"));
    assert_eq!(trees[1].name(), "accompagnatore");
}

/// THE MEASURE THAT COULD HAVE COME OUT DIFFERENTLY. A detached tree has no
/// branch to go back to, and reporting the previous tree's branch for it would
/// be worse than reporting none: it would name a branch this tree is not on.
#[test]
fn a_detached_tree_carries_no_branch_instead_of_the_last_one_seen() {
    let trees = parse_worktrees(PORCELAIN);
    assert_eq!(trees[2].branch, None);
    let shown = render(Path::new("/somewhere/project"), &trees);
    let line = shown
        .lines()
        .find(|line| line.starts_with("staccato"))
        .expect("the tree is listed");
    assert!(line.ends_with("detached"), "{line}");
}

/// A path a person chose is exactly where a space turns up, and the human
/// listing aligns its columns with spaces.
#[test]
fn a_path_with_a_space_stays_one_path() {
    let trees =
        parse_worktrees("worktree /somewhere/my trees/one\nHEAD abc\nbranch refs/heads/main\n");
    assert_eq!(trees[0].path, "/somewhere/my trees/one");
    assert_eq!(trees[0].name(), "one");
}

#[test]
fn git_tells_us_when_a_tree_is_locked_or_its_directory_is_gone() {
    let trees = parse_worktrees("worktree /a\nHEAD abc\nbranch refs/heads/x\nlocked because I said so\nprunable gitdir file points to non-existent location\n");
    assert!(trees[0].locked);
    assert!(trees[0].prunable);
    assert!(render(Path::new("/a-repo"), &trees).contains("its directory is gone"));
}

/// A branch is allowed a slash and a directory name is not: `work/thing` would
/// quietly nest a directory called `work`.
#[test]
fn a_branch_with_a_slash_does_not_become_two_directories() {
    assert_eq!(name_for("work/the-thing"), "the-thing");
    assert_eq!(name_for("main"), "main");
    assert_eq!(
        tree_path(Path::new("/somewhere/project"), "the-thing"),
        Path::new("/somewhere/project-worktrees/the-thing")
    );
}

/// Beside the repository and never inside it: inside, every tool that walks
/// the project would walk every tree of it as well.
#[test]
fn a_new_tree_is_cut_beside_the_repository_not_within_it() {
    let repo = Path::new("/somewhere/project");
    let cut = tree_path(repo, "thing");
    assert!(
        !cut.starts_with(repo),
        "{} is inside the repository",
        cut.display()
    );
}

#[test]
fn nothing_at_all_reads_as_nothing_at_all() {
    assert_eq!(render(Path::new("/a-repo"), &[]), "no worktrees");
    assert!(parse_worktrees("").is_empty());
}

/// A tree a step left behind is disk nobody asked for until the listing says
/// whose it is and how many there are.
#[test]
fn a_tree_kept_from_a_step_is_listed_with_its_run_and_counted() {
    let trees = parse_worktrees(
        "worktree /somewhere/project\nHEAD abc\nbranch refs/heads/sorgenti\n\n\
         worktree /somewhere/project-worktrees/corsa-1/implementa\nHEAD def\ndetached\n",
    );

    let shown = render(Path::new("/somewhere/project"), &trees);
    let line = shown
        .lines()
        .find(|line| line.starts_with("implementa"))
        .expect("the kept tree is listed");

    assert!(line.contains("corsa-1") && line.contains("implementa"), "{line}");
    assert!(shown.contains("1 of these"), "the kept trees are not counted:\n{shown}");
    assert!(
        !shown.lines().next().expect("the repository's own line").contains("corsa-1"),
        "a tree a person cut was read as a step's:\n{shown}"
    );
}
