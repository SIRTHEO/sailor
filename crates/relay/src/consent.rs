//! What the emptying stands on, read at the moment it types.
//!
//! **THREE READINGS, TOGETHER OR NOT AT ALL.** A reading a run took when it
//! started says nothing about a terminal hours later, and a run can park that
//! long: the session in there may have been replaced since. So the emptying
//! mints its own consent, and a line that empties is typed only through it.

use crate::{freedom_now, freedom_of, reset_line_of, typed_into, Freedom};
use flow::ActionError;
use serde_json::{json, Value};
use std::path::Path;

/// Leave to empty one terminal now. Its fields are private and [`asked_now`]
/// is the only thing that makes one, so nothing types a reset without it.
pub(crate) struct Consent {
    tty: String,
    cli: String,
    line: String,
}

pub(crate) enum Asked {
    Given(Consent),
    NotYet(String),
}

/// The screen, the store and the session's own context, read now.
pub(crate) fn asked_now(
    catalog: &toolbox::Catalog,
    root: &Path,
    tty: &str,
    cli: &str,
) -> Result<Asked, ActionError> {
    let line = reset_line_of(catalog, cli)?;
    if let Freedom::NotYet(why) = freedom_now(catalog, root, tty, cli)? {
        return Ok(Asked::NotYet(why));
    }
    // Read after the screen, which may take seconds, so it is the last word.
    let words = freedom_of(catalog, cli)?.and_leaves_nothing_open_in_its_record;
    if let Err(why) = handed_on_by_whoever_is_there(root, tty, cli, words.as_ref()) {
        return Ok(Asked::NotYet(why));
    }
    Ok(Asked::Given(Consent {
        tty: tty.to_owned(),
        cli: cli.to_owned(),
        line,
    }))
}

impl Consent {
    pub(crate) fn empty(self, root: &Path) -> Result<Value, ActionError> {
        typed_into(root, &self.tty, &self.line)?;
        Ok(json!({
            "tty": self.tty,
            "cli": self.cli,
            "typed": self.line,
        }))
    }
}

/// The session in that terminal now left a mandate nobody has taken, its own
/// context stands at oblige, and its record leaves nothing open. Every reading
/// that cannot be taken is a no.
fn handed_on_by_whoever_is_there(
    root: &Path,
    tty: &str,
    cli: &str,
    words: Option<&toolbox::descriptor::OpenInRecord>,
) -> Result<(), String> {
    let register = root.join(sessions::SESSIONS_FILE);
    if !register.exists() {
        return Err(format!(
            "{tty}: there is no register of sessions, so nobody can say who is in there"
        ));
    }
    let row = sessions::Sessions::open(&register)
        .ok()
        .and_then(|store| store.terminal(tty).ok().flatten())
        .ok_or_else(|| format!("{tty}: the register does not know this terminal"))?;
    if !row.is_open() || row.is_detached() {
        return Err(format!(
            "{tty}: the register has this terminal closed or left alone"
        ));
    }
    let there = row
        .session_id
        .filter(|id| !id.is_empty())
        .ok_or_else(|| format!("{tty}: the register names no session in there"))?;
    let left = sessions::mandate::read(&sessions::mandate::address_in(root, tty))
        .ok_or_else(|| format!("{tty}: no mandate waits for this terminal"))?;
    if let Some(taken) = &left.taken {
        return Err(format!(
            "{tty}: the mandate here was taken by «{}», so nothing is handed on",
            taken.by
        ));
    }
    // **THE TTY IS REUSED.** A mandate its author did not stay to see through
    // is not the consent of whoever sits there now.
    if left.written.session != there {
        return Err(format!(
            "{tty}: the mandate was left by «{}» and «{there}» is in there now, so the \
             session that would be emptied never handed anything on",
            left.written.session
        ));
    }
    if left.written.engine != cli {
        return Err(format!(
            "{tty}: the mandate was left by «{}», not by «{cli}»",
            left.written.engine
        ));
    }
    let transcript = row
        .transcript_path
        .filter(|path| !path.is_empty())
        .ok_or_else(|| format!("{tty}: «{there}» names no transcript to measure"))?;
    match actions::session_fill::stands_at_oblige(&transcript) {
        Some(true) => {}
        Some(false) => {
            return Err(format!(
                "{tty}: «{there}» does not stand at oblige now, so it is not finished"
            ))
        }
        None => {
            return Err(format!(
                "{tty}: the context of «{there}» cannot be measured, and unmeasured is not full"
            ))
        }
    }
    let Some(words) = words else {
        return Ok(());
    };
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |since| since.as_secs() as i64);
    match crate::open_work::open_in(&transcript, words, now) {
        Some(open) if open.is_empty() => Ok(()),
        Some(open) => Err(format!(
            "{tty}: «{there}» still waits on {}, and emptying it now would lose it",
            open.iter()
                .map(crate::open_work::Open::said)
                .collect::<Vec<_>>()
                .join(", ")
        )),
        None => Err(format!(
            "{tty}: the record of «{there}» cannot be read, and unread is not finished"
        )),
    }
}
