//! The refusal `sailor flow check` gives a sensor, against the registry the
//! product ships: what an action declares about itself decides, not its name.

use flow::FlowFile;
use registry::{registry_in, House};
use serde_json::{json, Value};

fn flow_watching(sensor: Value) -> FlowFile {
    serde_json::from_value(json!({
        "id": "watching-flow",
        "description": "a flow started by what it watches",
        "graph": {"steps": [{
            "id": "trigger", "deps": [], "action": "trigger", "max_attempts": 1, "when": null,
            "with": {"source": "sensor", "sensor": sensor},
            "input_schema": {"type": "any"}, "output_schema": {"type": "any"}
        }]},
        "inputs": {}
    }))
    .expect("a flow with a sensor trigger")
}

fn refused(sensor: Value) -> Option<String> {
    trigger::sensor::refusal_of(
        &flow_watching(sensor),
        &registry_in(House::empty(), None, None),
    )
}

#[test]
fn a_sensor_that_reads_a_command_or_the_store_is_accepted() {
    let command = json!({
        "action": actions::SHELL_CHECK_ACTION,
        "with": {"command": "git rev-parse HEAD", "timeout_secs": 10},
        "pointer": "/status",
        "cooldown_secs": 60
    });
    assert_eq!(refused(command), None);
    let count = json!({
        "action": actions::store::STORE_SELECT_ACTION,
        "with": {"collection": "open-work", "limit": 100},
        "pointer": "/count"
    });
    assert_eq!(refused(count), None);
}

#[test]
fn a_sensor_that_writes_spends_or_names_nothing_is_refused() {
    let writes = refused(json!({
        "action": actions::store::STORE_WRITE_ACTION,
        "with": {"collection": "c", "value": 1, "written_by": "sensor"}
    }))
    .expect("a sensor that writes into the store is refused");
    assert!(writes.contains("only reads"), "{writes}");

    let spends = refused(json!({"action": actions::EXTERNAL_ENGINE_ACTION}))
        .expect("a sensor that asks an engine is refused");
    assert!(spends.contains("may spend"), "{spends}");

    let unknown = refused(json!({"action": "nobody-registers-this"}))
        .expect("a sensor nobody registers is refused");
    assert!(unknown.contains("nobody-registers-this"), "{unknown}");
}

/// A repository with a trunk, a policy on it, a finished branch nobody holds
/// and a finished branch a second tree stands on, taken down with the test.
struct Scratch(std::path::PathBuf);

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn git(at: &std::path::Path, args: &[&str]) {
    let out = std::process::Command::new("git")
        .arg("-C")
        .arg(at)
        .args(args)
        .output()
        .expect("git runs");
    assert!(
        out.status.success(),
        "git {args:?}: {}",
        String::from_utf8_lossy(&out.stderr)
    );
}

fn a_root_with_finished_work() -> (Scratch, std::path::PathBuf) {
    let at = std::env::temp_dir().join(format!("sensed-finished-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&at);
    std::fs::create_dir_all(at.join("tree")).expect("the tree's directory");
    let scratch = Scratch(at.clone());
    let tree = at.join("tree");
    git(&at, &["init", "-q", "--bare", "origin.git"]);
    git(&at, &["init", "-q", "-b", "main", "tree"]);
    git(&tree, &["config", "user.email", "sailor@example.invalid"]);
    git(&tree, &["config", "user.name", "Sailor"]);
    git(&tree, &["config", "sailor.trunk", "main"]);
    std::fs::create_dir_all(tree.join(".sailor")).expect("the policy's directory");
    std::fs::write(
        tree.join(".sailor/delivery-policy.json"),
        r#"{"merge":"ask","push":"ask","release":"ask","remote":"origin"}"#,
    )
    .expect("a policy on the trunk");
    git(&tree, &["add", "-A"]);
    git(&tree, &["commit", "-q", "-m", "the work"]);
    git(
        &tree,
        &[
            "remote",
            "add",
            "origin",
            &at.join("origin.git").to_string_lossy(),
        ],
    );
    git(&tree, &["push", "-q", "origin", "main"]);
    for branch in ["work/free-to-close", "work/a-tree-stands-on-it"] {
        git(&tree, &["branch", branch, "main"]);
    }
    let second = at.join("second");
    git(
        &tree,
        &[
            "worktree",
            "add",
            "-q",
            &second.to_string_lossy(),
            "work/a-tree-stands-on-it",
        ],
    );
    (scratch, tree)
}

/// **THE FLOW THAT CLOSES FINISHED WORK STARTS BY ITSELF.** Its sensor, read
/// the way the beat reads it — offered the project root and nothing else —
/// sees the finished branch nobody stands on, and only that one.
#[test]
fn the_shipped_flow_that_closes_finished_work_watches_its_root() {
    let text = flow::system::FLOWS
        .iter()
        .find(|(name, _)| *name == "close-the-finished-work")
        .map(|(_, text)| *text)
        .expect("the flow is shipped");
    let shipped: FlowFile = serde_json::from_str(text).expect("the shipped flow parses");
    let registry = registry_in(House::empty(), None, None);
    assert_eq!(trigger::sensor::refusal_of(&shipped, &registry), None);
    let sensor = trigger::sensor::declared_by(&shipped)
        .expect("its trigger is a sensor")
        .expect("and a sensor it declares well");

    let (_scratch, root) = a_root_with_finished_work();
    let eyes = trigger::sensor::Eyes {
        registry: std::sync::Arc::new(registry),
        root: Some(root),
        timeout: trigger::sensor::SENSOR_TIMEOUT,
    };
    let reading = trigger::sensor::read(&sensor, &eyes).expect("the root is read");

    assert_eq!(reading, json!(["work/free-to-close"]));
}

#[test]
fn the_forge_a_policy_declares_reaches_its_program_through_a_descriptor() {
    let forges = registry::House::empty().tools.forge_programs();
    let program = actions::proven::program_for(&forges, "github")
        .expect("a shipped descriptor speaks for the forge this repository declares");
    assert!(!program.program.is_empty());
    assert!(program.never_inherited.contains(&program.token_variable));
    assert_eq!(
        actions::proven::program_for(&forges, "a-forge-nobody-declares"),
        None
    );
}
