//! What the `subflow` step needs and the flow crate cannot have.
//!
//! Three things: **where** flows are looked for, **which actions** run the
//! inner one, and **where** the child run is written. The first and third want
//! the ledger, which `flow` deliberately does not know — `ledger` depends on
//! `flow`, not the other way round. The second is a cycle: the step must run
//! with the registry it is itself registered in, and a direct reference cannot
//! be built.
//!
//! The cycle closes lazily: the child's registry is built when needed and then
//! kept — one nesting level, one registry. It is not free, since every registry
//! detects the machine's tools again, but it only happens if somebody actually
//! nests, and `flow::subflow::MAX_DEPTH` bounds it.

use std::path::PathBuf;
use std::sync::{Arc, OnceLock};

use flow::subflow::{RunNote, SubflowHost};
use flow::system::{self, FlowSource};
use flow::{ActionError, ActionRegistry, Execution, RecordStore};
use ledger::Ledger;

use crate::{default_registry, record_child_run, stopped_by_cap, FlowRun};

/// The "where" of the *yours* source when this machine declares no home.
///
/// **Not an empty path, and that is the point.** An empty path becomes the
/// relative `flows`, meaning the current directory: the project's flows would
/// also show up as "yours", and on a name clash the wrong one would win. An
/// absolute path that does not exist reads zero flows, which is the true
/// answer.
const NO_HOME: &str = "/sailor-non-ha-una-casa-su-questa-macchina";

/// The bridge between the `subflow` step and the rest of Sailor.
pub struct LedgerHost {
    /// `None` when whoever built the registry has no ledger: the step then
    /// refuses to run, and says why.
    ledger: Option<Ledger>,
    watcher: Option<Arc<dyn actions::StepSinks>>,
    /// The registry the child runs with, built on first call.
    nested: OnceLock<Arc<ActionRegistry>>,
}

impl LedgerHost {
    pub fn new(ledger: Option<Ledger>, watcher: Option<Arc<dyn actions::StepSinks>>) -> Self {
        Self {
            ledger,
            watcher,
            nested: OnceLock::new(),
        }
    }

    /// The ledger, or the error saying why nothing runs without one.
    ///
    /// Not a technical limitation: a child would run fine on an in-memory
    /// store. What it could not do is **be traced back** from the step that
    /// called it, and work that disappears inside other work is the opacity
    /// this product exists to remove.
    fn deposit(&self) -> Result<&Ledger, ActionError> {
        self.ledger.as_ref().ok_or_else(|| {
            ActionError::new(
                "no_ledger",
                "without a ledger the child run could not be traced back to the step that called it",
            )
        })
    }
}

impl SubflowHost for LedgerHost {
    /// The same sources as `sailor flow run`, not a second rule. The precedence
    /// — *system* < *yours* < *project* — lives in
    /// `flow::system::sources_from_env`. If a `subflow` looked elsewhere, two
    /// machines would run different flows under the same name without saying so.
    fn sources(&self) -> Vec<FlowSource> {
        let home = ledger::sailor_home()
            .map(|home| home.join("flows"))
            .unwrap_or_else(|| PathBuf::from(NO_HOME));
        system::sources_from_env(&home)
    }

    /// The child's actions are the parent's, built on first call and kept.
    fn actions(&self) -> Result<Arc<ActionRegistry>, ActionError> {
        Ok(self
            .nested
            .get_or_init(|| Arc::new(default_registry(self.ledger.clone(), self.watcher.clone())))
            .clone())
    }

    fn store(&self) -> Result<Arc<dyn RecordStore>, ActionError> {
        Ok(Arc::new(self.deposit()?.clone()) as Arc<dyn RecordStore>)
    }

    fn note_run(&self, note: &RunNote<'_>) -> Result<(), ActionError> {
        // Who started it is a step, and it is readable: this is how the ledger
        // tells a child run from a hand-launched one carrying the same flow.
        let started_by = format!("subflow {}", note.parent_step_id);
        record_child_run(
            self.deposit()?,
            note.flow,
            FlowRun {
                run_id: note.run_id,
                status: note.status,
                started_at: note.started_at,
                ended_at: note.ended_at,
                error: note.error.clone(),
                started_by: &started_by,
            },
            note.parent_run_id,
        )
        .map_err(|said| ActionError::new("child_run_not_recorded", said))
    }

    fn why(&self, execution: &Execution) -> Option<String> {
        stopped_by_cap(execution)
    }
}
