//! A flow file that replaces a shipped one is said on the command line: the
//! list marks it, `flow where` prints the whole chain, and `flow restore` puts
//! the shipped flow back by archiving the file, never by deleting it.

use serde_json::json;
use std::path::{Path, PathBuf};
use std::process::Command;

struct Scratch(PathBuf);

impl Scratch {
    fn new(name: &str) -> Self {
        let path = std::env::temp_dir().join(format!(
            "sailor-replaces-{name}-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&path);
        std::fs::create_dir_all(&path).expect("the scratch directory");
        Scratch(path.canonicalize().expect("the scratch directory resolves"))
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// A shipped flow's name, taken from the binary's own list.
fn shipped() -> &'static str {
    flow::system::FLOWS.first().expect("a flow ships inside the binary").0
}

fn write_flow(dir: &Path, id: &str) -> PathBuf {
    std::fs::create_dir_all(dir).expect("a flows folder");
    let flow = json!({
        "id": id, "description": "a home flow", "inputs": {},
        "graph": {"steps": [{
            "id": "say", "deps": [], "action": "shell_check", "max_attempts": 1, "when": null,
            "with": {"command": "true", "timeout_secs": 5},
            "input_schema": {"type": "any"}, "output_schema": {"type": "any"}
        }]}
    });
    let path = dir.join(format!("{id}.flow.json"));
    std::fs::write(&path, serde_json::to_string_pretty(&flow).expect("a flow serialises"))
        .expect("the flow file");
    path
}

struct Said {
    code: Option<i32>,
    text: String,
}

fn sailor(scratch: &Scratch, declared: Option<&Path>, args: &[&str]) -> Said {
    let outside = scratch.0.join("outside");
    std::fs::create_dir_all(&outside).expect("a folder in no project");
    let mut command = Command::new(env!("CARGO_BIN_EXE_sailor"));
    command
        .args(args)
        .current_dir(&outside)
        .env("HOME", scratch.0.join("person"))
        .env("SAILOR_HOME", scratch.0.join("home"))
        .env("SAILOR_LEDGER", scratch.0.join("ledger"))
        .env_remove("XDG_CONFIG_HOME")
        .env_remove("SAILOR_FLOWS");
    if let Some(declared) = declared {
        command.env("SAILOR_FLOWS", declared);
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

/// The origin cell of a flow's row in `sailor flow list`.
fn origin_in_the_list(said: &Said, name: &str) -> Option<String> {
    said.text
        .lines()
        .map(|line| line.split('\t').collect::<Vec<_>>())
        .find(|cells| cells.len() >= 4 && cells[0] == name)
        .map(|cells| cells[2].to_owned())
}

fn archived_in(folder: &Path) -> Vec<PathBuf> {
    std::fs::read_dir(folder)
        .map(|entries| entries.flatten().map(|entry| entry.path()).collect())
        .unwrap_or_default()
}

#[test]
fn the_list_marks_a_flow_of_yours_that_replaces_a_shipped_one_and_says_where_it_resolved() {
    let scratch = Scratch::new("list");
    write_flow(&scratch.0.join("home").join("flows"), shipped());

    let list = sailor(&scratch, None, &["flow", "list"]);

    assert_eq!(list.code, Some(0), "{}", list.text);
    let replaces = catalogue::say(
        "cli.flow.list_origin_replaces",
        &[("origin", "yours"), ("replaced", "built in")],
    );
    assert_eq!(origin_in_the_list(&list, shipped()), Some(replaces), "{}", list.text);
    let outside = scratch.0.join("outside").display().to_string();
    assert!(list.text.contains(&outside), "the list says where it resolved: {}", list.text);
}

#[test]
fn where_prints_every_candidate_and_the_one_that_runs() {
    let scratch = Scratch::new("where");
    let file = write_flow(&scratch.0.join("home").join("flows"), shipped());

    let chain = sailor(&scratch, None, &["flow", "where", shipped()]);

    assert_eq!(chain.code, Some(0), "{}", chain.text);
    let runs = catalogue::say(
        "cli.flow.where_runs",
        &[("origin", "yours"), ("path", &file.display().to_string())],
    );
    let replaced = catalogue::say(
        "cli.flow.where_replaced",
        &[("origin", "built in"), ("path", flow::system::PLACE)],
    );
    assert!(chain.text.contains(&runs), "the winner is named: {}", chain.text);
    assert!(chain.text.contains(&replaced), "the shipped flow is not absent: {}", chain.text);
    assert!(
        chain.text.find(&replaced) < chain.text.find(&runs),
        "least specific first: {}",
        chain.text
    );
    let outside = scratch.0.join("outside").display().to_string();
    assert!(chain.text.contains(&outside), "the chain says where it resolved: {}", chain.text);

    let nobody = sailor(&scratch, None, &["flow", "where", "a-flow-nobody-wrote"]);
    assert_eq!(nobody.code, Some(1), "{}", nobody.text);
}

#[test]
fn restore_archives_the_file_and_the_shipped_flow_runs_again() {
    let scratch = Scratch::new("restore");
    let file = write_flow(&scratch.0.join("home").join("flows"), shipped());
    let written = std::fs::read(&file).expect("the file is there");

    let restored = sailor(&scratch, None, &["flow", "restore", shipped()]);

    assert_eq!(restored.code, Some(0), "{}", restored.text);
    assert!(!file.exists(), "the file left the flows folder");
    let archive = archived_in(&scratch.0.join("home").join("flows-archived"));
    assert_eq!(archive.len(), 1, "one file archived: {archive:?}");
    let name = archive[0].file_name().expect("a name").to_string_lossy().into_owned();
    assert!(
        name.starts_with(&format!("{}.", shipped())) && name.ends_with(".flow.json"),
        "{name}"
    );
    assert_eq!(std::fs::read(&archive[0]).expect("the archive reads"), written, "moved, not rewritten");
    assert!(restored.text.contains(&archive[0].display().to_string()), "it says where: {}", restored.text);

    let list = sailor(&scratch, None, &["flow", "list"]);
    assert_eq!(origin_in_the_list(&list, shipped()).as_deref(), Some("built in"), "{}", list.text);

    let again = sailor(&scratch, None, &["flow", "restore", shipped()]);
    assert_eq!(again.code, Some(1), "{}", again.text);
    assert!(
        again.text.contains(&catalogue::say("cli.flow.restore_already_built_in", &[("flow", shipped())])),
        "{}",
        again.text
    );
}

/// A read-only flows folder lets the file be archived but not removed: both
/// paths are said, the flow that runs does not change, and a second attempt
/// makes no second archive.
#[cfg(unix)]
#[test]
fn an_original_that_stays_is_archived_once_and_both_paths_are_said() {
    use std::os::unix::fs::PermissionsExt;

    struct Writable(PathBuf);
    impl Drop for Writable {
        fn drop(&mut self) {
            let _ = std::fs::set_permissions(&self.0, std::fs::Permissions::from_mode(0o755));
        }
    }

    let scratch = Scratch::new("stays");
    let folder = scratch.0.join("home").join("flows");
    let file = write_flow(&folder, shipped());
    std::fs::set_permissions(&folder, std::fs::Permissions::from_mode(0o555)).expect("a read-only folder");
    let _writable = Writable(folder.clone());

    let first = sailor(&scratch, None, &["flow", "restore", shipped()]);
    let second = sailor(&scratch, None, &["flow", "restore", shipped()]);

    let archives = archived_in(&scratch.0.join("home").join("flows-archived"));
    assert_eq!(archives.len(), 1, "one archive after two attempts: {archives:?}\n{}\n{}", first.text, second.text);
    assert!(file.exists(), "the original stays");
    let stays = catalogue::say(
        "cli.flow.restore_original_stays",
        &[
            ("flow", shipped()),
            ("path", &file.display().to_string()),
            ("archive", &archives[0].display().to_string()),
        ],
    );
    for said in [&first, &second] {
        assert_eq!(said.code, Some(1), "{}", said.text);
        assert!(said.text.contains(&stays), "both paths are said: {}", said.text);
    }
    let list = sailor(&scratch, None, &["flow", "list"]);
    let replaces = catalogue::say(
        "cli.flow.list_origin_replaces",
        &[("origin", "yours"), ("replaced", "built in")],
    );
    assert_eq!(origin_in_the_list(&list, shipped()), Some(replaces), "{}", list.text);
}

#[test]
fn restore_refuses_a_flow_with_nothing_built_in_underneath_and_leaves_it() {
    let scratch = Scratch::new("nothing-under");
    let file = write_flow(&scratch.0.join("home").join("flows"), "a-home-flow");

    let refused = sailor(&scratch, None, &["flow", "restore", "a-home-flow"]);

    assert_eq!(refused.code, Some(1), "{}", refused.text);
    assert!(file.exists(), "the file stays where it was");
    assert!(archived_in(&scratch.0.join("home").join("flows-archived")).is_empty());
    assert!(
        refused.text.contains(&catalogue::say(
            "cli.flow.restore_nothing_built_in",
            &[("flow", "a-home-flow"), ("path", &file.display().to_string())],
        )),
        "{}",
        refused.text
    );
}

/// `SAILOR_FLOWS` says where the person's flows are: a home file outside it is
/// not what runs, so it is not moved; the declared folder's own file is.
#[test]
fn under_a_declared_folder_only_that_folder_s_file_is_restored() {
    let scratch = Scratch::new("declared");
    let home_file = write_flow(&scratch.0.join("home").join("flows"), shipped());
    let declared = scratch.0.join("declared").join("flows");
    std::fs::create_dir_all(&declared).expect("the declared folder");

    let refused = sailor(&scratch, Some(&declared), &["flow", "restore", shipped()]);
    assert_eq!(refused.code, Some(1), "{}", refused.text);
    assert!(home_file.exists(), "the home file is not the one running, and stays");

    let declared_file = write_flow(&declared, shipped());
    let restored = sailor(&scratch, Some(&declared), &["flow", "restore", shipped()]);
    assert_eq!(restored.code, Some(0), "{}", restored.text);
    assert!(!declared_file.exists(), "the declared folder's file was archived");
    assert!(home_file.exists(), "the home file is still untouched");
    assert_eq!(archived_in(&scratch.0.join("declared").join("flows-archived")).len(), 1);
}
