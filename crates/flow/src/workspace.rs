//! Where the project root is, and what the project declares about itself. A
//! flow must not know where the repository lives: `sviluppa-sailor.flow.json`
//! carried its author's home as `"workdir"` on seven steps, and launched from a
//! clone it worked — and committed — in the main repository, saying nothing.
//! The root comes from whoever launches; an absolute path inside a flow is a
//! flow that can be run in one place only. See fault 25.

use serde::Deserialize;
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
/// Still in Italian on purpose. The origin family — `di sistema`, `tuoi`,
/// `dichiarati`, `del progetto` — is compared literally, here by
/// `tests/a_flow_that_calls_another.rs` and in `system.rs`, so it moves only in
/// the same edit as the assertions that name it.
pub const ORIGIN_DECLARED: &str = "del progetto";

/// The origin of flows from a project *guessed* by walking up to a `flows/`
/// directory, with no marker at all. The warning rides in the origin and that
/// is not laziness: `sailor flow list` prints the origin on every row and the
/// window shows it beside every source, so it is the one place a reader really
/// looks and there is exactly one of it. A warning written elsewhere would be a
/// second truth to keep aligned — fault 10 — and would show only once.
pub const ORIGIN_GUESSED: &str = "del progetto (nessun sailor.json: radice indovinata)";

/// What a project declares about itself in its [`MARKER`]. Unknown fields are
/// kept, never a reason to discard the file: it is fault 8, where a descriptor
/// carrying a field this version did not know was thrown out whole.
/// `deny_unknown_fields` here would mean a project opened with a Sailor older
/// than the one that wrote it stops working, and stops silently — the root
/// vanishes, and paths go back to resolving wherever the process sits.
#[derive(Debug, Clone, Default, Deserialize, PartialEq, Eq)]
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
