//! What a session event starts.
//!
//! **THE HOOK MUST COME BACK AT ONCE.** A person's prompt waits on this call,
//! so the run is started and let go: nothing here waits for a flow to finish,
//! and nothing here types into a terminal.

use sessions::{Sessions, Verdict, ACTED, BROKE, DEFERRED};
pub use trigger::Happened;
use trigger::{deferral, On};
use ui::gather::FlowSource;

/// How a flow is started, given its name and the delivery. Handed in so a test
/// can watch what the arc asks for without starting anything.
pub type Starter<'a> = &'a mut dyn FnMut(&str, &str) -> Result<String, String>;

/// Which flows watch for a session event, and what each of them asks of it.
///
/// A flow declares it in its trigger step's `on`; a flow whose trigger names
/// another source is not a candidate and leaves no row, because it was never
/// asked.
pub fn watchers(sources: &[FlowSource]) -> Vec<(String, On)> {
    let mut found = Vec::new();
    for (name, _, entry) in ui::gather::load_all_flows(sources) {
        let Ok(flow) = entry else {
            continue;
        };
        let Some(step) = flow
            .graph
            .steps()
            .iter()
            .find(|step| step.action == trigger::TRIGGER_ACTION)
        else {
            continue;
        };
        let declared = step
            .with
            .as_ref()
            .or_else(|| flow.inputs.get(&step.id))
            .cloned()
            .unwrap_or_default();
        if declared.get("source").and_then(serde_json::Value::as_str) != Some(SESSION_EVENT) {
            continue;
        }
        match declared.get("on").cloned().map(serde_json::from_value) {
            Some(Ok(on)) => found.push((name, on)),
            // A flow that asks for this source and says nothing about which
            // event would start on every event of every tree. It is left out
            // and said so, rather than firing on everything.
            _ => found.push((name, On::default())),
        }
    }
    found
}

/// The id of the shipped source. Written once: a flow declaring it by another
/// spelling is a flow nothing starts, in silence.
pub const SESSION_EVENT: &str = "session-event";

/// Judges every watching flow against one event, writes a row for each, and
/// starts what it should.
///
/// **A ROW FOR EVERY EVALUATION, THE REFUSALS INCLUDED.** The relay this
/// replaces declined 2,803 times of 2,834 leaving no trace, so a guard that
/// had stopped working looked exactly like one doing its job.
pub fn evaluate(
    store: &Sessions,
    event_id: i64,
    happened: &Happened,
    sources: &[FlowSource],
    at: i64,
    start: Starter<'_>,
) -> Vec<Verdict> {
    let mut written = Vec::new();
    for (flow, on) in watchers(sources) {
        if let Some(why) = deferral(&on, happened) {
            written.push(note(store, event_id, &flow, DEFERRED, Some(why), None, at));
            continue;
        }
        // The same event replayed starts nothing a second time: a command line
        // that retries its hook would otherwise do the work twice.
        if store.already_judged(event_id, &flow).unwrap_or(false) {
            continue;
        }
        let (verdict, why, run) = match start(&flow, &delivery(happened)) {
            Ok(said) => (ACTED, None, Some(said)),
            Err(complaint) => (BROKE, Some(complaint), None),
        };
        written.push(note(
            store,
            event_id,
            &flow,
            verdict,
            why.as_deref(),
            run.as_deref(),
            at,
        ));
    }
    written
}

/// What the flow is handed: the event, in the shape its trigger step reads.
///
/// **WHAT A PERSON TYPED DOES NOT TRAVEL.** The phrase is matched in memory
/// and stays there: passed on it would enter an argument list, a process table
/// and the ledger's own record of the child, and a secret typed once into a
/// store that keeps everything stays on that disk for as long as the disk does.
fn delivery(happened: &Happened) -> String {
    serde_json::json!({
        "event": happened.event,
        "tree": happened.tree,
        "tty": happened.tty,
        "session": happened.session,
        "transcript": happened.transcript,
    })
    .to_string()
}

fn note(
    store: &Sessions,
    event_id: i64,
    flow: &str,
    verdict: &str,
    why: Option<&str>,
    run_id: Option<&str>,
    at: i64,
) -> Verdict {
    let row = Verdict {
        event_id,
        flow: flow.to_owned(),
        verdict: verdict.to_owned(),
        why: why.map(str::to_owned),
        run_id: run_id.map(str::to_owned),
        decided_at: at,
    };
    let _ = store.record_verdict(&row);
    row
}

/// Starts `sailor flow run` as a process of its own and lets it go.
///
/// It goes through the one road that records a child in the ledger, and then
/// says so is finished with it: a run started because a session spoke must
/// outlive the hook that started it, and dropping the handle would stop it.
pub fn launch_detached(flow: &str, delivery: &str) -> Result<String, String> {
    let binary = std::env::current_exe().map_err(|error| {
        catalogue::say(
            "cli.arc.no_binary_of_my_own",
            &[("error", &error.to_string())],
        )
    })?;
    let here = std::env::current_dir().map_err(|error| {
        catalogue::say(
            "cli.arc.no_directory_of_my_own",
            &[("error", &error.to_string())],
        )
    })?;
    let ledger = ledger::default_directory().and_then(|dir| ledger::Ledger::open(&dir).ok());
    let supervisor = machine::Supervisor::over(ledger);
    let started = supervisor.start(machine::child::Spec {
        process_id: format!("session-event-{flow}-{}", machine::now()),
        command: binary.display().to_string(),
        args: vec![
            "flow".to_owned(),
            "run".to_owned(),
            flow.to_owned(),
            delivery.to_owned(),
        ],
        working_directory: here,
        port: None,
        purpose: format!("a session event starts «{flow}»"),
        started_by: "sailor session event".to_owned(),
        environment: Vec::new(),
        // It outlives this call: its lines would land in the middle of
        // somebody else's work, minutes after the hook that lit it ended.
        speaks: false,
    })?;
    Ok(format!("started as pid {}", started.let_it_go()))
}
