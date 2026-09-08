//! The trees a repository is checked out into.
//!
//! One copy, read by the command line and by the window alike: two would
//! answer differently about which branch a tree is on, and `remove` acts on it.

pub mod branches;
pub mod ratchet;

use std::path::{Path, PathBuf};
use std::process::Command;

/// One checkout of a repository, as git reports it.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct Worktree {
    pub path: String,
    pub head: String,
    /// `None` when the tree is on a detached head, which is a state a person
    /// can be surprised by: it has no branch to go back to.
    pub branch: Option<String>,
    pub locked: bool,
    /// Git can already tell that a tree's directory is gone.
    pub prunable: bool,
}

impl Worktree {
    /// The last segment of the path, which is what a person calls this tree.
    pub fn name(&self) -> &str {
        Path::new(&self.path)
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or(&self.path)
    }
}

/// Reads `git worktree list --porcelain`.
///
/// The porcelain form and not the human one, which aligns columns with spaces:
/// a path with a space in it cannot then be told from the column after it, and
/// a name a person chose is where a space turns up.
pub fn parse_worktrees(porcelain: &str) -> Vec<Worktree> {
    let mut trees = Vec::new();
    let mut current: Option<Worktree> = None;
    for line in porcelain.lines() {
        let line = line.trim_end();
        if let Some(path) = line.strip_prefix("worktree ") {
            if let Some(tree) = current.take() {
                trees.push(tree);
            }
            current = Some(Worktree {
                path: path.to_owned(),
                head: String::new(),
                branch: None,
                locked: false,
                prunable: false,
            });
            continue;
        }
        let Some(tree) = current.as_mut() else {
            continue;
        };
        if let Some(head) = line.strip_prefix("HEAD ") {
            tree.head = head.to_owned();
        } else if let Some(branch) = line.strip_prefix("branch ") {
            tree.branch = Some(branch.trim_start_matches("refs/heads/").to_owned());
        } else if line == "locked" || line.starts_with("locked ") {
            tree.locked = true;
        } else if line == "prunable" || line.starts_with("prunable ") {
            tree.prunable = true;
        }
    }
    if let Some(tree) = current.take() {
        trees.push(tree);
    }
    trees
}

const BESIDE_A_CHECKOUT: &str = "-worktrees";

/// Where the trees of a repository go: beside it, not inside it. Inside, every
/// tool that walks the project would walk every tree of it as well.
pub fn trees_root(repo: &Path) -> PathBuf {
    let stem = repo
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("repo");
    let parent = repo.parent().map(Path::to_path_buf).unwrap_or_default();
    parent.join(format!("{stem}{BESIDE_A_CHECKOUT}"))
}

/// Where a new tree goes.
pub fn tree_path(repo: &Path, name: &str) -> PathBuf {
    trees_root(repo).join(name)
}

/// A branch name is not a directory name: `work/thing` would nest a directory.
pub fn name_for(branch: &str) -> String {
    branch.rsplit('/').next().unwrap_or(branch).to_owned()
}

fn git(repo: &Path, args: &[&str]) -> Result<String, String> {
    let output = Command::new("git")
        .arg("-C")
        .arg(repo)
        .args(args)
        .output()
        .map_err(|error| format!("cannot run git: {error}"))?;
    if !output.status.success() {
        return Err(String::from_utf8_lossy(&output.stderr).trim().to_owned());
    }
    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}

pub fn list(repo: &Path) -> Result<Vec<Worktree>, String> {
    Ok(parse_worktrees(&git(
        repo,
        &["worktree", "list", "--porcelain"],
    )?))
}

/// Local refs only: a remote-tracking name is somebody else's choice.
pub fn branch_names(repo: &Path) -> Result<Vec<String>, String> {
    let listed = git(repo, &["for-each-ref", "--format=%(refname:short)", "refs/heads"])?;
    Ok(listed
        .lines()
        .map(str::trim)
        .filter(|name| !name.is_empty())
        .map(str::to_owned)
        .collect())
}

/// Cuts a tree for `branch`, creating the branch if it does not exist yet.
pub fn create(repo: &Path, branch: &str, name: Option<&str>) -> Result<PathBuf, String> {
    let name = name.map(str::to_owned).unwrap_or_else(|| name_for(branch));
    let path = tree_path(repo, &name);
    if path.exists() {
        return Err(format!("{} already exists", path.display()));
    }
    let known = git(
        repo,
        &["rev-parse", "--verify", &format!("refs/heads/{branch}")],
    )
    .is_ok();
    let target = path.to_string_lossy().into_owned();
    let args: Vec<&str> = if known {
        vec!["worktree", "add", &target, branch]
    } else {
        vec!["worktree", "add", &target, "-b", branch]
    };
    git(repo, &args)?;
    Ok(path)
}

/// A tree Sailor cut and is answerable for until somebody closes it. The opener
/// is a pid because a tree outlives its run exactly when that process died, and
/// only a pid can be asked whether it is still breathing. See fault 97.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct OpenTree {
    pub path: String,
    pub repo: String,
    pub run: String,
    pub step: String,
    pub opened_by_pid: u32,
    pub opened_at: i64,
    /// Handed in: this crate talks to git, not to the kernel. `None` is unasked.
    #[serde(default)]
    pub opened_by_born_at: Option<i64>,
}

/// Where the trees Sailor cuts are written down. A trait, so this crate keeps
/// talking to git and nothing else and whoever holds Sailor's state implements
/// it. No do-nothing register is offered: that is fault 97 itself.
pub trait OpenTrees {
    fn tree_opened(&self, tree: &OpenTree) -> Result<(), String>;
    fn tree_closed(&self, path: &str) -> Result<(), String>;
    fn trees_left_open(&self) -> Result<Vec<OpenTree>, String>;
}

fn now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|since| since.as_secs() as i64)
        .unwrap_or_default()
}

/// The tree one step of one run works in, detached so no branch is left behind.
/// An existing one is the answer: a retried step needs what its first attempt
/// left. Cutting and writing down are one gesture: a tree the register refused
/// goes straight back, since nobody could ever find it again.
pub fn tree_for(
    repo: &Path,
    run: &str,
    step: &str,
    register: &dyn OpenTrees,
    opened_by_born_at: Option<i64>,
) -> Result<PathBuf, String> {
    let path = tree_path(repo, &format!("{}/{}", safe(run), safe(step)));
    let opened = OpenTree {
        path: path.to_string_lossy().into_owned(),
        repo: repo.to_string_lossy().into_owned(),
        run: run.to_owned(),
        step: step.to_owned(),
        opened_by_pid: std::process::id(),
        opened_at: now(),
        opened_by_born_at,
    };
    if path.exists() {
        register.tree_opened(&opened)?;
        return Ok(path);
    }
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|error| format!("{}: {error}", parent.display()))?;
    }
    let target = path.to_string_lossy().into_owned();
    git(repo, &["worktree", "add", "--detach", &target, "HEAD"])?;
    if let Err(why) = register.tree_opened(&opened) {
        let _ = take_down(repo, &path);
        return Err(why);
    }
    Ok(path)
}

/// A run id becomes a directory name: a separator would cut the tree elsewhere.
fn safe(name: &str) -> String {
    name.chars()
        .map(|letter| if letter.is_ascii_alphanumeric() || letter == '-' || letter == '_' { letter } else { '-' })
        .collect()
}

/// What became of a tree asked to close. A kept tree carries why, not a
/// sentence: the words a person reads belong to whoever speaks to them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Closing {
    TakenDown,
    /// Git would not take it down, in git's own words.
    GitRefused(String),
    HoldsACommitNobodyElseHas(String),
}

/// What a person's sweep did with one tree. Kept apart from [`Closing`]: the
/// two extra answers are ones only a sweep can give.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Swept {
    Closed(Closing),
    /// Named by its branch, or by its head when it is on none.
    NotInTheTrunkYet(String),
    /// The directory was gone and the register was the last to hold it.
    AlreadyGone,
}

/// Takes down the tree cut for one step, once nobody is coming back to it.
/// Two things keep it, and neither is ever overridden: git's refusal over
/// uncommitted work, and a commit that only this tree holds. See fault 89.
/// Taking down and forgetting are one gesture, or Sailor keeps looking for it.
pub fn close_tree(repo: &Path, tree: &Path, register: &dyn OpenTrees) -> Closing {
    if let Some(commit) = a_commit_no_branch_holds(repo, tree) {
        return Closing::HoldsACommitNobodyElseHas(commit);
    }
    match take_down(repo, tree) {
        Err(refusal) => Closing::GitRefused(refusal),
        Ok(()) => {
            let _ = register.tree_closed(&tree.to_string_lossy());
            Closing::TakenDown
        }
    }
}

/// A person's sweep decides on history and not on files: nothing the trunk has
/// not got is touched, branch or detached. Over-conservative on purpose, since
/// a tree kept is only named and a tree taken down is gone.
pub fn close_if_the_trunk_holds_it(repo: &Path, tree: &Path, register: &dyn OpenTrees) -> Swept {
    if !tree.exists() {
        let _ = register.tree_closed(&tree.to_string_lossy());
        return Swept::AlreadyGone;
    }
    if !the_trunk_already_holds(repo, tree) {
        return Swept::NotInTheTrunkYet(what_it_carries(tree));
    }
    Swept::Closed(close_tree(repo, tree, register))
}

fn take_down(repo: &Path, tree: &Path) -> Result<(), String> {
    let at = tree.to_string_lossy().into_owned();
    git(repo, &["worktree", "remove", &at])?;
    // The run's directory goes with its last step: a full one errors.
    if let Some(parent) = tree.parent() {
        let _ = std::fs::remove_dir(parent);
    }
    Ok(())
}

/// Unreadable head, missing trunk, no git: all false, because the answer that
/// keeps a tree standing is the one that loses nothing.
pub fn the_trunk_already_holds(repo: &Path, tree: &Path) -> bool {
    let Ok(head) = git(tree, &["rev-parse", "HEAD"]) else {
        return false;
    };
    let head = head.trim();
    !head.is_empty()
        && git(
            repo,
            &["merge-base", "--is-ancestor", head, branches::TRUNK],
        )
        .is_ok()
}

/// The branch a tree is on, or its head when it is on none: the address a
/// person needs to go and look at what would have been lost.
fn what_it_carries(tree: &Path) -> String {
    let branch = git(tree, &["symbolic-ref", "--short", "HEAD"])
        .map(|name| name.trim().to_owned())
        .unwrap_or_default();
    if !branch.is_empty() {
        return branch;
    }
    git(tree, &["rev-parse", "--short", "HEAD"])
        .map(|head| head.trim().to_owned())
        .unwrap_or_default()
}

/// The head of `tree` when no branch of `repo` holds it. Git's refusal does
/// not cover a commit the engine made inside a detached tree, and taking the
/// tree down would leave that commit unreachable.
fn a_commit_no_branch_holds(repo: &Path, tree: &Path) -> Option<String> {
    let head = git(tree, &["rev-parse", "HEAD"]).ok()?;
    let head = head.trim().to_owned();
    if head.is_empty() {
        return None;
    }
    let holders = git(repo, &["branch", "--all", "--contains", &head]).unwrap_or_default();
    holders.trim().is_empty().then_some(head)
}

/// The run and the step a tree was cut for, or nothing for a tree a person
/// cut. Read off the shape `tree_for` builds and not off one checkout's trees
/// root: a run launched inside another checkout cuts beside that one.
pub fn run_and_step_of(tree: &Worktree) -> Option<(String, String)> {
    let step = Path::new(&tree.path);
    let run = step.parent()?;
    let beside = run.parent()?.file_name()?.to_str()?;
    if !beside.ends_with(BESIDE_A_CHECKOUT) {
        return None;
    }
    Some((
        run.file_name()?.to_str()?.to_owned(),
        step.file_name()?.to_str()?.to_owned(),
    ))
}

/// Takes a tree down. Git refuses while the tree holds uncommitted work, and
/// that refusal is kept: losing work is not a thing this command may do.
pub fn remove(repo: &Path, name: &str) -> Result<PathBuf, String> {
    let trees = list(repo)?;
    let found = trees
        .iter()
        .find(|tree| tree.name() == name)
        .ok_or_else(|| format!("no worktree called {name}"))?;
    let path = PathBuf::from(&found.path);
    git(repo, &["worktree", "remove", &found.path])?;
    Ok(path)
}

/// One file git reports as changed, with its two-letter porcelain status.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct ChangedFile {
    pub path: String,
    pub status: String,
}

/// The working tree against its last commit. `diff` is git's own text: a second
/// one would disagree with the terminal's and neither would say so.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct Changes {
    pub root: String,
    pub files: Vec<ChangedFile>,
    pub diff: String,
}

/// Reads `git status --porcelain -z`: two letters of status, a space, the
/// path, a NUL. The `-z` form because the line form quotes a path with a
/// space or an accent, and a quoted path is one no editor can open. A rename
/// carries the new name first and the old one as a second entry.
pub fn parse_status(porcelain: &str) -> Vec<ChangedFile> {
    let mut files = Vec::new();
    let mut entries = porcelain.split('\0').filter(|entry| !entry.is_empty());
    while let Some(entry) = entries.next() {
        if entry.len() < 4 {
            continue;
        }
        let (status, path) = entry.split_at(2);
        let path = path.strip_prefix(' ').unwrap_or(path);
        if status.starts_with('R') || status.starts_with('C') {
            entries.next();
        }
        files.push(ChangedFile {
            status: status.to_owned(),
            path: path.to_owned(),
        });
    }
    files
}

/// What changed in `root` since its last commit, as git says it.
///
/// Against `HEAD` so staged and unstaged changes both show: an agent that ran
/// `git add` has still changed the tree. With no commit yet there is no `HEAD`,
/// and the plain diff is what git can answer.
pub fn changes(root: &Path) -> Result<Changes, String> {
    let status = git(root, &["status", "--porcelain", "-z", "--untracked-files=all"])?;
    let diff = match git(root, &["diff", "HEAD"]) {
        Ok(text) => text,
        Err(_) => git(root, &["diff"])?,
    };
    Ok(Changes {
        root: root.to_string_lossy().into_owned(),
        files: parse_status(&status),
        diff,
    })
}

/// The repository the current directory belongs to.
pub fn root() -> Result<PathBuf, String> {
    let here = std::env::current_dir().map_err(|error| error.to_string())?;
    let top = Command::new("git")
        .arg("-C")
        .arg(&here)
        .args(["rev-parse", "--show-toplevel"])
        .output()
        .map_err(|error| format!("cannot run git: {error}"))?;
    if !top.status.success() {
        return Err("not inside a git repository".to_owned());
    }
    Ok(PathBuf::from(String::from_utf8_lossy(&top.stdout).trim()))
}

/// The checkout around a directory, as git names it and with every symlink
/// resolved, so the same tree reached by two paths is one tree. `None` outside
/// any repository, or with no git to ask.
pub fn tree_around(here: &Path) -> Option<PathBuf> {
    let top = Command::new("git")
        .arg("-C")
        .arg(here)
        .args(["rev-parse", "--show-toplevel"])
        .output()
        .ok()?;
    if !top.status.success() {
        return None;
    }
    let found = String::from_utf8_lossy(&top.stdout).trim().to_owned();
    if found.is_empty() {
        return None;
    }
    let path = PathBuf::from(found);
    Some(path.canonicalize().unwrap_or(path))
}

/// Whether a directory is the top of the repository that tracks it. Discovery
/// climbs: a tree unpacked under a repository's `target/` answers with it.
pub fn is_the_top_of_its_repository(here: &Path) -> bool {
    let Some(top) = tree_around(here) else {
        return false;
    };
    here.canonicalize().is_ok_and(|here| here == top)
}

/// What a judge prints when its subject is not in the tree it was pointed at.
pub const MEASURED_NOTHING: &str = "measured nothing:";

pub fn measured_nothing(because: &str) {
    println!("\n{MEASURED_NOTHING} {because}");
}

/// A judge's receipt, read back by `sailor ratchet`: hence a shared constant.
pub const MEASURED: &str = "measured:";
pub const AGAINST: &str = " against ";

pub fn measured(walked: usize, what: &str) {
    println!("\n{MEASURED} {walked} {what}");
}

pub fn measured_against(walked: usize, what: &str, held: usize, oracle: &str) {
    println!("\n{MEASURED} {walked} {what}{AGAINST}{held} {oracle}");
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A register of this test's own: the rules live here, the store elsewhere.
    #[derive(Default)]
    struct APage(std::sync::Mutex<Vec<OpenTree>>);

    impl APage {
        fn open_now(&self) -> Vec<OpenTree> {
            self.0.lock().map(|held| held.clone()).unwrap_or_default()
        }
    }

    impl OpenTrees for APage {
        fn tree_opened(&self, tree: &OpenTree) -> Result<(), String> {
            let mut held = self.0.lock().map_err(|_| "the page is poisoned".to_owned())?;
            held.retain(|known| known.path != tree.path);
            held.push(tree.clone());
            Ok(())
        }

        fn tree_closed(&self, path: &str) -> Result<(), String> {
            let mut held = self.0.lock().map_err(|_| "the page is poisoned".to_owned())?;
            held.retain(|known| known.path != path);
            Ok(())
        }

        fn trees_left_open(&self) -> Result<Vec<OpenTree>, String> {
            Ok(self.open_now())
        }
    }

    /// A register that refuses every write: the store is unreachable.
    struct ARefusal;

    impl OpenTrees for ARefusal {
        fn tree_opened(&self, _tree: &OpenTree) -> Result<(), String> {
            Err("the page will not take it".to_owned())
        }

        fn tree_closed(&self, _path: &str) -> Result<(), String> {
            Ok(())
        }

        fn trees_left_open(&self) -> Result<Vec<OpenTree>, String> {
            Ok(Vec::new())
        }
    }

    /// A scratch place of this test's own, never a directory of this machine.
    fn a_scratch(label: &str) -> PathBuf {
        let path = std::env::temp_dir().join(format!(
            "sailor-workspace-{label}-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&path);
        std::fs::create_dir_all(&path).expect("a scratch");
        path
    }

    /// Only the repository's own settings: nothing of the account running this.
    fn run_git(at: &Path, args: &[&str]) -> std::process::Output {
        Command::new("git")
            .arg("-C")
            .arg(at)
            .args(args)
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .output()
            .expect("git runs")
    }

    /// A repository with one commit: no tree can be cut from an empty history.
    fn a_repository(label: &str) -> (PathBuf, PathBuf) {
        let scratch = a_scratch(label);
        let repo = scratch.join("project");
        std::fs::create_dir_all(&repo).expect("the repository");
        for args in [
            &["init", "-q"][..],
            &["config", "user.email", "test@example"],
            &["config", "user.name", "test"],
        ] {
            assert!(run_git(&repo, args).status.success(), "git {args:?}");
        }
        std::fs::write(repo.join("README"), "a tree to cut from\n").expect("a file");
        assert!(run_git(&repo, &["add", "README"]).status.success());
        assert!(run_git(&repo, &["commit", "-q", "-m", "the first"]).status.success());
        (scratch, repo)
    }

    /// A step's tree that answered and left nothing goes; one holding work
    /// that is not committed stays, and says in git's words why.
    #[test]
    fn a_tree_holding_work_is_kept_and_a_clean_one_is_taken_down() {
        let (scratch, repo) = a_repository("closing");
        let page = APage::default();
        let clean = tree_for(&repo, "run-1", "clean", &page, None).expect("a tree");
        let dirty = tree_for(&repo, "run-1", "dirty", &page, None).expect("a tree");
        std::fs::write(dirty.join("left-behind"), "half a thought\n").expect("work left");

        let went = close_tree(&repo, &clean, &page);
        let stayed = close_tree(&repo, &dirty, &page);
        let clean_is_gone = !clean.exists();
        let work_is_there = dirty.join("left-behind").exists();
        let listed = String::from_utf8_lossy(&run_git(&repo, &["worktree", "list"]).stdout)
            .into_owned();
        let dirty = dirty.to_string_lossy().into_owned();
        let _ = std::fs::remove_dir_all(&scratch);

        assert_eq!(went, Closing::TakenDown);
        assert!(clean_is_gone, "the clean tree is still on disk");
        assert!(
            matches!(stayed, Closing::GitRefused(ref said) if !said.is_empty()),
            "{stayed:?}"
        );
        assert!(work_is_there, "the work was lost");
        assert!(listed.contains(&dirty), "{listed}");
    }

    /// The refusal is the safety property: what overrides it is never written.
    #[test]
    fn taking_a_tree_down_never_forces_it() {
        let source = include_str!("lib.rs");
        let overriding = format!("--{}", "force");
        assert!(!source.contains(&overriding), "the refusal can be overridden");
    }

    /// No branch holds a commit made inside a detached tree, so taking the
    /// tree down would lose it.
    #[test]
    fn a_tree_holding_a_commit_no_branch_has_is_kept() {
        let (scratch, repo) = a_repository("committed");
        let page = APage::default();
        let tree = tree_for(&repo, "run-2", "committed", &page, None).expect("a tree");
        std::fs::write(tree.join("answer"), "the engine's work\n").expect("work");
        assert!(run_git(&tree, &["add", "answer"]).status.success());
        assert!(run_git(&tree, &["commit", "-q", "-m", "what it found"]).status.success());

        let stayed = close_tree(&repo, &tree, &page);
        let there = tree.join("answer").exists();
        let _ = std::fs::remove_dir_all(&scratch);

        assert!(
            matches!(stayed, Closing::HoldsACommitNobodyElseHas(ref at) if at.len() >= 7),
            "{stayed:?}"
        );
        assert!(there, "the commit's tree was taken down anyway");
    }

    /// **FAULT 97.** Cutting writes down who asked, when and for which run and
    /// step; closing takes the entry away.
    #[test]
    fn a_tree_sailor_cuts_is_one_sailor_knows_it_has_open() {
        let (scratch, repo) = a_repository("written-down");
        let page = APage::default();

        let tree = tree_for(&repo, "run-4", "asks", &page, Some(1_700_000_000)).expect("a tree");
        let open = page.trees_left_open().expect("the page reads back");
        let closed = close_tree(&repo, &tree, &page);
        let after = page.trees_left_open().expect("the page reads back");
        let _ = std::fs::remove_dir_all(&scratch);

        assert_eq!(open.len(), 1, "{open:?}");
        assert_eq!(open[0].path, tree.to_string_lossy());
        assert_eq!(open[0].run, "run-4");
        assert_eq!(open[0].step, "asks");
        assert_eq!(open[0].opened_by_pid, std::process::id());
        assert_eq!(
            open[0].opened_by_born_at,
            Some(1_700_000_000),
            "the second the opener was born is written down, or a reused number              reads as the opener for ever"
        );
        assert!(open[0].opened_at > 0, "the tree was opened at no time");
        assert_eq!(closed, Closing::TakenDown);
        assert!(after.is_empty(), "a tree taken down is still on the page: {after:?}");
    }

    /// A tree nobody wrote down is the fault itself: the cut is undone.
    #[test]
    fn a_tree_the_register_refuses_is_never_left_standing() {
        let (scratch, repo) = a_repository("unwritten");

        let refused = tree_for(&repo, "run-5", "unwritten", &ARefusal, None).expect_err("no page, no tree");
        let listed = String::from_utf8_lossy(&run_git(&repo, &["worktree", "list"]).stdout)
            .into_owned();
        let standing = tree_path(&repo, "run-5/unwritten").exists();
        let _ = std::fs::remove_dir_all(&scratch);

        assert!(!refused.is_empty(), "the refusal said nothing");
        assert!(!standing, "the tree is on disk with nobody holding its address");
        assert!(!listed.contains("run-5"), "git still holds it:\n{listed}");
    }

    /// **THE ONE THAT MATTERS.** A tree whose branch the trunk has not got is
    /// named and left exactly where it is, work and all.
    #[test]
    fn a_sweep_never_closes_a_tree_whose_work_the_trunk_has_not_got() {
        let (scratch, repo) = a_repository("sweeping");
        assert!(run_git(&repo, &["branch", "-M", branches::TRUNK]).status.success());
        let page = APage::default();
        let merged = create(&repo, "work/gia-dentro", None).expect("a tree on a merged branch");
        let ahead = create(&repo, "work/ancora-fuori", None).expect("a tree on its own branch");
        std::fs::write(ahead.join("answer"), "a night of work\n").expect("work");
        assert!(run_git(&ahead, &["add", "answer"]).status.success());
        assert!(run_git(&ahead, &["commit", "-q", "-m", "not in the trunk"]).status.success());
        for tree in [&merged, &ahead] {
            page.tree_opened(&OpenTree {
                opened_by_born_at: None,
                path: tree.to_string_lossy().into_owned(),
                repo: repo.to_string_lossy().into_owned(),
                run: "by-hand".to_owned(),
                step: "by-hand".to_owned(),
                opened_by_pid: std::process::id(),
                opened_at: now(),
            })
            .expect("the page takes it");
        }

        let went = close_if_the_trunk_holds_it(&repo, &merged, &page);
        let stayed = close_if_the_trunk_holds_it(&repo, &ahead, &page);
        let work_is_there = ahead.join("answer").exists();
        let still_open = page.trees_left_open().expect("the page reads back");
        let _ = std::fs::remove_dir_all(&scratch);

        assert_eq!(went, Swept::Closed(Closing::TakenDown));
        assert_eq!(
            stayed,
            Swept::NotInTheTrunkYet("work/ancora-fuori".to_owned()),
            "a tree the trunk has not got was swept away"
        );
        assert!(work_is_there, "the work was lost");
        assert_eq!(still_open.len(), 1, "{still_open:?}");
        assert!(still_open[0].path.ends_with("ancora-fuori"), "{still_open:?}");
    }

    /// A tree removed by hand leaves the page, or the sweep keeps naming it.
    #[test]
    fn a_tree_already_gone_leaves_the_page() {
        let (scratch, repo) = a_repository("gone");
        let page = APage::default();
        let tree = tree_for(&repo, "run-6", "gone", &page, None).expect("a tree");
        std::fs::remove_dir_all(&tree).expect("somebody removed it by hand");

        let closing = close_if_the_trunk_holds_it(&repo, &tree, &page);
        let after = page.trees_left_open().expect("the page reads back");
        let _ = std::fs::remove_dir_all(&scratch);

        assert_eq!(closing, Swept::AlreadyGone);
        assert!(after.is_empty(), "{after:?}");
    }

    /// A tree cut under a run and a step says which; one a person cut does not.
    #[test]
    fn a_step_tree_says_which_run_and_step_it_belongs_to() {
        let of = |path: &str| {
            run_and_step_of(&Worktree {
                path: path.to_owned(),
                head: String::new(),
                branch: None,
                locked: false,
                prunable: false,
            })
        };

        assert_eq!(
            of("/somewhere/project-worktrees/run-1/implementa"),
            Some(("run-1".to_owned(), "implementa".to_owned()))
        );
        assert_eq!(
            of("/somewhere/a-copy-worktrees/run-1/implementa"),
            Some(("run-1".to_owned(), "implementa".to_owned())),
            "a run launched inside another checkout cuts beside that one"
        );
        assert_eq!(of("/somewhere/project-worktrees/a-branch"), None);
        assert_eq!(of("/elsewhere/run-1/implementa"), None);
    }

    /// A directory deep in a checkout names that checkout, in its real path;
    /// a directory outside every checkout names nothing.
    #[test]
    fn the_tree_around_a_directory_is_its_checkout_and_none_outside_one() {
        let scratch = std::env::temp_dir().join(format!("sailor-workspace-tree-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&scratch);
        let repo = scratch.join("a-checkout");
        let inside = repo.join("crates").join("deep");
        std::fs::create_dir_all(&inside).expect("scratch");
        let init = Command::new("git").arg("-C").arg(&repo).args(["init", "--quiet"]).status().expect("git");
        assert!(init.success());
        let outside = scratch.join("nowhere");
        std::fs::create_dir_all(&outside).expect("scratch");

        let found = tree_around(&inside);
        let none = tree_around(&outside);
        let real_repo = repo.canonicalize().expect("real");
        let _ = std::fs::remove_dir_all(&scratch);

        assert_eq!(found, Some(real_repo));
        assert!(none.is_none(), "{none:?}");
    }

    /// **FAULT 100.** A directory inside a repository answers like the top.
    #[test]
    fn only_the_top_of_a_repository_says_it_is_tracked_from_here() {
        let scratch = std::env::temp_dir().join(format!("sailor-workspace-top-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&scratch);
        let repo = scratch.join("a-checkout");
        let under = repo.join("target").join("unpacked");
        std::fs::create_dir_all(&under).expect("scratch");
        let init = Command::new("git").arg("-C").arg(&repo).args(["init", "--quiet"]).status().expect("git");
        assert!(init.success());

        let top = is_the_top_of_its_repository(&repo);
        let inside = is_the_top_of_its_repository(&under);
        let outside = is_the_top_of_its_repository(&scratch);
        let _ = std::fs::remove_dir_all(&scratch);

        assert!(top, "the checkout is the top of itself");
        assert!(!inside, "a tree unpacked under target/ is not the top of the repository around it");
        assert!(!outside, "no repository, no top");
    }
}
