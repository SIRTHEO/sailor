//! A step handed to the **agent already alive**, instead of to a new process.
//!
//! **SURFACE: `gate`. POWERS CLAIMED: none.** It does not read the world, does
//! not touch it, does not write to the store on its own account: it offers a
//! brief and waits. The declaration is written here because the four surfaces
//! of `docs/the-four-surfaces.md` do not exist in the code yet, and a new
//! action that stays silent while the criterion is being born becomes the first
//! unwritten exception — the way the window reached eight kinds of step against
//! three that are executed. Whoever carries the surfaces into the registry
//! finds this line written and need not guess it.
//!
//! **WHY IT EXISTS.** Measured: a four-step flow costs **2.79 times** a single
//! prompt on the same job, and the ratio of the spend is the ratio of the turns
//! — 62 against 30. Every step starts a process that rediscovers the repository
//! from nothing. The alternative is for the flow to **describe** the work and
//! for the agent already alive in the terminal to carry it out, context in
//! hand. But an agent working outside the engine writes nothing to the store,
//! and then the half Sailor exists for disappears. This action holds both
//! halves: the step stays a record, its intent written before and its outcome
//! after; someone else executes it.
//!
//! **IT STARTS NOTHING, AND THAT IS NO FAILING.** The value of this step is
//! exactly that no process is born: starting one would bring back the cost the
//! action exists to remove.

use crate::{sink_for_step, Pipe, StepSinks};
use flow::{
    Action, ActionError, ActionOutcome, EffectStatus, SharedState, StepRecord, StepSpecies,
};
use serde::Deserialize;
use serde_json::Value;
use std::collections::BTreeMap;
use std::sync::Arc;

/// The name this action registers itself under in a `flow::ActionRegistry`.
pub const HANDED_TO_AGENT_ACTION: &str = "handed_to_agent";

/// The store collection where `sailor step close` writes who closed a handed
/// step.
///
/// **IT LIVES HERE, NOT IN THE COMMAND, BECAUSE TWO SIDES READ IT.** The closer
/// writes; whoever opens the next step reads it to refuse a judge who is also
/// the author. A name copied into both places diverges at the first typo, and
/// the refusal would stop firing **in silence** — the lock would open by itself
/// with no test ever turning red.
pub const HOLDER_COLLECTION: &str = "handoff_holders";

/// The address, inside `HOLDER_COLLECTION`, of whoever closed a step.
pub fn holder_key(run_id: &str, step_id: &str) -> String {
    format!("{run_id}/{step_id}")
}

/// What a handed step declares.
///
/// **TWO FIELDS ARE DECLARED HERE AND READ ELSEWHERE, ON PURPOSE.**
/// `inspect_effect` reads `handoff_timeout_secs` and `sailor step open` reads
/// `same_holder_ok`, both straight from `record.input` — by the time they are
/// wanted the struct is gone: there is a record in the store and nothing else.
/// They stay in this struct for two measurable reasons: without them
/// `unknown_fields` would call them typos and `flow check` would reject a
/// well-written step; and a deadline missing or written as text would break the
/// step on resume rather than when it runs.
#[allow(dead_code)]
#[derive(Debug, Deserialize)]
struct HandoffSpec {
    /// The work, in full. **It stays whole in the record's `input`**, which the
    /// store keeps untruncated; `said` gets the short line, because `said` is
    /// cut at 16 KB (`flow::MAX_SAID_BYTES`) and a long brief would be lost in
    /// there mid-sentence.
    mandate: String,
    /// Who it is offered to. A label for whoever is watching, and the default
    /// `sailor step open` suggests, **not** a credential: see the weakness
    /// declared in `crates/sailor/src/step_cmd.rs`.
    holder: String,
    /// How many seconds someone has to take it on. Past that, the step reads as
    /// unapplied and a resume puts it back among the ready.
    handoff_timeout_secs: u64,
    /// Whether whoever closed a dependency may open this step as well.
    ///
    /// **DENY BY DEFAULT, NOT AN ALLOW LIST.** `false` means whoever produced
    /// the work does not judge it — the standing constraint «whoever creates
    /// does not judge». The default is the denial because a forgotten allow
    /// list lets everything through, while a forgotten denial at worst stops a
    /// job, and that shows up at once.
    #[serde(default)]
    same_holder_ok: bool,
    /// The closed choices the person may close this step with, each with the
    /// facts it rests on. A question in free text is not a step.
    #[serde(default)]
    options: Vec<Choice>,
    /// What the mandate leans on beside the engine, `kind:name`, the same
    /// field as `EngineSpec::needs_extensions`: read by `flow check`, not here.
    #[serde(default)]
    needs_extensions: Vec<String>,
    /// What this action does not recognise, for the same reason as
    /// `EngineSpec::extra`: a typo in the `with` is named at `flow check`, that
    /// is, before spending.
    #[serde(flatten)]
    extra: BTreeMap<String, Value>,
}

/// One way a person may close a handed step, and what it rests on.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct Choice {
    pub label: String,
    #[serde(default)]
    pub facts: String,
}

/// The choices of a handed step's `with`, as `flow check` reads them: none
/// when the block is absent or not a list.
pub fn choices_of(with: &Value) -> Vec<Choice> {
    with.get("options")
        .and_then(|options| serde_json::from_value(options.clone()).ok())
        .unwrap_or_default()
}

fn choices_lines(options: &[Choice]) -> String {
    options
        .iter()
        .map(|choice| {
            if choice.facts.is_empty() {
                format!("  · {}", choice.label)
            } else {
                format!("  · {} — {}", choice.label, choice.facts)
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// The action that hands a step to whoever is already alive.
pub struct HandoffAction {
    watcher: Option<Arc<dyn StepSinks>>,
    /// What time it is, for `inspect_effect`.
    ///
    /// **INJECTABLE BECAUSE THE DEADLINE MUST BE TESTABLE.** `inspect_effect`
    /// gets no clock from the trait — `reconcile` gets one and does not pass it
    /// on — and a deadline test against the real clock would have to wait for
    /// real, that is, be a fixed sleep: the fault that has broken a test of
    /// this house in the wrong place before.
    now: Arc<dyn Fn() -> i64 + Send + Sync>,
}

impl Default for HandoffAction {
    fn default() -> Self {
        Self::new()
    }
}

impl HandoffAction {
    pub fn new() -> Self {
        Self {
            watcher: None,
            now: Arc::new(|| {
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map_or(0, |elapsed| elapsed.as_secs() as i64)
            }),
        }
    }

    /// With someone watching: the brief appears **while** the step offers it,
    /// not once the run is over. That is the whole point: a person must see
    /// what was asked at the moment it is asked.
    pub fn watched_by(mut self, watcher: Option<Arc<dyn StepSinks>>) -> Self {
        self.watcher = watcher;
        self
    }

    /// With a declared clock, for the deadline tests.
    pub fn at_time(mut self, now: Arc<dyn Fn() -> i64 + Send + Sync>) -> Self {
        self.now = now;
        self
    }
}

impl Action for HandoffAction {
    /// From the real struct, like the two twin actions: a list written by hand
    /// alongside would be a second copy of the same truth.
    fn unknown_fields(&self, declared: &Value) -> Vec<String> {
        match serde_json::from_value::<HandoffSpec>(declared.clone()) {
            Ok(spec) => spec.extra.into_keys().collect(),
            Err(_) => Vec::new(),
        }
    }

    /// **REDOING THIS STEP IS SAFE, AND THE REASON IS WHAT IT REALLY DOES.**
    /// The action's effect is to *offer* a brief, not to carry it out. Offering
    /// it twice duplicates nothing in the world: it duplicates a line of text.
    /// The real work is done by a person or an agent, and that is guarded by
    /// `sailor step open` refusing to open a step that is not waiting.
    fn species(&self) -> StepSpecies {
        StepSpecies::Repeatable
    }

    /// **IT ASKS THE OPERATING SYSTEM NOTHING, AND THAT IS THE POINT.** Whoever
    /// holds a handed step is no process: it is a deadline written in the
    /// record. Questioning the kernel here would be fault 12 redone — inside the
    /// sandbox `pgrep` answers empty *with no error*, and a blind sensor that
    /// answers «nobody» is worse than an absent one, because downstream trusts
    /// it.
    ///
    /// No new column: the deadline sits in `input`, the starting instant in
    /// `started_at`, and both are in the record already.
    ///
    /// Before the deadline, `Unknown` — *I cannot tell whether anyone is on it,
    /// so I declare nothing*. After it, `NotApplied`: nobody took it on within
    /// the time the step gave itself.
    fn inspect_effect(
        &self,
        record: &StepRecord,
        _shared: &SharedState,
    ) -> Result<EffectStatus, ActionError> {
        let Some(limit) = record
            .input
            .get("handoff_timeout_secs")
            .and_then(Value::as_i64)
        else {
            // A record with no readable deadline is never declared expired: a
            // handover with no ceiling is ambiguous, and ambiguity is kept.
            return Ok(EffectStatus::Unknown(
                "the handover declares no readable deadline".to_owned(),
            ));
        };
        let deadline = record.started_at.saturating_add(limit);
        if (self.now)() < deadline {
            Ok(EffectStatus::Unknown(format!(
                "handed over, and the deadline has not passed: {} seconds to go",
                deadline - (self.now)()
            )))
        } else {
            Ok(EffectStatus::NotApplied)
        }
    }

    fn execute(&self, input: &Value, shared: &SharedState) -> Result<ActionOutcome, ActionError> {
        let live = sink_for_step(&self.watcher, shared);
        // **REFERENCES ARRIVE ALREADY RESOLVED, AND THEY MATTER MOST HERE.** A
        // handed step's brief is almost always the work the preceding step
        // decided: without `$from` it would stay a constant frozen at the
        // moment the flow was born, and the handover would be worth nothing.
        // Resolving them is `step_input`'s job, for every action and once.
        let spec: HandoffSpec = serde_json::from_value(input.clone())
            .map_err(|error| ActionError::new("invalid_input", error.to_string()))?;
        if spec.mandate.trim().is_empty() {
            return Err(ActionError::new(
                "invalid_input",
                "an empty brief is not a job: nobody would know what they were asked for",
            ));
        }
        let run_id = shared
            .get(flow::CURRENT_RUN)
            .and_then(Value::as_str)
            .unwrap_or("<an unknown run>");
        let step_id = shared
            .get(flow::CURRENT_STEP)
            .and_then(Value::as_str)
            .unwrap_or("<an unknown step>");

        // THE BRIEF IS SEEN AS IT HAPPENS, not once the run is over: whoever
        // watches must be able to take it on right away.
        if let Some(live) = live.as_deref() {
            let choices = if spec.options.is_empty() {
                String::new()
            } else {
                format!("\nclose it with one of:\n{}\n", choices_lines(&spec.options))
            };
            live.chunk(
                Pipe::Stdout,
                format!(
                    "\n── brief handed to «{}» ──\n{}\n{choices}──\n",
                    spec.holder, spec.mandate
                )
                .as_bytes(),
            );
        }

        // **THE LINE IS SHORT ON PURPOSE.** The whole brief stays in the
        // record's `input`, which the store keeps uncut; `said` is truncated at
        // 16 KB, and putting it there would lose its tail exactly where it runs
        // long, which is where it matters.
        Ok(ActionOutcome::Waiting(format!(
            "handed to «{}»; the brief is in the step's input, not here. \
             Take it with: sailor step open --run {run_id} --step {step_id} --as {}",
            spec.holder, spec.holder
        )))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::sync::Mutex;

    fn shared_for(run_id: &str, step_id: &str) -> SharedState {
        let mut shared = SharedState::new();
        shared.insert(flow::CURRENT_RUN.to_owned(), json!(run_id));
        shared.insert(flow::CURRENT_STEP.to_owned(), json!(step_id));
        shared
    }

    /// **NO PROCESS IS BORN, AND THE VERDICT IS «WAITING».** The action's value
    /// stated as a measurement: were someone to make it start something, the
    /// cost it exists to remove would come back.
    #[test]
    fn a_handed_step_waits_instead_of_going() {
        let action = HandoffAction::new();
        let outcome = action
            .execute(
                &json!({
                    "mandate": "repair fault 25",
                    "holder": "claude-vivo",
                    "handoff_timeout_secs": 3600
                }),
                &shared_for("run-1", "implementa"),
            )
            .expect("the handover does not fail");
        match outcome {
            ActionOutcome::Waiting(line) => {
                assert!(
                    line.contains("sailor step open --run run-1 --step implementa"),
                    "the line has to carry the command to run: {line}"
                );
                assert!(line.contains("claude-vivo"), "{line}");
            }
            ActionOutcome::Went(value) => {
                panic!("a handover does not know its own result: {value}")
            }
            // A handover waits for **a person**, and a person does not come
            // back by themselves on the next beat: `NotYet` would put the step
            // back in play while somebody is holding it.
            ActionOutcome::NotYet(line) => {
                panic!("a handover waits for a person, not for a beat: {line}")
            }
        }
    }

    /// **A LONG BRIEF LIVES IN THE INPUT, NOT IN `said`.** The short line stays
    /// short even with a 40 KB brief: the difference between a job that reads
    /// whole and one cut off mid-sentence.
    #[test]
    fn a_long_mandate_stays_out_of_the_said_line() {
        let long = "repair ".repeat(6000);
        assert!(
            long.len() > flow::MAX_SAID_BYTES,
            "the fixture has to exceed the `said` ceiling, or it proves nothing"
        );
        let action = HandoffAction::new();
        let outcome = action
            .execute(
                &json!({
                    "mandate": long.clone(),
                    "holder": "claude-vivo",
                    "handoff_timeout_secs": 60
                }),
                &shared_for("run-1", "implementa"),
            )
            .expect("the handover does not fail");
        let ActionOutcome::Waiting(line) = outcome else {
            panic!("a handover puts itself in waiting");
        };
        assert!(
            !line.contains(&long),
            "the brief must not land in the line: it would be cut at {} bytes",
            flow::MAX_SAID_BYTES
        );
        assert!(
            line.len() < flow::MAX_SAID_BYTES,
            "the line has to stay short: {} bytes",
            line.len()
        );
    }

    /// A `with` carrying a typo is named at check time, before spending.
    #[test]
    fn a_misspelled_field_is_named_before_the_run() {
        let action = HandoffAction::new();
        let unknown = action.unknown_fields(&json!({
            "mandate": "x",
            "holder": "chi",
            "handoff_timeout_secs": 1,
            "handoff_timeout_sec": 30
        }));
        assert_eq!(unknown, vec!["handoff_timeout_sec".to_owned()]);
    }

    /// **BEFORE THE DEADLINE NOTHING IS DECLARED.** The difference between «I
    /// cannot tell» and «it was never done»; closing it on the wrong side would
    /// take the work out of the hands of whoever is doing it.
    #[test]
    fn before_the_deadline_the_effect_is_unknown() {
        let clock = Arc::new(Mutex::new(1_000i64));
        let reading = Arc::clone(&clock);
        let action =
            HandoffAction::new().at_time(Arc::new(move || *reading.lock().expect("orologio sano")));
        let mut record = StepRecord::started(
            "run-1",
            "implementa",
            1,
            1,
            vec![],
            json!(null),
            vec![],
            1_000,
        );
        record.input = json!({"handoff_timeout_secs": 600});
        record.started_at = 1_000;

        let inspected = action
            .inspect_effect(&record, &SharedState::new())
            .expect("the probe answers");
        assert!(
            matches!(inspected, EffectStatus::Unknown(_)),
            "inside the window nothing is declared: {inspected:?}"
        );

        *clock.lock().expect("a sane clock") = 1_601;
        let inspected = action
            .inspect_effect(&record, &SharedState::new())
            .expect("the probe answers");
        assert_eq!(
            inspected,
            EffectStatus::NotApplied,
            "past the deadline nobody took it up"
        );
    }

    /// An empty brief is no job at all.
    #[test]
    fn an_empty_mandate_is_refused() {
        let action = HandoffAction::new();
        let error = action
            .execute(
                &json!({"mandate": "   ", "holder": "somebody", "handoff_timeout_secs": 1}),
                &shared_for("run-1", "implementa"),
            )
            .expect_err("an empty brief is refused");
        assert_eq!(error.class, "invalid_input");
    }

    /// Redoing a handover is safe: what it duplicates is a line of text.
    #[test]
    fn handing_a_step_over_twice_is_safe() {
        assert_eq!(HandoffAction::new().species(), StepSpecies::Repeatable);
    }
}
