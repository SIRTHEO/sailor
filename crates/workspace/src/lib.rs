//! The trees a repository is checked out into.
//!
//! One copy, read by the command line and by the window alike. Two copies of
//! this would answer differently about which branch a tree is on, and the one
//! that acts on the answer is `remove`.

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

/// Where a new tree goes: beside the repository, not inside it.
///
/// Inside would make every tree a directory the repository itself can see, and
/// every tool that walks the project would walk all of them. This is the shape
/// already on this machine, read rather than invented.
pub fn tree_path(repo: &Path, name: &str) -> PathBuf {
    let stem = repo
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("repo");
    let parent = repo.parent().map(Path::to_path_buf).unwrap_or_default();
    parent.join(format!("{stem}-worktrees")).join(name)
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
