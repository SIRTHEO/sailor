//! Nothing belonging to the machine this was written on reaches the public
//! repository: no absolute path from a developer's home, no private name.
//!
//! **THE CHECK ARMS ITSELF FROM THE MACHINE, NOT FROM A LIST.** A list of
//! private names could not live in a public file without publishing exactly
//! what it exists to keep out. So the forbidden strings are read at run time.

use std::path::{Path, PathBuf};

/// The suffixes a reader can open and read words in.
///
/// **`.html` WAS MISSING AND `design/` WENT UNREAD.** The directory was walked
/// and every file in it dropped, so four design pages — three of them naming
/// the author's own tree — were invisible while the walker reported over a
/// hundred files scanned. A list of what to read is a list of what to skip.
const READ_AS_TEXT: &[&str] = &[
    ".rs", ".ts", ".tsx", ".css", ".mjs", ".md", ".json", ".toml", ".yml", ".html",
];

/// The places a reader of the published repository looks in.
const SCANNED_PLACES: &[&str] = &[
    "crates",
    "desktop/src",
    "desktop/src-tauri/src",
    "desktop/scripts",
    "docs",
    "design",
    "flows",
    "i18n",
    ".github",
];

/// Everything a reader of the published repository can open.
fn published_files() -> Vec<PathBuf> {
    let root = repo_root();
    let mut found = Vec::new();
    for place in SCANNED_PLACES {
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
            && READ_AS_TEXT
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
/// written in full, and the tilde form of the very same directory walked past
/// it — that is how people write a path in a document. Nine survived, one in a
/// shipped flow document telling every reader to `cd` where only its author
/// can. Not a leak: an instruction that is false for everybody else. The shape
/// still comes off the machine — git says where the tree is, no name is typed.
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
    // Where the list is and what counts as a name are read from
    // `toolbox::privacy`, the same place the command that writes a fault reads
    // them: two readers of one list drift, and the drift shows up as a gate
    // that passes what the store already took.
    let Some(list) = toolbox::privacy::where_the_names_are(
        std::env::var("SAILOR_PRIVATE_NAMES").ok(),
        std::env::var("HOME").ok(),
    ) else {
        println!("unarmed: nothing says where the private names are");
        return;
    };
    let Ok(text) = std::fs::read_to_string(&list) else {
        println!("unarmed: no private-names list at {}", list.display());
        return;
    };
    let names = toolbox::privacy::names_in(&text);
    println!(
        "armed with {} private names from {}",
        names.len(),
        list.display()
    );
    for name in names {
        let hits = occurrences_of(&name);
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

/// **A COUNT IS NOT A COVERAGE.** «More than a hundred files» stayed true while
/// every `.html` in `design/` was dropped on the floor. Asked against what git
/// tracks in the same places, the hole names itself: whatever is not obviously
/// binary must either be read or be given a reason, and a new text format shows
/// up as red instead of as silence.
#[test]
fn nothing_git_tracks_in_those_places_goes_unread() {
    let root = repo_root();
    let seen: std::collections::BTreeSet<PathBuf> = published_files().into_iter().collect();
    let out = std::process::Command::new("git")
        .args(["ls-files", "-z"])
        .current_dir(&root)
        .output()
        .expect("git ls-files");
    assert!(out.status.success(), "git could not list the tracked files");

    // Only what carries no words at all. `.svg` is deliberately absent: it is
    // text, and a path written into one would be published like any other.
    let binary = [
        ".png", ".jpg", ".jpeg", ".gif", ".ico", ".woff", ".woff2", ".ttf", ".pdf",
    ];
    let mut unread = Vec::new();
    for entry in String::from_utf8_lossy(&out.stdout).split('\0') {
        // On a directory boundary: plain `starts_with` makes `desktop/src-tauri`
        // look like a file inside `desktop/src`, and the gap it reports is not
        // real.
        let inside = SCANNED_PLACES.iter().any(|place| {
            entry
                .strip_prefix(*place)
                .is_some_and(|r| r.starts_with('/'))
        });
        if entry.is_empty() || !inside {
            continue;
        }
        if binary.iter().any(|suffix| entry.ends_with(suffix))
            // Excluded from the walk on purpose: every needle it names would
            // otherwise be its own hit.
            || entry.ends_with("nothing_from_this_machine_is_published.rs")
        {
            continue;
        }
        if !seen.contains(&root.join(entry)) {
            unread.push(entry.to_owned());
        }
    }

    assert!(
        unread.is_empty(),
        "git tracks {} file(s) in the scanned places that the walker never \
         opens, so nothing in them can ever be found: {:?}. Add the suffix to \
         READ_AS_TEXT, or say here why it is not read",
        unread.len(),
        unread
    );
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
