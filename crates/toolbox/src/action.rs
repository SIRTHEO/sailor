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

/// L'ingresso del passo.
///
/// **LO SCHEMA RESTA CHIUSO**, e un campo in più costa una riga qui: `familia`
/// al posto di `family` deve restare un errore detto a chi ha scritto il passo,
/// non un filtro che sparisce in silenzio. La prova
/// `the_flow_action_rejects_an_input_it_cannot_read` tiene ferma quella metà.
///
/// **PERÒ CHI COMPONE L'INGRESSO NON È SOLO CHI SCRIVE IL FLUSSO**: l'esecutore
/// aggiunge il `workdir` a ogni passo il cui schema dichiarato lo accetterebbe,
/// e `{"type": "any"}` accetta tutto. Guasto misurato il 01/09/2026 sul flusso
/// spedito `what-this-machine-has`, che dichiara proprio quello: dentro
/// una cartella con `sailor.json` moriva sempre — `unknown field 'workdir'`,
/// `failure_class: invalid_input` — e fuori da un progetto girava, perché senza
/// radice non c'è niente da offrire. Un campo non dichiarato qui non è «un
/// campo che nessuno usa»: può essere l'esecutore stesso.
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
    /// La cartella da cui contare i `descriptor_paths` scritti relativi.
    ///
    /// **NON LA SCRIVE CHI FA IL FLUSSO: LA METTE L'ESECUTORE**, ed è la radice
    /// del progetto. Dichiararla qui la rende un dato invece che un campo
    /// tollerato e buttato via: un descrittore scritto `.sailor/tools.d/x.json`
    /// si legge dalla radice del progetto e non da dove sta il processo, che è
    /// il guasto 25. Assente — flusso lanciato fuori da un progetto — un
    /// percorso relativo resta relativo, com'era prima.
    #[serde(default)]
    workdir: Option<String>,
}

fn yes() -> bool {
    true
}

/// A descriptor path, counted from the right directory.
///
/// Expansion comes first: `~/x` is absolute even though it does not start with
/// `/`, and joining it to a root would make a plausible, wrong path. A variable
/// that does not exist stays written as it is (see `Machine::expand`) and stays
/// relative — better a file not found with its name in plain sight than one
/// found by accident somewhere else.
fn rooted(machine: &Machine, workdir: Option<&str>, raw: &str) -> PathBuf {
    let expanded = PathBuf::from(machine.expand(raw));
    if expanded.is_absolute() {
        return expanded;
    }
    match workdir {
        Some(root) => PathBuf::from(machine.expand(root)).join(expanded),
        None => expanded,
    }
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
            let path = rooted(&machine, spec.workdir.as_deref(), raw);
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
