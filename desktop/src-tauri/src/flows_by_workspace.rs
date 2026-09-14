//! What runs in every checkout of every workspace, read by the one precedence
//! `flow::system::resolve` holds: one pass per checkout, never a second rule.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Condvar, Mutex, OnceLock};
use std::time::{Duration, Instant};

use serde::Serialize;

/// How long a whole reading may take, from the first folder to the last checkout.
pub(crate) const A_READING_TAKES_AT_MOST: Duration = Duration::from_secs(5);

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

/// Why something carries no rows. Facts, not sentences: the window words them.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub(crate) enum Refusal {
    Unreadable { why: String },
    TimedOut { seconds: u64 },
    Stopped,
}

/// A source outside every checkout that did not answer, and was left out.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub(crate) struct Unanswered {
    pub origin: &'static str,
    pub dir: PathBuf,
    pub refused: Refusal,
}

/// One place flows are resolved from. `flows` holds only the names whose
/// winner differs from the one outside every workspace.
#[derive(Debug, Clone, Serialize)]
pub(crate) struct Context {
    pub workspace: Option<String>,
    pub root: Option<String>,
    pub branch: Option<String>,
    /// Git did not answer in time: no branch is known, which is not «no branch».
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub branch_unread: bool,
    pub current: bool,
    pub flows: Vec<FlowRow>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub troubles: Vec<flow::system::SourceTrouble>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub unanswered: Vec<Unanswered>,
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
            limit: A_READING_TAKES_AT_MOST,
        }
    }

    fn home_flows(&self) -> Option<PathBuf> {
        self.home.as_ref().map(|home| home.join("flows"))
    }
}

/// The trees git lists for a root, each with its branch; `None` where git has none.
pub(crate) type Listing = Option<Vec<(PathBuf, Option<String>)>>;

/// One pass of `resolve` over some sources, and what the disk refused on the way.
#[derive(Debug, Clone)]
pub(crate) struct Resolution {
    rows: Vec<FlowRow>,
    troubles: Vec<flow::system::SourceTrouble>,
    root_unreadable: Option<String>,
}

/// Everything a reading asks of the disk and of git, behind one seam.
pub(crate) trait Disk: Send + Sync + 'static {
    fn trees(&self, root: &Path) -> Listing;
    fn resolve(&self, sources: &[flow::system::FlowSource], resolved_in: Option<&Path>) -> Resolution;
}

pub(crate) struct TheDisk;

impl Disk for TheDisk {
    fn trees(&self, root: &Path) -> Listing {
        if !root.join(".git").exists() {
            return None;
        }
        let trees = workspace::list(root).ok()?;
        Some(
            trees
                .into_iter()
                .map(|tree| (PathBuf::from(tree.path), tree.branch))
                .collect(),
        )
    }

    fn resolve(&self, sources: &[flow::system::FlowSource], resolved_in: Option<&Path>) -> Resolution {
        if let Some(Err(why)) = resolved_in.map(std::fs::read_dir) {
            return Resolution {
                rows: Vec::new(),
                troubles: Vec::new(),
                root_unreadable: Some(why.to_string()),
            };
        }
        Resolution {
            rows: rows_of(sources, resolved_in),
            troubles: flow::system::unread_sources(sources),
            root_unreadable: None,
        }
    }
}

struct Flight<R> {
    answer: Mutex<Option<Option<R>>>,
    landed: Condvar,
}

impl<R: Clone> Flight<R> {
    fn wait_until(&self, until: Instant, seconds: u64) -> Result<R, Refusal> {
        let mut answer = self.answer.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        loop {
            if let Some(landed) = answer.as_ref() {
                return landed.clone().ok_or(Refusal::Stopped);
            }
            let left = until.saturating_duration_since(Instant::now());
            if left.is_zero() {
                return Err(Refusal::TimedOut { seconds });
            }
            answer = match self.landed.wait_timeout(answer, left) {
                Ok((guard, _)) => guard,
                Err(poisoned) => poisoned.into_inner().0,
            };
        }
    }
}

/// **ONE READING IN FLIGHT PER KEY.** A second asker waits on the first instead of
/// starting another, so a stalled folder holds one thread however often it is asked.
pub(crate) struct Flights<R> {
    open: Mutex<HashMap<String, Arc<Flight<R>>>>,
    live: AtomicUsize,
}

impl<R: Clone + Send + 'static> Flights<R> {
    fn new() -> Self {
        Flights {
            open: Mutex::new(HashMap::new()),
            live: AtomicUsize::new(0),
        }
    }

    fn ask(&'static self, key: String, work: impl FnOnce() -> R + Send + 'static) -> Arc<Flight<R>> {
        let mut open = self.open.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        if let Some(flight) = open.get(&key) {
            return Arc::clone(flight);
        }
        let flight = Arc::new(Flight {
            answer: Mutex::new(None),
            landed: Condvar::new(),
        });
        open.insert(key.clone(), Arc::clone(&flight));
        drop(open);
        self.live.fetch_add(1, Ordering::SeqCst);
        let landing = Arc::clone(&flight);
        std::thread::spawn(move || {
            let answer = std::panic::catch_unwind(std::panic::AssertUnwindSafe(work)).ok();
            *landing.answer.lock().unwrap_or_else(|poisoned| poisoned.into_inner()) = Some(answer);
            landing.landed.notify_all();
            let mut open = self.open.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
            if open.get(&key).is_some_and(|held| Arc::ptr_eq(held, &landing)) {
                open.remove(&key);
            }
            drop(open);
            self.live.fetch_sub(1, Ordering::SeqCst);
        });
        flight
    }
}

/// The readings of the Flows place in flight, whoever asked for them.
pub(crate) struct Crew {
    trees: Flights<Listing>,
    resolutions: Flights<Resolution>,
}

impl Crew {
    pub(crate) fn new() -> Self {
        Crew {
            trees: Flights::new(),
            resolutions: Flights::new(),
        }
    }

    /// Readings started and not yet finished, answered or abandoned.
    pub(crate) fn live(&self) -> usize {
        self.trees.live.load(Ordering::SeqCst) + self.resolutions.live.load(Ordering::SeqCst)
    }
}

fn the_crew() -> &'static Crew {
    static CREW: OnceLock<Crew> = OnceLock::new();
    CREW.get_or_init(Crew::new)
}

/// **ONE DEADLINE FOR THE WHOLE READING.** Folders outside the checkouts and git
/// share its first half; nothing waits past `last`.
#[derive(Debug, Clone, Copy)]
struct Deadline {
    first: Instant,
    last: Instant,
    seconds: u64,
}

impl Deadline {
    fn from(limit: Duration) -> Self {
        let now = Instant::now();
        Deadline {
            first: now + limit / 2,
            last: now + limit,
            seconds: limit.as_millis().div_ceil(1000) as u64,
        }
    }
}

/// The checkout the window stands in, and outside every workspace. It asks
/// for no other checkout: opening the place must not wait on all of them.
#[tauri::command(async)]
pub(crate) fn flows_here() -> Reading {
    let disk: Arc<dyn Disk> = Arc::new(TheDisk);
    here(&Ground::from_env(Vec::new()), the_crew(), &disk)
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
    let disk: Arc<dyn Disk> = Arc::new(TheDisk);
    everywhere(&Ground::from_env(seen), the_crew(), &disk)
}

pub(crate) fn here(ground: &Ground, crew: &'static Crew, disk: &Arc<dyn Disk>) -> Reading {
    let deadline = Deadline::from(ground.limit);
    let home_flows = ground.home_flows();
    let declared = ground.declared.as_deref();
    let outside_sources = flow::system::sources(home_flows.as_deref(), None, declared);
    let outside_flight = ask_resolution(crew, disk, outside_sources.clone(), None);
    let root = ground.working.as_deref().and_then(flow::workspace::find_root);
    let trees_flight = root.clone().map(|root| ask_trees(crew, disk, root));
    let outside = outside_from(&outside_flight, outside_sources, deadline);

    let (Some(root), Some(trees_flight)) = (root, trees_flight) else {
        return Reading {
            outside: outside.context,
            contexts: Vec::new(),
        };
    };
    let (branch, branch_unread) = match trees_flight.wait_until(deadline.first, deadline.seconds) {
        Ok(listing) => (branch_in(listing.as_deref(), &root), false),
        Err(_) => (None, true),
    };
    let checkout = Checkout {
        workspace: workspace_name(&root),
        root,
        branch,
        current: true,
    };
    let mut contexts = resolve_checkouts(crew, disk, &[checkout], home_flows.as_deref(), declared, &outside, deadline);
    for context in &mut contexts {
        context.branch_unread = branch_unread;
    }
    Reading {
        outside: outside.context,
        contexts,
    }
}

pub(crate) fn everywhere(ground: &Ground, crew: &'static Crew, disk: &Arc<dyn Disk>) -> Result<Reading, String> {
    let deadline = Deadline::from(ground.limit);
    let known = match &ground.home {
        Some(home) => flow::workspace::known_including(home, &ground.seen, 0)?,
        None => Vec::new(),
    };
    let standing = ground
        .working
        .as_deref()
        .and_then(flow::workspace::find_root)
        .map(|root| settled(&root));
    let home_flows = ground.home_flows();
    let declared = ground.declared.as_deref();
    let outside_sources = flow::system::sources(home_flows.as_deref(), None, declared);
    let outside_flight = ask_resolution(crew, disk, outside_sources.clone(), None);
    let listings: Vec<_> = known
        .iter()
        .map(|entry| ask_trees(crew, disk, entry.root.clone()))
        .collect();
    let outside = outside_from(&outside_flight, outside_sources, deadline);

    let mut seen: Vec<PathBuf> = Vec::new();
    let mut checkouts = Vec::new();
    let mut refused = Vec::new();
    for (entry, listing) in known.iter().zip(&listings) {
        let whole = Checkout {
            workspace: entry.name.clone(),
            root: entry.root.clone(),
            branch: None,
            current: false,
        };
        let trees = match listing.wait_until(deadline.first, deadline.seconds) {
            Err(refusal) => {
                refused.push(refused_context(&whole, refusal));
                continue;
            }
            Ok(Some(trees)) if !trees.is_empty() => trees,
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

    let mut contexts = resolve_checkouts(crew, disk, &checkouts, home_flows.as_deref(), declared, &outside, deadline);
    contexts.extend(refused);
    Ok(Reading {
        outside: outside.context,
        contexts,
    })
}

/// Outside every workspace, and the sources left out because they did not answer.
pub(crate) struct Outside {
    context: Context,
    skipped: Vec<flow::system::FlowSource>,
}

fn outside_from(
    flight: &Flight<Resolution>,
    sources: Vec<flow::system::FlowSource>,
    deadline: Deadline,
) -> Outside {
    let blank = Context {
        workspace: None,
        root: None,
        branch: None,
        branch_unread: false,
        current: false,
        flows: Vec::new(),
        troubles: Vec::new(),
        unanswered: Vec::new(),
        refused: None,
    };
    match flight.wait_until(deadline.first, deadline.seconds) {
        Ok(resolution) => Outside {
            context: Context {
                flows: resolution.rows,
                troubles: resolution.troubles,
                ..blank
            },
            skipped: Vec::new(),
        },
        Err(refusal) => {
            // The shipped flows live in the binary, so they are resolved here with no disk at all.
            let (on_disk, shipped): (Vec<_>, Vec<_>) = sources.into_iter().partition(|source| !source.is_builtin());
            Outside {
                context: Context {
                    flows: rows_of(&shipped, None),
                    unanswered: on_disk
                        .iter()
                        .map(|source| Unanswered {
                            origin: source.origin,
                            dir: source.dir.clone(),
                            refused: refusal.clone(),
                        })
                        .collect(),
                    ..blank
                },
                skipped: on_disk,
            }
        }
    }
}

/// Every checkout asked at once, then each waited for until the one deadline.
pub(crate) fn resolve_checkouts(
    crew: &'static Crew,
    disk: &Arc<dyn Disk>,
    checkouts: &[Checkout],
    home_flows: Option<&Path>,
    declared: Option<&Path>,
    outside: &Outside,
    deadline: Deadline,
) -> Vec<Context> {
    let flights: Vec<_> = checkouts
        .iter()
        .map(|checkout| {
            let mut sources = flow::system::sources(home_flows, Some(&checkout.root), declared);
            sources.retain(|source| !outside.skipped.contains(source));
            ask_resolution(crew, disk, sources, Some(checkout.root.clone()))
        })
        .collect();
    checkouts
        .iter()
        .zip(flights)
        .map(|(checkout, flight)| match flight.wait_until(deadline.last, deadline.seconds) {
            Err(refusal) => refused_context(checkout, refusal),
            Ok(Resolution {
                root_unreadable: Some(why),
                ..
            }) => refused_context(checkout, Refusal::Unreadable { why }),
            Ok(resolution) => Context {
                flows: resolution
                    .rows
                    .into_iter()
                    .filter(|row| {
                        !outside
                            .context
                            .flows
                            .iter()
                            .any(|there| there.chain.name == row.chain.name && there.chain.winner == row.chain.winner)
                    })
                    .collect(),
                // Home and a declared folder are said once, outside; here only the checkout's own.
                troubles: resolution
                    .troubles
                    .into_iter()
                    .filter(|trouble| {
                        trouble.origin != flow::system::YOUR_ORIGIN && trouble.origin != flow::system::DECLARED_ORIGIN
                    })
                    .collect(),
                refused: None,
                ..refused_context(checkout, Refusal::Stopped)
            },
        })
        .collect()
}

fn ask_resolution(
    crew: &'static Crew,
    disk: &Arc<dyn Disk>,
    sources: Vec<flow::system::FlowSource>,
    resolved_in: Option<PathBuf>,
) -> Arc<Flight<Resolution>> {
    let key = format!("{sources:?}\n{resolved_in:?}");
    let disk = Arc::clone(disk);
    crew.resolutions
        .ask(key, move || disk.resolve(&sources, resolved_in.as_deref()))
}

fn ask_trees(crew: &'static Crew, disk: &Arc<dyn Disk>, root: PathBuf) -> Arc<Flight<Listing>> {
    let key = root.to_string_lossy().into_owned();
    let disk = Arc::clone(disk);
    crew.trees.ask(key, move || disk.trees(&root))
}

fn refused_context(checkout: &Checkout, refusal: Refusal) -> Context {
    Context {
        workspace: Some(checkout.workspace.clone()),
        root: Some(checkout.root.to_string_lossy().into_owned()),
        branch: checkout.branch.clone(),
        branch_unread: false,
        current: checkout.current,
        flows: Vec::new(),
        troubles: Vec::new(),
        unanswered: Vec::new(),
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

fn branch_in(trees: Option<&[(PathBuf, Option<String>)]>, root: &Path) -> Option<String> {
    let place = settled(root);
    trees?
        .iter()
        .find(|(path, _)| settled(path) == place)?
        .1
        .clone()
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

    /// A crew of its own per test: the counter of one test is not moved by another.
    fn crew() -> &'static Crew {
        Box::leak(Box::new(Crew::new()))
    }

    fn the_disk() -> Arc<dyn Disk> {
        Arc::new(TheDisk)
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

    /// Outside every workspace, then these checkouts, through the crew and the real disk.
    fn take(home_flows: Option<&Path>, declared: Option<&Path>, checkouts: Vec<Checkout>) -> Reading {
        let (crew, disk, deadline) = (crew(), the_disk(), Deadline::from(PLENTY));
        let sources = flow::system::sources(home_flows, None, declared);
        let flight = ask_resolution(crew, &disk, sources.clone(), None);
        let outside = outside_from(&flight, sources, deadline);
        let contexts = resolve_checkouts(crew, &disk, &checkouts, home_flows, declared, &outside, deadline);
        Reading {
            outside: outside.context,
            contexts,
        }
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

    /// A disk that takes its time: listing, resolving, or only resolving one folder.
    struct SlowDisk {
        listing: Duration,
        resolving: Duration,
        only_where: Option<PathBuf>,
    }

    impl Disk for SlowDisk {
        fn trees(&self, root: &Path) -> Listing {
            std::thread::sleep(self.listing);
            Some(vec![(root.to_path_buf(), Some("main".to_owned()))])
        }

        fn resolve(&self, sources: &[flow::system::FlowSource], resolved_in: Option<&Path>) -> Resolution {
            let slow = match &self.only_where {
                Some(dir) => sources.iter().any(|source| &source.dir == dir),
                None => true,
            };
            if slow {
                std::thread::sleep(self.resolving);
            }
            TheDisk.resolve(sources, resolved_in)
        }
    }

    /// A disk that answers nothing until the gate opens.
    struct GatedDisk {
        gate: Arc<(Mutex<bool>, Condvar)>,
    }

    impl GatedDisk {
        fn hold(&self) {
            let (open, opened) = &*self.gate;
            let mut open = open.lock().expect("the gate");
            while !*open {
                open = opened.wait(open).expect("the gate");
            }
        }
    }

    impl Disk for GatedDisk {
        fn trees(&self, root: &Path) -> Listing {
            self.hold();
            Some(vec![(root.to_path_buf(), None)])
        }

        fn resolve(&self, sources: &[flow::system::FlowSource], resolved_in: Option<&Path>) -> Resolution {
            self.hold();
            TheDisk.resolve(sources, resolved_in)
        }
    }

    /// Two workspaces remembered in a home, each a folder with a marker and a `.git`.
    fn two_workspaces(scratch: &Path) -> (PathBuf, Vec<PathBuf>) {
        let home = scratch.join("home");
        std::fs::create_dir_all(&home).expect("the home");
        let roots: Vec<PathBuf> = ["one", "two"]
            .iter()
            .map(|name| {
                let root = checkout(scratch, name, false).root;
                std::fs::create_dir_all(root.join(".git")).expect("a .git");
                flow::workspace::remember_in(&home, &root, 1).expect("the workspace is remembered");
                root
            })
            .collect();
        (home, roots)
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

        let reading = take(Some(&home_flows), None, vec![plain.clone(), overriding.clone(), gone]);

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

        let reading = take(None, None, vec![tree]);

        let row = &reading.contexts[0].flows[0];
        assert_eq!(row.chain.name, "half-written");
        assert_eq!(row.steps, None);
        assert!(row.broken.is_some());
    }

    /// **ONE DEADLINE, FROM THE LIST TO THE LAST CHECKOUT.** A slow list and a slow
    /// resolution together end at the limit, not at twice it.
    #[test]
    fn one_deadline_holds_the_whole_reading_from_the_list_to_the_resolution() {
        let scratch = Scratch::new("one-deadline");
        let (home, roots) = two_workspaces(&scratch.0);
        let limit = Duration::from_millis(1000);
        let ground = Ground {
            home: Some(home),
            declared: None,
            working: None,
            seen: Vec::new(),
            limit,
        };

        let quick_enough: Arc<dyn Disk> = Arc::new(SlowDisk {
            listing: Duration::from_millis(300),
            resolving: Duration::from_millis(300),
            only_where: None,
        });
        let started = Instant::now();
        let reading = everywhere(&ground, crew(), &quick_enough).expect("the register reads");
        assert!(started.elapsed() < limit + Duration::from_millis(300), "took {:?}", started.elapsed());
        assert_eq!(reading.contexts.len(), roots.len());
        for context in &reading.contexts {
            assert!(context.refused.is_none(), "read within the limit: {context:?}");
        }

        let too_slow: Arc<dyn Disk> = Arc::new(SlowDisk {
            listing: Duration::from_millis(700),
            resolving: Duration::from_millis(900),
            only_where: None,
        });
        let started = Instant::now();
        let reading = everywhere(&ground, crew(), &too_slow).expect("the register reads");
        let took = started.elapsed();
        assert!(took < limit + Duration::from_millis(300), "a slow list and a slow resolution took {took:?}");
        assert_eq!(reading.contexts.len(), roots.len(), "no checkout is dropped");
        for context in &reading.contexts {
            assert_eq!(context.refused, Some(Refusal::TimedOut { seconds: 1 }), "{context:?}");
        }
    }

    /// **TEN CHANGES OF MODE HOLD NO MORE READINGS THAN THERE ARE THINGS TO READ.**
    #[test]
    fn asking_again_joins_the_reading_in_flight_instead_of_starting_another() {
        let scratch = Scratch::new("in-flight");
        let (home, roots) = two_workspaces(&scratch.0);
        let gate = Arc::new((Mutex::new(false), Condvar::new()));
        let disk: Arc<dyn Disk> = Arc::new(GatedDisk { gate: Arc::clone(&gate) });
        let crew = crew();
        let ground = Ground {
            home: Some(home),
            declared: None,
            working: Some(roots[0].clone()),
            seen: Vec::new(),
            limit: Duration::from_millis(40),
        };

        let mut after_the_first_pair = 0;
        for change in 0..10 {
            if change % 2 == 0 {
                let _ = everywhere(&ground, crew, &disk).expect("the register reads");
            } else {
                let _ = here(&ground, crew, &disk);
            }
            if change == 1 {
                after_the_first_pair = crew.live();
            }
        }
        let held = crew.live();
        // Outside, the two listings, and the checkout the window stands in: `here`
        // resolves it even while git has not answered, with its branch unread.
        let things_to_read = 1 + roots.len() + 1;
        assert!(held > 0, "the gated readings are in flight");
        assert_eq!(held, after_the_first_pair, "eight more changes of mode started more readings");
        assert!(held <= things_to_read, "{held} readings in flight for {things_to_read} things to read");

        let (open, opened) = &*gate;
        *open.lock().expect("the gate") = true;
        opened.notify_all();
        let waited = Instant::now();
        while crew.live() > 0 && waited.elapsed() < Duration::from_secs(5) {
            std::thread::sleep(Duration::from_millis(10));
        }
        assert_eq!(crew.live(), 0, "every reading ends once the disk answers");
    }

    /// **A FOLDER OF YOURS THAT DOES NOT ANSWER DOES NOT HOLD THE PLACE**: it is
    /// said, the shipped flows and the checkout's own are shown.
    #[test]
    fn a_source_that_does_not_answer_is_said_and_the_rest_is_shown() {
        let scratch = Scratch::new("stalled-home");
        let home = scratch.0.join("home");
        let home_flows = home.join("flows");
        write_flow(&home_flows, "a-flow-of-yours", 1);
        let tree = checkout(&scratch.0, "tree", true);
        write_flow(&tree.root.join("flows"), "a-project-flow", 2);
        let limit = Duration::from_millis(600);
        let disk: Arc<dyn Disk> = Arc::new(SlowDisk {
            listing: Duration::ZERO,
            resolving: Duration::from_secs(4),
            only_where: Some(home_flows.clone()),
        });
        let ground = Ground {
            home: Some(home),
            declared: None,
            working: Some(tree.root.clone()),
            seen: Vec::new(),
            limit,
        };

        let started = Instant::now();
        let reading = here(&ground, crew(), &disk);

        assert!(started.elapsed() < limit + Duration::from_millis(300), "took {:?}", started.elapsed());
        assert_eq!(
            reading.outside.unanswered,
            vec![Unanswered {
                origin: flow::system::YOUR_ORIGIN,
                dir: home_flows,
                refused: Refusal::TimedOut { seconds: 1 },
            }]
        );
        assert!(reading.outside.flows.iter().any(|row| row.chain.name == shipped()), "the shipped flows are shown");
        assert!(reading.outside.flows.iter().all(|row| row.chain.name != "a-flow-of-yours"));
        assert_eq!(reading.contexts.len(), 1);
        assert!(reading.contexts[0].refused.is_none(), "{:?}", reading.contexts[0].refused);
        assert!(reading.contexts[0].flows.iter().any(|row| row.chain.name == "a-project-flow"));
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

        let reading = everywhere(&ground, crew(), &the_disk()).expect("the register reads");
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

        let standing = here(&ground, crew(), &the_disk());
        assert_eq!(standing.contexts.len(), 1, "the checkout view reads one checkout: {:?}", standing.contexts);
        assert_eq!(standing.contexts[0].root.as_deref(), Some(other.to_string_lossy().as_ref()));
        assert_eq!(standing.contexts[0].branch.as_deref(), Some("work/an-override"));
        assert!(!standing.contexts[0].branch_unread);
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

        let reading = here(&ground, crew(), &the_disk());
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
