//! The step that reads a repository's own fix commits and names the ones a
//! benchmark task could be cut from: a fix that touched a test file and a
//! file that is not one. It reads git and writes nothing.

use super::task::is_test_path;
use flow::{Action, ActionError, ActionOutcome, RedoEvidence, SharedState, StepRecord, StepSpecies};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::process::Command;

/// The name the step registers under.
pub const BENCH_CANDIDATES_ACTION: &str = "bench_candidates";

pub const DEFAULT_SINCE: &str = "2026-08-20";
pub const DEFAULT_LIMIT: usize = 200;
pub const DEFAULT_SET: &str = "stage-1";

/// The window's shell is a workspace of its own: its tests do not run from the
/// root of this tree, so a fix that only touched it is not a task here.
const OUTSIDE_THE_WORKSPACE: &str = "desktop/";

const KNOWN_FIELDS: &[&str] = &["repo", "since", "limit", "set", "text"];

#[derive(Debug, Deserialize)]
struct CandidatesInput {
    #[serde(default)]
    repo: Option<String>,
    #[serde(default)]
    since: Option<String>,
    #[serde(default)]
    limit: Option<usize>,
    #[serde(default)]
    set: Option<String>,
    /// The trigger's text: a JSON object of the fields above, which wins over
    /// them, so a launch can name the repository without editing the flow.
    #[serde(default)]
    text: Option<String>,
}

/// A fix commit a task could be cut from, with what the cut needs to know.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Candidate {
    pub repo: String,
    pub set: String,
    /// Where the task goes, when the step knows its home: a child run builds
    /// its registry over the machine's house, and the set must land where the
    /// parent that reads it back was built.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub home: Option<String>,
    pub fix_commit: String,
    pub base_commit: String,
    pub subject: String,
    pub test_files: Vec<String>,
    pub other_files: Vec<String>,
    pub fault: Option<i64>,
}

/// One commit as `git log` prints it under the format this step asks for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LoggedCommit {
    pub sha: String,
    pub parents: Vec<String>,
    pub subject: String,
    pub body: String,
}

/// Runs git in `repo` and hands back its standard output, or what it said on
/// the other pipe. A missing git and a directory that is not a repository are
/// both here: the caller tells them apart by the text.
pub(crate) fn git(repo: &Path, args: &[&str]) -> Result<String, String> {
    let output = Command::new("git")
        .arg("-C")
        .arg(repo)
        .args(args)
        .output()
        .map_err(|error| format!("git could not be started: {error}"))?;
    if !output.status.success() {
        return Err(String::from_utf8_lossy(&output.stderr).trim().to_owned());
    }
    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}

/// Splits the record-per-commit text of
/// `git log --format=%H%x00%P%x00%s%x00%b%x1e`.
pub fn parse_log(text: &str) -> Vec<LoggedCommit> {
    text.split('\u{1e}')
        .map(str::trim_start)
        .filter(|record| !record.trim().is_empty())
        .filter_map(|record| {
            let mut fields = record.splitn(4, '\0');
            let sha = fields.next()?.trim().to_owned();
            let parents = fields
                .next()?
                .split_whitespace()
                .map(str::to_owned)
                .collect();
            let subject = fields.next()?.trim().to_owned();
            let body = fields.next().unwrap_or("").trim().to_owned();
            Some(LoggedCommit {
                sha,
                parents,
                subject,
                body,
            })
        })
        .collect()
}

pub fn is_a_fix(subject: &str) -> bool {
    subject.trim_start().to_lowercase().starts_with("fix")
}

/// The number after the word «fault» in a text, if any: `fault 39`,
/// `fault #39`, `Fault 39.`.
pub fn fault_number(text: &str) -> Option<i64> {
    faults_named_in(text).first().copied()
}

/// Every number that follows the word «fault» in a text, in order.
pub fn faults_named_in(text: &str) -> Vec<i64> {
    let words: Vec<&str> = text.split_whitespace().collect();
    words
        .windows(2)
        .filter_map(|pair| {
            let word = pair[0].trim_matches(|c: char| !c.is_alphanumeric());
            if !word.eq_ignore_ascii_case("fault") {
                return None;
            }
            pair[1]
                .trim_matches(|c: char| !c.is_ascii_digit())
                .parse::<i64>()
                .ok()
        })
        .collect()
}

/// The files a commit touched, the window's shell left out.
pub(crate) fn files_of(repo: &Path, sha: &str) -> Result<Vec<String>, String> {
    Ok(git(
        repo,
        &["diff-tree", "--no-commit-id", "--name-only", "-r", sha],
    )?
    .lines()
    .map(str::trim)
    .filter(|line| !line.is_empty() && !line.starts_with(OUTSIDE_THE_WORKSPACE))
    .map(str::to_owned)
    .collect())
}

/// Test files and the others, from a commit's file list.
pub fn split_files(files: &[String]) -> (Vec<String>, Vec<String>) {
    files
        .iter()
        .cloned()
        .partition(|path| is_test_path(path))
}

/// The repository the step reads: the one named, else the run's root, else
/// where the process stands.
pub(crate) fn repo_of(named: Option<&str>, shared: &SharedState) -> Result<PathBuf, ActionError> {
    if let Some(path) = named.filter(|path| !path.trim().is_empty()) {
        return Ok(PathBuf::from(path));
    }
    if let Some(root) = shared.get(flow::WORKSPACE_ROOT).and_then(Value::as_str) {
        return Ok(PathBuf::from(root));
    }
    std::env::current_dir()
        .map_err(|error| ActionError::new("no_repository", format!("no repository was named and the current directory cannot be read: {error}")))
}

pub(crate) fn not_a_repository(repo: &Path, said: &str) -> ActionError {
    let said = format!("{}: {said}", repo.display());
    if said.contains("could not be started") {
        ActionError::new("git_missing", said)
    } else {
        ActionError::new("not_a_repository", said)
    }
}

pub struct BenchCandidatesAction {
    home: Option<PathBuf>,
}

impl BenchCandidatesAction {
    pub fn new(home: Option<PathBuf>) -> Self {
        Self { home }
    }
}

/// The three fields as asked, the trigger's text winning where it names one.
fn settle(asked: CandidatesInput) -> Result<CandidatesInput, ActionError> {
    let Some(text) = asked.text.as_deref().map(str::trim).filter(|text| !text.is_empty()) else {
        return Ok(asked);
    };
    let carried: CandidatesInput = serde_json::from_str(text).map_err(|error| {
        ActionError::new(
            "invalid_input",
            format!("the trigger's text is not a JSON object of repo, since, limit and set: {error}"),
        )
    })?;
    Ok(CandidatesInput {
        repo: carried.repo.or(asked.repo),
        since: carried.since.or(asked.since),
        limit: carried.limit.or(asked.limit),
        set: carried.set.or(asked.set),
        text: None,
    })
}

impl Action for BenchCandidatesAction {
    fn execute(&self, input: &Value, shared: &SharedState) -> Result<ActionOutcome, ActionError> {
        let asked: CandidatesInput = serde_json::from_value(input.clone()).map_err(|error| {
            ActionError::new(
                "invalid_input",
                format!("«{BENCH_CANDIDATES_ACTION}» takes repo, since, limit and set: {error}"),
            )
        })?;
        let asked = settle(asked)?;
        let repo = repo_of(asked.repo.as_deref(), shared)?;
        let since = asked.since.unwrap_or_else(|| DEFAULT_SINCE.to_owned());
        let limit = asked.limit.unwrap_or(DEFAULT_LIMIT);
        let set = asked.set.unwrap_or_else(|| DEFAULT_SET.to_owned());

        let log = git(
            &repo,
            &[
                "log",
                &format!("--since={since}"),
                "--no-merges",
                "--format=%H%x00%P%x00%s%x00%b%x1e",
            ],
        )
        .map_err(|said| not_a_repository(&repo, &said))?;
        let commits = parse_log(&log);
        let mut candidates = Vec::new();
        for commit in &commits {
            if candidates.len() >= limit {
                break;
            }
            if !is_a_fix(&commit.subject) {
                continue;
            }
            let Some(base) = commit.parents.first() else {
                continue;
            };
            let files = files_of(&repo, &commit.sha).map_err(|said| not_a_repository(&repo, &said))?;
            let (test_files, other_files) = split_files(&files);
            if test_files.is_empty() || other_files.is_empty() {
                continue;
            }
            candidates.push(Candidate {
                repo: repo.display().to_string(),
                set: set.clone(),
                home: self.home.as_ref().map(|home| home.display().to_string()),
                fix_commit: commit.sha.clone(),
                base_commit: base.clone(),
                subject: commit.subject.clone(),
                test_files,
                other_files,
                fault: fault_number(&commit.subject).or_else(|| fault_number(&commit.body)),
            });
        }
        Ok(ActionOutcome::Went(json!({
            "repo": repo.display().to_string(),
            "since": since,
            "limit": limit,
            "set": set,
            "scanned": commits.len(),
            "candidates": candidates,
        })))
    }

    fn unknown_fields(&self, declared: &Value) -> Vec<String> {
        declared
            .as_object()
            .map(|object| {
                object
                    .keys()
                    .filter(|name| !KNOWN_FIELDS.contains(&name.as_str()))
                    .cloned()
                    .collect()
            })
            .unwrap_or_default()
    }

    fn may_spend(&self, _declared: Option<&Value>) -> bool {
        false
    }

    fn redo_evidence(&self, _record: &StepRecord) -> RedoEvidence {
        RedoEvidence::TouchesNothing
    }

    fn species(&self) -> StepSpecies {
        StepSpecies::Repeatable
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bench::fixture::FixtureRepository;

    #[test]
    fn the_log_format_splits_into_commits_with_their_parents_and_bodies() {
        let text = "aaa\0bbb\0fix(x): one\0the body\nfault 7\u{1e}\nbbb\0\0feat: two\0\u{1e}\n";
        let commits = parse_log(text);
        assert_eq!(commits.len(), 2);
        assert_eq!(commits[0].sha, "aaa");
        assert_eq!(commits[0].parents, vec!["bbb"]);
        assert_eq!(commits[0].subject, "fix(x): one");
        assert_eq!(commits[0].body, "the body\nfault 7");
        assert!(commits[1].parents.is_empty(), "a root commit has no parent");
        assert_eq!(commits[1].body, "");
    }

    #[test]
    fn a_fault_number_is_the_number_after_the_word_fault() {
        assert_eq!(fault_number("closes fault 39 for good"), Some(39));
        assert_eq!(fault_number("Fault #12."), Some(12));
        assert_eq!(fault_number("the default faults nobody"), None);
        assert_eq!(fault_number("fault"), None);
        assert_eq!(fault_number("fault forty"), None);
        assert!(is_a_fix("Fix(sessions): a thing"));
        assert!(!is_a_fix("feat: fix nothing"));
    }

    /// The fixture's fix commit is listed with its base and its files; the
    /// commit that touched only a test file is not, and neither is the feature.
    #[test]
    fn a_fix_that_touched_a_test_and_the_code_is_a_candidate_and_nothing_else_is() {
        let fixture = FixtureRepository::new("candidates");
        let said = BenchCandidatesAction::new(Some(fixture.home.clone()))
            .execute(
                &json!({"repo": fixture.repo.display().to_string(), "since": "2000-01-01"}),
                &SharedState::new(),
            )
            .expect("git reads");
        let ActionOutcome::Went(said) = said else {
            panic!("reading a log is not a refusal");
        };
        let candidates = said["candidates"].as_array().expect("a list");
        let shas: Vec<&str> = candidates
            .iter()
            .map(|candidate| candidate["fix_commit"].as_str().expect("a sha"))
            .collect();
        assert_eq!(
            shas,
            vec![fixture.green_fix.as_str(), fixture.fix.as_str()],
            "newest first, the test-only commit and the feature left out: {said}"
        );
        let fix = &candidates[1];
        assert_eq!(fix["base_commit"], fixture.bug);
        assert_eq!(fix["test_files"], json!(["tests/adds.rs"]));
        assert_eq!(fix["other_files"], json!(["src/lib.rs"]));
        assert_eq!(fix["fault"], 7);
        assert_eq!(fix["set"], DEFAULT_SET);
        assert_eq!(fix["home"], fixture.home.display().to_string());
        assert_eq!(said["scanned"], 4);
    }

    #[test]
    fn the_limit_keeps_the_newest_and_the_trigger_text_wins_over_the_fields() {
        let fixture = FixtureRepository::new("candidates-limit");
        let text = json!({"repo": fixture.repo.display().to_string(), "limit": 1, "set": "stage-9"});
        let said = BenchCandidatesAction::new(None)
            .execute(
                &json!({"repo": "/nowhere", "since": "2000-01-01", "text": text.to_string()}),
                &SharedState::new(),
            )
            .expect("git reads");
        let ActionOutcome::Went(said) = said else {
            panic!("reading a log is not a refusal");
        };
        assert_eq!(said["candidates"].as_array().map(Vec::len), Some(1));
        assert_eq!(said["candidates"][0]["fix_commit"], fixture.green_fix);
        assert_eq!(said["set"], "stage-9");
    }

    #[test]
    fn a_directory_that_is_not_a_repository_is_an_error_and_not_an_empty_list() {
        let dir = std::env::temp_dir().join(format!("sailor-bench-not-a-repo-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("a directory");
        let refused = BenchCandidatesAction::new(None)
            .execute(&json!({"repo": dir.display().to_string()}), &SharedState::new())
            .expect_err("no repository is not zero candidates");
        assert_eq!(refused.class, "not_a_repository", "{}", refused.said);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
