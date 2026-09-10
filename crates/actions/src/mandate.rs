//! Depositing a mandate, and taking one.
//!
//! **SURFACE: the store. POWERS CLAIMED: reading the tree with git, writing
//! one file of the store.** Nothing is typed anywhere and no session is
//! touched: a deposit that also reset a terminal would make the refusal and
//! the destruction one act.

use flow::{Action, ActionError, ActionOutcome, SharedState, StepSpecies};
use serde::Deserialize;
use serde_json::{json, Value};
use sessions::mandate::{self, Mandate, Work, Written};
use std::path::{Path, PathBuf};

pub const MANDATE_DEPOSIT_ACTION: &str = "mandate_deposit";
pub const MANDATE_RESUME_ACTION: &str = "mandate_resume";
pub const MANDATE_WAITING_ACTION: &str = "mandate_waiting";

const DEPOSIT_FIELDS: &[&str] = &[
    "tree",
    "tty",
    "session",
    "engine",
    "model",
    "tokens",
    "transcript",
    "reread",
    "alongside",
    "work",
    "store",
];

const RESUME_FIELDS: &[&str] = &["tree", "tty", "session", "store"];

const WAITING_FIELDS: &[&str] = &["tty", "store"];

pub fn register_mandate(registry: &mut flow::ActionRegistry) {
    registry.register(MANDATE_DEPOSIT_ACTION, DepositAction);
    registry.register(MANDATE_RESUME_ACTION, ResumeAction);
    registry.register(MANDATE_WAITING_ACTION, WaitingAction);
}

#[derive(Debug, Deserialize)]
struct DepositSpec {
    tree: String,
    tty: String,
    session: String,
    engine: String,
    #[serde(default)]
    model: Option<String>,
    #[serde(default)]
    tokens: u64,
    #[serde(default)]
    transcript: Option<String>,
    #[serde(default)]
    reread: Vec<String>,
    #[serde(default)]
    alongside: Vec<String>,
    /// The half only the session knows. Its shape is refused here, where its
    /// author is still alive to be asked again.
    work: Work,
    #[serde(default)]
    store: Option<String>,
}

#[derive(Debug, Deserialize)]
struct WaitingSpec {
    tty: String,
    #[serde(default)]
    store: Option<String>,
}

#[derive(Debug, Deserialize)]
struct ResumeSpec {
    tree: String,
    tty: String,
    /// Whoever is taking it on: the mark that makes a second taking visible.
    session: String,
    #[serde(default)]
    store: Option<String>,
}

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
        .map_err(|error| ActionError::new("mandate_incomplete", error.to_string()))
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

/// The tree as it stands, with the uncommitted set reduced to one digest.
fn standing_of(tree: &Path) -> (String, String, String) {
    let standing = workspace::standing(tree);
    let digest = flow::digest_input(&Value::String(standing.uncommitted));
    (standing.branch, standing.head, digest)
}

struct DepositAction;

impl Action for DepositAction {
    fn execute(&self, input: &Value, _shared: &SharedState) -> Result<ActionOutcome, ActionError> {
        deposited(input).map(ActionOutcome::Went)
    }

    fn unknown_fields(&self, declared: &Value) -> Vec<String> {
        unknown_of(declared, DEPOSIT_FIELDS)
    }

    fn species(&self) -> StepSpecies {
        StepSpecies::Repeatable
    }
}

/// The deposit itself, so the command a person types and the node a flow runs
/// are the same act: one shape, one refusal, one archive of what came before.
pub fn deposited(input: &Value) -> Result<Value, ActionError> {
    let spec: DepositSpec = read_input(input)?;
    let root = store_root(&spec.store)?;
    let tree = PathBuf::from(&spec.tree);
    let (branch, head, uncommitted) = standing_of(&tree);
    let mandate = Mandate {
        written: Written {
            tree: spec.tree,
            tty: spec.tty,
            session: spec.session,
            engine: spec.engine,
            model: spec.model,
            tokens: spec.tokens,
            at: sessions::now(),
            branch,
            head,
            uncommitted,
            reread: spec.reread,
            transcript: spec.transcript,
            alongside: spec.alongside,
        },
        work: spec.work,
        taken: None,
    };
    let blank = mandate::blank_fields(&mandate);
    if !blank.is_empty() {
        return Err(ActionError::new(
            "mandate_incomplete",
            format!(
                "the mandate leaves {} field(s) blank, and whoever could fill them is still \
                     here: {}",
                blank.len(),
                blank.join(", ")
            ),
        ));
    }
    let archived = mandate::deposit(&root, &mandate)
        .map_err(|error| ActionError::new("mandate_not_written", error.to_string()))?;
    Ok(json!({
        "tty": mandate.written.tty,
        "session": mandate.written.session,
        "at": mandate.written.at,
        "branch": mandate.written.branch,
        "head": mandate.written.head,
        "tokens": mandate.written.tokens,
        "reread": mandate.written.reread,
        "archived": archived.map(|path| path.to_string_lossy().into_owned()),
    }))
}

struct ResumeAction;

impl Action for ResumeAction {
    fn execute(&self, input: &Value, _shared: &SharedState) -> Result<ActionOutcome, ActionError> {
        let spec: ResumeSpec = read_input(input)?;
        let root = store_root(&spec.store)?;
        let path = mandate::address_in(&root, &spec.tty);
        // Not yet, and not broken: between the ask and the writing the agent
        // is still typing, and a red step there parks the relay for good.
        let Some(held) = mandate::read(&path) else {
            return Ok(ActionOutcome::NotYet(format!(
                "{}: no mandate has been deposited",
                spec.tty
            )));
        };
        if let Some(taken) = &held.taken {
            return Err(ActionError::new(
                "mandate_already_taken",
                format!(
                    "this mandate was taken by «{}»: a second successor on one mandate does the \
                     same work twice",
                    taken.by
                ),
            ));
        }
        let tree = PathBuf::from(&spec.tree);
        let (_, head, uncommitted) = standing_of(&tree);
        let freshness = mandate::freshness(&held, &head, &uncommitted);
        let moved = workspace::what_moved_since(&tree, &held.written.head);
        mandate::consume(&path, &spec.session, sessions::now())
            .map_err(|error| ActionError::new("mandate_not_taken", error.to_string()))?;
        Ok(ActionOutcome::Went(json!({
            "tty": spec.tty,
            "taken_by": spec.session,
            "stale": freshness.stale,
            "head_moved": freshness.head_moved,
            "uncommitted_moved": freshness.uncommitted_moved,
            // What landed under the work, so a stale mandate arrives with the
            // difference beside it rather than as a refusal.
            "since": moved,
            "written": held.written,
            "work": held.work,
        })))
    }

    fn unknown_fields(&self, declared: &Value) -> Vec<String> {
        unknown_of(declared, RESUME_FIELDS)
    }

    fn species(&self) -> StepSpecies {
        StepSpecies::Repeatable
    }
}

/// Whether a handover is on disk for one terminal and nobody has taken it.
///
/// **NOTHING DESTRUCTIVE STANDS ON A SILENCE.** It is what a flow asks before
/// emptying a session: a terminal with no mandate waiting has handed nothing
/// on, and emptying it would throw away work its author never wrote down.
struct WaitingAction;

impl Action for WaitingAction {
    fn execute(&self, input: &Value, _shared: &SharedState) -> Result<ActionOutcome, ActionError> {
        let spec: WaitingSpec = read_input(input)?;
        let root = store_root(&spec.store)?;
        let path = mandate::address_in(&root, &spec.tty);
        let Some(held) = mandate::read(&path) else {
            return Ok(ActionOutcome::NotYet(format!(
                "{}: no mandate has been deposited for this terminal",
                spec.tty
            )));
        };
        if let Some(taken) = &held.taken {
            return Ok(ActionOutcome::NotYet(format!(
                "{}: the mandate here was taken by «{}», so this session handed nothing on",
                spec.tty, taken.by
            )));
        }
        Ok(ActionOutcome::Went(json!({
            "tty": spec.tty,
            // The line that wrote it, so whoever empties the session reads the
            // name of the product from the handover and not from a flow file.
            "engine": held.written.engine,
            "session": held.written.session,
            "at": held.written.at,
        })))
    }

    fn unknown_fields(&self, declared: &Value) -> Vec<String> {
        unknown_of(declared, WAITING_FIELDS)
    }

    fn species(&self) -> StepSpecies {
        StepSpecies::Repeatable
    }
}
