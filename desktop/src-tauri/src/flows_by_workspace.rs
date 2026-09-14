//! What runs in every checkout of every workspace, read by the one precedence
//! `flow::system::resolve` holds: one pass per checkout, never a second rule.

use std::path::{Path, PathBuf};

use serde::Serialize;

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

/// One place flows are resolved from. `flows` holds only the names whose
/// winner differs from the one outside every workspace.
#[derive(Debug, Clone, Serialize)]
pub(crate) struct Context {
    pub workspace: Option<String>,
    pub root: Option<String>,
    pub branch: Option<String>,
    pub current: bool,
    pub flows: Vec<FlowRow>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub unreadable: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub(crate) struct Reading {
    /// Every name as it resolves with no project source: yours and built in,
    /// sent once instead of once per checkout.
    pub outside: Context,
    pub contexts: Vec<Context>,
}

/// Every known checkout, and outside every workspace.
///
/// **A REGISTER THAT WILL NOT READ IS REFUSED, NOT EMPTIED**: an empty list
/// there would say «no workspace» when the truth is «could not look».
#[tauri::command]
pub(crate) fn flows_by_workspace() -> Result<Reading, String> {
    let home = ledger::sailor_home();
    let home_flows = home.as_ref().map(|home| home.join("flows"));
    let declared = std::env::var_os("SAILOR_FLOWS").map(PathBuf::from);
    let standing = std::env::current_dir()
        .ok()
        .and_then(|here| workspace::tree_around(&here));
    let known = match &home {
        Some(home) => {
            let seen = sessions::Sessions::default_path()
                .ok()
                .and_then(|path| sessions::Sessions::open(path).ok())
                .and_then(|store| store.trees_worked_in().ok())
                .unwrap_or_default();
            flow::workspace::known_including(home, &seen, 0)?
        }
        None => Vec::new(),
    };
    let checkouts = checkouts_of(&known, standing.as_deref());
    Ok(take(home_flows.as_deref(), declared.as_deref(), &checkouts))
}

/// The checkouts of every known workspace, each once. A root git will not
/// list is still a context of its own, with no branch.
fn checkouts_of(known: &[flow::workspace::Known], standing: Option<&Path>) -> Vec<Checkout> {
    let standing = standing.map(settled);
    let mut seen: Vec<PathBuf> = Vec::new();
    let mut checkouts = Vec::new();
    for entry in known {
        let trees: Vec<(PathBuf, Option<String>)> = match workspace::list(&entry.root) {
            Ok(listed) if entry.root.join(".git").exists() && !listed.is_empty() => listed
                .into_iter()
                .map(|tree| (PathBuf::from(tree.path), tree.branch))
                .collect(),
            _ => vec![(entry.root.clone(), None)],
        };
        for (root, branch) in trees {
            let place = settled(&root);
            if seen.contains(&place) {
                continue;
            }
            checkouts.push(Checkout {
                workspace: entry.name.clone(),
                current: standing.as_ref() == Some(&place),
                root,
                branch,
            });
            seen.push(place);
        }
    }
    checkouts
}

fn settled(path: &Path) -> PathBuf {
    path.canonicalize().unwrap_or_else(|_| path.to_path_buf())
}

pub(crate) fn take(
    home_flows: Option<&Path>,
    declared: Option<&Path>,
    checkouts: &[Checkout],
) -> Reading {
    let outside = rows_of(&flow::system::sources(home_flows, None, declared), None);
    let contexts = checkouts
        .iter()
        .map(|checkout| context_of(checkout, home_flows, declared, &outside))
        .collect();
    Reading {
        outside: Context {
            workspace: None,
            root: None,
            branch: None,
            current: false,
            flows: outside,
            unreadable: None,
        },
        contexts,
    }
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
    let mut context = Context {
        workspace: Some(checkout.workspace.clone()),
        root: Some(checkout.root.to_string_lossy().into_owned()),
        branch: checkout.branch.clone(),
        current: checkout.current,
        flows: Vec::new(),
        unreadable: unreadable(&checkout.root),
    };
    if context.unreadable.is_some() {
        return context;
    }
    let sources = flow::system::sources(home_flows, Some(&checkout.root), declared);
    context.flows = rows_of(&sources, Some(&checkout.root))
        .into_iter()
        .filter(|row| {
            !outside
                .iter()
                .any(|there| there.chain.name == row.chain.name && there.chain.winner == row.chain.winner)
        })
        .collect();
    context
}

/// Why a checkout cannot be resolved: `resolve` reads a folder it cannot open
/// as an empty one, which here would draw a refused checkout as a quiet one.
fn unreadable(root: &Path) -> Option<String> {
    if let Err(why) = std::fs::read_dir(root) {
        return Some(why.to_string());
    }
    let flows = root.join("flows");
    match std::fs::read_dir(&flows) {
        Err(why) if flows.exists() => Some(why.to_string()),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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

    fn checkout(scratch: &Path, tree: &str, current: bool) -> Checkout {
        let root = scratch.join(tree);
        std::fs::create_dir_all(&root).expect("the checkout");
        std::fs::write(root.join(flow::workspace::MARKER), r#"{"name":"an-invented-workspace"}"#)
            .expect("the marker");
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

        let reading = take(Some(&home_flows), None, &[plain.clone(), overriding.clone(), gone]);

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

        assert_eq!(reading.contexts.len(), 3, "no checkout is dropped");
        let of = |root: &Path| {
            reading
                .contexts
                .iter()
                .find(|context| context.root.as_deref() == Some(root.to_string_lossy().as_ref()))
                .expect("the checkout is in the reading")
        };

        let plain_context = of(&plain.root);
        assert!(plain_context.current);
        assert_eq!(plain_context.workspace.as_deref(), Some("an-invented-workspace"));
        assert_eq!(plain_context.branch.as_deref(), Some("work/plain"));
        assert!(plain_context.unreadable.is_none());
        assert!(plain_context.flows.is_empty(), "nothing differs from outside: {:?}", plain_context.flows);

        let overriding_context = of(&overriding.root);
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

        let gone_context = of(&scratch.0.join("a-checkout-nobody-made"));
        assert!(gone_context.flows.is_empty());
        assert!(gone_context.unreadable.is_some(), "an unreadable checkout carries its reason");
    }

    /// A flow file that will not load is a row with its reason, not a hole.
    #[test]
    fn a_broken_winner_is_a_row_with_its_reason() {
        let scratch = Scratch::new("broken");
        let tree = checkout(&scratch.0, "tree", true);
        std::fs::create_dir_all(tree.root.join("flows")).expect("the flows folder");
        std::fs::write(tree.root.join("flows").join(format!("half-written{}", ".flow.json")), "{")
            .expect("the broken file");

        let reading = take(None, None, &[tree]);

        let row = &reading.contexts[0].flows[0];
        assert_eq!(row.chain.name, "half-written");
        assert_eq!(row.steps, None);
        assert!(row.broken.is_some());
    }
}
