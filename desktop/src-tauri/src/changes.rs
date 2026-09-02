//! What an agent changed in a workspace, and the way to the editor.
//!
//! The knowledge is `crates/workspace`, the same git the command line reads:
//! the diff shown is git's text, and nothing here computes a difference.

use std::process::{Command, Stdio};

/// The working tree of `root` against its last commit, as git says it.
#[tauri::command]
pub(crate) fn workspace_changes(root: String) -> Result<workspace::Changes, String> {
    workspace::changes(std::path::Path::new(&root))
}

/// The command that opens a file in the editor the person already uses.
///
/// `SAILOR_EDITOR` names it outright, `VISUAL` is the editor a person set for
/// a window, and with neither the system's own opener decides from the file —
/// `EDITOR` is not read, because it is almost always a terminal editor, which
/// has no terminal to open in from here.
pub(crate) fn editor_command(
    declared: Option<String>,
    visual: Option<String>,
) -> (String, Vec<String>) {
    for candidate in [declared, visual].into_iter().flatten() {
        let mut words = candidate.split_whitespace().map(str::to_owned);
        if let Some(program) = words.next() {
            return (program, words.collect());
        }
    }
    (SYSTEM_OPENER.to_owned(), Vec::new())
}

#[cfg(target_os = "macos")]
const SYSTEM_OPENER: &str = "open";
#[cfg(not(target_os = "macos"))]
const SYSTEM_OPENER: &str = "xdg-open";

/// Hands a file to the editor. Returns once the editor was started, not once
/// it was closed: an editor is where the person goes on working.
#[tauri::command]
pub(crate) fn open_in_editor(path: String) -> Result<(), String> {
    let (program, args) = editor_command(
        std::env::var("SAILOR_EDITOR").ok().filter(|value| !value.trim().is_empty()),
        std::env::var("VISUAL").ok().filter(|value| !value.trim().is_empty()),
    );
    Command::new(&program)
        .args(&args)
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
            ("code".to_owned(), vec!["--wait".to_owned()])
        );
        assert_eq!(
            editor_command(None, Some("zed".to_owned())),
            ("zed".to_owned(), Vec::new())
        );
        assert_eq!(editor_command(None, None), (SYSTEM_OPENER.to_owned(), Vec::new()));
        assert_eq!(
            editor_command(Some("   ".to_owned()), None),
            (SYSTEM_OPENER.to_owned(), Vec::new()),
            "a blank declaration is no declaration"
        );
    }
}
