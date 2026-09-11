//! Asking whoever opened a terminal to read it or to type into it.
//!
//! **SAILOR HOLDS ALMOST NOTHING.** A session it did not open belongs to the
//! program that opened it, and the only way in is the door that program
//! declares. This file knows how to knock and nothing about who answers.

use flow::ActionError;
use std::path::Path;
use std::time::Duration;
use toolbox::descriptor::KeepsTerminals;

/// How long a keeper has to answer before it counts as unreachable.
const LONG_ENOUGH: Duration = Duration::from_secs(10);

/// A terminal, whoever keeps it, and how they are asked about it.
pub struct Keeper {
    pub id: String,
    pub handle: String,
    keeps: KeepsTerminals,
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
        handle: kept.handle,
        keeps,
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

    fn asked(&self, argv: &[String], line: Option<&str>) -> Result<Vec<u8>, ActionError> {
        let mut filled = argv.iter().map(|word| {
            word.replace("{handle}", &self.handle)
                .replace("{line}", line.unwrap_or_default())
        });
        let Some(program) = filled.next() else {
            return Err(self.broke("declares a command with no name in it"));
        };
        let rest: Vec<String> = filled.collect();
        let mut command = std::process::Command::new(&program);
        command.args(&rest).stdin(std::process::Stdio::null());
        match actions::run_with_timeout(command, LONG_ENOUGH) {
            actions::RunOutcome::Finished { status, stdout, .. } if status.success() => Ok(stdout),
            actions::RunOutcome::Finished { status, stderr, .. } => Err(self.broke(&format!(
                "`{program}` exited with {}: {}",
                status
                    .code()
                    .map(|code| code.to_string())
                    .unwrap_or_else(|| "a signal".to_owned()),
                String::from_utf8_lossy(&stderr).trim()
            ))),
            actions::RunOutcome::TimedOut => {
                Err(self.broke(&format!("`{program}` did not answer in time")))
            }
            actions::RunOutcome::SpawnFailed(why) => {
                Err(self.broke(&format!("`{program}` did not start: {why}")))
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
