//! The on-disk format of a flow: the graph plus the values it starts with,
//! because the graph declares shapes and never names a command or an engine.
//! Born twice in one night once, in `ui::registry` and `sailor::flow_cmd`: the
//! fields coincided by luck, and one added field would have had the window and
//! the command line reading two formats under one name. A third copy no
//! compiler ties to this one — `desktop/src/flow.ts` — changes when this does.

use crate::{Graph, Schedule};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeMap;

/// A declared flow: the graph plus the values it starts with.
///
/// `graph` goes through `Graph`'s validation at load time — cycles, missing
/// dependencies, zero caps and destructive merges are refused there, not
/// halfway through a run. `inputs` becomes the request's `root_inputs`: one
/// entry per step with no dependencies.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FlowFile {
    pub id: String,
    pub description: String,
    pub graph: Graph,
    pub inputs: BTreeMap<String, Value>,
    /// When the flow is due, how much it weighs, where it may write.
    ///
    /// Optional because both cases really exist, not as a fallback: a flow
    /// launched by hand has no recurrence, and forcing one on it would say that
    /// something, sooner or later, starts it by itself. `None` means "runs when
    /// someone asks" — a fact, not a hole to fill. Absent is written absent.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub schedule: Option<Schedule>,
    /// What one run may spend, in currency micro-units (`1_000_000` is one
    /// unit); absent is written absent, as for `schedule`. `None` is not zero:
    /// `None` is "nobody set a limit", `Some(0)` is "must not spend" and stops
    /// before the first paid call. The default is `None`, or a cap appearing by
    /// itself would stop runs nobody asked to stop. What it cannot promise is
    /// on [`crate::Spend`]: the cap is a guarantee over costs engines declare.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub spend_cap_micros: Option<i64>,
    /// How long one run of this flow may last, in seconds, counted from the
    /// first start: a resume inherits the same deadline instead of granting
    /// itself a fresh one. Absent is written absent, and a run of a flow
    /// without a wall is what it always was, key for key.
    #[serde(
        default,
        deserialize_with = "wall_secs",
        skip_serializing_if = "Option::is_none"
    )]
    pub wall_secs: Option<u64>,
    /// How many runs of this flow may be taken in all. A turn is one recorded
    /// run, whoever launched it and however it ended; the n+1th closes before
    /// opening anything.
    #[serde(
        default,
        deserialize_with = "max_turns",
        skip_serializing_if = "Option::is_none"
    )]
    pub max_turns: Option<u32>,
    /// This flow looks after the tree it runs in, so each of its closed runs
    /// leaves one line of its own beside the run's header.
    #[serde(default, skip_serializing_if = "not_declared")]
    pub self_care: bool,
}

fn not_declared(declared: &bool) -> bool {
    !*declared
}

/// A whole number the flow declared, or a refusal that names the field.
///
/// Written out rather than left to the derived reader: a value of the wrong
/// shape there is refused with a line and a column, and whoever wrote the file
/// is told the type but never the field.
fn whole_number<'de, D>(deserializer: D, field: &str) -> Result<Option<u64>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let declared = Option::<Value>::deserialize(deserializer)?;
    match declared {
        None | Some(Value::Null) => Ok(None),
        Some(Value::Number(number)) => number.as_u64().map(Some).ok_or_else(|| {
            serde::de::Error::custom(format!(
                "`{field}` is a whole number of at least zero, and «{number}» is not one"
            ))
        }),
        Some(other) => Err(serde::de::Error::custom(format!(
            "`{field}` is a whole number, and «{other}» is not one"
        ))),
    }
}

fn wall_secs<'de, D>(deserializer: D) -> Result<Option<u64>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    whole_number(deserializer, "wall_secs")
}

fn max_turns<'de, D>(deserializer: D) -> Result<Option<u32>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let Some(turns) = whole_number(deserializer, "max_turns")? else {
        return Ok(None);
    };
    u32::try_from(turns).map(Some).map_err(|_| {
        serde::de::Error::custom(
            "`max_turns` is a count of runs, and this one is larger than any tree will hold",
        )
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The format the window and the command line must read the same way. If
    /// this test falls, the two have drifted apart again.
    #[test]
    fn a_declared_flow_carries_its_graph_and_its_values() {
        let text = r#"{
            "id": "prima-corsa",
            "description": "a single check",
            "graph": {
                "steps": [{
                    "id": "clean",
                    "deps": [],
                    "input_schema": {"type": "any"},
                    "output_schema": {"type": "any"},
                    "when": null,
                    "action": "shell_check",
                    "max_attempts": 1
                }]
            },
            "inputs": { "clean": { "command": "true", "timeout_secs": 5 } }
        }"#;

        let file: FlowFile = serde_json::from_str(text).expect("valid flow");

        assert_eq!(file.id, "prima-corsa");
        assert_eq!(file.graph.steps().len(), 1);
        assert_eq!(file.inputs["clean"]["command"], "true");
        assert_eq!(file.graph.steps()[0].phase, None);
    }

    /// A step may name the moment of the process it belongs to. The word goes
    /// in and comes out as written, and a step that names none writes no key:
    /// a file rewritten by Sailor must not grow a `"phase": null` its author
    /// never typed.
    #[test]
    fn a_step_may_name_its_phase_and_the_file_keeps_it_as_written() {
        let text = r#"{
            "id": "fasi",
            "description": "one step in a named phase",
            "graph": {
                "steps": [{
                    "id": "build",
                    "deps": [],
                    "input_schema": {"type": "any"},
                    "output_schema": {"type": "any"},
                    "when": null,
                    "action": "shell_check",
                    "max_attempts": 1,
                    "phase": "construction"
                }, {
                    "id": "unphased",
                    "deps": [],
                    "input_schema": {"type": "any"},
                    "output_schema": {"type": "any"},
                    "when": null,
                    "action": "shell_check",
                    "max_attempts": 1
                }]
            },
            "inputs": {}
        }"#;

        let file: FlowFile = serde_json::from_str(text).expect("valid flow");
        assert_eq!(file.graph.steps()[0].phase.as_deref(), Some("construction"));
        assert_eq!(file.graph.steps()[1].phase, None);

        let written = serde_json::to_value(&file).expect("a flow serializes");
        let steps = &written["graph"]["steps"];
        assert_eq!(steps[0]["phase"], "construction");
        assert!(steps[1].get("phase").is_none(), "{steps}");

        let again: FlowFile = serde_json::from_value(written).expect("what was written loads");
        assert_eq!(again, file);
    }

    /// A bare graph is not a flow: without `inputs` nobody knows what values it
    /// starts from, and without `id` it has no name to be invoked by.
    #[test]
    fn a_naked_graph_is_not_a_flow_file() {
        let text = r#"{"steps": []}"#;

        assert!(serde_json::from_str::<FlowFile>(text).is_err());
    }

    fn a_flow_declaring(extra: &str) -> Result<FlowFile, serde_json::Error> {
        serde_json::from_str(&format!(
            r#"{{"id": "p", "description": "d",
                "graph": {{"steps": []}}, "inputs": {{}}{extra}}}"#
        ))
    }

    /// A wall and a count of turns go in and come back as written, and a flow
    /// declaring neither writes no key: a file rewritten by the tool must not
    /// grow declarations its author never typed.
    #[test]
    fn a_wall_and_a_count_of_turns_are_kept_as_written() {
        let declared = a_flow_declaring(r#", "wall_secs": 900, "max_turns": 3, "self_care": true"#)
            .expect("valid flow");
        assert_eq!(declared.wall_secs, Some(900));
        assert_eq!(declared.max_turns, Some(3));
        assert!(declared.self_care);

        let bare = a_flow_declaring("").expect("valid flow");
        assert_eq!(bare.wall_secs, None);
        assert_eq!(bare.max_turns, None);
        assert!(!bare.self_care);

        let written = serde_json::to_value(&bare).expect("a flow serializes");
        for key in ["wall_secs", "max_turns", "self_care"] {
            assert!(written.get(key).is_none(), "{written}");
        }
    }

    /// A malformed declaration is refused with the field named. A line and a
    /// column tell whoever wrote the file the type and never which key.
    #[test]
    fn a_wall_that_is_not_a_number_is_refused_by_name() {
        let said = a_flow_declaring(r#", "wall_secs": "un'ora""#)
            .expect_err("a wall of text is no wall")
            .to_string();

        assert!(said.contains("wall_secs"), "{said}");
    }

    #[test]
    fn a_count_of_turns_that_is_not_a_number_is_refused_by_name() {
        let said = a_flow_declaring(r#", "max_turns": [3]"#)
            .expect_err("a list is no count")
            .to_string();

        assert!(said.contains("max_turns"), "{said}");
    }
}
