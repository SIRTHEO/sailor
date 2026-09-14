//! The catalogue: templates and examples a flow is made from. Embedded in the
//! binary beside the shipped flows and never among them — no source lists an
//! entry, so nothing resolves, lists or runs one. A flow made from an entry
//! gets a name of its own and records which entry, and which text of it.

use crate::system::{self, FlowSource};
use crate::FlowFile;
use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;

/// Entry name and text. The name is the file's without `.json`, and the judge
/// holds the two together as `FLOWS` does for shipped flows.
pub const ENTRIES: &[(&str, &str)] = &[
    (
        "a-text-piece-from-a-brief",
        include_str!("../starters/templates/a-text-piece-from-a-brief.json"),
    ),
    (
        "a-check-that-stops-the-run",
        include_str!("../starters/examples/a-check-that-stops-the-run.json"),
    ),
    (
        "one-flow-calls-another",
        include_str!("../starters/examples/one-flow-calls-another.json"),
    ),
];

/// The key of a value filled in when a flow is made: `{"$input": "engine"}`.
pub const INPUT_MARK: &str = "$input";

/// What an entry gives a person without being run.
const SHAPES_OF_A_MACHINE: &[&str] = &["~/", "/Users/", "/home/", "/private/tmp", "/var/folders", "C:\\Users\\"];

const ENGINE_ACTION: &str = "external_engine";
const CALLING_ACTIONS: &[&str] = &["subflow", "for_each"];

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Kind {
    /// A recurring task with the parts that change left as inputs.
    Template,
    /// One capability shown working, with the result a person should see.
    Example,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Input {
    pub means: String,
    pub required: bool,
}

/// What the entry's flow leans on, declared so the judge can hold it to its graph.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Needs {
    pub actions: Vec<String>,
    /// Engine ids, or the input that names one.
    pub engines: Vec<Value>,
    pub flows: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Teaches {
    pub capability: String,
    pub result: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Entry {
    pub entry: String,
    pub kind: Kind,
    pub purpose: String,
    pub inputs: BTreeMap<String, Input>,
    pub needs: Needs,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub teaches: Option<Teaches>,
    /// The flow without its `id`: the name is given when a flow is made.
    pub flow: Value,
}

/// An entry that passed the judge, with the digest of its text.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Listed {
    pub entry: Entry,
    pub version: String,
}

/// Where a flow made from an entry is written.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Destination {
    /// The project in sight.
    Workspace,
    /// The reader's own flows, or the folder `SAILOR_FLOWS` declares.
    Home,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Created {
    pub flow: String,
    pub entry: String,
    pub origin: &'static str,
    pub dir: PathBuf,
}

pub fn version_of(text: &str) -> String {
    Sha256::digest(text.as_bytes())
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

/// Every entry, judged. A refused one stays in the list with its reasons, so a
/// broken entry is said rather than missing.
pub fn all() -> Vec<(&'static str, Result<Listed, Vec<String>>)> {
    ENTRIES
        .iter()
        .map(|(name, text)| (*name, read(name, text)))
        .collect()
}

pub fn read(name: &str, text: &str) -> Result<Listed, Vec<String>> {
    let refused = judge(name, text);
    if !refused.is_empty() {
        return Err(refused);
    }
    serde_json::from_str(text)
        .map(|entry| Listed {
            entry,
            version: version_of(text),
        })
        .map_err(|error| vec![unreadable(name, &error.to_string())])
}

pub fn find(name: &str) -> Result<Listed, String> {
    let Some((_, text)) = ENTRIES.iter().find(|(known, _)| *known == name) else {
        let names: Vec<&str> = ENTRIES.iter().map(|(known, _)| *known).collect();
        return Err(catalogue::say(
            "flow.catalogue.unknown_entry",
            &[("entry", name), ("entries", &names.join(", "))],
        ));
    };
    read(name, text).map_err(|refused| refused.join("\n"))
}

/// Every reason an entry is not fit to hand out. Empty means it passes.
pub fn judge(name: &str, text: &str) -> Vec<String> {
    let entry: Entry = match serde_json::from_str(text) {
        Ok(entry) => entry,
        Err(error) => return vec![unreadable(name, &error.to_string())],
    };
    let id = entry.entry.as_str();
    let mut refused = Vec::new();
    let mut say = |key: &str, values: &[(&str, &str)]| {
        let mut all = vec![("entry", id)];
        all.extend_from_slice(values);
        refused.push(catalogue::say(key, &all));
    };
    if id != name {
        say("flow.catalogue.judge.name_differs", &[("file", name)]);
    }
    if !is_one_sentence(&entry.purpose) {
        say("flow.catalogue.judge.purpose", &[]);
    }
    for (input, declared) in &entry.inputs {
        if declared.means.trim().is_empty() {
            say("flow.catalogue.judge.input_unexplained", &[("input", input)]);
        }
    }
    let mut filled = BTreeSet::new();
    inputs_filled_in(&entry.flow, &mut filled);
    for input in filled.iter().filter(|input| !entry.inputs.contains_key(*input)) {
        say("flow.catalogue.judge.input_undeclared", &[("input", input)]);
    }
    for input in entry.inputs.keys().filter(|input| !filled.contains(*input)) {
        say("flow.catalogue.judge.input_unused", &[("input", input)]);
    }
    let teaches = entry
        .teaches
        .as_ref()
        .is_some_and(|said| !said.capability.trim().is_empty() && !said.result.trim().is_empty());
    if entry.kind == Kind::Example && !teaches {
        say("flow.catalogue.judge.teaches_nothing", &[]);
    }

    let steps = entry
        .flow
        .pointer("/graph/steps")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let used = Used::of(&steps);
    for (key, declared, found) in [
        (
            "flow.catalogue.judge.actions_differ",
            entry.needs.actions.iter().cloned().collect::<BTreeSet<_>>(),
            used.actions,
        ),
        (
            "flow.catalogue.judge.engines_differ",
            entry.needs.engines.iter().map(Value::to_string).collect(),
            used.engines,
        ),
        (
            "flow.catalogue.judge.flows_differ",
            entry.needs.flows.iter().cloned().collect(),
            used.flows,
        ),
    ] {
        if declared != found {
            say(
                key,
                &[("declared", &listed(&declared)), ("used", &listed(&found))],
            );
        }
    }
    for flow in &entry.needs.flows {
        if !system::FLOWS.iter().any(|(shipped, _)| shipped == flow) {
            say("flow.catalogue.judge.flow_not_shipped", &[("flow", flow)]);
        }
    }
    for (step, pinned) in &used.pinned_engines {
        say("flow.catalogue.judge.engine_pinned", &[("step", step), ("engine", pinned)]);
    }
    for step in &used.pinned_models {
        say("flow.catalogue.judge.model_pinned", &[("step", step)]);
    }

    if let Ok(whole) = serde_json::from_str::<Value>(text) {
        let mut strings = Vec::new();
        strings_in(&whole, "", &mut strings);
        for (at, said) in strings {
            if SHAPES_OF_A_MACHINE.iter().any(|shape| said.contains(shape)) {
                say("flow.catalogue.judge.machine_path", &[("at", &at)]);
            }
        }
    }
    for field in ["id", "from"] {
        if entry.flow.get(field).is_some() {
            say("flow.catalogue.judge.carries_identity", &[("field", field)]);
        }
    }
    let described = entry
        .flow
        .get("description")
        .and_then(Value::as_str)
        .is_some_and(|said| !said.trim().is_empty());
    if !described {
        say("flow.catalogue.judge.no_description", &[]);
    }
    let every_input: BTreeMap<String, String> = entry
        .inputs
        .keys()
        .map(|input| (input.clone(), format!("{input}-given")))
        .collect();
    let probe = document(&entry, &version_of(text), &format!("{id}-made"), &every_input);
    if let Err(error) = system::flow_of_document(&probe) {
        say("flow.catalogue.judge.graph_refused", &[("error", &error)]);
    }
    refused
}

/// The flow document an entry makes under `name`, or the sentence saying why not.
pub fn instantiate(
    listed: &Listed,
    name: &str,
    given: &BTreeMap<String, String>,
) -> Result<Value, String> {
    let entry = &listed.entry;
    system::safe_flow_id(name)?;
    if name == entry.entry {
        return Err(catalogue::say(
            "flow.catalogue.same_name_as_entry",
            &[("entry", &entry.entry)],
        ));
    }
    if let Some(unknown) = given.keys().find(|input| !entry.inputs.contains_key(*input)) {
        let takes: Vec<&str> = entry.inputs.keys().map(String::as_str).collect();
        let takes = if takes.is_empty() {
            catalogue::say("flow.catalogue.no_inputs_taken", &[])
        } else {
            takes.join(", ")
        };
        return Err(catalogue::say(
            "flow.catalogue.unknown_input",
            &[("entry", &entry.entry), ("input", unknown), ("inputs", &takes)],
        ));
    }
    for (input, declared) in &entry.inputs {
        let blank = given.get(input).is_none_or(|value| value.trim().is_empty());
        if declared.required && blank {
            return Err(catalogue::say(
                "flow.catalogue.missing_input",
                &[("entry", &entry.entry), ("input", input), ("means", &declared.means)],
            ));
        }
    }
    Ok(document(entry, &listed.version, name, given))
}

pub fn place(sources: &[FlowSource], destination: Destination) -> Result<&FlowSource, String> {
    let (wanted, refusal): (&[&str], &str) = match destination {
        Destination::Workspace => (
            &[crate::workspace::ORIGIN_DECLARED, crate::workspace::ORIGIN_GUESSED],
            "flow.catalogue.no_workspace",
        ),
        Destination::Home => (
            &[system::YOUR_ORIGIN, system::DECLARED_ORIGIN],
            "flow.catalogue.no_home",
        ),
    };
    sources
        .iter()
        .rev()
        .find(|source| wanted.contains(&source.origin))
        .ok_or_else(|| catalogue::say(refusal, &[]))
}

/// The project when one is in sight, and the home otherwise — where `flow new` writes.
pub fn default_destination(sources: &[FlowSource]) -> Destination {
    if place(sources, Destination::Workspace).is_ok() {
        Destination::Workspace
    } else {
        Destination::Home
    }
}

/// Makes a flow from an entry and writes it. `check` is what the caller knows
/// and this crate cannot: which actions are registered, what `flow check` refuses.
pub fn create(
    sources: &[FlowSource],
    entry: &str,
    name: &str,
    given: &BTreeMap<String, String>,
    destination: Destination,
    check: &dyn Fn(&FlowFile) -> Result<(), String>,
) -> Result<Created, String> {
    let listed = find(entry)?;
    let document = instantiate(&listed, name, given)?;
    if let Some((_, origin, _)) = system::load_all(sources)
        .into_iter()
        .find(|(known, _, _)| known == name)
    {
        return Err(catalogue::say(
            "cli.flow.already_exists",
            &[("flow", name), ("origin", origin)],
        ));
    }
    let source = place(sources, destination)?;
    let flow = system::flow_of_document(&document)?;
    check(&flow)?;
    system::save_document_in(&source.dir, &document)?;
    Ok(Created {
        flow: name.to_owned(),
        entry: listed.entry.entry,
        origin: source.origin,
        dir: source.dir.clone(),
    })
}

fn document(entry: &Entry, version: &str, name: &str, given: &BTreeMap<String, String>) -> Value {
    let mut made = Map::new();
    made.insert("id".to_owned(), Value::String(name.to_owned()));
    if let Some(Value::Object(fields)) = filled(&entry.flow, given) {
        made.extend(fields);
    }
    made.insert(
        "from".to_owned(),
        json!({"catalogue": entry.entry, "version": version}),
    );
    Value::Object(made)
}

fn input_named(value: &Value) -> Option<&str> {
    match value {
        Value::Object(fields) if fields.len() == 1 => fields.get(INPUT_MARK)?.as_str(),
        _ => None,
    }
}

/// The value with every input written in. An optional input nobody gave takes
/// the field or the list item holding it away with it, rather than leaving a hole.
fn filled(value: &Value, given: &BTreeMap<String, String>) -> Option<Value> {
    if let Some(input) = input_named(value) {
        return given
            .get(input)
            .filter(|said| !said.trim().is_empty())
            .map(|said| Value::String(said.clone()));
    }
    match value {
        Value::Object(fields) => Some(Value::Object(
            fields
                .iter()
                .filter_map(|(key, item)| filled(item, given).map(|item| (key.clone(), item)))
                .collect(),
        )),
        Value::Array(items) => Some(Value::Array(
            items.iter().filter_map(|item| filled(item, given)).collect(),
        )),
        other => Some(other.clone()),
    }
}

fn inputs_filled_in(value: &Value, found: &mut BTreeSet<String>) {
    if let Some(input) = input_named(value) {
        found.insert(input.to_owned());
        return;
    }
    match value {
        Value::Object(fields) => fields.values().for_each(|item| inputs_filled_in(item, found)),
        Value::Array(items) => items.iter().for_each(|item| inputs_filled_in(item, found)),
        _ => {}
    }
}

fn strings_in(value: &Value, at: &str, found: &mut Vec<(String, String)>) {
    match value {
        Value::String(said) => found.push((at.to_owned(), said.clone())),
        Value::Object(fields) => {
            for (key, item) in fields {
                strings_in(item, &format!("{at}/{key}"), found);
            }
        }
        Value::Array(items) => {
            for (index, item) in items.iter().enumerate() {
                strings_in(item, &format!("{at}/{index}"), found);
            }
        }
        _ => {}
    }
}

#[derive(Default)]
struct Used {
    actions: BTreeSet<String>,
    engines: BTreeSet<String>,
    flows: BTreeSet<String>,
    pinned_engines: Vec<(String, String)>,
    pinned_models: Vec<String>,
}

impl Used {
    fn of(steps: &[Value]) -> Used {
        let mut used = Used::default();
        for step in steps {
            let id = step.get("id").and_then(Value::as_str).unwrap_or_default();
            let action = step.get("action").and_then(Value::as_str).unwrap_or_default();
            used.actions.insert(action.to_owned());
            let with = step.get("with").cloned().unwrap_or(Value::Null);
            if CALLING_ACTIONS.contains(&action) {
                if let Some(flow) = with.get("flow").and_then(Value::as_str) {
                    used.flows.insert(flow.to_owned());
                }
            }
            if action != ENGINE_ACTION {
                continue;
            }
            if with.get("model").is_some() {
                used.pinned_models.push(id.to_owned());
            }
            let tools = match with.get("tool") {
                Some(Value::Array(items)) => items.clone(),
                Some(one) => vec![one.clone()],
                None => Vec::new(),
            };
            for tool in tools {
                if input_named(&tool).is_none() {
                    let named = tool.as_str().map_or_else(|| tool.to_string(), str::to_owned);
                    used.pinned_engines.push((id.to_owned(), named));
                }
                used.engines.insert(tool.to_string());
            }
        }
        used
    }
}

fn listed(names: &BTreeSet<String>) -> String {
    if names.is_empty() {
        catalogue::say("flow.catalogue.none", &[])
    } else {
        names.iter().cloned().collect::<Vec<_>>().join(", ")
    }
}

fn unreadable(name: &str, error: &str) -> String {
    catalogue::say(
        "flow.catalogue.judge.unreadable",
        &[("entry", name), ("error", error)],
    )
}

/// One sentence: some words, no line break, and no full stop before the last.
fn is_one_sentence(said: &str) -> bool {
    let said = said.trim();
    let body = said.trim_end_matches(['.', '!', '?']);
    !body.is_empty()
        && !said.contains('\n')
        && ![". ", "! ", "? "].iter().any(|stop| body.contains(stop))
}
