//! A descriptor may say whether what goes through it trains the provider's
//! next model, in one of three words; a fourth word refuses the descriptor.

use models::pact::DataPact;
use toolbox::descriptor::Descriptor;

#[test]
fn a_pact_outside_the_three_words_refuses_the_descriptor() {
    let refused = serde_json::from_str::<Descriptor>(
        r#"{"id": "x", "family": "ai_cli", "data_pact": "sometimes"}"#,
    )
    .expect_err("a fourth word is refused");
    assert!(refused.to_string().contains("sometimes"), "{refused}");
}

#[test]
fn the_three_words_read_and_silence_is_unknown() {
    let parsed = |text: &str| serde_json::from_str::<Descriptor>(text).expect("parses");
    let silent = parsed(r#"{"id": "x", "family": "ai_cli"}"#);
    assert_eq!(silent.data_pact, None, "nobody measured is not a no");
    let said = parsed(r#"{"id": "x", "family": "ai_cli", "data_pact": "does_not_train"}"#);
    assert_eq!(said.data_pact, Some(DataPact::DoesNotTrain));
    let trains = parsed(r#"{"id": "x", "family": "ai_cli", "data_pact": "trains"}"#);
    assert_eq!(trains.data_pact, Some(DataPact::Trains));
}
