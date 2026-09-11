//! The nodes a relay is composed of: measure a session, read whether anybody
//! is being waited for in it, type into it, and empty it. The order they run
//! in is a flow file, because a relay written as one function is 1,400 lines
//! whose every refusal disappears.

pub mod keeper;

use flow::{Action, ActionError, ActionOutcome, SharedState};
use serde::Deserialize;
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

pub const MEASURE_TERMINAL_ACTION: &str = "measure_terminal";
pub const TYPE_INTO_TERMINAL_ACTION: &str = "type_into_terminal";
pub const EMPTY_TERMINAL_ACTION: &str = "empty_terminal";
pub const WAIT_FREE_ACTION: &str = "wait_free";

pub fn register_relay(registry: &mut flow::ActionRegistry) {
    registry.register(MEASURE_TERMINAL_ACTION, MeasureTerminalAction);
    registry.register(TYPE_INTO_TERMINAL_ACTION, TypeIntoTerminalAction);
    registry.register(EMPTY_TERMINAL_ACTION, EmptyTerminalAction);
    registry.register(WAIT_FREE_ACTION, WaitFreeAction);
}

/// Where the terminals' files live for this step: declarable so a run can be
/// pointed elsewhere, and defaulted so the ordinary case says nothing.
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
    /// The budget past which this session counts as full. Required, and with no
    /// value written here: what counts as too full is a decision, and one taken
    /// inside a node could not be argued with by the flow that uses it.
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

/// How a line is typed is the letterbox's business, not this crate's.
fn typed_into(root: &std::path::Path, tty: &str, line: &str) -> Result<(), ActionError> {
    let address = terminal::inbox::address_in(root, tty);
    let Err(error) = terminal::inbox::press_line(&address, line) else {
        return Ok(());
    };
    // **THE OTHER ROAD, AND ONLY THEN.** A terminal Sailor holds answers at its
    // own letterbox; one somebody else opened answers to them, and asking them
    // first would put a line through a stranger for a door we own.
    let machine = toolbox::Machine::current();
    let catalog = toolbox::Catalog::load(&toolbox::default_sources(&machine));
    match keeper::of(&catalog, root, tty) {
        Some(keeper) => keeper.types_a_line(line),
        None => Err(ActionError::new(
            "terminal_not_held",
            format!(
                "{}: Sailor is not holding this terminal and nobody has said who keeps it \
                 ({error})",
                address.display()
            ),
        )),
    }
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
        // Asked here and not left to whoever wrote the flow: a forgotten step
        // would empty a session holding a person's question.
        if let Freedom::NotYet(why) = freedom_now(&catalog, &root, &spec.tty, &spec.cli)? {
            return Ok(ActionOutcome::NotYet(why));
        }
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

/// What empties a session of this command line, or the refusal. Never a line
/// written here: a product's fact inside a node makes the relay work for that
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

/// **SURFACE: reading. POWERS CLAIMED: reading one file of the store.**
#[allow(dead_code)]
#[derive(Debug, Deserialize)]
struct FreeSpec {
    tty: String,
    /// Which command line is running in there, by descriptor id. What counts
    /// as a painted prompt is a fact about one product, never about terminals.
    cli: String,
    #[serde(default)]
    store: Option<String>,
    #[serde(flatten)]
    extra: BTreeMap<String, Value>,
}

/// Waits until nobody is being waited for in a terminal Sailor holds.
/// **NOT YET, NEVER WAITING**: a step in a person's hands does not come back
/// (fault 62), and one that says not yet returns to the ready set.
struct WaitFreeAction;

impl Action for WaitFreeAction {
    fn execute(&self, input: &Value, _shared: &SharedState) -> Result<ActionOutcome, ActionError> {
        let spec: FreeSpec = read_input(input)?;
        let root = store_root(&spec.store)?;
        let machine = toolbox::Machine::current();
        let catalog = toolbox::Catalog::load(&toolbox::default_sources(&machine));
        match freedom_now(&catalog, &root, &spec.tty, &spec.cli)? {
            Freedom::NotYet(why) => Ok(ActionOutcome::NotYet(why)),
            Freedom::Free { prompt } => Ok(ActionOutcome::Went(json!({
                "tty": spec.tty,
                "cli": spec.cli,
                "free": true,
                "prompt": prompt,
            }))),
        }
    }

    fn unknown_fields(&self, declared: &Value) -> Vec<String> {
        unknown_of(declared, &["tty", "cli", "store"])
    }
}

enum Freedom {
    Free { prompt: String },
    NotYet(String),
}

/// One terminal's screen, read against what its command line declares.
/// **NOT YET IN EVERY CASE BUT ONE**: only a still screen showing the prompt
/// and none of the marks lets go of it.
fn freedom_now(
    catalog: &toolbox::Catalog,
    root: &Path,
    tty: &str,
    cli: &str,
) -> Result<Freedom, ActionError> {
    let free_when = freedom_of(catalog, cli)?;
    let painted = match painted_now(catalog, root, tty, free_when.and_still_for_seconds)? {
        Painted::Bytes(bytes) => bytes,
        Painted::NotYet(why) => return Ok(Freedom::NotYet(why)),
    };
    let seen = terminal::screen::as_a_person_sees_it(&painted);
    if let Some(held) = free_when
        .and_none_of_these
        .iter()
        .find(|mark| seen.contains(mark.as_str()))
    {
        return Ok(Freedom::NotYet(format!(
            "{tty}: «{held}» is on the screen, so somebody is being waited for"
        )));
    }
    match free_when
        .the_prompt_shows
        .iter()
        .find(|mark| seen.contains(mark.as_str()))
    {
        Some(prompt) => Ok(Freedom::Free {
            prompt: prompt.clone(),
        }),
        None => Ok(Freedom::NotYet(format!(
            "{tty}: the prompt is not painted, and a quiet screen is not a free one"
        ))),
    }
}

/// What is on that terminal now, by whichever road there is to it.
enum Painted {
    Bytes(Vec<u8>),
    NotYet(String),
}

/// The screen, and the proof that it has stood still for as long as declared.
/// **TWO ROADS AND NO THIRD**: Sailor's own, where the file says when it last
/// changed, and the keeper's, where the screen is asked for twice a stillness
/// apart, because somebody else's terminal gives no file to date.
fn painted_now(
    catalog: &toolbox::Catalog,
    root: &Path,
    tty: &str,
    still_for: u64,
) -> Result<Painted, ActionError> {
    let where_it_is = terminal::screen::address_in(root, tty);
    if let Some(painted) = terminal::screen::read(&where_it_is) {
        // A screen outlives its terminal, and what it leaves is still (fault 156).
        if !painted.is_still_held() {
            return Ok(Painted::NotYet(format!(
                "{tty}: whoever painted this screen is gone, so it says nothing about now"
            )));
        }
        let still = terminal::screen::still_for(&where_it_is).unwrap_or_default();
        if still.as_secs() < still_for {
            return Ok(Painted::NotYet(format!(
                "{tty}: the screen was painted {}s ago and stands still for {still_for}s when \
                 nobody is being waited for, so this is a session at work",
                still.as_secs()
            )));
        }
        return Ok(Painted::Bytes(painted.bytes));
    }
    let Some(keeper) = keeper::of(catalog, root, tty) else {
        return Ok(Painted::NotYet(format!(
            "{tty}: nothing has been painted for this terminal and nobody keeps it, so there is \
             nothing to read"
        )));
    };
    let first = keeper.reads_the_screen()?;
    std::thread::sleep(std::time::Duration::from_secs(still_for));
    let again = keeper.reads_the_screen()?;
    if first != again {
        return Ok(Painted::NotYet(format!(
            "{tty}: the screen changed inside {still_for}s, so this is a session at work"
        )));
    }
    Ok(Painted::Bytes(again))
}

/// What this command line says a free session of it looks like, or the refusal.
/// Never a mark written here: one guessed would call a session free on every
/// other product.
fn freedom_of(
    catalog: &toolbox::Catalog,
    cli: &str,
) -> Result<toolbox::descriptor::FreeWhen, ActionError> {
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
    known.descriptor.free_when.clone().ok_or_else(|| {
        ActionError::new(
            "freedom_not_declared",
            format!(
                "«{cli}» does not declare how a session of it shows that nobody is waiting on it. \
                 Nobody has measured it, which is not the same as it being impossible: add \
                 `free_when` to its descriptor rather than typing into a session that may be \
                 holding a question"
            ),
        )
    })
}
