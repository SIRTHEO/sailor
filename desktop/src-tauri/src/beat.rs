//! The beat that starts due flows from inside the window.
//!
//! **A DUE FLOW IS RUN BY SOMETHING INSIDE THE SYSTEM.** The schedule is the
//! constraint; the window reading its flows is the event that judges it; a
//! thread waking every minute is the deadline for when nobody reads. Cron
//! outside stays as the net under all three.

// The judgement is `flow::is_due`, the one `flow tick` uses, read fresh each
// time from the schedule, the ledger and the clock: nothing is remembered
// between beats, so a window killed and reopened decides the same.

use flow::system::FAULT_WRITER;
use flow::{FailureStreak, FaultToWrite, FlowFile};
use serde::Serialize;
use std::collections::{BTreeMap, BTreeSet};
use std::sync::{Arc, Mutex, TryLockError};
use std::time::Duration;
use tauri::{AppHandle, Emitter, Manager};
use ui::gather::{default_ledger_dir, flow_sources, load_all_flows};

/// How long the deadline waits between two beats of its own.
pub const EVERY: Duration = Duration::from_secs(60);

/// What the ledger says started a run the beat started.
pub const ORIGIN: &str = "window · schedule";

/// What the ledger says started a fault the beat asked to be written.
pub const FAULT_ORIGIN: &str = "window · fault";

/// The event the window hears after each beat, carrying the `Report`.
pub const EVENT: &str = "flow_beat";

/// What became of one flow at one beat.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "verdict", rename_all = "snake_case")]
pub enum Verdict {
    Ran { run_id: String },
    Held { why: String },
    Broke { why: String },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Decision {
    pub flow: String,
    #[serde(flatten)]
    pub verdict: Verdict,
}

/// **A BEAT SAYS WHAT IT DID NOT DO, AND WHY.** Every known flow gets a line,
/// the held ones included: a guard that declines in silence cannot be told
/// from a guard that is broken.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct Report {
    pub at: i64,
    pub decisions: Vec<Decision>,
}

/// The last report, and a lock so two beats never judge the same instant.
#[derive(Default)]
pub struct Beat {
    last: Mutex<Option<Report>>,
    judging: Mutex<()>,
}

type Known = (String, &'static str, Result<FlowFile, String>);

/// Which of `known` are due at `now`, and for each held one the reason.
///
/// `running` names the flows this window has under way: a flow whose last
/// run is still open is not started a second time on top of itself.
pub fn judge(
    known: &[Known],
    last: &BTreeMap<String, i64>,
    running: &[String],
    now: i64,
) -> Vec<(String, Option<String>)> {
    known
        .iter()
        .map(|(name, _, entry)| {
            let why = match entry {
                Err(_) => Some(catalogue::say("cli.flow.will_not_load", &[])),
                Ok(flow) if running.iter().any(|id| id == &flow.id) => {
                    Some(catalogue::say("desktop.beat.still_running", &[]))
                }
                Ok(flow) => match flow.schedule.as_ref() {
                    None => Some(catalogue::say("cli.flow.no_schedule_by_hand_only", &[])),
                    Some(schedule) => {
                        let last_run = last.get(&flow.id).copied();
                        if flow::is_due(schedule, last_run, now) {
                            None
                        } else {
                            Some(match last_run {
                                Some(seconds) => catalogue::say(
                                    "cli.flow.not_due_last_ran",
                                    &[("minutes", &((now - seconds) / 60).to_string())],
                                ),
                                None => catalogue::say("cli.flow.not_due", &[]),
                            })
                        }
                    }
                },
            };
            (name.clone(), why)
        })
        .collect()
}

/// One decision in two words, the shape a line of the beat is written in.
fn said(verdict: &Verdict) -> (&'static str, &str) {
    match verdict {
        Verdict::Ran { run_id } => ("ran", run_id.as_str()),
        Verdict::Held { why } => ("hold", why.as_str()),
        Verdict::Broke { why } => ("broke", why.as_str()),
    }
}

/// **A BEAT PRINTS WHAT CHANGED, NOT WHAT IS.** It judges every known flow
/// every minute, and printing all of it was thirty lines a minute for as long
/// as the window stays open — on a machine meant to stay on for years, tens of
/// thousands of lines a day, among which the one that matters is invisible.
/// A steady schedule now says nothing, and every transition still says itself.
pub fn news<'a>(previous: Option<&Report>, current: &'a Report) -> Vec<&'a Decision> {
    current
        .decisions
        .iter()
        .filter(|decision| {
            let before = previous.and_then(|report| {
                report
                    .decisions
                    .iter()
                    .find(|seen| seen.flow == decision.flow)
            });
            match before {
                // A flow nobody judged before is news whatever it says: it has
                // just appeared, or this is the first beat of this window.
                None => true,
                Some(before) => said(&before.verdict) != said(&decision.verdict),
            }
        })
        .collect()
}

fn now_secs() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|elapsed| elapsed.as_secs() as i64)
        .unwrap_or(0)
}

/// What one look at the ledger tells the beat: when each flow last started,
/// which flows keep failing, and which failures a beat already wrote a fault
/// about. The ledger stays in hand to note the faults this beat asks for.
#[derive(Default)]
struct Glance {
    last_started: BTreeMap<String, i64>,
    streaks: Vec<FailureStreak>,
    faults_written: BTreeSet<String>,
    ledger: Option<ledger::Ledger>,
}

/// A ledger that is not there yet means nothing has ever run, and then
/// everything is due and nothing has failed.
fn glance() -> Result<Glance, String> {
    let dir = default_ledger_dir();
    if !dir.join("state.db").exists() {
        return Ok(Glance::default());
    }
    ledger::Ledger::open(&dir)
        .and_then(|ledger| {
            Ok(Glance {
                last_started: ledger.last_started_at()?,
                streaks: ledger.failure_streaks(flow::FAILURES_THAT_MAKE_A_FAULT)?,
                faults_written: ledger.faults_written()?,
                ledger: Some(ledger),
            })
        })
        .map_err(|error| format!("{}: {error}", dir.display()))
}

/// The faults the beat starts the writer for now: the shared rule, minus the
/// case only a window has — the writer still busy with an earlier fault, which
/// is not started on top of itself and is asked again at the next beat.
pub fn faults_to_start(
    streaks: &[FailureStreak],
    faults_written: &BTreeSet<String>,
    running: &[String],
) -> Vec<FaultToWrite> {
    if running.iter().any(|id| id == FAULT_WRITER) {
        return Vec::new();
    }
    flow::faults_due(streaks, faults_written)
}

/// Starts the fault writer about `fault` and remembers the failed run whatever
/// the start came to: a writer that cannot run is said once, not every minute.
fn write_fault(app: &AppHandle, runs: &Arc<crate::run::Runs>, glance: &Glance, fault: &FaultToWrite, now: i64) -> Decision {
    let times = fault.length.to_string();
    let not_written = |why: &str| Verdict::Broke {
        why: catalogue::say(
            "desktop.beat.fault_not_written",
            &[("flow", &fault.flow), ("times", &times), ("why", why)],
        ),
    };
    let mut verdict = match crate::run::start(app, runs, FAULT_WRITER, Some(&fault.flow), FAULT_ORIGIN.to_owned()) {
        Ok(started) => {
            println!(
                "beat\t{FAULT_WRITER}\tfault\t{}",
                catalogue::say(
                    "desktop.beat.writes_fault",
                    &[("flow", &fault.flow), ("times", &times), ("run_id", &started.run_id)],
                )
            );
            Verdict::Ran {
                run_id: started.run_id,
            }
        }
        Err(why) => not_written(&why),
    };
    if let Some(ledger) = &glance.ledger {
        if let Err(error) = ledger.remember_fault_written(&fault.flow, &fault.run_id, FAULT_ORIGIN, now) {
            verdict = not_written(&error.to_string());
        }
    }
    Decision {
        flow: FAULT_WRITER.to_owned(),
        verdict,
    }
}

/// One beat, now. Returns nothing when another beat is judging this instant.
pub fn once(app: &AppHandle) -> Option<Report> {
    let beat = app.state::<Arc<Beat>>().inner().clone();
    let _judging = match beat.judging.try_lock() {
        Ok(guard) => guard,
        Err(TryLockError::WouldBlock) => return None,
        Err(TryLockError::Poisoned(poisoned)) => poisoned.into_inner(),
    };
    let runs = app.state::<Arc<crate::run::Runs>>().inner().clone();
    let now = now_secs();
    let known = load_all_flows(&flow_sources());
    let decisions = match glance() {
        Err(why) => known
            .iter()
            .map(|(flow, _, _)| Decision {
                flow: flow.clone(),
                verdict: Verdict::Held {
                    why: catalogue::say("desktop.beat.could_not_look", &[("why", &why)]),
                },
            })
            .collect(),
        Ok(glance) => {
            let running = runs.running_flows();
            let mut decisions: Vec<Decision> = judge(&known, &glance.last_started, &running, now)
                .into_iter()
                .map(|(flow, why)| {
                    let verdict = match why {
                        Some(why) => Verdict::Held { why },
                        None => match crate::run::start(app, &runs, &flow, None, ORIGIN.to_owned()) {
                            Ok(started) => Verdict::Ran {
                                run_id: started.run_id,
                            },
                            Err(why) => Verdict::Broke { why },
                        },
                    };
                    Decision { flow, verdict }
                })
                .collect();
            let faults = faults_to_start(&glance.streaks, &glance.faults_written, &running);
            // The writer's line says what it did this beat, not that it has
            // no schedule: the fault decisions take its place.
            if !faults.is_empty() {
                decisions.retain(|decision| decision.flow != FAULT_WRITER);
            }
            for fault in &faults {
                decisions.push(write_fault(app, &runs, &glance, fault, now));
            }
            decisions
        }
    };
    let report = Report { at: now, decisions };
    let mut held = beat.last.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    for decision in news(held.as_ref(), &report) {
        let (word, rest) = said(&decision.verdict);
        println!("beat\t{}\t{word}\t{rest}", decision.flow);
    }
    *held = Some(report.clone());
    drop(held);
    let _ = app.emit(EVENT, &report);
    crate::events::emit(app, "beat", &report);
    Some(report)
}

/// The event: whoever reads the flows has them judged, off the calling thread.
pub fn on_read(app: &AppHandle) {
    let app = app.clone();
    std::thread::spawn(move || {
        once(&app);
    });
}

/// The deadline: a beat every `EVERY`, for as long as the window is open.
pub fn keep(app: &AppHandle) {
    let app = app.clone();
    std::thread::spawn(move || loop {
        once(&app);
        std::thread::sleep(EVERY);
    });
}

/// The last beat's report, for whoever wants to know what was held and why.
#[tauri::command]
pub(crate) fn beat_report(beat: tauri::State<'_, Arc<Beat>>) -> Option<Report> {
    beat.last
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .clone()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn decided(flow: &str, verdict: Verdict) -> Decision {
        Decision {
            flow: flow.to_owned(),
            verdict,
        }
    }

    fn held(why: &str) -> Verdict {
        Verdict::Held {
            why: why.to_owned(),
        }
    }

    fn reported(decisions: Vec<Decision>) -> Report {
        Report { at: 100, decisions }
    }

    /// **A BEAT PRINTS WHAT CHANGED, NOT WHAT IS.** Thirty flows judged every
    /// minute was thirty lines a minute for as long as the window stays open,
    /// and this window is meant to stay open for years: the line that matters
    /// was already invisible among the ones that say «not due» forever.
    #[test]
    fn a_schedule_that_did_not_move_says_nothing() {
        let steady = reported(vec![
            decided("notte", held("not due, last ran 3 minutes ago")),
            decided("relay", held("no schedule: it starts by hand only")),
        ]);
        // The first beat of a window says everything: nobody has heard it yet.
        assert_eq!(news(None, &steady).len(), 2);
        assert!(news(Some(&steady), &steady).is_empty());
    }

    #[test]
    fn every_transition_still_says_itself() {
        let before = reported(vec![
            decided("notte", held("not due, last ran 3 minutes ago")),
            decided("relay", held("no schedule: it starts by hand only")),
        ]);
        let after = reported(vec![
            decided(
                "notte",
                Verdict::Ran {
                    run_id: "prima-corsa".to_owned(),
                },
            ),
            decided("relay", held("no schedule: it starts by hand only")),
        ]);
        let said = news(Some(&before), &after);
        assert_eq!(said.len(), 1);
        assert_eq!(said[0].flow, "notte");

        // A hold whose reason changed is news too: «not due» becoming «the
        // engine is not signed in» is the whole of what a person needs.
        let refused = reported(vec![
            decided("notte", held("not due, last ran 3 minutes ago")),
            decided("relay", held("the engine is not signed in")),
        ]);
        assert_eq!(news(Some(&before), &refused).len(), 1);

        // And a flow nobody judged before is news whatever it says.
        let arrived = reported(vec![decided("un-altro", held("not due"))]);
        assert_eq!(news(Some(&before), &arrived).len(), 1);
    }

    use flow::system::YOUR_ORIGIN;

    fn flow_called(id: &str, every: Option<u64>) -> Known {
        let mut file = serde_json::json!({
            "id": id,
            "description": "a flow for the beat to judge",
            "graph": { "steps": [] },
            "inputs": {}
        });
        if let Some(seconds) = every {
            file["schedule"] = serde_json::json!({
                "recurrence": { "kind": "every_seconds", "seconds": seconds },
                "weight": "light"
            });
        }
        let file: FlowFile = serde_json::from_value(file).expect("a flow with no steps loads");
        (id.to_owned(), YOUR_ORIGIN, Ok(file))
    }

    /// **THE ABSURD CASE FIRST**: with no schedule nothing is ever due, no
    /// matter how long ago it ran. Then the one that matters: a flow that
    /// should run every minute and last ran two minutes ago is due, the same
    /// one that ran ten seconds ago is held, and each held one says why.
    #[test]
    fn a_scheduled_flow_is_due_when_its_interval_has_passed_and_says_why_when_not() {
        let known = vec![
            flow_called("by-hand", None),
            flow_called("stale", Some(60)),
            flow_called("fresh", Some(60)),
            flow_called("never", Some(60)),
            ("torn".to_owned(), YOUR_ORIGIN, Err("torn".to_owned())),
        ];
        let now = 1_000_000;
        let last = BTreeMap::from([
            ("by-hand".to_owned(), now - 100_000),
            ("stale".to_owned(), now - 120),
            ("fresh".to_owned(), now - 10),
        ]);
        let judged: BTreeMap<String, Option<String>> =
            judge(&known, &last, &[], now).into_iter().collect();
        assert_eq!(judged["stale"], None, "two minutes past a one-minute interval is due");
        assert_eq!(judged["never"], None, "a flow that never ran is due");
        assert!(judged["by-hand"].as_deref().is_some_and(|why| why.contains("by hand")));
        assert!(judged["fresh"].as_deref().is_some_and(|why| why.contains("not due")));
        assert!(judged["torn"].as_deref().is_some_and(|why| why.contains("will not load")));
    }

    /// A flow still running from an earlier start is not started on top of
    /// itself, even though its last start is past the interval.
    #[test]
    fn a_flow_still_running_is_not_started_again() {
        let known = vec![flow_called("long", Some(60))];
        let now = 1_000_000;
        let last = BTreeMap::from([("long".to_owned(), now - 600)]);
        let judged = judge(&known, &last, &["long".to_owned()], now);
        assert!(
            judged[0].1.as_deref().is_some_and(|why| why.contains("still running")),
            "{judged:?}"
        );
        let judged = judge(&known, &last, &[], now);
        assert_eq!(judged[0].1, None, "with nothing running the same flow is due");
    }

    /// A flow three failures deep has its fault started, unless the writer is
    /// still busy with an earlier one, or this failure was already written.
    #[test]
    fn a_fault_is_started_unless_the_writer_is_still_busy_or_it_was_written() {
        let streaks = vec![FailureStreak {
            flow: "relay".to_owned(),
            runs: vec!["relay-3".to_owned(), "relay-2".to_owned(), "relay-1".to_owned()],
        }];
        let nothing_written = BTreeSet::new();
        let due = faults_to_start(&streaks, &nothing_written, &[]);
        assert_eq!(due.len(), 1, "{due:?}");
        assert_eq!((due[0].flow.as_str(), due[0].run_id.as_str()), ("relay", "relay-3"));
        assert!(faults_to_start(&streaks, &nothing_written, &[FAULT_WRITER.to_owned()]).is_empty());
        assert!(faults_to_start(&streaks, &BTreeSet::from(["relay-3".to_owned()]), &[]).is_empty());
    }

    /// The report the window hears is tagged the way the contract says.
    #[test]
    fn a_decision_serialises_with_its_verdict_flat() {
        let ran = serde_json::to_value(Decision {
            flow: "x".to_owned(),
            verdict: Verdict::Ran {
                run_id: "x-1".to_owned(),
            },
        })
        .expect("serialise");
        assert_eq!(ran, serde_json::json!({ "flow": "x", "verdict": "ran", "run_id": "x-1" }));
        let held = serde_json::to_value(Decision {
            flow: "y".to_owned(),
            verdict: Verdict::Held {
                why: "not due".to_owned(),
            },
        })
        .expect("serialise");
        assert_eq!(held, serde_json::json!({ "flow": "y", "verdict": "held", "why": "not due" }));
    }
}
