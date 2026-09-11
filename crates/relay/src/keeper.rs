//! Asking whoever opened a terminal to read it or to type into it.
//!
//! **SAILOR HOLDS ALMOST NOTHING.** A session it did not open belongs to the
//! program that opened it, and the only way in is the door that program
//! declares. This file knows how to knock and nothing about who answers.

use flow::ActionError;
use serde_json::Value;
use std::path::Path;
use std::sync::OnceLock;
use std::time::Duration;
use toolbox::descriptor::{InTheList, KeepsTerminals};

/// How long a keeper has to answer before it counts as unreachable.
const LONG_ENOUGH: Duration = Duration::from_secs(10);

/// A terminal, whoever keeps it, and how they are asked about it.
pub struct Keeper {
    pub id: String,
    /// The name the session carries, which is not always the one its keeper's
    /// own commands want.
    pub named: String,
    keeps: KeepsTerminals,
    handle: OnceLock<String>,
}

/// Who keeps that terminal, or nothing where nobody has said.
pub fn of(catalog: &toolbox::Catalog, root: &Path, tty: &str) -> Option<Keeper> {
    let store = sessions::Sessions::open(root.join(sessions::SESSIONS_FILE)).ok()?;
    let kept = store.keeper_of(tty).ok().flatten()?;
    let known = catalog
        .live()
        .into_iter()
        .find(|loaded| loaded.descriptor.id == kept.keeper)?;
    let keeps = known.descriptor.keeps_terminals.clone()?;
    Some(Keeper {
        id: kept.keeper,
        named: kept.handle,
        keeps,
        handle: OnceLock::new(),
    })
}

impl Keeper {
    /// The last of what that terminal showed, as its keeper prints it.
    pub fn reads_the_screen(&self) -> Result<Vec<u8>, ActionError> {
        self.asked(&self.keeps.reads_the_screen, None)
    }

    /// One line typed into that terminal, by its keeper.
    pub fn types_a_line(&self, line: &str) -> Result<(), ActionError> {
        self.asked(&self.keeps.types_a_line, Some(line)).map(|_| ())
    }

    /// The name this terminal goes by to its keeper's own commands.
    ///
    /// **MEASURED, NOT GUESSED.** Where the keeper declares a list, the name a
    /// session carries is looked up in it; where it declares none, the two are
    /// the same name and nothing is run.
    fn handle(&self) -> Result<&str, ActionError> {
        if let Some(found) = self.handle.get() {
            return Ok(found);
        }
        let found = match (&self.keeps.lists_them, &self.keeps.in_the_list) {
            (listing, Some(how)) if !listing.is_empty() => {
                let printed = self.ran(&listing.clone(), None)?;
                self.found_in(&printed, how)?
            }
            _ => self.named.clone(),
        };
        let _ = self.handle.set(found);
        Ok(self.handle.get().map(String::as_str).unwrap_or_default())
    }

    /// The entry of the list this session is, and the handle it carries.
    fn found_in(&self, printed: &[u8], how: &InTheList) -> Result<String, ActionError> {
        let listed: Value = serde_json::from_slice(printed)
            .map_err(|error| self.broke(&format!("printed a list nobody can read: {error}")))?;
        let mut at = &listed;
        for key in &how.at {
            at = at
                .get(key)
                .ok_or_else(|| self.broke(&format!("printed a list with no «{key}» in it")))?;
        }
        let entries = at
            .as_array()
            .ok_or_else(|| self.broke("printed a list that is not a list"))?;
        for entry in entries {
            let named: Vec<String> = how
                .known_by
                .iter()
                .map(|field| {
                    entry
                        .get(field)
                        .and_then(Value::as_str)
                        .unwrap_or_default()
                        .to_owned()
                })
                .collect();
            if named.join(&how.joined_by) != self.named {
                continue;
            }
            return entry
                .get(&how.the_handle_is)
                .and_then(Value::as_str)
                .map(str::to_owned)
                .ok_or_else(|| {
                    self.broke(&format!(
                        "knows this terminal and gives it no «{}»",
                        how.the_handle_is
                    ))
                });
        }
        Err(self.broke(&format!(
            "does not know a terminal called «{}» any more",
            self.named
        )))
    }

    fn asked(&self, argv: &[String], line: Option<&str>) -> Result<Vec<u8>, ActionError> {
        let handle = self.handle()?.to_owned();
        self.ran(argv, Some((&handle, line.unwrap_or_default())))
    }

    fn ran(&self, argv: &[String], filling: Option<(&str, &str)>) -> Result<Vec<u8>, ActionError> {
        let (handle, line) = filling.unwrap_or_default();
        let mut filled = argv
            .iter()
            .map(|word| word.replace("{handle}", handle).replace("{line}", line));
        let Some(program) = filled.next() else {
            return Err(self.broke("declares a command with no name in it"));
        };
        let rest: Vec<String> = filled.collect();
        // The whole command, because «it exited with 1» sends whoever reads it
        // looking at the wrong one of the two this file runs.
        let ran = format!("`{program} {}`", rest.join(" "));
        let mut command = std::process::Command::new(&program);
        command.args(&rest).stdin(std::process::Stdio::null());
        match actions::run_with_timeout(command, LONG_ENOUGH) {
            actions::RunOutcome::Finished { status, stdout, .. } if status.success() => Ok(stdout),
            actions::RunOutcome::Finished { status, stderr, .. } => Err(self.broke(&format!(
                "{ran} exited with {}: {}",
                status
                    .code()
                    .map(|code| code.to_string())
                    .unwrap_or_else(|| "a signal".to_owned()),
                String::from_utf8_lossy(&stderr).trim()
            ))),
            actions::RunOutcome::TimedOut => {
                Err(self.broke(&format!("{ran} did not answer in time")))
            }
            actions::RunOutcome::SpawnFailed(why) => {
                Err(self.broke(&format!("{ran} did not start: {why}")))
            }
        }
    }

    fn broke(&self, why: &str) -> ActionError {
        ActionError::new(
            "keeper_did_not_answer",
            format!(
                "«{}» keeps this terminal and {why}. A keeper nobody can reach is not a terminal \
                 that is free",
                self.id
            ),
        )
    }
}
