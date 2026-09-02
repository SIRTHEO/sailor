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

/// The root a path stands in, or the refusal: a place with no marker at or
/// above it is not a project, and moving there would leave the window working
/// wherever it landed, which is fault 19 with a button on it.
pub(crate) fn root_to_work_in(asked: &std::path::Path) -> Result<std::path::PathBuf, String> {
    if !asked.is_dir() {
        return Err(format!("{} is not a directory", asked.display()));
    }
    flow::workspace::find_root(asked).ok_or_else(|| {
        format!(
            "no {} at or above {}: not a project",
            flow::workspace::MARKER,
            asked.display()
        )
    })
}

/// Moves the window into a project. From here on the flows, the runs and the
/// census resolve against this root, because every one of them reads the
/// working directory; the register notes the visit before the move, so a
/// register that cannot be written refuses the whole gesture.
#[tauri::command]
pub(crate) fn work_here(root: String) -> Result<String, String> {
    let found = root_to_work_in(std::path::Path::new(&root))?;
    let home = ledger::sailor_home().ok_or_else(|| "no home: HOME is not set".to_owned())?;
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|since| since.as_secs() as i64)
        .unwrap_or(0);
    flow::workspace::remember_in(&home, &found, now)?;
    std::env::set_current_dir(&found)
        .map_err(|error| format!("cannot move into {}: {error}", found.display()))?;
    Ok(found.display().to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// **A PLACE WITHOUT A MARKER IS REFUSED BY NAME, AND A PLACE INSIDE A
    /// PROJECT RESOLVES TO ITS ROOT.** Only the resolution is tested: moving
    /// the process is one line, and a test that moved it would move every
    /// other test in this crate along with it.
    #[test]
    fn a_project_is_found_from_inside_it_and_refused_where_there_is_none() {
        let scratch = std::env::temp_dir().join(format!("sailor-work-here-{}", std::process::id()));
        let project = scratch.join("project");
        let inside = project.join("src").join("deep");
        let bare = scratch.join("bare");
        std::fs::create_dir_all(&inside).expect("the scratch tree is made");
        std::fs::create_dir_all(&bare).expect("the bare directory is made");
        std::fs::write(project.join(flow::workspace::MARKER), "{}").expect("the marker is written");

        assert_eq!(root_to_work_in(&inside).expect("inside a project"), project);
        assert_eq!(root_to_work_in(&project).expect("at the root"), project);

        let refused = root_to_work_in(&bare).expect_err("no marker anywhere above");
        assert!(refused.contains("not a project"), "{refused}");
        let missing = root_to_work_in(&scratch.join("nowhere")).expect_err("no such directory");
        assert!(missing.contains("not a directory"), "{missing}");

        let _ = std::fs::remove_dir_all(&scratch);
    }
}
