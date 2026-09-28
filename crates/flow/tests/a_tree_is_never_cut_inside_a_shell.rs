//! **TAKING A TREE IS A STEP OF ITS OWN, AND IT IS WRITTEN DOWN.** A shell that
//! cuts a tree and then works in it leaves nothing to give back when the work
//! fails: the step produces no output, the give-back reading the path off it is
//! skipped, and the tree stands. The rule is the one `actions/src/worktree.rs`
//! opens with, said for the other half of the life: a flow step is not a shell
//! program. ADR-026.

use serde_json::Value;

/// What a shell step says when it cuts a tree behind the register's back.
const CUTTING: &str = "worktree add";

#[test]
fn no_shipped_flow_cuts_a_tree_inside_a_shell() {
    let mut caught: Vec<String> = Vec::new();
    for (name, text) in flow::system::FLOWS {
        let flow: Value = serde_json::from_str(text)
            .unwrap_or_else(|why| panic!("{name} does not read as a flow: {why}"));
        for step in steps_of(&flow) {
            let id = step
                .get("id")
                .and_then(Value::as_str)
                .unwrap_or("a step with no id");
            if command_of(step).is_some_and(|command| command.contains(CUTTING)) {
                caught.push(format!("{name}/{id}"));
            }
        }
    }
    assert!(
        caught.is_empty(),
        "these steps cut a tree through a shell, so nothing writes it down and nothing can give \
         it back: {}. Cut it with the action that writes it down instead, in a step that does \
         nothing else.",
        caught.join(", ")
    );
}

fn steps_of(flow: &Value) -> impl Iterator<Item = &serde_json::Map<String, Value>> {
    flow.get("graph")
        .and_then(|graph| graph.get("steps"))
        .and_then(Value::as_array)
        .map(Vec::as_slice)
        .unwrap_or_default()
        .iter()
        .filter_map(Value::as_object)
}

/// The command a step runs, wherever the step keeps it: a shell names it
/// `command`, and a step that runs none answers nothing rather than an empty
/// string, so a step with no `with` at all cannot read as a clean one.
fn command_of(step: &serde_json::Map<String, Value>) -> Option<&str> {
    step.get("with")?.get("command")?.as_str()
}
