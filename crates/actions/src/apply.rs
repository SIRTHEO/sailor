//! `apply_patch`: the one place a proposal becomes a change on disk.
//!
//! **SURFACE: the working tree. POWERS CLAIMED: writing, and only inside what
//! two lists agree on.** The assent is data and lives outside the source tree,
//! where a person widens it by writing a line. The denial is compiled in, and
//! widening it means recompiling, which no run can do by itself.

use flow::{Action, ActionError, ActionOutcome, SharedState, StepSpecies};
use serde::Deserialize;
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::Command;

/// The name this action registers under.
pub const APPLY_PATCH_ACTION: &str = "apply_patch";

/// The file that says what a patch may touch, under Sailor's home and outside
/// any source tree.
pub const THE_ASSENT_FILE: &str = "autocura.json";

/// What no assent can open: the file that grants, the module that applies, and
/// the test that defends the two. Compiled, so widening it takes a compiler.
const THE_WALL: &[&str] = &[
    THE_ASSENT_FILE,
    "crates/actions/src/apply.rs",
    "crates/sailor/tests/the_wall_a_patch_cannot_widen.rs",
];

/// What a step declares to apply a patch.
#[derive(Debug, Deserialize)]
struct PatchSpec {
    /// The unified diff, as data: the step before produced it and wrote
    /// nothing.
    patch: String,
    /// The tree it applies to. The executor fills it in with the run's root
    /// unless the step asked for a tree of its own.
    workdir: Option<String>,
    #[serde(flatten)]
    extra: BTreeMap<String, Value>,
}

/// The paths a unified diff touches, as the diff itself names them.
///
/// Both sides are read: a diff that deletes a file names it only on the left,
/// and a rename names two different paths.
pub fn paths_touched(patch: &str) -> Vec<String> {
    let mut found = Vec::new();
    for line in patch.lines() {
        let path = line
            .strip_prefix("+++ ")
            .or_else(|| line.strip_prefix("--- "))
            .map(|rest| rest.split('\t').next().unwrap_or(rest).trim());
        let Some(path) = path else {
            continue;
        };
        if path == "/dev/null" {
            continue;
        }
        let path = path
            .strip_prefix("a/")
            .or_else(|| path.strip_prefix("b/"))
            .unwrap_or(path);
        if !path.is_empty() && !found.iter().any(|seen| seen == path) {
            found.push(path.to_owned());
        }
    }
    found
}

/// Whether the wall answers for this path. Matched on the whole path and on
/// its file name, so an assent naming another directory cannot smuggle the
/// assent file itself back in.
pub fn behind_the_wall(path: &str) -> bool {
    let name = Path::new(path).file_name().and_then(|name| name.to_str());
    THE_WALL
        .iter()
        .any(|walled| path.ends_with(walled) || name == Some(*walled))
}

/// What the assent file says may be touched: paths, as prefixes of what a
/// patch names. A file that is not there says nothing may be, which is the
/// direction that worries.
pub fn assented(home: Option<&Path>) -> Result<Vec<String>, String> {
    let Some(home) = home else {
        return Ok(Vec::new());
    };
    let path = home.join(THE_ASSENT_FILE);
    let Ok(text) = std::fs::read_to_string(&path) else {
        return Ok(Vec::new());
    };
    let document: Value = serde_json::from_str(&text)
        .map_err(|error| format!("{}: {error}", path.display()))?;
    let listed = document
        .get("may_touch")
        .and_then(Value::as_array)
        .map(|paths| {
            paths
                .iter()
                .filter_map(Value::as_str)
                .map(str::to_owned)
                .collect()
        })
        .unwrap_or_default();
    Ok(listed)
}

/// Why this patch may not be applied, if it may not. The wall is asked first:
/// an assent that names a walled path is answered by the wall, not obeyed.
pub fn why_not(paths: &[String], assent: &[String]) -> Option<String> {
    if paths.is_empty() {
        return Some("the patch names no file: there is nothing to apply".to_owned());
    }
    if let Some(walled) = paths.iter().find(|path| behind_the_wall(path)) {
        return Some(format!(
            "«{walled}» is behind the wall, which no assent opens: it is the file that \
             grants, the code that applies, or the test that defends them"
        ));
    }
    let unasked: Vec<&String> = paths
        .iter()
        .filter(|path| !assent.iter().any(|allowed| path.starts_with(allowed.as_str())))
        .collect();
    if !unasked.is_empty() {
        return Some(format!(
            "nothing assents to {unasked:?}. What a patch may touch is written in \
             «{THE_ASSENT_FILE}» under Sailor's home, by a person"
        ));
    }
    None
}

fn git(dir: &Path, args: &[&str], stdin: Option<&str>) -> Result<String, String> {
    use std::io::Write;
    let mut child = Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(args)
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .map_err(|error| format!("git {}: {error}", args.join(" ")))?;
    if let Some(text) = stdin {
        child
            .stdin
            .as_mut()
            .ok_or_else(|| "git took no input".to_owned())?
            .write_all(text.as_bytes())
            .map_err(|error| format!("git {}: {error}", args.join(" ")))?;
    }
    let done = child
        .wait_with_output()
        .map_err(|error| format!("git {}: {error}", args.join(" ")))?;
    if done.status.success() {
        Ok(String::from_utf8_lossy(&done.stdout).into_owned())
    } else {
        Err(format!(
            "git {}: {}",
            args.join(" "),
            String::from_utf8_lossy(&done.stderr).trim()
        ))
    }
}

/// Applies a patch to `tree`, or says why it did not.
pub fn apply(tree: &Path, patch: &str, home: Option<&Path>) -> Result<Vec<String>, String> {
    let paths = paths_touched(patch);
    let assent = assented(home)?;
    if let Some(why) = why_not(&paths, &assent) {
        return Err(why);
    }
    git(tree, &["apply", "--check", "-"], Some(patch))?;
    git(tree, &["apply", "-"], Some(patch))?;
    Ok(paths)
}

pub struct ApplyPatchAction;

impl Action for ApplyPatchAction {
    fn unknown_fields(&self, declared: &Value) -> Vec<String> {
        match serde_json::from_value::<PatchSpec>(declared.clone()) {
            Ok(spec) => spec.extra.into_keys().collect(),
            Err(_) => Vec::new(),
        }
    }

    /// A patch applied twice does not apply twice: the second run finds the
    /// change already there and git refuses. Nothing here may be redone by a
    /// machine deciding it is safe.
    fn species(&self) -> StepSpecies {
        StepSpecies::HandToHuman
    }

    fn execute(&self, input: &Value, _shared: &SharedState) -> Result<ActionOutcome, ActionError> {
        let spec: PatchSpec = serde_json::from_value(input.clone())
            .map_err(|error| ActionError::new("invalid_input", error.to_string()))?;
        let tree = spec
            .workdir
            .map(PathBuf::from)
            .ok_or_else(|| {
                ActionError::new(
                    "invalid_input",
                    "a patch applies to a tree, and this step names none",
                )
            })?;
        match apply(&tree, &spec.patch, ledger::sailor_home().as_deref()) {
            Ok(paths) => Ok(ActionOutcome::Went(json!({"changed": paths}))),
            Err(why) => Err(ActionError::new("patch_refused", why)),
        }
    }
}

pub fn register_apply_patch(registry: &mut flow::ActionRegistry) {
    registry.register(APPLY_PATCH_ACTION, ApplyPatchAction);
}
