//! The one channel every fact of the shell crosses on: a run's step, a beat,
//! the build's state, a terminal that closed. The bar listens here and
//! nowhere else, so a new kind of fact reaches it without a new channel.

use serde::Serialize;
use tauri::{AppHandle, Emitter};

pub const SAILOR_EVENT: &str = "sailor_event";

#[derive(Debug, Clone, Serialize)]
pub struct SailorEvent {
    /// `run` | `beat` | `build` | `terminal`, and whatever comes next.
    pub kind: String,
    pub at: i64,
    pub payload: serde_json::Value,
}

/// Emits one fact on the channel; a payload that does not serialise is
/// dropped, as a lost event on any channel is.
pub fn emit<T: Serialize>(app: &AppHandle, kind: &str, payload: &T) {
    let Ok(payload) = serde_json::to_value(payload) else {
        return;
    };
    let at = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |since| since.as_secs() as i64);
    let _ = app.emit(
        SAILOR_EVENT,
        SailorEvent {
            kind: kind.to_owned(),
            at,
            payload,
        },
    );
}
