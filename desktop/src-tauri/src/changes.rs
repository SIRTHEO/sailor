//! What an agent changed in a workspace, and the way to the editor.
//!
//! The knowledge is `crates/workspace`, the same git the command line reads:
//! the diff shown is git's text, and nothing here computes a difference.

use std::process::{Command, Stdio};

use serde::Serialize;

/// The working tree of `root` against its last commit, as git says it.
#[tauri::command]
pub(crate) fn workspace_changes(root: String) -> Result<workspace::Changes, String> {
    workspace::changes(std::path::Path::new(&root))
}

/// Who gets the file, of the three that can.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum Chosen {
    /// `SAILOR_EDITOR`, named for Sailor.
    Declared,
    /// `VISUAL`, the editor a person set for a window.
    Visual,
    /// **NOBODY NAMED ONE.** The file goes to whatever this machine opens
    /// that kind of file with, and that association is not a promise to edit.
    System,
}

/// What will run on a file, and who chose it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub(crate) struct Opener {
    pub kind: Chosen,
    pub program: String,
    pub args: Vec<String>,
}

/// The command that opens a file, and which of the three named it.
///
/// `SAILOR_EDITOR` names it outright, `VISUAL` is the editor a person set for
/// a window, and with neither the system's own opener decides from the file —
/// `EDITOR` is not read, because it is almost always a terminal editor, which
/// has no terminal to open in from here.
pub(crate) fn editor_command(declared: Option<String>, visual: Option<String>) -> Opener {
    for (kind, candidate) in [(Chosen::Declared, declared), (Chosen::Visual, visual)] {
        let Some(candidate) = candidate else { continue };
        let mut words = candidate.split_whitespace().map(str::to_owned);
        if let Some(program) = words.next() {
            return Opener { kind, program, args: words.collect() };
        }
    }
    Opener {
        kind: Chosen::System,
        program: SYSTEM_OPENER.to_owned(),
        args: Vec::new(),
    }
}

#[cfg(target_os = "macos")]
const SYSTEM_OPENER: &str = "open";
#[cfg(not(target_os = "macos"))]
const SYSTEM_OPENER: &str = "xdg-open";

/// What this machine will open a file with, read from the environment.
fn opener_here() -> Opener {
    editor_command(
        std::env::var("SAILOR_EDITOR").ok().filter(|value| !value.trim().is_empty()),
        std::env::var("VISUAL").ok().filter(|value| !value.trim().is_empty()),
    )
}

/// **ASKED BEFORE ANYTHING IS OPENED**, so a button says what it will do
/// instead of promising an editor nobody declared.
#[tauri::command]
pub(crate) fn who_opens_files() -> Opener {
    opener_here()
}

/// Hands a file to the editor. Returns once the editor was started, not once
/// it was closed: an editor is where the person goes on working.
#[tauri::command]
pub(crate) fn open_in_editor(path: String) -> Result<(), String> {
    let opener = opener_here();
    let program = opener.program;
    Command::new(&program)
        .args(&opener.args)
        .arg(&path)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map(|_| ())
        .map_err(|error| format!("could not start the editor `{program}` on {path}: {error}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The declared editor wins, its arguments travel with it, and with
    /// nothing declared the system opener is what runs — never `EDITOR`.
    #[test]
    fn the_editor_is_the_declared_one_or_the_system_opener() {
        assert_eq!(
            editor_command(Some("code --wait".to_owned()), Some("vim".to_owned())),
            Opener {
                kind: Chosen::Declared,
                program: "code".to_owned(),
                args: vec!["--wait".to_owned()]
            }
        );
        assert_eq!(
            editor_command(None, Some("zed".to_owned())),
            Opener { kind: Chosen::Visual, program: "zed".to_owned(), args: Vec::new() }
        );
        assert_eq!(
            editor_command(None, None),
            Opener { kind: Chosen::System, program: SYSTEM_OPENER.to_owned(), args: Vec::new() }
        );
        assert_eq!(
            editor_command(Some("   ".to_owned()), None),
            Opener { kind: Chosen::System, program: SYSTEM_OPENER.to_owned(), args: Vec::new() },
            "a blank declaration is no declaration"
        );
    }

    /// **THE FALLBACK IS NOT AN EDITOR AND SAYS SO.** Whoever asks gets the
    /// three apart: an editor somebody named, and the association this
    /// machine happens to hold, which opens a file without being able to
    /// write it.
    #[test]
    fn the_system_opener_does_not_travel_as_a_declared_editor() {
        let system = editor_command(None, None);
        assert_eq!(system.kind, Chosen::System);
        assert_ne!(editor_command(Some("nano".to_owned()), None).kind, Chosen::System);
        assert_ne!(editor_command(None, Some("nano".to_owned())).kind, Chosen::System);
        assert_eq!(
            serde_json::to_value(&system).unwrap()["kind"],
            serde_json::json!("system"),
            "the page reads this word"
        );
    }
}
