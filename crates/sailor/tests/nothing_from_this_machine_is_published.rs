//! Nothing belonging to the machine this was written on reaches the public
//! repository: no absolute path from a developer's home, no private name.
//!
//! **THE CHECK ARMS ITSELF FROM THE MACHINE, NOT FROM A LIST.** A list of
//! private names could not live in a public file without publishing exactly
//! what it exists to keep out. So the forbidden strings are read at run time.

use std::path::{Path, PathBuf};

/// Where the machine keeps names that must not be committed: one per line,
/// `#` for a comment. `SAILOR_PRIVATE_NAMES` overrides the location.
const PRIVATE_NAMES: &str = "personal/.sailor-notes/private-names";

/// Everything a reader of the published repository can open.
fn published_files() -> Vec<PathBuf> {
    let root = repo_root();
    let mut found = Vec::new();
    for place in [
        "crates",
        "desktop/src",
        "desktop/src-tauri/src",
        "desktop/scripts",
        "docs",
        "design",
        "flows",
        ".github",
    ] {
        walk(&root.join(place), &mut found);
    }
    for file in ["AGENTS.md", "README.md", "Cargo.toml"] {
        let path = root.join(file);
        if path.is_file() {
            found.push(path);
        }
    }
    found
}

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("the crate sits two levels under the root")
        .to_owned()
}

fn walk(dir: &Path, found: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let name = entry.file_name().to_string_lossy().into_owned();
        if path.is_dir() {
            if !matches!(name.as_str(), "target" | ".git" | "node_modules" | "dist") {
                walk(&path, found);
            }
        } else if !name.ends_with("-lock.json")
            // Itself excluded, or every needle it names would be its own hit.
            && name != "nothing_from_this_machine_is_published.rs"
            && [".rs", ".ts", ".tsx", ".css", ".mjs", ".md", ".json", ".toml", ".yml"]
                .iter()
                .any(|suffix| name.ends_with(suffix))
        {
            found.push(path);
        }
    }
}

/// Every place a forbidden string appears, as `path:line`.
fn occurrences_of(needle: &str) -> Vec<String> {
    let lowered = needle.to_lowercase();
    let root = repo_root();
    let mut hits = Vec::new();
    for path in published_files() {
        let Ok(text) = std::fs::read_to_string(&path) else {
            continue;
        };
        for (number, line) in text.lines().enumerate() {
            if line.to_lowercase().contains(&lowered) {
                let shown = path.strip_prefix(&root).unwrap_or(&path);
                hits.push(format!("{}:{}", shown.display(), number + 1));
            }
        }
    }
    hits
}

/// **THE HOME OF WHOEVER RUNS THIS.** Not the shape `/Users/<x>/`: fixtures
/// legitimately carry invented homes, and `flow_cmd` must name the prefixes it
/// detects. What may never be committed is the real one — which every machine
/// knows about itself, so no list is needed and every contributor is covered.
#[test]
fn no_path_from_the_machine_this_runs_on_is_written_down() {
    let Ok(home) = std::env::var("HOME") else {
        return;
    };
    // A very short `HOME` would match half the tree: better silent than lying.
    if home.len() < 6 {
        return;
    }
    let hits = occurrences_of(&home);
    assert!(
        hits.is_empty(),
        "the home directory of whoever is working here is written into {} places, \
         and this repository is world-readable: {}",
        hits.len(),
        hits.join(", ")
    );
}

/// **THE SAME PLACE, SPELLED WITH A TILDE.** The check above forbids the home
/// written in full, and `~/personal/sailor` walked straight past it: it is the
/// same directory written the way people write it in documents. Four survived,
/// one of them in a shipped flow document telling every reader to `cd` into a
/// directory only its author has. Still a shape read off the machine — git says
/// where the tree is, nobody writes a name down.
#[test]
fn the_repository_does_not_name_its_own_place_on_this_machine() {
    let Ok(home) = std::env::var("HOME") else {
        return;
    };
    let Some(tree) = main_worktree() else {
        return;
    };
    let Ok(below) = tree.strip_prefix(&home) else {
        // The tree is not under this home: nothing to abbreviate, nothing to say.
        return;
    };
    let tilde = format!("~{}", Path::new("/").join(below).display());
    if tilde.len() < 6 {
        return;
    }

    let hits = occurrences_of(&tilde);
    assert!(
        hits.is_empty(),
        "«{tilde}» is where this repository sits on one machine, and it is \
         written into {} places that ship. A reader who is not its author has \
         no such directory: {}",
        hits.len(),
        hits.join(", ")
    );
}

/// Where the repository proper sits, asked of git so that a worktree answers
/// with the tree it belongs to rather than with itself.
fn main_worktree() -> Option<PathBuf> {
    let out = std::process::Command::new("git")
        .args(["rev-parse", "--path-format=absolute", "--git-common-dir"])
        .current_dir(repo_root())
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    let git_dir = PathBuf::from(String::from_utf8(out.stdout).ok()?.trim());
    git_dir.parent().map(Path::to_path_buf)
}

/// The names that cannot be listed in public: read from outside the repository,
/// so this is armed on the machine that could leak them and quiet elsewhere.
#[test]
fn the_names_this_machine_declares_private_appear_nowhere() {
    let list = std::env::var("SAILOR_PRIVATE_NAMES")
        .map(PathBuf::from)
        .unwrap_or_else(|_| {
            std::env::var("HOME")
                .map(|home| PathBuf::from(home).join(PRIVATE_NAMES))
                .unwrap_or_default()
        });
    let Ok(text) = std::fs::read_to_string(&list) else {
        println!("unarmed: no private-names list at {}", list.display());
        return;
    };
    let names: Vec<&str> = text
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty() && !line.starts_with('#'))
        .collect();
    println!(
        "armed with {} private names from {}",
        names.len(),
        list.display()
    );
    for name in names {
        let hits = occurrences_of(name);
        // The name itself is never echoed: this message is read on a terminal
        // whose scrollback can be pasted anywhere.
        assert!(
            hits.is_empty(),
            "a name declared private in {} appears in {} places: {}",
            list.display(),
            hits.len(),
            hits.join(", ")
        );
    }
}

/// **WHOEVER MEASURES GETS MEASURED.** If the walker stopped finding files, both
/// tests above would go green for ever while the repository leaked.
#[test]
fn the_check_can_still_see_the_files_it_reads() {
    let files = published_files();
    println!("{} published files scanned", files.len());
    assert!(
        files.len() > 100,
        "only {} files found: the walker is blind",
        files.len()
    );
    assert!(
        !occurrences_of("spend_cap_micros").is_empty(),
        "a string known to be in the sources was not found: the reader is blind"
    );
    assert!(
        occurrences_of("nessun-testo-simile-esiste-in-questo-albero").is_empty(),
        "a string known to be absent was found: the reader says yes to everything"
    );
}
