//! The beat's sensor pass, end to end: a flow whose trigger is a sensor is
//! started by `tick_flows` only when what the sensor reads has changed, and
//! the run it starts is in the ledger with the reading before and after.

use ledger::Ledger;
use sailor::flow_cmd::beat::tick_flows;
use serde_json::{json, Value};
use std::fs;
use std::path::Path;
use ui::gather::FlowSource;

const FLOW: &str = "watch-a-file";

fn write_flow(flows: &Path, watched: &Path) {
    let command = format!("cat '{}'", watched.display());
    let flow = json!({
        "id": FLOW,
        "description": "started when the watched file says something else",
        "graph": {"steps": [{
            "id": "trigger", "deps": [], "action": "trigger", "max_attempts": 1, "when": null,
            "with": {"source": "sensor", "sensor": {
                "action": "shell_check",
                "with": {"command": command, "timeout_secs": 10, "answer_shape": {"type": "string"}},
                "pointer": "/answer"
            }},
            "input_schema": {"type": "any"}, "output_schema": {"type": "any"}
        }]},
        "inputs": {}
    });
    fs::write(flows.join(format!("{FLOW}.flow.json")), flow.to_string())
        .expect("the flow is written");
}

fn runs_of_the_flow(ledger: &Ledger) -> i64 {
    ledger
        .runs_by_entity()
        .expect("the ledger reads")
        .into_iter()
        .find(|(entity, _, _)| entity == FLOW)
        .map(|(_, count, _)| count)
        .unwrap_or(0)
}

fn line_of_the_flow(report: &str) -> &str {
    report
        .lines()
        .find(|line| line.starts_with(&format!("{FLOW}\t")))
        .unwrap_or_else(|| panic!("the beat says nothing about {FLOW}: {report}"))
}

#[test]
fn a_baseline_starts_nothing_a_change_starts_one_run_and_no_change_starts_none() {
    let scratch = std::env::temp_dir().join(format!("sailor-beat-sensor-{}", std::process::id()));
    let _ = fs::remove_dir_all(&scratch);
    let (home, ledger_dir, flows) = (
        scratch.join("home"),
        scratch.join("ledger"),
        scratch.join("flows"),
    );
    for directory in [&home, &ledger_dir, &flows] {
        fs::create_dir_all(directory).expect("a scratch directory");
    }
    // The only test in this binary, so the process environment is its own.
    std::env::set_var("SAILOR_HOME", &home);
    std::env::set_var("SAILOR_LEDGER", &ledger_dir);
    let watched = scratch.join("watched.json");
    fs::write(&watched, "\"a1\"").expect("the watched file");
    write_flow(&flows, &watched);
    let sources = vec![FlowSource {
        origin: "test",
        dir: flows.clone(),
    }];

    let first = tick_flows(&sources).expect("the first beat");
    assert!(line_of_the_flow(&first).contains("\thold\t"), "{first}");
    let ledger = Ledger::open(&ledger_dir).expect("the beat left a ledger");
    assert_eq!(
        runs_of_the_flow(&ledger),
        0,
        "a baseline starts nothing: {first}"
    );

    fs::write(&watched, "\"b2\"").expect("the watched file changes");
    let second = tick_flows(&sources).expect("the second beat");
    assert!(line_of_the_flow(&second).contains("\tran\t"), "{second}");
    assert_eq!(
        runs_of_the_flow(&ledger),
        1,
        "one change, one run: {second}"
    );
    let run = ledger
        .last_finished_run(FLOW)
        .expect("the ledger reads")
        .expect("the run closed");
    let steps = ledger.steps(&run.run_id).expect("its steps read");
    let trigger = steps
        .iter()
        .find(|step| step.step_id == "trigger")
        .expect("the run has its trigger step");
    let text: Value = serde_json::from_str(
        trigger.input["text"]
            .as_str()
            .expect("the trigger was handed a text"),
    )
    .expect("the text is one JSON object");
    assert_eq!(
        (text["before"].clone(), text["after"].clone()),
        (json!("a1"), json!("b2"))
    );

    let third = tick_flows(&sources).expect("the third beat");
    assert!(line_of_the_flow(&third).contains("\thold\t"), "{third}");
    assert_eq!(
        runs_of_the_flow(&ledger),
        1,
        "nothing changed, nothing started: {third}"
    );

    let _ = fs::remove_dir_all(&scratch);
}
