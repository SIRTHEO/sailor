//! The three cases of a spend, asked on their own. A rule nobody asks on its
//! own lives only inside whoever uses it: `Spend` has documented three cases
//! since it was born, and for all that time the only way to ask was
//! `is_complete()`. When `sailor flow cost` printed "1.6674" for a run that had
//! cost 7.2080, no test was red — the distinction was right in the engine and
//! never reached the reader. Here the rule has a test of its own.

use flow::{CostReading, Spend};

/// How the store sums up a run: how many calls, how much known cost, and how
/// many of those calls said nothing.
fn spent(micros: i64, calls: i64, calls_without_cost: i64) -> Spend {
    Spend {
        micros,
        calls,
        calls_without_cost,
        dearest_micros: None,
    }
}

/// The easy case, and it has to be here: without it, a reading that always
/// declared itself incomplete would pass the other two tests.
#[test]
fn every_call_measured_reads_as_the_total() {
    assert_eq!(
        spent(1_667_400, 4, 0).reading(),
        CostReading::Exact(1_667_400)
    );
}

/// One silent call turns the reading around. These are the real numbers of the
/// handed-off run from the A/B: four calls, three with no known cost, and the
/// fourth at 1.6674 that `sailor flow cost` presented as the total of a run
/// that had cost 7.2080.
#[test]
fn one_unmeasured_call_turns_the_total_into_a_floor() {
    assert_eq!(
        spent(1_667_400, 4, 3).reading(),
        CostReading::AtLeast {
            known_micros: 1_667_400,
            calls: 4,
            calls_without_cost: 3,
        },
        "with a silent call the number is a floor, not a sum"
    );
}

/// Spending zero is not the same as spending an unknown amount — the third
/// case. Both runs have the same `micros`; only one really spent zero.
#[test]
fn nothing_spent_and_nothing_known_are_two_different_readings() {
    assert_eq!(spent(0, 2, 0).reading(), CostReading::Nothing);
    assert_eq!(
        spent(0, 2, 2).reading(),
        CostReading::AtLeast {
            known_micros: 0,
            calls: 2,
            calls_without_cost: 2,
        },
        "no measurement is not \"spent zero\": these are two different runs"
    );
}

/// The bridge between the boolean and the three cases: the reading must go
/// through `is_complete()`, not a second comparison beside it.
///
/// Said plainly, this does not catch the mutant that matters — it compares two
/// things that move together, so it stays green. The two tests above catch it;
/// this goes red the day someone rewrites `reading()` on its own comparison.
#[test]
fn the_reading_agrees_with_what_the_engine_calls_complete() {
    for spend in [
        spent(0, 0, 0),
        spent(500, 1, 0),
        spent(500, 3, 2),
        spent(0, 1, 1),
    ] {
        let reads_as_a_floor = matches!(spend.reading(), CostReading::AtLeast { .. });
        assert_eq!(
            reads_as_a_floor,
            !spend.is_complete(),
            "a floor and an incomplete total are one thing said twice: {spend:?}"
        );
    }
}
