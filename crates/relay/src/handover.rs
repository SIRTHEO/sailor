//! Emptying a session that handed its work on, as the one move of a transaction.
//!
//! **SURFACE: writing into a live session. POWERS CLAIMED: typing the line a
//! descriptor declares, once.** The clear is sent only when the session
//! declared its mandate finished and every gate agrees on the same activity:
//! no instruction since, no sub-agent running, no call left running or
//! scheduled in its record, a free screen. Unknown is a no.

use crate::{freedom_now, reset_line_of, store_root, typed_into, unknown_of, Freedom};
use flow::{Action, ActionError, ActionOutcome, SharedState, StepSpecies};
use serde::Deserialize;
use serde_json::{json, Value};
use sessions::handover::State;
use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;
use std::time::{Duration, Instant};
use toolbox::descriptor::{OutlivesTheTurn, SUBAGENT_STARTED, SUBAGENT_STOPPED};

pub const HAND_OVER_ACTION: &str = "hand_over";
pub const RESUME_SUCCESSOR_ACTION: &str = "resume_successor";

/// How long a screen is waited for: a turn just ended is still being painted.
const PATIENCE_SECONDS: u64 = 30;

#[derive(Debug, Deserialize)]
struct HandOverSpec {
    tty: String,
    session: String,
    #[serde(default)]
    transcript: Option<String>,
    #[serde(default)]
    store: Option<String>,
    /// `false` records the decision and types nothing.
    #[serde(default = "sent")]
    dispatch: bool,
    #[serde(default = "patience")]
    patience_seconds: u64,
}

fn patience() -> u64 {
    PATIENCE_SECONDS
}

fn sent() -> bool {
    true
}

pub struct HandOverAction;

impl Action for HandOverAction {
    fn execute(&self, input: &Value, _shared: &SharedState) -> Result<ActionOutcome, ActionError> {
        let spec: HandOverSpec = serde_json::from_value(input.clone())
            .map_err(|error| ActionError::new("invalid_input", error.to_string()))?;
        let root = store_root(&spec.store)?;
        let machine = toolbox::Machine::current();
        let catalog = toolbox::Catalog::load(&toolbox::default_sources(&machine));
        hand_over(
            &catalog,
            &root,
            &spec.tty,
            &spec.session,
            spec.transcript.as_deref(),
            spec.dispatch,
            Duration::from_secs(spec.patience_seconds),
        )
        .map(ActionOutcome::Went)
    }

    fn unknown_fields(&self, declared: &Value) -> Vec<String> {
        unknown_of(
            declared,
            &["tty", "session", "transcript", "store", "dispatch", "patience_seconds"],
        )
    }

    fn species(&self) -> StepSpecies {
        StepSpecies::Repeatable
    }
}

fn held(why: String) -> Value {
    json!({"handed_over": false, "why": why})
}

/// The whole decision, with the catalog and the store named.
pub fn hand_over(
    catalog: &toolbox::Catalog,
    root: &Path,
    tty: &str,
    session: &str,
    transcript: Option<&str>,
    dispatch: bool,
    patience: Duration,
) -> Result<Value, ActionError> {
    let fault = |error: sessions::SessionError| ActionError::new("sessions", error.to_string());
    let path = root.join(sessions::SESSIONS_FILE);
    if !path.exists() {
        return Ok(held("no session has announced itself in this store".to_owned()));
    }
    let store = sessions::Sessions::open(&path).map_err(fault)?;
    let Some(handover) = store.open_for(tty, session).map_err(fault)? else {
        return Ok(held(format!("{tty}: this session declared no finished handover")));
    };
    let now = sessions::now();
    let read = store.activity_of(session).map_err(fault)?;
    let cancel = |why: String| -> Result<Value, ActionError> {
        store
            .advance(&handover.id, State::Finished, State::Cancelled, &why, now)
            .map_err(fault)?;
        Ok(held(why))
    };
    let wait = |why: String| -> Result<Value, ActionError> {
        store.explain(&handover.id, &why, now).map_err(fault)?;
        Ok(held(why))
    };
    let line = catalog
        .live()
        .into_iter()
        .find(|loaded| loaded.descriptor.id == handover.engine)
        .map(|loaded| loaded.descriptor.clone())
        .ok_or_else(|| {
            ActionError::new(
                "unknown_command_line",
                format!("«{}»: no descriptor of that name is loaded", handover.engine),
            )
        })?;
    let reset = reset_line_of(catalog, &handover.engine)?;

    // A new instruction after the mandate is new work the mandate does not hold.
    let since = store.events_after(session, handover.generation).map_err(fault)?;
    if let Some(asked) = line.event_for("asked") {
        if since.iter().any(|name| name == asked) {
            return cancel(format!("«{asked}» arrived after the mandate: new work, not handed on"));
        }
    }
    let written = sessions::mandate::read(&sessions::mandate::address_in(root, tty));
    match &written {
        Some(mandate)
            if mandate.taken.is_none()
                && mandate.written.session == session
                && mandate.written.at == handover.mandate_at => {}
        _ => return cancel("the mandate this handover was declared on is no longer waiting".to_owned()),
    }

    let (Some(started), Some(stopped)) =
        (line.event_for(SUBAGENT_STARTED), line.event_for(SUBAGENT_STOPPED))
    else {
        return wait(format!("«{}» declares no sub-agent events: nobody can say none run", line.id));
    };
    let all = store.events_after(session, 0).map_err(fault)?;
    let running = all.iter().filter(|name| *name == started).count() as i64
        - all.iter().filter(|name| *name == stopped).count() as i64;
    if running > 0 {
        return wait(format!("{running} sub-agent(s) of this session are still running"));
    }

    let Some(declared) = &line.outlives_the_turn else {
        return wait(format!("«{}» does not declare how its record shows work left running", line.id));
    };
    let Some(record) = transcript.and_then(|path| actions::session_fill::tail_of(path, RECORD_READ)) else {
        return wait("the session's record cannot be read".to_owned());
    };
    let left = work_left_running(&record, declared);
    if !left.is_empty() {
        return wait(left.join("; "));
    }

    if let Some(why) = not_free_within(catalog, root, tty, &handover.engine, patience)? {
        return wait(why);
    }
    if !dispatch {
        return Ok(json!({"handed_over": false, "would_clear": true, "handover": handover.id}));
    }
    // The licence is taken only on the activity every gate read.
    if !store.clear_if_still(&handover.id, read, sessions::now()).map_err(fault)? {
        return Ok(held("the session moved while it was being checked".to_owned()));
    }
    match typed_into(root, tty, &reset) {
        Ok(()) => {
            store
                .advance(&handover.id, State::Clearing, State::AwaitingSuccessor, "sent", sessions::now())
                .map_err(fault)?;
            Ok(json!({"handed_over": true, "handover": handover.id, "typed": reset}))
        }
        Err(error) => {
            store
                .advance(
                    &handover.id,
                    State::Clearing,
                    State::RecoveryRequired,
                    &error.to_string(),
                    sessions::now(),
                )
                .map_err(fault)?;
            Err(error)
        }
    }
}

/// Why the screen is still not free once `patience` has run out, or nothing
/// when it came free. Asked again every second: a turn just ended repaints.
fn not_free_within(
    catalog: &toolbox::Catalog,
    root: &Path,
    tty: &str,
    engine: &str,
    patience: Duration,
) -> Result<Option<String>, ActionError> {
    let began = Instant::now();
    loop {
        match freedom_now(catalog, root, tty, engine)? {
            Freedom::Free { .. } => return Ok(None),
            Freedom::NotYet(why) if began.elapsed() >= patience => return Ok(Some(why)),
            Freedom::NotYet(_) => std::thread::sleep(Duration::from_secs(1)),
        }
    }
}

#[derive(Debug, Deserialize)]
struct ResumeSpec {
    tty: String,
    session: String,
    /// What sets the successor going, typed as a prompt.
    line: String,
    #[serde(default)]
    store: Option<String>,
    #[serde(default = "sent")]
    dispatch: bool,
    #[serde(default = "patience")]
    patience_seconds: u64,
}

pub struct ResumeSuccessorAction;

impl Action for ResumeSuccessorAction {
    fn execute(&self, input: &Value, _shared: &SharedState) -> Result<ActionOutcome, ActionError> {
        let spec: ResumeSpec = serde_json::from_value(input.clone())
            .map_err(|error| ActionError::new("invalid_input", error.to_string()))?;
        let root = store_root(&spec.store)?;
        let machine = toolbox::Machine::current();
        let catalog = toolbox::Catalog::load(&toolbox::default_sources(&machine));
        resume_successor(
            &catalog,
            &root,
            &spec.tty,
            &spec.session,
            &spec.line,
            spec.dispatch,
            Duration::from_secs(spec.patience_seconds),
        )
        .map(ActionOutcome::Went)
    }

    fn unknown_fields(&self, declared: &Value) -> Vec<String> {
        unknown_of(
            declared,
            &["tty", "session", "line", "store", "dispatch", "patience_seconds"],
        )
    }

    fn species(&self) -> StepSpecies {
        StepSpecies::Repeatable
    }
}

/// Sets going the successor that reserved a handover: after a clear the
/// command line waits for a prompt, and a mandate nobody is asked to read is
/// a mandate that sits there. Typed once, on a free screen.
pub fn resume_successor(
    catalog: &toolbox::Catalog,
    root: &Path,
    tty: &str,
    session: &str,
    line: &str,
    dispatch: bool,
    patience: Duration,
) -> Result<Value, ActionError> {
    let fault = |error: sessions::SessionError| ActionError::new("sessions", error.to_string());
    let not_prompted = |why: String| json!({"prompted": false, "why": why});
    let path = root.join(sessions::SESSIONS_FILE);
    if !path.exists() {
        return Ok(not_prompted("no session has announced itself in this store".to_owned()));
    }
    let store = sessions::Sessions::open(&path).map_err(fault)?;
    let Some(handover) = store
        .successor_in(session, State::Verifying)
        .map_err(fault)?
        .filter(|found| found.tty == tty)
    else {
        return Ok(not_prompted(format!("{tty}: this session reserved no handover")));
    };
    let descriptor = catalog
        .live()
        .into_iter()
        .find(|loaded| loaded.descriptor.id == handover.engine)
        .map(|loaded| loaded.descriptor.clone());
    let spoke = |store: &sessions::Sessions| -> Result<bool, ActionError> {
        let asked = descriptor.as_ref().and_then(|found| found.event_for("asked"));
        let events = store.events_after(session, 0).map_err(fault)?;
        Ok(asked.is_some_and(|asked| events.iter().any(|name| name == asked)))
    };
    if spoke(&store)? {
        store
            .advance(&handover.id, State::Verifying, State::Resumed, "a person set it going", sessions::now())
            .map_err(fault)?;
        return Ok(not_prompted("a person already spoke to the successor".to_owned()));
    }
    if let Some(why) = not_free_within(catalog, root, tty, &handover.engine, patience)? {
        // Nobody else will come back to it: the successor waits for a prompt.
        store
            .advance(&handover.id, State::Verifying, State::RecoveryRequired, &why, sessions::now())
            .map_err(fault)?;
        return Ok(not_prompted(why));
    }
    if !dispatch {
        return Ok(json!({"prompted": false, "would_prompt": true, "handover": handover.id}));
    }
    if spoke(&store)?
        || !store
            .advance(&handover.id, State::Verifying, State::Prompted, "set going", sessions::now())
            .map_err(fault)?
    {
        return Ok(not_prompted("the successor moved while it was being checked".to_owned()));
    }
    match typed_into(root, tty, line) {
        Ok(()) => Ok(json!({"prompted": true, "handover": handover.id})),
        Err(error) => {
            store
                .advance(&handover.id, State::Prompted, State::RecoveryRequired, &error.to_string(), sessions::now())
                .map_err(fault)?;
            Err(error)
        }
    }
}

/// How much of a record is read from its end, as the fill reading does.
const RECORD_READ: u64 = 32 * 1024 * 1024;

/// The calls a record shows still running past the turn: unanswered, sent to
/// the background and not ended, or scheduling work for later. The shape of a
/// call is the one the fill reading already reads in the same record.
pub fn work_left_running(record: &str, declared: &OutlivesTheTurn) -> Vec<String> {
    let mut calls: BTreeMap<String, (String, bool)> = BTreeMap::new();
    let mut answered = BTreeSet::new();
    let mut left = Vec::new();
    for row in record.lines().filter_map(|line| serde_json::from_str::<Value>(line).ok()) {
        let Some(blocks) = row["message"]["content"].as_array() else {
            continue;
        };
        for block in blocks {
            match block["type"].as_str() {
                Some("tool_use") => {
                    let name = block["name"].as_str().unwrap_or_default().to_owned();
                    let id = block["id"].as_str().unwrap_or_default().to_owned();
                    if declared.schedules.contains(&name) {
                        left.push(format!("«{name}» scheduled work after the turn"));
                    }
                    let background = declared
                        .background_when
                        .iter()
                        .any(|field| block["input"][field.as_str()] == json!(true));
                    calls.insert(id, (name, background));
                }
                Some("tool_result") => {
                    if let Some(id) = block["tool_use_id"].as_str() {
                        answered.insert(id.to_owned());
                    }
                }
                _ => {}
            }
        }
    }
    for (id, (name, background)) in &calls {
        if !answered.contains(id) {
            left.push(format!("«{name}» has no answer yet"));
        } else if *background && !record.contains(&declared.ended_mark.replace("{id}", id)) {
            left.push(format!("«{name}» is still running in the background"));
        }
    }
    left
}
