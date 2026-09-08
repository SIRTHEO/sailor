//! No file over the scale, counted as a ratchet: the ones over it today are
//! seeded, and the number may only fall.

use std::path::{Path, PathBuf};
use workspace::ratchet::{weigh, Weighed};

const LINES_OUT_OF_SCALE: usize = 2_000;

/// How many files run over today. Downwards only.
const OUT_OF_SCALE_TODAY: usize = 4;

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

#[test]
fn no_more_files_run_past_the_scale_than_today() {
    let walked = sources();
    workspace::measured(walked.len(), "sources counted line by line");
    let mut over: Vec<(usize, PathBuf)> = walked
        .into_iter()
        .filter_map(|path| {
            let lines = std::fs::read_to_string(&path).ok()?.lines().count();
            (lines > LINES_OUT_OF_SCALE).then_some((lines, path))
        })
        .collect();
    over.sort_by_key(|entry| std::cmp::Reverse(entry.0));
    let listed: Vec<String> = over.iter().map(|(n, p)| format!("{n} {}", p.display())).collect();
    match weigh(OUT_OF_SCALE_TODAY, over.len()) {
        Weighed::TreeIsAbove(more) => panic!(
            "files over {LINES_OUT_OF_SCALE} lines: {} ({more} more than the seed's \
             {OUT_OF_SCALE_TODAY}). Split by responsibility:\n{}",
            over.len(),
            listed.join("\n")
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
