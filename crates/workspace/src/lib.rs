//! The trees a repository is checked out into.
//!
//! One copy, read by the command line and by the window alike. Two copies of
//! this would answer differently about which branch a tree is on, and the one
//! that acts on the answer is `remove`.

pub mod branches;

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
/// The porcelain form and not the human one: the human one aligns columns with
/// spaces, so a path with a space in it cannot be told from the column after
/// it, and a name a person chose is exactly where a space turns up.
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

/// The tree one step of one run works in, detached so no branch is left behind.
/// An existing one is the answer: a retried step needs what its first attempt
/// left.
pub fn tree_for(repo: &Path, run: &str, step: &str) -> Result<PathBuf, String> {
    let path = tree_path(repo, &format!("{}/{}", safe(run), safe(step)));
    if path.exists() {
        return Ok(path);
    }
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|error| format!("{}: {error}", parent.display()))?;
    }
    let target = path.to_string_lossy().into_owned();
    git(repo, &["worktree", "add", "--detach", &target, "HEAD"])?;
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

/// Takes down the tree cut for one step, once nobody is coming back to it.
/// Two things keep it, and neither is ever overridden: git's refusal over
/// uncommitted work, and a commit that only this tree holds. See fault 89.
pub fn close_tree(repo: &Path, tree: &Path) -> Closing {
    if let Some(commit) = a_commit_no_branch_holds(repo, tree) {
        return Closing::HoldsACommitNobodyElseHas(commit);
    }
    let at = tree.to_string_lossy().into_owned();
    match git(repo, &["worktree", "remove", &at]) {
        Err(refusal) => Closing::GitRefused(refusal),
        Ok(_) => {
            // The run's directory goes with its last step: a full one errors.
            if let Some(parent) = tree.parent() {
                let _ = std::fs::remove_dir(parent);
            }
            Closing::TakenDown
        }
    }
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
/// Against `HEAD` so that staged and unstaged changes both show: an agent
/// that ran `git add` has still changed the tree. A repository with no commit
/// yet has no `HEAD`, and then the plain diff is what git can answer.
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

#[cfg(test)]
mod tests {
    use super::*;

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
        let clean = tree_for(&repo, "run-1", "clean").expect("a tree");
        let dirty = tree_for(&repo, "run-1", "dirty").expect("a tree");
        std::fs::write(dirty.join("left-behind"), "half a thought\n").expect("work left");

        let went = close_tree(&repo, &clean);
        let stayed = close_tree(&repo, &dirty);
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
        let tree = tree_for(&repo, "run-2", "committed").expect("a tree");
        std::fs::write(tree.join("answer"), "the engine's work\n").expect("work");
        assert!(run_git(&tree, &["add", "answer"]).status.success());
        assert!(run_git(&tree, &["commit", "-q", "-m", "what it found"]).status.success());

        let stayed = close_tree(&repo, &tree);
        let there = tree.join("answer").exists();
        let _ = std::fs::remove_dir_all(&scratch);

        assert!(
            matches!(stayed, Closing::HoldsACommitNobodyElseHas(ref at) if at.len() >= 7),
            "{stayed:?}"
        );
        assert!(there, "the commit's tree was taken down anyway");
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
}
