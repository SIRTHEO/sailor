//! What runs in every checkout of every workspace, read by the one precedence
//! `flow::system::resolve` holds: one pass per checkout, never a second rule.

use std::path::{Path, PathBuf};
use std::sync::{mpsc, Arc};
use std::time::{Duration, Instant};

use serde::Serialize;

/// How long the window waits for the checkouts before saying which did not answer.
pub(crate) const A_CHECKOUT_TAKES_AT_MOST: Duration = Duration::from_secs(5);

/// A checkout to resolve flows in, and the workspace it belongs to.
#[derive(Debug, Clone)]
pub(crate) struct Checkout {
    pub workspace: String,
    pub root: PathBuf,
    pub branch: Option<String>,
    pub current: bool,
}

/// One flow name as it resolves in one context.
#[derive(Debug, Clone, Serialize)]
pub(crate) struct FlowRow {
    #[serde(flatten)]
    pub chain: flow::system::Chain,
    /// `None` when the winning file will not load: a broken flow has no steps.
    pub steps: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub broken: Option<String>,
}

/// Why a checkout carries no rows. Facts, not sentences: the window words them.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub(crate) enum Refusal {
    Unreadable { why: String },
    TimedOut { seconds: u64 },
    Stopped,
}

/// One place flows are resolved from. `flows` holds only the names whose
/// winner differs from the one outside every workspace.
#[derive(Debug, Clone, Serialize)]
pub(crate) struct Context {
    pub workspace: Option<String>,
    pub root: Option<String>,
    pub branch: Option<String>,
    pub current: bool,
    pub flows: Vec<FlowRow>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub troubles: Vec<flow::system::SourceTrouble>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub refused: Option<Refusal>,
}

#[derive(Debug, Clone, Serialize)]
pub(crate) struct Reading {
    /// Every name as it resolves with no project source: yours and built in,
    /// sent once instead of once per checkout.
    pub outside: Context,
    pub contexts: Vec<Context>,
}

/// What a reading stands on, handed in so a test can stand somewhere else.
pub(crate) struct Ground {
    pub home: Option<PathBuf>,
    pub declared: Option<PathBuf>,
    pub working: Option<PathBuf>,
    pub seen: Vec<PathBuf>,
    pub limit: Duration,
}

impl Ground {
    fn from_env(seen: Vec<PathBuf>) -> Ground {
        Ground {
            home: ledger::sailor_home(),
            declared: std::env::var_os("SAILOR_FLOWS").map(PathBuf::from),
            working: std::env::current_dir().ok(),
            seen,
            limit: A_CHECKOUT_TAKES_AT_MOST,
        }
    }

    fn home_flows(&self) -> Option<PathBuf> {
        self.home.as_ref().map(|home| home.join("flows"))
    }
}

/// The checkout the window stands in, and outside every workspace. It asks
/// for no other checkout: opening the place must not wait on all of them.
#[tauri::command(async)]
pub(crate) fn flows_here() -> Reading {
    here(&Ground::from_env(Vec::new()))
}

/// Every known checkout, and outside every workspace.
///
/// **A REGISTER THAT WILL NOT READ IS REFUSED, NOT EMPTIED**: an empty list
/// there would say «no workspace» when the truth is «could not look».
#[tauri::command(async)]
pub(crate) fn flows_by_workspace() -> Result<Reading, String> {
    let seen = sessions::Sessions::default_path()
        .ok()
        .and_then(|path| sessions::Sessions::open(path).ok())
        .and_then(|store| store.trees_worked_in().ok())
        .unwrap_or_default();
    everywhere(&Ground::from_env(seen))
}

pub(crate) fn here(ground: &Ground) -> Reading {
    let home_flows = ground.home_flows();
    let outside = outside_of(home_flows.as_deref(), ground.declared.as_deref());
    let Some(root) = ground.working.as_deref().and_then(flow::workspace::find_root) else {
        return Reading {
            outside,
            contexts: Vec::new(),
        };
    };
    let checkout = Checkout {
        workspace: workspace_name(&root),
        root,
        branch: None,
        current: true,
    };
    let declared = ground.declared.clone();
    let rows = outside.flows.clone();
    let contexts = contexts_within(ground.limit, vec![checkout], move |one| {
        let located = Checkout {
            branch: branch_of(&one.root),
            ..one.clone()
        };
        context_of(&located, home_flows.as_deref(), declared.as_deref(), &rows)
    });
    Reading { outside, contexts }
}

pub(crate) fn everywhere(ground: &Ground) -> Result<Reading, String> {
    let known = match &ground.home {
        Some(home) => flow::workspace::known_including(home, &ground.seen, 0)?,
        None => Vec::new(),
    };
    let standing = ground
        .working
        .as_deref()
        .and_then(flow::workspace::find_root)
        .map(|root| settled(&root));
    let listed = read_each(ground.limit, known.clone(), |entry: flow::workspace::Known| {
        entry
            .root
            .join(".git")
            .exists()
            .then(|| workspace::list(&entry.root).ok())
            .flatten()
    });

    let mut seen: Vec<PathBuf> = Vec::new();
    let mut checkouts = Vec::new();
    let mut refused = Vec::new();
    for (entry, answer) in known.iter().zip(listed) {
        let whole = Checkout {
            workspace: entry.name.clone(),
            root: entry.root.clone(),
            branch: None,
            current: false,
        };
        let trees: Vec<(PathBuf, Option<String>)> = match answer {
            Err(refusal) => {
                refused.push(refused_context(&whole, refusal));
                continue;
            }
            Ok(Some(trees)) if !trees.is_empty() => trees
                .into_iter()
                .map(|tree| (PathBuf::from(tree.path), tree.branch))
                .collect(),
            Ok(_) => vec![(entry.root.clone(), None)],
        };
        for (root, branch) in trees {
            let place = settled(&root);
            if seen.contains(&place) {
                continue;
            }
            checkouts.push(Checkout {
                current: standing.as_ref() == Some(&place),
                root,
                branch,
                ..whole.clone()
            });
            seen.push(place);
        }
    }

    let home_flows = ground.home_flows();
    let mut reading = take(home_flows.as_deref(), ground.declared.as_deref(), checkouts, ground.limit);
    reading.contexts.extend(refused);
    Ok(reading)
}

/// Outside every workspace, then each checkout within the limit.
pub(crate) fn take(
    home_flows: Option<&Path>,
    declared: Option<&Path>,
    checkouts: Vec<Checkout>,
    limit: Duration,
) -> Reading {
    let outside = outside_of(home_flows, declared);
    let (home_flows, declared) = (home_flows.map(Path::to_path_buf), declared.map(Path::to_path_buf));
    let rows = outside.flows.clone();
    let contexts = contexts_within(limit, checkouts, move |one| {
        context_of(one, home_flows.as_deref(), declared.as_deref(), &rows)
    });
    Reading { outside, contexts }
}

fn outside_of(home_flows: Option<&Path>, declared: Option<&Path>) -> Context {
    let sources = flow::system::sources(home_flows, None, declared);
    Context {
        workspace: None,
        root: None,
        branch: None,
        current: false,
        flows: rows_of(&sources, None),
        troubles: flow::system::unread_sources(&sources),
        refused: None,
    }
}

/// Each checkout read by `read`, in order; one that has not answered by the
/// limit is a refusal and the others are kept.
pub(crate) fn contexts_within<F>(limit: Duration, checkouts: Vec<Checkout>, read: F) -> Vec<Context>
where
    F: Fn(&Checkout) -> Context + Send + Sync + 'static,
{
    let answers = read_each(limit, checkouts.clone(), move |one: Checkout| read(&one));
    checkouts
        .iter()
        .zip(answers)
        .map(|(checkout, answer)| answer.unwrap_or_else(|refusal| refused_context(checkout, refusal)))
        .collect()
}

/// Every item on a thread of its own, against one deadline. A thread past the
/// deadline cannot be stopped from here: it finishes alone and its answer is dropped.
fn read_each<T, R, F>(limit: Duration, items: Vec<T>, read: F) -> Vec<Result<R, Refusal>>
where
    T: Send + 'static,
    R: Send + 'static,
    F: Fn(T) -> R + Send + Sync + 'static,
{
    let read = Arc::new(read);
    let (send, receive) = mpsc::channel();
    let count = items.len();
    for (at, item) in items.into_iter().enumerate() {
        let (send, read) = (send.clone(), Arc::clone(&read));
        std::thread::spawn(move || {
            let answer = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| read(item)));
            let _ = send.send((at, answer.ok()));
        });
    }
    drop(send);
    let mut answers: Vec<Option<Result<R, Refusal>>> = (0..count).map(|_| None).collect();
    let deadline = Instant::now() + limit;
    for _ in 0..count {
        match receive.recv_timeout(deadline.saturating_duration_since(Instant::now())) {
            Ok((at, answer)) => answers[at] = Some(answer.ok_or(Refusal::Stopped)),
            Err(_) => break,
        }
    }
    let seconds = limit.as_millis().div_ceil(1000) as u64;
    answers
        .into_iter()
        .map(|answer| answer.unwrap_or(Err(Refusal::TimedOut { seconds })))
        .collect()
}

fn refused_context(checkout: &Checkout, refusal: Refusal) -> Context {
    Context {
        workspace: Some(checkout.workspace.clone()),
        root: Some(checkout.root.to_string_lossy().into_owned()),
        branch: checkout.branch.clone(),
        current: checkout.current,
        flows: Vec::new(),
        troubles: Vec::new(),
        refused: Some(refusal),
    }
}

fn workspace_name(root: &Path) -> String {
    flow::workspace::declaration_at(root)
        .ok()
        .map(|declared| declared.name)
        .filter(|name| !name.is_empty())
        .unwrap_or_else(|| {
            root.file_name()
                .map(|name| name.to_string_lossy().into_owned())
                .unwrap_or_default()
        })
}

fn branch_of(root: &Path) -> Option<String> {
    let place = settled(root);
    workspace::list(root)
        .ok()?
        .into_iter()
        .find(|tree| settled(Path::new(&tree.path)) == place)?
        .branch
}

fn settled(path: &Path) -> PathBuf {
    path.canonicalize().unwrap_or_else(|_| path.to_path_buf())
}

fn rows_of(sources: &[flow::system::FlowSource], resolved_in: Option<&Path>) -> Vec<FlowRow> {
    flow::system::resolve(sources, resolved_in)
        .into_iter()
        .map(|resolved| {
            let (steps, broken) = match resolved.entry {
                Ok(flow) => (Some(flow.graph.steps().len()), None),
                Err(why) => (None, Some(why)),
            };
            FlowRow {
                chain: resolved.chain,
                steps,
                broken,
            }
        })
        .collect()
}

fn context_of(
    checkout: &Checkout,
    home_flows: Option<&Path>,
    declared: Option<&Path>,
    outside: &[FlowRow],
) -> Context {
    if let Err(why) = std::fs::read_dir(&checkout.root) {
        return refused_context(
            checkout,
            Refusal::Unreadable {
                why: why.to_string(),
            },
        );
    }
    let sources = flow::system::sources(home_flows, Some(&checkout.root), declared);
    let flows = rows_of(&sources, Some(&checkout.root))
        .into_iter()
        .filter(|row| {
            !outside
                .iter()
                .any(|there| there.chain.name == row.chain.name && there.chain.winner == row.chain.winner)
        })
        .collect();
    // Home and a declared folder are said once, outside; here only the checkout's own.
    let troubles = flow::system::unread_sources(&sources)
        .into_iter()
        .filter(|trouble| trouble.origin != flow::system::YOUR_ORIGIN && trouble.origin != flow::system::DECLARED_ORIGIN)
        .collect();
    Context {
        flows,
        troubles,
        refused: None,
        ..refused_context(checkout, Refusal::Stopped)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;

    struct Scratch(PathBuf);

    impl Scratch {
        fn new(name: &str) -> Self {
            let path = std::env::temp_dir().join(format!("sailor-by-workspace-{name}-{}", std::process::id()));
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

    const PLENTY: Duration = Duration::from_secs(60);

    fn write_flow(dir: &Path, id: &str, steps: usize) {
        std::fs::create_dir_all(dir).expect("a flows folder");
        let steps: Vec<serde_json::Value> = (0..steps)
            .map(|at| {
                serde_json::json!({
                    "id": format!("step-{at}"), "deps": [], "action": "shell_check",
                    "max_attempts": 1, "when": null,
                    "with": {"command": "true", "timeout_secs": 5},
                    "input_schema": {"type": "any"}, "output_schema": {"type": "any"}
                })
            })
            .collect();
        let flow = serde_json::json!({
            "id": id, "description": "an invented flow", "inputs": {},
            "graph": {"steps": steps}
        });
        let file = dir.join(format!("{id}{}", ".flow.json"));
        std::fs::write(file, flow.to_string()).expect("the flow is written");
    }

    const MARKER_TEXT: &str = r#"{"name":"an-invented-workspace"}"#;

    fn checkout(scratch: &Path, tree: &str, current: bool) -> Checkout {
        let root = scratch.join(tree);
        std::fs::create_dir_all(&root).expect("the checkout");
        std::fs::write(root.join(flow::workspace::MARKER), MARKER_TEXT).expect("the marker");
        Checkout {
            workspace: "an-invented-workspace".to_owned(),
            root,
            branch: Some(format!("work/{tree}")),
            current,
        }
    }

    fn shipped() -> &'static str {
        flow::system::FLOWS.first().expect("a flow ships inside the binary").0
    }

    fn git(dir: &Path, args: &[&str]) {
        let done = std::process::Command::new("git")
            .arg("-C")
            .arg(dir)
            .args(["-c", "core.hooksPath=/dev/null", "-c", "commit.gpgsign=false"])
            .args(["-c", "user.name=an-invented-person", "-c", "user.email=nobody@nowhere.invalid"])
            .args(args)
            .env_remove("GIT_DIR")
            .env_remove("GIT_WORK_TREE")
            .env_remove("GIT_INDEX_FILE")
            .output()
            .expect("git runs");
        assert!(done.status.success(), "git {args:?}: {}", String::from_utf8_lossy(&done.stderr));
    }

    fn context_at<'a>(reading: &'a Reading, root: &Path) -> &'a Context {
        reading
            .contexts
            .iter()
            .find(|context| context.root.as_deref() == Some(root.to_string_lossy().as_ref()))
            .unwrap_or_else(|| panic!("{} is not in the reading: {:?}", root.display(), reading.contexts))
    }

    /// **TWO CHECKOUTS OF ONE WORKSPACE PICK TWO WINNERS FOR ONE NAME**, and a
    /// flow of yours and a shipped one travel once, not once per checkout.
    #[test]
    fn each_checkout_carries_its_own_winner_and_the_rest_travels_once() {
        let scratch = Scratch::new("winners");
        let home_flows = scratch.0.join("home").join("flows");
        write_flow(&home_flows, shipped(), 2);
        write_flow(&home_flows, "a-flow-of-yours", 3);

        let plain = checkout(&scratch.0, "plain", true);
        let overriding = checkout(&scratch.0, "overriding", false);
        write_flow(&overriding.root.join("flows"), shipped(), 4);
        let gone = Checkout {
            root: scratch.0.join("a-checkout-nobody-made"),
            ..checkout(&scratch.0, "placeholder", false)
        };

        let reading = take(Some(&home_flows), None, vec![plain.clone(), overriding.clone(), gone], PLENTY);

        let outside_shipped = reading
            .outside
            .flows
            .iter()
            .find(|row| row.chain.name == shipped())
            .expect("the shipped name resolves outside every workspace");
        assert_eq!(outside_shipped.chain.winner.origin, flow::system::YOUR_ORIGIN);
        assert_eq!(outside_shipped.steps, Some(2));
        assert_eq!(
            outside_shipped.chain.replaced.iter().map(|one| one.origin).collect::<Vec<_>>(),
            vec![flow::system::BUILTIN_ORIGIN]
        );
        let shipped_everywhere = reading
            .outside
            .flows
            .iter()
            .filter(|row| row.chain.winner.origin == flow::system::BUILTIN_ORIGIN)
            .count();
        assert_eq!(shipped_everywhere, flow::system::FLOWS.len() - 1, "every other shipped flow, once");
        assert!(reading.outside.flows.iter().any(|row| row.chain.name == "a-flow-of-yours"));
        assert!(reading.outside.troubles.is_empty(), "{:?}", reading.outside.troubles);

        assert_eq!(reading.contexts.len(), 3, "no checkout is dropped");

        let plain_context = context_at(&reading, &plain.root);
        assert!(plain_context.current);
        assert_eq!(plain_context.workspace.as_deref(), Some("an-invented-workspace"));
        assert_eq!(plain_context.branch.as_deref(), Some("work/plain"));
        assert!(plain_context.refused.is_none());
        assert!(plain_context.flows.is_empty(), "nothing differs from outside: {:?}", plain_context.flows);

        let overriding_context = context_at(&reading, &overriding.root);
        assert_eq!(overriding_context.flows.len(), 1, "only the name this checkout decides");
        let winner = &overriding_context.flows[0];
        assert_eq!(winner.chain.name, shipped());
        assert_eq!(winner.chain.winner.origin, flow::workspace::ORIGIN_DECLARED);
        assert_eq!(winner.steps, Some(4));
        assert_eq!(
            winner.chain.replaced.iter().map(|one| one.origin).collect::<Vec<_>>(),
            vec![flow::system::BUILTIN_ORIGIN, flow::system::YOUR_ORIGIN]
        );
        assert_eq!(winner.chain.resolved_in.as_deref(), Some(overriding.root.as_path()));

        let gone_context = context_at(&reading, &scratch.0.join("a-checkout-nobody-made"));
        assert!(gone_context.flows.is_empty());
        assert!(
            matches!(gone_context.refused, Some(Refusal::Unreadable { .. })),
            "an unreadable checkout carries its reason: {:?}",
            gone_context.refused
        );
    }

    /// A flow file that will not load is a row with its reason, not a hole.
    #[test]
    fn a_broken_winner_is_a_row_with_its_reason() {
        let scratch = Scratch::new("broken");
        let tree = checkout(&scratch.0, "tree", true);
        std::fs::create_dir_all(tree.root.join("flows")).expect("the flows folder");
        std::fs::write(tree.root.join("flows").join(format!("half-written{}", ".flow.json")), "{")
            .expect("the broken file");

        let reading = take(None, None, vec![tree], PLENTY);

        let row = &reading.contexts[0].flows[0];
        assert_eq!(row.chain.name, "half-written");
        assert_eq!(row.steps, None);
        assert!(row.broken.is_some());
    }

    /// **A CHECKOUT THAT DOES NOT ANSWER IS SAID, AND THE OTHERS DO NOT WAIT FOR IT.**
    #[test]
    fn a_checkout_that_does_not_answer_is_said_and_does_not_hold_the_others() {
        let scratch = Scratch::new("slow");
        let quick = checkout(&scratch.0, "quick", true);
        let slow = checkout(&scratch.0, "slow", false);
        let slow_root = slow.root.clone();
        let started = Instant::now();

        let contexts = contexts_within(Duration::from_millis(300), vec![quick.clone(), slow.clone()], move |one| {
            if one.root == slow_root {
                std::thread::sleep(Duration::from_secs(4));
            }
            context_of(one, None, None, &[])
        });

        assert!(started.elapsed() < Duration::from_secs(2), "waited {:?} for a checkout that never answers", started.elapsed());
        assert_eq!(contexts.len(), 2);
        assert_eq!(contexts[0].root.as_deref(), Some(quick.root.to_string_lossy().as_ref()));
        assert!(contexts[0].refused.is_none(), "{:?}", contexts[0].refused);
        assert!(!contexts[0].flows.is_empty(), "the quick checkout was read");
        assert_eq!(contexts[1].root.as_deref(), Some(slow.root.to_string_lossy().as_ref()));
        assert_eq!(contexts[1].refused, Some(Refusal::TimedOut { seconds: 1 }));
    }

    /// **THROUGH THE REGISTER AND GIT**: a workspace remembered in a home, its
    /// repository and one worktree, found as the command finds them.
    #[test]
    fn the_checkouts_are_found_through_the_register_and_git_worktree_list() {
        let scratch = Scratch::new("discovery");
        let home = scratch.0.join("home");
        let repo = scratch.0.join("repo");
        let other = scratch.0.join("other-checkout");
        std::fs::create_dir_all(&repo).expect("the repository");
        std::fs::create_dir_all(&home).expect("the home");
        std::fs::write(repo.join(flow::workspace::MARKER), MARKER_TEXT).expect("the marker");
        git(&repo, &["init", "-q", "-b", "main"]);
        git(&repo, &["add", flow::workspace::MARKER]);
        git(&repo, &["commit", "-q", "-m", "an invented start"]);
        git(&repo, &["worktree", "add", "-q", "-b", "work/an-override", &other.to_string_lossy()]);
        write_flow(&other.join("flows"), shipped(), 4);
        flow::workspace::remember_in(&home, &repo, 1).expect("the workspace is remembered");

        let ground = Ground {
            home: Some(home),
            declared: None,
            working: Some(other.clone()),
            seen: Vec::new(),
            limit: PLENTY,
        };

        let reading = everywhere(&ground).expect("the register reads");
        assert_eq!(reading.contexts.len(), 2, "{:?}", reading.contexts);
        let main = context_at(&reading, &repo);
        assert_eq!(main.workspace.as_deref(), Some("an-invented-workspace"));
        assert_eq!(main.branch.as_deref(), Some("main"));
        assert!(!main.current);
        assert!(main.flows.is_empty(), "{:?}", main.flows);
        let overriding = context_at(&reading, &other);
        assert_eq!(overriding.branch.as_deref(), Some("work/an-override"));
        assert!(overriding.current, "the window stands in this checkout");
        assert_eq!(overriding.flows.len(), 1);
        assert_eq!(overriding.flows[0].chain.winner.origin, flow::workspace::ORIGIN_DECLARED);

        let standing = here(&ground);
        assert_eq!(standing.contexts.len(), 1, "the checkout view reads one checkout: {:?}", standing.contexts);
        assert_eq!(standing.contexts[0].root.as_deref(), Some(other.to_string_lossy().as_ref()));
        assert_eq!(standing.contexts[0].branch.as_deref(), Some("work/an-override"));
        assert_eq!(standing.contexts[0].workspace.as_deref(), Some("an-invented-workspace"));
        assert_eq!(standing.contexts[0].flows.len(), 1);
    }

    /// **A FOLDER OF YOURS THAT REFUSED THE READING TRAVELS AS A TROUBLE**, not as
    /// a home with no flows in it.
    #[test]
    fn a_folder_of_yours_that_refuses_the_reading_travels_as_a_trouble() {
        let scratch = Scratch::new("refusing-home");
        let home = scratch.0.join("home");
        let home_flows = home.join("flows");
        write_flow(&home_flows, "a-flow-behind-a-lock", 1);
        std::fs::set_permissions(&home_flows, std::fs::Permissions::from_mode(0o000)).expect("the lock");
        let ground = Ground {
            home: Some(home),
            declared: None,
            working: None,
            seen: Vec::new(),
            limit: PLENTY,
        };

        let reading = here(&ground);
        let refused_by_the_disk = std::fs::read_dir(&home_flows).is_err();
        std::fs::set_permissions(&home_flows, std::fs::Permissions::from_mode(0o755)).expect("the unlock");

        assert!(refused_by_the_disk, "this user reads a folder with no permissions: the check proves nothing here");
        assert_eq!(reading.outside.troubles.len(), 1, "{:?}", reading.outside.troubles);
        assert_eq!(reading.outside.troubles[0].origin, flow::system::YOUR_ORIGIN);
        assert_eq!(reading.outside.troubles[0].dir, home_flows);
        assert!(reading.contexts.is_empty());
    }

    /// A command the page calls and the shell never declared answers nothing.
    #[test]
    fn both_readings_are_declared_to_the_page() {
        let main = include_str!("main.rs");
        let declared = &main[main.find("generate_handler![").expect("the declaration")..];
        for command in ["flows_by_workspace::flows_here,", "flows_by_workspace::flows_by_workspace,"] {
            assert!(declared.contains(command), "{command} is not declared in main.rs");
        }
    }
}
