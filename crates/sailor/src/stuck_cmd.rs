//! `sailor stuck`: which steps of which flows keep breaking, and on what. It
//! reads the store and calls no engine, so asking costs nothing.
//!
//! **A RED RUN NOBODY READS IS A GREEN TREE.** Every gate here passes on
//! sources; a step that breaks on a value only the machine has is invisible to
//! all of them, and the store has held every one of those breaks.

use ledger::{BreakingStep, HandoverMissed, Ledger, StillbornRun};

/// How many breaks make a pattern rather than an accident.
const A_PATTERN_STARTS_AT: u64 = 3;

/// How much of a complaint is worth printing: the rest is a stack trace.
const A_COMPLAINT_IN_ONE_LINE: usize = 120;

pub fn run(args: &[String]) -> i32 {
    match dispatch(args) {
        Ok(message) => {
            println!("{message}");
            0
        }
        Err(message) => {
            eprintln!("sailor stuck: {message}");
            1
        }
    }
}

/// The shape of `sailor stuck`. See `flow_cmd::USAGE`.
pub const USAGE: &[crate::Form] = &[crate::Form {
    form: "sailor stuck [--at-least <n>]",
    says_key: "",
}];

fn dispatch(args: &[String]) -> Result<String, String> {
    let at_least = at_least_in(args)?;
    let directory = ledger::default_directory()
        .ok_or_else(|| catalogue::say("cli.stuck.no_home_no_store", &[]))?;
    let ledger = Ledger::open(&directory).map_err(|error| error.to_string())?;
    let found = ledger
        .steps_that_keep_breaking(at_least)
        .map_err(|error| error.to_string())?;
    let stillborn = ledger
        .runs_that_died_before_a_step(at_least)
        .map_err(|error| error.to_string())?;
    let never_emptied = ledger
        .handovers_owed_and_missed(at_least)
        .map_err(|error| error.to_string())?;
    Ok([
        report(&found, at_least),
        before_a_step(&stillborn),
        never_emptied_section(&never_emptied),
    ]
        .into_iter()
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>()
        .join("\n\n"))
}

fn at_least_in(args: &[String]) -> Result<u64, String> {
    match args {
        [] => Ok(A_PATTERN_STARTS_AT),
        [flag, value] if flag == "--at-least" => value
            .parse()
            .map_err(|_| catalogue::say("cli.stuck.at_least_is_a_number", &[("value", value)])),
        _ => Err(format!(
            "{} {}",
            catalogue::say("cli.usage_heading", &[]),
            USAGE[0].form
        )),
    }
}

/// **PUBLIC SO A TEST READS THE WORDS THE PERSON READS.** The times a step went
/// are printed beside the times it broke: a count on its own accuses the busy.
pub fn report(found: &[BreakingStep], at_least: u64) -> String {
    if found.is_empty() {
        return catalogue::say(
            "cli.stuck.nothing_keeps_breaking",
            &[("at_least", &at_least.to_string())],
        );
    }
    let mut lines = vec![catalogue::say(
        "cli.stuck.steps_that_keep_breaking",
        &[
            ("count", &found.len().to_string()),
            ("at_least", &at_least.to_string()),
        ],
    )];
    for step in found {
        lines.push(catalogue::say(
            "cli.stuck.one_step",
            &[
                ("broke", &step.broke.to_string()),
                ("went", &step.went.to_string()),
                ("flow", &step.flow),
                ("step", &step.step_id),
                ("last", &last_break(step.last_at)),
            ],
        ));
        if let Some(detail) = detail_of(step) {
            lines.push(format!("      {detail}"));
        }
    }
    lines.push(catalogue::say("cli.stuck.a_break_is_not_a_fault", &[]));
    lines.join("\n")
}

/// **A COUNT CANNOT FALL, SO A CURED STEP STAYS AT THE TOP.** When it last
/// broke is the only part of a lifetime total that says whether the cure held.
fn last_break(at: i64) -> String {
    if at <= 0 {
        return catalogue::say("cli.stuck.never_dated", &[]);
    }
    let ago = (machine::now() - at).max(0);
    if ago < 86_400 {
        return catalogue::say("cli.stuck.hours_ago", &[("hours", &(ago / 3_600).to_string())]);
    }
    catalogue::say("cli.stuck.days_ago", &[("days", &(ago / 86_400).to_string())])
}

/// The kind of break first, then the words: the store already tells a broken
/// contract from a refusal that is working, and a list that hides the kind
/// makes a reader open every one of them to find out.
fn detail_of(step: &BreakingStep) -> Option<String> {
    let said = step
        .said
        .as_deref()
        .map(in_one_line)
        .filter(|said| !said.is_empty());
    match (step.failure_class.as_deref(), said) {
        (Some(kind), Some(said)) => Some(format!("{kind} · {said}")),
        (Some(kind), None) => Some(kind.to_owned()),
        (None, said) => said,
    }
}

/// A complaint as one line: a report that unfolds a stack trace is one nobody
/// finishes reading.
fn in_one_line(said: &str) -> String {
    let flat: String = said
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .collect::<Vec<_>>()
        .join(" ");
    match flat.char_indices().nth(A_COMPLAINT_IN_ONE_LINE) {
        Some((at, _)) => format!("{}…", &flat[..at]),
        None => flat,
    }
}

/// **A RUN REFUSED AT ITS OWN INPUT LEAVES NO STEP TO COUNT.** Every reading
/// above is step-shaped, so without this one a flow can fail every half hour
/// for days and read as silence.
pub fn before_a_step(found: &[StillbornRun]) -> String {
    if found.is_empty() {
        return String::new();
    }
    let mut lines = vec![catalogue::say(
        "cli.stuck.died_before_a_step",
        &[("count", &found.len().to_string())],
    )];
    for run in found {
        lines.push(catalogue::say(
            "cli.stuck.one_stillborn",
            &[
                ("times", &run.times.to_string()),
                ("flow", &run.flow),
                ("last", &last_break(run.last_at)),
            ],
        ));
        if let Some(error) = run.error.as_deref() {
            lines.push(format!("      {}", in_one_line(error)));
        }
    }
    lines.join("\n")
}

/// **A HANDOVER OWED AND NOT MADE ENDS ITS RUN GREEN.** The relay skips the
/// emptying when the screen is never free, the run completes, and the session
/// goes on to be compacted with nothing having named the miss.
pub fn never_emptied_section(found: &[HandoverMissed]) -> String {
    if found.is_empty() {
        return String::new();
    }
    let mut lines = vec![catalogue::say(
        "cli.stuck.handover_owed_and_missed",
        &[("count", &found.len().to_string())],
    )];
    for terminal in found {
        lines.push(catalogue::say(
            "cli.stuck.one_never_emptied",
            &[
                ("missed", &terminal.missed().to_string()),
                ("owed", &terminal.owed.to_string()),
                ("tty", &terminal.tty),
                ("last", &last_break(terminal.last_at)),
            ],
        ));
        if let Some(said) = terminal.said.as_deref() {
            lines.push(format!("      {}", in_one_line(said)));
        }
    }
    lines.push(catalogue::say("cli.stuck.a_missed_handover_ends_green", &[]));
    lines.join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn breaking(flow: &str, step: &str, broke: u64, went: u64, said: &str) -> BreakingStep {
        classed(flow, step, broke, went, said, None)
    }

    fn classed(
        flow: &str,
        step: &str,
        broke: u64,
        went: u64,
        said: &str,
        failure_class: Option<&str>,
    ) -> BreakingStep {
        BreakingStep {
            flow: flow.to_owned(),
            step_id: step.to_owned(),
            broke,
            went,
            failure_class: failure_class.map(str::to_owned),
            said: (!said.is_empty()).then(|| said.to_owned()),
            last_at: 0,
        }
    }

    #[test]
    fn a_store_with_no_pattern_in_it_says_so_rather_than_printing_a_heading() {
        let said = report(&[], 3);
        assert!(
            !said.is_empty() && !said.contains('\n'),
            "an empty store printed a list: {said}"
        );
    }

    #[test]
    fn a_step_is_named_with_the_times_it_worked_beside_the_times_it_broke() {
        let said = report(&[breaking("lab-nodes-status", "check", 154, 132, "")], 3);
        for held in ["154", "132", "lab-nodes-status", "check"] {
            assert!(said.contains(held), "«{held}» is not in the report: {said}");
        }
    }

    /// **A STACK TRACE IS NOT A COMPLAINT.**
    #[test]
    fn a_complaint_of_many_lines_is_printed_as_one() {
        let trace = "refused by command\nTraceback (most recent call last):\n  File \"x\"";
        let said = report(&[breaking("a-flow", "a-step", 9, 0, trace)], 3);
        assert_eq!(
            said.lines().count(),
            4,
            "the trace unfolded into the report: {said}"
        );
        assert!(said.contains("Traceback"), "the complaint was lost: {said}");
    }

    #[test]
    fn a_complaint_longer_than_the_line_is_cut_and_says_so() {
        let long = "x".repeat(A_COMPLAINT_IN_ONE_LINE * 2);
        let said = report(&[breaking("a-flow", "a-step", 9, 0, &long)], 3);
        assert!(said.contains('…'), "a cut complaint did not say it was cut");
        assert!(
            !said.contains(&long),
            "the whole complaint was printed anyway"
        );
    }

    /// **A REFUSAL THAT WORKS LOOKS LIKE A DEFECT WITHOUT ITS KIND.**
    #[test]
    fn the_kind_of_break_is_named_before_the_words() {
        let said = report(
            &[classed("a-flow", "a-step", 9, 0, "it saw «[]»", Some("answer_off_shape"))],
            3,
        );
        assert!(
            said.contains("answer_off_shape · it saw"),
            "the kind is missing or not first: {said}"
        );
    }

    #[test]
    fn a_break_the_store_never_classed_still_shows_its_words() {
        let said = report(&[breaking("a-flow", "a-step", 9, 0, "it fell over")], 3);
        assert!(said.contains("it fell over"), "the words were lost: {said}");
    }

    /// **A CURED STEP KEEPS ITS COUNT.** Without a date the list cannot say
    /// which of these is still happening.
    #[test]
    fn a_step_says_when_it_last_broke() {
        let now = machine::now();
        let mut step = breaking("a-flow", "a-step", 9, 0, "");
        step.last_at = now - 3 * 86_400;
        assert!(report(&[step], 3).contains('3'), "the days are not said");
        let mut never = breaking("a-flow", "a-step", 9, 0, "");
        never.last_at = 0;
        let said = report(&[never], 3);
        assert!(!said.contains(&now.to_string()), "a raw clock reached the reader");
    }

    fn stillborn(flow: &str, times: u64, error: Option<&str>) -> StillbornRun {
        StillbornRun {
            flow: flow.to_owned(),
            times,
            last_at: 0,
            error: error.map(str::to_owned),
        }
    }

    /// **A RUN REFUSED AT ITS OWN INPUT LEAVES NO STEP TO COUNT.**
    #[test]
    fn a_flow_whose_runs_never_began_is_named_with_what_refused_them() {
        let said = before_a_step(&[stillborn(
            "keep-the-index-fresh",
            74,
            Some("$.text: expected required property"),
        )]);
        for held in ["74", "keep-the-index-fresh", "$.text"] {
            assert!(said.contains(held), "«{held}» is missing: {said}");
        }
    }

    #[test]
    fn no_stillborn_runs_print_no_second_section_at_all() {
        assert!(before_a_step(&[]).is_empty(), "an empty section was printed");
    }

    #[test]
    fn a_refusal_of_many_lines_is_printed_as_one() {
        let said = before_a_step(&[stillborn("a-flow", 9, Some("refused\n  at $.x\n  and $.y"))]);
        assert_eq!(said.lines().count(), 3, "the refusal unfolded: {said}");
    }

    fn never_emptied(tty: &str, owed: u64, made: u64, said: Option<&str>) -> HandoverMissed {
        HandoverMissed {
            tty: tty.to_owned(),
            owed,
            made,
            last_at: 0,
            said: said.map(str::to_owned),
        }
    }

    /// **THE MISS IS THE NUMBER, AND THE CHANCES ARE THE CONTEXT.** A count of
    /// misses alone accuses a terminal that was asked once.
    #[test]
    fn a_terminal_that_never_hands_on_is_named_with_what_refused_it() {
        let said = never_emptied_section(&[never_emptied(
            "ttys015",
            8,
            0,
            Some("ttys015: «\u{25ef} » is on the screen, so somebody is being waited for"),
        )]);
        for held in ["8", "ttys015", "on the screen"] {
            assert!(said.contains(held), "«{held}» is missing: {said}");
        }
    }

    #[test]
    fn a_terminal_that_hands_on_every_time_is_not_in_the_section() {
        assert!(
            never_emptied_section(&[]).is_empty(),
            "an empty section was printed"
        );
        assert_eq!(never_emptied("ttys003", 8, 8, None).missed(), 0);
        assert_eq!(never_emptied("ttys003", 8, 2, None).missed(), 6);
    }

    /// A made handover can never outnumber the chances it was given.
    #[test]
    fn more_made_than_owed_does_not_wrap_around() {
        assert_eq!(never_emptied("ttys003", 1, 4, None).missed(), 0);
    }

    #[test]
    fn the_threshold_is_read_from_the_line_and_falls_back_to_a_pattern() {
        assert_eq!(at_least_in(&[]).expect("no argument"), A_PATTERN_STARTS_AT);
        assert_eq!(
            at_least_in(&["--at-least".to_owned(), "20".to_owned()]).expect("a number"),
            20
        );
        assert!(at_least_in(&["--at-least".to_owned(), "many".to_owned()]).is_err());
        assert!(at_least_in(&["--whatever".to_owned()]).is_err());
    }
}
