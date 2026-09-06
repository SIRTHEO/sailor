//! `sailor flow relocate`: strips a tree's prefix from the paths a flow wrote down.

use flow::FlowFile;
use serde_json::Value;
use std::path::{Path, PathBuf};
use ui::gather::FlowSource;

use super::hazards::{hardcoded_paths, POSITION_FIELDS};
use super::run_and_resume::workspace_root;

/// Strips from a flow the absolute paths that sit under the root.
///
/// **WHY A COMMAND AND NOT A SCRIPT.** It is fault 15: a flow's trigger was
/// once changed with a Python script that rewrote the JSON, because
/// `sailor flow` had only `list`, `due`, `check` and `run`. A tool people work
/// around records nothing of what happens next to it.
///
/// **IT REWRITES FIELDS, NOT PROMPTS.** A `workdir` is a field: its value means
/// something only to the program, and replacing it is a translation. Prompt
/// text is an instruction written by a person for another intelligence:
/// rewriting it rewrites the instruction, and nobody asked this command to do
/// that. Those it merely **prints**.
///
/// **THE PREFIX CAN BE DECLARED, BECAUSE THE NORMAL CASE IS ANOTHER TREE.** A
/// flow to be moved almost always names the copy it was written on — another
/// clone, or somebody else's machine — and that path **does not sit under** the
/// root of whoever is moving it. Unsaid, the command cannot know whether
/// `/Users/someone/project` meant «the root» or a real directory that must stay
/// put: guessing here would rewrite a legitimate path. It is declared as the
/// second argument — positional like `run`'s mandate, which is the shape of
/// this command line — and what does not match shows in the report.
pub(super) fn relocate_flow(sources: &[FlowSource], name: &str, from: Option<&str>) -> Result<String, String> {
    let root = workspace_root().ok_or_else(|| {
        catalogue::say(
            "cli.flow.no_workspace_root_here",
            &[("marker", flow::workspace::MARKER)],
        )
    })?;
    // The prefix to strip: the declared one, or the root itself when the flow
    // was written right here.
    let old_root = from.map(PathBuf::from).unwrap_or_else(|| root.clone());
    let path = flow_file_path(sources, name)?;
    let text = std::fs::read_to_string(&path).map_err(|error| {
        catalogue::say(
            "cli.flow.cannot_read_file",
            &[("path", &path.display().to_string()), ("error", &error.to_string())],
        )
    })?;
    // Work on the raw document, not the typed `FlowFile`: a flow may carry
    // fields this version does not know, and rewriting it from the type would
    // lose them in silence — fault 8 applied to a user's file.
    let mut document: Value = serde_json::from_str(&text).map_err(|error| {
        catalogue::say(
            "cli.flow.not_valid_json",
            &[("path", &path.display().to_string()), ("error", &error.to_string())],
        )
    })?;

    let (mut moved, mut left_alone) = relocate_workdirs(&mut document, &old_root).ok_or_else(|| {
        catalogue::say(
            "cli.flow.no_steps_to_relocate",
            &[("path", &path.display().to_string())],
        )
    })?;
    let (from_inputs, kept) = relocate_declared_inputs(&mut document, &old_root);
    moved.extend(from_inputs);
    left_alone.extend(kept);

    if !moved.is_empty() {
        let mut rewritten = serde_json::to_string_pretty(&document).map_err(|error| {
            catalogue::say(
                "cli.flow.cannot_recompose_flow",
                &[("error", &error.to_string())],
            )
        })?;
        rewritten.push('\n');
        std::fs::write(&path, rewritten).map_err(|error| {
            catalogue::say(
                "cli.flow.cannot_write_file",
                &[("path", &path.display().to_string()), ("error", &error.to_string())],
            )
        })?;
    }

    let mut report = catalogue::say(
        "cli.flow.relocate_heading",
        &[
            ("root", &root.display().to_string()),
            ("prefix", &old_root.display().to_string()),
            ("path", &path.display().to_string()),
        ],
    );
    let moved_said = if moved.is_empty() {
        catalogue::say("cli.flow.no_field_moved", &[])
    } else {
        format!("{}\n  {}", moved.len(), moved.join("\n  "))
    };
    report.push_str(&catalogue::say(
        "cli.flow.fields_moved",
        &[("moved", &moved_said)],
    ));
    if !left_alone.is_empty() {
        report.push_str(&catalogue::say(
            "cli.flow.outside_the_prefix_left_alone",
            &[("fields", &left_alone.join("; "))],
        ));
    }
    // Paths inside texts are shown, never touched: the reader decides.
    let flow: FlowFile = serde_json::from_str(&text).map_err(|error| {
        catalogue::say(
            "cli.flow.not_a_valid_flow",
            &[("path", &path.display().to_string()), ("error", &error.to_string())],
        )
    })?;
    let in_text: Vec<String> = hardcoded_paths(&flow)
        .iter()
        .filter(|found| !found.fatal)
        .map(|found| format!("{} in «{}» ({})", found.step, found.field, found.value))
        .collect();
    if !in_text.is_empty() {
        report.push_str(&catalogue::say(
            "cli.flow.paths_inside_text_to_fix_by_hand",
            &[
                ("count", &in_text.len().to_string()),
                ("paths", &in_text.join("\n  ")),
            ],
        ));
    }
    Ok(report)
}

/// The same, on the inputs a person writes by hand.
///
/// **THE CURE LOOKS WHERE THE JUDGE LOOKS**: `hardcoded_paths` reads the
/// declared inputs too, so a flow could be refused for a field this command
/// never walked. A field equal to the root becomes `.` and is not removed:
/// an input is read by name, and taking it away changes what arrives.
fn relocate_declared_inputs(document: &mut Value, old_root: &Path) -> (Vec<String>, Vec<String>) {
    let mut moved = Vec::new();
    let mut left_alone = Vec::new();
    let Some(inputs) = document.get_mut("inputs").and_then(Value::as_object_mut) else {
        return (moved, left_alone);
    };
    for (name, declared) in inputs.iter_mut() {
        walk_inputs_for_places(name, declared, old_root, &mut moved, &mut left_alone);
    }
    (moved, left_alone)
}

fn walk_inputs_for_places(
    name: &str,
    value: &mut Value,
    old_root: &Path,
    moved: &mut Vec<String>,
    left_alone: &mut Vec<String>,
) {
    let Some(fields) = value.as_object_mut() else {
        return;
    };
    for (key, inner) in fields.iter_mut() {
        if !POSITION_FIELDS.contains(&key.as_str()) {
            walk_inputs_for_places(name, inner, old_root, moved, left_alone);
            continue;
        }
        let Value::String(declared) = inner else {
            continue;
        };
        if !(declared.starts_with('/') || declared.starts_with("~/")) {
            continue;
        }
        match relative_to(old_root, declared) {
            Some(rest) => {
                let rest = if rest.is_empty() { ".".to_owned() } else { rest };
                moved.push(format!("{name}: «{declared}» → «{rest}»"));
                *inner = Value::String(rest);
            }
            None => left_alone.push(format!("{name}: «{declared}»")),
        }
    }
}

/// Strips the prefix from the document's `workdir`s, without touching disk.
///
/// It sits apart from the command because it is the part that can be tested:
/// what surrounds it reads the current directory and writes a file, and a test
/// changing the process directory would ruin the others running alongside — it
/// is fault 21, avoided here by needing no process at all.
///
/// Returns `None` if the document has not even a list of steps.
fn relocate_workdirs(document: &mut Value, old_root: &Path) -> Option<(Vec<String>, Vec<String>)> {
    let mut moved = Vec::new();
    let mut left_alone = Vec::new();
    let steps = document
        .get_mut("graph")
        .and_then(|graph| graph.get_mut("steps"))
        .and_then(Value::as_array_mut)?;
    for step in steps {
        let step_id = step
            .get("id")
            .and_then(Value::as_str)
            .map_or_else(|| catalogue::say("cli.flow.step_without_id", &[]), str::to_owned);
        let Some(with) = step.get_mut("with").and_then(Value::as_object_mut) else {
            continue;
        };
        // Text only: a `{"$from": …}` is a reference, resolved at run time by
        // whoever knows against what. Rewriting it would be inventing.
        let Some(Value::String(declared)) = with.get(WORKDIR_KEY).cloned() else {
            continue;
        };
        match relative_to(old_root, &declared) {
            // It equals the root: the field is no longer needed, and a
            // `workdir` worth the root is noise inviting an absolute rewrite.
            Some(rest) if rest.is_empty() => {
                // **`shift_remove` AND NOT `remove`.** With `preserve_order`
                // on — and it is — `remove` is a *swap*: it pulls the last key
                // into the hole and reorders the file. A command that takes a
                // field away and reshuffles the object in exchange makes an
                // unreadable diff, where nobody can tell what was decided from
                // what was moved. Measured: 62 lines changed instead of 7.
                with.shift_remove(WORKDIR_KEY);
                moved.push(catalogue::say(
                    "cli.flow.workdir_removed_was_the_root",
                    &[("step", &step_id)],
                ));
            }
            Some(rest) => {
                with.insert(WORKDIR_KEY.to_owned(), Value::String(rest.clone()));
                moved.push(format!("{step_id}: «{declared}» → «{rest}»"));
            }
            // Outside the prefix: it is not this command that decides what the
            // writer meant.
            None => left_alone.push(format!("{step_id}: «{declared}»")),
        }
    }
    Some((moved, left_alone))
}

/// The file a flow comes from, looked for in the sources that are directories.
fn flow_file_path(sources: &[FlowSource], name: &str) -> Result<PathBuf, String> {
    // From the most specific to the least: that is the one that wins at run
    // time, and rewriting one that never runs leaves the fault where it was.
    for source in sources.iter().rev() {
        if source.is_builtin() {
            continue;
        }
        let candidate = source.dir.join(format!("{name}.flow.json"));
        if candidate.is_file() {
            return Ok(candidate);
        }
    }
    Err(catalogue::say(
        "cli.flow.flow_is_not_a_file_on_disk",
        &[("name", name)],
    ))
}

/// The rest of `path` under `root`, or `None` if it does not sit under it.
///
/// Returns the empty string when the two coincide: the case where the field is
/// to be taken away, not rewritten.
fn relative_to(root: &Path, path: &str) -> Option<String> {
    let candidate = Path::new(path);
    let rest = candidate.strip_prefix(root).ok()?;
    Some(rest.display().to_string())
}

/// The field that says where a step works. The name lives in the flow crate:
/// two constants of one value in two crates are fault 10 in miniature.
const WORKDIR_KEY: &str = flow::WORKDIR_FIELD;

#[cfg(test)]
mod tests {
    use super::*;

    // ── moving a flow from one tree to another ────────────────────────

    fn document_with_workdir(workdir: &str) -> Value {
        serde_json::json!({
            "id": "prova", "description": "d",
            "graph": {"steps": [{
                "id": "unico", "deps": [], "action": "external_engine",
                "max_attempts": 1, "when": null,
                "input_schema": {"type": "any"}, "output_schema": {"type": "any"},
                // **`workdir` IS NOT THE SECOND TO LAST, AND THE POSITION IS
                // THE PROOF.** Removing the second-to-last field, swap and
                // shift give the same order: such a fixture lets the defect
                // through. Here two follow it, so the two ways diverge.
                "with": {
                    "tool": "git", "workdir": workdir,
                    "timeout_secs": 5, "args": ["status"]
                }
            }]},
            "inputs": {}
        })
    }

    /// It equals the root: the field goes. Keeping it as `"."` would be noise
    /// inviting the next person to rewrite it absolute.
    #[test]
    fn a_workdir_equal_to_the_root_is_removed() {
        let mut document = document_with_workdir("/vecchio/albero");

        let (moved, left) =
            relocate_workdirs(&mut document, Path::new("/vecchio/albero")).expect("ha dei passi");

        assert_eq!(moved.len(), 1);
        assert!(left.is_empty());
        assert!(document["graph"]["steps"][0]["with"]
            .get("workdir")
            .is_none());
    }

    /// Under the root: the relative piece stays, which is what makes the flow
    /// runnable on any clone.
    #[test]
    fn a_workdir_under_the_root_keeps_only_the_rest() {
        let mut document = document_with_workdir("/vecchio/albero/desktop");

        relocate_workdirs(&mut document, Path::new("/vecchio/albero")).expect("ha dei passi");

        assert_eq!(document["graph"]["steps"][0]["with"]["workdir"], "desktop");
    }

    /// **TAKING A FIELD AWAY MUST NOT REORDER THE FILE.** With `preserve_order`
    /// on, `Map::remove` is a *swap*: it pulls the last key into the hole.
    /// Measured on the real flow: 62 lines changed instead of 7, and a diff in
    /// which what was decided can no longer be told from what was moved.
    #[test]
    fn removing_a_workdir_does_not_reorder_the_other_fields() {
        let mut document = document_with_workdir("/vecchio/albero");

        relocate_workdirs(&mut document, Path::new("/vecchio/albero")).expect("ha dei passi");

        let keys: Vec<&str> = document["graph"]["steps"][0]["with"]
            .as_object()
            .expect("un oggetto")
            .keys()
            .map(String::as_str)
            .collect();
        assert_eq!(
            keys,
            vec!["tool", "timeout_secs", "args"],
            "l'ordine resta quello: uno swap metterebbe «args» prima di «timeout_secs»"
        );
    }

    /// Outside the prefix: left alone, and said. Guessing that `/altro/posto`
    /// meant «the root» would rewrite a path somebody put there on purpose.
    #[test]
    fn a_workdir_outside_the_prefix_is_left_alone_and_reported() {
        let mut document = document_with_workdir("/altro/posto");

        let (moved, left) =
            relocate_workdirs(&mut document, Path::new("/vecchio/albero")).expect("ha dei passi");

        assert!(moved.is_empty());
        assert_eq!(left.len(), 1);
        assert_eq!(
            document["graph"]["steps"][0]["with"]["workdir"],
            "/altro/posto"
        );
    }

    /// **A REFERENCE IS NEVER REWRITTEN.** `{"$from": "/innesco/text"}` is a
    /// pointer resolved at run time against the real input: there is nothing
    /// here to move, and touching it would be inventing.
    #[test]
    fn a_workdir_that_is_a_reference_is_never_touched() {
        let mut document = serde_json::json!({
            "id": "prova", "description": "d",
            "graph": {"steps": [{
                "id": "unico", "deps": [], "action": "external_engine",
                "max_attempts": 1, "when": null,
                "input_schema": {"type": "any"}, "output_schema": {"type": "any"},
                "with": {"workdir": {"$from": "/innesco/text"}}
            }]},
            "inputs": {}
        });

        let (moved, left) =
            relocate_workdirs(&mut document, Path::new("/vecchio/albero")).expect("ha dei passi");

        assert!(moved.is_empty() && left.is_empty());
        assert_eq!(
            document["graph"]["steps"][0]["with"]["workdir"],
            serde_json::json!({"$from": "/innesco/text"})
        );
    }

    /// **THE CURE MUST REACH WHERE THE REFUSAL POINTS.** `flow check` refuses a
    /// flow for an absolute place field in the declared inputs and names
    /// `flow relocate` as the way out; relocate walked only the steps, so on
    /// this machine it printed a report, changed nothing, exited zero, and the
    /// check stayed red on the very field it had named.
    #[test]
    fn a_place_field_in_the_declared_inputs_is_relocated_too() {
        let mut document = serde_json::json!({
            "id": "prova", "description": "d",
            "graph": {"steps": []},
            "inputs": {
                "riferimenti": {
                    "workdir": "/vecchio/albero/pagina",
                    "root_path": "/vecchio/albero/pagina",
                    "brief": "un testo che nomina /vecchio/albero e resta com'è"
                },
                "altrove": {"workdir": "/una/casa/fuori"},
                "sulla-radice": {"workdir": "/vecchio/albero"}
            }
        });

        let (moved, left) = relocate_declared_inputs(&mut document, Path::new("/vecchio/albero"));

        assert_eq!(document["inputs"]["riferimenti"]["workdir"], "pagina");
        // AN INPUT IS READ BY NAME: the field on the root becomes «.», never
        // taken away, or the flow would receive one key less than it declares.
        assert_eq!(document["inputs"]["sulla-radice"]["workdir"], ".");
        // Outside the prefix, and not a place field: neither is this command's
        // to decide.
        assert_eq!(document["inputs"]["altrove"]["workdir"], "/una/casa/fuori");
        assert_eq!(
            document["inputs"]["riferimenti"]["root_path"],
            "/vecchio/albero/pagina"
        );
        assert!(document["inputs"]["riferimenti"]["brief"]
            .as_str()
            .expect("il testo")
            .contains("/vecchio/albero"));
        assert_eq!(moved.len(), 2, "spostati: {moved:?}");
        assert_eq!(left.len(), 1, "lasciati: {left:?}");
    }
}
