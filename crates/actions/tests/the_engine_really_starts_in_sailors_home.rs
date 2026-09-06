//! The engine **really** starts inside the home the profile declares.
//!
//! **WHY THIS PROOF SITS BESIDE `the_equipment_reaches_the_engines`.** That one
//! proves the *rule* — which environment must be composed — and would stay green
//! with the rule unplugged: `ExternalEngineAction` need only go on passing
//! `spec.env` to the invocation, which is exactly fault 18. A correct rule
//! nobody calls is indistinguishable from an absent rule when you read the
//! tests. Here we watch the one thing that cannot be faked: a real process,
//! started by the step, printing the variable it received.
//!
//! **ONE `#[test]` IN THIS FILE, AND IT IS NOT LAZINESS.** The test must declare
//! `PROFILES_STATE_PATH`, which belongs to the **process**: `cargo test` runs
//! one binary's tests on several threads of the same process, so a second test
//! in here would read a variable this one wrote while running. A file of its own
//! is a process of its own. The two cases that matter are therefore two arms of
//! one body.

use flow::{Action, ActionOutcome, SharedState};
use serde_json::json;
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

/// A throwaway directory under `$TMPDIR`, deleted when the test ends. No
/// external dependency: the same pattern used elsewhere in the tree.
struct TempDir(PathBuf);

impl TempDir {
    fn new() -> Self {
        let unique = format!(
            "actions-equipment-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("the clock does not run backwards")
                .as_nanos()
        );
        let path = std::env::temp_dir().join(unique);
        fs::create_dir_all(&path).expect("the scratch directory");
        TempDir(path)
    }

    fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

/// A fake `codex`: named after the real executable — the link
/// `profiles::cli_for_executable` works on — printing the one thing worth
/// knowing, which home it received.
fn a_fake_codex_that_prints_its_home(dir: &Path) -> String {
    let path = dir.join("codex");
    fs::write(&path, "#!/bin/sh\nprintf 'HOME_IS=%s\\n' \"$CODEX_HOME\"\n")
        .expect("write the fake engine");
    fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).expect("renderlo eseguibile");
    path.to_string_lossy().into_owned()
}

fn what_the_engine_said(outcome: &ActionOutcome) -> String {
    let ActionOutcome::Went(value) = outcome else {
        panic!("the step had to go: {outcome:?}");
    };
    value
        .get("stdout")
        .and_then(serde_json::Value::as_str)
        .expect("the engine's output")
        .to_owned()
}

/// **FAULT 18 AGAINST A REAL PROCESS.**
///
/// First arm: a step declaring nothing must start the engine in the active
/// profile's home. It used to start with the home of whoever opened the
/// terminal, and `CODEX_HOME` arrived empty.
///
/// Second arm: a step declaring the variable wins. Both are needed — the first
/// alone would stay green if the profile overrode the step, the second alone
/// would stay green if the profile never arrived.
///
/// *Mutant run*: put `env: spec.env.clone()` back in the invocation. The first
/// arm turns red, the second stays green.
#[test]
fn the_engine_really_starts_inside_the_home_the_profile_declares() {
    let dir = TempDir::new();
    let bin = a_fake_codex_that_prints_its_home(dir.path());
    let home_of_the_profile = dir.path().join("case").join("codex").join("lavoro");

    let state = dir.path().join("profili.json");
    fs::write(
        &state,
        json!({
            "profiles": [
                {"name": "lavoro", "cli_id": "codex", "home_dir": home_of_the_profile}
            ],
            "active": {"codex": "lavoro"}
        })
        .to_string(),
    )
    .expect("write the profiles state");
    std::env::set_var("PROFILES_STATE_PATH", &state);

    let action = actions::ExternalEngineAction::new();

    let shared = SharedState::new();
    let said = what_the_engine_said(
        &action
            .execute(&json!({"bin": bin, "timeout_secs": 30}), &shared)
            .expect("the step had to go through"),
    );
    assert!(
        said.contains(&format!("HOME_IS={}", home_of_the_profile.display())),
        "the engine started with the home of whoever opened the terminal: {said:?}"
    );

    let shared = SharedState::new();
    let said = what_the_engine_said(
        &action
            .execute(
                &json!({
                    "bin": bin,
                    "env": {"CODEX_HOME": "/a/home/written/in/the/step"},
                    "timeout_secs": 30
                }),
                &shared,
            )
            .expect("the step had to go through"),
    );
    assert!(
        said.contains("HOME_IS=/a/home/written/in/the/step"),
        "the profile overrode what the step declares: {said:?}"
    );
}
