//! No file over the scale, counted as a ratchet: the ones over it today are
//! seeded, and the number may only fall.
//!
//! **A FILE IS TWO FILES.** Judges sit beside their subject here, so the two
//! are weighed apart: a wall of logic is split by responsibility, a wall of
//! judges is moved to a file of its own, and those are not the same repair.

use std::path::{Path, PathBuf};
use workspace::ratchet::{weigh, Weighed};

const LINES_OUT_OF_SCALE: usize = 2_000;

/// How many files carry more product than that today. Downwards only.
const OUT_OF_SCALE_TODAY: usize = 0;

/// And how many carry more judges than that. A separate seed, because the two
/// fall for different reasons.
const JUDGE_WALLS_TODAY: usize = 2;

/// Where a file stops being what it does and starts being what proves it.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Weight {
    product: usize,
    judges: usize,
}

/// A file that is nothing but judges, by where it lives or what it is called.
fn all_judges(path: &Path) -> bool {
    let name = path.file_name().and_then(|it| it.to_str()).unwrap_or_default();
    path.components()
        .any(|part| part.as_os_str() == "tests" || part.as_os_str() == "__tests__")
        || name == "tests.rs"
        || name.contains(".test.")
        || name.contains(".spec.")
}

/// **THE SEAM IS DECLARED, NOT GUESSED.** `#[cfg(test)]` is where this tree
/// puts its judges; a file without one is all product, which is not the same
/// as a file whose judges nobody found.
fn weigh_the_file(path: &Path, text: &str) -> Weight {
    let lines: Vec<&str> = text.lines().collect();
    if all_judges(path) {
        return Weight { product: 0, judges: lines.len() };
    }
    match lines.iter().position(|line| line.trim_start().starts_with("#[cfg(test)]")) {
        Some(at) => Weight { product: at, judges: lines.len() - at },
        None => Weight { product: lines.len(), judges: 0 },
    }
}

fn sources() -> Vec<PathBuf> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let mut found = Vec::new();
    for top in ["crates", "desktop/src", "desktop/src-tauri/src"] {
        walk(&root.join(top), &mut found);
    }
    found
}

fn walk(dir: &Path, found: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let name = entry.file_name();
        if path.is_dir() {
            if name != "target" && name != "node_modules" {
                walk(&path, found);
            }
        } else if matches!(
            path.extension().and_then(|e| e.to_str()),
            Some("rs") | Some("ts") | Some("tsx")
        ) {
            found.push(path);
        }
    }
}

fn weighed() -> Vec<(PathBuf, Weight)> {
    sources()
        .into_iter()
        .filter_map(|path| {
            let text = std::fs::read_to_string(&path).ok()?;
            let weight = weigh_the_file(&path, &text);
            Some((path, weight))
        })
        .collect()
}

fn listed(over: &[(PathBuf, usize)]) -> String {
    over.iter()
        .map(|(path, lines)| format!("{lines} {}", path.display()))
        .collect::<Vec<_>>()
        .join("\n")
}

fn over_the_scale(
    all: &[(PathBuf, Weight)],
    of: impl Fn(&Weight) -> usize,
) -> Vec<(PathBuf, usize)> {
    let mut over: Vec<(PathBuf, usize)> = all
        .iter()
        .map(|(path, weight)| (path.clone(), of(weight)))
        .filter(|(_, lines)| *lines > LINES_OUT_OF_SCALE)
        .collect();
    over.sort_by_key(|(_, lines)| std::cmp::Reverse(*lines));
    over
}

#[test]
fn no_more_files_run_past_the_scale_than_today() {
    let all = weighed();
    workspace::measured(all.len(), "sources weighed, product apart from judges");
    let over = over_the_scale(&all, |weight| weight.product);
    match weigh(OUT_OF_SCALE_TODAY, over.len()) {
        Weighed::TreeIsAbove(more) => panic!(
            "files over {LINES_OUT_OF_SCALE} lines of product: {} ({more} more than the seed's \
             {OUT_OF_SCALE_TODAY}). Split by responsibility:\n{}",
            over.len(),
            listed(&over)
        ),
        Weighed::TreeIsBelow(apart) => panic!(
            "the seed says {OUT_OF_SCALE_TODAY} and the tree holds {}, {apart} apart: write \
             OUT_OF_SCALE_TODAY = {}",
            over.len(),
            over.len()
        ),
        Weighed::Holds => {}
    }
}

#[test]
fn no_more_walls_of_judges_than_today() {
    let all = weighed();
    let over = over_the_scale(&all, |weight| weight.judges);
    match weigh(JUDGE_WALLS_TODAY, over.len()) {
        Weighed::TreeIsAbove(more) => panic!(
            "files carrying over {LINES_OUT_OF_SCALE} lines of judges: {} ({more} more than the \
             seed's {JUDGE_WALLS_TODAY}). Move them to a file of their own, beside the one they \
             judge:\n{}",
            over.len(),
            listed(&over)
        ),
        Weighed::TreeIsBelow(apart) => panic!(
            "the seed says {JUDGE_WALLS_TODAY} and the tree holds {}, {apart} apart: write \
             JUDGE_WALLS_TODAY = {}",
            over.len(),
            over.len()
        ),
        Weighed::Holds => {}
    }
}

/// **A COUNTER THAT READ THE WHOLE FILE MEASURED NEITHER THING.** Proved on the
/// shapes it gets wrong: a small module behind a wall of judges, a file that is
/// all judges by where it lives, and one with no judges at all.
#[test]
fn the_scale_tells_what_a_file_does_from_what_proves_it() {
    let module = "fn one() {}\nfn two() {}\n#[cfg(test)]\nmod tests {\n}\n";
    assert_eq!(
        weigh_the_file(Path::new("crates/a/src/lib.rs"), module),
        Weight { product: 2, judges: 3 },
        "the judges beside a module were counted as what it does"
    );
    assert_eq!(
        weigh_the_file(Path::new("crates/a/tests/a_judge.rs"), module),
        Weight { product: 0, judges: 5 },
        "a file that is nothing but judges was read as product"
    );
    assert_eq!(
        weigh_the_file(Path::new("crates/a/src/lib.rs"), "fn one() {}\n"),
        Weight { product: 1, judges: 0 },
        "a file with no judges was credited with some"
    );
}

/// The perimeter answers: a walk that finds nothing declares no file over the
/// scale, and that is a green having read nothing.
#[test]
fn the_scale_can_still_see_the_files_it_weighs() {
    let all = weighed();
    assert!(
        all.len() > 100,
        "the walk found {} sources, so nothing was weighed",
        all.len()
    );
    assert!(
        all.iter().any(|(_, weight)| weight.judges > 0),
        "not one file was found to carry a judge, so the seam is never seen"
    );
}
