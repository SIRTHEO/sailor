//! `sailor stuck`: which steps of which flows keep breaking, and on what. It
//! reads the store and calls no engine, so asking costs nothing.
//!
//! **A RED RUN NOBODY READS IS A GREEN TREE.** Every gate here passes on
//! sources; a step that breaks on a value only the machine has is invisible to
//! all of them, and the store has held every one of those breaks.

use ledger::{BreakingStep, Ledger};

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
    Ok(report(&found, at_least))
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
            ],
        ));
        if let Some(said) = step.said.as_deref().filter(|said| !said.trim().is_empty()) {
            lines.push(format!("      {}", in_one_line(said)));
        }
    }
    lines.push(catalogue::say("cli.stuck.a_break_is_not_a_fault", &[]));
    lines.join("\n")
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

#[cfg(test)]
mod tests {
    use super::*;

    fn breaking(flow: &str, step: &str, broke: u64, went: u64, said: &str) -> BreakingStep {
        BreakingStep {
            flow: flow.to_owned(),
            step_id: step.to_owned(),
            broke,
            went,
            failure_class: None,
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
