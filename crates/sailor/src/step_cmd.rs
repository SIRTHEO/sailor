//! `sailor step`: taking on and closing a step a flow handed to an agent that is
//! already alive.
//!
//! **FOUR VERBS.** `open` takes the work on and `close` says how it went; the
//! work in between is not Sailor's to run. `approve` and `reject` are a
//! person's verdict in one gesture, and a rejection stops the run.
//!
//! **THE FIRST WEAKNESS, DECLARED RATHER THAN HIDDEN: `--as <who>` IS A NAME
//! CHOSEN BY WHOEVER WRITES IT.** The refusal below applies «the author does not
//! judge», but on a freely chosen name it is **a lock with the key in the
//! excluded party's pocket**: `--as someone-else` and it never fires. Sailor has
//! no session id to read, and no store field can say who is typing; until it has
//! one, this holds against distraction, not against anyone set on evading it.

use crate::Form;
use actions::handoff::{holder_key, HOLDER_COLLECTION};
use flow::{Completion, Decision, FlowFile, InProcessExecutor, Outcome, StepRecord};
use ledger::{EngineIdentity, Ledger, ModelCallRecord, StoreRecord};
use serde_json::Value;
use std::collections::BTreeMap;
use std::fmt::Write as _;

pub fn run(args: &[String]) -> i32 {
    match dispatch(args) {
        Ok(message) => {
            println!("{message}");
            0
        }
        Err(message) => {
            eprintln!("sailor step: {message}");
            1
        }
    }
}

fn dispatch(args: &[String]) -> Result<String, String> {
    match args.first().map(String::as_str) {
        Some("open") => open_step(&flags(&args[1..])?),
        Some("close") => close_step(&flags(&args[1..])?),
        Some("approve") => decide_step(&flags(&args[1..])?, Verdict::Approved),
        Some("reject") => decide_step(&flags(&args[1..])?, Verdict::Rejected),
        _ => Err(usage()),
    }
}

/// The forms of `sailor step`, one per line. See `flow_cmd::USAGE`.
pub const USAGE: &[Form] = &[
    Form {
        form: "sailor step open --run <run> --step <step> --as <who>",
        says_key: "",
    },
    Form {
        form: "sailor step close --run <run> --step <step> --as <who> --outcome <went|broke> [--output-file <file>] [--turns <n>] [--said <text>]",
        says_key: "",
    },
    Form {
        form: "sailor step approve --run <run> --step <step> --as <who> [--why <text>] [--output-file <file>] [--turns <n>]",
        says_key: "cli.step.form.approve",
    },
    Form {
        form: "sailor step reject --run <run> --step <step> --as <who> --why <text>",
        says_key: "cli.step.form.reject",
    },
];

fn usage() -> String {
    format!(
        "{}\n  {}",
        catalogue::say("cli.usage_heading", &[]),
        crate::forms_as_lines(USAGE).join("\n  ")
    )
}

/// The options written on the line, in `--name value` pairs.
///
/// **AN OPTION WITH NO VALUE IS AN ERROR, NOT AN EMPTY.** `--step --as who`
/// would take `--as` as the step id and hunt for a step that does not exist,
/// with a message sending the reader to the flow instead of the command line.
fn flags(args: &[String]) -> Result<BTreeMap<String, String>, String> {
    let mut found = BTreeMap::new();
    let mut rest = args.iter();
    while let Some(name) = rest.next() {
        let Some(name) = name.strip_prefix("--") else {
            return Err(format!(
                "{}; {}",
                catalogue::say("cli.not_something_i_know", &[("word", name)]),
                usage()
            ));
        };
        let value = rest.next().ok_or_else(|| {
            catalogue::say(
                "cli.option_wants_a_value",
                &[("option", &format!("--{name}"))],
            )
        })?;
        if let Some(other) = value.strip_prefix("--") {
            return Err(catalogue::say(
                "cli.value_is_another_option",
                &[("option", name), ("given", other)],
            ));
        }
        found.insert(name.to_owned(), value.clone());
    }
    Ok(found)
}

fn required<'a>(found: &'a BTreeMap<String, String>, name: &str) -> Result<&'a str, String> {
    found.get(name).map(String::as_str).ok_or_else(|| {
        format!(
            "{}; {}",
            catalogue::say("cli.option_missing", &[("option", &format!("--{name}"))]),
            usage()
        )
    })
}

// ── taking it on ─────────────────────────────────────────────────────────

/// Opens a fresh attempt on a handed step, in the name of whoever takes it.
/// **THE INPUT IS COPIED AS IS, AND THAT IS NOT LAZINESS.** `input_digest` comes
/// from the input: an identical copy makes the digests match, so
/// `flow::attempt_relation` says `SameInput` — the same work resumed, and true.
/// Rebuilding it, or writing the taker in, would say `DifferentInput`: two jobs
/// where there is one, and the mandate re-fingerprinted without being changed.
fn open_step(found: &BTreeMap<String, String>) -> Result<String, String> {
    let ledger = open_ledger()?;
    open_step_in(&ledger, found)
}

/// The body of `open`, with the store declared rather than derived from `HOME`.
///
/// **SEPARATE, OR IT CANNOT BE TESTED.** `ledger::default_directory` reads an
/// environment variable, global to the process: a test that wrote it would spoil
/// the others at random, and whoever saw the red would look at the wrong module.
pub fn open_step_in(ledger: &Ledger, found: &BTreeMap<String, String>) -> Result<String, String> {
    let run_id = required(found, "run")?;
    let step_id = required(found, "step")?;
    let holder = required(found, "as")?;

    let records = ledger.steps(run_id).map_err(|error| {
        catalogue::say(
            "cli.step.cannot_read_run",
            &[("run_id", run_id), ("error", &error.to_string())],
        )
    })?;
    let latest = last_attempt(&records, step_id).ok_or_else(|| {
        catalogue::say(
            "cli.step.no_step_called",
            &[("run_id", run_id), ("step_id", step_id)],
        )
    })?;

    // **ONLY WHAT IS WAITING IS OPENED.** A step gone, broken or still open was
    // handed to nobody: opening it again would redo work the engine is doing, or
    // undo an outcome.
    match latest.outcome {
        Some(Outcome::Waiting) => {}
        None => {
            return Err(catalogue::say(
                "cli.step.already_open",
                &[("step_id", step_id), ("run_id", run_id)],
            ))
        }
        Some(other) => {
            return Err(catalogue::say(
                "cli.step.not_waiting",
                &[("step_id", step_id), ("outcome", &format!("{other:?}"))],
            ))
        }
    }

    refuse_the_author_as_judge(ledger, run_id, step_id, holder, latest)?;

    let now = now_secs()?;
    let mut started = StepRecord::started(
        run_id,
        step_id,
        latest.attempt + 1,
        latest.epoch + 1,
        latest.deps.clone(),
        latest.input.clone(),
        latest.gates.clone(),
        now,
    );
    started.attempt_relation = flow::attempt_relation(&records, &started);
    // **NO PID, AND THE EMPTY IS AN ASSERTION.** This field names «the process
    // holding the step», and here no process holds it: an agent in a terminal
    // does, child of nothing and indistinguishable to the kernel. The pid of
    // *this* command would be a convenient lie — it exits at once, so a resume
    // would read that pid as dead and close the handoff under the agent at work.
    // What holds a handed step is a deadline in the record, not a process.
    started.held_by_pid = None;
    // **WHO TOOK IT ON IS WRITTEN HERE, NOT ONLY AT CLOSE.** Until this line the
    // store learnt the name in `HOLDER_COLLECTION` when the step was closed, so
    // between open and close nothing said who was working: no other terminal
    // could ask, and `close` could not tell one taker from another.
    started.taken_on_by = Some(holder.to_owned());
    // The species stays as frozen at the handoff: an action rewritten in the
    // meantime must not change the verdict on a step already offered.
    started.species = latest.species;
    ledger.append_step_started(&started).map_err(|error| {
        catalogue::say(
            "cli.step.cannot_open_step",
            &[("step_id", step_id), ("error", &error.to_string())],
        )
    })?;

    // **THE MANDATE IS READ FROM THE INPUT, NEVER FROM `said`.** `said` holds
    // the short line, cut at 16 KB; the work in full is in the input, which the
    // store keeps whole. Reading the wrong side would hand the taker a mandate
    // truncated mid-sentence, and never say so.
    let mandate = started
        .input
        .get("mandate")
        .and_then(Value::as_str)
        .unwrap_or("<this step carries no written brief>");
    Ok(catalogue::say(
        "cli.step.taken_on",
        &[
            ("step_id", step_id),
            ("holder", holder),
            ("run_id", run_id),
            ("attempt", &started.attempt.to_string()),
            ("mandate", mandate),
        ],
    ))
}

/// **THE AUTHOR DOES NOT JUDGE, APPLIED TO DEPENDENCIES.** Whoever closed a step
/// this one depends on is its author; leaving them the step that judges that work
/// is self-review, and the permanent constraint does not say «usually».
/// **ASKED TWICE, AT OPEN AND AT CLOSE**: at open alone the door would stay as
/// wide — open under any name, **close** under the author's — and the close is
/// where a verdict is written; at open it stops early, which costs less.
/// **DENIAL IS THE DEFAULT**: a forgotten allow-list passes everything, while a
/// forgotten denial at worst stops a job and shows at once. The flow declares
/// the permission, step by step, with `same_holder_ok`.
fn refuse_the_author_as_judge(
    ledger: &Ledger,
    run_id: &str,
    step_id: &str,
    holder: &str,
    record: &StepRecord,
) -> Result<(), String> {
    let same_holder_ok = record
        .input
        .get("same_holder_ok")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    if same_holder_ok {
        return Ok(());
    }
    for dependency in &record.deps {
        let written = ledger
            .read_record(HOLDER_COLLECTION, &holder_key(run_id, dependency))
            .map_err(|error| {
                catalogue::say(
                    "cli.step.cannot_read_who_closed",
                    &[("dependency", dependency), ("error", &error.to_string())],
                )
            })?;
        if written.is_some_and(|found| found.written_by == holder) {
            return Err(catalogue::say(
                "cli.step.author_would_judge",
                &[
                    ("holder", holder),
                    ("dependency", dependency),
                    ("step_id", step_id),
                ],
            ));
        }
    }
    Ok(())
}

/// **A CLOSE LANDS ON THE ATTEMPT ITS AUTHOR OPENED, OR ON NONE.** `close_in`
/// takes the newest open record, and a handed step is held by a deadline: once
/// that deadline puts the step back among the ready and somebody else takes it
/// on, the first taker's close would land on the second taker's attempt and
/// write a result into work still being done.
///
/// A record with no name was written before the field. It passes: refusing
/// every step opened before this code would stop live work to punish an old
/// row, and the field is written at every open from now on.
fn refuse_a_stranger_to_this_attempt(
    run_id: &str,
    step_id: &str,
    holder: &str,
    open: &StepRecord,
) -> Result<(), String> {
    match open.taken_on_by.as_deref() {
        Some(taker) if taker != holder => Err(catalogue::say(
            "cli.step.taken_on_by_somebody_else",
            &[
                ("step_id", step_id),
                ("run_id", run_id),
                ("taker", taker),
                ("holder", holder),
                ("attempt", &open.attempt.to_string()),
            ],
        )),
        _ => Ok(()),
    }
}

// ── closing ──────────────────────────────────────────────────────────────

/// Closes the open step, validating the output against the step's schema.
/// **THE VALIDATION IS HERE BECAUSE `RecordStore::close` DOES NOT DO IT.** The
/// engine validates in `run_one`, before closing; this command goes around the
/// engine, and with no check a malformed output would enter the store as good.
/// The damage would show three steps later, when the step depending on this one
/// gets an input its schema refuses — and the blame would fall on it.
fn close_step(found: &BTreeMap<String, String>) -> Result<String, String> {
    // Outcomes are checked **before** the store is opened: a typo on the command
    // line must not cost an open file, and the message it yields speaks of the
    // line, not of the machine.
    let _ = declared_outcome(found)?;
    let ledger = open_ledger()?;
    let run_id = required(found, "run")?;
    let flow = flow_of_run(&ledger, run_id)?;
    close_step_in(&ledger, &flow, found)
}

/// The outcome whoever closes declares.
///
/// **TWO ONLY, AND IT IS NO SIMPLIFICATION.** `Waiting`, `Skipped` and `Stopped`
/// are the engine's words for things that happened to it: a handed step that
/// «skips» or «waits» would be a sentence with no referent, leaving the run in a
/// state no resume knows how to unblock.
fn declared_outcome(found: &BTreeMap<String, String>) -> Result<Outcome, String> {
    match required(found, "outcome")? {
        "went" => Ok(Outcome::Went),
        "broke" => Ok(Outcome::Broke),
        other => Err(catalogue::say(
            "cli.step.outcome_not_declarable",
            &[("outcome", other)],
        )),
    }
}

/// The body of `close`, with the store and the flow declared, not derived.
pub fn close_step_in(
    ledger: &Ledger,
    flow: &FlowFile,
    found: &BTreeMap<String, String>,
) -> Result<String, String> {
    let closing = Closing {
        outcome: declared_outcome(found)?,
        verdict: None,
        why: None,
    };
    close_in(ledger, flow, found, closing)
}

/// How a step is being closed: the outcome the store keeps, and the verdict a
/// person declared when the close is one of theirs.
///
/// `close` and the two decisions share every refusal — the author who would
/// judge, the process still holding the step, the output the schema rejects —
/// and a second copy of that body would let one of the three drift silently.
struct Closing<'a> {
    outcome: Outcome,
    verdict: Option<Verdict>,
    why: Option<&'a str>,
}

fn close_in(
    ledger: &Ledger,
    flow: &FlowFile,
    found: &BTreeMap<String, String>,
    closing: Closing<'_>,
) -> Result<String, String> {
    let run_id = required(found, "run")?;
    let step_id = required(found, "step")?;
    let holder = required(found, "as")?;
    let outcome = closing.outcome;

    let records = ledger.steps(run_id).map_err(|error| {
        catalogue::say(
            "cli.step.cannot_read_run",
            &[("run_id", run_id), ("error", &error.to_string())],
        )
    })?;
    // It finds the open record itself: whoever closes need not know which
    // attempt it reached, and asking would be a number to get wrong.
    let open = records
        .iter()
        .filter(|record| record.step_id == step_id && record.outcome.is_none())
        .max_by_key(|record| (record.attempt, record.epoch))
        .ok_or_else(|| {
            catalogue::say(
                "cli.step.none_open",
                &[("run_id", run_id), ("step_id", step_id)],
            )
        })?;

    // **ONLY WHAT `sailor step open` OPENED IS CLOSED.** Closing a step another
    // process holds pulls it out from under it; a holder the kernel denies holds
    // nothing, and its step would otherwise stay open for ever.
    if open.held_by_pid.is_some() && ledger::still_held(open) != ledger::StillHeld::Released {
        return Err(catalogue::say(
            "cli.step.held_by_the_engine",
            &[
                ("step_id", step_id),
                ("pid", &open.held_by_pid.unwrap_or_default().to_string()),
                ("run_id", run_id),
            ],
        ));
    }

    refuse_a_stranger_to_this_attempt(run_id, step_id, holder, open)?;

    // The refusal matters most here: the close is the gesture that writes a
    // verdict, and a verdict on one's own work is worth nothing.
    refuse_the_author_as_judge(ledger, run_id, step_id, holder, open)?;

    let step = flow.graph.step(step_id).ok_or_else(|| {
        catalogue::say(
            "cli.step.flow_declares_no_such_step",
            &[("flow", &flow.id), ("step_id", step_id)],
        )
    })?;

    let output = match outcome {
        Outcome::Went => match found.get("output-file") {
            // Closing `went` with no output while another step depends on this
            // one would stop the run there, with «no typed output», on a step
            // that is not at fault. The log keeps a null output apart from no
            // output (fault 33); the refusal stays because a defect should show
            // where it is born, not three steps later.
            None => {
                let waiting_on_it = dependents_of(flow, step_id);
                if !waiting_on_it.is_empty() {
                    return Err(catalogue::say(
                        "cli.step.would_close_without_output",
                        &[
                            ("step_id", step_id),
                            ("dependents", &waiting_on_it.join(", ")),
                        ],
                    ));
                }
                None
            }
            Some(path) => {
                let text = std::fs::read_to_string(path).map_err(|error| {
                    catalogue::say(
                        "cli.step.cannot_read_file",
                        &[("path", path), ("error", &error.to_string())],
                    )
                })?;
                let value: Value = serde_json::from_str(&text).map_err(|error| {
                    catalogue::say(
                        "cli.step.not_valid_json",
                        &[("path", path), ("error", &error.to_string())],
                    )
                })?;
                step.output_schema.validate(&value).map_err(|error| {
                    catalogue::say(
                        "cli.step.output_breaks_the_schema",
                        &[("step_id", step_id), ("error", &error.to_string())],
                    )
                })?;
                Some(value)
            }
        },
        // A broken step has no output to validate: it produced none.
        _ => None,
    };

    let now = now_secs()?;
    let attempt = open.attempt;
    let epoch = open.epoch;
    ledger
        .close_step(
            run_id,
            step_id,
            attempt,
            epoch,
            Completion {
                outcome,
                output,
                said: closing
                    .why
                    .map(str::to_owned)
                    .or_else(|| found.get("said").cloned()),
                failure_class: match (outcome, closing.verdict) {
                    // A rejected step did not break: somebody read it and
                    // refused it, and the two are counted apart.
                    (Outcome::Broke, Some(Verdict::Rejected)) => {
                        Some(REJECTED_BY_A_PERSON.to_owned())
                    }
                    (Outcome::Broke, _) => Some("handed_back".to_owned()),
                    _ => None,
                },
                refusal: None,
                ran: None,
                ended_at: now,
                bytes_seen: None,
                bytes_discarded: None,
            },
        )
        .map_err(|error| {
            catalogue::say(
                "cli.step.cannot_close_step",
                &[("step_id", step_id), ("error", &error.to_string())],
            )
        })?;

    // Who closed it stays written: `open` rereads it on the step depending on
    // this one, to refuse a judge who is also the author. **The decision goes in
    // the same entry**, never a second one: two rows about one gesture come
    // apart, and the reader has no way to tell which of them lied.
    let mut written = serde_json::Map::new();
    written.insert(
        "outcome".to_owned(),
        Value::String(format!("{outcome:?}").to_lowercase()),
    );
    if let Some(verdict) = closing.verdict {
        written.insert(
            "decision".to_owned(),
            Value::String(verdict.as_text().to_owned()),
        );
    }
    if let Some(why) = closing.why {
        written.insert("why".to_owned(), Value::String(why.to_owned()));
    }
    ledger
        .put_record(&StoreRecord {
            collection: HOLDER_COLLECTION.to_owned(),
            key: holder_key(run_id, step_id),
            value: Value::Object(written),
            written_by: holder.to_owned(),
            written_at: now,
        })
        .map_err(|error| {
            catalogue::say(
                "cli.step.cannot_record_who_closed",
                &[("step_id", step_id), ("error", &error.to_string())],
            )
        })?;

    let mut report = match closing.verdict {
        Some(verdict) => the_decision_read_back(verdict, run_id, step_id, holder, closing.why),
        None => catalogue::say(
            "cli.step.closed",
            &[
                ("step_id", step_id),
                ("holder", holder),
                (
                    "outcome",
                    &catalogue::say(
                        match outcome {
                            Outcome::Went => "cli.step.outcome.went",
                            _ => "cli.step.outcome.broke",
                        },
                        &[],
                    ),
                ),
            ],
        ),
    };

    if let Some(turns) = found.get("turns") {
        let turns: u64 = turns
            .parse()
            .map_err(|_| catalogue::say("cli.step.turns_not_a_number", &[("turns", turns)]))?;
        write_self_declared_turns(ledger, run_id, step_id, holder, turns, now)?;
        let _ = write!(
            report,
            "\n{}",
            catalogue::say(
                "cli.step.self_declared_turns",
                &[("turns", &turns.to_string())]
            )
        );
    }

    // A rejection is the end of the run, so there is nothing to say about what
    // comes next: what it owes the reader is that nothing does, and why.
    if closing.verdict == Some(Verdict::Rejected) {
        let why = closing.why.unwrap_or_default();
        stop_the_run(ledger, flow, run_id, step_id, holder, why, now)?;
        let _ = write!(
            report,
            "\n{}",
            catalogue::say("cli.step.run_stopped_by_the_rejection", &[])
        );
        return Ok(report);
    }

    // **WHAT IS READY NOW, AND THE LINE TO RESUME.** Whoever closes a step by
    // hand has no graph in front of them: without this they would open it to
    // learn whether they unblocked anything — leaving Sailor to interrogate it.
    let decision = InProcessExecutor
        .decision(&flow.graph, run_id, ledger, &flow::SystemClock)
        .map_err(|error| {
            catalogue::say(
                "cli.step.cannot_work_out_what_is_ready",
                &[("error", &error.to_string())],
            )
        })?;
    let _ = write!(
        report,
        "\n{}",
        what_comes_next(&decision, run_id, now_secs()?)
    );
    Ok(report)
}

/// What can be done now, told to whoever has just closed.
///
/// The clock comes from outside: a postponed step is told as how long is left,
/// and a function reading the clock itself could not be interrogated.
fn what_comes_next(decision: &Decision, run_id: &str, now: i64) -> String {
    match decision {
        Decision::Ready(steps) => catalogue::say(
            "cli.step.ready_now",
            &[("steps", &steps.join(", ")), ("run_id", run_id)],
        ),
        Decision::Waiting(steps) => catalogue::say(
            "cli.step.waiting_for_someone",
            &[("steps", &steps.join(", ")), ("run_id", run_id)],
        ),
        Decision::NotYet { steps, due_at } => catalogue::say(
            "cli.step.not_yet",
            &[
                ("steps", &steps.join(", ")),
                ("seconds", &(due_at - now).max(0).to_string()),
                ("run_id", run_id),
            ],
        ),
        Decision::Running(steps) => {
            catalogue::say("cli.step.still_running", &[("steps", &steps.join(", "))])
        }
        Decision::Stopped(steps) => catalogue::say(
            "cli.step.stopped_in_the_store",
            &[("steps", &steps.join(", "))],
        ),
        Decision::Failed(steps) => catalogue::say(
            "cli.step.broken_past_the_attempts",
            &[("steps", &steps.join(", "))],
        ),
        Decision::CapReached(stop) => registry::why_it_stopped(stop),
        Decision::Halted {
            reason,
            not_started,
        } => registry::why_it_halted(*reason, not_started),
        Decision::Complete => catalogue::say("cli.step.run_complete", &[]),
    }
}

/// Writes a **self-declared** consumption row, marked as such.
///
/// **THE SECOND WEAKNESS: ON A FLOW WITH HANDOFFS THE SPEND CAP STOPS BEING A
/// GUARANTEE.** The cap (`FlowFile::spend_cap_micros`) is measured on the
/// `model_calls` rows, which the engine wrote from what the provider declares;
/// this one is written by **whoever did the work**, on a number they counted
/// themselves, and is verifiable nowhere. So `cost_micros` stays `None` and is
/// no invented figure: the row lands in `Spend::calls_without_cost`,
/// `Spend::is_complete()` turns false, and every place showing the cap already
/// says the real spend is higher. An estimate would be worse than the empty — it
/// would make *complete* a sum that is not, and the cap would fire on an invented
/// figure with nobody able to notice.
fn write_self_declared_turns(
    ledger: &Ledger,
    run_id: &str,
    step_id: &str,
    holder: &str,
    turns: u64,
    now: i64,
) -> Result<(), String> {
    ledger
        .record_model_call(&ModelCallRecord {
            call_id: format!("{run_id}:{step_id}:{now}:handed"),
            run_id: run_id.to_owned(),
            step_id: Some(step_id.to_owned()),
            // No session: this step opens no process — the work goes to an
            // agent already alive in the terminal, whose conversation is not
            // Sailor's and cannot be resumed from here.
            session_id: None,
            // And nothing was asked of one either: this step never reached for
            // a session, so there is no fallback to confess.
            session_mode: None,
            work_kind: None,
            fell_back_from: Vec::new(),
            // The row's reason lives in `purpose`: whoever sums a run's calls
            // must be able to separate what the engine measured from what
            // someone declared about themselves.
            purpose: "handed_to_agent:self_declared".to_owned(),
            cli: holder.to_owned(),
            requested_model: String::new(),
            actual_model: String::new(),
            input_tokens: None,
            output_tokens: None,
            cached_tokens: None,
            cache_write_tokens: None,
            cache_write_long_tokens: None,
            total_tokens: None,
            turns: Some(turns),
            // See the comment above: the empty is the honest part of this row.
            cost_micros: None,
            declared_cost_micros: None,
            price_currency: None,
            input_price_micros_per_million: None,
            output_price_micros_per_million: None,
            cached_price_micros_per_million: None,
            cache_write_price_micros_per_million: None,
            cache_write_long_price_micros_per_million: None,
            // **DECLARED BY AN AGENT, AND NOT «NONE».** Sailor started nothing
            // here: the agent already alive in the terminal did the work, and
            // under which identity Sailor does not and cannot know. An empty
            // would confuse that with «no profile in force», a different thing:
            // there the house is the process's, and can be gone and looked at.
            engine_identity: EngineIdentity::DeclaredByAnAgent,
            retry_chain: Vec::new(),
            error_type: None,
            started_at: now,
            ended_at: Some(now),
        })
        .map_err(|error| {
            catalogue::say(
                "cli.step.cannot_record_turns",
                &[("error", &error.to_string())],
            )
        })
}

// ── deciding ─────────────────────────────────────────────────────────────

/// The failure class a rejected step carries, apart from the one a step that
/// was simply handed back gets.
const REJECTED_BY_A_PERSON: &str = "rejected_by_a_person";

/// What a person declared about a step that was waiting for them.
///
/// **NOT A SECOND SPELLING OF `--outcome`.** `close --outcome went` says the
/// work was done; `approve` says somebody looked at it and let it through, and
/// the store keeps the two apart — an approval nobody can tell from a close is
/// indistinguishable from a step nobody read.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Verdict {
    Approved,
    Rejected,
}

impl Verdict {
    /// The outcome the store keeps for this verdict.
    ///
    /// A rejection is `Broke` and not `Stopped`: `Stopped` is the engine's word
    /// for a step the store holds still, and a person declaring it would leave
    /// the run in a state no resume knows how to unblock. What says the run
    /// ended by a person's hand is the halt request, not the step's outcome.
    fn outcome(self) -> Outcome {
        match self {
            Self::Approved => Outcome::Went,
            Self::Rejected => Outcome::Broke,
        }
    }

    /// The word the store keeps beside who wrote it.
    fn as_text(self) -> &'static str {
        match self {
            Self::Approved => "approved",
            Self::Rejected => "rejected",
        }
    }
}

/// Approves or rejects a step that is waiting for a person.
fn decide_step(found: &BTreeMap<String, String>, verdict: Verdict) -> Result<String, String> {
    // The reason is demanded **before** the store is opened, like the outcome
    // of `close`: a line that cannot be honoured must not cost an open file.
    let _ = reason_for(found, verdict)?;
    let ledger = open_ledger()?;
    let run_id = required(found, "run")?;
    let flow = flow_of_run(&ledger, run_id)?;
    decide_step_in(&ledger, &flow, found, verdict)
}

/// Why the decision went that way. **A REJECTION HAS TO SAY IT**: read back a
/// week later, a refusal with no reason is a step nobody can act on and a run
/// nobody can restart in good conscience.
fn reason_for(found: &BTreeMap<String, String>, verdict: Verdict) -> Result<Option<&str>, String> {
    let given = found
        .get("why")
        .map(String::as_str)
        .filter(|why| !why.trim().is_empty());
    match (verdict, given) {
        (Verdict::Rejected, None) => Err(catalogue::say("cli.step.rejection_wants_a_reason", &[])),
        (_, given) => Ok(given),
    }
}

/// The body of a decision, with the store and the flow declared, not derived.
///
/// **TAKING IT ON IS PART OF DECIDING.** A step still waiting is opened here
/// through `open_step_in`, so a decision is one gesture and not two — and every
/// refusal that gesture carries, the author who would judge above all, is the
/// same code and not a copy of it.
pub fn decide_step_in(
    ledger: &Ledger,
    flow: &FlowFile,
    found: &BTreeMap<String, String>,
    verdict: Verdict,
) -> Result<String, String> {
    let why = reason_for(found, verdict)?;
    let run_id = required(found, "run")?;
    let step_id = required(found, "step")?;
    let records = ledger.steps(run_id).map_err(|error| {
        catalogue::say(
            "cli.step.cannot_read_run",
            &[("run_id", run_id), ("error", &error.to_string())],
        )
    })?;
    if last_attempt(&records, step_id)
        .is_some_and(|latest| latest.outcome == Some(Outcome::Waiting))
    {
        open_step_in(ledger, found)?;
    }
    close_in(
        ledger,
        flow,
        found,
        Closing {
            outcome: verdict.outcome(),
            verdict: Some(verdict),
            why,
        },
    )
}

/// The decision as whoever typed it reads it back.
fn the_decision_read_back(
    verdict: Verdict,
    run_id: &str,
    step_id: &str,
    holder: &str,
    why: Option<&str>,
) -> String {
    match verdict {
        Verdict::Rejected => catalogue::say(
            "cli.step.rejected",
            &[
                ("step_id", step_id),
                ("holder", holder),
                ("run_id", run_id),
                ("why", why.unwrap_or_default()),
            ],
        ),
        Verdict::Approved => {
            let mut said = catalogue::say(
                "cli.step.approved",
                &[("step_id", step_id), ("holder", holder), ("run_id", run_id)],
            );
            if let Some(why) = why {
                let _ = write!(
                    said,
                    "\n{}",
                    catalogue::say("cli.step.approved_because", &[("why", why)])
                );
            }
            said
        }
    }
}

/// Closes the run a rejection ended: a request the engine reads, and a header
/// a person reads without running anything.
///
/// **BOTH, AND NEITHER ALONE.** With the request only the run reads `waiting`,
/// still out for a decision that was made; with the header only, the next
/// resume opens the fronts the rejection meant to stop.
fn stop_the_run(
    ledger: &Ledger,
    flow: &FlowFile,
    run_id: &str,
    step_id: &str,
    holder: &str,
    why: &str,
    now: i64,
) -> Result<(), String> {
    ledger
        .request_halt(run_id, why, holder, now)
        .map_err(|error| {
            catalogue::say(
                "cli.step.cannot_ask_the_run_to_stop",
                &[("run_id", run_id), ("error", &error.to_string())],
            )
        })?;
    let header = ledger
        .run_header(run_id)
        .map_err(|error| {
            catalogue::say(
                "cli.step.cannot_read_run",
                &[("run_id", run_id), ("error", &error.to_string())],
            )
        })?
        .ok_or_else(|| catalogue::say("cli.step.no_such_run", &[("run_id", run_id)]))?;
    crate::flow_cmd::record_run(
        ledger,
        flow,
        run_id,
        "stopped",
        header.started_at,
        Some(now),
        Some(catalogue::say(
            "cli.step.run_rejected_by",
            &[("holder", holder), ("step_id", step_id), ("why", why)],
        )),
        Some(flow::StopReason::ByHand),
    )
}

// ── the common tools ─────────────────────────────────────────────────────

/// The steps that demand this one's typed output.
///
/// A dependency declared skippable does not count: that step knows how to go on
/// without it, which is the reason it is declared so.
fn dependents_of(flow: &FlowFile, step_id: &str) -> Vec<String> {
    flow.graph
        .steps()
        .iter()
        .filter(|other| {
            other.deps.iter().any(|dependency| dependency == step_id)
                && !flow.graph.dependency_is_skippable(&other.id, step_id)
        })
        .map(|other| other.id.clone())
        .collect()
}

/// The last attempt on a step, however it went.
fn last_attempt<'a>(records: &'a [StepRecord], step_id: &str) -> Option<&'a StepRecord> {
    records
        .iter()
        .filter(|record| record.step_id == step_id)
        .max_by_key(|record| (record.attempt, record.epoch))
}

/// The flow a run runs on, found again from the store.
///
/// **THE GRAPH IS NOT ASKED OF WHOEVER TYPES.** A `--flow` on the command line
/// would let the wrong name through, and a step's output would be validated
/// against another step's schema — the check saying yes while looking at the
/// wrong thing. The run knows where it came from: it is in `runs.entity`.
pub fn flow_of_run(ledger: &Ledger, run_id: &str) -> Result<FlowFile, String> {
    let header = ledger
        .run_header(run_id)
        .map_err(|error| {
            catalogue::say(
                "cli.step.cannot_read_run",
                &[("run_id", run_id), ("error", &error.to_string())],
            )
        })?
        .ok_or_else(|| catalogue::say("cli.step.no_such_run", &[("run_id", run_id)]))?;
    if header.entity.is_empty() {
        return Err(catalogue::say(
            "cli.step.run_declares_no_flow",
            &[("run_id", run_id)],
        ));
    }
    let sources = ui::gather::flow_sources();
    let known = ui::gather::load_all_flows(&sources);
    match known.iter().find(|(name, _, _)| *name == header.entity) {
        Some((_, _, Ok(flow))) => Ok(flow.clone()),
        Some((_, origin, Err(reason))) => Err(catalogue::say(
            "cli.step.flow_does_not_load",
            &[
                ("flow", &header.entity),
                ("origin", origin),
                ("run_id", run_id),
                ("reason", reason),
            ],
        )),
        None => Err(catalogue::say(
            "cli.step.flow_no_longer_found",
            &[("flow", &header.entity), ("run_id", run_id)],
        )),
    }
}

pub(crate) fn open_ledger() -> Result<Ledger, String> {
    let dir = ledger::default_directory().ok_or_else(|| catalogue::say("cli.no_home", &[]))?;
    Ledger::open(&dir).map_err(|error| {
        catalogue::say(
            "cli.step.cannot_open_the_store",
            &[
                ("path", &dir.display().to_string()),
                ("error", &error.to_string()),
            ],
        )
    })
}

fn now_secs() -> Result<i64, String> {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|elapsed| elapsed.as_secs() as i64)
        .map_err(|error| catalogue::say("cli.clock_before_epoch", &[("error", &error.to_string())]))
}

#[cfg(test)]
mod tests {
    use super::*;
    use flow::{AttemptRelation, StepSpecies};
    use serde_json::json;
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicU64, Ordering};

    fn words(given: &[&str]) -> Vec<String> {
        given.iter().map(|word| (*word).to_owned()).collect()
    }

    static NEXT_DIRECTORY: AtomicU64 = AtomicU64::new(0);

    /// A directory that deletes itself: the store is a file, and a file left
    /// behind makes the next test pass for the wrong reason.
    struct TestDirectory(PathBuf);

    impl TestDirectory {
        fn new(label: &str) -> Self {
            let sequence = NEXT_DIRECTORY.fetch_add(1, Ordering::Relaxed);
            let path = std::env::temp_dir().join(format!(
                "sailor-step-{label}-{}-{sequence}",
                std::process::id()
            ));
            std::fs::create_dir_all(&path).expect("creare la cartella di prova");
            Self(path)
        }
    }

    impl Drop for TestDirectory {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    /// A flow of two steps: one produces, the other judges. The minimal shape in
    /// which «the author does not judge» means anything.
    fn a_flow() -> FlowFile {
        serde_json::from_str(
            r#"{
                "id": "consegna-di-prova",
                "description": "un passo consegnato e il suo giudizio",
                "graph": {
                    "steps": [
                        {
                            "id": "implementa",
                            "deps": [],
                            "input_schema": {"type": "any"},
                            "output_schema": {"type": "any"},
                            "when": null,
                            "action": "handed_to_agent",
                            "max_attempts": 3
                        },
                        {
                            "id": "verdetto",
                            "deps": ["implementa"],
                            "input_schema": {"type": "any"},
                            "output_schema": {
                                "type": "object",
                                "properties": {"verdict": {"type": "string"}},
                                "required": ["verdict"],
                                "allow_extra": false
                            },
                            "when": null,
                            "action": "handed_to_agent",
                            "max_attempts": 3
                        }
                    ]
                },
                "inputs": {}
            }"#,
        )
        .expect("the scratch flow is valid")
    }

    fn handed_input(step_id: &str) -> Value {
        json!({
            "mandate": format!("do the work of {step_id}"),
            "holder": "claude-vivo",
            "handoff_timeout_secs": 3600
        })
    }

    /// A store with a run in it and a step already handed over.
    fn a_handed_run(directory: &TestDirectory, step_id: &str, deps: Vec<String>) -> Ledger {
        let ledger = Ledger::open(&directory.0).expect("the ledger opens");
        ledger
            .record_run(&ledger::RunRecord {
                run_id: "run-1".to_owned(),
                kind: "flow".to_owned(),
                entity: "consegna-di-prova".to_owned(),
                parent_run_id: None,
                started_by: "prova".to_owned(),
                status: "waiting".to_owned(),
                total_cost_micros: 0,
                error: None,
                started_at: 100,
                ended_at: Some(150),
                worktree: None,
                stop_reason: None,
            })
            .expect("recording the run");
        hand_over(&ledger, step_id, deps);
        ledger
    }

    /// Writes the step as the engine would: opened, then closed with the
    /// «waiting» outcome, which is what `handed_to_agent` produces.
    fn hand_over(ledger: &Ledger, step_id: &str, deps: Vec<String>) {
        let mut record = StepRecord::started(
            "run-1",
            step_id,
            1,
            1,
            deps,
            handed_input(step_id),
            vec![],
            110,
        );
        record.species = Some(StepSpecies::Repeatable);
        record.held_by_pid = Some(std::process::id());
        ledger
            .append_step_started(&record)
            .expect("opening the step");
        ledger
            .close_step(
                "run-1",
                step_id,
                1,
                1,
                Completion {
                    outcome: Outcome::Waiting,
                    output: None,
                    said: Some("consegnato a «claude-vivo»".to_owned()),
                    failure_class: None,
                    refusal: None,
                    ran: None,
                    ended_at: 150,
                    bytes_seen: None,
                    bytes_discarded: None,
                },
            )
            .expect("handing it over");
    }

    fn options(pairs: &[(&str, &str)]) -> BTreeMap<String, String> {
        pairs
            .iter()
            .map(|(name, value)| ((*name).to_owned(), (*value).to_owned()))
            .collect()
    }

    /// **THE OPEN CARRIES THE VERY SAME INPUT.**
    ///
    /// The digest comes from the input: an identical copy makes
    /// `attempt_relation` say the same work resumed, not a new job. A rebuilt
    /// input would show two jobs where there is one, and re-fingerprint the
    /// mandate living inside it without changing it.
    #[test]
    fn opening_a_handed_step_carries_the_very_same_input() {
        let directory = TestDirectory::new("stesso-ingresso");
        let ledger = a_handed_run(&directory, "implementa", vec![]);

        let report = open_step_in(
            &ledger,
            &options(&[
                ("run", "run-1"),
                ("step", "implementa"),
                ("as", "chi-lavora"),
            ]),
        )
        .expect("the step is taken on");
        assert!(
            report.contains("do the work of implementa"),
            "the mandate is read from the input: {report}"
        );

        let records = ledger.steps("run-1").expect("reading the steps back");
        let opened = records
            .iter()
            .find(|record| record.outcome.is_none())
            .expect("there is an open attempt");
        assert_eq!(opened.attempt, 2);
        assert_eq!(opened.epoch, 2);
        assert_eq!(
            opened.input,
            handed_input("implementa"),
            "the input is copied exactly as it was"
        );
        assert_eq!(
            opened.attempt_relation,
            Some(AttemptRelation::SameInput),
            "same input and same brakes: it is the same work taken up again"
        );
        assert_eq!(
            opened.held_by_pid, None,
            "no process holds a handed step: writing a pid there would make it \
             be declared dead at the first resume"
        );
    }

    /// **AN OUTPUT THE STEP DOES NOT DECLARE IS REFUSED.**
    ///
    /// `RecordStore::close` validates nothing. Without this check a malformed
    /// output would enter the store as good and kill the run three steps later,
    /// where the blame would fall on an innocent step.
    #[test]
    fn an_output_the_step_does_not_declare_is_refused() {
        let directory = TestDirectory::new("uscita-non-dichiarata");
        let ledger = a_handed_run(&directory, "verdetto", vec!["implementa".to_owned()]);
        open_step_in(
            &ledger,
            &options(&[
                ("run", "run-1"),
                ("step", "verdetto"),
                ("as", "chi-giudica"),
            ]),
        )
        .expect("the step is taken on");

        let wrong = directory.0.join("uscita.json");
        std::fs::write(&wrong, r#"{"esito": "va bene"}"#).expect("writing the output");
        let error = close_step_in(
            &ledger,
            &a_flow(),
            &options(&[
                ("run", "run-1"),
                ("step", "verdetto"),
                ("as", "chi-giudica"),
                ("outcome", "went"),
                ("output-file", wrong.to_str().expect("a readable path")),
            ]),
        )
        .expect_err("an output outside the schema is refused");
        assert!(error.contains("does not meet step"), "{error}");

        let records = ledger.steps("run-1").expect("reading the steps back");
        assert!(
            records
                .iter()
                .any(|record| record.step_id == "verdetto" && record.outcome.is_none()),
            "the step stays open: refusing and closing all the same would be worse \
             than not checking"
        );
    }

    /// The output the step declares passes, and the step closes.
    #[test]
    fn the_declared_output_closes_the_step() {
        let directory = TestDirectory::new("uscita-dichiarata");
        let ledger = a_handed_run(&directory, "verdetto", vec!["implementa".to_owned()]);
        open_step_in(
            &ledger,
            &options(&[
                ("run", "run-1"),
                ("step", "verdetto"),
                ("as", "chi-giudica"),
            ]),
        )
        .expect("the step is taken on");

        let good = directory.0.join("uscita.json");
        std::fs::write(&good, r#"{"verdict": "va bene"}"#).expect("writing the output");
        let report = close_step_in(
            &ledger,
            &a_flow(),
            &options(&[
                ("run", "run-1"),
                ("step", "verdetto"),
                ("as", "chi-giudica"),
                ("outcome", "went"),
                ("output-file", good.to_str().expect("a readable path")),
                ("turns", "12"),
            ]),
        )
        .expect("the declared output is accepted");
        assert!(report.contains("closed"), "{report}");

        let spent = ledger.spent_in_run("run-1").expect("the spend can be asked for");
        assert_eq!(spent.calls, 1, "the declared turns write a call");
        assert_eq!(
            spent.calls_without_cost, 1,
            "a self-declared call carries no cost"
        );
        assert!(
            !spent.is_complete(),
            "with a hand-over inside it, a run's total stops being complete — and \
             that is what makes the spend cap a guarantee only over what is known"
        );
    }

    /// **WHOEVER WROTE A DEPENDENCY CANNOT CLOSE THE STEP THAT JUDGES IT.**
    ///
    /// The permanent constraint «the author does not judge», applied to the
    /// gesture where the judgement is really written. The refusal at open would
    /// not be enough on its own: one would open under any name and close under
    /// the author's.
    #[test]
    fn whoever_wrote_a_dependency_cannot_close_the_step_that_judges_it() {
        let directory = TestDirectory::new("autore-giudice");
        let ledger = a_handed_run(&directory, "implementa", vec![]);
        hand_over(&ledger, "verdetto", vec!["implementa".to_owned()]);

        // The judgement is taken on **first**, while nobody has closed the
        // dependency yet and the author does not exist: this is the order in
        // which the close-side refusal is the only one left standing.
        open_step_in(
            &ledger,
            &options(&[("run", "run-1"), ("step", "verdetto"), ("as", "autore")]),
        )
        .expect("nobody has authored anything yet");

        // «autore» does the work and closes it.
        open_step_in(
            &ledger,
            &options(&[("run", "run-1"), ("step", "implementa"), ("as", "autore")]),
        )
        .expect("the author takes the work");
        let done = directory.0.join("implementa.json");
        std::fs::write(&done, r#"{"fatto": true}"#).expect("writing the output");
        close_step_in(
            &ledger,
            &a_flow(),
            &options(&[
                ("run", "run-1"),
                ("step", "implementa"),
                ("as", "autore"),
                ("outcome", "went"),
                ("output-file", done.to_str().expect("a readable path")),
            ]),
        )
        .expect("the author closes their own work");

        // And now it tries to judge itself, closing the attempt it really holds:
        // the close is the gesture that counts, and the one a door left open at
        // open time would allow.
        let good = directory.0.join("verdetto.json");
        std::fs::write(&good, r#"{"verdict": "va bene"}"#).expect("writing the output");
        let refused = close_step_in(
            &ledger,
            &a_flow(),
            &options(&[
                ("run", "run-1"),
                ("step", "verdetto"),
                ("as", "autore"),
                ("outcome", "went"),
                ("output-file", good.to_str().expect("a readable path")),
            ]),
        )
        .expect_err("the author does not close the step that judges them");
        assert!(refused.contains("does not judge"), "{refused}");

        // Handed back by its deadline, it cannot be taken on again either.
        ledger
            .close_step(
                "run-1",
                "verdetto",
                2,
                2,
                Completion {
                    outcome: Outcome::Waiting,
                    output: None,
                    said: None,
                    failure_class: None,
                    refusal: None,
                    ran: None,
                    ended_at: 400,
                    bytes_seen: None,
                    bytes_discarded: None,
                },
            )
            .expect("the deadline hands it back");
        let refused = open_step_in(
            &ledger,
            &options(&[("run", "run-1"), ("step", "verdetto"), ("as", "autore")]),
        )
        .expect_err("the author does not open the step that judges them");
        assert!(refused.contains("does not judge"), "{refused}");
    }

    /// The permission exists and is declared in the step: a denial with no way
    /// out would stop the flows where the same hand is the right choice.
    #[test]
    fn the_flow_can_declare_that_the_same_hand_is_allowed() {
        let directory = TestDirectory::new("stessa-mano");
        let ledger = a_handed_run(&directory, "implementa", vec![]);

        let mut record = StepRecord::started(
            "run-1",
            "verdetto",
            1,
            1,
            vec!["implementa".to_owned()],
            json!({
                "mandate": "giudica",
                "holder": "claude-vivo",
                "handoff_timeout_secs": 3600,
                "same_holder_ok": true
            }),
            vec![],
            110,
        );
        record.species = Some(StepSpecies::Repeatable);
        ledger.append_step_started(&record).expect("aprire");
        ledger
            .close_step(
                "run-1",
                "verdetto",
                1,
                1,
                Completion {
                    outcome: Outcome::Waiting,
                    output: None,
                    said: None,
                    failure_class: None,
                    refusal: None,
                    ran: None,
                    ended_at: 150,
                    bytes_seen: None,
                    bytes_discarded: None,
                },
            )
            .expect("handing it over");

        open_step_in(
            &ledger,
            &options(&[("run", "run-1"), ("step", "implementa"), ("as", "autore")]),
        )
        .expect("the author takes the work");
        let done = directory.0.join("implementa.json");
        std::fs::write(&done, r#"{"fatto": true}"#).expect("writing the output");
        close_step_in(
            &ledger,
            &a_flow(),
            &options(&[
                ("run", "run-1"),
                ("step", "implementa"),
                ("as", "autore"),
                ("outcome", "went"),
                ("output-file", done.to_str().expect("a readable path")),
            ]),
        )
        .expect("the author closes");

        open_step_in(
            &ledger,
            &options(&[("run", "run-1"), ("step", "verdetto"), ("as", "autore")]),
        )
        .expect("the step declares them allowed, so it goes through");
    }

    /// **CLOSING «WENT» WITH NO OUTPUT, WHILE SOMEONE WAITS FOR IT, IS
    /// REFUSED.** Without the refusal the run stops at the step *after*, with
    /// «no typed output», and whoever looks hunts the defect in the wrong step.
    /// The log keeps a null output apart from no output (fault 33); the refusal
    /// is about where a defect shows, not about what the log can say.
    #[test]
    fn closing_as_went_without_an_output_is_refused_when_a_step_waits_for_it() {
        let directory = TestDirectory::new("uscita-che-manca");
        let ledger = a_handed_run(&directory, "implementa", vec![]);
        open_step_in(
            &ledger,
            &options(&[("run", "run-1"), ("step", "implementa"), ("as", "chi")]),
        )
        .expect("the step is taken on");

        let error = close_step_in(
            &ledger,
            &a_flow(),
            &options(&[
                ("run", "run-1"),
                ("step", "implementa"),
                ("as", "chi"),
                ("outcome", "went"),
            ]),
        )
        .expect_err("with no output the step after would not start");
        assert!(
            error.contains("verdetto"),
            "it must name whoever is waiting: {error}"
        );
        assert!(error.contains("--output-file"), "{error}");
    }

    /// A step with nobody downstream closes with no output: demanding one would
    /// be a formality stopping finished work.
    #[test]
    fn a_last_step_closes_without_an_output() {
        let directory = TestDirectory::new("ultimo-passo");
        let ledger = a_handed_run(&directory, "verdetto", vec!["implementa".to_owned()]);
        open_step_in(
            &ledger,
            &options(&[("run", "run-1"), ("step", "verdetto"), ("as", "chi")]),
        )
        .expect("the step is taken on");
        close_step_in(
            &ledger,
            &a_flow(),
            &options(&[
                ("run", "run-1"),
                ("step", "verdetto"),
                ("as", "chi"),
                ("outcome", "went"),
            ]),
        )
        .expect("nobody depends on «verdetto»: it closes with no output");
    }

    /// **A STEP THAT IS REALLY RUNNING IS NOT CLOSED BY HAND.**
    ///
    /// A record with a pid was opened by the executor, and that process is at
    /// work: closing it from another terminal pulls the step out from under it,
    /// and its close fails with «already closed» — a gesture made elsewhere
    /// breaking a healthy run.
    #[test]
    fn a_step_a_live_executor_holds_cannot_be_closed_by_hand() {
        let directory = TestDirectory::new("tenuto-dal-motore");
        let ledger = a_handed_run(&directory, "implementa", vec![]);
        // As the engine would open it: with its own pid written in.
        let mut record = StepRecord::started(
            "run-1",
            "implementa",
            2,
            2,
            vec![],
            handed_input("implementa"),
            vec![],
            200,
        );
        record.held_by_pid = Some(std::process::id());
        ledger.append_step_started(&record).expect("the engine opens it");

        let error = close_step_in(
            &ledger,
            &a_flow(),
            &options(&[
                ("run", "run-1"),
                ("step", "implementa"),
                ("as", "chi"),
                ("outcome", "went"),
            ]),
        )
        .expect_err("a step the engine holds is not closed by hand");
        assert!(error.contains("the engine is running it"), "{error}");
        assert!(
            ledger
                .steps("run-1")
                .expect("reading the steps back")
                .iter()
                .any(|found| found.attempt == 2 && found.outcome.is_none()),
            "the engine's attempt stays open: closing it would break the running run"
        );
    }

    /// The deadline gave the step back and somebody else took it on. The first
    /// taker, still at work, must not close the second one's attempt.
    #[test]
    fn the_taker_whose_handover_expired_does_not_close_the_next_takers_attempt() {
        let directory = TestDirectory::new("due-prese-in-carico");
        let ledger = a_handed_run(&directory, "implementa", vec![]);
        open_step_in(
            &ledger,
            &options(&[("run", "run-1"), ("step", "implementa"), ("as", "alice")]),
        )
        .expect("alice takes it on");
        ledger
            .close_step(
                "run-1",
                "implementa",
                2,
                2,
                Completion {
                    outcome: Outcome::Waiting,
                    output: None,
                    said: Some("la scadenza lo rimette fra i pronti".to_owned()),
                    failure_class: None,
                    refusal: None,
                    ran: None,
                    ended_at: 300,
                    bytes_seen: None,
                    bytes_discarded: None,
                },
            )
            .expect("the deadline hands it back");
        open_step_in(
            &ledger,
            &options(&[("run", "run-1"), ("step", "implementa"), ("as", "bob")]),
        )
        .expect("bob takes it on");

        let error = close_step_in(
            &ledger,
            &a_flow(),
            &options(&[
                ("run", "run-1"),
                ("step", "implementa"),
                ("as", "alice"),
                ("outcome", "went"),
            ]),
        )
        .expect_err("alice does not close bob's attempt");
        assert!(error.contains("bob"), "{error}");
        assert!(
            ledger
                .steps("run-1")
                .expect("reading the steps back")
                .iter()
                .any(|found| found.taken_on_by.as_deref() == Some("bob")
                    && found.outcome.is_none()),
            "bob's attempt stays open: he is still working on it"
        );
    }

    /// A crash leaves a row open with a number in it; the process behind that
    /// number is gone, and the step must still be closable by hand.
    #[test]
    fn a_step_whose_holder_the_kernel_denies_can_be_closed_by_hand() {
        let directory = TestDirectory::new("tenuto-da-nessuno");
        let ledger = a_handed_run(&directory, "implementa", vec![]);
        let mut record = StepRecord::started(
            "run-1",
            "implementa",
            2,
            2,
            vec![],
            handed_input("implementa"),
            vec![],
            200,
        );
        record.held_by_pid = Some(std::process::id());
        record.held_by = Some(flow::HolderIdentity {
            born_at: Some(1),
            invocation: "una-corsa-finita-male".to_owned(),
        });
        ledger
            .append_step_started(&record)
            .expect("the engine opens it");

        let left = directory.0.join("lasciato.json");
        std::fs::write(&left, r#"{"done": true}"#).expect("writing the output");
        close_step_in(
            &ledger,
            &a_flow(),
            &options(&[
                ("run", "run-1"),
                ("step", "implementa"),
                ("as", "chi"),
                ("outcome", "went"),
                ("output-file", left.to_str().expect("a readable path")),
            ]),
        )
        .expect("nobody holds it any more");
        assert!(
            ledger
                .steps("run-1")
                .expect("reading the steps back")
                .iter()
                .all(|found| found.outcome.is_some() || found.attempt != 2),
            "the abandoned attempt is closed"
        );
    }

    /// A step that is not waiting cannot be taken on: it was handed to nobody.
    #[test]
    fn a_step_that_was_not_handed_over_cannot_be_taken() {
        let directory = TestDirectory::new("non-consegnato");
        let ledger = a_handed_run(&directory, "implementa", vec![]);
        open_step_in(
            &ledger,
            &options(&[("run", "run-1"), ("step", "implementa"), ("as", "chi")]),
        )
        .expect("the first one takes it");
        let error = open_step_in(
            &ledger,
            &options(&[("run", "run-1"), ("step", "implementa"), ("as", "un-altro")]),
        )
        .expect_err("a step already open is not taken up again");
        assert!(error.contains("is open"), "{error}");
    }

    #[test]
    fn an_option_without_a_value_is_refused() {
        let error = flags(&words(&["--run", "--step", "implementa"]))
            .expect_err("an option with no value is refused");
        assert!(error.contains("the real value is missing"), "{error}");
    }

    #[test]
    fn options_come_back_as_pairs() {
        let found = flags(&words(&[
            "--run",
            "run-1",
            "--step",
            "implementa",
            "--as",
            "chi",
        ]))
        .expect("the pairs read back");
        assert_eq!(found.get("run").map(String::as_str), Some("run-1"));
        assert_eq!(found.get("step").map(String::as_str), Some("implementa"));
        assert_eq!(found.get("as").map(String::as_str), Some("chi"));
    }

    #[test]
    fn a_missing_option_names_itself() {
        let error = required(&BTreeMap::new(), "run").expect_err("it is missing");
        assert!(error.contains("--run"), "{error}");
    }

    /// An outcome a person cannot declare is refused before the store is
    /// touched: `Waiting` and `Skipped` are the engine's, not a hand's.
    #[test]
    fn only_went_and_broke_can_be_declared_by_hand() {
        let found: BTreeMap<String, String> = [
            ("run", "run-1"),
            ("step", "implementa"),
            ("as", "chi"),
            ("outcome", "waiting"),
        ]
        .into_iter()
        .map(|(name, value)| (name.to_owned(), value.to_owned()))
        .collect();
        let error = close_step(&found).expect_err("«waiting» is not declared by hand");
        assert!(error.contains("`went` and `broke`"), "{error}");
    }

    #[test]
    fn what_comes_next_names_the_resume_line() {
        let next = what_comes_next(&Decision::Ready(vec!["verdetto".to_owned()]), "run-1", 100);
        assert!(next.contains("sailor flow resume run-1"), "{next}");
        assert!(next.contains("verdetto"), "{next}");
    }

    /// A postponed step says how long is left, not just that it is not ready:
    /// without the number, a reader cannot tell a moment from tomorrow.
    #[test]
    fn what_comes_next_says_how_long_a_postponed_step_has_left() {
        let next = what_comes_next(
            &Decision::NotYet {
                steps: vec!["raccogli-il-mandato".to_owned()],
                due_at: 130,
            },
            "run-1",
            100,
        );
        assert!(next.contains("raccogli-il-mandato"), "{next}");
        assert!(next.contains("30 s"), "{next}");
        assert!(next.contains("sailor flow resume run-1"), "{next}");
    }

    /// What the store kept about who decided a step, if anybody did.
    fn what_was_decided(ledger: &Ledger, step_id: &str) -> Value {
        ledger
            .read_record(HOLDER_COLLECTION, &holder_key("run-1", step_id))
            .expect("the store answers")
            .expect("somebody closed the step")
            .value
    }

    fn last_record(ledger: &Ledger, step_id: &str) -> StepRecord {
        let records = ledger.steps("run-1").expect("reading the steps back");
        last_attempt(&records, step_id)
            .expect("the step has an attempt")
            .clone()
    }

    /// **A DECISION IS ONE GESTURE.** A step still waiting is taken on and let
    /// through in the same command: asking a person to `open` before they may
    /// approve is ceremony, and the taking-on is implied by the deciding.
    #[test]
    fn approving_a_waiting_step_takes_it_on_and_lets_it_through() {
        let directory = TestDirectory::new("approva");
        let ledger = a_handed_run(&directory, "verdetto", vec![]);

        let report = decide_step_in(
            &ledger,
            &a_flow(),
            &options(&[
                ("run", "run-1"),
                ("step", "verdetto"),
                ("as", "chi-giudica"),
                ("why", "the output matches what was asked for"),
            ]),
            Verdict::Approved,
        )
        .expect("the step is approved");

        assert!(report.contains("chi-giudica"), "{report}");
        assert!(
            report.contains("the output matches what was asked for"),
            "an approval says why when a reason was given: {report}"
        );
        let closed = last_record(&ledger, "verdetto");
        assert_eq!(closed.outcome, Some(Outcome::Went));
        assert_eq!(
            closed.attempt, 2,
            "the waiting attempt was taken on, not overwritten"
        );
    }

    /// **AN APPROVAL THAT LEAVES NO TRACE IS A STEP NOBODY READ.** Who, when,
    /// what — and the reason when one was given — go into the same entry the
    /// close already writes.
    #[test]
    fn an_approval_is_told_apart_from_a_close_that_says_nothing() {
        let directory = TestDirectory::new("traccia");
        let ledger = a_handed_run(&directory, "verdetto", vec![]);
        decide_step_in(
            &ledger,
            &a_flow(),
            &options(&[
                ("run", "run-1"),
                ("step", "verdetto"),
                ("as", "chi-giudica"),
                ("why", "measured on the tree, not declared"),
            ]),
            Verdict::Approved,
        )
        .expect("the step is approved");

        let decided = what_was_decided(&ledger, "verdetto");
        assert_eq!(
            decided.get("decision").and_then(Value::as_str),
            Some("approved")
        );
        assert_eq!(
            decided.get("why").and_then(Value::as_str),
            Some("measured on the tree, not declared")
        );
        let written = ledger
            .read_record(HOLDER_COLLECTION, &holder_key("run-1", "verdetto"))
            .expect("the store answers")
            .expect("somebody closed the step");
        assert_eq!(written.written_by, "chi-giudica", "who decided it");
        assert!(written.written_at > 0, "when they decided it");
    }

    /// The control that makes the one above mean something: a plain close says
    /// nothing about a decision, so an approval is not a rename of it.
    #[test]
    fn a_plain_close_declares_no_decision() {
        let directory = TestDirectory::new("chiusura-semplice");
        let ledger = a_handed_run(&directory, "verdetto", vec![]);
        open_step_in(
            &ledger,
            &options(&[
                ("run", "run-1"),
                ("step", "verdetto"),
                ("as", "chi-giudica"),
            ]),
        )
        .expect("the step is taken on");
        close_step_in(
            &ledger,
            &a_flow(),
            &options(&[
                ("run", "run-1"),
                ("step", "verdetto"),
                ("as", "chi-giudica"),
                ("outcome", "went"),
            ]),
        )
        .expect("the step is closed");

        assert_eq!(
            what_was_decided(&ledger, "verdetto").get("decision"),
            None,
            "a close is not a verdict, and the store must not read as if it were"
        );
    }

    /// **A REFUSAL WITH NO REASON CANNOT BE READ BACK**, and it is refused
    /// before anything is written: a half-rejected step would be worse than
    /// none.
    #[test]
    fn a_rejection_with_no_reason_is_refused_and_writes_nothing() {
        let directory = TestDirectory::new("rifiuto-muto");
        let ledger = a_handed_run(&directory, "verdetto", vec![]);
        let error = decide_step_in(
            &ledger,
            &a_flow(),
            &options(&[
                ("run", "run-1"),
                ("step", "verdetto"),
                ("as", "chi-giudica"),
                ("why", "   "),
            ]),
            Verdict::Rejected,
        )
        .expect_err("a rejection with no reason does not go through");
        assert!(error.contains("--why"), "{error}");
        assert_eq!(
            last_record(&ledger, "verdetto").outcome,
            Some(Outcome::Waiting),
            "the step is still waiting: nothing was written"
        );
    }

    /// Who decided, when, what, and why: the four a rejection owes whoever
    /// reads the run afterwards.
    #[test]
    fn a_rejection_writes_who_decided_what_and_why() {
        let directory = TestDirectory::new("rifiuto");
        let ledger = a_handed_run(&directory, "verdetto", vec![]);
        decide_step_in(
            &ledger,
            &a_flow(),
            &options(&[
                ("run", "run-1"),
                ("step", "verdetto"),
                ("as", "chi-giudica"),
                (
                    "why",
                    "the brief asked for a measurement and got an opinion",
                ),
            ]),
            Verdict::Rejected,
        )
        .expect("the step is rejected");

        let decided = what_was_decided(&ledger, "verdetto");
        assert_eq!(
            decided.get("decision").and_then(Value::as_str),
            Some("rejected")
        );
        assert_eq!(
            decided.get("why").and_then(Value::as_str),
            Some("the brief asked for a measurement and got an opinion")
        );
        let closed = last_record(&ledger, "verdetto");
        assert_eq!(closed.outcome, Some(Outcome::Broke));
        assert_eq!(
            closed.failure_class.as_deref(),
            Some(REJECTED_BY_A_PERSON),
            "a rejected step is counted apart from one that was handed back"
        );
        assert_eq!(
            closed.said.as_deref(),
            Some("the brief asked for a measurement and got an opinion"),
            "the reason is on the step as well, where whoever reads the run finds it"
        );
    }

    /// **A REJECTION CLOSES THE RUN, IT DOES NOT LEAVE IT HANGING.** The
    /// request stops the next resume from opening a front; the header says so
    /// to whoever only looks.
    #[test]
    fn a_rejection_stops_the_run_and_the_run_says_why() {
        let directory = TestDirectory::new("rifiuto-ferma");
        let ledger = a_handed_run(&directory, "verdetto", vec![]);
        decide_step_in(
            &ledger,
            &a_flow(),
            &options(&[
                ("run", "run-1"),
                ("step", "verdetto"),
                ("as", "chi-giudica"),
                ("why", "it names a machine that is not the reader's"),
            ]),
            Verdict::Rejected,
        )
        .expect("the step is rejected");

        assert_eq!(
            ledger
                .why_the_halt_was_asked("run-1")
                .expect("the store answers"),
            Some((
                "chi-giudica".to_owned(),
                "it names a machine that is not the reader's".to_owned()
            ))
        );
        let header = ledger
            .run_header("run-1")
            .expect("the store answers")
            .expect("the run has a header");
        assert_eq!(header.status, "stopped");
        assert_eq!(header.stop_reason.as_deref(), Some("by_hand"));
        assert_eq!(
            header.started_at, 100,
            "the run started when it started: a rejection is not a new beginning"
        );
        let error = header.error.unwrap_or_default();
        assert!(error.contains("chi-giudica"), "{error}");
        assert!(
            error.contains("it names a machine that is not the reader's"),
            "{error}"
        );
    }

    /// The control for the one above: an approval leaves the run open, so
    /// "stopped" is the rejection's doing and not something every decision does.
    #[test]
    fn an_approval_leaves_the_run_where_it_was() {
        let directory = TestDirectory::new("approva-non-ferma");
        let ledger = a_handed_run(&directory, "verdetto", vec![]);
        decide_step_in(
            &ledger,
            &a_flow(),
            &options(&[
                ("run", "run-1"),
                ("step", "verdetto"),
                ("as", "chi-giudica"),
            ]),
            Verdict::Approved,
        )
        .expect("the step is approved");

        assert_eq!(
            ledger.halt_request("run-1").expect("the store answers"),
            None,
            "an approval asks nothing to stop"
        );
        let header = ledger
            .run_header("run-1")
            .expect("the store answers")
            .expect("the run has a header");
        assert_eq!(header.status, "waiting", "the header was not rewritten");
    }

    /// **THE AUTHOR DOES NOT JUDGE, AND DECIDING IS JUDGING.** The refusal is
    /// not re-implemented here: it is the one `open` already applies, reached
    /// because a decision takes the step on.
    #[test]
    fn the_author_of_the_work_cannot_approve_it() {
        let directory = TestDirectory::new("autore-approva");
        let ledger = a_handed_run(&directory, "implementa", vec![]);
        let produced = directory.0.join("uscita.json");
        std::fs::write(&produced, r#"{"verdict": "done"}"#).expect("writing the output");
        decide_step_in(
            &ledger,
            &a_flow(),
            &options(&[
                ("run", "run-1"),
                ("step", "implementa"),
                ("as", "chi-lavora"),
                ("output-file", &produced.display().to_string()),
            ]),
            Verdict::Approved,
        )
        .expect("the work is closed by whoever did it");
        hand_over(&ledger, "verdetto", vec!["implementa".to_owned()]);

        let error = decide_step_in(
            &ledger,
            &a_flow(),
            &options(&[
                ("run", "run-1"),
                ("step", "verdetto"),
                ("as", "chi-lavora"),
                ("why", "looks fine to me"),
            ]),
            Verdict::Approved,
        )
        .expect_err("whoever wrote the work does not approve it");
        assert!(error.contains("implementa"), "{error}");
    }
}
