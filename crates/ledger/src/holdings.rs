//! What a process made on this machine and answers for until it lets go.
//!
//! **THE CHAIN, NOT THE CLOCK.** A thing outside Sailor's memory — a worktree,
//! a build directory, a socket — is taken by a process, for a run, and ends
//! when both are over. Read from age instead, and the answer is a guess that
//! deletes live work on a slow day and hoards dead work on a fast one.

use crate::{Ledger, StoreRecord, WhoHoldsThePid, born_second_of, who_holds_the_pid};
use serde_json::Value;

/// The one name for this collection; two spellings would be two registers.
pub const HOLDINGS: &str = "holdings";

const LET_GO_AT: &str = "let_go_at";

/// A thing on this machine that a process made and has not let go of.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Holding {
    /// What sort of thing it is, so a reader asks for its own and no others.
    pub kind: String,
    /// What it is, in whatever way its kind names one: a path, a port, an id.
    pub name: String,
    pub held_by_pid: u32,
    /// The second the kernel says that process began. `None` is unasked, and
    /// then a recycled number cannot be told from the process that took it.
    #[serde(default)]
    pub held_by_born_at: Option<i64>,
    /// The run it was taken for. `None` means it was taken for no one run and
    /// outlives every process: a declared cache, kept until somebody says so.
    #[serde(default)]
    pub for_run: Option<String>,
    pub taken_at: i64,
    pub purpose: String,
}

/// Who a thing still belongs to, in the order a reader must ask.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Whose {
    /// The process that took it is still there: nobody else may touch it.
    TheProcessThatTookIt,
    /// That process is gone but its run has not ended, so somebody may return.
    TheRunItWasTakenFor,
    /// Taken for no run: kept by declaration, never reclaimed on its own.
    KeptOnPurpose,
    /// The process is gone and its run is over.
    Nobody,
    /// The machine would not say. A refusal is not evidence.
    Uncertain,
}

/// Whether a run is still open, asked of whoever holds that knowledge.
pub type RunIsOpen<'a> = &'a dyn Fn(&str) -> Result<bool, String>;

pub fn whose(holding: &Holding, run_is_open: RunIsOpen) -> Whose {
    match who_still_holds_it(holding) {
        Whose::Nobody => {}
        settled => return settled,
    }
    let Some(run) = holding.for_run.as_deref() else {
        return Whose::KeptOnPurpose;
    };
    match run_is_open(run) {
        Ok(true) => Whose::TheRunItWasTakenFor,
        Ok(false) => Whose::Nobody,
        Err(_) => Whose::Uncertain,
    }
}

fn who_still_holds_it(holding: &Holding) -> Whose {
    match (holding.held_by_born_at, who_holds_the_pid(holding.held_by_pid)) {
        (_, WhoHoldsThePid::Nobody) => Whose::Nobody,
        (None, _) | (_, WhoHoldsThePid::AliveButUnsaid) => Whose::TheProcessThatTookIt,
        (Some(born), WhoHoldsThePid::Since(second)) => {
            if second == born {
                Whose::TheProcessThatTookIt
            } else {
                Whose::Nobody
            }
        }
    }
}

/// A holding this process takes now, for the run that wanted it.
pub fn this_process_takes(
    kind: &str,
    name: &str,
    for_run: Option<String>,
    purpose: &str,
) -> Holding {
    let pid = std::process::id();
    Holding {
        kind: kind.to_owned(),
        name: name.to_owned(),
        held_by_pid: pid,
        held_by_born_at: born_second_of(pid),
        for_run,
        taken_at: now(),
        purpose: purpose.to_owned(),
    }
}

fn now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |since| since.as_secs() as i64)
}

fn under(kind: &str, name: &str) -> String {
    format!("{kind}\u{1f}{name}")
}

impl Ledger {
    pub fn holding_taken(&self, holding: &Holding) -> Result<(), String> {
        let value = serde_json::to_value(holding).map_err(|error| error.to_string())?;
        self.put_record(&StoreRecord {
            collection: HOLDINGS.to_owned(),
            key: under(&holding.kind, &holding.name),
            value,
            written_by: format!("pid {}", holding.held_by_pid),
            written_at: holding.taken_at,
        })
        .map_err(|error| error.to_string())
    }

    pub fn holding_let_go(&self, kind: &str, name: &str) -> Result<(), String> {
        let Some(mut held) = self
            .read_record(HOLDINGS, &under(kind, name))
            .map_err(|error| error.to_string())?
        else {
            return Ok(());
        };
        if let Some(fields) = held.value.as_object_mut() {
            fields.insert(LET_GO_AT.to_owned(), Value::from(now()));
        }
        held.written_at = now();
        self.put_record(&held).map_err(|error| error.to_string())
    }

    /// The holdings of one kind that nobody has let go of yet.
    pub fn holdings_left_held(&self, kind: &str) -> Result<Vec<Holding>, String> {
        let held = self
            .records_in(HOLDINGS)
            .map_err(|error| error.to_string())?;
        Ok(held
            .into_iter()
            .filter(|entry| entry.value.get(LET_GO_AT).is_none())
            .filter_map(|entry| serde_json::from_value::<Holding>(entry.value).ok())
            .filter(|holding| holding.kind == kind)
            .collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn a_scratch(label: &str) -> std::path::PathBuf {
        let path = std::env::temp_dir().join(format!(
            "sailor-holdings-{label}-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&path);
        path
    }

    fn taken_by(pid: u32, born_at: Option<i64>, for_run: Option<&str>) -> Holding {
        Holding {
            kind: "build-directory".to_owned(),
            name: format!("/nowhere/target/measure-{pid}"),
            held_by_pid: pid,
            held_by_born_at: born_at,
            for_run: for_run.map(str::to_owned),
            taken_at: 1_700_000_000,
            purpose: "measuring a ratchet".to_owned(),
        }
    }

    fn a_run_that_is_open(_: &str) -> Result<bool, String> {
        Ok(true)
    }

    fn a_run_that_ended(_: &str) -> Result<bool, String> {
        Ok(false)
    }

    #[test]
    fn what_this_very_process_took_is_held_by_it_and_by_nobody_else() {
        let mine = this_process_takes("build-directory", "/nowhere/target/x", None, "measuring");

        assert_eq!(mine.held_by_pid, std::process::id());
        assert_eq!(
            whose(&mine, &a_run_that_ended),
            Whose::TheProcessThatTookIt,
            "the process that took it is running this very assertion"
        );
    }

    #[test]
    fn a_number_the_kernel_gave_to_somebody_else_does_not_still_hold_it() {
        let mine =
            this_process_takes("build-directory", "/nowhere/target/x", Some("run-7".to_owned()), "measuring");
        let recycled = Holding {
            held_by_born_at: mine.held_by_born_at.map(|born| born - 3600),
            ..mine
        };

        assert_eq!(
            whose(&recycled, &a_run_that_ended),
            Whose::Nobody,
            "a pid alive under a different birth second is a different process"
        );
    }

    #[test]
    fn a_holding_whose_process_died_is_still_the_runs_until_the_run_ends() {
        let dead = taken_by(u32::MAX - 1, Some(1_700_000_000), Some("run-7"));

        assert_eq!(whose(&dead, &a_run_that_is_open), Whose::TheRunItWasTakenFor);
        assert_eq!(whose(&dead, &a_run_that_ended), Whose::Nobody);
    }

    #[test]
    fn a_holding_taken_for_no_run_outlives_the_process_that_took_it() {
        let cache = taken_by(u32::MAX - 1, Some(1_700_000_000), None);

        assert_eq!(
            whose(&cache, &a_run_that_ended),
            Whose::KeptOnPurpose,
            "a declared cache is not reclaimed because whoever made it went home"
        );
    }

    #[test]
    fn a_store_that_will_not_say_whether_the_run_ended_settles_nothing() {
        let dead = taken_by(u32::MAX - 1, Some(1_700_000_000), Some("run-7"));

        assert_eq!(
            whose(&dead, &|_| Err("the store will not open".to_owned())),
            Whose::Uncertain,
            "a refusal is not evidence that nobody wants it"
        );
    }

    #[test]
    fn a_holding_let_go_is_no_longer_held_and_the_others_stay() {
        let root = a_scratch("let-go");
        let store = Ledger::open(&root).expect("a store opens");
        let first = taken_by(11, Some(1), Some("run-1"));
        let second = taken_by(12, Some(2), Some("run-2"));
        store.holding_taken(&first).expect("the first goes in");
        store.holding_taken(&second).expect("the second goes in");

        store
            .holding_let_go(&first.kind, &first.name)
            .expect("the first lets go");
        let left = store
            .holdings_left_held("build-directory")
            .expect("read them back");

        assert_eq!(left, vec![second], "letting go of one took the other too");
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn a_reader_of_one_kind_never_sees_another_kind() {
        let root = a_scratch("kinds");
        let store = Ledger::open(&root).expect("a store opens");
        let build = taken_by(11, Some(1), None);
        let tree = Holding {
            kind: "worktree".to_owned(),
            ..taken_by(12, Some(2), None)
        };
        store.holding_taken(&build).expect("the build goes in");
        store.holding_taken(&tree).expect("the tree goes in");

        let left = store
            .holdings_left_held("build-directory")
            .expect("read them back");

        assert_eq!(left, vec![build], "one kind answered about another");
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn letting_go_of_something_never_taken_is_not_an_error() {
        let root = a_scratch("never-taken");
        let store = Ledger::open(&root).expect("a store opens");

        let answer = store.holding_let_go("build-directory", "/nowhere/never");

        assert!(answer.is_ok(), "letting go of nothing complained: {answer:?}");
        let _ = std::fs::remove_dir_all(&root);
    }
}
