//! The refusal `sailor flow check` gives a sensor, against the registry the
//! product ships: what an action declares about itself decides, not its name.

use flow::FlowFile;
use registry::{registry_in, House};
use serde_json::{json, Value};

fn flow_watching(sensor: Value) -> FlowFile {
    serde_json::from_value(json!({
        "id": "watching-flow",
        "description": "a flow started by what it watches",
        "graph": {"steps": [{
            "id": "trigger", "deps": [], "action": "trigger", "max_attempts": 1, "when": null,
            "with": {"source": "sensor", "sensor": sensor},
            "input_schema": {"type": "any"}, "output_schema": {"type": "any"}
        }]},
        "inputs": {}
    }))
    .expect("a flow with a sensor trigger")
}

fn refused(sensor: Value) -> Option<String> {
    trigger::sensor::refusal_of(&flow_watching(sensor), &registry_in(House::empty(), None, None))
}

#[test]
fn a_sensor_that_reads_a_command_or_the_store_is_accepted() {
    let command = json!({
        "action": actions::SHELL_CHECK_ACTION,
        "with": {"command": "git rev-parse HEAD", "timeout_secs": 10},
        "pointer": "/status",
        "cooldown_secs": 60
    });
    assert_eq!(refused(command), None);
    let count = json!({
        "action": actions::store::STORE_SELECT_ACTION,
        "with": {"collection": "open-work", "limit": 100},
        "pointer": "/count"
    });
    assert_eq!(refused(count), None);
}

#[test]
fn a_sensor_that_writes_spends_or_names_nothing_is_refused() {
    let writes = refused(json!({
        "action": actions::store::STORE_WRITE_ACTION,
        "with": {"collection": "c", "value": 1, "written_by": "sensor"}
    }))
    .expect("a sensor that writes into the store is refused");
    assert!(writes.contains("only reads"), "{writes}");

    let spends = refused(json!({"action": actions::EXTERNAL_ENGINE_ACTION}))
        .expect("a sensor that asks an engine is refused");
    assert!(spends.contains("may spend"), "{spends}");

    let unknown = refused(json!({"action": "nobody-registers-this"}))
        .expect("a sensor nobody registers is refused");
    assert!(unknown.contains("nobody-registers-this"), "{unknown}");
}
