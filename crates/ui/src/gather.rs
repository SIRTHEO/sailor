//! The bridge between the ledger on disk and the pure counts. `Ledger::open`
//! creates the directory and the two `.db` files when they are missing, so
//! opening it merely to look would leave a trace that no flow ever produced.
//! Hence the check that the ledger already exists before it gets opened.

use crate::parse::{parse_model_calls, parse_runs};
use crate::registry::FlowRegistry;
use flow::StepRecord;
use ledger::{Ledger, ModelCallRecord, RunRecord};
use std::collections::BTreeMap;
use std::fmt;
use std::path::{Path, PathBuf};

pub struct GatherError(String);

/// **`.expect()` PRINTS THE `Debug`, NOT THE `Display`.** A derived one turned
/// the sentence into `GatherError("…")`, quotes and escapes included.
impl fmt::Debug for GatherError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(self, formatter)
    }
}

impl fmt::Display for GatherError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{}", self.0)
    }
}

impl std::error::Error for GatherError {}

pub struct GatheredData {
    pub runs: Vec<RunRecord>,
    pub steps_by_run: BTreeMap<String, Vec<StepRecord>>,
    pub calls_by_run: BTreeMap<String, Vec<ModelCallRecord>>,
}

/// True only when `state.db` and `events.db` already exist: the sign that
/// something really ran, rather than that somebody merely looked.
pub fn ledger_present(dir: &Path) -> bool {
    dir.join("state.db").exists() && dir.join("events.db").exists()
}

pub fn gather(dir: &Path) -> Result<Option<GatheredData>, GatherError> {
    if !ledger_present(dir) {
        return Ok(None);
    }
    let ledger = Ledger::open(dir).map_err(|error| GatherError(error.to_string()))?;
    let dump = ledger
        .projection_dump()
        .map_err(|error| GatherError(error.to_string()))?;
    let runs = parse_runs(&dump);
    let calls = parse_model_calls(&dump);

    let mut steps_by_run = BTreeMap::new();
    for run in &runs {
        let steps = ledger
            .steps(&run.run_id)
            .map_err(|error| GatherError(error.to_string()))?;
        steps_by_run.insert(run.run_id.clone(), steps);
    }

    let mut calls_by_run: BTreeMap<String, Vec<ModelCallRecord>> = BTreeMap::new();
    for call in calls {
        calls_by_run
            .entry(call.run_id.clone())
            .or_default()
            .push(call);
    }

    Ok(Some(GatheredData {
        runs,
        steps_by_run,
        calls_by_run,
    }))
}

/// Reads the flows of one source. A broken flow enters the registry with its
/// reason rather than vanishing — the reason sits in `flow::system`.
///
/// **THE SYSTEM SOURCE PASSES HERE TOO, AND IT IS NOT A DIRECTORY.** Flows
/// shipped with the product live inside the binary; `read_dir` would give zero,
/// and a caller would print «system: 0 flows» beside system flows that run.
pub fn load_flow_registry(dir: &Path) -> FlowRegistry {
    // The recognition sits here and not in the callers because there is more
    // than one of them — the window counts the entries of every source — and a
    // forgotten branch out there is invisible.
    if flow::system::is_place(dir) {
        return flow::system::builtin_registry();
    }
    flow::system::load_registry(dir)
}

/// Sailor's home: flows, ledger, configuration. **A PRODUCT THAT KNOWS ONE
/// HOME IS NO PRODUCT**: the two functions below once named the folders of
/// whoever develops Sailor and fell back to his username, so anyone installing
/// it carried another machine along. **AND THE DISCOVERY LIVES IN `ledger`**:
/// the ledger is opened by whoever runs the flows, and a second idea of where
/// home sits has the two look elsewhere with neither of them reporting a fault.
pub fn sailor_home() -> PathBuf {
    // Home is discovered the way any program on this system discovers it, and
    // this machine goes back to being a configured case that declares
    // `SAILOR_HOME` in the command opening the window. The last rung is the
    // working directory and not an invented path: with no `HOME` the least
    // wrong place is where the program was started, and it is noticed at once —
    // a plausible path belonging to someone else makes the data look lost.
    ledger::sailor_home().unwrap_or_else(|| PathBuf::from("."))
}

/// Where the ledger lives: the events and the projection of the runs.
///
/// `SAILOR_LEDGER` moves it on its own, for anyone keeping state elsewhere — a
/// different disk, a synced folder, a ledger shared between two machines.
pub fn default_ledger_dir() -> PathBuf {
    ledger::default_directory().unwrap_or_else(|| sailor_home().join("ledger"))
}

/// Where the declared flows live: Sailor's home, under `flows/`. `SAILOR_FLOWS`
/// moves them, for developers who keep them in the source tree.
///
/// **THE LEDGER IS STATE, THE FLOWS ARE SOURCES.** Keeping them under a single
/// root made them look like the same thing, which is the reason for the mix-up
/// the body records: the window answered `"flows": []` and nobody knew why.
pub fn default_flows_dir() -> PathBuf {
    // The fault this closes: it looked for the flows **beside the ledger**, at
    // `<ledger_dir>/flows`, a folder that has never existed, while the fourteen
    // real ones sat in the source tree. The empty list was no error to read: it
    // was the exact answer to a question asked in the wrong place.
    flows_dir_from(
        std::env::var_os("SAILOR_FLOWS").map(PathBuf::from),
        sailor_home(),
    )
}

/// Where a flow comes from: the type lives in the flow crate, because it is
/// never only the window asking where flows live — a step asks too, and two
/// answers to one question are the fault `crates/flow/src/file.rs` records
/// having already paid for on the file format.
pub use flow::system::FlowSource;

/// Every place flows are searched for, in the order they are searched.
///
/// **THREE, AND HAVING ONLY ONE WAS THE FAULT.** Home holds the flows of
/// whoever uses Sailor, valid wherever they go; a project holds the flows of
/// that project alone, which go with it and concern nobody else; system flows
/// ship inside the binary, so a clean install already finds some.
pub fn flow_sources() -> Vec<FlowSource> {
    // The window showed «no flows» while the command line ran four: one looked
    // in the user's home, the other in `flows/` under the working directory.
    // Neither of the two was wrong on its own — what was wrong is that there
    // was only one, because the two places serve two different purposes.
    let declared = std::env::var_os("SAILOR_FLOWS").map(PathBuf::from);
    let working = std::env::current_dir().ok();
    // Least specific first — system < yours < the project's — so whoever wants
    // a different shipped flow writes one under the same name in their own home
    // or their own project, and theirs wins. The rule is the order, and the why
    // of the order lives in `flow::system`.
    flow::system::sources(
        &sailor_home().join("flows"),
        working.as_deref(),
        declared.as_deref().map(Path::new),
    )
}

/// The flows of every source, each with the origin it came from.
///
/// On a name clash the last source wins, that is the most specific one, and the
/// origin stays visible on every row: a silent substitution leaves people
/// believing they edited a flow that is not the one running.
pub fn load_all_flows(
    sources: &[FlowSource],
) -> Vec<(String, &'static str, Result<flow::FlowFile, String>)> {
    flow::system::load_all(sources)
}

/// The choice, without the environment: this is what tests exercise.
///
/// Environment variables are global to the process and tests run in parallel
/// inside it, so a test that wrote one would wreck the others at random and the
/// red would point at the wrong module. An empty string counts as «unset» —
/// what a script exporting a variable with no value leaves behind.
fn flows_dir_from(explicit: Option<PathBuf>, home: PathBuf) -> PathBuf {
    explicit
        .filter(|path| !path.as_os_str().is_empty())
        .unwrap_or_else(|| home.join("flows"))
}

#[cfg(test)]
mod flow_sources_tests {
    use super::*;

    /// THE SOURCE THAT MAKES SAILOR A PRODUCT, and the test sits here because
    /// this is where the window asks for it: on a freshly installed machine,
    /// with nobody having copied anything, there are flows. Precedence and
    /// override rules are tested in `flow::system`, where they live.
    #[test]
    fn the_window_always_sees_the_shipped_flows_first() {
        let sources = flow_sources();
        // The literal, not the constant: `FlowSource::builtin()` sets `origin`
        // from `BUILTIN_ORIGIN`, so comparing the two asks the value whether it
        // equals itself and cannot fail. The constant's own doc asks for the
        // literal, so the label moves in one edit with every reader of it.
        assert_eq!(sources[0].origin, "built in");
        assert!(sources[0].is_builtin());
        assert!(
            !load_flow_registry(&sources[0].dir).is_empty(),
            "the system source is not a directory and must be read from the binary"
        );
    }

    /// THE NUMBER THE WINDOW SHOWS BESIDE EACH SOURCE comes through
    /// `load_flow_registry`. Were the system source to answer zero, a reader
    /// would see «system: 0 flows» beside system flows that are running, with
    /// no way to tell that the count is wrong rather than the list.
    #[test]
    fn counting_the_system_source_gives_the_shipped_flows() {
        assert_eq!(
            load_flow_registry(&FlowSource::builtin().dir).len(),
            flow::system::FLOWS.len()
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::fs;

    fn temp_test_dir(label: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "sailor-ui-gather-test-{label}-{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).expect("creating the temporary directory");
        dir
    }

    #[test]
    fn load_flow_registry_loads_valid_flow_file_with_declarative_schema() {
        let dir = temp_test_dir("valid-flow");
        let flow_content = json!({
            "id": "mio-flusso",
            "description": "A valid test flow",
            "graph": {
                "steps": [{
                    "id": "passo-uno",
                    "deps": [],
                    "action": "shell_check",
                    "max_attempts": 1,
                    "when": null,
                    "input_schema": {"type": "any"},
                    "output_schema": {"type": "any"}
                }],
                "skippable_dependencies": []
            },
            "inputs": {
                "passo-uno": {"command": "echo ok"}
            }
        });
        fs::write(
            dir.join("mio-flusso.flow.json"),
            serde_json::to_string(&flow_content).unwrap(),
        )
        .expect("writing the flow file");

        let registry = load_flow_registry(&dir);
        assert_eq!(registry.len(), 1);
        let entry = registry.get("mio-flusso").expect("entry present");
        let flow = entry.as_ref().expect("valid flow");
        assert_eq!(flow.id, "mio-flusso");
        assert_eq!(flow.description, "A valid test flow");
        assert_eq!(flow.graph.steps().len(), 1);
        assert_eq!(flow.graph.steps()[0].id, "passo-uno");

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn load_flow_registry_records_broken_flow_with_reason_instead_of_silently_skipping() {
        let dir = temp_test_dir("broken-flow");
        // Invalid JSON: the syntax stops halfway.
        fs::write(
            dir.join("flusso-tronco.flow.json"),
            r#"{"id": "flusso-tronco", "description": "#,
        )
        .expect("writing the truncated file");

        // A file with a cycle in its graph.
        let cyclic_flow = json!({
            "id": "flusso-ciclico",
            "description": "A flow with a circular dependency",
            "graph": {
                "steps": [
                    {
                        "id": "a",
                        "deps": ["b"],
                        "action": "test",
                        "max_attempts": 1,
                        "when": null,
                        "input_schema": {"type": "any"},
                        "output_schema": {"type": "any"}
                    },
                    {
                        "id": "b",
                        "deps": ["a"],
                        "action": "test",
                        "max_attempts": 1,
                        "when": null,
                        "input_schema": {"type": "any"},
                        "output_schema": {"type": "any"}
                    }
                ],
                "skippable_dependencies": []
            },
            "inputs": {}
        });
        fs::write(
            dir.join("flusso-ciclico.flow.json"),
            serde_json::to_string(&cyclic_flow).unwrap(),
        )
        .expect("writing the cyclic file");

        let registry = load_flow_registry(&dir);
        // Both used to be skipped in silence, leaving registry.len() at 0.
        assert_eq!(
            registry.len(),
            2,
            "both broken flows must be in the registry"
        );

        let truncated = registry
            .get("flusso-tronco")
            .expect("truncated flow present");
        assert!(
            truncated.is_err(),
            "the truncated file must be marked as an error"
        );
        let reason_truncated = truncated.as_ref().unwrap_err();
        // The reason must say **what is wrong**, not merely name a file: a
        // reason made of nothing but a path would pass a check on the path.
        assert!(
            reason_truncated.contains("is not a valid flow"),
            "reason: {reason_truncated}"
        );

        let cyclic = registry.get("flusso-ciclico").expect("cyclic flow present");
        assert!(
            cyclic.is_err(),
            "the flow with a cycle must be marked as an error"
        );
        let reason_cyclic = cyclic.as_ref().unwrap_err();
        assert!(
            reason_cyclic.contains("backward dependency")
                || reason_cyclic.contains("is not a valid flow"),
            "reason: {reason_cyclic}"
        );

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn load_flow_registry_rejects_naked_graph_format_with_reason() {
        let dir = temp_test_dir("naked-graph");
        let naked = json!({
            "steps": [{
                "id": "nudo",
                "deps": [],
                "action": "test",
                "max_attempts": 1,
                "when": null,
                "input_schema": {"type": "any"},
                "output_schema": {"type": "any"}
            }]
        });
        fs::write(
            dir.join("vecchio-grafo.json"),
            serde_json::to_string(&naked).unwrap(),
        )
        .expect("writing the flow file");

        let registry = load_flow_registry(&dir);
        assert_eq!(registry.len(), 1);
        let entry = registry.get("vecchio-grafo").expect("entry present");
        assert!(
            entry.is_err(),
            "the old bare-graph format, lacking {{ id, description, graph, inputs }}, must be refused with a reason"
        );

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn load_flow_registry_ignores_non_json_files() {
        let dir = temp_test_dir("non-json");
        fs::write(dir.join("README.md"), "Documentation").expect("writing the file");
        fs::write(dir.join(".DS_Store"), "binary data").expect("writing the file");

        let registry = load_flow_registry(&dir);
        assert!(
            registry.is_empty(),
            "files that are not JSON must stay out of the registry"
        );

        let _ = fs::remove_dir_all(&dir);
    }

    /// THE FAULT THIS TEST EXISTS TO CATCH: the window looked for flows beside
    /// the ledger and answered `"flows": []` with no error. Flows live in
    /// Sailor's home; the ledger is another thing and sits beside them, never
    /// above them.
    #[test]
    fn the_flows_live_in_their_own_folder_not_inside_the_ledger() {
        let chosen = flows_dir_from(None, PathBuf::from("/home/sailor"));
        assert_eq!(chosen, PathBuf::from("/home/sailor/flows"));
        assert!(
            !chosen.starts_with("/home/sailor/ledger"),
            "the flows folder must not sit inside the ledger: {}",
            chosen.display()
        );
    }

    /// The explicit rung wins: whoever names the flows folder does not want it
    /// deduced from home. It serves Sailor's own developers, who keep the flows
    /// in the source tree while home sits elsewhere.
    #[test]
    fn the_explicit_folder_wins_over_the_home() {
        assert_eq!(
            flows_dir_from(
                Some(PathBuf::from("/here/the/flows")),
                PathBuf::from("/home/sailor")
            ),
            PathBuf::from("/here/the/flows")
        );
    }

    /// A variable exported with no value is no path: taken literally it would
    /// send the search to the root of the disk, and `read_dir` on `/flows`
    /// fails in silence, giving an empty list again — the same fault it started
    /// from, with another cause.
    #[test]
    fn an_empty_variable_counts_as_unset() {
        assert_eq!(
            flows_dir_from(Some(PathBuf::new()), PathBuf::from("/home/sailor")),
            PathBuf::from("/home/sailor/flows")
        );
    }

    /// WHOEVER READS THE LEDGER MUST LOOK WHERE ITS WRITERS WRITE. The window
    /// and whoever runs the flows must ask the same function for home: two
    /// ideas of where the ledger lives give no error, they give a window saying
    /// «no runs» while the runs are there.
    #[test]
    fn the_window_asks_the_ledger_where_the_ledger_lives() {
        let declared = std::env::temp_dir().join(format!("sailor-ledger-here-{}", std::process::id()));
        std::env::set_var("SAILOR_LEDGER", &declared);
        let asked = default_ledger_dir();
        assert_eq!(asked, declared);
        assert_eq!(asked, ledger::default_directory().expect("a declared ledger is a directory"));
    }
}
