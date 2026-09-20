//! A flow step is not a shell program: three numbers that can only go down.
//!
//! `AGENTS.md` asks for an action in `crates/actions`, never a script. A
//! `shell_check` holding 4.335 characters is a script in a string: unparsed,
//! untested, failing by quoting or by timeout. **Scaffolding for goal #47** —
//! each seed is a debt, and at zero the file goes.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

/// Above this a step stops being a check and becomes a program. The median
/// step in this tree is 1.464 characters, which is the size of the problem.
const A_PROGRAM_NOT_A_CHECK: usize = 1000;

/// Steps over that size today. Downwards only.
const SHELL_PROGRAMS_TODAY: usize = 54;

/// Copies beyond the first of a shell body repeated between flows. `policy` is
/// 2.652 characters standing in four delivery flows: one edit, four files, and
/// nothing makes them agree.
const REDUNDANT_SHELL_COPIES_TODAY: usize = 9;

/// The longest single step. It shrinks or it stays.
const LONGEST_SHELL_STEP_TODAY: usize = 4335;

/// Bodies shorter than this are one command and repeat legitimately.
const TOO_SHORT_TO_COUNT: usize = 200;

const WHERE_FLOWS_LIVE: &[&str] = &["flows", "crates/flow/system"];

struct Weighed {
    programs: usize,
    redundant: usize,
    longest: usize,
    worst: Vec<(String, String, usize)>,
}

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("the crate sits two levels under the root")
        .to_path_buf()
}

fn flow_files(root: &Path) -> Vec<PathBuf> {
    let mut found: Vec<PathBuf> = WHERE_FLOWS_LIVE
        .iter()
        .flat_map(|under| std::fs::read_dir(root.join(under)).into_iter().flatten())
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| path.to_string_lossy().ends_with(".flow.json"))
        .collect();
    found.sort();
    found
}

fn shell_bodies(root: &Path) -> Vec<(String, String, String)> {
    let mut bodies = Vec::new();
    for path in flow_files(root) {
        let text = std::fs::read_to_string(&path).expect("a flow file reads");
        let flow: serde_json::Value = match serde_json::from_str(&text) {
            Ok(flow) => flow,
            Err(error) => panic!("{} is not JSON: {error}", path.display()),
        };
        let name = path
            .file_name()
            .map(|name| name.to_string_lossy().replace(".flow.json", ""))
            .unwrap_or_default();
        let steps = flow["graph"]["steps"].as_array().cloned().unwrap_or_default();
        for step in steps {
            if step["action"].as_str() != Some("shell_check") {
                continue;
            }
            let id = step["id"].as_str().unwrap_or("?").to_owned();
            let body = serde_json::to_string(&step["with"]).unwrap_or_default();
            bodies.push((name.clone(), id, body));
        }
    }
    bodies
}

fn weigh(root: &Path) -> Weighed {
    let bodies = shell_bodies(root);
    let mut repeated: BTreeMap<&str, usize> = BTreeMap::new();
    for (_, _, body) in &bodies {
        if body.len() >= TOO_SHORT_TO_COUNT {
            *repeated.entry(body.as_str()).or_default() += 1;
        }
    }
    let mut worst: Vec<(String, String, usize)> = bodies
        .iter()
        .filter(|(_, _, body)| body.len() > A_PROGRAM_NOT_A_CHECK)
        .map(|(flow, id, body)| (flow.clone(), id.clone(), body.len()))
        .collect();
    worst.sort_by_key(|(_, _, size)| std::cmp::Reverse(*size));
    worst.truncate(5);
    Weighed {
        programs: bodies
            .iter()
            .filter(|(_, _, body)| body.len() > A_PROGRAM_NOT_A_CHECK)
            .count(),
        redundant: repeated.values().filter(|seen| **seen > 1).map(|seen| seen - 1).sum(),
        longest: bodies.iter().map(|(_, _, body)| body.len()).max().unwrap_or(0),
        worst,
    }
}

fn does_not_rise(what: &str, constant: &str, seed: usize, measured: usize, weighed: &Weighed) {
    if measured <= seed {
        if measured < seed {
            eprintln!("{what} is down to {measured}: lower {constant} to it and keep the ground.");
        }
        return;
    }
    let mut said = format!(
        "{what}: {measured} against a seed of {seed}. A step that grew past its \
         check belongs in `crates/actions` with a name. Lowering {constant} is \
         the repair; raising it has to be argued in the commit.\n\nThe longest today:\n"
    );
    for (flow, id, size) in &weighed.worst {
        said.push_str(&format!("  {size:5} chars  {flow} · {id}\n"));
    }
    panic!("{said}");
}

#[test]
fn no_step_grows_into_a_program() {
    let weighed = weigh(&root());
    does_not_rise(
        "shell steps over a thousand characters",
        "SHELL_PROGRAMS_TODAY",
        SHELL_PROGRAMS_TODAY,
        weighed.programs,
        &weighed,
    );
}

#[test]
fn a_shell_body_is_not_copied_between_flows() {
    let weighed = weigh(&root());
    does_not_rise(
        "shell bodies repeated between flows",
        "REDUNDANT_SHELL_COPIES_TODAY",
        REDUNDANT_SHELL_COPIES_TODAY,
        weighed.redundant,
        &weighed,
    );
}

#[test]
fn the_longest_step_does_not_get_longer() {
    let weighed = weigh(&root());
    does_not_rise(
        "the longest shell step",
        "LONGEST_SHELL_STEP_TODAY",
        LONGEST_SHELL_STEP_TODAY,
        weighed.longest,
        &weighed,
    );
}
