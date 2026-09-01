//! The nodes a relay is composed of: measure a session, type into it, empty
//! it, and take what it left behind.
//!
//! Four powers over the world, and no sequence. The order in which they run is
//! a flow file, because a relay written as one function is 1,400 lines whose
//! every refusal disappears, and the refusals are the part nobody could see.

use flow::{Action, ActionError, ActionOutcome, SharedState};
use serde::Deserialize;
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::path::PathBuf;

pub const MEASURE_TERMINAL_ACTION: &str = "measure_terminal";
pub const TYPE_INTO_TERMINAL_ACTION: &str = "type_into_terminal";
pub const EMPTY_TERMINAL_ACTION: &str = "empty_terminal";
pub const TAKE_MANDATE_ACTION: &str = "take_mandate";

pub fn register_relay(registry: &mut flow::ActionRegistry) {
    registry.register(MEASURE_TERMINAL_ACTION, MeasureTerminalAction);
    registry.register(TYPE_INTO_TERMINAL_ACTION, TypeIntoTerminalAction);
    registry.register(EMPTY_TERMINAL_ACTION, EmptyTerminalAction);
    registry.register(TAKE_MANDATE_ACTION, TakeMandateAction);
}

/// Where the terminals' files live for this step.
///
/// Declarable so a run can be pointed somewhere else, and defaulted to the
/// store so the ordinary case says nothing.
fn store_root(declared: &Option<String>) -> Result<PathBuf, ActionError> {
    match declared {
        Some(written) => Ok(PathBuf::from(written)),
        None => ledger::default_directory().ok_or_else(|| {
            ActionError::new("no_store", "I cannot tell where the store lives".to_owned())
        }),
    }
}

fn read_input<T: serde::de::DeserializeOwned>(input: &Value) -> Result<T, ActionError> {
    serde_json::from_value(input.clone())
        .map_err(|error| ActionError::new("invalid_input", error.to_string()))
}

fn unknown_of(declared: &Value, known: &[&str]) -> Vec<String> {
    match declared.as_object() {
        Some(fields) => fields
            .keys()
            .filter(|name| !known.contains(&name.as_str()))
            .cloned()
            .collect(),
        None => Vec::new(),
    }
}

/// **SURFACE: reading. POWERS CLAIMED: reading one file of the store.**
#[allow(dead_code)]
#[derive(Debug, Deserialize)]
struct MeasureSpec {
    tty: String,
    /// The budget past which this session counts as full.
    ///
    /// Required, and with no value written here. What counts as too full is a
    /// decision, and one taken inside a node could not be argued with by the
    /// flow that uses it.
    ceiling: u64,
    #[serde(default)]
    store: Option<String>,
    #[serde(flatten)]
    extra: BTreeMap<String, Value>,
}

struct MeasureTerminalAction;

impl Action for MeasureTerminalAction {
    fn execute(&self, input: &Value, _shared: &SharedState) -> Result<ActionOutcome, ActionError> {
        let spec: MeasureSpec = read_input(input)?;
        let root = store_root(&spec.store)?;
        // Unreadable is not empty. Answering zero here would let a full session
        // pass for a fresh one, which is the direction this must never take.
        let Some(counted) = terminal::tally::read(&terminal::tally::address_in(&root, &spec.tty))
        else {
            return Ok(ActionOutcome::Went(json!({
                "tty": spec.tty,
                "counted": false,
                "past_the_ceiling": false,
                "why": "nothing has been counted for this terminal",
            })));
        };
        let reading = sessions::fullness::measure(
            counted.total(),
            &sessions::fullness::Model::default(),
            spec.ceiling,
        );
        Ok(ActionOutcome::Went(json!({
            "tty": spec.tty,
            "counted": true,
            "bytes": reading.bytes,
            "estimated_tokens": reading.estimated_tokens,
            "ceiling": reading.ceiling,
            "past_the_ceiling": reading.past_the_ceiling,
        })))
    }

    fn unknown_fields(&self, declared: &Value) -> Vec<String> {
        unknown_of(declared, &["tty", "ceiling", "store"])
    }
}

/// **SURFACE: writing into a live session. POWERS CLAIMED: typing.**
#[allow(dead_code)]
#[derive(Debug, Deserialize)]
struct TypeSpec {
    tty: String,
    line: String,
    #[serde(default)]
    store: Option<String>,
    #[serde(flatten)]
    extra: BTreeMap<String, Value>,
}

struct TypeIntoTerminalAction;

impl Action for TypeIntoTerminalAction {
    fn execute(&self, input: &Value, _shared: &SharedState) -> Result<ActionOutcome, ActionError> {
        let spec: TypeSpec = read_input(input)?;
        let root = store_root(&spec.store)?;
        typed_into(&root, &spec.tty, &spec.line)?;
        // The instant travels with the outcome so a later step can refuse a
        // mandate older than the moment one was asked for.
        Ok(ActionOutcome::Went(json!({
            "tty": spec.tty,
            "typed": spec.line,
            "at": sessions::now(),
        })))
    }

    fn unknown_fields(&self, declared: &Value) -> Vec<String> {
        unknown_of(declared, &["tty", "line", "store"])
    }
}

/// The carriage return is what a terminal receives when someone presses Enter.
fn typed_into(root: &std::path::Path, tty: &str, line: &str) -> Result<(), ActionError> {
    let address = terminal::inbox::address_in(root, tty);
    let mut bytes = line.as_bytes().to_vec();
    bytes.push(b'\r');
    terminal::inbox::press(&address, &bytes).map_err(|error| {
        ActionError::new(
            "nobody_accompanying",
            format!("{}: nobody is accompanying this terminal ({error})", address.display()),
        )
    })
}

/// **SURFACE: writing into a live session. POWERS CLAIMED: typing, and asking
/// a descriptor.**
#[allow(dead_code)]
#[derive(Debug, Deserialize)]
struct EmptySpec {
    tty: String,
    /// Which command line is running in there, by descriptor id.
    cli: String,
    #[serde(default)]
    store: Option<String>,
    #[serde(flatten)]
    extra: BTreeMap<String, Value>,
}

struct EmptyTerminalAction;

impl Action for EmptyTerminalAction {
    fn execute(&self, input: &Value, _shared: &SharedState) -> Result<ActionOutcome, ActionError> {
        let spec: EmptySpec = read_input(input)?;
        let root = store_root(&spec.store)?;
        let machine = toolbox::Machine::current();
        let catalog = toolbox::Catalog::load(&toolbox::default_sources(&machine));
        let line = reset_line_of(&catalog, &spec.cli)?;
        typed_into(&root, &spec.tty, &line)?;
        Ok(ActionOutcome::Went(json!({
            "tty": spec.tty,
            "cli": spec.cli,
            "typed": line,
        })))
    }

    fn unknown_fields(&self, declared: &Value) -> Vec<String> {
        unknown_of(declared, &["tty", "cli", "store"])
    }
}

/// What empties a session of this command line, or the refusal.
///
/// Never a line written here. What empties a context is a fact about one
/// product, and a product's fact inside a node makes the relay work for that
/// one and misfire silently on every other.
pub fn reset_line_of(catalog: &toolbox::Catalog, cli: &str) -> Result<String, ActionError> {
    let known = catalog
        .live()
        .into_iter()
        .find(|loaded| loaded.descriptor.id == cli)
        .ok_or_else(|| {
            ActionError::new(
                "unknown_command_line",
                format!("«{cli}»: no descriptor of that name is loaded"),
            )
        })?;
    known
        .descriptor
        .reset_line()
        .map(str::to_owned)
        .ok_or_else(|| {
            ActionError::new(
                "reset_not_declared",
                format!(
                    "«{cli}» does not declare how a running session of it is emptied. \
                     Nobody has measured it, which is not the same as it being impossible: \
                     add `reset_context` to its descriptor rather than guessing a line"
                ),
            )
        })
}

/// **SURFACE: reading. POWERS CLAIMED: reading one file, and removing it.**
#[allow(dead_code)]
#[derive(Debug, Deserialize)]
struct TakeSpec {
    tty: String,
    /// A mandate older than this is somebody else's leftover.
    ///
    /// The same terminal hands over many times. Without this, a beat would
    /// read the mandate of the previous handover and send the successor back
    /// to work already done.
    #[serde(default)]
    not_before: Option<i64>,
    #[serde(default)]
    store: Option<String>,
    #[serde(flatten)]
    extra: BTreeMap<String, Value>,
}

struct TakeMandateAction;

impl Action for TakeMandateAction {
    fn execute(&self, input: &Value, _shared: &SharedState) -> Result<ActionOutcome, ActionError> {
        let spec: TakeSpec = read_input(input)?;
        let root = store_root(&spec.store)?;
        let path = terminal::mandate::address_in(&root, &spec.tty);
        let Some(left) = terminal::mandate::read(&path) else {
            return Ok(ActionOutcome::Waiting(format!(
                "{}: no mandate has been left yet",
                spec.tty
            )));
        };
        if spec.not_before.is_some_and(|floor| left.at < floor) {
            return Ok(ActionOutcome::Waiting(format!(
                "{}: the only mandate here is older than this handover",
                spec.tty
            )));
        }
        // Taken and not merely read: left in place, the next beat would hand
        // the same work on a second time.
        terminal::mandate::taken(&path)
            .map_err(|error| ActionError::new("mandate_not_taken", error.to_string()))?;
        Ok(ActionOutcome::Went(json!({
            "tty": spec.tty,
            "mandate": left.text,
            "written_at": left.at,
        })))
    }

    fn unknown_fields(&self, declared: &Value) -> Vec<String> {
        unknown_of(declared, &["tty", "not_before", "store"])
    }
}
