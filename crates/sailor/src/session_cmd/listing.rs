//! The register as a person reads it: what each terminal is doing, how long
//! it has rested, and the rows nobody holds any more.

use super::*;

/// What a row's standing really is, once the machine has been asked.
/// **«OPEN» IS NOT «SOMEBODY IS THERE»**: a terminal killed without closing
/// keeps its row for ever, and whoever reads this list to decide anything
/// about a session must tell the two apart. Until now only the window could —
/// `Census::abandoned` was there and no command on the line invoked it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Standing {
    Open,
    NobodyThere,
    Closed,
    /// The machine would not say. **Unknown is not «somebody is there»**, and
    /// it is not «nobody» either.
    Unknown,
}

pub(crate) fn standing_of(
    row: &sessions::store::TerminalRow,
    abandoned: &sessions::census::Abandoned,
) -> Standing {
    if !row.is_open() {
        return Standing::Closed;
    }
    match abandoned {
        sessions::census::Abandoned::CouldNotLook { .. } => Standing::Unknown,
        sessions::census::Abandoned::Seen { ttys } => {
            if ttys.iter().any(|tty| tty == &row.tty) {
                Standing::NobodyThere
            } else {
                Standing::Open
            }
        }
    }
}

pub(super) fn word_for(standing: Standing) -> String {
    catalogue::say(
        match standing {
            Standing::Open => "cli.session.row_open",
            Standing::NobodyThere => "cli.session.row_nobody_there",
            Standing::Closed => "cli.session.row_closed",
            Standing::Unknown => "cli.session.row_standing_unknown",
        },
        &[],
    )
}

/// An open terminal whose last word was a `Stop` this long ago is at rest:
/// finished or merely waiting for a prompt, which the register cannot tell.
const AT_REST_FOR_LONG_SECS: i64 = 3600;

pub(super) fn since_words(secs: i64) -> String {
    let secs = secs.max(0);
    let (days, hours, minutes) = (secs / 86_400, (secs % 86_400) / 3600, (secs % 3600) / 60);
    match (days, hours) {
        (0, 0) => format!("{minutes}m"),
        (0, _) => format!("{hours}h {minutes:02}m"),
        _ => format!("{days}d {hours}h"),
    }
}

pub(super) fn list_terminals(request: &Request<'_>) -> Result<Report, String> {
    let store = request.store()?;
    let rows = store.terminals().map_err(|error| error.to_string())?;
    let abandoned = sessions::census::Census::of(&sessions::census::LocalMachine).abandoned(&rows);
    if request.options.contains_key("json") {
        let said: Vec<serde_json::Value> = rows
            .iter()
            .map(|row| {
                let mut value = serde_json::to_value(row).unwrap_or(serde_json::Value::Null);
                if let Some(fields) = value.as_object_mut() {
                    fields.insert(
                        "standing".to_owned(),
                        serde_json::Value::String(
                            match standing_of(row, &abandoned) {
                                Standing::Open => "open",
                                Standing::NobodyThere => "nobody_there",
                                Standing::Closed => "closed",
                                Standing::Unknown => "unknown",
                            }
                            .to_owned(),
                        ),
                    );
                }
                value
            })
            .collect();
        let text = serde_json::to_string_pretty(&said).map_err(|error| error.to_string())?;
        return Ok(Report::spoken(text));
    }
    if rows.is_empty() {
        return Ok(Report::spoken(catalogue::say(
            "cli.session.none_checked_in",
            &[],
        )));
    }
    let detached = catalogue::say("cli.session.row_detached", &[]);
    let attached = catalogue::say("cli.session.row_attached", &[]);
    let events = catalogue::say("cli.session.row_events", &[]);
    let mut text = String::new();
    let mut nobody = 0;
    let mut at_rest_for_long = 0;
    for row in &rows {
        let found = store.events_on(&row.tty).unwrap_or_default();
        let howmany = found.len();
        let standing = standing_of(row, &abandoned);
        if standing == Standing::NobodyThere {
            nobody += 1;
        }
        let last = found.iter().max_by_key(|event| event.occurred_at);
        let rest = match (standing, last) {
            (Standing::Closed | Standing::NobodyThere, _) | (_, None) => String::new(),
            (_, Some(event)) => {
                let secs = request.at - event.occurred_at;
                if event.name == "Stop" && secs >= AT_REST_FOR_LONG_SECS {
                    at_rest_for_long += 1;
                }
                catalogue::say(
                    if event.name == "Stop" {
                        "cli.session.row_at_rest"
                    } else {
                        "cli.session.row_last_event"
                    },
                    &[("for", &since_words(secs)), ("event", &event.name)],
                )
            }
        };
        let _ = writeln!(
            text,
            "{:<10} {:<14} {:<13} {:<11} {events}={:<4} {} {} {rest}",
            row.tty,
            row.ancestor.as_deref().unwrap_or("?"),
            word_for(standing),
            if row.is_detached() {
                &detached
            } else {
                &attached
            },
            howmany,
            row.session_id.as_deref().unwrap_or("-"),
            row.worktree,
        );
    }
    if at_rest_for_long > 0 {
        text.push('\n');
        text.push_str(&catalogue::say(
            "cli.session.at_rest_for_long",
            &[("count", &at_rest_for_long.to_string())],
        ));
        text.push('\n');
    }
    // A word in a column teaches nothing on its own: what the reading means is
    // said once, below, where somebody about to act on it will read it.
    match &abandoned {
        sessions::census::Abandoned::CouldNotLook { refusal } => {
            text.push('\n');
            text.push_str(&catalogue::say(
                "cli.session.could_not_look_at_terminals",
                &[("why", &refusal.to_string())],
            ));
        }
        sessions::census::Abandoned::Seen { .. } if nobody > 0 => {
            text.push('\n');
            text.push_str(&catalogue::say(
                "cli.session.rows_hold_nobody",
                &[("count", &nobody.to_string())],
            ));
        }
        sessions::census::Abandoned::Seen { .. } => {}
    }
    Ok(Report::spoken(text.trim_end().to_owned()))
}

/// The rows still open whose terminal the census no longer sees: closed here,
/// with an event that names who noticed, because the census is taken when an
/// event arrives and at no other moment. Only the register is written; the
/// deposit's announcement belongs to whoever held the terminal.
pub(super) fn close_the_gone(request: &Request<'_>, store: &Sessions) -> Result<Vec<String>, String> {
    let rows = store.terminals().map_err(|error| error.to_string())?;
    let sessions::census::Abandoned::Seen { ttys } = request.census.abandoned(&rows) else {
        return Ok(Vec::new());
    };
    let mut closed = Vec::new();
    for row in rows.iter().filter(|row| ttys.contains(&row.tty)) {
        if !store
            .close_terminal(&row.tty, request.at)
            .map_err(|error| error.to_string())?
        {
            continue;
        }
        store
            .record_event(&TerminalEvent {
                tty: row.tty.clone(),
                session_id: row.session_id.clone(),
                worktree: Some(row.worktree.clone()),
                ancestor: row.ancestor.clone(),
                name: "gone".to_owned(),
                transcript_path: row.transcript_path.clone(),
                occurred_at: request.at,
                payload: Some(format!("{{\"noticed_by\":\"{}\"}}", request.tty)),
            })
            .map_err(|error| error.to_string())?;
        closed.push(row.tty.clone());
    }
    Ok(closed)
}

/// The sentence, plus whatever the announcement could not do: said, not
/// returned as an error, because the row is already in the other store and
/// failing the hook would stop the person working. One function and three
/// callers, so the same fact is not told three times.
pub(super) fn also_saying(said: String, announced: Result<(), String>) -> String {
    match announced {
        Ok(()) => said,
        Err(why) => format!(
            "{said}\n{}",
            catalogue::say("cli.session.not_announced", &[("why", &why)])
        ),
    }
}
