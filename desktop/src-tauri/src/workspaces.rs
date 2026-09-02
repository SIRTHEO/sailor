//! The projects Sailor has been opened in, as the window asks for them.
//!
//! **THE REGISTER LIVES IN `flow::workspace`, NOT HERE.** This reads it and
//! reshapes it for the canvas; what a project is, and when it counts as gone,
//! belongs to the engine — the command line answers with the same code, and
//! two answers that drift apart beat no answer nobody likes.

use serde::Serialize;

/// A project as the canvas receives it. The field names are the contract
/// written in `desktop/src/workspaces.ts`: whoever changes one changes both.
#[derive(Serialize)]
pub(crate) struct Project {
    root: String,
    name: String,
    first_seen: i64,
    last_seen: i64,
    /// `declared` when the marker is still where it was left, `gone` when it is
    /// not. Read at the moment of asking: a stored answer would be a second
    /// truth to keep aligned with the disk.
    standing: &'static str,
    /// Whether this is the project the window is standing in. The list is
    /// worth little without it — «switch project» needs to know what not to
    /// switch to.
    current: bool,
}

/// The projects, most recently opened first.
///
/// **AN EMPTY HOUSE IS AN EMPTY LIST, NOT A FAILURE.** Nobody has opened a
/// project yet is a complete answer, and the canvas has a card for it; an error
/// there would make a first run look like a broken install.
#[tauri::command]
pub(crate) fn workspaces() -> Result<Vec<Project>, String> {
    let home =
        ledger::sailor_home().ok_or_else(|| "no house to read: HOME is not set".to_owned())?;
    let here = std::env::current_dir()
        .ok()
        .and_then(|from| flow::workspace::find_root(&from));

    Ok(flow::workspace::known_in(&home)?
        .into_iter()
        .map(|known| Project {
            standing: match flow::workspace::standing_of(&known) {
                flow::workspace::Standing::Declared => "declared",
                flow::workspace::Standing::Gone => "gone",
            },
            current: here.as_deref() == Some(known.root.as_path()),
            root: known.root.to_string_lossy().into_owned(),
            name: known.name,
            first_seen: known.first_seen,
            last_seen: known.last_seen,
        })
        .collect())
}

/// What a project declares about itself, read from its own marker.
///
/// Separate from the list on purpose: the list is read on every paint, and a
/// declaration is read when someone looks at one project. Reading every marker
/// to draw a list would touch the disk once per row for something nobody asked.
#[tauri::command]
pub(crate) fn workspace_declaration(root: String) -> Result<serde_json::Value, String> {
    let declared = flow::workspace::declaration_at(std::path::Path::new(&root))?;
    serde_json::to_value(&declared).map_err(|error| error.to_string())
}
