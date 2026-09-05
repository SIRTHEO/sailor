//! `sailor flow tick` and `sailor flow due`: the beat that starts what is due,
//! writes a fault after a streak of failures, and lists the runs left waiting.

use ledger::Ledger;
use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;
use ui::gather::FlowSource;

use super::run_and_resume::{run_flow, seat_of};
use super::{default_ledger_dir, known_flows, nothing_found};

/// Le corse ferme in attesa di qualcuno, in coda a un elenco.
///
/// **STA IN CODA A `list` E A `due` PERCHÉ È LÌ CHE SI GUARDA.** Una consegna
/// che nessuno raccoglie non compare da nessuna parte: non è un passo aperto,
/// quindi `unfinished_runs` non la trova, e il flusso da cui viene risulta
/// «girato di recente», quindi `due` lo dichiara non dovuto. Sparisce due volte.
pub(super) fn waiting_report() -> String {
    let ledger = default_ledger_dir()
        .ok()
        .filter(|dir| dir.join("state.db").exists())
        .and_then(|dir| Ledger::open(&dir).ok());
    let waiting = ledger
        .as_ref()
        .and_then(|ledger| ledger.waiting_runs().ok())
        .unwrap_or_default();
    // Two lists and not one: a run somebody must come and take is not a run
    // that comes back by itself, and reading them together sends a person to
    // take a step nobody handed them. An empty first list cannot return early
    // any more, or the second one would never be reached.
    let to_ask_again = ledger
        .as_ref()
        .and_then(|ledger| ledger.runs_to_ask_again().ok())
        .unwrap_or_default();
    let mut report = if waiting.is_empty() {
        catalogue::say("cli.flow.no_run_is_waiting", &[])
    } else {
        let mut lines = catalogue::say(
            "cli.flow.runs_waiting_for_somebody",
            &[("count", &waiting.len().to_string())],
        );
        for run in waiting {
            let _ = write!(
                lines,
                "\n  {}\t{}\tsailor flow resume {}",
                run.run_id, run.entity, run.run_id
            );
        }
        lines
    };
    if !to_ask_again.is_empty() {
        let _ = write!(
            report,
            "\n{}",
            catalogue::say(
                "cli.flow.runs_to_ask_again",
                &[("count", &to_ask_again.len().to_string())]
            )
        );
        for run in to_ask_again {
            let _ = write!(
                report,
                "\n  {}\t{}\tsailor flow resume {}",
                run.run_id, run.entity, run.run_id
            );
        }
    }
    report
}

/// What the ledger can say about when each flow last started.
///
/// **«NOTHING HAS RUN» AND «I COULD NOT LOOK» ARE DIFFERENT ANSWERS**, and
/// fault 12 is the two of them sharing one. An empty map makes every scheduled
/// flow due, which is right the first time and a lie when the ledger simply
/// would not open — there the honest report is that nobody knows.
enum LastRuns {
    Read(Glance),
    NothingHasRunYet,
    CouldNotLook(String),
}

/// What one look at the ledger tells a beat: when each flow last started,
/// which flows keep failing, and which of those failures a beat already wrote
/// a fault about. The ledger it came from stays in hand, so the beat can note
/// the fault it writes in the same place the next beat will read.
#[derive(Default)]
struct Glance {
    last_started: BTreeMap<String, i64>,
    streaks: Vec<flow::FailureStreak>,
    faults_written: BTreeSet<String>,
    ledger: Option<Ledger>,
}

fn glance_at(ledger: &Ledger) -> Result<Glance, ledger::LedgerError> {
    Ok(Glance {
        last_started: ledger.last_started_at()?,
        streaks: ledger.failure_streaks(flow::FAILURES_THAT_MAKE_A_FAULT)?,
        faults_written: ledger.faults_written()?,
        ledger: Some(ledger.clone()),
    })
}

impl LastRuns {
    /// `consequence` is the catalogue key for what not looking cost, since the
    /// beat and the due list pay it differently.
    fn read_or_say_it_could_not(self, consequence: &str) -> Result<Glance, String> {
        match self {
            LastRuns::Read(found) => Ok(found),
            LastRuns::NothingHasRunYet => Ok(Glance::default()),
            LastRuns::CouldNotLook(why) => Err(catalogue::say(consequence, &[("why", &why)])),
        }
    }
}

fn last_runs() -> LastRuns {
    let dir = match default_ledger_dir() {
        Ok(dir) => dir,
        Err(why) => return LastRuns::CouldNotLook(why),
    };
    // A ledger that is not there yet is not a failure: nothing has ever run, so
    // everything is due, and that is the right answer.
    if !dir.join("state.db").exists() {
        return LastRuns::NothingHasRunYet;
    }
    match Ledger::open(&dir).and_then(|ledger| glance_at(&ledger)) {
        Ok(found) => LastRuns::Read(found),
        Err(error) => LastRuns::CouldNotLook(catalogue::say(
            "cli.flow.ledger_would_not_be_read",
            &[
                ("where", &dir.display().to_string()),
                ("error", &error.to_string()),
            ],
        )),
    }
}

/// Quali flussi sono dovuti adesso, e quando ciascuno è girato l'ultima volta.
///
/// PERCHÉ QUESTO COMANDO ESISTE PRIMA DI UNO SCHEDULATORE. Finché nessuno sa
/// dire *che cosa dovrebbe girare adesso*, un cron non si può convertire in
/// flusso: si convertirebbe che cosa fa, perdendo quando lo fa. Qui la domanda
/// riceve una risposta che una persona può leggere e smentire — che è il
/// gradino prima di lasciarla eseguire a una macchina.
///
/// L'ora si legge **una volta sola** e si passa a tutti: due flussi giudicati su
/// due istanti diversi non sono confrontabili, e la differenza si vede solo nei
/// casi rari, cioè quando fa più danno.
/// One beat: what is due right now, and nothing remembered between beats.
///
/// The decision is a function of the schedule, the last run and the clock, all
/// three read fresh. Killed at any instant and restarted, the next beat decides
/// exactly what it would have decided without the interruption — which is the
/// one property a thing that runs forever has to have on a machine that sleeps.
pub(super) fn tick_flows(sources: &[FlowSource]) -> Result<String, String> {
    tick_flows_with(sources, last_runs(), &mut |name, mandate| {
        run_flow(sources, name, mandate)
    })
}

/// How the beat starts a flow, given its name and a mandate. Handed in so a
/// test can watch what the beat asks for without running anything.
type Starter<'a> = &'a mut dyn FnMut(&str, Option<&str>) -> Result<String, String>;

fn tick_flows_with(sources: &[FlowSource], last: LastRuns, start: Starter<'_>) -> Result<String, String> {
    let known = known_flows(sources);
    if known.is_empty() {
        return Ok(nothing_found(sources));
    }
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);
    let glance = last.read_or_say_it_could_not("cli.flow.beat_could_not_look")?;
    let last = &glance.last_started;

    let mut report = String::new();
    let mut ran = 0usize;
    let mut held = 0usize;
    for (name, _, entry) in known {
        // **A BEAT SAYS WHAT IT DID NOT DO, AND WHY.** The relay this replaces
        // declined 2,803 times out of 2,834 and left no trace of any of them,
        // so nobody could tell a working guard from a broken one.
        let reason = match &entry {
            Err(_) => Some(catalogue::say("cli.flow.will_not_load", &[])),
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
        if let Some(why) = reason {
            held += 1;
            let _ = writeln!(report, "{name}\thold\t{why}");
            continue;
        }
        ran += 1;
        match start(&name, None) {
            Ok(said) => {
                let _ = writeln!(report, "{name}\tran\t{}", said.lines().next().unwrap_or(""));
            }
            // A beat that stopped at the first broken flow would let one
            // failure hold back every other schedule on the machine.
            Err(complaint) => {
                let _ = writeln!(
                    report,
                    "{name}\tbroke\t{}",
                    complaint.lines().next().unwrap_or("")
                );
            }
        }
    }
    // A flow that failed three times in a row owes the register a line, and
    // the fault writer is started here with that flow as its mandate. The
    // failed run is remembered whatever the start came to: a writer that
    // cannot run says so once, instead of being asked again at every beat.
    for fault in flow::faults_due(&glance.streaks, &glance.faults_written) {
        ran += 1;
        let (word, said) = match start(flow::system::FAULT_WRITER, Some(&fault.flow)) {
            Ok(said) => ("ran", said),
            Err(complaint) => ("broke", complaint),
        };
        let _ = writeln!(
            report,
            "{}\t{word}\t{}",
            flow::system::FAULT_WRITER,
            catalogue::say(
                "cli.flow.beat_writes_fault",
                &[
                    ("flow", &fault.flow),
                    ("times", &fault.length.to_string()),
                    ("said", said.lines().next().unwrap_or("")),
                ],
            )
        );
        if let Some(ledger) = &glance.ledger {
            ledger
                .remember_fault_written(
                    &fault.flow,
                    &fault.run_id,
                    seat_of(std::env::var_os("SAILOR_TERMINAL").is_some()),
                    now,
                )
                .map_err(|error| error.to_string())?;
        }
    }
    let _ = write!(report, "{ran} run, {held} held");
    Ok(report)
}

pub(super) fn due_flows(sources: &[FlowSource]) -> Result<String, String> {
    let known = known_flows(sources);
    if known.is_empty() {
        return Ok(nothing_found(sources));
    }
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);
    let last = last_runs()
        .read_or_say_it_could_not("cli.flow.due_could_not_look")?
        .last_started;

    let mut report = String::new();
    let mut due = 0usize;
    let mut unplanned = 0usize;
    for (name, _, entry) in known {
        let Ok(flow) = entry else {
            let _ = writeln!(
                report,
                "{name}\t{}",
                catalogue::say("cli.flow.will_not_load", &[])
            );
            continue;
        };
        let Some(schedule) = flow.schedule.as_ref() else {
            unplanned += 1;
            continue;
        };
        let last_run = last.get(&flow.id).copied();
        let verdict = if flow::is_due(schedule, last_run, now) {
            due += 1;
            catalogue::say("cli.flow.due", &[])
        } else {
            catalogue::say("cli.flow.not_yet", &[])
        };
        let when = match last_run {
            Some(seconds) => catalogue::say(
                "cli.flow.last_run_minutes_ago",
                &[("minutes", &((now - seconds) / 60).to_string())],
            ),
            None => catalogue::say("cli.flow.never_run", &[]),
        };
        let _ = writeln!(report, "{}\t{verdict}\t{when}", flow.id);
    }
    let _ = write!(
        report,
        "{}\n{}",
        catalogue::say(
            "cli.flow.due_now_and_unplanned",
            &[
                ("due", &due.to_string()),
                ("unplanned", &unplanned.to_string()),
            ],
        ),
        waiting_report()
    );
    Ok(report)
}

#[cfg(test)]
mod tests {
    use super::super::{dispatch, now_secs, USAGE};
    use super::*;
    use std::fs;

    // ── the beat ─────────────────────────────────────────────────────────

    /// A form the help promises and the dispatch refuses is a form discovered
    /// by whoever types it.
    #[test]
    fn the_beat_is_promised_and_accepted() {
        assert!(
            USAGE.iter().any(|line| line.form.contains("flow tick")),
            "the help must promise the beat"
        );
        let nowhere: Vec<FlowSource> = Vec::new();
        let said = dispatch(&["tick".to_owned()], &nowhere)
            .expect("«tick» must reach its own arm, not the usage error");
        assert!(said.contains("no flow found"), "{said}");
    }

    /// A beat with nothing due must still say so. The relay this replaces
    /// declined 2,803 times out of 2,834 without leaving a trace, and that is
    /// what made a working guard indistinguishable from a broken one.
    #[test]
    fn a_beat_that_does_nothing_still_says_what_it_held_and_why() {
        let scratch = std::env::temp_dir().join(format!("sailor-battito-{}", std::process::id()));
        let _ = fs::remove_dir_all(&scratch);
        fs::create_dir_all(&scratch).expect("create the test directory");
        let flow = scratch.join("senza-orario.flow.json");
        fs::write(
            &flow,
            r#"{"id":"senza-orario","description":"un flusso senza pianificazione",
                "graph":{"steps":[{"id":"innesco","deps":[],"action":"trigger","max_attempts":1,
                "when":null,"with":{"source":"manual"},
                "input_schema":{"type":"any"},"output_schema":{"type":"any"}}]},"inputs":{}}"#,
        )
        .expect("write a flow with no schedule");

        let sources = vec![FlowSource {
            origin: "prova",
            dir: scratch.clone(),
        }];
        // The last runs are handed in rather than read: a test that opens the
        // ledger of whoever runs it is fault 5, and here it would also decide
        // the answer.
        let said = tick_flows_with(&sources, LastRuns::NothingHasRunYet, &mut never_starts)
            .expect("a beat over one flow works");
        assert!(
            said.contains("senza-orario\thold\tno schedule"),
            "a held flow must name itself and say why: {said}"
        );
        assert!(said.contains("0 run, 1 held"), "{said}");
        let _ = fs::remove_dir_all(&scratch);
    }

    /// A beat that could not read the ledger must not answer like a beat that
    /// found nothing due: without the last runs every scheduled flow reads as
    /// never run, so the blind beat would fire everything and call it a
    /// schedule. It is fault 12 in another suit — a `sense` that cannot tell
    /// zero from "I could not look".
    #[test]
    fn a_beat_that_could_not_read_the_ledger_says_so_instead_of_deciding() {
        let scratch = std::env::temp_dir().join(format!("sailor-cieco-{}", std::process::id()));
        let _ = fs::remove_dir_all(&scratch);
        fs::create_dir_all(&scratch).expect("create the test directory");
        fs::write(
            scratch.join("ogni-minuto.flow.json"),
            r#"{"id":"ogni-minuto","description":"un flusso a intervallo",
                "schedule":{"recurrence":{"kind":"every_seconds","seconds":60},"weight":"light"},
                "graph":{"steps":[{"id":"innesco","deps":[],"action":"trigger","max_attempts":1,
                "when":null,"with":{"source":"manual","text":"vai"},
                "input_schema":{"type":"any"},"output_schema":{"type":"any"}}]},"inputs":{}}"#,
        )
        .expect("write a scheduled flow");
        let sources = vec![FlowSource {
            origin: "prova",
            dir: scratch.clone(),
        }];

        let complaint = tick_flows_with(
            &sources,
            LastRuns::CouldNotLook("unable to open database file".to_owned()),
            &mut never_starts,
        )
        .expect_err("a blind beat is not a beat that did nothing");
        assert!(
            complaint.contains("unable to open database file"),
            "the beat must carry why it could not look: {complaint}"
        );
        assert!(
            !complaint.contains("0 run"),
            "and must not read as a count: {complaint}"
        );

        // The same flow, with a ledger that answers: the beat decides instead
        // of refusing. Without this half the test above would pass on a beat
        // that complains every time. The last run is now, so nothing is started
        // and this test touches no real ledger.
        let just_ran = Glance {
            last_started: BTreeMap::from([("ogni-minuto".to_owned(), now_secs().unwrap_or(0))]),
            ..Glance::default()
        };
        let said = tick_flows_with(&sources, LastRuns::Read(just_ran), &mut never_starts)
            .expect("a beat that could look works");
        assert!(said.contains("ogni-minuto\thold\tnot due"), "{said}");
        assert!(said.contains("0 run, 1 held"), "{said}");
        let _ = fs::remove_dir_all(&scratch);
    }

    /// A starter for beats that must not start anything: the test is about
    /// what the beat holds, and a start would be the defect.
    fn never_starts(name: &str, _: Option<&str>) -> Result<String, String> {
        panic!("the beat started {name}")
    }

    fn a_closed_run(flow: &str, run_id: &str, status: &str, started_at: i64) -> ledger::RunRecord {
        ledger::RunRecord {
            run_id: run_id.to_owned(),
            kind: "flow".to_owned(),
            entity: flow.to_owned(),
            parent_run_id: None,
            started_by: "test".to_owned(),
            status: status.to_owned(),
            total_cost_micros: 0,
            error: None,
            started_at,
            ended_at: Some(started_at + 1),
            worktree: None,
        }
    }

    /// **A FLOW THAT FAILS THREE TIMES IN A ROW WRITES A FAULT BY ITSELF.**
    /// The beat sees the streak in the ledger, starts the fault writer with
    /// that flow as its mandate, says so in its report, and remembers the
    /// failed run so the next beat does not ask twice. The fault writer's own
    /// failures never start it about itself.
    #[test]
    fn a_flow_that_failed_three_times_in_a_row_has_its_fault_written_once() {
        let scratch = std::env::temp_dir().join(format!("sailor-guasto-{}", std::process::id()));
        let _ = fs::remove_dir_all(&scratch);
        fs::create_dir_all(&scratch).expect("create the test directory");
        fs::write(
            scratch.join("ogni-minuto.flow.json"),
            r#"{"id":"ogni-minuto","description":"un flusso a intervallo",
                "schedule":{"recurrence":{"kind":"every_seconds","seconds":60},"weight":"light"},
                "graph":{"steps":[{"id":"innesco","deps":[],"action":"trigger","max_attempts":1,
                "when":null,"with":{"source":"manual","text":"vai"},
                "input_schema":{"type":"any"},"output_schema":{"type":"any"}}]},"inputs":{}}"#,
        )
        .expect("write a scheduled flow");
        let sources = vec![FlowSource {
            origin: "prova",
            dir: scratch.clone(),
        }];
        let now = now_secs().unwrap_or(0);
        let ledger = Ledger::open(scratch.join("ledger")).expect("a scratch ledger");
        for (run, at) in [("ogni-minuto-1", now - 30), ("ogni-minuto-2", now - 20), ("ogni-minuto-3", now - 10)] {
            ledger
                .record_run(&a_closed_run("ogni-minuto", run, "failed", at))
                .expect("a failed run");
        }
        for (run, at) in [("writer-1", now - 30), ("writer-2", now - 20), ("writer-3", now - 10)] {
            ledger
                .record_run(&a_closed_run(flow::system::FAULT_WRITER, run, "failed", at))
                .expect("a failed run of the fault writer");
        }

        let mut started: Vec<(String, Option<String>)> = Vec::new();
        let said = tick_flows_with(
            &sources,
            LastRuns::Read(glance_at(&ledger).expect("a glance")),
            &mut |name, mandate| {
                started.push((name.to_owned(), mandate.map(str::to_owned)));
                Ok(format!("flow {name} complete; run {name}-77"))
            },
        )
        .expect("a beat over a failing flow works");
        assert_eq!(
            started,
            vec![(flow::system::FAULT_WRITER.to_owned(), Some("ogni-minuto".to_owned()))],
            "{said}"
        );
        assert!(said.contains("ogni-minuto\thold\tnot due"), "{said}");
        assert!(
            said.contains("write-down-what-broke\tran\t«ogni-minuto» failed 3 runs in a row"),
            "the report must say which flow, how often, and that the fault is being written: {said}"
        );
        assert!(said.contains("write-down-what-broke-77"), "and name the run: {said}");
        assert!(said.contains("1 run, 1 held"), "{said}");
        assert_eq!(
            ledger.faults_written().expect("the memory"),
            BTreeSet::from(["ogni-minuto-3".to_owned()])
        );

        // The next beat over the same ledger owes nothing: the fault is written.
        let said = tick_flows_with(
            &sources,
            LastRuns::Read(glance_at(&ledger).expect("a glance")),
            &mut never_starts,
        )
        .expect("a beat after the fault works");
        assert!(said.contains("0 run, 1 held"), "{said}");
        drop(ledger);
        let _ = fs::remove_dir_all(&scratch);
    }
}
