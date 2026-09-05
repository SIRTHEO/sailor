//! "What do the flows on this machine ask for that is not here?"
//!
//! **WHAT POWER IT HAS.** One only: reading the flow files in the places Sailor
//! looks for them. The comparison it makes afterwards is not a power, it is
//! composition — and that is why all of it sits here, and not in an interpreter
//! inside the flow.

use crate::Finding;
use flow::system::{self, FlowSource};
use flow::{Action, ActionError, ActionOutcome, FlowFile, SharedState, StepSpecies};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::path::{Path, PathBuf};

/// The name the action registers under.
pub const TOOL_NEEDS_ACTION: &str = "tool_needs";

/// Registers the action under its stable name.
pub fn register_needs(registry: &mut flow::ActionRegistry) {
    registry.register(TOOL_NEEDS_ACTION, ToolNeedsAction);
}

/// The step's input.
///
/// **NO `deny_unknown_fields`, DELIBERATELY.** The input of a step with a
/// dependency *is* the output of that dependency with `with` laid over it, so
/// everything the detection produced arrives too. Refusing unknown fields would
/// refuse the only way this action can be invoked.
#[derive(Debug, Deserialize)]
struct NeedsSpec {
    /// What the detection step found on this machine.
    ///
    /// **THE ACTION MAKES NO DETECTION OF ITS OWN, AND THAT IS NOT A GAP.** The
    /// flow declares the chain instead of hiding it, the detection is paid for
    /// once, and *another* machine's detection — a list arrived from outside —
    /// works here unchanged, because this action never knew where it came from.
    findings: Vec<Finding>,
    /// Flow directories to look at instead of the usual ones.
    #[serde(default)]
    flows_dirs: Vec<String>,
    /// Whether to look where Sailor always looks too: the shipped flows, home's,
    /// the project's.
    #[serde(default = "yes")]
    include_default_sources: bool,
}

fn yes() -> bool {
    true
}

/// A tool at least one step asks for, and how it stands here.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Need {
    /// The tool's identifier, as the step writes it.
    pub tool: String,
    /// Which steps ask for it, written `flow/step`: without this line the reader
    /// knows something is missing and not what will stop working.
    pub asked_by: Vec<String>,
    /// Where the executable is, when there is one.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub executable: Option<String>,
    /// Why it is not here, when it is not.
    #[serde(skip_serializing_if = "String::is_empty")]
    pub reason: String,
    /// The descriptor's note: it is where installing it is written down, and it
    /// is all the reader will have to put things right.
    #[serde(skip_serializing_if = "String::is_empty")]
    pub note: String,
}

/// Answers "do these flows run here?".
///
/// **WHY DETECTION IS NOT ENOUGH.** `detect_tools` answers "what is here": a
/// list, and a list tells nobody what to do with it. The question a person
/// really has is *do these flows, on this machine, run?* — which needs what the
/// machine offers and what the flows ask for. This one brings the second half.
pub struct ToolNeedsAction;

impl Action for ToolNeedsAction {
    fn execute(&self, input: &Value, _shared: &SharedState) -> Result<ActionOutcome, ActionError> {
        let spec: NeedsSpec = serde_json::from_value(input.clone()).map_err(|error| {
            ActionError::new(
                "invalid_input",
                format!(
                    "{error}. This step goes after a detection: it takes that step's \
                     `findings` as its input, and on its own it does not look at the machine"
                ),
            )
        })?;

        let mut sources: Vec<FlowSource> = Vec::new();
        if spec.include_default_sources {
            sources.extend(default_flow_sources());
        }
        for raw in &spec.flows_dirs {
            sources.push(FlowSource {
                origin: "declared in the step",
                dir: PathBuf::from(raw),
            });
        }

        let mut asked: BTreeMap<String, Vec<String>> = BTreeMap::new();
        let mut named_binaries: Vec<String> = Vec::new();
        let mut flows_seen = 0usize;
        let mut steps_seen = 0usize;
        let mut flows_broken: Vec<String> = Vec::new();
        for (name, _, entry) in system::load_all(&sources) {
            match entry {
                Ok(flow) => {
                    flows_seen += 1;
                    steps_seen += steps_read(&flow);
                    for (tool, step) in tools_named_by(&flow) {
                        asked
                            .entry(tool)
                            .or_default()
                            .push(format!("{name}/{step}"));
                    }
                    for step in binaries_named_by(&flow) {
                        named_binaries.push(format!("{name}/{step}"));
                    }
                }
                // A BROKEN FLOW IS NOT A FLOW THAT ASKS FOR NOTHING. Counting it
                // as zero would say "this is all you need" to someone holding
                // half the list.
                Err(_) => flows_broken.push(name),
            }
        }

        let found: BTreeMap<&str, &Finding> = spec
            .findings
            .iter()
            .map(|finding| (finding.name.as_str(), finding))
            .collect();

        let mut present: Vec<Need> = Vec::new();
        let mut missing: Vec<Need> = Vec::new();
        let mut unknown: Vec<Need> = Vec::new();
        for (tool, asked_by) in asked {
            match found.get(tool.as_str()) {
                Some(finding) if finding.presence.is_present() => present.push(Need {
                    tool,
                    asked_by,
                    executable: finding.executable.clone(),
                    reason: String::new(),
                    note: finding.note.clone(),
                }),
                Some(finding) => missing.push(Need {
                    tool,
                    asked_by,
                    executable: None,
                    reason: presence_reason(finding),
                    note: finding.note.clone(),
                }),
                None => unknown.push(Need {
                    tool,
                    asked_by,
                    executable: None,
                    reason: "no descriptor declares it: it is not that it is missing from this \
                             machine, it is that Sailor does not know what it is. Add one by \
                             writing a JSON file in ~/.config/sailor/tools.d/, no recompile"
                        .to_string(),
                    note: String::new(),
                }),
            }
        }

        let looked_in: Vec<String> = sources
            .iter()
            .map(|source| format!("{}: {}", source.origin, source.dir.display()))
            .collect();
        let report = report_of(
            flows_seen,
            steps_seen,
            &flows_broken,
            &looked_in,
            Asked {
                present: &present,
                missing: &missing,
                unknown: &unknown,
            },
            &named_binaries,
        );

        Ok(ActionOutcome::Went(json!({
            "flows_seen": flows_seen,
            "steps_seen": steps_seen,
            "flows_broken": flows_broken,
            "looked_in": looked_in,
            "present": present,
            "missing": missing,
            "unknown": unknown,
            "steps_naming_a_binary": named_binaries,
            "report": report,
        })))
    }

    /// It reads files and compares two lists: doing it again changes nothing.
    fn species(&self) -> StepSpecies {
        StepSpecies::Repeatable
    }
}

/// The places Sailor looks for flows on this machine.
///
/// **HOME IS ASKED OF THE LEDGER**, as the window does: two ideas of where home
/// is do not give an error, they give a list that talks about flows nobody runs.
fn default_flow_sources() -> Vec<FlowSource> {
    let home = ledger::sailor_home().unwrap_or_else(|| PathBuf::from("."));
    let working = std::env::current_dir().ok();
    let declared = std::env::var_os("SAILOR_FLOWS").map(PathBuf::from);
    system::sources(
        &home.join("flows"),
        working.as_deref(),
        declared.as_deref().map(Path::new),
    )
}

/// The tools a flow asks for, step by step.
///
/// **IT LOOKS WHERE THE EXECUTOR READS THEM**, at the top level of `with` and of
/// the values declared for a step without dependencies: that is where
/// `external_engine` looks for `tool`. Searching deeper would find the word
/// `tool` inside a prompt or a response schema and count it as a request.
fn tools_named_by(flow: &FlowFile) -> Vec<(String, String)> {
    let mut out = Vec::new();
    for step in flow.graph.steps() {
        for place in [step.with.as_ref(), flow.inputs.get(&step.id)] {
            let Some(declared) = place.and_then(|value| value.get("tool")) else {
                continue;
            };
            for tool in named_in(declared) {
                out.push((tool, step.id.clone()));
            }
        }
    }
    out.sort();
    out.dedup();
    out
}

/// The tool names a `tool` field holds: one, or every string of a list.
/// **A LIST COUNTS AS EVERY NAME IN IT**: reading only the string dropped the
/// alternatives in silence, so the flows written to be portable were the ones
/// the census went blind on.
fn named_in(declared: &Value) -> Vec<String> {
    match declared {
        Value::String(one) => vec![one.clone()],
        Value::Array(many) => many
            .iter()
            .filter_map(Value::as_str)
            .map(str::to_owned)
            .collect(),
        _ => Vec::new(),
    }
}

/// How many steps were read looking for a tool. **A REPORT THAT SAYS «NOTHING
/// IS MISSING» WITHOUT SAYING OVER HOW MANY STEPS CANNOT BE ARGUED WITH**: this
/// one read ten of fifty-three and nobody could see it from the outside.
fn steps_read(flow: &FlowFile) -> usize {
    flow.graph.steps().len()
}

/// The steps that name a binary instead of a tool.
///
/// **NOT AN ERROR, A MEASUREMENT WORTH HAVING.** A step that writes `bin` runs
/// only where that name is on the runner's path, and no list of missing tools
/// will ever see it: it is the silent way a flow stops being portable. Saying so
/// here costs one line.
fn binaries_named_by(flow: &FlowFile) -> Vec<String> {
    let mut out = Vec::new();
    for step in flow.graph.steps() {
        for place in [step.with.as_ref(), flow.inputs.get(&step.id)] {
            if place.and_then(|value| value.get("bin")).is_some() {
                out.push(step.id.clone());
            }
        }
    }
    out.sort();
    out.dedup();
    out
}

fn presence_reason(finding: &Finding) -> String {
    match &finding.presence {
        crate::Presence::Present(reason)
        | crate::Presence::Absent(reason)
        | crate::Presence::Undetermined(reason) => reason.clone(),
    }
}

/// "1 steps name" is a phrase that makes whoever writes it look broken, and
/// whoever reads a report wrong in its form stops trusting its numbers too.
fn count(quantity: usize, one: &str, many: &str) -> String {
    if quantity == 1 {
        format!("{quantity} {one}")
    } else {
        format!("{quantity} {many}")
    }
}

/// What the flows ask for, sorted by what this machine can say about it.
struct Asked<'a> {
    present: &'a [Need],
    missing: &'a [Need],
    unknown: &'a [Need],
}

/// The answer written for a person to read. It sits beside the data and not in
/// its place: the window shows this, another step takes the lists.
fn report_of(
    flows_seen: usize,
    steps_seen: usize,
    flows_broken: &[String],
    looked_in: &[String],
    asked: Asked<'_>,
    named_binaries: &[String],
) -> String {
    let Asked {
        present,
        missing,
        unknown,
    } = asked;
    let mut text = String::new();
    let _ = write!(
        text,
        "{} read in {}, {} looked at; they ask for {}.",
        count(flows_seen, "flow", "flows"),
        count(looked_in.len(), "place", "places"),
        count(steps_seen, "step", "steps"),
        count(
            present.len() + missing.len() + unknown.len(),
            "tool",
            "tools"
        )
    );
    if !flows_broken.is_empty() {
        let _ = write!(
            text,
            "\n\nWARNING: {} could not be read, so this list is partial: {}.",
            count(flows_broken.len(), "flow", "flows"),
            flows_broken.join(", ")
        );
    }
    if !present.is_empty() {
        let _ = write!(text, "\n\nHere, and these flows run here:");
        for need in present {
            let _ = write!(
                text,
                "\n  {} — {} (asked for by {})",
                need.tool,
                need.executable.as_deref().unwrap_or("found"),
                need.asked_by.join(", ")
            );
        }
    }
    if missing.is_empty() && unknown.is_empty() {
        let _ = write!(
            text,
            "\n\nNothing is missing: every tool a flow asks for is on this machine."
        );
    }
    if !missing.is_empty() {
        let _ = write!(text, "\n\nMISSING HERE, and without them these steps stop:");
        for need in missing {
            let _ = write!(
                text,
                "\n  {} — {}\n    stops: {}",
                need.tool,
                need.reason,
                need.asked_by.join(", ")
            );
            if !need.note.is_empty() {
                let _ = write!(text, "\n    where to get it: {}", need.note);
            }
        }
    }
    if !unknown.is_empty() {
        let _ = write!(
            text,
            "\n\nASKED FOR BY A FLOW AND UNKNOWN TO SAILOR — these are not fixed by installing \
             something, they are fixed by writing a descriptor:"
        );
        for need in unknown {
            let _ = write!(
                text,
                "\n  {} — asked for by {}",
                need.tool,
                need.asked_by.join(", ")
            );
        }
    }
    if !named_binaries.is_empty() {
        let _ = write!(
            text,
            "\n\n{} a binary instead of a tool ({}): they run only where that name is on the \
             runner's path, and no list like this one can notice.",
            count(named_binaries.len(), "step names", "steps name"),
            named_binaries.join(", ")
        );
    }
    let _ = write!(text, "\n\nLooked in:\n  {}", looked_in.join("\n  "));
    text
}

#[cfg(test)]
mod tests {
    use super::*;

    fn a_flow_asking(tool: Value) -> FlowFile {
        let document = json!({
            "id": "un-flusso",
            "description": "d",
            "graph": {"steps": [{
                "id": "leggi", "deps": [], "action": "external_engine",
                "max_attempts": 1, "when": null,
                "input_schema": {"type": "any"}, "output_schema": {"type": "any"},
                "with": {"tool": tool}
            }]},
            "inputs": {}
        });
        serde_json::from_value(document).expect("the flow parses")
    }

    /// **A STEP THAT OFFERS ALTERNATIVES ASKS FOR ALL OF THEM.** Reading only
    /// the string counted none, so the flows written to be portable were the
    /// ones this went blind on — and the report said nothing was missing.
    #[test]
    fn a_tool_declared_as_a_list_is_asked_for_by_every_name_in_it() {
        let asked = tools_named_by(&a_flow_asking(json!(["unmotore", "un-altro"])));
        let names: Vec<&str> = asked.iter().map(|(tool, _)| tool.as_str()).collect();
        assert_eq!(names, vec!["un-altro", "unmotore"], "asked: {asked:?}");
        assert!(asked.iter().all(|(_, step)| step == "leggi"));

        // The control: the string still counts, and nothing else does.
        assert_eq!(tools_named_by(&a_flow_asking(json!("unmotore"))).len(), 1);
        assert!(tools_named_by(&a_flow_asking(json!({"id": "unmotore"}))).is_empty());
        assert!(tools_named_by(&a_flow_asking(json!([1, true]))).is_empty());
    }

    /// A report saying nothing is missing without saying over how many steps
    /// cannot be argued with by whoever reads it.
    #[test]
    fn the_report_says_how_many_steps_it_looked_at() {
        let nothing = Asked {
            present: &[],
            missing: &[],
            unknown: &[],
        };
        let text = report_of(2, 53, &[], &["a place".to_owned()], nothing, &[]);
        assert!(text.contains("53 steps looked at"), "{text}");
    }
}
