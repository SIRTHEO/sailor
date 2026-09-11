//! **TWO LOOPS BEAT, AND WHAT ONE SWEEPS THE OTHER MUST SWEEP.** The judgement
//! is shared and lives in `flow_cmd::beat`; the loops calling it are not. The
//! parked runs were swept by the command line alone, and nothing but a person
//! ever runs a tick on this machine, so from the window they simply stayed.

use std::path::{Path, PathBuf};

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .expect("the workspace root")
}

fn read(at: &str) -> String {
    let path = root().join(at);
    std::fs::read_to_string(&path)
        .unwrap_or_else(|error| panic!("reading {}: {error}", path.display()))
}

/// A name read where it is **called** and not where it is declared: over the
/// whole file this check stayed green with the call taken out.
fn body_of(source: &str, signature: &str) -> String {
    let from = source
        .find(signature)
        .unwrap_or_else(|| panic!("«{signature}» is gone: this check no longer measures anything"));
    let rest = &source[from..];
    let end = rest.find("\n}\n").map(|at| at + 2).unwrap_or(rest.len());
    rest[..end].to_owned()
}

/// The one pass each loop makes, by the signature that opens it.
const THE_PASS: &[(&str, &str)] = &[
    ("crates/sailor/src/flow_cmd/beat.rs", "fn tick_flows_with("),
    ("desktop/src-tauri/src/beat.rs", "pub fn once("),
];

#[test]
fn every_beat_asks_the_parked_runs_again_in_the_pass_it_makes() {
    for (at, signature) in THE_PASS {
        let pass = body_of(&read(at), signature);

        // THE CONTROL: a body that read as empty would agree with anything.
        assert!(pass.len() > 200, "{at}: «{signature}» read as {} bytes", pass.len());
        assert!(
            pass.contains("ask_the_parked_again"),
            "{at}: the beat makes a pass and does not ask the parked runs again. \
             One loop sweeping and the other leaving them is how fifty-six ran up."
        );
    }
}

/// The judgement is shared or it is two judgements: the window reaches into
/// `sailor::flow_cmd::beat` rather than keeping a reading of its own.
#[test]
fn the_window_asks_the_shared_judgement_rather_than_growing_its_own() {
    let window = read("desktop/src-tauri/src/beat.rs");

    assert!(
        window.contains("sailor::flow_cmd::beat::ask_the_parked_again"),
        "the window's beat sweeps the parked runs by some reading of its own"
    );
}
