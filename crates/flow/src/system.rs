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
pub const PLACE: &str = "(spediti col prodotto)";

/// What the system source is called for a reader. Still in Italian on purpose:
/// this is a literal other code compares — `sailor::flow_cmd` asserts
/// `report.contains("di sistema")` on a report built from it — so it moves when
/// that assertion moves, in the same edit, and not before.
pub const BUILTIN_ORIGIN: &str = "di sistema";

/// The flows the product ships with: flow name and file text. Embedded like
/// `toolbox::descriptor::BUILTIN` and for the same reason — a freshly installed
/// binary, or one copied to another machine, has to answer without anyone
/// having copied a folder. The name is what would read on disk without
/// `.flow.json`, and a test checks it matches the `id` declared inside: two
/// names for one thing would make "run this" and "override this" differ.
pub const FLOWS: &[(&str, &str)] = &[
    (
        "strumenti-di-questa-macchina",
        include_str!("../system/strumenti-di-questa-macchina.flow.json"),
    ),
    (
        "migrazione-a-sailor",
        include_str!("../system/migrazione-a-sailor.flow.json"),
    ),
    // Shipped because the shipped rules name it: the routing rules in
    // `crates/terminal/descriptors/default.json` travel inside the binary and
    // send work here, so as a project flow the rule pointed at nothing on every
    // machine but this one — and no test saw it, since all of them ran here.
    (
        "smista-il-lavoro",
        include_str!("../system/smista-il-lavoro.flow.json"),
    ),
];

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
/// precedence rule: on a name clash the later source wins, so `di sistema` <
/// `tuoi` < `del progetto`.
pub fn sources(
    home_flows: &Path,
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
            origin: "dichiarati",
            dir: declared.to_path_buf(),
        });
        return sources;
    }
    sources.push(FlowSource {
        origin: "tuoi",
        dir: home_flows.to_path_buf(),
    });
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
fn project_flows(working: &Path, home_flows: &Path) -> Option<(&'static str, PathBuf)> {
    if let Some(root) = crate::workspace::find_root(working) {
        let flows = root.join("flows");
        // Home is never also the project: counting it twice would show every
        // flow in duplicate.
        return (flows != home_flows).then_some((crate::workspace::ORIGIN_DECLARED, flows));
    }
    // The fallback stays: removing it would make the flows of every project
    // that has not declared itself vanish at once — this repository included,
    // until someone writes it a marker. That is a deprecation, and a
    // deprecation is not done alone: it stays, and the origin it carries says
    // out loud that it is a fallback.
    project_flows_from(working, home_flows)
        .map(|flows| (crate::workspace::ORIGIN_GUESSED, flows))
}

/// The same sources, read from this process's environment: one copy of the
/// precedence rule, never two. The first copy is `ui::gather::flow_sources`;
/// the second was about to be born for whoever builds the action registry,
/// because the `subflow` step must look for the flow it calls exactly where
/// `sailor flow run` looks, or two machines run different flows under one name
/// without saying so. Home stays an argument: `flow` must not need `ledger`.
pub fn sources_from_env(home_flows: &Path) -> Vec<FlowSource> {
    let declared = std::env::var_os("SAILOR_FLOWS").map(PathBuf::from);
    let working = std::env::current_dir().ok();
    sources(home_flows, working.as_deref(), declared.as_deref())
}

/// The project's flows directory, found by walking up — not a luxury: a program
/// is almost never started at the project root. The window starts in
/// `desktop/src-tauri`, an editor where its last file was, a terminal where the
/// user stood; measured, the window opened to work on Sailor saw none of
/// Sailor's four flows. The directory must hold a flow, not merely be named
/// `flows`, or an empty one stops the climb short of the real one.
pub fn project_flows_from(working: &Path, home_flows: &Path) -> Option<PathBuf> {
    let mut here = Some(working);
    while let Some(directory) = here {
        let candidate = directory.join("flows");
        if candidate != home_flows && holds_a_flow(&candidate) {
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
/// forgotten branch would say "di sistema: 0 flows" while system flows run.
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
    let mut registry = FlowRegistry::new();
    let Ok(entries) = std::fs::read_dir(dir) else {
        return registry;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let file_name = path
            .file_name()
            .map(|name| name.to_string_lossy())
            .unwrap_or_default();
        let is_flow_json = file_name.ends_with(".flow.json");
        let is_json = path.extension().and_then(|ext| ext.to_str()) == Some("json");
        if !is_flow_json && !is_json {
            continue;
        }
        let name = file_name
            .strip_suffix(".flow.json")
            .or_else(|| file_name.strip_suffix(".json"))
            .unwrap_or(&file_name)
            .to_owned();
        if name.is_empty() {
            continue;
        }
        let text = match std::fs::read_to_string(&path) {
            Ok(content) => content,
            Err(error) => {
                registry.insert(
                    name,
                    Err(format!("cannot read {}: {error}", path.display())),
                );
                continue;
            }
        };
        match serde_json::from_str::<FlowFile>(&text) {
            Ok(flow) => {
                registry.insert(name, Ok(flow));
            }
            Err(error) => {
                registry.insert(
                    name,
                    Err(format!("{} is not a valid flow: {error}", path.display())),
                );
            }
        }
    }
    registry
}

/// The flows of every source, each with the origin it came from.
///
/// On a name clash the last source wins — the most specific — the same rule as
/// tool descriptors, for the same reason: whoever works on a project expects
/// the project's flow to be the one that runs. The replacement is not silent:
/// the origin stays visible on every line.
pub fn load_all(sources: &[FlowSource]) -> Vec<(String, &'static str, Result<FlowFile, String>)> {
    let mut found: Vec<(String, &'static str, Result<FlowFile, String>)> = Vec::new();
    for source in sources {
        for (name, entry) in registry_of(source) {
            match found.iter_mut().find(|(existing, _, _)| existing == &name) {
                Some(slot) => *slot = (name, source.origin, entry),
                None => found.push((name, source.origin, entry)),
            }
        }
    }
    found.sort_by(|a, b| a.0.cmp(&b.0));
    found
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
    let id = safe_flow_id(&flow.id)?;
    fs::create_dir_all(flows_dir)
        .map_err(|error| format!("cannot prepare the flows directory: {error}"))?;
    let file_name = format!("{id}.flow.json");
    reject_a_name_that_collides_only_by_case(flows_dir, &file_name)?;
    let target = flows_dir.join(&file_name);
    let mut text = serde_json::to_string_pretty(flow)
        .map_err(|error| format!("cannot compose the flow as JSON: {error}"))?;
    // A text file ends with a newline: without one, `git diff` says so on every
    // rewritten flow, and the next hand-added line lands stuck to the last.
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
        Err(error) => Err(format!(
            "cannot delete {}: {error}",
            target.display()
        )),
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
        let dir = std::env::temp_dir().join(format!(
            "sailor-sistema-{label}-{}",
            std::process::id()
        ));
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
        let places = sources(Path::new("/home/flows"), None, None);
        assert_eq!(places[0], FlowSource::builtin());
        assert_eq!(places[0].origin, "di sistema");
        assert_eq!(places.last().expect("at least one").origin, "tuoi");
    }

    /// `SAILOR_FLOWS` clears home and project out of the way, not the binary's
    /// own equipment: that is in no folder, so there is no folder to replace.
    #[test]
    fn a_declared_folder_replaces_the_disk_but_not_the_binary() {
        let places = sources(
            Path::new("/home/flows"),
            None,
            Some(Path::new("/here/the/flows")),
        );
        let origins: Vec<&str> = places.iter().map(|p| p.origin).collect();
        assert_eq!(origins, vec!["di sistema", "dichiarati"]);
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

        let all = load_all(&sources(&home_flows, None, None));

        let (_, origin, _) = all
            .iter()
            .find(|(name, _, _)| name == shipped)
            .expect("the flow is there");
        assert_eq!(*origin, "tuoi", "the user's own wins");
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
        let all = load_all(&sources(&nowhere.join("flows"), None, None));
        assert_eq!(all.len(), FLOWS.len());
        assert!(all.iter().all(|(_, origin, _)| *origin == "di sistema"));
    }

    /// The measured defect: the window started in `desktop/src-tauri` and did
    /// not see the project's flows, two directories further up.
    #[test]
    fn the_project_flows_are_found_from_a_subfolder() {
        let root = scratch("walk-up");
        put_flow(&root.join("flows"), "one");
        let deep = root.join("desktop").join("src-tauri");
        fs::create_dir_all(&deep).expect("subdirectory");

        let found = project_flows_from(&deep, Path::new("/home/elsewhere/flows"));

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

        let found = project_flows_from(&middle, Path::new("/home/elsewhere/flows"));

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

        assert_eq!(project_flows_from(&home, &home_flows), None);
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

        let places = sources(Path::new("/home/flows"), Some(&deep), None);

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

        let places = sources(Path::new("/home/flows"), Some(&deep), None);

        let last = places.last().expect("at least one");
        assert_eq!(last.dir, root.join("flows"));
        assert_eq!(last.origin, crate::workspace::ORIGIN_GUESSED);

        let _ = fs::remove_dir_all(&root);
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
        let error =
            save_in(&dir, &a_full_flow("under/directory")).expect_err("id with / refused");
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
