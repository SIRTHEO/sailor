//! Il nucleo sincrono dei flussi di Sailor.
//!
//! Il grafo è un dato, le azioni sono sostituibili e ogni effetto è preceduto da
//! un record d'intenzione. Il deposito durevole vive dietro un tratto: questo
//! crate definisce la semantica che devono rispettare sia SQLite sia le prove in
//! processo.

mod executor;
mod file;
mod graph;
mod record;
mod schedule;
mod schema;
pub mod subflow;
pub mod system;

pub use executor::{
    attempt_relation, latest_for, run_status, same_gates, step_input, Action, ActionError,
    ActionOutcome, ActionRegistry, Clock, Completion, Decision, EffectStatus, Execution,
    ExecutionRequest, Executor, FlowError, InMemoryRecordStore, InProcessExecutor, ProcessProbe,
    Reconciliation, ReconciliationRequest, RecordStore, SharedState, Spend, SpendStop, SystemClock,
    CURRENT_CAP, CURRENT_RUN, CURRENT_STEP,
};
pub use file::FlowFile;
pub use graph::{Condition, DependencyEdge, Graph, GraphError, Step};
pub use record::{
    digest_input, truncate_said, AttemptRelation, Outcome, StepRecord, StepSpecies, MAX_SAID_BYTES,
};
pub use schedule::{is_due, Recurrence, Schedule, Weight};
pub use schema::{SchemaError, ValueSchema};
