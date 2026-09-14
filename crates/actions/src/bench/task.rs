//! One task of the frozen benchmark, as it is written to disk and read back by
//! the builder, the judge and the baseline run.

use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// Where the sets live under Sailor's home: `bench/<set>/<task>.json`, beside
/// the flows and the price list and outside every tree an engine can read.
pub const BENCH_DIR: &str = "bench";

/// A change made against `base_commit`, judged by tests it never sees.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Task {
    pub id: String,
    /// The repository the commits belong to, as a path on this machine.
    pub repo: String,
    pub base_commit: String,
    pub fix_commit: String,
    /// What the agent is told: the fault's own words, or the commit's reason.
    pub prompt: String,
    /// `fault <n>` or `commit body`, so a reader knows which it was.
    pub prompt_source: String,
    /// The test files the fix commit added or changed, relative to the repo.
    pub test_files: Vec<String>,
    /// The unified diff of `test_files` between base and fix.
    pub hidden_test_patch: String,
    /// The unified diff of every other file between base and fix.
    pub gold_patch: String,
    pub gold_added_lines: u64,
    /// The command that runs the hidden tests, argument by argument, from the
    /// root of the tree.
    pub test_command: Vec<String>,
    #[serde(default)]
    pub validated: Option<Validation>,
}

/// How many times the hidden tests were seen red on the base and green on the
/// fix, and when.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct Validation {
    pub red_on_base: u32,
    pub green_on_fix: u32,
    pub validated_at: i64,
}

impl Task {
    pub fn path_in(home: &Path, set: &str, id: &str) -> PathBuf {
        home.join(BENCH_DIR).join(set).join(format!("{id}.json"))
    }

    pub fn load(path: &Path) -> Result<Task, String> {
        let text = std::fs::read_to_string(path)
            .map_err(|error| format!("{}: {error}", path.display()))?;
        serde_json::from_str(&text).map_err(|error| format!("{}: {error}", path.display()))
    }

    pub fn save(&self, path: &Path) -> Result<(), String> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|error| format!("{}: {error}", parent.display()))?;
        }
        let text = serde_json::to_string_pretty(self).map_err(|error| error.to_string())?;
        std::fs::write(path, text).map_err(|error| format!("{}: {error}", path.display()))
    }

    /// Every task of a set, in file-name order.
    pub fn load_set(home: &Path, set: &str) -> Result<Vec<Task>, String> {
        let dir = home.join(BENCH_DIR).join(set);
        let mut paths: Vec<PathBuf> = std::fs::read_dir(&dir)
            .map_err(|error| format!("{}: {error}", dir.display()))?
            .filter_map(Result::ok)
            .map(|entry| entry.path())
            .filter(|path| path.extension().is_some_and(|ext| ext == "json"))
            .collect();
        paths.sort();
        paths.iter().map(|path| Task::load(path)).collect()
    }
}

/// Whether a path names a test file: anything under a `tests/` directory, a
/// `tests.rs`, or a `*_test.rs`. The judge and the builder share this rule so
/// what one hides is exactly what the other refuses to see touched.
pub fn is_test_path(path: &str) -> bool {
    let normalised = path.replace('\\', "/");
    let name = normalised.rsplit('/').next().unwrap_or(&normalised);
    normalised.split('/').any(|segment| segment == "tests")
        || name == "tests.rs"
        || name.ends_with("_test.rs")
}

/// The files a unified diff touches, from its `+++ b/` lines.
pub fn files_in_diff(diff: &str) -> Vec<String> {
    diff.lines()
        .filter_map(|line| line.strip_prefix("+++ b/"))
        .map(|path| path.trim().to_owned())
        .collect()
}

/// Whether a diff adds or removes a test attribute inside a non-test file:
/// the second way of touching the tests, which the path rule cannot see.
pub fn diff_touches_test_attributes(diff: &str) -> bool {
    diff.lines()
        .filter(|line| (line.starts_with('+') || line.starts_with('-')) && !line.starts_with("+++") && !line.starts_with("---"))
        .any(|line| {
            let body = line[1..].trim_start();
            body.starts_with("#[test]")
                || body.starts_with("#[ignore")
                || body.starts_with("#[cfg(test)]")
                || body.starts_with("#[should_panic")
        })
}

/// The number of `+` lines of a diff, its headers left out.
pub fn added_lines(diff: &str) -> u64 {
    diff.lines()
        .filter(|line| line.starts_with('+') && !line.starts_with("+++"))
        .count() as u64
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_test_file_is_known_by_its_path() {
        assert!(is_test_path("crates/flow/tests/a_flow.rs"));
        assert!(is_test_path("crates/ledger/src/tests.rs"));
        assert!(is_test_path("crates/x/src/thing_test.rs"));
        assert!(!is_test_path("crates/flow/src/executor.rs"));
        assert!(!is_test_path("crates/flow/src/testsuite.rs"));
    }

    #[test]
    fn a_diff_that_adds_a_test_attribute_touches_the_tests() {
        let diff = "--- a/crates/x/src/lib.rs\n+++ b/crates/x/src/lib.rs\n@@ -1 +1,3 @@\n fn a() {}\n+#[test]\n+fn b() {}\n";
        assert!(diff_touches_test_attributes(diff));
        let plain = "--- a/crates/x/src/lib.rs\n+++ b/crates/x/src/lib.rs\n@@ -1 +1,2 @@\n fn a() {}\n+fn b() {}\n";
        assert!(!diff_touches_test_attributes(plain));
        assert_eq!(files_in_diff(plain), vec!["crates/x/src/lib.rs"]);
        assert_eq!(added_lines(plain), 1);
    }

    #[test]
    fn a_task_round_trips_through_its_file() {
        let dir = std::env::temp_dir().join(format!("sailor-bench-task-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let task = Task {
            id: "t1".into(),
            repo: "/repo".into(),
            base_commit: "a".into(),
            fix_commit: "b".into(),
            prompt: "it broke".into(),
            prompt_source: "fault 1".into(),
            test_files: vec!["crates/x/tests/t.rs".into()],
            hidden_test_patch: String::new(),
            gold_patch: String::new(),
            gold_added_lines: 0,
            test_command: vec!["cargo".into(), "test".into()],
            validated: None,
        };
        let path = Task::path_in(&dir, "stage-1", "t1");
        task.save(&path).unwrap();
        assert_eq!(Task::load(&path).unwrap(), task);
        assert_eq!(Task::load_set(&dir, "stage-1").unwrap(), vec![task]);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
