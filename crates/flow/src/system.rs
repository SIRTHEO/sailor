//! Where flows come from, and which ones the product ships with. One answer
//! lives here and whoever wants it imports it: the answer used to live in
//! `ui::gather`, which held while only the window asked and stopped holding
//! once a flow step had to ask too. Shipped flows are embedded in the binary,
//! so there is no install path to guess wrong, and they are not switched off
//! but overridden by name — the way to change a shipped flow is to write one.

use crate::FlowFile;
use std::collections::BTreeMap;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

/// What gets written under "where" for the system source.
///
/// Not a path, and it must not look like one. Shipped flows are not in a
/// folder, they are inside the binary. Whoever shows the sources shows this
/// line too, and a plausible folder that does not exist would send someone
/// hunting for files in it — or creating some, where nobody would read them.
pub const PLACE: &str = "(shipped with the product)";

/// What the system source is called for a reader.
///
/// **THE ORIGIN FAMILY IS HERE AND IN [`crate::workspace`], AND IT MOVES
/// TOGETHER.** These are literals other code compares, so a member moves only in
/// the same edit as every assertion naming it. Two of them had no constant and
/// were written out where used, which is how a family drifts into four literals.
pub const BUILTIN_ORIGIN: &str = "built in";

/// Flows from the folder `SAILOR_FLOWS` names. Declaring that variable is a
/// statement about where *your* flows are, so it replaces home and project both.
pub const DECLARED_ORIGIN: &str = "declared";

/// Flows from the reader's own home, which is where they land with no variable
/// set and nothing declared.
pub const YOUR_ORIGIN: &str = "yours";

/// The shipped flow that turns a broken run into a line of the fault register.
/// Named here because the beat starts it by itself, and must never start it
/// about its own failures.
pub const FAULT_WRITER: &str = "write-down-what-broke";

/// The flows the product ships with: flow name and file text. Embedded like
/// `toolbox::descriptor::BUILTIN` and for the same reason — a freshly installed
/// binary, or one copied to another machine, has to answer without anyone
/// having copied a folder. The name is what would read on disk without
/// `.flow.json`, and a test checks it matches the `id` declared inside: two
/// names for one thing would make "run this" and "override this" differ.
pub const FLOWS: &[(&str, &str)] = &[
    (
        "what-this-machine-has",
        include_str!("../system/what-this-machine-has.flow.json"),
    ),
    // A hard question taken to a strong model with the material already in
    // hand. The refusal of an empty brief is a step, not a comment: the same
    // engine on the same question spent its whole window exploring and
    // answered nothing.
    (
        "consult-a-strong-model",
        include_str!("../system/consult-a-strong-model.flow.json"),
    ),
    (
        "migrate-to-sailor",
        include_str!("../system/migrate-to-sailor.flow.json"),
    ),
    // A project's queue, drained one task at a time: the walkthrough presents
    // it as a home flow of the product, not a project's own.
    (
        "take-the-next-work",
        include_str!("../system/take-the-next-work.flow.json"),
    ),
    // Shipped because the shipped rules name it: the routing rules in
    // `crates/terminal/descriptors/default.json` travel inside the binary and
    // send work here, so as a project flow the rule pointed at nothing on every
    // machine but this one — and no test saw it, since all of them ran here.
    (
        "dispatch-the-work",
        include_str!("../system/dispatch-the-work.flow.json"),
    ),
    // Mechanical work on one file by the local runner, handed over as a
    // proposal: the chain names no paid subscription.
    (
        "sweep-the-tree",
        include_str!("../system/sweep-the-tree.flow.json"),
    ),
    // What follows a fault is a check, and nobody writes it while repairing:
    // this asks the ledger how a run broke and leaves the line in the register.
    (
        FAULT_WRITER,
        include_str!("../system/write-down-what-broke.flow.json"),
    ),
    // The watch this terminal keeps: three readings and no engine, so it can
    // run at every beat without costing a call.
    (
        "watch-the-crew",
        include_str!("../system/watch-the-crew.flow.json"),
    ),
    // What the machine has left, and what Sailor can give back.
    (
        "free-the-machine",
        include_str!("../system/free-the-machine.flow.json"),
    ),
    (
        "draft-a-flow",
        include_str!("../system/draft-a-flow.flow.json"),
    ),
    // One agent, one task, one named account; the child of the flow below.
    (
        "one-agent-on-one-task",
        include_str!("../system/one-agent-on-one-task.flow.json"),
    ),
    // The crew is declared in the trigger, one entry per agent.
    (
        "put-the-crew-to-work",
        include_str!("../system/put-the-crew-to-work.flow.json"),
    ),
    // Once a day an engine reads every memory and says what to keep and drop.
    (
        "consolidate-memories",
        include_str!("../system/consolidate-memories.flow.json"),
    ),
    // The oldest open fault becomes the mandate; nothing open, no engine.
    (
        "take-the-next-fault",
        include_str!("../system/take-the-next-fault.flow.json"),
    ),
    // The ordinary gesture that reaches the two actions naming everything
    // else's dead powers — without this flow, they were their own example.
    (
        "find-the-dead-powers",
        include_str!("../system/find-the-dead-powers.flow.json"),
    ),
    (
        "remember-in-the-graph",
        include_str!("../system/remember-in-the-graph.flow.json"),
    ),
    (
        "recall-a-decision",
        include_str!("../system/recall-a-decision.flow.json"),
    ),
    (
        "notice-a-divergence",
        include_str!("../system/notice-a-divergence.flow.json"),
    ),
    // The relay's first subscriber: it asks, and asks for nothing else.
    (
        "ask-for-a-mandate",
        include_str!("../system/ask-for-a-mandate.flow.json"),
    ),
    // The delivery loop of docs/the-delivery-loop.md, one flow per outward step.
    (
        "open-the-draft-pull-request",
        include_str!("../system/open-the-draft-pull-request.flow.json"),
    ),
    (
        "review-a-pinned-commit",
        include_str!("../system/review-a-pinned-commit.flow.json"),
    ),
    (
        "integrate-on-the-trunk",
        include_str!("../system/integrate-on-the-trunk.flow.json"),
    ),
    (
        "cut-a-release",
        include_str!("../system/cut-a-release.flow.json"),
    ),
    (
        "close-the-work",
        include_str!("../system/close-the-work.flow.json"),
    ),
    // And the second, the only destructive one: it empties a session that has
    // handed on, and only once nobody is being waited for in there.
    (
        "empty-a-session-that-handed-on",
        include_str!("../system/empty-a-session-that-handed-on.flow.json"),
    ),
];

/// What the catalogue answers for a name it does not ship.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PastName {
    /// The shipped flow doing that work today.
    CarriedOnBy(&'static str),
    /// It existed, and nothing carries it: an answer, not a missing flow.
    NothingCarriesIt,
}

/// Names the ledger keeps runs under and no file answers to any more.
///
/// **READ ONLY WHERE NOTHING ELSE ANSWERS**, so no entry shadows a flow of
/// somebody's own. A successor is written only where `git log
/// --diff-filter=R` recorded the rename and the steps match: a plausible
/// pair is a guess, and it sends whoever asks to the wrong flow.
pub const PAST_NAMES: &[(&str, PastName)] = &[
    ("accendi-la-macchina", PastName::NothingCarriesIt),
    ("allinea-con-develop", PastName::NothingCarriesIt),
    ("c-e-una-via-per-notion", PastName::NothingCarriesIt),
    ("che-cosa-gira", PastName::NothingCarriesIt),
    ("che-sappiamo-di-noi", PastName::NothingCarriesIt),
    ("il-giro-di-prova", PastName::NothingCarriesIt),
    ("mandato-corrente", PastName::NothingCarriesIt),
    ("migrazione-a-sailor", PastName::CarriedOnBy("migrate-to-sailor")),
    ("passa-il-testimone", PastName::NothingCarriesIt),
    ("prova-dei-turni", PastName::NothingCarriesIt),
    ("prova-rapporto", PastName::NothingCarriesIt),
    ("prova-research-usa-e-getta", PastName::NothingCarriesIt),
    ("prova-scorre", PastName::NothingCarriesIt),
    ("qualcuno-ha-spinto-su-develop", PastName::NothingCarriesIt),
    ("rotto", PastName::NothingCarriesIt),
    ("route-a-breakage-mutant", PastName::NothingCarriesIt),
    ("smista-il-lavoro", PastName::CarriedOnBy("dispatch-the-work")),
    ("spegni-la-macchina", PastName::NothingCarriesIt),
    (
        "strumenti-di-questa-macchina",
        PastName::CarriedOnBy("what-this-machine-has"),
    ),
    ("try-dormant-steps", PastName::NothingCarriesIt),
    ("try-unused-actions", PastName::NothingCarriesIt),
];

/// What became of a name, for whoever asks the catalogue about one it does not
/// hold. `None` is a name this product never carried.
pub fn past_name(name: &str) -> Option<PastName> {
    PAST_NAMES
        .iter()
        .find(|(past, _)| *past == name)
        .map(|(_, became)| *became)
}

/// A place where flows are looked for, with the name a reader sees.
///
/// `dir` for the system source is [`PLACE`]: not a folder, and the only way to
/// keep one type for every source without suggesting shipped flows can be
/// changed by opening a file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FlowSource {
    pub origin: &'static str,
    pub dir: PathBuf,
}

impl FlowSource {
    /// The source of the flows shipped with the product.
    pub fn builtin() -> FlowSource {
        FlowSource {
            origin: BUILTIN_ORIGIN,
            dir: PathBuf::from(PLACE),
        }
    }

    /// True if this source is the one embedded in the binary.
    pub fn is_builtin(&self) -> bool {
        is_place(&self.dir)
    }
}

/// True if this "where" names the embedded flows instead of a folder.
pub fn is_place(dir: &Path) -> bool {
    dir == Path::new(PLACE)
}

/// The flows loaded from a registry: the valid ones, and the refused ones with
/// their reason. The same shape for disk and for embedded, because a reader
/// must not have to treat them differently.
pub type FlowRegistry = BTreeMap<String, Result<FlowFile, String>>;

/// Every place flows are looked for, least specific first. Order is the only
/// precedence rule: on a name clash the later source wins, so [`BUILTIN_ORIGIN`]
/// < [`YOUR_ORIGIN`] < [`crate::workspace::ORIGIN_DECLARED`].
///
/// `home_flows` is `None` when no home is known, and then there is no *yours*
/// source at all: a relative or invented folder would read somebody else's flows.
pub fn sources(
    home_flows: Option<&Path>,
    working: Option<&Path>,
    declared: Option<&Path>,
) -> Vec<FlowSource> {
    // The system source is here even under `SAILOR_FLOWS`, which says where
    // *your* flows are and duly makes home and project vanish. Shipped flows
    // sit in no folder, so there is no folder to replace: they are the binary's
    // own equipment, like the tool descriptors, where `SAILOR_TOOL_DESCRIPTORS`
    // adds and never removes `Source::Builtin`. A same-named flow in the
    // declared folder wins over one of them, and the origin says it happened.
    let mut sources = vec![FlowSource::builtin()];
    if let Some(declared) = declared.filter(|path| !path.as_os_str().is_empty()) {
        sources.push(FlowSource {
            origin: DECLARED_ORIGIN,
            dir: declared.to_path_buf(),
        });
        return sources;
    }
    if let Some(home_flows) = home_flows {
        sources.push(FlowSource {
            origin: YOUR_ORIGIN,
            dir: home_flows.to_path_buf(),
        });
    }
    if let Some((origin, dir)) = working.and_then(|working| project_flows(working, home_flows)) {
        sources.push(FlowSource { origin, dir });
    }
    sources
}

/// The project's flows directory and the origin to show: marker first, the old
/// `flows/` walk-up second, and the marker wins alone — consulting the walk-up
/// after it would let a `flows/` higher up override a project that declared
/// itself, which is to say the declaration would declare nothing. With a marker
/// `root.join("flows")` is the answer even when that folder does not exist: a
/// project with no flows is honest, and empty beats somebody else's.
fn project_flows(working: &Path, home_flows: Option<&Path>) -> Option<(&'static str, PathBuf)> {
    if let Some(root) = crate::workspace::find_root(working) {
        let flows = root.join("flows");
        // Home is never also the project: counting it twice would show every
        // flow in duplicate.
        return (Some(flows.as_path()) != home_flows).then_some((crate::workspace::ORIGIN_DECLARED, flows));
    }
    // The fallback stays: removing it would make the flows of every project
    // that has not declared itself vanish at once — this repository included,
    // until someone writes it a marker. That is a deprecation, and a
    // deprecation is not done alone: it stays, and the origin it carries says
    // out loud that it is a fallback.
    project_flows_from(working, home_flows).map(|flows| (crate::workspace::ORIGIN_GUESSED, flows))
}

/// The same sources, read from this process's environment: one copy of the
/// precedence rule, never two. The first copy is `ui::gather::flow_sources`;
/// the second was about to be born for whoever builds the action registry,
/// because the `subflow` step must look for the flow it calls exactly where
/// `sailor flow run` looks, or two machines run different flows under one name
/// without saying so. Home stays an argument: `flow` must not need `ledger`.
pub fn sources_from_env(home_flows: Option<&Path>) -> Vec<FlowSource> {
    let declared = std::env::var_os("SAILOR_FLOWS").map(PathBuf::from);
    let working = std::env::current_dir().ok();
    sources(home_flows, working.as_deref(), declared.as_deref())
}

/// The sentence a reader needs when no home was known and nothing was declared:
/// without it, a flow of theirs that is not found reads as a flow that is gone.
pub fn no_home_said(sources: &[FlowSource]) -> Option<String> {
    let looked_for_yours = sources
        .iter()
        .any(|source| source.origin == YOUR_ORIGIN || source.origin == DECLARED_ORIGIN);
    (!looked_for_yours).then(|| catalogue::say("flow.sources.no_home", &[]))
}

/// The project's flows directory, found by walking up — not a luxury: a program
/// is almost never started at the project root. The window starts in
/// `desktop/src-tauri`, an editor where its last file was, a terminal where the
/// user stood; measured, the window opened to work on Sailor saw none of
/// Sailor's four flows. The directory must hold a flow, not merely be named
/// `flows`, or an empty one stops the climb short of the real one.
pub fn project_flows_from(working: &Path, home_flows: Option<&Path>) -> Option<PathBuf> {
    let mut here = Some(working);
    while let Some(directory) = here {
        let candidate = directory.join("flows");
        if Some(candidate.as_path()) != home_flows && holds_a_flow(&candidate) {
            return Some(candidate);
        }
        here = directory.parent();
    }
    None
}

fn holds_a_flow(dir: &Path) -> bool {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return false;
    };
    entries.flatten().any(|entry| {
        entry
            .file_name()
            .to_str()
            .is_some_and(|name| name.ends_with(".flow.json"))
    })
}

/// The registry of one source, whatever it is.
///
/// The system source has no folder to read: it is recognised by its "where" and
/// served from the binary. It goes through here rather than a branch in the
/// caller because whoever shows the sources counts each one's entries, and a
/// forgotten branch would say "built in: 0 flows" while system flows run.
pub fn registry_of(source: &FlowSource) -> FlowRegistry {
    if source.is_builtin() {
        builtin_registry()
    } else {
        load_registry(&source.dir)
    }
}

/// The flows shipped with the product, read from the binary.
///
/// A shipped flow that will not read stays in the registry with its reason,
/// like a broken one on disk: making it vanish silently would mean a release
/// loses a flow with nobody noticing. The test below keeps that from reaching
/// whoever installs — it falls here first.
pub fn builtin_registry() -> FlowRegistry {
    let mut registry = FlowRegistry::new();
    for (name, text) in FLOWS {
        let entry = serde_json::from_str::<FlowFile>(text)
            .map_err(|error| format!("shipped flow \"{name}\" is not valid: {error}"));
        registry.insert((*name).to_owned(), entry);
    }
    registry
}

/// Reads the declarative flows in a directory. Every `*.flow.json` or `*.json`
/// enters the registry: valid ones loaded, unreadable or malformed ones kept
/// with the reason of refusal, so the window can show them marked. Skipping
/// them silently was defended as "the page must not break over a half-written
/// file" — but half written lasts milliseconds and broken is permanent, and
/// alike treatment leaves a short list nobody can tell is short.
pub fn load_registry(dir: &Path) -> FlowRegistry {
    flow_files_in(dir)
        .into_iter()
        .map(|(name, path)| {
            let entry = load_file(&path);
            (name, entry)
        })
        .collect()
}

/// One flow file, loaded, or the reason it will not load.
fn load_file(path: &Path) -> Result<FlowFile, String> {
    let text = fs::read_to_string(path)
        .map_err(|error| format!("cannot read {}: {error}", path.display()))?;
    serde_json::from_str::<FlowFile>(&text)
        .map_err(|error| format!("{} is not a valid flow: {error}", path.display()))
}

/// The flow files of a folder by the name the engine resolves them under: the
/// file name without `.flow.json`, or without `.json`. Two files of one name in
/// one folder keep the last the folder lists, as the registry always did.
fn flow_files_in(dir: &Path) -> BTreeMap<String, PathBuf> {
    let Ok(entries) = fs::read_dir(dir) else {
        return BTreeMap::new();
    };
    entries
        .flatten()
        .filter_map(|entry| {
            let path = entry.path();
            let file_name = path.file_name()?.to_string_lossy().into_owned();
            let name = file_name
                .strip_suffix(".flow.json")
                .or_else(|| file_name.strip_suffix(".json"))?
                .to_owned();
            (!name.is_empty()).then_some((name, path))
        })
        .collect()
}

/// One place a flow name can come from: the file, or [`PLACE`] when shipped.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct Candidate {
    pub origin: &'static str,
    pub path: PathBuf,
}

/// Every candidate for one flow name and the one that runs. `replaced` is least
/// specific first, and `resolved_in` is the working directory the sources were
/// read from: two processes standing in two places can pick two winners.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct Chain {
    pub name: String,
    pub resolved_in: Option<PathBuf>,
    pub replaced: Vec<Candidate>,
    pub winner: Candidate,
}

impl Chain {
    /// True when the flow that runs hides one shipped with the product.
    pub fn replaces_builtin(&self) -> bool {
        self.replaced.iter().any(|candidate| candidate.origin == BUILTIN_ORIGIN)
    }
}

/// One flow name as every reader sees it: its chain, and the loaded entry of
/// the file that wins. Both come from one pass over the sources.
pub struct Resolved {
    pub chain: Chain,
    pub entry: Result<FlowFile, String>,
}

type Seen = (Vec<Candidate>, Option<Result<FlowFile, String>>);

/// The one reading of precedence, by name: [`chains`] and [`load_all`] are
/// views of it, so a mark and the flow it marks describe the same file.
pub fn resolve(sources: &[FlowSource], resolved_in: Option<&Path>) -> Vec<Resolved> {
    let mut by_name: BTreeMap<String, Seen> = BTreeMap::new();
    for source in sources {
        let entries: Vec<(String, PathBuf, Result<FlowFile, String>)> = if source.is_builtin() {
            builtin_registry()
                .into_iter()
                .map(|(name, entry)| (name, PathBuf::from(PLACE), entry))
                .collect()
        } else {
            flow_files_in(&source.dir)
                .into_iter()
                .map(|(name, path)| {
                    let entry = load_file(&path);
                    (name, path, entry)
                })
                .collect()
        };
        for (name, path, entry) in entries {
            let slot = by_name.entry(name).or_default();
            slot.0.push(Candidate {
                origin: source.origin,
                path,
            });
            slot.1 = Some(entry);
        }
    }
    by_name
        .into_iter()
        .filter_map(|(name, (mut candidates, entry))| {
            let winner = candidates.pop()?;
            Some(Resolved {
                chain: Chain {
                    name,
                    resolved_in: resolved_in.map(Path::to_path_buf),
                    replaced: candidates,
                    winner,
                },
                entry: entry?,
            })
        })
        .collect()
}

/// The precedence chain of every flow name the sources hold, by name.
pub fn chains(sources: &[FlowSource], resolved_in: Option<&Path>) -> Vec<Chain> {
    resolve(sources, resolved_in)
        .into_iter()
        .map(|resolved| resolved.chain)
        .collect()
}

/// The chain of one name, or `None` when no source holds it.
pub fn chain_of(sources: &[FlowSource], resolved_in: Option<&Path>, name: &str) -> Option<Chain> {
    chains(sources, resolved_in)
        .into_iter()
        .find(|chain| chain.name == name)
}

/// The folder a restored file is moved into, beside the flows folder it left.
/// Its name is not `flows`, so no walk up ever reads it as a project's flows.
pub const ARCHIVE_FOLDER: &str = "flows-archived";

/// Why a flow was not restored.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RestoreRefusal {
    AlreadyBuiltIn,
    NothingBuiltIn { path: PathBuf },
    OutsideDeclared { path: PathBuf },
    ArchiveIsALink { path: PathBuf, archive: PathBuf },
    ArchivedButOriginalStays { path: PathBuf, archive: PathBuf, error: String },
    CouldNotMove { path: PathBuf, archive: PathBuf, error: String },
}

/// How many other names a restore tries when the archive's name is taken.
const ARCHIVE_NAMES_TO_TRY: u32 = 100;

/// The name a restore tries first: `<name>.<now>` and the file's own suffix, in
/// [`ARCHIVE_FOLDER`] beside the flows folder the file leaves.
pub fn archive_path_for(chain: &Chain, now: i64) -> PathBuf {
    archive_named(chain, now, 0)
}

fn archive_named(chain: &Chain, now: i64, attempt: u32) -> PathBuf {
    let path = &chain.winner.path;
    let folder = path.parent().unwrap_or(Path::new(""));
    let suffix = path
        .file_name()
        .map(|file| file.to_string_lossy().into_owned())
        .and_then(|file| file.strip_prefix(chain.name.as_str()).map(str::to_owned))
        .unwrap_or_else(|| ".flow.json".to_owned());
    let stamp = match attempt {
        0 => now.to_string(),
        n => format!("{now}.{n}"),
    };
    folder
        .parent()
        .unwrap_or(folder)
        .join(ARCHIVE_FOLDER)
        .join(format!("{}.{stamp}{suffix}", chain.name))
}

/// The disk gestures a restore makes, so a test can make one of them fail.
trait Disk {
    fn link(&self, from: &Path, to: &Path) -> std::io::Result<()> {
        fs::hard_link(from, to)
    }
    fn remove(&self, path: &Path) -> std::io::Result<()> {
        fs::remove_file(path)
    }
    fn write_synced(&self, file: &mut fs::File, bytes: &[u8]) -> std::io::Result<()> {
        use std::io::Write as _;
        file.write_all(bytes)?;
        file.sync_all()
    }
}

struct ThisDisk;

impl Disk for ThisDisk {}

/// What a restore did: the archive, and the sentence about a scratch file that
/// could not be removed, when one is left.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Restored {
    pub archive: PathBuf,
    pub scratch_left: Option<String>,
}

struct Moved {
    stays: Option<std::io::Error>,
    scratch_left: Option<String>,
}

static NEXT_STAGING: AtomicU64 = AtomicU64::new(0);

/// Moves a file to a name nothing holds, and never replaces what is there: a
/// hard link fails on a taken name. The archive exists before the original is
/// removed, and an original that will not go is said apart from a failed move.
fn move_without_replacing(from: &Path, to: &Path, disk: &dyn Disk) -> std::io::Result<Moved> {
    let scratch_left = match disk.link(from, to) {
        Ok(()) => None,
        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => return Err(error),
        Err(_) => copy_then_link(from, to, disk)?,
    };
    Ok(Moved {
        stays: disk.remove(from).err(),
        scratch_left,
    })
}

/// Where no hard link reaches the original: the copy is written under a
/// scratch name, checked, then linked to the archive's name, so that name
/// never holds a partial file. A scratch file that will not go is said.
fn copy_then_link(from: &Path, to: &Path, disk: &dyn Disk) -> std::io::Result<Option<String>> {
    let bytes = fs::read(from)?;
    let name = to.file_name().map(|file| file.to_string_lossy().into_owned()).unwrap_or_default();
    let serial = NEXT_STAGING.fetch_add(1, Ordering::Relaxed);
    let staging = to.with_file_name(format!(".{name}.{}-{serial}.staging", std::process::id()));
    let mut file = fs::OpenOptions::new().write(true).create_new(true).open(&staging)?;
    let linked = write_check_and_link(&mut file, &bytes, &staging, to, disk);
    drop(file);
    let left = disk.remove(&staging).err().map(|error| {
        catalogue::say(
            "flow.restore.scratch_left",
            &[("scratch", &staging.display().to_string()), ("error", &error.to_string())],
        )
    });
    match (linked, left) {
        (Ok(()), left) => Ok(left),
        (Err(error), None) => Err(error),
        (Err(error), Some(said)) => Err(std::io::Error::new(error.kind(), format!("{error}; {said}"))),
    }
}

fn write_check_and_link(
    file: &mut fs::File,
    bytes: &[u8],
    staging: &Path,
    to: &Path,
    disk: &dyn Disk,
) -> std::io::Result<()> {
    disk.write_synced(file, bytes)?;
    if fs::read(staging)? != bytes {
        return Err(std::io::Error::other("the copy does not match the original"));
    }
    disk.link(staging, to)
}

/// The mark a restore leaves when it archived a file it could not remove: the
/// source and the archive, one a line. Only that archive is ever reused.
fn pending_marker(chain: &Chain, first: &Path) -> PathBuf {
    first.with_file_name(format!(".{}.pending", chain.name))
}

fn write_pending(marker: &Path, source: &Path, archive: &Path) -> std::io::Result<()> {
    use std::io::Write as _;
    let mut file = fs::OpenOptions::new().write(true).create_new(true).open(marker)?;
    writeln!(file, "{}\n{}", source.display(), archive.display())?;
    file.sync_all()
}

/// The archive a failed attempt made of this very file, as its marker names it:
/// the same source, a plain file beside the marker, the same bytes. A marker
/// that no longer describes one is removed, and nothing is reused.
fn archive_left_pending(chain: &Chain, first: &Path) -> Option<PathBuf> {
    let marker = pending_marker(chain, first);
    let text = fs::read_to_string(&marker).ok()?;
    let mut lines = text.lines();
    let (source, archive) = (lines.next().map(PathBuf::from), lines.next().map(PathBuf::from));
    let ours = match (source, archive) {
        (Some(source), Some(archive)) => (source == chain.winner.path
            && archive.parent() == first.parent()
            && fs::symlink_metadata(&archive).is_ok_and(|meta| meta.file_type().is_file())
            && fs::read(&archive).ok().is_some_and(|held| fs::read(&source).ok() == Some(held)))
        .then_some(archive),
        _ => None,
    };
    if ours.is_none() {
        let _ = fs::remove_file(&marker);
    }
    ours
}

/// Puts the shipped flow back by moving the file that runs out of discovery:
/// into [`ARCHIVE_FOLDER`], named `<name>.<now>` plus its own suffix. Never a
/// delete, and never a file other than the winner: under `SAILOR_FLOWS` only a
/// file in that folder can be the winner, and anything else is refused.
pub fn restore(chain: &Chain, sources: &[FlowSource], now: i64) -> Result<Restored, RestoreRefusal> {
    restore_on(chain, sources, now, &ThisDisk)
}

fn restore_on(
    chain: &Chain,
    sources: &[FlowSource],
    now: i64,
    disk: &dyn Disk,
) -> Result<Restored, RestoreRefusal> {
    if chain.winner.origin == BUILTIN_ORIGIN {
        return Err(RestoreRefusal::AlreadyBuiltIn);
    }
    let path = chain.winner.path.clone();
    if !chain.replaces_builtin() {
        return Err(RestoreRefusal::NothingBuiltIn { path });
    }
    let folder = path.parent().unwrap_or(Path::new(""));
    if let Some(declared) = sources.iter().find(|source| source.origin == DECLARED_ORIGIN) {
        if folder != declared.dir {
            return Err(RestoreRefusal::OutsideDeclared { path });
        }
    }
    let first = archive_path_for(chain, now);
    if let Some(parent) = first.parent() {
        fs::create_dir_all(parent).map_err(|error| RestoreRefusal::CouldNotMove {
            path: path.clone(),
            archive: first.clone(),
            error: error.to_string(),
        })?;
    }
    let marker = pending_marker(chain, &first);
    if let Some(archive) = archive_left_pending(chain, &first) {
        return match disk.remove(&path) {
            Ok(()) => {
                let _ = fs::remove_file(&marker);
                Ok(Restored { archive, scratch_left: None })
            }
            Err(error) => Err(RestoreRefusal::ArchivedButOriginalStays {
                path,
                archive,
                error: error.to_string(),
            }),
        };
    }
    for attempt in 0..ARCHIVE_NAMES_TO_TRY {
        let archive = archive_named(chain, now, attempt);
        match move_without_replacing(&path, &archive, disk) {
            Ok(Moved { stays: None, scratch_left }) => return Ok(Restored { archive, scratch_left }),
            Ok(Moved { stays: Some(error), scratch_left }) => {
                let mut said: Vec<String> = vec![error.to_string()];
                said.extend(scratch_left);
                if let Err(error) = write_pending(&marker, &path, &archive) {
                    said.push(catalogue::say(
                        "flow.restore.pending_not_written",
                        &[("marker", &marker.display().to_string()), ("error", &error.to_string())],
                    ));
                }
                return Err(RestoreRefusal::ArchivedButOriginalStays {
                    path,
                    archive,
                    error: said.join("; "),
                });
            }
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
                let is_link = fs::symlink_metadata(&archive)
                    .is_ok_and(|meta| meta.file_type().is_symlink());
                if is_link {
                    return Err(RestoreRefusal::ArchiveIsALink { path, archive });
                }
            }
            Err(error) => {
                return Err(RestoreRefusal::CouldNotMove {
                    path,
                    archive,
                    error: error.to_string(),
                })
            }
        }
    }
    Err(RestoreRefusal::CouldNotMove {
        path,
        archive: first,
        error: format!("every one of {ARCHIVE_NAMES_TO_TRY} archive names is taken"),
    })
}

/// The flows of every source, each with the origin it came from.
///
/// On a name clash the last source wins — the most specific — the same rule as
/// tool descriptors, for the same reason: whoever works on a project expects
/// the project's flow to be the one that runs. The replacement is not silent:
/// the origin stays visible on every line.
pub fn load_all(sources: &[FlowSource]) -> Vec<(String, &'static str, Result<FlowFile, String>)> {
    resolve(sources, None)
        .into_iter()
        .map(|resolved| (resolved.chain.name, resolved.chain.winner.origin, resolved.entry))
        .collect()
}

// ── writing a flow, and deleting one ─────────────────────────────────────
//
// What stayed in the desktop shell, and not by oversight: the check that the
// actions a flow names exist. `actions`, `trigger` and `registry` all depend on
// this crate, so pulling that in would be a cycle — and would be wrong anyway:
// which actions exist depends on who assembles the program, not on the format.

/// Writes a flow into the flows directory: whoever knows where flows live is
/// who writes them. In the desktop shell, outside the Rust workspace, the
/// command line could not call this and `sailor flow cap` would have had to
/// rewrite it — fault 10, two authors of one file with two ideas of what a safe
/// name is and of how to replace a file without showing it half-written. It
/// takes a built `FlowFile`, not JSON, so a bad graph fails in `Graph::validate`.
pub fn save_in(flows_dir: &Path, flow: &FlowFile) -> Result<(), String> {
    let document = serde_json::to_value(flow)
        .map_err(|error| format!("cannot compose the flow as JSON: {error}"))?;
    save_document_in(flows_dir, &document)
}

/// The flow a document declares, or the one refusal every caller shows.
pub fn flow_of_document(document: &serde_json::Value) -> Result<FlowFile, String> {
    serde_json::from_value(document.clone())
        .map_err(|error| format!("the flow fails the engine's validation: {error}"))
}

/// Writes a flow **keeping the key order its author gave it**, where
/// [`save_in`] rebuilds it. One door, so a refused graph enters by neither.
pub fn save_document_in(flows_dir: &Path, document: &serde_json::Value) -> Result<(), String> {
    let flow = flow_of_document(document)?;
    let id = safe_flow_id(&flow.id)?;
    fs::create_dir_all(flows_dir)
        .map_err(|error| format!("cannot prepare the flows directory: {error}"))?;
    let file_name = format!("{id}.flow.json");
    reject_a_name_that_collides_only_by_case(flows_dir, &file_name)?;
    let target = flows_dir.join(&file_name);
    let mut text = serde_json::to_string_pretty(document)
        .map_err(|error| format!("cannot compose the flow as JSON: {error}"))?;
    // Without the newline `git diff` says so on every rewritten flow.
    text.push('\n');
    write_atomically(&target, text.as_bytes())
}

/// Deletes a flow from the flows directory.
pub fn delete_in(flows_dir: &Path, name: &str) -> Result<(), String> {
    let id = safe_flow_id(name)?;
    let target = flows_dir.join(format!("{id}.flow.json"));
    match fs::remove_file(&target) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            Err(format!("flow \"{name}\" does not exist"))
        }
        Err(error) => Err(format!("cannot delete {}: {error}", target.display())),
    }
}

/// An id that would climb out of the flows directory (empty, or holding `/`,
/// `\` or `..`) is a traversal path: refuse it rather than quietly cleaning it
/// up — the person must see that the name was refused.
pub fn safe_flow_id(id: &str) -> Result<&str, String> {
    if id.is_empty() {
        return Err("a flow name cannot be empty".to_owned());
    }
    if id.contains('/') || id.contains('\\') || id.contains("..") {
        return Err(format!(
            "\"{id}\" is not a safe flow name: no path separators"
        ));
    }
    Ok(id)
}

/// Two names differing only in case are the same file, and the disk does not
/// say so. On APFS as macOS installs it — and on Windows — saving "myflow" over
/// an existing "MyFlow" raises no error: it replaces the content and keeps the
/// old name, so the saver believes they made a new flow and deleted another.
/// It refuses rather than choosing for them: "did you mean to overwrite that?"
/// is a question for someone with a person in front of them.
fn reject_a_name_that_collides_only_by_case(
    flows_dir: &Path,
    file_name: &str,
) -> Result<(), String> {
    // Not in `safe_flow_id`, which judges a name on its own: answering this one
    // means looking at what the directory already holds.
    let Ok(entries) = fs::read_dir(flows_dir) else {
        return Ok(());
    };
    for entry in entries.flatten() {
        let existing = entry.file_name();
        let existing = existing.to_string_lossy();
        if existing.as_ref() != file_name && existing.eq_ignore_ascii_case(file_name) {
            return Err(format!(
                "\"{existing}\" already exists, and on this disk it is the same file as \
                 \"{file_name}\": writing it would replace that one without you noticing. \
                 Pick another name, or edit the one that is there."
            ));
        }
    }
    Ok(())
}

static TEMP_SEQUENCE: AtomicU64 = AtomicU64::new(0);

/// Atomic write: a temporary file beside the target, then `rename`. Whoever
/// re-reads the directory (the window, or a run) must not be able to see a
/// half-written file — `rename` on the same filesystem is indivisible, a direct
/// `write` on the target is not.
fn write_atomically(target: &Path, contents: &[u8]) -> Result<(), String> {
    let temp_path = temp_path_for(target);
    fs::write(&temp_path, contents).map_err(|error| {
        format!(
            "cannot write the temporary file {}: {error}",
            temp_path.display()
        )
    })?;
    fs::rename(&temp_path, target).map_err(|error| {
        let _ = fs::remove_file(&temp_path);
        format!("cannot replace {}: {error}", target.display())
    })
}

fn temp_path_for(target: &Path) -> PathBuf {
    let file_name = target
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("flow");
    let unique = TEMP_SEQUENCE.fetch_add(1, Ordering::Relaxed);
    target.with_file_name(format!(".{file_name}.tmp-{}-{unique}", std::process::id()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(label: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("sailor-sistema-{label}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).expect("scratch directory");
        dir
    }

    fn put_flow(dir: &Path, name: &str) {
        fs::create_dir_all(dir).expect("directory");
        fs::write(dir.join(format!("{name}.flow.json")), "{}").expect("flow");
    }

    /// This must fall on us, never on whoever installs. A shipped flow lives
    /// inside the binary: if it is malformed no user can repair it — they can
    /// only write their own under the same name, without knowing why.
    #[test]
    fn every_shipped_flow_loads() {
        let registry = builtin_registry();
        assert_eq!(registry.len(), FLOWS.len(), "no repeated name");
        for (name, entry) in &registry {
            assert!(entry.is_ok(), "shipped flow \"{name}\": {entry:?}");
        }
    }

    /// The file name is the flow name. Overriding a system flow means writing a
    /// file with that name, while running it calls it by `id`. Were the two to
    /// diverge, "I overrode it" and "I ran it" would name different flows.
    #[test]
    fn the_shipped_name_is_the_declared_id() {
        for (name, entry) in builtin_registry() {
            let flow = entry.expect("valid flow");
            assert_eq!(flow.id, name, "file name and declared id");
        }
    }

    /// The system source is the least specific: a same-named flow written at
    /// home must win, or "customisable" is a word with no mechanism behind it.
    #[test]
    fn the_system_source_is_the_least_specific() {
        let places = sources(Some(Path::new("/home/flows")), None, None);
        assert_eq!(places[0], FlowSource::builtin());
        assert_eq!(places[0].origin, "built in");
        assert_eq!(places.last().expect("at least one").origin, "yours");
    }

    /// `SAILOR_FLOWS` clears home and project out of the way, not the binary's
    /// own equipment: that is in no folder, so there is no folder to replace.
    #[test]
    fn a_declared_folder_replaces_the_disk_but_not_the_binary() {
        let places = sources(
            Some(Path::new("/home/flows")),
            None,
            Some(Path::new("/here/the/flows")),
        );
        let origins: Vec<&str> = places.iter().map(|p| p.origin).collect();
        assert_eq!(origins, vec!["built in", "declared"]);
    }

    /// A system flow is overridden by writing one with the same name, and the
    /// origin says so: without that line, whoever edited their own would not
    /// know whether they are looking at theirs or the shipped one.
    #[test]
    fn a_home_flow_overrides_a_system_flow_of_the_same_name() {
        let base = scratch("override");
        let home_flows = base.join("home").join("flows");
        let shipped = FLOWS[0].0;
        put_flow(&home_flows, shipped);

        let all = load_all(&sources(Some(&home_flows), None, None));

        let (_, origin, _) = all
            .iter()
            .find(|(name, _, _)| name == shipped)
            .expect("the flow is there");
        assert_eq!(*origin, "yours", "the user's own wins");
        assert_eq!(
            all.iter().filter(|(name, _, _)| name == shipped).count(),
            1,
            "one line, not two copies"
        );
        // It replaces, it does not add. Without this line the test stays green
        // even if the system source vanishes entirely, because one flow winning
        // over nothing reads the same as one flow overriding another.
        assert_eq!(
            all.len(),
            FLOWS.len(),
            "the other shipped flows remain: {all:?}"
        );

        let _ = fs::remove_dir_all(&base);
    }

    /// On a freshly installed machine — no folder, nothing copied — the system
    /// flows are there all the same.
    #[test]
    fn a_fresh_machine_still_has_the_system_flows() {
        let nowhere = std::env::temp_dir().join("sailor-home-that-never-exists");
        let all = load_all(&sources(Some(&nowhere.join("flows")), None, None));
        assert_eq!(all.len(), FLOWS.len());
        assert!(all.iter().all(|(_, origin, _)| *origin == "built in"));
    }

    /// The measured defect: the window started in `desktop/src-tauri` and did
    /// not see the project's flows, two directories further up.
    #[test]
    fn the_project_flows_are_found_from_a_subfolder() {
        let root = scratch("walk-up");
        put_flow(&root.join("flows"), "one");
        let deep = root.join("desktop").join("src-tauri");
        fs::create_dir_all(&deep).expect("subdirectory");

        let found = project_flows_from(&deep, Some(Path::new("/home/elsewhere/flows")));

        assert_eq!(found, Some(root.join("flows")));
        let _ = fs::remove_dir_all(&root);
    }

    /// A directory named `flows` but empty must not stop the climb: the reader
    /// would see an empty list where their own flows should be.
    #[test]
    fn an_empty_flows_folder_does_not_stop_the_climb() {
        let root = scratch("empty");
        put_flow(&root.join("flows"), "real");
        let middle = root.join("inside");
        fs::create_dir_all(middle.join("flows")).expect("empty directory named flows");

        let found = project_flows_from(&middle, Some(Path::new("/home/elsewhere/flows")));

        assert_eq!(found, Some(root.join("flows")));
        let _ = fs::remove_dir_all(&root);
    }

    /// Home is not a project: counting it twice would show every flow in
    /// duplicate, and the reader would not know which of the two runs.
    #[test]
    fn the_home_is_never_also_the_project() {
        let home = scratch("home");
        let home_flows = home.join("flows");
        put_flow(&home_flows, "mine");

        assert_eq!(project_flows_from(&home, Some(&home_flows)), None);
        let _ = fs::remove_dir_all(&home);
    }

    /// Counting the system source's entries must give the number of shipped
    /// flows, not zero: whoever shows "where I looked and what I found" comes
    /// through here, and a zero there is indistinguishable from a fault.
    #[test]
    fn counting_the_builtin_source_does_not_count_a_folder() {
        assert_eq!(registry_of(&FlowSource::builtin()).len(), FLOWS.len());
    }

    /// A `flows/` higher up does not beat the marker. This tests precedence,
    /// not the walk-up: were the fallback consulted first, the declared project
    /// would lose its own flows to those of the project containing it — and
    /// whoever wrote `sailor.json` would have declared nothing.
    #[test]
    fn a_flows_folder_above_the_marker_does_not_win() {
        let outer = scratch("marker-vs-flows");
        put_flow(&outer.join("flows"), "belongs-to-the-one-above");
        let project = outer.join("project");
        fs::create_dir_all(&project).expect("project directory");
        fs::write(project.join(crate::workspace::MARKER), "{}").expect("marker");
        let deep = project.join("crates").join("inside");
        fs::create_dir_all(&deep).expect("subdirectory");

        let places = sources(Some(Path::new("/home/flows")), Some(&deep), None);

        let last = places.last().expect("at least one");
        assert_eq!(last.dir, project.join("flows"), "the declared root wins");
        assert_eq!(last.origin, crate::workspace::ORIGIN_DECLARED);

        let _ = fs::remove_dir_all(&outer);
    }

    /// With no marker the fallback stays, and declares itself: the origin says
    /// the root was guessed, so a reader knows why the flows are those ones.
    #[test]
    fn without_a_marker_the_climb_still_works_and_says_so() {
        let root = scratch("fallback");
        put_flow(&root.join("flows"), "one");
        let deep = root.join("desktop").join("src-tauri");
        fs::create_dir_all(&deep).expect("subdirectory");

        let places = sources(Some(Path::new("/home/flows")), Some(&deep), None);

        let last = places.last().expect("at least one");
        assert_eq!(last.dir, root.join("flows"));
        assert_eq!(last.origin, crate::workspace::ORIGIN_GUESSED);

        let _ = fs::remove_dir_all(&root);
    }

    /// The chain names every candidate, least specific first, and its winner is
    /// the one `load_all` keeps for every name: one order, read twice.
    #[test]
    fn a_chain_names_every_candidate_and_its_winner_is_the_one_that_loads() {
        let base = scratch("chain");
        let home_flows = base.join("home").join("flows");
        let project = base.join("project");
        fs::create_dir_all(&project).expect("project directory");
        fs::write(project.join(crate::workspace::MARKER), "{}").expect("marker");
        let shipped = FLOWS[0].0;
        put_flow(&home_flows, shipped);
        put_flow(&project.join("flows"), shipped);
        put_flow(&home_flows, "a-home-flow");

        let places = sources(Some(&home_flows), Some(&project), None);
        let all = chains(&places, Some(&project));

        let chain = all.iter().find(|chain| chain.name == shipped).expect("the chain");
        let origins: Vec<&str> = chain.replaced.iter().map(|candidate| candidate.origin).collect();
        assert_eq!(origins, vec![BUILTIN_ORIGIN, YOUR_ORIGIN]);
        assert_eq!(chain.replaced[0].path, PathBuf::from(PLACE));
        assert_eq!(chain.winner.origin, crate::workspace::ORIGIN_DECLARED);
        assert_eq!(chain.winner.path, project.join("flows").join(format!("{shipped}.flow.json")));
        assert_eq!(chain.resolved_in.as_deref(), Some(project.as_path()));
        assert!(chain.replaces_builtin());
        let alone = all.iter().find(|chain| chain.name == "a-home-flow").expect("the chain");
        assert!(alone.replaced.is_empty() && !alone.replaces_builtin());

        let loaded = load_all(&places);
        assert_eq!(loaded.len(), all.len(), "one chain per name");
        for (name, origin, _) in &loaded {
            let chain = all.iter().find(|chain| &chain.name == name).expect("a chain per name");
            assert_eq!(chain.winner.origin, *origin, "{name}");
        }
        let _ = fs::remove_dir_all(&base);
    }

    /// Restoring moves the winner beside its folder, keeps every byte, and the
    /// next reading finds the shipped flow running again.
    #[test]
    fn restoring_moves_the_file_that_replaces_a_shipped_flow_and_never_deletes_it() {
        let base = scratch("restore");
        let home_flows = base.join("home").join("flows");
        let shipped = FLOWS[0].0;
        put_flow(&home_flows, shipped);
        let file = home_flows.join(format!("{shipped}.flow.json"));
        let written = fs::read(&file).expect("the file");
        let places = sources(Some(&home_flows), None, None);
        let chain = chain_of(&places, None, shipped).expect("the chain");

        let archive = restore(&chain, &places, 42).expect("restored").archive;

        assert_eq!(archive, base.join("home").join(ARCHIVE_FOLDER).join(format!("{shipped}.42.flow.json")));
        assert!(!file.exists());
        assert_eq!(fs::read(&archive).expect("the archive"), written);
        let after = chain_of(&places, None, shipped).expect("the chain");
        assert_eq!(after.winner.origin, BUILTIN_ORIGIN);
        assert_eq!(restore(&after, &places, 43), Err(RestoreRefusal::AlreadyBuiltIn));
        let _ = fs::remove_dir_all(&base);
    }

    /// A file that replaces nothing shipped is not restored to anything, and a
    /// file outside a declared folder is not the person's, so neither moves.
    #[test]
    fn restoring_refuses_what_hides_nothing_shipped_and_what_lies_outside_the_declared_folder() {
        let base = scratch("refuse");
        let home_flows = base.join("home").join("flows");
        put_flow(&home_flows, "a-home-flow");
        let places = sources(Some(&home_flows), None, None);
        let alone = chain_of(&places, None, "a-home-flow").expect("the chain");
        let path = home_flows.join("a-home-flow.flow.json");
        assert_eq!(
            restore(&alone, &places, 1),
            Err(RestoreRefusal::NothingBuiltIn { path: path.clone() })
        );
        assert!(path.exists());

        let shipped = FLOWS[0].0;
        put_flow(&home_flows, shipped);
        let declared = base.join("declared");
        fs::create_dir_all(&declared).expect("declared folder");
        let mixed = vec![
            FlowSource::builtin(),
            FlowSource { origin: DECLARED_ORIGIN, dir: declared },
            FlowSource { origin: YOUR_ORIGIN, dir: home_flows.clone() },
        ];
        let chain = chain_of(&mixed, None, shipped).expect("the chain");
        let outside = home_flows.join(format!("{shipped}.flow.json"));
        assert_eq!(
            restore(&chain, &mixed, 1),
            Err(RestoreRefusal::OutsideDeclared { path: outside.clone() })
        );
        assert!(outside.exists());
        let _ = fs::remove_dir_all(&base);
    }

    /// An archive already at the chosen name is never overwritten: the file goes
    /// to another name, the old archive keeps every byte, and the winner leaves.
    #[test]
    fn restoring_never_overwrites_an_archive_already_there() {
        let base = scratch("restore-taken");
        let home_flows = base.join("home").join("flows");
        let shipped = FLOWS[0].0;
        put_flow(&home_flows, shipped);
        let file = home_flows.join(format!("{shipped}.flow.json"));
        let taken = base.join("home").join(ARCHIVE_FOLDER).join(format!("{shipped}.42.flow.json"));
        fs::create_dir_all(taken.parent().expect("a folder")).expect("archive folder");
        fs::write(&taken, "an archive from before").expect("the older archive");
        let places = sources(Some(&home_flows), None, None);
        let chain = chain_of(&places, None, shipped).expect("the chain");

        let archive = restore(&chain, &places, 42).expect("restored under another name").archive;

        assert_ne!(archive, taken, "the older archive's name was reused");
        assert_eq!(fs::read_to_string(&taken).expect("still there"), "an archive from before");
        assert!(archive.exists() && !file.exists(), "moved to {}", archive.display());
        let _ = fs::remove_dir_all(&base);
    }

    /// A link at the archive's name, even one pointing nowhere, is refused: the
    /// move follows no link and replaces none, and the winner stays in place.
    #[cfg(unix)]
    #[test]
    fn restoring_refuses_a_link_where_the_archive_would_go() {
        let base = scratch("restore-link");
        let home_flows = base.join("home").join("flows");
        let shipped = FLOWS[0].0;
        put_flow(&home_flows, shipped);
        let file = home_flows.join(format!("{shipped}.flow.json"));
        let link = base.join("home").join(ARCHIVE_FOLDER).join(format!("{shipped}.42.flow.json"));
        fs::create_dir_all(link.parent().expect("a folder")).expect("archive folder");
        std::os::unix::fs::symlink(base.join("nowhere"), &link).expect("a dangling link");
        let places = sources(Some(&home_flows), None, None);
        let chain = chain_of(&places, None, shipped).expect("the chain");

        let refused = restore(&chain, &places, 42);

        assert!(
            matches!(refused, Err(RestoreRefusal::ArchiveIsALink { .. })),
            "a link at the archive's name was replaced: {refused:?}"
        );
        assert!(fs::symlink_metadata(&link).expect("the link").file_type().is_symlink());
        assert!(file.exists(), "the winner stays where it was");
        let _ = fs::remove_dir_all(&base);
    }

    /// One reading gives both the chain and the loaded entry of its winner, so
    /// the mark and the flow shown can never describe two different files.
    #[test]
    fn one_reading_gives_the_chain_and_the_entry_of_the_file_that_wins() {
        let base = scratch("resolve");
        let home_flows = base.join("home").join("flows");
        let shipped = FLOWS[0].0;
        fs::create_dir_all(&home_flows).expect("home flows");
        let winner = home_flows.join(format!("{shipped}.flow.json"));
        let mut document: serde_json::Value = serde_json::from_str(FLOWS[0].1).expect("a shipped flow");
        document["description"] = serde_json::json!("the home copy");
        fs::write(&winner, document.to_string()).expect("the home copy");
        let places = sources(Some(&home_flows), None, None);

        let read = resolve(&places, Some(&base));

        let one = read.iter().find(|one| one.chain.name == shipped).expect("the name");
        assert_eq!(one.chain.winner.path, winner);
        assert_eq!(one.entry.as_ref().expect("it loads").description, "the home copy");
        let loaded = load_all(&places);
        assert_eq!(loaded.len(), read.len());
        for ((name, origin, entry), one) in loaded.iter().zip(&read) {
            assert_eq!((name, *origin), (&one.chain.name, one.chain.winner.origin));
            assert_eq!(entry.as_ref().ok().map(|flow| &flow.description), one.entry.as_ref().ok().map(|flow| &flow.description));
        }
        let _ = fs::remove_dir_all(&base);
    }

    /// A disk whose gestures fail where a test says: a link from one file, a
    /// write cut in half, the removal of one file.
    struct FaultyDisk {
        no_link_from: Option<PathBuf>,
        broken_write: bool,
        stuck: Option<PathBuf>,
        stuck_staging: bool,
    }

    impl Disk for FaultyDisk {
        fn link(&self, from: &Path, to: &Path) -> std::io::Result<()> {
            if self.no_link_from.as_deref() == Some(from) {
                return Err(std::io::Error::other("another filesystem"));
            }
            fs::hard_link(from, to)
        }
        fn remove(&self, path: &Path) -> std::io::Result<()> {
            if self.stuck.as_deref() == Some(path) {
                return Err(std::io::Error::other("the folder is read-only"));
            }
            if self.stuck_staging && path.to_string_lossy().ends_with(".staging") {
                return Err(std::io::Error::other("the scratch file is held open"));
            }
            fs::remove_file(path)
        }
        fn write_synced(&self, file: &mut fs::File, bytes: &[u8]) -> std::io::Result<()> {
            use std::io::Write as _;
            if self.broken_write {
                file.write_all(&bytes[..bytes.len() / 2])?;
                return Err(std::io::Error::other("the disk filled up"));
            }
            file.write_all(bytes)?;
            file.sync_all()
        }
    }

    fn archived_in(folder: &Path) -> Vec<PathBuf> {
        fs::read_dir(folder)
            .map(|entries| entries.flatten().map(|entry| entry.path()).collect())
            .unwrap_or_default()
    }

    /// A copy that breaks halfway leaves nothing under the archive folder, not
    /// even a scratch file, and the source keeps every byte: a second attempt
    /// then archives under the first name, not beside a corrupt one.
    #[test]
    fn a_copy_that_breaks_leaves_no_partial_archive_and_the_source_whole() {
        let base = scratch("restore-broken-copy");
        let home_flows = base.join("home").join("flows");
        let shipped = FLOWS[0].0;
        put_flow(&home_flows, shipped);
        let file = home_flows.join(format!("{shipped}.flow.json"));
        let written = fs::read(&file).expect("the file");
        let places = sources(Some(&home_flows), None, None);
        let chain = chain_of(&places, None, shipped).expect("the chain");
        let broken = FaultyDisk { no_link_from: Some(file.clone()), broken_write: true, stuck: None, stuck_staging: false };

        let refused = restore_on(&chain, &places, 42, &broken);

        assert!(refused.is_err(), "{refused:?}");
        let folder = base.join("home").join(ARCHIVE_FOLDER);
        assert_eq!(archived_in(&folder), Vec::<PathBuf>::new(), "a partial archive or a scratch file is left");
        assert_eq!(fs::read(&file).expect("the source"), written, "the source was touched");
        let copying = FaultyDisk { no_link_from: Some(file.clone()), broken_write: false, stuck: None, stuck_staging: false };
        let archive = restore_on(&chain, &places, 42, &copying).expect("the second attempt archives").archive;
        assert_eq!(archive, archive_path_for(&chain, 42));
        assert_eq!(fs::read(&archive).expect("the archive"), written);
        assert_eq!(archived_in(&folder), vec![archive]);
        let _ = fs::remove_dir_all(&base);
    }

    /// An original that cannot be removed is archived once: the next attempt
    /// finds its own archive and retries only the removal, and both say both
    /// paths while the file that runs does not change.
    #[test]
    fn an_original_that_cannot_be_removed_is_archived_once_and_both_paths_are_said() {
        let base = scratch("restore-stuck");
        let home_flows = base.join("home").join("flows");
        let shipped = FLOWS[0].0;
        put_flow(&home_flows, shipped);
        let file = home_flows.join(format!("{shipped}.flow.json"));
        let places = sources(Some(&home_flows), None, None);
        let chain = chain_of(&places, None, shipped).expect("the chain");
        let stuck = FaultyDisk { no_link_from: None, broken_write: false, stuck: Some(file.clone()), stuck_staging: false };

        let first = restore_on(&chain, &places, 42, &stuck);
        let second = restore_on(&chain, &places, 43, &stuck);

        let everything = archived_in(&base.join("home").join(ARCHIVE_FOLDER));
        let archives: Vec<PathBuf> = everything
            .iter()
            .filter(|path| !path.file_name().is_some_and(|name| name.to_string_lossy().starts_with('.')))
            .cloned()
            .collect();
        assert_eq!(archives.len(), 1, "one archive after two attempts: {everything:?}");
        assert!(pending_marker(&chain, &archives[0]).exists(), "the attempt left no mark: {everything:?}");
        assert!(file.exists(), "the original stays");
        for said in [&first, &second] {
            let text = format!("{said:?}");
            assert!(matches!(said, Err(RestoreRefusal::ArchivedButOriginalStays { .. })), "{text}");
            assert!(text.contains(&file.display().to_string()), "{text}");
            assert!(text.contains(&archives[0].display().to_string()), "{text}");
        }
        let after = chain_of(&places, None, shipped).expect("the chain");
        assert_eq!(after.winner.path, file, "the flow that runs changed");
        let _ = fs::remove_dir_all(&base);
    }

    /// An older archive that happens to hold the same bytes is history, not
    /// this restore's archive: a new one is made under this restore's name.
    #[test]
    fn an_older_archive_with_the_same_bytes_is_not_taken_for_this_restore() {
        let base = scratch("restore-history");
        let home_flows = base.join("home").join("flows");
        let shipped = FLOWS[0].0;
        put_flow(&home_flows, shipped);
        let file = home_flows.join(format!("{shipped}.flow.json"));
        let folder = base.join("home").join(ARCHIVE_FOLDER);
        let older = folder.join(format!("{shipped}.7.flow.json"));
        fs::create_dir_all(&folder).expect("archive folder");
        fs::copy(&file, &older).expect("an older archive with the same bytes");
        let places = sources(Some(&home_flows), None, None);
        let chain = chain_of(&places, None, shipped).expect("the chain");

        let restored = restore_on(&chain, &places, 42, &ThisDisk);

        let text = format!("{restored:?}");
        assert!(text.contains(&archive_path_for(&chain, 42).display().to_string()), "{text}");
        assert!(!text.contains(&older.display().to_string()), "an unrelated archive is named: {text}");
        assert!(older.exists() && archive_path_for(&chain, 42).exists(), "{:?}", archived_in(&folder));
        assert!(!file.exists());
        let _ = fs::remove_dir_all(&base);
    }

    /// After a removal that failed, the retry reuses the archive that attempt
    /// made, and never an older one with the same bytes beside it.
    #[test]
    fn after_a_stuck_removal_the_retry_reuses_only_its_own_archive() {
        let base = scratch("restore-own");
        let home_flows = base.join("home").join("flows");
        let shipped = FLOWS[0].0;
        put_flow(&home_flows, shipped);
        let file = home_flows.join(format!("{shipped}.flow.json"));
        let folder = base.join("home").join(ARCHIVE_FOLDER);
        let older = folder.join(format!("{shipped}.7.flow.json"));
        fs::create_dir_all(&folder).expect("archive folder");
        fs::copy(&file, &older).expect("an older archive with the same bytes");
        let places = sources(Some(&home_flows), None, None);
        let chain = chain_of(&places, None, shipped).expect("the chain");
        let stuck = FaultyDisk { no_link_from: None, broken_write: false, stuck: Some(file.clone()), stuck_staging: false };
        let own = archive_path_for(&chain, 42);

        let first = restore_on(&chain, &places, 42, &stuck);
        let second = restore_on(&chain, &places, 43, &stuck);
        let third = restore_on(&chain, &places, 44, &ThisDisk);

        for said in [&first, &second, &third] {
            let text = format!("{said:?}");
            assert!(text.contains(&own.display().to_string()), "{text}");
            assert!(!text.contains(&older.display().to_string()), "{text}");
        }
        let mut archives = archived_in(&folder);
        archives.retain(|path| !path.file_name().is_some_and(|name| name.to_string_lossy().starts_with('.')));
        archives.sort();
        assert_eq!(archives, vec![own.clone(), older.clone()]);
        assert!(!file.exists(), "the third attempt removes the original");
        let _ = fs::remove_dir_all(&base);
    }

    /// A scratch file that cannot be removed is said, with its name: the
    /// outcome is not reported as a clean success.
    #[test]
    fn a_scratch_file_that_cannot_be_removed_is_said() {
        let base = scratch("restore-scratch-left");
        let home_flows = base.join("home").join("flows");
        let shipped = FLOWS[0].0;
        put_flow(&home_flows, shipped);
        let file = home_flows.join(format!("{shipped}.flow.json"));
        let places = sources(Some(&home_flows), None, None);
        let chain = chain_of(&places, None, shipped).expect("the chain");
        let held = FaultyDisk { no_link_from: Some(file.clone()), broken_write: false, stuck: None, stuck_staging: true };

        let restored = restore_on(&chain, &places, 42, &held);

        let left: Vec<PathBuf> = archived_in(&base.join("home").join(ARCHIVE_FOLDER))
            .into_iter()
            .filter(|path| path.to_string_lossy().ends_with(".staging"))
            .collect();
        assert_eq!(left.len(), 1, "the held scratch file is still there: {left:?}");
        let text = format!("{restored:?}");
        assert!(
            matches!(&restored, Ok(Restored { scratch_left: Some(_), .. })),
            "a scratch file left is reported as a clean success: {text}"
        );
        assert!(text.contains(&left[0].display().to_string()), "the scratch file is not said: {text}");
        let _ = fs::remove_dir_all(&base);
    }

    // ── writing a flow ──────────────────────────────────────────────────
    //
    // These tests were in the desktop shell, which is outside the workspace, so
    // `cargo test --workspace` never ran them. They came here with the code
    // they test.

    /// A complete flow: two steps, a dependency, a schedule, some inputs. The
    /// identity test needs it — a threadbare flow could lose nothing in a round
    /// trip.
    fn a_full_flow(id: &str) -> FlowFile {
        let text = format!(
            r#"{{
                "id": "{id}",
                "description": "two steps, a recurrence and some inputs",
                "graph": {{
                    "steps": [
                        {{
                            "id": "first", "deps": [], "action": "shell_check",
                            "max_attempts": 1, "when": null,
                            "input_schema": {{"type": "any"}},
                            "output_schema": {{"type": "any"}}
                        }},
                        {{
                            "id": "second", "deps": ["first"], "action": "shell_check",
                            "max_attempts": 3, "when": null,
                            "with": {{"command": "true"}},
                            "input_schema": {{"type": "any"}},
                            "output_schema": {{"type": "any"}}
                        }}
                    ]
                }},
                "inputs": {{ "first": {{ "command": "true", "timeout_secs": 5 }} }},
                "schedule": {{
                    "recurrence": {{ "kind": "daily_at", "hour": 3, "minute": 30 }},
                    "weight": "heavy",
                    "perimeter": ["/some/directory"]
                }}
            }}"#
        );
        serde_json::from_str(&text).expect("the test flow is valid")
    }

    fn read_back(dir: &Path, id: &str) -> FlowFile {
        let text = fs::read_to_string(dir.join(format!("{id}.flow.json")))
            .expect("the written file reads back");
        serde_json::from_str(&text).expect("and deserialises")
    }

    fn entries(dir: &Path) -> Vec<String> {
        fs::read_dir(dir)
            .expect("readable directory")
            .flatten()
            .map(|entry| entry.file_name().to_string_lossy().into_owned())
            .collect()
    }

    /// Setting a cap must lose nothing else. That is the real risk of `sailor
    /// flow cap`: read a flow, change one field, write it back — and the round
    /// trip drops the schedule, or a step's `with`, with nobody noticing until
    /// that flow misses its nightly appointment. The comparison is on the
    /// `FlowFile`, not the text: comparing text would go red on an indentation
    /// change, which is not a loss.
    #[test]
    fn setting_the_cap_leaves_the_rest_of_the_flow_identical() {
        let dir = scratch("cap-and-identity");
        let before = a_full_flow("with-cap");
        save_in(&dir, &before).expect("first write");

        let mut with_cap = read_back(&dir, "with-cap");
        with_cap.spend_cap_micros = Some(250_000);
        save_in(&dir, &with_cap).expect("rewrite with the cap");
        let after = read_back(&dir, "with-cap");

        assert_eq!(
            after.spend_cap_micros,
            Some(250_000),
            "the cap is the one that was set"
        );
        // And all the rest is as before, field by field: if `FlowFile` grows one
        // day, this comparison grows with it without anyone remembering to. The
        // mutant that counts is a field the round trip loses — mark
        // `FlowFile::schedule` `#[serde(skip_serializing)]`, which is what
        // happens to whoever adds a field and forgets the writing side, and the
        // flow comes back with no schedule and this line goes red.
        let mut without_the_cap = after.clone();
        without_the_cap.spend_cap_micros = None;
        assert_eq!(
            without_the_cap, before,
            "the round trip changed something besides the cap"
        );

        let _ = fs::remove_dir_all(&dir);
    }

    /// Clearing the cap returns it to `None`, which is not `Some(0)`: the first
    /// is "nobody set a limit", the second is "must not spend anything".
    #[test]
    fn clearing_the_cap_writes_no_cap_instead_of_a_zero() {
        let dir = scratch("cap-cleared");
        let mut flow = a_full_flow("without-cap");
        flow.spend_cap_micros = Some(500);
        save_in(&dir, &flow).expect("write with the cap");

        flow.spend_cap_micros = None;
        save_in(&dir, &flow).expect("rewrite without it");

        assert_eq!(read_back(&dir, "without-cap").spend_cap_micros, None);
        let _ = fs::remove_dir_all(&dir);
    }

    /// A text file ends with a newline. Without one, `git diff` writes "\ No
    /// newline at end of file" on every flow that passes through here, and the
    /// next hand-added line lands stuck to the last. It costs one character,
    /// and it shows at once on every flow `sailor flow cap` rewrites.
    #[test]
    fn a_written_flow_ends_with_a_newline() {
        let dir = scratch("newline");
        save_in(&dir, &a_full_flow("ended-well")).expect("write");

        let text = fs::read_to_string(dir.join("ended-well.flow.json")).expect("read back");

        assert!(text.ends_with('\n'), "the file does not end with a newline");
        let _ = fs::remove_dir_all(&dir);
    }

    /// A field that was absent must not come back as `null`. Whoever rewrites a
    /// flow — `sailor flow cap`, or the canvas — must add it no line nobody
    /// wrote: `"schedule": null` and `"spend_cap_micros": null` say nothing the
    /// absence does not already say, and fill the diff with noise for someone
    /// re-reading their own flow after the command. Absent and `null` read back
    /// the same, which `clearing_the_cap_writes_no_cap_instead_of_a_zero` holds.
    #[test]
    fn a_field_that_was_absent_does_not_come_back_as_null() {
        let dir = scratch("no-nulls");
        let mut bare = a_full_flow("bare");
        bare.schedule = None;
        bare.spend_cap_micros = None;
        save_in(&dir, &bare).expect("write");

        let text = fs::read_to_string(dir.join("bare.flow.json")).expect("read back");

        assert!(!text.contains("schedule"), "{text}");
        assert!(!text.contains("spend_cap_micros"), "{text}");
        assert_eq!(read_back(&dir, "bare"), bare, "and reads back identical");
        let _ = fs::remove_dir_all(&dir);
    }

    /// The measure that could have come out differently: without the `..` check
    /// this id would write outside the flows directory, into its parent.
    #[test]
    fn a_flow_id_that_climbs_out_of_the_directory_is_refused() {
        let dir = scratch("escape");
        // The escape target sits outside the throwaway directory: it must be
        // cleaned before and after, or a mutant that lets it through dirties
        // `$TMPDIR` for later runs instead of showing itself here.
        let escaped = dir
            .parent()
            .expect("the test directory has a parent")
            .join("escaped.flow.json");
        let _ = fs::remove_file(&escaped);

        let error = save_in(&dir, &a_full_flow("../escaped")).expect_err("id with .. refused");

        assert!(error.contains("path separators"), "{error}");
        assert!(entries(&dir).is_empty(), "the directory stays empty");
        assert!(!escaped.exists(), "and nothing left the directory");
        let _ = fs::remove_file(&escaped);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_flow_id_with_a_path_separator_is_refused() {
        let dir = scratch("separator");
        let error = save_in(&dir, &a_full_flow("under/directory")).expect_err("id with / refused");
        assert!(error.contains("path separators"), "{error}");
        assert!(entries(&dir).is_empty());
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn an_empty_flow_id_is_refused_and_writes_nothing() {
        let dir = scratch("empty-id");
        let error = save_in(&dir, &a_full_flow("")).expect_err("empty id refused");
        assert!(error.contains("empty"), "{error}");
        assert!(entries(&dir).is_empty());
        let _ = fs::remove_dir_all(&dir);
    }

    /// The file system does not say the two are the same file. On APFS as macOS
    /// installs it, saving "myflow" over an existing "MyFlow" replaces the
    /// content with no error and keeps the old name.
    #[test]
    fn a_name_that_differs_only_by_case_is_refused() {
        let dir = scratch("case");
        save_in(&dir, &a_full_flow("MyFlow")).expect("the first one writes");

        let error = save_in(&dir, &a_full_flow("myflow")).expect_err("the second is refused");

        assert!(error.contains("MyFlow"), "{error}");
        // And what was there stays whole: the refusal must have touched
        // nothing, which is the reason it exists.
        assert_eq!(read_back(&dir, "MyFlow").id, "MyFlow");
        assert_eq!(entries(&dir).len(), 1);
        let _ = fs::remove_dir_all(&dir);
    }

    /// The measure that could have come out differently: the second write
    /// carries a different description, so a mutant that skipped `fs::rename`
    /// would leave the first one on disk.
    #[test]
    fn a_second_write_replaces_the_content_instead_of_leaving_it() {
        let dir = scratch("replacement");
        save_in(&dir, &a_full_flow("same-id")).expect("first write");
        let mut second = a_full_flow("same-id");
        second.description = "second version, different from the first".to_owned();
        save_in(&dir, &second).expect("second write");

        assert_eq!(
            read_back(&dir, "same-id").description,
            "second version, different from the first"
        );
        assert_eq!(entries(&dir).len(), 1, "and no temporary file left behind");
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn deleting_removes_the_flow_and_says_so_when_there_is_nothing_to_remove() {
        let dir = scratch("deletion");
        save_in(&dir, &a_full_flow("to-delete")).expect("write");
        delete_in(&dir, "to-delete").expect("delete");
        assert!(entries(&dir).is_empty());

        let error = delete_in(&dir, "never-existed").expect_err("an absent flow is not deleted");
        assert!(error.contains("does not exist"), "{error}");
        let _ = fs::remove_dir_all(&dir);
    }
}
