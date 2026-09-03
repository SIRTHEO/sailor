//! A profile may send its command line to another endpoint, unmodified and
//! with nothing in between: only where the endpoint speaks the command line's
//! own protocol, and only with a key the machine holds.

use profiles::{endpoint_environment, find_cli, Profile, ProfileEndpoint};
use std::path::PathBuf;

fn profile(endpoint: Option<ProfileEndpoint>) -> Profile {
    Profile {
        name: "altrove".to_owned(),
        cli_id: "codex".to_owned(),
        home_dir: PathBuf::from("/case/codex/altrove"),
        endpoint,
    }
}

fn endpoint(protocol: &str) -> ProfileEndpoint {
    ProfileEndpoint {
        url: "http://localhost:11434/v1".to_owned(),
        key_var: "A_KEY_ON_THIS_MACHINE".to_owned(),
        protocol: protocol.to_owned(),
    }
}

#[test]
fn a_native_endpoint_becomes_the_two_variables_and_the_key_is_never_in_the_store() {
    let cli = find_cli("codex").expect("known");
    // The control first: no endpoint, nothing to point.
    assert!(endpoint_environment(cli, &profile(None), &|_| None).expect("fits").is_empty());

    let keys = |variable: &str| (variable == "A_KEY_ON_THIS_MACHINE").then(|| "the-key".to_owned());
    let env = endpoint_environment(cli, &profile(Some(endpoint("openai-responses"))), &keys).expect("native");
    assert_eq!(env.get("OPENAI_BASE_URL").map(String::as_str), Some("http://localhost:11434/v1"));
    assert_eq!(env.get("OPENAI_API_KEY").map(String::as_str), Some("the-key"));
    let written = serde_json::to_string(&profile(Some(endpoint("openai-responses")))).expect("serialises");
    assert!(!written.contains("the-key"), "the key stays on the machine: {written}");
}

#[test]
fn a_foreign_protocol_a_missing_key_and_a_line_with_no_variable_are_refused_by_name() {
    let cli = find_cli("codex").expect("known");
    let keys = |_: &str| Some("the-key".to_owned());
    let foreign = endpoint_environment(cli, &profile(Some(endpoint("anthropic-messages"))), &keys)
        .expect_err("another protocol is not translated");
    assert!(foreign.contains("anthropic-messages") && foreign.contains("openai-responses"), "{foreign}");

    let unset = endpoint_environment(cli, &profile(Some(endpoint("openai-responses"))), &|_| None)
        .expect_err("a key the machine lacks");
    assert!(unset.contains("A_KEY_ON_THIS_MACHINE"), "{unset}");

    let no_variable = find_cli("gemini").expect("known");
    let mut elsewhere = profile(Some(endpoint("openai-responses")));
    elsewhere.cli_id = "gemini".to_owned();
    let refused = endpoint_environment(no_variable, &elsewhere, &keys).expect_err("no known variable");
    assert!(refused.contains("no variable is known"), "{refused}");
}
