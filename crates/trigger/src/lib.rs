//! Where the signal that starts a flow comes from.
//!
//! **WHY AN ENTRY NODE EXISTS.** A graph of steps never says where the work
//! comes from: the first node held the delivery as a constant written inside
//! the file, and changing the job meant rewriting the flow. From the trigger
//! on, the graph is the same, and the delivery is a datum that enters.

/// The entry node: a step with no dependencies that **waits for a signal** and
/// hands the steps downstream what the signal carried.
///
/// **THE BORDER, DECLARED INSTEAD OF SIMULATED.** The manual trigger is real —
/// the signal is what the launcher put in its hand, and it is the source the
/// window's button will use. The terminal one **listens to nothing**.
pub mod action;

/// **THE SOURCES ARE A LIST, NOT A `match`.** No terminal, no product and no
/// path of this machine is named in this crate: the code knows two *shapes* of
/// source — one that carries the signal with it (manual), one that would see it
/// appear in a terminal session — and which terminals exist is what the
/// descriptors say, added by writing one line of JSON.
pub mod descriptor;

pub use action::{register_default, TriggerAction, TRIGGER_ACTION};
pub use descriptor::{
    Catalog, Kind, Listen, Loaded, MissedRun, Periodic, Problem, Source, TriggerDescriptor,
};

use serde::Serialize;
use std::path::PathBuf;
use toolbox::Machine;

/// What a signal carried, in the shape the steps downstream read.
///
/// **EVERY FIELD IS A TEXT, EVEN WHEN EMPTY.** A missing or null field would
/// break the next step's `$join` — it joins text and refuses the rest — and the
/// break would land on a step that has nothing to do with it. A signal with no
/// sender says so with an empty string, and the message's author decides on it.
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
    /// The shape of the source: `manual`, `terminal`, `periodic`.
    pub kind: String,
    /// What the last closed run of this flow left, when there is one: absent
    /// rather than empty, or a first run reads as one that learnt nothing.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub previous_report: Option<serde_json::Value>,
}

/// Where trigger descriptors are taken from.
///
/// The same rules as the tool descriptors, deliberately: whoever learned where
/// a command line is added should not learn it a second time to add a trigger.
/// In the order they win: the shipped ones first, the user's after.
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
