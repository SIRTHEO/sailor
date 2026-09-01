//! The shape in which the machine census reaches whoever looks: every entry
//! declares a known kind, a name and an origin, and anything unreachable
//! carries its reason. Without a reason, «off» is a word nobody can correct.
//! **THE ROOTS ARE BUILT HERE, NEVER TAKEN FROM `$HOME`**: rerunning the whole
//! battery against an empty home turned this red with the code unchanged. The
//! injection point does exist — it is `collect`'s parameter — so it is used.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, Ordering};

/// A counter, never the clock alone. `cargo test` runs the tests in one
/// process and the macOS clock has no nanosecond resolution, so two directories
/// born in the same instant used to steal each other's place.
static NEXT_SCRATCH: AtomicU32 = AtomicU32::new(0);

fn scratch_dir(label: &str) -> PathBuf {
    let serial = NEXT_SCRATCH.fetch_add(1, Ordering::Relaxed);
    let dir = std::env::temp_dir().join(format!(
        "ui-inventory-{}-{serial}-{label}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    dir
}

fn write(path: &Path, text: &str) {
    std::fs::create_dir_all(path.parent().expect("every file has a directory"))
        .expect("creating the directory of the test file");
    std::fs::write(path, text).expect("writing the test file");
}

/// A home and a repo built on purpose: two commands, a rule and a hook, picked
/// because they cover all three states the window knows how to draw.
///
/// The broken hook points at a file that does not exist, and that is the only
/// way to produce an «inactive» state carrying its own reason: under home
/// reachability is «active» by construction, under a repo it is «unknown».
fn fixture_roots(label: &str) -> Vec<inventory::Root> {
    // **WHAT THIS GIVES UP, DECLARED.** It no longer proves that skills and
    // hooks really exist on this machine: that is a fact about the world, and
    // not what the window must be able to count on. It proves that *given* a
    // known tree the shape comes out right — and a known tree can demand what
    // `$HOME` could not, that all three reachability states really appear.
    let base = scratch_dir(label);
    let home = base.join("home");
    let repo = base.join("work-repo");

    write(
        &home.join(".claude/commands/greet.md"),
        "# Greet\n\nA test command.\n",
    );
    write(
        &home.join(".claude/rules/a-rule.md"),
        "# A rule\n\nThe text of the rule.\n",
    );
    write(
        &home.join(".claude/settings.json"),
        r#"{
  "hooks": {
    "PreToolUse": [
      {
        "matcher": "Bash",
        "hooks": [
          { "type": "command", "command": "/does/not/exist/dead-hook.sh --check" }
        ]
      }
    ]
  }
}
"#,
    );
    write(
        &repo.join(".claude/commands/work.md"),
        "# Work\n\nA command that counts inside this repo alone.\n",
    );

    vec![inventory::Root::home(&home), inventory::Root::repo(&repo)]
}

/// **IT RECEIVES THE ROOTS, IT DOES NOT GO LOOKING FOR THEM.**
/// `collect_survey(&default_roots(ledger::sailor_home()))` is the right shape
/// for whoever looks at the real machine — the command line and the window —
/// but not for a test, which would go back to depending on `$HOME`. `collect`
/// sits beside `collect_survey` for exactly this: whoever builds the roots by
/// hand has no survey to pass.
fn census(roots: &[inventory::Root]) -> serde_json::Value {
    let found = inventory::collect(roots);
    serde_json::to_value(&found).expect("the census serializes")
}

#[test]
fn the_census_answers_with_the_shape_the_window_reads() {
    let roots = fixture_roots("shape");
    let body = census(&roots);

    let entries = body["entries"].as_array().expect("array of entries");
    let declared = body["roots"].as_array().expect("array of roots");
    assert_eq!(declared.len(), roots.len(), "every root declares itself");
    let stale = body["stale_plugin_copies"]
        .as_u64()
        .expect("number of cached copies");

    // The fixture tree carries four entries: were the list empty, the answer
    // would have the right shape and the wrong content, and this check catches
    // that. The same guard as before, now against a number we know.
    assert_eq!(
        entries.len(),
        4,
        "the fixture tree carries two commands, a rule and a hook: {entries:?}"
    );

    let known_kinds = ["skill", "agent", "command", "rule", "hook"];
    for entry in entries {
        let kind = entry["kind"].as_str().expect("every entry declares a kind");
        assert!(known_kinds.contains(&kind), "unexpected kind: {kind}");
        assert!(entry["name"].as_str().is_some(), "every entry has a name");
        assert!(
            entry["origin"].as_str().is_some(),
            "every entry declares its origin"
        );
        let state = entry["reach"]["state"]
            .as_str()
            .expect("every entry declares reach.state");
        assert!(
            ["active", "inactive", "unknown"].contains(&state),
            "unexpected reachability state: {state}"
        );
        if state != "active" {
            assert!(
                entry["reach"]["reason"].as_str().is_some(),
                "whatever is not active carries its reason in writing"
            );
        }
    }

    // No plugin cache in a tree built just now: this used to be «that is not an
    // absurd number», which was all anyone could ask of an unknown machine.
    assert_eq!(stale, 0, "the fixture tree has no cached copies");
}

/// **ALL THREE STATES APPEAR, AND THAT COULD NOT BE DEMANDED BEFORE.** The loop
/// above checks «if it is not active then a reason is there»: over an all-active
/// list it would stay green having never run that line, which is how a test
/// confirms itself. With the fixture tree the states can be demanded.
#[test]
fn every_reachability_state_carries_what_the_window_needs() {
    let roots = fixture_roots("states");
    let body = census(&roots);
    let entries = body["entries"].as_array().expect("array of entries");

    let state_of = |wanted: &str| -> Vec<&serde_json::Value> {
        entries
            .iter()
            .filter(|entry| entry["reach"]["state"].as_str() == Some(wanted))
            .collect()
    };

    let active = state_of("active");
    assert!(
        !active.is_empty(),
        "home is reachable: something must be active"
    );

    let inactive = state_of("inactive");
    assert_eq!(
        inactive.len(),
        1,
        "a single hook points at a file that is absent: {inactive:?}"
    );
    let reason = inactive[0]["reach"]["reason"]
        .as_str()
        .expect("an inactive entry carries its reason");
    assert!(
        reason.contains("dead-hook.sh"),
        "the reason names the missing file: {reason}"
    );

    let unknown = state_of("unknown");
    assert!(
        !unknown.is_empty(),
        "what lives in a repo counts only there, and the window must say so"
    );
    assert!(
        unknown[0]["reach"]["reason"].as_str().is_some(),
        "«unknown» carries its own reason too"
    );
}
