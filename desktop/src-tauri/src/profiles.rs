//! The profiles of the command lines Sailor knows, as the window asks for them.
//!
//! **THE ANSWER IS `sailor::profiles_cmd`'S, NOT THIS MODULE'S.** Whether a
//! profile can be used is decided by asking the real command line inside *that*
//! profile's home, and a second implementation here would be free to disagree
//! with `sailor profiles list` on the one machine where it matters.

use serde::Serialize;

/// A command line the table knows: how it is invoked, whether it does profiles
/// on its own, and how its home moves.
///
/// **THE NOTES TRAVEL.** `native_profiles_note` and `home_note` say *how we
/// know*, and dropping them here would leave the window asserting things with
/// no evidence — which is the opposite of the product.
#[derive(Serialize)]
pub(crate) struct CommandLine {
    id: String,
    display_name: String,
    executable: String,
    native_profiles: &'static str,
    native_profiles_note: String,
    /// `variable` when a variable moves the whole home, `symlink` when only a
    /// credentials file is swapped, `none` when there is no known way.
    home_mechanism: &'static str,
    /// The variable's name, or the swapped path. Empty when there is neither.
    home_detail: String,
    home_note: String,
}

/// One profile as the window draws it. `access` is the verdict to act on and
/// `said` the engine's own words: the first is never derived from the second.
#[derive(Serialize)]
pub(crate) struct Row {
    cli_id: String,
    name: String,
    home_dir: String,
    active: bool,
    access: &'static str,
    said: String,
}

fn native(state: &::profiles::NativeProfiles) -> &'static str {
    match state {
        ::profiles::NativeProfiles::Supported => "supported",
        ::profiles::NativeProfiles::NotSupported => "not supported",
        // NOT "no": nobody checked, and the note says why.
        ::profiles::NativeProfiles::Unverified => "unverified",
    }
}

fn access(state: sailor::profiles_cmd::Access) -> &'static str {
    match state {
        sailor::profiles_cmd::Access::Yes => "yes",
        sailor::profiles_cmd::Access::No => "no",
        sailor::profiles_cmd::Access::NotKnown => "not known",
        sailor::profiles_cmd::Access::HomeDoesNotMove => "home does not move",
    }
}

#[tauri::command]
pub(crate) fn profile_command_lines() -> Vec<CommandLine> {
    ::profiles::known_clis()
        .iter()
        .map(|cli| {
            let (home_mechanism, home_detail) = match &cli.home {
                ::profiles::HomeMechanism::EnvVar(name) => ("variable", name.clone()),
                ::profiles::HomeMechanism::CredentialSymlink { relative_path } => {
                    ("symlink", relative_path.clone())
                }
                ::profiles::HomeMechanism::Unknown => ("none", String::new()),
            };
            CommandLine {
                id: cli.id.to_owned(),
                display_name: cli.display_name.to_owned(),
                executable: cli.executable.to_owned(),
                native_profiles: native(&cli.native_profiles),
                native_profiles_note: cli.native_profiles_note.to_owned(),
                home_mechanism,
                home_detail,
                home_note: cli.home_note.to_owned(),
            }
        })
        .collect()
}

/// **THIS READ RUNS COMMANDS**, one per profile: `codex login status` and its
/// like, which read a local file and call no model. It is the only way to
/// answer about a profile that is not in force, and it is why the window asks
/// for it when the screen opens rather than on every redraw.
#[tauri::command]
pub(crate) fn profiles() -> Result<Vec<Row>, String> {
    Ok(sailor::profiles_cmd::overview(None)?
        .into_iter()
        .map(|row| Row {
            cli_id: row.cli_id,
            name: row.name,
            home_dir: row.home_dir.display().to_string(),
            active: row.active,
            access: access(row.access),
            said: row.said,
        })
        .collect())
}

/// **A SWITCH IS A FACT LIKE ANY OTHER, AND IT CROSSES THE ONE CHANNEL.** Who
/// the engines run as is drawn in the bar, and that reader hears nothing else:
/// the read behind it starts a command per profile, so it listens for this and
/// for nothing a run says. Without the line here it would go stale until the
/// window was reopened.
#[tauri::command]
pub(crate) fn profile_switch(app: tauri::AppHandle, cli_id: String, name: String) -> Result<(), String> {
    sailor::profiles_cmd::switch(&cli_id, &name)?;
    crate::events::emit(&app, "profile", &serde_json::json!({ "cli_id": cli_id, "name": name }));
    Ok(())
}

#[tauri::command]
pub(crate) fn profile_create(app: tauri::AppHandle, cli_id: String, name: String) -> Result<(), String> {
    sailor::profiles_cmd::create(&cli_id, &name)?;
    crate::events::emit(&app, "profile", &serde_json::json!({ "cli_id": cli_id, "name": name }));
    Ok(())
}

#[cfg(test)]
mod tests {
    /// **EVERY COMMAND LINE IN THE TABLE REACHES THE WINDOW, AND SAYS HOW IT IS
    /// KNOWN.** A row that arrived without its note would assert «no native
    /// profiles» with nothing behind it, and the reader could not tell a
    /// checked no from an unchecked one.
    #[test]
    fn no_command_line_reaches_the_window_stripped_of_its_evidence() {
        let table = ::profiles::known_clis();
        assert!(!table.is_empty(), "an empty table measures nothing");

        let rows = super::profile_command_lines();
        assert_eq!(
            rows.len(),
            table.len(),
            "a command line was lost on the way"
        );
        for row in &rows {
            assert!(
                !row.native_profiles_note.trim().is_empty(),
                "{}: no note on native profiles",
                row.id
            );
            assert!(
                !row.home_note.trim().is_empty(),
                "{}: no note on the home",
                row.id
            );
        }

        // THE ABSURD CASE: the three mechanisms must not collapse into one
        // word. The table holds at least one that moves by variable and one
        // that moves not at all, and if they read the same the window would
        // promise a switch that does nothing.
        let kinds: std::collections::BTreeSet<_> =
            rows.iter().map(|row| row.home_mechanism).collect();
        assert!(
            kinds.len() > 1,
            "every home mechanism reads the same: {kinds:?}"
        );
        assert!(
            rows.iter().any(|row| row.home_mechanism == "none"),
            "the command line whose home does not move is not marked as such",
        );
    }
}
