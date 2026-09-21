use super::*;

fn options(ids: &[&str]) -> Vec<DeclaredOption> {
    ids.iter()
        .map(|id| DeclaredOption {
            id: (*id).to_owned(),
            description: format!("the {id} option"),
        })
        .collect()
}

fn passed() -> CheckResult {
    CheckResult::Passed {
        stdout: String::new(),
    }
}

const SCORED: &str = r#"{"option_ids":["Explore","Plan"],"probabilities":[0.09,0.91],"prompt_sha256":"d173df","model":{"source":"Qwen/Qwen3.5-4B","revision":"851bf6e"}}"#;

#[test]
fn a_scored_row_above_the_threshold_is_a_choice() {
    let read = read_choice(&passed(), SCORED, &options(&["Explore", "Plan"]), 0.7);
    assert_eq!(read["outcome"], "chose");
    assert_eq!(read["chosen"], "Plan");
    assert_eq!(read["version"], "d173df");
    assert_eq!(read["model"], "Qwen/Qwen3.5-4B@851bf6e");
    assert_eq!(read["probabilities"]["Explore"], 0.09);
}

#[test]
fn what_the_row_does_not_publish_turns_a_winner_into_an_abstention() {
    let declared = options(&["Explore", "Plan"]);
    for (row, threshold) in [
        (SCORED.replace(r#""prompt_sha256":"d173df","#, ""), 0.7),
        (SCORED.to_owned(), 0.95),
        (
            SCORED.replace(r#""Explore","Plan""#, r#""Explore","Deploy""#),
            0.7,
        ),
    ] {
        let read = read_choice(&passed(), &row, &declared, threshold);
        assert_eq!(read["outcome"], "abstained", "{row}");
        assert!(
            read.get("chosen").is_none(),
            "an abstention hands on no winner"
        );
    }
}

#[test]
fn a_scorer_that_fails_or_writes_nothing_is_data_not_an_error() {
    let declared = options(&["a", "b"]);
    let down = CheckResult::Failed {
        code: Some(1),
        stdout: String::new(),
        stderr: "no model\n".into(),
    };
    for (answer, scored) in [
        (down, SCORED),
        (CheckResult::TimedOut, SCORED),
        (passed(), ""),
        (
            passed(),
            r#"{"option_ids":["a","b"],"probabilities":[1.0]}"#,
        ),
    ] {
        let read = read_choice(&answer, scored, &declared, 0.5);
        assert_eq!(read["outcome"], "failed", "{scored}");
        assert_eq!(read["offered"], json!(["a", "b"]));
    }
}

#[test]
fn the_decision_row_carries_state_question_and_options() {
    let row = decision_row(&json!({"diff": "x"}), "Which?", &options(&["a", "b"]));
    assert_eq!(row["state"], json!({"diff": "x"}));
    assert_eq!(row["question"], "Which?");
    assert_eq!(row["options"][1]["id"], "b");
}

#[test]
fn the_command_reads_the_row_and_writes_the_scored_one_where_it_is_told() {
    let action = LocalChoiceAction { watcher: None };
    let input = json!({
        "command": r#"id=$(sed -n 's/.*"state":"\([^"]*\)".*/\1/p' "$SAILOR_DECISION_INPUT"); printf '{"option_ids":["a","b"],"probabilities":[0.1,0.9],"prompt_sha256":"h","model":{"source":"m"},"state_was":"%s"}\n' "$id" > "$SAILOR_DECISION_OUTPUT""#,
        "evidence": "b",
        "question": "Which?",
        "options": [{"id": "a", "description": "A"}, {"id": "b", "description": "B"}],
        "threshold": 0.5,
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
