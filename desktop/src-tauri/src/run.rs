//! Starting a flow from the window, and watching it run.
//!
//! **WHY THE SHELL EXECUTES INSTEAD OF LAUNCHING THE BINARY.** Measured:
//! `sailor flow run` gives one line, at the end — twelve seconds of silence
//! and then a verdict, which is the worst thing an execution view can be,
//! because it looks broken. And the binary takes no mandate: a delivery would
//! mean rewriting the flow on disk, so this morning's would stay in tomorrow's
//! file. Executing here costs a thread and gives both.
//!
//! **IT MIRRORS `flow_cmd::run_flow`, AND THE TWO ARE KEPT IN STEP.** The
//! ledger-then-registry order, the shape of a `run_id`, the two `record_run`
//! around the execution: the same forty lines, private to a binary and
//! unreachable from here. The duplication is declared rather than hidden.
//!
//! What streams is the **state** of the steps, at the instant the ledger makes
//! it durable. The **text** a step produces does not, and not by a choice made
//! here: `actions` reads stdout with `read_to_end` on a thread, and that buffer
//! is readable only at the join. Where it would change: `drain_and_wait`.

// `Decision` is gone: the decision-to-status translation moved into `registry`
// alongside its command-line twin, and the import stayed behind. Nobody saw it
// because this shell sits outside the workspace, so `cargo test --workspace`
// does not print its warnings.
use actions::{LiveSink, Pipe, StepSinks};
use flow::{
    ActionRegistry, Completion, Execution, Executor, FlowError, FlowFile, InProcessExecutor,
    Ran, RecordStore, Refusal, StepRecord, SystemClock,
};
use ledger::Ledger;
use serde::Serialize;
use serde_json::{json, Value};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};
use tauri::{AppHandle, Emitter, State};

use ui::gather::default_ledger_dir;

/// The channel on which the window receives what happens in a run.
pub const RUN_EVENT: &str = "sailor://run";

// ── what the window receives ────────────────────────────────────────────

/// One fact of the run, numbered.
///
/// **THE NUMBER IS NOT DECORATION.** Whoever opens the view asks first for what
/// already happened, then listens: between the two the run goes on, and an event
/// can arrive twice or fall in the gap. With `seq` monotonic per run a listener
/// drops what it already has — without it the view would show the same step
/// twice and have no way of noticing.
#[derive(Debug, Clone, Serialize)]
pub struct RunEvent {
    pub run_id: String,
    pub seq: u64,
    /// `step_started` | `step_text` | `step_closed` | `run_ended` | `note`
    pub kind: String,
    pub at: i64,
    pub step_id: Option<String>,
    pub payload: Value,
}

/// The state of a run as whoever looks in now sees it.
#[derive(Debug, Clone, Serialize)]
pub struct RunSnapshot {
    pub run_id: String,
    pub flow: String,
    pub started_at: i64,
    /// `running` while the thread works, then the engine's final status.
    pub status: String,
    pub events: Vec<RunEvent>,
}

/// What the button receives back when the run starts.
#[derive(Debug, Clone, Serialize)]
pub struct StartedRun {
    pub run_id: String,
    pub flow: String,
    pub started_at: i64,
}

/// Where the text of whoever presses the button ends up — or why there is no
/// place for it.
///
/// The answer comes **before** executing: a mandate with nowhere to go has to
/// be said while it is being written, not after the flow started ignoring it.
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum MandateTarget {
    /// The text goes into that field of that step's inputs.
    Field { step: String, field: String },
    /// Nowhere to put it, with the reason in plain words.
    None { why: String },
}

/// The name of the trigger action, and the field it carries the mandate in.
///
/// **READ FROM THE FLOW THE ENGINE WRITES, NOT DECIDED HERE.** The contract
/// lives in `flows/dispatch-the-work.flow.json`: a step with no dependencies,
/// `"action": "trigger"`, `"with": {"source": "manual"}`, and the mandate in
/// `inputs.<step>.text`. If those names change there they change here — and the
/// window says «no place for it» instead of writing into a field nobody reads,
/// which is the right way to be wrong.
const TRIGGER_ACTION: &str = "trigger";
const TRIGGER_FIELD: &str = "text";
const MANUAL_SOURCE: &str = "manual";

/// The text field of a step's action, when it has one.
///
/// An external engine takes the mandate on the stdin of the program it calls;
/// a trigger node carries it in its own text. A shell check takes a command,
/// not a mandate: slipping text written by a person into it would be a shell
/// injection, not a feature.
fn text_field_of(action: &str) -> Option<&'static str> {
    match action {
        TRIGGER_ACTION => Some(TRIGGER_FIELD),
        "external_engine" => Some("stdin"),
        _ => None,
    }
}

/// How a flow is triggered: where it starts, and whether it takes a mandate.
#[derive(Debug, Clone, Serialize)]
pub struct FlowTrigger {
    pub flow: String,
    /// The steps with no dependencies: the graph starts there.
    pub roots: Vec<String>,
    pub mandate: MandateTarget,
    /// True if the flow has a schedule of its own: then the button is not the
    /// only way it starts, and whoever watches has to know.
    pub scheduled: bool,
}

// ── the registry of runs lives in the shell ─────────────────────────────

/// A live run, with everything it has said so far.
struct RunState {
    flow: String,
    started_at: i64,
    status: String,
    events: Vec<RunEvent>,
    next_seq: u64,
    /// Somebody pressed stop: the executor reads it before the next front.
    halt: bool,
}

/// **RUNS DO NOT BELONG TO THE PAGE.** They live here, in the shell, for the
/// reason that makes them useful: closing the view panel, focusing another
/// flow or reloading the page must not stop work that is running, nor lose
/// what has already been said. The thread goes on, the events pile up in this
/// map, and whoever looks in again finds them all.
#[derive(Default)]
pub struct Runs(Mutex<HashMap<String, RunState>>);

impl Runs {
    /// Adds a fact to the run and sends it to the window, in that order.
    ///
    /// The registry is written first, then the announcement goes out: the
    /// other way round, whoever received the announcement and asked for the
    /// list at once might not find the fact just announced in it.
    fn publish(
        &self,
        app: &AppHandle,
        run_id: &str,
        kind: &str,
        step_id: Option<String>,
        payload: Value,
    ) {
        let event = {
            let mut runs = self.lock_map();
            let Some(state) = runs.get_mut(run_id) else {
                return;
            };
            let event = RunEvent {
                run_id: run_id.to_owned(),
                seq: state.next_seq,
                kind: kind.to_owned(),
                at: now_secs(),
                step_id,
                payload,
            };
            state.next_seq += 1;
            state.events.push(event.clone());
            event
        };
        // Outside the lock: `emit` crosses the bridge to the window, and
        // holding it while that happens would block the run's thread.
        let _ = app.emit(RUN_EVENT, &event);
        crate::events::emit(app, "run", &event);
    }

    /// A run that ended says so even when another run's thread died holding
    /// the registry: a status dropped there leaves the window showing a run
    /// still going for as long as it stays open.
    fn set_status(&self, run_id: &str, status: &str) {
        if let Some(state) = self.lock_map().get_mut(run_id) {
            state.status = status.to_owned();
        }
    }

    /// Marks the run to stop before its next front. Refused, with the reason,
    /// for a run this window does not hold or one that has already ended.
    pub(crate) fn request_halt(&self, run_id: &str) -> Result<(), String> {
        let mut runs = self.lock_map();
        let Some(state) = runs.get_mut(run_id) else {
            return Err(format!("no run {run_id} in this window"));
        };
        if state.status != "running" {
            return Err(format!("run {run_id} is not running: {}", state.status));
        }
        state.halt = true;
        Ok(())
    }

    pub(crate) fn halt_requested(&self, run_id: &str) -> bool {
        self.lock_map().get(run_id).is_some_and(|state| state.halt)
    }
}

// ── the text of a running step ──────────────────────────────────────────

/// One step's bytes on their way to the window.
///
/// A pipe breaks wherever it happens to break — sometimes halfway through a
/// character. The tail of an incomplete one is held until the rest arrives, so
/// an accented letter never reaches the window as a replacement mark; past four
/// bytes it is not a split character, and goes out as it is.
struct StepText {
    step: String,
    emit: Arc<dyn Fn(&str, Pipe, String) + Send + Sync>,
    /// One per pipe: stdout and stderr are drained by two threads, and a single
    /// buffer would splice one's tail onto the other's head.
    tails: Mutex<(Vec<u8>, Vec<u8>)>,
}

impl LiveSink for StepText {
    fn chunk(&self, pipe: Pipe, bytes: &[u8]) {
        let text = {
            let mut tails = crate::locks::locked(&self.tails);
            let buffer = match pipe {
                Pipe::Stdout => &mut tails.0,
                Pipe::Stderr => &mut tails.1,
            };
            buffer.extend_from_slice(bytes);
            take_whole_characters(buffer)
        };
        if !text.is_empty() {
            (self.emit)(&self.step, pipe, text);
        }
    }
}

/// The longest prefix of `buffer` that is whole text; the rest stays behind.
fn take_whole_characters(buffer: &mut Vec<u8>) -> String {
    let whole = match std::str::from_utf8(buffer) {
        Ok(_) => buffer.len(),
        Err(error) => error.valid_up_to(),
    };
    // Four bytes is the longest a character can be, so a longer tail is not one
    // waiting to be completed: holding it would silence the pipe for good.
    let cut = if buffer.len() - whole > 4 {
        buffer.len()
    } else {
        whole
    };
    let rest = buffer.split_off(cut);
    let text = String::from_utf8_lossy(buffer).into_owned();
    *buffer = rest;
    text
}

/// Hands each step somewhere to put what it says while it runs.
struct LiveText {
    emit: Arc<dyn Fn(&str, Pipe, String) + Send + Sync>,
}

impl StepSinks for LiveText {
    fn sink_for(&self, step: &str) -> Arc<dyn LiveSink> {
        Arc::new(StepText {
            step: step.to_owned(),
            emit: self.emit.clone(),
            tails: Mutex::new((Vec::new(), Vec::new())),
        })
    }
}

// ── the ledger, watched while it writes ─────────────────────────────────

/// The real ledger, with a witness beside it.
///
/// **WHY A DECORATOR AND NOT A POLL.** The other road was to ask the ledger
/// every so often: it would work — SQLite in WAL takes concurrent reads — but it
/// would add a delay picked at random and show a step «started» up to half a
/// second after it started. `RecordStore` has three methods: wrapping it costs
/// twenty lines and the event goes out the instant the fact becomes durable.
///
/// **The ledger first, the announcement after.** If the write fails nothing is
/// announced: a window showing a step the ledger never recorded tells of a run
/// that does not exist.
struct WatchedStore {
    inner: Ledger,
    app: AppHandle,
    runs: Arc<Runs>,
    run_id: String,
}

impl RecordStore for WatchedStore {
    fn append_started(&self, record: StepRecord) -> Result<(), FlowError> {
        let announced = record.clone();
        self.inner.append_started(record)?;
        self.runs.publish(
            &self.app,
            &self.run_id,
            "step_started",
            Some(announced.step_id.clone()),
            serde_json::to_value(&announced).unwrap_or(Value::Null),
        );
        Ok(())
    }

    fn close(
        &self,
        run_id: &str,
        step_id: &str,
        attempt: u32,
        epoch: u64,
        completion: Completion,
    ) -> Result<(), FlowError> {
        let announced = announced_close(step_id, attempt, epoch, &completion);
        self.inner
            .close(run_id, step_id, attempt, epoch, completion)?;
        self.runs.publish(
            &self.app,
            &self.run_id,
            "step_closed",
            Some(step_id.to_owned()),
            announced,
        );
        Ok(())
    }

    fn records(&self, run_id: &str) -> Result<Vec<StepRecord>, FlowError> {
        self.inner.records(run_id)
    }

    /// The ledger underneath knows the spend, and this decorator does not
    /// comment on it: **a shell answering zero would make the cap mute only in
    /// the window**, which is exactly where someone is watching a run start.
    fn spent(&self, run_id: &str) -> Result<flow::Spend, FlowError> {
        self.inner.spent(run_id)
    }

    /// The stop lives in the window's registry of runs, where the button
    /// wrote it: this is the only store that can carry it to the executor.
    fn halt_requested(&self, run_id: &str) -> Result<bool, FlowError> {
        Ok(self.runs.halt_requested(run_id))
    }
}

/// The `step_closed` fact as the window receives it. `Completion` does not
/// serialise, so the fields a watcher needs are picked by hand: `said` is the
/// raw text of the step, `refusal` the check that refused and what it saw,
/// `ran` the program and the arguments the step actually started.
fn announced_close(step_id: &str, attempt: u32, epoch: u64, completion: &Completion) -> Value {
    json!({
        "step_id": step_id,
        "attempt": attempt,
        "epoch": epoch,
        "outcome": format!("{:?}", completion.outcome),
        "output": completion.output,
        "said": completion.said,
        "failure_class": completion.failure_class,
        "refusal": completion.refusal,
        "ran": completion.ran,
        "ended_at": completion.ended_at,
        "bytes_seen": completion.bytes_seen,
        "bytes_discarded": completion.bytes_discarded,
    })
}

/// Resumes a run the ledger holds — one parked on a person and just closed —
/// through this window: the run joins the registry, every step reaches the
/// console, and Stop applies to it. The root is the one the window stands
/// in, and the answer says so, because the ledger keeps no root of a run's own.
pub(crate) fn resume(
    app: &AppHandle,
    runs: &Arc<Runs>,
    ledger: Ledger,
    flow: FlowFile,
    run_id: String,
) -> Result<String, String> {
    let header = ledger
        .run_header(&run_id)
        .map_err(|error| format!("cannot read run {run_id}: {error}"))?
        .ok_or_else(|| format!("no run {run_id} in the ledger"))?;
    let root = std::env::current_dir()
        .ok()
        .and_then(|working| flow::workspace::find_root(&working));
    {
        let mut known = runs.lock_map();
        if known.get(&run_id).is_some_and(|state| state.status == "running") {
            return Err(format!("run {run_id} is already running in this window"));
        }
        known.insert(
            run_id.clone(),
            RunState {
                flow: flow.id.clone(),
                started_at: header.started_at,
                status: "running".to_owned(),
                events: Vec::new(),
                next_seq: 0,
                halt: false,
            },
        );
    }
    let where_it_runs = root
        .as_ref()
        .map_or("no project root: steps that declare a workdir will fail".to_owned(), |root| {
            format!("in {}", root.display())
        });
    let handle = runs.clone();
    let app = app.clone();
    let id = run_id.clone();
    std::thread::spawn(move || {
        let mut store = WatchedStore {
            inner: ledger.clone(),
            app: app.clone(),
            runs: handle.clone(),
            run_id: id.clone(),
        };
        let outcome = sailor::flow_cmd::resume_run_with(&ledger, &flow, &id, &mut store, root.as_deref());
        // The status the resume recorded is the ledger's word for it; the
        // report, right or wrong, reaches the console as the run's last line.
        let status = ledger
            .run_header(&id)
            .ok()
            .flatten()
            .map_or("incomplete".to_owned(), |header| header.status);
        let (report, error) = match outcome {
            Ok(report) => (Some(report), None),
            Err(error) => (None, Some(error)),
        };
        handle.set_status(&id, &status);
        handle.publish(
            &app,
            &id,
            "run_ended",
            None,
            json!({ "status": status, "error": error, "report": report, "ended_at": now_secs() }),
        );
    });
    Ok(format!("run {run_id} is resuming {where_it_runs}; follow it in the console"))
}

/// Asks a run held by this window to stop before its next front. The step
/// running now finishes: the engine cannot take a step back from an agent
/// already at work, and the window says so instead of pretending.
#[tauri::command]
pub(crate) fn stop_run(
    app: AppHandle,
    runs: State<'_, Arc<Runs>>,
    run_id: String,
) -> Result<(), String> {
    runs.request_halt(&run_id)?;
    runs.publish(
        &app,
        &run_id,
        "stop_requested",
        None,
        json!({ "by": who(), "at": now_secs() }),
    );
    Ok(())
}

// ── the commands the window calls ───────────────────────────────────────

/// How this flow is triggered, and whether it takes a hand-written mandate.
#[tauri::command]
pub(crate) fn flow_trigger(name: String) -> Result<FlowTrigger, String> {
    let flow = load_flow(&name)?;
    Ok(trigger_of(&flow))
}

/// Starts a flow. Returns as soon as the run is under way, not when it ends.
#[tauri::command]
pub(crate) fn start_run(
    app: AppHandle,
    runs: State<'_, Arc<Runs>>,
    name: String,
    mandate: Option<String>,
) -> Result<StartedRun, String> {
    let origin = origin_label(mandate.as_deref());
    start(&app, runs.inner(), &name, mandate.as_deref(), origin)
}

/// One road for the button and for the beat: whoever starts a run comes
/// through here, and `origin` is what tells the two apart in the ledger.
pub(crate) fn start(
    app: &AppHandle,
    runs: &Arc<Runs>,
    name: &str,
    mandate: Option<&str>,
    origin: String,
) -> Result<StartedRun, String> {
    let flow = load_flow(name)?;
    // The same refusal as `flow_cmd::run_flow`, asked of the one place that
    // answers it: a flow requiring a guaranteed cap this machine cannot give
    // must not start from the button either.
    if let Some(why) = sailor::flow_cmd::check::why_a_run_here_would_not_start(&flow) {
        return Err(why);
    }

    // THE LEDGER BEFORE THE REGISTRY: `store_write` and `store_read` own it,
    // and a registry built first would declare two actions that exist to be
    // missing. It is the same note as `flow_cmd::run_flow`.
    let ledger_dir = default_ledger_dir();
    let ledger = Ledger::open(&ledger_dir).map_err(|error| {
        format!(
            "non riesco ad aprire il deposito {}: {error}",
            ledger_dir.display()
        )
    })?;
    // THE NAME BEFORE THE REGISTRY. The witness carries the run it belongs to,
    // so the run has to have a name before the registry that holds it is built.
    // Nothing is written yet: a name spent on a flow that turns out to name a
    // missing action costs nothing.
    let started_at = now_secs();
    let run_id = format!("{}-{}", flow.id, nanos());
    // What a step says while it runs reaches the window through here. The
    // ledger tells the window when a step opens and closes; this tells it what
    // the step is saying in between, which the ledger only learns at the end.
    let watcher: Arc<dyn StepSinks> = Arc::new(LiveText {
        emit: Arc::new({
            let app = app.clone();
            let runs = runs.clone();
            let run_id = run_id.clone();
            move |step: &str, pipe: Pipe, text: String| {
                runs.publish(
                    &app,
                    &run_id,
                    "step_text",
                    Some(step.to_owned()),
                    json!({ "pipe": pipe.name(), "text": text }),
                );
            }
        }),
    });
    let registry = default_registry(&ledger, Some(watcher));
    let missing: Vec<&str> = flow
        .graph
        .steps()
        .iter()
        .filter(|step| registry.get(&step.action).is_none())
        .map(|step| step.action.as_str())
        .collect();
    if !missing.is_empty() {
        return Err(format!(
            "il flusso {} nomina azioni che il motore non conosce: {}",
            flow.id,
            missing.join(", ")
        ));
    }

    // The mandate goes in here, in memory, and does not touch the file on
    // disk: the flow document describes the work, not the last time somebody
    // pressed the button.
    let inputs = inputs_with_mandate(&flow, mandate)?;

    // WHERE THE CALL CAME FROM, written by the system at the moment it starts.
    // It is not an agent's account: it is the shell declaring its own origin
    // before any step runs, and it stays in the append-only ledger even if the
    // run crashes on the first step.
    record_run(
        &ledger, &flow, &run_id, "running", started_at, None, None, &origin, None,
    )?;

    {
        let mut known = runs.lock_map();
        known.insert(
            run_id.clone(),
            RunState {
                flow: flow.id.clone(),
                started_at,
                status: "running".to_owned(),
                events: Vec::new(),
                next_seq: 0,
                halt: false,
            },
        );
    }

    let handle = runs.clone();
    let app = app.clone();
    let started = StartedRun {
        run_id: run_id.clone(),
        flow: flow.id.clone(),
        started_at,
    };

    // **WHOEVER LAUNCHES SAYS WHERE IT DECIDED TO WORK, BEFORE STARTING**, for
    // the button as much as for the terminal: without this line the plan has a
    // silent way of being wrong, the same as fault 25. Resolved here and not
    // inside the thread, so the line comes out before the run begins.
    let root = std::env::current_dir()
        .ok()
        .and_then(|working| flow::workspace::find_root(&working));
    match root.as_deref() {
        Some(root) => println!("radice del progetto: {}", root.display()),
        None => println!(
            "radice del progetto: nessuna (nessun {} risalendo da qui); \
             i passi che dichiarano «workdir» falliranno",
            flow::workspace::MARKER
        ),
    }

    // THE WORK DOES NOT SIT ON THE WINDOW'S THREAD. `execute` blocks and
    // reports nothing until it is done: leaving it on the thread that serves
    // the commands would freeze the interface for the whole run — half an
    // hour, on flows that call an agent.
    std::thread::spawn(move || {
        let mut store = WatchedStore {
            inner: ledger.clone(),
            app: app.clone(),
            runs: handle.clone(),
            run_id: run_id.clone(),
        };
        // **`registry` BUILDS THE REQUEST, NOT THIS FILE.** It was written here
        // too, and that is fault 10 in place: the two copies have drifted apart
        // three times already. With the project root in the middle the next
        // divergence would have been a run from the window working where the
        // process is while the same run from the terminal works in the right
        // root — and neither would say so. The input stays what the button
        // holds: the window can launch the same flow with a different mandate.
        let mut request =
            registry::execution_request(Some(&ledger), &flow, &run_id, root.as_deref(), started_at);
        request.root_inputs = inputs;
        let result = InProcessExecutor.execute(
            &flow.graph,
            request,
            &mut store,
            &registry,
            &SystemClock,
        );

        let ended_at = now_secs();
        let (status, error) = match &result {
            // A run stopped by the cap carries the numbers with it: what the
            // cap was, what came out as spent, and which steps never started.
            // Without them the window would hold one word and no reason.
            Ok(execution) => (
                execution_status(execution).to_owned(),
                registry::stopped_by_cap(execution)
                    .or_else(|| registry::halted_by_hand(execution)),
            ),
            Err(failure) => ("failed".to_owned(), Some(failure.to_string())),
        };
        let stop_reason = result.as_ref().ok().and_then(registry::how_it_stopped);
        let _ = record_run(
            &ledger,
            &flow,
            &run_id,
            &status,
            started_at,
            Some(ended_at),
            error.clone(),
            &origin,
            stop_reason,
        );
        handle.set_status(&run_id, &status);
        handle.publish(
            &app,
            &run_id,
            "run_ended",
            None,
            json!({ "status": status, "error": error, "ended_at": ended_at }),
        );
    });

    Ok(started)
}

/// Everything a run has said so far, for whoever looks in now.
#[tauri::command]
pub(crate) fn run_snapshot(
    runs: State<'_, Arc<Runs>>,
    run_id: String,
) -> Result<RunSnapshot, String> {
    let known = runs.lock_map();
    let state = known
        .get(&run_id)
        .ok_or_else(|| format!("run {run_id} is not known to this window"))?;
    Ok(RunSnapshot {
        run_id: run_id.clone(),
        flow: state.flow.clone(),
        started_at: state.started_at,
        status: state.status.clone(),
        events: state.events.clone(),
    })
}

/// In what way a run is open.
///
/// **TWO WAYS, NOT ONE, AND THE LEDGER KEEPS THEM IN TWO PLACES.** A run at
/// work has a step **with no outcome**; a run handed to a person has the step
/// **closed** with outcome `Waiting`, because whoever must run it is not a
/// process whose death is awaited. Asking only one of the two questions makes
/// the other half vanish — the fault `waiting_runs` documents: a mandate
/// nobody picked up disappeared, and the only way to find it was to remember.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum OpenState {
    /// Somebody or something is working on it right now.
    Working,
    /// It is stopped, and starts again only if a person does something.
    Waiting,
}

/// A step that is open right now, and for how long.
///
/// **«THREE OPEN STEPS» IS NOT AN ANSWER.** The real question is *which*, and
/// for how long: a step open for six minutes is working, the same step open
/// for three hours is hung. The model is Temporal's «Pending Activities»
/// section — activity type, current attempt, attempts left, heartbeat — built
/// because the event history alone is not enough: `ActivityTaskStarted` does
/// not appear until the activity has finished or run out of attempts. None of
/// the agent tools compared has an equivalent.
#[derive(Debug, Clone, Serialize)]
pub(crate) struct OpenStep {
    pub step_id: String,
    /// Which attempt: `2` on an open step means the first one fell.
    pub attempt: u32,
    pub open_for_secs: i64,
}

/// An open run, whoever started it.
#[derive(Debug, Clone, Serialize)]
pub(crate) struct OpenRun {
    pub run_id: String,
    /// What it worked on: the flow, or the thing the run names.
    pub entity: String,
    /// At work, or stopped waiting for a person.
    pub state: OpenState,
    /// How many steps still have an open outcome. Zero for those waiting.
    pub open_steps: usize,
    /// **Which** steps, and for how long. Empty for those waiting.
    pub open_now: Vec<OpenStep>,
    /// Since when this state has lasted: the opening of the oldest step for
    /// those at work, the start of the wait for those waiting.
    pub since: i64,
    /// True if this window is the one that started it.
    pub started_here: bool,
    /// Steps with an outcome already, counted once each.
    pub steps_done: usize,
    /// Steps the flow declares; `None` when the flow cannot be read back.
    pub steps_total: Option<usize>,
}

/// «4 of 7»: how far the run is, from the ledger and the flow it names.
///
/// The flow is read back through the same door `step close` uses; a run whose
/// flow cannot be found still shows how many steps it has done, so the count
/// never invents a total.
fn progress_of(ledger: &Ledger, run_id: &str) -> (usize, Option<usize>) {
    let done = ledger
        .steps(run_id)
        .map(|steps| {
            steps
                .iter()
                .filter(|step| step.outcome.is_some())
                .map(|step| step.step_id.clone())
                .collect::<std::collections::HashSet<String>>()
                .len()
        })
        .unwrap_or(0);
    let total = sailor::step_cmd::flow_of_run(ledger, run_id)
        .ok()
        .map(|file| file.graph.steps().len());
    (done, total)
}

/// **All** the runs with at least one open step, not only ours.
///
/// **WHY `known_runs` WAS NOT ENOUGH.** It reads a map in this process's memory:
/// a run launched from the terminal, from another window or from a daemon does
/// not appear, and Sailor's first screen — which must say «what is happening
/// now» — would lie to anyone working in more than one place. It is the standing
/// constraint «clarity for whoever watches»: an interface showing only its own
/// corner is worse than one showing nothing, because it looks complete.
///
/// **THE LEDGER IS THE ORACLE, AND MEMORY IS ONLY A LABEL.** The list comes from
/// two questions to the ledger — `unfinished_runs` for those at work,
/// `waiting_runs` for those waiting on a person; what this process knows on top
/// only says *which* are its own, because a run started here can be followed
/// live and one started elsewhere cannot. With no ledger the list is empty: not
/// an error, a machine nothing has run on yet.
///
/// **WHOEVER WAITS WINS OVER WHOEVER WORKS** when a run would appear in both
/// answers. Not an aesthetic preference: of the two states only one asks
/// something of the watcher, and showing it as «at work» would leave them
/// waiting forever on a process that will not return.
#[tauri::command]
pub(crate) fn open_runs(runs: State<'_, Arc<Runs>>) -> Result<Vec<OpenRun>, String> {
    let ledger_dir = default_ledger_dir();
    if !ledger_dir.exists() {
        return Ok(Vec::new());
    }
    let ledger = Ledger::open(&ledger_dir)
        .map_err(|error| format!("cannot open the ledger: {error}"))?;
    let unfinished = ledger
        .unfinished_runs()
        .map_err(|error| format!("cannot read the open runs: {error}"))?;
    let waiting = ledger
        .waiting_runs()
        .map_err(|error| format!("cannot read the waiting runs: {error}"))?;
    let known = runs.lock_map();

    let now = now_secs();
    let mut all: Vec<OpenRun> = waiting
        .into_iter()
        .map(|run| {
            let (steps_done, steps_total) = progress_of(&ledger, &run.run_id);
            OpenRun {
                started_here: known.contains_key(&run.run_id),
                run_id: run.run_id,
                entity: run.entity,
                state: OpenState::Waiting,
                open_steps: 0,
                open_now: Vec::new(),
                since: run.waiting_since,
                steps_done,
                steps_total,
            }
        })
        .collect();
    let held: std::collections::HashSet<String> =
        all.iter().map(|run| run.run_id.clone()).collect();
    for run in unfinished
        .into_iter()
        .filter(|run| !held.contains(&run.run_id))
    {
        // ONE MORE QUESTION PER OPEN RUN, and they are few by construction:
        // only what is in flight now lands here, not the history. If one day
        // they were many, the place to repair is the ledger, with a single
        // query that brings the steps too — not here, by dropping the detail,
        // because the detail is the answer.
        let open_now = ledger
            .steps(&run.run_id)
            .map(|steps| {
                steps
                    .into_iter()
                    .filter(|step| step.outcome.is_none())
                    .map(|step| OpenStep {
                        step_id: step.step_id,
                        attempt: step.attempt,
                        open_for_secs: now - step.started_at,
                    })
                    .collect()
            })
            // A step that cannot be read does not make the run vanish: the
            // count stays, the detail is missing, and the row shows all the
            // same. Losing the row would be the big damage.
            .unwrap_or_default();
        let (steps_done, steps_total) = progress_of(&ledger, &run.run_id);
        all.push(OpenRun {
            started_here: known.contains_key(&run.run_id),
            run_id: run.run_id,
            entity: run.entity,
            state: OpenState::Working,
            open_steps: run.open_steps,
            open_now,
            since: run.oldest_started_at,
            steps_done,
            steps_total,
        });
    }

    // The oldest on top: a watcher looks first for what has been stuck the
    // longest, not for what has just started. The order here is on time alone
    // — grouping by state is a choice for whoever draws, and this command
    // serves those who draw nothing too.
    all.sort_by_key(|run| run.since);
    Ok(all)
}

/// The runs this window started, the most recent last.
///
/// It serves whoever reloads the page while a flow runs: without this list the
/// view would start again empty and the run would go on with nobody watching
/// it.
#[tauri::command]
pub(crate) fn known_runs(runs: State<'_, Arc<Runs>>) -> Vec<RunSnapshot> {
    let known = runs.lock_map();
    let mut all: Vec<RunSnapshot> = known
        .iter()
        .map(|(run_id, state)| RunSnapshot {
            run_id: run_id.clone(),
            flow: state.flow.clone(),
            started_at: state.started_at,
            status: state.status.clone(),
            events: state.events.clone(),
        })
        .collect();
    all.sort_by_key(|snapshot| snapshot.started_at);
    all
}

impl Runs {
    fn lock_map(&self) -> std::sync::MutexGuard<'_, HashMap<String, RunState>> {
        crate::locks::locked(&self.0)
    }

    /// The flows this window is running right now, by flow id.
    pub(crate) fn running_flows(&self) -> Vec<String> {
        self.lock_map()
            .values()
            .filter(|state| state.status == "running")
            .map(|state| state.flow.clone())
            .collect()
    }
}

// ── what went into a node, over time ────────────────────────────────────

/// One time a step was crossed.
///
/// **IT ALL COMES FROM THE LEDGER, NOTHING FROM THE WINDOW'S MEMORY.** The runs
/// this window started are a handful; the ones that node saw go by can be
/// hundreds, started from the command line, from a schedule or from a window
/// closed months ago. Reading from its own memory would give a history that
/// begins when the program opened — a history that looks complete and is not.
#[derive(Debug, Clone, Serialize)]
pub struct StepPassage {
    pub run_id: String,
    pub attempt: u32,
    pub started_at: i64,
    pub ended_at: Option<i64>,
    pub outcome: Option<String>,
    pub failure_class: Option<String>,
    /// Which check refused, where, by which rule and what it saw: the
    /// structure beside the class, so the window need not parse `said`.
    pub refusal: Option<Refusal>,
    /// The program and the arguments the step started, after resolution: what
    /// a person would have to type to reach the same outcome by hand.
    pub ran: Option<Ran>,
    /// Where the run started from: the origin, written by the system.
    pub started_by: String,
    /// What went into **this** node, that time.
    pub input: Value,
    /// The mandate the run started with, if it carried one.
    pub mandate: Option<String>,
    /// Who sent the signal, as the source knew it. Empty when it did not know:
    /// that is a fact, not a field to be filled.
    pub signal_who: Option<String>,
    /// Where the signal came from: the window, a panel, a session.
    pub signal_where: Option<String>,
    pub said: Option<String>,
    pub output: Option<Value>,
}

/// Everything that went through a node, most recent first.
///
/// What a run cost: tokens, cache and money, from the ledger.
///
/// **IT REDOES NO ARITHMETIC.** `ui::dashboard` computes the totals: already
/// pure, already tested, the same code that serves the `sailor ui` page. Two
/// views of the same spend adding up on their own would give two figures, and
/// «which of the two is right» would have no answer.
///
/// **A PARTIAL TOTAL DECLARES ITSELF.** `TokenTotals::is_partial` says whether
/// a call did not report its counts or has no price: whoever shows these
/// numbers must show that too, or presents a sum that hides what it does not
/// know. With no ledger it returns `None`, not an error: a program that has
/// executed nothing yet is not broken.
///
/// **READ ON DEMAND, WHEN A RUN ENDS OR WHEN SOMEBODY LOOKS.** Opening the
/// ledger and walking the runs is not work for every heartbeat.
#[tauri::command]
pub(crate) fn run_usage(run_id: String) -> Result<Option<ui::dashboard::ExecutionView>, String> {
    let ledger_dir = default_ledger_dir();
    let Some(data) = ui::gather::gather(&ledger_dir)
        .map_err(|error| format!("cannot read the ledger: {error}"))?
    else {
        return Ok(None);
    };
    let Some(run) = data.runs.iter().find(|run| run.run_id == run_id) else {
        // A run just started may not be in the projection yet: that is not an
        // error, it is «not yet», and the window tries again on the next beat.
        return Ok(None);
    };
    let steps = data.steps_by_run.get(&run_id).cloned().unwrap_or_default();
    let calls = data.calls_by_run.get(&run_id).cloned().unwrap_or_default();
    Ok(Some(ui::dashboard::summarize_run(
        run,
        &steps,
        &calls,
        now_secs(),
    )))
}

/// **READ ON DEMAND, NOT CONTINUOUSLY.** Rebuilding this history means opening
/// the ledger and walking the runs: the kind of work done when somebody clicks
/// a node, not on every beat of a run that is going.
#[tauri::command]
pub(crate) fn step_history(
    flow: String,
    step: String,
    limit: Option<usize>,
) -> Result<Vec<StepPassage>, String> {
    let ledger_dir = default_ledger_dir();
    let Some(data) = ui::gather::gather(&ledger_dir)
        .map_err(|error| format!("cannot read the ledger: {error}"))?
    else {
        // No ledger is not a fault: it is a program that has executed nothing
        // yet, and saying so as an error would send someone hunting a fault
        // that is not there.
        return Ok(Vec::new());
    };

    let mut passages = Vec::new();
    for run in &data.runs {
        if run.entity != flow {
            continue;
        }
        let Some(steps) = data.steps_by_run.get(&run.run_id) else {
            continue;
        };
        let signal = signal_of_run(steps);
        for record in steps {
            if record.step_id != step {
                continue;
            }
            passages.push(passage_of(record, &run.started_by, &signal));
        }
    }

    // Most recent first: whoever opens this list almost always wants the last.
    passages.sort_by(|a, b| {
        b.started_at
            .cmp(&a.started_at)
            .then(b.attempt.cmp(&a.attempt))
    });
    passages.truncate(limit.unwrap_or(25));
    Ok(passages)
}

/// One record of the ledger, as the history panel reads it.
fn passage_of(record: &StepRecord, started_by: &str, signal: &RunSignal) -> StepPassage {
    StepPassage {
        run_id: record.run_id.clone(),
        attempt: record.attempt,
        started_at: record.started_at,
        ended_at: record.ended_at,
        outcome: record.outcome.map(|outcome| format!("{outcome:?}")),
        failure_class: record.failure_class.clone(),
        refusal: record.refusal.clone(),
        ran: record.ran.clone(),
        started_by: started_by.to_owned(),
        input: record.input.clone(),
        mandate: signal.text.clone(),
        signal_who: signal.who.clone(),
        signal_where: signal.where_from.clone(),
        said: record.said.clone(),
        output: record.output.clone(),
    }
}

/// What the signal a run started with carried.
///
/// The ledger records each step's input the moment it opens, so **the signal is
/// already written by the system** and does not depend on a model deciding to
/// tell it. Nothing new is recorded here: what was there is read back.
///
/// `who` and `where` are the fields a trigger node carries. An empty field
/// stays `None` and not an empty string: the window must be able to keep quiet
/// about what the signal did not know, instead of showing a label with no value
/// beside it.
#[derive(Debug, Default, Clone)]
struct RunSignal {
    text: Option<String>,
    who: Option<String>,
    where_from: Option<String>,
}

fn signal_of_run(steps: &[StepRecord]) -> RunSignal {
    let Some(root) = steps.iter().find(|record| record.deps.is_empty()) else {
        return RunSignal::default();
    };
    let Value::Object(input) = &root.input else {
        return RunSignal::default();
    };

    fn text_at(input: &serde_json::Map<String, Value>, key: &str) -> Option<String> {
        match input.get(key) {
            Some(Value::String(text)) if !text.is_empty() => Some(text.clone()),
            _ => None,
        }
    }

    RunSignal {
        // The two fields a mandate can have gone into, in the same order
        // `text_field_of` picks them.
        text: text_at(input, TRIGGER_FIELD).or_else(|| text_at(input, "stdin")),
        who: text_at(input, "who"),
        where_from: text_at(input, "where"),
    }
}

// ── the rules, kept out of the commands to test them ────────────────────

/// The steps the graph begins from: the ones that wait on nobody.
fn roots_of(flow: &FlowFile) -> Vec<String> {
    flow.graph
        .steps()
        .iter()
        .filter(|step| step.deps.is_empty())
        .map(|step| step.id.clone())
        .collect()
}

/// Where the text of whoever presses the button ends up.
///
/// **IT REFUSES INSTEAD OF GUESSING.** A mandate put in the wrong place gives
/// no error: the flow starts and works on yesterday's mandate, and whoever
/// pressed believes they gave their own. The cases with no certain place return
/// the reason, which the window shows beside the field before anyone types.
fn mandate_target(flow: &FlowFile) -> MandateTarget {
    let roots = roots_of(flow);
    let [root] = roots.as_slice() else {
        return MandateTarget::None {
            why: if roots.is_empty() {
                "the flow has no starting step: every step waits on another".to_owned()
            } else {
                format!(
                    "the flow starts from {} steps ({}), and one mandate does not say which it goes to",
                    roots.len(),
                    roots.join(", ")
                )
            },
        };
    };

    let Some(step) = flow.graph.steps().iter().find(|step| &step.id == root) else {
        return MandateTarget::None {
            why: "the starting step is not in the graph".to_owned(),
        };
    };

    let Some(field) = text_field_of(&step.action) else {
        return MandateTarget::None {
            why: format!(
                "the starting step «{root}» runs «{}», which has no text input",
                step.action
            ),
        };
    };

    // A trigger declares where the signal comes from. If it is not a person's
    // gesture, a button promising to hand it one would promise something that
    // step is not waiting for from here.
    if step.action == TRIGGER_ACTION {
        if let Some(Value::Object(fixed)) = &step.with {
            match fixed.get("source") {
                Some(Value::String(source)) if source == MANUAL_SOURCE => {}
                Some(Value::String(source)) => {
                    return MandateTarget::None {
                        why: format!(
                            "the trigger «{root}» waits for a signal of kind «{source}», not a person's gesture"
                        ),
                    };
                }
                _ => {}
            }
        }
    }

    // `with` wins over the keys received as input: if it already declares that
    // field, the mandate would be written and then overridden with no error.
    // That is the worst way to lose a text, so it is refused up front.
    if let Some(Value::Object(fixed)) = &step.with {
        if fixed.contains_key(field) {
            return MandateTarget::None {
                why: format!(
                    "il passo «{root}» dichiara già il proprio «{field}» nei parametri fissi: \
                     una consegna scritta qui verrebbe scavalcata senza dirlo"
                ),
            };
        }
    }

    MandateTarget::Field {
        step: root.clone(),
        field: field.to_owned(),
    }
}

fn trigger_of(flow: &FlowFile) -> FlowTrigger {
    FlowTrigger {
        flow: flow.id.clone(),
        roots: roots_of(flow),
        mandate: mandate_target(flow),
        scheduled: flow.schedule.is_some(),
    }
}

/// The run's inputs, with the mandate inside if there is one.
///
/// A mandate with nowhere to go **stops the start**: executing while ignoring
/// it would give a run that looks like it received the text and worked on
/// something else.
fn inputs_with_mandate(
    flow: &FlowFile,
    mandate: Option<&str>,
) -> Result<std::collections::BTreeMap<String, Value>, String> {
    let mut inputs = flow.inputs.clone();
    let Some(text) = mandate.filter(|text| !text.trim().is_empty()) else {
        return Ok(inputs);
    };

    match mandate_target(flow) {
        MandateTarget::Field { step, field } => {
            let entry = inputs.entry(step).or_insert_with(|| json!({}));
            match entry {
                Value::Object(map) => {
                    let is_trigger = field == TRIGGER_FIELD;
                    map.insert(field, Value::String(text.to_owned()));
                    // WHO AND FROM WHERE, as this source knows it. A trigger
                    // records `who` and `where` beside the text, and filling
                    // them here makes the signal traceable months later:
                    // without it, *what* arrived is written and not from whom.
                    //
                    // Only for a trigger node: an external engine has no such
                    // fields, and writing them in would invent a parameter
                    // its action does not read.
                    if is_trigger {
                        map.entry("who".to_owned()).or_insert(Value::String(who()));
                        map.entry("where".to_owned())
                            .or_insert(Value::String(WHERE.to_owned()));
                    }
                    Ok(inputs)
                }
                _ => Err(
                    "gli ingressi del passo di partenza non sono un oggetto: non c'è dove \
                     scrivere la consegna"
                        .to_owned(),
                ),
            }
        }
        MandateTarget::None { why } => {
            Err(format!("this flow takes no mandate: {why}"))
        }
    }
}

/// How the run ended. **No longer a copy**: it was written here too, the same
/// as `flow_cmd` by good will; now it is the same by construction. The boolean
/// serves only a process's exit code, and no process exits here.
fn execution_status(execution: &Execution) -> &'static str {
    registry::execution_status(execution).0
}

/// The actions the engine can run: **the same list as the terminal's**.
///
/// **THIS LIST USED TO DRIFT, AND HAD DONE SO THREE TIMES.** A hand copy of the
/// `sailor flow run` list lived here, kept in step by good will. The `trigger`
/// crate was registered there and not here: the button answered «unknown
/// action: trigger» on a flow the terminal ran. It happened again with the
/// usage measure, in silence — the window built a registry **with no tool
/// resolver** (every step naming `claude-code` instead of a path fell with
/// `no_tool_resolver`) and **with no ledger** (no cost row for runs launched
/// here). Now the list is one and lives in `crates/registry`.
fn default_registry(
    ledger: &Ledger,
    watcher: Option<Arc<dyn actions::StepSinks>>,
) -> ActionRegistry {
    registry::default_registry(Some(ledger.clone()), watcher)
}

/// Where the signal this shell sends comes from. It is the window, always: not
/// a value to guess, it is what this program is.
const WHERE: &str = "the Sailor window";

/// Who pressed, **as this source knows it**, which is little: the window has no
/// person's identity — no login, no account — and the only true thing to hand is
/// the user the program runs as.
///
/// **IT RETURNS EMPTY RATHER THAN INVENT.** `Signal` declares that a signal that
/// does not know who sent it says so with an empty string; a plausible but
/// unverified name written there would be worse than admitting it is unknown,
/// because nobody would go and check any more.
pub(crate) fn who() -> String {
    std::env::var("USER").unwrap_or_default()
}

/// Where a run started from, in a line that reads without being decoded.
///
/// **IT CARRIES THE ORIGIN, NOT THE CONTENT.** It says somebody pressed in this
/// window and whether they attached a mandate; the mandate's text does not come
/// in here. The text is already recorded where it belongs — in the inputs of
/// the step that receives it — and copying it into the run's label too would
/// mean writing it twice in places with different rules.
fn origin_label(mandate: Option<&str>) -> String {
    let carried = mandate.is_some_and(|text| !text.trim().is_empty());
    if carried {
        "finestra · innesco manuale, con consegna".to_owned()
    } else {
        "finestra · innesco manuale".to_owned()
    }
}

/// Records the run's header.
///
/// **THIS COPY NO LONGER EXISTS, AND THAT IS THE POINT.** The same twenty lines
/// of `flow_cmd` lived here. There is now a crate the two roads share: both
/// used to write the zero total by hand, and repairing only one would have
/// given two different figures for the same run depending on which button was
/// pressed.
#[allow(clippy::too_many_arguments)]
fn record_run(
    ledger: &Ledger,
    flow: &FlowFile,
    run_id: &str,
    status: &str,
    started_at: i64,
    ended_at: Option<i64>,
    error: Option<String>,
    started_by: &str,
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
            started_by,
            stop_reason,
        },
    )
}

/// The flow with that name, looked up **where the canvas found it**.
///
/// **THE DEFECT THIS FUNCTION HAD.** The name used to become a path inside
/// `default_flows_dir()` — `~/.config/sailor/flows`, one directory — while the
/// list the window draws comes from three sources: shipped inside the binary,
/// home, and the project. On this machine the seven existing flows are in the
/// other two, and that directory does not even exist. Result: `flow_trigger`
/// failed on every one, every trigger stayed `mute`, and **the ▶ Run button was
/// grey on every node**.
///
/// **AND THE NAME NO LONGER BECOMES A PATH.** Looked up in a list already
/// built, a name that list does not hold opens nothing: there is nowhere to
/// escape from, and the check that was needed before — `safe_name` — goes with
/// the reason that kept it alive. It is the same choice already argued in
/// `flow_cmd::known_flows`, and the two roads now share it.
fn load_flow(name: &str) -> Result<FlowFile, String> {
    let known = ui::gather::load_all_flows(&ui::gather::flow_sources());
    match known.iter().find(|(known, _, _)| known == name) {
        Some((_, _, Ok(flow))) => Ok(flow.clone()),
        Some((_, origin, Err(reason))) => Err(format!(
            "flow «{name}» ({origin}) does not load: {reason}"
        )),
        None => {
            let names: Vec<&str> = known.iter().map(|(name, _, _)| name.as_str()).collect();
            Err(format!(
                "no flow is called «{name}»; the ones I see are: {}",
                if names.is_empty() {
                    "nessuno".to_owned()
                } else {
                    names.join(", ")
                }
            ))
        }
    }
}

fn now_secs() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|elapsed| elapsed.as_secs() as i64)
        .unwrap_or(0)
}

fn nanos() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|elapsed| elapsed.as_nanos())
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn flow_with(steps: Value, inputs: Value) -> FlowFile {
        serde_json::from_value(json!({
            "id": "prova",
            "description": "",
            "graph": { "steps": steps },
            "inputs": inputs,
        }))
        .expect("il flusso di prova si carica")
    }

    /// A run as the registry holds it while it runs.
    fn held_run(runs: &Runs, run_id: &str, status: &str) {
        runs.lock_map().insert(
            run_id.to_owned(),
            RunState {
                flow: "prova".to_owned(),
                started_at: 0,
                status: status.to_owned(),
                events: Vec::new(),
                next_seq: 0,
                halt: false,
            },
        );
    }

    /// **STOP IS A FACT THE EXECUTOR READS, AND ONLY A RUNNING RUN CAN CARRY
    /// IT.** An unknown run and an ended one are refused with the reason, so
    /// a button pressed late does not report a stop nobody will honour.
    #[test]
    fn a_stop_is_held_for_a_running_run_and_refused_otherwise() {
        let runs = Runs::default();
        held_run(&runs, "live", "running");
        held_run(&runs, "done", "complete");

        assert!(!runs.halt_requested("live"));
        runs.request_halt("live").expect("a running run takes the stop");
        assert!(runs.halt_requested("live"));

        let ended = runs.request_halt("done").expect_err("an ended run refuses");
        assert!(ended.contains("is not running: complete"), "{ended}");
        assert!(!runs.halt_requested("done"));

        let unknown = runs.request_halt("nobody").expect_err("an unknown run refuses");
        assert!(unknown.contains("no run nobody"), "{unknown}");
    }

    /// A run that ends is marked even when another thread died holding the
    /// registry. Dropped, the window shows that run going for as long as it
    /// stays open, and nothing anywhere says why.
    #[test]
    fn a_run_that_ends_is_marked_after_a_thread_died_holding_the_registry() {
        let runs = Arc::new(Runs::default());
        held_run(&runs, "live", "running");

        let taken = runs.clone();
        let died = std::thread::spawn(move || {
            let _held = taken.lock_map();
            panic!("a thread dies holding the registry");
        });
        assert!(died.join().is_err(), "the thread had to die holding it");

        runs.set_status("live", "complete");
        assert_eq!(runs.lock_map()["live"].status, "complete");
    }

    fn refused_by_shape() -> Refusal {
        Refusal::new("answer_shape", "$.verdict", flow::RefusalRule::NotAllowed, "\"remvoe\"")
    }

    /// The window shows which rule refused and at which path: the check
    /// travels as structure in the closing fact, beside the class it explains.
    #[test]
    fn a_closing_fact_carries_the_refusal_as_structure() {
        let completion = Completion {
            outcome: flow::Outcome::Broke,
            output: None,
            said: Some("off shape".to_owned()),
            failure_class: Some("answer_off_shape".to_owned()),
            refusal: Some(refused_by_shape()),
            ran: None,
            ended_at: 7,
            bytes_seen: None,
            bytes_discarded: None,
        };
        let announced = announced_close("verdict", 1, 0, &completion);
        assert_eq!(announced["refusal"]["check"], "answer_shape");
        assert_eq!(announced["refusal"]["path"], "$.verdict");
        assert_eq!(announced["refusal"]["rule"], "not_allowed");
        assert_eq!(announced["refusal"]["seen"], "\"remvoe\"");

        let plain = Completion { refusal: None, ..completion };
        assert!(announced_close("verdict", 1, 0, &plain)["refusal"].is_null());
    }

    fn ran_a_shell_line() -> Ran {
        Ran::new("sh", ["-c", "echo hi"])
    }

    /// The line a step started travels in the closing fact, so whoever is
    /// watching sees the command instead of guessing it from the outcome.
    #[test]
    fn a_closing_fact_carries_the_line_the_step_ran() {
        let completion = Completion {
            outcome: flow::Outcome::Went,
            output: None,
            said: None,
            failure_class: None,
            refusal: None,
            ran: Some(ran_a_shell_line()),
            ended_at: 7,
            bytes_seen: None,
            bytes_discarded: None,
        };
        let announced = announced_close("verdict", 1, 0, &completion);
        assert_eq!(announced["ran"]["program"], "sh");
        assert_eq!(announced["ran"]["args"][0], "-c");
        assert_eq!(announced["ran"]["args"][1], "echo hi");

        let quiet = Completion { ran: None, ..completion };
        assert!(announced_close("verdict", 1, 0, &quiet)["ran"].is_null());
    }

    /// The same structure reaches the history of a step, read back from the
    /// ledger, so an old refusal is as legible as the one just made.
    #[test]
    fn a_passage_carries_the_refusal_of_its_record() {
        let mut record =
            StepRecord::started("r1", "verdict", 1, 0, Vec::new(), json!({}), Vec::new(), 1);
        record.refusal = Some(refused_by_shape());
        let passage = passage_of(&record, "window", &RunSignal::default());
        assert_eq!(passage.refusal, Some(refused_by_shape()));

        record.refusal = None;
        assert_eq!(passage_of(&record, "window", &RunSignal::default()).refusal, None);
    }

    /// The same line reaches the history of a step, read back from the ledger,
    /// so a run from months ago says what it started as plainly as this one.
    #[test]
    fn a_passage_carries_the_line_its_record_ran() {
        let mut record =
            StepRecord::started("r1", "verdict", 1, 0, Vec::new(), json!({}), Vec::new(), 1);
        record.ran = Some(ran_a_shell_line());
        let passage = passage_of(&record, "window", &RunSignal::default());
        assert_eq!(passage.ran, Some(ran_a_shell_line()));

        record.ran = None;
        assert_eq!(passage_of(&record, "window", &RunSignal::default()).ran, None);
    }

    fn engine_step(id: &str, deps: Vec<&str>, with: Value) -> Value {
        json!({
            "id": id,
            "deps": deps,
            "action": "external_engine",
            "max_attempts": 1,
            "when": null,
            "with": with,
            "input_schema": { "type": "any" },
            "output_schema": { "type": "any" }
        })
    }

    /// Collects what a step says, the way the window would receive it.
    fn heard(step: &str) -> (Arc<dyn LiveSink>, Arc<Mutex<Vec<(String, String)>>>) {
        let said: Arc<Mutex<Vec<(String, String)>>> = Arc::new(Mutex::new(Vec::new()));
        let sinks = LiveText {
            emit: Arc::new({
                let said = said.clone();
                move |_step: &str, pipe: Pipe, text: String| {
                    said.lock()
                        .expect("not poisoned")
                        .push((pipe.name().to_owned(), text));
                }
            }),
        };
        (sinks.sink_for(step), said)
    }

    /// THE MEASURE THAT COULD HAVE COME OUT DIFFERENTLY. A pipe breaks where it
    /// breaks, and half of this project's output is accented: decoding each
    /// chunk on its own turns every letter unlucky enough to straddle a break
    /// into a replacement mark. Emitting the tail regardless makes this red.
    #[test]
    fn a_character_split_across_two_chunks_arrives_whole() {
        let (sink, said) = heard("engine");
        let text = "perché".as_bytes();
        let split = text.len() - 1;
        sink.chunk(Pipe::Stdout, &text[..split]);
        sink.chunk(Pipe::Stdout, &text[split..]);

        let said = said.lock().expect("not poisoned");
        let whole: String = said.iter().map(|(_, text)| text.as_str()).collect();
        assert_eq!(whole, "perché");
        assert!(
            !whole.contains('\u{fffd}'),
            "a letter reached the window broken"
        );
    }

    /// Two threads drain the two pipes: one buffer between them would splice
    /// the tail of one onto the head of the other.
    #[test]
    fn the_two_pipes_do_not_splice_into_each_other() {
        let (sink, said) = heard("engine");
        let out = "è".as_bytes();
        sink.chunk(Pipe::Stdout, &out[..1]);
        sink.chunk(Pipe::Stderr, b"broke\n");
        sink.chunk(Pipe::Stdout, &out[1..]);

        let said = said.lock().expect("not poisoned");
        assert_eq!(
            said.as_slice(),
            [
                ("err".to_owned(), "broke\n".to_owned()),
                ("out".to_owned(), "è".to_owned())
            ]
        );
    }

    /// A tail that will never be completed must not silence the pipe for good.
    #[test]
    fn bytes_that_are_not_a_split_character_stop_being_waited_for() {
        let (sink, said) = heard("engine");
        sink.chunk(Pipe::Stdout, &[0xff, 0xfe, 0xfd, 0xfc, 0xfb]);

        let said = said.lock().expect("not poisoned");
        assert_eq!(
            said.len(),
            1,
            "five bytes that no character starts were held back"
        );
    }

    /// THE MEASURE THAT COULD HAVE COME OUT DIFFERENTLY. While the shell hands
    /// the engine no witness, a running step says nothing until it closes, and
    /// nothing in the window is red about it: the panel simply stays empty.
    /// This reads the line that decides it.
    #[test]
    fn the_shell_hands_the_engine_somewhere_to_put_a_running_step_s_text() {
        const SOURCE: &str = include_str!("run.rs");
        // Spelled in two halves on purpose: written whole, the assertion would
        // find itself in the file it reads and pass on its own text.
        let no_witness = format!("default_registry(&ledger, {})", "None");
        assert!(
            !SOURCE.contains(&no_witness),
            "the shell builds the action registry with no witness, so the text \
             of a running step reaches nobody"
        );
    }

    #[test]
    fn a_single_engine_root_takes_the_mandate_on_its_stdin() {
        let flow = flow_with(
            json!([engine_step(
                "dispatch",
                vec![],
                json!({ "bin": "true", "timeout_secs": 5 })
            )]),
            json!({ "dispatch": { "bin": "true", "timeout_secs": 5 } }),
        );
        match mandate_target(&flow) {
            MandateTarget::Field { step, field } => {
                assert_eq!(step, "dispatch");
                assert_eq!(field, "stdin");
            }
            other => panic!("atteso un bersaglio, trovato {other:?}"),
        }

        let inputs = inputs_with_mandate(&flow, Some("fai questa cosa")).expect("consegna accolta");
        assert_eq!(
            inputs["dispatch"]["stdin"],
            json!("fai questa cosa"),
            "la consegna deve entrare nello stdin del passo di partenza"
        );
    }

    /// THE MEASURE THAT COULD HAVE COME OUT DIFFERENTLY: the step already
    /// declares `stdin` in its fixed parameters, which win over the inputs.
    /// Without the check in `mandate_target` the mandate would be written to
    /// the inputs, overridden by `with` at execution, and lost with no error:
    /// whoever pressed would believe they had given their own text. Remove that
    /// check and this test goes red.
    #[test]
    fn a_root_that_fixes_its_own_stdin_refuses_the_mandate_instead_of_losing_it() {
        let flow = flow_with(
            json!([engine_step(
                "dispatch",
                vec![],
                json!({ "bin": "true", "stdin": "gia' deciso", "timeout_secs": 5 })
            )]),
            json!({}),
        );
        match mandate_target(&flow) {
            MandateTarget::None { why } => assert!(why.contains("scavalcata"), "{why}"),
            other => panic!("atteso un rifiuto, trovato {other:?}"),
        }
        let error = inputs_with_mandate(&flow, Some("la mia consegna"))
            .expect_err("una consegna che verrebbe persa ferma la partenza");
        assert!(error.contains("scavalcata"), "{error}");
    }

    /// A shell check takes a command, not text written by a person: slipping
    /// it in there would be a shell injection.
    #[test]
    fn a_shell_root_has_no_place_for_a_mandate() {
        let flow = flow_with(
            json!([{
                "id": "solo", "deps": [], "action": "shell_check", "max_attempts": 1,
                "when": null, "input_schema": { "type": "any" }, "output_schema": { "type": "any" }
            }]),
            json!({ "solo": { "command": "true", "timeout_secs": 5 } }),
        );
        match mandate_target(&flow) {
            MandateTarget::None { why } => assert!(why.contains("no text input"), "{why}"),
            other => panic!("atteso un rifiuto, trovato {other:?}"),
        }
    }

    #[test]
    fn two_roots_leave_the_mandate_without_an_address() {
        let flow = flow_with(
            json!([
                engine_step("uno", vec![], json!({ "bin": "true", "timeout_secs": 5 })),
                engine_step("due", vec![], json!({ "bin": "true", "timeout_secs": 5 })),
            ]),
            json!({}),
        );
        match mandate_target(&flow) {
            MandateTarget::None { why } => assert!(why.contains("2 steps"), "{why}"),
            other => panic!("atteso un rifiuto, trovato {other:?}"),
        }
    }

    /// No mandate written: the flow starts with its own inputs exactly as they
    /// are, even when it would have nowhere to put one.
    #[test]
    fn no_mandate_leaves_the_declared_inputs_untouched() {
        let flow = flow_with(
            json!([{
                "id": "solo", "deps": [], "action": "shell_check", "max_attempts": 1,
                "when": null, "input_schema": { "type": "any" }, "output_schema": { "type": "any" }
            }]),
            json!({ "solo": { "command": "true", "timeout_secs": 5 } }),
        );
        let inputs = inputs_with_mandate(&flow, None).expect("nessuna consegna, nessun problema");
        assert_eq!(inputs["solo"]["command"], json!("true"));
        // Whitespace is not a mandate: it would be a refusal over a text
        // nobody actually wrote.
        let blank = inputs_with_mandate(&flow, Some("   ")).expect("il bianco non è una consegna");
        assert_eq!(blank["solo"]["command"], json!("true"));
    }

    fn trigger_step(id: &str, with: Value) -> Value {
        json!({
            "id": id,
            "deps": [],
            "action": "trigger",
            "max_attempts": 1,
            "when": null,
            "with": with,
            "input_schema": { "type": "any" },
            "output_schema": { "type": "any" }
        })
    }

    /// THE REAL CONTRACT, read from `flows/dispatch-the-work.flow.json`: a
    /// `trigger` step of manual source carries the mandate in its own `text`,
    /// not in a `stdin`.
    ///
    /// THE MEASURE THAT COULD HAVE COME OUT DIFFERENTLY: if `text_field_of` did
    /// not know the `trigger` action, the mandate would be refused on a flow
    /// that expects it — and the button would say «no place for it» in front of
    /// a node built to receive it.
    #[test]
    fn a_manual_trigger_root_takes_the_mandate_in_its_own_text() {
        let flow = flow_with(
            json!([
                trigger_step("trigger", json!({ "source": "manual" })),
                engine_step(
                    "dispatch",
                    vec!["trigger"],
                    json!({ "bin": "true", "timeout_secs": 5 })
                ),
            ]),
            json!({ "trigger": { "text": "la consegna di ieri" } }),
        );
        match mandate_target(&flow) {
            MandateTarget::Field { step, field } => {
                assert_eq!(step, "trigger");
                assert_eq!(field, "text");
            }
            other => panic!("atteso il campo dell'innesco, trovato {other:?}"),
        }

        let inputs = inputs_with_mandate(&flow, Some("la consegna di oggi")).expect("accolta");
        assert_eq!(
            inputs["trigger"]["text"],
            json!("la consegna di oggi"),
            "la consegna di chi preme deve sostituire quella scritta nel file"
        );
    }

    /// A trigger waiting for a signal that is not a person's gesture takes no
    /// hand-written mandate: the button would promise something that step is
    /// not waiting for from there.
    #[test]
    fn a_trigger_waiting_for_another_kind_of_signal_refuses_the_mandate() {
        let flow = flow_with(
            json!([trigger_step("trigger", json!({ "source": "schedule" }))]),
            json!({}),
        );
        match mandate_target(&flow) {
            MandateTarget::None { why } => assert!(why.contains("schedule"), "{why}"),
            other => panic!("atteso un rifiuto, trovato {other:?}"),
        }
    }

    /// **EVERY ACTION OF A SHIPPED FLOW MUST BE IN THE WINDOW'S REGISTRY.**
    ///
    /// It is the same check `start_run` makes before executing (the one that
    /// answers «the flow names actions the engine does not know»), here on the
    /// flows inside the binary. The shell used to build a list of its own,
    /// shorter than the terminal's: `tool_needs` and the tool resolver were
    /// missing, so a flow shipped with the product was refused by the window
    /// and accepted by the terminal.
    ///
    /// The ledger is not needed here: the missing actions are not the ones
    /// that write.
    #[test]
    fn every_action_of_a_shipped_flow_is_known_to_the_window() {
        let known = registry::registry_in(registry::House::empty(), None, None);
        for name in ["what-this-machine-has", "migrate-to-sailor"] {
            let flow = load_flow(name).expect("i flussi di sistema si caricano");
            for step in flow.graph.steps() {
                assert!(
                    known.get(&step.action).is_some(),
                    "«{}» nomina l'azione «{}», che la finestra non conosce",
                    name,
                    step.action
                );
            }
        }
    }

    /// **THE PROOF OF THE REPAIR, AND IT READS NOBODY'S MACHINE.**
    ///
    /// The system flows live **inside the binary**: they are on any machine,
    /// even a freshly installed one, even where `~/.config/sailor` does not
    /// exist. This `load_flow` used to find none of them — it looked in one
    /// directory, and not that one — so `flow_trigger` failed, the trigger
    /// stayed mute and the ▶ Run button was grey on every node of the canvas.
    ///
    /// Put `default_flows_dir()` back in place of the list and this test goes
    /// red (measured).
    #[test]
    fn a_flow_shipped_inside_the_binary_is_loadable_from_the_window() {
        let flow = load_flow("what-this-machine-has")
            .expect("un flusso di sistema si carica ovunque, senza niente sul disco");
        assert!(
            !flow.graph.steps().is_empty(),
            "e arriva col suo grafo, non come guscio vuoto"
        );
    }

    /// **THE SAME GUARANTEE AS BEFORE, REACHED ANOTHER WAY.** There was a test
    /// on `safe_name` here, the check that stopped a name from climbing out of
    /// the directory when the name became a path. Now the name is looked up in
    /// a list: it opens nothing by construction, and `safe_name` is gone. The
    /// test stays, because what must be guaranteed is the same — a crooked name
    /// must read nothing — and it is the behaviour that is tested, not the
    /// function that achieved it.
    #[test]
    fn a_flow_name_that_climbs_out_of_the_directory_opens_nothing() {
        for malformed in ["../evaso", "sotto/cartella", "", "/etc/passwd"] {
            let outcome = load_flow(malformed);
            assert!(
                outcome.is_err(),
                "«{malformed}» non è il nome di nessun flusso: non deve caricare niente"
            );
            let why = outcome.unwrap_err();
            assert!(
                why.contains("no flow is called"),
                "e il motivo dev'essere che non è in elenco, non un errore di lettura: {why}"
            );
        }
    }
}
