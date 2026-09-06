//! The dry probe tries the line **in the home it will really run in**.
//!
//! **WHY THE STEP STARTING IN THE RIGHT HOME IS NOT ENOUGH.** A flow step starts
//! the engine inside the active profile's home — the cure for fault 18 — while
//! `RealDryProbe`, what `sailor flow check` uses to call a command line sound,
//! passed an **empty** environment. The two roads diverge exactly where it hurts
//! most: the check tries an authenticated engine, because it inherits the home
//! of whoever opened the terminal, and the real run starts in a home that may
//! hold no credential at all. The check goes green and the run fails — and
//! whoever read the green did nothing wrong.
//!
//! **NOT A TEXTBOOK CASE.** On this machine both declared `codex` profiles
//! (`lavoro` and `prove`) pointed at directories **with no `auth.json`**: every
//! `codex` call from a step would have started unauthenticated, and `flow check`
//! would have gone on calling the line sound.
//!
//! **THIS PROOF DOES NOT SAY THE HOME IS AUTHENTICATED, AND THE BOUNDARY HAS TO
//! BE WRITTEN DOWN.** Starting from the right home, missing credentials were
//! supposed to be named by the engine in its own words (`unusable_when` lists
//! them). **Measured, that is false**: the probe removes the question on purpose,
//! the engine stops there, and never reaches the checks that would come after.
//! `CODEX_HOME=<empty directory> codex exec < /dev/null` answers «No prompt
//! provided via stdin» **identically** to a full home — same words, exit 1 both
//! times — and `flow check` goes on saying the line is sound. What closes here
//! is only the divergence between the world tried and the world worked in, which
//! is already the thing that reassured wrongly.
//!
//! **THE MISSING QUESTION IS NOW ASKED, AND NOT HERE.** Descriptors declare
//! `login_status` — how to ask an engine whether the home it starts from is
//! authenticated — and `flow check` asks it beside this probe: see
//! `crates/actions/tests/the_engine_says_whether_the_home_is_authenticated`.
//! They stay apart because they are two questions, and this one keeps answering
//! only its own.
//!
//! **ONE `#[test]` IN THIS FILE**, for the reason written in
//! `the_engine_really_starts_in_sailors_home`: `PROFILES_STATE_PATH` belongs to
//! the process, and two tests in one binary would write over each other.

use actions::{DryProbe, DryRun, RealDryProbe};
use serde_json::json;
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

/// A throwaway directory under `$TMPDIR`, deleted when the test ends.
struct TempDir(PathBuf);

impl TempDir {
    fn new() -> Self {
        let unique = format!(
            "actions-dry-probe-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("l'orologio non va all'indietro")
                .as_nanos()
        );
        let path = std::env::temp_dir().join(unique);
        fs::create_dir_all(&path).expect("cartella di prova");
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
/// `profiles::cli_for_executable` works on — printing the one thing worth knowing.
fn a_fake_codex_that_prints_its_home(dir: &Path) -> String {
    let path = dir.join("codex");
    fs::write(&path, "#!/bin/sh\nprintf 'CASA=%s\\n' \"$CODEX_HOME\"\n")
        .expect("scrivere il finto motore");
    fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).expect("renderlo eseguibile");
    path.to_string_lossy().into_owned()
}

/// **THE DRY PROBE AND THE REAL RUN MUST START FROM THE SAME HOME.**
///
/// *The mutant that puts the original defect back*: `env: BTreeMap::new()`
/// inside `RealDryProbe::run`. This turns red with an empty `CASA=`, which is
/// exactly what the probe used to print.
#[test]
fn the_dry_probe_starts_inside_the_home_the_profile_declares() {
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
    .expect("scrivere lo stato dei profili");
    std::env::set_var("PROFILES_STATE_PATH", &state);

    let DryRun::Answered { stdout, .. } = RealDryProbe.run(&bin, &[], None) else {
        panic!("il finto motore risponde sempre: qui non c'è niente da aspettare");
    };

    assert!(
        stdout.contains(&format!("CASA={}", home_of_the_profile.display())),
        "il vaglio a secco ha provato la riga in una casa diversa da quella in cui \
         girerà: {stdout:?}"
    );
}
