//! Where the project root is, and what the project declares about itself. A
//! flow must not know where the repository lives: `sviluppa-sailor.flow.json`
//! carried its author's home as `"workdir"` on seven steps, and launched from a
//! clone it worked — and committed — in the main repository, saying nothing.
//! The root comes from whoever launches; an absolute path inside a flow is a
//! flow that can be run in one place only. See fault 25.

use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

/// The file that declares "a Sailor project starts here".
///
/// A marker, not a mandatory configuration. It may be `{}`: what counts is
/// that it exists, because its position is what answers the question. The
/// contents serve whoever wants to declare something more.
pub const MARKER: &str = "sailor.json";

/// The origin of flows from a project that declared itself with [`MARKER`].
///
/// The family lives here and in [`crate::system`], and is compared literally —
/// by `tests/a_flow_that_calls_another.rs` and in `system.rs` — so a member moves
/// only in the same edit as the assertions that name it.
pub const ORIGIN_DECLARED: &str = "this project";

/// The origin of flows from a project *guessed* by walking up to a `flows/`
/// directory, with no marker at all. The warning rides in the origin and that
/// is not laziness: `sailor flow list` prints the origin on every row and the
/// window shows it beside every source, so it is the one place a reader really
/// looks and there is exactly one of it. A warning written elsewhere would be a
/// second truth to keep aligned — fault 10 — and would show only once.
pub const ORIGIN_GUESSED: &str = "this project (no sailor.json: root guessed)";

/// What a project declares about itself in its [`MARKER`]. Unknown fields are
/// kept, never a reason to discard the file: it is fault 8, where a descriptor
/// carrying a field this version did not know was thrown out whole.
/// `deny_unknown_fields` here would mean a project opened with a Sailor older
/// than the one that wrote it stops working, and stops silently — the root
/// vanishes, and paths go back to resolving wherever the process sits.
#[derive(Debug, Clone, Default, Deserialize, Serialize, PartialEq, Eq)]
pub struct Declaration {
    /// What the project is called for a reader. Empty means "it did not say",
    /// and whoever displays it falls back to the folder name.
    #[serde(default)]
    pub name: String,
    /// The documents whoever works here must read before touching anything.
    #[serde(default)]
    pub rules: Vec<String>,
    /// The project's checks, by name. Stays empty until someone fills it in by
    /// hand: guessing `cargo test` for an arbitrary project is the same
    /// presumption as the absolute path fault 25 is about.
    #[serde(default)]
    pub checks: BTreeMap<String, String>,
    /// Where the project's own equipment lives, if it has any.
    #[serde(default)]
    pub equipment: Option<String>,
    /// What this version does not recognise, kept rather than refused. Whoever
    /// wants to warn about it asks [`Declaration::unknown_fields`].
    #[serde(flatten)]
    pub extra: BTreeMap<String, Value>,
}

impl Declaration {
    /// The names of the fields this version does not know.
    ///
    /// Whoever displays them makes a warning *on the field*, never a refusal of
    /// the whole file: that is the shape fault 8 left written.
    pub fn unknown_fields(&self) -> Vec<String> {
        self.extra.keys().cloned().collect()
    }
}

/// The project root: the first directory holding a [`MARKER`], walking up from
/// `from` for the same reason `system::project_flows_from` does — a program is
/// almost never started at the root; the window starts in `desktop/src-tauri`.
/// The two live two steps apart so "which project is this" cannot become two
/// answers — fault 19. `None` is not "the current directory": nobody declared a
/// root, and whoever needs one fails saying so, not working wherever it lands.
pub fn find_root(from: &Path) -> Option<PathBuf> {
    let mut here = Some(from);
    while let Some(directory) = here {
        if directory.join(MARKER).is_file() {
            return Some(directory.to_path_buf());
        }
        here = directory.parent();
    }
    None
}

/// Reads a root's declaration.
///
/// An empty or unreadable marker is not a broken project: `{}` is a legitimate
/// declaration, and a file that will not read still leaves the root standing —
/// its *position* answers the question, not its contents.
pub fn declaration_at(root: &Path) -> Result<Declaration, String> {
    let path = root.join(MARKER);
    let text = std::fs::read_to_string(&path)
        .map_err(|error| format!("cannot read {}: {error}", path.display()))?;
    serde_json::from_str(&text)
        .map_err(|error| format!("{} is not a valid declaration: {error}", path.display()))
}

// ── The projects Sailor has been opened in ──────────────────────────────

/// The file in the home that holds them, beside `flows/` and `triggers.d/`.
///
/// **CONFIGURATION, NOT HISTORY.** A project one has opened has to list the
/// same on a machine where the ledger was never created — which is every
/// machine until the first run. The ledger answers "what happened"; this
/// answers "what am I working on", and the two fail independently.
pub const REGISTER: &str = "workspaces.json";

/// A project Sailor has been opened in.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Known {
    /// Where it is. The identity of an entry: two names may collide, a path
    /// may not.
    pub root: PathBuf,
    /// What it called itself when it was first seen. Kept even when the
    /// marker is gone, because a list of paths with no names is unreadable.
    pub name: String,
    /// Since when this project is worked on. Cannot be reconstructed from
    /// anything else once lost, which is why seeing it again never moves it.
    pub first_seen: i64,
    /// When it was last opened. What the list is ordered by.
    pub last_seen: i64,
    /// Fields a newer Sailor wrote and this one does not know. Kept, never a
    /// reason to discard the entry — fault 8, in the place it would hurt most.
    #[serde(flatten)]
    pub extra: BTreeMap<String, Value>,
}

/// The projects `home` remembers, plus the ones Sailor has worked in. `seen`
/// is what the caller worked in, never a scan of the disk — that would find
/// other people's projects and call them yours.
pub fn known_including(home: &Path, seen: &[PathBuf], at: i64) -> Result<Vec<Known>, String> {
    let mut known = known_in(home)?;
    for tree in seen {
        // The tree a terminal reports is not the root: work happens in
        // subfolders, so the marker is looked for walking up.
        let Some(root) = find_root(tree) else { continue };
        if known.iter().any(|entry| entry.root == root) {
            continue;
        }
        known.push(Known {
            name: name_declared_in(&root),
            root,
            // The register learns the real date next time somebody opens one.
            first_seen: at,
            last_seen: at,
            extra: BTreeMap::new(),
        });
    }
    known.sort_by(|left, right| right.last_seen.cmp(&left.last_seen));
    Ok(known)
}

/// What a tree calls itself, falling back to the directory name.
fn name_declared_in(root: &Path) -> String {
    declaration_at(root)
        .ok()
        .map(|declared| declared.name)
        .filter(|name| !name.is_empty())
        .unwrap_or_else(|| {
            root.file_name()
                .and_then(|name| name.to_str())
                .unwrap_or("?")
                .to_owned()
        })
}

/// Whether a remembered project is still declared where it was left.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Standing {
    /// The marker is where it was: the project is there.
    Declared,
    /// The path holds no marker any more — moved, renamed, or deleted. The
    /// entry stays on the list carrying this, because a list that silently
    /// shrinks cannot be told from a list that never had the entry.
    Gone,
}

/// What the register file holds. A struct and not a bare array so a later
/// version can add a sibling field without every older Sailor refusing to read.
#[derive(Debug, Default, Serialize, Deserialize)]
struct Register {
    #[serde(default)]
    workspaces: Vec<Known>,
    #[serde(flatten)]
    extra: BTreeMap<String, Value>,
}

/// The projects remembered in `home`, most recently opened first.
///
/// **A HOME THAT HAS SEEN NOTHING IS NOT A FAILURE.** No file means no
/// projects, and that is a complete answer: returning an error there would
/// make every caller treat a first run as a broken install.
pub fn known_in(home: &Path) -> Result<Vec<Known>, String> {
    let path = home.join(REGISTER);
    let text = match std::fs::read_to_string(&path) {
        Ok(text) => text,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => return Err(format!("cannot read {}: {error}", path.display())),
    };
    let register: Register = serde_json::from_str(&text)
        .map_err(|error| format!("{} is not a valid register: {error}", path.display()))?;
    let mut seen = register.workspaces;
    seen.sort_by(|left, right| right.last_seen.cmp(&left.last_seen));
    Ok(seen)
}

/// Writes down that `root` was opened at `at`.
///
/// **SEEING IT AGAIN MOVES ONE DATE AND LEAVES THE OTHER.** `first_seen`
/// answers "since when do I work on this" and nothing else can reconstruct it;
/// `last_seen` answers "was I here today" and is rewritten every time.
pub fn remember_in(home: &Path, root: &Path, at: i64) -> Result<(), String> {
    let path = home.join(REGISTER);
    let mut register: Register = match std::fs::read_to_string(&path) {
        Ok(text) => serde_json::from_str(&text)
            .map_err(|error| format!("{} is not a valid register: {error}", path.display()))?,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Register::default(),
        Err(error) => return Err(format!("cannot read {}: {error}", path.display())),
    };

    // The name is read from the declaration when there is one, and falls back
    // to the directory: a project with an empty `{}` marker still needs a name
    // to be picked out of a list.
    let name = declaration_at(root)
        .ok()
        .map(|declared| declared.name)
        .filter(|name| !name.is_empty())
        .or_else(|| {
            root.file_name()
                .map(|name| name.to_string_lossy().into_owned())
        })
        .unwrap_or_default();

    match register
        .workspaces
        .iter_mut()
        .find(|entry| entry.root == root)
    {
        Some(entry) => {
            entry.last_seen = at;
            if !name.is_empty() {
                entry.name = name;
            }
        }
        None => register.workspaces.push(Known {
            root: root.to_path_buf(),
            name,
            first_seen: at,
            last_seen: at,
            extra: BTreeMap::new(),
        }),
    }

    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|error| format!("cannot create {}: {error}", parent.display()))?;
    }
    let mut text = serde_json::to_string_pretty(&register)
        .map_err(|error| format!("cannot compose the register: {error}"))?;
    text.push('\n');
    std::fs::write(&path, text).map_err(|error| format!("cannot write {}: {error}", path.display()))
}

/// Whether the marker is still where this entry was left.
///
/// Read at the moment of asking, never stored: a stored answer would be a
/// second truth to keep aligned with the disk, which is fault 10.
pub fn standing_of(known: &Known) -> Standing {
    if known.root.join(MARKER).is_file() {
        Standing::Declared
    } else {
        Standing::Gone
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn scratch(label: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "sailor-workspace-{label}-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
        ));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).expect("scratch directory");
        dir
    }

    /// A counter in the name, not the clock alone: it is fault 21 — `cargo test`
    /// sends these tests through one process, and on a clock without nanosecond
    /// resolution they stole each other's directory.
    static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

    fn put_marker(dir: &Path, text: &str) {
        fs::create_dir_all(dir).expect("directory");
        fs::write(dir.join(MARKER), text).expect("marker");
    }

    /// **SIX ON THE DISK, ZERO ON THE LIST**: a tree that declared itself
    /// without `workspace init` was invisible to every screen.
    #[test]
    fn a_tree_sailor_worked_in_joins_the_list_if_it_declares_itself() {
        let home = scratch("home-seen");
        let declared = scratch("seen-declared");
        put_marker(&declared, r#"{"name":"il-progetto"}"#);
        let bare = scratch("seen-bare");
        fs::create_dir_all(&bare).expect("a tree with no marker");

        let known = known_including(&home, &[declared.clone(), bare.clone()], 100)
            .expect("the list is readable");

        let roots: Vec<&PathBuf> = known.iter().map(|entry| &entry.root).collect();
        assert_eq!(roots, vec![&declared], "a tree with no marker is not a project");
        assert_eq!(known[0].name, "il-progetto", "the marker names it, not the folder");
    }

    /// A terminal opened three folders down belongs to the project above it:
    /// reading the reported path as a root would have listed nothing.
    #[test]
    fn a_terminal_below_the_root_still_names_its_project() {
        let home = scratch("home-below");
        let root = scratch("below-root");
        put_marker(&root, r#"{"name":"la-casa"}"#);
        let deep = root.join("un-servizio").join("src");
        fs::create_dir_all(&deep).expect("a folder to work in");

        let known = known_including(&home, &[deep], 100).expect("the list is readable");

        assert_eq!(known.len(), 1, "the project was not found walking up: {known:?}");
        assert_eq!(known[0].root, root);
        assert_eq!(known[0].name, "la-casa");
    }

    /// **SEEING IT AGAIN NEVER MOVES `first_seen`.** It answers «since when do I
    /// work on this» and nothing else can reconstruct it once overwritten.
    #[test]
    fn a_project_already_registered_keeps_its_dates() {
        let home = scratch("home-dates");
        let root = scratch("dates-root");
        put_marker(&root, r#"{"name":"sailor"}"#);
        remember_in(&home, &root, 10).expect("registering it");

        let known = known_including(&home, &[root.clone()], 999).expect("the list is readable");

        assert_eq!(known.len(), 1, "the seen tree was added a second time: {known:?}");
        assert_eq!(known[0].first_seen, 10, "the date of the first day was rewritten");
    }

    /// The everyday case: work happens three directories below the root.
    #[test]
    fn the_root_is_the_folder_with_the_marker() {
        let root = scratch("walk-up");
        put_marker(&root, "{}");
        let deep = root.join("crates").join("flow").join("src");
        fs::create_dir_all(&deep).expect("subdirectory");

        assert_eq!(find_root(&deep), Some(root.clone()));

        let _ = fs::remove_dir_all(&root);
    }

    /// With no marker the answer is `None`, not the current directory:
    /// answering with it would let a flow work wherever it lands without saying
    /// so, which is fault 25 put back on its feet.
    #[test]
    fn without_a_marker_there_is_no_root() {
        let orphan = scratch("no-marker");
        let deep = orphan.join("one").join("two");
        fs::create_dir_all(&deep).expect("subdirectory");

        assert_eq!(find_root(&deep), None);

        let _ = fs::remove_dir_all(&orphan);
    }

    /// The nearest marker wins: a project inside a project is its own.
    #[test]
    fn the_nearest_marker_wins() {
        let outer = scratch("nested");
        put_marker(&outer, "{}");
        let inner = outer.join("inside");
        put_marker(&inner, "{}");

        assert_eq!(find_root(&inner), Some(inner.clone()));

        let _ = fs::remove_dir_all(&outer);
    }

    /// Fault 8, on this file: a field this version does not know must not make
    /// the declaration be discarded — it stays in `extra`, and a warning can be
    /// raised on it.
    #[test]
    fn an_unknown_field_is_kept_not_refused() {
        let root = scratch("unknown-field");
        put_marker(
            &root,
            r#"{"name": "sailor", "rules": ["AGENTS.md"], "tomorrow": 3}"#,
        );

        let declared = declaration_at(&root).expect("it reads all the same");

        assert_eq!(declared.name, "sailor");
        assert_eq!(declared.rules, vec!["AGENTS.md".to_owned()]);
        assert_eq!(declared.unknown_fields(), vec!["tomorrow".to_owned()]);

        let _ = fs::remove_dir_all(&root);
    }

    /// An empty marker is a legitimate declaration: position is what counts.
    #[test]
    fn an_empty_marker_is_a_valid_declaration() {
        let root = scratch("empty");
        put_marker(&root, "{}");

        let declared = declaration_at(&root).expect("empty declaration");

        assert_eq!(declared, Declaration::default());
        assert!(declared.checks.is_empty(), "checks is never guessed");

        let _ = fs::remove_dir_all(&root);
    }
}
