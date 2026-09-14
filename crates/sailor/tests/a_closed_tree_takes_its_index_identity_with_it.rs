//! A tree taken down by a person takes its index identity with it; a tree
//! taken down by the sweep leaves that identity written down, never deleted.
//!
//! The index is a fake server, as in the nodes' own tests: a test against the
//! real one proves an installation, and could not come out otherwise.

use actions::mcp::ServerSpec;
use sailor::retire_index::{retire, IndexTending, Retired, PRUNE_TOOL};
use sailor::worktree_cmd::{close_one, remove_one, retire_one, sweep};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;
use workspace::index_identity::{identity_of, IdentityRule};
use workspace::OpenTrees;

const ANOTHER_IDENTITY: &str = "other0000000";

#[test]
fn closing_a_tree_retires_its_identity_and_no_other() {
    let scratch = a_scratch("close");
    let repo = a_repository_in(&scratch);
    let store = ledger::Ledger::open(scratch.join("store")).expect("a store");
    let tree = workspace::create(&repo, "work/done", None).expect("a merged tree");
    let identity = identity_as_listed(&repo, "done");
    let index = an_index(&scratch, &identity, &[PRUNE_TOOL, "codebase_search"]);

    let said = close_one(&repo, "done", &store as &dyn OpenTrees, &index.tending).expect("the close");
    let gone = !tree.exists();
    let applied = index.applied();
    let _ = std::fs::remove_dir_all(&scratch);

    assert!(gone, "{said}");
    assert!(said.contains(&identity) && said.contains("retired"), "{said}");
    assert_eq!(applied.len(), 1, "{applied:?}");
    assert!(applied[0].contains(&format!("\"identity\":\"{identity}\"")), "{}", applied[0]);
    assert!(applied[0].contains("\"confirmationToken\":\"tok-tree\""), "{}", applied[0]);
    assert!(applied[0].contains("\"acknowledgeNoRemoteWriters\":true"), "{}", applied[0]);
    assert!(!applied[0].contains(ANOTHER_IDENTITY), "{}", applied[0]);
}

#[test]
fn removing_a_tree_retires_its_identity_too() {
    let scratch = a_scratch("remove");
    let repo = a_repository_in(&scratch);
    let tree = workspace::create(&repo, "work/done", None).expect("a tree");
    let identity = identity_as_listed(&repo, "done");
    let index = an_index(&scratch, &identity, &[PRUNE_TOOL]);

    let said = remove_one(&repo, "done", &index.tending).expect("the remove");
    let gone = !tree.exists();
    let applied = index.applied();
    let _ = std::fs::remove_dir_all(&scratch);

    assert!(gone, "{said}");
    assert!(said.contains(&identity) && said.contains("retired"), "{said}");
    assert_eq!(applied.len(), 1, "{applied:?}");
}

/// **THE CLOSE STAYS GREEN, AND THE REASON IS IN THE OUTPUT.** The installed
/// index may not offer the tool yet, or may not be there at all.
#[test]
fn an_index_without_the_tool_or_out_of_reach_leaves_the_close_green_and_says_why() {
    let scratch = a_scratch("blind");
    let repo = a_repository_in(&scratch);
    let store = ledger::Ledger::open(scratch.join("store")).expect("a store");
    let first = workspace::create(&repo, "work/first", None).expect("a tree");
    let second = workspace::create(&repo, "work/second", None).expect("a tree");
    let first_identity = identity_as_listed(&repo, "first");
    let second_identity = identity_as_listed(&repo, "second");
    let without_the_tool = an_index(&scratch, &first_identity, &["codebase_search"]);
    let out_of_reach = IndexTending::with(
        Some(ServerSpec {
            command: scratch.join("no-such-server").to_string_lossy().into_owned(),
            args: Vec::new(),
            env: Default::default(),
            cwd: None,
        }),
        IdentityRule::default(),
        Duration::from_secs(10),
    );

    let said_first = close_one(&repo, "first", &store as &dyn OpenTrees, &without_the_tool.tending)
        .expect("the close is green");
    let said_second =
        close_one(&repo, "second", &store as &dyn OpenTrees, &out_of_reach).expect("the close is green");
    let both_gone = !first.exists() && !second.exists();
    let applied = without_the_tool.applied();
    let _ = std::fs::remove_dir_all(&scratch);

    assert!(both_gone, "{said_first}\n{said_second}");
    assert!(
        said_first.contains(&first_identity) && said_first.contains("was not retired"),
        "{said_first}"
    );
    assert!(said_first.contains(PRUNE_TOOL), "the missing tool is not named:\n{said_first}");
    assert!(
        said_second.contains(&second_identity) && said_second.contains("could not be asked"),
        "{said_second}"
    );
    assert!(applied.is_empty(), "{applied:?}");
}

#[test]
fn a_repository_declaring_no_index_server_says_where_it_would() {
    let scratch = a_scratch("undeclared");
    let repo = a_repository_in(&scratch);
    let store = ledger::Ledger::open(scratch.join("store")).expect("a store");
    workspace::create(&repo, "work/done", None).expect("a tree");
    let none = IndexTending::with(None, IdentityRule::default(), Duration::from_secs(10));

    let said = close_one(&repo, "done", &store as &dyn OpenTrees, &none).expect("the close is green");
    let _ = std::fs::remove_dir_all(&scratch);

    assert!(said.contains("sailor.indexServer"), "{said}");
}

/// A pin committed on the trunk is in every tree of the repository: the
/// identity it names is the trunk's own, and closing one tree keeps it.
#[test]
fn a_pinned_identity_another_tree_still_carries_is_kept() {
    let scratch = a_scratch("pinned");
    let repo = a_repository_in(&scratch);
    let store = ledger::Ledger::open(scratch.join("store")).expect("a store");
    std::fs::write(
        repo.join(workspace::index_identity::PIN_FILE),
        "{\"projectId\": \"shared-pin\"}\n",
    )
    .expect("the pin");
    run_git(&repo, &["add", workspace::index_identity::PIN_FILE]);
    run_git(&repo, &["commit", "-q", "-m", "pin the index"]);
    let tree = workspace::create(&repo, "work/done", None).expect("a tree");
    let index = an_index(&scratch, "shared-pin", &[PRUNE_TOOL]);

    let said = close_one(&repo, "done", &store as &dyn OpenTrees, &index.tending).expect("the close");
    let gone = !tree.exists();
    let applied = index.applied();
    let _ = std::fs::remove_dir_all(&scratch);

    assert!(gone, "{said}");
    assert!(said.contains("shared-pin") && said.contains("still carries"), "{said}");
    assert!(applied.is_empty(), "a shared pin was retired:\n{applied:?}");
}

/// **THE SWEEP DELETES NO INDEX.** It writes the identity down and names the
/// gesture; the gesture, typed, is what retires it and closes the row.
#[test]
fn the_sweep_writes_the_identity_down_and_the_gesture_retires_it() {
    let scratch = a_scratch("sweep");
    let repo = a_repository_in(&scratch);
    let store = ledger::Ledger::open(scratch.join("store")).expect("a store");
    let tree = workspace::create(&repo, "work/done", None).expect("a merged tree");
    let identity = identity_as_listed(&repo, "done");
    let index = an_index(&scratch, &identity, &[PRUNE_TOOL]);

    let swept = sweep(&repo, &store as &dyn OpenTrees, &[], &IdentityRule::default()).expect("the sweep");
    let gone = !tree.exists();
    let applied_by_the_sweep = index.applied();
    let left = store.identities_left_behind().expect("the rows");

    let retired = retire_one(&repo, &identity, &store as &dyn OpenTrees, &index.tending).expect("the gesture");
    let applied_by_the_gesture = index.applied();
    let left_after = store.identities_left_behind().expect("the rows");
    let _ = std::fs::remove_dir_all(&scratch);

    assert!(gone, "{swept}");
    assert!(applied_by_the_sweep.is_empty(), "the sweep deleted an index:\n{applied_by_the_sweep:?}");
    assert_eq!(left.len(), 1, "{swept}");
    assert_eq!(left[0].identity, identity);
    assert!(swept.contains(&format!("sailor worktree retire {identity}")), "{swept}");
    assert!(retired.contains("retired"), "{retired}");
    assert_eq!(applied_by_the_gesture.len(), 1, "{applied_by_the_gesture:?}");
    assert!(left_after.is_empty(), "{left_after:?}");
}

/// An identity the inventory holds back is left exactly there, and the
/// refusal comes back in words rather than as a second call.
#[test]
fn an_identity_the_index_holds_back_is_not_applied() {
    let scratch = a_scratch("held");
    let repo = a_repository_in(&scratch);
    let index = an_index(&scratch, "busy00000000", &[PRUNE_TOOL]);

    let became = retire(&repo, "busy00000000", &index.tending);
    let applied = index.applied();
    let _ = std::fs::remove_dir_all(&scratch);

    assert!(matches!(&became, Retired::Refused { why, .. } if why.contains("Indexing in progress")), "{became:?}");
    assert!(applied.is_empty(), "{applied:?}");
}

struct AnIndex {
    tending: IndexTending,
    applied_log: PathBuf,
}

impl AnIndex {
    fn applied(&self) -> Vec<String> {
        std::fs::read_to_string(&self.applied_log)
            .unwrap_or_default()
            .lines()
            .map(str::to_owned)
            .collect()
    }
}

/// A fake index whose inventory holds `identity` with the token `tok-tree`,
/// another identity nobody asked about, and a third mid-write. Every apply
/// it receives is written to a log, which is what the assertions read.
fn an_index(scratch: &Path, identity: &str, tools: &[&str]) -> AnIndex {
    let applied_log = scratch.join("applied.log");
    let tools = tools
        .iter()
        .map(|name| format!("{{\"name\":\"{name}\"}}"))
        .collect::<Vec<_>>()
        .join(",");
    // Two backslashes reach the script, so printf hands the JSON a literal
    // «\n» and the answer stays one line, as the protocol wants.
    let report = format!(
        "Stored project inventory:\\\\n\\\\nIdentity: {identity}\\\\n  Path: /gone\\\\n  Path state: absent-on-this-host\\\\n  Confirmation token: tok-tree\\\\n\\\\nIdentity: {ANOTHER_IDENTITY}\\\\n  Path: /elsewhere\\\\n  Confirmation token: tok-other\\\\n\\\\nIdentity: busy00000000\\\\n  Indexing in progress: a metadata record is mid-write\\\\n  Confirmation token: tok-busy\\\\n"
    );
    let script = format!(
        "#!/bin/sh\n\
         while IFS= read -r line; do\n\
         \x20 case \"$line\" in *'\"method\":\"notifications/'*) continue;; esac\n\
         \x20 id=$(printf '%s' \"$line\" | sed -n 's/.*\"id\":\\([0-9][0-9]*\\).*/\\1/p')\n\
         \x20 case \"$line\" in\n\
         \x20   *'\"method\":\"initialize\"'*) printf '{{\"jsonrpc\":\"2.0\",\"id\":%s,\"result\":{{\"protocolVersion\":\"2024-11-05\",\"capabilities\":{{}},\"serverInfo\":{{\"name\":\"fake\",\"version\":\"0\"}}}}}}\\n' \"$id\" ;;\n\
         \x20   *'\"method\":\"tools/list\"'*) printf '{{\"jsonrpc\":\"2.0\",\"id\":%s,\"result\":{{\"tools\":[{tools}]}}}}\\n' \"$id\" ;;\n\
         \x20   *'\"apply\":true'*) printf '%s\\n' \"$line\" >> '{log}'; printf '{{\"jsonrpc\":\"2.0\",\"id\":%s,\"result\":{{\"content\":[{{\"type\":\"text\",\"text\":\"Removed all inventoried resources for identity: see the log\"}}]}}}}\\n' \"$id\" ;;\n\
         \x20   *'\"method\":\"tools/call\"'*) printf '{{\"jsonrpc\":\"2.0\",\"id\":%s,\"result\":{{\"content\":[{{\"type\":\"text\",\"text\":\"{report}\"}}]}}}}\\n' \"$id\" ;;\n\
         \x20 esac\n\
         done\n",
        log = applied_log.to_string_lossy(),
    );
    let path = scratch.join("index.sh");
    std::fs::write(&path, script).expect("the fake index is written");
    let tending = IndexTending::with(
        Some(ServerSpec {
            command: "sh".to_owned(),
            args: vec![path.to_string_lossy().into_owned()],
            env: Default::default(),
            cwd: None,
        }),
        IdentityRule::default(),
        Duration::from_secs(10),
    );
    AnIndex {
        tending,
        applied_log,
    }
}

/// The identity of a tree as git lists it: the path git reports is the one
/// the close reads, and on this platform it can differ from the one built.
fn identity_as_listed(repo: &Path, name: &str) -> String {
    let trees = workspace::list(repo).expect("the trees");
    let found = trees.iter().find(|tree| tree.name() == name).expect("the tree is listed");
    identity_of(Path::new(&found.path), found.branch.as_deref(), &IdentityRule::default())
        .expect("an identity")
        .id
}

fn a_scratch(label: &str) -> PathBuf {
    let path = std::env::temp_dir().join(format!(
        "sailor-index-identity-{label}-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&path);
    std::fs::create_dir_all(&path).expect("a scratch");
    path
}

/// Only the repository's own settings: nothing of the account running this.
fn run_git(at: &Path, args: &[&str]) {
    let done = Command::new("git")
        .arg("-C")
        .arg(at)
        .args(args)
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .output()
        .expect("git runs");
    assert!(
        done.status.success(),
        "git {args:?}: {}",
        String::from_utf8_lossy(&done.stderr)
    );
}

fn a_repository_in(scratch: &Path) -> PathBuf {
    let repo = scratch.join("project");
    std::fs::create_dir_all(&repo).expect("the repository");
    run_git(&repo, &["init", "-q"]);
    run_git(&repo, &["config", "user.email", "prove@example"]);
    run_git(&repo, &["config", "user.name", "prove"]);
    std::fs::write(repo.join("README"), "a tree to cut from\n").expect("a file");
    run_git(&repo, &["add", "README"]);
    run_git(&repo, &["commit", "-q", "-m", "the first"]);
    run_git(&repo, &["branch", "-M", workspace::branches::TRUNK]);
    repo
}
