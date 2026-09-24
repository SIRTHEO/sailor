//! Which version of a shipped flow a person's copy replaces. A copy that
//! wins over a shipped flow keeps winning, but once the shipped one has
//! risen past what was copied the reader is told, instead of the copy
//! hiding every later correction in silence.

use crate::system::FLOWS;
use crate::FlowFile;
use serde_json::Value;

/// A copy that replaces an older version of its shipped flow than the one
/// the product carries now. `copied` is absent when the copy never said.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct Stale {
    pub shipped: u32,
    pub copied: Option<u32>,
}

/// The version of the shipped flow of this name, if one is shipped and says.
pub fn shipped_version(name: &str) -> Option<u32> {
    let (_, text) = FLOWS.iter().find(|(shipped, _)| *shipped == name)?;
    serde_json::from_str::<FlowFile>(text).ok()?.version
}

/// The shipped version a copy was made from: what it declares it replaces,
/// or else the version it still carries from a copy made by hand.
pub fn copied_from(yours: &FlowFile) -> Option<u32> {
    yours.replaces.or(yours.version)
}

/// Whether `yours`, standing in place of `shipped`, was copied from an
/// older version than the one shipped now.
pub fn stale(yours: &FlowFile, shipped: &FlowFile) -> Option<Stale> {
    let now = shipped.version?;
    let copied = copied_from(yours);
    match copied {
        Some(copied) if copied >= now => None,
        _ => Some(Stale {
            shipped: now,
            copied,
        }),
    }
}

/// Stamps a person's copy of a shipped flow with the version it replaces,
/// and takes off the shipped `version` it may still carry. Only a copy
/// written where no file `standing` was is taken to replace today's version;
/// one rewritten in place keeps what it said, or its silence.
pub fn stamp_the_copy(document: &mut Value, standing: Option<&Value>) {
    let Some(id) = document.get("id").and_then(Value::as_str) else {
        return;
    };
    let Some(shipped) = shipped_version(id) else {
        return;
    };
    let Some(fields) = document.as_object_mut() else {
        return;
    };
    let carried = fields.remove("version").and_then(|value| value.as_u64());
    let declared = fields.get("replaces").and_then(Value::as_u64);
    let said_before = standing.and_then(|before| {
        (before.get("replaces").and_then(Value::as_u64))
            .or_else(|| before.get("version").and_then(Value::as_u64))
    });
    let written_anew = standing.is_none().then_some(u64::from(shipped));
    if let Some(replaces) = declared.or(carried).or(said_before).or(written_anew) {
        fields.insert("replaces".to_owned(), Value::from(replaces));
    }
}

/// What a flow of yours changes from the shipped flow it replaces, as JSON
/// pointers: a field named is a field the reader can open in both files, which
/// a rendered diff is not.
pub fn what_yours_changes(yours: &FlowFile, shipped: &FlowFile) -> Vec<String> {
    let (Ok(yours), Ok(shipped)) = (serde_json::to_value(yours), serde_json::to_value(shipped))
    else {
        return Vec::new();
    };
    let mut found = Vec::new();
    walk_apart("", &yours, &shipped, &mut found);
    // Which version each says it is, or replaces, is not a change to the flow.
    found.retain(|field| field != "/version" && field != "/replaces");
    found
}

fn walk_apart(
    at: &str,
    yours: &serde_json::Value,
    shipped: &serde_json::Value,
    found: &mut Vec<String>,
) {
    use serde_json::Value;
    match (yours, shipped) {
        (Value::Object(yours), Value::Object(shipped)) => {
            let mut keys: Vec<&String> = yours.keys().chain(shipped.keys()).collect();
            keys.sort_unstable();
            keys.dedup();
            for key in keys {
                let under = format!("{at}/{key}");
                match (yours.get(key), shipped.get(key)) {
                    (Some(yours), Some(shipped)) => walk_apart(&under, yours, shipped, found),
                    _ => found.push(under),
                }
            }
        }
        // By position, not by id: a step inserted in the middle does move
        // every one after it, and matching by id would call that no change.
        (Value::Array(yours), Value::Array(shipped)) => {
            for index in 0..yours.len().max(shipped.len()) {
                let under = format!("{at}/{index}");
                match (yours.get(index), shipped.get(index)) {
                    (Some(yours), Some(shipped)) => walk_apart(&under, yours, shipped, found),
                    _ => found.push(under),
                }
            }
        }
        (yours, shipped) if yours != shipped => found.push(at.to_owned()),
        _ => {}
    }
}

#[cfg(test)]
mod tests;
