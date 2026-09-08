//! Every cargo Sailor starts says how many compilers it wants.
//!
//! **THE MACHINE IS SHARED BETWEEN SESSIONS**, so the core count is a ceiling
//! and not an answer. One command asked `machine::how_many_compilers`; the
//! release, which clones HEAD and builds it whole, took every core.

use std::path::{Path, PathBuf};

/// The verbs that compile. `metadata` and `generate-lockfile` do not, and
/// asking them for compilers would be a word nobody reads.
const COMPILES: &[&str] = &["\"build\"", "\"test\"", "\"clippy\"", "\"check\""];

/// How a count of compilers is written, in either of cargo's two spellings.
const SAYS_HOW_MANY: &[&str] = &["--jobs", "-j"];

/// How far past `Command::new("cargo")` the arguments of one invocation reach.
/// Measured on the longest of them: the release's suite, at nine lines.
const AN_INVOCATION_IS_THIS_LONG: usize = 20;

const ITSELF: &str = "no_cargo_of_ours_takes_the_whole_machine.rs";

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("the crate sits two levels under the root")
        .to_path_buf()
}

fn sources(root: &Path) -> Vec<PathBuf> {
    let mut found = Vec::new();
    for place in ["crates", "desktop/src-tauri/src"] {
        walk(&root.join(place), &mut found);
    }
    found
}

fn walk(dir: &Path, found: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            if path.file_name().is_some_and(|name| name == "target") {
                continue;
            }
            walk(&path, found);
        } else if path.extension().is_some_and(|kind| kind == "rs")
            && !path.file_name().is_some_and(|name| name == ITSELF)
        {
            found.push(path);
        }
    }
}

/// Every invocation of cargo that compiles and does not say how many, as
/// `path:line`.
fn takes_the_whole_machine(root: &Path) -> Vec<String> {
    let mut loud = Vec::new();
    for path in sources(root) {
        let Ok(text) = std::fs::read_to_string(&path) else {
            continue;
        };
        let lines: Vec<&str> = text.lines().collect();
        for (number, line) in lines.iter().enumerate() {
            if !line.contains("Command::new(\"cargo\")") {
                continue;
            }
            let until = (number + AN_INVOCATION_IS_THIS_LONG).min(lines.len());
            let invocation = lines[number..until].join("\n");
            let compiles = COMPILES.iter().any(|verb| invocation.contains(verb));
            let says = SAYS_HOW_MANY.iter().any(|word| invocation.contains(word));
            if compiles && !says {
                let shown = path.strip_prefix(root).unwrap_or(&path);
                loud.push(format!("{}:{}", shown.display(), number + 1));
            }
        }
    }
    loud
}

/// **NOT ONE OF THEM, AND NO SEED.** A cargo that takes the machine kills
/// whatever else is on it, and there is no version of that worth keeping.
#[test]
fn every_cargo_that_compiles_says_how_many_compilers_it_wants() {
    let root = root();
    let loud = takes_the_whole_machine(&root);
    workspace::measured(sources(&root).len(), "sources read for the cargo they start");
    assert!(
        loud.is_empty(),
        "these start a cargo that compiles without saying how many compilers fit, \
         and this machine is shared between sessions: {loud:?}. Ask \
         `machine::how_many_compilers`, or say a number outright",
        );
}

/// Whoever measures gets measured: a reader that stopped seeing the shape
/// would call every one of them quiet.
#[test]
fn the_reader_finds_an_invocation_planted_for_it() {
    let root = std::env::temp_dir().join(format!("sailor-cargo-jobs-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    let crates = root.join("crates").join("sample").join("src");
    std::fs::create_dir_all(&crates).expect("a throwaway crate");
    std::fs::write(
        crates.join("greedy.rs"),
        "fn go() {\n    Command::new(\"cargo\")\n        .args([\"test\", \"--quiet\"])\n}\n",
    )
    .expect("the planted invocation");
    std::fs::write(
        crates.join("polite.rs"),
        "fn go() {\n    Command::new(\"cargo\")\n        .args([\"test\", \"--jobs\", \"2\"])\n}\n",
    )
    .expect("the polite one");

    let found = takes_the_whole_machine(&root);

    assert_eq!(found.len(), 1, "one of the two was planted greedy: {found:?}");
    assert!(found[0].contains("greedy.rs"), "{found:?}");
    let _ = std::fs::remove_dir_all(&root);
}
