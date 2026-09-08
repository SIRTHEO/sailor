//! What may be written down about a terminal, and what is a body.
//!
//! A body is not kept. A secret typed once into a store that keeps everything
//! stays on that disk for as long as the disk lives, and leaves it with the
//! first export. So the rule is not a list of what to drop — that list is
//! never finished — but a list of what may stay.

use serde_json::{Map, Value};

/// The keys a hook payload may leave behind, and the whole of it. Each one
/// answers a question Sailor already asks: how many, who did what and where,
/// how it ended, and what to read when diagnosing.
pub const OPERATIONAL_KEYS: &[&str] = &[
    "context_tokens",
    "cwd",
    "effort",
    "estimated_cache_write_usd",
    "hook_event_name",
    "model",
    "permission_mode",
    "prompt_cache_likely_expired",
    "prompt_id",
    "scratchpad_dir",
    "seconds_since_last_response",
    "session_id",
    "source",
    "stop_hook_active",
    "transcript_path",
    "trigger",
];

/// Set to [`KEEP_BODIES_MEANS`], and to nothing else, to keep them as they
/// arrive.
pub const KEEP_BODIES: &str = "SAILOR_KEEP_CONVERSATION_BODIES";

/// A sentence and not a `1`, so nobody arrives here by copying a flag.
pub const KEEP_BODIES_MEANS: &str = "i-accept-that-secrets-are-kept-for-ever";

pub fn bodies_are_kept(declared: Option<String>) -> bool {
    declared.as_deref() == Some(KEEP_BODIES_MEANS)
}

/// The operational metadata of a payload, or `None` when none of it survives.
///
/// An allowed key holding an object or an array is dropped with the rest: a
/// list of running tasks carries the words that started them. A payload that
/// is not a JSON object is dropped whole — what cannot be taken apart cannot
/// be filtered.
pub fn operational_only(raw: &str) -> Option<String> {
    let Ok(Value::Object(arrived)) = serde_json::from_str::<Value>(raw) else {
        return None;
    };
    let kept: Map<String, Value> = arrived
        .into_iter()
        .filter(|(key, value)| OPERATIONAL_KEYS.contains(&key.as_str()) && !value.is_object())
        .filter(|(_, value)| !value.is_array())
        .collect();
    if kept.is_empty() {
        return None;
    }
    serde_json::to_string(&kept).ok()
}

/// The metadata, unless this machine asked for the bodies.
pub fn what_is_kept(raw: &str, declared: Option<String>) -> Option<String> {
    if bodies_are_kept(declared) {
        return (!raw.trim().is_empty()).then(|| raw.to_owned());
    }
    operational_only(raw)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every secret in this file is invented.
    #[test]
    fn what_a_person_typed_and_what_the_engine_answered_do_not_survive() {
        let raw = r#"{"session_id":"abc","hook_event_name":"UserPromptSubmit",
            "prompt":"the passphrase is jacaranda-77",
            "last_assistant_message":"noted, jacaranda-77"}"#;
        let kept = operational_only(raw).expect("the metadata survives");
        assert!(!kept.contains("jacaranda"), "{kept}");
        assert!(kept.contains("abc"), "{kept}");
        assert!(kept.contains("UserPromptSubmit"), "{kept}");
    }

    #[test]
    fn a_key_nobody_allowed_does_not_reach_the_disk() {
        let kept = operational_only(r#"{"cwd":"/work","invented_tomorrow":"marlinspike-3"}"#)
            .expect("the metadata survives");
        assert!(!kept.contains("marlinspike"), "{kept}");
    }

    /// A name that sounds operational can hold the sentence a task started with.
    #[test]
    fn an_allowed_name_holding_a_container_is_dropped_with_the_bodies() {
        let raw = r#"{"cwd":"/work","source":[{"said":"kestrel-nine"}],
            "trigger":{"said":"kestrel-nine"}}"#;
        let kept = operational_only(raw).expect("the metadata survives");
        assert!(!kept.contains("kestrel"), "{kept}");
    }

    #[test]
    fn a_payload_that_is_not_an_object_is_dropped_whole() {
        assert_eq!(operational_only(r#""albatross-12""#), None);
        assert_eq!(operational_only("not json at all"), None);
        assert_eq!(operational_only("{}"), None);
    }

    #[test]
    fn keeping_the_bodies_takes_the_whole_sentence() {
        assert!(!bodies_are_kept(None));
        assert!(!bodies_are_kept(Some("1".to_owned())));
        assert!(!bodies_are_kept(Some("yes".to_owned())));
        assert!(bodies_are_kept(Some(KEEP_BODIES_MEANS.to_owned())));
    }

    #[test]
    fn asked_in_so_many_words_the_payload_arrives_whole() {
        let raw = r#"{"prompt":"the passphrase is petrel-44"}"#;
        let kept = what_is_kept(raw, Some(KEEP_BODIES_MEANS.to_owned())).expect("kept");
        assert!(kept.contains("petrel-44"), "{kept}");
        assert_eq!(what_is_kept(raw, None), None);
    }
}
