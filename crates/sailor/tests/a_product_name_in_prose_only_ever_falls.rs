//! **NAMING A TOOL IS DATA; POSITIONING AGAINST IT IS FRAMING — AND ONLY A
//! PERSON CAN TELL THEM APART.** In a descriptor a product name is a fact, in
//! a gate it is the vocabulary of the ban, in prose it can be either: a
//! measurement of this machine, or this work sold as a reaction to somebody
//! else's. No rule of place separates those, so this counts instead, and the
//! count only falls. A new mention is red until somebody reads it.

use std::path::{Path, PathBuf};

/// Measured on a clean `HEAD` — `git archive HEAD | tar -x` — and never on the
/// working tree: several sessions write in this checkout, and a seed taken
/// from uncommitted lines describes a tree nobody else has.
const MENTIONS_IN_PROSE_TODAY: usize = 43;

/// The shells and workspaces this work could be sold against. **Not**
/// `PRODUCT_NAMES` from `no_product_name_decides_anything`: that one holds the
/// engines Sailor drives, which prose names thousands of times by design.
const PRODUCTS: &[&str] = &["orca", "warp", "vscode", "iterm", "tmux"];

/// Prose that describes Sailor to a reader. `AGENTS.md` is out on purpose: it
/// carries the ban itself, so its mentions can never fall.
const PROSE: &[&str] = &["docs", "README.md"];

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .expect("the workspace root")
}

fn prose_files() -> Vec<PathBuf> {
    let root = root();
    let mut found = Vec::new();
    for place in PROSE {
        let at = root.join(place);
        if at.is_file() {
            found.push(at);
        } else {
            walk(&at, &mut found);
        }
    }
    found.sort();
    found
}

fn walk(directory: &Path, found: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(directory) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            walk(&path, found);
        } else if path.extension().is_some_and(|kind| kind == "md") {
            found.push(path);
        }
    }
}

/// Every mention, with the file it sits in, so a red run says where to look.
fn mentions() -> Vec<(PathBuf, usize)> {
    let mut counted = Vec::new();
    for file in prose_files() {
        let Ok(text) = std::fs::read_to_string(&file) else {
            continue;
        };
        let lowered = text.to_lowercase();
        let here: usize = PRODUCTS.iter().map(|name| lowered.matches(name).count()).sum();
        if here > 0 {
            counted.push((file, here));
        }
    }
    counted
}

#[test]
fn the_mentions_in_prose_only_ever_fall() {
    let counted = mentions();
    let total: usize = counted.iter().map(|(_, here)| here).sum();

    let mut heaviest = counted.clone();
    heaviest.sort_by_key(|entry| std::cmp::Reverse(entry.1));
    let where_they_are: Vec<String> = heaviest
        .iter()
        .take(5)
        .map(|(file, here)| format!("{here} in {}", file.display()))
        .collect();

    assert!(
        total <= MENTIONS_IN_PROSE_TODAY,
        "product names in prose: {total} (the seed is {MENTIONS_IN_PROSE_TODAY}). \
         A new one is not forbidden, it is unread: decide whether it records a \
         measurement or sells this work against a product. If it is a \
         measurement, it belongs in the notes with no remote; if it is framing, \
         it does not belong at all. The seed does not rise. \
         Heaviest first: {where_they_are:?}"
    );
}

/// A count that stopped counting reads as agreement — fault 22. Both halves:
/// files were found, and the products are still spelled as the tree spells them.
#[test]
fn the_check_can_still_see_what_it_counts() {
    let files = prose_files();
    assert!(
        files.len() >= 20,
        "only {} prose files were found, so almost nothing was counted",
        files.len()
    );

    let counted = mentions();
    assert!(
        !counted.is_empty(),
        "no product name was found anywhere, which after {} files means the \
         search stopped matching, not that the prose is clean",
        files.len()
    );
}

/// A seed far above the floor is a seed nobody re-measured, and it buys silence
/// for work nobody did. Twelve is the width of one cleanup.
#[test]
fn the_seed_stays_close_to_what_the_tree_holds() {
    let total: usize = mentions().iter().map(|(_, here)| here).sum();
    assert!(
        MENTIONS_IN_PROSE_TODAY <= total + 12,
        "the seed says {MENTIONS_IN_PROSE_TODAY} and the tree holds {total}: \
         lower the seed to what was measured, so the next mention is caught"
    );
}
