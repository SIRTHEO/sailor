//! See fault 181. The unit tests beside this one judge an answer already in
//! hand; this runs the real probe, so only it can fail the way that fault did.
//! Skipped, not silent, without `SAILOR_TEST_CLAUDE_HOME`: CI has no login.

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
