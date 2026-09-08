//! Every name the ledger keeps runs under gets an answer, and never silence.
//!
//! A renamed or dropped flow leaves its runs behind under the old name, and
//! asking the catalogue about one used to end in «no flow is called that,
//! here are the others» — which reads as a typo while the runs sit in the
//! ledger. «Nothing carries it» is an answer, so it is written as one.

use flow::system::{self, PastName};

/// The names asked about here, written out rather than read from this machine.
/// A judge reading the ledger of whoever runs it gives a verdict on their
/// machine, not on the catalogue — and it would be green on an empty one.
const NAMES_THE_LEDGER_HOLDS: &[&str] = &[
    "accendi-la-macchina",
    "allinea-con-develop",
    "c-e-una-via-per-notion",
    "che-cosa-gira",
    "che-sappiamo-di-noi",
    "il-giro-di-prova",
    "mandato-corrente",
    "migrazione-a-sailor",
    "passa-il-testimone",
    "prova-dei-turni",
    "prova-rapporto",
    "prova-research-usa-e-getta",
    "prova-scorre",
    "qualcuno-ha-spinto-su-develop",
    "rotto",
    "route-a-breakage-mutant",
    "smista-il-lavoro",
    "spegni-la-macchina",
    "strumenti-di-questa-macchina",
    "try-dormant-steps",
    "try-unused-actions",
];

/// The renames git recorded under `--diff-filter=R`, with the steps of the two
/// files matching. Nothing else earns a successor: a plausible pair is a guess,
/// and a guess here sends somebody to the wrong flow with no way to tell.
const RENAMES_GIT_RECORDED: &[(&str, &str)] = &[
    ("migrazione-a-sailor", "migrate-to-sailor"),
    ("smista-il-lavoro", "dispatch-the-work"),
    ("strumenti-di-questa-macchina", "what-this-machine-has"),
];

fn is_shipped(name: &str) -> bool {
    system::FLOWS.iter().any(|(shipped, _)| *shipped == name)
}

#[test]
fn every_name_the_ledger_holds_gets_an_answer() {
    let silent: Vec<&str> = NAMES_THE_LEDGER_HOLDS
        .iter()
        .copied()
        .filter(|name| !is_shipped(name) && system::past_name(name).is_none())
        .collect();
    assert!(
        silent.is_empty(),
        "the catalogue answers nothing for names the ledger keeps runs under: {silent:?}"
    );
}

#[test]
fn a_name_carried_on_names_a_flow_that_ships() {
    for (past, became) in system::PAST_NAMES {
        let PastName::CarriedOnBy(now) = became else {
            continue;
        };
        assert!(
            is_shipped(now),
            "«{past}» is answered with «{now}», which no shipped flow is called"
        );
    }
}

#[test]
fn only_a_rename_git_recorded_gets_a_successor() {
    for (past, became) in system::PAST_NAMES {
        let recorded = RENAMES_GIT_RECORDED
            .iter()
            .find(|(from, _)| from == past)
            .map(|(_, to)| *to);
        match became {
            PastName::CarriedOnBy(now) => assert_eq!(
                Some(*now),
                recorded,
                "«{past}» is sent to «{now}» with no rename behind it"
            ),
            PastName::NothingCarriesIt => assert_eq!(
                None, recorded,
                "«{past}» was renamed and the catalogue does not say so"
            ),
        }
    }
}

/// One name was proposed as the old form of `watch-the-crew` and shares no
/// step with it. A disproof is kept as a test: a proposal once written down
/// comes back.
#[test]
fn a_disproved_rename_is_not_written_in() {
    assert_eq!(
        Some(PastName::NothingCarriesIt),
        system::past_name("che-cosa-gira"),
        "«che-cosa-gira» shares no step with any shipped flow"
    );
}

#[test]
fn no_name_is_both_shipped_and_past() {
    let both: Vec<&str> = system::PAST_NAMES
        .iter()
        .map(|(past, _)| *past)
        .filter(|past| is_shipped(past))
        .collect();
    assert!(
        both.is_empty(),
        "these ship and are answered for as past names, so the two answers disagree: {both:?}"
    );
}

/// The answer reaches a person as a sentence, not as the key of one: the
/// catalogue falls back to the bare key, and that fallback is what a missing
/// entry looks like at the command line.
#[test]
fn the_answer_is_a_sentence_and_not_a_key() {
    for key in [
        "cli.flow.that_name_is_carried_on",
        "cli.flow.that_name_is_past",
    ] {
        let said = catalogue::say(key, &[("flow", "che-cosa-gira"), ("now", "watch-the-crew")]);
        assert_ne!(key, said, "the catalogue has no sentence for {key}");
        assert!(
            said.contains("che-cosa-gira"),
            "the sentence for {key} does not name the flow asked about: {said}"
        );
    }
}
