//! A product's own directory is a datum about that product, never a constant
//! in ours. The sibling check forbids a product name in a *decision*; this
//! forbids it in a *path*, which no decision sign catches: `join(".claude")`
//! reads as plumbing and is the whole coupling. Sailor grew product-specific
//! through this door while the other gate stayed green.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

/// The home directories of products Sailor talks to.
const PRODUCT_HOMES: &[&str] = &[
    ".claude",
    ".codex",
    ".gemini",
    ".orca",
    ".cursor",
    ".aider",
    ".ollama",
    ".continue",
];

/// Where a product's own paths are allowed to be written, because there they
/// are the subject rather than an assumption: the tool descriptors, which every
/// command line fills in for itself.
const WHERE_THEY_BELONG: &str = "crates/toolbox/descriptors/";

/// Sources that still name one, each with what it would take to remove it.
/// The list only shrinks: it is the debt written down, not permission. A file
/// leaves when the thing it hard-codes becomes a field of the descriptor, and
/// nothing new joins. Twenty-five lines across seven crates when first
/// measured, which is how far one command line had got inside a product meant
/// for six; twenty-three after the release stopped installing itself there.
const STILL_ALLOWED: &[(&str, &str)] = &[
    (
        "crates/inventory/src/discovery.rs",
        "the largest one: it inventories one product's skills, agents and \
         plugins, so for the other five Sailor reports nothing and does not say \
         it looked nowhere",
    ),
    (
        "crates/inventory/src/lib.rs",
        "same inventory, the reading half",
    ),
    (
        "crates/sailor/src/session_cmd.rs",
        "where the hooks are grafted. How a command line is told a session began \
         has no field in the descriptor yet, and the conduit is the road that \
         works for the ones with no hooks at all",
    ),
    (
        "crates/release/src/lib.rs",
        "the house Sailor installed itself into until 01/09/2026, kept so the \
         release can still find a stamp left there. It goes when a machine that \
         released before the move has had its stamp carried over",
    ),
    (
        "crates/ledger/src/lib.rs",
        "one legacy path, from before Sailor had a home of its own",
    ),
    (
        "crates/models/src/store.rs",
        "where the usage of one command line is kept",
    ),
    (
        "crates/models/src/remaining.rs",
        "where the credentials of one command line are read",
    ),
    (
        "crates/profiles/src/store_io.rs",
        "moves a product's home when a profile changes",
    ),
];

fn repository_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("the crate lives in <root>/crates/sailor")
        .to_path_buf()
}

/// Only what ships. Tests build fake homes on purpose, and a check that read
/// them would be red on the very thing that proves the code works elsewhere.
fn shipped_sources(root: &Path, into: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(root) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            let name = entry.file_name();
            let name = name.to_string_lossy();
            if name == "tests" || name == "target" || name == "descriptors" {
                continue;
            }
            shipped_sources(&path, into);
        } else if path.extension().is_some_and(|kind| kind == "rs") {
            into.push(path);
        }
    }
}

/// The line without the comment that ends it, so prose about a product stays
/// free. Cut short of a `//` inside a string, which means less code is read,
/// never more: this check can let something through, it cannot accuse wrongly.
fn code_part(line: &str) -> &str {
    match line.find("//") {
        Some(at) => &line[..at],
        None => line,
    }
}

#[test]
fn no_product_home_is_a_constant_in_the_code() {
    let root = repository_root();
    let mut sources = Vec::new();
    shipped_sources(&root.join("crates"), &mut sources);
    assert!(
        sources.len() > 20,
        "the sources were not read: {} files found",
        sources.len()
    );

    let excused: BTreeSet<&str> = STILL_ALLOWED.iter().map(|(file, _)| *file).collect();
    let mut caught = Vec::new();
    for path in &sources {
        let shown = path
            .strip_prefix(&root)
            .unwrap_or(path)
            .to_string_lossy()
            .into_owned();
        if excused.contains(shown.as_str()) {
            continue;
        }
        let Ok(text) = std::fs::read_to_string(path) else {
            continue;
        };
        for (number, line) in text.lines().enumerate() {
            let code = code_part(line);
            for home in PRODUCT_HOMES {
                if code.contains(&format!("\"{home}\"")) || code.contains(&format!("{home}/")) {
                    caught.push(format!("{shown}:{}  {}", number + 1, line.trim()));
                }
            }
        }
    }

    assert!(
        caught.is_empty(),
        "{} line(s) write a product's own directory into Sailor's code. That \
         directory belongs to that product and is a field of its descriptor, in \
         {WHERE_THEY_BELONG}: written here, Sailor works for one command line \
         and quietly does nothing for the other five.\n{}",
        caught.len(),
        caught.join("\n")
    );
}

/// A file on the list has to still name one, or the list becomes a place where
/// a coupling keeps its permission after being removed — and the next one slips
/// in behind it.
#[test]
fn every_excused_file_still_has_something_to_excuse() {
    let root = repository_root();
    for (file, _) in STILL_ALLOWED {
        let Ok(text) = std::fs::read_to_string(root.join(file)) else {
            panic!("«{file}» is excused and is not there");
        };
        let names_one = text.lines().any(|line| {
            let code = code_part(line);
            PRODUCT_HOMES.iter().any(|home| {
                code.contains(&format!("\"{home}\"")) || code.contains(&format!("{home}/"))
            })
        });
        assert!(
            names_one,
            "«{file}» is excused and names no product home any more: take it off \
             the list, so the list keeps shrinking"
        );
    }
}

/// The check has to be able to catch something, or it is a green light with no
/// bulb behind it.
#[test]
fn the_check_would_catch_a_real_one() {
    let offending = r#"    let settings = home.join(".claude").join("settings.json");"#;
    let code = code_part(offending);
    assert!(
        PRODUCT_HOMES
            .iter()
            .any(|home| code.contains(&format!("\"{home}\""))),
        "the detector no longer sees the line it exists for"
    );
    let prose = r#"    // the hooks of Claude Code live under ".claude""#;
    assert!(
        !PRODUCT_HOMES
            .iter()
            .any(|home| code_part(prose).contains(&format!("\"{home}\""))),
        "a comment about a product is not a coupling, and must stay free"
    );
}
