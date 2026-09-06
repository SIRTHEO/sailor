//! What is on this machine, as the window asks for it.
//!
//! DETECTION DOES NOT LIVE HERE. It lives in `toolbox`, where the list of what
//! to look for is data and not code: this module calls it and translates the
//! outcome into the shape the canvas expects. Should the window ever want to
//! know something more, the answer is another descriptor, not a branch here.
//!
//! WHY A TRANSLATION AND NOT THE RAW OUTCOME. The detector tells three states
//! apart — there, absent, could not look — and carries them with the reason.
//! The canvas needs to know whether it can use the tool (`available`), but the
//! reason is not thrown away: whoever sees a missing tool must be able to read
//! why, or the sole thing left to do is distrust the list.

use serde::Serialize;
use toolbox::{Presence, VersionReading};

/// A tool as the canvas receives it. The field names are the contract written
/// in `desktop/src/tools.ts`: whoever changes either changes both.
#[derive(Serialize)]
pub(crate) struct Tool {
    id: String,
    name: String,
    /// `ai_cli` | `mcp` | `tool`, or whatever family a descriptor declares: the
    /// canvas treats the kind as open, and a new family shows under its own
    /// name rather than making the tool vanish.
    kind: String,
    path: Option<String>,
    version: Option<String>,
    available: bool,
    /// **THE THREE ANSWERS, NOT TWO.** `available` says «can I use it», and for
    /// that a tool nobody could look at is as good as absent. But the two are
    /// not the same fact: one is fixed by installing, the other by finding out
    /// why the check could not run, and a list that merges them makes people
    /// install a second copy of what they already have. `toolbox` keeps them
    /// apart and the bridge used to throw the difference away.
    presence: &'static str,
    /// Why it stands so: present from where, absent for what reason, or beyond
    /// checking for what reason. Lacking this, a list cannot be corrected.
    reason: String,
    /// Which descriptor recognised it — the address at which to hold a wrong
    /// row to account.
    descriptor: String,
}

/// A line of the list of what to look for that would not read.
///
/// **NOT A MISSING TOOL, A FAULT IN THE LIST.** It lives apart from the
/// findings for that reason: shown among them it would read as «this tool is
/// broken», when what is broken is the descriptor that was supposed to find it.
#[derive(Serialize)]
pub(crate) struct BadLine {
    source: String,
    about: String,
    reason: String,
}

/// Everything one detection found, including what it could not.
#[derive(Serialize)]
pub(crate) struct Sweep {
    tools: Vec<Tool>,
    /// **THE DIRECTORIES IT LOOKED IN, SPELLED OUT.** A list that does not say
    /// where it searched cannot be contradicted, and somebody who knows they
    /// have a tool has no way to tell whether it is the tool that is missing or
    /// the folder that was never opened.
    looked_in: Vec<String>,
    problems: Vec<BadLine>,
}

/// The tools this machine offers.
///
/// **Detected on every request**, not once at startup: whoever installs a CLI
/// while the window is open must be able to use it without restarting, and the
/// cost is a few processes asked for their own version.
#[tauri::command]
pub(crate) fn discover_tools() -> Vec<Tool> {
    sweep().tools
}

/// The same single detection, with what the reduced view leaves out. One
/// detection and not two: `detect` starts processes, and asking twice to draw
/// one screen would double that for nothing.
#[tauri::command]
pub(crate) fn tools_sweep() -> Sweep {
    sweep()
}

fn sweep() -> Sweep {
    let machine = toolbox::Machine::current();
    let catalog = toolbox::Catalog::load(&toolbox::default_sources(&machine));
    let report = toolbox::detect(&catalog, &machine);
    Sweep {
        looked_in: report.looked_in.clone(),
        problems: report
            .problems
            .iter()
            .map(|problem| BadLine {
                source: problem.source.clone(),
                about: problem.about.clone(),
                reason: problem.reason.clone(),
            })
            .collect(),
        tools: report
            .findings
            .into_iter()
            .map(|found| Tool {
                id: found.name.clone(),
                name: if found.label.is_empty() {
                    found.name.clone()
                } else {
                    found.label
                },
                kind: found.family,
                path: found.executable,
                // A version never obtained stays absent, it does not become a
                // string that looks like a number. "Never asked" and "asked and
                // got no answer" differ to an investigator, but to the canvas
                // they are the same: there is no version to show.
                version: match found.version {
                    VersionReading::Declared(text) => Some(text),
                    VersionReading::NotAsked(_) | VersionReading::Unavailable(_) => None,
                },
                available: found.presence.is_present(),
                presence: match &found.presence {
                    Presence::Present(_) => "present",
                    Presence::Absent(_) => "absent",
                    Presence::Undetermined(_) => "undetermined",
                },
                reason: match &found.presence {
                    Presence::Present(why) => why.clone(),
                    Presence::Absent(why) => why.clone(),
                    Presence::Undetermined(why) => why.clone(),
                },
                descriptor: found.descriptor_id,
            })
            .collect(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// **«NOT INSTALLED» AND «I COULD NOT CHECK» MUST NOT ARRIVE THE SAME.**
    /// The bridge had one boolean, so a tool nobody could look at reached the
    /// window as absent — and the cure for the two is different: one is an
    /// install, the other is a broken check. `toolbox` says so in its own doc;
    /// this is the crossing where it was being undone.
    #[test]
    fn a_tool_nobody_could_look_at_does_not_arrive_as_a_missing_one() {
        let seen = tool_of(Presence::Present("found in /usr/bin".to_owned()));
        let gone = tool_of(Presence::Absent(
            "no such file in any PATH entry".to_owned(),
        ));
        let unknown = tool_of(Presence::Undetermined("the check timed out".to_owned()));

        // THE CONTROL FIRST: the three must not collapse into one word, or
        // every comparison below would hold on a bridge that says nothing.
        let words = [seen.presence, gone.presence, unknown.presence];
        assert_eq!(
            words
                .iter()
                .collect::<std::collections::BTreeSet<_>>()
                .len(),
            3,
            "the three states arrive as {words:?}",
        );
        assert_ne!(
            unknown.presence, gone.presence,
            "an unchecked tool reads as a missing one, which is the old defect",
        );
        // And the reason travels in every case: a state with no why cannot be
        // acted on, and cannot be corrected either.
        for tool in [&seen, &gone, &unknown] {
            assert!(
                !tool.reason.trim().is_empty(),
                "a state arrived with no reason"
            );
        }
    }

    fn tool_of(presence: Presence) -> Tool {
        Tool {
            id: "a".to_owned(),
            name: "A".to_owned(),
            kind: "tool".to_owned(),
            path: None,
            version: None,
            available: presence.is_present(),
            presence: match &presence {
                Presence::Present(_) => "present",
                Presence::Absent(_) => "absent",
                Presence::Undetermined(_) => "undetermined",
            },
            reason: match &presence {
                Presence::Present(why) | Presence::Absent(why) | Presence::Undetermined(why) => {
                    why.clone()
                }
            },
            descriptor: "d".to_owned(),
        }
    }
}
