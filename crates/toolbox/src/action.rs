//! Detection as a flow step.
//!
//! SAME SHAPE AS THE ACTIONS ALREADY REGISTERED, and it does not touch
//! `crates/actions`: a `flow::ActionRegistry` accepts anyone implementing the
//! trait, and whoever composes the registry decides what to put in it. The
//! wiring is one single line, at the point where the registry is built.

use crate::{default_sources, detect, Catalog, Machine, Source};
use flow::{Action, ActionError, ActionOutcome, SharedState, StepSpecies};
use serde::Deserialize;
use serde_json::{json, Value};
use std::path::PathBuf;

/// The name the action registers under: a stable name, an input read from JSON,
/// an output that is data — the same three shapes the other actions have.
pub const DETECT_TOOLS_ACTION: &str = "detect_tools";

/// Registers the action under its stable name.
pub fn register_default(registry: &mut flow::ActionRegistry) {
    registry.register(DETECT_TOOLS_ACTION, DetectToolsAction);
}

#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct DetectSpec {
    /// Descriptor files or directories to use instead of the usual sources.
    /// Empty means: the shipped ones plus the user's.
    #[serde(default)]
    descriptor_paths: Vec<String>,
    /// When set, the declared paths add to the usual sources instead of
    /// replacing them. A step that wants only its own descriptors sets
    /// `include_defaults: false`.
    #[serde(default = "yes")]
    include_defaults: bool,
    /// Which shipped catalogs to use, by name: today `tools` and `automations`.
    /// Empty means none *extra* — the tools catalog already arrives through
    /// `include_defaults`. A name no catalog carries becomes a problem in the
    /// output, not an empty list.
    #[serde(default)]
    builtin_catalogs: Vec<String>,
    /// One family only: `ai_cli`, `mcp_server`, `tool`, or any other word
    /// written in a descriptor.
    #[serde(default)]
    family: Option<String>,
    /// Whether a binary may be run to ask it its version. A flow running where
    /// execution is expensive switches this off, and every version becomes "not
    /// asked" instead of becoming false.
    #[serde(default = "yes")]
    version_probes: bool,
}

fn yes() -> bool {
    true
}

/// Answers "what can I use here?" from the descriptors and from the machine.
///
/// WHAT IS NOT AN ERROR OF THE ACTION: a missing tool, a binary that does not
/// answer, a badly written descriptor — all facts about the world, and they go
/// into the output. It fails only when its own input, written by whoever wrote
/// the step, cannot be read.
pub struct DetectToolsAction;

impl Action for DetectToolsAction {
    fn execute(&self, input: &Value, _shared: &SharedState) -> Result<ActionOutcome, ActionError> {
        // A missing input counts as an empty one: the commonest case — "tell me
        // what is here" — must not force writing an options object.
        let spec: DetectSpec = if input.is_null() {
            DetectSpec {
                include_defaults: true,
                version_probes: true,
                ..DetectSpec::default()
            }
        } else {
            serde_json::from_value(input.clone())
                .map_err(|error| ActionError::new("invalid_input", error.to_string()))?
        };
        let mut machine = Machine::current();
        machine.version_probes = spec.version_probes;
        let mut sources: Vec<Source> = if spec.include_defaults {
            default_sources(&machine)
        } else {
            Vec::new()
        };
        for name in &spec.builtin_catalogs {
            sources.push(Source::BuiltinNamed(name.clone()));
        }
        for raw in &spec.descriptor_paths {
            let path = PathBuf::from(machine.expand(raw));
            if path.is_dir() {
                sources.push(Source::Dir(path));
            } else {
                sources.push(Source::File(path));
            }
        }
        let catalog = Catalog::load(&sources);
        let report = detect(&catalog, &machine);
        let findings = match &spec.family {
            Some(family) => report
                .of_family(family)
                .into_iter()
                .cloned()
                .collect::<Vec<_>>(),
            None => report.findings.clone(),
        };
        let present = findings.iter().filter(|f| f.presence.is_present()).count();
        Ok(ActionOutcome::Went(json!({
            "findings": findings,
            "problems": report.problems,
            "looked_in": report.looked_in,
            "present": present,
            "total": findings.len(),
        })))
    }

    /// Redoing a detection is safe: it reads the world and says how it is. The
    /// only thing it runs is the version command a descriptor declares, and that
    /// field's contract is that it be a question, not a gesture — whoever puts a
    /// command in there that changes something has already broken the contract,
    /// and had broken it just as much with no interruption in the middle.
    fn species(&self) -> StepSpecies {
        StepSpecies::Repeatable
    }
}
