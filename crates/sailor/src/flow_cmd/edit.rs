//! `sailor flow edit`: changing a flow through the model the run reads. Before
//! it, every edit to a shipped flow was string surgery on JSON. See fault 15.
//!
//! **THE DOCUMENT IS EDITED, NOT REBUILT**, or the one changed line would be
//! buried under a reordering of the whole file. And it refuses what `flow
//! check` refuses, by asking [`super::check::refusals_of`] rather than a copy.

use flow::Graph;
use serde_json::{json, Map, Value};
use ui::gather::FlowSource;

use super::cap_and_schedule::{a_flow_i_may_rewrite, set_cap, set_schedule};
use super::check::refusals_of;
use super::{missing_actions, open_default_ledger};

/// The word that takes a value off instead of setting one, as everywhere else
/// a command writes into a flow.
const NONE: &str = "none";

/// The shape a step is born with: it accepts anything, runs always, and is
/// tried once. Every one of the four is the least a reader can be surprised
/// by, and each is changed by its own gesture afterwards.
fn a_new_step(id: &str, action: &str) -> Value {
    json!({
        "id": id,
        "deps": [],
        "input_schema": {"type": "any"},
        "output_schema": {"type": "any"},
        "when": null,
        "action": action,
        "max_attempts": 1
    })
}

pub(super) fn edit_flow(
    sources: &[FlowSource],
    name: &str,
    gesture: &[String],
) -> Result<String, String> {
    let words: Vec<&str> = gesture.iter().map(String::as_str).collect();
    match words.as_slice() {
        // The trigger and the cap already have a gesture that writes them, and
        // this one hands the work over rather than growing a second writer.
        ["trigger", value] => set_schedule(sources, name, value, None),
        ["trigger", value, weight] => set_schedule(sources, name, value, Some(weight)),
        ["cap", value] => set_cap(sources, name, value),
        ["add-step", step, action] => {
            rewrite(sources, name, |document| add_step(document, step, action))
        }
        ["remove-step", step] => rewrite(sources, name, |document| remove_step(document, step)),
        ["connect", from, to] => {
            rewrite(sources, name, |document| connect(document, from, to, true))
        }
        ["disconnect", from, to] => {
            rewrite(sources, name, |document| connect(document, from, to, false))
        }
        ["engines", step, chain] => {
            rewrite(sources, name, |document| engines(document, step, chain))
        }
        ["field", step, key, value] => rewrite(sources, name, |document| {
            set_field(document, step, key, value)
        }),
        _ => Err(super::usage()),
    }
}

/// Reads the flow's own document, hands it to `change`, and writes it back only
/// if the engine and every declared check accept what came out.
fn rewrite(
    sources: &[FlowSource],
    name: &str,
    change: impl FnOnce(&mut Value) -> Result<String, String>,
) -> Result<String, String> {
    let (_, source) = a_flow_i_may_rewrite(sources, name)?;
    let path = source.dir.join(format!("{name}.flow.json"));
    let shown = path.display().to_string();
    let text = std::fs::read_to_string(&path).map_err(|error| {
        catalogue::say(
            "cli.flow.file_cannot_be_read",
            &[("file", &shown), ("error", &error.to_string())],
        )
    })?;
    let mut document: Value = serde_json::from_str(&text).map_err(|error| {
        catalogue::say(
            "cli.flow.file_is_not_a_flow",
            &[("file", &shown), ("error", &error.to_string())],
        )
    })?;

    let said = change(&mut document)?;
    let flow = flow::system::flow_of_document(&document)?;
    refuse_unknown_actions(&flow.graph)?;
    if let Some(refusal) = refusals_of(&flow, &registry()).into_iter().next() {
        return Err(refusal);
    }
    flow::system::save_document_in(&source.dir, &document)?;
    Ok(catalogue::say(
        "cli.flow.edited",
        &[
            ("flow", name),
            ("origin", source.origin),
            ("said", &said),
            ("directory", &source.dir.display().to_string()),
        ],
    ))
}

/// The actions the engine can run, asked of the one list that assembles them.
fn registry() -> flow::ActionRegistry {
    registry::default_registry(open_default_ledger(), None)
}

fn refuse_unknown_actions(graph: &Graph) -> Result<(), String> {
    let missing = missing_actions(graph, &registry());
    if missing.is_empty() {
        return Ok(());
    }
    Err(catalogue::say(
        "cli.flow.action_the_engine_does_not_know",
        &[
            (
                "actions",
                &missing.into_iter().collect::<Vec<_>>().join(", "),
            ),
            ("known", &registry().names().join(", ")),
        ],
    ))
}

// ── the steps of the graph ───────────────────────────────────────────────

fn steps_of(document: &mut Value) -> Result<&mut Vec<Value>, String> {
    document
        .pointer_mut("/graph/steps")
        .and_then(Value::as_array_mut)
        .ok_or_else(|| catalogue::say("cli.flow.no_steps_to_edit", &[]))
}

fn position_of(steps: &[Value], id: &str) -> Option<usize> {
    steps
        .iter()
        .position(|step| step.get("id").and_then(Value::as_str) == Some(id))
}

fn no_such_step(document: &mut Value, id: &str) -> String {
    let names = match steps_of(document) {
        Ok(steps) => steps
            .iter()
            .filter_map(|step| step.get("id").and_then(Value::as_str))
            .collect::<Vec<_>>()
            .join(", "),
        Err(_) => String::new(),
    };
    catalogue::say(
        "cli.flow.no_step_by_that_name",
        &[("step", id), ("steps", &names)],
    )
}

fn add_step(document: &mut Value, id: &str, action: &str) -> Result<String, String> {
    if position_of(steps_of(document)?, id).is_some() {
        return Err(catalogue::say(
            "cli.flow.step_already_there",
            &[("step", id)],
        ));
    }
    steps_of(document)?.push(a_new_step(id, action));
    Ok(catalogue::say(
        "cli.flow.step_added",
        &[("step", id), ("action", action)],
    ))
}

/// Takes a step out, and with it every edge that named it and the values it
/// started from: a dependency on a step that is gone stops the graph loading,
/// and an input under a name no step carries is read by nothing.
fn remove_step(document: &mut Value, id: &str) -> Result<String, String> {
    let Some(at) = position_of(steps_of(document)?, id) else {
        return Err(no_such_step(document, id));
    };
    steps_of(document)?.remove(at);
    let mut freed = Vec::new();
    for step in steps_of(document)?.iter_mut() {
        let name = step
            .get("id")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_owned();
        if let Some(deps) = step.get_mut("deps").and_then(Value::as_array_mut) {
            let before = deps.len();
            deps.retain(|dep| dep.as_str() != Some(id));
            if deps.len() != before {
                freed.push(name);
            }
        }
    }
    if let Some(inputs) = document.get_mut("inputs").and_then(Value::as_object_mut) {
        inputs.remove(id);
    }
    Ok(catalogue::say(
        "cli.flow.step_removed",
        &[("step", id), ("freed", &freed.join(", "))],
    ))
}

/// Joins one step's output to another's input, or parts them. A step receives
/// what the steps it depends on produced, so the edge *is* the connection: no
/// second field says it, and inventing one would give the run two answers.
fn connect(document: &mut Value, from: &str, to: &str, joined: bool) -> Result<String, String> {
    if position_of(steps_of(document)?, from).is_none() {
        return Err(no_such_step(document, from));
    }
    let Some(at) = position_of(steps_of(document)?, to) else {
        return Err(no_such_step(document, to));
    };
    let deps = steps_of(document)?[at]
        .get_mut("deps")
        .and_then(Value::as_array_mut)
        .ok_or_else(|| catalogue::say("cli.flow.step_without_deps", &[("step", to)]))?;
    let held = deps.iter().any(|dep| dep.as_str() == Some(from));
    if held == joined {
        return Err(catalogue::say(
            match joined {
                true => "cli.flow.already_connected",
                false => "cli.flow.not_connected",
            },
            &[("from", from), ("to", to)],
        ));
    }
    match joined {
        true => deps.push(Value::String(from.to_owned())),
        false => deps.retain(|dep| dep.as_str() != Some(from)),
    }
    Ok(catalogue::say(
        match joined {
            true => "cli.flow.connected",
            false => "cli.flow.disconnected",
        },
        &[("from", from), ("to", to)],
    ))
}

// ── what a step declares ─────────────────────────────────────────────────

/// The `with` of a step, created empty if the step declares none yet.
fn with_of<'a>(document: &'a mut Value, step: &str) -> Result<&'a mut Map<String, Value>, String> {
    let Some(at) = position_of(steps_of(document)?, step) else {
        return Err(no_such_step(document, step));
    };
    let step = &mut steps_of(document)?[at];
    if !step["with"].is_object() {
        step["with"] = json!({});
    }
    step["with"]
        .as_object_mut()
        .ok_or_else(|| catalogue::say("cli.flow.step_without_deps", &[("step", "with")]))
}

/// Sets the chain of engines a step may be run by, in the order written.
///
/// One name is written as a word and several as a list, which is the shape the
/// run already reads: writing a list of one would be a second spelling of the
/// same chain, and every reader would have to know both.
fn engines(document: &mut Value, step: &str, chain: &str) -> Result<String, String> {
    let with = with_of(document, step)?;
    let before = actions::engines_named_in(&Value::Object(with.clone())).join(" → ");
    let named: Vec<&str> = chain
        .split(',')
        .map(str::trim)
        .filter(|name| !name.is_empty())
        .collect();
    match named.as_slice() {
        [] | [NONE] => {
            with.remove("tool");
        }
        [only] => {
            with.insert("tool".to_owned(), Value::String((*only).to_owned()));
        }
        many => {
            let list = many
                .iter()
                .map(|name| Value::String((*name).to_owned()))
                .collect();
            with.insert("tool".to_owned(), Value::Array(list));
        }
    }
    let after = actions::engines_named_in(&Value::Object(with.clone())).join(" → ");
    Ok(catalogue::say(
        "cli.flow.engines_written",
        &[
            ("step", step),
            ("before", &said_chain(&before)),
            ("after", &said_chain(&after)),
        ],
    ))
}

fn said_chain(chain: &str) -> String {
    match chain.is_empty() {
        true => NONE.to_owned(),
        false => chain.to_owned(),
    }
}

/// Writes one declared field of a step — `blind`, `data`, `tree` and whatever
/// else an action reads out of `with`.
///
/// **THE SHAPE IS THE WORD'S, NOT A STRING ALWAYS.** `blind` is read with
/// `as_bool`, so `"true"` written as text is not merely wrong, it is *ignored*:
/// the step is saved looking blind and runs seeing.
fn set_field(document: &mut Value, step: &str, key: &str, value: &str) -> Result<String, String> {
    let with = with_of(document, step)?;
    let before = with.get(key).cloned();
    if value == NONE {
        with.remove(key);
    } else {
        with.insert(key.to_owned(), as_written(value));
    }
    Ok(catalogue::say(
        "cli.flow.field_written",
        &[
            ("step", step),
            ("field", key),
            ("before", &said_value(before.as_ref())),
            ("after", &said_value(with.get(key))),
        ],
    ))
}

/// A typed value from a word: the two truths and a whole number keep their
/// shape, a list or a map is read as one, everything else is text.
///
/// **A FIELD A WORD CANNOT SAY WAS WRITTEN AS THE WORD ITSELF**, and the step
/// then ignored it in silence — `args`, `model` and `max_tokens` are all one or
/// the other. Only `[` and `{` are read this way, so no bare word changes.
fn as_written(value: &str) -> Value {
    if value.starts_with('[') || value.starts_with('{') {
        if let Ok(parsed) = serde_json::from_str::<Value>(value) {
            return parsed;
        }
    }
    match value {
        "true" => Value::Bool(true),
        "false" => Value::Bool(false),
        other => match other.parse::<i64>() {
            Ok(number) => Value::from(number),
            Err(_) => Value::String(other.to_owned()),
        },
    }
}

fn said_value(value: Option<&Value>) -> String {
    match value {
        None => NONE.to_owned(),
        Some(Value::String(text)) => text.clone(),
        Some(other) => other.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::super::test_support::*;
    use super::*;
    use std::fs;
    use std::path::Path;

    /// A flow written with its keys in an order **no serialisation would
    /// produce**: `inputs` first, `id` last, and `with` holding three keys in a
    /// deliberate order. Anything that rebuilds the document instead of editing
    /// it comes back sorted differently, and the preservation test sees it.
    const AS_ITS_AUTHOR_WROTE_IT: &str = r#"{
  "inputs": {
    "misura": {
      "timeout_secs": 5,
      "command": "true"
    }
  },
  "wall_secs": 900,
  "graph": {
    "steps": [
      {
        "id": "misura",
        "deps": [],
        "input_schema": {
          "type": "any"
        },
        "output_schema": {
          "type": "any"
        },
        "when": null,
        "action": "shell_check",
        "max_attempts": 1,
        "with": {
          "workdir": ".",
          "command": "true",
          "timeout_secs": 5
        }
      },
      {
        "id": "conta",
        "deps": [],
        "input_schema": {
          "type": "any"
        },
        "output_schema": {
          "type": "any"
        },
        "when": null,
        "action": "shell_check",
        "max_attempts": 1
      }
    ]
  },
  "description": "two steps nobody joined",
  "id": "da-cambiare"
}
"#;

    fn a_home_with_the_flow() -> (TestDirectory, Vec<FlowSource>) {
        let home = TestDirectory::new();
        home.write("da-cambiare.flow.json", AS_ITS_AUTHOR_WROTE_IT);
        let sources = flow::system::sources(&home.0, None, None);
        (home, sources)
    }

    fn saved(dir: &Path) -> String {
        fs::read_to_string(dir.join("da-cambiare.flow.json")).expect("the flow is on disk")
    }

    fn edit(sources: &[FlowSource], words: &[&str]) -> Result<String, String> {
        let gesture: Vec<String> = words.iter().map(|word| (*word).to_owned()).collect();
        edit_flow(sources, "da-cambiare", &gesture)
    }

    /// **THE MEASURE THAT COULD HAVE COME OUT DIFFERENT.** Everything outside
    /// the one changed line is compared byte for byte, key order included.
    /// The mutant: have `save_document_in` write `to_string_pretty(&flow)` —
    /// the built struct — instead of the document. `id` climbs to the top,
    /// `inputs` sorts, `with` reorders, and every one of these lines moves.
    #[test]
    fn an_edit_leaves_every_line_it_did_not_touch_byte_for_byte() {
        let (home, sources) = a_home_with_the_flow();
        edit(&sources, &["connect", "misura", "conta"]).expect("the two steps join");

        let expected = AS_ITS_AUTHOR_WROTE_IT.replace(
            "\"id\": \"conta\",\n        \"deps\": [],",
            "\"id\": \"conta\",\n        \"deps\": [\n          \"misura\"\n        ],",
        );
        assert_eq!(saved(&home.0), expected);
    }

    /// The same demand said in the way a person notices it: the keys of the
    /// document are where they were, not where a struct would put them.
    #[test]
    fn the_keys_keep_the_order_their_author_gave_them() {
        let (home, sources) = a_home_with_the_flow();
        edit(&sources, &["field", "misura", "blind", "true"]).expect("the field is written");

        let document: Value = serde_json::from_str(&saved(&home.0)).expect("it loads");
        let keys: Vec<&str> = document
            .as_object()
            .expect("an object")
            .keys()
            .map(String::as_str)
            .collect();
        assert_eq!(
            keys,
            vec!["inputs", "wall_secs", "graph", "description", "id"]
        );
    }

    /// **THE MEASURE THAT COULD HAVE COME OUT DIFFERENT.** `conta` depends on
    /// `misura` after the first gesture, so the second closes a cycle.
    /// `Graph::validate` refuses it. The mutant: drop the
    /// `serde_json::from_value::<FlowFile>` in `rewrite` — the file is written
    /// with the cycle in it, and the flow stops loading at all.
    #[test]
    fn a_connection_that_closes_a_cycle_is_refused_and_nothing_is_written() {
        let (home, sources) = a_home_with_the_flow();
        edit(&sources, &["connect", "misura", "conta"]).expect("the first edge");
        let before = saved(&home.0);

        let error = edit(&sources, &["connect", "conta", "misura"]).expect_err("a cycle");
        assert!(error.contains("validation"), "{error}");
        assert_eq!(
            saved(&home.0),
            before,
            "a refused graph does not touch the disk"
        );
    }

    /// **THE MEASURE THAT COULD HAVE COME OUT DIFFERENT.** A step declared
    /// blind while it asks to continue a session is exactly what `flow check`
    /// refuses. The mutant: take the `refusals_of` call out of `rewrite` — the
    /// flow is saved, and the next `sailor flow check` on it exits one.
    #[test]
    fn what_flow_check_refuses_is_refused_here_too() {
        let (home, sources) = a_home_with_the_flow();
        edit(&sources, &["field", "misura", "session", "continue"]).expect("the session is asked");
        let before = saved(&home.0);

        let error =
            edit(&sources, &["field", "misura", "blind", "true"]).expect_err("both at once");
        assert!(error.contains("blind"), "{error}");
        assert_eq!(saved(&home.0), before);
    }

    #[test]
    fn a_step_is_added_and_removed_and_the_edges_go_with_it() {
        let (home, sources) = a_home_with_the_flow();
        edit(&sources, &["add-step", "chiudi", "shell_check"]).expect("the step is born");
        edit(&sources, &["connect", "chiudi", "conta"]).expect("the edge");
        let document: Value = serde_json::from_str(&saved(&home.0)).expect("it loads");
        assert_eq!(document["graph"]["steps"][2]["id"], "chiudi");
        assert_eq!(document["graph"]["steps"][1]["deps"][0], "chiudi");

        edit(&sources, &["remove-step", "chiudi"]).expect("and it goes");
        let document: Value = serde_json::from_str(&saved(&home.0)).expect("it still loads");
        assert_eq!(
            document["graph"]["steps"].as_array().expect("steps").len(),
            2
        );
        assert!(
            document["graph"]["steps"][1]["deps"]
                .as_array()
                .expect("deps")
                .is_empty(),
            "an edge to a step that is gone stops the graph loading"
        );
    }

    /// A name the engine does not register is refused before the disk, with
    /// the list of what it does know: the same demand the window holds.
    #[test]
    fn a_step_of_an_action_nobody_registered_is_refused() {
        let (home, sources) = a_home_with_the_flow();
        let before = saved(&home.0);
        let error = edit(&sources, &["add-step", "ignoto", "mai-registrata"]).expect_err("refused");
        assert!(error.contains("mai-registrata"), "{error}");
        assert_eq!(saved(&home.0), before);
    }

    /// One name is a word, several are a list, and the word that takes the
    /// chain off leaves no `tool` behind.
    #[test]
    fn a_chain_is_written_in_the_shape_the_run_reads() {
        let (home, sources) = a_home_with_the_flow();
        edit(&sources, &["engines", "conta", "primo"]).expect("one engine");
        let document: Value = serde_json::from_str(&saved(&home.0)).expect("it loads");
        assert_eq!(document["graph"]["steps"][1]["with"]["tool"], "primo");

        edit(&sources, &["engines", "conta", "primo,secondo"]).expect("a chain");
        let document: Value = serde_json::from_str(&saved(&home.0)).expect("it loads");
        assert_eq!(
            actions::engines_named_in(&document["graph"]["steps"][1]["with"]),
            vec!["primo", "secondo"]
        );

        edit(&sources, &["engines", "conta", NONE]).expect("and off");
        let document: Value = serde_json::from_str(&saved(&home.0)).expect("it loads");
        assert!(document["graph"]["steps"][1]["with"].get("tool").is_none());
    }

    /// `blind` is read with `as_bool`: written as text the step would be saved
    /// looking blind and run seeing.
    #[test]
    fn a_declared_truth_is_written_as_a_truth_and_not_as_text() {
        let (home, sources) = a_home_with_the_flow();
        edit(&sources, &["field", "conta", actions::BLIND, "true"]).expect("blind");
        let document: Value = serde_json::from_str(&saved(&home.0)).expect("it loads");
        assert_eq!(
            document["graph"]["steps"][1]["with"][actions::BLIND],
            json!(true)
        );

        edit(
            &sources,
            &["field", "conta", actions::TREE, actions::A_TREE_OF_ITS_OWN],
        )
        .expect("a tree of its own");
        let document: Value = serde_json::from_str(&saved(&home.0)).expect("it loads");
        assert_eq!(
            document["graph"]["steps"][1]["with"][actions::TREE],
            actions::A_TREE_OF_ITS_OWN
        );
    }

    /// A shipped flow lives inside the binary: the way to change it is a
    /// namesake at home, and the command says so instead of writing a twin.
    #[test]
    fn a_shipped_flow_is_not_edited_and_no_file_is_born() {
        let home = TestDirectory::new();
        let sources = flow::system::sources(&home.0, None, None);
        let shipped = flow::system::FLOWS
            .first()
            .expect("at least one shipped flow")
            .0;
        let gesture = vec!["cap".to_owned(), NONE.to_owned()];
        let error = edit_flow(&sources, shipped, &gesture).expect_err("not rewritten");
        assert!(error.contains(shipped), "{error}");
        assert!(
            fs::read_dir(&home.0).expect("the home").next().is_none(),
            "editing a shipped flow must not write a namesake by surprise"
        );
    }

    /// The edit rewrites the file it read and creates none: `flows/` in a
    /// repository is guarded by a judge about what may live there, and a
    /// command that grew a file would walk straight past it.
    #[test]
    fn an_edit_writes_the_file_it_read_and_no_other() {
        let (home, sources) = a_home_with_the_flow();
        edit(&sources, &["add-step", "terzo", "shell_check"]).expect("a step");
        let mut names: Vec<String> = fs::read_dir(&home.0)
            .expect("the home")
            .flatten()
            .map(|entry| entry.file_name().to_string_lossy().into_owned())
            .collect();
        names.sort();
        assert_eq!(names, vec!["da-cambiare.flow.json"]);
    }

    #[test]
    fn a_step_nobody_wrote_is_named_along_with_the_ones_that_are_there() {
        let (_home, sources) = a_home_with_the_flow();
        let error = edit(&sources, &["connect", "misura", "fantasma"]).expect_err("no such step");
        assert!(
            error.contains("fantasma") && error.contains("conta"),
            "{error}"
        );
    }

    /// The gesture that writes the trigger is the one `sailor flow schedule`
    /// already uses: if this stopped handing the work over, the two would
    /// write the same field in two ways.
    #[test]
    fn the_trigger_is_written_by_the_gesture_that_already_writes_it() {
        let (home, sources) = a_home_with_the_flow();
        edit(&sources, &["trigger", "3600s", "light"]).expect("the trigger is written");
        let document: Value = serde_json::from_str(&saved(&home.0)).expect("it loads");
        assert_eq!(document["schedule"]["recurrence"]["seconds"], 3600);
        assert_eq!(document["schedule"]["weight"], "light");
    }

    /// **A FIELD THAT IS A LIST OR A MAP IS WRITTEN AS ONE.** Written as the
    /// word itself it looked accepted and the step ignored it: `model` is a map
    /// from engine to model, and a string there is a model nobody asks for.
    #[test]
    fn a_field_that_is_not_a_word_is_written_as_what_it_is() {
        let (home, sources) = a_home_with_the_flow();
        edit(&sources, &["field", "conta", "args", r#"["test", "--quiet"]"#])
            .expect("the list is written");
        edit(&sources, &["field", "conta", "model", r#"{"un-motore": "un-modello"}"#])
            .expect("the map is written");
        edit(&sources, &["field", "conta", "tool", "npm"]).expect("the word is written");
        let document: Value = serde_json::from_str(&saved(&home.0)).expect("it loads");
        let with = &document["graph"]["steps"][1]["with"];
        assert_eq!(with["args"], serde_json::json!(["test", "--quiet"]));
        assert_eq!(with["model"], serde_json::json!({"un-motore": "un-modello"}));
        assert_eq!(with["tool"], "npm", "and a word is still a word");
    }
}
