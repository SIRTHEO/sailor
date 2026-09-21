//! **A FLOW STEP IS NOT A SHELL PROGRAM.** Both of these stood in four flow
//! files as a string of shell — 1.966 characters and 314 — unparsed, untested,
//! and failing by quoting or by timeout. Goal #47. Neither reads the working
//! tree and neither trusts a binary on the path: `workspace::delivery` holds
//! the reading, and `sailor policy` reads through it too.

use flow::{Action, ActionError, ActionOutcome, SharedState, StepSpecies};
use serde::Deserialize;
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::path::PathBuf;

pub const DELIVERY_POLICY_ACTION: &str = "delivery_policy";
pub const THE_PERSON_SAID_ACTION: &str = "the_person_said";

const POLICY_FIELDS: &[&str] = &["repo"];
const SAID_FIELDS: &[&str] = &["said", "key"];

pub fn register_delivery(registry: &mut flow::ActionRegistry) {
    registry.register(DELIVERY_POLICY_ACTION, DeliveryPolicyAction);
    registry.register(THE_PERSON_SAID_ACTION, ThePersonSaidAction);
    registry.register(DELIVERY_REQUEST_ACTION, DeliveryRequestAction);
}

fn unknown_among(declared: &Value, known: &[&str]) -> Vec<String> {
    match declared.as_object() {
        Some(fields) => fields
            .keys()
            .filter(|name| !known.contains(&name.as_str()))
            .cloned()
            .collect(),
        None => Vec::new(),
    }
}

#[derive(Debug, Deserialize)]
struct PolicySpec {
    repo: PathBuf,
}

struct DeliveryPolicyAction;

impl Action for DeliveryPolicyAction {
    fn execute(&self, input: &Value, _shared: &SharedState) -> Result<ActionOutcome, ActionError> {
        let spec: PolicySpec = serde_json::from_value(input.clone())
            .map_err(|error| ActionError::new("invalid_input", error.to_string()))?;
        let policy = workspace::delivery::policy_on_the_trunk(&spec.repo)
            .map_err(|why| ActionError::new("no_trusted_policy", why.to_string()))?;
        let said = serde_json::to_value(&policy)
            .map_err(|error| ActionError::new("invalid_input", error.to_string()))?;
        Ok(ActionOutcome::Went(said))
    }

    fn unknown_fields(&self, declared: &Value) -> Vec<String> {
        unknown_among(declared, POLICY_FIELDS)
    }

    fn species(&self) -> StepSpecies {
        StepSpecies::Repeatable
    }

    fn redo_evidence(&self, _record: &flow::StepRecord) -> flow::RedoEvidence {
        flow::RedoEvidence::TouchesNothing
    }
}

#[derive(Debug, Deserialize)]
struct SaidSpec {
    #[serde(default)]
    said: Value,
    key: String,
}

/// A reference hands on a value or its JSON, and the flows spell it both ways.
fn as_answer(said: &Value) -> Value {
    match said.as_str() {
        Some(text) => serde_json::from_str(text).unwrap_or(Value::Null),
        None => said.clone(),
    }
}

/// **SILENCE IS NOT A YES, AND NEITHER IS ANYTHING BUT `true`.** A missing
/// answer, one that is not an object, and the string `"true"` all stop the
/// flow: none of them is a person having said yes.
struct ThePersonSaidAction;

impl Action for ThePersonSaidAction {
    fn execute(&self, input: &Value, _shared: &SharedState) -> Result<ActionOutcome, ActionError> {
        let spec: SaidSpec = serde_json::from_value(input.clone())
            .map_err(|error| ActionError::new("invalid_input", error.to_string()))?;
        if as_answer(&spec.said).get(&spec.key) != Some(&Value::Bool(true)) {
            return Err(ActionError::new(
                "not_authorized",
                format!(
                    "the person did not write {}: true; nothing proceeds",
                    spec.key
                ),
            ));
        }
        Ok(ActionOutcome::Went(json!({ "confirmed": true })))
    }

    fn unknown_fields(&self, declared: &Value) -> Vec<String> {
        unknown_among(declared, SAID_FIELDS)
    }

    fn species(&self) -> StepSpecies {
        StepSpecies::Repeatable
    }

    fn redo_evidence(&self, _record: &flow::StepRecord) -> flow::RedoEvidence {
        flow::RedoEvidence::TouchesNothing
    }
}

pub const DELIVERY_REQUEST_ACTION: &str = "delivery_request";

const REQUEST_FIELDS: &[&str] = &["mandate", "required", "optional", "exactly_one_of"];

/// The shapes a field of a mandate must have to be one. Checked by name because
/// that is what the mandate names them: a `branch` that is not a branch name
/// reaches git as an option, and a `forge_repo` that is not `owner/name` reaches
/// the forge as a path.
fn is_a_branch_name(said: &str) -> bool {
    let mut letters = said.chars();
    letters.next().is_some_and(char::is_alphanumeric)
        && letters.all(|letter| letter.is_alphanumeric() || "._/-".contains(letter))
}

fn is_a_number(said: &str) -> bool {
    !said.is_empty() && said.chars().all(|letter| letter.is_ascii_digit())
}

/// MAJOR.MINOR.PATCH, and whatever a hyphen introduces after it.
fn is_a_version(said: &str) -> bool {
    let (numbers, after) = said.split_once('-').map_or((said, None), |(a, b)| (a, Some(b)));
    let counted: Vec<&str> = numbers.split('.').collect();
    counted.len() == 3
        && counted.iter().all(|part| is_a_number(part))
        && after.is_none_or(|part| {
            !part.is_empty()
                && part
                    .chars()
                    .all(|letter| letter.is_ascii_alphanumeric() || letter == '.')
        })
}

fn is_owner_and_name(said: &str) -> bool {
    let plain = |part: &str| {
        !part.is_empty()
            && part
                .chars()
                .all(|letter| letter.is_ascii_alphanumeric() || "._-".contains(letter))
    };
    said.split_once('/')
        .is_some_and(|(owner, name)| plain(owner) && plain(name))
}

/// A field's name, the shape it must have, and how to say it has not.
type Shape = (&'static str, fn(&str) -> bool, &'static str);

#[derive(Debug, Deserialize)]
struct RequestSpec {
    /// **NOT `text`**: a step's `with` is laid over what it was handed, so a
    /// field named after the one it points at covers the value before the
    /// pointer is read, and the step is handed the pointer itself.
    mandate: String,
    required: Vec<String>,
    #[serde(default)]
    optional: BTreeMap<String, String>,
    /// Two fields of which the mandate must name exactly one, such as the issue
    /// a change closes and the reason there is none.
    #[serde(default)]
    exactly_one_of: Vec<String>,
}

/// **A MANDATE MISREAD IS FOUND OUT AFTER DELIVERY**, which is why this refuses
/// a field it does not read rather than ignoring it: a mandate naming `bran`
/// for `branch` would otherwise deliver something nobody asked for.
struct DeliveryRequestAction;

impl Action for DeliveryRequestAction {
    fn execute(&self, input: &Value, _shared: &SharedState) -> Result<ActionOutcome, ActionError> {
        let spec: RequestSpec = serde_json::from_value(input.clone())
            .map_err(|error| ActionError::new("invalid_input", error.to_string()))?;
        let said = read_mandate(&spec).map_err(|why| ActionError::new("mandate_off_shape", why))?;
        Ok(ActionOutcome::Went(said))
    }

    fn unknown_fields(&self, declared: &Value) -> Vec<String> {
        unknown_among(declared, REQUEST_FIELDS)
    }

    fn species(&self) -> StepSpecies {
        StepSpecies::Repeatable
    }

    fn redo_evidence(&self, _record: &flow::StepRecord) -> flow::RedoEvidence {
        flow::RedoEvidence::TouchesNothing
    }
}

/// An issue written `87` and one written `"87"` are the same mandate: a person
/// writing one by hand has no reason to know which the flow wants.
fn as_word(said: Option<&Value>) -> String {
    match said {
        Some(Value::String(text)) => text.clone(),
        Some(Value::Null) | None => String::new(),
        Some(other) => other.to_string(),
    }
}

fn read_mandate(spec: &RequestSpec) -> Result<Value, String> {
    let asked: Value =
        serde_json::from_str(&spec.mandate).map_err(|why| format!("the mandate is not one JSON object: {why}"))?;
    let asked = asked
        .as_object()
        .ok_or("the mandate is not one JSON object")?;

    let missing: Vec<&str> = spec
        .required
        .iter()
        .filter(|key| as_word(asked.get(key.as_str())).trim().is_empty())
        .map(String::as_str)
        .collect();
    if !missing.is_empty() {
        return Err(format!("the mandate does not name: {}", missing.join(", ")));
    }
    let unknown: Vec<&str> = asked
        .keys()
        .filter(|key| !spec.required.contains(key) && !spec.optional.contains_key(*key))
        .map(String::as_str)
        .collect();
    if !unknown.is_empty() {
        return Err(format!(
            "the mandate names what this flow does not read: {}",
            unknown.join(", ")
        ));
    }

    let word = |key: &str, fallback: &str| match asked.get(key) {
        Some(said) => as_word(Some(said)),
        None => fallback.to_owned(),
    };
    let mut out: BTreeMap<String, String> = spec
        .optional
        .iter()
        .map(|(key, fallback)| (key.clone(), word(key, fallback)))
        .collect();
    for key in &spec.required {
        out.insert(key.clone(), word(key, ""));
    }

    let repo = PathBuf::from(out.get("repo").map(String::as_str).unwrap_or_default());
    if !repo.is_absolute() {
        return Err(format!(
            "the repo is not an absolute path to a tree: {}",
            repo.display()
        ));
    }
    let trunk = workspace::declared_trunk(&repo)?;
    let policy = workspace::delivery::policy_on_the_trunk(&repo)
        .map_err(|why| format!("the delivery policy is not readable from {trunk}: {why}"))?;
    if policy.remote.is_empty() {
        return Err(format!(
            "the delivery policy on {trunk} names no remote: the remote is a fact a repository declares"
        ));
    }
    if out.get("base").is_none_or(String::is_empty) {
        out.insert("base".to_owned(), trunk.clone());
    }
    out.insert("trunk".to_owned(), trunk);
    out.insert("remote".to_owned(), policy.remote);

    let shaped: [Shape; 4] = [
        ("branch", is_a_branch_name, "is not a plain branch name"),
        ("base", is_a_branch_name, "is not a plain branch name"),
        ("issue", is_a_number, "is not a number"),
        ("version", is_a_version, "is not MAJOR.MINOR.PATCH"),
    ];
    for (key, shape, complaint) in shaped {
        if let Some(said) = out.get(key).filter(|said| !said.is_empty()) {
            if !shape(said) {
                return Err(format!("the {key} {complaint}: {said}"));
            }
        }
    }
    if let Some(said) = out.get("forge_repo").filter(|said| !said.is_empty()) {
        if !is_owner_and_name(said) {
            return Err(format!("the forge_repo is not owner/name: {said}"));
        }
    }

    if !spec.exactly_one_of.is_empty() {
        let named: Vec<&String> = spec
            .exactly_one_of
            .iter()
            .filter(|key| out.get(*key).is_some_and(|said| !said.trim().is_empty()))
            .collect();
        let all = spec.exactly_one_of.join(" or ");
        if named.len() > 1 {
            return Err(format!("the mandate names more than one of {all}: it is one or the other"));
        }
        if named.is_empty() {
            return Err(format!("the mandate names none of {all}: say one of them"));
        }
    }

    Ok(serde_json::to_value(out).unwrap_or(Value::Null))
}
