//! A flow that starts because something it watches has changed (ADR-024).
//!
//! The beat is the clock, not the cause: it reads every sensor on each tick,
//! and a flow starts only when the reading differs from the one kept for it.
//! State is compared, not signals counted, so two changes between ticks are one
//! run carrying the first reading and the last.

use flow::{Action, ActionOutcome, ActionRegistry, FlowFile, SharedState, WORKDIR_FIELD};
use ledger::{Ledger, StoreRecord};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::sync::{mpsc, Arc};
use std::time::Duration;

/// The id of the shipped source a sensor flow names in its trigger step.
pub const SENSOR_SOURCE: &str = "sensor";

/// The store collection the readings are kept in, one entry per flow id.
pub const SENSOR_READINGS: &str = "sensor_readings";

/// Five minutes: a sensor that flickers starts its flow at most this often.
pub const DEFAULT_COOLDOWN_SECS: u64 = 300;

/// How long the beat waits on one sensor before calling it blind.
pub const SENSOR_TIMEOUT: Duration = Duration::from_secs(60);

/// A kept reading larger than this is replaced by its head: the fingerprint is
/// still taken on the whole value, so a change past the head is not missed.
pub const MAX_KEPT_VALUE_BYTES: usize = 16_384;

#[derive(Debug, Clone, PartialEq)]
pub struct Sensor {
    pub action: String,
    pub with: Value,
    pub pointer: Option<String>,
    pub cooldown_secs: u64,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Declared {
    action: String,
    #[serde(default)]
    with: Option<Value>,
    #[serde(default)]
    pointer: Option<String>,
    #[serde(default)]
    cooldown_secs: Option<i64>,
}

impl Sensor {
    /// The `sensor` object of a trigger step, or the reason it cannot be one.
    pub fn from_declared(declared: &Value) -> Result<Sensor, String> {
        let declared: Declared = serde_json::from_value(declared.clone()).map_err(|error| {
            catalogue::say("flow.sensor.malformed", &[("error", &error.to_string())])
        })?;
        if let Some(pointer) = &declared.pointer {
            if !is_a_json_pointer(pointer) {
                return Err(catalogue::say(
                    "flow.sensor.bad_pointer",
                    &[("pointer", pointer)],
                ));
            }
        }
        let cooldown_secs = match declared.cooldown_secs {
            None => DEFAULT_COOLDOWN_SECS,
            Some(seconds) => u64::try_from(seconds).map_err(|_| {
                catalogue::say(
                    "flow.sensor.negative_cooldown",
                    &[("seconds", &seconds.to_string())],
                )
            })?,
        };
        Ok(Sensor {
            action: declared.action,
            with: declared.with.unwrap_or_else(|| json!({})),
            pointer: declared.pointer,
            cooldown_secs,
        })
    }

    /// Why this sensor may not run with nobody watching, if it may not.
    pub fn refusal(&self, registry: &ActionRegistry) -> Option<String> {
        let Some(action) = registry.get(&self.action) else {
            return Some(catalogue::say(
                "flow.sensor.unknown_action",
                &[("action", &self.action)],
            ));
        };
        let mut reasons = Vec::new();
        if !action.only_reads(Some(&self.with)) {
            reasons.push(catalogue::say(
                "flow.sensor.writes",
                &[("action", &self.action)],
            ));
        }
        if action.may_spend(Some(&self.with)) {
            reasons.push(catalogue::say(
                "flow.sensor.may_spend",
                &[("action", &self.action)],
            ));
        }
        (!reasons.is_empty()).then(|| reasons.join("; "))
    }
}

/// RFC 6901: empty, or a sequence of `/`-led tokens where `~` escapes only 0 or 1.
fn is_a_json_pointer(pointer: &str) -> bool {
    if pointer.is_empty() {
        return true;
    }
    if !pointer.starts_with('/') {
        return false;
    }
    let mut characters = pointer.chars();
    while let Some(character) = characters.next() {
        if character == '~' && !matches!(characters.next(), Some('0' | '1')) {
            return false;
        }
    }
    true
}

/// The sensor a flow's trigger step declares: `None` for a flow whose trigger
/// names another source, the refusal for one that names this source badly.
pub fn declared_by(flow: &FlowFile) -> Option<Result<Sensor, String>> {
    let step = flow
        .graph
        .steps()
        .iter()
        .find(|step| step.action == crate::TRIGGER_ACTION)?;
    let declared = step
        .with
        .as_ref()
        .or_else(|| flow.inputs.get(&step.id))?;
    if declared.get("source").and_then(Value::as_str) != Some(SENSOR_SOURCE) {
        return None;
    }
    Some(match declared.get("sensor") {
        Some(sensor) => Sensor::from_declared(sensor),
        None => Err(catalogue::say("flow.sensor.not_declared", &[])),
    })
}

/// What `sailor flow check` says against a flow's sensor, if anything.
pub fn refusal_of(flow: &FlowFile, registry: &ActionRegistry) -> Option<String> {
    let reason = match declared_by(flow)? {
        Ok(sensor) => sensor.refusal(registry)?,
        Err(reason) => reason,
    };
    Some(catalogue::say(
        "cli.flow.sensor_refused",
        &[("flow", &flow.id), ("reason", &reason)],
    ))
}

/// What a sensor is read with: the actions, the project root a run of the
/// same flow would work in, and how long to wait.
pub struct Eyes {
    pub registry: Arc<ActionRegistry>,
    pub root: Option<PathBuf>,
    pub timeout: Duration,
}

/// The sensor's input with its `workdir` placed the way a step's is: relative
/// hangs off the root, absolute is refused, and absent is offered the root
/// only by an action that does not call the field unknown.
fn positioned(action: &dyn Action, with: &Value, root: Option<&Path>) -> Result<Value, String> {
    let Value::Object(fields) = with else {
        return Ok(with.clone());
    };
    let mut fields = fields.clone();
    match fields.get(WORKDIR_FIELD) {
        Some(Value::String(declared)) => {
            if declared.starts_with('/') || declared.starts_with("~/") {
                return Err(catalogue::say(
                    "flow.sensor.absolute_workdir",
                    &[("workdir", declared)],
                ));
            }
            let Some(root) = root else {
                return Err(catalogue::say("flow.sensor.no_root", &[("workdir", declared)]));
            };
            let placed = root.join(declared).display().to_string();
            fields.insert(WORKDIR_FIELD.to_owned(), placed.into());
        }
        Some(_) => {}
        None => {
            if let Some(root) = root {
                let mut offered = fields.clone();
                offered.insert(WORKDIR_FIELD.to_owned(), root.display().to_string().into());
                let offered = Value::Object(offered);
                if !action.unknown_fields(&offered).iter().any(|field| field == WORKDIR_FIELD) {
                    return Ok(offered);
                }
            }
        }
    }
    Ok(Value::Object(fields))
}

/// Runs the sensor once and returns what it read.
///
/// **THE ACTION IS NOT KILLED ON A TIMEOUT**, only left behind: an action has
/// no handle to stop it by, and a `shell_check` enforces its own limit anyway.
pub fn read(sensor: &Sensor, eyes: &Eyes) -> Result<Value, String> {
    let registry = &eyes.registry;
    let timeout = eyes.timeout;
    if let Some(refused) = sensor.refusal(registry) {
        return Err(refused);
    }
    let input = match registry.get(&sensor.action) {
        Some(action) => positioned(action, &sensor.with, eyes.root.as_deref())?,
        None => sensor.with.clone(),
    };
    let (send, receive) = mpsc::channel();
    let registry = Arc::clone(registry);
    let action = sensor.action.clone();
    std::thread::spawn(move || {
        let outcome = match registry.get(&action) {
            Some(found) => found.execute(&input, &SharedState::new()),
            None => Err(flow::ActionError::new("unknown_action", action)),
        };
        let _ = send.send(outcome);
    });
    let output = match receive.recv_timeout(timeout) {
        Err(_) => {
            return Err(catalogue::say(
                "flow.sensor.timed_out",
                &[("seconds", &timeout.as_secs().to_string())],
            ))
        }
        Ok(Err(error)) => {
            return Err(catalogue::say(
                "flow.sensor.failed",
                &[("why", &error.to_string())],
            ))
        }
        Ok(Ok(ActionOutcome::Went(output))) => output,
        Ok(Ok(ActionOutcome::Waiting(why) | ActionOutcome::NotYet(why))) => {
            return Err(catalogue::say("flow.sensor.did_not_answer", &[("why", &why)]))
        }
    };
    match &sensor.pointer {
        None => Ok(output),
        Some(pointer) => output.pointer(pointer).cloned().ok_or_else(|| {
            catalogue::say("flow.sensor.pointer_reaches_nothing", &[("pointer", pointer)])
        }),
    }
}

/// What is kept for one flow between beats.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Kept {
    /// The reading, bounded by [`MAX_KEPT_VALUE_BYTES`].
    pub value: Value,
    pub fingerprint: String,
    /// When this sensor last started the flow, or tried to. The cooldown runs
    /// from here, and a first reading has none.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_started_at: Option<i64>,
}

/// What the beat decides about one reading.
#[derive(Debug, Clone, PartialEq)]
pub enum Verdict {
    /// Nothing was kept: this reading becomes the one to compare against.
    Baseline(Kept),
    Unchanged,
    /// Changed, and the cooldown still holds for this many seconds.
    Cooling(u64),
    /// Changed: start with this text, then keep `Kept`.
    Start { text: String, kept: Kept },
    /// The sensor could not read: nothing starts and nothing is kept.
    Blind(String),
}

pub fn fingerprint(value: &Value) -> String {
    flow::digest_input(value)
}

fn bounded(value: &Value) -> Value {
    let text = flow::canonical_text(value);
    if text.len() <= MAX_KEPT_VALUE_BYTES {
        return value.clone();
    }
    let mut end = MAX_KEPT_VALUE_BYTES;
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    json!({"truncated": true, "bytes": text.len(), "head": &text[..end]})
}

/// The decision, on what was kept, what was read, and the clock. Pure.
pub fn decide(
    kept: Option<&Kept>,
    reading: Result<Value, String>,
    cooldown_secs: u64,
    now: i64,
) -> Verdict {
    let value = match reading {
        Ok(value) => value,
        Err(why) => return Verdict::Blind(why),
    };
    let after = fingerprint(&value);
    let Some(kept) = kept else {
        return Verdict::Baseline(Kept {
            value: bounded(&value),
            fingerprint: after,
            last_started_at: None,
        });
    };
    if kept.fingerprint == after {
        return Verdict::Unchanged;
    }
    if let Some(started) = kept.last_started_at {
        let since = now.saturating_sub(started).max(0) as u64;
        if since < cooldown_secs {
            return Verdict::Cooling(cooldown_secs - since);
        }
    }
    let after_value = bounded(&value);
    let text = json!({
        "before": kept.value,
        "after": after_value,
        "fingerprint_before": kept.fingerprint,
        "fingerprint_after": after,
    })
    .to_string();
    Verdict::Start {
        text,
        kept: Kept {
            value: after_value,
            fingerprint: after,
            last_started_at: Some(now),
        },
    }
}

/// The reading kept for a flow, if one was.
pub fn kept_for(ledger: &Ledger, flow_id: &str) -> Result<Option<Kept>, String> {
    let Some(record) = ledger
        .read_record(SENSOR_READINGS, flow_id)
        .map_err(|error| error.to_string())?
    else {
        return Ok(None);
    };
    serde_json::from_value(record.value)
        .map(Some)
        .map_err(|error| error.to_string())
}

fn keep(ledger: &Ledger, flow_id: &str, kept: &Kept, now: i64) -> Result<(), String> {
    let value = serde_json::to_value(kept).map_err(|error| error.to_string())?;
    ledger
        .put_record(&StoreRecord {
            collection: SENSOR_READINGS.to_owned(),
            key: flow_id.to_owned(),
            value,
            written_by: "sensor".to_owned(),
            written_at: now,
        })
        .map_err(|error| error.to_string())
}

/// How one sensor flow's beat came out, in the words the beat report uses.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Sensed {
    /// `hold`, `blind`, `ran` or `broke`.
    pub word: &'static str,
    pub said: String,
}

/// Reads one flow's sensor, decides, starts the flow when it should, and keeps
/// what has to be kept. `start` is handed the trigger text.
pub fn sense(
    ledger: &Ledger,
    flow_id: &str,
    sensor: &Sensor,
    eyes: &Eyes,
    now: i64,
    start: &mut dyn FnMut(&str) -> Result<String, String>,
) -> Sensed {
    let hold = |said: String| Sensed { word: "hold", said };
    let kept = match kept_for(ledger, flow_id) {
        Ok(kept) => kept,
        Err(error) => {
            return Sensed {
                word: "blind",
                said: catalogue::say("flow.sensor.could_not_remember", &[("error", &error)]),
            }
        }
    };
    let reading = read(sensor, eyes);
    let kept_or_blind = |kept: &Kept, said: String| match keep(ledger, flow_id, kept, now) {
        Ok(()) => said,
        Err(error) => catalogue::say("flow.sensor.could_not_remember", &[("error", &error)]),
    };
    match decide(kept.as_ref(), reading, sensor.cooldown_secs, now) {
        Verdict::Blind(why) => Sensed { word: "blind", said: why },
        Verdict::Unchanged => hold(catalogue::say("flow.sensor.unchanged", &[])),
        Verdict::Cooling(seconds) => hold(catalogue::say(
            "flow.sensor.cooling",
            &[("seconds", &seconds.to_string())],
        )),
        Verdict::Baseline(first) => hold(kept_or_blind(
            &first,
            catalogue::say("flow.sensor.baseline", &[]),
        )),
        Verdict::Start { text, kept: after } => match start(&text) {
            Ok(said) => Sensed {
                word: "ran",
                said: kept_or_blind(&after, said.lines().next().unwrap_or("").to_owned()),
            },
            // The change is not consumed, so it fires again once the cooldown
            // ends; the attempt is kept so a broken flow is not hammered.
            Err(complaint) => {
                let tried = Kept {
                    last_started_at: Some(now),
                    ..kept.unwrap_or_else(|| after.clone())
                };
                let said = complaint.lines().next().unwrap_or("").to_owned();
                Sensed {
                    word: "broke",
                    said: kept_or_blind(&tried, said),
                }
            }
        },
    }
}
