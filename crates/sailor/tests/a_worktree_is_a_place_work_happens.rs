//! `sailor worktree`, the part with a right and a wrong answer.
//!
//! What talks to git is a thin wrapper; what is tested here is what it reads
//! back, because that is where a tree can be misread into the wrong branch or
//! the wrong name — and a wrong name is what `remove` acts on.

use ledger::holdings::Whose;
use sailor::retire_index::IndexTending;
use sailor::worktree_cmd::{render, render_open, still_the_opener, sweep, Holders};
use std::path::{Path, PathBuf};
use std::process::Command;
use workspace::index_identity::IdentityRule;
use workspace::{name_for, parse_worktrees, tree_path, OpenTree, OpenTrees};

/// **NOT «main».** A trunk with another name is what catches one read from the code.
const A_TRUNK: &str = "tronco";

const PORCELAIN: &str = "\
worktree /somewhere/project
HEAD 495a93344af1912bfb72d85f9caf4ee70f11cdd8
branch refs/heads/main

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
    assert_eq!(trees[0].branch.as_deref(), Some("main"));
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
    let shown = render(&trees);
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
    assert!(render(&trees).contains("its directory is gone"));
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
    assert_eq!(render(&[]), "no worktrees");
    assert!(parse_worktrees("").is_empty());
}

/// A tree a step left behind is disk nobody asked for until the listing says
/// whose it is and how many there are.
#[test]
fn a_tree_kept_from_a_step_is_listed_with_its_run_and_counted() {
    let trees = parse_worktrees(
        "worktree /somewhere/project\nHEAD abc\nbranch refs/heads/main\n\n\
         worktree /somewhere/project-worktrees/corsa-1/implementa\nHEAD def\ndetached\n",
    );

    let shown = render(&trees);
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

fn an_entry(path: &str, opened_at: i64, pid: u32) -> OpenTree {
    born_entry(path, opened_at, pid, None)
}

fn born_entry(path: &str, opened_at: i64, pid: u32, opened_by_born_at: Option<i64>) -> OpenTree {
    OpenTree {
        opened_by_born_at,
        path: path.to_owned(),
        repo: "/somewhere/project".to_owned(),
        run: "corsa-7".to_owned(),
        step: "implementa".to_owned(),
        opened_by_pid: pid,
        opened_at,
    }
}

/// The register answers three things a listing of git's cannot: which run
/// asked, how long ago, and whether the process that asked is still there.
#[test]
fn the_open_trees_say_who_asked_and_whether_that_one_is_still_running() {
    let held = [
        an_entry("/somewhere/project-worktrees/corsa-7/implementa", 1_000, 11),
        an_entry("/somewhere/project-worktrees/corsa-7/verifica", 4_600, 22),
    ];

    let shown = render_open(&held, 11_800, |tree| tree.opened_by_pid == 22);
    let first = shown.lines().next().expect("the first tree");
    let second = shown.lines().nth(1).expect("the second tree");

    assert!(first.contains("corsa-7") && first.contains("implementa"), "{first}");
    assert!(first.contains("3h") && second.contains("2h"), "{shown}");
    assert!(first.contains("gone"), "a dead opener passed for a live one: {first}");
    assert!(second.contains("still running"), "{second}");
    assert_eq!(render_open(&[], 0, |_| true), "Sailor has no tree open: nothing it cut is still standing.");
}

/// **THE NUMBER IS TAKEN, THE TREE IS STILL NOBODY'S.** A row saying when its
/// opener was born reads a reused number for what it is; without that second
/// the listing can only say whether the number is taken, and says so of a row
/// nobody is behind any more.
#[test]
fn a_tree_whose_opener_was_born_at_another_second_is_shown_as_gone() {
    let mine = std::process::id();
    let born = ledger::born_second_of(mine).expect("this machine says when a process was born");
    let held = [
        born_entry("/somewhere/project-worktrees/corsa-7/mio", 1_000, mine, Some(born)),
        born_entry("/somewhere/project-worktrees/corsa-7/altrui", 1_000, mine, Some(born - 1)),
    ];

    let shown = render_open(&held, 11_800, still_the_opener);

    let first = shown.lines().next().expect("the first tree");
    let second = shown.lines().nth(1).expect("the second tree");
    assert!(first.contains("still running"), "this very process holds it: {first}");
    assert!(
        second.contains("gone"),
        "the same number under another second passed for the opener: {second}"
    );
}

/// **THE ONE THAT MATTERS, THROUGH THE GESTURE A PERSON TYPES.** The sweep
/// takes down what the trunk already holds and leaves standing, by name, the
/// tree carrying work nobody else has.
#[test]
fn the_sweep_takes_down_the_merged_trees_and_names_the_ones_holding_work() {
    let scratch = a_scratch("sweeping");
    let repo = a_repository_in(&scratch);
    let store = ledger::Ledger::open(scratch.join("store")).expect("a store");
    let merged = workspace::create(&repo, "work/gia-dentro", None, &store).expect("a merged tree");
    let ahead =
        workspace::create(&repo, "work/ancora-fuori", None, &store).expect("a tree of its own");
    let dirty =
        workspace::create(&repo, "work/mai-committato", None, &store).expect("a third tree");
    std::fs::write(ahead.join("answer"), "a night of work\n").expect("work");
    run_git(&ahead, &["add", "answer"]);
    run_git(&ahead, &["commit", "-q", "-m", "not in the trunk"]);
    std::fs::write(dirty.join("half"), "half a thought\n").expect("work");
    for tree in [&merged, &ahead, &dirty] {
        written_down(&store, &repo, tree);
    }

    let said = sweep(&repo, &store as &dyn OpenTrees, &let_go_long_ago(&[], &[]), &IdentityRule::default()).expect("the sweep runs");
    let merged_is_gone = !merged.exists();
    let work_is_there = ahead.join("answer").exists() && dirty.join("half").exists();
    let _ = std::fs::remove_dir_all(&scratch);

    assert!(merged_is_gone, "a merged tree was kept:\n{said}");
    assert!(work_is_there, "the sweep lost work:\n{said}");
    assert!(said.contains("work/ancora-fuori"), "the kept tree was not named:\n{said}");
    assert!(said.contains("mai-committato"), "the dirty tree was not named:\n{said}");
    assert!(said.contains("1 taken down, 2 kept"), "{said}");
}

/// **A ROW OVER A DIRECTORY THAT IS GONE IS A LEFTOVER THE SWEEP MUST CLEAR.**
/// The sweep walked git's list of trees, where such a row never appears, so
/// eleven of them stood for two days and kept waking the flow that exists to
/// clear them — fault 166, second face.
#[test]
fn the_sweep_clears_a_row_whose_tree_is_no_longer_on_disk() {
    let scratch = a_scratch("stale-row");
    let repo = a_repository_in(&scratch);
    let store = ledger::Ledger::open(scratch.join("store")).expect("a store");
    let cut = workspace::tree_for(&repo, "run-sparito", "a_step", &store as &dyn OpenTrees, None)
        .expect("a tree, written down as it is cut");
    // Taken away behind git's back, which is what a killed crew agent leaves.
    std::fs::remove_dir_all(&cut).expect("the directory goes");
    run_git(&repo, &["worktree", "prune"]);
    let held_before = store.trees_left_open().expect("the rows").len();

    let said = sweep(&repo, &store as &dyn OpenTrees, &let_go_long_ago(&[], &[]), &IdentityRule::default()).expect("the sweep runs");
    let held_after = store.trees_left_open().expect("the rows").len();
    let _ = std::fs::remove_dir_all(&scratch);

    assert_eq!(held_before, 1, "the row was never written: {said}");
    assert_eq!(held_after, 0, "the row over a gone directory was kept:\n{said}");
    assert!(said.contains("run-sparito"), "the row was cleared in silence:\n{said}");
}

/// **THE ONE THAT COST A SESSION ITS GROUND.** A tree whose work the trunk
/// already holds and that git would take down without a word is kept, and
/// named, while somebody is standing in it.
#[test]
fn a_merged_tree_somebody_is_working_in_is_kept_and_named() {
    let scratch = a_scratch("occupied");
    let repo = a_repository_in(&scratch);
    let store = ledger::Ledger::open(scratch.join("store")).expect("a store");
    let busy = workspace::create(&repo, "work/qualcuno-dentro", None, &store).expect("a merged tree");
    written_down(&store, &repo, &busy);

    let said = sweep(&repo, &store as &dyn OpenTrees, &let_go_long_ago(std::slice::from_ref(&busy), &[]), &IdentityRule::default()).expect("the sweep runs");
    let still_there = busy.exists();

    // And with nobody in it the same tree goes, or the guard is a sweep that
    // never sweeps.
    let again = sweep(&repo, &store as &dyn OpenTrees, &let_go_long_ago(&[], &[]), &IdentityRule::default()).expect("the sweep runs");
    let gone = !busy.exists();
    let _ = std::fs::remove_dir_all(&scratch);

    assert!(still_there, "a tree somebody was working in was taken down:\n{said}");
    assert!(said.contains("qualcuno-dentro"), "it was kept in silence:\n{said}");
    assert!(gone, "with nobody in it the tree was still kept:\n{again}");
}

/// **A THING SAILOR NEVER TOOK IS NOT SAILOR'S TO TAKE.** A tree another tab cut
/// by hand, still at the trunk, reads as one whose work the trunk already holds;
/// the sweep took two such trees down twice in ten minutes.
#[test]
fn a_tree_sailor_never_cut_is_named_and_never_taken_down() {
    let scratch = a_scratch("never-cut");
    let repo = a_repository_in(&scratch);
    let store = ledger::Ledger::open(scratch.join("store")).expect("a store");
    let by_hand = tree_path(&repo, "tagliato-a-mano");
    run_git(
        &repo,
        &[
            "worktree",
            "add",
            &by_hand.to_string_lossy(),
            "-b",
            "work/tagliato-a-mano",
        ],
    );

    let said = sweep(&repo, &store as &dyn OpenTrees, &let_go_long_ago(&[], &[]), &IdentityRule::default()).expect("the sweep runs");
    let still_there = by_hand.exists();

    written_down(&store, &repo, &by_hand);
    let again = sweep(&repo, &store as &dyn OpenTrees, &let_go_long_ago(&[], &[]), &IdentityRule::default()).expect("the sweep runs");
    let gone = !by_hand.exists();
    let _ = std::fs::remove_dir_all(&scratch);

    assert!(still_there, "a tree Sailor never cut was taken down:\n{said}");
    assert!(said.contains("tagliato-a-mano"), "it was kept in silence:\n{said}");
    assert!(gone, "written down and let go, the tree was still kept:\n{again}");
}

/// **A TREE CUT BY THE GESTURE A PERSON TYPES IS ONE SAILOR ANSWERS FOR.** It
/// was cut and never written down, so the flow that closes finished work could
/// only name it: the branch merged and the tree stayed. The row is written by
/// the door that cuts, so `sailor worktree create` and the window get it
/// without either of them remembering to.
#[test]
fn a_tree_cut_for_a_branch_is_written_down_as_it_is_cut() {
    let scratch = a_scratch("written-as-cut");
    let repo = a_repository_in(&scratch);
    let store = ledger::Ledger::open(scratch.join("store")).expect("a store");

    let cut_at =
        workspace::create(&repo, "work/scritto-subito", None, &store).expect("the tree is cut");
    let rows = store.trees_left_open().expect("the rows");
    let _ = std::fs::remove_dir_all(&scratch);

    let said = cut_at.to_string_lossy();
    assert!(
        rows.iter()
            .any(|row| row.path == said && row.step == "work/scritto-subito"),
        "the tree {said} was cut and not written down: {rows:?}"
    );
}

/// **A TREE TAKEN DOWN GOES OFF THE PAGE WITH IT.** Cutting wrote the row and
/// the gesture that removes one by name never paid it back, so `sailor
/// worktree open` went on listing trees that are gone and the flow that closes
/// finished work was answering for disk nobody would ever claim.
#[test]
fn a_tree_taken_down_by_name_leaves_the_register_with_it() {
    let scratch = a_scratch("removed-leaves");
    let repo = a_repository_in(&scratch);
    let store = ledger::Ledger::open(scratch.join("store")).expect("a store");
    let cut = workspace::create(&repo, "work/tolto-per-nome", None, &store).expect("a tree");
    let listed = store.trees_left_open().expect("the rows");

    let said = sailor::worktree_cmd::remove_one(
        &repo,
        "tolto-per-nome",
        &store as &dyn OpenTrees,
        &IndexTending::of(&repo),
    )
    .expect("the tree comes down");
    let after = store.trees_left_open().expect("the rows");
    let gone = !cut.exists();
    let _ = std::fs::remove_dir_all(&scratch);

    assert_eq!(
        listed.len(),
        1,
        "the tree was never written down: {listed:?}"
    );
    assert!(gone, "the tree is still on disk:\n{said}");
    assert!(
        after.is_empty(),
        "the tree is gone and the register still offers it: {after:?}"
    );
}

/// A tree cut minutes ago is somebody's first gesture, whoever wrote it down.
#[test]
fn a_tree_cut_less_than_an_hour_ago_stays_whoever_cut_it() {
    let scratch = a_scratch("young");
    let repo = a_repository_in(&scratch);
    let store = ledger::Ledger::open(scratch.join("store")).expect("a store");
    let young = workspace::create(&repo, "work/appena-tagliato", None, &store).expect("a tree");
    written_down(&store, &repo, &young);
    let nobody = |_: &OpenTree| Whose::Nobody;
    let right_now = Holders { occupied: &[], standing: &[], owner: &nobody, now: seconds_now() };

    let said = sweep(&repo, &store as &dyn OpenTrees, &right_now, &IdentityRule::default()).expect("the sweep runs");
    let still_there = young.exists();
    let later = sweep(&repo, &store as &dyn OpenTrees, &let_go_long_ago(&[], &[]), &IdentityRule::default()).expect("the sweep runs");
    let gone = !young.exists();
    let _ = std::fs::remove_dir_all(&scratch);

    assert!(still_there, "a tree cut a moment ago was taken down:\n{said}");
    assert!(said.contains("appena-tagliato"), "it was kept in silence:\n{said}");
    assert!(gone, "past the hour and let go, the tree was still kept:\n{later}");
}

/// **THE CHAIN, NOT THE REGISTER.** A row alone is not permission: the process
/// that cut the tree is asked first.
#[test]
fn a_tree_whose_opener_is_still_there_stays() {
    let scratch = a_scratch("opener-alive");
    let repo = a_repository_in(&scratch);
    let store = ledger::Ledger::open(scratch.join("store")).expect("a store");
    let held = workspace::create(&repo, "work/ancora-tenuto", None, &store).expect("a tree");
    written_down(&store, &repo, &held);
    let alive = |_: &OpenTree| Whose::TheProcessThatTookIt;
    let its_opener_is_there = Holders { occupied: &[], standing: &[], owner: &alive, now: i64::MAX / 2 };

    let said = sweep(&repo, &store as &dyn OpenTrees, &its_opener_is_there, &IdentityRule::default()).expect("the sweep runs");
    let still_there = held.exists();
    let _ = std::fs::remove_dir_all(&scratch);

    assert!(still_there, "a tree its opener still holds was taken down:\n{said}");
    assert!(said.contains("ancora-tenuto"), "it was kept in silence:\n{said}");
}

/// **A TERMINAL IS NOT THE ONLY WAY TO STAND IN A TREE.** A process with its
/// directory inside is read off the machine, and named by its pid.
#[test]
fn a_tree_a_process_works_in_stays_and_the_process_is_named() {
    let scratch = a_scratch("a-process-in-it");
    let repo = a_repository_in(&scratch);
    let store = ledger::Ledger::open(scratch.join("store")).expect("a store");
    let busy = workspace::create(&repo, "work/un-processo-dentro", None, &store).expect("a tree");
    written_down(&store, &repo, &busy);
    let mut inside = Command::new("sleep").arg("60").current_dir(&busy).spawn().expect("a process inside");

    let standing = machine::where_processes_stand().expect("the machine lists its processes");
    let said = sweep(&repo, &store as &dyn OpenTrees, &let_go_long_ago(&[], &standing), &IdentityRule::default()).expect("the sweep runs");
    let still_there = busy.exists();
    let _ = inside.kill();
    let _ = inside.wait();
    let _ = std::fs::remove_dir_all(&scratch);

    assert!(still_there, "a tree a process was working in was taken down:\n{said}");
    assert!(said.contains(&inside.id().to_string()), "the process was not named:\n{said}");
}

/// A tree written down the way Sailor writes down the trees it cuts.
fn written_down(store: &dyn OpenTrees, repo: &Path, tree: &Path) {
    store
        .tree_opened(&OpenTree {
            path: tree.to_string_lossy().into_owned(),
            repo: repo.to_string_lossy().into_owned(),
            run: "run-finita".to_owned(),
            step: "a_step".to_owned(),
            opened_by_pid: 1,
            opened_at: 0,
            opened_by_born_at: None,
        })
        .expect("the register takes it");
}

fn let_go(_: &OpenTree) -> Whose {
    Whose::Nobody
}

/// Everybody let go, and every tree is far past the hour.
fn let_go_long_ago<'a>(occupied: &'a [PathBuf], standing: &'a [(u32, PathBuf)]) -> Holders<'a> {
    Holders { occupied, standing, owner: &let_go, now: i64::MAX / 2 }
}

fn seconds_now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |since| since.as_secs() as i64)
}

fn a_scratch(label: &str) -> PathBuf {
    let path = std::env::temp_dir().join(format!("sailor-worktree-cmd-{label}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&path);
    std::fs::create_dir_all(&path).expect("a scratch");
    path
}

/// Only the repository's own settings: nothing of the account running this.
fn run_git(at: &Path, args: &[&str]) {
    let done = Command::new("git")
        .arg("-C")
        .arg(at)
        .args(args)
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .output()
        .expect("git runs");
    assert!(done.status.success(), "git {args:?}: {}", String::from_utf8_lossy(&done.stderr));
}

/// A repository whose trunk is named the way this project names its trunk:
/// the sweep asks git whether the trunk holds a tree, and no other branch is
/// the trunk.
fn a_repository_in(scratch: &Path) -> PathBuf {
    let repo = scratch.join("project");
    std::fs::create_dir_all(&repo).expect("the repository");
    run_git(&repo, &["init", "-q"]);
    run_git(&repo, &["config", "user.email", "prove@example"]);
    run_git(&repo, &["config", "user.name", "prove"]);
    std::fs::write(repo.join("README"), "a tree to cut from\n").expect("a file");
    run_git(&repo, &["add", "README"]);
    run_git(&repo, &["commit", "-q", "-m", "the first"]);
    run_git(&repo, &["branch", "-M", A_TRUNK]);
    run_git(&repo, &["config", workspace::branches::TRUNK_KEY, A_TRUNK]);
    repo
}
