//! Where the signal that starts a flow comes from.
//!
//! A trigger is a step with no dependencies that **waits for a signal** and
//! hands down what the signal carried, so the delivery is data entering the
//! graph rather than a constant written into the flow file. No terminal, no
//! product and no path of one machine is named here: the code knows two
//! *shapes* of source, and which terminals exist is a line of JSON.

pub mod action;
pub mod descriptor;

pub use action::{register_default, TriggerAction, TRIGGER_ACTION};
pub use descriptor::{Catalog, Kind, Listen, Loaded, Problem, Source, TriggerDescriptor};

use serde::Serialize;
use std::path::PathBuf;
use toolbox::Machine;

/// What a signal carried, in the shape the steps downstream read.
///
/// **EVERY FIELD IS A TEXT, EVEN WHEN EMPTY.** A missing or null field would
/// break the next step's `$join` — which joins text and refuses anything else —
/// and the break would land on a step that has nothing to do with it. A signal
/// that does not know who sent it says so with an empty string.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Signal {
    /// The delivery: the text the signal carried.
    pub text: String,
    /// Who sent it, as far as the source knows. Empty when it does not.
    pub who: String,
    /// From where: the session, the pane, the window. Empty when unknown.
    #[serde(rename = "where")]
    pub where_from: String,
    /// The `id` of the descriptor that recognised the signal.
    pub source: String,
    /// The shape of the source: `manual`, `terminal`.
    pub kind: String,
}

/// Where trigger descriptors are taken from.
///
/// The same rules as the tool descriptors, deliberately: whoever learned where
/// a command line is added should not learn it twice. Shipped ones first, the
/// user's after — later wins.
pub fn default_sources(machine: &Machine) -> Vec<Source> {
    let mut out = vec![Source::Builtin];
    out.push(Source::Dir(
        toolbox::sailor_home_for(machine).join("triggers.d"),
    ));
    if let Some(extra) = machine.env.get("SAILOR_TRIGGER_DESCRIPTORS") {
        for raw in extra.split(':').filter(|s| !s.is_empty()) {
            let path = PathBuf::from(machine.expand(raw));
            if path.is_dir() {
                out.push(Source::Dir(path));
            } else {
                out.push(Source::File(path));
            }
        }
    }
    out
}
