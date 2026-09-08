//! `sailor flow cap` and `sailor flow schedule`: the two settings a command may
//! write into a flow file, and the refusals that guard the writing.

use flow::FlowFile;
use std::fmt::Write as _;
use ui::gather::FlowSource;

use super::cost::in_units;
use super::{default_ledger_dir, one_flow};

// ── the spend cap of a flow ──────────────────────────────────────────────

/// **HOW MANY COSTED RUNS IT TAKES TO SUGGEST A CAP.**
///
/// Three, and it is not a rounded threshold: below three there is no spread to
/// look at. With two samples the highest and the lowest are the only two
/// values, and calling the greater of two «the worst observed» is a made-up
/// figure wearing the face of a measurement — precisely fault 22, a never
/// computed zero passing for a measurement, in another shape. Below the
/// threshold the command **refuses to suggest** and says what is there.
const RUNS_BEFORE_SUGGESTING: usize = 3;

/// What the ledger has seen a flow spend.
struct Observed {
    /// The recorded runs of that flow, however they went.
    runs: usize,
    /// Those that spent something known: the only ones to count on.
    costed_runs: usize,
    /// The dearest run observed, in micros.
    worst_run_micros: i64,
    /// The dearest call observed, in micros.
    dearest_call_micros: i64,
    /// How many calls declared no cost. They stay outside the figures above,
    /// and whoever reads a suggestion must know how many are missing.
    calls_without_cost: usize,
}

/// The count itself: one run per row, and for each run the cost of each of its
/// calls — `None` when that engine did not declare it.
///
/// **IT TAKES THE COSTS AND NOT THE LEDGER, SO IT CAN BE TESTED.** The default
/// ledger is one per process, and the tests run in parallel inside the same
/// one: a test wanting to point it elsewhere would have to write an
/// environment variable, wrecking the others at random. Here the rule — what a
/// «costed run» is, which is the worst, which the dearest call — is asked
/// without opening anything.
fn observed_from(runs: &[Vec<Option<i64>>]) -> Observed {
    let mut seen = Observed {
        runs: runs.len(),
        costed_runs: 0,
        worst_run_micros: 0,
        dearest_call_micros: 0,
        calls_without_cost: 0,
    };
    for calls in runs {
        let mut spent = 0i64;
        for call in calls {
            match call {
                Some(cost) => {
                    spent += cost;
                    seen.dearest_call_micros = seen.dearest_call_micros.max(*cost);
                }
                None => seen.calls_without_cost += 1,
            }
        }
        // **A COSTED RUN IS ONE THAT SPENT, NOT ONE THAT CALLED.** The 28 runs
        // out of 34 this ledger carries at zero are fault 22 — cost was the
        // constant zero back then — and counting them as samples would drag
        // every suggestion towards zero, towards a cap that stops every flow
        // before its first step.
        if spent > 0 {
            seen.costed_runs += 1;
            seen.worst_run_micros = seen.worst_run_micros.max(spent);
        }
    }
    seen
}

/// What the ledger knows about a flow's spending.
///
/// **AN ABSENT LEDGER IS NOT AN ERROR**: it is a machine that flow never ran
/// on, and the right answer is «zero runs», not a fault. A reader must be able
/// to ask the cap of a freshly written flow.
fn observed_spending(flow_id: &str) -> Result<Observed, String> {
    let Ok(dir) = default_ledger_dir() else {
        return Ok(observed_from(&[]));
    };
    let Some(data) = ui::gather::gather(&dir).map_err(|error| error.to_string())? else {
        return Ok(observed_from(&[]));
    };
    let runs: Vec<Vec<Option<i64>>> = data
        .runs
        .iter()
        .filter(|run| run.entity == flow_id)
        .map(|run| {
            data.calls_by_run
                .get(&run.run_id)
                .map(|calls| calls.iter().map(|call| call.cost_micros).collect())
                .unwrap_or_default()
        })
        .collect();
    Ok(observed_from(&runs))
}

/// **WHAT THE CAP DOES NOT PROMISE**, written every time a cap is there.
///
/// Without these two lines the cap reads as a guarantee on spending, and it is
/// no such thing. Whoever sets a cap and then finds a higher bill is right to
/// feel betrayed: better to say so when it is set.
pub(super) const WHAT_THE_CAP_DOES_NOT_PROMISE_KEY: &str = "cli.flow.what_the_cap_does_not_promise";

/// `sailor flow cap <name>`: the cap there is, and what the ledger saw.
pub(super) fn cap_of(sources: &[FlowSource], name: &str) -> Result<String, String> {
    let (flow, origin) = one_flow(sources, name)?;
    let mut report = format!("flow: {} ({origin})", flow.id);
    match flow.spend_cap_micros {
        None => report.push_str(&catalogue::say("cli.flow.cap_none_spends_freely", &[])),
        Some(cap) => {
            let _ = write!(
                report,
                "{}",
                catalogue::say(
                    "cli.flow.cap_is",
                    &[("micros", &cap.to_string()), ("units", &in_units(cap))]
                )
            );
            report.push_str(&catalogue::say(WHAT_THE_CAP_DOES_NOT_PROMISE_KEY, &[]));
        }
    }

    report.push_str(&what_the_ledger_saw(&observed_spending(&flow.id)?));
    Ok(report)
}

/// What the ledger saw, and whether a suggestion comes out of it.
///
/// Apart from `cap_of` because the three-run rule must be askable without a
/// ledger: `cap_of` opens a real one, and a real ledger in a test is an
/// environment variable global to the process.
fn what_the_ledger_saw(seen: &Observed) -> String {
    let mut said = format!(
        "\n{}",
        catalogue::say(
            "cli.flow.in_the_store",
            &[
                ("runs", &seen.runs.to_string()),
                ("costed", &seen.costed_runs.to_string()),
            ],
        )
    );
    if seen.calls_without_cost > 0 {
        let _ = write!(
            said,
            "\n{}",
            catalogue::say(
                "cli.flow.calls_without_cost",
                &[("count", &seen.calls_without_cost.to_string())],
            )
        );
    }

    if seen.costed_runs < RUNS_BEFORE_SUGGESTING {
        let _ = write!(
            said,
            "\n{}",
            catalogue::say(
                "cli.flow.no_suggestion_yet",
                &[
                    ("needed", &RUNS_BEFORE_SUGGESTING.to_string()),
                    ("costed", &seen.costed_runs.to_string()),
                ],
            )
        );
        return said;
    }

    // **WHY THE DEAREST CALL IS ADDED, AND IT IS NOT CAUTION.** The check fires
    // *before* opening a front, never inside a call: a run stops with the
    // granularity of a call, not of a micro. A cap set exactly on the worst
    // observed run therefore cuts runs of that size unpredictably — it depends
    // on how the spending falls across the fronts. The sum says: «the dearest
    // run I have seen, plus the grain I can stop at».
    let suggested = seen.worst_run_micros + seen.dearest_call_micros;
    let _ = write!(
        said,
        "\n{}",
        catalogue::say(
            "cli.flow.suggestion",
            &[
                ("suggested", &suggested.to_string()),
                ("units", &in_units(suggested)),
                ("worst_run", &in_units(seen.worst_run_micros)),
                ("dearest_call", &in_units(seen.dearest_call_micros)),
            ],
        )
    );
    said
}

/// The word that takes the cap off instead of setting one.
///
/// Without it the command could enter a state and not leave it: `0` is not
/// «none», it is «this flow must spend nothing».
const NO_CAP: &str = "none";

/// Words that used to be the only spelling, still accepted and no longer shown.
///
/// **A WORD A PERSON HAS ALREADY TYPED INTO A SCRIPT IS A PROMISE.** Dropping it
/// costs someone a run that fails for a reason the message cannot explain, so it
/// keeps working; leaving it in the help would teach it to whoever comes next.
const RETIRED_WORDS: &[(&str, &str)] =
    &[("nessuno", NO_CAP), ("leggero", LIGHT), ("pesante", HEAVY)];

/// What a typed word means, following [`RETIRED_WORDS`] once.
///
/// One hop and no more: an alias of an alias would make the accepted vocabulary
/// depend on the order of this list.
fn as_written_today(word: &str) -> &str {
    RETIRED_WORDS
        .iter()
        .find(|(retired, _)| *retired == word)
        .map_or(word, |(_, current)| *current)
}

/// `sailor flow cap <name> <micros|none>`: sets the cap or takes it off.
pub(super) fn set_cap(sources: &[FlowSource], name: &str, value: &str) -> Result<String, String> {
    let value = as_written_today(value);
    let wanted = if value == NO_CAP {
        None
    } else {
        let micros: i64 = value.parse().map_err(|_| {
            catalogue::say(
                "cli.flow.cap_not_a_number",
                &[("value", value), ("none", NO_CAP), ("name", name)],
            )
        })?;
        if micros < 0 {
            return Err(catalogue::say(
                "cli.flow.cap_negative",
                &[("micros", &micros.to_string()), ("none", NO_CAP)],
            ));
        }
        Some(micros)
    };

    let (mut flow, source) = a_flow_i_may_rewrite(sources, name)?;

    let before = flow.spend_cap_micros;
    if before == wanted {
        return Ok(catalogue::say(
            "cli.flow.cap_unchanged",
            &[
                ("flow", name),
                ("origin", source.origin),
                ("cap", &said_cap(before)),
            ],
        ));
    }
    flow.spend_cap_micros = wanted;
    flow::system::save_in(&source.dir, &flow)?;
    Ok(catalogue::say(
        "cli.flow.cap_written",
        &[
            ("flow", name),
            ("origin", source.origin),
            ("before", &said_cap(before)),
            ("after", &said_cap(wanted)),
            ("directory", &source.dir.display().to_string()),
        ],
    ))
}

/// A cap as a person reads it, including when there is none.
fn said_cap(cap: Option<i64>) -> String {
    match cap {
        None => NO_CAP.to_owned(),
        Some(micros) => format!("{micros} micro ({})", in_units(micros)),
    }
}

/// The flow a command may **rewrite**, and the source it sits in.
///
/// Apart, because both refusals belong to whoever writes, not to the cap: they
/// lived inside `set_cap` while it was the only gesture touching a file, and
/// copying them into the second would have been two copies of one rule. A
/// shipped flow is not rewritten — it is inside the binary, and the way is a
/// namesake at home, created by whoever wants it. And the file is named after
/// the `id`, or a twin appears: the register indexes by file name and writing
/// goes by `id`.
pub(super) fn a_flow_i_may_rewrite<'a>(
    sources: &'a [FlowSource],
    name: &str,
) -> Result<(FlowFile, &'a FlowSource), String> {
    let (flow, source) = where_it_lives(sources, name)?;
    if source.is_builtin() {
        return Err(catalogue::say(
            "cli.flow.ships_inside_the_binary",
            &[("flow", name)],
        ));
    }
    // **THE TWO NAMES ARE COMPARED, NOT WHETHER A FILE EXISTS.**
    // `target.exists()` answered the wrong question: when the directory holds
    // an `<id>.flow.json` belonging to *another* flow it says yes, the write
    // lands **in that flow's file**, and the command answers «done» with exit
    // zero — fault 50. The registry indexes by file name: `name` **is** the
    // file this flow comes from, and `save_in` writes `<id>.flow.json`.
    if name != flow.id {
        return Err(catalogue::say(
            "cli.flow.file_does_not_match_id",
            &[("name", name), ("id", &flow.id)],
        ));
    }
    // Two cases remain where the name matches and the file is **not** the one
    // that would be rewritten: a `<name>.json` without `.flow`, which the
    // registry loads and the write would not replace; and both together, where
    // the order the system lists the directory in decides which one runs.
    let target = source.dir.join(format!("{name}.flow.json"));
    let plain = source.dir.join(format!("{name}.json"));
    if !target.exists() {
        return Err(catalogue::say(
            "cli.flow.file_without_the_suffix",
            &[("name", name), ("file", &plain.display().to_string())],
        ));
    }
    if plain.exists() {
        return Err(catalogue::say(
            "cli.flow.two_files_one_name",
            &[("name", name)],
        ));
    }
    Ok((flow, source))
}

/// The word that takes the trigger off instead of setting one.
///
/// Same reason as [`NO_CAP`]. And a flow with no trigger is not a broken flow —
/// «it runs when somebody asks» is a fact, not a gap to fill.
const NO_SCHEDULE: &str = "none";

/// The two words a flow uses to say what one of its runs weighs.
const LIGHT: &str = "light";
const HEAVY: &str = "heavy";

/// A trigger as a person reads it, including when there is none.
///
/// **ONE SPELLING FOR BOTH COMMANDS.** It is read by whoever asks
/// `schedule <name>` and by whoever changes it: two different sentences for
/// the same fact would make anyone comparing them believe they had changed
/// more than they did.
fn said_schedule(schedule: Option<&flow::Schedule>) -> String {
    let Some(schedule) = schedule else {
        return catalogue::say(
            "cli.flow.no_schedule_starts_by_hand",
            &[("none", NO_SCHEDULE)],
        );
    };
    let when = match schedule.recurrence {
        flow::Recurrence::EverySeconds { seconds } => catalogue::say(
            "cli.flow.every_so_many_seconds",
            &[("seconds", &seconds.to_string())],
        ),
        flow::Recurrence::DailyAt { hour, minute } => catalogue::say(
            "cli.flow.once_a_day_at",
            &[("time", &format!("{hour:02}:{minute:02}"))],
        ),
        flow::Recurrence::WhenSomethingIsLeftBehind => {
            catalogue::say("cli.flow.when_something_is_left_behind", &[])
        }
    };
    let weight = match schedule.weight {
        flow::Weight::Light => LIGHT,
        flow::Weight::Heavy => HEAVY,
    };
    let perimeter = if schedule.perimeter.is_empty() {
        // Empty is «not declared», which is not «no limit»: a reader must be
        // able to tell the two apart, and the word says so.
        "not declared".to_owned()
    } else {
        schedule.perimeter.join(", ")
    };
    catalogue::say(
        "cli.flow.schedule_line",
        &[
            ("when", &when),
            ("weight", weight),
            ("perimeter", &perimeter),
        ],
    )
}

/// From a word to the recurrence it names, or to why it names none.
///
/// **THREE FORMS, RECOGNISED BY THEIR SHAPE,** and no flag: `none` takes it
/// off, `<number>s` is an interval, `HH:MM` an hour of the day. They are the
/// two forms `flow::Recurrence` knows plus the way out of them — a vocabulary
/// wider than the type it must fill would invent cases the engine cannot run.
fn recurrence_from(value: &str) -> Result<flow::Recurrence, String> {
    if let Some(digits) = value.strip_suffix('s') {
        let seconds: u64 = digits
            .parse()
            .map_err(|_| how_a_schedule_is_written(value))?;
        if seconds == 0 {
            return Err(catalogue::say(
                "cli.flow.every_zero_seconds",
                &[("none", NO_SCHEDULE)],
            ));
        }
        return Ok(flow::Recurrence::EverySeconds { seconds });
    }
    if let Some((hour, minute)) = value.split_once(':') {
        let hour: u32 = hour.parse().map_err(|_| how_a_schedule_is_written(value))?;
        let minute: u32 = minute
            .parse()
            .map_err(|_| how_a_schedule_is_written(value))?;
        if hour > 23 || minute > 59 {
            return Err(catalogue::say(
                "cli.flow.not_a_time_of_day",
                &[("value", value)],
            ));
        }
        return Ok(flow::Recurrence::DailyAt { hour, minute });
    }
    Err(how_a_schedule_is_written(value))
}

/// The accepted forms, written out in full whenever one is not recognised: a
/// refusal that does not say what to type forces a reading of the code.
fn how_a_schedule_is_written(value: &str) -> String {
    catalogue::say(
        "cli.flow.how_a_schedule_is_written",
        &[("value", value), ("none", NO_SCHEDULE)],
    )
}

fn weight_from(word: &str) -> Result<flow::Weight, String> {
    match as_written_today(word) {
        LIGHT => Ok(flow::Weight::Light),
        HEAVY => Ok(flow::Weight::Heavy),
        other => Err(catalogue::say(
            "cli.flow.not_a_weight",
            &[("word", other), ("light", LIGHT), ("heavy", HEAVY)],
        )),
    }
}

/// `sailor flow schedule <name>`: the trigger there is, and when it is due.
pub(super) fn schedule_of(sources: &[FlowSource], name: &str) -> Result<String, String> {
    let (flow, origin) = one_flow(sources, name)?;
    Ok(format!(
        "flow: {} ({origin})\ntrigger: {}",
        flow.id,
        said_schedule(flow.schedule.as_ref())
    ))
}

/// `sailor flow schedule <name> <every|at|none> [weight]`: sets, changes or
/// takes off the trigger.
///
/// **WHY THIS COMMAND EXISTS: FAULT 15 TO THE LETTER.** A trigger was once
/// changed by a Python script rewriting the JSON by hand, and a bypassed tool
/// records nothing around it: no refusal on system flows, no check the file is
/// named after the `id`, no graph validation on rewrite. **THE WEIGHT IS NOT
/// INVENTED**: without the word the command **refuses** instead of picking
/// [`LIGHT`] (fault 22 again); on a flow that has one, silence keeps it.
pub(super) fn set_schedule(
    sources: &[FlowSource],
    name: &str,
    value: &str,
    weight: Option<&str>,
) -> Result<String, String> {
    let (mut flow, source) = a_flow_i_may_rewrite(sources, name)?;
    let before = said_schedule(flow.schedule.as_ref());

    let wanted = if as_written_today(value) == NO_SCHEDULE {
        if let Some(word) = weight {
            return Err(catalogue::say(
                "cli.flow.no_schedule_has_no_weight",
                &[("none", NO_SCHEDULE), ("word", word)],
            ));
        }
        None
    } else {
        let recurrence = recurrence_from(value)?;
        let weight = match (weight, flow.schedule.as_ref()) {
            (Some(word), _) => weight_from(word)?,
            (None, Some(existing)) => existing.weight,
            (None, None) => {
                return Err(catalogue::say(
                    "cli.flow.no_schedule_yet_so_no_weight",
                    &[
                        ("name", name),
                        ("value", value),
                        ("light", LIGHT),
                        ("heavy", HEAVY),
                    ],
                ))
            }
        };
        Some(flow::Schedule {
            recurrence,
            weight,
            // **THE SCOPE IS KEPT, NOT REDECLARED.** It says where that work
            // may write: losing it while changing the hour would be a
            // permission widened by a command about something else.
            perimeter: flow
                .schedule
                .as_ref()
                .map(|existing| existing.perimeter.clone())
                .unwrap_or_default(),
        })
    };

    if flow.schedule == wanted {
        return Ok(catalogue::say(
            "cli.flow.schedule_unchanged",
            &[
                ("flow", name),
                ("origin", source.origin),
                ("before", &before),
            ],
        ));
    }
    flow.schedule = wanted;
    let after = said_schedule(flow.schedule.as_ref());
    flow::system::save_in(&source.dir, &flow)?;
    Ok(catalogue::say(
        "cli.flow.schedule_written",
        &[
            ("flow", name),
            ("origin", source.origin),
            ("before", &before),
            ("after", &after),
            ("directory", &source.dir.display().to_string()),
        ],
    ))
}

/// The flow **and the source it comes from**: rewriting it needs the
/// directory, not the name of the origin.
///
/// Looked up from the most specific to the least, the reverse of the order the
/// sources are listed in: on an equal name the last one wins, and whoever
/// rewrites must rewrite **the one that runs**. Rewriting the less specific
/// copy would leave the command saying «done» while the run keeps reading the
/// other.
fn where_it_lives<'a>(
    sources: &'a [FlowSource],
    name: &str,
) -> Result<(FlowFile, &'a FlowSource), String> {
    for source in sources.iter().rev() {
        match flow::system::registry_of(source).remove(name) {
            Some(Ok(flow)) => return Ok((flow, source)),
            Some(Err(reason)) => {
                return Err(catalogue::say(
                    "cli.flow.does_not_load_so_not_rewritten",
                    &[
                        ("flow", name),
                        ("origin", source.origin),
                        ("reason", &reason),
                    ],
                ))
            }
            None => continue,
        }
    }
    match one_flow(sources, name) {
        // The same message as `one_flow`, with the same list of names: two
        // different wordings for one «it is not there» would send somebody
        // hunting two defects where there is one.
        Err(reason) => Err(reason),
        // Cannot happen — `one_flow` reads the same sources as the loop above —
        // and if it did it would mean the two roads that look for a flow have
        // parted. Saying so is worth more than panicking in the hands of
        // whoever is using the command.
        Ok(_) => Err(catalogue::say(
            "cli.flow.loads_but_no_source_lists_it",
            &[("name", name)],
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::super::test_support::*;
    use super::super::usage;
    use super::*;
    use std::fs;

    /// **BELOW THREE COSTED RUNS NOTHING IS SUGGESTED, AND IT SAYS WHY.**
    ///
    /// The rule that keeps fault 22 out: in this machine's ledger six runs out
    /// of thirty-four have a cost other than zero, and no flow has three of
    /// them. A median over that column would give zero for every flow, a cap
    /// that stops every run before its first step.
    #[test]
    fn under_three_costed_runs_no_cap_is_suggested() {
        let two = observed_from(&[vec![Some(100)], vec![Some(200)]]);

        let said = what_the_ledger_saw(&two);

        assert!(said.contains("no suggestion"), "{said}");
        // The suggestion line starts on a new line: looking for «suggestion: »
        // without the newline would also find «no suggestion: ».
        assert!(!said.contains("\nsuggestion: "), "{said}");
        assert!(
            said.contains("there are 2"),
            "and it says what is there: {said}"
        );
    }

    /// **WITH THREE, THE SUGGESTION IS THE WORST PLUS THE DEAREST CALL.**
    ///
    /// The twin of the one above: without it a command that never suggested
    /// anything would pass the other and be good for nothing.
    #[test]
    fn with_three_costed_runs_the_suggestion_is_the_worst_plus_the_dearest_call() {
        let three = observed_from(&[
            vec![Some(100), Some(50)],
            vec![Some(400), Some(300)],
            vec![Some(200)],
        ]);

        let said = what_the_ledger_saw(&three);

        // Worst run 700, dearest call 400: 1100.
        assert!(said.contains("\nsuggestion: 1100 micro"), "{said}");
        assert!(!said.contains("no suggestion"), "{said}");
    }

    /// **A RUN THAT SPENT NOTHING IS NOT A SAMPLE.**
    ///
    /// Twenty-eight of the thirty-four runs in this ledger carry zero because
    /// cost *was* the constant zero back then. Counting them would drag every
    /// suggestion towards zero — towards a cap that stops everything — with the
    /// air of a measurement over many samples.
    #[test]
    fn runs_that_spent_nothing_are_not_samples() {
        let seen = observed_from(&[vec![Some(0)], vec![], vec![None, None], vec![Some(900)]]);

        assert_eq!(seen.runs, 4, "every run is there");
        assert_eq!(seen.costed_runs, 1, "but only one of them spent");
        assert_eq!(seen.worst_run_micros, 900);
        assert_eq!(seen.calls_without_cost, 2, "and two calls stay out");
    }

    /// **A SYSTEM FLOW IS NOT REWRITTEN, AND NO NEW ONE APPEARS.**
    ///
    /// It sits inside the binary: there is no file to change. The way is a
    /// namesake at home — created by whoever wants it, knowing they created it.
    /// A flow appearing by itself would change what runs with nobody having
    /// decided it.
    #[test]
    fn a_system_flow_refuses_the_cap_instead_of_growing_a_twin() {
        let home = TestDirectory::new();
        let sources = flow::system::sources(&home.0, None, None);
        let shipped = flow::system::FLOWS[0].0;

        let error = set_cap(&sources, shipped, "1000000").expect_err("a system flow");

        assert!(error.contains("ships inside the binary"), "{error}");
        assert!(
            entries_of(&home.0).is_empty(),
            "no file must have appeared at home: {:?}",
            entries_of(&home.0)
        );
    }

    /// Setting the cap writes **in the directory the flow comes from**, and
    /// touches nothing else in the file.
    #[test]
    fn setting_the_cap_writes_where_the_flow_lives() {
        let home = TestDirectory::new();
        home.write("prova.flow.json", &flow_json("shell_check", "[]", "{}"));
        let sources = flow::system::sources(&home.0, None, None);

        let said = set_cap(&sources, "prova", "750000").expect("the cap is written");

        assert!(said.contains("750000 micro"), "{said}");
        let after = written_flow(&home.0, "prova");
        assert_eq!(after.spend_cap_micros, Some(750_000));
        assert_eq!(after.description, "flusso di prova", "the rest is untouched");
        assert_eq!(after.graph.steps().len(), 1);
        assert_eq!(
            entries_of(&home.0).len(),
            1,
            "and no twin appeared: {:?}",
            entries_of(&home.0)
        );
    }

    /// **A FILE NOT NAMED AFTER ITS OWN `id` IS NOT REWRITTEN.**
    ///
    /// The registry indexes by file name, the write by `id`: where the two
    /// diverge, rewriting would create a second flow instead of replacing this
    /// one. Without the refusal the command would say «done» and leave two
    /// flows with the same `id` in the directory.
    #[test]
    fn a_file_named_differently_from_its_id_is_refused_instead_of_duplicated() {
        let home = TestDirectory::new();
        home.write(
            "altro-nome.flow.json",
            &flow_json("shell_check", "[]", "{}"),
        );
        let sources = flow::system::sources(&home.0, None, None);

        let error = set_cap(&sources, "altro-nome", "500").expect_err("name and id diverge");

        assert!(error.contains("a second flow"), "{error}");
        assert_eq!(entries_of(&home.0).len(), 1, "no twin on disk");
    }

    /// A value that is neither a number nor «none» is refused by saying what a
    /// micro is: getting the unit wrong sets a cap a thousand times lower than
    /// intended, and the run stops with nobody understanding why.
    #[test]
    fn a_cap_that_is_not_a_number_is_refused_with_the_unit_spelled_out() {
        let home = TestDirectory::new();
        home.write("prova.flow.json", &flow_json("shell_check", "[]", "{}"));
        let sources = flow::system::sources(&home.0, None, None);

        let error = set_cap(&sources, "prova", "1,50").expect_err("it is not a number of micros");
        assert!(error.contains("micro"), "{error}");

        let negative = set_cap(&sources, "prova", "-1").expect_err("a negative cap");
        assert!(negative.contains("a negative cap"), "{negative}");
    }

    /// **`none` TAKES THE CAP OFF, AND DOES NOT SET IT TO ZERO.** Without the
    /// word the command could enter a state and not leave it: `0` is «it must
    /// spend nothing», which stops the run before its first step.
    #[test]
    fn the_word_for_no_cap_clears_it_instead_of_setting_zero() {
        let home = TestDirectory::new();
        home.write("prova.flow.json", &flow_json("shell_check", "[]", "{}"));
        let sources = flow::system::sources(&home.0, None, None);
        set_cap(&sources, "prova", "500").expect("first it is set");

        set_cap(&sources, "prova", NO_CAP).expect("then it is taken off");

        assert_eq!(written_flow(&home.0, "prova").spend_cap_micros, None);
    }

    /// **A WORD ALREADY TYPED INTO A SCRIPT KEEPS WORKING.** The vocabulary is
    /// English; the words it replaced stay accepted and stay out of the help.
    /// Without this test the promise is a comment, and the first person to tidy
    /// up `as_written_today` breaks somebody's run with a message that cannot
    /// explain itself.
    #[test]
    fn the_words_that_used_to_be_the_only_ones_still_work() {
        let home = TestDirectory::new();
        home.write("prova.flow.json", &flow_json("shell_check", "[]", "{}"));
        let sources = flow::system::sources(&home.0, None, None);

        set_cap(&sources, "prova", "500").expect("first it is set");
        set_cap(&sources, "prova", "nessuno").expect("yesterday's word takes the cap off");
        assert_eq!(written_flow(&home.0, "prova").spend_cap_micros, None);

        set_schedule(&sources, "prova", "3600s", Some("leggero")).expect("and it picks a weight");
        assert_eq!(
            weight_from("leggero").expect("it is still a weight"),
            weight_from(LIGHT).expect("like today's word"),
            "«leggero» and «{LIGHT}» must say the same thing"
        );
        assert_eq!(
            weight_from("pesante").expect("it is still a weight"),
            weight_from(HEAVY).expect("like today's word")
        );

        set_schedule(&sources, "prova", "nessuno", None).expect("and it takes the trigger off");
        assert!(written_flow(&home.0, "prova").schedule.is_none());
    }

    /// The help does not name them: a retired word appearing where the command
    /// is learnt is not retired, it is the second official spelling.
    #[test]
    fn the_retired_words_are_accepted_and_never_taught() {
        let shown = usage();
        for (retired, _) in RETIRED_WORDS {
            assert!(
                !shown.contains(retired),
                "«{retired}» is retired and the help still teaches it:\n{shown}"
            );
        }
        for current in [NO_CAP, LIGHT, HEAVY] {
            assert!(
                shown.contains(current),
                "«{current}» is today's word and the help does not name it:\n{shown}"
            );
        }
    }

    // ── the trigger is changed from inside Sailor ────────────────────
    //
    // **FAULT 15 TO THE LETTER.** `sailor flow` could list, check and run — and
    // for the gesture that was really needed people left the system, rewriting
    // the JSON by hand with a Python script. A bypassed tool records nothing
    // around it, and no check of its own sees the bypass.

    /// **CHANGING THE TRIGGER IS A COMMAND, AND THE FILE ON DISK SAYS SO.**
    ///
    /// The mutant that breaks it is taking `flow::system::save_in` out of
    /// `set_schedule`: the command would keep answering «done» and the file
    /// would stay as it was — the worst defect of all, because it looks in
    /// every way like the work being done.
    #[test]
    fn the_trigger_of_a_flow_changes_from_inside_sailor() {
        let home = TestDirectory::new();
        home.write("prova.flow.json", &flow_json("shell_check", "[]", "{}"));
        let sources = flow::system::sources(&home.0, None, None);
        assert_eq!(
            written_flow(&home.0, "prova").schedule,
            None,
            "it starts from a flow with no trigger"
        );

        let said =
            set_schedule(&sources, "prova", "3600s", Some(LIGHT)).expect("the trigger is written");

        assert!(said.contains("every 3600s"), "{said}");
        let after = written_flow(&home.0, "prova");
        assert_eq!(
            after.schedule,
            Some(flow::Schedule {
                recurrence: flow::Recurrence::EverySeconds { seconds: 3600 },
                weight: flow::Weight::Light,
                perimeter: vec![],
            })
        );
        assert_eq!(after.description, "flusso di prova", "the rest is untouched");
        assert_eq!(after.graph.steps().len(), 1);
        assert_eq!(
            entries_of(&home.0).len(),
            1,
            "no twin on disk: {:?}",
            entries_of(&home.0)
        );
    }

    /// The hour of the day is the other form the engine can run, and the two go
    /// together: one alone would leave half the command unmeasured.
    #[test]
    fn an_hour_of_the_day_is_the_other_form_the_engine_can_run() {
        let home = TestDirectory::new();
        home.write("prova.flow.json", &flow_json("shell_check", "[]", "{}"));
        let sources = flow::system::sources(&home.0, None, None);

        set_schedule(&sources, "prova", "07:30", Some(HEAVY)).expect("l'ora si scrive");

        assert_eq!(
            written_flow(&home.0, "prova").schedule,
            Some(flow::Schedule {
                recurrence: flow::Recurrence::DailyAt {
                    hour: 7,
                    minute: 30
                },
                weight: flow::Weight::Heavy,
                perimeter: vec![],
            })
        );
    }

    /// **THE WEIGHT IS NOT INVENTED ON A FLOW THAT HAS NONE.**
    ///
    /// The obvious fallback would be «light», and it would be a made-up figure
    /// wearing the face of a declaration: whoever reads the `da-fare` note
    /// would see a weight nobody measured. The refusal writes the line to type,
    /// so it costs a keystroke and not a reading of the code.
    #[test]
    fn a_weight_nobody_declared_is_refused_instead_of_guessed() {
        let home = TestDirectory::new();
        home.write("prova.flow.json", &flow_json("shell_check", "[]", "{}"));
        let sources = flow::system::sources(&home.0, None, None);

        let error =
            set_schedule(&sources, "prova", "3600s", None).expect_err("there is no weight to keep");

        assert!(error.contains(LIGHT) && error.contains(HEAVY), "{error}");
        assert_eq!(
            written_flow(&home.0, "prova").schedule,
            None,
            "a refusal writes nothing"
        );
    }

    /// On a flow that already has a trigger, silence about the weight means
    /// «leave it as it is» — and the declared scope is not lost by changing the
    /// hour: that would be a permission widened by a command about something
    /// else.
    #[test]
    fn changing_only_the_hour_keeps_the_weight_and_the_perimeter() {
        let home = TestDirectory::new();
        home.write("prova.flow.json", &flow_json("shell_check", "[]", "{}"));
        let sources = flow::system::sources(&home.0, None, None);
        let mut with_perimeter = written_flow(&home.0, "prova");
        with_perimeter.schedule = Some(flow::Schedule {
            recurrence: flow::Recurrence::DailyAt { hour: 3, minute: 0 },
            weight: flow::Weight::Heavy,
            perimeter: vec!["~/progetti/sailor".to_owned()],
        });
        flow::system::save_in(&home.0, &with_perimeter).expect("the flow to start from");

        set_schedule(&sources, "prova", "05:15", None).expect("only the hour changes");

        let after = written_flow(&home.0, "prova")
            .schedule
            .expect("the trigger is there");
        assert_eq!(
            after.recurrence,
            flow::Recurrence::DailyAt {
                hour: 5,
                minute: 15
            }
        );
        assert_eq!(after.weight, flow::Weight::Heavy, "the weight stays what it was");
        assert_eq!(
            after.perimeter,
            vec!["~/progetti/sailor".to_owned()],
            "the perimeter is not lost by changing the hour"
        );
    }

    /// **`none` TAKES THE TRIGGER OFF**, as `none` takes the cap off: without
    /// the word the command could enter a state and not leave it, and a flow
    /// that starts by hand alone is a fact, not a gap to fill.
    #[test]
    fn the_word_for_no_trigger_clears_it() {
        let home = TestDirectory::new();
        home.write("prova.flow.json", &flow_json("shell_check", "[]", "{}"));
        let sources = flow::system::sources(&home.0, None, None);
        set_schedule(&sources, "prova", "3600s", Some(LIGHT)).expect("first it is set");

        set_schedule(&sources, "prova", NO_SCHEDULE, None).expect("then it is taken off");

        assert_eq!(written_flow(&home.0, "prova").schedule, None);
        // And absence is written absent, not `null`: whoever rereads their own
        // flow after the command must not find lines nobody wrote.
        let text = fs::read_to_string(home.0.join("prova.flow.json")).expect("rileggere");
        assert!(!text.contains("schedule"), "{text}");
    }

    /// Unrecognised forms are refused **by listing the right ones**: a refusal
    /// that does not say what to type sends people to read the code, outside
    /// the system — fault 15 all over again.
    #[test]
    fn a_trigger_that_is_not_one_of_the_three_forms_says_what_the_three_are() {
        let home = TestDirectory::new();
        home.write("prova.flow.json", &flow_json("shell_check", "[]", "{}"));
        let sources = flow::system::sources(&home.0, None, None);

        for wrong in ["ogni-tanto", "0s", "25:00", "07:70", "3600"] {
            let error =
                set_schedule(&sources, "prova", wrong, Some(LIGHT)).unwrap_or_else(|error| error);
            assert!(
                error.contains(NO_SCHEDULE) || error.contains("hours run from"),
                "«{wrong}» was accepted, or refused without saying how it is written: {error}"
            );
        }
        assert_eq!(
            written_flow(&home.0, "prova").schedule,
            None,
            "none of the wrong shapes wrote anything"
        );
    }

    /// **A SYSTEM FLOW IS NOT REWRITTEN**, and the refusal holds for every
    /// gesture that writes, not for the cap alone: it is the same check, called
    /// by both.
    #[test]
    fn a_system_flow_refuses_the_trigger_too() {
        let home = TestDirectory::new();
        let sources = flow::system::sources(&home.0, None, None);
        let shipped = flow::system::FLOWS[0].0;

        let error = set_schedule(&sources, shipped, "3600s", Some(LIGHT))
            .expect_err("a system flow");

        assert!(error.contains("ships inside the binary"), "{error}");
        assert!(
            entries_of(&home.0).is_empty(),
            "no file must have appeared at home: {:?}",
            entries_of(&home.0)
        );
    }

    /// **FAULT 41: THE COMMAND WROTE INTO ANOTHER FLOW'S FILE AND SAID «DONE».**
    ///
    /// The check inherited from `set_cap` asked whether `<id>.flow.json`
    /// **existed**, not whether it was *that* file. It held identically for
    /// `cap`, so both gestures are tested here or the repair would cover half
    /// the surface. The mutant: `target.exists()` back in place of the names.
    #[test]
    fn a_flow_whose_file_is_named_after_another_one_is_never_written_through() {
        let home = TestDirectory::new();
        // Two files, the same `id` inside: the registry indexes them by file
        // name, the write by `id`.
        home.write("prova.flow.json", &flow_json("shell_check", "[]", "{}"));
        home.write(
            "nome-diverso.flow.json",
            &flow_json("shell_check", "[]", "{}"),
        );
        let sources = flow::system::sources(&home.0, None, None);

        let refused = set_schedule(&sources, "nome-diverso", "3600s", Some(LIGHT))
            .expect_err("the file name is not the id");
        assert!(refused.contains("a second flow"), "{refused}");

        let refused_cap = set_cap(&sources, "nome-diverso", "500000")
            .expect_err("the same refusal holds for the cap");
        assert!(refused_cap.contains("a second flow"), "{refused_cap}");

        // **AND THE PART THAT COUNTS: THE BYSTANDER FLOW WAS NOT TOUCHED.** A
        // refusal that wrote anyway would be worse than the defect.
        let bystander = written_flow(&home.0, "prova");
        assert_eq!(
            bystander.schedule, None,
            "the trigger of «prova» is not touched"
        );
        assert_eq!(bystander.spend_cap_micros, None, "nor its cap");
        assert_eq!(written_flow(&home.0, "nome-diverso").schedule, None);
        assert_eq!(
            entries_of(&home.0).len(),
            2,
            "and no third file appeared: {:?}",
            entries_of(&home.0)
        );
    }

    /// A flow living in a `.json` without `.flow` is not rewritten: the write
    /// would go to a file other than the one read, and a twin would be born.
    /// Same defect as fault 50 from the other side — the file read and the file
    /// written must be the one file.
    #[test]
    fn a_flow_read_from_a_plain_json_is_refused_instead_of_duplicated() {
        let home = TestDirectory::new();
        home.write("prova.json", &flow_json("shell_check", "[]", "{}"));
        let sources = flow::system::sources(&home.0, None, None);

        let refused = set_schedule(&sources, "prova", "3600s", Some(LIGHT))
            .expect_err("the file read is not the one that would be written");

        assert!(refused.contains("a second flow"), "{refused}");
        assert_eq!(entries_of(&home.0).len(), 1, "no twin on disk");
    }

    /// Reading the trigger is a gesture of its own: whoever does not know what
    /// is there does not know what they are changing, and `flow list` hides it.
    #[test]
    fn asking_for_the_trigger_says_what_is_there_and_what_is_not() {
        let home = TestDirectory::new();
        home.write("prova.flow.json", &flow_json("shell_check", "[]", "{}"));
        let sources = flow::system::sources(&home.0, None, None);

        let before = schedule_of(&sources, "prova").expect("it reads");
        assert!(before.contains(NO_SCHEDULE), "{before}");

        set_schedule(&sources, "prova", "300s", Some(HEAVY)).expect("it is set");

        let after = schedule_of(&sources, "prova").expect("it reads back");
        assert!(after.contains("every 300s"), "{after}");
        assert!(after.contains(HEAVY), "{after}");
        assert!(
            after.contains("not declared"),
            "an empty scope says so: {after}"
        );
    }

    fn written_flow(dir: &std::path::Path, name: &str) -> FlowFile {
        let text = fs::read_to_string(dir.join(format!("{name}.flow.json")))
            .expect("the written flow reads back");
        serde_json::from_str(&text).expect("and it deserializes")
    }

    fn entries_of(dir: &std::path::Path) -> Vec<String> {
        fs::read_dir(dir)
            .map(|entries| {
                entries
                    .flatten()
                    .map(|entry| entry.file_name().to_string_lossy().into_owned())
                    .collect()
            })
            .unwrap_or_default()
    }
}
