//! Fault 181: `env_clear` before `claude auth status` kept only `PATH` and
//! `HOME`, and macOS's keychain would not answer without the session's own
//! identity — every home came back `NOT AUTHENTICATED`, including the ones a
//! worker was running under at that moment.
//!
//! **WHY THIS RUNS THE REAL ENGINE INSTEAD OF A RECORDED ANSWER.** The unit
//! tests in `the_engine_says_whether_the_home_is_authenticated.rs` prove the
//! *reading* of an answer already in hand; they cannot prove that the probe's
//! own environment still reaches the keychain, because they never build one.
//! Only a real `claude auth status`, launched the way the probe launches it,
//! can fail the way this fault failed.
//!
//! **SKIPPED, NOT SILENT, WITHOUT A HOME.** CI has no logged-in Claude home,
//! and a hard failure there would tell nobody anything about this bug. Set
//! `SAILOR_TEST_CLAUDE_HOME` to a real, authenticated home to run it for real.

use actions::{probe_login_status, LoginRecipe, LoginVerdict, RealDryProbe};
use models::usage::Pointer;
use std::collections::BTreeMap;

fn claude_recipe() -> LoginRecipe {
    LoginRecipe {
        args: vec!["auth".to_owned(), "status".to_owned()],
        answer: Some(Pointer::Path(vec!["loggedIn".to_owned()])),
        logged_in_when: vec!["true".to_owned()],
        logged_out_when: vec!["false".to_owned()],
    }
}

#[test]
fn the_real_probe_reaches_an_authenticated_home() {
    let Ok(home) = std::env::var("SAILOR_TEST_CLAUDE_HOME") else {
        println!(
            "skipped: set SAILOR_TEST_CLAUDE_HOME to a real, authenticated \
             claude home to run this test"
        );
        return;
    };

    let environment = BTreeMap::from([("CLAUDE_CONFIG_DIR".to_owned(), home)]);
    let verdict = probe_login_status(&RealDryProbe, "claude", &environment, &claude_recipe());

    assert!(
        matches!(verdict, LoginVerdict::LoggedIn { .. }),
        "an authenticated home must probe as logged in, not {verdict:?}"
    );
}
