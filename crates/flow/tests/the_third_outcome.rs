//! The third outcome: "not yet, ask again on the next beat" — see fault 62.
//!
//! The engine had two, and neither does this: `Waiting` parks for good —
//! nothing returns it to the ready set — and `Broke` returns in the *same
//! loop* with no wait at all, so the attempts burn together on one state of
//! the world.

use flow::{
    Action, ActionError, ActionOutcome, ActionRegistry, Clock, Decision, ExecutionRequest,
    Executor, FlowError, Graph, InMemoryRecordStore, InProcessExecutor, Outcome, SharedState, Step,
    ValueSchema,
};
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::Arc;

/// A stopped clock. Stopped and not advancing: what is measured here is *when*
/// a step comes back, and a clock that moves on its own would confuse the
/// instant it came back with the instant it was asked.
struct Stopped(i64);

impl Clock for Stopped {
    fn now(&self) -> Result<i64, FlowError> {
        Ok(self.0)
    }
}

fn step(id: &str, action: &str, max_attempts: u32) -> Step {
    Step {
        id: id.to_owned(),
        deps: Vec::new(),
        input_schema: ValueSchema::Any,
        output_schema: ValueSchema::Any,
        with: None,
        when: None,
        action: action.to_owned(),
        max_attempts,
        ask_again_after_secs: None,
        retry_after_secs: None,
        phase: None,
        stops_when: None,
        decides_done: false,
    }
}

fn request(run_id: &str) -> ExecutionRequest {
    ExecutionRequest {
        run_id: run_id.to_owned(),
        root_inputs: BTreeMap::new(),
        gates: Vec::new(),
        shared: SharedState::new(),
        spend_cap_micros: None,
        stops: flow::RunStops::default(),
    }
}

/// Answers "not yet" until somebody unlocks it. The shape of `take_mandate`:
/// the pause is the whole point of it, not a fault.
struct NotYetUntil {
    unlocked: Arc<AtomicBool>,
    asked: Arc<AtomicUsize>,
}

impl Action for NotYetUntil {
    fn execute(&self, _input: &Value, _shared: &SharedState) -> Result<ActionOutcome, ActionError> {
        self.asked.fetch_add(1, Ordering::SeqCst);
        if self.unlocked.load(Ordering::SeqCst) {
            Ok(ActionOutcome::Went(json!({"taken": true})))
        } else {
            Ok(ActionOutcome::NotYet("nothing has arrived yet".to_owned()))
        }
    }
}

struct AlwaysBreaks(Arc<AtomicUsize>);

impl Action for AlwaysBreaks {
    fn execute(&self, _input: &Value, _shared: &SharedState) -> Result<ActionOutcome, ActionError> {
        self.0.fetch_add(1, Ordering::SeqCst);
        Err(ActionError::new("nope", "it never works"))
    }
}

/// **THE MAIN CASE.** A step that says "not yet" becomes ready again at the
/// instant the flow declared — not before it, and not never.
#[test]
fn a_step_that_says_not_yet_becomes_ready_again_at_the_declared_instant() {
    let mut poll = step("poll", "poll", 1);
    poll.ask_again_after_secs = Some(30);
    let graph = Graph::new(vec![poll]).expect("valid graph");

    let store = InMemoryRecordStore::default();
    let unlocked = Arc::new(AtomicBool::new(false));
    let asked = Arc::new(AtomicUsize::new(0));
    let mut actions = ActionRegistry::default();
    actions.register(
        "poll",
        NotYetUntil {
            unlocked: Arc::clone(&unlocked),
            asked: Arc::clone(&asked),
        },
    );

    let execution = InProcessExecutor
        .execute(&graph, request("run"), &store, &actions, &Stopped(1_000))
        .expect("the run goes through");

    // The run ends saying when to come back, and the engine never slept.
    assert_eq!(
        execution.decisions.last(),
        Some(&Decision::NotYet {
            steps: vec!["poll".to_owned()],
            due_at: 1_030,
        }),
        "{:?}",
        execution.decisions
    );
    assert_eq!(flow::run_status(&execution), ("not_yet", false));
    assert_eq!(
        asked.load(Ordering::SeqCst),
        1,
        "one attempt only: the loop did not spin on the same step"
    );
    assert_eq!(store.all()[0].outcome, Some(Outcome::NotYet));

    // One second earlier it is not ready.
    assert!(
        matches!(
            InProcessExecutor
                .decision(&graph, "run", &store, &Stopped(1_029))
                .expect("readable decision"),
            Decision::NotYet { .. }
        ),
        "a second before the declared instant the step is not ready"
    );
    // At the declared instant it is.
    assert_eq!(
        InProcessExecutor
            .decision(&graph, "run", &store, &Stopped(1_030))
            .expect("readable decision"),
        Decision::Ready(vec!["poll".to_owned()])
    );

    // And the resume carries it home: the half the relay was missing.
    unlocked.store(true, Ordering::SeqCst);
    let second = InProcessExecutor
        .execute(&graph, request("run"), &store, &actions, &Stopped(1_030))
        .expect("the resume goes through");
    assert_eq!(second.decisions.last(), Some(&Decision::Complete));
    assert_eq!(asked.load(Ordering::SeqCst), 2);
}

/// **THE SAME STEP ANSWERING `Waiting` NEVER COMES BACK**, which is the
/// comparison that gives the case above its meaning: without it, "becomes ready
/// again" could be something the engine already did.
#[test]
fn the_same_step_answering_waiting_never_becomes_ready_again() {
    struct AlwaysWaits;
    impl Action for AlwaysWaits {
        fn execute(
            &self,
            _input: &Value,
            _shared: &SharedState,
        ) -> Result<ActionOutcome, ActionError> {
            Ok(ActionOutcome::Waiting("somebody must come".to_owned()))
        }
    }

    let mut parks = step("parks", "parks", 1);
    parks.ask_again_after_secs = Some(30);
    let graph = Graph::new(vec![parks]).expect("valid graph");
    let store = InMemoryRecordStore::default();
    let mut actions = ActionRegistry::default();
    actions.register("parks", AlwaysWaits);

    InProcessExecutor
        .execute(&graph, request("run"), &store, &actions, &Stopped(1_000))
        .expect("the run goes through");

    // An hour later, with the same wait declared: still waiting.
    assert_eq!(
        InProcessExecutor
            .decision(&graph, "run", &store, &Stopped(4_600))
            .expect("readable decision"),
        Decision::Waiting(vec!["parks".to_owned()]),
        "no clock puts back in play a step a person is holding"
    );
}

/// **WITH NO WAIT DECLARED THE STEP DOES NOT SPIN.** This is how the repair
/// could have been born worse than the fault: if "not yet" with no number meant
/// "at once", the executor would retry the step in the loop that just postponed
/// it, for ever.
#[test]
fn not_yet_with_no_declared_wait_ends_the_run_instead_of_spinning() {
    let graph = Graph::new(vec![step("poll", "poll", 1)]).expect("valid graph");
    let store = InMemoryRecordStore::default();
    let asked = Arc::new(AtomicUsize::new(0));
    let mut actions = ActionRegistry::default();
    actions.register(
        "poll",
        NotYetUntil {
            unlocked: Arc::new(AtomicBool::new(false)),
            asked: Arc::clone(&asked),
        },
    );

    let execution = InProcessExecutor
        .execute(&graph, request("run"), &store, &actions, &Stopped(1_000))
        .expect("the run ends");
    assert_eq!(
        execution.decisions.last(),
        Some(&Decision::NotYet {
            steps: vec!["poll".to_owned()],
            due_at: 1_001,
        })
    );
    assert_eq!(asked.load(Ordering::SeqCst), 1);
}

/// **NOR DOES A ZERO WRITTEN BY HAND.** Zero read literally would mean ready in
/// the very loop that postponed it; here it means "as soon as possible", and as
/// soon as possible is the next invocation.
#[test]
fn a_declared_wait_of_zero_is_still_the_next_invocation() {
    let mut poll = step("poll", "poll", 1);
    poll.ask_again_after_secs = Some(0);
    let graph = Graph::new(vec![poll]).expect("valid graph");
    let store = InMemoryRecordStore::default();
    let asked = Arc::new(AtomicUsize::new(0));
    let mut actions = ActionRegistry::default();
    actions.register(
        "poll",
        NotYetUntil {
            unlocked: Arc::new(AtomicBool::new(false)),
            asked: Arc::clone(&asked),
        },
    );

    let execution = InProcessExecutor
        .execute(&graph, request("run"), &store, &actions, &Stopped(1_000))
        .expect("the run ends");
    assert_eq!(
        execution.decisions.last(),
        Some(&Decision::NotYet {
            steps: vec!["poll".to_owned()],
            due_at: 1_001,
        })
    );
    assert_eq!(asked.load(Ordering::SeqCst), 1);
}

/// **"NOT YET" SPENDS NO ATTEMPT.** The relay's collecting step declares
/// `max_attempts: 1`: if a poll counted, the first beat would fail it and the
/// repair would be worse than the fault.
#[test]
fn not_yet_never_spends_an_attempt() {
    let mut poll = step("poll", "poll", 1);
    poll.ask_again_after_secs = Some(10);
    let graph = Graph::new(vec![poll]).expect("valid graph");
    let store = InMemoryRecordStore::default();
    let asked = Arc::new(AtomicUsize::new(0));
    let mut actions = ActionRegistry::default();
    actions.register(
        "poll",
        NotYetUntil {
            unlocked: Arc::new(AtomicBool::new(false)),
            asked: Arc::clone(&asked),
        },
    );

    for beat in 0..4 {
        let now = 1_000 + beat * 10;
        let execution = InProcessExecutor
            .execute(&graph, request("run"), &store, &actions, &Stopped(now))
            .expect("every beat goes through");
        assert!(
            matches!(execution.decisions.last(), Some(Decision::NotYet { .. })),
            "beat {beat}: {:?}",
            execution.decisions
        );
    }
    assert_eq!(asked.load(Ordering::SeqCst), 4);
    assert_eq!(store.all().len(), 4, "four polls, four records");
    assert_eq!(store.all()[3].attempt, 4);
}

/// **AND A STEP THAT POLLED BEFORE BREAKING STILL HAS ITS ATTEMPTS.**
///
/// The case the ceiling counts *breaks* for, and the only one that tells the
/// two rules apart: two polls and then the first break sit at the third
/// attempt, so on the ordinal `max_attempts: 2` would already be spent — a step
/// declared failed after breaking once.
#[test]
fn a_step_that_polled_before_breaking_still_has_all_its_attempts() {
    struct PollsThenBreaks(Arc<AtomicUsize>);
    impl Action for PollsThenBreaks {
        fn execute(
            &self,
            _input: &Value,
            _shared: &SharedState,
        ) -> Result<ActionOutcome, ActionError> {
            if self.0.fetch_add(1, Ordering::SeqCst) < 2 {
                return Ok(ActionOutcome::NotYet("still nothing".to_owned()));
            }
            Err(ActionError::new("nope", "and now it breaks"))
        }
    }

    let mut late = step("late", "late", 2);
    late.ask_again_after_secs = Some(10);
    let graph = Graph::new(vec![late]).expect("valid graph");
    let store = InMemoryRecordStore::default();
    let mut actions = ActionRegistry::default();
    actions.register("late", PollsThenBreaks(Arc::new(AtomicUsize::new(0))));

    let mut last = None;
    for beat in 0..3 {
        last = InProcessExecutor
            .execute(
                &graph,
                request("run"),
                &store,
                &actions,
                &Stopped(1_000 + beat * 10),
            )
            .expect("every beat goes through")
            .decisions
            .last()
            .cloned();
    }
    assert_eq!(last, Some(Decision::Failed(vec!["late".to_owned()])));
    let broken = store
        .all()
        .into_iter()
        .filter(|record| record.outcome == Some(Outcome::Broke))
        .count();
    assert_eq!(
        broken, 2,
        "two attempts declared, two breaks spent: the polls do not count"
    );
}

/// **THE ATTEMPTS DO NOT BURN TOGETHER.** With a declared wait one invocation
/// spends one attempt: the other two are spent by later beats, when the world
/// may have changed.
#[test]
fn declared_backoff_stops_the_attempts_burning_in_one_loop() {
    let mut flaky = step("flaky", "flaky", 3);
    flaky.retry_after_secs = Some(60);
    let graph = Graph::new(vec![flaky]).expect("valid graph");
    let store = InMemoryRecordStore::default();
    let tried = Arc::new(AtomicUsize::new(0));
    let mut actions = ActionRegistry::default();
    actions.register("flaky", AlwaysBreaks(Arc::clone(&tried)));

    let first = InProcessExecutor
        .execute(&graph, request("run"), &store, &actions, &Stopped(1_000))
        .expect("the run ends");
    assert_eq!(
        tried.load(Ordering::SeqCst),
        1,
        "one invocation, one attempt"
    );
    assert_eq!(
        first.decisions.last(),
        Some(&Decision::NotYet {
            steps: vec!["flaky".to_owned()],
            due_at: 1_060,
        })
    );

    // Before the wait is up it does not restart, even when called.
    let too_soon = InProcessExecutor
        .execute(&graph, request("run"), &store, &actions, &Stopped(1_059))
        .expect("the run ends");
    assert_eq!(tried.load(Ordering::SeqCst), 1);
    assert!(matches!(
        too_soon.decisions.last(),
        Some(Decision::NotYet { .. })
    ));

    InProcessExecutor
        .execute(&graph, request("run"), &store, &actions, &Stopped(1_060))
        .expect("the run ends");
    assert_eq!(tried.load(Ordering::SeqCst), 2);

    let last = InProcessExecutor
        .execute(&graph, request("run"), &store, &actions, &Stopped(1_120))
        .expect("the run ends");
    assert_eq!(tried.load(Ordering::SeqCst), 3);
    assert_eq!(
        last.decisions.last(),
        Some(&Decision::Failed(vec!["flaky".to_owned()])),
        "with the attempts spent the step fails, as it always has"
    );
}

/// **AND WITH NO WAIT DECLARED THEY STILL BURN TOGETHER**, on purpose. This is
/// the check that says what changes for whoever does not use the field:
/// nothing. Were it to go red, the default would have changed the behaviour of
/// every flow already written, with nobody asking for it.
#[test]
fn with_no_declared_backoff_the_attempts_still_burn_together() {
    let graph = Graph::new(vec![step("flaky", "flaky", 3)]).expect("valid graph");
    let store = InMemoryRecordStore::default();
    let tried = Arc::new(AtomicUsize::new(0));
    let mut actions = ActionRegistry::default();
    actions.register("flaky", AlwaysBreaks(Arc::clone(&tried)));

    let execution = InProcessExecutor
        .execute(&graph, request("run"), &store, &actions, &Stopped(1_000))
        .expect("the run ends");
    assert_eq!(
        tried.load(Ordering::SeqCst),
        3,
        "one invocation burns all three attempts"
    );
    assert_eq!(
        execution.decisions.last(),
        Some(&Decision::Failed(vec!["flaky".to_owned()]))
    );
}

/// **A RUN THAT COMES BACK DOES NOT READ AS A PARKED ONE.** With one step held
/// by a person and one coming back, the run says "not yet": whoever reads
/// "waiting" goes to collect a handover nobody gave them.
#[test]
fn a_run_with_something_coming_back_does_not_read_as_parked() {
    struct Waits;
    impl Action for Waits {
        fn execute(
            &self,
            _input: &Value,
            _shared: &SharedState,
        ) -> Result<ActionOutcome, ActionError> {
            Ok(ActionOutcome::Waiting("held by a person".to_owned()))
        }
    }

    let mut poll = step("poll", "poll", 1);
    poll.ask_again_after_secs = Some(30);
    let graph = Graph::new(vec![step("parks", "parks", 1), poll]).expect("valid graph");
    let store = InMemoryRecordStore::default();
    let mut actions = ActionRegistry::default();
    actions.register("parks", Waits);
    actions.register(
        "poll",
        NotYetUntil {
            unlocked: Arc::new(AtomicBool::new(false)),
            asked: Arc::new(AtomicUsize::new(0)),
        },
    );

    let execution = InProcessExecutor
        .execute(&graph, request("run"), &store, &actions, &Stopped(1_000))
        .expect("the run ends");
    assert_eq!(
        execution.decisions.last(),
        Some(&Decision::NotYet {
            steps: vec!["poll".to_owned()],
            due_at: 1_030,
        }),
        "{:?}",
        execution.decisions
    );
}
