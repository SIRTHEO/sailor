//! What a step makes of what its command said: the failures it tolerates, the
//! tail it reports when red, and the declared shape its answer must fit.

use crate::spec::EngineSpec;
use flow::{ActionError, Refusal, RefusalRule, ValueSchema};
use serde_json::Value;

/// The failure outcomes an external engine can produce.
pub(crate) const ENGINE_FAILURES: [&str; 3] = ["exit_error", "timed_out", "spawn_failed"];
/// Those of a shell check.
pub(crate) const CHECK_FAILURES: [&str; 2] = ["failed", "timed_out"];

/// An `accept` naming an impossible outcome is a mistake by whoever wrote the
/// step, not a silence: it grants a tolerance that never applies, and the step
/// turns red on the very day it was meant not to.
pub(crate) fn check_tolerance(accept: &[String], known: &[&str]) -> Result<(), ActionError> {
    for name in accept {
        if !known.contains(&name.as_str()) {
            return Err(ActionError::new(
                "invalid_input",
                format!(
                    "`accept` names «{name}», which this step cannot produce; the possible values are: {}",
                    known.join(", ")
                ),
            ));
        }
    }
    Ok(())
}

pub(crate) fn tolerates(accept: &[String], status: &str) -> bool {
    accept.iter().any(|name| name == status)
}

/// The outcomes that leave no answer to shape at all.
const SILENT_FAILURES: [&str; 2] = ["timed_out", "spawn_failed"];

/// **ASKING WITHOUT CHECKING AND CHECKING WITHOUT ASKING ARE THE SAME DEFECT**,
/// and this closes the circle on the side usually left open: an engine does not
/// honour a shape because somebody declared it in a field, it honours one it was
/// told. So the shape's text must really appear in what is about to be sent —
/// stdin or an argument — and if it does not, the step stops **before** paying
/// for a call that would surely fail.
pub(crate) fn shape_was_asked_for(written: &str, spec: &EngineSpec) -> Result<(), ActionError> {
    if let Some(silent) = SILENT_FAILURES
        .iter()
        .find(|status| tolerates(&spec.accept, status))
    {
        return Err(ActionError::new(
            "invalid_input",
            format!(
                "the step declares a shape for the answer and at the same time tolerates «{silent}», which leaves no answer at all: the two do not go together"
            ),
        ));
    }
    let mut sent = spec.stdin.clone().unwrap_or_default();
    for arg in &spec.args {
        sent.push('\n');
        sent.push_str(arg);
    }
    if sent.contains(written) {
        return Ok(());
    }
    Err(ActionError::new(
        "shape_not_in_prompt",
        format!(
            "the step demands an answer in a declared shape, and that shape does not appear in what it sends the engine: put it in the prompt with a {} reference to /answer_shape, so it is written once. The shape is: {written}",
            flow::reference::JSON_KEY
        ),
    ))
}

/// The same question with the reason under it, never the refused answer. The
/// excerpt is cut here afresh: a refusal read from an input has not been
/// through the constructor that cuts it. See fault 103.
pub(crate) fn asked_again(asked: &str, told: &Refusal) -> String {
    let told = Refusal::new(&told.check, &told.path, told.rule, &told.seen);
    format!(
        "{asked}\n\n{}",
        catalogue::say("engine.after_refusal", &[("why", &told.explain())])
    )
}

/// How much of what a command said goes into a broken step's message.
const SAID_TAIL: usize = 1200;

/// **THE LAST LINES, NOT THE FIRST.** An engine that fails writes the error at
/// the bottom, after pages of startup. And they belong in here: a broken step
/// writes no typed output, so without this text stdout and stderr die with the
/// process and whoever reads the ledger finds a red with no reason.
fn tail(text: &str) -> &str {
    let text = text.trim_end();
    if text.len() <= SAID_TAIL {
        return text;
    }
    let mut start = text.len() - SAID_TAIL;
    while !text.is_char_boundary(start) {
        start += 1;
    }
    &text[start..]
}

pub(crate) fn what_it_said(stdout: &str, stderr: &str) -> String {
    let mut parts = Vec::new();
    if !stderr.trim().is_empty() {
        parts.push(format!("stderr: {}", tail(stderr)));
    }
    if !stdout.trim().is_empty() {
        parts.push(format!("stdout: {}", tail(stdout)));
    }
    if parts.is_empty() {
        return "it said nothing, on stdout or on stderr".to_owned();
    }
    parts.join("\n")
}

/// `None` is not «exited with zero»: it is a process killed by a signal, and
/// confusing the two sends the reader hunting a fault in the wrong place.
pub(crate) fn how_it_exited(code: Option<i32>) -> String {
    match code {
        Some(code) => format!("it exited with code {code}"),
        None => "it was killed by a signal".to_owned(),
    }
}

/// Strips the control bytes a live terminal redraw leaves behind: a CSI
/// sequence (`ESC [` to its final letter) and a bare backspace. Fault 133:
/// `ollama --verbose` corrects a partial word by moving the cursor back
/// and erasing, even with stdout piped to a file, not a terminal. The
/// escape byte is illegal inside a JSON string, so the parse dies on
/// ollama's own redraw, not on what the model answered.
fn without_terminal_redraw(text: &str) -> String {
    let mut kept = String::with_capacity(text.len());
    let mut chars = text.chars();
    while let Some(ch) = chars.next() {
        if ch == '\u{8}' {
            continue;
        }
        if ch == '\u{1b}' {
            let mut lookahead = chars.clone();
            if lookahead.next() == Some('[') {
                chars = lookahead;
                for next in chars.by_ref() {
                    if next.is_ascii_alphabetic() {
                        break;
                    }
                }
                continue;
            }
        }
        kept.push(ch);
    }
    kept
}

/// The text to read as JSON inside what an engine said.
///
/// A model often frames its answer in a fenced block, sometimes after a line of
/// courtesy: **the first fenced block** wins, and failing that the whole text.
/// The outermost braces inside a sentence are not hunted for — that rule would
/// also accept half an answer, or an example quoted in passing, and wrong data
/// getting through is worse than a red.
fn json_body(said: &str) -> String {
    let cleaned = without_terminal_redraw(said);
    let trimmed = cleaned.trim();
    let Some(open) = trimmed.find("```") else {
        return trimmed.to_string();
    };
    let after = &trimmed[open + 3..];
    // The fence line may carry the language name: it is thrown away.
    let body = match after.find('\n') {
        Some(end) => &after[end + 1..],
        None => return trimmed.to_string(),
    };
    match body.find("```") {
        Some(close) => body[..close].trim().to_string(),
        None => body.trim().to_string(),
    }
}

/// Keeps only the fields the shape declares. `allow_extra` says what is
/// **tolerated** in the answer; this pruning says what is **forwarded**, and
/// they are two questions: the first guards against a verbose engine, the
/// second against the cost of carrying it down the whole chain.
fn pruned(shape: &ValueSchema, value: Value) -> Value {
    match (shape, value) {
        // No field declared and extras allowed: the shape says «an object,
        // whatever it holds», and pruning it would forward `{}` every time.
        (
            ValueSchema::Object {
                properties,
                allow_extra: true,
                ..
            },
            value @ Value::Object(_),
        ) if properties.is_empty() => value,
        (ValueSchema::Object { properties, .. }, Value::Object(fields)) => {
            let mut kept = serde_json::Map::new();
            for (name, item) in fields {
                if let Some(inner) = properties.get(&name) {
                    kept.insert(name, pruned(inner, item));
                }
            }
            Value::Object(kept)
        }
        (ValueSchema::Array { items }, Value::Array(values)) => Value::Array(
            values
                .into_iter()
                .map(|value| pruned(items, value))
                .collect(),
        ),
        (_, value) => value,
    }
}

/// The name under which the declared shape of an answer refuses one. One name
/// for two crates: the executor asks for it to know which refusal is worth a
/// second attempt.
pub(crate) use flow::ANSWER_SHAPE_CHECK;

/// How much of the text before the break the excerpt opens with, so the reader
/// sees what led to it and not only what follows.
const BEFORE_THE_BREAK: usize = 40;

/// The excerpt a broken answer is refused with: the text **around where it
/// broke**, which the bound then cuts. The head of a 21 KB answer says nothing
/// about a quote left open at character 1038 — fault 103.
fn around_the_break(body: &str, error: &serde_json::Error) -> String {
    let Some(at) = byte_at(body, error.line(), error.column()) else {
        return body.to_owned();
    };
    let mut start = at.saturating_sub(BEFORE_THE_BREAK);
    while !body.is_char_boundary(start) {
        start -= 1;
    }
    body[start..].to_owned()
}

fn byte_at(text: &str, line: usize, column: usize) -> Option<usize> {
    let mut offset = 0;
    for (index, row) in text.split_inclusive('\n').enumerate() {
        if index + 1 == line {
            return Some(offset + column.min(row.len()));
        }
        offset += row.len();
    }
    None
}

/// Reads an engine's answer against the shape the step declared.
pub(crate) fn shaped_answer(shape: &ValueSchema, said: &str) -> Result<Value, ActionError> {
    let body = json_body(said);
    let value: Value = serde_json::from_str(&body).map_err(|error| {
        ActionError::new(
            "answer_not_json",
            format!(
                "the step demands an answer in a declared shape, and what arrived is not JSON: {error}; it said: {}",
                tail(said)
            ),
        )
        .refused(Refusal::new(
            ANSWER_SHAPE_CHECK,
            "",
            RefusalRule::NotJson,
            &around_the_break(&body, &error),
        ))
    })?;
    shape.validate(&value).map_err(|error| {
        ActionError::new(
            "answer_off_shape",
            format!(
                "the answer does not respect the shape the step declared ({error}); it said: {}",
                tail(said)
            ),
        )
        .refused(error.refused_by(ANSWER_SHAPE_CHECK))
    })?;
    Ok(pruned(shape, value))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    /// **A SHAPE WITH NO FIELD AND EXTRAS ALLOWED IS «ANY OBJECT»**, not «no
    /// field»: pruned to its declaration it came out `{}`, and the flow a model
    /// had drafted whole reached the next step empty.
    #[test]
    fn an_object_shape_declaring_no_field_hands_the_whole_object_on() {
        let shape: ValueSchema = serde_json::from_value(json!({
            "type": "object", "properties": {}, "required": [], "allow_extra": true
        }))
        .expect("a shape");
        let whole = json!({"id": "a-draft", "graph": {"steps": []}});
        assert_eq!(pruned(&shape, whole.clone()), whole);
    }

    /// **THE EXCERPT SHOWS WHERE IT BROKE, NOT WHERE IT BEGAN.** In fault 103
    /// the answer opened as valid JSON for a thousand characters and died on a
    /// quote the model had not escaped; its first 160 bytes name no defect.
    #[test]
    fn a_broken_answer_is_refused_with_the_text_around_the_break() {
        let shape = ValueSchema::Any;
        let said = format!(
            "{{\"understanding\": \"{}the rule is failure_class == \"engine_exhausted\"\"}}",
            "a".repeat(1_000)
        );

        let error = shaped_answer(&shape, &said).expect_err("the quotes are not escaped");

        let seen = &error.refusal.expect("a refusal is recorded").seen;
        assert!(seen.contains("engine_exhausted"), "{seen}");
        assert!(!seen.starts_with("{\"understanding\""), "{seen}");
    }

    /// **FAULT 133, ON A TRANSCRIPT CAPTURED FOR REAL.** `ollama run
    /// qwen2.5-coder:14b --verbose`, stdout piped to a file, answered with
    /// the byte sequence below where it corrected «nume» mid-line —
    /// `ESC[4D ESC[K`, cursor back four columns then erase to the end of
    /// line. Embedded in a shaped answer, the raw ESC byte alone is enough
    /// to make a compliant JSON parser refuse the whole string.
    #[test]
    fn a_redraw_ollama_leaves_in_the_answer_does_not_stop_the_json_from_parsing() {
        let shape: ValueSchema = serde_json::from_value(json!({
            "type": "object", "properties": {"ok": {"type": "boolean"}, "note": {"type": "string"}}, "required": ["ok"], "allow_extra": false
        }))
        .expect("a shape");
        let said = "{\"ok\": true, \"note\": \"fino al 30\u{b0} nume\u{1b}[4D\u{1b}[Knumero finito\"}";

        let value = shaped_answer(&shape, said).expect("the redraw is not the model's answer");

        assert_eq!(value["ok"], json!(true));
    }
}
