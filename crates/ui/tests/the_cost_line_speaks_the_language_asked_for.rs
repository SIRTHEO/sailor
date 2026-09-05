//! The cost line and the undeclared-model bucket come from the catalogue, in
//! the language asked for. Its own binary, and so its own process: the
//! language is read from the environment, and setting it beside the crate's
//! other tests would decide theirs too.

use std::collections::BTreeMap;

/// **THE ABSURD CONTROL IS THE OTHER LANGUAGE.** Compared in English, a line
/// written back into the code would equal the catalogue's and stay green.
#[test]
fn asked_in_the_other_language_every_line_answers_in_it() {
    std::env::set_var(catalogue::LANGUAGE_VARIABLE, "it");

    let readings: Vec<(flow::CostReading, &str, BTreeMap<&str, String>)> = vec![
        (flow::CostReading::Nothing, "ui.cost.nothing", BTreeMap::new()),
        (
            flow::CostReading::Exact(1_667_400),
            "ui.cost.exact",
            BTreeMap::from([("units", "1.6674".to_owned())]),
        ),
        (
            flow::CostReading::AtLeast {
                known_micros: 0,
                calls: 3,
                calls_without_cost: 3,
            },
            "ui.cost.unknown",
            BTreeMap::from([("calls", "3".to_owned())]),
        ),
        (
            flow::CostReading::AtLeast {
                known_micros: 1_667_400,
                calls: 4,
                calls_without_cost: 3,
            },
            "ui.cost.at_least",
            BTreeMap::from([
                ("units", "1.6674".to_owned()),
                ("calls", "4".to_owned()),
                ("calls_without_cost", "3".to_owned()),
            ]),
        ),
    ];
    for (reading, key, values) in &readings {
        let values: Vec<(&str, &str)> = values
            .iter()
            .map(|(name, value)| (*name, value.as_str()))
            .collect();
        let italian = catalogue::look("it", key, &values).expect("the key is declared");
        let english = catalogue::look("en", key, &values).expect("the key is declared");
        assert_ne!(italian, english, "{key} is a copy of the source, and proves nothing");
        assert_eq!(
            ui::dashboard::how_the_cost_reads(reading),
            italian,
            "{key}: the line is not what the catalogue says in the language asked for"
        );
    }

    let bucket = catalogue::look("it", "ui.cost.model_not_declared", &[]).expect("declared");
    assert_ne!(bucket, catalogue::look("en", "ui.cost.model_not_declared", &[]).expect("declared"));
    assert_eq!(ui::dashboard::model_not_declared(), bucket);
}
