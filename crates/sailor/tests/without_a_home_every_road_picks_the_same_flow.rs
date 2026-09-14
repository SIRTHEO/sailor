//! Every road that turns a flow name into a file picks the same flow, or none,
//! when this process knows no home. Only the built binary can show it: each road
//! reads the environment and the working directory of its own process.

use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::process::Command;

struct Scratch(PathBuf);

impl Scratch {
    fn new(name: &str) -> Self {
        let path = std::env::temp_dir().join(format!(
            "sailor-without-a-home-{name}-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&path);
        std::fs::create_dir_all(&path).expect("the scratch directory");
        Scratch(path)
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn step(id: &str, action: &str, with: Value, deps: &[&str]) -> Value {
    json!({
        "id": id, "deps": deps, "action": action, "max_attempts": 1, "when": null,
        "with": with, "input_schema": {"type": "any"}, "output_schema": {"type": "any"}
    })
}

/// Prints what the step before it answered, as one `OUT=` line.
fn shown(after: &str) -> Value {
    step(
        "show",
        "shell_check",
        json!({"command": "printf 'OUT=%s\\n' \"$V\"", "env": {"V": {"$json": ""}}, "timeout_secs": 10}),
        &[after],
    )
}

fn write_flow(dir: &Path, id: &str, description: &str, steps: Vec<Value>) {
    std::fs::create_dir_all(dir).expect("a flows folder");
    let flow = json!({"id": id, "description": description, "inputs": {}, "graph": {"steps": steps}});
    std::fs::write(
        dir.join(format!("{id}.flow.json")),
        serde_json::to_string_pretty(&flow).expect("a flow serialises"),
    )
    .expect("the flow file");
}

/// The flow every road is asked about. It prints a marker when it runs and
/// names a tool of its own, so `tool_needs` can say whether it read it.
fn write_the_child(dir: &Path) {
    write_flow(
        dir,
        "example-child",
        "example child marker",
        vec![step(
            "say",
            "shell_check",
            json!({"command": "echo CHILD-RAN", "timeout_secs": 10, "tool": "marker-example-child"}),
            &[],
        )],
    );
}

/// The probes: one flow per road that is an action, and the parent that calls
/// the child through `subflow`.
fn write_the_probes(dir: &Path) {
    write_flow(dir, "example-parent", "a parent", vec![step("call", "subflow", json!({"flow": "example-child"}), &[])]);
    let probes = [
        ("probe-search", "flow_search", json!({"query": "example child marker"})),
        ("probe-needs", "tool_needs", json!({"findings": []})),
        ("probe-dormant", "dormant_steps", json!({})),
        ("probe-unused", "unused_actions", json!({})),
    ];
    for (id, action, with) in probes {
        write_flow(dir, id, "a probe", vec![step("ask", action, with, &[]), shown("ask")]);
    }
}

struct Said {
    code: Option<i32>,
    text: String,
}

fn sailor(cwd: &Path, ledger: &Path, home: Option<&Path>, args: &[&str]) -> Said {
    let mut command = Command::new(env!("CARGO_BIN_EXE_sailor"));
    command
        .args(args)
        .current_dir(cwd)
        .env_remove("HOME")
        .env_remove("SAILOR_HOME")
        .env_remove("XDG_CONFIG_HOME")
        .env_remove("SAILOR_FLOWS")
        .env("SAILOR_LEDGER", ledger);
    if let Some(home) = home {
        command.env("SAILOR_HOME", home);
    }
    let output = command.output().expect("the built binary starts");
    Said {
        code: output.status.code(),
        text: format!(
            "{}{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        ),
    }
}

/// The probe's answer, or `Null` when no road found the probe to run: that is a
/// verdict for the assertions, not a broken fixture.
fn out_of(said: &Said) -> Value {
    said.text
        .lines()
        .find_map(|line| line.split_once("· out] OUT="))
        .map(|(_, answer)| serde_json::from_str(answer).expect("the answer is JSON"))
        .unwrap_or(Value::Null)
}

/// What each road said about `example-child`: the origin when it has one, a
/// bare "seen" when the road only says it read the flow, `None` when it did not.
struct Roads {
    verdicts: Vec<(&'static str, Option<String>)>,
    flows_read_by_unused_actions: u64,
    rows_of_the_list: u64,
    run_refusal: String,
    subflow_refusal: String,
}

fn ask_every_road(cwd: &Path, ledger: &Path, home: Option<&Path>) -> Roads {
    let run = |args: &[&str]| sailor(cwd, ledger, home, args);
    let seen = |yes: bool| yes.then(|| "seen".to_owned());

    let dormant = out_of(&run(&["flow", "run", "probe-dormant"]));
    let unused = out_of(&run(&["flow", "run", "probe-unused"]));
    let search = out_of(&run(&["flow", "run", "probe-search"]));
    let needs = out_of(&run(&["flow", "run", "probe-needs"]));
    let list = run(&["flow", "list"]);
    let child = run(&["flow", "run", "example-child"]);
    let parent = run(&["flow", "run", "example-parent"]);

    let rows: Vec<Vec<&str>> = list
        .text
        .lines()
        .map(|line| line.split('\t').collect::<Vec<_>>())
        .filter(|cells| cells.len() >= 4 && cells[1].ends_with(" steps"))
        .collect();
    let listed = rows
        .iter()
        .find(|cells| cells[0] == "example-child")
        .map(|cells| cells[2].to_owned());
    let searched = search
        .get("hits")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .find(|hit| hit["flow"] == "example-child")
        .map(|hit| hit["origin"].as_str().unwrap_or_default().to_owned());
    let needed = ["unknown", "missing", "present"].iter().any(|kind| {
        needs[kind]
            .as_array()
            .into_iter()
            .flatten()
            .any(|need| need["tool"] == "marker-example-child")
    });

    let subflow_refusal = match parent
        .text
        .split_whitespace()
        .find(|word| word.starts_with("example-parent-"))
    {
        Some(run_id) if !parent.text.contains("CHILD-RAN") => ledger::Ledger::open(ledger)
            .expect("the ledger of this test")
            .said_of_failed_steps(run_id, 4, 4096)
            .expect("the failed steps read")
            .into_iter()
            .map(|excerpt| excerpt.said)
            .collect::<Vec<_>>()
            .join("\n"),
        _ => String::new(),
    };

    Roads {
        verdicts: vec![
            ("flow list", listed),
            ("flow_search", searched),
            ("flow run", seen(child.text.contains("CHILD-RAN"))),
            ("subflow", seen(parent.text.contains("CHILD-RAN"))),
            ("tool_needs", seen(needed)),
            ("dormant_steps", seen(dormant.to_string().contains("example-child"))),
        ],
        flows_read_by_unused_actions: unused["flows_read"].as_u64().unwrap_or_default(),
        rows_of_the_list: rows.len() as u64,
        run_refusal: if child.code == Some(0) { String::new() } else { child.text },
        subflow_refusal,
    }
}

fn assert_one_answer(roads: &Roads, expected: Option<&str>) {
    let summary = format!("{:?}", roads.verdicts);
    for (road, verdict) in &roads.verdicts {
        let agrees = match (verdict.as_deref(), expected) {
            (None, None) => true,
            (Some(said), Some(origin)) => said == "seen" || said == origin,
            _ => false,
        };
        assert!(agrees, "{road} disagrees with the others, expected {expected:?}: {summary}");
    }
    assert_eq!(
        roads.flows_read_by_unused_actions, roads.rows_of_the_list,
        "unused_actions reads as many flows as the list shows: {summary}"
    );
}

/// The measured disagreement: a `flows/` folder in the working directory, below
/// a declared root, was read as the person's own by the command line and not by
/// `subflow`, which then refused a flow the command line had just run.
#[test]
fn a_flows_folder_where_the_command_stands_is_nobody_s_home() {
    let scratch = Scratch::new("stands");
    let root = scratch.0.join("project");
    std::fs::create_dir_all(&root).expect("the project");
    std::fs::write(root.join("sailor.json"), "{}").expect("the marker");
    write_the_probes(&root.join("flows"));
    let work = root.join("work");
    write_the_child(&work.join("flows"));

    let roads = ask_every_road(&work, &scratch.0.join("ledger"), None);

    assert_one_answer(&roads, None);
    let no_home = catalogue::say("flow.sources.no_home", &[]);
    for (road, refusal) in [("flow run", &roads.run_refusal), ("subflow", &roads.subflow_refusal)] {
        assert!(refusal.contains(&no_home), "{road} says no home is known: {refusal}");
        assert!(!refusal.contains("yours ("), "{road} names no folder as yours: {refusal}");
    }
}

/// `SAILOR_HOME` declares the home by itself: a person who sets it and has no
/// `HOME` finds their flows on every road.
#[test]
fn a_declared_home_counts_without_home() {
    let scratch = Scratch::new("declared");
    let home = scratch.0.join("home");
    write_the_child(&home.join("flows"));
    write_the_probes(&home.join("flows"));
    let outside = scratch.0.join("outside");
    std::fs::create_dir_all(&outside).expect("a folder in no project");

    let roads = ask_every_road(&outside, &scratch.0.join("ledger"), Some(&home));

    assert_one_answer(&roads, Some("yours"));
}
