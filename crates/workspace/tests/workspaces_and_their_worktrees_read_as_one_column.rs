//! The gate for the left column: one reading has to answer the whole of it.

use std::path::{Path, PathBuf};
use std::process::Command;
use workspace::column::{self, Ground, Standing, TerminalAt, Where};

fn git(at: &Path, args: &[&str]) {
    let done = Command::new("git")
        .args(["-c", "user.email=gate@example.invalid", "-c", "user.name=gate"])
        .args(args)
        .current_dir(at)
        .output()
        .expect("git runs");
    assert!(
        done.status.success(),
        "git {args:?} in {}: {}",
        at.display(),
        String::from_utf8_lossy(&done.stderr)
    );
}

fn a_repository(at: &Path) {
    std::fs::create_dir_all(at).expect("the directory is made");
    std::fs::write(at.join(flow::workspace::MARKER), "{}").expect("the marker is written");
    git(at, &["init", "-q", "-b", "main", "."]);
    git(at, &["commit", "-q", "--allow-empty", "-m", "first"]);
}

fn a_flow(dir: &Path, id: &str, steps: usize) {
    std::fs::create_dir_all(dir).expect("the flows directory is made");
    let steps: Vec<_> = (0..steps)
        .map(|which| {
            let anything =
                serde_json::json!({ "type": "object", "properties": {}, "required": [], "allow_extra": true });
            serde_json::json!({
                "id": format!("step-{which}"),
                "deps": [],
                "action": "note",
                "max_attempts": 1,
                "when": null,
                "with": {},
                "input_schema": anything,
                "output_schema": anything,
            })
        })
        .collect();
    let document = serde_json::json!({
        "id": id,
        "description": "a flow the gate wrote",
        "graph": { "steps": steps },
        "inputs": {},
    });
    std::fs::write(
        dir.join(format!("{id}.flow.json")),
        serde_json::to_string(&document).expect("the flow serialises"),
    )
    .expect("the flow is written");
}

struct Yard {
    root: PathBuf,
}

impl Drop for Yard {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

/// One project with two trees, one project whose tree was deleted underneath
/// it, one project with no trees left, one terminal inside a tree and one
/// outside every tree.
fn a_small_world() -> Yard {
    static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let root = std::env::temp_dir().join(format!(
        "sailor-column-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
    ));
    let _ = std::fs::remove_dir_all(&root);
    let yard = Yard { root };
    let home = yard.root.join("home");
    std::fs::create_dir_all(&home).expect("the home is made");

    let alpha = yard.root.join("alpha");
    a_repository(&alpha);
    a_flow(&alpha.join("flows"), "an-alpha-flow", 7);
    git(
        &alpha,
        &[
            "worktree",
            "add",
            "-q",
            "-b",
            "work/second",
            "../alpha-worktrees/second",
        ],
    );

    let beta = yard.root.join("beta");
    a_repository(&beta);
    git(
        &beta,
        &[
            "worktree",
            "add",
            "-q",
            "-b",
            "work/lost",
            "../beta-worktrees/lost",
        ],
    );
    std::fs::remove_dir_all(yard.root.join("beta-worktrees").join("lost"))
        .expect("the tree is deleted underneath the project");

    let gamma = yard.root.join("gamma");
    std::fs::create_dir_all(&gamma).expect("the directory is made");
    std::fs::write(gamma.join(flow::workspace::MARKER), "{}").expect("the marker is written");

    for (which, root) in [&alpha, &beta, &gamma].into_iter().enumerate() {
        flow::workspace::remember_in(&home, root, 100 + which as i64).expect("the register takes it");
    }

    a_flow(&home.join("flows"), "a-flow-of-your-own", 2);
    yard
}

fn reading(yard: &Yard) -> column::Reading {
    let home = yard.root.join("home");
    let home_flows = home.join("flows");
    let second = yard.root.join("alpha-worktrees").join("second");
    let terminals = vec![
        TerminalAt {
            tty: "ttys001".to_owned(),
            worktree: second.to_string_lossy().into_owned(),
        },
        TerminalAt {
            tty: "ttys002".to_owned(),
            worktree: yard.root.join("elsewhere").to_string_lossy().into_owned(),
        },
    ];
    let boards = vec![(second.to_string_lossy().into_owned(), 31)];
    column::take(&Ground {
        home: &home,
        home_flows: &home_flows,
        standing_in: Some(&second),
        terminals: &terminals,
        boards: &boards,
        troubles: &[],
    })
}

#[test]
fn a_project_holds_its_several_trees_and_says_which_one_is_stood_in() {
    let yard = a_small_world();
    let read = reading(&yard);

    let alpha = read
        .projects
        .iter()
        .find(|project| project.name == "alpha")
        .expect("alpha is a project");
    assert_eq!(alpha.trees.len(), 2, "{:?}", alpha.trees);

    let stood_in: Vec<_> = alpha.trees.iter().filter(|tree| tree.stood_in).collect();
    assert_eq!(stood_in.len(), 1, "exactly one tree is stood in");
    assert_eq!(stood_in[0].name, "second");
    assert_eq!(stood_in[0].branch.as_deref(), Some("work/second"));
    assert_eq!(stood_in[0].board, 31);

    let main = alpha
        .trees
        .iter()
        .find(|tree| tree.name == "alpha")
        .expect("the checkout the project itself is");
    assert_eq!(main.branch.as_deref(), Some("main"));
    assert!(!main.stood_in);
    assert_eq!(main.board, 0, "a tree nobody works in has an empty board");
    assert_eq!(
        main.flows.iter().map(|flow| flow.id.as_str()).collect::<Vec<_>>(),
        vec!["an-alpha-flow"]
    );
    assert_eq!(main.flows[0].steps, 7, "{:?}", main.flows[0]);
}

#[test]
fn a_tree_deleted_from_disk_is_drawn_and_is_not_an_error() {
    let yard = a_small_world();
    let read = reading(&yard);

    let beta = read
        .projects
        .iter()
        .find(|project| project.name == "beta")
        .expect("beta is a project");
    let lost = beta
        .trees
        .iter()
        .find(|tree| tree.name == "lost")
        .expect("the deleted tree is still drawn");
    assert_eq!(lost.standing, Standing::Gone);
    assert_eq!(lost.branch.as_deref(), Some("work/lost"));

    let here = beta
        .trees
        .iter()
        .find(|tree| tree.name == "beta")
        .expect("the surviving tree");
    assert_eq!(here.standing, Standing::Here);
}

#[test]
fn a_project_with_no_trees_left_is_an_ordinary_answer() {
    let yard = a_small_world();
    let read = reading(&yard);

    let gamma = read
        .projects
        .iter()
        .find(|project| project.name == "gamma")
        .expect("gamma is a project");
    assert!(gamma.trees.is_empty(), "{:?}", gamma.trees);
    assert_eq!(gamma.standing, Standing::Here);
}

#[test]
fn the_flows_of_no_checkout_are_kept_apart_from_the_trees() {
    let yard = a_small_world();
    let read = reading(&yard);

    let yours: Vec<_> = read
        .flows_everywhere
        .iter()
        .filter(|flow| flow.origin == flow::system::YOUR_ORIGIN)
        .map(|flow| flow.id.as_str())
        .collect();
    assert_eq!(yours, vec!["a-flow-of-your-own"]);

    let built_in = read
        .flows_everywhere
        .iter()
        .filter(|flow| flow.origin == flow::system::BUILTIN_ORIGIN)
        .count();
    assert_eq!(built_in, flow::system::FLOWS.len());

    assert!(
        !read
            .flows_everywhere
            .iter()
            .any(|flow| flow.id == "an-alpha-flow"),
        "a checkout's flow never reads as belonging to none"
    );
}

#[test]
fn outside_every_tree_is_a_place_the_reading_names() {
    let yard = a_small_world();
    let read = reading(&yard);

    let inside = read
        .terminals
        .iter()
        .find(|terminal| terminal.tty == "ttys001")
        .expect("the terminal inside a tree");
    let Where::InATree { tree, project } = &inside.place else {
        panic!("{:?}", inside.place);
    };
    assert_eq!(Path::new(tree).file_name().unwrap(), "second");
    assert_eq!(project, "alpha");

    let outside = read
        .terminals
        .iter()
        .find(|terminal| terminal.tty == "ttys002")
        .expect("the terminal outside every tree");
    assert_eq!(outside.place, Where::OutsideEveryTree);

    let written = serde_json::to_value(&read.terminals).expect("the reading serialises");
    assert_eq!(
        written[1]["place"], "outside_every_tree",
        "outside is a value, never a missing field: {written}"
    );
}

#[test]
fn what_would_not_read_is_said_and_never_drawn_as_an_empty_column() {
    let yard = a_small_world();
    let home = yard.root.join("home");
    std::fs::write(home.join(flow::workspace::REGISTER), "not a register")
        .expect("the register is spoilt");

    let read = column::take(&Ground {
        home: &home,
        home_flows: &home.join("flows"),
        standing_in: None,
        terminals: &[],
        boards: &[],
        troubles: &["the terminals would not be read".to_owned()],
    });

    assert!(read.projects.is_empty());
    assert_eq!(read.troubles.len(), 2, "{:?}", read.troubles);
    assert!(
        read.troubles
            .iter()
            .any(|why| why.contains("the terminals would not be read")),
        "what the caller could not read is kept: {:?}",
        read.troubles
    );
    assert!(
        read.troubles.iter().any(|why| why.contains("register")),
        "an unreadable register is not no projects: {:?}",
        read.troubles
    );
}
