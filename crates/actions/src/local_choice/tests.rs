use super::*;

fn said(code: i32, line: &str) -> CheckResult {
    match code {
        0 => CheckResult::Passed {
            stdout: format!("{line}\n"),
        },
        _ => CheckResult::Failed {
            code: Some(code),
            stdout: format!("{line}\n"),
            stderr: String::new(),
        },
    }
}

const WON: &str = r#"{"outcome":"chose","chosen":"Plan","probability":0.91,"threshold":0.7,"offered":["Explore","Plan"],"version":"agents-v1"}"#;

#[test]
fn a_choice_that_publishes_everything_is_taken() {
    let read = read_choice(&said(0, WON), None);
    assert_eq!(read["outcome"], "chose");
    assert_eq!(read["chosen"], "Plan");
    assert_eq!(read["version"], "agents-v1");
}

#[test]
fn a_choice_that_leaves_out_what_it_decided_by_abstains() {
    for broken in [
        WON.replace(r#","version":"agents-v1""#, ""),
        WON.replace("0.91", "0.5"),
        WON.replace(r#""chosen":"Plan""#, r#""chosen":"Deploy""#),
    ] {
        let read = read_choice(&said(0, &broken), None);
        assert_eq!(read["outcome"], "abstained", "{broken}");
        assert!(
            read.get("chosen").is_none(),
            "an abstention hands on no winner"
        );
    }
}

#[test]
fn options_the_step_did_not_declare_are_not_a_choice() {
    let declared = ["Explore".to_owned(), "Plan".to_owned(), "Deploy".to_owned()];
    assert_eq!(
        read_choice(&said(0, WON), Some(&declared))["outcome"],
        "abstained"
    );
    assert_eq!(
        read_choice(&said(0, WON), Some(&declared[..2]))["outcome"],
        "chose"
    );
}

#[test]
fn a_decider_below_its_threshold_or_down_is_data_not_an_error() {
    let below = WON.replace(r#""outcome":"chose""#, r#""outcome":"abstained""#);
    assert_eq!(read_choice(&said(3, &below), None)["outcome"], "abstained");
    assert_eq!(
        read_choice(&said(4, r#"{"outcome":"unavailable"}"#), None)["outcome"],
        "failed"
    );
    assert_eq!(read_choice(&said(0, "not json"), None)["outcome"], "failed");
    let timed_out = read_choice(&CheckResult::TimedOut, None);
    assert_eq!(
        (
            timed_out["outcome"].as_str(),
            timed_out["offered"].as_array().map(Vec::len)
        ),
        (Some("failed"), Some(0))
    );
}

#[test]
fn the_evidence_reaches_the_command_as_data() {
    let action = LocalChoiceAction { watcher: None };
    let input = json!({
        "command": r#"printf '{"outcome":"chose","chosen":"%s","probability":1,"threshold":0.5,"offered":["a","b"],"version":"t1"}' "$SAILOR_EVIDENCE""#,
        "evidence": "b",
        "timeout_secs": 5
    });
    let ActionOutcome::Went(read) = action
        .execute(&input, &SharedState::new())
        .expect("a choice never fails its step")
    else {
        panic!("a choice that ran is Went");
    };
    assert_eq!(
        (read["outcome"].as_str(), read["chosen"].as_str()),
        (Some("chose"), Some("b"))
    );
}
