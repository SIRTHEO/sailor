//! Defects planted on purpose, and the real check run over them.
//!
//! The mandate is small enough for a command to judge: a `notes.md` with three
//! sections, each with a body. Three defects are planted — obvious, subtle, and
//! one a check reading text cannot see. The last is written down here, because
//! a check whose limits are undeclared is the failure this piece guards.

use actions::ShellCheckAction;
use flow::{
    Action, ActionError, ActionOutcome, ActionRegistry, Decision, ExecutionRequest, Executor,
    Graph, InMemoryRecordStore, InProcessExecutor, Outcome, RunStops, SharedState, Step,
    StopReason, SystemClock, ValueSchema,
};
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

/// What a run leaves behind: how it decided, how it reads, and the input each
/// step was handed.
type Ending = (Vec<Decision>, (&'static str, bool), Vec<(String, Value)>);

/// The whole instruction the run was given. It must not travel back to an
/// engine when the check fails: what travels back is the unresolved part.
const MANDATE: &str = "write notes.md with the sections uno, due and tre, each with a body";

/// What the check demands: a heading per section, and a line under it.
///
/// It reads text, and that is the boundary of what it can see. Awk is here
/// because a suite this small has no runner of its own, and the command a real
/// flow would name — `cargo test`, `npm test` — is the same shape: exit code,
/// and a complaint naming what is left.
const THE_CHECK: &str = r#"cd "$WORK" && awk '
  /^## / { current = substr($0, 4); seen[current] = 1; next }
  current != "" && NF > 0 { body[current] = 1 }
  END {
    split("uno due tre", want, " ")
    for (i = 1; i <= 3; i++) {
      name = want[i]
      if (!(name in seen)) { print "section " name " is missing"; bad = 1 }
      else if (!(name in body)) { print "section " name " has no body"; bad = 1 }
    }
    exit bad
  }
' notes.md"#;

/// What a naive check does instead: the headings are there, so it is done.
const THE_NAIVE_CHECK: &str =
    r#"cd "$WORK" && grep -q '^## uno' notes.md && grep -q '^## due' notes.md && grep -q '^## tre' notes.md"#;

/// Hands its input back. It stands where an engine would stand, and its record
/// is what says whether a second call would have carried the whole mandate.
struct Echo;

impl Action for Echo {
    fn execute(&self, input: &Value, _shared: &SharedState) -> Result<ActionOutcome, ActionError> {
        Ok(ActionOutcome::Went(input.clone()))
    }
}

fn step(id: &str, action: &str, deps: &[&str], with: Option<Value>) -> Step {
    Step {
        id: id.to_owned(),
        deps: deps.iter().map(|dep| (*dep).to_owned()).collect(),
        input_schema: ValueSchema::Any,
        output_schema: ValueSchema::Any,
        with,
        when: None,
        action: action.to_owned(),
        max_attempts: 1,
        ask_again_after_secs: None,
        retry_after_secs: None,
        phase: None,
        stops_when: None,
        decides_done: false,
    }
}

/// A directory of its own per case, under the temporary directory and nowhere
/// else, holding whatever `notes.md` that case planted.
fn work_with(case: &str, notes: Option<&str>) -> PathBuf {
    let home = std::env::temp_dir().join(format!("sailor-planted-{case}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&home);
    fs::create_dir_all(&home).expect("a scratch directory");
    if let Some(notes) = notes {
        fs::write(home.join("notes.md"), notes).expect("the work as it was left");
    }
    home
}

/// Mandate, then the check that decides, then the step that would call again.
fn run(work: &Path, command: &str) -> Ending {
    run_for(work, command, 30)
}

fn run_for(work: &Path, command: &str, seconds: u64) -> Ending {
    let gate = {
        let mut gate = step(
            "gate",
            "shell_check",
            &["mandate"],
            Some(json!({
                "command": command,
                "env": {"WORK": work.display().to_string()},
                "accept": ["failed", "timed_out"],
                "timeout_secs": seconds
            })),
        );
        gate.decides_done = true;
        gate
    };
    let graph = Graph::new(vec![
        step("mandate", "echo", &[], None),
        gate,
        step("again", "echo", &["gate"], None),
    ])
    .expect("a sane graph");

    let mut registry = ActionRegistry::default();
    registry.register("shell_check", ShellCheckAction::new());
    registry.register("echo", Echo);
    let store = InMemoryRecordStore::default();
    let execution = InProcessExecutor
        .execute(
            &graph,
            ExecutionRequest {
                run_id: "run".to_owned(),
                root_inputs: [("mandate".to_owned(), json!({"mandate": MANDATE}))]
                    .into_iter()
                    .collect(),
                gates: Vec::new(),
                shared: SharedState::new(),
                spend_cap_micros: None,
                stops: RunStops::default(),
            },
            &store,
            &registry,
            &SystemClock,
        )
        .expect("the run answers");

    let status = flow::run_status(&execution);
    let inputs = store
        .all()
        .into_iter()
        .map(|record| (record.step_id, record.input))
        .collect();
    (execution.decisions, status, inputs)
}

fn closed_on_a_check(decisions: &[Decision]) -> bool {
    matches!(
        decisions.last(),
        Some(Decision::Halted {
            reason: StopReason::Checked,
            ..
        })
    )
}

fn input_of(inputs: &[(String, Value)], step: &str) -> Option<Value> {
    inputs
        .iter()
        .find(|(id, _)| id == step)
        .map(|(_, input)| input.clone())
}

/// The work as it should have been left. The check passes, the run is done, and
/// the step that would have called an engine again never opens.
#[test]
fn work_that_is_really_finished_closes_the_run_on_the_check() {
    let work = work_with("done", Some("## uno\nprimo\n\n## due\nsecondo\n\n## tre\nterzo\n"));
    let (decisions, status, inputs) = run(&work, THE_CHECK);

    assert!(closed_on_a_check(&decisions), "{decisions:?}");
    assert_eq!(status, ("complete", true), "{decisions:?}");
    assert!(
        input_of(&inputs, "again").is_none(),
        "no engine was called again: {inputs:?}"
    );
}

/// **DEFECT ONE, THE OBVIOUS ONE: the work was never done.** No file at all.
/// The check goes red and names every section, and the run stays open.
#[test]
fn the_planted_defect_of_work_never_done_is_caught() {
    let work = work_with("absent", None);
    let (decisions, _, inputs) = run(&work, THE_CHECK);

    assert!(!closed_on_a_check(&decisions), "{decisions:?}");
    let back = input_of(&inputs, "again").expect("the run went on to ask again");
    assert_eq!(back["status"], "failed", "{back}");
}

/// **DEFECT TWO, THE SUBTLE ONE: the heading is there and the section is not.**
/// A naive check greps for the three headings and is satisfied; this one asks
/// for a body under each, so it names `tre` and only `tre`.
#[test]
fn the_planted_defect_of_a_heading_without_a_body_is_caught() {
    let notes = "## uno\nprimo\n\n## due\nsecondo\n\n## tre\n";
    let work = work_with("empty-section", Some(notes));

    let (naive, _, _) = run(&work, THE_NAIVE_CHECK);
    assert!(
        closed_on_a_check(&naive),
        "the naive check is supposed to wave this through: {naive:?}"
    );

    let (decisions, _, inputs) = run(&work, THE_CHECK);
    assert!(!closed_on_a_check(&decisions), "{decisions:?}");
    let back = input_of(&inputs, "again").expect("the run went on to ask again");
    let unresolved = back["unresolved"].as_str().expect("what is left is named");
    assert!(unresolved.contains("section tre has no body"), "{unresolved}");
    assert!(!unresolved.contains("section uno"), "only what is open goes back: {unresolved}");
    assert!(!unresolved.contains("section due"), "only what is open goes back: {unresolved}");
}

/// **DEFECT THREE, THE ONE THIS CHECK MISSES, AND IT IS DECLARED HERE.** The
/// third section exists only inside a fenced example: the document *mentions*
/// it instead of having it. A check that reads text cannot tell a section from
/// a quotation of one, so it passes and the run closes as done on work that is
/// not. This is the shortcut's cost, written down where it turns red the day
/// somebody teaches the check to see it.
#[test]
fn the_planted_defect_of_a_section_only_quoted_is_missed() {
    let notes = "## uno\nprimo\n\n## due\nsecondo\n\nesempio:\n\n```\n## tre\nterzo\n```\n";
    let work = work_with("quoted", Some(notes));
    let (decisions, status, _) = run(&work, THE_CHECK);

    assert!(
        closed_on_a_check(&decisions),
        "this defect is missed, not caught: {decisions:?}"
    );
    assert_eq!(status, ("complete", true));
}

/// **THE SECOND CALL CARRIES THE UNRESOLVED PART, NOT THE MANDATE.** The step
/// that would ask an engine again receives the verdict and what is open under
/// it — the instruction the run started from is not in what it is handed.
#[test]
fn only_what_is_unresolved_goes_back_to_an_engine() {
    let work = work_with("narrow", Some("## uno\nprimo\n"));
    let (_, _, inputs) = run(&work, THE_CHECK);

    let mandate = input_of(&inputs, "gate").expect("the check ran");
    assert!(
        mandate.to_string().contains(MANDATE),
        "the check itself came after the mandate: {mandate}"
    );

    let back = input_of(&inputs, "again").expect("the run went on to ask again");
    let text = back.to_string();
    assert!(
        !text.contains(MANDATE),
        "the whole mandate went back to the engine: {text}"
    );
    assert!(text.contains("section due is missing"), "{text}");
    assert!(text.contains("section tre is missing"), "{text}");
}

/// **A CHECK THAT COULD NOT RUN IS NOT A PASS.** The command is killed by its
/// own limit before it can say anything: the run stays open, and what goes
/// back says the check never gave a verdict rather than pretending to one.
#[test]
fn a_check_the_clock_kills_does_not_close_the_run() {
    let work = work_with("slow", Some("## uno\nprimo\n"));
    let (decisions, _, inputs) = run_for(&work, "sleep 30", 2);

    assert!(!closed_on_a_check(&decisions), "{decisions:?}");
    let back = input_of(&inputs, "again").expect("the run went on to ask again");
    assert_eq!(back["status"], "timed_out", "{back}");
}

/// A check whose command cannot even start is not a pass either, and it is the
/// step that goes red rather than the run that goes green.
#[test]
fn a_check_that_cannot_run_at_all_is_not_a_pass() {
    let work = work_with("broken", None);
    let gate = {
        let mut gate = step(
            "gate",
            "shell_check",
            &[],
            Some(json!({
                "command": "sailor-no-such-command-here",
                "env": {"WORK": work.display().to_string()},
                "timeout_secs": 30
            })),
        );
        gate.decides_done = true;
        gate
    };
    let graph = Graph::new(vec![gate]).expect("a sane graph");
    let mut registry = ActionRegistry::default();
    registry.register("shell_check", ShellCheckAction::new());
    let store = InMemoryRecordStore::default();
    let execution = InProcessExecutor
        .execute(
            &graph,
            ExecutionRequest {
                run_id: "run".to_owned(),
                root_inputs: BTreeMap::new(),
                gates: Vec::new(),
                shared: SharedState::new(),
                spend_cap_micros: None,
                stops: RunStops::default(),
            },
            &store,
            &registry,
            &SystemClock,
        )
        .expect("the run answers");

    assert!(!closed_on_a_check(&execution.decisions), "{:?}", execution.decisions);
    assert_eq!(flow::run_status(&execution).0, "failed");
    assert_eq!(
        store.all().first().and_then(|record| record.outcome),
        Some(Outcome::Broke)
    );
}
