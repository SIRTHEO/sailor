//! `sailor repeats`: how much a relaunch would have saved by answering a
//! repeated call from the record instead of calling the engine again. It
//! reads the store and calls no engine, so asking costs nothing.
//!
//! Answering from the record is worth its risk — a stale answer on a changed
//! tree — only if repeats are common, and that number is already in the store.

use ledger::{Ledger, RepeatedCalls};

/// A cost as the store keeps it: micros of a currency unit.
const MICROS_IN_A_UNIT: f64 = 1_000_000.0;

pub fn run(args: &[String]) -> i32 {
    match dispatch(args) {
        Ok(message) => {
            println!("{message}");
            0
        }
        Err(message) => {
            eprintln!("sailor repeats: {message}");
            1
        }
    }
}

/// The shape of `sailor repeats`. See `flow_cmd::USAGE`.
pub const USAGE: &[crate::Form] = &[crate::Form {
    form: "sailor repeats",
    says_key: "",
}];

fn dispatch(args: &[String]) -> Result<String, String> {
    if !args.is_empty() {
        return Err(format!(
            "{} {}",
            catalogue::say("cli.usage_heading", &[]),
            USAGE[0].form
        ));
    }
    let directory = ledger::default_directory()
        .ok_or_else(|| catalogue::say("cli.repeats.no_home_no_store", &[]))?;
    let ledger = Ledger::open(&directory).map_err(|error| error.to_string())?;
    let tally = ledger
        .repeated_engine_calls()
        .map_err(|error| error.to_string())?;
    Ok(report(&tally))
}

/// **PUBLIC SO A TEST READS THE WORDS THE PERSON READS.** A report checked
/// through captured output is checked one layer away from the reader.
///
/// The caveats print every time, never only when they are large: a share with
/// nothing beside it reads as money already saved.
pub fn report(tally: &RepeatedCalls) -> String {
    if tally.calls == 0 {
        return catalogue::say("cli.repeats.nothing_to_measure", &[]);
    }
    let share = format!("{:.1}", tally.served as f64 * 100.0 / tally.calls as f64);
    let mut lines = vec![
        catalogue::say(
            "cli.repeats.calls_recorded",
            &[
                ("calls", &tally.calls.to_string()),
                ("spent", &in_units(tally.spent_micros)),
            ],
        ),
        catalogue::say(
            "cli.repeats.would_have_been_answered",
            &[
                ("served", &tally.served.to_string()),
                ("share", &share),
                ("saved", &in_units(tally.served_micros)),
            ],
        ),
    ];
    if tally.served_on_an_unresolved_prompt > 0 {
        lines.push(catalogue::say(
            "cli.repeats.matched_on_a_pointer",
            &[("count", &tally.served_on_an_unresolved_prompt.to_string())],
        ));
    }
    if tally.served_without_a_cost > 0 {
        lines.push(catalogue::say(
            "cli.repeats.saved_but_never_priced",
            &[("count", &tally.served_without_a_cost.to_string())],
        ));
    }
    if tally.calls_without_a_key > 0 {
        lines.push(catalogue::say(
            "cli.repeats.no_step_so_no_key",
            &[("count", &tally.calls_without_a_key.to_string())],
        ));
    }
    lines.push(catalogue::say("cli.repeats.the_share_is_a_ceiling", &[]));
    lines.join("\n")
}

fn in_units(micros: i64) -> String {
    format!("{:.2}", micros as f64 / MICROS_IN_A_UNIT)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tally() -> RepeatedCalls {
        RepeatedCalls {
            calls: 55,
            served: 5,
            served_on_an_unresolved_prompt: 2,
            served_micros: 4_957_937,
            served_without_a_cost: 4,
            calls_without_a_key: 0,
            spent_micros: 37_886_794,
        }
    }

    #[test]
    fn the_report_says_the_share_and_that_it_is_a_ceiling() {
        let said = report(&tally());
        assert!(said.contains("55"), "{said}");
        assert!(said.contains("9.1"), "{said}");
        assert!(
            said.contains(&catalogue::say("cli.repeats.the_share_is_a_ceiling", &[])),
            "{said}"
        );
    }

    #[test]
    fn a_false_match_is_never_folded_into_the_share() {
        let said = report(&tally());
        assert!(
            said.contains(&catalogue::say(
                "cli.repeats.matched_on_a_pointer",
                &[("count", "2")]
            )),
            "{said}"
        );
        assert!(
            said.contains(&catalogue::say(
                "cli.repeats.saved_but_never_priced",
                &[("count", "4")]
            )),
            "{said}"
        );
    }

    #[test]
    fn an_empty_store_says_so_instead_of_saying_nothing_was_saved() {
        let said = report(&RepeatedCalls::default());
        assert_eq!(said, catalogue::say("cli.repeats.nothing_to_measure", &[]));
        assert!(!said.contains("0.0"), "{said}");
    }

    #[test]
    fn a_word_after_the_verb_is_refused() {
        let refused = dispatch(&["extra".to_owned()]).expect_err("it takes no argument");
        assert!(refused.contains("sailor repeats"), "{refused}");
    }
}
