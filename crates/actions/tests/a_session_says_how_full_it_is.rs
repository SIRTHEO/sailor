//! How full a session is, read from what the command line writes about itself.
//!
//! The three readings and the third answer: a source that cannot be read says
//! so, and a segment that was reset does not carry the segment before it.

use actions::session_fill::{from_bytes, from_rollout, from_transcript, MEASURE_SESSION_ACTION};
use flow::{ActionOutcome, SharedState};
use serde_json::{json, Value};

fn registry() -> flow::ActionRegistry {
    let mut registry = flow::ActionRegistry::default();
    actions::session_fill::register_measure(&mut registry);
    registry
}

fn measured(input: Value) -> Value {
    let registry = registry();
    let action = registry
        .get(MEASURE_SESSION_ACTION)
        .expect("the action is registered");
    let outcome = action
        .execute(&input, &SharedState::new())
        .expect("the reading does not break the step");
    match outcome {
        ActionOutcome::Went(value) => value,
        other => panic!("the reading did not go: {other:?}"),
    }
}

/// A directory of this test's own, taken down with it.
struct Scratch(std::path::PathBuf);

impl Scratch {
    fn new(name: &str) -> Self {
        let path = std::env::temp_dir().join(format!("sailor-fill-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&path);
        std::fs::create_dir_all(&path).expect("a directory to write in");
        Scratch(path)
    }

    fn holding(&self, name: &str, text: &str) -> String {
        let path = self.0.join(name);
        std::fs::write(&path, text).expect("the file is written");
        path.to_string_lossy().into_owned()
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// One answer of the command line, as it writes it down.
fn answered(input: u64, cache_read: u64, cache_write: u64) -> String {
    json!({"message": {"model": "a-model", "usage": {
        "input_tokens": input,
        "cache_read_input_tokens": cache_read,
        "cache_creation_input_tokens": cache_write,
    }}})
    .to_string()
}

/// **THE THREE FIELDS ARE ONE PROMPT.** Counting `input_tokens` alone
/// understates a warm cache by an order of magnitude, and the relay would wait
/// for a threshold the session passed hours ago.
#[test]
fn the_reading_of_a_prompt_is_the_sum_of_the_three_fields_the_command_line_writes() {
    let text = [answered(1_000, 0, 0), answered(2_000, 240_000, 8_000)].join("\n");

    let read = from_transcript(&text).expect("the transcript holds two answers");

    assert_eq!(read.tokens, 250_000, "the last prompt, whole");
    assert_eq!(read.records, 2);
    assert_eq!(read.model.as_deref(), Some("a-model"));
}

/// **A SEGMENT DOES NOT CARRY THE ONE BEFORE IT.** After a reset the context
/// starts again, and a reading that summed across it would call every session
/// full once it had been reset once.
#[test]
fn a_segment_that_was_reset_does_not_carry_the_segment_before_it() {
    let text = [
        answered(10_000, 280_000, 0),
        json!({"isCompactSummary": true, "message": {}}).to_string(),
        answered(4_000, 20_000, 1_000),
    ]
    .join("\n");

    let read = from_transcript(&text).expect("the transcript holds answers");

    assert_eq!(read.tokens, 25_000, "only the segment now open is counted");
}

/// **AND THE MARKER IS READ WHERE IT IS WRITTEN**, on a row that carries no
/// prompt of its own: a segment that opens fuller than the one it replaced is
/// caught by nothing else, and the fill rule alone would have missed it.
#[test]
fn a_segment_that_opens_fuller_than_the_one_before_is_still_a_new_segment() {
    let text = [
        answered(0, 30_000, 0),
        json!({"isCompactSummary": true, "message": {}}).to_string(),
        answered(0, 90_000, 0),
        answered(0, 95_000, 0),
    ]
    .join("\n");

    let read = from_transcript(&text).expect("the transcript holds answers");

    assert_eq!(
        read.records, 2,
        "the answer before the marker belongs to the segment before"
    );
}

/// And the fall counts even where nothing declares itself: no command line
/// takes back a third of its own prompt between two answers.
#[test]
fn a_prompt_that_falls_by_more_than_a_third_is_read_as_a_reset() {
    let text = [answered(0, 300_000, 0), answered(0, 40_000, 0)].join("\n");

    let read = from_transcript(&text).expect("the transcript holds answers");

    assert_eq!(read.tokens, 40_000);
    assert_eq!(
        read.records, 1,
        "the record before the reset belongs to the segment before"
    );
}

/// **THE OTHER COMMAND LINE'S TOTAL IS NOT ITS CONTEXT.** Its own record adds
/// every call the thread ever made and reaches tens of millions; read as fill
/// it would say «full» from the second answer onward and never say anything
/// else.
#[test]
fn the_second_reading_takes_the_prompt_and_never_the_thread_s_running_total() {
    let text = [
        json!({"type": "token_usage_record", "payload": {
            "usage": {"input_tokens": 23_605, "total_tokens": 23_610},
            "thread_token_usage": {"input_tokens": 23_605}}})
        .to_string(),
        json!({"type": "token_usage_record", "payload": {
            "usage": {"input_tokens": 41_000, "total_tokens": 41_200},
            "thread_token_usage": {"input_tokens": 33_703_262}}})
        .to_string(),
    ]
    .join("\n");

    let read = from_rollout(&text).expect("the rollout holds two records");

    assert_eq!(
        read.tokens, 41_000,
        "the prompt of the last answer, not the thread's total"
    );
    assert_eq!(read.records, 2);
}

/// The estimate for a held terminal whose engine says nothing about itself.
#[test]
fn the_estimate_pays_the_prologue_no_byte_ever_crosses() {
    assert_eq!(
        from_bytes(0).tokens,
        60_000,
        "the prologue costs tokens and no bytes"
    );
    assert_eq!(from_bytes(100_000).tokens, 128_000);
}

/// **THE THIRD ANSWER IS «I DO NOT KNOW».** A named source that cannot be read
/// leaves the relay unarmed, and saying «below» would let a full session pass
/// for a fresh one — the failure the whole relay exists to prevent.
#[test]
fn a_source_that_cannot_be_read_says_so_instead_of_saying_below() {
    let answer = measured(json!({"transcript": "/nowhere/at/all.jsonl"}));

    assert_eq!(answer["state"], json!("unknown"), "{answer}");
    assert_eq!(
        answer["source"],
        json!("transcript"),
        "and it names what it tried to read"
    );
    assert_eq!(answer["tokens"], json!(0));
}

/// A file that exists and holds nothing readable answers the same way.
#[test]
fn a_transcript_of_nothing_readable_is_unknown_too() {
    let scratch = Scratch::new("unreadable");
    let path = scratch.holding("empty.jsonl", "not json at all\n{\"message\": {}}\n");

    let answer = measured(json!({"transcript": path}));

    assert_eq!(answer["state"], json!("unknown"), "{answer}");
}

/// The three standings, and the thresholds a step may name for itself.
#[test]
fn the_standing_is_below_then_warn_then_oblige() {
    let scratch = Scratch::new("standing");
    for (tokens, expected) in [
        (100_000u64, "below"),
        (200_000, "warn"),
        (260_000, "oblige"),
    ] {
        let path = scratch.holding(&format!("{tokens}.jsonl"), &answered(tokens, 0, 0));

        let answer = measured(json!({"transcript": path}));

        assert_eq!(answer["state"], json!(expected), "{tokens}: {answer}");
        assert_eq!(answer["tokens"], json!(tokens));
    }
}

#[test]
fn a_step_may_name_thresholds_of_its_own() {
    let scratch = Scratch::new("thresholds");
    let path = scratch.holding("s.jsonl", &answered(90_000, 0, 0));

    let answer = measured(json!({"transcript": path, "warn": 50_000, "oblige": 80_000}));

    assert_eq!(answer["state"], json!("oblige"), "{answer}");
    assert_eq!(
        answer["oblige"],
        json!(80_000),
        "and the answer says what it judged against"
    );
}

/// **THE SECOND SIGNAL, AND THE ONE WITH A RIGHT ANSWER KNOWN.** An edit
/// refused because the file does not say what the agent believed it said is a
/// memory failing, not a task failing; measured over three thousand sessions
/// it is the one thing that rises with the context while every other error
/// falls.
#[test]
fn an_edit_refused_because_the_file_says_otherwise_is_counted_as_a_memory_miss() {
    let text = [
        answered(1_000, 0, 0),
        json!({"message": {"content": [
            {"type": "tool_result", "is_error": true, "content": "<tool_use_error>String to replace not found in file."},
            {"type": "tool_result", "is_error": true, "content": "Exit code 1: no such file"},
            {"type": "tool_result", "is_error": true, "content": "File has not been read yet. Read it first."},
        ]}})
        .to_string(),
    ]
    .join("\n");

    let read = from_transcript(&text).expect("the transcript holds an answer");

    assert_eq!(
        read.misses, 2,
        "the two of memory, and not the one of the world"
    );
}

/// The thresholds are the measured ones, and they are absolute: nine tenths of
/// a large window sits past every point where the miss rate had already
/// doubled.
#[test]
fn the_thresholds_are_tokens_and_not_a_share_of_a_window() {
    assert_eq!(actions::session_fill::WARN_TOKENS, 150_000);
    assert_eq!(actions::session_fill::OBLIGE_TOKENS, 250_000);
}
