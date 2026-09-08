//! Nothing belonging to the machine this was written on reaches the public
//! repository: no absolute path from a developer's home, no private name.
//!
//! **THE CHECK ARMS ITSELF FROM THE MACHINE, NOT FROM A LIST.** A list of
//! private names could not live in a public file without publishing exactly
//! what it exists to keep out. So the forbidden strings are read at run time.

use std::path::{Path, PathBuf};

/// What carries no words at all. Everything else git tracks is read: a list of
/// suffixes and a list of places both skipped in silence.
///
/// `.svg` is deliberately absent: it is text, and a path written into one is
/// published like any other.
const CARRIES_NO_WORDS: &[&str] = &[
    ".png", ".jpg", ".jpeg", ".gif", ".ico", ".woff", ".woff2", ".ttf", ".pdf",
];

/// Itself excluded: every needle it names would otherwise be its own hit.
const ITSELF: &str = "nothing_from_this_machine_is_published.rs";

/// Everything a reader of the published repository can open: what git tracks,
/// minus what carries no words.
///
/// **AND ONLY WHAT IS PUBLISHED**: a file git does not track is nobody's but
/// its author's, and accusing the sketches somebody keeps beside their work is
/// a red they cannot answer.
fn published_files() -> Vec<PathBuf> {
    published_files_under(&repo_root())
}

/// The same reading, of whatever tree it is pointed at, so the reader can be
/// put to a tree with a leak planted in it. Empty where git cannot answer: the
/// callers declare that as measuring nothing rather than as a clean tree.
fn published_files_under(root: &Path) -> Vec<PathBuf> {
    let Some(tracked) = tracked_paths(root) else {
        return Vec::new();
    };
    tracked
        .into_iter()
        .filter(|inside| {
            let name = inside.to_string_lossy();
            !CARRIES_NO_WORDS.iter().any(|suffix| name.ends_with(suffix)) && !name.ends_with(ITSELF)
        })
        .map(|inside| root.join(inside))
        .filter(|path| path.is_file())
        .collect()
}

/// What git tracks under this root, or `None` where git cannot answer.
fn tracked_paths(root: &Path) -> Option<std::collections::BTreeSet<PathBuf>> {
    let out = std::process::Command::new("git")
        .args(["ls-files", "-z"])
        .current_dir(root)
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    Some(
        String::from_utf8_lossy(&out.stdout)
            .split('\0')
            .filter(|entry| !entry.is_empty())
            .map(PathBuf::from)
            .collect(),
    )
}

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("the crate sits two levels under the root")
        .to_owned()
}

/// Every place a forbidden string appears, as `path:line`.
fn occurrences_of(needle: &str) -> Vec<String> {
    occurrences_under(&repo_root(), needle)
}

fn occurrences_under(root: &Path, needle: &str) -> Vec<String> {
    let lowered = needle.to_lowercase();
    let mut hits = Vec::new();
    for path in published_files_under(root) {
        let Ok(text) = std::fs::read_to_string(&path) else {
            continue;
        };
        for (number, line) in text.lines().enumerate() {
            // The rule is asked of `toolbox::privacy`, never copied: two
            // readers of one list drift.
            if toolbox::privacy::names_at(&line.to_lowercase(), &lowered).is_some() {
                let shown = path.strip_prefix(root).unwrap_or(&path);
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
/// every `.html` in `design/` was dropped on the floor, and stayed true again
/// while 42 tracked files sat outside every scanned place. The perimeter is now
/// what git tracks, so this asks the one question that is left: is anything
/// tracked, carrying words, that the reader never opens?
#[test]
fn nothing_git_tracks_goes_unread() {
    let root = repo_root();
    // Fault 100: outside the top of a repository the oracle is empty, not
    // clean — the answer names the files of whatever repository sits above.
    if !workspace::is_the_top_of_its_repository(&root) {
        workspace::measured_nothing("this tree is not the top of a repository, so the list of tracked files this check compares against is empty");
        return;
    }
    let Some(tracked) = tracked_paths(&root) else {
        workspace::measured_nothing("git would not list the tracked files, so there is no perimeter to read");
        return;
    };
    let seen: std::collections::BTreeSet<PathBuf> = published_files().into_iter().collect();
    workspace::measured_against(seen.len(), "files opened", tracked.len(), "paths git tracks");

    let unread: Vec<String> = tracked
        .iter()
        .map(|inside| inside.to_string_lossy().into_owned())
        .filter(|name| {
            !CARRIES_NO_WORDS.iter().any(|suffix| name.ends_with(suffix))
                && !name.ends_with(ITSELF)
                && !seen.contains(&root.join(name))
        })
        .collect();

    assert!(
        unread.is_empty(),
        "git tracks {} file(s) carrying words that the reader never opens, so \
         nothing in them can ever be found: {:?}. Read them, or name the suffix \
         in CARRIES_NO_WORDS and say here why it holds no words",
        unread.len(),
        unread
    );
}

/// **WHOEVER MEASURES GETS MEASURED.** If the walker stopped finding files, both
/// tests above would go green for ever while the repository leaked.
#[test]
fn the_check_can_still_see_the_files_it_reads() {
    let files = published_files();
    workspace::measured(files.len(), "published files opened");
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

/// **A CLEAN TREE CANNOT SAY THE READER WOULD CATCH A LEAK.** So one is planted:
/// a throwaway repository under the temporary directory carries an invented home
/// path inside a `design/*.html` — the very suffix whose absence once made that
/// whole directory invisible — and the reader is asked for it. Untracked, the
/// same words are nobody's business, and that half is asked too.
#[test]
fn a_home_path_planted_in_a_throwaway_repository_is_found() {
    let invented_home = "/Users/a-name-nobody-here-has";
    let root = std::env::temp_dir().join(format!(
        "sailor-planted-leak-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(root.join("design")).expect("a throwaway design directory");
    std::fs::create_dir_all(root.join("crates/sample/src")).expect("a throwaway crate directory");
    std::fs::write(
        root.join("design/preview.html"),
        format!("<p>open {invented_home}/personal/sailor/design</p>\n"),
    )
    .expect("the planted page writes");
    std::fs::write(
        root.join("crates/sample/src/sketch.rs"),
        format!("// {invented_home}/personal/notes\n"),
    )
    .expect("the untracked sketch writes");
    let started = std::process::Command::new("git")
        .arg("-C")
        .arg(&root)
        .args(["init", "-q"])
        .status()
        .expect("git starts a throwaway repository");
    assert!(started.success(), "the throwaway repository was not started");
    let added = std::process::Command::new("git")
        .arg("-C")
        .arg(&root)
        .args(["add", "--", "design/preview.html"])
        .status()
        .expect("git tracks the planted page");
    assert!(added.success(), "the planted page was not tracked");

    let hits = occurrences_under(&root, invented_home);
    std::fs::remove_dir_all(&root).expect("the throwaway repository goes");

    assert_eq!(
        hits,
        vec!["design/preview.html:1".to_owned()],
        "a home path was written into a tracked page and the reader did not \
         name it there, and there alone: the sketch git does not track is its \
         author's and must not be accused"
    );
}

/// **THE HOLE THIS PERIMETER CLOSED.** The reader used to look only inside a
/// hand-written list of directories, and 42 tracked files sat outside every one
/// of them: a leak in `sailor.json` or `CLAUDE.md` was published and the judge
/// stayed green. So one is planted where no such list would have reached.
#[test]
fn a_home_path_planted_outside_every_named_place_is_found() {
    let invented_home = "/Users/a-name-nobody-here-has";
    let root = std::env::temp_dir().join(format!(
        "sailor-planted-leak-at-the-root-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).expect("a throwaway repository directory");
    std::fs::write(
        root.join("harbour.json"),
        format!("{{\"moorings\": \"{invented_home}/personal/sailor\"}}\n"),
    )
    .expect("the planted descriptor writes");
    let started = std::process::Command::new("git")
        .arg("-C")
        .arg(&root)
        .args(["init", "-q"])
        .status()
        .expect("git starts a throwaway repository");
    assert!(started.success(), "the throwaway repository was not started");
    let added = std::process::Command::new("git")
        .arg("-C")
        .arg(&root)
        .args(["add", "--", "harbour.json"])
        .status()
        .expect("git tracks the planted descriptor");
    assert!(added.success(), "the planted descriptor was not tracked");

    let hits = occurrences_under(&root, invented_home);
    std::fs::remove_dir_all(&root).expect("the throwaway repository goes");

    assert_eq!(
        hits,
        vec!["harbour.json:1".to_owned()],
        "a home path was written into a tracked file at the top of the tree, \
         where no list of directories reaches, and the reader did not find it"
    );
}

/// **THE JUDGE MUST BE ABLE TO SAY IT DID NOT MEASURE.** Both roads to that
/// answer are asked here of a directory built for the purpose: it is not the
/// top of a repository, and git will not list it. A leak is put in it so an
/// empty answer cannot be mistaken for a clean tree — fault 100.
#[test]
fn a_tree_outside_a_repository_makes_the_judge_declare_it_measured_nothing() {
    let plain =
        std::env::temp_dir().join(format!("sailor-unpublished-{}-{}", std::process::id(), line!()));
    let _ = std::fs::remove_dir_all(&plain);
    std::fs::create_dir_all(plain.join("design")).expect("a scratch");
    std::fs::write(
        plain.join("design").join("a-sketch.html"),
        "<!-- /Users/somebody/personal/sailor -->\n",
    )
    .expect("a leak no repository tracks");

    assert!(
        !workspace::is_the_top_of_its_repository(&plain),
        "the road this judge takes to declare it measured nothing is closed"
    );
    assert_eq!(
        tracked_paths(&plain),
        None,
        "git listed a directory that is no repository, so the perimeter would be \
         read as empty instead of unreadable"
    );
    assert!(
        published_files_under(&plain).is_empty(),
        "with no perimeter to read the reader must open nothing: it opened files \
         and the planted leak would be judged against a list that is not this tree's"
    );

    let _ = std::fs::remove_dir_all(&plain);
}
