//! What a session still waits on by its own record: work that no process shows
//! and no screen paints. An agent launched in the background runs inside the
//! session, and emptying the session loses whatever it would have reported.

use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};
use std::io::BufRead;
use toolbox::descriptor::OpenInRecord;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Open {
    /// The last call made has no answer yet: a question to a person, or a
    /// permission nobody gave.
    Unanswered { call: String },
    /// Launched in the background, not reported on, not stopped, and younger
    /// than the declared age at which a silent launch is taken as lost.
    Launched { what: String },
    /// Messages queued for the session and not yet handed to it.
    Queued { count: usize },
}

impl Open {
    pub(crate) fn said(&self) -> String {
        match self {
            Open::Unanswered { call } => format!("a call to {call} that nobody answered"),
            Open::Launched { what } => format!("«{what}», launched and never reported on"),
            Open::Queued { count } => format!("{count} queued message(s) not yet delivered"),
        }
    }
}

struct Call {
    name: String,
    what: String,
    at: Option<i64>,
    row: usize,
}

/// Everything the record leaves open at `now`. `None` when it cannot be read,
/// which is never «nothing open».
pub(crate) fn open_in(transcript: &str, words: &OpenInRecord, now: i64) -> Option<Vec<Open>> {
    let file = std::fs::File::open(transcript).ok()?;
    read(std::io::BufReader::new(file), words, now)
}

pub(crate) fn read(record: impl BufRead, words: &OpenInRecord, now: i64) -> Option<Vec<Open>> {
    let mut calls: BTreeMap<String, Call> = BTreeMap::new();
    let mut answers: BTreeMap<String, String> = BTreeMap::new();
    let (mut reported, mut stopped) = (BTreeSet::new(), BTreeSet::new());
    let (mut queued, mut turn_began) = (0usize, 0usize);
    for (row, line) in record.lines().enumerate() {
        let line = line.ok()?;
        let Ok(entry) = serde_json::from_str::<Value>(&line) else {
            continue;
        };
        if entry["isSidechain"] == true {
            continue;
        }
        if entry["type"] == words.a_queue_row_is.as_str() {
            let operation = entry["operation"].as_str().unwrap_or_default();
            if words
                .the_queue_grows_on
                .iter()
                .any(|word| word == operation)
            {
                queued += 1;
            } else if words.and_shrinks_on.iter().any(|word| word == operation) {
                queued = queued.saturating_sub(1);
            } else if words.and_empties_on.iter().any(|word| word == operation) {
                queued = 0;
            }
        }
        let content = &entry["message"]["content"];
        let said = [content, &entry["content"], &entry["attachment"]["prompt"]];
        if let Some(text) = said.iter().find_map(|text| text.as_str()) {
            if text.contains(&words.a_report_carries) {
                reported.extend(between(text, &words.a_report_names_the_call_between));
            }
            if entry["type"] == "user" {
                turn_began = row;
            }
            continue;
        }
        let blocks = content.as_array().map(Vec::as_slice).unwrap_or_default();
        if entry["type"] == "user" && !blocks.iter().any(|block| block["type"] == "tool_result") {
            turn_began = row;
        }
        for block in blocks {
            match block["type"].as_str() {
                Some("tool_use") => {
                    let name = block["name"].as_str().unwrap_or_default().to_owned();
                    if name == words.a_stop_is_the_call {
                        if let Some(task) = block["input"][&words.a_stop_names_the_task_in].as_str()
                        {
                            stopped.insert(task.to_owned());
                        }
                    }
                    let what = block["input"]["description"]
                        .as_str()
                        .unwrap_or(&name)
                        .to_owned();
                    let at = entry["timestamp"]
                        .as_str()
                        .and_then(models::fuel::unix_secs_of_rfc3339);
                    let id = block["id"].as_str().unwrap_or_default().to_owned();
                    calls.insert(
                        id,
                        Call {
                            name,
                            what,
                            at,
                            row,
                        },
                    );
                }
                Some("tool_result") => {
                    let id = block["tool_use_id"].as_str().unwrap_or_default().to_owned();
                    answers.insert(id, block["content"].to_string());
                }
                _ => {}
            }
        }
    }
    let mut open = Vec::new();
    for (id, call) in &calls {
        let Some(answer) = answers.get(id) else {
            // Only the last turn's calls: an earlier one with no answer was
            // interrupted, and the session went on past it.
            if call.row > turn_began {
                open.push(Open::Unanswered {
                    call: call.name.clone(),
                });
            }
            continue;
        };
        if !answer.contains(&words.a_background_launch_answers) || reported.contains(id) {
            continue;
        }
        let task = task_named_in(answer, &words.the_task_is_named_after);
        if task.is_some_and(|task| stopped.contains(task)) {
            continue;
        }
        let lost = call
            .at
            .is_some_and(|at| now - at > words.a_launch_is_lost_after_seconds as i64);
        if !lost {
            open.push(Open::Launched {
                what: call.what.clone(),
            });
        }
    }
    if queued > 0 {
        open.push(Open::Queued { count: queued });
    }
    Some(open)
}

fn between<'a>(text: &'a str, tags: &'a [String; 2]) -> impl Iterator<Item = String> + 'a {
    text.split(tags[0].as_str()).skip(1).filter_map(|rest| {
        rest.split_once(tags[1].as_str())
            .map(|(id, _)| id.to_owned())
    })
}

fn task_named_in<'a>(answer: &'a str, after: &str) -> Option<&'a str> {
    let rest = answer.split_once(after)?.1;
    let end = rest
        .find(|c: char| !c.is_ascii_alphanumeric())
        .unwrap_or(rest.len());
    Some(&rest[..end]).filter(|task| !task.is_empty())
}

#[cfg(test)]
mod tests;
