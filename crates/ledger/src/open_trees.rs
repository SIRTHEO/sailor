//! The trees Sailor cut, kept where the rest of its state is kept.
//!
//! **IN THE STORE THAT WAS ALREADY THERE, NOT IN A TABLE OF ITS OWN.** The
//! generic collection is the space a caller fills with its own shape, and a
//! worktree is exactly that: `workspace` decides what an entry means, here it
//! only reaches the disk. See fault 97.

use crate::{Ledger, StoreRecord};
use serde_json::Value;
use workspace::{OpenTree, OpenTrees};

/// The one name for this collection. Two spellings would be two registers, and
/// the one anybody looks at would be the empty one — fault 12's shape.
pub const OPEN_TREES: &str = "open-worktrees";

/// The field that closes an entry. A store whose truth is appended does not
/// delete: the entry stays and stops counting as open.
const CLOSED_AT: &str = "closed_at";

fn now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |since| since.as_secs() as i64)
}

impl OpenTrees for Ledger {
    fn tree_opened(&self, tree: &OpenTree) -> Result<(), String> {
        let value = serde_json::to_value(tree).map_err(|error| error.to_string())?;
        self.put_record(&StoreRecord {
            collection: OPEN_TREES.to_owned(),
            key: tree.path.clone(),
            value,
            written_by: format!("pid {}", tree.opened_by_pid),
            written_at: tree.opened_at,
        })
        .map_err(|error| error.to_string())
    }

    fn tree_closed(&self, path: &str) -> Result<(), String> {
        let Some(mut held) = self
            .read_record(OPEN_TREES, path)
            .map_err(|error| error.to_string())?
        else {
            return Ok(());
        };
        if let Some(fields) = held.value.as_object_mut() {
            fields.insert(CLOSED_AT.to_owned(), Value::from(now()));
        }
        held.written_at = now();
        self.put_record(&held).map_err(|error| error.to_string())
    }

    fn trees_left_open(&self) -> Result<Vec<OpenTree>, String> {
        let held = self
            .records_in(OPEN_TREES)
            .map_err(|error| error.to_string())?;
        Ok(held
            .into_iter()
            .filter(|entry| entry.value.get(CLOSED_AT).is_none())
            .filter_map(|entry| serde_json::from_value(entry.value).ok())
            .collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn a_scratch(label: &str) -> std::path::PathBuf {
        let path = std::env::temp_dir().join(format!(
            "sailor-open-trees-{label}-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&path);
        path
    }

    fn an_entry(path: &str) -> OpenTree {
        OpenTree {
            path: path.to_owned(),
            repo: "/nowhere/project".to_owned(),
            run: "run-9".to_owned(),
            step: "asks".to_owned(),
            opened_by_pid: 4321,
            opened_at: 1_700_000_000,
        }
    }

    /// What was written is what comes back, and a closed entry stops counting
    /// as open without taking the record away.
    #[test]
    fn the_trees_sailor_opened_survive_the_store() {
        let directory = a_scratch("round-trip");
        let ledger = Ledger::open(&directory).expect("open the ledger");
        let first = an_entry("/nowhere/project-worktrees/run-9/asks");
        let second = an_entry("/nowhere/project-worktrees/run-9/answers");
        ledger.tree_opened(&first).expect("the first goes in");
        ledger.tree_opened(&second).expect("the second goes in");

        let open = ledger.trees_left_open().expect("read them back");
        ledger.tree_closed(&first.path).expect("the first closes");
        let after = ledger.trees_left_open().expect("read them back");
        let still_there = ledger
            .read_record(OPEN_TREES, &first.path)
            .expect("the record is readable");
        let _ = std::fs::remove_dir_all(&directory);

        assert_eq!(open.len(), 2, "{open:?}");
        assert!(open.contains(&first), "{open:?}");
        assert_eq!(after, vec![second]);
        assert!(still_there.is_some(), "closing deleted the entry");
    }

    /// Closing something nobody wrote down is not an error: a tree taken down
    /// twice, or one cut before this register existed, has nothing to close.
    #[test]
    fn closing_a_tree_the_store_never_had_is_no_error() {
        let directory = a_scratch("unknown");
        let ledger = Ledger::open(&directory).expect("open the ledger");

        let closed = ledger.tree_closed("/nowhere/project-worktrees/run-0/never");
        let open = ledger.trees_left_open().expect("read them back");
        let _ = std::fs::remove_dir_all(&directory);

        assert!(closed.is_ok(), "{closed:?}");
        assert!(open.is_empty(), "{open:?}");
    }
}
