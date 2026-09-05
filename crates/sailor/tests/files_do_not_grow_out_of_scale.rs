//! No file over the scale, counted as a ratchet: the ones over it today are
//! seeded, and the number may only fall.

use std::path::{Path, PathBuf};

const LINES_OUT_OF_SCALE: usize = 2_000;

/// How many files run over today. Downwards only.
const OUT_OF_SCALE_TODAY: usize = 5;

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
    let mut over: Vec<(usize, PathBuf)> = sources()
        .into_iter()
        .filter_map(|path| {
            let lines = std::fs::read_to_string(&path).ok()?.lines().count();
            (lines > LINES_OUT_OF_SCALE).then_some((lines, path))
        })
        .collect();
    over.sort_by_key(|entry| std::cmp::Reverse(entry.0));
    let listed: Vec<String> = over.iter().map(|(n, p)| format!("{n} {}", p.display())).collect();
    assert!(
        over.len() <= OUT_OF_SCALE_TODAY,
        "files over {LINES_OUT_OF_SCALE} lines: {} (the seed is {OUT_OF_SCALE_TODAY}). Split by responsibility:\n{}",
        over.len(),
        listed.join("\n")
    );
    assert_eq!(
        over.len(),
        OUT_OF_SCALE_TODAY,
        "fewer files over the scale than the seed says: lower OUT_OF_SCALE_TODAY to {}",
        over.len()
    );
}
