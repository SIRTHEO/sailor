//! `sailor flow run` and `sailor flow resume`: one run, its handoffs, and the
//! text of each step while it is running.

use flow::{
    ActionRegistry, Execution, Executor, FlowFile, InProcessExecutor, RecordStore, SystemClock,
};
use ledger::Ledger;
use serde_json::Value;
use std::fmt::Write as _;
use std::io::Write as IoWrite;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use ui::gather::FlowSource;

use super::{default_ledger_dir, missing_actions, new_run_id, now_secs, one_flow};

/// The last closed run's report, put where the trigger step carries it on.
/// Nothing to carry is nothing written: a first run must read as a first run,
/// and a trigger whose shape declares no room for it is handed nothing — an
/// offer a closed schema refuses kills the run before it spends anything.
fn put_previous_report(flow: &mut FlowFile, ledger: &Ledger) -> Result<(), String> {
    let Some(report) = ledger
        .last_run_report(&flow.id)
        .map_err(|error| {
            catalogue::say(
                "cli.flow.last_report_not_read",
                &[("flow", &flow.id), ("error", &error.to_string())],
            )
        })?
    else {
        return Ok(());
    };
    let Some(trigger) = flow
        .graph
        .steps()
        .iter()
        .find(|step| step.action == "trigger")
        .filter(|step| step.input_schema.accepts_property("previous_report"))
        .map(|step| step.id.clone())
    else {
        return Ok(());
    };
    let entry = flow
        .inputs
        .entry(trigger)
        .or_insert_with(|| Value::Object(serde_json::Map::new()));
    if let Value::Object(fields) = entry {
        fields.insert("previous_report".to_owned(), report);
    }
    Ok(())
}

/// Puts the mandate into the trigger step's input.
///
/// The step is found by the **action** it names, not by its id: a flow may call
/// its own trigger whatever it likes, and looking for a step named «trigger»
/// would work on the ones written so far and on nothing else.
fn put_mandate(flow: &mut FlowFile, text: &str) -> Result<(), String> {
    let trigger = flow
        .graph
        .steps()
        .iter()
        .find(|step| step.action == "trigger")
        .map(|step| step.id.clone())
        .ok_or_else(|| catalogue::say("cli.flow.no_trigger_step", &[("flow", &flow.id)]))?;
    let entry = flow
        .inputs
        .entry(trigger)
        .or_insert_with(|| Value::Object(serde_json::Map::new()));
    match entry {
        Value::Object(fields) => {
            fields.insert("text".to_owned(), Value::String(text.to_owned()));
            Ok(())
        }
        other => Err(catalogue::say(
            "cli.flow.trigger_input_not_an_object",
            &[("flow", &flow.id), ("other", &other.to_string())],
        )),
    }
}

/// What holds a handed step: the row's own holder first, then **a deadline
/// written in the record**. Fault 12 banned asking `pgrep`, which answers empty
/// inside the perimeter without an error; a row naming the second its holder was
/// born is asked of the kernel instead, and only a holder the kernel denies is
/// let go. A number alone stays held: *I cannot see* is not *it is dead*.
struct HandoffLease {
    now: i64,
}

impl flow::ProcessProbe for HandoffLease {
    fn is_running(&self, record: &flow::StepRecord) -> Result<bool, flow::FlowError> {
        match ledger::still_held(record) {
            ledger::StillHeld::Held | ledger::StillHeld::Uncertain => return Ok(true),
            ledger::StillHeld::Released => return Ok(false),
            ledger::StillHeld::Nobody => {}
        }
        let Some(limit) = record
            .input
            .get("handoff_timeout_secs")
            .and_then(Value::as_i64)
        else {
            // No readable deadline: it is held. The ambiguity is kept, it is
            // not settled on the convenient side.
            return Ok(true);
        };
        Ok(self.now < record.started_at.saturating_add(limit))
    }
}

/// Resumes a run: first reconciles what was left open, then executes **with the
/// same id**. **SEPARATE FROM `sailor step close`, BECAUSE THEY ARE TWO POWERS.**
/// `close` **remembers** — writes an outcome, spends nothing — while `resume`
/// **acts**, opens fronts and pays for billed calls; merging them would make a
/// write to the store spend money. `reconcile` had never run outside the tests,
/// hence a probe written to declare dead nothing it cannot see.
pub(super) fn resume_run(run_id: &str) -> Result<String, String> {
    let ledger = crate::step_cmd::open_ledger()?;
    let flow = crate::step_cmd::flow_of_run(&ledger, run_id)?;
    resume_run_in(&ledger, &flow, run_id)
}

/// The body of `resume`, with the store and the flow declared rather than
/// derived from `HOME` and the working directory: both are global to the
/// process, and a test that wrote them would spoil the others at random.
pub fn resume_run_in(ledger: &Ledger, flow: &FlowFile, run_id: &str) -> Result<String, String> {
    let root = workspace_root();
    announce_root(root.as_deref());
    let mut store = ledger.clone();
    resume_run_with(ledger, flow, run_id, &mut store, root.as_deref())
}

/// The body of `resume`, with the store and the root declared by the caller.
/// The command line resumes through the bare ledger, in the root it stands
/// in; the window resumes through a store that announces every step, and
/// says which root it resumed in, since the ledger does not keep a run's own.
pub fn resume_run_with(
    ledger: &Ledger,
    flow: &FlowFile,
    run_id: &str,
    store: &mut dyn RecordStore,
    root: Option<&Path>,
) -> Result<String, String> {
    let header = ledger
        .run_header(run_id)
        .map_err(|error| format!("cannot read run {run_id}: {error}"))?
        .ok_or_else(|| catalogue::say("cli.step.no_such_run", &[("run_id", run_id)]))?;
    let registry = default_registry(
        Some(ledger.clone()),
        Some(Arc::new(TerminalWatcher::new()) as Arc<dyn actions::StepSinks>),
    );
    // Reconciliation reads an unregistered action as an unknown effect and
    // parks every open step on a person who has nothing to decide. The first
    // run refuses this flow; so does the resume.
    let missing = missing_actions(&flow.graph, &registry);
    if !missing.is_empty() {
        return Err(catalogue::say(
            "cli.flow.names_unregistered_actions",
            &[
                ("flow", &flow.id),
                (
                    "actions",
                    &missing.into_iter().collect::<Vec<_>>().join(", "),
                ),
            ],
        ));
    }
    // **THE START INSTANT IS THE OLD ONE, NOT THE HOUR OF THE RESUME.** The
    // header is rewritten whole on every update: the resume hour here would make
    // the run look started when it was resumed. `last_started_at` depends on it —
    // which flows `sailor flow due` calls due — and so does the duration the
    // window shows. A run handed over and resumed a day later would read a minute.
    let started_at = header.started_at;
    let now = now_secs()?;

    // THE RESUME GOES THROUGH THE ONE CONSTRUCTOR, like the first run, and
    // keeps the same id: a request built by hand would lose the workspace
    // root in silence, and a new id would redo every step already paid for.
    let request = registry::execution_request(Some(ledger), flow, run_id, root, started_at);
    // Reconciliation sees what the execution will see: same root, same shared
    // state.
    let shared = request.shared.clone();
    let probe = HandoffLease { now };
    let reconciled = InProcessExecutor
        .reconcile(flow::ReconciliationRequest {
            graph: &flow.graph,
            run_id,
            store: &mut *store,
            actions: &registry,
            shared: &shared,
            processes: &probe,
            clock: &SystemClock,
        })
        .map_err(|error| format!("cannot reconcile run {run_id}: {error}"))?;

    let mut report = format!("run {run_id} — flow {}", flow.id);
    if !reconciled.still_running.is_empty() {
        let _ = write!(
            report,
            "\n{}",
            catalogue::say(
                "cli.flow.held_deadline_not_passed",
                &[("steps", &reconciled.still_running.join(", "))],
            )
        );
    }
    if !reconciled.closed_as_broke.is_empty() {
        let _ = write!(
            report,
            "\n{}",
            catalogue::say(
                "cli.flow.expired_back_among_the_ready",
                &[("steps", &reconciled.closed_as_broke.join(", "))],
            )
        );
    }
    if !reconciled.closed_as_waiting.is_empty() {
        let _ = write!(
            report,
            "\nleft to a person: {}",
            reconciled.closed_as_waiting.join(", ")
        );
    }

    let execution = InProcessExecutor
        .execute(&flow.graph, request, &*store, &registry, &SystemClock)
        .map_err(|error| format!("resuming run {run_id} failed: {error}"))?;

    let (status, exit_ok) = execution_status(&execution);
    let why =
        registry::stopped_by_cap(&execution).or_else(|| registry::halted_by_hand(&execution));
    record_run(
        ledger,
        flow,
        run_id,
        status,
        started_at,
        Some(now_secs()?),
        why.clone(),
        registry::how_it_stopped(&execution),
    )?;
    report.push_str(&catalogue::say("cli.flow.run_status", &[("status", status)]));
    if exit_ok {
        Ok(report)
    } else {
        match why {
            Some(why) => Err(format!("{report}\n{why}")),
            None => Err(report),
        }
    }
}

// ── the text of a step while the step runs ────────────────────────────

/// Which pipe the still-open line came from, if there is one.
///
/// `None` means «we are at the start of a line»: the next byte wants a marker
/// in front of it.
#[derive(Default)]
struct LineState {
    open: Option<actions::Pipe>,
}

/// Pours the bytes out as they arrived, prefixing `[step · out]` or
/// `[step · err]` to each line. **IT DECODES NOTHING**: bytes leave in the order
/// they came in, so a UTF-8 sequence split across two reads recomposes itself on
/// the terminal rather than becoming a replacement character or a panic; the
/// markers added are ASCII, at line start. **A LINE BELONGS TO ONE PIPE ONLY**:
/// stderr mid stdout line closes it first, or an error would read as normal output.
fn marked(
    out: &mut impl IoWrite,
    state: &mut LineState,
    step: &str,
    pipe: actions::Pipe,
    bytes: &[u8],
) -> std::io::Result<()> {
    let mut rest = bytes;
    while !rest.is_empty() {
        if state.open.is_some_and(|open| open != pipe) {
            out.write_all(b"\n")?;
            state.open = None;
        }
        if state.open.is_none() {
            write!(out, "[{step} · {}] ", pipe.name())?;
            state.open = Some(pipe);
        }
        match rest.iter().position(|byte| *byte == b'\n') {
            Some(end) => {
                out.write_all(&rest[..=end])?;
                state.open = None;
                rest = &rest[end + 1..];
            }
            None => {
                out.write_all(rest)?;
                rest = &[];
            }
        }
    }
    Ok(())
}

/// Where the steps' text ends up.
///
/// **A FACTORY AND NOT ONE WRITER**, because the handle on the terminal is taken
/// and released per chunk: holding it open between deliveries would block anyone
/// else writing, and the two threads draining the pipes deliver together.
type Screenward = Arc<dyn Fn() -> Box<dyn IoWrite> + Send + Sync>;

/// A step's sink: writes to the terminal what the step says, while it says it.
struct StepEcho {
    step: String,
    /// One lock per step: the two threads draining stdout and stderr call
    /// together, and with no serialising the lines would interleave halfway —
    /// the marker included.
    state: Mutex<LineState>,
    out: Screenward,
}

impl actions::LiveSink for StepEcho {
    fn chunk(&self, pipe: actions::Pipe, bytes: &[u8]) {
        // A poisoned lock is no reason to drop the step: all that happens here
        // is showing text, and the real work is elsewhere.
        let mut state = match self.state.lock() {
            Ok(state) => state,
            Err(poisoned) => poisoned.into_inner(),
        };
        // **ON STDERR, NOT ON STDOUT.** The final report leaves by stdout and
        // keeps its shape: redirecting it to a file must not catch the steps'
        // text there. Which descriptor it is, the factory says — chosen by
        // whoever built the sink.
        let mut out = (self.out)();
        let _ = marked(&mut out, &mut state, &self.step, pipe, bytes);
        // The flush per chunk is the point of the work: without it the text
        // would sit in a buffer until the end — the old defect moved a metre.
        let _ = out.flush();
    }
}

/// Shows a run's step text on the terminal.
///
/// It lives here and not in `actions` because it is a presentation decision:
/// where the text goes is picked by whoever composes the program. A second
/// consumer — a file, the window, the store — would be another `StepSinks`
/// implementation, not a change to the crate that executes.
struct TerminalWatcher {
    out: Screenward,
}

impl TerminalWatcher {
    /// The real terminal: stderr.
    fn new() -> Self {
        Self {
            out: Arc::new(|| Box::new(std::io::stderr().lock())),
        }
    }

    /// The same chain towards another destination.
    ///
    /// **IT EXISTS SO THE TEXT'S ARRIVAL CAN BE TIMED.** With stderr wired into
    /// `chunk`, the only check available was rereading the code and finding it
    /// convincing — exactly how the old defect got through: it delivered
    /// everything at the end and looked right.
    #[cfg(test)]
    fn writing_to(out: Screenward) -> Self {
        Self { out }
    }
}

impl actions::StepSinks for TerminalWatcher {
    fn sink_for(&self, step: &str) -> Arc<dyn actions::LiveSink> {
        Arc::new(StepEcho {
            step: step.to_owned(),
            state: Mutex::new(LineState::default()),
            out: Arc::clone(&self.out),
        })
    }
}

/// Runs a flow, with an optional mandate that enters through the trigger.
/// **THE MANDATE IS PASSED, NOT WRITTEN INTO THE FILE.** Measuring a flow against
/// a bare prompt needs both to get the same brief; the only way was rewriting the
/// `.flow.json` by hand before each run — fault 15, «Sailor has no command for its
/// own flows, so whoever works with it works around it». The text replaces the
/// trigger input's `text`; a flow with no trigger refuses it aloud (fault 20).
pub(super) fn run_flow(sources: &[FlowSource], name: &str, mandate: Option<&str>) -> Result<String, String> {
    let (mut flow, _) = one_flow(sources, name)?;
    if let Some(text) = mandate {
        put_mandate(&mut flow, text)?;
    }
    // Before the store, before the run's header, before anything is spent: a
    // flow that requires a guaranteed cap this machine cannot give it stops
    // here rather than finding out from the bill.
    if let Some(why) = super::check::why_a_run_here_would_not_start(&flow) {
        return Err(why);
    }
    // THE STORE BEFORE THE REGISTRY, and it is no detail of ordering: the
    // `store_write`/`store_read` nodes own it, so a registry built first would
    // lack them and call two existing actions missing.
    let ledger_dir = default_ledger_dir()?;
    let ledger = Ledger::open(&ledger_dir).map_err(|error| {
        catalogue::say(
            "cli.flow.ledger_will_not_open",
            &[
                ("directory", &ledger_dir.display().to_string()),
                ("error", &error.to_string()),
            ],
        )
    })?;
    put_previous_report(&mut flow, &ledger)?;
    // THE WATCHER IS THE TERMINAL, and only here: `flow check` executes nothing
    // and has no text to show.
    let registry = default_registry(
        Some(ledger.clone()),
        Some(Arc::new(TerminalWatcher::new()) as Arc<dyn actions::StepSinks>),
    );
    let missing = missing_actions(&flow.graph, &registry);
    if !missing.is_empty() {
        return Err(catalogue::say(
            "cli.flow.names_unregistered_actions",
            &[
                ("flow", &flow.id),
                (
                    "actions",
                    &missing.into_iter().collect::<Vec<_>>().join(", "),
                ),
            ],
        ));
    }

    let run_id = new_run_id(&flow.id)?;
    let started_at = now_secs()?;
    record_run(
        &ledger, &flow, &run_id, "running", started_at, None, None, None,
    )?;

    let store = ledger.clone();
    let result = execute_flow(
        Some(&ledger),
        &flow,
        &run_id,
        started_at,
        &store,
        &registry,
        &SystemClock,
    );
    match result {
        Ok(execution) => {
            let (status, exit_ok) = execution_status(&execution);
            // The cap reached carries its numbers: they land in the run's row,
            // so rereading the history a week later tells what the cap was then
            // and how much had been spent. A run that stopped itself says which
            // of the four reasons did it.
            let why = registry::stopped_by_cap(&execution)
                .or_else(|| registry::halted_by_hand(&execution));
            record_run(
                &ledger,
                &flow,
                &run_id,
                status,
                started_at,
                Some(now_secs()?),
                why.clone(),
                registry::how_it_stopped(&execution),
            )?;
            if exit_ok {
                Ok(catalogue::say(
                    "cli.flow.run_complete",
                    &[("flow", &flow.id), ("run", &run_id)],
                ))
            } else {
                Err(match why {
                    Some(why) => catalogue::say(
                        "cli.flow.run_stopped",
                        &[("flow", &flow.id), ("why", &why), ("run", &run_id)],
                    ),
                    None => catalogue::say(
                        "cli.flow.run_ended_with_status",
                        &[("flow", &flow.id), ("status", status), ("run", &run_id)],
                    ),
                })
            }
        }
        Err(error) => {
            let said = error.to_string();
            record_run(
                &ledger,
                &flow,
                &run_id,
                "failed",
                started_at,
                Some(now_secs()?),
                Some(said.clone()),
                None,
            )?;
            Err(catalogue::say(
                "cli.flow.run_failed",
                &[("flow", &flow.id), ("said", &said), ("run", &run_id)],
            ))
        }
    }
}

fn execute_flow(
    ledger: Option<&Ledger>,
    flow: &FlowFile,
    run_id: &str,
    started_at: i64,
    store: &dyn RecordStore,
    registry: &ActionRegistry,
    clock: &dyn flow::Clock,
) -> Result<Execution, Box<flow::FlowError>> {
    let root = workspace_root();
    announce_root(root.as_deref());
    InProcessExecutor
        .execute(
            &flow.graph,
            registry::execution_request(ledger, flow, run_id, root.as_deref(), started_at),
            store,
            registry,
            clock,
        )
        .map_err(Box::new)
}

/// The project root for this run, walking up from where it was launched.
pub(super) fn workspace_root() -> Option<PathBuf> {
    let working = std::env::current_dir().ok()?;
    flow::workspace::find_root(&working)
}

/// **THE LAUNCHER SAYS WHERE IT DECIDED TO WORK, BEFORE STARTING.**
///
/// Without this line the plan has a silent way of going wrong, **the same** as
/// the fault that closes: the flow works in a place nobody saw written down. A
/// missing root is information as much as its value is — it says in advance why
/// a step with `workdir` is about to fail.
fn announce_root(root: Option<&Path>) {
    match root {
        Some(root) => println!(
            "{}",
            catalogue::say(
                "cli.flow.project_root",
                &[("root", &root.display().to_string())]
            )
        ),
        None => println!(
            "{}",
            catalogue::say(
                "cli.flow.no_project_root",
                &[("marker", flow::workspace::MARKER)]
            )
        ),
    }
}

/// How the run ended. The body lives in `registry`, with its twin from the
/// shell: there were two, and a new `Decision` would have made them diverge.
fn execution_status(execution: &Execution) -> (&'static str, bool) {
    registry::execution_status(execution)
}

/// Records the run's header.
///
/// **THE BODY LIVES IN `registry`, AND NOT FOR ELEGANCE.** These twenty lines
/// were written in the window's shell too, and both wrote `total_cost_micros: 0`
/// by hand into a field the window shows: repairing one of the two would give
/// two different totals for the same run, depending on who had launched it.
#[allow(clippy::too_many_arguments)]
pub(super) fn record_run(
    ledger: &Ledger,
    flow: &FlowFile,
    run_id: &str,
    status: &str,
    started_at: i64,
    ended_at: Option<i64>,
    error: Option<String>,
    stop_reason: Option<flow::StopReason>,
) -> Result<(), String> {
    registry::record_flow_run(
        ledger,
        flow,
        registry::FlowRun {
            run_id,
            status,
            started_at,
            ended_at,
            error,
            started_by: seat_of(std::env::var_os("SAILOR_TERMINAL").is_some()),
            stop_reason,
        },
    )?;
    // The report is written after the header and only for a closed run, so it
    // reads the status this close just wrote. The store keeps it to one: a
    // resume that finds the run closed crosses this same point.
    if let Some(closed_at) = ended_at {
        ledger
            .write_run_report(run_id, "sailor flow", closed_at)
            .map_err(|error| {
                catalogue::say(
                    "cli.flow.report_not_written",
                    &[("run", run_id), ("error", &error.to_string())],
                )
            })?;
    }
    Ok(())
}

/// Which seat this command line runs from: a pane the terminal host opened
/// carries its mark, a bare shell does not. The window names its own seat.
pub(crate) fn seat_of(in_a_sailor_terminal: bool) -> &'static str {
    if in_a_sailor_terminal {
        "sailor flow, in a Sailor terminal"
    } else {
        "sailor flow, in a shell"
    }
}

/// The action registry lives in `crates/registry`, for a measured reason: this
/// list was written in the window's shell too, the two copies drifted apart
/// three times, and the last drift ran the same flow in two different ways
/// depending on who launched it.
use registry::default_registry;

#[cfg(test)]
mod tests {
    use super::super::test_support::*;
    use super::*;
    use flow::{Decision, InMemoryRecordStore, ProcessProbe, StepRecord};
    use registry::{registry_in, House};
    use std::time::{Duration, Instant};

    // ── the handoff probe ────────────────────────────────────────────────

    fn a_handed_record(started_at: i64, limit: Option<i64>, pid: Option<u32>) -> StepRecord {
        let input = match limit {
            Some(limit) => serde_json::json!({"handoff_timeout_secs": limit}),
            None => serde_json::json!({"mandate": "with no deadline"}),
        };
        let mut record = StepRecord::started(
            "run-1",
            "implementa",
            1,
            1,
            vec![],
            input,
            vec![],
            started_at,
        );
        record.held_by_pid = pid;
        record
    }

    /// A deadline in the future holds the step; once passed, it lets it go.
    #[test]
    fn the_lease_reads_the_deadline_and_not_the_kernel() {
        let probe = HandoffLease { now: 1_000 };
        assert!(
            probe
                .is_running(&a_handed_record(900, Some(600), None))
                .expect("the probe answers"),
            "the deadline is in the future: the step is held"
        );
        assert!(
            !probe
                .is_running(&a_handed_record(100, Some(600), None))
                .expect("the probe answers"),
            "the deadline has passed: nobody took it on"
        );
    }

    /// **I CANNOT SEE, SO I DO NOT DECLARE IT DEAD.** A record with a pid was
    /// opened by the in-process executor; this probe has no way to look at that
    /// process — and must not ask the operating system, which is fault 12. The
    /// same holds for a record with no readable deadline.
    #[test]
    fn what_the_lease_cannot_see_it_does_not_declare_dead() {
        let probe = HandoffLease { now: 1_000_000 };
        assert!(
            probe
                .is_running(&a_handed_record(1, Some(1), Some(4321)))
                .expect("the probe answers"),
            "with a pid written down the step is held, even with the deadline passed"
        );
        assert!(
            probe
                .is_running(&a_handed_record(1, None, None))
                .expect("the probe answers"),
            "with no readable deadline the ambiguity is kept"
        );
    }

    /// A holder the kernel denies lets the step go; a number alone does not.
    ///
    /// The row born in another epoch names a process that either is not there
    /// or is not the one that opened the step: both are «released».
    #[test]
    fn a_holder_the_kernel_denies_is_not_holding_anything() {
        let probe = HandoffLease { now: 1_000_000 };
        let mut denied = a_handed_record(1, Some(1), Some(std::process::id()));
        denied.held_by = Some(flow::HolderIdentity {
            born_at: Some(1),
            invocation: "una-corsa-che-non-c-e-piu".to_owned(),
        });
        assert!(
            !probe.is_running(&denied).expect("the probe answers"),
            "the number is this process, the second of birth is not: nobody holds it"
        );

        let mut mine = denied.clone();
        mine.held_by = Some(ledger::this_process_holds());
        assert!(
            probe.is_running(&mine).expect("the probe answers"),
            "this very process holds it"
        );
    }

    /// The seat a run is started from is written by the system, not told: a
    /// pane of the terminal host and a bare shell are two different rows.
    #[test]
    fn the_seat_of_a_run_names_the_pane_or_the_shell() {
        assert_eq!(seat_of(true), "sailor flow, in a Sailor terminal");
        assert_eq!(seat_of(false), "sailor flow, in a shell");
        assert_ne!(seat_of(true), seat_of(false));
    }

    /// **A LIVE HANDOFF IS NOT CLOSED UNDER THE AGENT WORKING ON IT.**
    ///
    /// It is the mutant that counts in all this work: a probe that always
    /// answers «no» closes as `Broke` a step someone is executing, and the
    /// resume relaunches it — two agents on one mandate, neither of them aware.
    #[test]
    fn a_live_handoff_is_not_closed_under_the_agent_who_holds_it() {
        let flow: FlowFile = serde_json::from_str(
            r#"{
                "id": "consegna-viva",
                "description": "un passo consegnato e ancora nei tempi",
                "graph": {"steps": [{
                    "id": "implementa",
                    "deps": [],
                    "input_schema": {"type": "any"},
                    "output_schema": {"type": "any"},
                    "when": null,
                    "action": "handed_to_agent",
                    "max_attempts": 3
                }]},
                "inputs": {}
            }"#,
        )
        .expect("the scratch flow is valid");

        let mut store =
            InMemoryRecordStore::from_records(vec![a_handed_record(1_000, Some(3_600), None)]);
        let registry = registry_in(House::empty(), None, None);
        let shared = flow::SharedState::new();
        let probe = HandoffLease { now: 1_100 };
        let report = InProcessExecutor
            .reconcile(flow::ReconciliationRequest {
                graph: &flow.graph,
                run_id: "run-1",
                store: &mut store,
                actions: &registry,
                shared: &shared,
                processes: &probe,
                clock: &SystemClock,
            })
            .expect("the reconciliation answers");

        assert_eq!(
            report.still_running,
            vec!["implementa".to_owned()],
            "the step is held: the deadline has not passed"
        );
        assert!(
            report.closed_as_broke.is_empty(),
            "closing it would put it back among the ready while somebody is working on it: {report:?}"
        );
        assert!(
            store.all()[0].outcome.is_none(),
            "the record must stay open"
        );
    }

    /// **A RESUME DOES NOT REWRITE THE START INSTANT.**
    ///
    /// A run's header is rewritten whole on every update. With the resume hour in
    /// it, a run handed over at night and resumed next morning would read as
    /// started in the morning: `sailor flow due` would call its flow not due, and
    /// the duration shown would be a minute instead of ten hours.
    #[test]
    fn resuming_a_run_keeps_the_hour_it_started() {
        let home = TestDirectory::new();
        let flow_file: FlowFile = serde_json::from_str(
            r#"{
                "id": "ripresa-di-prova",
                "description": "un passo gia' andato",
                "graph": {"steps": [{
                    "id": "implementa",
                    "deps": [],
                    "input_schema": {"type": "any"},
                    "output_schema": {"type": "any"},
                    "when": null,
                    "action": "handed_to_agent",
                    "max_attempts": 3
                }]},
                "inputs": {}
            }"#,
        )
        .expect("the scratch flow is valid");

        let ledger = Ledger::open(&home.0).expect("the ledger opens");
        ledger
            .record_run(&ledger::RunRecord {
                run_id: "run-vecchia".to_owned(),
                kind: "flow".to_owned(),
                entity: "ripresa-di-prova".to_owned(),
                parent_run_id: None,
                started_by: "prova".to_owned(),
                status: "waiting".to_owned(),
                total_cost_micros: 0,
                error: None,
                started_at: 1_000,
                ended_at: Some(1_500),
                worktree: None,
                stop_reason: None,
            })
            .expect("recording the run");
        let mut record = StepRecord::started(
            "run-vecchia",
            "implementa",
            1,
            1,
            vec![],
            serde_json::json!({"handoff_timeout_secs": 60}),
            vec![],
            1_100,
        );
        record.species = Some(flow::StepSpecies::Repeatable);
        ledger
            .append_step_started(&record)
            .expect("opening the step");
        ledger
            .close_step(
                "run-vecchia",
                "implementa",
                1,
                1,
                flow::Completion {
                    outcome: flow::Outcome::Went,
                    output: Some(serde_json::json!({})),
                    said: None,
                    failure_class: None,
                    refusal: None,
                    ran: None,
                    ended_at: 1_500,
                    bytes_seen: None,
                    bytes_discarded: None,
                },
            )
            .expect("closing the step");

        let report = resume_run_in(&ledger, &flow_file, "run-vecchia")
            .expect("the run resumes: its only step has already gone");
        assert!(report.contains("complete"), "{report}");

        let header = ledger
            .run_header("run-vecchia")
            .expect("the header reads back")
            .expect("the run exists");
        assert_eq!(
            header.started_at, 1_000,
            "the start instant stays the first run's: with the hour of the resume, \
             a run handed over at night and resumed the next morning would read \
             as started in the morning"
        );
        assert_eq!(header.status, "complete", "the resume updates the status");
    }

    /// A parked step never becomes ready again, so the resume must refuse the
    /// way the first run does.
    #[test]
    fn resuming_a_run_whose_action_nobody_registers_is_refused_and_not_parked() {
        let home = TestDirectory::new();
        let flow_file: FlowFile = serde_json::from_str(
            r#"{
                "id": "ripresa-senza-azione",
                "description": "un passo che nomina un'azione che nessuno registra",
                "graph": {"steps": [{
                    "id": "issa-la-vela",
                    "deps": [],
                    "input_schema": {"type": "any"},
                    "output_schema": {"type": "any"},
                    "when": null,
                    "action": "hoist_the_mainsail",
                    "max_attempts": 3
                }]},
                "inputs": {}
            }"#,
        )
        .expect("the scratch flow is valid");

        let ledger = Ledger::open(&home.0).expect("the ledger opens");
        ledger
            .record_run(&ledger::RunRecord {
                run_id: "corsa-orfana".to_owned(),
                kind: "flow".to_owned(),
                entity: "ripresa-senza-azione".to_owned(),
                parent_run_id: None,
                started_by: "prova".to_owned(),
                status: "waiting".to_owned(),
                total_cost_micros: 0,
                error: None,
                started_at: 1_000,
                ended_at: None,
                worktree: None,
                stop_reason: None,
            })
            .expect("recording the run");
        ledger
            .append_step_started(&StepRecord::started(
                "corsa-orfana",
                "issa-la-vela",
                1,
                1,
                vec![],
                serde_json::json!({}),
                vec![],
                1_100,
            ))
            .expect("opening the step");

        let refusal = resume_run_in(&ledger, &flow_file, "corsa-orfana")
            .expect_err("the resume refuses a flow naming an action nobody registers");
        assert!(
            refusal.contains("hoist_the_mainsail"),
            "the refusal must name the action: {refusal}"
        );

        let records = ledger
            .records("corsa-orfana")
            .expect("the records read back");
        assert!(
            records[0].outcome.is_none(),
            "the step was closed instead of left alone: {:?}",
            records[0].outcome
        );
    }

    // ── the report the run before it left ────────────────────────────

    /// The report of the last closed run reaches the trigger step's input, so
    /// the run after reads what the one before left, and a flow with nothing
    /// behind it carries nothing rather than an empty report.
    #[test]
    fn the_previous_report_reaches_the_trigger_and_only_when_there_is_one() {
        let json = r#"{
            "id": "un-flusso", "description": "flusso con innesco",
            "graph": {"steps": [{
                "id": "innesco", "deps": [], "action": "trigger", "max_attempts": 1,
                "when": null, "input_schema": {"type": "any"}, "output_schema": {"type": "any"}
            }]},
            "inputs": {"innesco": {"source": "manual", "text": "un incarico"}}
        }"#;
        let dir = std::env::temp_dir().join(format!(
            "sailor-rapporto-precedente-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        let ledger = Ledger::open(&dir).expect("the ledger opens");

        let mut first: FlowFile = serde_json::from_str(json).expect("it loads");
        put_previous_report(&mut first, &ledger).expect("nothing to carry over");
        assert!(
            first.inputs["innesco"].get("previous_report").is_none(),
            "with no run closed before it, the trigger carries nothing: {:?}",
            first.inputs["innesco"]
        );

        ledger
            .record_run(&ledger::RunRecord {
                run_id: "corsa-1".to_owned(),
                kind: "flow".to_owned(),
                entity: "un-flusso".to_owned(),
                parent_run_id: None,
                started_by: "test".to_owned(),
                status: "complete".to_owned(),
                total_cost_micros: 0,
                error: None,
                started_at: 10,
                ended_at: Some(20),
                worktree: None,
                stop_reason: None,
            })
            .expect("the run's row");
        ledger
            .write_run_report("corsa-1", "test", 30)
            .expect("the report is written");

        let mut second: FlowFile = serde_json::from_str(json).expect("it loads");
        put_previous_report(&mut second, &ledger).expect("the report goes in");

        assert_eq!(
            second.inputs["innesco"]["previous_report"]["run_id"], "corsa-1",
            "the run after reads the report of the one before: {:?}",
            second.inputs["innesco"]
        );
        let _ = std::fs::remove_dir_all(dir);
    }

    /// **A CLOSED TRIGGER IS OFFERED NOTHING IT DECLARED NO ROOM FOR.** Writing
    /// the report into a schema that forbids extras killed the run on «expected
    /// declared property» — so a flow whose trigger is closed could be launched
    /// once and never again, and the second launch died before spending.
    #[test]
    fn a_trigger_that_declares_no_room_for_the_report_is_not_handed_one() {
        let json = r#"{
            "id": "un-flusso-chiuso", "description": "innesco a forma chiusa",
            "graph": {"steps": [{
                "id": "innesco", "deps": [], "action": "trigger", "max_attempts": 1,
                "when": null, "output_schema": {"type": "any"},
                "input_schema": {"type": "object", "allow_extra": false,
                    "required": ["source", "text"],
                    "properties": {"source": {"type": "string"}, "text": {"type": "string"}}}
            }]},
            "inputs": {"innesco": {"source": "manual", "text": "un incarico"}}
        }"#;
        let dir = std::env::temp_dir().join(format!(
            "sailor-innesco-chiuso-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        let ledger = Ledger::open(&dir).expect("the ledger opens");
        ledger
            .record_run(&ledger::RunRecord {
                run_id: "corsa-1".to_owned(),
                kind: "flow".to_owned(),
                entity: "un-flusso-chiuso".to_owned(),
                parent_run_id: None,
                started_by: "test".to_owned(),
                status: "complete".to_owned(),
                total_cost_micros: 0,
                error: None,
                started_at: 10,
                ended_at: Some(20),
                worktree: None,
                stop_reason: None,
            })
            .expect("the run's row");
        ledger
            .write_run_report("corsa-1", "test", 30)
            .expect("the report is written");

        let mut flow: FlowFile = serde_json::from_str(json).expect("it loads");
        put_previous_report(&mut flow, &ledger).expect("nothing to carry over");

        assert!(
            flow.inputs["innesco"].get("previous_report").is_none(),
            "a closed-shape trigger does not receive the report: {:?}",
            flow.inputs["innesco"]
        );
        let _ = std::fs::remove_dir_all(dir);
    }

    // ── the mandate that enters through the trigger ──────────────────

    /// **ONE FLOW WITH TWO DIFFERENT MANDATES, WITHOUT TOUCHING THE FILE.**
    /// The way in used to be rewriting the `.flow.json`, so two runs back to
    /// back with two different briefs were impossible — the defect that made
    /// measuring a flow against a prompt impossible.
    #[test]
    fn a_mandate_from_the_command_line_reaches_the_trigger() {
        let json = r#"{
            "id": "prova", "description": "flusso con innesco",
            "graph": {"steps": [{
                "id": "innesco", "deps": [], "action": "trigger", "max_attempts": 1,
                "when": null, "input_schema": {"type": "any"}, "output_schema": {"type": "any"}
            }]},
            "inputs": {"innesco": {"source": "manual", "text": "quello di prima"}}
        }"#;
        let mut flow: FlowFile = serde_json::from_str(json).expect("it loads");

        put_mandate(&mut flow, "the work of right now").expect("the mandate goes in");

        assert_eq!(flow.inputs["innesco"]["text"], "the work of right now");
        assert_eq!(
            flow.inputs["innesco"]["source"], "manual",
            "and it does not take the rest of the input away"
        );
    }

    /// A flow with no trigger **refuses** the mandate rather than swallowing it:
    /// a brief that lands nowhere would run the flow on something else, while
    /// whoever wrote it would believe they had aimed it.
    #[test]
    fn a_flow_without_a_trigger_refuses_the_mandate() {
        let json = flow_json("shell_check", "[]", "{}");
        let mut flow: FlowFile = serde_json::from_str(&json).expect("it loads");

        let refused = put_mandate(&mut flow, "an errand").expect_err("it must not accept it");

        assert!(
            refused.contains("has no trigger step"),
            "and it says why: {refused}"
        );
    }

    // ── the marker on the live text ──────────────────────────────────

    /// The text `marked` produces, without touching any terminal.
    fn marking(step: &str, chunks: &[(actions::Pipe, &[u8])]) -> Vec<u8> {
        let mut out = Vec::new();
        let mut state = LineState::default();
        for (pipe, bytes) in chunks {
            marked(&mut out, &mut state, step, *pipe, bytes).expect("a Vec does not fail");
        }
        out
    }

    #[test]
    fn every_line_says_which_step_and_which_pipe_it_came_from() {
        let out = marking(
            "prova-le-cose",
            &[
                (actions::Pipe::Stdout, b"prima\nseconda\n"),
                (actions::Pipe::Stderr, b"guasto\n"),
            ],
        );
        assert_eq!(
            String::from_utf8(out).expect("plain ASCII"),
            "[prova-le-cose · out] prima\n\
             [prova-le-cose · out] seconda\n\
             [prova-le-cose · err] guasto\n"
        );
    }

    /// A CHUNK IS NOT A LINE: a read stops where it stops, and the marker goes
    /// at the start of a line, not of a chunk — or a line split in three would
    /// print three of them.
    #[test]
    fn a_line_split_across_chunks_gets_one_marker_only() {
        let out = marking(
            "passo",
            &[
                (actions::Pipe::Stdout, b"una riga "),
                (actions::Pipe::Stdout, b"spezzata "),
                (actions::Pipe::Stdout, b"in tre\n"),
            ],
        );
        assert_eq!(
            String::from_utf8(out).expect("plain ASCII"),
            "[passo · out] una riga spezzata in tre\n"
        );
    }

    /// NO CORRUPTED TEXT AND NO PANIC on a UTF-8 sequence cut in half between
    /// two reads: the bytes are never decoded, they leave in the order they came
    /// in, and the character recomposes itself.
    #[test]
    fn a_multibyte_character_split_between_chunks_comes_out_intact() {
        // «però» in UTF-8: the `ò` is two bytes, and the cut falls between them.
        let text = "però".as_bytes();
        let cut = text.len() - 1;
        let out = marking(
            "passo",
            &[
                (actions::Pipe::Stdout, &text[..cut]),
                (actions::Pipe::Stdout, &text[cut..]),
                (actions::Pipe::Stdout, b"\n"),
            ],
        );
        assert_eq!(
            String::from_utf8(out).expect("the text recomposes"),
            "[passo · out] però\n"
        );
    }

    /// A line belongs to one pipe only: if stderr interrupts a still-open stdout
    /// line, that line closes first — or an error would land under the marker of
    /// normal output.
    #[test]
    fn stderr_never_lands_inside_an_open_stdout_line() {
        let out = marking(
            "passo",
            &[
                (actions::Pipe::Stdout, b"a meta"),
                (actions::Pipe::Stderr, b"allarme\n"),
                (actions::Pipe::Stdout, b" e poi\n"),
            ],
        );
        assert_eq!(
            String::from_utf8(out).expect("plain ASCII"),
            "[passo · out] a meta\n\
             [passo · err] allarme\n\
             [passo · out]  e poi\n"
        );
    }

    // ── the text reaches the screen while the step runs ──────────────

    /// A fake screen that behaves like a buffered terminal: what is written to
    /// it stays invisible until it is asked to flush.
    ///
    /// **IT RECORDS THE INSTANT ON THE FLUSH, NOT ON THE `write`**: a recorder
    /// stamping the hour on every write would stay green with the flush taken
    /// out of the real code. «Written» and «visible» are two facts; this is the
    /// second one.
    struct Screen {
        start: Instant,
        pending: Mutex<Vec<u8>>,
        shown: Mutex<Vec<(Duration, Vec<u8>)>>,
    }

    impl Screen {
        fn new(start: Instant) -> Arc<Self> {
            Arc::new(Self {
                start,
                pending: Mutex::new(Vec::new()),
                shown: Mutex::new(Vec::new()),
            })
        }

        fn shown(&self) -> Vec<(Duration, Vec<u8>)> {
            self.shown.lock().expect("nessuno panica qui").clone()
        }

        /// Everything that became visible, in order.
        fn visible_text(&self) -> String {
            let joined: Vec<u8> = self
                .shown()
                .into_iter()
                .flat_map(|(_, bytes)| bytes)
                .collect();
            String::from_utf8_lossy(&joined).into_owned()
        }
    }

    /// The handle the factory hands out per chunk: it writes to the shared
    /// screen, so the handles after it continue the same text.
    struct ScreenHandle(Arc<Screen>);

    impl IoWrite for ScreenHandle {
        fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
            self.0
                .pending
                .lock()
                .expect("nessuno panica qui")
                .extend_from_slice(buf);
            Ok(buf.len())
        }

        fn flush(&mut self) -> std::io::Result<()> {
            let mut pending = self.0.pending.lock().expect("nessuno panica qui");
            if pending.is_empty() {
                return Ok(());
            }
            let bytes = std::mem::take(&mut *pending);
            self.0
                .shown
                .lock()
                .expect("nessuno panica qui")
                .push((self.0.start.elapsed(), bytes));
            Ok(())
        }
    }

    /// THE WHOLE CHAIN, TIMED: child pipe → `drain` → `StepEcho::chunk` →
    /// `marked` → write → flush → screen. The command prints, sleeps four
    /// seconds, prints again, and what is checked is **when** the first line
    /// became visible: checking the text is there at the end would be green even
    /// if it all showed on the child's death. Wide margins as in the twin test in
    /// `actions` — four seconds of sleep against a threshold of two, so a loaded
    /// machine does not redden it at random.
    #[test]
    fn the_terminal_shows_a_step_talking_while_the_step_is_still_running() {
        let start = Instant::now();
        let screen = Screen::new(start);
        let watcher = TerminalWatcher::writing_to({
            let screen = Arc::clone(&screen);
            Arc::new(move || Box::new(ScreenHandle(Arc::clone(&screen))) as Box<dyn IoWrite>)
        });
        let sink = actions::StepSinks::sink_for(&watcher, "un-passo-che-parla");

        let mut cmd = std::process::Command::new("sh");
        cmd.arg("-c").arg("echo primo; sleep 4; echo secondo");
        let outcome =
            actions::run_with_timeout_watched(cmd, Duration::from_secs(30), Some(sink.as_ref()));
        let whole = start.elapsed();
        assert!(
            whole >= Duration::from_secs(4),
            "the command really had to take four seconds, or the measure tells \
             nothing apart: {whole:?}"
        );

        let shown = screen.shown();
        let (when, bytes) = shown
            .first()
            .cloned()
            .expect("something had to become visible on the screen");
        // THE TIME BEFORE THE CONTENT: the instant is what this test measures,
        // and reading it last would hide the real reason for a red.
        assert!(
            when < Duration::from_secs(2),
            "the first piece became visible after {when:?}, that is with the end \
             of the command and not while it ran (whole duration {whole:?})"
        );
        let first = String::from_utf8_lossy(&bytes).into_owned();
        assert!(
            first.contains("[un-passo-che-parla · out] primo"),
            "whoever watches must know step and pipe from the very first line: {first:?}"
        );
        let all = screen.visible_text();
        assert!(
            all.contains("[un-passo-che-parla · out] secondo"),
            "what the step says afterwards must arrive too: {all:?}"
        );
        assert!(
            matches!(outcome, actions::RunOutcome::Finished { .. }),
            "it had to finish in time"
        );
    }

    #[test]
    fn inputs_become_root_inputs_without_being_changed() {
        let inputs = r#"{"root":{"command":"true","env":{},"timeout_secs":1}}"#;
        let json = flow_json("shell_check", "[]", inputs);
        let flow: FlowFile = serde_json::from_str(&json).expect("it loads");

        let request = registry::execution_request(None, &flow, "corsa-1", None, 0);

        assert_eq!(request.root_inputs, flow.inputs);
        assert_eq!(request.run_id, "corsa-1");
    }

    /// **WHAT THE FLOW DECLARES REACHES THE ACTION, PLUS THE ROOT.**
    ///
    /// Exact equality with the declared input no longer holds, and it is meant:
    /// whoever composes the input adds `workdir`, or a step with no declared
    /// directory would run where the process sits — fault 25. The test asks for
    /// two guarantees: what a person wrote is untouched, and the root is there.
    #[test]
    fn run_executes_the_registered_action_with_the_declared_input_plus_the_root() {
        let inputs = r#"{"root":{"command":"true","env":{},"timeout_secs":1}}"#;
        let json = flow_json("shell_check", "[]", inputs);
        let flow: FlowFile = serde_json::from_str(&json).expect("it loads");
        let store = InMemoryRecordStore::default();

        let execution = execute_flow(
            None,
            &flow,
            "corsa-1",
            0,
            &store,
            &registry_in(House::empty(), None, None),
            &Tick::new(0),
        )
        .expect("running the flow");

        assert_eq!(execution.decisions.last(), Some(&Decision::Complete));
        assert_eq!(store.all().len(), 1);
        let seen = &store.all()[0].input;
        for (field, value) in flow.inputs["root"].as_object().expect("an object") {
            assert_eq!(seen.get(field), Some(value), "«{field}» must not change");
        }
        assert_eq!(
            seen.get("workdir").and_then(Value::as_str),
            workspace_root()
                .as_deref()
                .map(|root| root.to_str().expect("a readable path")),
            "the working directory is the root, not where the process sits"
        );
    }
}
