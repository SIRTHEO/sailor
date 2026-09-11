//! The canvas writes: creating, editing and deleting a flow.
//!
//! **NO KNOWLEDGE OF THE DISK LIVES HERE ANY MORE.** Where the files are, which
//! name is safe, how a file is replaced without ever being seen half written:
//! all of it moved into `flow::system`, because `sailor flow cap` has to
//! rewrite a `.flow.json` and this module sits **outside the Rust workspace** —
//! the command line cannot call it. The two copies that would have been born of
//! that are fault 10.
//!
//! What remains is what truly belongs to the shell: taking the JSON that
//! arrives from the canvas, running it through the engine's validation
//! (`flow::FlowFile`, and with it `flow::Graph::validate`) and refusing a flow
//! that names actions the engine does not know — `registry::default_registry`,
//! the same list `sailor flow check` uses, never a copy rewritten here.
//!
//! **THAT LAST SENTENCE STOOD HERE WHILE IT WAS FALSE.** It said
//! "`actions::register_default` / `register_store`", two lines out of the
//! sixteen of the registry: the shell was its fifth copy. A rule stops being an
//! assertion and becomes a defence when a test interrogates it — here
//! `the_window_knows_every_action_the_engine_can_run`.

use flow::{ActionRegistry, FlowFile};
use std::path::Path;

use ui::gather::{default_ledger_dir, ledger_present};

/// The command the canvas calls to create or edit a flow.
#[tauri::command]
pub(crate) fn save_flow(flow: serde_json::Value) -> Result<String, String> {
    // The identifier is read here just to learn WHERE to write. Whether it is
    // valid is decided by `save_flow_in`, which holds all its own rules: an id
    // absent or not textual lands in the folder of the new flows and is refused
    // there, with the right message, rather than being refused here with a
    // worse message.
    let id = flow
        .get("id")
        .and_then(|id| id.as_str())
        .unwrap_or_default();
    let (origin, dir) = super::place_for(id);
    save_flow_in(&dir, flow).map(|()| origin.to_owned())
}

/// The command the canvas calls to delete a flow.
#[tauri::command]
pub(crate) fn delete_flow(name: String) -> Result<(), String> {
    delete_flow_in(&super::place_for(&name).1, &name)
}

/// The heart of `save_flow`, with the folder passed in rather than read from
/// the environment: the tests write into a throwaway folder, not among the real
/// flows.
///
/// A count copied into a comment ages on its own, and `docs/decisions.md`
/// forbids it for exactly that reason — where a fact is already recorded, the
/// text points at it rather than copying it. The number is said by
/// `sailor flow list`, which counts all three of the places they come from.
fn save_flow_in(flows_dir: &Path, flow_json: serde_json::Value) -> Result<(), String> {
    // Deserializing `FlowFile` invokes `Graph::try_from`, which calls
    // `Graph::validate`: cycles, missing or incompatible dependencies are
    // refused in there, never rechecked by hand.
    let flow: FlowFile = serde_json::from_value(flow_json)
        .map_err(|error| format!("the flow fails the engine's validation: {error}"))?;
    reject_unknown_actions(&flow)?;
    flow::system::save_in(flows_dir, &flow)
}

/// The heart of `delete_flow`, same reason for the folder passed by hand.
fn delete_flow_in(flows_dir: &Path, name: &str) -> Result<(), String> {
    flow::system::delete_in(flows_dir, name)
}

/// One registered action, with how the engine treats it.
///
/// **THESE ARE POLICIES, NOT CLAIMS.** Both come from a trait method with a
/// conservative default and cannot tell an author's answer from silence, so
/// what travels is how the engine behaves. The surface and the other powers of
/// `docs/the-four-surfaces.md` are absent because no action declares one.
#[derive(serde::Serialize)]
pub(crate) struct Registered {
    name: String,
    /// `repeatable`, `compensable` or `hand_to_human`: what redoing it costs.
    redo: &'static str,
    /// Whether the engine lets this action's output close a run.
    closes_a_run: bool,
}

/// Every action the engine can run, with the policy it runs under.
///
/// **THE LIST IS ASKED, NEVER WRITTEN**, and so is every field of it: a map of
/// policies kept beside the registry is the shape of fault 10, and this file
/// has already been its fifth copy once.
#[tauri::command]
pub(crate) fn engine_actions() -> Vec<Registered> {
    let registry = action_registry();
    let mut names = registry.names();
    names.sort_unstable();
    names
        .into_iter()
        .filter_map(|name| {
            registry.get(name).map(|action| Registered {
                name: name.to_owned(),
                redo: match action.species() {
                    flow::StepSpecies::Repeatable => "repeatable",
                    flow::StepSpecies::Compensable => "compensable",
                    flow::StepSpecies::HandToHuman => "hand_to_human",
                },
                closes_a_run: action.is_a_check(),
            })
        })
        .collect()
}

/// The actions the engine knows, so that saving refuses a flow a real run would
/// throw back with `unknown action`. The ledger enters the registry if it
/// exists already, and not otherwise — a static check must create none, for the
/// same reason as `sailor flow check`.
fn action_registry() -> ActionRegistry {
    let ledger_dir = default_ledger_dir();
    let ledger = ledger_present(&ledger_dir)
        .then(|| ledger::Ledger::open(&ledger_dir).ok())
        .flatten();
    action_registry_with(ledger)
}

/// The registry **with no ledger**: the shape a test can build without reading
/// the machine of whoever runs it (fault 5).
#[cfg(test)]
fn action_registry_without_deposit() -> ActionRegistry {
    action_registry_with(None)
}

/// **THERE IS ONE LIST, AND THIS USED TO BE THE FIFTH COPY.**
///
/// Three hand-picked lines lived here — `register_default`, `trigger`, and the
/// `store` when the ledger exists — while `crates/registry` registers sixteen.
/// So the window **refused at save time** five actions that do run from the
/// terminal: `detect_tools`, `tool_needs`, `history_ask`, `subflow` and — the
/// gravest — `handed_to_agent`, the step that hands the work to whoever is
/// already alive in the terminal. A central function of the product could not
/// be composed from the window.
///
/// It is fault 10 at its fifth occurrence, and this time the comment at the
/// head of the file already declared the right rule — "the same list
/// `sailor flow check` uses, never a copy rewritten here" — above the code that
/// violated it. A rule written and never verified is no defence.
fn action_registry_with(ledger: Option<ledger::Ledger>) -> ActionRegistry {
    registry::default_registry(ledger, None)
}

fn reject_unknown_actions(flow: &FlowFile) -> Result<(), String> {
    let registry = action_registry();
    let missing: Vec<&str> = flow
        .graph
        .steps()
        .iter()
        .filter(|step| registry.get(&step.action).is_none())
        .map(|step| step.action.as_str())
        .collect();
    if missing.is_empty() {
        Ok(())
    } else {
        Err(format!(
            "the flow uses actions the engine does not know: {}",
            missing.join(", ")
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::collections::BTreeSet;
    use std::fs;
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicU64, Ordering};

    static NEXT: AtomicU64 = AtomicU64::new(0);

    /// A throwaway folder for each test: it shows for itself when it stays
    /// empty, which a folder shared between tests would fail to guarantee.
    fn scratch_dir(label: &str) -> PathBuf {
        let sequence = NEXT.fetch_add(1, Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!(
            "sailor-desktop-flows-test-{label}-{}-{sequence}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).expect("the scratch folder is created");
        dir
    }

    fn valid_flow(id: &str) -> serde_json::Value {
        json!({
            "id": id,
            "description": "a flow for testing",
            "graph": {
                "steps": [{
                    "id": "lone",
                    "deps": [],
                    "action": "shell_check",
                    "max_attempts": 1,
                    "when": null,
                    "input_schema": {"type": "any"},
                    "output_schema": {"type": "any"}
                }]
            },
            "inputs": {"lone": {"command": "true", "timeout_secs": 5}}
        })
    }

    fn entries(dir: &Path) -> Vec<String> {
        fs::read_dir(dir)
            .expect("the folder is readable")
            .flatten()
            .map(|entry| entry.file_name().to_string_lossy().into_owned())
            .collect()
    }

    // ── what the shell decides for itself ───────────────────────────────
    //
    // The tests on dangerous names, case collisions, atomic replacement and
    // deletion moved into `crates/flow/src/system.rs` together with the code
    // they test — and from there `cargo test --workspace` runs them, which it
    // did not do here: this module sits outside the workspace. The one below
    // stays because it tests the **wiring**: were `save_flow_in` to stop going
    // through `flow::system::save_in`, no test over there would notice.

    #[test]
    fn save_flow_rejects_an_empty_id_and_writes_nothing() {
        let dir = scratch_dir("empty-id");
        let error = save_flow_in(&dir, valid_flow("")).expect_err("an empty id is refused");
        assert!(error.contains("empty"), "{error}");
        assert!(entries(&dir).is_empty(), "no file must appear");
    }

    // ── a graph the engine would refuse never touches the disk ──────────

    /// THE MEASURE THAT COULD HAVE COME OUT OTHERWISE: `a` and `b` depend on
    /// each other. `Graph::validate` throws that back as a cycle. Were the
    /// validation skipped (mutant: writing `text` without going through
    /// `serde_json::from_value::<FlowFile>` first), the file would show up in
    /// the folder all the same and this test would turn red.
    #[test]
    fn save_flow_rejects_a_cyclic_graph_and_writes_nothing() {
        let dir = scratch_dir("cyclic-graph");
        let cyclic = json!({
            "id": "cyclic",
            "description": "two steps waiting on each other",
            "graph": {
                "steps": [
                    {
                        "id": "a", "deps": ["b"], "action": "shell_check", "max_attempts": 1,
                        "when": null, "input_schema": {"type": "any"}, "output_schema": {"type": "any"}
                    },
                    {
                        "id": "b", "deps": ["a"], "action": "shell_check", "max_attempts": 1,
                        "when": null, "input_schema": {"type": "any"}, "output_schema": {"type": "any"}
                    }
                ]
            },
            "inputs": {}
        });
        let error = save_flow_in(&dir, cyclic).expect_err("a cyclic graph is refused");
        assert!(error.contains("fails the engine's validation"), "{error}");
        assert!(
            entries(&dir).is_empty(),
            "a refused graph must not touch the disk"
        );
    }

    #[test]
    fn save_flow_rejects_a_missing_dependency_and_writes_nothing() {
        let dir = scratch_dir("missing-dependency");
        let broken = json!({
            "id": "broken",
            "description": "depends on a step that does not exist",
            "graph": {
                "steps": [{
                    "id": "lone", "deps": ["ghost"], "action": "shell_check", "max_attempts": 1,
                    "when": null, "input_schema": {"type": "any"}, "output_schema": {"type": "any"}
                }]
            },
            "inputs": {}
        });
        let error = save_flow_in(&dir, broken).expect_err("a missing dependency is refused");
        assert!(error.contains("fails the engine's validation"), "{error}");
        assert!(entries(&dir).is_empty());
    }

    /// THE MEASURE THAT COULD HAVE COME OUT OTHERWISE: `never-registered`
    /// is neither `shell_check` nor `external_engine` nor an action of the
    /// ledger. Lacking `reject_unknown_actions` (mutant: making it always
    /// return `Ok`) the file would show up all the same, and this test turn red.
    #[test]
    fn save_flow_rejects_an_unknown_action_and_writes_nothing() {
        let dir = scratch_dir("unknown-action");
        let mut flow = valid_flow("unknown-action");
        flow["graph"]["steps"][0]["action"] = json!("never-registered");
        let error = save_flow_in(&dir, flow).expect_err("an unknown action is refused");
        assert!(error.contains("never-registered"), "{error}");
        assert!(entries(&dir).is_empty());
    }

    #[test]
    fn save_flow_accepts_the_two_actions_the_engine_always_knows() {
        let dir = scratch_dir("known-actions");
        assert!(save_flow_in(&dir, valid_flow("shell-ok")).is_ok());
        let mut with_engine = valid_flow("engine-ok");
        with_engine["graph"]["steps"][0]["action"] = json!("external_engine");
        with_engine["inputs"]["lone"] = json!({"bin": "true", "timeout_secs": 5});
        assert!(save_flow_in(&dir, with_engine).is_ok());
    }

    /// **THE WINDOW MUST BE ABLE TO SAVE ALL THE ENGINE CAN RUN.**
    ///
    /// The comment at the head of this file declares "the same list
    /// `sailor flow check` uses, never a copy rewritten here". `action_registry`
    /// was that copy instead: `register_default` plus two hand-picked lines,
    /// while the engine goes through `registry::default_registry`. It is fault
    /// 10 for the fifth time, and this time it refused at save time flows that
    /// do start from the terminal — the reverse of the twin defect, which
    /// offered in the palette actions the engine does not know.
    ///
    /// The comparison runs **with no ledger on either side**, or the test would
    /// read the state of the machine of whoever runs it (fault 5).
    #[test]
    fn the_window_knows_every_action_the_engine_can_run() {
        let engine = registry::registry_in(registry::House::empty(), None, None);
        let window = action_registry_without_deposit();
        let known: BTreeSet<&str> = window.names().into_iter().collect();
        let missing: Vec<&str> = engine
            .names()
            .into_iter()
            .filter(|name| !known.contains(name))
            .collect();
        assert!(
            missing.is_empty(),
            "the engine can run {} actions the window refuses at save time: {}",
            missing.len(),
            missing.join(", ")
        );
    }

    /// **THE PALETTE MUST OFFER NOTHING THE ENGINE WOULD REFUSE.**
    ///
    /// The twin defect of the previous one, and the one visible by hand:
    /// `ACTION_KIND` named six actions that exist in no crate at all —
    /// `pane_until_idle`, `signal_is_gone`, `deposit_write`, `pane_send`,
    /// `hand_to_human`, `pane_read` — and four of them sat in the box of the
    /// steps: pressing "wait", "ledger", "gesture" or "to a person" created a
    /// node that then failed to save.
    ///
    /// The anchor sits **outside both copies**: the window's vocabulary is read
    /// from its own file and compared against the engine's registry. Comparing
    /// two hand-written maps would let them be wrong together.
    #[test]
    fn the_window_vocabulary_names_only_actions_the_engine_registers() {
        let source =
            fs::read_to_string(PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../src/flow.ts"))
                .expect("the window's vocabulary is read from desktop/src/flow.ts");
        let named = action_names_in(&source, "const ACTION_KIND");
        assert!(
            named.len() > 4,
            "the vocabulary was not read: {} names found",
            named.len()
        );
        // With the ledger **open**, because six actions register then and not
        // before, and the window is right to draw them all the same.
        let dir = scratch_dir("vocabulary");
        let ledger = ledger::Ledger::open(&dir).expect("a ledger for testing");
        let engine = registry::registry_in(registry::House::under(&dir), Some(ledger), None);
        let known: BTreeSet<&str> = engine.names().into_iter().collect();
        let invented: Vec<&String> = named
            .iter()
            .filter(|name| !known.contains(name.as_str()))
            .collect();
        assert!(
            invented.is_empty(),
            "the window names {} actions the engine does not register: {:?}",
            invented.len(),
            invented
        );

        // And the other direction, which is how the worse defect hid itself:
        // `kindOf` falls back on "check" for a name it does not know, so the
        // seven `trigger` steps of the real flows were drawn as control nodes,
        // in silence and with nothing turning red.
        let undrawn: Vec<&str> = engine
            .names()
            .into_iter()
            .filter(|name| !named.iter().any(|k| k == name))
            .collect();
        assert!(
            undrawn.is_empty(),
            "the engine registers {} actions the window cannot draw, and that \
             would fall back on «check» in silence: {}",
            undrawn.len(),
            undrawn.join(", ")
        );
    }

    /// The action names inside a `const NAME: … = { key: value }` block of the
    /// window's source. It lives here and not in a generic reader because it is
    /// a single read and must stay readable: were the block to change shape one
    /// day, the test above fails on the minimum count rather than passing over
    /// an empty list.
    fn action_names_in(source: &str, header: &str) -> Vec<String> {
        let Some(start) = source.find(header) else {
            return Vec::new();
        };
        let body = &source[start..];
        let Some(open) = body.find('{') else {
            return Vec::new();
        };
        let Some(close) = body.find("};") else {
            return Vec::new();
        };
        body[open + 1..close]
            .lines()
            .filter_map(|line| line.split(':').next())
            .map(|name| name.trim().trim_matches('"').to_string())
            .filter(|name| !name.is_empty() && !name.starts_with("//"))
            .collect()
    }

    /// Deletion goes through the same place: were the shell to stop delegating,
    /// this line would notice.
    #[test]
    fn delete_flow_reports_a_flow_that_was_never_written() {
        let dir = scratch_dir("delete-missing");
        let error = delete_flow_in(&dir, "never-existed").expect_err("deleting an absent flow");
        assert!(error.contains("does not exist"), "{error}");
    }
}

/// **THE TEXT, AND NOT A FLOW REBUILT FROM ONE.** A flow that will not parse
/// still belongs on the map — it is called by name — and a rebuilt one drops
/// whatever the engine's own shape has no field for.
#[derive(serde::Serialize)]
pub(crate) struct FlowText {
    /// As it reads on disk, `.flow.json` and all: the name the engine resolves
    /// by is that one without the suffix, and the map takes it off itself.
    name: String,
    /// The source it came from, in the words that source gives itself.
    origin: &'static str,
    text: String,
    /// Set when the file would not open at all, which is not the same as a
    /// file whose text will not parse: there is no text either way, and only
    /// one of the two is the author's doing.
    #[serde(skip_serializing_if = "Option::is_none")]
    unreadable: Option<String>,
}

/// Every flow file the engine would look at, in the order it looks. **LEAST
/// SPECIFIC FIRST, SHADOWS AND ALL**: `load_all_flows` settles a name clash
/// before anybody sees it, and editing the copy that does not run is the fault
/// the order exists to make visible.
#[tauri::command]
pub(crate) fn flow_texts() -> Vec<FlowText> {
    ui::gather::flow_sources()
        .iter()
        .flat_map(texts_of)
        .collect()
}

fn texts_of(source: &flow::system::FlowSource) -> Vec<FlowText> {
    if source.is_builtin() {
        return flow::system::FLOWS
            .iter()
            .map(|(name, text)| FlowText {
                name: format!("{name}.flow.json"),
                origin: source.origin,
                text: (*text).to_owned(),
                unreadable: None,
            })
            .collect();
    }
    let Ok(entries) = std::fs::read_dir(&source.dir) else {
        return Vec::new();
    };
    let mut found: Vec<FlowText> = entries
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| path.extension().is_some_and(|kind| kind == "json"))
        .map(|path| read_one(&path, source.origin))
        .collect();
    found.sort_by(|one, other| one.name.cmp(&other.name));
    found
}

fn read_one(path: &Path, origin: &'static str) -> FlowText {
    let name = path
        .file_name()
        .unwrap_or_default()
        .to_string_lossy()
        .into_owned();
    match std::fs::read_to_string(path) {
        Ok(text) => FlowText {
            name,
            origin,
            text,
            unreadable: None,
        },
        Err(why) => FlowText {
            name,
            origin,
            text: String::new(),
            unreadable: Some(why.to_string()),
        },
    }
}

#[cfg(test)]
mod flow_text_tests {
    use super::*;

    /// **A MAP BUILT ON A SETTLED LIST CANNOT SEE A SHADOW.** The map's reading
    /// depends on receiving both copies of a name, in the order the engine
    /// reads them, so this asks the shipped source for its own texts and
    /// checks they arrive whole instead of parsed and rebuilt.
    #[test]
    fn every_shipped_flow_arrives_as_the_text_it_is() {
        let source = flow::system::FlowSource::builtin();
        let shipped = texts_of(&source);

        assert_eq!(
            shipped.len(),
            flow::system::FLOWS.len(),
            "the map would be drawn over fewer flows than ship"
        );
        for one in &shipped {
            assert!(
                one.name.ends_with(".flow.json"),
                "«{}» is not the name the engine resolves by",
                one.name
            );
            assert_eq!(one.origin, source.origin);
            assert!(one.unreadable.is_none());
            serde_json::from_str::<serde_json::Value>(&one.text)
                .unwrap_or_else(|why| panic!("«{}» did not arrive whole: {why}", one.name));
        }
    }

    /// A folder nobody made is not an error to draw: the engine looks in three
    /// places and two of them are commonly absent.
    #[test]
    fn a_source_with_no_folder_contributes_nothing_and_says_nothing() {
        let missing = flow::system::FlowSource {
            origin: "yours",
            dir: std::path::PathBuf::from("/nowhere/a-folder-nobody-made"),
        };

        assert!(texts_of(&missing).is_empty());
    }
}
