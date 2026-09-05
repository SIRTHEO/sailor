//! The synchronous core of Sailor's flows. The graph is data, actions are
//! swappable, and every effect is preceded by a record of intent. The durable
//! store lives behind a trait: this crate defines the semantics both SQLite and
//! the in-process test store must honour, and depends on neither. `actions`,
//! `registry`, `sailor` and `ui` already depend on `flow`, so what lands here
//! costs nobody a new dependency — and the arrow never points back.

mod executor;
mod file;
pub mod for_each;
mod graph;
mod record;
pub mod reference;
mod schedule;
mod schema;
mod streak;
pub mod subflow;
pub mod system;
pub mod workspace;

pub use executor::{
    attempt_relation, latest_for, run_status, same_gates, step_input, Action, ActionError,
    ActionOutcome, ActionRegistry, Clock, Completion, CostReading, Decision, EffectStatus,
    Execution, ExecutionRequest, Executor, FlowError, InMemoryRecordStore, InProcessExecutor,
    ProcessProbe, Reconciliation, ReconciliationRequest, RecordStore, SharedState, Spend,
    SpendStop, StepInput, SystemClock, AT_ONCE, CURRENT_CAP, CURRENT_RUN, CURRENT_STEP,
    WORKDIR_FIELD, WORKSPACE_ROOT,
};
pub use file::FlowFile;
pub use graph::{Condition, DependencyEdge, Graph, GraphError, Step};
pub use record::{
    digest_input, truncate_said, AttemptRelation, Outcome, Ran, Refusal, RefusalRule, StepRecord,
    StepSpecies, MAX_SAID_BYTES, MAX_SEEN_BYTES,
};
pub use schedule::{is_due, Recurrence, Schedule, Weight};
pub use schema::{SchemaError, ValueSchema};
pub use streak::{faults_due, FailureStreak, FaultToWrite, FAILURES_THAT_MAKE_A_FAULT};
