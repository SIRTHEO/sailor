//! What the emptying stands on, read at the moment it types.
//!
//! **THREE READINGS, TOGETHER OR NOT AT ALL.** A reading a run took when it
//! started says nothing about a terminal hours later, and a run can park that
//! long: the session in there may have been replaced since. So the emptying
//! mints its own consent, and a line that empties is typed only through it.

use crate::{freedom_now, reset_line_of, typed_into, Freedom};
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
    if let Err(why) = handed_on_by_whoever_is_there(root, tty, cli) {
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

/// The session in that terminal now left a mandate nobody has taken, and its
/// own context stands at oblige. Every reading that cannot be taken is a no.
fn handed_on_by_whoever_is_there(root: &Path, tty: &str, cli: &str) -> Result<(), String> {
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
        Some(true) => Ok(()),
        Some(false) => Err(format!(
            "{tty}: «{there}» does not stand at oblige now, so it is not finished"
        )),
        None => Err(format!(
            "{tty}: the context of «{there}» cannot be measured, and unmeasured is not full"
        )),
    }
}
