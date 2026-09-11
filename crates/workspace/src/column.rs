//! The whole left column in one reading: the projects, the several trees of
//! each, the flows that belong to no checkout, and where every terminal
//! stands. One answer, because a column assembled from four readings shows
//! four different instants of the same machine.

use serde::Serialize;
use std::path::{Path, PathBuf};

/// What the caller already knows. The reading asks the disk for nothing it
/// could have been handed: the terminals live in a register this crate must
/// not open, and a board is counted by whoever holds the ledger.
pub struct Ground<'a> {
    pub home: &'a Path,
    pub home_flows: &'a Path,
    /// The tree this reading is being taken from, when there is one.
    pub standing_in: Option<&'a Path>,
    pub terminals: &'a [TerminalAt],
    /// How many board entries stand at a path, tree by tree.
    pub boards: &'a [(String, usize)],
    /// What the caller itself could not read. It travels in the reading
    /// because a caller that answers with an empty list says «there is
    /// nothing» when the truth is «I could not look».
    pub troubles: &'a [String],
}

/// A terminal and the directory it was opened in, as its register holds it.
#[derive(Debug, Clone, Serialize)]
pub struct TerminalAt {
    pub tty: String,
    pub worktree: String,
}

/// Whether the thing is where it says it is. **A drawable answer, never an
/// error**: a tree deleted underneath a project is an ordinary row.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Standing {
    Here,
    Gone,
}

#[derive(Debug, Clone, Serialize)]
pub struct FlowLine {
    pub id: String,
    pub steps: usize,
    pub origin: &'static str,
    /// Why this flow will not load. A broken flow is drawn saying so; dropping
    /// it leaves a short list nobody can tell is short.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub trouble: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct Tree {
    pub name: String,
    pub path: String,
    /// `None` on a detached head: a state with no branch to go back to.
    pub branch: Option<String>,
    pub stood_in: bool,
    pub standing: Standing,
    pub board: usize,
    pub flows: Vec<FlowLine>,
}

#[derive(Debug, Clone, Serialize)]
pub struct Project {
    pub name: String,
    pub root: String,
    pub standing: Standing,
    pub trees: Vec<Tree>,
}

/// Where a terminal stands. Outside every tree is a value, so a reader never
/// has to tell "nowhere" from "the field was not written".
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Where {
    InATree { tree: String, project: String },
    OutsideEveryTree,
}

#[derive(Debug, Clone, Serialize)]
pub struct Terminal {
    pub tty: String,
    pub place: Where,
}

#[derive(Debug, Clone, Serialize)]
pub struct Reading {
    pub projects: Vec<Project>,
    pub flows_everywhere: Vec<FlowLine>,
    pub terminals: Vec<Terminal>,
    /// What would not read, the caller's own and this reading's. An empty
    /// column and a column nobody could look at are two different facts and
    /// are never one.
    pub troubles: Vec<String>,
}

/// The whole column, in one pass.
pub fn take(ground: &Ground) -> Reading {
    let mut troubles = ground.troubles.to_vec();
    let known = match flow::workspace::known_in(ground.home) {
        Ok(known) => known,
        Err(why) => {
            troubles.push(why);
            Vec::new()
        }
    };
    let projects: Vec<Project> = known
        .iter()
        .map(|entry| project_of(entry, ground))
        .collect();
    Reading {
        terminals: terminals_of(ground, &projects),
        flows_everywhere: flows_of_no_checkout(ground.home_flows),
        projects,
        troubles,
    }
}

fn project_of(known: &flow::workspace::Known, ground: &Ground) -> Project {
    let standing = match flow::workspace::standing_of(known) {
        flow::workspace::Standing::Declared => Standing::Here,
        flow::workspace::Standing::Gone => Standing::Gone,
    };
    Project {
        name: known.name.clone(),
        root: known.root.to_string_lossy().into_owned(),
        standing,
        trees: trees_of(&known.root, ground),
    }
}

/// The checkouts of a project. **The repository is asked only where one is
/// declared**: run from a directory that is not a checkout, git answers about
/// whatever repository happens to be above it, and those trees are somebody
/// else's.
fn trees_of(root: &Path, ground: &Ground) -> Vec<Tree> {
    if !root.join(".git").exists() {
        return Vec::new();
    }
    crate::list(root)
        .unwrap_or_default()
        .into_iter()
        .map(|tree| tree_of(&tree, ground))
        .collect()
}

fn tree_of(tree: &crate::Worktree, ground: &Ground) -> Tree {
    let path = PathBuf::from(&tree.path);
    let standing = if path.is_dir() {
        Standing::Here
    } else {
        Standing::Gone
    };
    let stood_in = match ground.standing_in {
        Some(here) => same_place(here, &path),
        None => false,
    };
    let board = ground
        .boards
        .iter()
        .filter(|(at, _)| inside(Path::new(at), &path))
        .map(|(_, held)| held)
        .sum();
    Tree {
        name: tree.name().to_owned(),
        path: tree.path.clone(),
        branch: tree.branch.clone(),
        stood_in,
        standing,
        board,
        flows: flows_of(&path),
    }
}

fn flows_of(tree: &Path) -> Vec<FlowLine> {
    let origin = if tree.join(flow::workspace::MARKER).is_file() {
        flow::workspace::ORIGIN_DECLARED
    } else {
        flow::workspace::ORIGIN_GUESSED
    };
    lines(flow::system::load_registry(&tree.join("flows")), origin)
}

/// The flows no checkout owns: the ones shipped, then the person's own.
fn flows_of_no_checkout(home_flows: &Path) -> Vec<FlowLine> {
    let mut everywhere = lines(
        flow::system::builtin_registry(),
        flow::system::BUILTIN_ORIGIN,
    );
    everywhere.extend(lines(
        flow::system::load_registry(home_flows),
        flow::system::YOUR_ORIGIN,
    ));
    everywhere
}

fn lines(registry: flow::system::FlowRegistry, origin: &'static str) -> Vec<FlowLine> {
    registry
        .into_iter()
        .map(|(id, entry)| match entry {
            Ok(flow) => FlowLine {
                id,
                steps: flow.graph.steps().len(),
                origin,
                trouble: None,
            },
            Err(why) => FlowLine {
                id,
                steps: 0,
                origin,
                trouble: Some(why),
            },
        })
        .collect()
}

fn terminals_of(ground: &Ground, projects: &[Project]) -> Vec<Terminal> {
    ground
        .terminals
        .iter()
        .map(|at| Terminal {
            tty: at.tty.clone(),
            place: place_of(Path::new(&at.worktree), projects),
        })
        .collect()
}

/// The deepest tree holding this directory, or outside every one of them. A
/// terminal is rarely opened at the top of its checkout, and a checkout beside
/// another whose name starts the same way is not inside it.
fn place_of(stands: &Path, projects: &[Project]) -> Where {
    let mut deepest: Option<(&Project, &Tree)> = None;
    for project in projects {
        for tree in &project.trees {
            if !inside(stands, Path::new(&tree.path)) {
                continue;
            }
            let deeper = match deepest {
                None => true,
                Some((_, held)) => tree.path.len() > held.path.len(),
            };
            if deeper {
                deepest = Some((project, tree));
            }
        }
    }
    match deepest {
        Some((project, tree)) => Where::InATree {
            tree: tree.path.clone(),
            project: project.name.clone(),
        },
        None => Where::OutsideEveryTree,
    }
}

/// The path with its links followed, where the disk can still answer. A tree
/// that is gone keeps the path it was written with, which is the one thing
/// that makes it findable again.
fn settled(path: &Path) -> PathBuf {
    path.canonicalize().unwrap_or_else(|_| path.to_path_buf())
}

fn same_place(one: &Path, other: &Path) -> bool {
    settled(one) == settled(other)
}

fn inside(place: &Path, tree: &Path) -> bool {
    settled(place).starts_with(settled(tree))
}
