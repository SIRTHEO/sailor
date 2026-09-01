//! Downloads the real catalog by running `curl` as a process — the road `notte`
//! took to OpenRouter, from `crates/notte/src/main.rs:563`, a crate no longer in
//! the repo. No authentication needed: no key in here.
//!
//! No tests here, the reason `notte::fetch_openrouter_body` had none: a test on
//! the real network is red when the line drops, not when the code is wrong.

use std::process::Command;

const CATALOG_URL: &str = "https://openrouter.ai/api/v1/models";

/// Downloads the catalog's JSON body. `MODELS_CATALOG_FETCH_OVERRIDE`, when
/// set, replaces `curl` with any command at all: it lets someone feed in a
/// fixed catalog without touching the network, the way `NOTTE_OPENROUTER_FETCH`
/// already does for `notte`.
pub fn fetch_catalog_body() -> String {
    if let Ok(cmd) = std::env::var("MODELS_CATALOG_FETCH_OVERRIDE") {
        return Command::new(cmd)
            .output()
            .ok()
            .and_then(|o| String::from_utf8(o.stdout).ok())
            .unwrap_or_default();
    }
    Command::new("curl")
        .args(["-sS", "-m", "30", CATALOG_URL])
        .output()
        .ok()
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .unwrap_or_default()
}
