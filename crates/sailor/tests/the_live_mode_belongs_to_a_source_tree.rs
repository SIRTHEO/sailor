//! **DEVELOPING SAILOR INSIDE SAILOR IS A PROMISE TO WHOEVER HAS THE SOURCE.**
//! The live mode watches a checkout, calls `cargo`, and offers a binary it
//! built a moment ago: to whoever downloaded an executable it would be a
//! button that cannot work. It is kept apart by construction — the supervisor
//! is a crate the command line does not depend on, and no target ships it.

use std::path::{Path, PathBuf};

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .expect("the workspace root")
}

fn manifest_of(crate_name: &str) -> String {
    let path = root().join("crates").join(crate_name).join("Cargo.toml");
    std::fs::read_to_string(&path).unwrap_or_else(|_| panic!("{} is readable", path.display()))
}

#[test]
fn nothing_the_executable_carries_knows_how_to_rebuild_it() {
    // The dependency is what would drag the live mode into the shipped binary:
    // a mention in a comment is prose, a line in `[dependencies]` is cargo.
    let manifest = manifest_of("sailor");
    let depends = manifest
        .lines()
        .any(|line| line.trim_start().starts_with("supervisor"));
    assert!(
        !depends,
        "the command line depends on the supervisor: whoever downloaded a binary \
         would carry the machinery that rebuilds a checkout they do not have"
    );
}

#[test]
fn no_release_target_ships_the_live_mode() {
    for target in release::TARGETS {
        assert_ne!(
            target.bin, "sailor-live",
            "«{}» ships the supervisor: the live mode is for a source tree",
            target.name
        );
    }
}
