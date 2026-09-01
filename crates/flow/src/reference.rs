//! How a step receives the previous step's work. The engine hands a step its
//! dependencies' output and the graph's `with` lays constants over it; with
//! those two alone a dispatch cannot be written, because the text the main node
//! produces has to reach the engine invocation and no constant can hold it. The
//! only route left was the next step being a script — the work leaving the
//! graph. The hole is closed by a reference, not by an interpreter.

use crate::ActionError;
use serde_json::{Map, Value};

/// The key naming a value taken from the step's own input.
///
/// A JSON pointer is already how the graph looks inside a value —
/// `Condition::PointerEquals` uses one to decide whether a step runs — so this
/// reuses it instead of inventing a second syntax for the same question.
pub const FROM_KEY: &str = "$from";
/// The key that runs several pieces together into one text. It joins text and
/// refuses everything else on purpose: deciding how a number is written belongs
/// to whoever composes the message, not to whoever delivers it.
pub const JOIN_KEY: &str = "$join";
/// The key that writes, as JSON, the value a pointer finds. `$join` alone did
/// not cover it: since a step can declare *the shape* of its own answer, that
/// shape has to reach the engine inside the prompt, and rewriting it by hand
/// there would mean two copies that one day diverge in silence. The only
/// conversion allowed, over the one thing there is no style choice about — a
/// structured value, written as JSON.
pub const JSON_KEY: &str = "$json";

/// Replaces the references inside a step's input with the values they name; an
/// input with no references comes back identical, so whoever does not use them
/// pays nothing. It runs once where the input is composed
/// ([`crate::step_input`]) and not inside each action — fault 28, and copying
/// the line into twelve actions of sixteen was fault 10 in twelve copies, not a
/// cure. How a step gets the previous step's work is the graph's semantics.
pub fn resolve_references(input: &Value) -> Result<Value, ActionError> {
    resolve_against(input, input)
}

/// Walks a value, replacing every reference found anywhere inside it — and that
/// is the declared limit: references are hunted in *all* the input, which holds
/// the dependencies' output too. Not a way in while engines answer with text; it
/// becomes one with an action that yields free objects (`store_read` returns the
/// stored value as it stands) fed to a node that executes. Whoever wires those
/// two nodes together must know.
fn resolve_against(value: &Value, root: &Value) -> Result<Value, ActionError> {
    match value {
        Value::Object(fields) => resolve_object(fields, root),
        Value::Array(items) => {
            let resolved: Result<Vec<Value>, ActionError> = items
                .iter()
                .map(|item| resolve_against(item, root))
                .collect();
            Ok(Value::Array(resolved?))
        }
        // Text is never looked inside: `{"$from": …}` written mid-sentence is a
        // sentence, not a reference.
        other => Ok(other.clone()),
    }
}

/// Dispatches on `$from`, `$join` and `$json`, rebuilding any other object field
/// by field. A resolved value is never read again: references are hunted in the
/// input as it arrived, and what comes out takes their place unexamined, so two
/// references do not chain and a reference cannot be born from data — whoever
/// writes the flow decides what gets read, whoever answers does not.
fn resolve_object(fields: &Map<String, Value>, root: &Value) -> Result<Value, ActionError> {
    if let Some(pointer) = fields.get(FROM_KEY) {
        if fields.len() != 1 {
            return Err(ambiguous(FROM_KEY));
        }
        return look_up(pointer, root);
    }
    if let Some(parts) = fields.get(JOIN_KEY) {
        if fields.len() != 1 {
            return Err(ambiguous(JOIN_KEY));
        }
        return join(parts, root);
    }
    if let Some(pointer) = fields.get(JSON_KEY) {
        if fields.len() != 1 {
            return Err(ambiguous(JSON_KEY));
        }
        let value = look_up(pointer, root)?;
        // `to_string` and not an indented form: this text ends up inside a
        // prompt, and extra lines are extra tokens on every call.
        return Ok(Value::String(
            serde_json::to_string(&value).expect("a value already in memory always reserialises"),
        ));
    }
    let mut resolved = Map::new();
    for (name, value) in fields {
        resolved.insert(name.clone(), resolve_against(value, root)?);
    }
    Ok(Value::Object(resolved))
}

fn look_up(pointer: &Value, root: &Value) -> Result<Value, ActionError> {
    let Some(pointer) = pointer.as_str() else {
        return Err(ActionError::new(
            "invalid_reference",
            format!("{FROM_KEY} wants a JSON pointer as text, not {pointer}"),
        ));
    };
    // A pointer without its leading slash never finds anything, and without
    // this the flow would be told "not contained" instead of "misspelled".
    if !pointer.is_empty() && !pointer.starts_with('/') {
        return Err(ActionError::new(
            "invalid_reference",
            format!("a JSON pointer starts with /: {pointer} does not"),
        ));
    }
    root.pointer(pointer).cloned().ok_or_else(|| {
        ActionError::new(
            "unresolved_reference",
            format!("the flow asks for {pointer}, which the step input does not contain"),
        )
    })
}

fn join(parts: &Value, root: &Value) -> Result<Value, ActionError> {
    let Some(parts) = parts.as_array() else {
        return Err(ActionError::new(
            "invalid_reference",
            format!("{JOIN_KEY} wants a list of pieces, not {parts}"),
        ));
    };
    let mut text = String::new();
    for part in parts {
        let resolved = resolve_against(part, root)?;
        match resolved.as_str() {
            Some(piece) => text.push_str(piece),
            // Joining a number to text would mean choosing how that number is
            // written: the message author's decision, not the deliverer's.
            None => {
                return Err(ActionError::new(
                    "unjoinable_reference",
                    format!("{JOIN_KEY} joins text, and {resolved} is not text"),
                ))
            }
        }
    }
    Ok(Value::String(text))
}

fn ambiguous(key: &str) -> ActionError {
    ActionError::new(
        "ambiguous_reference",
        format!("a {key} reference stands alone; here it has other keys beside it, and which one wins is anyone's guess"),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn an_input_without_references_comes_back_identical() {
        let input = json!({"bin": "codex", "args": ["exec"], "timeout_secs": 30});
        assert_eq!(resolve_references(&input).expect("no references"), input);
    }

    /// The case this module exists for: the text the previous step produced
    /// goes into the next step's invocation.
    #[test]
    fn a_reference_carries_the_previous_answer_into_the_next_call() {
        let input = json!({
            "stdout": "find the leftovers in ~/.claude",
            "stdin": {"$from": "/stdout"}
        });

        let resolved = resolve_references(&input).expect("the pointer exists");

        assert_eq!(resolved["stdin"], json!("find the leftovers in ~/.claude"));
    }

    #[test]
    fn join_puts_a_fixed_role_in_front_of_what_the_engine_said() {
        let input = json!({
            "stdout": "list the dead hooks",
            "stdin": {"$join": ["Do only your own section.\n\n", {"$from": "/stdout"}]}
        });

        let resolved = resolve_references(&input).expect("the pointer exists");

        assert_eq!(
            resolved["stdin"],
            json!("Do only your own section.\n\nlist the dead hooks")
        );
    }

    #[test]
    fn a_pointer_can_reach_a_named_dependency() {
        let input = json!({
            "codex": {"status": "ok", "stdout": "three leftovers"},
            "agy": {"status": "timed_out", "stdout": ""},
            "env": {"CODEX": {"$from": "/codex/stdout"}, "AGY_STATUS": {"$from": "/agy/status"}}
        });

        let resolved = resolve_references(&input).expect("the pointers exist");

        assert_eq!(resolved["env"]["CODEX"], json!("three leftovers"));
        assert_eq!(resolved["env"]["AGY_STATUS"], json!("timed_out"));
    }

    /// The measure that could have come out differently: a pointer that finds
    /// nothing must stop the step, not hand it the void — an engine invoked
    /// with an empty errand costs as much as one invoked properly, and answers
    /// something that looks like an answer.
    #[test]
    fn a_pointer_that_finds_nothing_stops_the_step() {
        let input = json!({"stdin": {"$from": "/codex/stdout"}});

        let error = resolve_references(&input).expect_err("there is no codex");

        assert_eq!(error.class, "unresolved_reference");
        assert!(error.said.contains("/codex/stdout"), "{}", error.said);
    }

    #[test]
    fn a_pointer_without_its_leading_slash_says_so() {
        let input = json!({"stdout": "x", "stdin": {"$from": "stdout"}});

        let error = resolve_references(&input).expect_err("misspelled pointer");

        assert_eq!(error.class, "invalid_reference");
        assert!(error.said.contains("starts with /"), "{}", error.said);
    }

    #[test]
    fn a_reference_with_other_keys_beside_it_is_refused() {
        let input = json!({"stdout": "x", "stdin": {"$from": "/stdout", "or_else": "y"}});

        let error = resolve_references(&input).expect_err("ambiguous reference");

        assert_eq!(error.class, "ambiguous_reference");
    }

    /// The shape a step demands of its own answer reaches the prompt through
    /// here: written once, in the field that then enforces it.
    #[test]
    fn a_declared_shape_becomes_the_text_that_goes_into_the_prompt() {
        let input = json!({
            "answer_shape": {"type": "object", "properties": {"total": {"type": "number"}}},
            "stdin": {"$join": ["Answer in this shape: ", {"$json": "/answer_shape"}]}
        });

        let resolved = resolve_references(&input).expect("the pointer exists");

        assert_eq!(
            resolved["stdin"],
            json!("Answer in this shape: {\"type\":\"object\",\"properties\":{\"total\":{\"type\":\"number\"}}}")
        );
    }

    /// `$json` writes the *same text* a checker would write when verifying the
    /// shape really is in the prompt: were the two to diverge, that check would
    /// say no on a correct prompt.
    #[test]
    fn the_written_shape_matches_what_a_reader_would_serialise() {
        let shape = json!({"type": "object", "required": ["a", "b"], "allow_extra": false});
        let input = json!({"answer_shape": shape.clone(), "text": {"$json": "/answer_shape"}});

        let resolved = resolve_references(&input).expect("the pointer exists");

        assert_eq!(
            resolved["text"].as_str().expect("text"),
            serde_json::to_string(&shape).expect("serialisable")
        );
    }

    #[test]
    fn a_json_reference_that_finds_nothing_stops_the_step() {
        let input = json!({"text": {"$json": "/not/here"}});
        let error = resolve_references(&input).expect_err("the pointer finds nothing");
        assert_eq!(error.class, "unresolved_reference");
    }

    #[test]
    fn join_refuses_to_decide_how_a_number_is_written() {
        let input =
            json!({"how_many": 3, "text": {"$join": ["there are ", {"$from": "/how_many"}]}});

        let error = resolve_references(&input).expect_err("a number is not text");

        assert_eq!(error.class, "unjoinable_reference");
    }

    /// The way in that stays shut: an engine answers with text, and inside a
    /// text no reference is ever recognised.
    #[test]
    fn a_reference_written_inside_an_engine_answer_stays_a_sentence() {
        let input = json!({
            "stdout": "I wrote {\"$from\": \"/secret\"} in the answer",
            "secret": "must not get out",
            "stdin": {"$from": "/stdout"}
        });

        let resolved = resolve_references(&input).expect("the pointer exists");

        assert!(
            resolved["stdin"].as_str().expect("text").contains("$from"),
            "the text stays text"
        );
        assert!(!resolved["stdin"]
            .as_str()
            .expect("text")
            .contains("must not get out"));
    }

    /// References read the input as it arrived, not as it is becoming: two
    /// references do not chain, and key order does not matter.
    #[test]
    fn a_resolved_value_is_not_read_again() {
        let input = json!({
            "secret": "x",
            "bridge": {"$from": "/secret"},
            "arrival": {"$from": "/bridge"}
        });

        let resolved = resolve_references(&input).expect("the pointers exist");

        assert_eq!(resolved["bridge"], json!("x"));
        assert_eq!(resolved["arrival"], json!({"$from": "/secret"}));
    }
}
