//! The rows of the `roles` collection, written by a person: a role names the
//! tools a step may use, and a flow that names one nobody wrote cannot run.

use flow::ActionError;
use ledger::{Ledger, StoreRecord};
use serde_json::{json, Value};

const COLLECTION: &str = "roles";

/// What a person asks for: this role resolves to these tools, in this order.
pub struct SetRole {
    pub name: String,
    pub tools: Vec<String>,
    pub replace: bool,
}

/// What was written, and the tools it took the place of.
#[derive(Debug)]
pub struct Set {
    pub tools: Vec<String>,
    pub replaced: Option<Vec<String>>,
}

fn refused(said: String) -> ActionError {
    ActionError::new("invalid_input", said)
}

/// Writes the row. A tool is accepted only if the machine declares it, a name
/// only if it is one word, and a row already there is kept unless replacing it
/// was asked: a role decides which engines run, so widening one is never a side
/// effect.
pub fn set(
    ledger: &Ledger,
    ask: &SetRole,
    declares: &dyn Fn(&str) -> bool,
    at: i64,
) -> Result<Set, ActionError> {
    let one_word = !ask.name.is_empty()
        && ask
            .name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '-'));
    if !one_word {
        return Err(refused(format!(
            "«{}» is not a role name: letters, digits, _ and - only",
            ask.name
        )));
    }
    if ask.tools.is_empty() {
        return Err(refused(format!(
            "role «{}» needs at least one tool",
            ask.name
        )));
    }
    for tool in &ask.tools {
        if tool.is_empty() || tool.contains('@') || tool.chars().any(char::is_whitespace) {
            return Err(refused(format!(
                "«{tool}» is not a tool name: no spaces, no @"
            )));
        }
        if !declares(tool) {
            return Err(refused(format!(
                "no tool is declared as «{tool}» on this machine"
            )));
        }
    }
    let previous = ledger
        .read_record(COLLECTION, &ask.name)
        .map_err(|error| ActionError::new("store_unreadable", error.to_string()))?;
    let replaced = match previous {
        None => None,
        Some(record) => {
            let held = held_tools(&record.value);
            if record.value.get("account").is_some() {
                return Err(refused(format!(
                    "role «{}» carries an account, which this cannot keep: edit it by hand",
                    ask.name
                )));
            }
            if !ask.replace {
                return Err(refused(format!(
                    "role «{}» already resolves to {}; ask to replace it to change that",
                    ask.name,
                    held.join(", ")
                )));
            }
            Some(held)
        }
    };
    ledger
        .put_record(&StoreRecord {
            collection: COLLECTION.to_owned(),
            key: ask.name.clone(),
            value: json!({"tools": ask.tools}),
            written_by: "a person, by hand".to_owned(),
            written_at: at,
        })
        .map_err(|error| ActionError::new("store_unreadable", error.to_string()))?;
    Ok(Set {
        tools: ask.tools.clone(),
        replaced,
    })
}

/// Every role and its tools, by name.
pub fn list(ledger: &Ledger) -> Result<Vec<(String, Vec<String>)>, ActionError> {
    let records = ledger
        .records_in(COLLECTION)
        .map_err(|error| ActionError::new("store_unreadable", error.to_string()))?;
    Ok(records
        .into_iter()
        .map(|record| (record.key, held_tools(&record.value)))
        .collect())
}

fn held_tools(value: &Value) -> Vec<String> {
    value
        .get("tools")
        .and_then(Value::as_array)
        .map(|tools| {
            tools
                .iter()
                .filter_map(Value::as_str)
                .map(str::to_owned)
                .collect()
        })
        .unwrap_or_default()
}
