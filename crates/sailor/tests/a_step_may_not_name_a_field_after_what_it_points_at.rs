//! A step whose `with` covers the value its own pointer reaches for.
//!
//! The other pointer check cannot see it: the shape carries the name, so the
//! pointer looks reachable, and what covers it is the step's own `with`. Found
//! by running a flow whose step read `"text": {"$from": "/text"}`.

use sailor::flow_cmd::check::refusals_of;
use serde_json::{json, Value};

fn a_flow_whose_step_reads(with: Value) -> flow::FlowFile {
    serde_json::from_value(json!({
        "id": "a-flow",
        "description": "one step reading what the trigger handed it",
        "inputs": {},
        "graph": {
            "steps": [
                {
                    "id": "trigger",
                    "deps": [],
                    "action": "trigger",
                    "max_attempts": 1,
                    "when": null,
                    "with": {},
                    "input_schema": { "type": "any" },
                    "output_schema": {
                        "type": "object",
                        "properties": { "text": { "type": "string" } },
                        "required": ["text"],
                        "allow_extra": true,
                    },
                },
                {
                    "id": "the_request",
                    "deps": ["trigger"],
                    "action": "delivery_request",
                    "max_attempts": 1,
                    "when": null,
                    "with": with,
                    "needs": ["git"],
                    "input_schema": { "type": "any" },
                    "output_schema": { "type": "any" },
                },
            ],
        },
    }))
    .expect("the flow reads")
}

/// The one sentence this check refuses with. Matching on it, and not on the
/// step's name, is what keeps the case honest: the other pointer check would
/// otherwise answer for it.
const COVERS: &str = "covers the value its own pointer reaches for";

fn registry() -> flow::ActionRegistry {
    registry::registry_in(registry::House::empty(), None, None)
}

#[test]
fn a_field_named_after_the_value_it_points_at_is_refused_before_the_run() {
    let flow = a_flow_whose_step_reads(json!({
        "text": { "$from": "/text" },
        "required": ["repo"],
    }));

    let refused = refusals_of(&flow, &registry());

    assert!(
        refused
            .iter()
            .any(|said| said.contains(COVERS) && said.contains("the_request in «text» (/text)")),
        "the refusal must name the step, the field and the pointer:\n{}",
        refused.join("\n")
    );
}

/// The same value under another name is what the repair looks like, and it
/// must not be refused: a check that refuses the cure teaches nothing.
#[test]
fn the_same_value_under_another_name_is_not_refused() {
    let flow = a_flow_whose_step_reads(json!({
        "mandate": { "$from": "/text" },
        "required": ["repo"],
    }));

    let refused = refusals_of(&flow, &registry());

    assert!(
        !refused.iter().any(|said| said.contains(COVERS)),
        "the repair was refused:\n{}",
        refused.join("\n")
    );
}

/// A pointer *into* what the field covers is covered as much as the field
/// itself: `/text/inner` reads the `text` the step declares, not the one it
/// was handed.
#[test]
fn a_pointer_into_the_field_is_covered_as_much_as_the_field_itself() {
    let flow = a_flow_whose_step_reads(json!({
        "text": { "$from": "/text/inner" },
        "required": ["repo"],
    }));

    let refused = refusals_of(&flow, &registry());

    assert!(
        refused.iter().any(|said| said.contains(COVERS)
            && said.contains("the_request in «text» (/text/inner)")),
        "a pointer under the covered name is covered too:\n{}",
        refused.join("\n")
    );
}
