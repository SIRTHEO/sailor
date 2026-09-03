//! The engines on this machine: is it here, is it signed in, how much is
//! left, and what gesture signs it in or puts it on the machine.
//!
//! **EVERY ANSWER IS THE ENGINE'S OR THE DESCRIPTOR'S, NEVER THIS MODULE'S**:
//! the equipment's detection, the profiles' sign-in probe, the quota channel
//! of `sailor remaining`. A second copy would be free to disagree.

use std::collections::BTreeMap;

use actions::{LoginVerdict, RealDryProbe, ToolResolver};
use serde::Serialize;
use toolbox::{Presence, VersionReading};

/// The gesture that signs an engine in, ready for a terminal.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub(crate) struct SignIn {
    pub program: String,
    pub args: Vec<String>,
    /// The sign-in continues inside the program: a browser, a code to type.
    pub interactive: bool,
    pub note: String,
}

/// The line that installs an engine, typed for a person to confirm.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub(crate) struct Install {
    pub line: String,
    pub note: String,
}

/// An engine set aside after saying its quota was spent: until when, and its words.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub(crate) struct SetAside {
    pub until: i64,
    pub said: String,
}

/// The cap the person wrote for an engine, and what the ledger sums for it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub(crate) struct Budget {
    pub cap_micros: i64,
    pub window_secs: i64,
    pub spent_micros: Option<i64>,
    pub spent_why: Option<String>,
}

#[derive(Serialize)]
pub(crate) struct Engine {
    pub id: String,
    pub label: String,
    /// `present` | `absent` | `undetermined`, and the reason beside it.
    pub presence: &'static str,
    pub reason: String,
    pub executable: Option<String>,
    pub version: Option<String>,
    /// `yes` | `no` | `not known`, and the engine's own words beside it.
    pub signed_in: &'static str,
    pub signed_in_said: String,
    /// The profile the engine runs under now, when the store names one.
    pub profile_in_force: Option<String>,
    pub quota: Vec<crate::models::Window>,
    /// Why there is no quota to show, when there is none.
    pub quota_why: Option<String>,
    pub sign_in: Option<SignIn>,
    pub install: Option<Install>,
    /// Why the chain would not knock on it now, when it would not.
    pub set_aside: Option<SetAside>,
    pub budget: Option<Budget>,
}

#[derive(Serialize)]
pub(crate) struct Engines {
    /// Where a terminal opened for a gesture starts: the directory the window
    /// stands in.
    pub workspace_root: String,
    pub engines: Vec<Engine>,
}

/// The verdict as the screen names it, and the words behind it.
///
/// **WHAT IS NOT A YES IS NEVER A YES.** A descriptor that declares no way to
/// ask, a probe that got no answer, an answer nobody could read: all three are
/// «not known», and the sentence says which.
pub(crate) fn signed_in_words(verdict: LoginVerdict) -> (&'static str, String) {
    match verdict {
        LoginVerdict::LoggedIn { said } => ("yes", one_line(&said)),
        LoginVerdict::LoggedOut { said } => ("no", one_line(&said)),
        LoginVerdict::NotDeclared => (
            "not known",
            "the descriptor does not say how to ask whether it is signed in".to_owned(),
        ),
        LoginVerdict::Unrecognised { said } => (
            "not known",
            format!("it answered «{}», which is neither of the declared forms", one_line(&said)),
        ),
        LoginVerdict::NoAnswer { why } => ("not known", format!("no answer: {why}")),
    }
}

fn one_line(said: &str) -> String {
    said.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// The two gestures a descriptor declares, given where the executable was
/// found. A sign-in wants the program that is really here; an install line
/// stands on its own.
pub(crate) fn gestures_of(
    descriptor: &toolbox::descriptor::Descriptor,
    executable: Option<&str>,
) -> (Option<SignIn>, Option<Install>) {
    let sign_in = match (&descriptor.login, executable) {
        (Some(login), Some(program)) => Some(SignIn {
            program: program.to_owned(),
            args: login.args.clone(),
            interactive: login.interactive,
            note: login.note.clone(),
        }),
        _ => None,
    };
    let install = descriptor.install.as_ref().map(|install| Install {
        line: install.line.clone(),
        note: install.note.clone(),
    });
    (sign_in, install)
}

fn file_name(path: &str) -> &str {
    std::path::Path::new(path)
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or(path)
}

/// What holds an engine back now: the list of engines set aside, and the
/// person's caps with the ledger's sum. Both come from the same files and
/// the same query the chain reads, never from a copy.
pub(crate) fn held_back(id: &str, now: i64) -> (Option<SetAside>, Option<Budget>) {
    let set_aside = actions::cooldown::default_path()
        .and_then(|path| actions::cooldown::set_aside_until(&path, id, now))
        .map(|aside| SetAside { until: aside.until, said: aside.said });
    let budget = actions::budget::default_path()
        .and_then(|path| actions::budget::declared(&path).ok())
        .and_then(|caps| caps.get(id).cloned())
        .map(|cap| {
            let dir = ui::gather::default_ledger_dir();
            let spent = if dir.join("state.db").exists() {
                ledger::Ledger::open(&dir)
                    .and_then(|ledger| ledger.spent_by_cli_since(id, now - cap.window_secs))
                    .map(|spend| spend.micros)
                    .map_err(|error| error.to_string())
            } else {
                Err("no ledger on this machine yet".to_owned())
            };
            Budget {
                cap_micros: cap.cap_micros,
                window_secs: cap.window_secs,
                spent_micros: spent.as_ref().ok().copied(),
                spent_why: spent.err(),
            }
        });
    (set_aside, budget)
}

/// **THIS READ RUNS COMMANDS**: one detection, and one `login status`-class
/// question per engine that is here and declares one. They read local files
/// and call no model; the screen asks when it opens, not on every redraw.
#[tauri::command]
pub(crate) fn engines() -> Result<Engines, String> {
    let machine = toolbox::Machine::current();
    let catalog = toolbox::Catalog::load(&toolbox::default_sources(&machine));
    let report = toolbox::detect(&catalog, &machine);
    let tools = toolbox::Tools::current();
    let store = profiles::store_io::load_store()
        .map_err(|error| format!("the profile store cannot be read: {error}"))?;
    let env: BTreeMap<String, String> = profiles::active_environment(&store).into_iter().collect();
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |since| since.as_secs() as i64);

    let mut engines = Vec::new();
    for found in report.findings.into_iter().filter(|found| found.family == "ai_cli") {
        let Some(descriptor) = catalog
            .live()
            .into_iter()
            .find(|loaded| loaded.descriptor.id == found.descriptor_id)
            .map(|loaded| loaded.descriptor.clone())
        else {
            continue;
        };
        let (sign_in, install) = gestures_of(&descriptor, found.executable.as_deref());
        let (signed_in, signed_in_said) = match (&found.presence, found.executable.as_deref()) {
            (Presence::Present(_), Some(bin)) => match tools.login_recipe(&descriptor.id) {
                Some(recipe) => {
                    signed_in_words(actions::probe_login_status(&RealDryProbe, bin, &env, &recipe))
                }
                None => signed_in_words(LoginVerdict::NotDeclared),
            },
            _ => ("not known", "it is not on this machine, so there is nobody to ask".to_owned()),
        };
        let profile_in_force = found.executable.as_deref().and_then(|bin| {
            profiles::known_clis()
                .iter()
                .find(|cli| cli.executable == file_name(bin))
                .and_then(|cli| store.active.get(cli.id).cloned())
        });
        // The channel is the descriptor's: an engine that declares none is
        // said so, and no engine is named here.
        let (quota, quota_why) = match toolbox::quota::read_one(&descriptor, &machine, now) {
            Some(reading) => match reading.result {
                Ok(found) => (found.into_iter().map(crate::models::Window::from).collect(), None),
                Err(why) => (Vec::new(), Some(why)),
            },
            None => (Vec::new(), Some("this engine declares no channel to read what is left".to_owned())),
        };
        let (set_aside, budget) = held_back(&descriptor.id, now);
        engines.push(Engine {
            id: descriptor.id.clone(),
            label: if found.label.is_empty() { found.name.clone() } else { found.label.clone() },
            presence: match &found.presence {
                Presence::Present(_) => "present",
                Presence::Absent(_) => "absent",
                Presence::Undetermined(_) => "undetermined",
            },
            reason: match &found.presence {
                Presence::Present(why) | Presence::Absent(why) | Presence::Undetermined(why) => why.clone(),
            },
            executable: found.executable.clone(),
            version: match found.version {
                VersionReading::Declared(text) => Some(text),
                VersionReading::NotAsked(_) | VersionReading::Unavailable(_) => None,
            },
            signed_in,
            signed_in_said,
            profile_in_force,
            quota,
            quota_why,
            sign_in,
            install,
            set_aside,
            budget,
        });
    }
    let workspace_root = std::env::current_dir()
        .map(|dir| dir.display().to_string())
        .map_err(|error| format!("the window stands nowhere: {error}"))?;
    Ok(Engines { workspace_root, engines })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// **NOTHING THAT IS NOT A YES BECOMES A YES.** The control first: a
    /// descriptor that never said how to ask is «not known», and so is an
    /// answer nobody could read; only the engine's own «logged in» is a yes.
    #[test]
    fn only_the_engines_own_yes_is_a_yes() {
        assert_eq!(signed_in_words(LoginVerdict::NotDeclared).0, "not known");
        assert_eq!(signed_in_words(LoginVerdict::Unrecognised { said: "hm".into() }).0, "not known");
        assert_eq!(signed_in_words(LoginVerdict::NoAnswer { why: "gone".into() }).0, "not known");
        let (verdict, said) = signed_in_words(LoginVerdict::LoggedOut { said: "Not\n logged in".into() });
        assert_eq!((verdict, said.as_str()), ("no", "Not logged in"));
        assert_eq!(signed_in_words(LoginVerdict::LoggedIn { said: "ok".into() }).0, "yes");
    }

    /// A sign-in wants the program that is really here: with no executable
    /// there is no gesture, however loudly the descriptor declares one. The
    /// install line stands on its own.
    #[test]
    fn a_sign_in_needs_the_executable_and_an_install_line_does_not() {
        let descriptor: toolbox::descriptor::Descriptor = serde_json::from_str(
            r#"{"id": "x", "family": "ai_cli",
                "login": {"args": ["login"], "interactive": true},
                "install": {"line": "brew install x", "note": "measured"}}"#,
        )
        .expect("parses");
        let (absent_sign_in, install) = gestures_of(&descriptor, None);
        assert!(absent_sign_in.is_none());
        assert_eq!(install.as_ref().map(|one| one.line.as_str()), Some("brew install x"));
        let (sign_in, _) = gestures_of(&descriptor, Some("/usr/local/bin/x"));
        let sign_in = sign_in.expect("the gesture");
        assert_eq!((sign_in.program.as_str(), sign_in.args.as_slice()), ("/usr/local/bin/x", &["login".to_owned()][..]));
        assert!(sign_in.interactive);
    }
}
