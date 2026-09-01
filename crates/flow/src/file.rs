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
    }

    /// A bare graph is not a flow: without `inputs` nobody knows what values it
    /// starts from, and without `id` it has no name to be invoked by.
    #[test]
    fn a_naked_graph_is_not_a_flow_file() {
        let text = r#"{"steps": []}"#;

        assert!(serde_json::from_str::<FlowFile>(text).is_err());
    }
}
